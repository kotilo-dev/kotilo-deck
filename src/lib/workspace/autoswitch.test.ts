import { describe, expect, it } from 'vitest';
import type { AutoSwitchRule, CalendarEvent, PomodoroState } from '#lib/ipc/index.ts';
import {
	AutoSwitcher,
	calendarCrossings,
	needsCalendar,
	parseAt,
	pickLatest,
	pomodoroStarted,
	ruleId,
	timeCrossings,
	type AutoSwitchDeps
} from './autoswitch';

/** Local time, so the tests don't depend on the machine's zone. 2026-11-02 is a Monday. */
const at = (day: number, h: number, m = 0, s = 0) => new Date(2026, 10, day, h, m, s).getTime();
const MON = 2;
/** getDay() of Monday. */
const MONDAY = 1;

const timeRule = (id: string, time: string, workspace: string, days: number[] = [], enabled = true): AutoSwitchRule => ({
	id,
	enabled,
	trigger: { type: 'time', at: time, days },
	workspace
});

const pomo = (phase: PomodoroState['phase'], running: boolean): PomodoroState => ({
	phase,
	running,
	endsAt: running ? 1 : null,
	remaining: 60,
	duration: 1500,
	session: 1,
	sessionsPerCycle: 4
});

const event = (id: string, start: number, end: number, feedId = 'work', allDay = false): CalendarEvent => ({
	id,
	feedId,
	title: id,
	start,
	end,
	allDay,
	location: null
});

function harness(rules: AutoSwitchRule[], opts: { workspaces?: string[]; active?: string; last?: number | null } = {}) {
	const workspaces = opts.workspaces ?? ['focus', 'ops', 'agents', 'evening'];
	const state = { active: opts.active ?? 'ops', stored: opts.last ?? null, switches: [] as string[] };
	const deps: AutoSwitchDeps = {
		rules: () => rules,
		hasWorkspace: (id) => workspaces.includes(id),
		activeId: () => state.active,
		switchTo: (id) => {
			state.active = id;
			state.switches.push(id);
		},
		loadLast: () => state.stored,
		saveLast: (ms) => (state.stored = ms)
	};
	return { sw: new AutoSwitcher(deps), state };
}

describe('time crossings', () => {
	it('parses HH:MM', () => {
		expect(parseAt('09:05')).toBe(545);
		expect(parseAt('9:05')).toBe(545);
		expect(parseAt('24:00')).toBeNull();
		expect(parseAt('nope')).toBeNull();
	});

	it('fires when the clock crosses `at` on a listed weekday only', () => {
		const rules = [timeRule('a', '09:00', 'focus', [MONDAY])];
		expect(timeCrossings(rules, at(MON, 8, 59), at(MON, 9, 0, 0) + 5)).toHaveLength(1);
		expect(timeCrossings(rules, at(MON, 9, 0) + 5, at(MON, 9, 1) + 5)).toHaveLength(0);
		// Tuesday is not listed.
		expect(timeCrossings(rules, at(3, 8, 59), at(3, 9, 1))).toHaveLength(0);
		// Empty days = every day.
		expect(timeCrossings([timeRule('b', '09:00', 'focus')], at(3, 8, 59), at(3, 9, 1))).toHaveLength(1);
	});

	it('ignores disabled and malformed rules', () => {
		const rules = [timeRule('a', '09:00', 'focus', [], false), timeRule('b', '9am', 'focus')];
		expect(timeCrossings(rules, at(MON, 8), at(MON, 10))).toEqual([]);
	});

	it('lists every crossing across midnight within the catch-up window', () => {
		const rules = [timeRule('a', '23:30', 'evening'), timeRule('b', '07:00', 'focus')];
		const out = timeCrossings(rules, at(MON, 23), at(MON + 1, 8));
		expect(out.map((f) => f.rule.id).sort()).toEqual(['a', 'b']);
	});

	it('picks the latest crossing, ties go to the first rule', () => {
		const a = timeRule('a', '09:00', 'focus');
		const b = timeRule('b', '10:00', 'ops');
		const c = timeRule('c', '10:00', 'agents');
		const rules = [a, c, b];
		expect(pickLatest(timeCrossings(rules, at(MON, 8), at(MON, 11)), rules)?.rule.id).toBe('c');
		expect(pickLatest([], rules)).toBeNull();
	});
});

