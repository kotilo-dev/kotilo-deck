import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { Todo } from '#lib/ipc/index.ts';

const ipc = vi.hoisted(() => ({
	handlers: {} as Record<string, (p: unknown) => void>,
	todoList: vi.fn(),
	todoAdd: vi.fn(),
	todoUpdate: vi.fn(),
	todoDelete: vi.fn(),
	todoReorder: vi.fn(),
	todoSnooze: vi.fn()
}));
vi.mock('#lib/ipc/index.ts', () => ({
	isTauri: () => false,
	on: (event: string, h: (p: unknown) => void) => {
		ipc.handlers[event] = h;
		return Promise.resolve(() => delete ipc.handlers[event]);
	},
	todoList: ipc.todoList,
	todoAdd: ipc.todoAdd,
	todoUpdate: ipc.todoUpdate,
	todoDelete: ipc.todoDelete,
	todoReorder: ipc.todoReorder,
	todoSnooze: ipc.todoSnooze
}));

import {
	applyOrder,
	dayDiff,
	dragTarget,
	dueChip,
	dueShort,
	dueSoon,
	groupByDay,
	moveBy,
	moveTo,
	openCount,
	pickVisible,
	reorderVisible,
	rowsThatFit,
	sortTodos
} from './logic';
import { createLocalTodoApi, createTauriTodoApi, LOCAL_KEY } from './store';

const NOW = new Date(2026, 10, 3, 10, 0).getTime(); // Tue 3 Nov 2026 10:00
const at = (mo: number, d: number, h: number, m = 0) => new Date(2026, mo - 1, d, h, m).getTime();
const words = { today: 'Today', tomorrow: 'Tomorrow', yesterday: 'Yesterday' };

let seq = 0;
const todo = (over: Partial<Todo> = {}): Todo => {
	seq++;
	return { id: seq, text: `t${seq}`, done: false, due: null, remindAt: null, position: seq, createdAt: 0, completedAt: null, ...over };
};
beforeEach(() => (seq = 0));

describe('ordering', () => {
	it('keeps open items by position and sinks completed ones (newest completion first)', () => {
		const list = [
			todo({ position: 3 }),
			todo({ done: true, completedAt: 5, position: 0 }),
			todo({ position: 1 }),
			todo({ done: true, completedAt: 9, position: 9 })
		];
		expect(sortTodos(list).map((t) => t.id)).toEqual([3, 1, 4, 2]);
		expect(openCount(list)).toBe(2);
	});

	it('moves an open item and keeps completed ones last', () => {
		const list = [todo(), todo(), todo(), todo({ done: true, completedAt: 1 })];
		expect(moveBy(list, 3, -1)).toEqual([1, 3, 2, 4]);
		expect(moveBy(list, 1, -1)).toBeNull(); // already first
		expect(moveBy(list, 3, 1)).toBeNull(); // already last open
		expect(moveBy(list, 4, -1)).toBeNull(); // completed items don't move
		expect(moveTo(list, 1, 99)).toEqual([2, 3, 1, 4]);
		expect(sortTodos(applyOrder(list, [2, 3, 1, 4])).map((t) => t.id)).toEqual([2, 3, 1, 4]);
	});

	it('maps a drag distance to a row index', () => {
		expect(dragTarget(1, 0, 40, 5)).toBe(1);
		expect(dragTarget(1, 61, 40, 5)).toBe(3);
		expect(dragTarget(1, -500, 40, 5)).toBe(0);
		expect(dragTarget(1, 500, 40, 5)).toBe(4);
		expect(dragTarget(2, 30, 0, 5)).toBe(2);
	});

	it('counts the rows that fit', () => {
		expect(rowsThatFit(134, 36, 4)).toBe(3);
		expect(rowsThatFit(156, 36, 4)).toBe(4);
		expect(rowsThatFit(0, 36, 4)).toBe(0);
	});
});

