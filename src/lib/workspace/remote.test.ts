import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { DeckSettings } from '#lib/ipc/index.ts';

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

import { BROWSER_MONITOR, defaultBackend, memoryStorage, remoteBackend } from './backend';

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
