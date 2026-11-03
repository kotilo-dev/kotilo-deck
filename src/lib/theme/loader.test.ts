import { describe, expect, it } from 'vitest';
import mocha from './themes/catppuccin-mocha.json';
import latte from './themes/catppuccin-latte.json';
import { hexToRgb, parseTheme, themeToCssVars, ThemeError } from './loader';
import { themes } from './index';

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
		expect([...themes.keys()].sort()).toEqual(['catppuccin-latte', 'catppuccin-mocha']);
	});
});
