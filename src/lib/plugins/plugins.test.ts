import { describe, expect, it, vi } from 'vitest';
import type { PluginManifest, PluginWidget } from '#lib/ipc/index.ts';
import { isIcon, iconTone } from '#lib/ui/icons.ts';
import { PluginBridge, type BridgeHost, type DeckMessage, type PluginState } from './bridge';
import { allWidgets, getWidget, isPluginDefinition, pluginPending, pluginSrc, PluginRegistry, settingsFromSchema, toDefinition, type RegistryDeps } from './registry.svelte';
import { readTheme, THEME_TOKENS } from './theme';
import { startTopic, type TopicIpc } from './topics';

const flush = () => new Promise((r) => setTimeout(r, 0));

const manifest = (p: Partial<PluginManifest> = {}): PluginManifest => ({
	apiVersion: 1,
	id: 'hello',
	name: 'Hello',
	icon: 'puzzle',
	sizes: ['2x1', '2x2'],
	defaultSize: '2x2',
	capabilities: [],
	multiInstance: true,
	settings: { type: 'object', properties: { greeting: { type: 'string', title: 'Greeting', default: 'Hello' } } },
	entry: 'index.html',
	topics: ['system'],
	...p
});

const plugin = (folder: string, p: Partial<PluginWidget> = {}, m: Partial<PluginManifest> = {}): PluginWidget => ({
	id: `ext.${folder}`,
	folder,
	manifest: manifest({ id: folder, ...m }),
	error: null,
	entryUrl: `http://kotilo-widget.localhost/${folder}/index.html`,
	...p
});

function fakeDeps(initial: PluginWidget[]) {
	let list = initial;
	let changed: (() => void) | null = null;
	const unlisten = vi.fn();
	const deps: RegistryDeps = {
		list: vi.fn(async () => list),
		onChanged: vi.fn(async (cb: () => void) => {
			changed = cb;
			return unlisten;
		}),
		enabled: () => true
	};
	return {
		deps,
		unlisten,
		set: (next: PluginWidget[]) => (list = next),
		fire: () => changed?.()
	};
}

