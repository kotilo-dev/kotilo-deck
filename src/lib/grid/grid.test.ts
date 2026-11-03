import { describe, expect, it } from 'vitest';
import {
	cellAt,
	closestSize,
	collisions,
	defaultDensity,
	firstFreeSlot,
	fits,
	fittingSizes,
	inBounds,
	isAutoDensity,
	layout,
	moveItem,
	nearestFreeSlot,
	nextDensity,
	nextSize,
	occupancy,
	overlaps,
	reflow,
	resizeItem,
	snapDrag,
	type GridGeometry,
	type Item
} from './index';

const G = { cols: 8, rows: 4 };
const item = (instance: string, x: number, y: number, w: number, h: number): Item => ({ instance, x, y, w, h });
const pos = (items: Item[] | null) => Object.fromEntries((items ?? []).map((i) => [i.instance, [i.x, i.y, i.w, i.h]]));

describe('rects', () => {
	it('detects overlap but not edge contact', () => {
		expect(overlaps(item('a', 0, 0, 2, 2), item('b', 1, 1, 2, 2))).toBe(true);
		expect(overlaps(item('a', 0, 0, 2, 2), item('b', 2, 0, 2, 2))).toBe(false);
		expect(overlaps(item('a', 0, 0, 2, 2), item('b', 0, 2, 2, 2))).toBe(false);
	});

	it('checks bounds', () => {
		expect(inBounds(item('a', 6, 2, 2, 2), G)).toBe(true);
		expect(inBounds(item('a', 7, 2, 2, 2), G)).toBe(false);
		expect(inBounds(item('a', -1, 0, 2, 2), G)).toBe(false);
		expect(inBounds(item('a', 0.5, 0, 2, 2), G)).toBe(false);
		expect(inBounds(item('a', 0, 0, 0, 2), G)).toBe(false);
	});
});

describe('layout / occupancy', () => {
	it('keeps out-of-bounds and overlapping items aside, earlier items win', () => {
		const items = [item('a', 0, 0, 4, 2), item('b', 2, 1, 2, 2), item('c', 7, 0, 2, 1), item('d', 4, 0, 2, 2)];
		const { placed, unplaced } = layout(items, G);
		expect(placed.map((i) => i.instance)).toEqual(['a', 'd']);
		expect(unplaced.map((i) => i.instance)).toEqual(['b', 'c']);
	});

	it('maps every cell to its instance', () => {
		const cells = occupancy([item('a', 0, 0, 2, 1), item('b', 7, 3, 1, 1)], G);
		expect(cells).toHaveLength(4);
		expect(cells[0].slice(0, 3)).toEqual(['a', 'a', null]);
		expect(cells[3][7]).toBe('b');
		expect(cells.flat().filter(Boolean)).toHaveLength(3);
	});
});

describe('fits / collisions', () => {
	const items = [item('a', 0, 0, 4, 2), item('b', 4, 0, 2, 2)];
	it('reports colliding items and ignores self', () => {
		expect(collisions(item('x', 3, 1, 2, 2), items, G).map((i) => i.instance)).toEqual(['a', 'b']);
		expect(fits(item('a', 0, 0, 4, 2), items, G, 'a')).toBe(true);
		expect(fits(item('x', 6, 0, 2, 2), items, G)).toBe(true);
		expect(fits(item('x', 6, 3, 2, 2), items, G)).toBe(false);
	});
});

describe('free slots', () => {
	const items = [item('a', 0, 0, 4, 2), item('b', 4, 0, 2, 2)];
	it('finds the first row-major slot', () => {
		expect(firstFreeSlot({ w: 2, h: 2 }, items, G)).toEqual({ x: 6, y: 0 });
		expect(firstFreeSlot({ w: 4, h: 2 }, items, G)).toEqual({ x: 0, y: 2 });
		expect(firstFreeSlot({ w: 9, h: 1 }, items, G)).toBeNull();
	});

	it('returns null when the grid is full', () => {
		const full = [item('a', 0, 0, 4, 4), item('b', 4, 0, 4, 4)];
		expect(firstFreeSlot({ w: 1, h: 1 }, full, G)).toBeNull();
	});

	it('finds the nearest slot to an origin', () => {
		expect(nearestFreeSlot({ w: 2, h: 1 }, items, G, { x: 5, y: 3 })).toEqual({ x: 5, y: 3 });
		// (6,0) and (4,2) are both 2 away; row-major order breaks the tie.
		expect(nearestFreeSlot({ w: 2, h: 2 }, items, G, { x: 4, y: 0 })).toEqual({ x: 6, y: 0 });
		expect(nearestFreeSlot(item('b', 4, 0, 2, 2), items, G, { x: 4, y: 0 }, 'b')).toEqual({ x: 4, y: 0 });
	});
});

