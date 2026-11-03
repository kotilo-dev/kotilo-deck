import type { Place } from '#lib/ipc/index.ts';
import { dtf } from '#lib/time/index.ts';
import type { WeatherLocation } from './settings';

/** Backend caches 15 min; we ask again on the same cadence. */
export const REFRESH_MS = 15 * 60_000;

/** Icon families (DESIGN §5.3). */
export type Family = 'clear' | 'partly' | 'cloud' | 'fog' | 'drizzle' | 'rain' | 'snow' | 'thunder';

/** WMO weather interpretation code → icon family (Open-Meteo docs). */
export function wmoFamily(code: number): Family {
	if (code === 0) return 'clear';
	if (code === 1 || code === 2) return 'partly';
	if (code === 3) return 'cloud';
	if (code === 45 || code === 48) return 'fog';
	if (code >= 51 && code <= 57) return 'drizzle';
	if ((code >= 61 && code <= 67) || (code >= 80 && code <= 82)) return 'rain';
	if ((code >= 71 && code <= 77) || code === 85 || code === 86) return 'snow';
	if (code >= 95 && code <= 99) return 'thunder';
	return 'cloud';
}

export type ConditionKey =
	| 'clear'
	| 'mainlyClear'
	| 'partly'
	| 'overcast'
	| 'fog'
	| 'drizzle'
	| 'rain'
	| 'showers'
	| 'snow'
	| 'thunder'
	| 'unknown';

/** WMO code → i18n key under `widgets.weather.cond`. */
export function wmoText(code: number): ConditionKey {
	if (code === 0) return 'clear';
	if (code === 1) return 'mainlyClear';
	if (code === 2) return 'partly';
	if (code === 3) return 'overcast';
	if (code >= 80 && code <= 82) return 'showers';
	const family = wmoFamily(code);
	if (family === 'cloud') return 'unknown';
	return family;
}

/** Retry delays after consecutive failures: 30 s, 1, 2, 5, 10 min, then every 15 min. */
const BACKOFF = [30_000, 60_000, 120_000, 300_000, 600_000];
export const backoffDelay = (attempt: number): number => BACKOFF[Math.max(0, attempt - 1)] ?? REFRESH_MS;

/** `-0.4` → "0°", `14.6` → "15°". */
export const formatTemp = (n: number): string => `${Math.round(n) || 0}°`;

/** "N min ago" as an i18n key + params. */
export function agoKey(minutes: number): { key: string; params: Record<string, number> } {
	if (minutes < 1) return { key: 'widgets.weather.justNow', params: {} };
	if (minutes < 60) return { key: 'widgets.weather.minAgo', params: { n: minutes } };
	return { key: 'widgets.weather.hoursAgo', params: { n: Math.floor(minutes / 60) } };
}

/** Open-Meteo local times look like "2026-11-03T14:00": show the wall clock as given. */
export const hourLabel = (iso: string): string => iso.slice(11, 16);

/** "2026-11-04" → "Wed" in the locale (no zone shifting). */
export function dayLabel(date: string, locale: string): string {
	const [y, m, d] = date.split('-').map(Number);
	return dtf(locale, { weekday: 'short', timeZone: 'UTC' }).format(new Date(Date.UTC(y, m - 1, d)));
}

export const validCoords = (lat: number, lon: number): boolean =>
	Number.isFinite(lat) && Number.isFinite(lon) && Math.abs(lat) <= 90 && Math.abs(lon) <= 180;

/** Settings JSON is untrusted: returns a usable location or null. */
export function asLocation(raw: unknown): WeatherLocation | null {
	if (!raw || typeof raw !== 'object') return null;
	const r = raw as Partial<WeatherLocation>;
	const lat = Number(r.latitude);
	const lon = Number(r.longitude);
	if (!validCoords(lat, lon)) return null;
	return {
		name: typeof r.name === 'string' && r.name.trim() ? r.name.trim() : `${lat.toFixed(2)}, ${lon.toFixed(2)}`,
		country: typeof r.country === 'string' ? r.country : '',
		admin1: typeof r.admin1 === 'string' ? r.admin1 : null,
		latitude: lat,
		longitude: lon
	};
}

export const fromPlace = (p: Place): WeatherLocation => ({
	name: p.name,
	country: p.country,
	admin1: p.admin1,
	latitude: p.latitude,
	longitude: p.longitude
});

/** "İstanbul, Istanbul, Türkiye" without repeating the city. */
export function placeLine(p: { name: string; admin1: string | null; country: string }): string {
	return [p.name, p.admin1 && p.admin1 !== p.name ? p.admin1 : null, p.country].filter(Boolean).join(', ');
}

/** Daily chips: the next three days; drop today when the report has more than three. */
export const chipDays = <T>(daily: T[]): T[] => (daily.length > 3 ? daily.slice(1, 4) : daily.slice(0, 3));
