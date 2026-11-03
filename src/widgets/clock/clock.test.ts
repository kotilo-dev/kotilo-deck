import { describe, expect, it } from 'vitest';
import { handAngles, handEnd, sanitizeZones, searchZones, zoneLabel } from './logic';

describe('clock hands', () => {
	it('places hands like the reference (9:48)', () => {
		const a = handAngles(9, 48);
		expect(a.hour).toBe(294);
		expect(a.minute).toBe(288);
	});
	it('wraps 24h input and moves the minute hand with seconds', () => {
		expect(handAngles(21, 0).hour).toBe(270);
		expect(handAngles(0, 30, 30).minute).toBe(183);
	});
	it('computes end points from the centre', () => {
		expect(handEnd(0, 40)).toEqual({ x: 75, y: 35 });
		expect(handEnd(90, 40)).toEqual({ x: 115, y: 75 });
		expect(handEnd(180, 10)).toEqual({ x: 75, y: 85 });
	});
});

describe('extra zones', () => {
	it('drops invalid / duplicate entries and caps at 3', () => {
		expect(
			sanitizeZones([
				{ timeZone: 'Europe/Berlin', label: '' },
				'America/New_York',
				{ timeZone: 'Nope/Zone', label: 'x' },
				{ timeZone: 'Europe/Berlin', label: 'dup' },
				{ timeZone: 'Asia/Tokyo', label: 'Tokyo office' },
				{ timeZone: 'UTC', label: '' }
			])
		).toEqual([
			{ timeZone: 'Europe/Berlin', label: '' },
			{ timeZone: 'America/New_York', label: '' },
			{ timeZone: 'Asia/Tokyo', label: 'Tokyo office' }
		]);
		expect(sanitizeZones(null)).toEqual([]);
	});

	it('labels zones by city unless overridden', () => {
		expect(zoneLabel({ timeZone: 'America/New_York', label: '' })).toBe('New York');
		expect(zoneLabel({ timeZone: 'America/New_York', label: ' NYC ' })).toBe('NYC');
	});
});

describe('searchZones', () => {
	const all = ['America/New_York', 'Europe/Istanbul', 'Asia/Istanbul_Fake', 'Europe/Berlin', 'America/Argentina/Buenos_Aires'];
	it('matches city and region terms, city prefix first', () => {
		expect(searchZones(all, 'ist')).toEqual(['Asia/Istanbul_Fake', 'Europe/Istanbul']);
		expect(searchZones(all, 'europe ist')).toEqual(['Europe/Istanbul']);
		expect(searchZones(all, 'new york')).toEqual(['America/New_York']);
		expect(searchZones(all, 'buenos')).toEqual(['America/Argentina/Buenos_Aires']);
		expect(searchZones(all, 'İSTANBUL')).toEqual(['Asia/Istanbul_Fake', 'Europe/Istanbul']);
	});
	it('limits an empty query', () => {
		expect(searchZones(all, '', 2)).toHaveLength(2);
	});
});
