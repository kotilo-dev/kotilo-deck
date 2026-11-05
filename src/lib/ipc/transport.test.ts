import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import {
	BACKOFF_MAX_MS,
	BACKOFF_MIN_MS,
	createTransport,
	wantsDemo,
	type DemoBackend,
	type SocketLike,
	type TransportEnv
} from './transport';

class FakeSocket implements SocketLike {
	static all: FakeSocket[] = [];
	readyState = 0;
	onopen: ((ev: unknown) => void) | null = null;
	onclose: ((ev: unknown) => void) | null = null;
	onerror: ((ev: unknown) => void) | null = null;
	onmessage: ((ev: { data: unknown }) => void) | null = null;
	closed = false;
	constructor(readonly url: string) {
		FakeSocket.all.push(this);
	}
	/** Like the LAN server: open, then the `lan://hello` frame naming the socket. */
	open(hello = true) {
		this.readyState = 1;
		this.onopen?.({});
		if (hello) this.send('lan://hello', { socketId: `sock-${FakeSocket.all.indexOf(this)}` });
	}
	drop() {
		this.readyState = 3;
		this.onclose?.({});
	}
	send(event: string, payload: unknown) {
		this.onmessage?.({ data: JSON.stringify({ event, payload }) });
	}
	close() {
		this.closed = true;
		this.readyState = 3;
	}
}

const json = (body: unknown, status = 200) =>
	new Response(JSON.stringify(body), { status, headers: { 'content-type': 'application/json' } });

const HELLO = { app: 'kotilo-deck', version: '0.4.0', remote: true };

function makeEnv(overrides: Partial<TransportEnv> = {}) {
	const calls: { url: string; init?: RequestInit }[] = [];
	const routes = new Map<string, (init?: RequestInit) => Response | Promise<Response>>();
	routes.set('/api/hello', () => json(HELLO));
	const env: TransportEnv = {
		tauri: () => false,
		fetch: vi.fn(async (url: string, init?: RequestInit) => {
			calls.push({ url, init });
			const route = routes.get(url);
			if (!route) return new Response('not found', { status: 404 });
			return route(init);
		}),
		openSocket: (url) => new FakeSocket(url),
		location: () => ({ protocol: 'http:', host: '192.168.1.5:47832' }),
		tauriInvoke: vi.fn(async () => 'tauri') as TransportEnv['tauriInvoke'],
		tauriListen: vi.fn(async () => () => {}) as TransportEnv['tauriListen'],
		setTimeout: (fn, ms) => setTimeout(fn, ms),
		clearTimeout: (id) => clearTimeout(id as ReturnType<typeof setTimeout>),
		...overrides
	};
	return { env, calls, routes };
}

const invokeCalls = (calls: { url: string }[], cmd: string) => calls.filter((c) => c.url === `/api/invoke/${cmd}`).length;

beforeEach(() => {
	FakeSocket.all = [];
});
afterEach(() => {
	vi.useRealTimers();
});

describe('transport mode detection', () => {
	it('inside Tauri never probes and uses the Tauri API', async () => {
		const { env, calls } = makeEnv({ tauri: () => true });
		const tr = createTransport(env);
		expect(await tr.init()).toBe('tauri');
		expect(tr.isTauri()).toBe(true);
		expect(tr.isRemote()).toBe(false);
		expect(tr.hasBackend()).toBe(true);
		expect(await tr.invoke('settings_get')).toBe('tauri');
		expect(env.tauriInvoke).toHaveBeenCalledWith('settings_get', undefined);
		await tr.listen('system://sample', () => {});
		expect(env.tauriListen).toHaveBeenCalled();
		expect(calls).toEqual([]);
	});

	it('detects remote mode from /api/hello once and caches it', async () => {
		const { env, calls } = makeEnv();
		const tr = createTransport(env);
		expect(tr.hasBackend()).toBe(false);
		const [a, b] = await Promise.all([tr.init(), tr.init()]);
		expect([a, b]).toEqual(['remote', 'remote']);
		await tr.init();
		expect(calls.filter((c) => c.url === '/api/hello')).toHaveLength(1);
		expect(calls[0].init?.credentials).toBe('same-origin');
		expect(tr.isRemote()).toBe(true);
		expect(tr.hasBackend()).toBe(true);
		expect(tr.isTauri()).toBe(false);
		expect(tr.hello()?.version).toBe('0.4.0');
	});

	it('falls back to local when /api/hello 404s (plain pnpm dev)', async () => {
		const { env, routes } = makeEnv();
		routes.delete('/api/hello');
		const tr = createTransport(env);
		expect(await tr.init()).toBe('local');
		expect(tr.hasBackend()).toBe(false);
		await expect(tr.invoke('settings_get')).rejects.toBe('Kotilo backend not available');
		const off = await tr.listen('system://sample', () => {});
		expect(typeof off).toBe('function');
		expect(FakeSocket.all).toHaveLength(0);
	});

	it('falls back to local when the dev server answers with index.html', async () => {
		const { env, routes } = makeEnv();
		routes.set('/api/hello', () => new Response('<!doctype html><html></html>', { status: 200 }));
		expect(await createTransport(env).init()).toBe('local');
	});

	it('falls back to local when hello is some other app or the network fails', async () => {
		const { env, routes } = makeEnv();
		routes.set('/api/hello', () => json({ app: 'other', remote: true }));
		expect(await createTransport(env).init()).toBe('local');
		const failing = makeEnv({ fetch: () => Promise.reject(new TypeError('offline')) });
		expect(await createTransport(failing.env).init()).toBe('local');
	});

	it('gives up on a hanging /api/hello after the timeout', async () => {
		vi.useFakeTimers();
		const { env } = makeEnv({ fetch: () => new Promise<Response>(() => {}), helloTimeoutMs: 100 });
		const p = createTransport(env).init();
		await vi.advanceTimersByTimeAsync(150);
		expect(await p).toBe('local');
	});

	it('without a location (unit tests, SSR) it is local', async () => {
		const { env, calls } = makeEnv({ location: () => null });
		expect(await createTransport(env).init()).toBe('local');
		expect(calls).toEqual([]);
	});
});

