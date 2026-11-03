/**
 * "Is the deck on screen?" for timers and backend subscriptions (idle budget, SPEC §3).
 *
 * Two signals, combined with AND:
 * - `deck://visibility` from the backend (tray, hotkey, close-to-tray). Primary: WebView2 does not
 *   reliably flip `document.hidden` when the native window is hidden.
 * - `document.hidden` (minimised, browser preview, other webviews). Secondary.
 *
 * Subscribers are told only when the combined value changes. Listeners on both sources exist only
 * while at least one subscriber does.
 */
import { getCurrentWindow } from '@tauri-apps/api/window';
import { on } from '#lib/ipc/index.ts';

export type VisibilityHandler = (visible: boolean) => void;

interface DocLike {
	readonly hidden: boolean;
	addEventListener(type: 'visibilitychange', cb: () => void): void;
	removeEventListener(type: 'visibilitychange', cb: () => void): void;
}

export interface VisibilityEnv {
	/** Absent outside a browser (SSR, unit tests). */
	doc?: DocLike;
	/** Subscribes to the backend's deck show/hide events; resolves to the unlisten function. */
	listenDeck: (cb: (visible: boolean) => void) => Promise<() => void>;
}

export interface Visibility {
	isVisible(): boolean;
	/** Returns the unsubscribe function (usable as a Svelte `$effect` teardown). */
	subscribe(cb: VisibilityHandler): () => void;
}

export function createVisibility(env: VisibilityEnv): Visibility {
	const subs = new Set<VisibilityHandler>();
	/** Last `deck://visibility` value; the deck starts shown. */
	let deckShown = true;
	let last = true;
	let unlistenDeck: (() => void) | null = null;
	/** Bumped on stop so a late `listenDeck` resolution is dropped. */
	let generation = 0;

	const compute = () => deckShown && !(env.doc?.hidden ?? false);

	function update(): void {
		const next = compute();
		if (next === last) return;
		last = next;
		for (const cb of [...subs]) cb(next);
	}

	const onDoc = () => update();

	function start(): void {
		const gen = ++generation;
		last = compute();
		env.doc?.addEventListener('visibilitychange', onDoc);
		env
			.listenDeck((visible) => {
				deckShown = visible;
				update();
			})
			.then(
				(u) => (gen === generation ? (unlistenDeck = u) : u()),
				() => {}
			);
	}

	function stop(): void {
		generation++;
		env.doc?.removeEventListener('visibilitychange', onDoc);
		unlistenDeck?.();
		unlistenDeck = null;
		// Events are missed while nobody listens: assume shown on restart rather than stay paused forever.
		deckShown = true;
	}

	return {
		isVisible: () => (subs.size > 0 ? last : compute()),
		subscribe(cb) {
			if (subs.size === 0) start();
			subs.add(cb);
			return () => {
				if (!subs.delete(cb)) return;
				if (subs.size === 0) stop();
			};
		}
	};
}

/**
 * `deck://visibility` describes the deck window only. Other windows (settings) ignore it, so their
 * timers (e.g. the theme auto-switch tick) keep running while the deck is hidden.
 */
export function isDeckWindow(): boolean {
	if (typeof window === 'undefined' || !('__TAURI_INTERNALS__' in window)) return true;
	try {
		return getCurrentWindow().label === 'deck';
	} catch {
		return true;
	}
}

const deck = createVisibility({
	doc: typeof document === 'undefined' ? undefined : document,
	listenDeck: (cb) => (isDeckWindow() ? on('deck://visibility', (p) => cb(p.visible)) : Promise.resolve(() => {}))
});

/** True while the deck window is shown and the page is not hidden. */
export const isDeckVisible = (): boolean => deck.isVisible();

/** Calls `cb(visible)` whenever the deck is shown or hidden. Returns the unsubscribe function. */
export const onDeckVisibility = (cb: VisibilityHandler): (() => void) => deck.subscribe(cb);
