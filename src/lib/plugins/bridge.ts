/**
 * Deck side of the plugin message protocol (docs/plugin-api.md "Messages"). DOM-free so it can be
 * unit-tested: PluginFrame supplies the iframe window, the state snapshot and the topic sources.
 */
import type { PluginTopic, SizeKey } from '#lib/ipc/index.ts';

export const PROTOCOL = 1;

export interface PluginTheme {
	variant: 'dark' | 'light';
	/** Every theme `--k-*` value without the prefix (incl. `radius`, `font-ui`, `font-mono`). */
	tokens: Record<string, string>;
}

/** Everything `init` carries (plugin-api.md). */
export interface PluginState {
	size: SizeKey;
	settings: Record<string, unknown>;
	theme: PluginTheme;
	locale: string;
	editing: boolean;
	available: boolean;
}

type Msg<T extends string, F> = { kotilo: typeof PROTOCOL; type: T } & F;

export type DeckMessage =
	| Msg<'init', PluginState>
	| Msg<'size', { size: SizeKey }>
	| Msg<'settings', { settings: Record<string, unknown> }>
	| Msg<'theme', { theme: PluginTheme }>
	| Msg<'editing', { editing: boolean }>
	| Msg<'visibility', { visible: boolean }>
	| Msg<'data', { topic: PluginTopic; payload: unknown }>;

export interface BridgeHost {
	/** The iframe's `contentWindow`; messages from anything else are dropped. */
	source(): unknown;
	/** Posts to the iframe. Must use targetOrigin '*' (the sandboxed frame has an opaque origin). */
	post(msg: DeckMessage): void;
	/** Current values for `init` and the change pushes. */
	state(): PluginState;
	visible(): boolean;
	/** Starts a live topic; `push` is called per payload. Returns the stop function. */
	subscribe(topic: PluginTopic, push: (payload: unknown) => void): () => void;
	/** First (or repeated) `ready` from the widget. */
	onReady?(): void;
}

/** postMessage structured-clones; Svelte proxies and functions can't be cloned, so send plain JSON. */
export const plain = <T>(v: T): T => (v === undefined ? v : JSON.parse(JSON.stringify(v)));

const isRecord = (v: unknown): v is Record<string, unknown> => typeof v === 'object' && v !== null && !Array.isArray(v);

type PushKey = 'size' | 'settings' | 'theme' | 'editing';
const PUSH_KEYS: readonly PushKey[] = ['size', 'settings', 'theme', 'editing'];

export class PluginBridge {
	#host: BridgeHost;
	#topics: ReadonlySet<PluginTopic>;
	#subs = new Map<PluginTopic, () => void>();
	/** JSON of the last value sent per field (after `ready`), so pushes go out only on real changes. */
	#sent = new Map<PushKey | 'visible', string>();
	#ready = false;
	#destroyed = false;

	constructor(host: BridgeHost, topics: readonly PluginTopic[]) {
		this.#host = host;
		this.#topics = new Set(topics);
	}

	get ready(): boolean {
		return this.#ready;
	}

	/** Topics with a live subscription (tests, debugging). */
	get subscribed(): PluginTopic[] {
		return [...this.#subs.keys()];
	}

	/** window `message` handler. Returns true when the message was accepted. */
	handle(event: { source: unknown; data: unknown }): boolean {
		if (this.#destroyed) return false;
		const own = this.#host.source();
		if (!own || event.source !== own) return false;
		const data = event.data;
		if (!isRecord(data) || data.kotilo !== PROTOCOL || typeof data.type !== 'string') return false;
		switch (data.type) {
			case 'ready':
				this.#onReady();
				return true;
			case 'subscribe':
				return this.#subscribe(data.topic);
			case 'unsubscribe':
				return this.#unsubscribe(data.topic);
			default:
				// Unknown widget → deck types are ignored (additive protocol).
				return false;
		}
	}

	/** Call whenever size / settings / theme / editing may have changed; posts only what differs. */
	update(): void {
		if (!this.#ready || this.#destroyed) return;
		const s = this.#host.state();
		for (const key of PUSH_KEYS) {
			const json = JSON.stringify(s[key]);
			if (this.#sent.get(key) === json) continue;
			this.#sent.set(key, json);
			this.#post({ kotilo: PROTOCOL, type: key, [key]: JSON.parse(json) } as DeckMessage);
		}
	}

	/** Deck shown/hidden. */
	setVisible(visible: boolean): void {
		if (!this.#ready || this.#destroyed) return;
		const json = String(visible);
		if (this.#sent.get('visible') === json) return;
		this.#sent.set('visible', json);
		this.#post({ kotilo: PROTOCOL, type: 'visibility', visible });
	}

	/** Stops every topic; the bridge ignores all further messages. */
	destroy(): void {
		this.#destroyed = true;
		this.#stopAll();
	}

	#onReady() {
		// A second `ready` means the frame reloaded: its old subscriptions are gone with it.
		this.#stopAll();
		this.#sent.clear();
		const s = plain(this.#host.state());
		for (const key of PUSH_KEYS) this.#sent.set(key, JSON.stringify(s[key]));
		this.#ready = true;
		this.#post({ kotilo: PROTOCOL, type: 'init', ...s });
		const visible = this.#host.visible();
		this.#sent.set('visible', String(visible));
		if (!visible) this.#post({ kotilo: PROTOCOL, type: 'visibility', visible });
		this.#host.onReady?.();
	}

	#subscribe(topic: unknown): boolean {
		if (!this.#isTopic(topic) || this.#subs.has(topic)) return false;
		let live = true;
		const stop = this.#host.subscribe(topic, (payload) => {
			if (live && !this.#destroyed) this.#post({ kotilo: PROTOCOL, type: 'data', topic, payload: plain(payload) });
		});
		this.#subs.set(topic, () => {
			live = false;
			stop();
		});
		return true;
	}

	#unsubscribe(topic: unknown): boolean {
		if (!this.#isTopic(topic)) return false;
		const stop = this.#subs.get(topic);
		if (!stop) return false;
		this.#subs.delete(topic);
		stop();
		return true;
	}

	/** Only topics declared in the manifest (plugin-api.md "subscribe"). */
	#isTopic(topic: unknown): topic is PluginTopic {
		return typeof topic === 'string' && this.#topics.has(topic as PluginTopic);
	}

	#stopAll() {
		const stops = [...this.#subs.values()];
		this.#subs.clear();
		for (const stop of stops) stop();
	}

	#post(msg: DeckMessage) {
		try {
			this.#host.post(msg);
		} catch (e) {
			console.error('kotilo-deck: plugin postMessage failed', e);
		}
	}
}
