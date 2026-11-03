import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { PomodoroState } from '#lib/ipc/index.ts';
import {
	circumference,
	DEFAULT_POMODORO,
	formatCountdown,
	goalFraction,
	LocalPomodoro,
	nextPhase,
	nextSession,
	phaseKey,
	phaseTone,
	primaryAction,
	remainingAt,
	remainingFraction,
	ringOffset,
	suggestedMinutes,
	suggestionApplies
} from './logic';

const state = (over: Partial<PomodoroState> = {}): PomodoroState => ({
	phase: 'focus',
	running: true,
	endsAt: 100_000,
	remaining: 1500,
	duration: 1500,
	session: 2,
	sessionsPerCycle: 4,
	...over
});

describe('countdown', () => {
	it('derives the remaining seconds from endsAt while running', () => {
		expect(remainingAt(state(), 100_000 - 1_104_000)).toBe(1104); // 18:24
		expect(remainingAt(state(), 100_000 - 1_103_400)).toBe(1104); // rounds up within the second
		expect(remainingAt(state(), 100_000)).toBe(0);
		expect(remainingAt(state(), 200_000)).toBe(0); // never negative
	});

	it('uses the stored remaining while paused or idle', () => {
		expect(remainingAt(state({ running: false, endsAt: null, remaining: 300 }), 9e12)).toBe(300);
		expect(remainingAt(state({ phase: 'idle', running: false, endsAt: null, remaining: 1500 }), 0)).toBe(1500);
	});

	it('formats mm:ss and h:mm:ss', () => {
		expect(formatCountdown(1500)).toBe('25:00');
		expect(formatCountdown(1104)).toBe('18:24');
		expect(formatCountdown(65)).toBe('01:05');
		expect(formatCountdown(0)).toBe('00:00');
		expect(formatCountdown(-3)).toBe('00:00');
		expect(formatCountdown(3723)).toBe('1:02:03');
	});
});

describe('ring progress', () => {
	const C = circumference(60);

	it('matches the DESIGN comp (18:24 of 25:00 → offset ≈ 99 of 377)', () => {
		expect(Math.round(C)).toBe(377);
		expect(ringOffset(C, 1104, 1500)).toBeCloseTo(99.5, 0);
	});

	it('is full at the start, empty at the end and clamped', () => {
		expect(ringOffset(C, 1500, 1500)).toBe(0);
		expect(ringOffset(C, 0, 1500)).toBe(C);
		expect(remainingFraction(2000, 1500)).toBe(1);
		expect(remainingFraction(10, 0)).toBe(0);
	});
});

describe('phases', () => {
	it('uses accent for focus/idle and success for breaks', () => {
		expect(phaseTone('focus')).toBe('accent');
		expect(phaseTone('idle')).toBe('accent');
		expect(phaseTone('short-break')).toBe('success');
		expect(phaseTone('long-break')).toBe('success');
		expect(phaseKey('long-break')).toBe('longBreak');
	});

	it('picks the primary action from the state', () => {
		expect(primaryAction({ phase: 'idle', running: false })).toBe('start');
		expect(primaryAction({ phase: 'focus', running: true })).toBe('pause');
		expect(primaryAction({ phase: 'short-break', running: false })).toBe('resume');
	});

	it('cycles focus → short break … → long break → focus (session 1)', () => {
		expect(nextPhase('focus', 1, 4)).toBe('short-break');
		expect(nextPhase('focus', 4, 4)).toBe('long-break');
		expect(nextPhase('short-break', 1, 4)).toBe('focus');
		// Same rule as src-tauri/src/widgets/pomodoro.rs: the index advances when a focus block ends.
		expect(nextSession('focus', 1, 4)).toBe(2);
		expect(nextSession('focus', 4, 4)).toBe(1);
		expect(nextSession('short-break', 2, 4)).toBe(2);
		expect(nextSession('long-break', 1, 4)).toBe(1);
	});

	it('daily goal fraction is clamped', () => {
		expect(goalFraction({ sessions: 3, goal: 8 })).toBe(0.375);
		expect(goalFraction({ sessions: 12, goal: 8 })).toBe(1);
		expect(goalFraction({ sessions: 1, goal: 0 })).toBe(0);
	});
});