describe('due labels', () => {
	it('flags due within an hour or overdue, not done ones', () => {
		expect(dueSoon(todo({ due: at(11, 3, 10, 59) }), NOW)).toBe(true);
		expect(dueSoon(todo({ due: at(11, 3, 11, 0) }), NOW)).toBe(false);
		expect(dueSoon(todo({ due: at(11, 3, 9) }), NOW)).toBe(true);
		expect(dueSoon(todo({ due: at(11, 3, 10, 30), done: true }), NOW)).toBe(false);
		expect(dueSoon(todo(), NOW)).toBe(false);
	});

	it('formats rows and the quick-add chip', () => {
		expect(dueShort(at(11, 3, 16), NOW, 'en-GB')).toBe('16:00');
		expect(dueShort(at(11, 3, 16), NOW, 'en-US')).toBe('4:00 PM');
		expect(dueShort(at(11, 4, 9), NOW, 'en-GB')).toBe('Wed 09:00');
		expect(dueShort(at(11, 2, 9), NOW, 'en-GB')).toBe('Mon 09:00');
		expect(dueShort(at(11, 6, 18), NOW, 'tr')).toBe('Cum 18:00');
		expect(dueShort(at(11, 20, 9), NOW, 'en-GB')).toBe('20 Nov');
		expect(dueShort(new Date(2027, 0, 2, 9).getTime(), NOW, 'en-GB')).toBe('2 Jan 2027');
		expect(dueChip(at(11, 4, 9), NOW, 'en-GB', words)).toBe('Tomorrow 09:00');
		expect(dueChip(at(11, 3, 15), NOW, 'en-GB', words)).toBe('Today 15:00');
		expect(dueChip(at(11, 20, 9), NOW, 'en-GB', words)).toBe('20 Nov 09:00');
		expect(dueChip(at(11, 6, 18), NOW, 'tr', { today: 'Bugün', tomorrow: 'Yarın', yesterday: 'Dün' })).toBe('Cum 18:00');
	});

	it('counts calendar days, not 24 h blocks', () => {
		expect(dayDiff(at(11, 4, 0, 5), at(11, 3, 23, 55))).toBe(1);
		expect(dayDiff(at(11, 3, 0, 0), at(11, 3, 23, 59))).toBe(0);
	});
});

describe('groupByDay', () => {
	it('groups in a fixed order and drops empty groups', () => {
		const list = [
			todo({ due: at(11, 10, 9) }), // 1 later
			todo({ due: at(11, 3, 9) }), // 2 overdue
			todo({ due: at(11, 3, 15) }), // 3 today
			todo(), // 4 none
			todo({ due: at(11, 4, 9) }), // 5 tomorrow
			todo({ due: at(11, 6, 9) }), // 6 day:3 (Fri)
			todo({ done: true, completedAt: 1, due: at(11, 3, 15) }) // 7 done
		];
		const g = groupByDay(list, NOW);
		expect(g.map((x) => x.key)).toEqual(['overdue', 'today', 'tomorrow', 'day:3', 'later', 'none', 'done']);
		expect(g.map((x) => x.items.map((t) => t.id))).toEqual([[2], [3], [5], [6], [1], [4], [7]]);
		expect(g[3].at).toBe(at(11, 6, 9));
	});
});

