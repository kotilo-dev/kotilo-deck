import { hexToRgb } from './loader';

const channel = (c: number) => {
	const s = c / 255;
	return s <= 0.03928 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4;
};

/** WCAG 2.x relative luminance of a hex colour. */
export function luminance(hex: string): number {
	const [r, g, b] = hexToRgb(hex).map(channel);
	return 0.2126 * r + 0.7152 * g + 0.0722 * b;
}

/** WCAG contrast ratio (1–21) between two hex colours. */
export function contrastRatio(a: string, b: string): number {
	const [hi, lo] = [luminance(a), luminance(b)].sort((x, y) => y - x);
	return (hi + 0.05) / (lo + 0.05);
}

/** `color-mix(in srgb, a <t*100>%, b)` as a hex colour (sRGB interpolation, like the CSS function). */
export function mixHex(a: string, b: string, t: number): string {
	const A = hexToRgb(a);
	const B = hexToRgb(b);
	return `#${A.map((v, i) => Math.round(v * t + B[i] * (1 - t)).toString(16).padStart(2, '0')).join('')}`;
}
