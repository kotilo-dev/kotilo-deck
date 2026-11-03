import { describe, expect, it } from 'vitest';
import mocha from './themes/catppuccin-mocha.json';
import latte from './themes/catppuccin-latte.json';
import { hexToRgb, parseTheme, themeToCssVars, ThemeError } from './loader';
import { contrastRatio } from './contrast';
import { DEFAULT_THEME_ID, effectiveThemeId, getTheme, themeLabel, themes } from './index';

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
		expect([...themes.keys()].sort()).toEqual(['catppuccin-latte', 'catppuccin-mocha', 'dracula', 'nord', 'tokyo-night']);
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
		expect(themeLabel(getTheme('tokyo-night'))).toBe('tokyo night');
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
