import { describe, expect, it } from 'vitest';
import { DEFAULT_SETTINGS } from '#lib/workspace/index.ts';
import { getTheme, glassSurfaceOpacity } from '#lib/theme/index.ts';
import {
	backdropValue,
	clockDrift,
	dimLevel,
	nightActive,
	resolveBackground,
	resolveBlur,
	resolveLook,
	resolveScrim,
	resolveSurfaceOpacity,
	SHIFT_PATTERN,
	shiftAt
} from './look';
import { acceleratorFrom, hotkeyProblem } from './ui/context.svelte';

const g = DEFAULT_SETTINGS.appearance;
const at = (h: number, m = 0) => new Date(2026, 10, 3, h, m);

describe('appearance layers (SPEC §5.3)', () => {
	it('background: workspace spec wins, wallpaper without a file falls back to solid', () => {
		expect(resolveBackground(undefined, g)).toEqual({ mode: 'solid', wallpaper: null });
		expect(resolveBackground(undefined, { ...g, background: 'wallpaper', wallpaper: null }).mode).toBe('solid');
		expect(resolveBackground(undefined, { ...g, background: 'wallpaper', wallpaper: 'a.jpg' })).toEqual({ mode: 'wallpaper', wallpaper: 'a.jpg' });
		expect(resolveBackground({ background: 'wallpaper:mountains.jpg' }, g)).toEqual({ mode: 'wallpaper', wallpaper: 'mountains.jpg' });
		expect(resolveBackground({ background: 'transparent' }, { ...g, background: 'wallpaper', wallpaper: 'a.jpg' }).mode).toBe('transparent');
		expect(resolveBackground({ background: 'bogus' }, { ...g, background: 'transparent' }).mode).toBe('transparent');
	});

	it('surface opacity: workspace > global > theme', () => {
		const theme = getTheme('catppuccin-mocha');
		expect(resolveSurfaceOpacity(undefined, g, theme)).toBe(theme.opacity.surface);
		expect(resolveSurfaceOpacity(undefined, { ...g, surfaceOpacity: 0.4 }, theme)).toBe(0.4);
		expect(resolveSurfaceOpacity({ opacity: 0.7 }, { ...g, surfaceOpacity: 0.4 }, theme)).toBe(0.7);
		expect(resolveSurfaceOpacity({ opacity: 3 }, g, theme)).toBe(1);
	});

	it('blur: instance > workspace > global, clamped, off at 0', () => {
		expect(resolveBlur(undefined, undefined, g)).toBe(0);
		expect(resolveBlur(undefined, { blur: 12 }, { ...g, blur: 4 })).toBe(12);
		expect(resolveBlur({ blur: 0 }, { blur: 12 }, g)).toBe(0);
		expect(resolveBlur(undefined, undefined, { ...g, blur: 99 })).toBe(40);
		expect(backdropValue(0)).toBe('none');
		expect(backdropValue(8)).toBe('blur(8px)');
	});

	it('look: workspace style wins, glass only over a wallpaper or transparent background (DESIGN §4.5)', () => {
		const glass = { ...g, style: 'glass' as const };
		expect(g.style).toBe('flat');
		expect(resolveLook(undefined, g, 'wallpaper')).toBe('flat');
		expect(resolveLook(undefined, glass, 'wallpaper')).toBe('glass');
		expect(resolveLook(undefined, glass, 'transparent')).toBe('glass');
		expect(resolveLook(undefined, glass, 'solid')).toBe('flat');
		expect(resolveLook({ style: 'flat' }, glass, 'wallpaper')).toBe('flat');
		expect(resolveLook({ style: 'glass' }, g, 'wallpaper')).toBe('glass');
		expect(resolveLook({ style: 'bogus' as never }, glass, 'wallpaper')).toBe('glass');
	});

	it('glass: saturated backdrop, still none when blur is off; scrim clamped', () => {
		expect(backdropValue(32, 'glass')).toBe('blur(32px) saturate(150%)');
		expect(backdropValue(0, 'glass')).toBe('none');
		expect(resolveScrim(g)).toBe(0.34);
		expect(resolveScrim({ ...g, scrim: 0.9 })).toBe(0.6);
		expect(resolveScrim({ ...g, scrim: NaN })).toBe(0.34);
	});

	it('glass surface opacity: user values still win, otherwise the contrast-safe glass default', () => {
		const theme = getTheme('catppuccin-mocha');
		expect(resolveSurfaceOpacity(undefined, g, theme, 'glass')).toBe(glassSurfaceOpacity(theme, 0.34));
		expect(resolveSurfaceOpacity(undefined, { ...g, surfaceOpacity: 0.4 }, theme, 'glass')).toBe(0.4);
		expect(resolveSurfaceOpacity({ opacity: 0.3 }, g, theme, 'glass')).toBe(0.3);
	});
});

