import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { Workspace } from '#lib/ipc/index.ts';
import { layout } from '#lib/grid/index.ts';
import {
	BROWSER_MONITOR,
	DEFAULT_SETTINGS,
	ECHO_GUARD_MS,
	normalizeSettings,
	PRESET_IDS,
	SAVE_DEBOUNCE_MS,
	WorkspaceError,
	WorkspaceStore,
	instanceId,
	makePreset,
	memoryStorage,
	parseWorkspace,
	slugify,
	storageBackend,
	uniqueId,
	type DeckBackend
} from './index';

const SIZES: Record<string, string[]> = {
	clock: ['2x1', '2x2', '4x2'],
	date: ['2x1'],
	weather: ['2x1', '2x2', '4x2'],
	system: ['2x2', '4x2'],
	todo: ['2x2', '4x2', '4x4'],
	media: ['4x1', '4x2'],
	volume: ['2x1', '2x2'],
	// Phase 3: listed here so the tests don't depend on the widget registry.
	pomodoro: ['2x1', '2x2', '4x2'],
	agents: ['2x1', '2x2', '4x2'],
	calendar: ['4x2', '4x4']
};
const sizesOf = (id: string) =>
	SIZES[id]?.map((s) => {
		const [w, h] = s.split('x').map(Number);
		return { w, h };
	});

function setup(viewport = { width: 1280, height: 720 }, backend: DeckBackend = storageBackend(memoryStorage())) {
	const spy = {
		save: vi.spyOn(backend, 'saveWorkspace'),
		del: vi.spyOn(backend, 'deleteWorkspace'),
		tray: vi.spyOn(backend, 'setTray'),
		settings: vi.spyOn(backend, 'setSettings')
	};
	const vp = { ...viewport };
	const store = new WorkspaceStore(backend, { sizesOf, viewport: () => vp });
	return { store, backend, spy, vp };
}

describe('validation', () => {
	const valid = () => structuredClone(makePreset('focus')) as unknown as Record<string, any>;

	it('accepts presets and the SPEC §4.1 example (no schemaVersion)', () => {
		expect(parseWorkspace(valid()).id).toBe('focus');
		const spec = {
			id: 'focus',
			name: 'Focus',
			theme: null,
			grid: { cols: 8, rows: 4 },
			appearance: { background: 'wallpaper:mountains.jpg', opacity: 0.8, blur: 0 },
			widgets: [
				{ instance: 'pomo-1', widget: 'pomodoro', x: 4, y: 0, w: 2, h: 2, settings: { appearance: { opacity: 0.6 } } }
			]
		};
		const ws = parseWorkspace(spec);
		expect(ws.schemaVersion).toBe(1);
		expect(ws.widgets[0].widget).toBe('pomodoro'); // widget ids aren't checked against the registry
	});

	it.each([
		['not an object', 42, 'shell.errors.notObject'],
		['future schema', { ...makePreset('ops'), schemaVersion: 9 }, 'shell.errors.version'],
		['no name', { ...makePreset('ops'), name: ' ' }, 'shell.errors.name'],
		['bad grid', { ...makePreset('ops'), grid: { cols: 0, rows: 4 } }, 'shell.errors.grid'],
		['huge grid', { ...makePreset('ops'), grid: { cols: 99, rows: 4 } }, 'shell.errors.grid'],
		['widgets not array', { ...makePreset('ops'), widgets: {} }, 'shell.errors.widgets'],
		['bad theme', { ...makePreset('ops'), theme: 3 }, 'shell.errors.theme']
	])('rejects %s', (_, input, key) => {
		let err: unknown;
		try {
			parseWorkspace(input);
		} catch (e) {
			err = e;
		}
		expect(err).toBeInstanceOf(WorkspaceError);
		expect((err as WorkspaceError).key).toBe(key);
		expect((err as WorkspaceError).message).not.toBe(key);
	});

	it('rejects a malformed widget and names it', () => {
		const ws = valid();
		ws.widgets[1].w = 0;
		expect(() => parseWorkspace(ws)).toThrow(/2/);
		const ws2 = valid();
		ws2.widgets[0].x = -1;
		expect(() => parseWorkspace(ws2)).toThrow(WorkspaceError);
	});

	it('repairs ids and duplicate instances without dropping data', () => {
		const ws = valid();
		ws.id = 'Not A Slug';
		ws.name = 'Mein Büro';
		ws.extra = { keep: true };
		ws.widgets[1].instance = ws.widgets[0].instance;
		const out = parseWorkspace(ws) as Workspace & { extra: unknown };
		expect(out.id).toBe('mein-buro');
		expect(out.extra).toEqual({ keep: true });
		expect(new Set(out.widgets.map((w) => w.instance)).size).toBe(out.widgets.length);
		expect(out.widgets).toHaveLength(valid().widgets.length);
	});

	it('slugifies and de-duplicates ids', () => {
		expect(slugify('Çalışma Alanı 2')).toBe('calisma-alani-2');
		expect(slugify('!!!')).toBe('workspace');
		expect(uniqueId('focus', ['focus', 'focus-2'])).toBe('focus-3');
		expect(instanceId('clock', ['clock-1', 'clock-3'])).toBe('clock-2');
	});
});