describe('remote invoke', () => {
	it('POSTs the JSON args and returns the JSON result', async () => {
		const { env, calls, routes } = makeEnv();
		routes.set('/api/invoke/weather_get', () => json({ temp: 21 }));
		const tr = createTransport(env);
		await tr.init();
		expect(await tr.invoke('weather_get', { latitude: 1, longitude: 2, unit: 'c' })).toEqual({ temp: 21 });
		const call = calls.find((c) => c.url === '/api/invoke/weather_get');
		expect(call?.init?.method).toBe('POST');
		expect(call?.init?.credentials).toBe('same-origin');
		expect(JSON.parse(String(call?.init?.body))).toEqual({ latitude: 1, longitude: 2, unit: 'c' });
	});

	it('sends {} without args and accepts an empty or null body', async () => {
		const { env, calls, routes } = makeEnv();
		routes.set('/api/invoke/calendar_refresh', () => new Response('', { status: 200 }));
		routes.set('/api/invoke/todo_delete', () => json(null));
		const tr = createTransport(env);
		await tr.init();
		expect(await tr.invoke('calendar_refresh')).toBeUndefined();
		expect(await tr.invoke('todo_delete', { id: 1 })).toBeNull();
		expect(calls.find((c) => c.url === '/api/invoke/calendar_refresh')?.init?.body).toBe('{}');
	});

	it('rejects with the server error string, like Tauri', async () => {
		const { env, routes } = makeEnv();
		routes.set('/api/invoke/settings_set', () => json({ error: 'command not allowed' }, 403));
		routes.set('/api/invoke/broken', () => new Response('oops', { status: 500 }));
		const tr = createTransport(env);
		await tr.init();
		await expect(tr.invoke('settings_set', { patch: {} })).rejects.toBe('command not allowed');
		await expect(tr.invoke('broken')).rejects.toBe('HTTP 500');
	});

	it('rejects with a string when the network fails', async () => {
		const { env, routes } = makeEnv();
		routes.set('/api/invoke/audio_state', () => Promise.reject(new TypeError('Failed to fetch')));
		const tr = createTransport(env);
		await tr.init();
		await expect(tr.invoke('audio_state')).rejects.toBe('Failed to fetch');
	});

	it('invoke before init detects the mode itself', async () => {
		const { env, routes } = makeEnv();
		routes.set('/api/invoke/settings_get', () => json({ theme: null }));
		expect(await createTransport(env).invoke('settings_get')).toEqual({ theme: null });
	});
});

