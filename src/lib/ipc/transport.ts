/**
 * IPC transport (ARCHITECTURE "Phase 4 ▸ LAN mode"). One `invoke` / `listen` pair for three environments:
 *
 * - **tauri**: the deck/settings webview → `@tauri-apps/api` invoke + events.
 * - **remote**: a tablet/browser served by the deck's LAN server → `POST /api/invoke/<cmd>` and one shared
 *   WebSocket on `/api/events` pushing `{ event, payload }`.
 * - **local**: plain `pnpm dev` (no backend; `/api/hello` 404s) → callers use their localStorage fallbacks.
 *
 * `init()` decides once (cached) and must have resolved before the app mounts (`routes/+layout.ts`).
 * Before that `isRemote()` / `hasBackend()` report false outside Tauri.
 */
import { convertFileSrc, invoke as tauriInvoke } from '@tauri-apps/api/core';
import { listen as tauriListen } from '@tauri-apps/api/event';

export type UnlistenFn = () => void;
/** Same shape Tauri hands to listeners (only the fields we use). */
export interface TransportEvent<T> {
	event: string;
	payload: T;
}
export type EventHandler<T> = (event: TransportEvent<T>) => void;
export type TransportMode = 'tauri' | 'remote' | 'local';

/** `GET /api/hello` body (ARCHITECTURE LAN mode). */
export interface Hello {
	app: 'kotilo-deck';
	version: string;
	remote: true;
}

export interface SocketLike {
	readonly readyState: number;
	onopen: ((ev: unknown) => void) | null;
	onclose: ((ev: unknown) => void) | null;
	onerror: ((ev: unknown) => void) | null;
	onmessage: ((ev: { data: unknown }) => void) | null;
	close(): void;
}

export interface TransportEnv {
	/** True inside the Tauri webview. */
	tauri: () => boolean;
	fetch: (url: string, init?: RequestInit) => Promise<Response>;
	/** Opens a WebSocket (browser: `new WebSocket(url)`). */
	openSocket: (url: string) => SocketLike;
	/** `location` of the page (protocol + host build the ws:// URL). */
	location: () => { protocol: string; host: string } | null;
	tauriInvoke: <T>(cmd: string, args?: Record<string, unknown>) => Promise<T>;
	tauriListen: <T>(event: string, handler: EventHandler<T>) => Promise<UnlistenFn>;
	setTimeout: (fn: () => void, ms: number) => unknown;
	clearTimeout: (id: unknown) => void;
	/** How long `/api/hello` may take before we assume there is no server. */
	helloTimeoutMs?: number;
}

/** Reconnect backoff of the events socket: 1 s, doubling, capped at 30 s; reset after a successful open. */
export const BACKOFF_MIN_MS = 1000;
export const BACKOFF_MAX_MS = 30_000;
/** How long a remote `*_subscribe` waits for the events socket before going ahead anyway. */
export const SOCKET_WAIT_MS = 5000;

const OPEN = 1;
/** First frame on every events socket (ARCHITECTURE "LAN mode"). */
const HELLO_EVENT = 'lan://hello';
const SUBSCRIBE = /^([a-z]+)_subscribe$/;
const UNSUBSCRIBE = /^([a-z]+)_unsubscribe$/;

/** Rejection value: like Tauri, a plain string (commands return `Result<T, String>`). */
const reason = (e: unknown): string => (e instanceof Error ? e.message : typeof e === 'string' ? e : String(e));

