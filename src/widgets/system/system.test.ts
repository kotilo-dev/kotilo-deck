import { describe, expect, it } from 'vitest';
import {
	busiestCore,
	coreBucket,
	gb,
	gbTotal,
	initialWarn,
	memLabel,
	peak,
	percent,
	pushHistory,
	sparklineArea,
	sparklinePoints,
	stepWarn,
	type WarnState
} from './logic';

const run = (values: number[], threshold = 85, start: WarnState = initialWarn) => {
	const states: WarnState[] = [];
	let s = start;
	for (const v of values) states.push((s = stepWarn(s, v, threshold)));
	return states;
};

describe('warn state machine', () => {
	it('warns only after 3 consecutive samples over the threshold', () => {
		expect(run([90, 90]).map((s) => s.warn)).toEqual([false, false]);
		expect(run([90, 90, 90]).at(-1)).toEqual({ warn: true, over: 3, under: 0 });
	});

	it('treats a value equal to the threshold as not over', () => {
		expect(run([85, 85, 85]).at(-1)?.warn).toBe(false);
	});

	it('resets the run on a dip before warning', () => {
		expect(run([90, 90, 50, 90, 90]).map((s) => s.warn)).toEqual([false, false, false, false, false]);
	});

	it('counts the episode while warning and clears after 3 under', () => {
		const states = run([90, 90, 90, 95, 40, 40, 40]);
		expect(states.map((s) => s.warn)).toEqual([false, false, true, true, true, true, false]);
		expect(states[3].over).toBe(4);
		expect(states.at(-1)).toEqual(initialWarn);
	});

	it('a spike while recovering restarts the clear count', () => {
		const states = run([90, 90, 90, 40, 40, 90, 40, 40]);
		expect(states.map((s) => s.warn)).toEqual([false, false, true, true, true, true, true, true]);
		expect(states.at(-1)?.under).toBe(2);
	});
});

describe('history', () => {
	it('keeps the newest 60', () => {
		let h: number[] = [];
		for (let i = 0; i < 70; i++) h = pushHistory(h, i);
		expect(h).toHaveLength(60);
		expect(h[0]).toBe(10);
		expect(h.at(-1)).toBe(69);
	});
});

describe('sparklinePoints', () => {
	it('anchors the newest sample at the right edge and maps 0–100 into the padded box', () => {
		expect(sparklinePoints([0, 100], 240, 36, 3)).toBe('120,35 240,1');
	});
	it('spans the full width with a full window', () => {
		const pts = sparklinePoints(Array(60).fill(50), 240, 36).split(' ');
		expect(pts).toHaveLength(60);
		expect(pts[0]).toBe('0,18');
		expect(pts.at(-1)).toBe('240,18');
	});
	it('clamps out-of-range values and handles empty input', () => {
		expect(sparklinePoints([150, -5], 10, 12, 2)).toBe('0,1 10,11');
		expect(sparklinePoints([], 240, 36)).toBe('');
	});
});

describe('units', () => {
	const GiB = 1024 ** 3;
	it('formats memory in the locale', () => {
		expect(gb(11.2 * GiB, 'en')).toBe('11.2');
		expect(gb(11.2 * GiB, 'tr')).toBe('11,2');
		expect(gbTotal(32 * GiB - 1000, 'en')).toBe('32');
		expect(gbTotal(15.5 * GiB, 'en')).toBe('15.5');
		expect(memLabel(820 * 1024 ** 2, 'en')).toBe('820 MB');
		expect(memLabel(1.4 * GiB, 'en')).toBe('1.4 GB');
	});
	it('computes percentages safely', () => {
		expect(percent(8, 32)).toBe(25);
		expect(percent(1, 0)).toBe(0);
	});
});

describe('4×2 refresh helpers (DESIGN §5.5)', () => {
	it('area closes the line down to the bottom edge', () => {
		expect(sparklineArea([], 240, 40)).toBe('');
		const line = sparklinePoints([0, 100], 240, 40, 3);
		expect(sparklineArea([0, 100], 240, 40, 3)).toBe(`120,40 ${line} 240,40`);
		expect(sparklineArea([50], 240, 40)).toBe(`240,40 ${sparklinePoints([50], 240, 40)} 240,40`);
	});

	it('peak is the window maximum, clamped', () => {
		expect(peak([])).toBeNull();
		expect(peak([12, 61.4, 30])).toBe(61);
		expect(peak([140])).toBe(100);
	});

	it('core buckets: <25 low, 25..threshold mid, >= threshold high', () => {
		expect(coreBucket(0, 85)).toBe('low');
		expect(coreBucket(24.9, 85)).toBe('low');
		expect(coreBucket(25, 85)).toBe('mid');
		expect(coreBucket(84.9, 85)).toBe('mid');
		expect(coreBucket(85, 85)).toBe('high');
		expect(coreBucket(NaN, 85)).toBe('low');
	});

	it('busiest core is 1-based, first wins a tie', () => {
		expect(busiestCore([])).toBeNull();
		expect(busiestCore([70, 12, 70])).toEqual({ core: 1, load: 70 });
		expect(busiestCore([5, 33.6, 9])).toEqual({ core: 2, load: 34 });
	});
});
