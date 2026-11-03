import { describe, expect, it } from 'vitest';
import type { CalendarEvent, CalendarFeed } from '#lib/ipc/index.ts';
import {
	agenda,
	countdown,
	countdownPhrase,
	dayWord,
	enabledFeeds,
	feedColor,
	feedsKey,
	fitRows,
	groupRows,
	listDay,
	OVERRUN_KEEP_MS,
	startOfDay,
	startOfNextDay,
	timeRange
} from './logic';

/** Tue 3 Nov 2026, 09:48 local. */
const NOW = new Date(2026, 10, 3, 9, 48).getTime();
const at = (day: number, h: number, m = 0) => new Date(2026, 10, day, h, m).getTime();
const midnight = (day: number) => new Date(2026, 10, day).getTime();

let n = 0;
const ev = (start: number, end: number, over: Partial<CalendarEvent> = {}): CalendarEvent => ({
	id: `work:${++n}:${start}`,
	feedId: 'work',
	title: `Event ${n}`,
	start,
	end,
	allDay: false,
	location: null,
	...over
});
const allDay = (day: number, days = 1, over: Partial<CalendarEvent> = {}) => ev(midnight(day), midnight(day + days), { allDay: true, ...over });

describe('countdown chip', () => {
	it('counts down in whole minutes (rounded up), then "now", then overrun', () => {
		const e = ev(at(3, 10), at(3, 10, 45));
		expect(countdown(e, at(3, 9, 48))).toEqual({ kind: 'in', minutes: 12 });
		expect(countdown(e, at(3, 9, 48) + 30_000)).toEqual({ kind: 'in', minutes: 12 });
		expect(countdown(e, at(3, 9, 59) + 59_000)).toEqual({ kind: 'in', minutes: 1 });
		expect(countdown(e, at(3, 10))).toEqual({ kind: 'now' });
		expect(countdown(e, at(3, 10, 44))).toEqual({ kind: 'now' });
		expect(countdown(e, at(3, 10, 45))).toEqual({ kind: 'over', minutes: 1 });
		expect(countdown(e, at(3, 10, 52))).toEqual({ kind: 'over', minutes: 7 });
		expect(countdown(allDay(3), NOW)).toEqual({ kind: 'all-day' });
	});

	it('phrases minutes, hours and days', () => {
		expect(countdownPhrase({ kind: 'in', minutes: 12 })).toEqual({ key: 'inMin', params: { n: 12 } });
		expect(countdownPhrase({ kind: 'in', minutes: 60 })).toEqual({ key: 'inHoursExact', params: { h: 1 } });
		expect(countdownPhrase({ kind: 'in', minutes: 135 })).toEqual({ key: 'inHours', params: { h: 2, m: 15 } });
		expect(countdownPhrase({ kind: 'in', minutes: 26 * 60 })).toEqual({ key: 'inDays', params: { n: 1 } });
		expect(countdownPhrase({ kind: 'now' })).toEqual({ key: 'now' });
		expect(countdownPhrase({ kind: 'over', minutes: 3 })).toEqual({ key: 'over', params: { n: 3 } });
		expect(countdownPhrase({ kind: 'all-day' })).toEqual({ key: 'allDay' });
	});
});

describe('agenda (next card + rows)', () => {
	it('puts the first timed event on the card and all-day events in the rows', () => {
		const freeze = allDay(3);
		const review = ev(at(3, 10), at(3, 10, 45));
		const dentist = ev(at(3, 13), at(3, 14));
		const { next, rest } = agenda([dentist, freeze, review], NOW);
		expect(next).toBe(review);
		expect(rest).toEqual([freeze, dentist]);
	});

	it('keeps an overrunning event on the card for a grace period, then drops it', () => {
		const standup = ev(at(3, 9, 30), at(3, 9, 45));
		const later = ev(at(3, 11), at(3, 12));
		expect(agenda([standup, later], NOW).next).toBe(standup);
		expect(agenda([standup, later], at(3, 9, 45) + OVERRUN_KEEP_MS).next).toBe(later);
		// An overrun event never shows as a row.
		const early = ev(at(3, 9), at(3, 9, 42));
		expect(agenda([later, standup, early], NOW)).toEqual({ next: early, rest: [later] });
	});

	it('falls back to an all-day event and can hide all-day events', () => {
		const freeze = allDay(3);
		expect(agenda([freeze], NOW)).toEqual({ next: freeze, rest: [] });
		expect(agenda([freeze], NOW, false)).toEqual({ next: null, rest: [] });
		expect(agenda([], NOW)).toEqual({ next: null, rest: [] });
	});
});

