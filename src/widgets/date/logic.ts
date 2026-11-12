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

/** Regions whose week starts on Sunday (CLDR), for engines without `Intl.Locale#getWeekInfo`. */
const SUNDAY_REGIONS = new Set(
	'AG AS BD BR BS BT BW BZ CA CN CO DM DO ET GT GU HK HN ID IL IN JM JP KE KH KR LA MH MM MO MT MX MZ NI NP PA PE PH PK PR PT PY SA SG SV TH TT TW UM US VE VI WS YE ZA ZW'.split(' ')
);

type WeekInfoLocale = Intl.Locale & { getWeekInfo?: () => { firstDay: number }; weekInfo?: { firstDay: number } };

/**
 * First day of the week for `locale` as a `Date#getDay()` index (0 = Sunday, 1 = Monday, …).
 * Uses the engine's CLDR week data when present; otherwise Sunday for the regions above, Monday elsewhere.
 */
export function weekStart(locale: string): number {
	try {
		const loc = new Intl.Locale(locale) as WeekInfoLocale;
		const info = loc.getWeekInfo?.() ?? loc.weekInfo;
		if (info && info.firstDay >= 1 && info.firstDay <= 7) return info.firstDay % 7;
		const region = loc.maximize().region;
		return region && SUNDAY_REGIONS.has(region) ? 0 : 1;
	} catch {
		return 1;
	}
}

export interface StripDay {
	/** Local midnight. */
	time: number;
	day: number;
	past: boolean;
	today: boolean;
}

/** The 7 days of the week containing `now`, starting on `start` (a `getDay()` index). */
export function weekStrip(now: Date, start: number): StripDay[] {
	const today = new Date(now.getFullYear(), now.getMonth(), now.getDate());
	const back = (today.getDay() - start + 7) % 7;
	return Array.from({ length: 7 }, (_, i) => {
		const d = new Date(today.getFullYear(), today.getMonth(), today.getDate() - back + i);
		return { time: d.getTime(), day: d.getDate(), past: i < back, today: i === back };
	});
}
