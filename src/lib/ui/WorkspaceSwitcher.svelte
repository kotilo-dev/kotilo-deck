<script lang="ts">
	/** Top-bar workspace pills (DESIGN §4.2). The active fill slides between pills over 200 ms. */
	import { t } from '#lib/i18n/index.ts';
	import { getDeck } from './deck.svelte';

	const { store } = getDeck();

	let nav = $state<HTMLElement>();
	let indicator = $state({ left: 0, width: 0, ready: false });

	function measure() {
		const el = nav?.querySelector<HTMLElement>('[aria-current="true"]');
		if (!el) return;
		indicator = { left: el.offsetLeft, width: el.offsetWidth, ready: true };
		el.scrollIntoView({ block: 'nearest', inline: 'nearest' });
	}

	$effect(() => {
		// Re-measure when the active id or any name (pill width) changes.
		void store.activeId;
		void store.workspaces.map((w) => w.name).join('\u0000');
		requestAnimationFrame(measure);
	});
</script>

<svelte:window onresize={measure} />

<nav bind:this={nav} class="pills k-bar-pill" aria-label={t('shell.topbar.workspaces')}>
	<span
		class="indicator"
		class:ready={indicator.ready}
		style:transform="translateX({indicator.left}px)"
		style:width="{indicator.width}px"
		aria-hidden="true"
	></span>
	{#each store.workspaces as ws (ws.id)}
		<button
			type="button"
			class="pill"
			aria-current={ws.id === store.activeId}
			onclick={() => store.switchTo(ws.id)}
		>
			{ws.name}
		</button>
	{/each}
</nav>

<style>
	.pills {
		position: relative;
		display: flex;
		align-items: center;
		gap: 4px;
		min-width: 0;
		max-width: 100%;
		padding: 4px;
		border-radius: 999px;
		border: 1px solid var(--k-border);
		background: var(--k-bg-alt);
		overflow-x: auto;
		scrollbar-width: none;
	}
	.indicator {
		position: absolute;
		top: 4px;
		left: 0;
		height: 36px;
		border-radius: 999px;
		background: var(--k-accent);
		opacity: 0;
	}
	.indicator.ready {
		opacity: 1;
		transition:
			transform 200ms ease,
			width 200ms ease;
	}
	.pill {
		position: relative;
		flex: none;
		height: 36px;
		padding: 0 18px;
		border: 0;
		border-radius: 999px;
		background: transparent;
		color: var(--k-text-muted);
		font-size: 13px;
		font-weight: 500;
		white-space: nowrap;
		transition: color 200ms ease;
	}
	.pill[aria-current='true'] {
		color: var(--k-on-accent);
		font-weight: 600;
	}
	.pill:focus-visible {
		outline-offset: -2px;
	}
</style>
