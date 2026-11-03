/**
 * Pomodoro maths and the browser-preview timer. Pure functions; the countdown is always derived from
 * `endsAt` on the shared 1 Hz tick (never polled per second), so every instance shows the same value.
 */
import type { PomodoroPhase, PomodoroSettings, PomodoroState, PomodoroToday } from '#lib/ipc/index.ts';

export type ActivePhase = Exclude<PomodoroPhase, 'idle'>;

export const DEFAULT_POMODORO: PomodoroSettings = {
	focus: 25,
	shortBreak: 5,
	longBreak: 15,
	sessionsPerCycle: 4,
	autoStartNext: false,
	dailyGoal: 8
};

/** Companion suggestion banner lifetime (DESIGN §5.8, protocol.md `pomodoro.suggest`). */
export const SUGGEST_MS = 60_000;

/** Seconds left at `nowMs`: from `endsAt` while running, else the stored `remaining`. Never negative. */
export function remainingAt(state: Pick<PomodoroState, 'running' | 'endsAt' | 'remaining'>, nowMs: number): number {
	if (state.running && state.endsAt !== null) return Math.max(0, Math.ceil((state.endsAt - nowMs) / 1000));
	return Math.max(0, Math.round(state.remaining));
}

/** Fraction of the phase still left, 0–1 (the ring shows the remaining arc and depletes clockwise). */
export function remainingFraction(remaining: number, duration: number): number {
	if (!(duration > 0)) return 0;
	return Math.min(1, Math.max(0, remaining / duration));
}

/** `stroke-dashoffset` for a ring of circumference `circ` (0 = full ring, `circ` = empty). */
export function ringOffset(circ: number, remaining: number, duration: number): number {
	return circ * (1 - remainingFraction(remaining, duration));
}

/** Circumference of a ring of radius r. */
export const circumference = (r: number): number => 2 * Math.PI * r;

/** "25:00", "4:05", "1:02:03" (≥ 1 h). */
export function formatCountdown(totalSeconds: number): string {
	const s = Math.max(0, Math.round(totalSeconds));
	const h = Math.floor(s / 3600);
	const m = Math.floor((s % 3600) / 60);
	const sec = String(s % 60).padStart(2, '0');
	return h > 0 ? `${h}:${String(m).padStart(2, '0')}:${sec}` : `${String(m).padStart(2, '0')}:${sec}`;
}

export const isBreak = (phase: PomodoroPhase): boolean => phase === 'short-break' || phase === 'long-break';

/** Ring / dot colour token: focus (and idle) use accent, breaks success (DESIGN §5.8). */
export const phaseTone = (phase: PomodoroPhase): 'accent' | 'success' => (isBreak(phase) ? 'success' : 'accent');

/** i18n key suffix for the header title. */
export const phaseKey = (phase: PomodoroPhase): 'focus' | 'shortBreak' | 'longBreak' =>
	phase === 'short-break' ? 'shortBreak' : phase === 'long-break' ? 'longBreak' : 'focus';

/** Primary button: start when idle, pause while running, resume when paused. */
export function primaryAction(state: Pick<PomodoroState, 'phase' | 'running'>): 'start' | 'pause' | 'resume' {
	if (state.phase === 'idle') return 'start';
	return state.running ? 'pause' : 'resume';
}

/** Minutes of `phase` from the settings. */
export function phaseMinutes(phase: ActivePhase, s: PomodoroSettings): number {
	return phase === 'focus' ? s.focus : phase === 'short-break' ? s.shortBreak : s.longBreak;
}

/** After focus #session: long break at the end of the cycle, else a short one. After a break: focus. */
export function nextPhase(phase: PomodoroPhase, session: number, sessionsPerCycle: number): ActivePhase {
	if (phase === 'focus') return session >= sessionsPerCycle ? 'long-break' : 'short-break';
	return 'focus';
}

/**
 * Session index once the phase after `phase` begins (same rule as the backend): a finished focus moves to the
 * next session, or back to 1 when it closed the cycle (long break); a break keeps the index.
 */
export function nextSession(phase: PomodoroPhase, session: number, sessionsPerCycle: number): number {
	if (phase !== 'focus') return session;
	return session >= sessionsPerCycle ? 1 : session + 1;
}

/** Daily stat ring fraction, 0–1. */
export const goalFraction = (today: Pick<PomodoroToday, 'sessions' | 'goal'>): number =>
	today.goal > 0 ? Math.min(1, Math.max(0, today.sessions / today.goal)) : 0;

/** Suggestion banners are pointless while a focus block is already running. */
export const suggestionApplies = (state: Pick<PomodoroState, 'phase' | 'running'> | null): boolean =>
	!(state && state.phase === 'focus' && state.running);

/** Clamps suggested minutes to the focus range (1–180); null/invalid → undefined (backend uses the focus setting). */
export function suggestedMinutes(minutes: number | null | undefined): number | undefined {
	if (minutes === null || minutes === undefined) return undefined;
	const m = Number(minutes);
	if (!Number.isFinite(m) || m <= 0) return undefined;
	return Math.min(180, Math.max(1, Math.round(m)));
}