describe('presets', () => {
	it.each(PRESET_IDS)('%s is valid, collision-free and uses supported sizes', (id) => {
		const ws = parseWorkspace(makePreset(id));
		expect(layout(ws.widgets, ws.grid).unplaced).toEqual([]);
		for (const w of ws.widgets) expect(SIZES[w.widget]).toContain(`${w.w}x${w.h}`);
	});

	it('Full places every widget once', () => {
		const ids = makePreset('full').widgets.map((w) => w.widget);
		expect([...ids].sort()).toEqual(Object.keys(SIZES).sort());
	});

	it('follow SPEC §4.1: Focus, Ops and Agents carry their core widgets', () => {
		const has = (id: 'focus' | 'ops' | 'agents', widgets: string[]) =>
			expect(makePreset(id).widgets.map((w) => w.widget)).toEqual(expect.arrayContaining(widgets));
		has('focus', ['clock', 'pomodoro', 'todo']);
		has('ops', ['system', 'media', 'calendar']);
		has('agents', ['agents', 'pomodoro', 'todo']);
	});

	it('presets are distinct layouts', () => {
		const sigs = PRESET_IDS.map((id) => JSON.stringify(makePreset(id).widgets.map((w) => [w.widget, w.x, w.y, w.w, w.h])));
		expect(new Set(sigs).size).toBe(PRESET_IDS.length);
	});
});

