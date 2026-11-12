/**
 * Pure rules for the deck's appearance layers (SPEC §5.3, DESIGN §2/§8) and burn-in protection (SPEC §2).
 * No DOM, no timers: the deck runtime (`ambient.svelte.ts`) and the settings window both use these.
 */
import type { Appearance, BurnInSettings, DeckAppearance, LookStyle } from '#lib/ipc/index.ts';
import { glassSurfaceOpacity, inWindow, minuteOfDay, parseHm, type Theme } from '#lib/theme/index.ts';

export const clamp = (v: number, lo: number, hi: number): number => Math.min(hi, Math.max(lo, v));
const num = (v: unknown): v is number => typeof v === 'number' && Number.isFinite(v);

export type BackgroundMode = DeckAppearance['background'];

export interface ResolvedBackground {
	mode: BackgroundMode;
	/** Wallpaper file name (wallpaper mode only). */
	wallpaper: string | null;
}

/**
 * Background layer: a workspace `appearance.background` ("solid" | "transparent" | "wallpaper:<file>")
 * overrides the global setting. Wallpaper mode without a file falls back to solid.
 */
export function resolveBackground(workspace: Appearance | undefined, global: DeckAppearance): ResolvedBackground {
	const spec = workspace?.background;
	if (typeof spec === 'string') {
		if (spec === 'solid' || spec === 'transparent') return { mode: spec, wallpaper: null };
		if (spec.startsWith('wallpaper:') && spec.length > 'wallpaper:'.length) {
			return { mode: 'wallpaper', wallpaper: spec.slice('wallpaper:'.length) };
		}
	}
	if (global.background === 'wallpaper') {
		return global.wallpaper ? { mode: 'wallpaper', wallpaper: global.wallpaper } : { mode: 'solid', wallpaper: null };
	}
	return { mode: global.background === 'transparent' ? 'transparent' : 'solid', wallpaper: null };
}

/**
 * Effective card look (DESIGN §4.5): workspace `style` > global. Glass needs something behind the cards,
 * so over a solid background it renders flat (the settings page says so).
 */
export function resolveLook(workspace: Appearance | undefined, global: DeckAppearance, background: BackgroundMode): LookStyle {
	const style = workspace?.style === 'flat' || workspace?.style === 'glass' ? workspace.style : global.style;
	return style === 'glass' && background !== 'solid' ? 'glass' : 'flat';
}

/** Glass wallpaper scrim (0–0.6), default 0.34. */
export const resolveScrim = (global: DeckAppearance): number => (num(global.scrim) ? clamp(global.scrim, 0, 0.6) : 0.34);

/** Blur the settings page proposes when Glass is picked while blur is still off. */
export const GLASS_BLUR = 32;

/**
 * Surface opacity without an instance override: workspace > global setting > theme (0–1). In the glass
 * look the theme fallback is the per-theme contrast-safe glass opacity instead (`glassSurfaceOpacity`).
 */
export function resolveSurfaceOpacity(
	workspace: Appearance | undefined,
	global: DeckAppearance,
	theme: Theme,
	look: LookStyle = 'flat'
): number {
	if (num(workspace?.opacity)) return clamp(workspace.opacity, 0, 1);
	if (num(global.surfaceOpacity)) return clamp(global.surfaceOpacity, 0, 1);
	return look === 'glass' ? glassSurfaceOpacity(theme, resolveScrim(global)) : theme.opacity.surface;
}

/** Backdrop blur px (0 = off) for one card: instance > workspace > global. */
export function resolveBlur(instance: Appearance | undefined, workspace: Appearance | undefined, global: DeckAppearance): number {
	const v = num(instance?.blur) ? instance.blur : num(workspace?.blur) ? workspace.blur : global.blur;
	return num(v) ? clamp(Math.round(v), 0, 40) : 0;
}

/** CSS value for `--k-backdrop`; `none` keeps the GPU idle when blur is off. Glass adds `saturate(150%)`. */
export const backdropValue = (px: number, look: LookStyle = 'flat'): string =>
	px > 0 ? (look === 'glass' ? `blur(${px}px) saturate(150%)` : `blur(${px}px)`) : 'none';

/** True while `now` is inside the night window. */
export function nightActive(night: BurnInSettings['night'], now: Date): boolean {
	if (!night.enabled) return false;
	const from = parseHm(night.from);
	const to = parseHm(night.to);
	return from !== null && to !== null && inWindow(minuteOfDay(now), from, to);
}

/** `--k-dim` (0.3–1): the lowest of the active night and idle dims. */
export function dimLevel(burnIn: BurnInSettings, now: Date, idle: boolean): number {
	let dim = 1;
	if (nightActive(burnIn.night, now) && num(burnIn.night.dim)) dim = Math.min(dim, burnIn.night.dim);
	if (idle && burnIn.idle.enabled && num(burnIn.idle.dim)) dim = Math.min(dim, burnIn.idle.dim);
	return clamp(dim, 0.3, 1);
}

/**
 * Pixel-shift offsets (px), all within ±4. Consecutive steps move 2–3 px so a shift is never noticeable,
 * and the cycle visits every quadrant before returning to the origin.
 */
export const SHIFT_PATTERN: readonly { x: number; y: number }[] = [
	{ x: 0, y: 0 },
	{ x: 2, y: 1 },
	{ x: 4, y: -1 },
	{ x: 2, y: -3 },
	{ x: 0, y: -4 },
	{ x: -2, y: -2 },
	{ x: -4, y: 0 },
	{ x: -3, y: 2 },
	{ x: -1, y: 4 },
	{ x: 1, y: 3 },
	{ x: 3, y: 4 },
	{ x: 2, y: 2 }
];

export const shiftAt = (step: number): { x: number; y: number } =>
	SHIFT_PATTERN[((step % SHIFT_PATTERN.length) + SHIFT_PATTERN.length) % SHIFT_PATTERN.length];

/**
 * Screensaver clock position as fractions (0–1) of the free space; changes every minute and walks
 * a slow figure so no pixel stays lit by the same digit.
 */
export function clockDrift(minute: number): { fx: number; fy: number } {
	const a = (minute % 60) / 60;
	return { fx: 0.5 + 0.45 * Math.sin(a * 2 * Math.PI), fy: 0.5 + 0.4 * Math.sin(a * 4 * Math.PI + Math.PI / 3) };
}

/** Pixel-shift interval choices (minutes; 0 = off) offered in settings. */
export const SHIFT_CHOICES = [0, 2, 5, 10, 15, 30, 60] as const;
export const IDLE_CHOICES = [1, 2, 5, 10, 15, 30, 60] as const;