describe('moveItem', () => {
	it('moves into free cells', () => {
		const items = [item('a', 0, 0, 2, 2), item('b', 4, 0, 2, 2)];
		expect(pos(moveItem(items, 'a', 6, 2, G))).toEqual({ a: [6, 2, 2, 2], b: [4, 0, 2, 2] });
	});

	it('rejects out-of-bounds targets and unknown ids', () => {
		const items = [item('a', 0, 0, 2, 2)];
		expect(moveItem(items, 'a', 7, 0, G)).toBeNull();
		expect(moveItem(items, 'zzz', 0, 0, G)).toBeNull();
	});

	it('swaps a same-size neighbour into the vacated cells', () => {
		const items = [item('a', 0, 0, 2, 2), item('b', 2, 0, 2, 2)];
		expect(pos(moveItem(items, 'a', 2, 0, G))).toEqual({ a: [2, 0, 2, 2], b: [0, 0, 2, 2] });
	});

	it('shifts a neighbour to the nearest free slot when the swap does not fit', () => {
		const items = [item('a', 0, 0, 2, 1), item('b', 2, 0, 2, 2)];
		const out = moveItem(items, 'a', 2, 0, G);
		expect(out).not.toBeNull();
		const b = out!.find((i) => i.instance === 'b')!;
		expect(overlaps(b, out!.find((i) => i.instance === 'a')!)).toBe(false);
		expect(layout(out!, G).unplaced).toEqual([]);
		expect([b.w, b.h]).toEqual([2, 2]);
	});

	it('rejects when displaced widgets have no room', () => {
		const items = [item('a', 0, 0, 4, 4), item('b', 4, 0, 4, 4)];
		expect(moveItem(items, 'a', 2, 0, G)).toBeNull();
	});

	it('displaces several items and keeps the grid collision-free', () => {
		const items = [item('big', 0, 0, 4, 2), item('s1', 4, 0, 2, 1), item('s2', 4, 1, 2, 1), item('s3', 6, 0, 2, 2)];
		const out = moveItem(items, 'big', 4, 0, G)!;
		expect(out).not.toBeNull();
		expect(layout(out, G).unplaced).toEqual([]);
		expect(out.find((i) => i.instance === 'big')).toMatchObject({ x: 4, y: 0 });
	});

	it('keeps settings and unplaced items untouched', () => {
		const items = [{ ...item('a', 0, 0, 2, 2), settings: { s: 1 } }, item('lost', 9, 9, 2, 2)];
		const out = moveItem(items, 'a', 2, 2, G)!;
		expect(out[0]).toMatchObject({ x: 2, y: 2, settings: { s: 1 } });
		expect(out[1]).toEqual(items[1]);
	});

	it('is a no-op for the same cell', () => {
		const items = [item('a', 1, 1, 2, 2)];
		expect(moveItem(items, 'a', 1, 1, G)).toEqual(items);
	});
});

describe('sizes', () => {
	const sizes = [
		{ w: 2, h: 1 },
		{ w: 2, h: 2 },
		{ w: 4, h: 2 }
	];

	it('cycles through supported sizes in manifest order', () => {
		const items = [item('c', 0, 0, 2, 1)];
		expect(nextSize(items, 'c', sizes, G)).toEqual({ w: 2, h: 2 });
		expect(nextSize([item('c', 0, 0, 2, 2)], 'c', sizes, G)).toEqual({ w: 4, h: 2 });
		expect(nextSize([item('c', 0, 0, 4, 2)], 'c', sizes, G)).toEqual({ w: 2, h: 1 });
	});

	it('skips sizes that do not fit', () => {
		const items = [item('c', 0, 0, 2, 1), item('x', 2, 0, 2, 2)];
		expect(nextSize(items, 'c', sizes, G)).toEqual({ w: 2, h: 2 });
		const blocked = [item('c', 0, 0, 2, 1), item('x', 0, 1, 8, 1)];
		expect(nextSize(blocked, 'c', sizes, G)).toBeNull();
		expect(fittingSizes(blocked, 'c', sizes, G)).toEqual([{ w: 2, h: 1 }]);
	});

	it('skips sizes that would leave the grid', () => {
		expect(nextSize([item('c', 6, 3, 2, 1)], 'c', sizes, G)).toBeNull();
	});

	it('starts from the first size when the current one is not supported', () => {
		expect(nextSize([item('c', 0, 0, 3, 3)], 'c', sizes, G)).toEqual({ w: 2, h: 1 });
	});

	it('resizes in place or rejects', () => {
		const items = [item('c', 0, 0, 2, 1), item('x', 2, 0, 2, 2)];
		expect(pos(resizeItem(items, 'c', { w: 2, h: 2 }, G))).toMatchObject({ c: [0, 0, 2, 2] });
		expect(resizeItem(items, 'c', { w: 4, h: 2 }, G)).toBeNull();
	});

	it('picks the closest fitting size for a handle drag', () => {
		const items = [item('c', 0, 0, 2, 1)];
		expect(closestSize(items, 'c', sizes, { w: 5, h: 3 }, G)).toEqual({ w: 4, h: 2 });
		expect(closestSize(items, 'c', sizes, { w: 2, h: 2 }, G)).toEqual({ w: 2, h: 2 });
		expect(closestSize(items, 'c', sizes, { w: 1, h: 1 }, G)).toEqual({ w: 2, h: 1 });
	});
});

