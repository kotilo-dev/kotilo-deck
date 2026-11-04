/**
 * Pomodoro data access. In the app the backend owns the one timer (it keeps running and notifies with the
 * UI hidden; every instance mirrors `pomodoro://state`). In the browser preview a shared in-memory
 * `LocalPomodoro` stands in, so the widget is usable with `pnpm dev`.
 */
import {
	hasBackend,
	on,
	pomodoroPause,
	pomodoroReset,
	pomodoroResume,
	pomodoroSkip,
	pomodoroStart,
	pomodoroState,
	pomodoroToday,
	type PomodoroSettings,
	type PomodoroState,
	type PomodoroToday
} from '#lib/ipc/index.ts';
import { normalizeSettings } from '#lib/workspace/index.ts';
import { DEFAULT_POMODORO, LocalPomodoro, type ActivePhase, type PhaseEnd } from './logic';

export interface Suggestion {
	reason: string;
	minutes: number | null;
}

export interface PomodoroApi {
	/** False in the browser preview (timer lives in this page only). */
	readonly persistent: boolean;
	state(): Promise<PomodoroState>;
	start(phase?: ActivePhase, minutes?: number): Promise<PomodoroState>;
	pause(): Promise<PomodoroState>;
	resume(): Promise<PomodoroState>;
	skip(): Promise<PomodoroState>;
	reset(): Promise<PomodoroState>;
	today(): Promise<PomodoroToday>;
	onState(cb: (s: PomodoroState) => void): () => void;
	onPhaseEnd(cb: (e: PhaseEnd) => void): () => void;
	onSuggest(cb: (s: Suggestion) => void): () => void;
}

/** Wraps the async `on()` so callers get a synchronous unsubscribe. */
function listen<T>(subscribe: (cb: (p: T) => void) => Promise<() => void>, cb: (p: T) => void): () => void {
	let disposed = false;
	let off: (() => void) | null = null;
	subscribe(cb).then(
		(u) => (disposed ? u() : (off = u)),
		() => {}
	);
	return () => {
		disposed = true;
		off?.();
	};
}

export function createTauriPomodoroApi(): PomodoroApi {
	return {
		persistent: true,
		state: () => pomodoroState(),
		start: (phase, minutes) => pomodoroStart(phase, minutes),
		pause: () => pomodoroPause(),
		resume: () => pomodoroResume(),
		skip: () => pomodoroSkip(),
		reset: () => pomodoroReset(),
		today: () => pomodoroToday(),
		onState: (cb) => listen((h) => on('pomodoro://state', h), cb),
		onPhaseEnd: (cb) => listen((h) => on('pomodoro://phase-end', h), cb),
		onSuggest: (cb) => listen((h) => on('companion://suggest', h), cb)
	};
}

/** Browser preview settings live in localStorage (same key as the workspace store's storage backend). */
function previewSettings(): PomodoroSettings {
	try {
		const raw = typeof localStorage === 'undefined' ? null : localStorage.getItem('kotilo-deck:settings');
		return normalizeSettings(raw ? JSON.parse(raw) : {}).pomodoro;
	} catch {
		return DEFAULT_POMODORO;
	}
}

export function createLocalPomodoroApi(timer: LocalPomodoro = new LocalPomodoro(previewSettings())): PomodoroApi {
	return {
		persistent: false,
		state: async () => timer.state,
		start: async (phase, minutes) => timer.start(phase, minutes),
		pause: async () => timer.pause(),
		resume: async () => timer.resume(),
		skip: async () => timer.skip(),
		reset: async () => timer.reset(),
		today: async () => timer.today(),
		onState: (cb) => timer.onState(cb),
		onPhaseEnd: (cb) => timer.onPhaseEnd(cb),
		// No companion in a browser: suggestions never arrive.
		onSuggest: () => () => {}
	};
}

let shared: PomodoroApi | null = null;

/** One api per page, shared by every Pomodoro instance (all show the one timer). */
export function pomodoroApi(): PomodoroApi {
	if (!shared) shared = hasBackend() ? createTauriPomodoroApi() : createLocalPomodoroApi();
	return shared;
}
