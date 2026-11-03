/**
 * Workspace auto-switch rules (SPEC §4.1), evaluated on the deck. Pure logic: the deck page feeds it
 * the shared minute tick, pomodoro events and calendar data; no timers of its own.
 *
 * - `time`: fires once when the clock crosses `at` (local) on a listed weekday. The last evaluation time is
 *   persisted, so a restart within the same minute doesn't fire again. While the deck is hidden the tick
 *   pauses; the first tick after showing catches up and applies only the latest due rule.
 * - `calendar-start`: an event start crossing now (same window as `time`, timed events only, event still
 *   running).
 * - `pomodoro-start`: state changes to running focus from anything that wasn't running focus.
 * - `pomodoro-end`: `pomodoro://phase-end` with phase focus.
 *
 * Rules are only triggers: a manual switch stays until the next trigger fires. Disabled rules and rules
 * targeting a missing workspace are ignored (they never shadow a valid rule).
 */
import type { AutoSwitchRule, CalendarEvent, PomodoroPhase, PomodoroState } from '#lib/ipc/index.ts';

/** Catch-up window after a long hide or downtime. */
export const MAX_CATCH_UP_MS = 24 * 60 * 60 * 1000;

export interface Firing {
	rule: AutoSwitchRule;
	/** Unix ms of the crossing. */
	at: number;
}

/** "HH:MM" → minutes since midnight, or null. */
export function parseAt(value: string): number | null {
	const m = /^(\d{1,2}):(\d{2})$/.exec(value);
	if (!m) return null;
	const h = Number(m[1]);
	const min = Number(m[2]);
	return h < 24 && min < 60 ? h * 60 + min : null;
}

/** Every `time` rule crossing in the window (from, to], local time. */
export function timeCrossings(rules: readonly AutoSwitchRule[], from: number, to: number): Firing[] {
	if (!(to > from)) return [];
	const start = Math.max(from, to - MAX_CATCH_UP_MS);
	const out: Firing[] = [];
	for (const rule of rules) {
		const tr = rule.trigger;
		if (!rule.enabled || tr.type !== 'time') continue;
		const minutes = parseAt(tr.at);
		if (minutes === null) continue;
		const day = new Date(start);
		day.setHours(0, 0, 0, 0);
		for (; day.getTime() <= to; day.setDate(day.getDate() + 1)) {
			const at = new Date(day.getFullYear(), day.getMonth(), day.getDate(), Math.floor(minutes / 60), minutes % 60).getTime();
			if (at <= start || at > to) continue;
			if (tr.days.length > 0 && !tr.days.includes(new Date(at).getDay())) continue;
			out.push({ rule, at });
		}
	}
	return out;
}

/** Event starts in (from, to] for `calendar-start` rules; all-day and already ended events never trigger. */
export function calendarCrossings(
	rules: readonly AutoSwitchRule[],
	events: readonly CalendarEvent[],
	from: number,
	to: number
): Firing[] {
	if (!(to > from)) return [];
	const start = Math.max(from, to - MAX_CATCH_UP_MS);
	const out: Firing[] = [];
	for (const rule of rules) {
		const tr = rule.trigger;
		if (!rule.enabled || tr.type !== 'calendar-start') continue;
		for (const ev of events) {
			if (ev.allDay || ev.start <= start || ev.start > to || ev.end <= to) continue;
			if (tr.feedId !== null && ev.feedId !== tr.feedId) continue;
			out.push({ rule, at: ev.start });
		}
	}
	return out;
}

/** Latest crossing wins; on a tie the rule listed first. */
export function pickLatest(firings: readonly Firing[], rules: readonly AutoSwitchRule[]): Firing | null {
	let best: Firing | null = null;
	for (const f of firings) {
		if (
			!best ||
			f.at > best.at ||
			(f.at === best.at && rules.indexOf(f.rule) < rules.indexOf(best.rule))
		)
			best = f;
	}
	return best;
}

