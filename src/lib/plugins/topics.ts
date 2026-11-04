/**
 * Live data for plugin `subscribe` (docs/plugin-api.md): each topic maps to the same IPC the built-in
 * widgets use. Backend subscriptions are ref-counted, so every successful subscribe is paired with
 * exactly one unsubscribe, also when the stop comes before the async calls settle.
 */
import {
	mediaState,
	mediaSubscribe,
	mediaUnsubscribe,
	on,
	pomodoroState,
	systemSubscribe,
	systemUnsubscribe,
	type DeckEvents,
	type PluginTopic
} from '#lib/ipc/index.ts';

export interface TopicIpc {
	subscribe?: () => Promise<void>;
	unsubscribe?: () => Promise<void>;
	/** Current value pushed once on subscribe (topics whose events fire only on change). */
	initial?: () => Promise<unknown>;
	listen: (cb: (payload: unknown) => void) => Promise<() => void>;
}

const listenTo =
	<K extends keyof DeckEvents>(event: K) =>
	(cb: (payload: unknown) => void) =>
		on(event, cb);

export const TOPIC_IPC: Record<PluginTopic, TopicIpc> = {
	system: { subscribe: systemSubscribe, unsubscribe: systemUnsubscribe, listen: listenTo('system://sample') },
	media: { subscribe: mediaSubscribe, unsubscribe: mediaUnsubscribe, initial: mediaState, listen: listenTo('media://state') },
	// Backend-owned timer: no subscribe command, `pomodoro://state` fires on every change.
	pomodoro: { initial: pomodoroState, listen: listenTo('pomodoro://state') }
};

/** Starts `topic` and returns its stop function. */
export function startTopic(ipc: TopicIpc, push: (payload: unknown) => void): () => void {
	let stopped = false;
	let unlisten: (() => void) | null = null;
	const emit = (p: unknown) => {
		if (!stopped) push(p);
	};
	ipc
		.listen(emit)
		.then((u) => (stopped ? u() : (unlisten = u)))
		.catch(() => {});
	const subscribed = ipc.subscribe
		? ipc.subscribe().then(
				() => true,
				() => false
			)
		: Promise.resolve(false);
	ipc
		.initial?.()
		.then(emit)
		.catch(() => {});
	return () => {
		if (stopped) return;
		stopped = true;
		unlisten?.();
		unlisten = null;
		void subscribed.then((ok) => {
			if (ok) ipc.unsubscribe?.().catch(() => {});
		});
	};
}