describe('companion suggestion', () => {
	it('is hidden while a focus block runs', () => {
		expect(suggestionApplies(null)).toBe(true);
		expect(suggestionApplies({ phase: 'focus', running: true })).toBe(false);
		expect(suggestionApplies({ phase: 'focus', running: false })).toBe(true);
		expect(suggestionApplies({ phase: 'short-break', running: true })).toBe(true);
	});

	it('clamps suggested minutes and leaves null to the backend default', () => {
		expect(suggestedMinutes(25)).toBe(25);
		expect(suggestedMinutes(null)).toBeUndefined();
		expect(suggestedMinutes(0)).toBeUndefined();
		expect(suggestedMinutes(500)).toBe(180);
		expect(suggestedMinutes(12.4)).toBe(12);
	});
});

describe('LocalPomodoro (browser preview timer)', () => {
	let now = 1_000_000;
	beforeEach(() => {
		vi.useFakeTimers();
		now = 1_000_000;
	});
	afterEach(() => vi.useRealTimers());

	const make = (over = {}) => new LocalPomodoro({ ...DEFAULT_POMODORO, focus: 1, shortBreak: 1, longBreak: 2, sessionsPerCycle: 2, ...over }, () => now);
	const advance = (ms: number) => {
		now += ms;
		vi.advanceTimersByTime(ms);
	};

	it('starts idle with the focus length', () => {
		const p = make();
		expect(p.state).toMatchObject({ phase: 'idle', running: false, remaining: 60, duration: 60, session: 1 });
	});

	it('pauses and resumes with the remaining time', () => {
		const p = make();
		p.start();
		advance(20_000);
		expect(p.pause()).toMatchObject({ running: false, remaining: 40, endsAt: null });
		advance(60_000);
		const r = p.resume();
		expect(r.running).toBe(true);
		expect(r.endsAt).toBe(now + 40_000);
	});

	it('finishes a focus block with one timer, counts it and announces the phase end', () => {
		const p = make();
		const ends: unknown[] = [];
		p.onPhaseEnd((e) => ends.push(e));
		p.start();
		advance(60_000);
		expect(ends).toEqual([{ phase: 'focus', next: 'short-break' }]);
		expect(p.state).toMatchObject({ phase: 'short-break', running: false, remaining: 60 });
		expect(p.today()).toEqual({ sessions: 1, focusMinutes: 1, goal: 8 });
	});

	it('auto-starts the next phase when enabled', () => {
		const p = make({ autoStartNext: true });
		p.start();
		advance(60_000);
		expect(p.state).toMatchObject({ phase: 'short-break', running: true, endsAt: now + 60_000 });
	});

	it('does not count a skipped focus and goes to the long break after the cycle', () => {
		const p = make();
		p.start();
		p.skip(); // focus 1 skipped → short break before session 2
		expect(p.today().sessions).toBe(0);
		expect(p.state).toMatchObject({ phase: 'short-break', session: 2 });
		p.skip(); // → focus 2
		expect(p.state).toMatchObject({ phase: 'focus', session: 2 });
		p.skip(); // focus 2 of 2 → long break, cycle restarts
		expect(p.state).toMatchObject({ phase: 'long-break', session: 1, duration: 120 });
		p.skip();
		expect(p.state).toMatchObject({ phase: 'focus', session: 1 });
	});

	it('starts a suggested block with custom minutes and resets to idle', () => {
		const p = make();
		expect(p.start('focus', 25)).toMatchObject({ phase: 'focus', duration: 1500, remaining: 1500 });
		expect(p.reset()).toMatchObject({ phase: 'idle', running: false, session: 1 });
	});

	it('notifies state listeners on every change', () => {
		const p = make();
		const seen: string[] = [];
		const off = p.onState((s) => seen.push(`${s.phase}:${s.running}`));
		p.start();
		p.pause();
		off();
		p.resume();
		expect(seen).toEqual(['focus:true', 'focus:false']);
	});
});
