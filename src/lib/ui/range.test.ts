import { describe, expect, it } from 'vitest';
import { rangePercent } from './range';

describe('rangePercent (.k-range fill)', () => {
	it('maps the value onto min–max', () => {
		expect(rangePercent(62, 0, 100)).toBe('62%');
		expect(rangePercent(20, 20, 100)).toBe('0%');
		expect(rangePercent(16, 0, 40)).toBe('40%');
		expect(rangePercent(1, 0, 3)).toBe('33.33%');
	});

	it('clamps and survives bad input', () => {
		expect(rangePercent(150, 0, 100)).toBe('100%');
		expect(rangePercent(-5, 0, 100)).toBe('0%');
		expect(rangePercent(NaN, 0, 100)).toBe('0%');
		expect(rangePercent(5, 10, 10)).toBe('0%');
	});
});