describe('remote events', () => {
	async function remote() {
		const ctx = makeEnv();
		ctx.routes.set('/api/invoke/system_subscribe', () => json(null));
		ctx.routes.set('/api/invoke/system_unsubscribe', () => json(null));
		ctx.routes.set('/api/invoke/media_subscribe', () => json(null));
		const tr = createTransport(ctx.env);
		await tr.init();
		return { ...ctx, tr };
	}

	it('shares one WebSocket on /api/events and routes {event,payload}', async () => {
		const { tr } = await remote();
		const a = vi.fn();
		const b = vi.fn();
		const c = vi.fn();
		await tr.listen('system://sample', a);
		await tr.listen('system://sample', b);
		await tr.listen('media://state', c);
		expect(FakeSocket.all).toHaveLength(1);
		const ws = FakeSocket.all[0];
		expect(ws.url).toBe('ws://192.168.1.5:47832/api/events');
		ws.open();
		ws.send('system://sample', { cpu: 5 });
		expect(a).toHaveBeenCalledWith({ event: 'system://sample', payload: { cpu: 5 } });
		expect(b).toHaveBeenCalledTimes(1);
		expect(c).not.toHaveBeenCalled();
	});

	it('uses wss:// on https pages', async () => {
		const { env } = makeEnv({ location: () => ({ protocol: 'https:', host: 'deck.lan' }) });
		const tr = createTransport(env);
		await tr.init();
		await tr.listen('x://y', () => {});
		expect(FakeSocket.all[0].url).toBe('wss://deck.lan/api/events');
	});

	it('unlisten stops delivery; bad frames and throwing handlers are ignored', async () => {
		const { tr } = await remote();
		const err = vi.spyOn(console, 'error').mockImplementation(() => {});
		const bad = vi.fn(() => {
			throw new Error('boom');
		});
		const good = vi.fn();
		const off = await tr.listen('pomodoro://state', good);
		await tr.listen('pomodoro://state', bad);
		const ws = FakeSocket.all[0];
		ws.open();
		ws.onmessage?.({ data: 'not json' });
		ws.onmessage?.({ data: JSON.stringify({ payload: 1 }) });
		ws.send('pomodoro://state', { phase: 'focus' });
		expect(good).toHaveBeenCalledTimes(1);
		expect(err).toHaveBeenCalled();
		off();
		ws.send('pomodoro://state', { phase: 'idle' });
		expect(good).toHaveBeenCalledTimes(1);
		err.mockRestore();
	});

	it('reconnects with 1 s → 30 s backoff and resets after a successful open', async () => {
		vi.useFakeTimers();
		const { tr } = await remote();
		await tr.listen('system://sample', () => {});
		FakeSocket.all[0].open();
		FakeSocket.all[0].drop();
		const waits: number[] = [];
		for (let i = 0; i < 7; i++) {
			const before = FakeSocket.all.length;
			let waited = 0;
			while (FakeSocket.all.length === before) {
				await vi.advanceTimersByTimeAsync(250);
				waited += 250;
			}
			waits.push(waited);
			FakeSocket.all.at(-1)!.drop();
		}
		expect(waits).toEqual([1000, 2000, 4000, 8000, 16000, BACKOFF_MAX_MS, BACKOFF_MAX_MS]);
		// A successful open resets the backoff.
		await vi.advanceTimersByTimeAsync(BACKOFF_MAX_MS);
		FakeSocket.all.at(-1)!.open();
		FakeSocket.all.at(-1)!.drop();
		const before = FakeSocket.all.length;
		await vi.advanceTimersByTimeAsync(BACKOFF_MIN_MS);
		expect(FakeSocket.all.length).toBe(before + 1);
		tr.dispose();
	});

	it('re-runs active *_subscribe calls after a reconnect, not unsubscribed ones', async () => {
		vi.useFakeTimers();
		const { tr, calls } = await remote();
		await tr.listen('system://sample', () => {});
		FakeSocket.all[0].open();
		await tr.invoke('system_subscribe');
		await tr.invoke('system_subscribe');
		await tr.invoke('system_unsubscribe');
		await tr.invoke('media_subscribe');
		expect(Object.fromEntries(tr.subscriptions())).toEqual({ system: 1, media: 1 });
		expect(invokeCalls(calls, 'system_subscribe')).toBe(2);

		FakeSocket.all[0].drop();
		await vi.advanceTimersByTimeAsync(BACKOFF_MIN_MS);
		expect(FakeSocket.all).toHaveLength(2);
		// Nothing is re-sent until the new socket is actually open.
		expect(invokeCalls(calls, 'system_subscribe')).toBe(2);
		FakeSocket.all[1].open();
		await vi.advanceTimersByTimeAsync(0);
		expect(invokeCalls(calls, 'system_subscribe')).toBe(3);
		expect(invokeCalls(calls, 'media_subscribe')).toBe(2);
		// Counts are unchanged by the replay.
		expect(Object.fromEntries(tr.subscriptions())).toEqual({ system: 1, media: 1 });
		tr.dispose();
	});

	it('a remote *_subscribe waits for the events socket to open first', async () => {
		const { tr, calls } = await remote();
		const p = tr.invoke('system_subscribe');
		await Promise.resolve();
		expect(FakeSocket.all).toHaveLength(1);
		expect(invokeCalls(calls, 'system_subscribe')).toBe(0);
		FakeSocket.all[0].open();
		await p;
		expect(invokeCalls(calls, 'system_subscribe')).toBe(1);
	});

	it('subscribes only after lan://hello and send its socket id as X-Kotilo-Socket', async () => {
		const { tr, calls } = await remote();
		const p = tr.invoke('system_subscribe');
		await Promise.resolve();
		FakeSocket.all[0].open(false);
		await Promise.resolve();
		expect(invokeCalls(calls, 'system_subscribe')).toBe(0);
		FakeSocket.all[0].send('lan://hello', { socketId: 'abc' });
		await p;
		const call = calls.find((c) => c.url === '/api/invoke/system_subscribe')!;
		expect((call.init?.headers as Record<string, string>)['x-kotilo-socket']).toBe('abc');
	});

	it('an unsubscribe drops the local count even when the server refuses it', async () => {
		const { tr, routes } = await remote();
		routes.set('/api/invoke/media_subscribe', () => json(null));
		routes.set('/api/invoke/media_unsubscribe', () => json({ error: 'unknown socket' }, 400));
		await tr.listen('media://state', () => {});
		FakeSocket.all[0].open();
		await tr.invoke('media_subscribe');
		await expect(tr.invoke('media_unsubscribe')).rejects.toBe('unknown socket');
		expect(tr.subscriptions().size).toBe(0);
	});

	it('a failed subscribe is not counted', async () => {
		const { tr, routes } = await remote();
		routes.set('/api/invoke/audio_subscribe', () => json({ error: 'no audio' }, 500));
		await tr.listen('audio://state', () => {});
		FakeSocket.all[0].open();
		await expect(tr.invoke('audio_subscribe')).rejects.toBe('no audio');
		expect(tr.subscriptions().size).toBe(0);
	});
});