describe('WorkspaceStore', () => {
	beforeEach(() => {
		vi.useFakeTimers();
	});
	afterEach(() => {
		vi.useRealTimers();
	});

	it('seeds presets on first run and saves them (debounced)', async () => {
		const { store, spy } = setup();
		await store.init();
		expect(store.workspaces.map((w) => w.id)).toEqual(['focus', 'ops', 'agents', 'full']);
		expect(store.activeId).toBe('focus');
		expect(spy.save).not.toHaveBeenCalled();
		await vi.advanceTimersByTimeAsync(SAVE_DEBOUNCE_MS);
		expect(spy.save).toHaveBeenCalledTimes(4);
		expect(spy.tray).toHaveBeenCalledWith(expect.arrayContaining([{ id: 'ops', name: makePreset('ops').name }]), 'focus');
	});

	it('survives a restart: workspaces, edits and the active one per monitor', async () => {
		const backend = storageBackend(memoryStorage());
		const a = setup(undefined, backend).store;
		await a.init();
		a.switchTo('ops');
		a.rename('ops', 'Operations');
		a.removeWidget('clock-1');
		await a.flush();

		const b = setup(undefined, backend).store;
		await b.init();
		expect(b.activeId).toBe('ops');
		expect(b.active.name).toBe('Operations');
		expect(b.active.widgets.some((w) => w.instance === 'clock-1')).toBe(false);
		expect(b.settings.activeWorkspace[BROWSER_MONITOR]).toBe('ops');
	});

	it('coalesces rapid edits into one save per workspace', async () => {
		const { store, spy } = setup();
		await store.init();
		await store.flush();
		spy.save.mockClear();
		store.addWidget('date', { w: 2, h: 1 });
		store.addWidget('date', { w: 2, h: 1 });
		await vi.advanceTimersByTimeAsync(SAVE_DEBOUNCE_MS / 2);
		store.addWidget('date', { w: 2, h: 1 });
		await vi.advanceTimersByTimeAsync(SAVE_DEBOUNCE_MS);
		expect(spy.save).toHaveBeenCalledTimes(1);
		expect(spy.save.mock.calls[0][0].widgets.filter((w) => w.widget === 'date')).toHaveLength(4);
	});

	it('creates, duplicates and switches', async () => {
		const { store } = setup();
		await store.init();
		const ws = store.create('  Meeting ');
		expect(ws.id).toBe('meeting');
		expect(store.activeId).toBe('meeting');
		const copy = store.duplicate('focus')!;
		expect(copy.id).not.toBe('focus');
		expect(copy.widgets).toEqual(store.workspaces.find((w) => w.id === 'focus')!.widgets);
		expect(store.activeId).toBe(copy.id);
		expect(store.create('Meeting').id).toBe('meeting-2');
	});

	it('cycles with wrap-around and records the direction', async () => {
		const { store } = setup();
		await store.init();
		store.cycle(-1);
		expect(store.activeId).toBe('full');
		expect(store.lastDir).toBe(-1);
		store.cycle(1);
		expect(store.activeId).toBe('focus');
		store.switchTo('nope');
		expect(store.activeId).toBe('focus');
	});

	it('never deletes the last workspace and moves off a deleted active one', async () => {
		const { store, spy } = setup();
		await store.init();
		expect(await store.remove('focus')).toBe(true);
		expect(store.activeId).toBe('ops');
		expect(spy.del).toHaveBeenCalledWith('focus');
		await store.remove('ops');
		await store.remove('agents');
		expect(store.workspaces).toHaveLength(1);
		expect(await store.remove('full')).toBe(false);
		expect(store.workspaces).toHaveLength(1);
	});

	it('rejects empty renames', async () => {
		const { store } = setup();
		await store.init();
		expect(store.rename('focus', '   ')).toBe(false);
		expect(store.active.name).toBe(makePreset('focus').name);
	});

	it('resets to a preset keeping id and name', async () => {
		const { store } = setup();
		await store.init();
		store.rename('focus', 'Mine');
		store.setTheme('workspace', 'nord');
		store.setWidgets([]);
		store.resetToPreset('focus', 'full');
		expect(store.active.name).toBe('Mine');
		expect(store.active.theme).toBeNull();
		expect(store.active.widgets).toHaveLength(makePreset('full').widgets.length);
	});

	it('exports and re-imports a workspace under a fresh id', async () => {
		const { store } = setup();
		await store.init();
		const json = store.exportJson('full');
		const imported = store.importJson(json);
		expect(imported.id).toBe('full-2');
		expect(store.activeId).toBe('full-2');
		expect(store.active.widgets).toEqual(JSON.parse(json).widgets);
	});

	it('rejects bad imports with a message and changes nothing', async () => {
		const { store } = setup();
		await store.init();
		expect(() => store.importJson('{nope')).toThrow(WorkspaceError);
		expect(() => store.importJson('{"name":"x","grid":{"cols":8,"rows":4}}')).toThrow(WorkspaceError);
		expect(store.workspaces).toHaveLength(4);
	});

	it('keeps unknown widgets through a round trip', async () => {
		const { store } = setup();
		await store.init();
		const ws = makePreset('focus');
		ws.name = 'Imported';
		ws.widgets.push({ instance: 'nonexistent-widget-1', widget: 'nonexistent-widget', x: 6, y: 2, w: 2, h: 2, settings: { list: 'x' } });
		store.importJson(JSON.stringify(ws));
		expect(store.active.widgets.find((w) => w.widget === 'nonexistent-widget')).toMatchObject({ x: 6, y: 2, settings: { list: 'x' } });
	});

	it('adds widgets in the first free slot and reports a full grid', async () => {
		const { store } = setup();
		await store.init();
		// Focus leaves 6,2 (2×1) and the 4×1 strip at 4,3 free.
		expect(store.addWidget('clock', { w: 4, h: 2 })).toBeNull();
		expect(store.addWidget('clock', { w: 2, h: 1 })).toMatchObject({ instance: 'clock-2', x: 6, y: 2 });
		expect(store.addWidget('media', { w: 4, h: 1 })).toMatchObject({ x: 4, y: 3 });
		expect(store.addWidget('date', { w: 2, h: 1 })).toBeNull();
	});

	it('places the same widget twice at different sizes (Phase 1 acceptance)', async () => {
		const { store } = setup();
		await store.init();
		store.create('Blank');
		store.addWidget('clock', { w: 4, h: 2 });
		store.addWidget('clock', { w: 2, h: 1 });
		const clocks = store.active.widgets.filter((w) => w.widget === 'clock');
		expect(clocks.map((c) => c.instance)).toEqual(['clock-1', 'clock-2']);
		expect(new Set(clocks.map((c) => `${c.w}x${c.h}`)).size).toBe(2);
	});

	it('density change reflows and lists what does not fit, keeping it', async () => {
		const { store } = setup();
		await store.init();
		store.switchTo('full');
		const lost = store.setGrid({ cols: 6, rows: 3 });
		expect(lost.length).toBeGreaterThan(0);
		expect(store.active.widgets).toHaveLength(makePreset('full').widgets.length);
		expect(store.unplaced.full).toEqual(lost);
		expect(store.setGrid({ cols: 12, rows: 6 })).toEqual([]);
	});

	it('follows orientation only for workspaces on the auto density', async () => {
		const { store, vp } = setup();
		await store.init();
		store.switchTo('ops');
		store.setGrid({ cols: 10, rows: 5 });
		vp.width = 600;
		vp.height = 1024;
		store.applyViewport();
		const focus = store.workspaces.find((w) => w.id === 'focus')!;
		expect(focus.grid).toEqual({ cols: 4, rows: 8 });
		expect(store.workspaces.find((w) => w.id === 'ops')!.grid).toEqual({ cols: 10, rows: 5 });
		expect(layout(focus.widgets, focus.grid).unplaced).toEqual([]);
	});

	it('seeds portrait presets on a portrait screen', async () => {
		const { store } = setup({ width: 600, height: 1024 });
		await store.init();
		expect(store.active.grid).toEqual({ cols: 4, rows: 8 });
		for (const ws of store.workspaces) expect(layout(ws.widgets, ws.grid).unplaced).toEqual([]);
		expect(Object.values(store.unplaced).flat()).toEqual([]);
	});

	it('restores the per-monitor workspace when the monitor changes', async () => {
		const { store } = setup();
		await store.init();
		store.setMonitor('DISPLAY2');
		store.switchTo('agents');
		store.setMonitor(BROWSER_MONITOR);
		expect(store.activeId).toBe('focus');
		store.setMonitor('DISPLAY2');
		expect(store.activeId).toBe('agents');
	});

	it('theme scopes: global setting vs workspace override', async () => {
		const { store, spy } = setup();
		await store.init();
		store.setTheme('global', 'dracula');
		expect(store.settings.theme).toBe('dracula');
		await store.flush();
		expect(spy.settings).toHaveBeenCalledWith({ theme: 'dracula' });
		store.setTheme('workspace', 'nord');
		expect(store.active.theme).toBe('nord');
		store.setTheme('workspace', null);
		expect(store.active.theme).toBeNull();
	});

	it('skips invalid stored workspaces without deleting them', async () => {
		const storage = memoryStorage();
		storage.setItem('kotilo-deck:workspaces', JSON.stringify({ bad: { id: 'bad' }, ok: makePreset('ops') }));
		const { store, spy } = setup(undefined, storageBackend(storage));
		await store.init();
		expect(store.workspaces.map((w) => w.id)).toEqual(['ops']);
		expect(spy.del).not.toHaveBeenCalled();
		expect(JSON.parse(storage.getItem('kotilo-deck:workspaces')!).bad).toEqual({ id: 'bad' });
	});

	it('serializes saves and deletes per workspace: a slow save never lands after the delete', async () => {
		const storage = memoryStorage();
		const backend = storageBackend(storage);
		const log: string[] = [];
		let release!: () => void;
		const gate = new Promise<void>((r) => (release = r));
		const realSave = backend.saveWorkspace.bind(backend);
		backend.saveWorkspace = async (ws) => {
			log.push(`save:${ws.id}:start`);
			if (ws.id === 'ops') await gate;
			await realSave(ws);
			log.push(`save:${ws.id}:end`);
		};
		const realDel = backend.deleteWorkspace.bind(backend);
		backend.deleteWorkspace = async (id) => {
			log.push(`delete:${id}`);
			await realDel(id);
		};
		const { store } = setup(undefined, backend);
		await store.init();
		await vi.advanceTimersByTimeAsync(SAVE_DEBOUNCE_MS); // seeding saves start; ops blocks
		const removed = store.remove('ops');
		await vi.advanceTimersByTimeAsync(10);
		expect(log).not.toContain('delete:ops');
		release();
		await removed;
		expect(log.indexOf('save:ops:end')).toBeLessThan(log.indexOf('delete:ops'));
		expect(Object.keys(JSON.parse(storage.getItem('kotilo-deck:workspaces')!))).not.toContain('ops');
	});

	it('writes settings in order even when the backend answers at different speeds', async () => {
		const backend = storageBackend(memoryStorage());
		const seen: string[] = [];
		const real = backend.setSettings.bind(backend);
		let n = 0;
		backend.setSettings = async (patch) => {
			const delay = [30, 1, 15][n++ % 3];
			await new Promise((r) => setTimeout(r, delay));
			seen.push(patch.activeWorkspace?.[BROWSER_MONITOR] ?? '?');
			return real(patch);
		};
		const { store } = setup(undefined, backend);
		await store.init();
		await vi.advanceTimersByTimeAsync(100);
		await store.flush();
		seen.length = 0;
		store.switchTo('ops');
		store.switchTo('agents');
		store.switchTo('full');
		await vi.advanceTimersByTimeAsync(100);
		await store.flush();
		expect(seen).toEqual(['ops', 'agents', 'full']);
		expect(store.activeId).toBe('full');
		expect((await backend.getSettings()).activeWorkspace[BROWSER_MONITOR]).toBe('full');
	});

	it('ignores a stale activeWorkspace echo but follows another window afterwards', async () => {
		const { store } = setup();
		await store.init();
		await store.flush();
		store.switchTo('agents');
		await store.flush();
		const echo = { ...store.settings, activeWorkspace: { [BROWSER_MONITOR]: 'ops' } };
		store.applyRemoteSettings(echo);
		expect(store.activeId).toBe('agents');
		await vi.advanceTimersByTimeAsync(ECHO_GUARD_MS + 1);
		store.applyRemoteSettings({ ...echo, theme: 'nord' });
		expect(store.activeId).toBe('ops');
		expect(store.settings.theme).toBe('nord');
	});

	it('reloads files written by another window, keeping unsaved local edits', async () => {
		const backend = storageBackend(memoryStorage());
		const { store } = setup(undefined, backend);
		await store.init();
		await store.flush();
		const other = new WorkspaceStore(backend, { sizesOf, viewport: () => ({ width: 1280, height: 720 }), role: 'settings' });
		await other.init();
		other.rename('ops', 'Ops renamed');
		other.duplicate('full');
		await other.flush();
		const focusBefore = store.workspaces.find((w) => w.id === 'focus');
		store.addWidget('date', { w: 2, h: 1 }); // unsaved edit on the active (focus) workspace
		await store.reload();
		expect(store.workspaces.find((w) => w.id === 'ops')!.name).toBe('Ops renamed');
		expect(store.workspaces).toHaveLength(5);
		expect(store.workspaces.find((w) => w.id === 'focus')).toBe(focusBefore);
		expect(store.activeId).toBe('focus');
	});

	it('reload never resurrects a workspace deleted elsewhere, and the other window wins on a changed file', async () => {
		const backend = storageBackend(memoryStorage());
		const { store } = setup(undefined, backend);
		await store.init();
		await store.flush();
		store.switchTo('ops');
		const other = new WorkspaceStore(backend, { sizesOf, viewport: () => ({ width: 1280, height: 720 }), role: 'settings' });
		await other.init();
		await other.remove('agents');
		other.rename('full', 'Everything');
		await other.flush();
		// Unsaved deck edits on both workspaces the settings window touched, plus a brand-new one.
		store.switchTo('agents');
		store.addWidget('date', { w: 2, h: 1 });
		store.switchTo('full');
		store.setGrid({ cols: 12, rows: 6 });
		const fresh = store.create('Brand new');
		await store.reload();
		await store.flush();
		const ids = ((await backend.listWorkspaces()) as Workspace[]).map((w) => w.id);
		expect(ids).not.toContain('agents');
		expect(store.workspaces.some((w) => w.id === 'agents')).toBe(false);
		expect(ids).toContain(fresh.id);
		const full = ((await backend.listWorkspaces()) as Workspace[]).find((w) => w.id === 'full')!;
		expect(full.name).toBe('Everything');
		expect(full.grid).toEqual({ cols: 8, rows: 4 });
	});

	it('settings role: no switching, no tray, no seeding writes, no viewport reflow', async () => {
		const backend = storageBackend(memoryStorage());
		const tray = vi.spyOn(backend, 'setTray');
		const save = vi.spyOn(backend, 'saveWorkspace');
		const store = new WorkspaceStore(backend, { sizesOf, viewport: () => ({ width: 600, height: 1024 }), role: 'settings' });
		await store.init();
		await vi.advanceTimersByTimeAsync(SAVE_DEBOUNCE_MS);
		expect(save).not.toHaveBeenCalled();
		expect(store.active.grid).toEqual({ cols: 8, rows: 4 });
		store.duplicate('ops');
		expect(store.activeId).toBe('focus');
		store.setDefaultFor('DISPLAY2', 'ops');
		await store.flush();
		expect((await backend.getSettings()).activeWorkspace.DISPLAY2).toBe('ops');
		expect(tray).not.toHaveBeenCalled();
	});

	it('undo restores a removed widget in place, or in the first free slot', async () => {
		const { store } = setup();
		await store.init();
		store.create('Blank');
		store.addWidget('clock', { w: 4, h: 2 });
		store.addWidget('todo', { w: 2, h: 2 });
		const before = JSON.parse(JSON.stringify(store.active.widgets)) as Workspace['widgets'];
		const removed = store.removeWidget('clock-1')!;
		expect(removed.widget.instance).toBe('clock-1');
		expect(store.restoreWidget(removed.workspace, removed.widget)).toBe(true);
		expect(store.active.widgets.find((w) => w.instance === 'clock-1')).toMatchObject(before.find((w) => w.instance === 'clock-1')!);
		const again = store.removeWidget('clock-1')!;
		const added = store.addWidget('clock', { w: 4, h: 2 })!; // takes the old cells and the old id
		expect([added.instance, added.x, added.y]).toEqual(['clock-1', 0, 0]);
		expect(store.restoreWidget(again.workspace, again.widget)).toBe(true);
		const restored = store.active.widgets.at(-1)!;
		expect(restored.instance).not.toBe('clock-1');
		expect([restored.x, restored.y]).not.toEqual([0, 0]);
		expect(layout(store.active.widgets, store.active.grid).unplaced).toEqual([]);
		expect(store.removeWidget('nope')).toBeNull();
	});

	it('updateSettings rejects with the backend error and re-syncs', async () => {
		const backend = storageBackend(memoryStorage());
		const real = backend.setSettings.bind(backend);
		backend.setSettings = async (patch) => {
			if (patch.hotkey === 'Ctrl+Alt+X') throw new Error('hotkey taken');
			return real(patch);
		};
		const { store } = setup(undefined, backend);
		await store.init();
		await expect(store.updateSettings({ hotkey: 'Ctrl+Alt+X' })).rejects.toThrow('hotkey taken');
		expect(store.settings.hotkey).toBe(DEFAULT_SETTINGS.hotkey);
		await store.updateSettings({ hotkey: 'Ctrl+Alt+J' });
		expect(store.settings.hotkey).toBe('Ctrl+Alt+J');
	});
});

