/**
 * Calendar view logic (DESIGN §5.4). Pure functions; every formatter takes the locale explicitly.
 * Events come sorted by start from `calendarUpcoming`; this module picks the "next" card, groups the
 * following rows by day and derives the countdown chip from the shared minute tick.
 */
import type { CalendarEvent, CalendarFeed, FeedColor } from '#lib/ipc/index.ts';
import { dtf } from '#lib/time/index.ts';

/** DESIGN §5.4: 4×2 shows next + 3 rows, 4×4 next + 8 rows. */
export const ROW_CAP = { '4x2': 3, '4x4': 8 } as const;
/** A timed event stays on the next card (danger chip) this long after its end, until the backend drops it. */
export const OVERRUN_KEEP_MS = 10 * 60_000;
/** Re-read `calendarUpcoming` at least this often while shown (backend cache is 15 min). */
export const RELOAD_MS = 15 * 60_000;
export const UPCOMING_LIMIT = 20;
export const UPCOMING_DAYS = 7;

const MIN = 60_000;

/** Local midnight of `ms`. */
export function startOfDay(ms: number): number {
	const d = new Date(ms);
	d.setHours(0, 0, 0, 0);
	return d.getTime();
}

/** Local midnight of the day after `ms` (DST-safe: not just +24 h). */
export function startOfNextDay(ms: number): number {
	const d = new Date(startOfDay(ms));
	d.setDate(d.getDate() + 1);
	return d.getTime();
}

/** Day the event is listed under: its start day, or today for an event that started earlier and is still on. */
export const listDay = (event: Pick<CalendarEvent, 'start'>, now: number): number => startOfDay(Math.max(event.start, startOfDay(now)));

export type Countdown =
	| { kind: 'in'; minutes: number }
	| { kind: 'now' }
	| { kind: 'over'; minutes: number }
	| { kind: 'all-day' };

/** "in 12 min" until the start (rounded up), "now" while it runs, overrun minutes after the end. */
export function countdown(event: Pick<CalendarEvent, 'start' | 'end' | 'allDay'>, now: number): Countdown {
	if (event.allDay) return { kind: 'all-day' };
	if (now < event.start) return { kind: 'in', minutes: Math.max(1, Math.ceil((event.start - now) / MIN)) };
	if (now < event.end) return { kind: 'now' };
	return { kind: 'over', minutes: Math.max(1, Math.floor((now - event.end) / MIN)) };
}

export interface Phrase {
	key: string;
	params?: Record<string, number>;
}

/** i18n key (under `widgets.calendar.`) + params for the chip. */
export function countdownPhrase(cd: Countdown): Phrase {
	switch (cd.kind) {
		case 'all-day':
			return { key: 'allDay' };
		case 'now':
			return { key: 'now' };
		case 'over':
			return { key: 'over', params: { n: cd.minutes } };
		case 'in': {
			const m = cd.minutes;
			if (m < 60) return { key: 'inMin', params: { n: m } };
			if (m < 24 * 60) {
				const h = Math.floor(m / 60);
				const rest = m % 60;
				return rest ? { key: 'inHours', params: { h, m: rest } } : { key: 'inHoursExact', params: { h } };
			}
			return { key: 'inDays', params: { n: Math.round(m / (24 * 60)) } };
		}
	}
}

/** Still worth showing at `now`: timed events until their end (+ overrun grace on the next card), all-day until their end. */
const live = (e: CalendarEvent, now: number, grace: number): boolean => (e.allDay ? e.end > now : e.end + grace > now);

export interface Agenda {
	next: CalendarEvent | null;
	rest: CalendarEvent[];
}

/**
 * Next card = the first timed event (an all-day event would sit there all day as "now"); all-day events
 * go to the rows. With no timed event left, the first all-day event takes the card.
 */
export function agenda(events: readonly CalendarEvent[], now: number, includeAllDay = true): Agenda {
	const sorted = [...events]
		.filter((e) => includeAllDay || !e.allDay)
		.sort((a, b) => a.start - b.start || Number(b.allDay) - Number(a.allDay) || a.end - b.end);
	const next = sorted.find((e) => !e.allDay && live(e, now, OVERRUN_KEEP_MS)) ?? sorted.find((e) => e.allDay && live(e, now, 0)) ?? null;
	const rest = sorted.filter((e) => e !== next && live(e, now, 0));
	return { next, rest };
}

