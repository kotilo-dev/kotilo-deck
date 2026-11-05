import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { DeckSettings, Workspace } from '#lib/ipc/index.ts';

const ipc = vi.hoisted(() => ({
	handlers: {} as Record<string, (p: unknown) => void>,
	host: {} as Record<string, unknown>,
	settingsGet: vi.fn(),
	settingsSet: vi.fn(),
	workspaceList: vi.fn(async () => []),
	workspaceSave: vi.fn(async () => {}),
	workspaceDelete: vi.fn(async () => {}),
	platformCapabilities: vi.fn(async () => ['audio']),
	traySetWorkspaces: vi.fn()
}));
vi.mock('#lib/ipc/index.ts', () => ({
	isTauri: () => false,
	isRemote: () => true,
	isDemo: () => false,
	on: (event: string, h: (p: unknown) => void) => {
		ipc.handlers[event] = h;
		return Promise.resolve(() => delete ipc.handlers[event]);
	},
	settingsGet: ipc.settingsGet,
	settingsSet: ipc.settingsSet,
	settingsExportAll: vi.fn(),
	settingsImportAll: vi.fn(),
	workspaceList: ipc.workspaceList,
	workspaceSave: ipc.workspaceSave,
	workspaceDelete: ipc.workspaceDelete,
	platformCapabilities: ipc.platformCapabilities,
	monitorList: vi.fn(),
	traySetWorkspaces: ipc.traySetWorkspaces
}));

import { BROWSER_MONITOR, defaultBackend, memoryStorage, remoteBackend, type DeckBackend } from './backend';
import { WorkspaceStore } from './store.svelte';

beforeEach(() => {
	vi.clearAllMocks();
	ipc.host = { theme: 'nord', activeWorkspace: { DISPLAY1: 'ops' }, hotkey: 'Ctrl+Alt+K' };
	ipc.settingsGet.mockImplementation(async () => ipc.host);
});

describe('remote backend (LAN tablet/browser)', () => {
	it('is the default backend in remote mode', async () => {
		// localStorage is absent under node; the factory still picks the remote backend.
		const b = defaultBackend();
		await expect(b.deleteWorkspace('x')).rejects.toThrow();
		expect(await b.currentMonitor({} as DeckSettings)).toBe(BROWSER_MONITOR);
	});

	it('reads host settings and overlays the per-device theme and active workspace', async () => {
		const b = remoteBackend(memoryStorage());
		let s = await b.getSettings();
		expect(s.theme).toBe('nord');
		expect(s.hotkey).toBe('Ctrl+Alt+K');

		await b.setSettings({ theme: 'dracula', activeWorkspace: { [BROWSER_MONITOR]: 'focus' } });
		expect(ipc.settingsSet).not.toHaveBeenCalled();
		s = await b.getSettings();
		expect(s.theme).toBe('dracula');
		expect(s.activeWorkspace).toEqual({ DISPLAY1: 'ops', [BROWSER_MONITOR]: 'focus' });

		// An explicit null override (default theme) also wins over the host's theme.
		await b.setSettings({ theme: null });
		expect((await b.getSettings()).theme).toBeNull();
	});

	it('never calls settings_set; non per-device keys are rejected', async () => {
		const b = remoteBackend(memoryStorage());
		await b.getSettings();
		await expect(b.setSettings({ hotkey: 'Ctrl+Alt+J' })).rejects.toThrow();
		expect(ipc.settingsSet).not.toHaveBeenCalled();
	});

	it('saves workspaces on the host but keeps delete, import and export on the host only', async () => {
		const b = remoteBackend(memoryStorage());
		const ws = { schemaVersion: 1, id: 'focus', name: 'Focus', theme: null, grid: { cols: 8, rows: 4 }, widgets: [] } as const;
		await b.saveWorkspace({ ...ws, widgets: [] });
		expect(ipc.workspaceSave).toHaveBeenCalled();
		await expect(b.deleteWorkspace('focus')).rejects.toThrow();
		expect(ipc.workspaceDelete).not.toHaveBeenCalled();
		await expect(b.exportAll()).rejects.toThrow();
		await expect(b.importAll('{}')).rejects.toThrow();
		await b.setTray([], 'focus');
		expect(ipc.traySetWorkspaces).not.toHaveBeenCalled();
		expect(await b.capabilities()).toEqual(['audio']);
	});

	it('host settings://changed keeps this device’s overrides', async () => {
		const storage = memoryStorage();
		const b = remoteBackend(storage);
		await b.getSettings();
		await b.setSettings({ theme: 'dracula' });
		const seen: DeckSettings[] = [];
		const off = b.watch({ settings: (s) => seen.push(s), workspaces: () => {} });
		ipc.handlers['settings://changed']({ ...ipc.host, theme: 'latte', hotkey: 'Ctrl+Alt+L' });
		expect(seen.at(-1)?.theme).toBe('dracula');
		expect(seen.at(-1)?.hotkey).toBe('Ctrl+Alt+L');
		expect(typeof ipc.handlers['workspace://changed']).toBe('function');
		off();
	});
});

