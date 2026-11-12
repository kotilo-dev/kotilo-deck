/**
 * In-memory demo backend (`?demo`, never inside Tauri; see `./transport.ts`). Answers every command the deck
 * and the widgets use with realistic, deterministic, made-up data relative to `now()` (README screenshots
 * pin the clock with Playwright), and pushes the same events the Tauri backend would. Writes only touch
 * memory: a reload starts over.
 *
 * URL parameters: `theme=<id>` (global theme), `workspace=<id>` (active workspace).
 * Scripting hook for `scripts/screenshots.mjs`: `window.__kotiloDemo.setTheme(id)` / `.emit(event, payload)`.
 */
import type {
	Agent,
	Approval,
	AudioState,
	AudioSession,
	CalendarEvent,
	CalendarFeed,
	Capability,
	CompanionSnapshot,
	CompanionStatus,
	DeckSettings,
	LanStatus,
	MediaSession,
	MediaState,
	MonitorInfo,
	PomodoroPhase,
	PomodoroState,
	PomodoroToday,
	ProcessInfo,
	SystemInfo,
	SystemSample,
	Todo,
	TodoPatch,
	UpdateStatus,
	Wallpaper,
	WeatherReport,
	Workspace
} from './index';
import type { DemoBackend, EventHandler, UnlistenFn } from './transport';
import { DEFAULT_SETTINGS, makePreset, PRESET_IDS } from '#lib/workspace/index.ts';

export interface DemoOptions {
	params?: URLSearchParams;
	/** Clock (tests); defaults to Date.now, which Playwright may pin. */
	now?: () => number;
	/** Timers (tests); default the global ones. */
	setInterval?: (fn: () => void, ms: number) => unknown;
	clearInterval?: (id: unknown) => void;
	/** Expose `window.__kotiloDemo` (default true in a browser). */
	expose?: boolean;
}

export const DEMO_MONITOR = 'DEMO-1';
const DEMO_WALLPAPER: Wallpaper = { name: 'ember.svg', kind: 'image', path: '' };
const MIN = 60_000;
const HOUR = 60 * MIN;
const GIB = 1024 ** 3;

const ISTANBUL = { name: 'İstanbul', country: 'Türkiye', admin1: 'İstanbul', latitude: 41.01, longitude: 28.95 };

const clone = <T>(v: T): T => (v === undefined ? v : (JSON.parse(JSON.stringify(v)) as T));
const pad = (n: number) => String(n).padStart(2, '0');
/** Local "YYYY-MM-DD" / "YYYY-MM-DDTHH:00" like Open-Meteo returns for the location's zone. */
const localDate = (d: Date) => `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`;
const localHour = (d: Date) => `${localDate(d)}T${pad(d.getHours())}:00`;

/** Album art: a soft diagonal gradient with two circles (SVG data URL, no real artwork). */
function artwork(a: string, b: string): string {
	const svg =
		`<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 120 120">` +
		`<defs><linearGradient id="g" x1="0" y1="0" x2="1" y2="1">` +
		`<stop offset="0" stop-color="${a}"/><stop offset="1" stop-color="${b}"/></linearGradient></defs>` +
		`<rect width="120" height="120" fill="url(#g)"/>` +
		`<circle cx="84" cy="38" r="22" fill="rgb(255 255 255 / 0.22)"/>` +
		`<circle cx="40" cy="86" r="34" fill="rgb(255 255 255 / 0.12)"/></svg>`;
	return `data:image/svg+xml;charset=utf-8,${encodeURIComponent(svg)}`;
}

const TRACKS = [
	{ title: 'Low Tide Signals', artist: 'Mira Vale', album: 'Harbor Lights', duration: 241, art: ['rgb(244 114 182)', 'rgb(99 102 241)'] },
	{ title: 'Paper Satellites', artist: 'The Quiet Ninth', album: 'Night Shift', duration: 198, art: ['rgb(52 211 153)', 'rgb(14 116 144)'] },
	{ title: 'Amber Loop', artist: 'Ostrander', album: 'Slow Machines', duration: 263, art: ['rgb(251 191 36)', 'rgb(220 38 38)'] }
] as const;

