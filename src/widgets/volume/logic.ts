/** Pure-ish helpers for the Volume widget: throttled sends, optimistic values, device list. */
import type { AudioDevice, AudioSession } from '#lib/ipc/index.ts';

/** ~30 volume writes per second while dragging. */
export const THROTTLE_MS = 33;
/** After the last local change, remote values are ignored this long so the slider doesn't jump back. */
export const HOLD_MS = 700;
/** Mixer sheet refresh cadence while open (the contract has no per-session events). */
export const MIXER_REFRESH_MS = 2000;

export interface Throttle<T> {
	/** Sends now if the interval has passed, else keeps the latest value for a trailing send. */
	push(value: T): void;
	/** Sends a pending trailing value immediately (pointer up / change). */
	flush(): void;
	cancel(): void;
}

/** Leading + trailing throttle: the first and the last value of a drag always go out. */
export function createThrottle<T>(send: (value: T) => void, intervalMs = THROTTLE_MS): Throttle<T> {
	let last = -Infinity;
	let pending: { value: T } | null = null;
	let timer: ReturnType<typeof setTimeout> | null = null;

	const fire = () => {
		timer = null;
		if (!pending) return;
		const { value } = pending;
		pending = null;
		last = Date.now();
		send(value);
	};

	return {
		push(value) {
			const now = Date.now();
			if (now - last >= intervalMs && timer === null) {
				pending = null;
				last = now;
				send(value);
				return;
			}
			pending = { value };
			if (timer === null) timer = setTimeout(fire, Math.max(0, intervalMs - (now - last)));
		},
		flush() {
			if (timer !== null) clearTimeout(timer);
			fire();
		},
		cancel() {
			if (timer !== null) clearTimeout(timer);
			timer = null;
			pending = null;
		}
	};
}

/**
 * Optimistic UI value: a local value wins over the remote one until `HOLD_MS` after the last local
 * change (and for as long as the user is still holding the control). Then the remote value — the
 * backend's `audio://state` — is shown again, which reconciles any write that didn't take.
 */
export interface Optimistic<T> {
	set(value: T): void;
	/** Pointer down/up on the control: no settling while held. */
	hold(held: boolean): void;
	/** Local value if still fresh, else `remote`. */
	value(remote: T): T;
	readonly active: boolean;
	dispose(): void;
}

export function createOptimistic<T>(onSettle: () => void, holdMs = HOLD_MS): Optimistic<T> {
	let local: { value: T } | null = null;
	let held = false;
	let timer: ReturnType<typeof setTimeout> | null = null;

	const clear = () => {
		if (timer !== null) clearTimeout(timer);
		timer = null;
	};
	const arm = () => {
		clear();
		if (held || !local) return;
		timer = setTimeout(() => {
			timer = null;
			local = null;
			onSettle();
		}, holdMs);
	};

	return {
		set(value) {
			local = { value };
			arm();
		},
		hold(h) {
			held = h;
			arm();
		},
		value: (remote) => (local ? local.value : remote),
		get active() {
			return local !== null;
		},
		dispose() {
			clear();
			local = null;
		}
	};
}

/** 0–1 → 0–100 integer. */
export const toPct = (v: number): number => Math.round(Math.min(1, Math.max(0, Number.isFinite(v) ? v : 0)) * 100);
/** Slider 0–100 → 0–1. */
export const fromPct = (p: number): number => Math.min(100, Math.max(0, Math.round(p))) / 100;

export interface DeviceRows {
	rows: AudioDevice[];
	/** True when some devices didn't fit: the last slot becomes "More devices…". */
	more: boolean;
}

/** 2×2 radio list: default device first, then the rest in OS order; `slots` rows fit. */
export function deviceRows(devices: readonly AudioDevice[], defaultId: string | null, slots: number): DeviceRows {
	if (slots <= 0) return { rows: [], more: devices.length > 0 };
	const ordered = [...devices.filter((d) => d.id === defaultId), ...devices.filter((d) => d.id !== defaultId)];
	if (ordered.length <= slots) return { rows: ordered, more: false };
	return { rows: ordered.slice(0, Math.max(0, slots - 1)), more: true };
}

/** Mixer rows: stable alphabetical order so rows don't jump between refreshes. */
export const sortSessions = (sessions: readonly AudioSession[]): AudioSession[] =>
	[...sessions].sort((a, b) => a.name.localeCompare(b.name) || a.id.localeCompare(b.id));

/**
 * Merges a refreshed session list with rows the user touched in the last `HOLD_MS`
 * (their local volume/mute stay until the hold passes).
 */
export function mergeSessions(
	remote: readonly AudioSession[],
	local: readonly AudioSession[],
	touchedAt: ReadonlyMap<string, number>,
	now: number,
	holdMs = HOLD_MS
): AudioSession[] {
	const mine = new Map(local.map((s) => [s.id, s]));
	return sortSessions(
		remote.map((r) => {
			const t = touchedAt.get(r.id);
			const l = mine.get(r.id);
			return l && t !== undefined && now - t < holdMs ? { ...r, volume: l.volume, muted: l.muted } : r;
		})
	);
}

/** Speaker glyph level: 0 = muted/silent (slash), 1 = low, 2 = high. */
export const speakerLevel = (volume: number, muted: boolean): 0 | 1 | 2 => (muted || volume <= 0 ? 0 : volume < 0.5 ? 1 : 2);
