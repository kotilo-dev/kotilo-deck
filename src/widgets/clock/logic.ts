import { cityFromZone, isValidTimeZone, utcOffsetMinutes, formatUtcOffset } from '#lib/time/index.ts';
import { MAX_ZONES, type ExtraZone } from './settings';

/** Hand angles in degrees clockwise from 12 o'clock. */
export function handAngles(hour: number, minute: number, second = 0): { hour: number; minute: number } {
	return {
		hour: ((hour % 12) + minute / 60) * 30,
		minute: (minute + second / 60) * 6
	};
}

/** End point of a hand of `length` from centre (`c`, `c`) at `deg`. Rounded to 0.01 for stable SVG output. */
export function handEnd(deg: number, length: number, c = 75): { x: number; y: number } {
	const rad = (deg * Math.PI) / 180;
	const r = (n: number) => Math.round(n * 100) / 100;
	return { x: r(c + length * Math.sin(rad)), y: r(c - length * Math.cos(rad)) };
}

/** Keeps valid, unique zones (max 3) from untrusted settings JSON. */
export function sanitizeZones(raw: unknown): ExtraZone[] {
	if (!Array.isArray(raw)) return [];
	const seen = new Set<string>();
	const out: ExtraZone[] = [];
	for (const item of raw) {
		const tz = typeof item === 'string' ? item : (item as Partial<ExtraZone> | null)?.timeZone;
		if (!isValidTimeZone(tz) || seen.has(tz)) continue;
		seen.add(tz);
		const label = typeof item === 'object' && item && typeof item.label === 'string' ? item.label : '';
		out.push({ timeZone: tz, label });
		if (out.length === MAX_ZONES) break;
	}
	return out;
}

export const zoneLabel = (z: ExtraZone): string => z.label.trim() || cityFromZone(z.timeZone);

/** Locale-independent fold ("İstanbul" / "ISTANBUL" / "ıstanbul" → "istanbul"); the OS locale may be Turkish. */
export const normalize = (s: string): string =>
	s
		.normalize('NFD')
		.replace(/[̀-ͯ]/g, '')
		.replace(/ı/g, 'i')
		.toLowerCase()
		.replace(/[_/]+/g, ' ');

/**
 * Case-insensitive search over IANA ids; matches on the whole id ("europe ist") or the
 * city part. City-prefix matches sort first.
 */
export function searchZones(all: readonly string[], query: string, limit = 40): string[] {
	const q = normalize(query.trim());
	if (!q) return all.slice(0, limit);
	const terms = q.split(/\s+/);
	const scored: { tz: string; score: number }[] = [];
	for (const tz of all) {
		const hay = normalize(tz);
		if (!terms.every((term) => hay.includes(term))) continue;
		const city = normalize(cityFromZone(tz));
		scored.push({ tz, score: city.startsWith(q) ? 0 : city.includes(q) ? 1 : 2 });
	}
	scored.sort((a, b) => a.score - b.score || a.tz.localeCompare(b.tz));
	return scored.slice(0, limit).map((s) => s.tz);
}

export const zoneOffsetLabel = (tz: string, at: Date = new Date()): string => formatUtcOffset(utcOffsetMinutes(at, tz));
