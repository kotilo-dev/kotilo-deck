/**
 * Persistence seam for the workspace store. Inside Tauri everything goes through the typed IPC contract;
 * in a plain browser (`pnpm dev`) settings and workspaces live in localStorage so the deck stays usable.
 */
import { emitTo } from '@tauri-apps/api/event';
import { getCurrentWindow } from '@tauri-apps/api/window';
import {
	isTauri,
	monitorList,
	on,
	settingsExportAll,
	settingsImportAll,
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
import { t } from '#lib/i18n/index.ts';

export const DEFAULT_SETTINGS: DeckSettings = {
	schemaVersion: 1,
	theme: null,
	monitor: null,
	activeWorkspace: {},
	alwaysOnTop: false,
	hotkey: 'Ctrl+Alt+K',
	autostart: false,
	displayMode: 'fullscreen',
	appearance: { background: 'solid', wallpaper: null, surfaceOpacity: null, blur: 0, windowOpacity: 1 },
	burnIn: {
		shiftMinutes: 10,
		night: { enabled: false, from: '22:00', to: '07:00', dim: 0.5 },
		idle: { enabled: false, minutes: 15, dim: 0.4, clockFace: true }
	},
	themeAuto: { mode: 'off', lightFrom: '07:00', darkFrom: '19:00' },
	temperatureUnit: 'c',
	notificationSound: true
};

const obj = (v: unknown): Record<string, unknown> =>
	typeof v === 'object' && v !== null && !Array.isArray(v) ? (v as Record<string, unknown>) : {};

/**
 * Fills missing/partial Phase 2 groups with defaults (a settings file from Phase 1, a hand-edited
 * localStorage entry, or an older export). Never throws.
 */
export function normalizeSettings(input: unknown): DeckSettings {
	const s = obj(input);
	const d = DEFAULT_SETTINGS;
	const burnIn = obj(s.burnIn);
	return {
		...d,
		...s,
		activeWorkspace: { ...obj(s.activeWorkspace) } as Record<string, string>,
		appearance: { ...d.appearance, ...obj(s.appearance) },
		burnIn: {
			...d.burnIn,
			...burnIn,
			night: { ...d.burnIn.night, ...obj(burnIn.night) },
			idle: { ...d.burnIn.idle, ...obj(burnIn.idle) }
		},
		themeAuto: { ...d.themeAuto, ...obj(s.themeAuto) }
	} as DeckSettings;
}

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
	/**
	 * Changes made by another window (Tauri `settings://changed` / `workspace://changed`; in a browser the
	 * `storage` event from another tab). Returns the unsubscribe.
	 */
	watch(handlers: ExternalHandlers): () => void;
	/** Tell the other window (deck ↔ settings) that workspace files changed. */
	notifyWorkspacesChanged(): Promise<void>;
	/** Same for settings written outside `setSettings` (import). */
	notifySettingsChanged(settings: DeckSettings): Promise<void>;
	/** All settings + workspaces as one JSON document. */
	exportAll(): Promise<string>;
	importAll(json: string): Promise<void>;
}

export interface ExternalHandlers {
	settings?: (settings: DeckSettings) => void;
	workspaces?: () => void;
}

/** Window labels (tauri.conf / settings_open). */
export const DECK_WINDOW = 'deck';
export const SETTINGS_WINDOW = 'settings';

const otherWindow = (): string => {
	try {
		return getCurrentWindow().label === SETTINGS_WINDOW ? DECK_WINDOW : SETTINGS_WINDOW;
	} catch {
		return DECK_WINDOW;
	}
};

/** Browser export format (the Tauri backend defines its own; only it reads it back). */
export interface BrowserExport {
	kind: 'kotilo-deck-export';
	schemaVersion: 1;
	settings: DeckSettings;
	workspaces: unknown[];
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
	setTray: traySetWorkspaces,
	watch(handlers) {
		const offs = [
			handlers.settings ? on('settings://changed', handlers.settings) : null,
			handlers.workspaces ? on('workspace://changed', () => handlers.workspaces?.()) : null
		];
		return () => {
			for (const p of offs) void p?.then((off) => off());
		};
	},
	async notifyWorkspacesChanged() {
		try {
			await emitTo(otherWindow(), 'workspace://changed', {});
		} catch {
			// The other window may not exist; nothing to tell.
		}
	},
	async notifySettingsChanged(settings) {
		try {
			await emitTo(otherWindow(), 'settings://changed', settings);
		} catch {
			// As above.
		}
	},
	exportAll: settingsExportAll,
	importAll: settingsImportAll
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
			return normalizeSettings(read<unknown>(SETTINGS_KEY, {}));
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
		async setTray() {},
		watch(handlers) {
			if (typeof window === 'undefined') return () => {};
			const onStorage = (e: StorageEvent) => {
				if (e.key === SETTINGS_KEY) void this.getSettings().then((s) => handlers.settings?.(s));
				else if (e.key === WORKSPACES_KEY) handlers.workspaces?.();
			};
			window.addEventListener('storage', onStorage);
			return () => window.removeEventListener('storage', onStorage);
		},
		async notifyWorkspacesChanged() {
			// Other tabs get the `storage` event automatically.
		},
		async notifySettingsChanged() {},
		async exportAll() {
			const doc: BrowserExport = {
				kind: 'kotilo-deck-export',
				schemaVersion: 1,
				settings: await this.getSettings(),
				workspaces: await this.listWorkspaces()
			};
			return JSON.stringify(doc, null, 2) + '\n';
		},
		async importAll(json) {
			let doc: Partial<BrowserExport>;
			try {
				doc = JSON.parse(json);
			} catch {
				throw new Error(t('shell.errors.json'));
			}
			if (doc?.kind !== 'kotilo-deck-export' || typeof doc.settings !== 'object' || !Array.isArray(doc.workspaces)) {
				throw new Error(t('settings.backup.invalid'));
			}
			const all: Record<string, unknown> = {};
			for (const w of doc.workspaces) {
				const id = (w as { id?: unknown } | null)?.id;
				if (typeof id === 'string') all[id] = w;
			}
			storage.setItem(SETTINGS_KEY, JSON.stringify(normalizeSettings(doc.settings)));
			storage.setItem(WORKSPACES_KEY, JSON.stringify(all));
		}
	};
}

export function memoryStorage(): Pick<Storage, 'getItem' | 'setItem'> {
	const map = new Map<string, string>();
	return { getItem: (k) => map.get(k) ?? null, setItem: (k, v) => void map.set(k, v) };
}

export const defaultBackend = (): DeckBackend =>
	isTauri() ? tauriBackend : storageBackend(typeof localStorage !== 'undefined' ? localStorage : memoryStorage());
