/**
 * Persistence seam for the workspace store. Inside Tauri everything goes through the typed IPC contract;
 * in a plain browser (`pnpm dev`) settings and workspaces live in localStorage so the deck stays usable.
 */
import {
	isTauri,
	monitorList,
	platformCapabilities,
	settingsGet,
	settingsSet,
	traySetWorkspaces,
	workspaceDelete,
	workspaceList,
	workspaceSave,
	type Capability,
	type DeckSettings,
	type Workspace
} from '#lib/ipc/index.ts';

export const DEFAULT_SETTINGS: DeckSettings = {
	schemaVersion: 1,
	theme: null,
	monitor: null,
	activeWorkspace: {},
	alwaysOnTop: false,
	hotkey: 'Ctrl+Alt+K'
};

/** Monitor key used for `settings.activeWorkspace` outside Tauri. */
export const BROWSER_MONITOR = 'browser';

export interface DeckBackend {
	getSettings(): Promise<DeckSettings>;
	setSettings(patch: Partial<DeckSettings>): Promise<DeckSettings>;
	/** Raw entries; the store validates each one. */
	listWorkspaces(): Promise<unknown[]>;
	saveWorkspace(workspace: Workspace): Promise<void>;
	deleteWorkspace(id: string): Promise<void>;
	/** Name of the monitor the deck is on. */
	currentMonitor(settings: DeckSettings): Promise<string>;
	capabilities(): Promise<Capability[]>;
	setTray(items: { id: string; name: string }[], active: string): Promise<void>;
}

export const tauriBackend: DeckBackend = {
	getSettings: settingsGet,
	setSettings: settingsSet,
	listWorkspaces: workspaceList,
	saveWorkspace: workspaceSave,
	deleteWorkspace: workspaceDelete,
	async currentMonitor(settings) {
		// Same rule as the backend (DeckSettings.monitor): pinned, else first non-primary, else primary.
		const monitors = await monitorList();
		const pick =
			monitors.find((m) => m.name === settings.monitor) ?? monitors.find((m) => !m.primary) ?? monitors[0];
		return pick?.name ?? BROWSER_MONITOR;
	},
	capabilities: platformCapabilities,
	setTray: traySetWorkspaces
};

const SETTINGS_KEY = 'kotilo-deck:settings';
const WORKSPACES_KEY = 'kotilo-deck:workspaces';

/** localStorage-backed fallback (also used with an in-memory Storage in tests). */
export function storageBackend(storage: Pick<Storage, 'getItem' | 'setItem'>): DeckBackend {
	const read = <T>(key: string, fallback: T): T => {
		try {
			const raw = storage.getItem(key);
			return raw ? (JSON.parse(raw) as T) : fallback;
		} catch {
			return fallback;
		}
	};
	const readWorkspaces = () => read<Record<string, unknown>>(WORKSPACES_KEY, {});
	return {
		async getSettings() {
			return { ...DEFAULT_SETTINGS, ...read<Partial<DeckSettings>>(SETTINGS_KEY, {}) };
		},
		async setSettings(patch) {
			const next = { ...(await this.getSettings()), ...patch };
			storage.setItem(SETTINGS_KEY, JSON.stringify(next));
			return next;
		},
		async listWorkspaces() {
			return Object.values(readWorkspaces());
		},
		async saveWorkspace(ws) {
			storage.setItem(WORKSPACES_KEY, JSON.stringify({ ...readWorkspaces(), [ws.id]: ws }));
		},
		async deleteWorkspace(id) {
			const all = readWorkspaces();
			delete all[id];
			storage.setItem(WORKSPACES_KEY, JSON.stringify(all));
		},
		async currentMonitor() {
			return BROWSER_MONITOR;
		},
		async capabilities() {
			// No OS access in a browser: only capability-free widgets are available.
			return [];
		},
		async setTray() {}
	};
}

export function memoryStorage(): Pick<Storage, 'getItem' | 'setItem'> {
	const map = new Map<string, string>();
	return { getItem: (k) => map.get(k) ?? null, setItem: (k, v) => void map.set(k, v) };
}

export const defaultBackend = (): DeckBackend =>
	isTauri() ? tauriBackend : storageBackend(typeof localStorage !== 'undefined' ? localStorage : memoryStorage());