describe('dim and burn-in (SPEC §2)', () => {
	const b = DEFAULT_SETTINGS.burnIn;
	const nightOn = { ...b, night: { ...b.night, enabled: true } };
	const idleOn = { ...b, idle: { ...b.idle, enabled: true } };

	it('night window wraps past midnight', () => {
		expect(nightActive(nightOn.night, at(23))).toBe(true);
		expect(nightActive(nightOn.night, at(6, 59))).toBe(true);
		expect(nightActive(nightOn.night, at(7))).toBe(false);
		expect(nightActive(b.night, at(23))).toBe(false);
	});

	it('takes the lowest active dim, never below 0.3', () => {
		expect(dimLevel(b, at(23), true)).toBe(1);
		expect(dimLevel(nightOn, at(23), false)).toBe(b.night.dim);
		expect(dimLevel(idleOn, at(12), true)).toBe(b.idle.dim);
		expect(dimLevel({ ...nightOn, idle: { ...b.idle, enabled: true, dim: 0.1 } }, at(23), true)).toBe(0.3);
	});

	it('pixel shift stays within ±4 px, moves at most 3 px per step and cycles', () => {
		for (let i = 0; i < SHIFT_PATTERN.length; i++) {
			const a = shiftAt(i);
			const n = shiftAt(i + 1);
			expect(Math.abs(a.x)).toBeLessThanOrEqual(4);
			expect(Math.abs(a.y)).toBeLessThanOrEqual(4);
			expect(Math.max(Math.abs(n.x - a.x), Math.abs(n.y - a.y))).toBeLessThanOrEqual(3);
		}
		expect(shiftAt(SHIFT_PATTERN.length)).toEqual(shiftAt(0));
		expect(shiftAt(-1)).toEqual(shiftAt(SHIFT_PATTERN.length - 1));
	});

	it('clock face drift stays inside the free space', () => {
		for (let m = 0; m < 60; m++) {
			const { fx, fy } = clockDrift(m);
			expect(fx).toBeGreaterThanOrEqual(0);
			expect(fx).toBeLessThanOrEqual(1);
			expect(fy).toBeGreaterThanOrEqual(0);
			expect(fy).toBeLessThanOrEqual(1);
		}
		expect(clockDrift(0)).not.toEqual(clockDrift(1));
	});
});

describe('hotkey field', () => {
	const key = (init: Partial<KeyboardEvent>) => ({ ctrlKey: false, altKey: false, shiftKey: false, metaKey: false, ...init }) as KeyboardEvent;

	it('records accelerators from key events', () => {
		expect(acceleratorFrom(key({ key: 'k', code: 'KeyK', ctrlKey: true, altKey: true }))).toBe('Ctrl+Alt+K');
		expect(acceleratorFrom(key({ key: '1', code: 'Digit1', shiftKey: true, metaKey: true }))).toBe('Shift+Super+1');
		expect(acceleratorFrom(key({ key: 'F5', code: 'F5', ctrlKey: true }))).toBe('Ctrl+F5');
		expect(acceleratorFrom(key({ key: 'Control', code: 'ControlLeft', ctrlKey: true }))).toBeNull();
	});

	it('validates before asking the backend', () => {
		expect(hotkeyProblem('')).toBe('empty');
		expect(hotkeyProblem('K')).toBe('modifier');
		expect(hotkeyProblem('Ctrl+Alt')).toBe('key');
		expect(hotkeyProblem('Ctrl+K+J')).toBe('key');
		expect(hotkeyProblem('Ctrl+Alt+K')).toBeNull();
		expect(hotkeyProblem('CommandOrControl+Shift+Space')).toBeNull();
	});
});
