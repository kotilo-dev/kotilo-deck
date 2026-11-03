/**
 * ICS calendar feed list helpers (Settings ▸ Weather & calendar). Pure: validation, masking, ids, order.
 * Feed URLs often carry a secret token (Google "secret address", Outlook publish links), so the UI only
 * ever shows them masked to the host.
 */
import type { CalendarFeed, FeedColor } from '#lib/ipc/index.ts';
import { slugify, uniqueId } from '#lib/workspace/index.ts';

/** The six token colours a feed can use, rendered as `var(--k-<name>)`. */
export const FEED_COLORS: readonly FeedColor[] = ['accent', 'accent-2', 'success', 'warning', 'danger', 'info'];

export const feedColorVar = (c: FeedColor): string => `var(--k-${c})`;

export type FeedUrlProblem = 'empty' | 'scheme' | 'invalid';

/** https:// or webcal:// with a host. Returns the trimmed URL or the problem. */
export function checkFeedUrl(raw: string): { url: string } | { problem: FeedUrlProblem } {
	const url = raw.trim();
	if (!url) return { problem: 'empty' };
	const scheme = /^([a-z][a-z0-9+.-]*):/i.exec(url)?.[1]?.toLowerCase();
	if (scheme !== 'https' && scheme !== 'webcal') return { problem: 'scheme' };
	let parsed: URL;
	try {
		// Parse webcal as https: same authority rules, and non-special schemes parse hosts loosely.
		parsed = new URL(scheme === 'webcal' ? `https:${url.slice(url.indexOf(':') + 1)}` : url);
	} catch {
		return { problem: 'invalid' };
	}
	if (!parsed.hostname || !/^\/\//.test(url.slice(url.indexOf(':') + 1))) return { problem: 'invalid' };
	return { url };
}

/** "calendar.google.com/…" — the host only, never path or query (may hold a secret). */
export function maskFeedUrl(url: string): string {
	const rest = url.trim().replace(/^[a-z][a-z0-9+.-]*:/i, '');
	if (!rest.startsWith('//')) return '…';
	try {
		const host = new URL(`https:${rest}`).hostname;
		return host ? `${host}/…` : '…';
	} catch {
		return '…';
	}
}

/** Stable id for a new feed from its name, unique among `feeds`. */
export function newFeedId(name: string, feeds: readonly CalendarFeed[]): string {
	const slug = slugify(name);
	const base = slug === 'workspace' && !/workspace/i.test(name) ? 'feed' : slug;
	return uniqueId(base, feeds.map((f) => f.id));
}

/** First colour not used yet, else cycles. */
export function nextFeedColor(feeds: readonly CalendarFeed[]): FeedColor {
	const used = new Set(feeds.map((f) => f.color));
	return FEED_COLORS.find((c) => !used.has(c)) ?? FEED_COLORS[feeds.length % FEED_COLORS.length];
}

/** Moves the feed `id` by `dir` (−1 up, +1 down); returns a new array (unchanged when at an edge). */
export function moveFeed<T extends { id: string }>(list: readonly T[], id: string, dir: -1 | 1): T[] {
	const i = list.findIndex((f) => f.id === id);
	const j = i + dir;
	if (i < 0 || j < 0 || j >= list.length) return [...list];
	const next = [...list];
	[next[i], next[j]] = [next[j], next[i]];
	return next;
}
