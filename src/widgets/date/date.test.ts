import { describe, expect, it } from 'vitest';
import { dateFacts } from './logic';

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
