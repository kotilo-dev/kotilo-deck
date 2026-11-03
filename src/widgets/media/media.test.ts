import { describe, expect, it } from 'vitest';
import type { MediaSession } from '#lib/ipc/index.ts';
import { applyOverrides, byline, currentPosition, formatTime, HOLD_MS, nextRepeat, progress, repeatKey, seekSeconds } from './logic';

const session = (over: Partial<MediaSession> = {}): MediaSession => ({
	source: 'Spotify',
	title: 'Weightless',
	artist: 'Marconi Union',
	album: null,
	artwork: null,
	playing: true,
	position: 100,
	duration: 480,
	positionAt: 10_000,
	shuffle: false,
	repeat: 'none',
	canPrev: true,
	canNext: true,
	canPlayPause: true,
	...over
});

describe('timeline', () => {
	it('extrapolates while playing and holds while paused', () => {
		expect(currentPosition(session(), 10_000)).toBe(100);
		expect(currentPosition(session(), 22_500)).toBe(112.5);
		expect(currentPosition(session({ playing: false }), 99_000)).toBe(100);
	});

	it('clamps to the duration and never goes negative', () => {
		expect(currentPosition(session(), 10_000 + 1_000_000)).toBe(480);
		expect(currentPosition(session({ position: 5 }), 0)).toBe(5); // clock skew: positionAt in the future
		expect(currentPosition(session({ duration: null }), 20_000)).toBe(110);
		expect(currentPosition(session({ position: null }), 20_000)).toBeNull();
	});

	it('computes progress only with a usable duration', () => {
		expect(progress(120, 480)).toBe(0.25);
		expect(progress(600, 480)).toBe(1);
		expect(progress(10, null)).toBeNull();
		expect(progress(10, 0)).toBeNull();
		expect(progress(null, 480)).toBeNull();
	});

	it('formats times', () => {
		expect(formatTime(192)).toBe('3:12');
		expect(formatTime(59.9)).toBe('0:59');
		expect(formatTime(3723)).toBe('1:02:03');
		expect(formatTime(null)).toBe('0:00');
		expect(formatTime(Number.NaN)).toBe('0:00');
		expect(formatTime(-4)).toBe('0:00');
	});

	it('maps a fraction to a seek target', () => {
		expect(seekSeconds(0.5, 480)).toBe(240);
		expect(seekSeconds(1.2, 480)).toBe(480);
		expect(seekSeconds(-1, 480)).toBe(0);
		expect(seekSeconds(1 / 3, 100)).toBe(33.3);
	});
});

describe('controls', () => {
	it('labels and cycles repeat', () => {
		expect(repeatKey('none')).toBe('widgets.media.repeatOff');
		expect(repeatKey('list')).toBe('widgets.media.repeatList');
		expect(repeatKey('track')).toBe('widgets.media.repeatTrack');
		expect(nextRepeat('none')).toBe('list');
		expect(nextRepeat('list')).toBe('track');
		expect(nextRepeat('track')).toBe('none');
		expect(nextRepeat(null)).toBeNull();
	});

	it('joins the byline without empty parts', () => {
		expect(byline('Marconi Union', 'Spotify')).toBe('Marconi Union · Spotify');
		expect(byline('', 'Microsoft Edge')).toBe('Microsoft Edge');
		expect(byline(' ', null, undefined)).toBe('');
	});
});

describe('applyOverrides', () => {
	it('flips play state locally and freezes the position at the click', () => {
		const s = session();
		const out = applyOverrides(s, { playing: { value: false, at: 12_000 } }, 12_100);
		expect(out.playing).toBe(false);
		expect(currentPosition(out, 20_000)).toBe(102);
	});

	it('drops overrides after the hold or once the backend agrees', () => {
		const s = session();
		expect(applyOverrides(s, { playing: { value: false, at: 12_000 } }, 12_000 + HOLD_MS).playing).toBe(true);
		expect(applyOverrides(session({ playing: false }), { playing: { value: false, at: 12_000 } }, 12_100).positionAt).toBe(10_000);
	});

	it('keeps a local seek until a newer backend reading arrives', () => {
		const o = { seek: { position: 300, at: 15_000 } };
		expect(currentPosition(applyOverrides(session(), o, 15_500), 15_500)).toBe(300.5);
		expect(applyOverrides(session({ positionAt: 15_200, position: 301 }), o, 15_500).position).toBe(301);
	});

	it('ignores shuffle/repeat overrides when the source has no such control', () => {
		const o = { shuffle: { value: true, at: 1 }, repeat: { value: 'list' as const, at: 1 } };
		expect(applyOverrides(session(), o, 2)).toMatchObject({ shuffle: true, repeat: 'list' });
		expect(applyOverrides(session({ shuffle: null, repeat: null }), o, 2)).toMatchObject({ shuffle: null, repeat: null });
	});
});
