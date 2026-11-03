/**
 * Pure cell-grid placement logic (SPEC §4.2, DESIGN §4.3/§6). No DOM, no Svelte: every function takes
 * plain items and returns new arrays, so the deck, edit mode and tests share one source of truth.
 *
 * "Placed" = in bounds and not overlapping an earlier placed item (array order is priority).
 * Items that are not placed are never deleted: they stay in the workspace file untouched and the
 * shell lists them as "doesn't fit" until a density change or reflow gives them room again.
 */

export interface Size {
	w: number;
	h: number;
}
export interface Rect extends Size {
	x: number;
	y: number;
}
export interface Item extends Rect {
	instance: string;
}
export interface GridSize {
	cols: number;
	rows: number;
}

export const sizeKey = (s: Size): `${number}x${number}` => `${s.w}x${s.h}`;

export const overlaps = (a: Rect, b: Rect): boolean =>
	a.x < b.x + b.w && b.x < a.x + a.w && a.y < b.y + b.h && b.y < a.y + a.h;

export const inBounds = (r: Rect, g: GridSize): boolean =>
	Number.isInteger(r.x) &&
	Number.isInteger(r.y) &&
	r.w >= 1 &&
	r.h >= 1 &&
	r.x >= 0 &&
	r.y >= 0 &&
	r.x + r.w <= g.cols &&
	r.y + r.h <= g.rows;

/** Splits items into placed (rendered) and unplaced (kept, hidden), preserving order. */
export function layout<T extends Item>(items: readonly T[], g: GridSize): { placed: T[]; unplaced: T[] } {
	const placed: T[] = [];
	const unplaced: T[] = [];
	for (const it of items) {
		if (inBounds(it, g) && !placed.some((p) => overlaps(p, it))) placed.push(it);
		else unplaced.push(it);
	}
	return { placed, unplaced };
}

/** `rows × cols` matrix of the instance id covering each cell (null = free). Only placed items count. */
export function occupancy(items: readonly Item[], g: GridSize): (string | null)[][] {
	const cells: (string | null)[][] = Array.from({ length: g.rows }, () => Array<string | null>(g.cols).fill(null));
	for (const it of layout(items, g).placed) {
		for (let y = it.y; y < it.y + it.h; y++) for (let x = it.x; x < it.x + it.w; x++) cells[y][x] = it.instance;
	}
	return cells;
}

/** Placed items (except `ignore`) overlapping `rect`. */
export function collisions<T extends Item>(rect: Rect, items: readonly T[], g: GridSize, ignore?: string): T[] {
	return layout(items, g).placed.filter((it) => it.instance !== ignore && overlaps(it, rect));
}

/** True when `rect` is in bounds and overlaps no placed item other than `ignore`. */
export function fits(rect: Rect, items: readonly Item[], g: GridSize, ignore?: string): boolean {
	return inBounds(rect, g) && collisions(rect, items, g, ignore).length === 0;
}

/** First free top-left (row-major scan) where `size` fits, or null when there is no room. */
export function firstFreeSlot(size: Size, items: readonly Item[], g: GridSize, ignore?: string): { x: number; y: number } | null {
	for (let y = 0; y + size.h <= g.rows; y++) {
		for (let x = 0; x + size.w <= g.cols; x++) {
			if (fits({ x, y, w: size.w, h: size.h }, items, g, ignore)) return { x, y };
		}
	}
	return null;
}

/** Free slot for `size` closest (Manhattan, then row-major) to `origin`, or null. */
export function nearestFreeSlot(
	size: Size,
	items: readonly Item[],
	g: GridSize,
	origin: { x: number; y: number },
	ignore?: string
): { x: number; y: number } | null {
	let best: { x: number; y: number } | null = null;
	let bestD = Infinity;
	for (let y = 0; y + size.h <= g.rows; y++) {
		for (let x = 0; x + size.w <= g.cols; x++) {
			const d = Math.abs(x - origin.x) + Math.abs(y - origin.y);
			if (d < bestD && fits({ x, y, w: size.w, h: size.h }, items, g, ignore)) {
				best = { x, y };
				bestD = d;
			}
		}
	}
	return best;
}