const runningFocus = (s: PomodoroState | null) => !!s && s.phase === 'focus' && s.running;

/** True when `next` is running focus and `prev` wasn't (start, resume, auto-start after a break). */
export const pomodoroStarted = (prev: PomodoroState | null, next: PomodoroState): boolean =>
	runningFocus(next) && !runningFocus(prev);

export interface AutoSwitchDeps {
	rules(): readonly AutoSwitchRule[];
	hasWorkspace(id: string): boolean;
	activeId(): string;
	/** Switch and tell the user (toast). Only called when the target differs from the active workspace. */
	switchTo(id: string, rule: AutoSwitchRule): void;
	/** Persisted last evaluation time (Unix ms), null when unknown. */
	loadLast(): number | null;
	saveLast(ms: number): void;
}

export class AutoSwitcher {
	#deps: AutoSwitchDeps;
	#last: number | null = null;
	#savedMinute = -1;
	#events: CalendarEvent[] = [];
	#pomodoro: PomodoroState | null = null;
	#pomodoroKnown = false;

	constructor(deps: AutoSwitchDeps) {
		this.#deps = deps;
	}

	/** First evaluation at boot: catches up from the persisted time (first run ever: nothing to catch up). */
	start(now: number): void {
		const stored = this.#deps.loadLast();
		this.#last = stored !== null && stored <= now ? stored : now;
		this.evaluate(now);
	}

	/** Shared tick (also fires right after the deck is shown again). */
	evaluate(now: number): void {
		const from = this.#last ?? now;
		this.#last = now;
		const minute = Math.floor(now / 60000);
		if (minute !== this.#savedMinute) {
			this.#savedMinute = minute;
			this.#deps.saveLast(now);
		}
		if (now <= from) return; // clock went backwards or same instant
		const rules = this.#valid();
		const best = pickLatest([...timeCrossings(rules, from, now), ...calendarCrossings(rules, this.#events, from, now)], rules);
		if (best) this.#apply(best.rule);
	}

	/** Upcoming calendar events (refreshed on `calendar://changed`). */
	setEvents(events: readonly CalendarEvent[]): void {
		this.#events = [...events];
	}

	/** Seeds the pomodoro state at boot without firing. */
	seedPomodoro(state: PomodoroState | null): void {
		if (this.#pomodoroKnown) return;
		this.#pomodoro = state;
		this.#pomodoroKnown = true;
	}

	/** `pomodoro://state`. The very first state, if not seeded, only seeds. */
	pomodoroState(next: PomodoroState): void {
		const prev = this.#pomodoro;
		const known = this.#pomodoroKnown;
		this.#pomodoro = next;
		this.#pomodoroKnown = true;
		if (known && pomodoroStarted(prev, next)) this.#first('pomodoro-start');
	}

	/** `pomodoro://phase-end`. */
	phaseEnd(phase: Exclude<PomodoroPhase, 'idle'>): void {
		if (phase === 'focus') this.#first('pomodoro-end');
	}

	#valid(): AutoSwitchRule[] {
		return this.#deps.rules().filter((r) => r.enabled && this.#deps.hasWorkspace(r.workspace));
	}

	#first(type: 'pomodoro-start' | 'pomodoro-end') {
		const rule = this.#valid().find((r) => r.trigger.type === type);
		if (rule) this.#apply(rule);
	}

	#apply(rule: AutoSwitchRule) {
		if (rule.workspace === this.#deps.activeId()) return;
		this.#deps.switchTo(rule.workspace, rule);
	}
}

/** Fresh rule id, unique among `taken`. */
export function ruleId(taken: readonly string[]): string {
	let n = taken.length + 1;
	while (taken.includes(`rule-${n}`)) n++;
	return `rule-${n}`;
}

/** Whether any enabled rule needs calendar data (so the deck only fetches events when needed). */
export const needsCalendar = (rules: readonly AutoSwitchRule[]): boolean =>
	rules.some((r) => r.enabled && r.trigger.type === 'calendar-start');
