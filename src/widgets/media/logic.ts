/** Pure helpers for the Media widget: timeline extrapolation, time labels, repeat state. */
import type { MediaSession } from '#lib/ipc/index.ts';
import { contrastRatio, hexToRgb, mixHex } from '#lib/theme/index.ts';

/** After a local action (play/pause, seek), backend values that disagree are ignored this long. */
export const HOLD_MS = 1500;

type Timeline = Pick<MediaSession, 'playing' | 'position' | 'duration' | 'positionAt'>;

/**
 * Position in seconds at `now`: the backend's `position` read at `positionAt`, advanced by the elapsed
 * time while playing and clamped to [0, duration]. Null when the source reports no timeline.
 */
export function currentPosition(s: Timeline, now: number): number | null {
	if (s.position === null) return null;
	const elapsed = s.playing ? Math.max(0, now - s.positionAt) / 1000 : 0;
	const p = Math.max(0, s.position + elapsed);
	return s.duration !== null && s.duration > 0 ? Math.min(p, s.duration) : p;
}

/** 0–1 progress, or null without a usable timeline. */
export function progress(position: number | null, duration: number | null): number | null {
	if (position === null || duration === null || !(duration > 0)) return null;
	return Math.min(1, Math.max(0, position / duration));
}

/** 192 → "3:12", 3723 → "1:02:03", NaN → "0:00". */
export function formatTime(seconds: number | null): string {
	const s = Number.isFinite(seconds) && seconds! > 0 ? Math.floor(seconds!) : 0;
	const h = Math.floor(s / 3600);
	const m = Math.floor((s % 3600) / 60);
	const sec = String(s % 60).padStart(2, '0');
	return h > 0 ? `${h}:${String(m).padStart(2, '0')}:${sec}` : `${m}:${sec}`;
}

/** Seek target in seconds for a 0–1 fraction of the duration. */
export const seekSeconds = (fraction: number, duration: number): number =>
	Math.round(Math.min(1, Math.max(0, fraction)) * Math.max(0, duration) * 10) / 10;

/** i18n key for the repeat button's label. */
export const repeatKey = (r: MediaSession['repeat']): string =>
	r === 'track' ? 'widgets.media.repeatTrack' : r === 'list' ? 'widgets.media.repeatList' : 'widgets.media.repeatOff';

/** Optimistic next repeat mode for `cycle-repeat` (Windows order: none → list → track → none). */
export const nextRepeat = (r: MediaSession['repeat']): MediaSession['repeat'] =>
	r === null ? null : r === 'none' ? 'list' : r === 'list' ? 'track' : 'none';

/** "Artist · Source" without dangling separators when a field is empty. */
export const byline = (...parts: (string | null | undefined)[]): string =>
	parts
		.map((p) => p?.trim())
		.filter(Boolean)
		.join(' · ');

/**
 * Applies optimistic local overrides (play state, seek) on top of a backend session. Each override
 * expires `HOLD_MS` after it was made, or as soon as the backend agrees with it.
 */
export interface Overrides {
	playing?: { value: boolean; at: number };
	seek?: { position: number; at: number };
	shuffle?: { value: boolean; at: number };
	repeat?: { value: MediaSession['repeat']; at: number };
}

export function applyOverrides(s: MediaSession, o: Overrides, now: number): MediaSession {
	let out = s;
	if (o.playing && now - o.playing.at < HOLD_MS && o.playing.value !== s.playing) {
		// Keep the extrapolated position continuous when flipping play state locally.
		const pos = currentPosition(s, o.playing.at);
		out = { ...out, playing: o.playing.value, position: pos, positionAt: o.playing.at };
	}
	if (o.seek && now - o.seek.at < HOLD_MS && s.positionAt < o.seek.at) {
		out = { ...out, position: o.seek.position, positionAt: o.seek.at };
	}
	if (o.shuffle && now - o.shuffle.at < HOLD_MS && s.shuffle !== null) out = { ...out, shuffle: o.shuffle.value };
	if (o.repeat && now - o.repeat.at < HOLD_MS && s.repeat !== null) out = { ...out, repeat: o.repeat.value };
	return out;
}

