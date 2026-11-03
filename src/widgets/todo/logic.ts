/** Pure helpers for the Todo widget: ordering, day groups, due labels, reorder. */
import type { Todo } from '#lib/ipc/index.ts';
import { dtf } from '#lib/time/index.ts';

const HOUR = 3_600_000;

/** Open items by `position`, then completed ones (newest completion first) — completed sink (DESIGN §5.9). */
export function sortTodos(todos: readonly Todo[]): Todo[] {
	const open = todos.filter((t) => !t.done).sort((a, b) => a.position - b.position || a.id - b.id);
	const done = todos
		.filter((t) => t.done)
		.sort((a, b) => (b.completedAt ?? 0) - (a.completedAt ?? 0) || a.position - b.position);
	return [...open, ...done];
}

export const openCount = (todos: readonly Todo[]): number => todos.filter((t) => !t.done).length;

/** Warning colour: open, has a due time, and it is less than an hour away (or already past). */
export const dueSoon = (t: Todo, now: number): boolean => !t.done && t.due !== null && t.due - now < HOUR;

const utcDay = (ms: number): number => {
	const d = new Date(ms);
	return Date.UTC(d.getFullYear(), d.getMonth(), d.getDate());
};

/** Whole local calendar days from `now` to `ms` (DST-safe: compares local dates, not hours). */
export const dayDiff = (ms: number, now: number): number => Math.round((utcDay(ms) - utcDay(now)) / 86_400_000);

/** "09:00" (tr, en-GB) / "9:00 AM" (en-US): the OS locale decides the hour cycle. */
export function formatTime(ms: number, locale: string): string {
	const cycle = dtf(locale, { hour: 'numeric' }).resolvedOptions().hourCycle;
	const twelve = cycle === 'h11' || cycle === 'h12';
	return dtf(locale, { hour: twelve ? 'numeric' : '2-digit', minute: '2-digit' }).format(new Date(ms));
}

export interface DueParts {
	/** today / tomorrow / yesterday get words from i18n; others come pre-formatted. */
	day: 'today' | 'tomorrow' | 'yesterday' | 'weekday' | 'date';
	/** "Fri" for this week, "5 Nov" further out. */
	dayText: string;
	time: string;
}

export function dueParts(due: number, now: number, locale: string): DueParts {
	const diff = dayDiff(due, now);
	const time = formatTime(due, locale);
	const d = new Date(due);
	if (diff === 0) return { day: 'today', dayText: '', time };
	if (diff === 1) return { day: 'tomorrow', dayText: '', time };
	if (diff === -1) return { day: 'yesterday', dayText: '', time };
	if (diff > 1 && diff < 7) return { day: 'weekday', dayText: dtf(locale, { weekday: 'short' }).format(d), time };
	const sameYear = d.getFullYear() === new Date(now).getFullYear();
	return {
		day: 'date',
		dayText: dtf(locale, sameYear ? { day: 'numeric', month: 'short' } : { day: 'numeric', month: 'short', year: 'numeric' }).format(d),
		time
	};
}

export interface DueWords {
	today: string;
	tomorrow: string;
	yesterday: string;
}

export interface DueRow {
	/** Short weekday within ±6 days, a date further out, null today. */
	day: string | null;
	/** Null for dates more than a week away (the date alone is enough at 11 px). */
	time: string | null;
}

/** Row label parts (mono 11, right): "11:00" today, "Fri 18:00" within a week, "5 Nov" later. */
export function dueRow(due: number, now: number, locale: string): DueRow {
	const diff = dayDiff(due, now);
	const time = formatTime(due, locale);
	if (diff === 0) return { day: null, time };
	if (Math.abs(diff) < 7) return { day: dtf(locale, { weekday: 'short' }).format(new Date(due)), time };
	return { day: dueParts(due, now, locale).dayText, time: null };
}

export const dueShort = (due: number, now: number, locale: string): string => {
	const r = dueRow(due, now, locale);
	return [r.day, r.time].filter(Boolean).join(' ');
};

/** Chip inside the quick-add field: always day + time ("Today 15:00", "Fri 18:00", "5 Nov 09:00"). */
export function dueChip(due: number, now: number, locale: string, words: DueWords): string {
	const p = dueParts(due, now, locale);
	const day = p.day === 'weekday' || p.day === 'date' ? p.dayText : words[p.day];
	return `${day} ${p.time}`;
}

export type GroupKey = 'overdue' | 'today' | 'tomorrow' | 'later' | 'none' | 'done' | `day:${number}`;

export interface TodoGroup {
	key: GroupKey;
	/** For `day:N` groups: a timestamp on that day, for the weekday label. */
	at?: number;
	items: Todo[];
}

/**
 * 4×4 day groups (DESIGN §5.9): Overdue · Today · Tomorrow · the next five weekdays · Later ·
 * No date · Completed. Items keep their manual order inside a group; empty groups are dropped.
 */