describe('plugin registry (docs/plugin-api.md)', () => {
	it('merges built-ins with valid plugins; invalid folders are listed but not placeable', async () => {
		const f = fakeDeps([plugin('hello'), plugin('broken', { manifest: null, error: 'manifest.json: apiVersion must be 1', entryUrl: null })]);
		const reg = new PluginRegistry(f.deps);
		expect(reg.loaded).toBe(false);
		reg.start();
		reg.start(); // idempotent
		await flush();
		expect(f.deps.list).toHaveBeenCalledTimes(1);
		expect(f.deps.onChanged).toHaveBeenCalledTimes(1);
		expect(reg.loaded).toBe(true);
		expect(reg.entries).toHaveLength(2);

		const hello = getWidget('ext.hello', reg);
		expect(isPluginDefinition(hello)).toBe(true);
		expect(hello?.manifest).toMatchObject({ id: 'ext.hello', name: 'Hello', sizes: ['2x1', '2x2'], defaultSize: '2x2' });
		expect(hello?.defaults).toEqual({ greeting: 'Hello' });
		expect(getWidget('ext.broken', reg)).toBeUndefined();

		// Built-ins resolve as before and never become plugin definitions.
		const clock = getWidget('clock', reg);
		expect(clock?.manifest.id).toBe('clock');
		expect(isPluginDefinition(clock)).toBe(false);
		const ids = allWidgets(reg).map((d) => d.manifest.id);
		expect(ids).toContain('clock');
		expect(ids).toContain('ext.hello');
		expect(ids).not.toContain('ext.broken');
	});

	it('updates on plugin://changed without a reload; a vanished plugin resolves to undefined (Unknown card)', async () => {
		const f = fakeDeps([plugin('hello')]);
		const reg = new PluginRegistry(f.deps);
		reg.start();
		await flush();
		expect(getWidget('ext.hello', reg)).toBeDefined();
		expect(getWidget('ext.clock2', reg)).toBeUndefined();

		f.set([plugin('clock2', {}, { name: 'Clock 2', sizes: ['4x2'], defaultSize: '4x2' })]);
		f.fire();
		await flush();
		expect(getWidget('ext.hello', reg)).toBeUndefined();
		expect(getWidget('ext.clock2', reg)?.manifest.sizes).toEqual(['4x2']);

		reg.stop();
		expect(f.unlisten).toHaveBeenCalledTimes(1);
	});

	it('keeps the last list when a refresh fails and drops out-of-order answers', async () => {
		const f = fakeDeps([plugin('hello')]);
		const reg = new PluginRegistry(f.deps);
		reg.start();
		await flush();
		(f.deps.list as ReturnType<typeof vi.fn>).mockRejectedValueOnce(new Error('io'));
		await reg.refresh();
		expect(reg.error).toBe('io');
		expect(getWidget('ext.hello', reg)).toBeDefined();

		let release!: (v: PluginWidget[]) => void;
		(f.deps.list as ReturnType<typeof vi.fn>).mockImplementationOnce(() => new Promise((r) => (release = r)));
		const slow = reg.refresh();
		f.set([]);
		await reg.refresh();
		release([plugin('stale')]);
		await slow;
		expect(reg.entries).toEqual([]);
		expect(reg.error).toBeNull();
	});

	it('is pending only for ext.* ids until the first list; without a backend it loads empty', async () => {
		const f = fakeDeps([]);
		const reg = new PluginRegistry(f.deps);
		expect(pluginPending('ext.hello', reg)).toBe(true);
		expect(pluginPending('clock', reg)).toBe(false);
		// Lazy start from getWidget (may run inside a $derived: no synchronous state writes).
		expect(getWidget('ext.hello', reg)).toBeUndefined();
		await flush();
		expect(pluginPending('ext.hello', reg)).toBe(false);

		const off = new PluginRegistry({ ...fakeDeps([]).deps, enabled: () => false });
		off.start();
		expect(off.loaded).toBe(false);
		await flush();
		expect(off.loaded).toBe(true);
	});

	it('sanitises manifests: unknown setting types, bad sizes and topics are dropped', () => {
		const def = toDefinition(
			plugin('x', {}, {
				sizes: ['2x2', '13x1', 'big' as never],
				defaultSize: '13x1',
				topics: ['system', 'files' as never],
				icon: '',
				settings: {
					type: 'object',
					properties: {
						on: { type: 'boolean', default: true },
						mode: { type: 'string', enum: ['a', 'b', 3], default: 'c' },
						n: { type: 'number', minimum: 0, maximum: 10, default: 4 },
						list: { type: 'array' },
						appearance: { type: 'object' }
					}
				}
			})
		)!;
		expect(def.manifest.sizes).toEqual(['2x2']);
		expect(def.manifest.defaultSize).toBe('2x2');
		expect(def.manifest.icon).toBe('puzzle');
		expect(def.plugin.topics).toEqual(['system']);
		expect(Object.keys(def.manifest.settings.properties)).toEqual(['on', 'mode', 'n']);
		expect(def.manifest.settings.properties.mode).toMatchObject({ type: 'string', enum: ['a', 'b'] });
		expect(def.defaults).toEqual({ on: true, n: 4 });
		expect(settingsFromSchema(undefined)).toEqual({ schema: { type: 'object', properties: {} }, defaults: {} });
		expect(toDefinition(plugin('y', {}, { sizes: [] }))).toBeNull();
		expect(toDefinition({ ...plugin('z'), id: 'z' })).toBeNull();
	});

	it('uses the backend URL in the webview and /plugins/<folder>/<entry> over the LAN', () => {
		const info = { folder: 'hello', entry: 'dist/my page.html', entryUrl: 'http://kotilo-widget.localhost/hello/dist/my%20page.html' };
		expect(pluginSrc(info, false)).toBe(info.entryUrl);
		expect(pluginSrc(info, true)).toBe('/plugins/hello/dist/my%20page.html');
		expect(pluginSrc({ ...info, entryUrl: null }, false)).toBeNull();
	});
});

