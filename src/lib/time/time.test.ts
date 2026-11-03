import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import {
	cityFromZone,
	dayOfYear,
	delayToBoundary,
	formatClock,
	formatUtcOffset,
	isValidTimeZone,
	isoWeek,
	minutesSince,
	subscribeTick,
	tickInterval,
	utcOffsetMinutes,
	zonedParts
} from './index';

describe('isoWeek', () => {
	it.each([
		[2026, 11, 3, 45],
		[2026, 1, 1, 1], // Thursday
		[2021, 1, 3, 53], // Sunday belongs to 2020-W53
		[2024, 12, 30, 1], // Monday belongs to 2025-W01
		[2020, 12, 31, 53],
		[2027, 1, 1, 53] // Friday belongs to 2026-W53
	])('%i-%i-%i → W%i', (y, m, d, w) => expect(isoWeek(y, m, d)).toBe(w));
});

describe('dayOfYear', () => {
	it('counts from 1', () => {
		expect(dayOfYear(2026, 1, 1)).toBe(1);
		expect(dayOfYear(2026, 11, 3)).toBe(307);
		expect(dayOfYear(2024, 12, 31)).toBe(366);
	});
});

describe('time zones', () => {
	const at = new Date(Date.UTC(2026, 10, 3, 6, 37, 14));

	it('reads wall-clock parts in a zone', () => {
		expect(zonedParts(at, 'Europe/Istanbul')).toEqual({
			year: 2026,
			month: 11,
			day: 3,
			hour: 9,
			minute: 37,
			second: 14,
			weekday: 2
		});
		expect(zonedParts(at, 'America/New_York').hour).toBe(1);
	});

	it('computes and formats UTC offsets', () => {
		expect(utcOffsetMinutes(at, 'Europe/Istanbul')).toBe(180);
		expect(utcOffsetMinutes(at, 'Asia/Kolkata')).toBe(330);
		expect(utcOffsetMinutes(at, 'UTC')).toBe(0);
		expect(formatUtcOffset(180)).toBe('UTC+3');
		expect(formatUtcOffset(-300)).toBe('UTC-5');
		expect(formatUtcOffset(330)).toBe('UTC+5:30');
		expect(formatUtcOffset(0)).toBe('UTC');
	});

	it('derives city labels and validates zones', () => {
		expect(cityFromZone('Europe/Istanbul')).toBe('Istanbul');
		expect(cityFromZone('America/Argentina/Buenos_Aires')).toBe('Buenos Aires');
		expect(isValidTimeZone('Europe/Berlin')).toBe(true);
		expect(isValidTimeZone('Mars/Olympus')).toBe(false);
		expect(isValidTimeZone('')).toBe(false);
	});
});

describe('formatClock', () => {
	const at = new Date(Date.UTC(2026, 10, 3, 18, 7, 4));

	it('formats 24h with seconds split out', () => {
		expect(formatClock(at, { locale: 'en-GB', hour12: false, timeZone: 'Europe/Istanbul' })).toEqual({
			hm: '21:07',
			seconds: '04',
			period: null
		});
	});

	it('formats 12h with a day period', () => {
		const t = formatClock(at, { locale: 'en-US', hour12: true, timeZone: 'Europe/Istanbul' });
		expect(t.hm).toBe('9:07');
		expect(t.period).toBe('PM');
	});

	it('never renders 24:00 at midnight', () => {
		const midnight = new Date(Date.UTC(2026, 10, 3, 21, 0, 0));
		expect(formatClock(midnight, { locale: 'en-US', hour12: false, timeZone: 'Europe/Istanbul' }).hm).toBe('00:00');
	});
});

describe('minutesSince', () => {
	it('floors and clamps', () => {
		expect(minutesSince(0, 6.9 * 60_000)).toBe(6);
		expect(minutesSince(10_000, 0)).toBe(0);
	});
});

describe('subscribeTick', () => {
	beforeEach(() => {
		vi.useFakeTimers();
		vi.setSystemTime(new Date(Date.UTC(2026, 10, 3, 6, 37, 14, 250)));
	});
	afterEach(() => vi.useRealTimers());

	it('aligns to the boundary', () => {
		expect(delayToBoundary(14_250, 1000)).toBe(755);
		expect(delayToBoundary(14_250, 60_000)).toBe(45_755);
	});

	it('ticks per minute unless someone wants seconds, with one shared timer', () => {
		const minute = vi.fn();
		const offMinute = subscribeTick(minute);
		expect(tickInterval()).toBe(60_000);

		vi.advanceTimersByTime(45_000);
		expect(minute).not.toHaveBeenCalled();
		vi.advanceTimersByTime(1_000);
		expect(minute).toHaveBeenCalledTimes(1);

		const second = vi.fn();
		const offSecond = subscribeTick(second, { seconds: true });
		expect(tickInterval()).toBe(1000);
		vi.advanceTimersByTime(3_000);
		expect(second).toHaveBeenCalledTimes(3);
		expect(minute).toHaveBeenCalledTimes(4);
		expect(vi.getTimerCount()).toBe(1);

		offSecond();
		expect(tickInterval()).toBe(60_000);
		offMinute();
		expect(tickInterval()).toBeNull();
		expect(vi.getTimerCount()).toBe(0);
	});
});
