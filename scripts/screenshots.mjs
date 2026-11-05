#!/usr/bin/env node
// README screenshots + GIF from the in-memory demo backend (`/?demo`, src/lib/ipc/demo.ts).
//
//   pnpm screenshots            build, serve with `vite preview`, capture everything
//   pnpm screenshots --no-build reuse an existing build/
//
// Writes docs/screenshots/: deck-<preset>-<theme>.png (1920×1080), deck-edit.png, deck-7in-<preset>.png (1024×600),
// deck-portrait.png (4×8), widgets/<widget>-<theme>.png and deck.gif (pure JS: gifenc + pngjs, no ffmpeg).
// The clock is pinned and animations are off, so reruns produce the same pictures.
import { spawn } from 'node:child_process';
import { mkdirSync, rmSync, statSync, readdirSync, writeFileSync } from 'node:fs';
import { createServer } from 'node:net';
import { join, resolve } from 'node:path';
import { chromium } from 'playwright';
import { PNG } from 'pngjs';
import gifenc from 'gifenc';

/** @typedef {import('playwright').Browser} Browser */
/** @typedef {import('playwright').Page} Page */
/** @typedef {{ width: number; height: number; data: Uint8Array }} Frame */

const { GIFEncoder, quantize, applyPalette } = gifenc;

const ROOT = resolve(import.meta.dirname, '..');
const OUT = join(ROOT, 'docs', 'screenshots');
const VITE = join(ROOT, 'node_modules', 'vite', 'bin', 'vite.js');

export const THEMES = ['catppuccin-mocha', 'tokyo-night', 'nord', 'catppuccin-latte'];
export const PRESETS = ['focus', 'ops', 'agents', 'full'];
/** Each widget is cropped from the preset where it is largest. */
const WIDGETS = {
	clock: 'focus',
	pomodoro: 'focus',
	weather: 'focus',
	todo: 'focus',
	date: 'focus',
	system: 'ops',
	calendar: 'ops',
	media: 'ops',
	volume: 'ops',
	agents: 'agents'
};
/** Thursday morning, Istanbul: a pomodoro running, stand-up next. */
const CLOCK = new Date('2026-11-05T10:42:00+03:00');
const GIF_MAX_BYTES = 3 * 1024 * 1024;

const args = new Set(process.argv.slice(2));
const log = (/** @type {unknown[]} */ ...a) => console.log('[screenshots]', ...a);

/** @param {string} cmd @param {string[]} argv @param {import('node:child_process').SpawnOptions} [opts] */
function run(cmd, argv, opts = {}) {
	/** @type {Promise<void>} */
	const done = new Promise((ok, fail) => {
		const p = spawn(cmd, argv, { cwd: ROOT, stdio: 'inherit', ...opts });
		p.on('exit', (code) => (code === 0 ? ok() : fail(new Error(`${argv.join(' ')} exited with ${code}`))));
		p.on('error', fail);
	});
	return done;
}

/** @returns {Promise<number>} */
function freePort() {
	return new Promise((ok, fail) => {
		const srv = createServer();
		srv.unref();
		srv.on('error', fail);
		srv.listen(0, '127.0.0.1', () => {
			const { port } = /** @type {import('node:net').AddressInfo} */ (srv.address());
			srv.close(() => ok(port));
		});
	});
}

async function startPreview() {
	const port = await freePort();
	const proc = spawn(process.execPath, [VITE, 'preview', '--host', '127.0.0.1', '--port', String(port), '--strictPort'], {
		cwd: ROOT,
		stdio: ['ignore', 'pipe', 'inherit']
	});
	const base = `http://127.0.0.1:${port}`;
	for (let i = 0; i < 100; i++) {
		if (proc.exitCode !== null) throw new Error('vite preview exited early');
		try {
			if ((await fetch(base)).ok) return { base, stop: () => proc.kill() };
		} catch {
			// not up yet
		}
		await new Promise((r) => setTimeout(r, 200));
	}
	proc.kill();
	throw new Error('vite preview did not start');
}

const STILL_CSS = `*, *::before, *::after {
	animation-duration: 0s !important; animation-delay: 0s !important;
	transition-duration: 0s !important; transition-delay: 0s !important; caret-color: transparent !important;
}`;

