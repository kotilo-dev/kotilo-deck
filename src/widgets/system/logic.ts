/** Pure helpers for the System widget (warn rule, history, sparkline, units). */

/** Consecutive samples needed to enter or leave the warning state (SysPulse rule, DESIGN §5.5). */
export const WARN_RUN = 3;
/** Sparkline window: 60 samples at 1 Hz. */
export const HISTORY = 60;

export interface WarnState {
	warn: boolean;
	/** Samples above the threshold: the current run before warning, the whole episode while warning. */
	over: number;
	/** Consecutive samples at/below the threshold while warning. */
	under: number;
}

export const initialWarn: WarnState = { warn: false, over: 0, under: 0 };

/**
 * value > threshold for 3 consecutive samples → warn; while warning, 3 consecutive samples
 * at or below the threshold → clear. A single dip or spike never flips the state.
 */
export function stepWarn(state: WarnState, value: number, threshold: number): WarnState {
	const isOver = value > threshold;
	if (!state.warn) {
		const over = isOver ? state.over + 1 : 0;
		return { warn: over >= WARN_RUN, over, under: 0 };
	}
	if (isOver) return { warn: true, over: state.over + 1, under: 0 };
	const under = state.under + 1;
	return under >= WARN_RUN ? { ...initialWarn } : { warn: true, over: state.over, under };
}

/** Appends `value`, keeping the newest `cap` entries. Returns a new array. */
export function pushHistory(history: readonly number[], value: number, cap = HISTORY): number[] {
	const next = history.length >= cap ? history.slice(history.length - cap + 1) : history.slice();
	next.push(value);
	return next;
}

const clampPct = (v: number) => Math.min(100, Math.max(0, Number.isFinite(v) ? v : 0));
const r1 = (n: number) => Math.round(n * 10) / 10;

/**
 * SVG polyline points for 0–100 values. Newest sample sits at the right edge; slots are spaced
 * for a full `capacity` window so the line grows in from the right. `pad` keeps a 2 px stroke
 * inside the box.
 */
export function sparklinePoints(values: readonly number[], width: number, height: number, capacity = HISTORY, pad = 1): string {
	if (values.length === 0) return '';
	const step = width / Math.max(1, capacity - 1);
	const span = height - pad * 2;
	const start = width - (values.length - 1) * step;
	return values
		.map((v, i) => `${r1(start + i * step)},${r1(pad + (1 - clampPct(v) / 100) * span)}`)
		.join(' ');
}

/**
 * SVG polygon points for the sparkline's area fill (DESIGN §5.5, §1.4 exception a): the line's points
 * closed down to the bottom edge, so the fill sits under the line and bleeds to the box's bottom.
 */
export function sparklineArea(values: readonly number[], width: number, height: number, capacity = HISTORY, pad = 1): string {
	if (values.length === 0) return '';
	const line = sparklinePoints(values, width, height, capacity, pad);
	const step = width / Math.max(1, capacity - 1);
	const start = r1(width - (values.length - 1) * step);
	return `${start},${height} ${line} ${r1(width)},${height}`;
}

/** Highest sample in the window, rounded (CPU tile "peak N% · 60 s"); null without samples. */
export const peak = (values: readonly number[]): number | null =>
	values.length ? Math.round(Math.max(...values.map(clampPct))) : null;

/** Core bar colour bucket: below 25 % info, 25 % up to the threshold accent, at/above the threshold warning. */
export type CoreBucket = 'low' | 'mid' | 'high';
export const coreBucket = (load: number, threshold: number): CoreBucket =>
	clampPct(load) >= threshold ? 'high' : clampPct(load) < 25 ? 'low' : 'mid';

/** Busiest core, 1-based for display; the first one wins a tie. Null without per-core data. */
export function busiestCore(perCore: readonly number[]): { core: number; load: number } | null {
	let best = -1;
	for (let i = 0; i < perCore.length; i++) if (best < 0 || clampPct(perCore[i]) > clampPct(perCore[best])) best = i;
	return best < 0 ? null : { core: best + 1, load: Math.round(clampPct(perCore[best])) };
}

export const percent = (part: number, total: number): number => (total > 0 ? clampPct((part / total) * 100) : 0);

const GIB = 1024 ** 3;
const MIB = 1024 ** 2;
const nf = new Map<string, Intl.NumberFormat>();
function num(locale: string, digits: number): Intl.NumberFormat {
	const key = `${locale}|${digits}`;
	let f = nf.get(key);
	if (!f) {
		f = new Intl.NumberFormat(locale, { minimumFractionDigits: digits, maximumFractionDigits: digits });
		nf.set(key, f);
	}
	return f;
}

/** Bytes → GiB with one decimal ("11.2", "11,2" in tr). */
export const gb = (bytes: number, locale: string, digits = 1): string => num(locale, digits).format(bytes / GIB);

/** Total memory, whole GiB unless that hides a fraction worth showing ("32", "15.5"). */
export function gbTotal(bytes: number, locale: string): string {
	const v = bytes / GIB;
	return num(locale, Math.abs(v - Math.round(v)) < 0.05 ? 0 : 1).format(v);
}

/** Process RAM: "820 MB" below 1 GiB, "1.4 GB" above. */
export const memLabel = (bytes: number, locale: string): string =>
	bytes < GIB ? `${num(locale, 0).format(bytes / MIB)} MB` : `${gb(bytes, locale)} GB`;
