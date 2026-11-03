/**
 * Deck runtime for everything that depends on wall-clock time or user presence (SPEC §2, §5.3):
 * night/idle dim, pixel shift, screensaver clock face, and the light/dark auto switch.
 *
 * Driven only by the shared minute tick (`#lib/time`), so it adds no timers of its own. Everything
 * pauses while the deck window is hidden (`deck://visibility`, with `document.hidden` as a fallback).
 */
import type { DeckSettings } from '#lib/ipc/index.ts';
import { on } from '#lib/ipc/index.ts';
import { subscribeTick } from '#lib/time/index.ts';
import { wantedVariant } from '#lib/theme/index.ts';
import { dimLevel, shiftAt } from './look';

export interface AmbientOptions {
	settings: () => DeckSettings;
	/** false in the settings window: only the theme auto switch is needed there. */
	burnIn?: boolean;
}

const ACTIVITY = ['pointerdown', 'pointermove', 'wheel', 'touchstart'] as const;

export class Ambient {
	/** Minute resolution. */
	now = $state(new Date());
	osDark = $state(false);
	visible = $state(true);
	idle = $state(false);
	/** Index into SHIFT_PATTERN. */
	step = $state(0);

	readonly #settings: () => DeckSettings;
	readonly #burnIn: boolean;
	#lastActivity = Date.now();
	#minutesSinceShift = 0;
	#untick: (() => void) | null = null;
	#hostVisible = true;

	dim = $derived.by(() => (this.#burnIn ? dimLevel(this.#settings().burnIn, this.now, this.idle) : 1));
	shift = $derived.by(() => (this.#burnIn && this.#settings().burnIn.shiftMinutes > 0 ? shiftAt(this.step) : { x: 0, y: 0 }));
	clockFace = $derived.by(() => {
		const idle = this.#settings().burnIn.idle;
		return this.#burnIn && this.idle && idle.enabled && idle.clockFace;
	});
	wanted = $derived.by(() => wantedVariant(this.#settings().themeAuto, this.now, this.osDark));

	constructor(opts: AmbientOptions) {
		this.#settings = opts.settings;
		this.#burnIn = opts.burnIn ?? true;
	}

	/** Installs listeners; returns the cleanup. */
	start(): () => void {
		const scheme = matchMedia('(prefers-color-scheme: dark)');
		const onScheme = () => (this.osDark = scheme.matches);
		onScheme();
		scheme.addEventListener('change', onScheme);

		const onActivity = (e: Event) => {
			this.#lastActivity = Date.now();
			// Taps on the wake layer wake on click instead, so the tap never reaches a widget underneath.
			if (this.idle && !(e.target instanceof Element && e.target.closest('[data-wake]'))) this.wake();
		};
		// The key that wakes the deck is swallowed, so it doesn't also trigger a shortcut ("E" = edit).
		const onKey = (e: KeyboardEvent) => {
			const wasIdle = this.idle;
			onActivity(e);
			if (wasIdle) {
				e.preventDefault();
				e.stopImmediatePropagation();
			}
		};
		if (this.#burnIn) {
			for (const ev of ACTIVITY) window.addEventListener(ev, onActivity, { capture: true, passive: true });
			window.addEventListener('keydown', onKey, { capture: true });
		}

		const onDocVisibility = () => this.#setVisible(this.#hostVisible);
		document.addEventListener('visibilitychange', onDocVisibility);
		// Only the deck follows the deck window's visibility; the settings window keeps its theme tick.
		const unlisten = this.#burnIn
			? on('deck://visibility', ({ visible }) => {
					this.#hostVisible = visible;
					this.#setVisible(visible);
				})
			: Promise.resolve(() => {});

		this.#setVisible(true);
		return () => {
			scheme.removeEventListener('change', onScheme);
			for (const ev of ACTIVITY) window.removeEventListener(ev, onActivity, { capture: true });
			window.removeEventListener('keydown', onKey, { capture: true });
			document.removeEventListener('visibilitychange', onDocVisibility);
			void unlisten.then((off) => off());
			this.#untick?.();
			this.#untick = null;
		};
	}

	wake(): void {
		this.#lastActivity = Date.now();
		this.idle = false;
	}

	#setVisible(host: boolean) {
		const visible = host && !document.hidden;
		this.visible = visible;
		if (!visible) {
			this.#untick?.();
			this.#untick = null;
			return;
		}
		if (this.#untick) return;
		this.wake();
		this.now = new Date();
		this.#untick = subscribeTick((d) => this.#onTick(d));
	}

	#onTick(d: Date) {
		// The shared tick may run at 1 Hz for clocks with seconds; only act on minute changes.
		if (d.getMinutes() === this.now.getMinutes() && d.getTime() - this.now.getTime() < 60_000) return;
		this.now = d;
		if (!this.#burnIn) return;
		const s = this.#settings().burnIn;
		if (s.shiftMinutes > 0 && ++this.#minutesSinceShift >= s.shiftMinutes) {
			this.#minutesSinceShift = 0;
			this.step++;
		}
		if (s.idle.enabled && !this.idle && Date.now() - this.#lastActivity >= s.idle.minutes * 60_000) this.idle = true;
		if (!s.idle.enabled && this.idle) this.idle = false;
	}
}