// ── bridge ────────────────────────────────────────────────────────────────

const STATE: PluginState = {
	size: '2x2',
	settings: { greeting: 'Hi' },
	theme: { variant: 'dark', tokens: { bg: '#1e1e2e', radius: '16px', 'font-ui': 'Inter', 'font-mono': 'JetBrains Mono' } },
	locale: 'tr-TR',
	editing: false,
	available: true
};

function harness(topics: ('system' | 'media' | 'pomodoro')[] = ['system'], visible = true) {
	const frame = { name: 'frame' };
	const sent: DeckMessage[] = [];
	let state: PluginState = structuredClone(STATE);
	const stops = new Map<string, ReturnType<typeof vi.fn>>();
	const pushes = new Map<string, (p: unknown) => void>();
	const host: BridgeHost = {
		source: () => frame,
		post: (m) => sent.push(m),
		state: () => state,
		visible: () => visible,
		subscribe: vi.fn((topic, push) => {
			const stop = vi.fn();
			stops.set(topic, stop);
			pushes.set(topic, push);
			return stop;
		})
	};
	const bridge = new PluginBridge(host, topics);
	const msg = (data: unknown, source: unknown = frame) => bridge.handle({ source, data });
	return { bridge, host, frame, sent, stops, pushes, msg, setState: (p: Partial<PluginState>) => (state = { ...state, ...p }) };
}