export function createTransport(env: TransportEnv) {
	let mode: TransportMode | null = null;
	let hello: Hello | null = null;
	let pending: Promise<TransportMode> | null = null;

	// ── remote events socket ──
	const handlers = new Map<string, Set<EventHandler<unknown>>>();
	let socket: SocketLike | null = null;
	let backoff = BACKOFF_MIN_MS;
	let retryTimer: unknown = null;
	/** Set once an open socket closed; the next open re-runs active subscriptions. */
	let lost = false;
	let disposed = false;
	let openWaiters: (() => void)[] = [];
	/** From the server's first frame `lan://hello`; sent as `X-Kotilo-Socket` so subscriptions are tied to this socket. */
	let socketId: string | null = null;
	/** Remote `<area>_subscribe` calls not yet matched by an unsubscribe, per area. */
	const subs = new Map<string, number>();

	const isTauri = (): boolean => env.tauri();
	const isRemote = (): boolean => !isTauri() && mode === 'remote';
	const hasBackend = (): boolean => isTauri() || mode === 'remote';

	async function probe(): Promise<TransportMode> {
		if (isTauri()) return 'tauri';
		if (!env.location()) return 'local';
		let timer: unknown = null;
		try {
			const ctrl = typeof AbortController === 'undefined' ? null : new AbortController();
			const timeout = new Promise<never>((_, reject) => {
				timer = env.setTimeout(() => {
					ctrl?.abort();
					reject(new Error('timeout'));
				}, env.helloTimeoutMs ?? 3000);
			});
			const res = await Promise.race([
				env.fetch('/api/hello', {
					credentials: 'same-origin',
					headers: { accept: 'application/json' },
					signal: ctrl?.signal
				}),
				timeout
			]);
			if (!res.ok) return 'local';
			// The Vite dev server answers unknown paths with index.html; that is not a deck.
			const body = (await Promise.race([res.json(), timeout])) as Partial<Hello> | null;
			if (body && body.app === 'kotilo-deck' && body.remote === true) {
				hello = body as Hello;
				return 'remote';
			}
			return 'local';
		} catch {
			return 'local';
		} finally {
			if (timer !== null) env.clearTimeout(timer);
		}
	}

	/** Detects the mode once; later calls return the cached result. */
	function init(): Promise<TransportMode> {
		if (mode) return Promise.resolve(mode);
		pending ??= probe().then((m) => (mode = m));
		return pending;
	}

	// ── socket lifecycle ──

	function wsUrl(): string | null {
		const loc = env.location();
		if (!loc) return null;
		return `${loc.protocol === 'https:' ? 'wss' : 'ws'}://${loc.host}/api/events`;
	}

	function connect() {
		if (disposed || socket) return;
		const url = wsUrl();
		if (!url) return;
		let ws: SocketLike;
		try {
			ws = env.openSocket(url);
		} catch {
			scheduleReconnect();
			return;
		}
		socket = ws;
		ws.onopen = () => {
			backoff = BACKOFF_MIN_MS;
		};
		ws.onmessage = (ev) => dispatch(ev.data);
		ws.onerror = () => {
			// onclose follows and handles the retry.
		};
		ws.onclose = () => {
			if (socket !== ws) return;
			socket = null;
			socketId = null;
			// The server released this socket's subscriptions; whatever we hold must be re-sent.
			lost = true;
			scheduleReconnect();
		};
	}

	function scheduleReconnect() {
		if (disposed || retryTimer !== null) return;
		const wait = backoff;
		backoff = Math.min(backoff * 2, BACKOFF_MAX_MS);
		retryTimer = env.setTimeout(() => {
			retryTimer = null;
			connect();
		}, wait);
	}

	function dispatch(data: unknown) {
		let msg: { event?: unknown; payload?: unknown };
		try {
			msg = typeof data === 'string' ? JSON.parse(data) : (data as typeof msg);
		} catch {
			return;
		}
		if (!msg || typeof msg.event !== 'string') return;
		if (msg.event === HELLO_EVENT) {
			const id = (msg.payload as { socketId?: unknown } | null)?.socketId;
			if (typeof id === 'string' && id) onHello(id);
			return;
		}
		const set = handlers.get(msg.event);
		if (!set) return;
		const e: TransportEvent<unknown> = { event: msg.event, payload: msg.payload };
		for (const h of [...set]) {
			try {
				h(e);
			} catch (err) {
				console.error('kotilo-deck: event handler failed', msg.event, err);
			}
		}
	}

	/** The socket is usable once the server named it: wake waiters, re-send lost subscriptions. */
	function onHello(id: string) {
		socketId = id;
		const waiters = openWaiters;
		openWaiters = [];
		for (const w of waiters) w();
		if (lost) {
			lost = false;
			resubscribe();
		}
	}

	function resubscribe() {
		for (const [area, n] of subs) {
			for (let i = 0; i < n; i++) void post(`${area}_subscribe`, {}).catch(() => {});
		}
	}

	/** Resolves once the socket is open (or after SOCKET_WAIT_MS), so the server can tie a subscription to it. */
	function socketReady(): Promise<void> {
		if (socket?.readyState === OPEN && socketId) return Promise.resolve();
		connect();
		return new Promise((resolve) => {
			const done = () => {
				env.clearTimeout(timer);
				resolve();
			};
			const timer = env.setTimeout(() => {
				openWaiters = openWaiters.filter((w) => w !== done);
				resolve();
			}, SOCKET_WAIT_MS);
			openWaiters.push(done);
		});
	}

	// ── remote invoke ──

	async function post<T>(cmd: string, args: Record<string, unknown>): Promise<T> {
		let res: Response;
		try {
			res = await env.fetch(`/api/invoke/${encodeURIComponent(cmd)}`, {
				method: 'POST',
				credentials: 'same-origin',
				headers: {
					'content-type': 'application/json',
					accept: 'application/json',
					...(socketId ? { 'x-kotilo-socket': socketId } : {})
				},
				body: JSON.stringify(args)
			});
		} catch (e) {
			throw reason(e);
		}
		const text = await res.text().catch(() => '');
		let body: unknown = undefined;
		if (text) {
			try {
				body = JSON.parse(text);
			} catch {
				body = undefined;
			}
		}
		if (!res.ok) {
			const msg = body && typeof body === 'object' && typeof (body as { error?: unknown }).error === 'string'
				? (body as { error: string }).error
				: `HTTP ${res.status}`;
			throw msg;
		}
		return body as T;
	}

	async function invoke<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
		if (isTauri()) return env.tauriInvoke<T>(cmd, args);
		if ((await init()) !== 'remote') throw 'Kotilo backend not available';
		const sub = SUBSCRIBE.exec(cmd);
		const unsub = sub ? null : UNSUBSCRIBE.exec(cmd);
		if (sub) await socketReady();
		if (unsub) {
			// Drop the local count even if the call fails (e.g. the socket just closed and the
			// server already released it), so a reconnect never re-subscribes it.
			const n = (subs.get(unsub[1]) ?? 0) - 1;
			if (n > 0) subs.set(unsub[1], n);
			else subs.delete(unsub[1]);
		}
		const result = await post<T>(cmd, args ?? {});
		if (sub) subs.set(sub[1], (subs.get(sub[1]) ?? 0) + 1);
		return result;
	}

	async function listen<T>(event: string, handler: EventHandler<T>): Promise<UnlistenFn> {
		if (isTauri()) return env.tauriListen<T>(event, handler);
		if ((await init()) !== 'remote') return () => {};
		let set = handlers.get(event);
		if (!set) handlers.set(event, (set = new Set()));
		const h = handler as EventHandler<unknown>;
		set.add(h);
		connect();
		return () => {
			const s = handlers.get(event);
			if (!s) return;
			s.delete(h);
			if (s.size === 0) handlers.delete(event);
		};
	}

	/** Tests: stop reconnecting and close the socket. */
	function dispose() {
		disposed = true;
		if (retryTimer !== null) env.clearTimeout(retryTimer);
		retryTimer = null;
		const s = socket;
		socket = null;
		s?.close();
	}

	return {
		init,
		invoke,
		listen,
		isTauri,
		isRemote,
		hasBackend,
		/** Server hello (version) in remote mode, else null. */
		hello: () => hello,
		dispose,
		/** Tests: active remote subscriptions per area. */
		subscriptions: () => new Map(subs)
	};
}

