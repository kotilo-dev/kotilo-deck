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

/**
 * Maps a theme to `--k-*` CSS custom properties (DESIGN §2).
 * `surfaceOpacity` overrides the theme default (workspace / instance appearance).
 */
export function themeToCssVars(theme: Theme, surfaceOpacity = theme.opacity.surface): Record<string, string> {
	const vars: Record<string, string> = {};
	for (const token of COLOR_TOKENS) vars[`--k-${token}`] = theme.colors[token];

	const [r, g, b] = hexToRgb(theme.colors.surface);
	vars['--k-surface-a'] = `rgba(${r}, ${g}, ${b}, ${surfaceOpacity})`;
	vars['--k-on-accent'] = theme.variant === 'dark' ? theme.colors['bg-alt'] : '#ffffff';
	vars['--k-radius'] = theme.radius;
	vars['--k-font-ui'] = theme.font.ui;
	vars['--k-font-mono'] = theme.font.mono;
	return vars;
}
