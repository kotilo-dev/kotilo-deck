<script lang="ts">
	/**
	 * Host for every external widget (docs/plugin-api.md): a sandboxed iframe filling the card, plus the
	 * postMessage bridge. The card surface (`--k-surface-a`) shows through the transparent frame.
	 */
	import { untrack } from 'svelte';
	import type { SizeKey } from '#lib/ipc/index.ts';
	import { locale, t } from '#lib/i18n/index.ts';
	import { isDeckVisible, onDeckVisibility } from '#lib/time/index.ts';
	import FallbackCard from '#lib/ui/FallbackCard.svelte';
	import { PluginBridge, type PluginState } from './bridge';
	import { pluginSrc, type PluginInfo } from './registry.svelte';
	import { domStyleSource, readTheme } from './theme';
	import { startTopic, TOPIC_IPC } from './topics';

	/** plugin-api.md: no `ready` within this → inline error with Retry. */
	const READY_TIMEOUT_MS = 5000;

	let {
		size,
		settings,
		instance,
		editing,
		available,
		plugin,
		name = plugin?.id ?? ''
	}: {
		size: SizeKey;
		settings: Record<string, unknown>;
		instance: string;
		editing: boolean;
		available: boolean;
		plugin?: PluginInfo;
		name?: string;
	} = $props();

	let iframe = $state<HTMLIFrameElement>();
	let attempt = $state(0);
	let status = $state<'loading' | 'ready' | 'error'>('loading');
	/** Bridge of the mounted frame; not reactive (prop pushes call into it). */
	let bridge: PluginBridge | null = null;

	const src = $derived(plugin ? pluginSrc(plugin) : null);
	/**
	 * Registry refreshes rebuild PluginInfo objects; only a real change (manifest topics, entry, URL)
	 * remounts the frame, so `plugin://changed` for another folder never resets this one.
	 */
	const pluginKey = $derived(plugin ? JSON.stringify(plugin) : '');
	const compact = $derived(size.endsWith('x1'));
	const sizeLabel = $derived(size.replace('x', '×'));
	/** `appearance` is deck-owned (opacity / blur), not part of the plugin's settings. */
	const pluginSettings = $derived.by(() => {
		const { appearance: _appearance, ...rest } = settings;
		return rest;
	});

	const snapshot = (el: HTMLIFrameElement): PluginState => ({
		size,
		settings: pluginSettings,
		theme: readTheme(domStyleSource(document.documentElement, el)),
		locale,
		editing,
		available
	});

	$effect(() => {
		void attempt;
		void pluginKey;
		const el = iframe;
		const info = untrack(() => plugin);
		if (!el || !info || !src) return;
		status = 'loading';
		let timer: ReturnType<typeof setTimeout> | undefined;
		const b = new PluginBridge(
			{
				source: () => el.contentWindow,
				// The frame is sandboxed without allow-same-origin, so its origin is opaque ("null"): '*' is the only
				// targetOrigin that reaches it. Safe because `el.contentWindow` is our own frame and messages carry
				// only what the plugin is allowed to see.
				post: (msg) => el.contentWindow?.postMessage(msg, '*'),
				state: () => untrack(() => snapshot(el)),
				visible: () => isDeckVisible(),
				subscribe: (topic, push) => startTopic(TOPIC_IPC[topic], push),
				onReady: () => {
					clearTimeout(timer);
					status = 'ready';
				}
			},
			info.topics
		);
		bridge = b;
		timer = setTimeout(() => {
			if (!b.ready) status = 'error';
		}, READY_TIMEOUT_MS);
		const onMessage = (e: MessageEvent) => void b.handle(e);
		window.addEventListener('message', onMessage);
		const stopVisibility = onDeckVisibility((v) => b.setVisible(v));

		// Theme / appearance changes land as inline styles on the root (applyTheme) or on the card frame
		// (per-instance surface opacity). Coalesced to one read per frame; the bridge drops no-op pushes.
		let raf = 0;
		const schedule = () => {
			if (!raf) raf = requestAnimationFrame(() => ((raf = 0), b.update()));
		};
		const observer = new MutationObserver(schedule);
		observer.observe(document.documentElement, { attributes: true, attributeFilter: ['style', 'data-theme', 'class'] });
		const card = el.closest('[data-instance]');
		if (card) observer.observe(card, { attributes: true, attributeFilter: ['style'] });

		return () => {
			clearTimeout(timer);
			if (raf) cancelAnimationFrame(raf);
			observer.disconnect();
			stopVisibility();
			window.removeEventListener('message', onMessage);
			b.destroy();
			if (bridge === b) bridge = null;
		};
	});

	// size / settings / editing pushes (theme goes through the observer above).
	$effect(() => {
		void size;
		void JSON.stringify(pluginSettings);
		void editing;
		void available;
		bridge?.update();
	});

	function retry() {
		status = 'loading';
		attempt++;
	}
</script>

{#if !src}
	<FallbackCard {compact} icon="alert" title={name} meta={sizeLabel} body={t('plugins.frame.noEntry')} />
{:else if status === 'error'}
	<FallbackCard {compact} icon="alert" title={name} meta={sizeLabel} body={t('plugins.frame.timeout')} onretry={retry} />
{:else}
	<section class="k-card plugin" aria-label={name} aria-busy={status === 'loading'} data-plugin={plugin?.id} data-plugin-instance={instance}>
		{#if status === 'loading'}
			<div class="skeleton" aria-hidden="true">
				<span class="bar" style:width="40%" style:height="12px"></span>
				<span class="bar fill"></span>
			</div>
		{/if}
		{#key `${attempt}|${pluginKey}`}
			<iframe
				bind:this={iframe}
				class:shown={status === 'ready'}
				{src}
				title={name}
				sandbox="allow-scripts"
				referrerpolicy="no-referrer"
				loading="eager"
			></iframe>
		{/key}
		{#if editing}
			<!-- iframes swallow pointer events: this keeps long-press / drag on the deck while editing. -->
			<div class="overlay" aria-hidden="true"></div>
		{/if}
	</section>
{/if}

<style>
	.plugin {
		position: relative;
	}
	iframe {
		position: absolute;
		inset: 0;
		width: 100%;
		height: 100%;
		border: 0;
		display: block;
		background: transparent;
		/* Matches the frame document's default scheme; a mismatch makes Chromium paint an opaque canvas. */
		color-scheme: normal;
		opacity: 0;
	}
	iframe.shown {
		opacity: 1;
	}
	.overlay {
		position: absolute;
		inset: 0;
		background: transparent;
	}
	.skeleton {
		position: absolute;
		inset: 0;
		padding: 18px 20px;
		display: flex;
		flex-direction: column;
		gap: 12px;
	}
	.bar {
		display: block;
		border-radius: 8px;
		background: var(--k-bg-alt);
	}
	.bar.fill {
		flex: 1;
		min-height: 0;
	}
</style>