// ── Artwork tint (DESIGN §5.7, flat look only) ─────────────────────────────

export type Rgb = [number, number, number];

/**
 * Average colour of RGBA pixels (a downscaled artwork), weighting by alpha so transparent corners don't
 * pull it toward black. Null when every pixel is transparent.
 */
export function averageColor(data: ArrayLike<number>): Rgb | null {
	let r = 0;
	let g = 0;
	let b = 0;
	let w = 0;
	for (let i = 0; i + 3 < data.length; i += 4) {
		const a = data[i + 3] / 255;
		r += data[i] * a;
		g += data[i + 1] * a;
		b += data[i + 2] * a;
		w += a;
	}
	return w > 0 ? [Math.round(r / w), Math.round(g / w), Math.round(b / w)] : null;
}

function rgbToHsl([r, g, b]: Rgb): [number, number, number] {
	const [R, G, B] = [r / 255, g / 255, b / 255];
	const max = Math.max(R, G, B);
	const min = Math.min(R, G, B);
	const l = (max + min) / 2;
	if (max === min) return [0, 0, l];
	const d = max - min;
	const s = l > 0.5 ? d / (2 - max - min) : d / (max + min);
	const h = max === R ? (G - B) / d + (G < B ? 6 : 0) : max === G ? (B - R) / d + 2 : (R - G) / d + 4;
	return [h / 6, s, l];
}

function hslToRgb([h, s, l]: [number, number, number]): Rgb {
	if (s === 0) return [Math.round(l * 255), Math.round(l * 255), Math.round(l * 255)];
	const q = l < 0.5 ? l * (1 + s) : l + s - l * s;
	const p = 2 * l - q;
	const f = (t: number) => {
		const x = t < 0 ? t + 1 : t > 1 ? t - 1 : t;
		if (x < 1 / 6) return p + (q - p) * 6 * x;
		if (x < 1 / 2) return q;
		if (x < 2 / 3) return p + (q - p) * (2 / 3 - x) * 6;
		return p;
	};
	return [Math.round(f(h + 1 / 3) * 255), Math.round(f(h) * 255), Math.round(f(h - 1 / 3) * 255)];
}

/** Share of the artwork colour in the flat card's background and border (DESIGN §5.7). */
export const TINT_SHARE = 0.3;

const toHex = (c: Rgb) => `#${c.map((v) => v.toString(16).padStart(2, '0')).join('')}`;

/**
 * Artwork colour safe to tint the card with in this theme. Saturation is capped at 0.75 (neon covers would
 * glow); then the colour is pulled toward the surface in 10 % steps until, mixed 30 % into the surface, text
 * keeps 4.5:1 and muted text 3:1 (or the plain surface's own ratio when a theme starts below that). The
 * pull ends at the surface itself, so the result always passes.
 */
export function tintColor(rgb: Rgb, theme: { surface: string; text: string; muted: string }): Rgb {
	const [h, sat, l] = rgbToHsl(rgb);
	const base = hslToRgb([h, Math.min(sat, 0.75), l]);
	const surface = hexToRgbTuple(theme.surface);
	const need = (fg: string, min: number) => Math.min(min, contrastRatio(fg, theme.surface));
	const needText = need(theme.text, 4.5);
	const needMuted = need(theme.muted, 3);
	for (let k = 0; k <= 10; k++) {
		const c = base.map((v, i) => Math.round(v + (surface[i] - v) * (k / 10))) as Rgb;
		const card = mixHex(toHex(c), theme.surface, TINT_SHARE);
		if (contrastRatio(theme.text, card) >= needText && contrastRatio(theme.muted, card) >= needMuted) return c;
	}
	return surface;
}

const hexToRgbTuple = (hex: string): Rgb => {
	const [r, g, b] = hexToRgb(hex);
	return [r, g, b];
};

/** CSS value for the inline `--art` property (an `rgb()` function, so `lint:hex` stays clean). */
export const rgbCss = ([r, g, b]: Rgb): string => `rgb(${r} ${g} ${b})`;
