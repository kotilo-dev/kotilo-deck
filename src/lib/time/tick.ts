/**
 * One shared wall-clock tick for every widget (clocks, "N min ago" labels).
 *
 * A single `setTimeout` chain is aligned to the next second boundary while any subscriber
 * wants seconds, otherwise to the next minute boundary, so an always-on deck idles at one
 * wake-up per minute. The chain is paused while the deck is hidden (`deck://visibility`,
 * with `document.hidden` as a secondary signal) and fires immediately when it is shown again.
 */
import { isDeckVisible, onDeckVisibility } from './visibility';

export type TickHandler = (now: Date) => void;

interface Subscriber {
	cb: TickHandler;
	seconds: boolean;
}

/** Fire a few ms after the boundary so formatted values have definitely rolled over. */
const SLACK_MS = 5;

const subs = new Set<Subscriber>();
let timer: ReturnType<typeof setTimeout> | null = null;
let stopVisibility: (() => void) | null = null;

const hidden = (): boolean => !isDeckVisible();

function wantsSeconds(): boolean {
	for (const s of subs) if (s.seconds) return true;
	return false;
}

/** Current timer step in ms (1000 or 60000), or null when idle. Exposed for tests. */
export function tickInterval(): 1000 | 60000 | null {
	if (subs.size === 0 || timer === null) return null;
	return wantsSeconds() ? 1000 : 60000;
}

/** Milliseconds from `nowMs` to the next multiple of `step`, plus slack. */
export function delayToBoundary(nowMs: number, step: number): number {
	return step - (nowMs % step) + SLACK_MS;
}

function clear(): void {
	if (timer !== null) clearTimeout(timer);
	timer = null;
}

function schedule(): void {
	clear();
	if (subs.size === 0 || hidden()) return;
	timer = setTimeout(fire, delayToBoundary(Date.now(), wantsSeconds() ? 1000 : 60000));
}

function fire(): void {
	timer = null;
	const now = new Date();
	for (const s of [...subs]) s.cb(now);
	schedule();
}

function onVisibility(visible: boolean): void {
	if (visible) fire();
	else clear();
}

function setListening(on: boolean): void {
	if (on === (stopVisibility !== null)) return;
	if (on) stopVisibility = onDeckVisibility(onVisibility);
	else {
		stopVisibility?.();
		stopVisibility = null;
	}
}

/**
 * Calls `cb` on every second (when `seconds`) or minute boundary. Returns the unsubscribe
 * function, so it can be returned straight from a Svelte `$effect`.
 */
export function subscribeTick(cb: TickHandler, opts: { seconds?: boolean } = {}): () => void {
	const sub: Subscriber = { cb, seconds: opts.seconds ?? false };
	subs.add(sub);
	setListening(true);
	schedule();
	return () => {
		if (!subs.delete(sub)) return;
		if (subs.size === 0) {
			clear();
			setListening(false);
		} else {
			schedule();
		}
	};
}
