import { dayOfYear, isoWeek, zonedParts } from '#lib/time/index.ts';

export interface DateFacts {
	day: number;
	week: number;
	dayOfYear: number;
}

/** Calendar facts for the date shown by the widget (OS zone unless given). */
export function dateFacts(now: Date, timeZone?: string): DateFacts {
	const p = zonedParts(now, timeZone);
	return { day: p.day, week: isoWeek(p.year, p.month, p.day), dayOfYear: dayOfYear(p.year, p.month, p.day) };
}