describe('calendar crossings', () => {
	const rule: AutoSwitchRule = { id: 'c', enabled: true, trigger: { type: 'calendar-start', feedId: null }, workspace: 'agents' };
	const onlyHome: AutoSwitchRule = { ...rule, id: 'h', trigger: { type: 'calendar-start', feedId: 'home' } };

	it('fires for a timed event starting in the window, filtered by feed', () => {
		const evs = [event('standup', at(MON, 10), at(MON, 10, 15))];
		expect(calendarCrossings([rule], evs, at(MON, 9, 59), at(MON, 10) + 5)).toHaveLength(1);
		expect(calendarCrossings([onlyHome], evs, at(MON, 9, 59), at(MON, 10) + 5)).toHaveLength(0);
		expect(calendarCrossings([onlyHome], [event('x', at(MON, 10), at(MON, 11), 'home')], at(MON, 9, 59), at(MON, 10, 1))).toHaveLength(1);
	});

	it('skips all-day events and events that already ended', () => {
		const evs = [event('holiday', at(MON, 0), at(MON + 1, 0), 'work', true), event('short', at(MON, 10), at(MON, 10, 5))];
		expect(calendarCrossings([rule], evs, at(MON - 1, 23), at(MON, 10, 30))).toEqual([]);
	});

	it('needsCalendar only for enabled calendar rules', () => {
		expect(needsCalendar([rule])).toBe(true);
		expect(needsCalendar([{ ...rule, enabled: false }, timeRule('t', '09:00', 'focus')])).toBe(false);
	});
});

