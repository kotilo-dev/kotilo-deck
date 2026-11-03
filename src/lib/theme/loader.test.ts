import { describe, expect, it } from 'vitest';
import mocha from './themes/catppuccin-mocha.json';
import latte from './themes/catppuccin-latte.json';
import { hexToRgb, parseTheme, themeToCssVars, ThemeError } from './loader';
import { contrastRatio } from './contrast';
import {
	bundledThemes,
	DEFAULT_THEME_ID,
	effectiveThemeId,
	getTheme,
	inWindow,
	isUserTheme,
	pairFor,
	parseHm,
	parseUserThemes,
	setUserThemes,
	themeLabel,
	themes,
	userThemeIssues,
	wantedVariant
} from './index';

describe('parseTheme', () => {
	it('accepts the bundled themes', () => {
		expect(parseTheme(mocha).id).toBe('catppuccin-mocha');
		expect(parseTheme(latte).variant).toBe('light');
	});

	it('rejects a missing colour token', () => {
		const broken = structuredClone(mocha) as any;
		delete broken.colors.accent;
		expect(() => parseTheme(broken)).toThrow(ThemeError);
	});

	it('rejects non-hex colours and bad opacity', () => {
		expect(() => parseTheme({ ...mocha, colors: { ...mocha.colors, bg: 'red' } })).toThrow(/colors.bg/);
		expect(() => parseTheme({ ...mocha, opacity: { surface: 2, bg: 1 } })).toThrow(/opacity.surface/);
	});

	it('fills defaults for optional fields', () => {
		const { radius, font, opacity, ...rest } = mocha;
		const t = parseTheme(rest);
		expect(t.radius).toBe('16px');
		expect(t.opacity.surface).toBe(0.85);
		expect(t.font.mono).toContain('JetBrains Mono');
	});
});

describe('themeToCssVars', () => {
	it('maps tokens 1:1 and derives surface-a / on-accent', () => {
		const vars = themeToCssVars(parseTheme(mocha));
		expect(vars['--k-bg']).toBe('#1e1e2e');
		expect(vars['--k-surface-a']).toBe('rgba(49, 50, 68, 0.85)');
		expect(vars['--k-on-accent']).toBe('#181825');
		expect(themeToCssVars(parseTheme(latte))['--k-on-accent']).toBe('#ffffff');
	});

	it('applies a surface opacity override', () => {
		expect(themeToCssVars(parseTheme(mocha), 0.5)['--k-surface-a']).toBe('rgba(49, 50, 68, 0.5)');
	});
});

describe('hexToRgb', () => {
	it('expands short hex', () => {
		expect(hexToRgb('#fff')).toEqual([255, 255, 255]);
	});
});

describe('bundled registry', () => {
	it('loads every theme in themes/', () => {
		expect([...themes.keys()].sort()).toEqual([
			'ayu-dark',
			'ayu-light',
			'ayu-mirage',
			'catppuccin-frappe',
			'catppuccin-latte',
			'catppuccin-macchiato',
			'catppuccin-mocha',
			'dracula',
			'everforest-dark',
			'everforest-light',
			'github-dark',
			'github-light',
			'gruvbox-dark',
			'gruvbox-light',
			'kanagawa-dragon',
			'kanagawa-wave',
			'material-ocean',
			'material-palenight',
			'monokai-pro',
			'nightfox',
			'nord',
			'one-dark',
			'oxocarbon',
			'rose-pine',
			'rose-pine-dawn',
			'rose-pine-moon',
			'solarized-dark',
			'solarized-light',
			'tokyo-night',
			'tokyo-night-day',
			'tokyo-night-storm',
		]);
	});

	it('resolves the effective theme: workspace → settings → default', () => {
		expect(effectiveThemeId('nord', 'dracula')).toBe('nord');
		expect(effectiveThemeId(null, 'dracula')).toBe('dracula');
		expect(effectiveThemeId(null, null)).toBe(DEFAULT_THEME_ID);
		expect(effectiveThemeId('does-not-exist', null)).toBe(DEFAULT_THEME_ID);
		expect(getTheme(undefined).id).toBe(DEFAULT_THEME_ID);
	});

	it('derives short chip labels', () => {
		expect(themeLabel(getTheme('catppuccin-mocha'))).toBe('mocha');
		expect(themeLabel(getTheme('dracula'))).toBe('dracula');
		expect(themeLabel(getTheme('catppuccin-latte'))).toBe('latte');
		// Dropping the family word would leave "night", "dark", "pine dawn": keep those distinctive.
		expect(themeLabel(getTheme('tokyo-night'))).toBe('tokyo night');
		expect(themeLabel(getTheme('tokyo-night-day'))).toBe('tokyo day');
		expect(themeLabel(getTheme('github-dark'))).toBe('github dark');
		expect(themeLabel(getTheme('rose-pine-dawn'))).toBe('rosé pine dawn');
		expect(themeLabel({ ...getTheme('nord'), short: 'My Nord' })).toBe('my nord');
	});

	it('gives every bundled theme a distinct chip label', () => {
		const labels = [...bundledThemes.values()].map(themeLabel);
		expect(new Set(labels).size).toBe(labels.length);
	});

	it('links pairs only to bundled themes, both ways', () => {
		for (const theme of themes.values()) {
			if (!theme.pair) continue;
			expect(themes.get(theme.pair)?.pair, theme.id).toBe(theme.id);
		}
	});
});

