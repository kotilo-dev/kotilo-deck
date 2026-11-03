import { describe, expect, it } from 'vitest';
import type { CalendarFeed } from '#lib/ipc/index.ts';
import { checkFeedUrl, FEED_COLORS, feedColorVar, maskFeedUrl, moveFeed, newFeedId, nextFeedColor } from './feeds';

const feed = (id: string, color: CalendarFeed['color'] = 'accent'): CalendarFeed => ({
	id,
	name: id,
	url: `https://example.com/${id}.ics`,
	color,
	enabled: true
});

describe('calendar feeds', () => {
	it('accepts https and webcal URLs with a host', () => {
		expect(checkFeedUrl('  https://calendar.google.com/calendar/ical/x/private-abc/basic.ics ')).toEqual({
			url: 'https://calendar.google.com/calendar/ical/x/private-abc/basic.ics'
		});
		expect(checkFeedUrl('webcal://p01-caldav.icloud.com/published/2/abc')).toEqual({ url: 'webcal://p01-caldav.icloud.com/published/2/abc' });
		expect(checkFeedUrl('WEBCAL://example.com/a.ics')).toHaveProperty('url');
	});

	it('rejects empty, other schemes and malformed URLs', () => {
		expect(checkFeedUrl('  ')).toEqual({ problem: 'empty' });
		expect(checkFeedUrl('http://example.com/a.ics')).toEqual({ problem: 'scheme' });
		expect(checkFeedUrl('file:///C:/cal.ics')).toEqual({ problem: 'scheme' });
		expect(checkFeedUrl('example.com/a.ics')).toEqual({ problem: 'scheme' });
		expect(checkFeedUrl('https://')).toEqual({ problem: 'invalid' });
		expect(checkFeedUrl('webcal:example.com')).toEqual({ problem: 'invalid' });
	});

	it('masks URLs to the host', () => {
		expect(maskFeedUrl('https://calendar.google.com/calendar/ical/me%40x/private-secret/basic.ics')).toBe('calendar.google.com/…');
		expect(maskFeedUrl('webcal://user:pw@p01.icloud.com/published/2/token?x=1')).toBe('p01.icloud.com/…');
		expect(maskFeedUrl('garbage')).toBe('…');
		expect(maskFeedUrl('https://outlook.office365.com/owa/calendar/abc/reachcalendar.ics')).not.toContain('abc');
	});

	it('makes stable unique slugs', () => {
		expect(newFeedId('Work', [])).toBe('work');
		expect(newFeedId('Work', [feed('work')])).toBe('work-2');
		expect(newFeedId('İş takvimi', [])).toBe('is-takvimi');
		expect(newFeedId('💼', [])).toBe('feed');
	});

	it('picks an unused colour, then cycles', () => {
		expect(nextFeedColor([])).toBe('accent');
		expect(nextFeedColor([feed('a', 'accent'), feed('b', 'success')])).toBe('accent-2');
		const all = FEED_COLORS.map((c, i) => feed(`f${i}`, c));
		expect(FEED_COLORS).toContain(nextFeedColor(all));
		expect(feedColorVar('accent-2')).toBe('var(--k-accent-2)');
	});

	it('reorders within bounds', () => {
		const list = [feed('a'), feed('b'), feed('c')];
		expect(moveFeed(list, 'b', -1).map((f) => f.id)).toEqual(['b', 'a', 'c']);
		expect(moveFeed(list, 'c', 1).map((f) => f.id)).toEqual(['a', 'b', 'c']);
		expect(moveFeed(list, 'x', 1)).toEqual(list);
	});
});
