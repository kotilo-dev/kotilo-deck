import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { Workspace } from '#lib/ipc/index.ts';
import { layout } from '#lib/grid/index.ts';
import {
	BROWSER_MONITOR,
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
	system: ['2x2', '4x2']
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
		expect(ws.widgets[0].widget).toBe('pomodoro'); // unknown widget ids are kept
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
		expect(out.widgets).toHaveLength(3);
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

	it('Full places a widget twice with different sizes', () => {
		const clocks = makePreset('full').widgets.filter((w) => w.widget === 'clock');
		expect(clocks).toHaveLength(2);
		expect(new Set(clocks.map((c) => `${c.w}x${c.h}`)).size).toBe(2);
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
		ws.widgets.push({ instance: 'todo-1', widget: 'todo', x: 0, y: 2, w: 4, h: 2, settings: { list: 'x' } });
		store.importJson(JSON.stringify(ws));
		expect(store.active.widgets.find((w) => w.widget === 'todo')).toMatchObject({ x: 0, y: 2, settings: { list: 'x' } });
	});

	it('adds widgets in the first free slot and reports a full grid', async () => {
		const { store } = setup();
		await store.init();
		const placed = store.addWidget('clock', { w: 4, h: 2 })!;
		expect(placed).toMatchObject({ instance: 'clock-2', x: 0, y: 2 });
		expect(store.addWidget('clock', { w: 4, h: 2 })).toMatchObject({ x: 4, y: 2 });
		expect(store.addWidget('clock', { w: 4, h: 2 })).toBeNull();
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
});
