import { describe, expect, it } from 'vitest';
import type { CalendarEvent } from '#lib/ipc/index.ts';
import { hourTicks, layoutTimeline, nowAt, span, timelineRange, type Range } from './timeline';

const at = (h: number, m = 0) => new Date(2026, 10, 5, h, m).getTime();
let n = 0;
const ev = (start: number, end: number, extra: Partial<CalendarEvent> = {}): CalendarEvent => ({
	id: `e${n++}`,
	feedId: 'work',
	title: 'x',
	start,
	end,
	allDay: false,
	location: null,
	...extra
});
const day: Range = timelineRange(at(10, 42), '09:00', '18:00');

describe('timelineRange', () => {
	it('uses the settings, falls back on bad or backwards values', () => {
		expect(day).toEqual({ start: at(9), end: at(18) });
		expect(timelineRange(at(10), '08:30', '20:00')).toEqual({ start: at(8, 30), end: at(20) });
		expect(timelineRange(at(10), 'nope', '18:00')).toEqual(day);
		expect(timelineRange(at(10), '18:00', '09:00')).toEqual(day);
		expect(timelineRange(at(10), undefined, undefined)).toEqual(day);
	});

	it('snaps to 15-minute columns and can reach midnight', () => {
		expect(timelineRange(at(10), '09:10', '17:50')).toEqual({ start: at(9), end: at(18) });
		expect(timelineRange(at(10), '22:00', '23:59').end).toBe(new Date(2026, 10, 6).getTime());
	});
});

describe('span (event → column span)', () => {
	it('positions on 15-minute columns of the range', () => {
		// 9 h = 36 columns
		expect(span(at(11), at(11, 15), day)).toEqual({ left: 8 / 36, width: 1 / 36 });
		expect(span(at(13, 30), at(14, 30), day)).toEqual({ left: 18 / 36, width: 4 / 36 });
		// a 10:05–10:20 event covers the 10:00 and 10:15 columns
		expect(span(at(10, 5), at(10, 20), day)).toEqual({ left: 4 / 36, width: 2 / 36 });
	});

	it('clips events that start before or end after the range; drops ones outside', () => {
		expect(span(at(7), at(10), day)).toEqual({ left: 0, width: 4 / 36 });
		expect(span(at(17), at(21), day)).toEqual({ left: 32 / 36, width: 4 / 36 });
		expect(span(at(7), at(9), day)).toBeNull();
		expect(span(at(18), at(19), day)).toBeNull();
		expect(span(at(12), at(12), day)).toEqual({ left: 12 / 36, width: 1 / 36 });
	});
});

describe('layoutTimeline (lanes)', () => {
	it('single events take the full height; labels only from 30 minutes', () => {
		const { blocks, more } = layoutTimeline([ev(at(11), at(11, 15)), ev(at(13, 30), at(14, 30))], day);
		expect(blocks.map((b) => [b.lane, b.lanes, b.label])).toEqual([
			[0, 1, false],
			[0, 1, true]
		]);
		expect(more).toEqual([]);
	});

	it('two overlapping events split into two sub-lanes; a later free slot reuses lane 0', () => {
		const { blocks } = layoutTimeline([ev(at(10), at(11)), ev(at(10, 30), at(12)), ev(at(11), at(11, 30))], day);
		expect(blocks.map((b) => [b.event.start, b.lane, b.lanes])).toEqual([
			[at(10), 0, 2],
			[at(10, 30), 1, 2],
			[at(11), 0, 2]
		]);
	});

	it('more than two concurrent events collapse into a +N block in lane 1', () => {
		const { blocks, more } = layoutTimeline(
			[ev(at(10), at(12)), ev(at(10), at(11)), ev(at(10, 30), at(11, 30)), ev(at(11), at(11, 45))],
			day
		);
		expect(blocks.map((b) => [b.event.start, b.event.end, b.lane])).toEqual([[at(10), at(12), 0]]);
		expect(more).toEqual([{ left: 4 / 36, width: 7 / 36, lane: 1, count: 3 }]);
	});

	it('all-day events go to the top lane unless hidden; clipped timed events keep a label by visible length', () => {
		const allDay = ev(new Date(2026, 10, 5).getTime(), new Date(2026, 10, 6).getTime(), { allDay: true });
		const early = ev(at(8), at(9, 20));
		const out = layoutTimeline([allDay, early], day);
		expect(out.allDay).toEqual([allDay]);
		expect(out.blocks[0].label).toBe(false);
		expect(layoutTimeline([allDay], day, false).allDay).toEqual([]);
	});
});

describe('now marker and hour ticks', () => {
	it('now is a fraction inside the range, null outside', () => {
		expect(nowAt(at(13, 30), day)).toBe(0.5);
		expect(nowAt(at(8, 59), day)).toBeNull();
		expect(nowAt(at(18, 1), day)).toBeNull();
	});

	it('ticks every full hour, inclusive', () => {
		const ticks = hourTicks(day);
		expect(ticks).toHaveLength(10);
		expect(ticks[0]).toEqual({ at: 0, time: at(9) });
		expect(ticks.at(-1)).toEqual({ at: 1, time: at(18) });
		expect(hourTicks({ start: at(8, 30), end: at(10) }).map((t) => t.time)).toEqual([at(9), at(10)]);
	});
});