describe('contrast (SPEC §9, DESIGN §2)', () => {
	it('computes WCAG ratios', () => {
		expect(contrastRatio('#000000', '#ffffff')).toBeCloseTo(21, 5);
		expect(contrastRatio('#777777', '#777777')).toBe(1);
	});

	for (const theme of themes.values()) {
		it(`${theme.id}: text and text-muted reach 4.5:1 on surface and bg`, () => {
			const c = theme.colors;
			for (const fg of ['text', 'text-muted'] as const) {
				for (const bg of ['surface', 'bg'] as const) {
					expect(contrastRatio(c[fg], c[bg]), `${fg} on ${bg}`).toBeGreaterThanOrEqual(4.5);
				}
			}
		});

		it(`${theme.id}: on-accent label reaches 4.5:1 on accent`, () => {
			expect(contrastRatio(themeToCssVars(theme)['--k-on-accent'], theme.colors.accent)).toBeGreaterThanOrEqual(4.5);
		});
	}
});

describe('user themes (SPEC §5.3)', () => {
	const custom = { ...mocha, id: 'my-theme', name: 'My Theme' };

	it('skips invalid entries with a message instead of throwing', () => {
		const { themes: ok, errors } = parseUserThemes([custom, { id: 'broken', name: 'B' }, 42, { ...custom }]);
		expect(ok.map((t) => t.id)).toEqual(['my-theme']);
		expect(errors.map((e) => e.label)).toEqual(['broken', '#3', 'my-theme']);
	});

	it('adds user themes and lets the same id override a bundled one', () => {
		const override = { ...mocha, name: 'Mocha (mine)', colors: { ...mocha.colors, accent: '#ff0000' } };
		const { themes: list, errors } = parseUserThemes([custom, override]);
		setUserThemes(list, [{ label: 'x', message: 'bad' }]);
		try {
			expect(themes.get('my-theme')?.name).toBe('My Theme');
			expect(getTheme('catppuccin-mocha').colors.accent).toBe('#ff0000');
			expect(isUserTheme('catppuccin-mocha')).toBe(true);
			expect(userThemeIssues.get('x')).toBe('bad');
			expect(errors).toEqual([]);
			expect(themes.size).toBe(bundledThemes.size + 1);
		} finally {
			setUserThemes([]);
		}
		expect(getTheme('catppuccin-mocha').colors.accent).toBe(mocha.colors.accent);
		expect(themes.has('my-theme')).toBe(false);
	});
});

describe('light/dark auto switch', () => {
	const at = (h: number, m = 0) => new Date(2026, 10, 3, h, m);
	const auto = (mode: 'off' | 'os' | 'schedule') => ({ mode, lightFrom: '07:00', darkFrom: '19:00' });

	it('parses HH:MM and checks wrapping windows', () => {
		expect(parseHm('07:30')).toBe(450);
		expect(parseHm('24:00')).toBeNull();
		expect(parseHm('7')).toBeNull();
		expect(inWindow(23 * 60, 22 * 60, 7 * 60)).toBe(true);
		expect(inWindow(12 * 60, 22 * 60, 7 * 60)).toBe(false);
		expect(inWindow(5, 5, 5)).toBe(false);
	});

	it('wants a variant from the OS or the schedule', () => {
		expect(wantedVariant(auto('off'), at(12), true)).toBeNull();
		expect(wantedVariant(auto('os'), at(12), true)).toBe('dark');
		expect(wantedVariant(auto('os'), at(12), false)).toBe('light');
		expect(wantedVariant(auto('schedule'), at(12), true)).toBe('light');
		expect(wantedVariant(auto('schedule'), at(19), false)).toBe('dark');
		expect(wantedVariant(auto('schedule'), at(6, 59), false)).toBe('dark');
		expect(wantedVariant({ mode: 'schedule', lightFrom: 'x', darkFrom: '19:00' }, at(12), false)).toBeNull();
	});

	it('follows the pair only when it has the wanted variant', () => {
		const lookup = (id: string) => themes.get(id);
		expect(pairFor(getTheme('catppuccin-mocha'), 'light', lookup).id).toBe('catppuccin-latte');
		expect(pairFor(getTheme('catppuccin-latte'), 'light', lookup).id).toBe('catppuccin-latte');
		expect(pairFor(getTheme('catppuccin-mocha'), null, lookup).id).toBe('catppuccin-mocha');
		expect(pairFor(getTheme('dracula'), 'light', lookup).id).toBe('dracula');
	});
});