describe('remote orientation view (DECISIONS 68)', () => {
	const SIZES: Record<string, { w: number; h: number }[]> = {
		clock: [
			{ w: 2, h: 1 },
			{ w: 2, h: 2 }
		],
		todo: [
			{ w: 2, h: 2 },
			{ w: 4, h: 2 }
		]
	};
	const hostWorkspace = (): Workspace => ({
		schemaVersion: 1,
		id: 'ops',
		name: 'Ops',
		theme: null,
		grid: { cols: 8, rows: 4 },
		widgets: [
			{ instance: 'clock-1', widget: 'clock', x: 0, y: 0, w: 2, h: 2, settings: {} },
			{ instance: 'todo-1', widget: 'todo', x: 2, y: 0, w: 4, h: 2, settings: {} },
			{ instance: 'clock-2', widget: 'clock', x: 6, y: 0, w: 2, h: 1, settings: {} }
		]
	});
	const LANDSCAPE = { width: 1024, height: 600 };
	const PORTRAIT = { width: 600, height: 1024 };
	const plain = <T>(v: T): T => JSON.parse(JSON.stringify(v));
	const lastSaved = () => (ipc.workspaceSave.mock.calls.at(-1) as unknown[] | undefined)?.[0] as Workspace;

	function remoteStore(viewport: { width: number; height: number }, backend: DeckBackend = remoteBackend(memoryStorage())) {
		const vp = { ...viewport };
		const store = new WorkspaceStore(backend, { sizesOf: (id) => SIZES[id], viewport: () => vp });
		return { store, vp };
	}

	beforeEach(() => {
		ipc.workspaceList.mockImplementation(async () => [hostWorkspace()] as never);
	});

	it('a portrait remote init reflows in memory and saves nothing', async () => {
		const { store } = remoteStore(PORTRAIT);
		await store.init();
		expect(store.active.grid).toEqual({ cols: 4, rows: 8 });
		expect(store.layoutLock).toBe('landscape');
		await store.flush();
		expect(ipc.workspaceSave).not.toHaveBeenCalled();

		// Layout edits on the reflowed view are refused; nothing becomes dirty.
		expect(store.addWidget('clock', { w: 2, h: 1 })).toBeNull();
		expect(store.removeWidget('clock-1')).toBeNull();
		expect(store.setGrid({ cols: 6, rows: 4 })).toEqual([]);
		await store.flush();
		expect(ipc.workspaceSave).not.toHaveBeenCalled();
	});

	it('a landscape remote edit saves to the host', async () => {
		const { store } = remoteStore(LANDSCAPE);
		await store.init();
		expect(store.layoutLock).toBeNull();
		expect(store.addWidget('clock', { w: 2, h: 1 })).not.toBeNull();
		await store.flush();
		expect(ipc.workspaceSave).toHaveBeenCalledTimes(1);
		expect(lastSaved().grid).toEqual({ cols: 8, rows: 4 });
		expect(lastSaved().widgets).toHaveLength(4);
	});

	it('an orientation change in remote mode saves nothing and restores the stored layout', async () => {
		const { store, vp } = remoteStore(LANDSCAPE);
		await store.init();
		const before = plain(store.active.widgets);
		Object.assign(vp, PORTRAIT);
		store.applyViewport();
		expect(store.active.grid).toEqual({ cols: 4, rows: 8 });
		await store.flush();
		expect(ipc.workspaceSave).not.toHaveBeenCalled();

		Object.assign(vp, LANDSCAPE);
		store.applyViewport();
		expect(store.active.grid).toEqual({ cols: 8, rows: 4 });
		expect(plain(store.active.widgets)).toEqual(before);
		expect(store.layoutLock).toBeNull();
		await store.flush();
		expect(ipc.workspaceSave).not.toHaveBeenCalled();
	});

	it('non-layout edits on a reflowed view save the stored layout', async () => {
		const { store } = remoteStore(PORTRAIT);
		await store.init();
		store.rename('ops', 'Ops board');
		store.updateWidgetSettings('clock-1', { seconds: true });
		await store.flush();
		expect(lastSaved().name).toBe('Ops board');
		expect(lastSaved().grid).toEqual({ cols: 8, rows: 4 });
		const host = hostWorkspace();
		host.widgets[0].settings = { seconds: true };
		expect(lastSaved().widgets).toEqual(host.widgets);
	});

	it('a host change reloads into the reflowed view without saving', async () => {
		const { store } = remoteStore(PORTRAIT);
		await store.init();
		const changed = hostWorkspace();
		changed.widgets.pop();
		ipc.workspaceList.mockImplementation(async () => [changed] as never);
		await store.reload();
		expect(store.active.grid).toEqual({ cols: 4, rows: 8 });
		expect(store.active.widgets).toHaveLength(2);
		await store.flush();
		expect(ipc.workspaceSave).not.toHaveBeenCalled();
	});

	it('the host deck reloading after workspace://changed saves nothing', async () => {
		const host: DeckBackend = { ...remoteBackend(memoryStorage()), remote: false };
		const { store } = remoteStore(LANDSCAPE, host);
		await store.init();
		await store.flush();
		const changed = hostWorkspace();
		changed.name = 'Ops (tablet)';
		ipc.workspaceList.mockImplementation(async () => [changed] as never);
		await store.reload();
		store.applyViewport();
		await store.flush();
		expect(store.active.name).toBe('Ops (tablet)');
		expect(ipc.workspaceSave).not.toHaveBeenCalled();
	});
});
