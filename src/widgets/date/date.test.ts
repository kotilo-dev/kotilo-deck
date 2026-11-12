import { describe, expect, it } from 'vitest';
import { dateFacts, weekStart, weekStrip } from './logic';

describe('dateFacts', () => {
	it('matches the reference day (Tue 3 Nov 2026)', () => {
		expect(dateFacts(new Date(Date.UTC(2026, 10, 3, 9)), 'Europe/Istanbul')).toEqual({ day: 3, week: 45, dayOfYear: 307 });
	});
	it('uses the zone, not UTC, near midnight', () => {
		// 22:30 UTC on 31 Dec is already 1 Jan in Istanbul (ISO week 53 of 2026).
		expect(dateFacts(new Date(Date.UTC(2026, 11, 31, 22, 30)), 'Europe/Istanbul')).toEqual({ day: 1, week: 53, dayOfYear: 1 });
		expect(dateFacts(new Date(Date.UTC(2026, 11, 31, 22, 30)), 'UTC')).toEqual({ day: 31, week: 53, dayOfYear: 365 });
	});
});

describe('week strip', () => {
	it('week start follows the locale', () => {
		expect(weekStart('en-US')).toBe(0);
		expect(weekStart('tr-TR')).toBe(1);
		expect(weekStart('en-GB')).toBe(1);
		expect(weekStart('de')).toBe(1);
		expect(weekStart('not a locale!')).toBe(1);
	});

	it('seven days around today, past days flagged, crossing a month', () => {
		const thu = new Date(2026, 10, 5, 10, 42); // Thursday
		const mon = weekStrip(thu, 1);
		expect(mon.map((d) => d.day)).toEqual([2, 3, 4, 5, 6, 7, 8]);
		expect(mon.map((d) => d.past)).toEqual([true, true, true, false, false, false, false]);
		expect(mon.findIndex((d) => d.today)).toBe(3);
		expect(weekStrip(thu, 0).map((d) => d.day)).toEqual([1, 2, 3, 4, 5, 6, 7]);
		expect(weekStrip(new Date(2026, 10, 1), 1).map((d) => d.day)).toEqual([26, 27, 28, 29, 30, 31, 1]);
	});
});