const FEEDS: CalendarFeed[] = [
	{ id: 'work', name: 'Work', url: 'https://calendar.example.com/work.ics', color: 'accent', enabled: true },
	{ id: 'personal', name: 'Personal', url: 'https://calendar.example.com/personal.ics', color: 'success', enabled: true },
	{ id: 'design', name: 'Design', url: 'https://calendar.example.com/design.ics', color: 'accent-2', enabled: true }
];

/** One CPU reading of a gentle, deterministic wave at step `k`. */
export function demoSample(k: number, t: number): SystemSample {
	const cpu = 26 + 9 * Math.sin(k / 6) + 4 * Math.sin(k / 2.3 + 1);
	const perCore = Array.from({ length: 8 }, (_, i) =>
		Math.max(2, Math.min(98, cpu + 14 * Math.sin(k / (3 + i) + i * 1.7) - (i % 3) * 4))
	);
	return {
		t,
		cpu: Math.round(cpu * 10) / 10,
		perCore: perCore.map((v) => Math.round(v * 10) / 10),
		memUsed: Math.round((13.6 + 0.5 * Math.sin(k / 20)) * GIB),
		memTotal: 32 * GIB,
		gpu: Math.round(18 + 7 * Math.sin(k / 9 + 2))
	};
}

export function createDemoBackend(opts: DemoOptions = {}): DemoBackend & {
	emit: (event: string, payload: unknown) => void;
	dispose: () => void;
} {
	const params = opts.params ?? new URLSearchParams();
	const now = opts.now ?? (() => Date.now());
	const setIntervalFn = opts.setInterval ?? ((fn, ms) => setInterval(fn, ms));
	const clearIntervalFn = opts.clearInterval ?? ((id) => clearInterval(id as ReturnType<typeof setInterval>));
	const t0 = now();

	// ── events ──
	const handlers = new Map<string, Set<EventHandler<unknown>>>();
	function emit(event: string, payload: unknown) {
		for (const h of [...(handlers.get(event) ?? [])]) {
			try {
				h({ event, payload: clone(payload) });
			} catch (err) {
				console.error('kotilo-deck demo: event handler failed', event, err);
			}
		}
	}

	// ── settings + workspaces ──
	const workspaceParam = params.get('workspace');
	// `?look=glass`: the glass look over the bundled "Ember" gradient (static/wallpapers/ember.svg, made for
	// this repo, safe to ship in screenshots).
	const glass = params.get('look') === 'glass';
	let settings: DeckSettings = {
		...clone(DEFAULT_SETTINGS),
		theme: params.get('theme') || null,
		monitor: DEMO_MONITOR,
		activeWorkspace: workspaceParam ? { [DEMO_MONITOR]: workspaceParam } : {},
		// Stills must not drift: no pixel shift.
		burnIn: { ...clone(DEFAULT_SETTINGS.burnIn), shiftMinutes: 0 },
		calendarFeeds: clone(FEEDS),
		onboarding: { completed: true, version: 1 },
		...(glass
			? { appearance: { ...clone(DEFAULT_SETTINGS.appearance), background: 'wallpaper', wallpaper: DEMO_WALLPAPER.name, style: 'glass', blur: 32 } }
			: {})
	};
	const workspaces = new Map<string, Workspace>();
	for (const id of PRESET_IDS) {
		const ws = makePreset(id);
		for (const w of ws.widgets) if (w.widget === 'weather') w.settings = { ...w.settings, location: { ...ISTANBUL } };
		workspaces.set(ws.id, ws);
	}

	const merge = (patch: Partial<DeckSettings>): DeckSettings => {
		const p = clone(patch) as Record<string, unknown>;
		const s = settings as unknown as Record<string, unknown>;
		const next: Record<string, unknown> = { ...s };
		for (const [k, v] of Object.entries(p)) {
			const deep = ['appearance', 'burnIn', 'themeAuto', 'companion', 'pomodoro', 'lan', 'updates', 'onboarding'];
			next[k] = deep.includes(k) && v && typeof v === 'object' ? { ...(s[k] as object), ...(v as object) } : v;
		}
		settings = next as unknown as DeckSettings;
		emit('settings://changed', settings);
		return clone(settings);
	};

	// ── system ──
	const info: SystemInfo = { cpuName: 'Demo 8-Core Processor', cores: 8, gpuName: 'Demo Graphics 16 GB', memTotal: 32 * GIB };
	let k = 0;
	let systemSubs = 0;
	let sampler: unknown = null;
	const nextSample = () => emit('system://sample', demoSample(k++, now()));
	function systemSubscribe() {
		systemSubs++;
		// A minute of history right away so the sparkline is not empty on the first frame.
		setTimeout(() => {
			for (let i = 0; i < 60; i++) nextSample();
		}, 0);
		sampler ??= setIntervalFn(nextSample, 1000);
	}
	function systemUnsubscribe() {
		systemSubs = Math.max(0, systemSubs - 1);
		if (systemSubs === 0 && sampler !== null) {
			clearIntervalFn(sampler);
			sampler = null;
		}
	}
	const processes: ProcessInfo[] = [
		{ name: 'code', cpu: 7.4, memory: 1.9 * GIB },
		{ name: 'node', cpu: 5.1, memory: 0.8 * GIB },
		{ name: 'rust-analyzer', cpu: 3.6, memory: 2.4 * GIB },
		{ name: 'browser', cpu: 2.8, memory: 3.1 * GIB },
		{ name: 'kotilo-deck', cpu: 0.9, memory: 0.2 * GIB },
		{ name: 'terminal', cpu: 0.4, memory: 0.1 * GIB }
	];

	// ── weather ──
	function weather(unit: 'c' | 'f'): WeatherReport {
		const conv = (c: number) => (unit === 'f' ? Math.round(c * 1.8 + 32) : c);
		const at = new Date(now());
		const hourly = [17, 18, 18, 17, 15, 14].map((temp, i) => {
			const d = new Date(at);
			d.setMinutes(0, 0, 0);
			d.setHours(d.getHours() + i + 1);
			return { time: localHour(d), temp: conv(temp), code: [2, 2, 3, 3, 61, 3][i] };
		});
		const daily = [
			[19, 12, 2],
			[16, 11, 61],
			[18, 10, 1],
			[20, 12, 0]
		].map(([max, min, code], i) => {
			const d = new Date(at);
			d.setDate(d.getDate() + i);
			return { date: localDate(d), max: conv(max), min: conv(min), code };
		});
		return {
			fetchedAt: now() - 6 * MIN,
			stale: false,
			unit,
			current: { temp: conv(17), code: 2, isDay: true, high: conv(19), low: conv(12) },
			hourly,
			daily
		};
	}

	// ── audio ──
	const audio: AudioState = {
		device: { id: 'speakers', name: 'Speakers (USB Audio)', isDefault: true },
		volume: 0.62,
		muted: false,
		devices: [
			{ id: 'speakers', name: 'Speakers (USB Audio)', isDefault: true },
			{ id: 'headphones', name: 'Headphones', isDefault: false },
			{ id: 'display', name: 'Display Audio', isDefault: false }
		]
	};
	const sessions: AudioSession[] = [
		{ id: 'music', name: 'Music Player', volume: 0.8, muted: false },
		{ id: 'browser', name: 'Web Browser', volume: 0.55, muted: false },
		{ id: 'chat', name: 'Voice Chat', volume: 0.7, muted: true }
	];
	const audioChanged = () => emit('audio://state', audio);

	// ── media ──
	let track = 0;
	let media: MediaSession = mediaFor(0, 83, true);
	function mediaFor(i: number, position: number, playing: boolean): MediaSession {
		const tr = TRACKS[i % TRACKS.length];
		return {
			source: 'Music Player',
			title: tr.title,
			artist: tr.artist,
			album: tr.album,
			artwork: artwork(tr.art[0], tr.art[1]),
			playing,
			position,
			duration: tr.duration,
			positionAt: now(),
			shuffle: false,
			repeat: 'list',
			canPrev: true,
			canNext: true,
			canPlayPause: true
		};
	}
	const mediaPosition = () =>
		media.playing && media.position !== null ? media.position + (now() - media.positionAt) / 1000 : (media.position ?? 0);
	const mediaState = (): MediaState => ({ session: media });
	const mediaChanged = () => emit('media://state', mediaState());

	// ── todos ──
	const today = (h: number, m = 0) => {
		const d = new Date(now());
		d.setHours(h, m, 0, 0);
		return d.getTime();
	};
	let todos: Todo[] = [
		{ text: 'Ship the v1.0 release notes', due: today(16), remindAt: today(16), done: false },
		{ text: 'Review the plugin API draft', due: null, remindAt: null, done: false },
		{ text: 'Answer design feedback on the tablet layout', due: null, remindAt: null, done: false },
		{ text: 'Refresh README screenshots', due: today(11, 30), remindAt: null, done: true },
		{ text: 'Book the team offsite room', due: today(24 + 10), remindAt: null, done: false }
	].map((x, i) => ({
		id: i + 1,
		...x,
		position: i,
		createdAt: t0 - (5 - i) * HOUR,
		completedAt: x.done ? t0 - 40 * MIN : null
	}));
	let nextTodoId = todos.length + 1;
	const todoChanged = () => emit('todo://changed', {});
	const findTodo = (id: unknown) => {
		const t = todos.find((x) => x.id === id);
		if (!t) throw `todo ${String(id)} not found`;
		return t;
	};

	// ── pomodoro ──
	const pomoLen = (phase: Exclude<PomodoroPhase, 'idle'>) =>
		60 * (phase === 'focus' ? settings.pomodoro.focus : phase === 'short-break' ? settings.pomodoro.shortBreak : settings.pomodoro.longBreak);
	let pomodoro: PomodoroState = {
		phase: 'focus',
		running: true,
		endsAt: t0 + (14 * 60 + 32) * 1000,
		remaining: 14 * 60 + 32,
		duration: 25 * 60,
		session: 2,
		sessionsPerCycle: 4
	};
	const pomodoroToday: PomodoroToday = { sessions: 3, focusMinutes: 75, goal: 8 };
	const pomoSet = (next: Partial<PomodoroState>): PomodoroState => {
		pomodoro = { ...pomodoro, ...next };
		emit('pomodoro://state', pomodoro);
		return clone(pomodoro);
	};
	const pomoRemaining = () =>
		pomodoro.running && pomodoro.endsAt !== null ? Math.max(0, Math.round((pomodoro.endsAt - now()) / 1000)) : pomodoro.remaining;
	function pomoStart(phase: Exclude<PomodoroPhase, 'idle'>, minutes?: number | null): PomodoroState {
		const duration = minutes ? minutes * 60 : pomoLen(phase);
		return pomoSet({ phase, running: true, duration, remaining: duration, endsAt: now() + duration * 1000 });
	}

	// ── companion ──
	const companionStatus: CompanionStatus = {
		enabled: true,
		listening: true,
		connected: true,
		client: '1.0.0',
		port: 47831,
		error: null
	};
	const agents: Agent[] = [
		{ id: 'agent-1', repo: 'kotilo-deck', name: 'Builder', status: 'working', task: 'Writing the release checklist', since: t0 - 12 * MIN },
		{ id: 'agent-2', repo: 'harbor-api', name: 'Reviewer', status: 'waiting', task: 'Reviewing the auth refactor', since: t0 - 3 * MIN }
	];
	let approvals: Approval[] = [
		{
			id: 'approval-1',
			agentId: 'agent-2',
			summary: 'Run the test suite in harbor-api',
			detail: 'pnpm test --run',
			requestedAt: t0 - 1 * MIN
		}
	];
	const snapshot = (): CompanionSnapshot => ({
		agents,
		approvals,
		usage: { tokens: 184_320, costUsd: 2.37, budgetUsd: 10 },
		updatedAt: now() - 4000
	});

	// ── calendar ──
	function events(): CalendarEvent[] {
		// Anchored on the next half hour, so "today" always has upcoming events whatever the clock says.
		const half = 30 * MIN;
		const base = Math.ceil(now() / half) * half;
		const midnight = new Date(now());
		midnight.setHours(0, 0, 0, 0);
		const list: Omit<CalendarEvent, 'id'>[] = [
			{ feedId: 'work', title: 'Release week', start: midnight.getTime(), end: midnight.getTime() + 24 * HOUR, allDay: true, location: null },
			{ feedId: 'work', title: 'Team stand-up', start: base, end: base + 15 * MIN, allDay: false, location: 'Video call' },
			{ feedId: 'design', title: 'Design review: tablet mode', start: base + 2.5 * HOUR, end: base + 3.5 * HOUR, allDay: false, location: 'Room 3' },
			{ feedId: 'work', title: 'Release planning', start: base + 5 * HOUR, end: base + 6 * HOUR, allDay: false, location: null },
			{ feedId: 'personal', title: 'Climbing gym', start: base + 7.5 * HOUR, end: base + 9 * HOUR, allDay: false, location: null },
			{ feedId: 'personal', title: 'Lunch with Ada', start: base + 26 * HOUR, end: base + 27 * HOUR, allDay: false, location: 'Karaköy' },
			{ feedId: 'work', title: 'Sprint retro', start: base + 29 * HOUR, end: base + 30 * HOUR, allDay: false, location: null }
		];
		return list
			.filter((e) => e.end > now())
			.sort((a, b) => a.start - b.start)
			.map((e, i) => ({ ...e, id: `${e.feedId}:demo-${i}:${e.start}` }));
	}

	// ── update ──
	let update: UpdateStatus = {
		current: '1.0.0',
		state: 'idle',
		available: null,
		progress: null,
		error: null,
		errorCode: null,
		lastChecked: t0 - 2 * HOUR
	};

	const lan: LanStatus = {
		enabled: false,
		listening: false,
		port: 47832,
		urls: [],
		qrSvg: null,
		clients: 0,
		error: null,
		errorCode: null
	};
	const monitors: MonitorInfo[] = [
		{ name: DEMO_MONITOR, width: 1920, height: 1080, scaleFactor: 1, primary: false },
		{ name: 'DEMO-MAIN', width: 2560, height: 1440, scaleFactor: 1.25, primary: true }
	];
	const capabilities: Capability[] = ['monitors', 'system-info', 'gpu', 'audio', 'media', 'notifications', 'companion'];

	type Args = Record<string, unknown>;
	const notInDemo = (cmd: string) => () => {
		throw `${cmd} is not available in the demo`;
	};
	const commands: Record<string, (a: Args) => unknown> = {
		platform_capabilities: () => capabilities,
		settings_get: () => settings,
		settings_set: (a) => merge((a.patch ?? {}) as Partial<DeckSettings>),
		settings_open: () => {
			if (typeof window !== 'undefined') window.open('/settings?demo', 'kotilo-deck-settings', 'width=960,height=680');
		},
		settings_export_all: () => JSON.stringify({ settings, workspaces: [...workspaces.values()] }, null, 2),
		settings_import_all: notInDemo('Import'),
		workspace_list: () => [...workspaces.values()],
		workspace_save: (a) => {
			const ws = a.workspace as Workspace;
			workspaces.set(ws.id, clone(ws));
		},
		workspace_delete: (a) => void workspaces.delete(String(a.id)),
		monitor_list: () => monitors,
		monitor_select: (a) => void merge({ monitor: String(a.name) }),
		tray_set_workspaces: () => undefined,
		system_info: () => info,
		system_subscribe: () => systemSubscribe(),
		system_unsubscribe: () => systemUnsubscribe(),
		system_top_processes: () => processes,
		weather_geocode: () => [
			{ ...ISTANBUL, timezone: 'Europe/Istanbul' },
			{ name: 'Ankara', country: 'Türkiye', admin1: 'Ankara', latitude: 39.93, longitude: 32.86, timezone: 'Europe/Istanbul' }
		],
		weather_get: (a) => weather(a.unit === 'f' ? 'f' : 'c'),
		wallpaper_list: () => (glass ? [clone(DEMO_WALLPAPER)] : []),
		wallpaper_import: () => null,
		theme_user_list: () => [],
		file_save_as: notInDemo('Saving files'),
		file_open_text: notInDemo('Opening files'),
		audio_state: () => audio,
		audio_set_volume: (a) => {
			audio.volume = Math.max(0, Math.min(1, Number(a.volume)));
			audioChanged();
		},
		audio_set_muted: (a) => {
			audio.muted = !!a.muted;
			audioChanged();
		},
		audio_set_default_device: (a) => {
			for (const d of audio.devices) d.isDefault = d.id === a.id;
			audio.device = audio.devices.find((d) => d.isDefault) ?? null;
			audioChanged();
		},
		audio_sessions: () => sessions,
		audio_set_session_volume: (a) => {
			const s = sessions.find((x) => x.id === a.id);
			if (s) s.volume = Math.max(0, Math.min(1, Number(a.volume)));
		},
		audio_set_session_muted: (a) => {
			const s = sessions.find((x) => x.id === a.id);
			if (s) s.muted = !!a.muted;
		},
		audio_subscribe: () => undefined,
		audio_unsubscribe: () => undefined,
		media_state: () => mediaState(),
		media_control: (a) => {
			switch (a.action) {
				case 'play-pause':
					media = { ...media, position: mediaPosition(), positionAt: now(), playing: !media.playing };
					break;
				case 'next':
				case 'prev':
					track = (track + (a.action === 'next' ? 1 : TRACKS.length - 1)) % TRACKS.length;
					media = { ...mediaFor(track, 0, true), shuffle: media.shuffle, repeat: media.repeat };
					break;
				case 'toggle-shuffle':
					media = { ...media, shuffle: !media.shuffle };
					break;
				case 'cycle-repeat':
					media = { ...media, repeat: media.repeat === 'none' ? 'list' : media.repeat === 'list' ? 'track' : 'none' };
					break;
			}
			mediaChanged();
		},
		media_seek: (a) => {
			media = { ...media, position: Number(a.seconds), positionAt: now() };
			mediaChanged();
		},
		media_subscribe: () => undefined,
		media_unsubscribe: () => undefined,
		todo_list: () => [...todos].sort((a, b) => a.position - b.position),
		todo_add: (a) => {
			const todo: Todo = {
				id: nextTodoId++,
				text: String(a.text ?? ''),
				done: false,
				due: (a.due as number | null) ?? null,
				remindAt: (a.remindAt as number | null) ?? null,
				position: Math.max(-1, ...todos.map((t) => t.position)) + 1,
				createdAt: now(),
				completedAt: null
			};
			todos.push(todo);
			todoChanged();
			return todo;
		},
		todo_update: (a) => {
			const t = findTodo(a.id);
			const patch = (a.patch ?? {}) as TodoPatch;
			Object.assign(t, patch);
			if ('done' in patch) t.completedAt = patch.done ? now() : null;
			todoChanged();
			return t;
		},
		todo_delete: (a) => {
			todos = todos.filter((t) => t.id !== a.id);
			todoChanged();
		},
		todo_reorder: (a) => {
			(a.ids as number[]).forEach((id, i) => {
				const t = todos.find((x) => x.id === id);
				if (t) t.position = i;
			});
			todoChanged();
		},
		todo_snooze: (a) => {
			const t = findTodo(a.id);
			t.remindAt = now() + Number(a.minutes) * MIN;
			todoChanged();
			return t;
		},
		pomodoro_state: () => ({ ...pomodoro, remaining: pomoRemaining() }),
		pomodoro_start: (a) =>
			pomoStart(
				(a.phase as Exclude<PomodoroPhase, 'idle'> | null) ?? (pomodoro.phase === 'idle' ? 'focus' : pomodoro.phase),
				a.minutes as number | null
			),
		pomodoro_pause: () => pomoSet({ running: false, remaining: pomoRemaining(), endsAt: null }),
		pomodoro_resume: () => pomoSet({ running: true, endsAt: now() + pomodoro.remaining * 1000 }),
		pomodoro_skip: () => {
			const next = pomodoro.phase === 'focus' ? (pomodoro.session >= pomodoro.sessionsPerCycle ? 'long-break' : 'short-break') : 'focus';
			const session = next === 'focus' ? (pomodoro.session % pomodoro.sessionsPerCycle) + 1 : pomodoro.session;
			const duration = pomoLen(next);
			return pomoSet({ phase: next, session, running: false, duration, remaining: duration, endsAt: null });
		},
		pomodoro_reset: () =>
			pomoSet({ phase: 'idle', running: false, session: 1, endsAt: null, remaining: pomoLen('focus'), duration: pomoLen('focus') }),
		pomodoro_today: () => pomodoroToday,
		companion_status: () => companionStatus,
		companion_snapshot: () => snapshot(),
		companion_respond: (a) => {
			const ap = approvals.find((x) => x.id === a.id);
			if (!ap) throw 'unknown approval';
			approvals = approvals.filter((x) => x.id !== a.id);
			const agent = agents.find((x) => x.id === ap.agentId);
			if (agent && a.decision === 'approve') agent.status = 'working';
			emit('companion://snapshot', snapshot());
		},
		companion_open_desktop: notInDemo('Opening Kotilo'),
		calendar_upcoming: (a) => {
			const limit = typeof a.limit === 'number' ? a.limit : 20;
			const days = typeof a.days === 'number' ? a.days : 7;
			return {
				events: events()
					.filter((e) => e.start < now() + days * 24 * HOUR)
					.slice(0, limit),
				fetchedAt: now() - 8 * MIN,
				stale: false,
				errors: []
			};
		},
		calendar_refresh: () => void emit('calendar://changed', {}),
		lan_status: () => lan,
		plugin_list: () => [],
		plugin_open_folder: notInDemo('Opening folders'),
		update_status: () => update,
		update_check: () => {
			update = { ...update, lastChecked: now() };
			emit('update://status', update);
			return update;
		},
		update_install: notInDemo('Installing updates'),
		crash_last: () => null,
		crash_dismiss: () => undefined,
		log_frontend_error: () => undefined,
		report_issue_url: () => 'https://github.com/kotilo-dev/kotilo-deck/issues/new',
		open_report_issue: () => undefined,
		open_logs_folder: notInDemo('Opening folders')
	};

	async function invoke<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
		const fn = commands[cmd];
		if (!fn) throw `demo: unknown command ${cmd}`;
		// Results are copies, like data that crossed IPC.
		return clone(fn(args ?? {})) as T;
	}

	async function listen<T>(event: string, handler: EventHandler<T>): Promise<UnlistenFn> {
		let set = handlers.get(event);
		if (!set) handlers.set(event, (set = new Set()));
		const h = handler as EventHandler<unknown>;
		set.add(h);
		return () => {
			handlers.get(event)?.delete(h);
		};
	}

	function dispose() {
		if (sampler !== null) clearIntervalFn(sampler);
		sampler = null;
		handlers.clear();
	}

	const backend = { invoke, listen, emit, dispose };
	if ((opts.expose ?? true) && typeof window !== 'undefined') {
		(window as unknown as Record<string, unknown>).__kotiloDemo = {
			setTheme: (id: string | null) => merge({ theme: id }),
			emit
		};
	}
	return backend;
}
