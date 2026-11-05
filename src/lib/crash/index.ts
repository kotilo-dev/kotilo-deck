/**
 * Frontend error reporting (SPEC §8 Phase 5 "crash log"): uncaught `error` / `unhandledrejection` events
 * go to the backend tracing log via `log_frontend_error`. Host only; failures are swallowed and the hook
 * never reports its own errors, so it can't loop.
 */

export type ErrorLogger = (message: string, stack: string | null) => Promise<unknown>;

export interface HookOptions {
	/** Same message again within this window is dropped (render loops). Default 2000 ms. */
	repeatMs?: number;
	/** At most this many reports per minute from this window (the backend also caps at 20). Default 20. */
	perMinute?: number;
	now?: () => number;
}

const MAX_MESSAGE = 500;
const MAX_STACK = 4000;
/** Thrown/rejected strings are free text from anywhere: keep a short prefix only. */
const MAX_STRING = 200;

const clip = (s: string, n: number) => (s.length > n ? s.slice(0, n) : s);

/**
 * `object (Response)`, `number`, `function`…: what a non-Error value was, never what it contained
 * (objects can carry tokens, settings, response bodies into the log).
 */
export function typeDescription(value: unknown): string {
	const type = typeof value;
	if (type !== 'object') return type;
	let name = '';
	try {
		const ctor = (Object.getPrototypeOf(value) as { constructor?: unknown } | null)?.constructor;
		if (typeof ctor === 'function' && typeof ctor.name === 'string') name = ctor.name;
	} catch {
		// Proxies / exotic objects: the type alone is enough.
	}
	return /^[\w$]{1,60}$/.test(name) ? `object (${name})` : 'object';
}

/**
 * Message + stack from whatever was thrown or rejected. Errors keep name, message and stack; strings are
 * clipped to 200 chars; anything else is logged as `<fallback>: <type description>` without its contents.
 */
export function describeError(value: unknown, fallback = 'Unknown error'): { message: string; stack: string | null } {
	let isError = false;
	try {
		isError = value instanceof Error;
	} catch {
		// A Proxy's getPrototypeOf trap may throw; treat it as a plain object.
	}
	if (isError && value instanceof Error) {
		const message = value.message ? `${value.name}: ${value.message}` : value.name || fallback;
		return { message: clip(message, MAX_MESSAGE), stack: value.stack ? clip(value.stack, MAX_STACK) : null };
	}
	if (typeof value === 'string') return { message: value ? clip(value, MAX_STRING) : fallback, stack: null };
	if (value === undefined || value === null) return { message: fallback, stack: null };
	return { message: clip(`${fallback}: ${typeDescription(value)}`, MAX_MESSAGE), stack: null };
}

/** Normalises a window `error` / `unhandledrejection` event; null for events we ignore (resource loads). */
export function fromEvent(e: Event): { message: string; stack: string | null } | null {
	if (e.type === 'unhandledrejection') {
		return describeError((e as PromiseRejectionEvent).reason, 'Unhandled promise rejection');
	}
	if (e.type === 'error') {
		const ev = e as ErrorEvent;
		// A failed <img>/<video> load also dispatches "error" (capture phase only) without a message.
		if (ev.error === undefined && !ev.message) return null;
		if (ev.error !== undefined && ev.error !== null) {
			const d = describeError(ev.error, ev.message || 'Error');
			return d;
		}
		const where = ev.filename ? ` (${ev.filename}:${ev.lineno ?? 0}:${ev.colno ?? 0})` : '';
		return { message: clip(`${ev.message}${where}`, MAX_MESSAGE), stack: null };
	}
	return null;
}

/**
 * Installs the two listeners on `target` (window). Returns the uninstaller. `log` failures (command
 * missing, LAN, rate limit) are swallowed; errors thrown while reporting are never reported again.
 */
export function installErrorHooks(target: Pick<EventTarget, 'addEventListener' | 'removeEventListener'>, log: ErrorLogger, opts: HookOptions = {}): () => void {
	const repeatMs = opts.repeatMs ?? 2000;
	const perMinute = opts.perMinute ?? 20;
	const now = opts.now ?? Date.now;
	let reporting = false;
	let last: { message: string; at: number } | null = null;
	let sent: number[] = [];

	const handler = (e: Event) => {
		if (reporting) return;
		const info = fromEvent(e);
		if (!info) return;
		const at = now();
		if (last && last.message === info.message && at - last.at < repeatMs) return;
		sent = sent.filter((t) => at - t < 60_000);
		if (sent.length >= perMinute) return;
		last = { message: info.message, at };
		sent.push(at);
		reporting = true;
		try {
			void Promise.resolve()
				.then(() => log(info.message, info.stack))
				.catch(() => {});
		} catch {
			// Never let the reporter itself raise.
		} finally {
			reporting = false;
		}
	};
	target.addEventListener('error', handler);
	target.addEventListener('unhandledrejection', handler);
	return () => {
		target.removeEventListener('error', handler);
		target.removeEventListener('unhandledrejection', handler);
	};
}