describe('row grouping', () => {
	it('lists today without a heading and later days under headings, all-day first', () => {
		const a = ev(at(3, 13), at(3, 14));
		const b = ev(at(4, 9), at(4, 10));
		const c = allDay(4);
		const rows = groupRows([a, c, b], NOW, 8);
		expect(rows).toEqual([
			{ kind: 'event', event: a },
			{ kind: 'day', day: midnight(4) },
			{ kind: 'event', event: c },
			{ kind: 'event', event: b }
		]);
	});

	it('lists a multi-day all-day event that started earlier under today', () => {
		const trip = allDay(1, 5);
		expect(listDay(trip, NOW)).toBe(midnight(3));
		expect(groupRows([trip], NOW, 3)).toEqual([{ kind: 'event', event: trip }]);
	});

	it('caps the number of events (headings do not count)', () => {
		const list = [ev(at(3, 11), at(3, 12)), ev(at(4, 11), at(4, 12)), ev(at(5, 11), at(5, 12)), ev(at(6, 11), at(6, 12))];
		const rows = groupRows(list, NOW, 3);
		expect(rows.filter((r) => r.kind === 'event')).toHaveLength(3);
		expect(rows.at(-1)?.kind).toBe('event');
	});

	it('fits rows into the height and never ends on a heading', () => {
		const rows = groupRows([ev(at(3, 11), at(3, 12)), ev(at(4, 11), at(4, 12))], NOW, 8);
		// event(32) + gap(4) + heading(18) = 54 → the heading would dangle.
		expect(fitRows(rows, 60, 32, 18, 4)).toEqual([rows[0]]);
		expect(fitRows(rows, 32 + 4 + 18 + 4 + 32, 32, 18, 4)).toEqual(rows);
		expect(fitRows(rows, 0, 32, 18, 4)).toEqual([]);
	});
});

describe('formatting', () => {
	it('formats time ranges with Intl and all-day as null', () => {
		expect(timeRange(ev(at(3, 10), at(3, 10, 45)), 'en-GB')).toBe('10:00 – 10:45');
		expect(timeRange(ev(at(3, 13), at(3, 14)), 'tr-TR')).toBe('13:00 – 14:00');
		expect(timeRange(ev(at(3, 23), at(4, 1)), 'en-GB')).toBe('23:00 – 01:00');
		expect(timeRange(allDay(3), 'en-GB')).toBeNull();
	});

	it('names today and tomorrow', () => {
		expect(dayWord(midnight(3), NOW)).toBe('today');
		expect(dayWord(midnight(4), NOW)).toBe('tomorrow');
		expect(dayWord(midnight(5), NOW)).toBeNull();
		expect(startOfNextDay(NOW)).toBe(midnight(4));
		expect(startOfDay(NOW)).toBe(midnight(3));
	});

	it('maps feed colours to tokens only', () => {
		expect(feedColor('accent-2')).toBe('var(--k-accent-2)');
		expect(feedColor('success')).toBe('var(--k-success)');
		expect(feedColor('red')).toBe('var(--k-accent)');
		expect(feedColor(undefined)).toBe('var(--k-accent)');
	});

	it('counts enabled feeds and keys the list by id + url', () => {
		const feeds: CalendarFeed[] = [
			{ id: 'work', name: 'Work', url: 'https://a', color: 'accent', enabled: true },
			{ id: 'home', name: 'Home', url: 'https://b', color: 'success', enabled: false }
		];
		expect(enabledFeeds(feeds).map((f) => f.id)).toEqual(['work']);
		expect(feedsKey(feeds)).toBe(feedsKey([{ ...feeds[0], name: 'Renamed', color: 'info' }, feeds[1]]));
		expect(feedsKey(feeds)).not.toBe(feedsKey([{ ...feeds[0], url: 'https://c' }]));
	});
});
