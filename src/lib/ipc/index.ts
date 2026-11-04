/**
 * Typed IPC contract between the deck UI and the Tauri backend (ARCHITECTURE.md "IPC contract").
 * Rust side mirrors these types with `#[serde(rename_all = "camelCase")]`.
 *
 * Transport (`./transport.ts`): Tauri IPC in the webview, HTTP + WebSocket on a LAN tablet/browser
 * (`isRemote()`), nothing in plain `pnpm dev` (`hasBackend()` false → callers fall back to
 * localStorage / "not available" states instead of throwing).
 */
import { hasBackend, invoke, isRemote, isTauri, listen, type UnlistenFn } from './transport';

export { hasBackend, isRemote, isTauri, type UnlistenFn };

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

	// ── Phase 3 (additive; defaults in doc comments) ──
	/** Companion server (protocol.md). Default enabled true, allowWorkspaceSwitch false, notifyApprovals true. */
	companion: { enabled: boolean; allowWorkspaceSwitch: boolean; notifyApprovals: boolean };
	/** One shared timer for all pomodoro widgets. Defaults 25 / 5 / 15 min, 4 per cycle, autoStartNext false, dailyGoal 8. */
	pomodoro: PomodoroSettings;
	/** ICS feeds (SPEC §4.4 Calendar). Default []. */
	calendarFeeds: CalendarFeed[];
	/** Workspace auto-switch rules (SPEC §4.1), evaluated by the deck UI. Default []. */
	autoSwitch: AutoSwitchRule[];

	// ── Phase 4 (additive; defaults in doc comments) ──
	/** Tablet/browser mode (ARCHITECTURE "LAN mode"). Default enabled false, port 47832 (1024–65535, never 47831). */
	lan: { enabled: boolean; port: number };
	/** Auto-update. Default autoCheck true (startup + every 24 h; never installs without a click). */
	updates: { autoCheck: boolean };
}

export interface PomodoroSettings {
	/** Minutes, 1–180. */
	focus: number;
	/** Minutes, 1–60. */
	shortBreak: number;
	/** Minutes, 1–120. */
	longBreak: number;
	/** 1–12. */
	sessionsPerCycle: number;
	autoStartNext: boolean;
	/** Focus sessions per day, 1–24 (daily stat ring). */
	dailyGoal: number;
}

/** Token colour name, never a hex (theme-driven per-feed colour). */
export type FeedColor = 'accent' | 'accent-2' | 'success' | 'warning' | 'danger' | 'info';

export interface CalendarFeed {
	/** Slug `[a-z0-9-]+`, stable. */
	id: string;
	name: string;
	/** https:// or webcal:// (treated as https). Never logged (may carry a secret). */
	url: string;
	color: FeedColor;
	enabled: boolean;
}

export type AutoSwitchTrigger =
	| { type: 'time'; at: string; /** 0=Sun … 6=Sat; empty = every day. */ days: number[] }
	| { type: 'pomodoro-start' }
	| { type: 'pomodoro-end' }
	| { type: 'calendar-start'; /** null = any feed. */ feedId: string | null };

