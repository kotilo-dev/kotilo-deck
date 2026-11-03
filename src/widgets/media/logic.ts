/** Pure helpers for the Media widget: timeline extrapolation, time labels, repeat state. */
import type { MediaSession } from '#lib/ipc/index.ts';

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
