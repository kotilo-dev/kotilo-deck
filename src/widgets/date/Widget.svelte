<script lang="ts">
	import { locale, t } from '#lib/i18n/index.ts';
	import { formatMonth, formatWeekday, subscribeTick } from '#lib/time/index.ts';
	import type { WidgetProps } from '../types';
	import { dateFacts } from './logic';
	import { defaults, type DateSettings } from './settings';

	let { size, settings }: WidgetProps<DateSettings> = $props();

	const s = $derived({ ...defaults, ...settings });

	// Minute ticks are enough to roll over at midnight.
	let now = $state(new Date());
	$effect(() => subscribeTick((d) => (now = d)));

	const facts = $derived(dateFacts(now));
</script>

<section class="k-card date" data-size={size} aria-label={t('widgets.date.title')}>
	<div class="row">
		<div class="left">
			<span class="num">{facts.day}</span>
			<span class="month">{formatMonth(now, locale, 'long')}</span>
		</div>
		<div class="right">
			<span class="weekday">{formatWeekday(now, locale, 'long')}</span>
			<span class="sub">{t('widgets.common.week', { week: facts.week })}</span>
			{#if s.dayOfYear}
				<span class="sub">{t('widgets.common.day', { day: facts.dayOfYear })}</span>
			{/if}
		</div>
	</div>
</section>

<style>
	.date {
		container-type: size;
		padding: 14px 18px;
		justify-content: center;
		font-family: var(--k-font-ui);
	}
	.row {
		display: flex;
		align-items: center;
		justify-content: space-between;
		gap: 12px;
		min-width: 0;
	}
	.left,
	.right {
		display: flex;
		flex-direction: column;
		gap: 2px;
		min-width: 0;
	}
	.right {
		align-items: flex-end;
		text-align: right;
	}
	.num {
		font-family: var(--k-font-mono);
		font-variant-numeric: tabular-nums;
		font-size: 32px;
		font-weight: 500;
		line-height: 1;
		letter-spacing: -0.03em;
	}
	.month,
	.sub {
		font-size: 12px;
		color: var(--k-text-muted);
	}
	.weekday {
		font-size: 13px;
		font-weight: 600;
	}
	.month,
	.sub,
	.weekday {
		white-space: nowrap;
		overflow: hidden;
		text-overflow: ellipsis;
		max-width: 100%;
	}
	@container (height < 72px) {
		.month {
			display: none;
		}
	}
</style>
