/**
 * Todo data access. In the app the backend owns the list (SQLite, reminders fire with the UI hidden);
 * in the browser preview a localStorage-backed stand-in with the same surface keeps the widget usable.
 * Every widget instance shows the same list: both implementations broadcast `changed` to all of them.
 */
import {
	hasBackend,
	on,
	todoAdd,
	todoDelete,
	todoList,
	todoReorder,
	todoSnooze,
	todoUpdate,
	type Todo,
	type TodoPatch
} from '#lib/ipc/index.ts';

export interface TodoApi {
	/** False in the browser preview (data lives in this browser's localStorage only). */
	readonly persistent: boolean;
	list(): Promise<Todo[]>;
	add(text: string, due: number | null, remindAt: number | null): Promise<Todo>;
	update(id: number, patch: TodoPatch): Promise<Todo>;
	remove(id: number): Promise<void>;
	/** Full new order of ids. */
	reorder(ids: number[]): Promise<void>;
	snooze(id: number, minutes: number): Promise<Todo>;
	/** Any row changed (this or another instance/window, or the reminder scheduler). */
	onChanged(cb: () => void): () => void;
	/** A reminder fired; the OS notification is the backend's job. */
	onReminder(cb: (id: number) => void): () => void;
}

/** Wraps the async `on()` so callers get a synchronous unsubscribe. */
function listen<T>(subscribe: (cb: (p: T) => void) => Promise<() => void>, cb: (p: T) => void): () => void {
	let disposed = false;
	let off: (() => void) | null = null;
	subscribe(cb).then(
		(u) => (disposed ? u() : (off = u)),
		() => {}
	);
	return () => {
		disposed = true;
		off?.();
	};
}

export function createTauriTodoApi(): TodoApi {
	return {
		persistent: true,
		list: () => todoList(),
		add: (text, due, remindAt) => todoAdd(text, due, remindAt),
		update: (id, patch) => todoUpdate(id, patch),
		remove: (id) => todoDelete(id),
		reorder: (ids) => todoReorder(ids),
		snooze: (id, minutes) => todoSnooze(id, minutes),
		onChanged: (cb) => listen<Record<string, never>>((h) => on('todo://changed', h), () => cb()),
		onReminder: (cb) => listen<{ id: number }>((h) => on('todo://reminder', h), (p) => cb(p.id))
	};
}

type StorageLike = Pick<Storage, 'getItem' | 'setItem'>;

export const LOCAL_KEY = 'kotilo-deck:todos';

interface LocalFile {
	schemaVersion: 1;
	nextId: number;
	todos: Todo[];
}

const isTodo = (t: unknown): t is Todo => {
	if (!t || typeof t !== 'object') return false;
	const o = t as Record<string, unknown>;
	return typeof o.id === 'number' && typeof o.text === 'string' && typeof o.done === 'boolean' && typeof o.position === 'number';
};

/**
 * Browser-preview store. Mirrors the backend contract closely enough to exercise the UI: reminders
 * fire from a single timer (only while someone listens for them) and are cleared once fired.
 */
export function createLocalTodoApi(storage: StorageLike | null, now: () => number = Date.now): TodoApi {
	let file: LocalFile = { schemaVersion: 1, nextId: 1, todos: [] };
	try {
		const raw = storage?.getItem(LOCAL_KEY);
		const parsed = raw ? (JSON.parse(raw) as Partial<LocalFile>) : null;
		if (parsed && parsed.schemaVersion === 1 && Array.isArray(parsed.todos)) {
			const todos = parsed.todos.filter(isTodo);
			file = { schemaVersion: 1, todos, nextId: Math.max(Number(parsed.nextId) || 1, ...todos.map((t) => t.id + 1)) };
		}
	} catch {
		// Corrupt preview data: start empty.
	}

	const changed = new Set<() => void>();
	const reminders = new Set<(id: number) => void>();
	let timer: ReturnType<typeof setTimeout> | null = null;

	const clone = (t: Todo): Todo => ({ ...t });
	const find = (id: number) => {
		const t = file.todos.find((x) => x.id === id);
		if (!t) throw new Error(`todo ${id} not found`);
		return t;
	};

	function armReminders() {
		if (timer !== null) clearTimeout(timer);
		timer = null;
		if (reminders.size === 0) return;
		const next = file.todos
			.filter((t) => !t.done && t.remindAt !== null)
			.reduce<Todo | null>((a, t) => (!a || t.remindAt! < a.remindAt! ? t : a), null);
		if (!next) return;
		// setTimeout caps at ~24.8 days; re-arm in steps.
		const delay = Math.min(2 ** 31 - 1, Math.max(0, next.remindAt! - now()));
		timer = setTimeout(fireDue, delay);
	}

	function fireDue() {
		timer = null;
		const t = now();
		const due = file.todos.filter((x) => !x.done && x.remindAt !== null && x.remindAt <= t);
		for (const x of due) x.remindAt = null;
		if (due.length) commit();
		for (const x of due) for (const cb of [...reminders]) cb(x.id);
		armReminders();
	}

	function commit() {
		try {
			storage?.setItem(LOCAL_KEY, JSON.stringify(file));
		} catch {
			// Quota / private mode: keep it in memory.
		}
		armReminders();
		for (const cb of [...changed]) cb();
	}

	return {
		persistent: false,
		list: async () => file.todos.map(clone),
		async add(text, due, remindAt) {
			const position = file.todos.reduce((m, t) => Math.max(m, t.position), -1) + 1;
			const todo: Todo = { id: file.nextId++, text, done: false, due, remindAt, position, createdAt: now(), completedAt: null };
			file.todos.push(todo);
			commit();
			return clone(todo);
		},
		async update(id, patch) {
			const t = find(id);
			if (patch.text !== undefined) t.text = patch.text;
			if (patch.due !== undefined) t.due = patch.due;
			if (patch.remindAt !== undefined) t.remindAt = patch.remindAt;
			if (patch.done !== undefined && patch.done !== t.done) {
				t.done = patch.done;
				t.completedAt = patch.done ? now() : null;
			}
			commit();
			return clone(t);
		},
		async remove(id) {
			file.todos = file.todos.filter((t) => t.id !== id);
			commit();
		},
		async reorder(ids) {
			ids.forEach((id, i) => {
				const t = file.todos.find((x) => x.id === id);
				if (t) t.position = i;
			});
			commit();
		},
		async snooze(id, minutes) {
			const t = find(id);
			t.remindAt = now() + minutes * 60_000;
			commit();
			return clone(t);
		},
		onChanged(cb) {
			changed.add(cb);
			return () => changed.delete(cb);
		},
		onReminder(cb) {
			reminders.add(cb);
			armReminders();
			return () => {
				reminders.delete(cb);
				armReminders();
			};
		}
	};
}

let shared: TodoApi | null = null;

/** One store per page, shared by every Todo instance. */
export function todoApi(): TodoApi {
	if (!shared) {
		shared = hasBackend()
			? createTauriTodoApi()
			: createLocalTodoApi(typeof localStorage === 'undefined' ? null : localStorage);
	}
	return shared;
}