describe('plugin bridge (docs/plugin-api.md "Messages")', () => {
	it('ignores messages from other sources or without kotilo: 1', () => {
		const h = harness();
		expect(h.msg({ kotilo: 1, type: 'ready' }, { name: 'other window' })).toBe(false);
		expect(h.msg({ kotilo: 1, type: 'ready' }, null)).toBe(false);
		expect(h.msg({ kotilo: 2, type: 'ready' })).toBe(false);
		expect(h.msg({ type: 'ready' })).toBe(false);
		expect(h.msg('ready')).toBe(false);
		expect(h.msg(null)).toBe(false);
		expect(h.msg({ kotilo: '1', type: 'ready' })).toBe(false);
		expect(h.sent).toEqual([]);
		expect(h.bridge.ready).toBe(false);
	});

	it('answers ready with an init carrying size, settings, theme, locale, editing, available', () => {
		const h = harness();
		expect(h.msg({ kotilo: 1, type: 'ready' })).toBe(true);
		expect(h.sent).toEqual([
			{
				kotilo: 1,
				type: 'init',
				size: '2x2',
				settings: { greeting: 'Hi' },
				theme: { variant: 'dark', tokens: { bg: '#1e1e2e', radius: '16px', 'font-ui': 'Inter', 'font-mono': 'JetBrains Mono' } },
				locale: 'tr-TR',
				editing: false,
				available: true
			}
		]);
		// Plain data (structured-clone safe), not the host's objects.
		const init = h.sent[0] as Extract<DeckMessage, { type: 'init' }>;
		expect(init.settings).not.toBe(STATE.settings);
	});

	it('follows init with visibility:false when the deck is hidden', () => {
		const h = harness(['system'], false);
		h.msg({ kotilo: 1, type: 'ready' });
		expect(h.sent.map((m) => m.type)).toEqual(['init', 'visibility']);
		expect(h.sent[1]).toEqual({ kotilo: 1, type: 'visibility', visible: false });
	});

	it('pushes only changed fields after ready, nothing before', () => {
		const h = harness();
		h.setState({ size: '2x1' });
		h.bridge.update();
		h.bridge.setVisible(false);
		expect(h.sent).toEqual([]);

		h.msg({ kotilo: 1, type: 'ready' });
		h.sent.length = 0;
		h.bridge.update();
		expect(h.sent).toEqual([]);

		h.setState({ size: '2x2', editing: true });
		h.bridge.update();
		expect(h.sent).toEqual([
			{ kotilo: 1, type: 'size', size: '2x2' },
			{ kotilo: 1, type: 'editing', editing: true }
		]);
		h.sent.length = 0;
		h.setState({ settings: { greeting: 'Yo' }, theme: { variant: 'light', tokens: { bg: '#eff1f5' } } });
		h.bridge.update();
		expect(h.sent.map((m) => m.type)).toEqual(['settings', 'theme']);
		h.sent.length = 0;
		h.bridge.setVisible(false);
		h.bridge.setVisible(false);
		h.bridge.setVisible(true);
		expect(h.sent).toEqual([
			{ kotilo: 1, type: 'visibility', visible: false },
			{ kotilo: 1, type: 'visibility', visible: true }
		]);
	});

	it('subscribes only to topics declared in the manifest, once each, and forwards data', () => {
		const h = harness(['system']);
		h.msg({ kotilo: 1, type: 'ready' });
		expect(h.msg({ kotilo: 1, type: 'subscribe', topic: 'media' })).toBe(false);
		expect(h.msg({ kotilo: 1, type: 'subscribe', topic: 'files' })).toBe(false);
		expect(h.msg({ kotilo: 1, type: 'subscribe' })).toBe(false);
		expect(h.host.subscribe).not.toHaveBeenCalled();

		expect(h.msg({ kotilo: 1, type: 'subscribe', topic: 'system' })).toBe(true);
		expect(h.msg({ kotilo: 1, type: 'subscribe', topic: 'system' })).toBe(false);
		expect(h.host.subscribe).toHaveBeenCalledTimes(1);
		expect(h.bridge.subscribed).toEqual(['system']);

		h.sent.length = 0;
		h.pushes.get('system')!({ cpu: 12 });
		expect(h.sent).toEqual([{ kotilo: 1, type: 'data', topic: 'system', payload: { cpu: 12 } }]);

		expect(h.msg({ kotilo: 1, type: 'unsubscribe', topic: 'system' })).toBe(true);
		expect(h.stops.get('system')).toHaveBeenCalledTimes(1);
		expect(h.msg({ kotilo: 1, type: 'unsubscribe', topic: 'system' })).toBe(false);
		h.sent.length = 0;
		h.pushes.get('system')!({ cpu: 13 });
		expect(h.sent).toEqual([]);
	});

	it('ignores unknown message types (additive protocol)', () => {
		const h = harness();
		expect(h.msg({ kotilo: 1, type: 'navigate', url: 'https://example.com' })).toBe(false);
		expect(h.sent).toEqual([]);
	});

	it('stops every topic on destroy and ignores later messages', () => {
		const h = harness(['system', 'media', 'pomodoro']);
		h.msg({ kotilo: 1, type: 'ready' });
		for (const topic of ['system', 'media', 'pomodoro']) h.msg({ kotilo: 1, type: 'subscribe', topic });
		h.bridge.destroy();
		for (const topic of ['system', 'media', 'pomodoro']) expect(h.stops.get(topic)).toHaveBeenCalledTimes(1);
		expect(h.bridge.subscribed).toEqual([]);
		h.sent.length = 0;
		expect(h.msg({ kotilo: 1, type: 'ready' })).toBe(false);
		h.pushes.get('media')!({ session: null });
		expect(h.sent).toEqual([]);
	});

	it('a second ready (frame reload) drops old subscriptions and re-sends init', () => {
		const h = harness();
		h.msg({ kotilo: 1, type: 'ready' });
		h.msg({ kotilo: 1, type: 'subscribe', topic: 'system' });
		h.sent.length = 0;
		h.msg({ kotilo: 1, type: 'ready' });
		expect(h.stops.get('system')).toHaveBeenCalledTimes(1);
		expect(h.sent.map((m) => m.type)).toEqual(['init']);
	});
});

// ── topics ────────────────────────────────────────────────────────────────