/**
 * Moves `id` so its top-left lands on (x, y). Placed items in the way shift: first into the cells the
 * moved item vacated (swap), otherwise to the nearest free slot. Returns null (reject → shake) when
 * the target is out of bounds or any displaced item has nowhere to go. Unplaced items ride along.
 */
export function moveItem<T extends Item>(items: readonly T[], id: string, x: number, y: number, g: GridSize): T[] | null {
	const { placed } = layout(items, g);
	const moving = placed.find((it) => it.instance === id);
	if (!moving) return null;
	const target: Rect = { x, y, w: moving.w, h: moving.h };
	if (!inBounds(target, g)) return null;
	if (x === moving.x && y === moving.y) return [...items];

	const hits = placed.filter((it) => it.instance !== id && overlaps(it, target));
	const fixed: Item[] = placed.filter((it) => it.instance !== id && !hits.includes(it));
	fixed.push({ ...moving, ...target });

	const moved = new Map<string, Rect>([[id, target]]);
	const order = [...hits].sort((a, b) => a.y - b.y || a.x - b.x);
	for (const hit of order) {
		const swap = { x: moving.x, y: moving.y, w: hit.w, h: hit.h };
		const slot = fits(swap, fixed, g) ? swap : nearestFreeSlot(hit, fixed, g, hit);
		if (!slot) return null;
		const rect = { x: slot.x, y: slot.y, w: hit.w, h: hit.h };
		fixed.push({ ...hit, ...rect });
		moved.set(hit.instance, rect);
	}
	return items.map((it) => {
		const r = moved.get(it.instance);
		return r && placed.includes(it) ? { ...it, x: r.x, y: r.y } : it;
	});
}

/** Resizes `id` in place (top-left fixed). Null when the new rect is out of bounds or collides. */
export function resizeItem<T extends Item>(items: readonly T[], id: string, size: Size, g: GridSize): T[] | null {
	const it = items.find((i) => i.instance === id);
	if (!it) return null;
	if (!fits({ x: it.x, y: it.y, w: size.w, h: size.h }, items, g, id)) return null;
	return items.map((i) => (i.instance === id ? { ...i, w: size.w, h: size.h } : i));
}

/** Supported sizes (in manifest order) that fit at the item's top-left right now, current size included. */
export function fittingSizes(items: readonly Item[], id: string, sizes: readonly Size[], g: GridSize): Size[] {
	const it = items.find((i) => i.instance === id);
	if (!it) return [];
	return sizes.filter((s) => (s.w === it.w && s.h === it.h) || fits({ x: it.x, y: it.y, w: s.w, h: s.h }, items, g, id));
}

/** Next size after the current one in `sizes` order that fits; skips sizes that don't. Null = none. */
export function nextSize(items: readonly Item[], id: string, sizes: readonly Size[], g: GridSize): Size | null {
	const it = items.find((i) => i.instance === id);
	if (!it || sizes.length === 0) return null;
	const cur = sizes.findIndex((s) => s.w === it.w && s.h === it.h);
	for (let step = 1; step <= sizes.length; step++) {
		const s = sizes[(cur + step + sizes.length) % sizes.length];
		if (s.w === it.w && s.h === it.h) continue;
		if (fits({ x: it.x, y: it.y, w: s.w, h: s.h }, items, g, id)) return s;
	}
	return null;
}

/** Fitting supported size closest to a desired w×h (corner-handle drag). Falls back to the current size. */
export function closestSize(items: readonly Item[], id: string, sizes: readonly Size[], desired: Size, g: GridSize): Size | null {
	const options = fittingSizes(items, id, sizes, g);
	if (options.length === 0) return null;
	return options.reduce((best, s) => {
		const d = Math.abs(s.w - desired.w) + Math.abs(s.h - desired.h);
		const bd = Math.abs(best.w - desired.w) + Math.abs(best.h - desired.h);
		return d < bd ? s : best;
	});
}

/**
 * Re-lays items onto a (new) grid. Pass 1 keeps every item that still fits where it is. Pass 2 puts the
 * rest in the first free slot at their current size, then at smaller supported sizes (largest first).
 * Items with no room keep their old coordinates and are reported in `unplaced`: data is never lost.
 */
