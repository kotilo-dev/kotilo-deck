/**
 * Day timeline layout for the Calendar widget (DESIGN §5.4). Pure functions: positions are fractions
 * (0–1) of the visible range, snapped to 15-minute columns, so the view only multiplies by its width.
 */
import type { CalendarEvent } from '#lib/ipc/index.ts';
import { parseHm } from '#lib/theme/index.ts';
import { startOfDay } from './logic';

const MIN = 60_000;
export const COLUMN_MS = 15 * MIN;
/** Blocks shorter than this carry no label. */
export const LABEL_MIN_MS = 30 * MIN;
export const DEFAULT_FROM = '09:00';
export const DEFAULT_TO = '18:00';
/** Sub-lanes before overlapping events collapse into a "+N" block. */
export const MAX_LANES = 2;

export interface Range {
	start: number;
	end: number;
}

/** Wall-clock minute of `day` (DST-safe: built from the local date, not midnight + minutes). */
function atMinute(day: number, minute: number): number {
	const d = new Date(startOfDay(day));
	d.setHours(Math.floor(minute / 60), minute % 60, 0, 0);
	return d.getTime();
}

/**
 * Visible range on the day of `now` from "HH:MM" settings. Bad values or an empty/backwards range fall back
 * to 09:00–18:00; "24:00" is not valid input, so the latest end is 23:59 rounded up to a column (midnight).
 */
export function timelineRange(now: number, from: unknown, to: unknown): Range {
	let a = parseHm(typeof from === 'string' ? from : null);
	let b = parseHm(typeof to === 'string' ? to : null);
	if (a === null || b === null || b <= a) {
		a = parseHm(DEFAULT_FROM)!;
		b = parseHm(DEFAULT_TO)!;
	}
	const start = Math.floor(a / 15) * 15;
	const end = Math.ceil(b / 15) * 15;
	return { start: atMinute(now, start), end: end >= 24 * 60 ? atMinute(now, 0) + 24 * 60 * MIN : atMinute(now, end) };
}

const columns = (r: Range) => Math.max(1, Math.round((r.end - r.start) / COLUMN_MS));

/** Column span of [start, end) clipped to the range, as fractions; null when it lies outside. */
export function span(start: number, end: number, r: Range): { left: number; width: number } | null {
	const s = Math.max(start, r.start);
	const e = Math.min(Math.max(end, start), r.end);
	if (e <= s && !(start === end && start >= r.start && start < r.end)) return null;
	const n = columns(r);
	const c0 = Math.floor((s - r.start) / COLUMN_MS);
	const c1 = Math.max(c0 + 1, Math.ceil((e - r.start) / COLUMN_MS));
	return { left: c0 / n, width: (Math.min(n, c1) - c0) / n };
}

export interface Block {
	event: CalendarEvent;
	left: number;
	width: number;
	/** 0 = top sub-lane, 1 = bottom. */
	lane: number;
	/** Sub-lanes in this block's overlap cluster (1 = full height, 2 = half). */
	lanes: number;
	/** Long enough (≥ 30 min visible) to carry its title. */
	label: boolean;
}

export interface More {
	left: number;
	width: number;
	lane: number;
	count: number;
}

export interface TimelineLayout {
	allDay: CalendarEvent[];
	blocks: Block[];
	more: More[];
}

/**
 * Lays out today's events in `r`. Overlapping timed events form clusters; within a cluster each event takes
 * the first free sub-lane. With at most two concurrent events everything shows; beyond that the first lane
 * keeps its events and every other event of the cluster merges into one "+N" block in lane 1.
 */
export function layoutTimeline(events: readonly CalendarEvent[], r: Range, includeAllDay = true): TimelineLayout {
	const allDay = includeAllDay ? events.filter((e) => e.allDay && e.start < r.end && e.end > r.start) : [];
	const timed = events
		.filter((e) => !e.allDay && span(e.start, e.end, r) !== null)
		.sort((a, b) => a.start - b.start || b.end - a.end);

	const blocks: Block[] = [];
	const more: More[] = [];
	let i = 0;
	while (i < timed.length) {
		// Cluster: events chained by overlap.
		const cluster = [timed[i]];
		let clusterEnd = timed[i].end;
		let j = i + 1;
		while (j < timed.length && timed[j].start < clusterEnd) {
			cluster.push(timed[j]);
			clusterEnd = Math.max(clusterEnd, timed[j].end);
			j++;
		}
		i = j;

		const laneEnds: number[] = [];
		const lanesOf = cluster.map((e) => {
			let lane = laneEnds.findIndex((end) => end <= e.start);
			if (lane < 0) lane = laneEnds.length;
			laneEnds[lane] = e.end;
			return lane;
		});
		const needed = laneEnds.length;
		const lanes = Math.min(needed, MAX_LANES);

		if (needed <= MAX_LANES) {
			cluster.forEach((event, k) => blocks.push(place(event, r, lanesOf[k], lanes)));
		} else {
			const rest: CalendarEvent[] = [];
			cluster.forEach((event, k) => (lanesOf[k] === 0 ? blocks.push(place(event, r, 0, lanes)) : rest.push(event)));
			const sp = span(Math.min(...rest.map((e) => e.start)), Math.max(...rest.map((e) => e.end)), r)!;
			more.push({ ...sp, lane: 1, count: rest.length });
		}
	}
	return { allDay, blocks, more };
}

function place(event: CalendarEvent, r: Range, lane: number, lanes: number): Block {
	const sp = span(event.start, event.end, r)!;
	const visible = Math.min(event.end, r.end) - Math.max(event.start, r.start);
	return { event, ...sp, lane, lanes, label: visible >= LABEL_MIN_MS };
}

/** "Now" marker position (0–1), or null outside the range. */
export const nowAt = (now: number, r: Range): number | null => (now >= r.start && now <= r.end ? (now - r.start) / (r.end - r.start) : null);

/** Full hours inside the range (inclusive), with their position. */
export function hourTicks(r: Range): { at: number; time: number }[] {
	const out: { at: number; time: number }[] = [];
	const d = new Date(r.start);
	if (d.getMinutes() || d.getSeconds()) d.setHours(d.getHours() + 1, 0, 0, 0);
	for (let t = d.getTime(); t <= r.end; ) {
		out.push({ at: (t - r.start) / (r.end - r.start), time: t });
		const n = new Date(t);
		n.setHours(n.getHours() + 1);
		t = n.getTime();
	}
	return out;
}