export type Row = { kind: 'day'; day: number } | { kind: 'event'; event: CalendarEvent };

/**
 * Rows grouped by day: today's events first without a heading, later days under a heading. All-day events
 * lead their day (events are sorted by start and all-day starts at midnight). At most `cap` events.
 */
export function groupRows(events: readonly CalendarEvent[], now: number, cap: number): Row[] {
	const today = startOfDay(now);
	const byDay = new Map<number, CalendarEvent[]>();
	for (const e of events) {
		const day = listDay(e, now);
		const list = byDay.get(day);
		if (list) list.push(e);
		else byDay.set(day, [e]);
	}
	const rows: Row[] = [];
	let count = 0;
	for (const day of [...byDay.keys()].sort((a, b) => a - b)) {
		if (count >= cap) break;
		const list = byDay.get(day)!.sort((a, b) => Number(b.allDay) - Number(a.allDay) || a.start - b.start);
		if (day !== today) rows.push({ kind: 'day', day });
		for (const event of list) {
			if (count >= cap) break;
			rows.push({ kind: 'event', event });
			count++;
		}
	}
	return rows;
}

/** Keeps the leading rows that fit `heightPx`; never ends on a dangling day heading. */
export function fitRows(rows: readonly Row[], heightPx: number, rowPx: number, headPx: number, gapPx: number): Row[] {
	const out: Row[] = [];
	let used = 0;
	for (const r of rows) {
		const h = (r.kind === 'day' ? headPx : rowPx) + (out.length ? gapPx : 0);
		if (used + h > heightPx) break;
		out.push(r);
		used += h;
	}
	while (out.length && out[out.length - 1].kind === 'day') out.pop();
	return out;
}

/** "10:00 – 10:45" in the locale (Intl range format where available); null for all-day events. */
export function timeRange(event: Pick<CalendarEvent, 'start' | 'end' | 'allDay'>, locale: string): string | null {
	if (event.allDay) return null;
	const f = dtf(locale, { hour: '2-digit', minute: '2-digit' });
	const a = new Date(event.start);
	const b = new Date(Math.max(event.start, event.end));
	if (startOfDay(event.start) === startOfDay(event.end) && typeof f.formatRange === 'function') {
		return f.formatRange(a, b).replace(/\s*[–-]\s*/, ' – ');
	}
	return `${f.format(a)} – ${f.format(b)}`;
}

/** Heading phrase for a day: "today" / "tomorrow" key, else null (caller formats the weekday). */
export function dayWord(day: number, now: number): 'today' | 'tomorrow' | null {
	const today = startOfDay(now);
	if (day === today) return 'today';
	if (day === startOfNextDay(today)) return 'tomorrow';
	return null;
}

/** "Wed 4 Nov" */
export const shortDay = (day: number, locale: string): string =>
	dtf(locale, { weekday: 'short', day: 'numeric', month: 'short' }).format(new Date(day));

const COLORS: readonly FeedColor[] = ['accent', 'accent-2', 'success', 'warning', 'danger', 'info'];

/** CSS colour of a feed: a theme token only, never a hex; unknown → accent. */
export const feedColor = (color: unknown): string => `var(--k-${COLORS.includes(color as FeedColor) ? color : 'accent'})`;

export const feedById = (feeds: readonly CalendarFeed[], id: string): CalendarFeed | undefined => feeds.find((f) => f.id === id);

export const enabledFeeds = (feeds: readonly CalendarFeed[]): CalendarFeed[] => feeds.filter((f) => f.enabled !== false && !!f.url);

/** Identity of the feed list for "did it change?" checks (name/colour edits don't need a refetch). */
export const feedsKey = (feeds: readonly CalendarFeed[]): string =>
	enabledFeeds(feeds)
		.map((f) => `${f.id}|${f.url}`)
		.join('\n');
