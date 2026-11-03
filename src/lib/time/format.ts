/**
 * Date/time helpers built on `Intl` (SPEC cross-cutting i18n: OS locale). Pure functions;
 * every formatter takes the locale explicitly so tests are deterministic. Formatter objects
 * are cached because clocks call these every second.
 */

const cache = new Map<string, Intl.DateTimeFormat>();

export function dtf(locale: string, opts: Intl.DateTimeFormatOptions): Intl.DateTimeFormat {
	const key = `${locale}|${JSON.stringify(opts)}`;
	let f = cache.get(key);
	if (!f) {
		f = new Intl.DateTimeFormat(locale, opts);
		cache.set(key, f);
	}
	return f;
}

/** IANA zone of the OS. */
export const localTimeZone = (): string => Intl.DateTimeFormat().resolvedOptions().timeZone;

export function isValidTimeZone(tz: unknown): tz is string {
	if (typeof tz !== 'string' || tz === '') return false;
	try {
		new Intl.DateTimeFormat('en', { timeZone: tz });
		return true;
	} catch {
		return false;
	}
}

/** All IANA zones the runtime knows, falling back to a short list on old engines. */
export function supportedTimeZones(): string[] {
	const fn = (Intl as { supportedValuesOf?: (key: string) => string[] }).supportedValuesOf;
	const list = fn ? fn('timeZone') : [];
	return list.length ? list : ['UTC', 'Europe/London', 'Europe/Berlin', 'Europe/Istanbul', 'America/New_York', 'Asia/Tokyo'];
}

/** "Europe/Istanbul" → "Istanbul", "America/Argentina/Buenos_Aires" → "Buenos Aires". */
export function cityFromZone(tz: string): string {
	const last = tz.split('/').pop() ?? tz;
	return last.replace(/_/g, ' ');
}

export interface ZonedParts {
	year: number;
	/** 1–12 */
	month: number;
	day: number;
	/** 0–23 */
	hour: number;
	minute: number;
	second: number;
	/** 0 = Sunday … 6 = Saturday */
	weekday: number;
}

/** Wall-clock fields of `date` in `timeZone` (OS zone when omitted). */
export function zonedParts(date: Date, timeZone?: string): ZonedParts {
	if (!timeZone) {
		return {
			year: date.getFullYear(),
			month: date.getMonth() + 1,
			day: date.getDate(),
			hour: date.getHours(),
			minute: date.getMinutes(),
			second: date.getSeconds(),
			weekday: date.getDay()
		};
	}
	const f = dtf('en-US', {
		timeZone,
		hourCycle: 'h23',
		year: 'numeric',
		month: 'numeric',
		day: 'numeric',
		hour: 'numeric',
		minute: 'numeric',
		second: 'numeric'
	});
	const p: Record<string, number> = {};
	for (const part of f.formatToParts(date)) {
		if (part.type !== 'literal') p[part.type] = Number(part.value);
	}
	const { year, month, day } = p;
	return {
		year,
		month,
		day,
		hour: p.hour % 24,
		minute: p.minute,
		second: p.second,
		weekday: new Date(Date.UTC(year, month - 1, day)).getUTCDay()
	};
}

/** ISO-8601 week number (weeks start Monday; week 1 contains the first Thursday). */
export function isoWeek(year: number, month: number, day: number): number {
	const d = new Date(Date.UTC(year, month - 1, day));
	const dow = d.getUTCDay() || 7;
	d.setUTCDate(d.getUTCDate() + 4 - dow); // Thursday of this week decides the year
	const yearStart = Date.UTC(d.getUTCFullYear(), 0, 1);
	return Math.ceil(((d.getTime() - yearStart) / 86_400_000 + 1) / 7);
}

/** 1-based day of the year. */
export function dayOfYear(year: number, month: number, day: number): number {
	return Math.round((Date.UTC(year, month - 1, day) - Date.UTC(year, 0, 1)) / 86_400_000) + 1;
}

/** Offset of `timeZone` from UTC at `date`, in minutes (Istanbul → 180). */
export function utcOffsetMinutes(date: Date, timeZone?: string): number {
	if (!timeZone) return -date.getTimezoneOffset();
	const p = zonedParts(date, timeZone);
	const wall = Date.UTC(p.year, p.month - 1, p.day, p.hour, p.minute, p.second);
	const actual = Math.floor(date.getTime() / 1000) * 1000;
	return Math.round((wall - actual) / 60_000);
}

/** 180 → "UTC+3", -330 → "UTC-5:30", 0 → "UTC". */
export function formatUtcOffset(minutes: number): string {
	if (minutes === 0) return 'UTC';
	const sign = minutes > 0 ? '+' : '-';
	const abs = Math.abs(minutes);
	const h = Math.floor(abs / 60);
	const m = abs % 60;
	return `UTC${sign}${h}${m ? `:${String(m).padStart(2, '0')}` : ''}`;
}

export interface ClockText {
	/** "09:37" / "9:37" in the locale's separator. */
	hm: string;
	/** "14" */
	seconds: string;
	/** "PM" etc. in 12-hour mode, else null. */
	period: string | null;
}

export function formatClock(
	date: Date,
	opts: { locale: string; hour12: boolean; timeZone?: string }
): ClockText {
	const f = dtf(opts.locale, {
		timeZone: opts.timeZone || undefined,
		hour: opts.hour12 ? 'numeric' : '2-digit',
		minute: '2-digit',
		second: '2-digit',
		...(opts.hour12 ? { hour12: true } : { hourCycle: 'h23' as const })
	});
	let hm = '';
	let seconds = '';
	let period: string | null = null;
	let afterMinute = false;
	for (const part of f.formatToParts(date)) {
		if (part.type === 'dayPeriod') period = part.value;
		else if (part.type === 'second') seconds = part.value;
		else if (part.type === 'hour') hm += part.value;
		else if (part.type === 'minute') {
			hm += part.value;
			afterMinute = true;
		} else if (part.type === 'literal' && hm && !afterMinute) hm += part.value;
	}
	return { hm: hm.trim(), seconds: seconds.padStart(2, '0'), period };
}

/** "Tuesday, 3 November" (shape follows the locale). */
export const formatLongDate = (date: Date, locale: string, timeZone?: string): string =>
	dtf(locale, { timeZone: timeZone || undefined, weekday: 'long', day: 'numeric', month: 'long' }).format(date);

/** "Tue 3 Nov" (shape follows the locale). */
export const formatShortDate = (date: Date, locale: string, timeZone?: string): string =>
	dtf(locale, { timeZone: timeZone || undefined, weekday: 'short', day: 'numeric', month: 'short' }).format(date);

export const formatWeekday = (date: Date, locale: string, style: 'long' | 'short' = 'long', timeZone?: string): string =>
	dtf(locale, { timeZone: timeZone || undefined, weekday: style }).format(date);

export const formatMonth = (date: Date, locale: string, style: 'long' | 'short' = 'long', timeZone?: string): string =>
	dtf(locale, { timeZone: timeZone || undefined, month: style }).format(date);

/** Whole minutes elapsed since `then` (never negative). */
export const minutesSince = (then: number, now: number): number => Math.max(0, Math.floor((now - then) / 60_000));