/** @param {Browser} browser @param {{ width: number; height: number; scale?: number; still?: boolean }} opts */
async function newPage(browser, { width, height, scale = 1, still = true }) {
	const context = await browser.newContext({
		viewport: { width, height },
		deviceScaleFactor: scale,
		reducedMotion: still ? 'reduce' : 'no-preference',
		locale: 'en-US',
		timezoneId: 'Europe/Istanbul',
		colorScheme: 'dark'
	});
	const page = await context.newPage();
	page.on('pageerror', (e) => console.warn('[page error]', e.message));
	page.on('console', (m) => m.type() === 'error' && console.warn('[console]', m.text()));
	await page.clock.setFixedTime(CLOCK);
	return { context, page };
}

/** Loaded, every widget rendered (no skeleton cards), fonts in, a few samples drawn. */
/** @param {Page} page */
async function settle(page, still = true) {
	await page.waitForSelector('[data-instance]', { timeout: 20_000 });
	// Skeletons inside closed sheets (process list, mixer) don't count.
	const busy = () =>
		[...document.querySelectorAll('[data-instance] [aria-busy="true"]')]
			.filter((el) => el.checkVisibility())
			.map((el) => el.closest('[data-instance]')?.getAttribute('data-instance'))
			.join(', ');
	try {
		await page.waitForFunction(
			() => ![...document.querySelectorAll('[data-instance] [aria-busy="true"]')].some((el) => el.checkVisibility()),
			null,
			{ timeout: 20_000 }
		);
	} catch (e) {
		throw new Error(`widgets still loading: ${await page.evaluate(busy)}`, { cause: e });
	}
	await page.evaluate(() => document.fonts.ready);
	if (still) await page.addStyleTag({ content: STILL_CSS });
	await page.waitForTimeout(1200);
}

/** @param {Page} page @param {string} base @param {{ theme: string; workspace?: string; still?: boolean }} opts */
async function open(page, base, { theme, workspace, still = true }) {
	const q = new URLSearchParams({ demo: '', theme });
	if (workspace) q.set('workspace', workspace);
	await page.goto(`${base}/?${q.toString().replace('demo=', 'demo')}`);
	await settle(page, still);
}

const kb = (/** @type {number} */ n) => `${(n / 1024).toFixed(0)} KB`;

/** @param {Browser} browser @param {string} base */
async function stills(browser, base) {
	mkdirSync(join(OUT, 'widgets'), { recursive: true });
	const { context, page } = await newPage(browser, { width: 1920, height: 1080 });
	for (const theme of THEMES) {
		for (const preset of PRESETS) {
			await open(page, base, { theme, workspace: preset });
			await page.screenshot({ path: join(OUT, `deck-${preset}-${theme}.png`) });
			for (const [widget, from] of Object.entries(WIDGETS)) {
				if (from !== preset) continue;
				await page.locator(`[data-instance="${widget}-1"]`).screenshot({ path: join(OUT, 'widgets', `${widget}-${theme}.png`) });
			}
			log('deck', preset, theme);
		}
	}
	// Edit mode (E): dot grid, resize handles, drawer button.
	await open(page, base, { theme: THEMES[0], workspace: 'focus' });
	await page.keyboard.press('e');
	await page.waitForTimeout(500);
	await page.screenshot({ path: join(OUT, 'deck-edit.png') });
	log('edit');
	await context.close();

	// 7" 1024×600 USB display, every preset in the default theme.
	const seven = await newPage(browser, { width: 1024, height: 600 });
	for (const preset of PRESETS) {
		await open(seven.page, base, { theme: THEMES[0], workspace: preset });
		await seven.page.screenshot({ path: join(OUT, `deck-7in-${preset}.png`) });
	}
	await seven.context.close();
	log('7-inch');

	// Portrait mount: the automatic density turns 8×4 into 4×8.
	const portrait = await newPage(browser, { width: 1080, height: 1920 });
	await open(portrait.page, base, { theme: 'tokyo-night', workspace: 'full' });
	await portrait.page.screenshot({ path: join(OUT, 'deck-portrait.png') });
	await portrait.context.close();
	log('portrait');
}

// ── GIF ──

/** @param {Buffer} buf @returns {Frame} */
function rgba(buf) {
	const png = PNG.sync.read(buf);
	return { width: png.width, height: png.height, data: new Uint8Array(png.data.buffer, png.data.byteOffset, png.data.length) };
}

