import { contrastRatio, mixHex } from './contrast';
import { COLOR_TOKENS, type Theme } from './types';

const HEX = /^#(?:[0-9a-fA-F]{3}|[0-9a-fA-F]{6})$/;

export class ThemeError extends Error {}

/** Validates unknown JSON against the theme contract; throws ThemeError naming the offending field. */
export function parseTheme(input: unknown): Theme {
	if (typeof input !== 'object' || input === null) throw new ThemeError('theme must be an object');
	const t = input as Record<string, any>;

	for (const key of ['id', 'name'] as const) {
		if (typeof t[key] !== 'string' || t[key].length === 0) throw new ThemeError(`missing ${key}`);
	}
	if (t.variant !== 'dark' && t.variant !== 'light') {
		throw new ThemeError(`${t.id}: variant must be "dark" or "light"`);
	}
	if (typeof t.colors !== 'object' || t.colors === null) throw new ThemeError(`${t.id}: missing colors`);
	for (const token of COLOR_TOKENS) {
		const value = t.colors[token];
		if (typeof value !== 'string' || !HEX.test(value)) {
			throw new ThemeError(`${t.id}: colors.${token} must be a hex colour`);
		}
	}

	const surface = t.opacity?.surface ?? 0.85;
	const bg = t.opacity?.bg ?? 1;
	for (const [name, v] of [['surface', surface], ['bg', bg]] as const) {
		if (typeof v !== 'number' || v < 0 || v > 1) {
			throw new ThemeError(`${t.id}: opacity.${name} must be between 0 and 1`);
		}
	}

	return {
		id: t.id,
		name: t.name,
		variant: t.variant,
		pair: typeof t.pair === 'string' ? t.pair : undefined,
		short: typeof t.short === 'string' && t.short.trim() ? t.short.trim() : undefined,
		colors: Object.fromEntries(COLOR_TOKENS.map((k) => [k, t.colors[k]])) as Theme['colors'],
		radius: typeof t.radius === 'string' ? t.radius : '16px',
		opacity: { surface, bg },
		font: {
			ui: t.font?.ui ?? 'Inter, system-ui, sans-serif',
			mono: t.font?.mono ?? 'JetBrains Mono, ui-monospace, monospace'
		}
	};
}

export function hexToRgb(hex: string): [number, number, number] {
	let h = hex.slice(1);
	if (h.length === 3) h = [...h].map((c) => c + c).join('');
	const n = parseInt(h, 16);
	return [(n >> 16) & 255, (n >> 8) & 255, n & 255];
}

/** Card look (DESIGN §4.5). */
export type Look = 'flat' | 'glass';

/** Glass: muted text is lifted toward text for contrast over a wallpaper (DESIGN §4.5). */
export const GLASS_MUTED_LIFT = 0.45;

/**
 * Maps a theme to `--k-*` CSS custom properties (DESIGN §2).
 * `surfaceOpacity` overrides the theme default (workspace / instance appearance). `look` adds the
 * derived surface tokens; the flat values reproduce the pre-glass look exactly.
 */
export function themeToCssVars(
	theme: Theme,
	surfaceOpacity = theme.opacity.surface,
	{ look = 'flat', scrim = 0 }: { look?: Look; scrim?: number } = {}
): Record<string, string> {
	const vars: Record<string, string> = {};
	const c = theme.colors;
	for (const token of COLOR_TOKENS) vars[`--k-${token}`] = c[token];

	const [r, g, b] = hexToRgb(c.surface);
	vars['--k-surface-a'] = `rgba(${r}, ${g}, ${b}, ${surfaceOpacity})`;
	vars['--k-on-accent'] = theme.variant === 'dark' ? c['bg-alt'] : '#ffffff';
	vars['--k-radius'] = theme.radius;
	vars['--k-font-ui'] = theme.font.ui;
	vars['--k-font-mono'] = theme.font.mono;

	if (look === 'glass') {
		const inset = `color-mix(in srgb, ${c['bg-alt']} 45%, transparent)`;
		vars['--k-inset'] = inset;
		vars['--k-inset-deep'] = inset;
		vars['--k-inset-ring'] = `color-mix(in srgb, ${c.text} 8%, transparent)`;
		vars['--k-card-border'] = `color-mix(in srgb, ${c.text} 18%, transparent)`;
		vars['--k-text-muted'] = mixHex(c['text-muted'], c.text, GLASS_MUTED_LIFT);
		vars['--k-scrim'] = `color-mix(in srgb, ${c.bg} ${Math.round(scrim * 100)}%, transparent)`;
	} else {
		vars['--k-inset'] = c['bg-alt'];
		vars['--k-inset-deep'] = c.bg;
		vars['--k-inset-ring'] = 'transparent';
		vars['--k-card-border'] = c.border;
		vars['--k-scrim'] = 'transparent';
	}
	return vars;
}

/** Worst cases behind a glass card: a blur cannot turn a white or black area into anything else. */
const GLASS_BACKDROPS = ['#ffffff', '#000000'] as const;
export const GLASS_MIN_OPACITY = 0.52;

/**
 * Default glass surface opacity for a theme: the lowest value from 0.52 up at which text and the lifted
 * muted text reach 4.5:1 over a pure white and a pure black wallpaper under the given scrim (DESIGN §4.5,
 * §9). Light themes and low-contrast palettes come out higher, so no theme fails the rule by default.
 */
export function glassSurfaceOpacity(theme: Theme, scrim: number): number {
	const c = theme.colors;
	const muted = mixHex(c['text-muted'], c.text, GLASS_MUTED_LIFT);
	const ok = (alpha: number) =>
		GLASS_BACKDROPS.every((wall) => {
			const card = mixHex(c.surface, mixHex(c.bg, wall, scrim), alpha);
			return contrastRatio(c.text, card) >= 4.5 && contrastRatio(muted, card) >= 4.5;
		});
	for (let pct = Math.round(GLASS_MIN_OPACITY * 100); pct < 100; pct++) {
		if (ok(pct / 100)) return pct / 100;
	}
	return 1;
}