describe('local (browser preview) store', () => {
	const mem = () => {
		const data = new Map<string, string>();
		return { getItem: (k: string) => data.get(k) ?? null, setItem: (k: string, v: string) => void data.set(k, v), data };
	};

	beforeEach(() => vi.useFakeTimers({ now: NOW }));
	afterEach(() => vi.useRealTimers());

	it('adds, completes, reorders, deletes and persists', async () => {
		const storage = mem();
		const api = createLocalTodoApi(storage);
		const changed = vi.fn();
		api.onChanged(changed);
		const a = await api.add('a', null, null);
		const b = await api.add('b', at(11, 3, 12), at(11, 3, 12));
		expect([a.id, b.id, a.position, b.position]).toEqual([1, 2, 0, 1]);
		const done = await api.update(a.id, { done: true });
		expect(done.completedAt).toBe(NOW);
		await api.reorder([2, 1]);
		await api.remove(1);
		expect(changed).toHaveBeenCalledTimes(5);

		const reopened = createLocalTodoApi(storage);
		const list = await reopened.list();
		expect(list).toHaveLength(1);
		expect(list[0]).toMatchObject({ id: 2, text: 'b', position: 0 });
		expect((await reopened.add('c', null, null)).id).toBe(3); // ids never reused
	});

	it('fires reminders once, clears remindAt, and snoozes', async () => {
		const api = createLocalTodoApi(null);
		const fired = vi.fn();
		const off = api.onReminder(fired);
		const t = await api.add('x', NOW + 60_000, NOW + 60_000);
		vi.advanceTimersByTime(59_000);
		expect(fired).not.toHaveBeenCalled();
		vi.advanceTimersByTime(1_000);
		expect(fired).toHaveBeenCalledWith(t.id);
		expect((await api.list())[0].remindAt).toBeNull();

		const s = await api.snooze(t.id, 10);
		expect(s.remindAt).toBe(NOW + 60_000 + 10 * 60_000);
		vi.advanceTimersByTime(10 * 60_000);
		expect(fired).toHaveBeenCalledTimes(2);
		off();
		expect(vi.getTimerCount()).toBe(0);
	});

	it('ignores corrupt storage', async () => {
		const storage = mem();
		storage.setItem(LOCAL_KEY, '{nope');
		expect(await createLocalTodoApi(storage).list()).toEqual([]);
		storage.setItem(LOCAL_KEY, JSON.stringify({ schemaVersion: 1, nextId: 1, todos: [{ id: 'x' }, todo({ id: 7 })] }));
		const api = createLocalTodoApi(storage);
		expect((await api.list()).map((t) => t.id)).toEqual([7]);
		expect((await api.add('n', null, null)).id).toBe(8);
	});
});

describe('tauri store', () => {
	it('maps calls onto the IPC contract and forwards events', async () => {
		const api = createTauriTodoApi();
		ipc.todoAdd.mockResolvedValue(todo());
		await api.add('milk', 5, 5);
		expect(ipc.todoAdd).toHaveBeenCalledWith('milk', 5, 5);
		await api.snooze(3, 10);
		expect(ipc.todoSnooze).toHaveBeenCalledWith(3, 10);
		await api.reorder([2, 1]);
		expect(ipc.todoReorder).toHaveBeenCalledWith([2, 1]);

		const reminder = vi.fn();
		const off = api.onReminder(reminder);
		await Promise.resolve();
		ipc.handlers['todo://reminder']({ id: 4 });
		expect(reminder).toHaveBeenCalledWith(4);
		off();
		expect(ipc.handlers['todo://reminder']).toBeUndefined();
	});
});

describe('visible rows', () => {
	it('keeps rows with a fired reminder on screen', () => {
		const list = [todo(), todo(), todo(), todo(), todo()];
		expect(pickVisible(list, 3, new Set()).map((t) => t.id)).toEqual([1, 2, 3]);
		expect(pickVisible(list, 3, new Set([5])).map((t) => t.id)).toEqual([1, 2, 5]);
		expect(pickVisible(list, 0, new Set([5]))).toEqual([]);
	});

	it('reorders inside a visible sub-list and keeps hidden items in place', () => {
		// open order 1..5; a group shows only 2, 4, 5
		const list = [todo(), todo(), todo(), todo(), todo(), todo({ done: true, completedAt: 1 })];
		expect(reorderVisible(list, [2, 4, 5], 5, 0)).toEqual([1, 5, 2, 3, 4, 6]);
		expect(reorderVisible(list, [2, 4, 5], 2, 2)).toEqual([1, 3, 4, 5, 2, 6]);
		expect(reorderVisible(list, [2, 4, 5], 4, 1)).toBeNull();
		expect(reorderVisible(list, [2, 4, 5], 9, 0)).toBeNull();
	});
});