function fakeTopic(opts: { subscribeFails?: boolean; withSubscribe?: boolean; initial?: unknown } = {}) {
	let emit: ((p: unknown) => void) | null = null;
	let resolveListen!: (u: () => void) => void;
	const unlisten = vi.fn();
	const ipc: TopicIpc = {
		listen: vi.fn((cb) => {
			emit = cb;
			return new Promise<() => void>((r) => (resolveListen = r));
		}),
		...(opts.withSubscribe === false
			? {}
			: {
					subscribe: vi.fn(() => (opts.subscribeFails ? Promise.reject(new Error('nope')) : Promise.resolve())),
					unsubscribe: vi.fn(() => Promise.resolve())
				}),
		...(opts.initial !== undefined ? { initial: vi.fn(async () => opts.initial) } : {})
	};
	return { ipc, unlisten, emit: (p: unknown) => emit?.(p), listened: () => resolveListen(unlisten) };
}

describe('plugin topics: ref-correct backend subscriptions', () => {
	it('pairs one unsubscribe with a successful subscribe and unlistens even if stop came first', async () => {
		const f = fakeTopic();
		const push = vi.fn();
		const stop = startTopic(f.ipc, push);
		stop();
		stop();
		f.listened();
		await flush();
		expect(f.unlisten).toHaveBeenCalledTimes(1);
		expect(f.ipc.subscribe).toHaveBeenCalledTimes(1);
		expect(f.ipc.unsubscribe).toHaveBeenCalledTimes(1);
		f.emit({ cpu: 1 });
		expect(push).not.toHaveBeenCalled();
	});

	it('does not unsubscribe when subscribe failed; topics without a command just listen', async () => {
		const f = fakeTopic({ subscribeFails: true });
		const stop = startTopic(f.ipc, vi.fn());
		await flush();
		stop();
		await flush();
		expect(f.ipc.unsubscribe).not.toHaveBeenCalled();

		const p = fakeTopic({ withSubscribe: false, initial: { phase: 'idle' } });
		const push = vi.fn();
		const stopP = startTopic(p.ipc, push);
		p.listened();
		await flush();
		expect(push).toHaveBeenCalledWith({ phase: 'idle' });
		p.emit({ phase: 'focus' });
		expect(push).toHaveBeenLastCalledWith({ phase: 'focus' });
		stopP();
		expect(p.unlisten).toHaveBeenCalledTimes(1);
	});
});

// ── theme + icons ─────────────────────────────────────────────────────────

describe('plugin theme payload', () => {
	it('sends every theme --k-* token without the prefix, excluding runtime dim/backdrop', () => {
		const values: Record<string, string> = { '--k-bg': ' #1e1e2e', '--k-radius': '16px', '--k-font-ui': 'Inter', '--k-extra': 'x', '--k-dim': '0.4' };
		const theme = readTheme({
			names: () => ['--k-extra', '--k-dim', '--k-backdrop', 'color'],
			value: (n) => values[n] ?? '',
			colorScheme: () => 'light'
		});
		expect(theme).toEqual({ variant: 'light', tokens: { bg: '#1e1e2e', radius: '16px', 'font-ui': 'Inter', extra: 'x' } });
		expect(THEME_TOKENS).toEqual(expect.arrayContaining(['bg', 'surface-a', 'on-accent', 'radius', 'font-ui', 'font-mono']));
		expect(readTheme({ names: () => [], value: () => '', colorScheme: () => '' }).variant).toBe('dark');
	});

	it('resolves only own icon keys (plugin manifests carry arbitrary strings)', () => {
		expect(isIcon('puzzle')).toBe(true);
		expect(isIcon('toString')).toBe(false);
		expect(isIcon('constructor')).toBe(false);
		expect(iconTone('constructor')).toBe('var(--k-accent)');
		expect(iconTone('clock')).toBe('var(--k-accent)');
		expect(iconTone('cloud')).toBe('var(--k-warning)');
	});
});
