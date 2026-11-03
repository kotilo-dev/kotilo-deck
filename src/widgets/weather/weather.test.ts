import { describe, expect, it } from 'vitest';
import {
	agoKey,
	asLocation,
	backoffDelay,
	chipDays,
	dayLabel,
	formatTemp,
	hourLabel,
	placeLine,
	REFRESH_MS,
	wmoFamily,
	wmoText
} from './logic';

describe('WMO mapping', () => {
	it.each([
		[0, 'clear', 'clear'],
		[1, 'partly', 'mainlyClear'],
		[2, 'partly', 'partly'],
		[3, 'cloud', 'overcast'],
		[45, 'fog', 'fog'],
		[48, 'fog', 'fog'],
		[51, 'drizzle', 'drizzle'],
		[57, 'drizzle', 'drizzle'],
		[61, 'rain', 'rain'],
		[67, 'rain', 'rain'],
		[80, 'rain', 'showers'],
		[82, 'rain', 'showers'],
		[71, 'snow', 'snow'],
		[77, 'snow', 'snow'],
		[86, 'snow', 'snow'],
		[95, 'thunder', 'thunder'],
		[99, 'thunder', 'thunder'],
		[42, 'cloud', 'unknown']
	])('code %i → %s / %s', (code, family, text) => {
		expect(wmoFamily(code)).toBe(family);
		expect(wmoText(code)).toBe(text);
	});
});

describe('backoff', () => {
	it('grows then settles at the refresh cadence', () => {
		expect([1, 2, 3, 4, 5, 6, 20].map(backoffDelay)).toEqual([
			30_000,
			60_000,
			120_000,
			300_000,
			600_000,
			REFRESH_MS,
			REFRESH_MS
		]);
	});
});

describe('formatting', () => {
	it('rounds temperatures without "-0"', () => {
		expect(formatTemp(14.6)).toBe('15°');
		expect(formatTemp(-0.4)).toBe('0°');
		expect(formatTemp(-3.5)).toBe('-3°');
	});

	it('labels age', () => {
		expect(agoKey(0).key).toBe('widgets.weather.justNow');
		expect(agoKey(6)).toEqual({ key: 'widgets.weather.minAgo', params: { n: 6 } });
		expect(agoKey(125)).toEqual({ key: 'widgets.weather.hoursAgo', params: { n: 2 } });
	});

	it('labels hours and days without shifting zones', () => {
		expect(hourLabel('2026-11-03T14:00')).toBe('14:00');
		expect(dayLabel('2026-11-04', 'en-US')).toBe('Wed');
	});

	it('builds place lines', () => {
		expect(placeLine({ name: 'İstanbul', admin1: 'Istanbul', country: 'Türkiye' })).toBe(
			'İstanbul, Istanbul, Türkiye'
		);
		expect(placeLine({ name: 'Berlin', admin1: 'Berlin', country: 'Germany' })).toBe('Berlin, Germany');
	});

	it('picks three chip days', () => {
		expect(chipDays([1, 2, 3])).toEqual([1, 2, 3]);
		expect(chipDays([0, 1, 2, 3])).toEqual([1, 2, 3]);
	});
});

describe('asLocation', () => {
	it('accepts valid coordinates and names manual ones', () => {
		expect(asLocation({ latitude: 41.01, longitude: 28.95, name: '' })).toEqual({
			name: '41.01, 28.95',
			country: '',
			admin1: null,
			latitude: 41.01,
			longitude: 28.95
		});
	});

	it('rejects junk', () => {
		expect(asLocation(null)).toBeNull();
		expect(asLocation({ latitude: 91, longitude: 0 })).toBeNull();
		expect(asLocation({ latitude: 'x', longitude: 0 })).toBeNull();
	});
});