export type Transport = ReturnType<typeof createTransport>;

const browserEnv: TransportEnv = {
	tauri: () => typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window,
	fetch: (url, init) => fetch(url, init),
	openSocket: (url) => new WebSocket(url) as unknown as SocketLike,
	location: () => (typeof location === 'undefined' ? null : { protocol: location.protocol, host: location.host }),
	tauriInvoke: (cmd, args) => tauriInvoke(cmd, args),
	tauriListen: (event, handler) => tauriListen(event, handler),
	setTimeout: (fn, ms) => setTimeout(fn, ms),
	clearTimeout: (id) => clearTimeout(id as ReturnType<typeof setTimeout>)
};

const transport = createTransport(browserEnv);

/** Detect tauri / remote / local once. Awaited by `routes/+layout.ts` before anything renders. */
export const init = transport.init;
export const invoke = transport.invoke;
export const listen = transport.listen;
/** Inside the Tauri webview (window APIs, emitTo, convertFileSrc, path APIs are available). */
export const isTauri = transport.isTauri;
/** A tablet/browser talking to the deck's LAN server. */
export const isRemote = transport.isRemote;
/** Is there a backend at all (Tauri or remote)? False in plain `pnpm dev`. */
export const hasBackend = transport.hasBackend;
export const remoteHello = transport.hello;

/** `<img>/<video>` src for a wallpaper: the asset protocol in Tauri, the LAN server's `/wallpapers/<file>` remotely. */
export function wallpaperSrc(w: { name: string; path: string }): string {
	if (isTauri()) return convertFileSrc(w.path);
	return `/wallpapers/${encodeURIComponent(w.name)}`;
}