/** @param {Frame} a @param {Frame} b @param {number} t @returns {Frame} */
function blend(a, b, t) {
	const out = new Uint8Array(a.data.length);
	for (let i = 0; i < out.length; i++) out[i] = a.data[i] + (b.data[i] - a.data[i]) * t;
	return { ...a, data: out };
}

/** @param {Browser} browser @param {string} base */
async function gif(browser, base) {
	// 1280×720 CSS px rendered at 0.625 → 800×450 frames.
	const { context, page } = await newPage(browser, { width: 1280, height: 720, scale: 0.625 });
	await open(page, base, { theme: THEMES[0], workspace: 'focus' });
	const shot = async () => rgba(await page.screenshot());
	/** @type {[Frame, number][]} [frame, hold ms] */
	const states = [[await shot(), 1800]];
	/** @param {() => Promise<unknown>} action @param {number} hold */
	const step = async (action, hold) => {
		await action();
		await page.waitForTimeout(700);
		states.push([await shot(), hold]);
	};
	const current = () => document.querySelector('[aria-current="true"]')?.textContent?.trim() ?? '';
	const next = async () => {
		const before = await page.evaluate(current);
		await page.keyboard.press('Control+Alt+ArrowRight');
		await page.waitForFunction(
			(b) => (document.querySelector('[aria-current="true"]')?.textContent?.trim() ?? '') !== b,
			before
		);
	};
	await step(next, 1500); // Ops
	await step(next, 1500); // Agents
	await step(next, 1500); // Full
	/** @param {string} id */
	const theme = (id) => async () => {
		const bg = () => getComputedStyle(document.documentElement).getPropertyValue('--k-bg');
		const before = await page.evaluate(bg);
		await page.evaluate((t) => /** @type {any} */ (window).__kotiloDemo.setTheme(t), id);
		await page.waitForFunction((b) => getComputedStyle(document.documentElement).getPropertyValue('--k-bg') !== b, before);
	};
	await step(theme('tokyo-night'), 1300);
	await step(theme('catppuccin-latte'), 1300);
	await step(theme('nord'), 1300);
	await step(next, 1800); // back to Focus
	await context.close();

	/** @param {number} blends */
	const encode = (blends) => {
		const enc = GIFEncoder();
		/** @param {Frame} f @param {number} delay */
		const write = (f, delay) => {
			const palette = quantize(f.data, 256, { format: 'rgb565' });
			enc.writeFrame(applyPalette(f.data, palette, 'rgb565'), f.width, f.height, { palette, delay });
		};
		states.forEach(([f, hold], i) => {
			write(f, hold);
			const [n] = states[(i + 1) % states.length];
			for (let k = 1; k <= blends; k++) write(blend(f, n, k / (blends + 1)), 70);
		});
		enc.finish();
		return enc.bytes();
	};
	let bytes = encode(3);
	if (bytes.length > GIF_MAX_BYTES) bytes = encode(1);
	if (bytes.length > GIF_MAX_BYTES) bytes = encode(0);
	writeFileSync(join(OUT, 'deck.gif'), bytes);
	log('gif', kb(bytes.length));
}

/** @param {string} dir @returns {number} */
function total(dir) {
	let n = 0;
	for (const e of readdirSync(dir, { withFileTypes: true })) {
		const p = join(dir, e.name);
		n += e.isDirectory() ? total(p) : statSync(p).size;
	}
	return n;
}

async function main() {
	if (!args.has('--no-build')) {
		log('building');
		await run(process.execPath, [VITE, 'build', '--logLevel', 'warn'], { stdio: ['ignore', 'ignore', 'inherit'] });
	}
	if (!args.has('--gif-only')) rmSync(OUT, { recursive: true, force: true });
	mkdirSync(OUT, { recursive: true });
	const server = await startPreview();
	log('serving', server.base);
	const browser = await chromium.launch();
	try {
		if (!args.has('--gif-only')) await stills(browser, server.base);
		await gif(browser, server.base);
	} finally {
		await browser.close();
		server.stop();
	}
	log('total', `${(total(OUT) / 1024 / 1024).toFixed(1)} MB`, 'in', OUT);
}

main().catch((e) => {
	console.error(e);
	process.exit(1);
});
