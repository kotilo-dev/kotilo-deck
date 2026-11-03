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

	// ── Phase 2 (additive; missing keys take these defaults on the backend) ──
	/** Launch with the OS (tauri-plugin-autostart). Default false. */
	autostart: boolean;
	/** "fullscreen" (default) on the chosen monitor, or "overlay" = normal-sized always-on-top window. */
	displayMode: 'fullscreen' | 'overlay';
	/** SPEC §5.3 global appearance; workspace/instance `appearance` override surface opacity/blur. */
	appearance: DeckAppearance;
	/** SPEC §2 burn-in protection + §5.3 dim. */
	burnIn: BurnInSettings;
	/** Light/dark auto switch using the theme's `pair`: off, follow OS, or schedule (lightFrom–darkFrom "HH:MM"). Default off, 07:00, 19:00. */
	themeAuto: { mode: 'off' | 'os' | 'schedule'; lightFrom: string; darkFrom: string };
	/** Global unit; weather instances may override. Default "c". */
	temperatureUnit: 'c' | 'f';
	/** Sound with reminder/pomodoro notifications. Default true. */
	notificationSound: boolean;
}

export interface DeckAppearance {
	/** Default "solid". */
	background: 'solid' | 'wallpaper' | 'transparent';
	/** File name inside ~/.kotilo/deck/wallpapers/ (image or video). Default null. */
	wallpaper: string | null;
	/** 0–1; null = theme `opacity.surface`. */
	surfaceOpacity: number | null;
	/** Backdrop blur px 0–40. Default 0 (off). */
	blur: number;
	/** 0.2–1, overlay mode only. Default 1. */
	windowOpacity: number;
}

export interface BurnInSettings {
	/** Pixel shift interval in minutes; 0 = off. Default 10. Shift is within ±4 px. */
	shiftMinutes: number;
	/** Night dim between from–to ("HH:MM"), target 0.3–1. Default disabled, 22:00–07:00, 0.5. */
	night: { enabled: boolean; from: string; to: string; dim: number };
	/** Idle dim after N minutes without input, optional screensaver clock. Default disabled, 15, 0.4, true. */
	idle: { enabled: boolean; minutes: number; dim: number; clockFace: boolean };
}

export const settingsGet = () => invoke<DeckSettings>('settings_get');
/** Merges `patch` into the stored settings (nested `appearance`, `burnIn`, `themeAuto` deep-merge; `activeWorkspace` is replaced) and returns the result. */
export const settingsSet = (patch: Partial<DeckSettings>) => invoke<DeckSettings>('settings_set', { patch });
/** Shows (creating on first use) the separate 960×680 settings window, label "settings", route `/settings`. */
export const settingsOpen = () => invoke<void>('settings_open');
/** All settings + workspaces as one JSON document. */
export const settingsExportAll = () => invoke<string>('settings_export_all');
/** Replaces settings + workspaces from an export (validated; existing files backed up first). */
export const settingsImportAll = (json: string) => invoke<void>('settings_import_all', { json });

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

// ── Appearance assets & files (Phase 2) ─────────────────────────────────────

export interface Wallpaper {
	/** File name in ~/.kotilo/deck/wallpapers/. */
	name: string;
	kind: 'image' | 'video';
	/** Absolute path; render with `convertFileSrc(path)` (asset protocol scoped to the wallpapers dir). */
	path: string;
}

export const wallpaperList = () => invoke<Wallpaper[]>('wallpaper_list');
/** Native file picker; copies the chosen image/video into the wallpapers dir. null = cancelled. */
export const wallpaperImport = () => invoke<Wallpaper | null>('wallpaper_import');

/** Raw user theme JSONs from ~/.kotilo/themes/*.json (validated by the TS loader). Hot reload: `theme://user-changed`. */
export const themeUserList = () => invoke<unknown[]>('theme_user_list');

/** Native save dialog + write (WebView2 blob downloads are unreliable). false = cancelled. */
export const fileSaveAs = (defaultName: string, contents: string) =>
	invoke<boolean>('file_save_as', { defaultName, contents });
/** Native open dialog; UTF-8 contents or null if cancelled. */
export const fileOpenText = (extensions: string[]) => invoke<string | null>('file_open_text', { extensions });

// ── Volume widget (capability "audio") ──────────────────────────────────────

export interface AudioDevice {
	id: string;
	name: string;
	isDefault: boolean;
}

export interface AudioSession {
	/** Stable per app session. */
	id: string;
	/** App display name, e.g. "Spotify". */
	name: string;
	/** 0–1. */
	volume: number;
	muted: boolean;
}

export interface AudioState {
	/** Default output device; null when there is none. */
	device: AudioDevice | null;
	/** 0–1 master volume of the default output. */
	volume: number;
	muted: boolean;
	devices: AudioDevice[];
}

