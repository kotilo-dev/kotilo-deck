import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import {
	createOptimistic,
	createThrottle,
	deviceRows,
	fromPct,
	HOLD_MS,
	mergeSessions,
	speakerLevel,
	THROTTLE_MS,
	toPct
} from './logic';

beforeEach(() => vi.useFakeTimers({ now: 1_000_000 }));
afterEach(() => vi.useRealTimers());

describe('createThrottle', () => {
	it('sends the first value at once, then at most one per interval, always ending on the last', () => {
		const send = vi.fn();
		const th = createThrottle(send);
		th.push(1);
		expect(send).toHaveBeenLastCalledWith(1);
		th.push(2);
		th.push(3);
		expect(send).toHaveBeenCalledTimes(1);
		vi.advanceTimersByTime(THROTTLE_MS);
		expect(send).toHaveBeenCalledTimes(2);
		expect(send).toHaveBeenLastCalledWith(3);
		vi.advanceTimersByTime(1000);
		expect(send).toHaveBeenCalledTimes(2);
	});

	it('caps a 1 s drag at about 30 writes', () => {
		const send = vi.fn();
		const th = createThrottle(send);
		for (let i = 0; i < 250; i++) {
			th.push(i);
			vi.advanceTimersByTime(4);
		}
		vi.advanceTimersByTime(THROTTLE_MS);
		expect(send.mock.calls.length).toBeLessThanOrEqual(32);
		expect(send.mock.calls.length).toBeGreaterThanOrEqual(28);
		expect(send).toHaveBeenLastCalledWith(249);
	});

	it('flush sends the trailing value now; cancel drops it', () => {
		const send = vi.fn();
		const th = createThrottle(send);
		th.push(1);
		th.push(2);
		th.flush();
		expect(send).toHaveBeenLastCalledWith(2);
		th.push(3);
		th.cancel();
		vi.advanceTimersByTime(1000);
		expect(send).toHaveBeenCalledTimes(2);
		expect(vi.getTimerCount()).toBe(0);
	});
});

describe('createOptimistic', () => {
	it('shows the local value until the hold passes, then the remote one', () => {
		const settle = vi.fn();
		const o = createOptimistic<number>(settle);
		expect(o.value(40)).toBe(40);
		o.set(70);
		expect(o.value(40)).toBe(70);
		vi.advanceTimersByTime(HOLD_MS - 1);
		expect(o.value(40)).toBe(70);
		vi.advanceTimersByTime(1);
		expect(settle).toHaveBeenCalledTimes(1);
		expect(o.value(55)).toBe(55);
		expect(o.active).toBe(false);
	});

	it('never settles while the control is held', () => {
		const o = createOptimistic<number>(() => {});
		o.hold(true);
		o.set(10);
		vi.advanceTimersByTime(HOLD_MS * 5);
		expect(o.value(90)).toBe(10);
		o.hold(false);
		vi.advanceTimersByTime(HOLD_MS);
		expect(o.value(90)).toBe(90);
	});

	it('dispose clears the timer', () => {
		const o = createOptimistic<boolean>(() => {});
		o.set(true);
		o.dispose();
		expect(vi.getTimerCount()).toBe(0);
		expect(o.value(false)).toBe(false);
	});
});

describe('values and lists', () => {
	it('converts between 0–1 and percent safely', () => {
		expect(toPct(0.624)).toBe(62);
		expect(toPct(1.4)).toBe(100);
		expect(toPct(Number.NaN)).toBe(0);
		expect(fromPct(62)).toBe(0.62);
		expect(fromPct(-3)).toBe(0);
		expect(speakerLevel(0.3, false)).toBe(1);
		expect(speakerLevel(0.8, false)).toBe(2);
		expect(speakerLevel(0.8, true)).toBe(0);
		expect(speakerLevel(0, false)).toBe(0);
	});

	const dev = (id: string) => ({ id, name: id.toUpperCase(), isDefault: false });

	it('puts the default device first and reserves a "more" slot when they do not fit', () => {
		const ds = [dev('a'), dev('b'), dev('c')];
		expect(deviceRows(ds, 'c', 3)).toEqual({ rows: [dev('c'), dev('a'), dev('b')], more: false });
		expect(deviceRows(ds, 'b', 2)).toEqual({ rows: [dev('b')], more: true });
		expect(deviceRows(ds, null, 0)).toEqual({ rows: [], more: true });
		expect(deviceRows([], null, 2)).toEqual({ rows: [], more: false });
	});

	it('keeps recently touched mixer rows local and sorts by name', () => {
		const remote = [
			{ id: '2', name: 'Spotify', volume: 0.2, muted: false },
			{ id: '1', name: 'Edge', volume: 0.5, muted: false }
		];
		const local = [{ id: '2', name: 'Spotify', volume: 0.9, muted: true }];
		const merged = mergeSessions(remote, local, new Map([['2', 1000]]), 1000 + HOLD_MS - 1);
		expect(merged.map((s) => [s.name, s.volume, s.muted])).toEqual([
			['Edge', 0.5, false],
			['Spotify', 0.9, true]
		]);
		expect(mergeSessions(remote, local, new Map([['2', 1000]]), 1000 + HOLD_MS)[1].volume).toBe(0.2);
	});
});
