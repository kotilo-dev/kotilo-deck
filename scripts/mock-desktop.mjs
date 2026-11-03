#!/usr/bin/env node
// @ts-nocheck -- plain Node dev script, not part of the app build
// Reference client for protocol.md (Kotilo desktop side). Node >= 22, no dependencies.
//
//   node scripts/mock-desktop.mjs [options]
//
//   --approve-request "summary"   send a new approval request after connecting
//   --suggest                     send pomodoro.suggest
//   --switch <slug>               send workspace.switch
//   --port <n>                    default 47831
//   --token-file <path>           default ~/.kotilo/companion.token
//   --once                        exit when the connection closes (no reconnect)
//
// While connected, type a line + Enter to request another approval with that summary.
// Manual Phase 3 acceptance test: tap Approve/Deny on the Deck and watch for
// "approval.respond reached the desktop" here.

import { randomBytes } from 'node:crypto';
import { existsSync, linkSync, mkdirSync, readFileSync, renameSync, unlinkSync, writeFileSync } from 'node:fs';
import { homedir } from 'node:os';
import { dirname, join } from 'node:path';
import { createInterface } from 'node:readline';

if (typeof WebSocket === 'undefined') {
	console.error('mock-desktop needs Node >= 22 (global WebSocket).');
	process.exit(1);
}

// ── Arguments ───────────────────────────────────────────────────────────────

const args = process.argv.slice(2);
const opts = { port: 47831, tokenFile: join(homedir(), '.kotilo', 'companion.token'), once: false };
for (let i = 0; i < args.length; i++) {
	const a = args[i];
	const value = () => {
		const v = args[++i];
		if (v === undefined) {
			console.error(`${a} needs a value`);
			process.exit(2);
		}
		return v;
	};
	if (a === '--approve-request') opts.approveRequest = value();
	else if (a === '--suggest') opts.suggest = true;
	else if (a === '--switch') opts.switchTo = value();
	else if (a === '--port') opts.port = Number(value());
	else if (a === '--token-file') opts.tokenFile = value();
	else if (a === '--once') opts.once = true;
	else if (a === '--help' || a === '-h') {
		console.log(readFileSync(new URL(import.meta.url), 'utf8').split('\n').slice(1, 16).join('\n'));
		process.exit(0);
	} else {
		console.error(`unknown option ${a} (try --help)`);
		process.exit(2);
	}
}

// ── Token (protocol.md §2): read, or create if missing; never overwrite a valid one ──

const VALID = /^[0-9a-f]{64}$/;

function readToken() {
	try {
		const t = readFileSync(opts.tokenFile, 'utf8').trim();
		return VALID.test(t) ? t : null;
	} catch {
		return null;
	}
}

function loadOrCreateToken() {
	const existing = readToken();
	if (existing) return existing;
	mkdirSync(dirname(opts.tokenFile), { recursive: true });
	const token = randomBytes(32).toString('hex');
	const tmp = join(dirname(opts.tokenFile), `.companion.token.${randomBytes(6).toString('hex')}.tmp`);
	writeFileSync(tmp, token, { mode: 0o600, flag: 'wx' });
	try {
		linkSync(tmp, opts.tokenFile); // fails if the Deck created it meanwhile
		log('created', opts.tokenFile);
		return token;
	} catch {
		const raced = readToken();
		if (raced) return raced;
		renameSync(tmp, opts.tokenFile); // invalid content: replace
		return token;
	} finally {
		if (existsSync(tmp)) unlinkSync(tmp);
	}
}

// ── Output ──────────────────────────────────────────────────────────────────

const time = () => new Date().toTimeString().slice(0, 8);
function log(...parts) {
	console.log(`[${time()}]`, ...parts);
}

function describe(msg) {
	const p = msg.payload ?? {};
	switch (msg.type) {
		case 'hello.ok':
			return `${p.server} ${p.serverVersion}, protocol ${p.protocol}`;
		case 'pomodoro.state': {
			const left = p.running && p.endsAt ? Math.max(0, Math.round((p.endsAt - Date.now()) / 1000)) : p.remaining;
			const mmss = `${Math.floor(left / 60)}:${String(left % 60).padStart(2, '0')}`;
			return `${p.phase} ${p.running ? 'running' : 'stopped'} ${mmss} left of ${p.duration / 60} min, session ${p.session}/${p.sessionsPerCycle}`;
		}
		case 'approval.respond':
			return `id=${p.id} decision=${p.decision}`;
		case 'todo.added':
			return `#${p.id} "${p.text}"${p.due ? ` due ${new Date(p.due).toLocaleString()}` : ''}`;
		default:
			return JSON.stringify(p);
	}
}