export function reflow<T extends Item>(
	items: readonly T[],
	g: GridSize,
	sizesOf: (item: T) => readonly Size[]
): { items: T[]; unplaced: string[] } {
	const { placed } = layout(items, g);
	const accepted: Item[] = [...placed];
	const result = new Map<string, T>(placed.map((p) => [p.instance, p]));
	const unplaced: string[] = [];

	for (const it of items) {
		if (result.has(it.instance)) continue;
		const area = it.w * it.h;
		const smaller = sizesOf(it)
			.filter((s) => s.w * s.h < area || (s.w * s.h === area && (s.w !== it.w || s.h !== it.h)))
			.sort((a, b) => b.w * b.h - a.w * a.h);
		let done = false;
		for (const size of [{ w: it.w, h: it.h }, ...smaller]) {
			const slot = firstFreeSlot(size, accepted, g);
			if (slot) {
				const next = { ...it, ...slot, ...size };
				accepted.push(next);
				result.set(it.instance, next);
				done = true;
				break;
			}
		}
		if (!done) unplaced.push(it.instance);
	}
	return { items: items.map((it) => result.get(it.instance) ?? it), unplaced };
}

// ── Density (SPEC §4.2: 6×3 … 12×6; default 8×4 landscape, 4×8 portrait) ─────

export const DENSITIES_LANDSCAPE: readonly GridSize[] = [
	{ cols: 6, rows: 3 },
	{ cols: 8, rows: 4 },
	{ cols: 10, rows: 5 },
	{ cols: 12, rows: 6 }
];
export const DENSITIES_PORTRAIT: readonly GridSize[] = DENSITIES_LANDSCAPE.map(({ cols, rows }) => ({ cols: rows, rows: cols }));

export const isPortrait = (width: number, height: number): boolean => height > width;

export const defaultDensity = (width: number, height: number): GridSize =>
	isPortrait(width, height) ? { cols: 4, rows: 8 } : { cols: 8, rows: 4 };

/**
 * A workspace is on the automatic density when its grid equals the default of either orientation.
 * Only those follow the monitor's orientation; any other density was picked by the user and is kept.
 */
export const isAutoDensity = (g: GridSize): boolean => (g.cols === 8 && g.rows === 4) || (g.cols === 4 && g.rows === 8);

/** Next density in the orientation's ladder (wraps). A non-ladder grid starts at the smallest. */
export function nextDensity(current: GridSize, width: number, height: number): GridSize {
	const ladder = isPortrait(width, height) ? DENSITIES_PORTRAIT : DENSITIES_LANDSCAPE;
	const i = ladder.findIndex((d) => d.cols === current.cols && d.rows === current.rows);
	return { ...ladder[(i + 1) % ladder.length] };
}

// ── Geometry (pointer → cell) ───────────────────────────────────────────────

export interface GridGeometry extends GridSize {
	/** Content box of the grid (inside padding), client px. */
	left: number;
	top: number;
	width: number;
	height: number;
	gap: number;
}

export function cellSize(geo: GridGeometry): { cw: number; ch: number } {
	return {
		cw: (geo.width - geo.gap * (geo.cols - 1)) / geo.cols,
		ch: (geo.height - geo.gap * (geo.rows - 1)) / geo.rows
	};
}

/** Cell under a client point, clamped into the grid. */
export function cellAt(px: number, py: number, geo: GridGeometry): { x: number; y: number } {
	const { cw, ch } = cellSize(geo);
	const clamp = (v: number, max: number) => Math.min(max, Math.max(0, v));
	return {
		x: clamp(Math.floor((px - geo.left + geo.gap / 2) / (cw + geo.gap)), geo.cols - 1),
		y: clamp(Math.floor((py - geo.top + geo.gap / 2) / (ch + geo.gap)), geo.rows - 1)
	};
}

/** Top-left cell for an item of `size` whose original rect was dragged by (dx, dy) px; nearest snap. */
export function snapDrag(rect: Rect, dx: number, dy: number, geo: GridGeometry): { x: number; y: number } {
	const { cw, ch } = cellSize(geo);
	const x = Math.round(rect.x + dx / (cw + geo.gap));
	const y = Math.round(rect.y + dy / (ch + geo.gap));
	return {
		x: Math.min(geo.cols - rect.w, Math.max(0, x)),
		y: Math.min(geo.rows - rect.h, Math.max(0, y))
	};
}