export interface AutoSwitchRule {
	id: string;
	enabled: boolean;
	trigger: AutoSwitchTrigger;
	/** Target workspace id. */
	workspace: string;
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

// ── Pomodoro (backend-owned timer so it keeps running and notifies with the UI hidden) ──

export type PomodoroPhase = 'idle' | 'focus' | 'short-break' | 'long-break';

/** Same shape as protocol.md `pomodoro.state`. */
export interface PomodoroState {
	phase: PomodoroPhase;
	running: boolean;
	/** Unix ms while running, else null. UI derives the countdown from this (no per-second events). */
	endsAt: number | null;
	/** Seconds left; authoritative while paused. */
	remaining: number;
	/** Full length of the phase in seconds. */
	duration: number;
	/** 1-based focus session index in the cycle. */
	session: number;
	sessionsPerCycle: number;
}

export interface PomodoroToday {
	/** Completed focus sessions today (local day). */
	sessions: number;
	focusMinutes: number;
	goal: number;
}

export const pomodoroState = () => invoke<PomodoroState>('pomodoro_state');
/** Starts `phase` (default: focus when idle, else the current phase) from its full length. */
export const pomodoroStart = (phase?: Exclude<PomodoroPhase, 'idle'>, minutes?: number) =>
	invoke<PomodoroState>('pomodoro_start', { phase: phase ?? null, minutes: minutes ?? null });
export const pomodoroPause = () => invoke<PomodoroState>('pomodoro_pause');
export const pomodoroResume = () => invoke<PomodoroState>('pomodoro_resume');
/** Ends the current phase now and moves to the next (a skipped focus is not counted). */
export const pomodoroSkip = () => invoke<PomodoroState>('pomodoro_skip');
/** Back to idle, session 1. */
export const pomodoroReset = () => invoke<PomodoroState>('pomodoro_reset');
export const pomodoroToday = () => invoke<PomodoroToday>('pomodoro_today');

// ── Companion (protocol.md; Deck is the server) ─────────────────────────────

export interface CompanionStatus {
	enabled: boolean;
	/** Server bound to 127.0.0.1:port. false when disabled or the port is taken (`error` says why). */
	listening: boolean;
	connected: boolean;
	/** `clientVersion` from the desktop's hello. */
	client: string | null;
	port: number;
	error: string | null;
}

export type AgentStatus = 'idle' | 'working' | 'waiting';

export interface Agent {
	id: string;
	repo: string;
	name: string;
	status: AgentStatus;
	task: string | null;
	since: number | null;
}

export interface Approval {
	id: string;
	agentId: string;
	summary: string;
	detail: string | null;
	requestedAt: number;
}

export interface Usage {
	tokens: number;
	costUsd: number;
	budgetUsd: number | null;
}

/** Last snapshot from the desktop; cleared (agents [], approvals [], usage null) when it disconnects. */
export interface CompanionSnapshot {
	agents: Agent[];
	approvals: Approval[];
	usage: Usage | null;
	/** Unix ms of the last desktop message, null if none since connect. */
	updatedAt: number | null;
}

export const companionStatus = () => invoke<CompanionStatus>('companion_status');
export const companionSnapshot = () => invoke<CompanionSnapshot>('companion_snapshot');
/** Sends `approval.respond`; rejects if not connected. Optimistically removes the approval locally. */
export const companionRespond = (id: string, decision: 'approve' | 'deny') =>
	invoke<void>('companion_respond', { id, decision });
/** Tries to launch the Kotilo desktop app (`kotilo://` URL scheme). Rejects if nothing handles it. */
export const companionOpenDesktop = () => invoke<void>('companion_open_desktop');

// ── Calendar widget (ICS feeds fetched by the backend) ──────────────────────

export interface CalendarEvent {
	/** Stable per occurrence: `<feedId>:<uid>:<start>`. */
	id: string;
	feedId: string;
	title: string;
	/** Unix ms. All-day events start at local midnight. */
	start: number;
	end: number;
	allDay: boolean;
	location: string | null;
}

export interface CalendarUpcoming {
	/** Sorted by start; events already ended are excluded; recurring events expanded. */
	events: CalendarEvent[];
	/** Unix ms of the oldest successful feed fetch in this result, null if none. */
	fetchedAt: number | null;
	/** True when at least one feed is served from cache after a failed refresh. */
	stale: boolean;
	/** Per-feed errors (message is user-safe: no URL). */
	errors: { feedId: string; message: string }[];
}

/** Next events within `days` (default 7) across enabled feeds, at most `limit` (default 20). Cached 15 min. */
export const calendarUpcoming = (limit?: number, days?: number) =>
	invoke<CalendarUpcoming>('calendar_upcoming', { limit: limit ?? null, days: days ?? null });
/** Forces a refetch of all enabled feeds (Retry button / feed list changed). */
export const calendarRefresh = () => invoke<void>('calendar_refresh');

// ── Phase 4: LAN mode (tablet/browser) ──────────────────────────────────────

export interface LanStatus {
	enabled: boolean;
	listening: boolean;
	port: number;
	/** `http://<ipv4>:<port>/?token=…` per non-loopback IPv4; contains the secret token, never log it. */
	urls: string[];
	/** SVG markup of a QR code for urls[0]; null when not listening. Rendered as an <img> data URL, never as innerHTML. */
	qrSvg: string | null;
	/** Open browser connections. */
	clients: number;
	/** Last bind/serve error, e.g. "port 47832 is in use". */
	error: string | null;
}

export const lanStatus = () => invoke<LanStatus>('lan_status');

// ── Phase 4: widget plugins (docs/plugin-api.md) ────────────────────────────

/** Live data a plugin may ask for; the deck forwards only topics declared in its manifest. */
export type PluginTopic = 'system' | 'media' | 'pomodoro';

/** `~/.kotilo/deck/widgets/<folder>/manifest.json`, validated by the backend. Same fields as a built-in manifest plus: */
export interface PluginManifest {
	/** Always 1 for now. */
	apiVersion: 1;
	/** Equal to the folder name, `^[a-z0-9][a-z0-9-]{0,31}$`. */
	id: string;
	name: string;
	/** Shell icon key; unknown keys fall back to "puzzle". */
	icon: string;
	sizes: SizeKey[];
	defaultSize: SizeKey;
	capabilities: Capability[];
	multiInstance: boolean;
	settings: { type: 'object'; properties: Record<string, unknown> };
	/** Relative path of the HTML entry inside the folder. Default "index.html". */
	entry: string;
	/** Default []. */
	topics: PluginTopic[];
	description?: string;
	author?: string;
	version?: string;
}

export interface PluginWidget {
	/** `ext.<folder>` — the `widget` value in workspace files. Built-in ids never contain a dot. */
	id: string;
	folder: string;
	manifest: PluginManifest | null;
	/** Why the folder was rejected (Settings ▸ Widgets). */
	error: string | null;
	/** iframe src inside the Tauri webview (`http://kotilo-widget.localhost/<folder>/<entry>` on Windows). In LAN mode use `/plugins/<folder>/<entry>`. */
	entryUrl: string | null;
}

export const pluginList = () => invoke<PluginWidget[]>('plugin_list');
/** Opens ~/.kotilo/deck/widgets/ (created if missing) in the file manager. */
export const pluginOpenFolder = () => invoke<void>('plugin_open_folder');

// ── Phase 4: auto-update ────────────────────────────────────────────────────

export type UpdateState = 'idle' | 'checking' | 'available' | 'downloading' | 'ready' | 'error' | 'unsupported';

export interface UpdateInfo {
	version: string;
	/** RFC 3339. */
	date: string | null;
	notes: string | null;
}

export interface UpdateStatus {
	/** Running app version. */
	current: string;
	state: UpdateState;
	available: UpdateInfo | null;
	/** 0–1 while downloading. */
	progress: number | null;
	error: string | null;
	/** Unix ms of the last finished check. */
	lastChecked: number | null;
}

export const updateStatus = () => invoke<UpdateStatus>('update_status');
/** Checks now, also when autoCheck is off. */
export const updateCheck = () => invoke<UpdateStatus>('update_check');
/** Downloads + installs the available update, then relaunches. */
export const updateInstall = () => invoke<void>('update_install');

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
	/** Every pomodoro change (not per tick). */
	'pomodoro://state': PomodoroState;
	/** A phase finished (OS notification already shown); UI may play a cue / trigger auto-switch rules. */
	'pomodoro://phase-end': { phase: Exclude<PomodoroPhase, 'idle'>; next: PomodoroPhase };
	'companion://status': CompanionStatus;
	'companion://snapshot': CompanionSnapshot;
	/** protocol.md `pomodoro.suggest`; UI shows the 60 s banner. */
	'companion://suggest': { reason: string; minutes: number | null };
	/** Calendar cache refreshed (any feed). */
	'calendar://changed': Record<string, never>;
	/** Phase 4 */
	'lan://status': LanStatus;
	/** A folder under ~/.kotilo/deck/widgets/ was added/changed/removed; call pluginList again. */
	'plugin://changed': Record<string, never>;
	'update://status': UpdateStatus;
}

export function on<K extends keyof DeckEvents>(event: K, handler: (payload: DeckEvents[K]) => void): Promise<UnlistenFn> {
	// No backend → a no-op unlisten (the transport decides).
	return listen<DeckEvents[K]>(event, (e) => handler(e.payload));
}