export const audioState = () => invoke<AudioState>('audio_state');
export const audioSetVolume = (volume: number) => invoke<void>('audio_set_volume', { volume });
export const audioSetMuted = (muted: boolean) => invoke<void>('audio_set_muted', { muted });
export const audioSetDefaultDevice = (id: string) => invoke<void>('audio_set_default_device', { id });
export const audioSessions = () => invoke<AudioSession[]>('audio_sessions');
export const audioSetSessionVolume = (id: string, volume: number) =>
	invoke<void>('audio_set_session_volume', { id, volume });
export const audioSetSessionMuted = (id: string, muted: boolean) =>
	invoke<void>('audio_set_session_muted', { id, muted });
/** Ref-counted (reset on page load); backend pushes `audio://state` from OS change notifications, no polling. */
export const audioSubscribe = () => invoke<void>('audio_subscribe');
export const audioUnsubscribe = () => invoke<void>('audio_unsubscribe');

// ── Media widget (capability "media") ───────────────────────────────────────

export interface MediaSession {
	/** Source app display name, e.g. "Spotify", "Microsoft Edge". */
	source: string;
	title: string;
	artist: string;
	album: string | null;
	/** data: URL (png/jpeg) or null. */
	artwork: string | null;
	playing: boolean;
	/** Seconds; null when the source reports no timeline. */
	position: number | null;
	duration: number | null;
	/** Unix ms when `position` was read, so the UI can extrapolate while playing. */
	positionAt: number;
	shuffle: boolean | null;
	repeat: 'none' | 'track' | 'list' | null;
	canPrev: boolean;
	canNext: boolean;
	canPlayPause: boolean;
}

/** session null = nothing playing. */
export interface MediaState {
	session: MediaSession | null;
}

export type MediaAction = 'play-pause' | 'next' | 'prev' | 'toggle-shuffle' | 'cycle-repeat';

export const mediaState = () => invoke<MediaState>('media_state');
export const mediaControl = (action: MediaAction) => invoke<void>('media_control', { action });
export const mediaSeek = (seconds: number) => invoke<void>('media_seek', { seconds });
/** Ref-counted (reset on page load); backend pushes `media://state` on session/playback/property events. */
export const mediaSubscribe = () => invoke<void>('media_subscribe');
export const mediaUnsubscribe = () => invoke<void>('media_unsubscribe');

// ── Todo widget (SQLite owned by the backend so reminders fire with the UI hidden) ──

export interface Todo {
	id: number;
	text: string;
	done: boolean;
	/** Unix ms or null. */
	due: number | null;
	/** Unix ms the reminder fires (usually = due); null = no reminder. */
	remindAt: number | null;
	/** Ascending order key. */
	position: number;
	createdAt: number;
	completedAt: number | null;
}

export type TodoPatch = Partial<Pick<Todo, 'text' | 'done' | 'due' | 'remindAt'>>;

export const todoList = () => invoke<Todo[]>('todo_list');
export const todoAdd = (text: string, due: number | null, remindAt: number | null) =>
	invoke<Todo>('todo_add', { text, due, remindAt });
export const todoUpdate = (id: number, patch: TodoPatch) => invoke<Todo>('todo_update', { id, patch });
export const todoDelete = (id: number) => invoke<void>('todo_delete', { id });
/** Full new order of ids. */
export const todoReorder = (ids: number[]) => invoke<void>('todo_reorder', { ids });
/** Sets remindAt to now + minutes. */
export const todoSnooze = (id: number, minutes: number) => invoke<Todo>('todo_snooze', { id, minutes });

// ── Events (backend → UI) ───────────────────────────────────────────────────

export interface DeckEvents {
	'system://sample': SystemSample;
	/** Global Ctrl+Alt+←/→. */
	'workspace://cycle': { dir: -1 | 1 };
	/** Tray "Workspaces ▸ <name>". */
	'workspace://switch': { id: string };
	/** Deck moved to another monitor (select, reconnect); grid re-derives density if needed. */
	'monitor://changed': MonitorInfo;
	/** To every window after any successful settings_set (deck ↔ settings window sync). */
	'settings://changed': DeckSettings;
	/** Deck window shown/hidden (tray, hotkey, close-to-tray). Don't rely on document.hidden in WebView2. */
	'deck://visibility': { visible: boolean };
	'audio://state': AudioState;
	'media://state': MediaState;
	/** Any todo row changed (any window or the reminder scheduler). */
	'todo://changed': Record<string, never>;
	/** Reminder fired (OS notification already shown by the backend); the widget pulses the row. */
	'todo://reminder': { id: number };
	/** A file in ~/.kotilo/themes/ was added/changed/removed; call themeUserList again. */
	'theme://user-changed': Record<string, never>;
	/** Workspace files changed from another window (settings import, rename) → reload. */
	'workspace://changed': Record<string, never>;
	/** Tray Quit: windows get ~500 ms to flush pending saves before the process exits. */
	'deck://quitting': Record<string, never>;
}

export function on<K extends keyof DeckEvents>(event: K, handler: (payload: DeckEvents[K]) => void): Promise<UnlistenFn> {
	if (!isTauri()) return Promise.resolve(() => {});
	return listen<DeckEvents[K]>(event, (e) => handler(e.payload));
}