describe('settings normalization and browser backup', () => {
	it('fills partial Phase 2 groups', () => {
		const s = normalizeSettings({ theme: 'nord', burnIn: { night: { enabled: true } }, appearance: { blur: 8 } });
		expect(s.theme).toBe('nord');
		expect(s.burnIn.night).toEqual({ ...DEFAULT_SETTINGS.burnIn.night, enabled: true });
		expect(s.burnIn.idle).toEqual(DEFAULT_SETTINGS.burnIn.idle);
		expect(s.appearance).toEqual({ ...DEFAULT_SETTINGS.appearance, blur: 8 });
		expect(normalizeSettings(null)).toEqual(DEFAULT_SETTINGS);
	});

	it('exports and imports everything in a browser', async () => {
		const a = storageBackend(memoryStorage());
		await a.setSettings({ theme: 'dracula' });
		await a.saveWorkspace(makePreset('ops'));
		const json = await a.exportAll();
		const b = storageBackend(memoryStorage());
		await b.importAll(json);
		expect((await b.getSettings()).theme).toBe('dracula');
		expect(((await b.listWorkspaces()) as Workspace[]).map((w) => w.id)).toEqual(['ops']);
		await expect(b.importAll('{"kind":"other"}')).rejects.toThrow();
		await expect(b.importAll('{nope')).rejects.toThrow();
	});
});