describe('AutoSwitcher', () => {
	it('fires a time rule once on the minute tick and not again after a restart within the minute', () => {
		const rules = [timeRule('a', '09:00', 'focus')];
		const { sw, state } = harness(rules);
		sw.start(at(MON, 8, 58));
		sw.evaluate(at(MON, 8, 59) + 5);
		expect(state.switches).toEqual([]);
		sw.evaluate(at(MON, 9, 0) + 5);
		expect(state.switches).toEqual(['focus']);
		sw.evaluate(at(MON, 9, 1) + 5);
		expect(state.switches).toEqual(['focus']);

		// Restart at 09:00:40 with the persisted time from the 09:00 tick: no refire.
		const again = harness(rules, { last: at(MON, 9, 0) + 5, active: 'ops' });
		again.sw.start(at(MON, 9, 0, 40));
		expect(again.state.switches).toEqual([]);
	});

	it('does not catch up on the very first run', () => {
		const { sw, state } = harness([timeRule('a', '00:01', 'focus')]);
		sw.start(at(MON, 12));
		expect(state.switches).toEqual([]);
	});

	it('catches up after downtime with only the latest due rule', () => {
		const rules = [timeRule('a', '09:00', 'focus'), timeRule('b', '12:00', 'agents'), timeRule('c', '18:00', 'evening')];
		const { sw, state } = harness(rules, { last: at(MON, 8) });
		sw.start(at(MON, 13));
		expect(state.switches).toEqual(['agents']);
	});

	it('catches up when the deck is shown again (tick paused while hidden)', () => {
		const rules = [timeRule('a', '09:00', 'focus'), timeRule('b', '12:00', 'agents')];
		const { sw, state } = harness(rules);
		sw.start(at(MON, 8));
		// Hidden from 08:00 to 12:30: the next tick is the one fired on show.
		sw.evaluate(at(MON, 12, 30));
		expect(state.switches).toEqual(['agents']);
	});

	it('leaves a manual switch alone until the next trigger', () => {
		const rules = [timeRule('a', '09:00', 'focus'), timeRule('b', '10:00', 'agents')];
		const { sw, state } = harness(rules);
		sw.start(at(MON, 8, 59));
		sw.evaluate(at(MON, 9, 0) + 5);
		state.active = 'ops'; // user picks Ops by hand
		for (let m = 1; m < 60; m++) sw.evaluate(at(MON, 9, m) + 5);
		expect(state.active).toBe('ops');
		sw.evaluate(at(MON, 10, 0) + 5);
		expect(state.active).toBe('agents');
	});

	it('ignores rules targeting a missing workspace without shadowing valid ones', () => {
		const rules = [timeRule('gone', '09:00', 'deleted'), timeRule('ok', '09:00', 'focus')];
		const { sw, state } = harness(rules);
		sw.start(at(MON, 8, 59));
		sw.evaluate(at(MON, 9, 0) + 5);
		expect(state.switches).toEqual(['focus']);
	});

	it('does nothing when the target is already active', () => {
		const { sw, state } = harness([timeRule('a', '09:00', 'focus')], { active: 'focus' });
		sw.start(at(MON, 8, 59));
		sw.evaluate(at(MON, 9, 0) + 5);
		expect(state.switches).toEqual([]);
	});

	it('ignores a clock that went backwards', () => {
		const { sw, state } = harness([timeRule('a', '09:00', 'focus')]);
		sw.start(at(MON, 9, 30));
		sw.evaluate(at(MON, 8, 30));
		sw.evaluate(at(MON, 8, 59));
		expect(state.switches).toEqual([]);
	});

	it('pomodoro-start fires on a transition to running focus only', () => {
		const rules: AutoSwitchRule[] = [{ id: 'p', enabled: true, trigger: { type: 'pomodoro-start' }, workspace: 'focus' }];
		const { sw, state } = harness(rules);
		sw.seedPomodoro(pomo('idle', false));
		sw.pomodoroState(pomo('focus', true));
		expect(state.switches).toEqual(['focus']);
		state.active = 'ops';
		sw.pomodoroState(pomo('focus', true)); // same state again (settings change): no refire
		expect(state.active).toBe('ops');
		sw.pomodoroState(pomo('short-break', true));
		sw.pomodoroState(pomo('focus', true)); // auto-started next focus
		expect(state.switches).toEqual(['focus', 'focus']);
		expect(pomodoroStarted(pomo('focus', false), pomo('focus', true))).toBe(true);
	});

	it('the first unseeded pomodoro state only seeds', () => {
		const rules: AutoSwitchRule[] = [{ id: 'p', enabled: true, trigger: { type: 'pomodoro-start' }, workspace: 'focus' }];
		const { sw, state } = harness(rules);
		sw.pomodoroState(pomo('focus', true));
		expect(state.switches).toEqual([]);
	});

	it('pomodoro-end fires on the end of a focus phase', () => {
		const rules: AutoSwitchRule[] = [{ id: 'e', enabled: true, trigger: { type: 'pomodoro-end' }, workspace: 'evening' }];
		const { sw, state } = harness(rules);
		sw.phaseEnd('short-break');
		expect(state.switches).toEqual([]);
		sw.phaseEnd('focus');
		expect(state.switches).toEqual(['evening']);
	});

	it('calendar-start fires from loaded events on the tick', () => {
		const rules: AutoSwitchRule[] = [{ id: 'c', enabled: true, trigger: { type: 'calendar-start', feedId: null }, workspace: 'agents' }];
		const { sw, state } = harness(rules);
		sw.start(at(MON, 9, 58));
		sw.setEvents([event('standup', at(MON, 10), at(MON, 10, 30))]);
		sw.evaluate(at(MON, 9, 59) + 5);
		expect(state.switches).toEqual([]);
		sw.evaluate(at(MON, 10, 0) + 5);
		expect(state.switches).toEqual(['agents']);
	});

	it('makes unique rule ids', () => {
		expect(ruleId([])).toBe('rule-1');
		expect(ruleId(['rule-2', 'rule-3'])).toBe('rule-4');
		expect(ruleId(['rule-1', 'rule-3'])).toBe('rule-4');
		expect(ruleId(['rule-3'])).toBe('rule-2');
	});
});