describe('demo mode (?demo)', () => {
	function fakeDemo() {
		const demo: DemoBackend = {
			invoke: vi.fn(async (cmd: string) => `demo:${cmd}`) as DemoBackend['invoke'],
			listen: vi.fn(async () => () => {}) as DemoBackend['listen']
		};
		const loadDemo = vi.fn(async () => demo);
		return { demo, loadDemo };
	}

	it('parses the query string', () => {
		expect(wantsDemo('?demo')).toBe(true);
		expect(wantsDemo('?theme=nord&demo=1')).toBe(true);
		expect(wantsDemo('')).toBe(false);
		expect(wantsDemo('?token=abc')).toBe(false);
		expect(wantsDemo('?demonstration')).toBe(false);
	});

	it('uses the in-memory backend and never probes the LAN server', async () => {
		const { demo, loadDemo } = fakeDemo();
		const { env, calls } = makeEnv({ search: () => '?demo&theme=nord', loadDemo });
		const tr = createTransport(env);
		expect(await tr.init()).toBe('demo');
		expect(loadDemo).toHaveBeenCalledTimes(1);
		expect((loadDemo.mock.calls[0] as unknown as [URLSearchParams])[0].get('theme')).toBe('nord');
		expect(tr.isDemo()).toBe(true);
		expect(tr.hasBackend()).toBe(true);
		expect(tr.isRemote()).toBe(false);
		expect(tr.isTauri()).toBe(false);
		expect(await tr.invoke('settings_get')).toBe('demo:settings_get');
		await tr.listen('system://sample', () => {});
		expect(demo.listen).toHaveBeenCalled();
		expect(calls).toEqual([]);
		expect(FakeSocket.all).toHaveLength(0);
	});

	it('is never active inside Tauri', async () => {
		const { loadDemo } = fakeDemo();
		const { env } = makeEnv({ tauri: () => true, search: () => '?demo', loadDemo });
		const tr = createTransport(env);
		expect(await tr.init()).toBe('tauri');
		expect(tr.isDemo()).toBe(false);
		expect(loadDemo).not.toHaveBeenCalled();
		expect(await tr.invoke('settings_get')).toBe('tauri');
	});

	it('leaves remote mode alone without ?demo', async () => {
		const { loadDemo } = fakeDemo();
		const { env } = makeEnv({ search: () => '?token=secret', loadDemo });
		const tr = createTransport(env);
		expect(await tr.init()).toBe('remote');
		expect(tr.isDemo()).toBe(false);
		expect(tr.isRemote()).toBe(true);
		expect(loadDemo).not.toHaveBeenCalled();
	});

	it('falls back to the normal probe when the demo module fails to load', async () => {
		const err = vi.spyOn(console, 'error').mockImplementation(() => {});
		const { env, routes } = makeEnv({ search: () => '?demo', loadDemo: () => Promise.reject(new Error('chunk')) });
		routes.delete('/api/hello');
		const tr = createTransport(env);
		expect(await tr.init()).toBe('local');
		expect(tr.isDemo()).toBe(false);
		expect(tr.hasBackend()).toBe(false);
		err.mockRestore();
	});
});
