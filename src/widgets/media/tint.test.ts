import { describe, expect, it } from 'vitest';
import { bundledThemes, contrastRatio, mixHex } from '#lib/theme/index.ts';
import { averageColor, rgbCss, tintColor, TINT_SHARE, type Rgb } from './logic';

const hex = ([r, g, b]: Rgb) => `#${[r, g, b].map((v) => v.toString(16).padStart(2, '0')).join('')}`;

describe('averageColor', () => {
	it('averages RGBA pixels weighted by alpha', () => {
		expect(averageColor([255, 0, 0, 255, 0, 0, 255, 255])).toEqual([128, 0, 128]);
		expect(averageColor([255, 0, 0, 255, 0, 255, 0, 0])).toEqual([255, 0, 0]);
		expect(averageColor([1, 2, 3, 0])).toBeNull();
		expect(averageColor([])).toBeNull();
	});
});

describe('tintColor', () => {
	it('formats as rgb() so no hex reaches the widget', () => {
		expect(rgbCss([1, 2, 3])).toBe('rgb(1 2 3)');
	});

	// Worst-case covers: black, white, pure primaries, neon, muddy.
	const covers: Rgb[] = [
		[0, 0, 0],
		[255, 255, 255],
		[255, 0, 0],
		[0, 255, 0],
		[0, 0, 255],
		[255, 255, 0],
		[0, 255, 255],
		[255, 0, 255],
		[120, 90, 40]
	];

	it('keeps a mid-tone cover as is in Mocha', () => {
		const c = bundledThemes.get('catppuccin-mocha')!.colors;
		const t = { surface: c.surface, text: c.text, muted: c['text-muted'] };
		expect(tintColor([120, 90, 160], t)).toEqual([120, 90, 160]);
	});

	it('text stays ≥ 4.5:1 and muted text ≥ 3:1 on the tinted card in every bundled theme', () => {
		const fails: string[] = [];
		for (const [id, theme] of bundledThemes) {
			for (const c of covers) {
				const t = { surface: theme.colors.surface, text: theme.colors.text, muted: theme.colors['text-muted'] };
				const card = mixHex(hex(tintColor(c, t)), t.surface, TINT_SHARE);
				const text = contrastRatio(theme.colors.text, card);
				const muted = contrastRatio(theme.colors['text-muted'], card);
				const base = (fg: string, min: number) => Math.min(min, contrastRatio(fg, t.surface));
				if (text < base(t.text, 4.5) || muted < base(t.muted, 3)) fails.push(`${id} ${c}: ${text.toFixed(2)} / ${muted.toFixed(2)}`);
			}
		}
		expect(fails).toEqual([]);
	});
});
