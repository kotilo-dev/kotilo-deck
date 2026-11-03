/**
 * Typed IPC contract between the deck UI and the Tauri backend (ARCHITECTURE.md "IPC contract").
 * Rust side mirrors these types with `#[serde(rename_all = "camelCase")]`.
 *
 * Outside Tauri (plain `pnpm dev` in a browser) `isTauri()` is false; callers fall back
 * (workspaces → localStorage, system/weather → "not available" states) instead of throwing.
 */
import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';

export const isTauri = (): boolean => typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

// ── Platform ────────────────────────────────────────────────────────────────

export type Capability =
	| 'monitors'
	| 'system-info'
	| 'gpu'
	| 'audio'
	| 'media'
	| 'notifications'
	| 'transparency'
	| 'companion';

export const platformCapabilities = () => invoke<Capability[]>('platform_capabilities');

// ── Settings (tauri-plugin-store "settings.json", owned by the backend) ─────

export interface DeckSettings {
	schemaVersion: 1;
	/** Theme id; null = DEFAULT_THEME_ID. */
	theme: string | null;
	/** Monitor name the deck is pinned to; null = first non-primary, else primary. */
	monitor: string | null;
	/** Last active workspace id per monitor name. */
	activeWorkspace: Record<string, string>;
	alwaysOnTop: boolean;
	/** Global shortcut, Tauri accelerator syntax. */
	hotkey: string;
}

export const settingsGet = () => invoke<DeckSettings>('settings_get');
/** Shallow-merges `patch` into the stored settings and returns the result. */
export const settingsSet = (patch: Partial<DeckSettings>) => invoke<DeckSettings>('settings_set', { patch });

// ── Workspaces (~/.kotilo/deck/workspaces/<id>.json, SPEC §4.1) ─────────────

export type SizeKey = `${number}x${number}`;

export interface Appearance {
	/** 0–1 surface opacity override. */
	opacity?: number;
	/** Backdrop blur px (Phase 2). */
	blur?: number;
	/** Background spec, e.g. "solid" | "wallpaper:<file>" | "transparent" (Phase 2). */
	background?: string;
}

export interface WidgetInstance {
	instance: string;
	widget: string;
	x: number;
	y: number;
	w: number;
	h: number;
	settings?: Record<string, unknown> & { appearance?: Appearance };
}

export interface Workspace {
	schemaVersion: 1;
	/** Slug `[a-z0-9-]+`, also the file name. */
	id: string;
	name: string;
	theme: string | null;
	grid: { cols: number; rows: number };
	appearance?: Appearance;
	widgets: WidgetInstance[];
}

/** All workspaces on disk, already migrated to the current schemaVersion. Empty on first run. */
export const workspaceList = () => invoke<Workspace[]>('workspace_list');
/** Atomic write of `<id>.json`. */
export const workspaceSave = (workspace: Workspace) => invoke<void>('workspace_save', { workspace });
export const workspaceDelete = (id: string) => invoke<void>('workspace_delete', { id });

// ── Window / monitors / tray ────────────────────────────────────────────────

export interface MonitorInfo {
	name: string;
	width: number;
	height: number;
	scaleFactor: number;
	primary: boolean;
}

export const monitorList = () => invoke<MonitorInfo[]>('monitor_list');
/** Moves the deck fullscreen onto `name` and persists it in settings. */
export const monitorSelect = (name: string) => invoke<void>('monitor_select', { name });
/** Tells the tray which workspaces exist so its "Workspaces" submenu stays in sync. */
export const traySetWorkspaces = (items: { id: string; name: string }[], active: string) =>
	invoke<void>('tray_set_workspaces', { items, active });

// ── System widget ───────────────────────────────────────────────────────────

export interface SystemSample {
	/** Unix ms. */
	t: number;
	/** 0–100. */
	cpu: number;
	perCore: number[];
	memUsed: number;
	memTotal: number;
	/** 0–100, null when the platform has no `gpu` capability or the read failed. */
	gpu: number | null;
}

export interface SystemInfo {
	cpuName: string;
	cores: number;
	gpuName: string | null;
	memTotal: number;
}

export interface ProcessInfo {
	name: string;
	cpu: number;
	memory: number;
}

export const systemInfo = () => invoke<SystemInfo>('system_info');
/**
 * Ref-counted: the backend samples (1 Hz, emitting `system://sample`) only while
 * at least one subscriber exists. Pair every subscribe with an unsubscribe.
 */
export const systemSubscribe = () => invoke<void>('system_subscribe');
export const systemUnsubscribe = () => invoke<void>('system_unsubscribe');
export const systemTopProcesses = () => invoke<ProcessInfo[]>('system_top_processes');

// ── Weather widget (Open-Meteo via backend; no API key) ─────────────────────

export interface Place {
	name: string;
	country: string;
	admin1: string | null;
	latitude: number;
	longitude: number;
	timezone: string;
}

export interface WeatherReport {
	/** Unix ms of the successful upstream fetch this data came from. */
	fetchedAt: number;
	/** True when upstream failed and this is the last cached report. */
	stale: boolean;
	unit: 'c' | 'f';
	current: { temp: number; code: number; isDay: boolean; high: number; low: number };
	/** Next 6 hours. */
	hourly: { time: string; temp: number; code: number }[];
	/** Today + next 2 days. */
	daily: { date: string; max: number; min: number; code: number }[];
}

export const weatherGeocode = (query: string) => invoke<Place[]>('weather_geocode', { query });
/** Cached 15 min (memory + disk). Rejects only when upstream fails and there is no cache. */
export const weatherGet = (latitude: number, longitude: number, unit: 'c' | 'f') =>
	invoke<WeatherReport>('weather_get', { latitude, longitude, unit });

// ── Events (backend → UI) ───────────────────────────────────────────────────

export interface DeckEvents {
	'system://sample': SystemSample;
	/** Global Ctrl+Alt+←/→. */
	'workspace://cycle': { dir: -1 | 1 };
	/** Tray "Workspaces ▸ <name>". */
	'workspace://switch': { id: string };
	/** Deck moved to another monitor (select, reconnect); grid re-derives density if needed. */
	'monitor://changed': MonitorInfo;
}

export function on<K extends keyof DeckEvents>(event: K, handler: (payload: DeckEvents[K]) => void): Promise<UnlistenFn> {
	if (!isTauri()) return Promise.resolve(() => {});
	return listen<DeckEvents[K]>(event, (e) => handler(e.payload));
}
