#!/usr/bin/env node
// Idle soak run (SPEC §1 "idle < 1% CPU, < 120 MB RAM with all widgets enabled", SPEC §8
// performance budget). Starts a built deck binary on a throwaway data folder holding one
// workspace with every built-in widget, waits a warm-up, samples the process tree, then
// reports and gates on the budget.
//
//   node scripts/soak.mjs --app src-tauri/target/release/kotilo-deck.exe            # 60 s warm-up + 10 min
//   node scripts/soak.mjs --app <binary> --minutes 2 --warmup 30                    # quick local try
//
// Flags: --minutes (10) --warmup <s> (60) --interval <s> (5) --cpu-budget <%> (1)
// --ram-budget <MB> (120) --cpu-tolerance <%> / --ram-tolerance <%> (0; extra headroom on
// the gate, e.g. on a shared CI runner; the real target and whether it was met are still
// printed) --max-log-lines (30)
// --json <file> --markdown <file> --logs <dir> (copy the deck's logs there) --keep (leave
// the temp data folder).
// With $GITHUB_STEP_SUMMARY set, the Markdown summary is appended to it too.
//
// What is measured (the process tree = the deck process plus every descendant, found via
// the parent PID chain and checked against creation times so a reused PID never counts):
// - Windows: kotilo-deck.exe and its own WebView2 tree (msedgewebview2.exe browser,
//   renderer, GPU, utility, crashpad). The WebView2 user data folder is a fresh temp folder,
//   so the deck never shares a browser process with another WebView2 app.
// - Linux: the deck process and WebKitGTK's WebKitWebProcess / WebKitNetworkProcess.
// - RAM gate = peak private working set of the whole tree (Task Manager's "Memory"
//   column; Linux: Private_Clean + Private_Dirty from smaps_rollup). That is the memory
//   the user pays for; shared DLL/library pages are left out. The main process alone,
//   private bytes (commit) and the full working set are reported too.
// - CPU = total CPU time delta across the tree / (wall time × logical cores). The gate is
//   the average over the sampling window; p95 of the per-interval values is reported.
//
// Exit codes: 0 within budget, 1 over budget, 2 the run itself failed (app exited, no
// webview, bad arguments).
import { execFileSync, spawn } from 'node:child_process';
import { appendFileSync, closeSync, cpSync, existsSync, mkdirSync, mkdtempSync, openSync, readdirSync, readFileSync, rmSync, statSync, writeFileSync } from 'node:fs';
import { availableParallelism, tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { createInterface } from 'node:readline';
import { fileURLToPath } from 'node:url';
import { parseArgs } from 'node:util';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const isWindows = process.platform === 'win32';

/**
 * @typedef {{ pid: number, ppid: number, name: string, cpu: number, privateBytes: number, workingSet: number, privateWs: number, created: number }} Proc
 *   cpu: seconds of CPU time so far; memory in bytes (privateWs -1 when unknown); created: an opaque start stamp.
 * @typedef {{ t: number, procs: Map<number, Proc> }} Snapshot  t: seconds (monotonic per sampler).
 */

// ── Arguments ───────────────────────────────────────────────────────────────

const { values: args } = parseArgs({
	options: {
		app: { type: 'string' },
		minutes: { type: 'string', default: '10' },
		warmup: { type: 'string', default: '60' },
		interval: { type: 'string', default: '5' },
		'cpu-budget': { type: 'string', default: '1' },
		'ram-budget': { type: 'string', default: '120' },
		'cpu-tolerance': { type: 'string', default: '0' },
		'ram-tolerance': { type: 'string', default: '0' },
		'max-log-lines': { type: 'string', default: '30' },
		json: { type: 'string' },
		markdown: { type: 'string' },
		logs: { type: 'string' },
		keep: { type: 'boolean', default: false },
		help: { type: 'boolean', short: 'h', default: false }
	}
});

if (args.help || !args.app) {
	console.log('usage: node scripts/soak.mjs --app <path to kotilo-deck binary> [--minutes 10] [--warmup 60] [--interval 5]');
	console.log('       [--cpu-budget 1] [--ram-budget 120] [--cpu-tolerance 0] [--ram-tolerance 0] [--max-log-lines 30] [--json f] [--markdown f] [--logs dir] [--keep]');
	process.exit(args.help ? 0 : 2);
}

/** @param {string} name @param {number} min */
function num(name, min) {
	const v = Number(/** @type {Record<string, unknown>} */ (args)[name]);
	if (!Number.isFinite(v) || v < min) {
		console.error(`--${name} must be a number >= ${min}`);
		process.exit(2);
	}
	return v;
}

const app = resolve(args.app);
const minutes = num('minutes', 0.1);
const warmupS = num('warmup', 0);
const intervalS = num('interval', 1);
const cpuBudget = num('cpu-budget', 0);
const ramBudgetMb = num('ram-budget', 1);
const cpuTolerance = num('cpu-tolerance', 0);
const ramTolerance = num('ram-tolerance', 0);
const maxLogLines = num('max-log-lines', 0);
const cores = availableParallelism();

if (!existsSync(app)) {
	console.error(`no such binary: ${app}`);
	process.exit(2);
}

// ── Throwaway profile with every built-in widget ────────────────────────────

/**
 * Every `src/widgets/<id>/` with a manifest and a Widget.svelte (the same rule as the
 * registry in src/widgets/index.ts) at its default size, first-fit packed on an 8-column
 * grid. Built from the manifests so a new widget joins the soak without edits here.
 */
function soakWorkspace() {
	const dir = join(root, 'src', 'widgets');
	/** @type {{ id: string, w: number, h: number }[]} */
	const items = [];
	for (const entry of readdirSync(dir, { withFileTypes: true })) {
		if (!entry.isDirectory()) continue;
		const manifestPath = join(dir, entry.name, 'manifest.json');
		if (!existsSync(manifestPath) || !existsSync(join(dir, entry.name, 'Widget.svelte'))) continue;
		const m = JSON.parse(readFileSync(manifestPath, 'utf8'));
		const [w, h] = String(m.defaultSize ?? m.sizes?.[0] ?? '2x2').split('x').map(Number);
		items.push({ id: m.id, w, h });
	}
	items.sort((a, b) => b.w * b.h - a.w * a.h || a.id.localeCompare(b.id));
	const cols = 8;
	/** @type {boolean[][]} */
	const used = [];
	const free = (/** @type {number} */ x, /** @type {number} */ y, /** @type {number} */ w, /** @type {number} */ h) => {
		for (let r = y; r < y + h; r++) for (let c = x; c < x + w; c++) if (used[r]?.[c]) return false;
		return true;
	};
	const widgets = items.map(({ id, w, h }) => {
		for (let y = 0; ; y++) {
			for (let x = 0; x + w <= cols; x++) {
				if (!free(x, y, w, h)) continue;
				for (let r = y; r < y + h; r++) for (let c = x; c < x + w; c++) (used[r] ??= [])[c] = true;
				return { instance: `${id}-1`, widget: id, x, y, w, h, settings: {} };
			}
		}
	});
	return {
		schemaVersion: 1,
		id: 'soak',
		name: 'Soak',
		theme: null,
		grid: { cols, rows: Math.max(4, used.length) },
		widgets
	};
}

const tmp = mkdtempSync(join(tmpdir(), 'kotilo-soak-'));
// `<tmp>/deck` so `~/.kotilo`-level files (companion token, user themes) land in `<tmp>` too.
const dataDir = join(tmp, 'deck');
const workspace = soakWorkspace();
mkdirSync(join(dataDir, 'workspaces'), { recursive: true });
writeFileSync(join(dataDir, 'workspaces', 'soak.json'), JSON.stringify(workspace, null, '\t'));
// Onboarding done (any flow version up to the max), no update check (no network, no dialog).
writeFileSync(
	join(dataDir, 'settings.json'),
	JSON.stringify({ schemaVersion: 1, onboarding: { completed: true, version: 1000 }, updates: { autoCheck: false } }, null, '\t')
);

// ── Process snapshots ───────────────────────────────────────────────────────

// One row per process: [pid, ppid, name, cpu 100ns, private bytes, working set, created ticks, private WS or -1].
const PS_SNAPSHOT = String.raw`
$ProgressPreference = 'SilentlyContinue'
[Threading.Thread]::CurrentThread.CurrentCulture = [Globalization.CultureInfo]::InvariantCulture
$perf = @{}
try { foreach ($q in Get-CimInstance Win32_PerfRawData_PerfProc_Process -Property IDProcess,WorkingSetPrivate) { $perf[[int]$q.IDProcess] = [long]$q.WorkingSetPrivate } } catch {}
$sb = [System.Text.StringBuilder]::new()
$t = ([Diagnostics.Stopwatch]::GetTimestamp() / [Diagnostics.Stopwatch]::Frequency).ToString('F4', [Globalization.CultureInfo]::InvariantCulture)
[void]$sb.Append('{"t":').Append($t).Append(',"p":[')
$first = $true
foreach ($p in Get-CimInstance Win32_Process -Property ProcessId,ParentProcessId,Name,KernelModeTime,UserModeTime,PrivatePageCount,WorkingSetSize,CreationDate) {
  $id = [int]$p.ProcessId
  $created = if ($p.CreationDate) { $p.CreationDate.ToUniversalTime().Ticks } else { 0 }
  $pws = if ($perf.ContainsKey($id)) { $perf[$id] } else { -1 }
  $name = ([string]$p.Name) -replace '[\\"]', '_'
  if (-not $first) { [void]$sb.Append(',') }
  $first = $false
  [void]$sb.Append('[').Append($id).Append(',').Append([int]$p.ParentProcessId).Append(',"').Append($name).Append('",').Append([long]$p.KernelModeTime + [long]$p.UserModeTime).Append(',').Append([long]$p.PrivatePageCount).Append(',').Append([long]$p.WorkingSetSize).Append(',').Append($created).Append(',').Append($pws).Append(']')
}
[void]$sb.Append(']}')
[Console]::Out.WriteLine($sb.ToString())
[Console]::Out.Flush()
`;

/** @param {string} script */
const encodePs = (script) => Buffer.from(script, 'utf16le').toString('base64');

/** @param {string} line @returns {Snapshot} */
function parseWindowsLine(line) {
	const raw = /** @type {{ t: number, p: [number, number, string, number, number, number, number, number][] }} */ (JSON.parse(line));
	/** @type {Map<number, Proc>} */
	const procs = new Map();
	for (const [pid, ppid, name, cpu, privateBytes, workingSet, created, privateWs] of raw.p) {
		procs.set(pid, { pid, ppid, name, cpu: cpu / 1e7, privateBytes, workingSet, privateWs, created });
	}
	return { t: raw.t, procs };
}

const clkTck = (() => {
	if (isWindows) return 100;
	try {
		return Number(execFileSync('getconf', ['CLK_TCK'], { encoding: 'utf8' }).trim()) || 100;
	} catch {
		return 100;
	}
})();
const pageSize = (() => {
	if (isWindows) return 4096;
	try {
		return Number(execFileSync('getconf', ['PAGESIZE'], { encoding: 'utf8' }).trim()) || 4096;
	} catch {
		return 4096;
	}
})();

/** /proc snapshot (Linux). @returns {Snapshot} */
function linuxSnapshot() {
	const t = performance.now() / 1000;
	/** @type {Map<number, Proc>} */
	const procs = new Map();
	for (const entry of readdirSync('/proc')) {
		if (!/^\d+$/.test(entry)) continue;
		try {
			const stat = readFileSync(`/proc/${entry}/stat`, 'utf8');
			const close = stat.lastIndexOf(')');
			const name = stat.slice(stat.indexOf('(') + 1, close);
			const f = stat.slice(close + 2).split(' '); // f[0] = field 3 (state)
			const pid = Number(entry);
			const ppid = Number(f[1]);
			const cpu = (Number(f[11]) + Number(f[12])) / clkTck; // utime + stime
			const created = Number(f[19]); // starttime
			const rss = Number(f[21]) * pageSize;
			let privateWs = -1;
			let privateBytes = -1;
			try {
				const rollup = readFileSync(`/proc/${entry}/smaps_rollup`, 'utf8');
				const kb = (/** @type {string} */ key) => Number(rollup.match(new RegExp(`^${key}:\\s+(\\d+)`, 'm'))?.[1] ?? 0) * 1024;
				privateWs = kb('Private_Clean') + kb('Private_Dirty');
				privateBytes = kb('Private_Dirty') + kb('SwapPss');
			} catch {
				// Not ours (permission) or gone; only our own tree needs these.
			}
			procs.set(pid, { pid, ppid, name, cpu, privateBytes, workingSet: rss, privateWs, created });
		} catch {
			// Exited while listing.
		}
	}
	return { t, procs };
}

/** Synchronous snapshot, for cleanup paths that cannot await. @returns {Snapshot | null} */
function snapshotSync() {
	try {
		if (!isWindows) return linuxSnapshot();
		const out = execFileSync('powershell.exe', ['-NoProfile', '-NonInteractive', '-EncodedCommand', encodePs(PS_SNAPSHOT)], {
			encoding: 'utf8',
			stdio: ['ignore', 'pipe', 'ignore'],
			maxBuffer: 64 * 1024 * 1024,
			windowsHide: true
		});
		return parseWindowsLine(out.trim().split(/\r?\n/).pop() ?? '');
	} catch {
		return null;
	}
}

/**
 * Snapshots every `intervalS` seconds. Windows: one long-lived PowerShell (starting one per
 * sample would cost more CPU than the deck). Linux: /proc in-process.
 * @returns {AsyncGenerator<Snapshot>}
 */
async function* sampler() {
	if (!isWindows) {
		for (;;) {
			const started = performance.now();
			yield linuxSnapshot();
			await new Promise((r) => setTimeout(r, Math.max(0, intervalS * 1000 - (performance.now() - started))));
		}
	}
	const loop = `$ErrorActionPreference = 'Stop'\nwhile ($true) {\n$sw = [Diagnostics.Stopwatch]::StartNew()\n${PS_SNAPSHOT}\n$left = ${Math.round(intervalS * 1000)} - $sw.ElapsedMilliseconds\nif ($left -gt 0) { Start-Sleep -Milliseconds $left }\n}`;
	const ps = spawn('powershell.exe', ['-NoProfile', '-NonInteractive', '-EncodedCommand', encodePs(loop)], {
		stdio: ['ignore', 'pipe', 'pipe'],
		windowsHide: true
	});
	samplerProcess = ps;
	let stderr = '';
	ps.stderr.on('data', (d) => (stderr += d));
	for await (const line of createInterface({ input: ps.stdout })) {
		if (line.startsWith('{')) yield parseWindowsLine(line);
	}
	throw new Error(`process sampler stopped${stderr ? `: ${stderr.trim().slice(0, 500)}` : ''}`);
}

/**
 * `rootPid` and every descendant. A child counts only if it started no earlier than its
 * parent (Windows reuses PIDs, and a stale parent PID can point at an unrelated process).
 * @param {Snapshot} snap @param {number} rootPid @returns {Proc[]}
 */
function processTree(snap, rootPid) {
	const rootProc = snap.procs.get(rootPid);
	if (!rootProc) return [];
	/** @type {Map<number, Proc[]>} */
	const children = new Map();
	for (const p of snap.procs.values()) {
		if (p.pid === p.ppid) continue;
		const list = children.get(p.ppid) ?? [];
		list.push(p);
		children.set(p.ppid, list);
	}
	const out = [rootProc];
	for (let i = 0; i < out.length; i++) {
		for (const c of children.get(out[i].pid) ?? []) if (c.created >= out[i].created && !out.includes(c)) out.push(c);
	}
	return out;
}

// ── Launch, clean-up ────────────────────────────────────────────────────────

/** @type {import('node:child_process').ChildProcess | null} */
let samplerProcess = null;
/** Every process ever seen in the tree, pid → creation stamp (to kill only those). */
const seen = new Map();
let rootPid = 0;
let cleanedUp = false;

/** Kills the whole tree (verified by creation stamp), synchronously, once. */
function cleanup() {
	if (cleanedUp) return [];
	cleanedUp = true;
	samplerProcess?.kill();
	/** @type {string[]} */
	let leftovers = [];
	for (let attempt = 0; attempt < 5; attempt++) {
		const snap = snapshotSync();
		if (!snap) break;
		const ours = [...snap.procs.values()].filter((p) => seen.get(p.pid) === p.created);
		if (rootPid) for (const p of processTree(snap, rootPid)) if (!ours.includes(p)) ours.push(p);
		leftovers = ours.map((p) => `${p.name} (${p.pid})`);
		if (ours.length === 0) break;
		for (const p of ours) {
			try {
				process.kill(p.pid, 'SIGKILL');
			} catch {
				// Already gone.
			}
		}
		Atomics.wait(new Int32Array(new SharedArrayBuffer(4)), 0, 0, 1000);
	}
	if (leftovers.length) console.error(`could not stop: ${leftovers.join(', ')}`);
	if (args.logs) {
		try {
			const out = resolve(args.logs);
			mkdirSync(out, { recursive: true });
			if (existsSync(join(dataDir, 'logs'))) cpSync(join(dataDir, 'logs'), out, { recursive: true });
			if (existsSync(appLog)) cpSync(appLog, join(out, 'app-output.log'));
		} catch (e) {
			console.error(`could not copy logs: ${e instanceof Error ? e.message : e}`);
		}
	}
	if (!args.keep) {
		try {
			rmSync(tmp, { recursive: true, force: true, maxRetries: 5, retryDelay: 500 });
		} catch {
			console.error(`temp data folder left behind: ${tmp}`);
		}
	}
	return leftovers;
}

process.on('exit', () => cleanup());
for (const sig of /** @type {const} */ (['SIGINT', 'SIGTERM', 'SIGHUP'])) {
	process.on(sig, () => {
		console.error(`\n${sig}: stopping the deck`);
		process.exit(2);
	});
}

const appLog = join(tmp, 'app-output.log');
const appLogFd = openSync(appLog, 'w');
const child = spawn(app, [], {
	stdio: ['ignore', appLogFd, appLogFd],
	env: {
		...process.env,
		KOTILO_DECK_DATA_DIR: dataDir,
		// Own WebView2 browser process (overrides the user data folder Tauri passes).
		WEBVIEW2_USER_DATA_FOLDER: join(tmp, 'webview2')
	}
});
closeSync(appLogFd);
rootPid = child.pid ?? 0;
if (!rootPid) {
	console.error(`could not start ${app}`);
	process.exit(2);
}
/** @type {string | null} */
let exited = null;
child.on('exit', (code, signal) => (exited = `exit code ${code ?? signal}`));
child.on('error', (e) => (exited = `failed to start: ${e.message}`));

const widgetList = workspace.widgets.map((w) => `${w.widget} ${w.w}x${w.h}`).join(', ');
console.log(`soak: ${app}`);
console.log(`      pid ${rootPid}, data ${dataDir}`);
console.log(`      ${workspace.widgets.length} widgets on a ${workspace.grid.cols}x${workspace.grid.rows} grid: ${widgetList}`);
console.log(`      warm-up ${warmupS} s, then ${minutes} min sampling every ${intervalS} s on ${cores} logical cores`);

// ── Sampling ────────────────────────────────────────────────────────────────

/** @param {number[]} xs @param {number} q */
function quantile(xs, q) {
	if (xs.length === 0) return 0;
	const s = [...xs].sort((a, b) => a - b);
	return s[Math.min(s.length - 1, Math.ceil(q * s.length) - 1)];
}
const mean = (/** @type {number[]} */ xs) => (xs.length ? xs.reduce((a, b) => a + b, 0) / xs.length : 0);
const mb = (/** @type {number} */ bytes) => bytes / (1024 * 1024);

/** @type {{ t: number, cpuPct: number, treePws: number, mainPws: number, treePrivate: number, treeWs: number, count: number }[]} */
const samples = [];
let cpuSeconds = 0;
let wallSeconds = 0;
/** @type {Map<number, Proc> | null} */
let prevTree = null;
let prevT = 0;
/** @type {Proc[]} */
let lastTree = [];
let measureFrom = 0;
let logOffset = 0;
/** @type {string | null} */
let failure = null;
const start = performance.now();

try {
	for await (const snap of sampler()) {
		if (exited) {
			failure = `the deck stopped during the run (${exited}); see ${appLog}`;
			break;
		}
		const tree = processTree(snap, rootPid);
		for (const p of tree) seen.set(p.pid, p.created);
		const elapsed = (performance.now() - start) / 1000;
		if (elapsed < warmupS) continue;

		if (!prevTree) {
			// First sample after warm-up: the baseline.
			measureFrom = elapsed;
			logOffset = logSize();
		} else {
			let delta = 0;
			for (const p of tree) {
				const before = prevTree.get(p.pid);
				// A process born in this interval brings all its CPU time so far.
				delta += before && before.created === p.created ? Math.max(0, p.cpu - before.cpu) : p.cpu;
			}
			const dt = snap.t - prevT;
			cpuSeconds += delta;
			wallSeconds += dt;
			const pws = (/** @type {Proc} */ p) => (p.privateWs >= 0 ? p.privateWs : p.privateBytes);
			const main = tree.find((p) => p.pid === rootPid);
			samples.push({
				t: elapsed,
				cpuPct: dt > 0 ? (100 * delta) / (dt * cores) : 0,
				treePws: tree.reduce((a, p) => a + pws(p), 0),
				mainPws: main ? pws(main) : 0,
				treePrivate: tree.reduce((a, p) => a + Math.max(0, p.privateBytes), 0),
				treeWs: tree.reduce((a, p) => a + p.workingSet, 0),
				count: tree.length
			});
		}
		prevTree = new Map(tree.map((p) => [p.pid, p]));
		prevT = snap.t;
		lastTree = tree;
		if (prevTree && elapsed - measureFrom >= minutes * 60) break;
	}
} catch (e) {
	failure = `sampling failed: ${e instanceof Error ? e.message : e}`;
}

// ── Log check (error loops) ─────────────────────────────────────────────────

function logFiles() {
	const dir = join(dataDir, 'logs');
	if (!existsSync(dir)) return [];
	return readdirSync(dir)
		.filter((f) => f.startsWith('deck.log'))
		.sort()
		.map((f) => join(dir, f));
}
function logSize() {
	return logFiles().reduce((a, f) => a + statSync(f).size, 0);
}
/** WARN/ERROR lines written during the sampling window, grouped by message. */
function logProblems() {
	const text = logFiles()
		.map((f) => readFileSync(f, 'utf8'))
		.join('');
	/** @type {Map<string, number>} */
	const counts = new Map();
	let total = 0;
	for (const line of Buffer.from(text).subarray(logOffset).toString('utf8').split(/\r?\n/)) {
		// ANSI colours are off in the file layer; strip them anyway.
		const plain = line.replace(/\x1b\[[0-9;]*m/g, '');
		const m = plain.match(/\s(WARN|ERROR)\s+(.*)$/);
		if (!m) continue;
		total++;
		const key = `${m[1]} ${m[2].slice(0, 160)}`;
		counts.set(key, (counts.get(key) ?? 0) + 1);
	}
	return { total, top: [...counts.entries()].sort((a, b) => b[1] - a[1]).slice(0, 8) };
}

// ── Report ──────────────────────────────────────────────────────────────────

const webviewName = isWindows ? /^msedgewebview2/i : /^WebKit(Web|Network)Process/;
const webviews = lastTree.filter((p) => webviewName.test(p.name)).length;
if (!failure && samples.length < 2) failure = 'not enough samples (is --minutes shorter than two intervals?)';
if (!failure && webviews === 0) failure = `no ${isWindows ? 'WebView2' : 'WebKitGTK'} process in the deck's tree; the UI never started`;

const logs = logProblems();
const cpuValues = samples.map((s) => s.cpuPct);
const result = {
	app,
	platform: process.platform,
	cores,
	warmupSeconds: warmupS,
	sampledSeconds: Math.round(wallSeconds),
	intervalSeconds: intervalS,
	samples: samples.length,
	widgets: workspace.widgets.map((w) => ({ widget: w.widget, size: `${w.w}x${w.h}` })),
	cpu: {
		avgPct: wallSeconds > 0 ? (100 * cpuSeconds) / (wallSeconds * cores) : 0,
		p95Pct: quantile(cpuValues, 0.95),
		maxPct: Math.max(0, ...cpuValues)
	},
	ramMb: {
		treePrivateWsPeak: mb(Math.max(0, ...samples.map((s) => s.treePws))),
		treePrivateWsAvg: mb(mean(samples.map((s) => s.treePws))),
		mainPrivateWsPeak: mb(Math.max(0, ...samples.map((s) => s.mainPws))),
		mainPrivateWsAvg: mb(mean(samples.map((s) => s.mainPws))),
		treePrivateBytesPeak: mb(Math.max(0, ...samples.map((s) => s.treePrivate))),
		treeWorkingSetPeak: mb(Math.max(0, ...samples.map((s) => s.treeWs)))
	},
	processes: lastTree.map((p) => ({ name: p.name, privateWsMb: mb(p.privateWs >= 0 ? p.privateWs : p.privateBytes) })),
	budget: {
		cpuPct: cpuBudget,
		cpuTolerancePct: cpuTolerance,
		cpuGatePct: cpuBudget * (1 + cpuTolerance / 100),
		ramMb: ramBudgetMb,
		ramTolerancePct: ramTolerance,
		ramGateMb: ramBudgetMb * (1 + ramTolerance / 100)
	},
	/** Whether the real SPEC targets were met (the gate may have tolerance on top). */
	targetsMet: { cpu: false, ram: false },
	log: { warnErrorLines: logs.total, maxLines: maxLogLines, top: logs.top.map(([line, count]) => ({ line, count })) },
	failure,
	/** @type {string[]} */
	overBudget: []
};

result.targetsMet = { cpu: result.cpu.avgPct <= cpuBudget, ram: result.ramMb.treePrivateWsPeak <= ramBudgetMb };
if (!failure) {
	if (result.cpu.avgPct > result.budget.cpuGatePct) result.overBudget.push(`average CPU ${result.cpu.avgPct.toFixed(2)} % > ${result.budget.cpuGatePct.toFixed(2)} %`);
	if (result.ramMb.treePrivateWsPeak > result.budget.ramGateMb) result.overBudget.push(`peak RAM ${result.ramMb.treePrivateWsPeak.toFixed(1)} MB > ${result.budget.ramGateMb.toFixed(1)} MB`);
	if (logs.total > maxLogLines) result.overBudget.push(`${logs.total} WARN/ERROR log lines while idle > ${maxLogLines} (error loop?)`);
}

const leftovers = cleanup();
if (leftovers.length && !failure) failure = `could not stop: ${leftovers.join(', ')}`;
result.failure = failure;

const ok = !failure && result.overBudget.length === 0;
const f2 = (/** @type {number} */ n) => n.toFixed(2);
const f1 = (/** @type {number} */ n) => n.toFixed(1);
const mark = (/** @type {boolean} */ pass) => (failure ? 'n/a' : pass ? 'pass' : '**over**');
/** @param {number} value @param {number} target @param {number} gate @param {number} tolerance */
const gated = (value, target, gate, tolerance) =>
	failure ? 'n/a' : value <= target ? 'pass' : value <= gate ? `**target missed**, within the ${tolerance} % CI tolerance` : '**over**';
const cpuStatus = gated(result.cpu.avgPct, cpuBudget, result.budget.cpuGatePct, cpuTolerance);
const ramStatus = gated(result.ramMb.treePrivateWsPeak, ramBudgetMb, result.budget.ramGateMb, ramTolerance);
const md = [
	`## Idle soak ${failure ? 'failed to run' : !ok ? 'over budget' : result.targetsMet.cpu && result.targetsMet.ram ? 'passed' : 'passed the CI gate, SPEC target missed'}`,
	'',
	failure ? `> ${failure}\n` : '',
	`${result.widgets.length} widgets (${result.widgets.map((w) => `${w.widget} ${w.size}`).join(', ')}), ${warmupS} s warm-up, ${result.sampledSeconds} s sampled every ${intervalS} s, ${cores} logical cores, ${process.platform}.`,
	'',
	'| Metric | Value | Target | Status |',
	'| --- | ---: | ---: | --- |',
	`| CPU average (tree) | ${f2(result.cpu.avgPct)} % | < ${cpuBudget} % | ${cpuStatus} |`,
	`| CPU p95 per ${intervalS} s | ${f2(result.cpu.p95Pct)} % | | |`,
	`| CPU max per ${intervalS} s | ${f2(result.cpu.maxPct)} % | | |`,
	`| RAM peak, private working set (tree) | ${f1(result.ramMb.treePrivateWsPeak)} MB | < ${ramBudgetMb} MB | ${ramStatus} |`,
	`| RAM average, private working set (tree) | ${f1(result.ramMb.treePrivateWsAvg)} MB | | |`,
	`| RAM peak / average, main process only | ${f1(result.ramMb.mainPrivateWsPeak)} / ${f1(result.ramMb.mainPrivateWsAvg)} MB | | |`,
	`| RAM peak, private bytes / commit (tree) | ${f1(result.ramMb.treePrivateBytesPeak)} MB | | |`,
	`| RAM peak, full working set incl. shared (tree) | ${f1(result.ramMb.treeWorkingSetPeak)} MB | | |`,
	`| WARN/ERROR log lines while idle | ${logs.total} | ≤ ${maxLogLines} | ${mark(logs.total <= maxLogLines)} |`,
	'',
	`Processes at the end: ${result.processes.map((p) => `${p.name} ${f1(p.privateWsMb)} MB`).join(', ') || '(none)'}`,
	'',
	...(logs.top.length ? ['Most frequent log lines:', '', ...logs.top.map(([line, count]) => `- ${count}× \`${line.replace(/`/g, "'")}\``), ''] : [])
].join('\n');

console.log('');
console.log(md);
/** @param {string} file @param {string} text */
function writeOut(file, text) {
	mkdirSync(dirname(resolve(file)), { recursive: true });
	writeFileSync(file, text);
}
if (args.json) writeOut(args.json, JSON.stringify(result, null, '\t'));
if (args.markdown) writeOut(args.markdown, md);
if (process.env.GITHUB_STEP_SUMMARY) appendFileSync(process.env.GITHUB_STEP_SUMMARY, `${md}\n`);
process.exit(failure ? 2 : ok ? 0 : 1);
