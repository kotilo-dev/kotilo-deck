<script lang="ts">
	/** Single transient notice (placement results, import errors). Polite live region. */
	import Icon from './Icon.svelte';
	import { getDeck } from './deck.svelte';

	const { ui } = getDeck();
</script>

<div class="region" role="status" aria-live="polite">
	{#if ui.notice}
		{#key ui.notice.id}
			<p class="toast" class:warning={ui.notice.tone === 'warning'}>
				{#if ui.notice.tone === 'warning'}<Icon name="alert" size={16} />{/if}
				{ui.notice.text}
			</p>
		{/key}
	{/if}
</div>

<style>
	.region {
		position: fixed;
		left: 50%;
		bottom: 64px;
		z-index: 30;
		transform: translateX(-50%);
		pointer-events: none;
		max-width: min(640px, calc(100vw - 48px));
	}
	.toast {
		margin: 0;
		display: flex;
		align-items: center;
		gap: 8px;
		padding: 10px 16px;
		border-radius: 12px;
		border: 1px solid var(--k-border);
		background: var(--k-bg-alt);
		color: var(--k-text);
		font-size: 13px;
		animation: k-toast-in 160ms ease-out;
	}
	.toast.warning {
		border-color: var(--k-warning);
		background: color-mix(in srgb, var(--k-warning) 14%, var(--k-bg-alt));
	}
	.toast.warning :global(svg) {
		color: var(--k-warning);
		flex: none;
	}
	@keyframes k-toast-in {
		from {
			opacity: 0;
			transform: translateY(6px);
		}
	}
</style>