export function groupByDay(todos: readonly Todo[], now: number): TodoGroup[] {
	const groups = new Map<GroupKey, TodoGroup>();
	const order: GroupKey[] = ['overdue', 'today', 'tomorrow'];
	for (let i = 2; i < 7; i++) order.push(`day:${i}`);
	order.push('later', 'none', 'done');
	for (const t of sortTodos(todos)) {
		let key: GroupKey;
		let at: number | undefined;
		if (t.done) key = 'done';
		else if (t.due === null) key = 'none';
		else if (t.due < now) key = 'overdue';
		else {
			const diff = dayDiff(t.due, now);
			if (diff <= 0) key = 'today';
			else if (diff === 1) key = 'tomorrow';
			else if (diff < 7) {
				key = `day:${diff}`;
				at = t.due;
			} else key = 'later';
		}
		const g = groups.get(key) ?? { key, at, items: [] };
		g.items.push(t);
		groups.set(key, g);
	}
	return order.flatMap((k) => (groups.has(k) ? [groups.get(k)!] : []));
}

/**
 * New full id order after moving open item `id` by `delta` (−1 up, +1 down) among open items.
 * Completed items keep their relative order after the open ones. Null when the move is a no-op.
 */
export function moveBy(todos: readonly Todo[], id: number, delta: number): number[] | null {
	const sorted = sortTodos(todos);
	const open = sorted.filter((t) => !t.done);
	const from = open.findIndex((t) => t.id === id);
	if (from < 0) return null;
	const to = Math.max(0, Math.min(open.length - 1, from + delta));
	if (to === from) return null;
	return moveTo(todos, id, to);
}

/** New full id order with open item `id` placed at open index `to`. */
export function moveTo(todos: readonly Todo[], id: number, to: number): number[] | null {
	const sorted = sortTodos(todos);
	const open = sorted.filter((t) => !t.done).map((t) => t.id);
	const from = open.indexOf(id);
	if (from < 0) return null;
	const target = Math.max(0, Math.min(open.length - 1, to));
	if (target === from) return null;
	open.splice(from, 1);
	open.splice(target, 0, id);
	return [...open, ...sorted.filter((t) => t.done).map((t) => t.id)];
}

/** Applies a full id order locally (optimistic reorder). */
export function applyOrder(todos: readonly Todo[], ids: readonly number[]): Todo[] {
	const pos = new Map(ids.map((id, i) => [id, i]));
	return todos.map((t) => (pos.has(t.id) ? { ...t, position: pos.get(t.id)! } : t));
}

/** Reorder target index from a vertical drag: rows of `rowPx` (incl. gap), clamped to the list. */
export function dragTarget(from: number, dy: number, rowPx: number, count: number): number {
	if (rowPx <= 0 || count <= 0) return from;
	return Math.max(0, Math.min(count - 1, from + Math.round(dy / rowPx)));
}

/** How many rows of `rowPx` with `gapPx` between them fit into `heightPx`. */
export function rowsThatFit(heightPx: number, rowPx: number, gapPx: number): number {
	if (heightPx <= 0) return 0;
	return Math.max(0, Math.floor((heightPx + gapPx) / (rowPx + gapPx)));
}

/** Horizontal swipe on a row: past this many px to the left the delete button stays revealed. */
export const SWIPE_REVEAL_PX = 56;

/**
 * The first `cap` rows of `sorted`, except rows in `keep` (a fired reminder) are always included,
 * displacing the last ordinary rows. Order is preserved.
 */
export function pickVisible(sorted: readonly Todo[], cap: number, keep: ReadonlySet<number>): Todo[] {
	if (cap <= 0) return [];
	const must = sorted.filter((t) => keep.has(t.id)).slice(0, cap);
	const room = cap - must.length;
	const rest = sorted.filter((t) => !keep.has(t.id)).slice(0, room);
	const chosen = new Set([...must, ...rest].map((t) => t.id));
	return sorted.filter((t) => chosen.has(t.id));
}

/**
 * Drag-reorder in a visible sub-list (a day group, a fitted list): `visibleIds` is the on-screen open
 * order, `to` the drop index in it. Returns the full new id order, keeping hidden items where they were
 * relative to their neighbours. Null for a no-op.
 */
export function reorderVisible(todos: readonly Todo[], visibleIds: readonly number[], id: number, to: number): number[] | null {
	const from = visibleIds.indexOf(id);
	if (from < 0) return null;
	const target = Math.max(0, Math.min(visibleIds.length - 1, to));
	if (target === from) return null;
	const next = visibleIds.filter((x) => x !== id);
	next.splice(target, 0, id);
	const sorted = sortTodos(todos);
	const open = sorted.filter((t) => !t.done).map((t) => t.id).filter((x) => x !== id);
	// Anchor on the visible neighbour after the drop point, else the one before it.
	const after = next[target + 1];
	const before = next[target - 1];
	const idx = after !== undefined ? open.indexOf(after) : open.indexOf(before) + 1;
	if (idx < 0) return null;
	open.splice(idx, 0, id);
	return [...open, ...sorted.filter((t) => t.done).map((t) => t.id)];
}
