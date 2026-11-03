<script lang="ts">
	/** Muted card used for "Unknown widget" and the widget error boundary (DESIGN §8). */
	import { t } from '#lib/i18n/index.ts';
	import Icon from './Icon.svelte';

	let {
		title,
		meta = '',
		body,
		icon = 'puzzle',
		compact = false,
		onretry
	}: { title: string; meta?: string; body: string; icon?: string; compact?: boolean; onretry?: () => void } = $props();
</script>

<section class="k-card fallback" class:compact aria-label={title}>
	<header>
		<span class="title">{title}</span>
		{#if meta}<span class="meta k-mono">{meta}</span>{/if}
	</header>
	<div class="body">
		<span class="icon"><Icon name={icon} size={compact ? 18 : 20} /></span>
		<p>{body}</p>
		{#if onretry}
			<button type="button" class="k-btn" onclick={onretry}>
				<Icon name="refresh" size={16} />{t('shell.widget.retry')}
			</button>
		{/if}
	</div>
</section>

<style>
	.fallback {
		padding: 18px 20px;
		gap: 10px;
	}
	.fallback.compact {
		padding: 14px 18px;
		gap: 6px;
	}
	header {
		display: flex;
		justify-content: space-between;
		align-items: baseline;
		gap: 8px;
		min-width: 0;
	}
	.title {
		font-size: 13px;
		font-weight: 600;
		white-space: nowrap;
		overflow: hidden;
		text-overflow: ellipsis;
	}
	.meta {
		font-size: 11px;
		color: var(--k-text-muted);
	}
	.body {
		flex: 1;
		min-height: 0;
		display: flex;
		flex-direction: column;
		align-items: center;
		justify-content: center;
		gap: 8px;
		text-align: center;
		color: var(--k-text-muted);
		overflow: hidden;
	}
	.compact .body {
		flex-direction: row;
		justify-content: flex-start;
		text-align: left;
	}
	p {
		margin: 0;
		font-size: 12px;
		overflow: hidden;
		display: -webkit-box;
		-webkit-line-clamp: 2;
		line-clamp: 2;
		-webkit-box-orient: vertical;
	}
	.icon {
		display: inline-flex;
		flex: none;
	}
	.k-btn {
		flex: none;
		font-size: 12px;
	}
</style>