describe('reflow', () => {
	const sizesOf = (i: Item) =>
		i.instance.startsWith('clock')
			? [
					{ w: 2, h: 1 },
					{ w: 2, h: 2 },
					{ w: 4, h: 2 }
				]
			: [{ w: i.w, h: i.h }];

	it('keeps items that still fit and relocates the rest', () => {
		const items = [item('a', 0, 0, 2, 2), item('b', 6, 2, 2, 2)];
		const out = reflow(items, { cols: 6, rows: 3 }, sizesOf);
		expect(out.unplaced).toEqual([]);
		expect(pos(out.items)).toEqual({ a: [0, 0, 2, 2], b: [2, 0, 2, 2] });
	});

	it('shrinks to a smaller supported size when the current one has no room', () => {
		const items = [item('x', 0, 0, 6, 2), item('clock', 0, 2, 4, 2)];
		const out = reflow(items, { cols: 6, rows: 3 }, sizesOf);
		expect(out.unplaced).toEqual([]);
		expect(pos(out.items).clock).toEqual([0, 2, 2, 1]);
	});

	it('reports items with no room and keeps their data', () => {
		const items = [item('x', 0, 0, 6, 3), { ...item('y', 6, 0, 2, 2), settings: { keep: true } }];
		const out = reflow(items, { cols: 6, rows: 3 }, sizesOf);
		expect(out.unplaced).toEqual(['y']);
		expect(out.items[1]).toEqual(items[1]);
		expect(out.items).toHaveLength(2);
	});

	it('brings unplaced items back when the grid grows', () => {
		const items = [item('x', 0, 0, 6, 3), item('y', 6, 0, 2, 2)];
		const out = reflow(items, { cols: 8, rows: 4 }, sizesOf);
		expect(out.unplaced).toEqual([]);
		expect(pos(out.items).y).toEqual([6, 0, 2, 2]);
	});

	it('swaps orientation 8×4 → 4×8', () => {
		const items = [item('a', 0, 0, 4, 2), item('b', 4, 0, 4, 2), item('c', 0, 2, 2, 2)];
		const out = reflow(items, { cols: 4, rows: 8 }, sizesOf);
		expect(out.unplaced).toEqual([]);
		expect(layout(out.items, { cols: 4, rows: 8 }).unplaced).toEqual([]);
	});
});

describe('density', () => {
	it('derives the default from the window aspect', () => {
		expect(defaultDensity(1920, 1080)).toEqual({ cols: 8, rows: 4 });
		expect(defaultDensity(1024, 600)).toEqual({ cols: 8, rows: 4 });
		expect(defaultDensity(600, 1024)).toEqual({ cols: 4, rows: 8 });
	});

	it('cycles 6×3 → 8×4 → 10×5 → 12×6 → 6×3', () => {
		let g = { cols: 6, rows: 3 };
		const seen = [];
		for (let i = 0; i < 4; i++) {
			g = nextDensity(g, 1280, 720);
			seen.push(`${g.cols}x${g.rows}`);
		}
		expect(seen).toEqual(['8x4', '10x5', '12x6', '6x3']);
		expect(nextDensity({ cols: 4, rows: 8 }, 600, 1024)).toEqual({ cols: 5, rows: 10 });
		expect(nextDensity({ cols: 7, rows: 3 }, 1280, 720)).toEqual({ cols: 6, rows: 3 });
	});

	it('treats only the orientation defaults as automatic', () => {
		expect(isAutoDensity({ cols: 8, rows: 4 })).toBe(true);
		expect(isAutoDensity({ cols: 4, rows: 8 })).toBe(true);
		expect(isAutoDensity({ cols: 10, rows: 5 })).toBe(false);
	});
});

describe('geometry', () => {
	// 1280×720 deck: grid content 1232 × 632 at (24, 64), gap 16 → cell 140 × 146.
	const geo: GridGeometry = { cols: 8, rows: 4, left: 24, top: 64, width: 1232, height: 632, gap: 16 };

	it('maps a point to its cell and clamps outside points', () => {
		expect(cellAt(24 + 10, 64 + 10, geo)).toEqual({ x: 0, y: 0 });
		expect(cellAt(24 + 156 + 10, 64 + 162 * 2 + 10, geo)).toEqual({ x: 1, y: 2 });
		expect(cellAt(5000, 5000, geo)).toEqual({ x: 7, y: 3 });
		expect(cellAt(-50, -50, geo)).toEqual({ x: 0, y: 0 });
	});

	it('snaps a drag delta to the nearest cell and keeps the item inside', () => {
		const r = { x: 0, y: 0, w: 2, h: 2 };
		expect(snapDrag(r, 156 * 0.4, 0, geo)).toEqual({ x: 0, y: 0 });
		expect(snapDrag(r, 156 * 0.6, 0, geo)).toEqual({ x: 1, y: 0 });
		expect(snapDrag(r, 99999, 99999, geo)).toEqual({ x: 6, y: 2 });
	});
});
