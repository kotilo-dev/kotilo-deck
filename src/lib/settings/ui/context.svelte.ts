/**
 * Settings-window state shared by the section components: the workspace/settings store (role
 * "settings"), toasts (DeckUi), and environment facts (version, paths, monitors, wallpapers, capabilities).
 */
import { getContext, setContext } from 'svelte';
import { getVersion } from '@tauri-apps/api/app';
import { appDataDir, homeDir, join } from '@tauri-apps/api/path';
import {
	isTauri,
	monitorList,
	wallpaperImport,
	wallpaperList,
	type Capability,
	type DeckAppearance,
	type DeckSettings,
	type MonitorInfo,
	type Wallpaper
} from '#lib/ipc/index.ts';
import { BROWSER_MONITOR, type DeckBackend, type WorkspaceStore } from '#lib/workspace/index.ts';
import type { DeckUi } from '#lib/ui/deck.svelte.ts';
import pkg from '../../../../package.json';

export const SECTIONS = [
	'general',
	'display',
	'appearance',
	'themes',
	'workspaces',
	'weather',
	'companion',
	'burnin',
	'about'
] as const;
export type SectionId = (typeof SECTIONS)[number];

export const isSection = (s: string): s is SectionId => (SECTIONS as readonly string[]).includes(s);

export interface AppearanceDraft {
	appearance: Partial<DeckAppearance>;
	night?: boolean;
	idle?: boolean;
}

export const draftDirty = (d: AppearanceDraft): boolean =>
	Object.keys(d.appearance).length > 0 || d.night !== undefined || d.idle !== undefined;

export class SettingsEnv {
	version = $state(pkg.version);
	/** Display paths; "~" form until the real ones are known. */
	settingsPath = $state('~/.kotilo/deck/settings.json');
	logsPath = $state('~/.kotilo/deck/logs');
	workspacesPath = $state('~/.kotilo/deck/workspaces');
	themesPath = $state('~/.kotilo/themes');
	wallpapersPath = $state('~/.kotilo/deck/wallpapers');
	capabilities = $state<ReadonlySet<Capability> | null>(null);
	monitors = $state<MonitorInfo[]>([]);
	monitorsLoaded = $state(false);
	wallpapers = $state<Wallpaper[]>([]);
	/** Unapplied Appearance-page edits (kept while browsing other sections; DESIGN §7 "Apply"). */
	draft = $state<AppearanceDraft>({ appearance: {} });
	readonly tauri = isTauri();

	async load(capabilities: () => Promise<Capability[]>): Promise<void> {
		capabilities()
			.then((c) => (this.capabilities = new Set(c)))
			.catch(() => (this.capabilities = new Set()));
		void this.loadMonitors();
		void this.loadWallpapers();
		if (!this.tauri) {
			this.settingsPath = 'localStorage · kotilo-deck:settings';
			return;
		}
		try {
			this.version = await getVersion();
		} catch {
			// keep package.json version
		}
		try {
			const home = await homeDir();
			const deck = await join(home, '.kotilo', 'deck');
			this.logsPath = await join(deck, 'logs');
			this.workspacesPath = await join(deck, 'workspaces');
			this.wallpapersPath = await join(deck, 'wallpapers');
			this.themesPath = await join(home, '.kotilo', 'themes');
			// tauri-plugin-store keeps settings.json in the app data dir.
			this.settingsPath = await join(await appDataDir(), 'settings.json');
		} catch {
			// keep the "~" forms
		}
	}

	async loadMonitors(): Promise<void> {
		if (!this.tauri) {
			this.monitorsLoaded = true;
			return;
		}
		try {
			this.monitors = await monitorList();
		} catch {
			this.monitors = [];
		}
		this.monitorsLoaded = true;
	}

	async loadWallpapers(): Promise<void> {
		if (!this.tauri) return;
		try {
			this.wallpapers = await wallpaperList();
		} catch {
			this.wallpapers = [];
		}
	}

	/** Native picker; null when cancelled. */
	async importWallpaper(): Promise<Wallpaper | null> {
		const w = await wallpaperImport();
		if (w) await this.loadWallpapers();
		return w;
	}

	/** Monitor names for "default workspace per monitor": connected ones, else the browser key. */
	monitorKeys(store: WorkspaceStore): string[] {
		const names = this.monitors.map((m) => m.name);
		if (!this.tauri) names.push(BROWSER_MONITOR);
		for (const k of Object.keys(store.settings.activeWorkspace)) if (!names.includes(k) && this.tauri) names.push(k);
		return names;
	}
}

export interface SettingsContext {
	store: WorkspaceStore;
	ui: DeckUi;
	env: SettingsEnv;
	backend: DeckBackend;
	go: (section: SectionId) => void;
	/** settings_set with a warning toast on failure; resolves false when it failed. */
	save: (patch: Partial<DeckSettings>) => Promise<boolean>;
}

/** Tauri accelerator from a key event ("Ctrl+Alt+K"), or null while only modifiers are held. */
export function acceleratorFrom(e: KeyboardEvent): string | null {
	if (['Control', 'Alt', 'Shift', 'Meta', 'AltGraph'].includes(e.key)) return null;
	const mods = [e.ctrlKey && 'Ctrl', e.altKey && 'Alt', e.shiftKey && 'Shift', e.metaKey && 'Super'].filter(Boolean);
	const code = e.code;
	const key = /^Key[A-Z]$/.test(code) ? code.slice(3) : /^Digit\d$/.test(code) ? code.slice(5) : code;
	return key ? [...mods, key].join('+') : null;
}

/** Client-side check before asking the backend (which also rejects keys that are already taken). */
export function hotkeyProblem(value: string): 'empty' | 'modifier' | 'key' | null {
	const parts = value.split('+').map((p) => p.trim()).filter(Boolean);
	if (parts.length === 0) return 'empty';
	const isMod = (p: string) => /^(ctrl|control|alt|option|shift|super|meta|cmd|command|commandorcontrol|cmdorctrl)$/i.test(p);
	if (!parts.some(isMod)) return 'modifier';
	if (parts.filter((p) => !isMod(p)).length !== 1) return 'key';
	return null;
}

const KEY = Symbol('kotilo-settings');
export const setSettingsContext = (ctx: SettingsContext) => setContext(KEY, ctx);
export const getSettingsContext = () => getContext<SettingsContext>(KEY);

/** Error → user text. */
export const errText = (e: unknown): string => (e instanceof Error ? e.message : String(e));
