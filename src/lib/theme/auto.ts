/**
 * Light/dark auto switch (SPEC §5.3): the effective theme is swapped for its `pair` when the wanted
 * variant differs. Themes without a (valid, opposite-variant) pair are kept as they are.
 */
import type { Theme } from './types';

export type Variant = 'light' | 'dark';

/** "07:30" → 450; null when malformed. */
export function parseHm(value: string | null | undefined): number | null {
	const m = /^(\d{1,2}):(\d{2})$/.exec(value ?? '');
	if (!m) return null;
	const h = Number(m[1]);
	const min = Number(m[2]);
	return h < 24 && min < 60 ? h * 60 + min : null;
}

export const minuteOfDay = (d: Date): number => d.getHours() * 60 + d.getMinutes();

/** True when `now` (minute of day) is inside [from, to); wraps past midnight; from == to = never. */
export function inWindow(now: number, from: number, to: number): boolean {
	if (from === to) return false;
	return from < to ? now >= from && now < to : now >= from || now < to;
}

/** Variant wanted by the auto-switch setting, or null when it's off / misconfigured. */
export function wantedVariant(
	auto: { mode: 'off' | 'os' | 'schedule'; lightFrom: string; darkFrom: string },
	now: Date,
	osDark: boolean
): Variant | null {
	if (auto.mode === 'os') return osDark ? 'dark' : 'light';
	if (auto.mode !== 'schedule') return null;
	const light = parseHm(auto.lightFrom);
	const dark = parseHm(auto.darkFrom);
	if (light === null || dark === null || light === dark) return null;
	return inWindow(minuteOfDay(now), light, dark) ? 'light' : 'dark';
}

/** `theme`, or its pair when the pair has the wanted variant. */
export function pairFor(theme: Theme, wanted: Variant | null, lookup: (id: string) => Theme | undefined): Theme {
	if (!wanted || theme.variant === wanted || !theme.pair) return theme;
	const pair = lookup(theme.pair);
	return pair && pair.variant === wanted ? pair : theme;
}
