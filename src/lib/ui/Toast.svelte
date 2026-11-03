<script lang="ts">
	/** Single transient notice (placement results, import errors, Undo). Polite live region. */
	import Icon from './Icon.svelte';
	import { getDeck } from './deck.svelte';

	const { ui } = getDeck();
</script>

<div class="region" role="status" aria-live="polite">
	{#if ui.notice}
		{#key ui.notice.id}
			<div class="toast" class:warning={ui.notice.tone === 'warning'} class:actionable={!!ui.notice.action}>
				{#if ui.notice.tone === 'warning'}<Icon name="alert" size={16} />{/if}
				<span class="text">{ui.notice.text}</span>
				{#if ui.notice.action}
					<button type="button" class="action" aria-keyshortcuts="Control+Z" onclick={() => ui.runAction()}>
						{ui.notice.action.label}
					</button>
				{/if}
			</div>
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
	.toast.actionable {
		padding: 0 6px 0 16px;
		pointer-events: auto;
	}
	.toast.warning {
		border-color: var(--k-warning);
		background: color-mix(in srgb, var(--k-warning) 14%, var(--k-bg-alt));
	}
	.toast.warning :global(svg) {
		color: var(--k-warning);
		flex: none;
	}
	.text {
		min-width: 0;
	}
	.action {
		flex: none;
		min-height: 44px;
		min-width: 44px;
		padding: 0 14px;
		border: 0;
		border-radius: 10px;
		background: transparent;
		/* Plain text colour keeps 4.5:1 in light themes; the accent marks it as the action. */
		color: var(--k-text);
		font-size: 13px;
		font-weight: 600;
		text-decoration: underline 2px var(--k-accent);
		text-underline-offset: 4px;
	}
	.action:hover {
		background: color-mix(in srgb, var(--k-accent) 12%, transparent);
	}
	@keyframes k-toast-in {
		from {
			opacity: 0;
			transform: translateY(6px);
		}
	}
</style>