export const idleState = (s: PomodoroSettings): PomodoroState => ({
	phase: 'idle',
	running: false,
	endsAt: null,
	remaining: s.focus * 60,
	duration: s.focus * 60,
	session: 1,
	sessionsPerCycle: s.sessionsPerCycle
});

/** Local day key "YYYY-MM-DD" of `ms`. */
const dayKey = (ms: number): string => {
	const d = new Date(ms);
	return `${d.getFullYear()}-${d.getMonth() + 1}-${d.getDate()}`;
};

export interface PhaseEnd {
	phase: ActivePhase;
	next: PomodoroPhase;
}

/**
 * Browser-preview timer with the backend's semantics (src/lib/ipc `pomodoro*`): one shared instance,
 * one `setTimeout` armed at `endsAt` (no per-second work), stats kept in memory for the session.
 */
export class LocalPomodoro {
	#state: PomodoroState;
	#settings: PomodoroSettings;
	#now: () => number;
	#timer: ReturnType<typeof setTimeout> | null = null;
	#today = { day: '', sessions: 0, focusMinutes: 0 };
	#onState = new Set<(s: PomodoroState) => void>();
	#onEnd = new Set<(e: PhaseEnd) => void>();

	constructor(settings: PomodoroSettings = DEFAULT_POMODORO, now: () => number = Date.now) {
		this.#settings = { ...settings };
		this.#now = now;
		this.#state = idleState(this.#settings);
	}

	get state(): PomodoroState {
		return { ...this.#state };
	}

	/** New durations apply to the next phase; an idle timer picks up the new focus length at once. */
	configure(settings: PomodoroSettings): void {
		this.#settings = { ...settings };
		if (this.#state.phase === 'idle') this.#set(idleState(this.#settings));
		else this.#set({ ...this.#state, sessionsPerCycle: settings.sessionsPerCycle });
	}

	start(phase?: ActivePhase, minutes?: number): PomodoroState {
		const p: ActivePhase = phase ?? (this.#state.phase === 'idle' ? 'focus' : this.#state.phase);
		const duration = Math.round((minutes ?? phaseMinutes(p, this.#settings)) * 60);
		this.#set({
			...this.#state,
			phase: p,
			running: true,
			duration,
			remaining: duration,
			endsAt: this.#now() + duration * 1000
		});
		return this.state;
	}

	pause(): PomodoroState {
		if (!this.#state.running) return this.state;
		this.#set({ ...this.#state, running: false, remaining: remainingAt(this.#state, this.#now()), endsAt: null });
		return this.state;
	}

	resume(): PomodoroState {
		if (this.#state.running || this.#state.phase === 'idle') return this.state;
		this.#set({ ...this.#state, running: true, endsAt: this.#now() + this.#state.remaining * 1000 });
		return this.state;
	}

	skip(): PomodoroState {
		if (this.#state.phase !== 'idle') this.#advance(false);
		return this.state;
	}

	reset(): PomodoroState {
		this.#set(idleState(this.#settings));
		return this.state;
	}

	today(): PomodoroToday {
		this.#rollDay();
		return { sessions: this.#today.sessions, focusMinutes: this.#today.focusMinutes, goal: this.#settings.dailyGoal };
	}

	onState(cb: (s: PomodoroState) => void): () => void {
		this.#onState.add(cb);
		return () => this.#onState.delete(cb);
	}

	onPhaseEnd(cb: (e: PhaseEnd) => void): () => void {
		this.#onEnd.add(cb);
		return () => this.#onEnd.delete(cb);
	}

	/** Test hook / timer callback: finishes the phase if its end has passed. */
	check(): void {
		const s = this.#state;
		if (s.running && s.endsAt !== null && s.endsAt <= this.#now()) this.#advance(true);
	}

	dispose(): void {
		if (this.#timer !== null) clearTimeout(this.#timer);
		this.#timer = null;
	}

	#rollDay(): void {
		const key = dayKey(this.#now());
		if (this.#today.day !== key) this.#today = { day: key, sessions: 0, focusMinutes: 0 };
	}

	#advance(completed: boolean): void {
		const s = this.#state;
		const ended = s.phase as ActivePhase;
		if (completed && ended === 'focus') {
			this.#rollDay();
			this.#today.sessions += 1;
			this.#today.focusMinutes += Math.round(s.duration / 60);
		}
		const next = nextPhase(ended, s.session, s.sessionsPerCycle);
		const session = nextSession(ended, s.session, s.sessionsPerCycle);
		const duration = phaseMinutes(next, this.#settings) * 60;
		const run = completed && this.#settings.autoStartNext;
		this.#set({
			...s,
			phase: next,
			session,
			duration,
			remaining: duration,
			running: run,
			endsAt: run ? this.#now() + duration * 1000 : null
		});
		if (completed) for (const cb of [...this.#onEnd]) cb({ phase: ended, next });
	}

	#set(next: PomodoroState): void {
		this.#state = next;
		this.dispose();
		if (next.running && next.endsAt !== null) {
			const delay = Math.min(2 ** 31 - 1, Math.max(0, next.endsAt - this.#now()));
			this.#timer = setTimeout(() => {
				this.#timer = null;
				this.check();
			}, delay);
		}
		for (const cb of [...this.#onState]) cb(this.state);
	}
}
