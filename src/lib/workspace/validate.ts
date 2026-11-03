import type { Workspace, WidgetInstance } from '#lib/ipc/index.ts';
import { t } from '#lib/i18n/index.ts';

/** Rejected workspace JSON. `key`/`params` are the i18n message (`shell.errors.*`), already rendered into `message`. */
export class WorkspaceError extends Error {
	constructor(
		readonly key: string,
		readonly params: Record<string, string | number> = {}
	) {
		super(t(key, params));
	}
}

const SLUG = /^[a-z0-9]+(?:-[a-z0-9]+)*$/;
/** Largest grid side accepted from a file (density ladder tops out at 12). */
export const MAX_GRID = 24;

export const isSlug = (s: unknown): s is string => typeof s === 'string' && SLUG.test(s) && s.length <= 64;

/** "Mein Büro!" → "mein-buro"; empty input → "workspace". */
export function slugify(name: string): string {
	const slug = name
		.normalize('NFKD')
		.replace(/[̀-ͯ]/g, '')
		.replace(/ı/g, 'i')
		.toLowerCase()
		.replace(/[^a-z0-9]+/g, '-')
		.replace(/^-+|-+$/g, '')
		.slice(0, 48)
		.replace(/-+$/, '');
	return slug || 'workspace';
}

/** `base`, else `base-2`, `base-3`, … not in `taken`. */
export function uniqueId(base: string, taken: Iterable<string>): string {
	const set = new Set(taken);
	if (!set.has(base)) return base;
	for (let n = 2; ; n++) if (!set.has(`${base}-${n}`)) return `${base}-${n}`;
}

/** Next free `<widget>-<n>` instance id. */
export const instanceId = (widget: string, taken: Iterable<string>): string => {
	const set = new Set(taken);
	for (let n = 1; ; n++) if (!set.has(`${widget}-${n}`)) return `${widget}-${n}`;
};

const isObject = (v: unknown): v is Record<string, unknown> => typeof v === 'object' && v !== null && !Array.isArray(v);
const isInt = (v: unknown, min: number): v is number => typeof v === 'number' && Number.isInteger(v) && v >= min;

function parseWidget(raw: unknown, index: number): WidgetInstance {
	const bad = (field: string) => new WorkspaceError('shell.errors.badWidget', { index: index + 1, field });
	if (!isObject(raw)) throw bad('object');
	if (typeof raw.instance !== 'string' || raw.instance.length === 0) throw bad('instance');
	if (typeof raw.widget !== 'string' || raw.widget.length === 0) throw bad('widget');
	for (const k of ['x', 'y'] as const) if (!isInt(raw[k], 0)) throw bad(k);
	for (const k of ['w', 'h'] as const) if (!isInt(raw[k], 1)) throw bad(k);
	if (raw.settings !== undefined && !isObject(raw.settings)) throw bad('settings');
	// Unknown widget ids and unknown fields are kept as-is: never lose data from a file.
	return raw as unknown as WidgetInstance;
}

/**
 * Validates workspace JSON (from disk, localStorage or an import) and normalises the shape:
 * missing `schemaVersion` → 1, missing/invalid `id` → slug of the name, missing `theme` → null,
 * duplicate instance ids get a suffix. Throws WorkspaceError on anything it cannot repair.
 */
export function parseWorkspace(input: unknown): Workspace {
	if (!isObject(input)) throw new WorkspaceError('shell.errors.notObject');
	const v = input.schemaVersion ?? 1;
	if (v !== 1) throw new WorkspaceError('shell.errors.version', { version: String(v) });
	if (typeof input.name !== 'string' || input.name.trim().length === 0) throw new WorkspaceError('shell.errors.name');
	if (input.theme !== undefined && input.theme !== null && typeof input.theme !== 'string') {
		throw new WorkspaceError('shell.errors.theme');
	}
	const grid = input.grid;
	if (!isObject(grid) || !isInt(grid.cols, 1) || !isInt(grid.rows, 1) || grid.cols > MAX_GRID || grid.rows > MAX_GRID) {
		throw new WorkspaceError('shell.errors.grid', { max: MAX_GRID });
	}
	if (input.appearance !== undefined && !isObject(input.appearance)) throw new WorkspaceError('shell.errors.appearance');
	if (!Array.isArray(input.widgets)) throw new WorkspaceError('shell.errors.widgets');

	const seen = new Set<string>();
	const widgets = input.widgets.map((raw, i) => {
		const w = parseWidget(raw, i);
		if (!seen.has(w.instance)) {
			seen.add(w.instance);
			return w;
		}
		const fixed = { ...w, instance: uniqueId(w.instance, seen) };
		seen.add(fixed.instance);
		return fixed;
	});

	return {
		...input,
		schemaVersion: 1,
		id: isSlug(input.id) ? input.id : slugify(input.name),
		name: input.name.trim(),
		theme: (input.theme as string | null | undefined) ?? null,
		grid: { cols: grid.cols, rows: grid.rows },
		widgets
	} as Workspace;
}