// ── Sample desktop state ────────────────────────────────────────────────────

const start = Date.now();
const agents = [
	{ id: 'a1', repo: 'kotilo-deck', name: 'claude', status: 'working', task: 'Phase 3 widgets', since: start - 600_000 },
	{ id: 'a2', repo: 'shell', name: 'claude', status: 'waiting', task: 'Run cargo build?', since: start - 60_000 },
	{ id: 'a3', repo: 'kotilo-web', name: 'codex', status: 'idle', since: start - 3_600_000 },
];
let approvals = [
	{ id: 'ap-1', agentId: 'a2', summary: 'Run cargo build', detail: 'cargo build --release', requestedAt: start - 60_000 },
];
let nextApproval = 2;

const envelope = (type, payload) => JSON.stringify({ v: 1, type, payload });

function waitingAgents() {
	const waiting = new Set(approvals.map((a) => a.agentId));
	return agents.map((a) => ({ ...a, status: waiting.has(a.id) ? 'waiting' : a.status === 'waiting' ? 'working' : a.status }));
}

// ── Connection ──────────────────────────────────────────────────────────────

let ws = null;
let backoff = 1000;

function send(type, payload) {
	if (!ws || ws.readyState !== WebSocket.OPEN) {
		log('not connected; dropped', type);
		return;
	}
	ws.send(envelope(type, payload));
	log('->', type, type === 'agents.state' ? `${payload.agents.length} agents, ${payload.approvals.length} approvals` : '');
}

const sendAgents = () => send('agents.state', { agents: waitingAgents(), approvals });

function requestApproval(summary) {
	const approval = { id: `ap-${nextApproval++}`, agentId: 'a1', summary, detail: null, requestedAt: Date.now() };
	approvals.push(approval);
	send('agent.approval.requested', approval);
	sendAgents();
}

function onMessage(msg) {
	log('<-', msg.type, describe(msg));
	if (msg.type === 'hello.ok') {
		backoff = 1000;
		sendAgents();
		send('usage.today', { tokens: 1_840_000, costUsd: 4.12, budgetUsd: 10 });
		setTimeout(() => {
			if (opts.approveRequest) requestApproval(opts.approveRequest);
			if (opts.suggest) send('pomodoro.suggest', { reason: 'Agent started a long task', minutes: 25 });
			if (opts.switchTo) send('workspace.switch', { slug: opts.switchTo });
		}, 500);
	} else if (msg.type === 'approval.respond') {
		const { id, decision } = msg.payload ?? {};
		const pending = approvals.some((a) => a.id === id);
		console.log(`\n  ACCEPTANCE: approval.respond reached the desktop (id=${id}, decision=${decision})\n`);
		// Like the real desktop: apply, confirm, and send the new snapshot. Not pending →
		// report the real outcome so the Deck can correct itself.
		approvals = approvals.filter((a) => a.id !== id);
		send('agent.approval.resolved', { id, decision: pending ? decision : 'expired', by: pending ? 'deck' : 'agent' });
		sendAgents();
	}
}

function connect() {
	const token = loadOrCreateToken();
	const url = `ws://127.0.0.1:${opts.port}/`;
	log('connecting to', url);
	ws = new WebSocket(url);
	ws.addEventListener('open', () => {
		ws.send(envelope('hello', { token, client: 'kotilo-desktop', clientVersion: 'mock-1.0.0' }));
	});
	ws.addEventListener('message', (e) => {
		try {
			onMessage(JSON.parse(String(e.data)));
		} catch (err) {
			log('unparseable frame:', err.message);
		}
	});
	ws.addEventListener('error', () => {}); // close follows with the details
	ws.addEventListener('close', (e) => {
		const why = { 1001: 'deck shutting down', 4400: 'malformed', 4401: 'bad token / no hello', 4409: 'replaced by another client', 4426: 'unsupported version' }[e.code] ?? '';
		log(`closed ${e.code}${why ? ` (${why})` : ''}`);
		ws = null;
		if (opts.once) process.exit(e.code === 1000 ? 0 : 1);
		// 4401: the token is re-read on the next attempt (connect() reads it every time).
		log(`reconnecting in ${backoff / 1000} s`);
		setTimeout(connect, backoff);
		backoff = Math.min(backoff * 2, 30_000);
	});
}

createInterface({ input: process.stdin }).on('line', (line) => {
	const summary = line.trim();
	if (summary) requestApproval(summary);
});
process.on('SIGINT', () => {
	log('bye');
	ws?.close(1000);
	process.exit(0);
});

connect();
