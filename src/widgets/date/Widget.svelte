<script lang="ts">
	import { locale, t } from '#lib/i18n/index.ts';
	import { dtf, formatMonth, formatWeekday, subscribeTick } from '#lib/time/index.ts';
	import type { WidgetProps } from '../types';
	import { dateFacts, weekStart, weekStrip } from './logic';
	import { defaults, type DateSettings } from './settings';

	let { size, settings }: WidgetProps<DateSettings> = $props();

	const s = $derived({ ...defaults, ...settings });

	// Minute ticks are enough to roll over at midnight.
	let now = $state(new Date());
	$effect(() => subscribeTick((d) => (now = d)));

	const facts = $derived(dateFacts(now));
	const strip = $derived(weekStrip(now, weekStart(locale)));
	const narrow = $derived(dtf(locale, { weekday: 'narrow' }));
	const longDay = $derived(dtf(locale, { weekday: 'long', day: 'numeric', month: 'long' }));
	const subLine = $derived(
		[
			formatMonth(now, locale, 'long'),
			t('widgets.common.week', { week: facts.week }),
			s.dayOfYear ? t('widgets.common.day', { day: facts.dayOfYear }) : null
		]
			.filter(Boolean)
			.join(' · ')
	);
</script>

<section class="k-card date" data-size={size} aria-label={t('widgets.date.title')}>
	<!-- Large 2×1 (DESIGN §5.2): scaled day number + weekday block over a week strip. Short: the row below. -->
	<div class="wide">
		<div class="top">
			<span class="num big">{facts.day}</span>
			<span class="wblock">
				<span class="wday">{formatWeekday(now, locale, 'long')}</span>
				<span class="wsub">{subLine}</span>
			</span>
		</div>
		<ol class="strip" aria-label={t('widgets.date.week')}>
			{#each strip as d (d.time)}
				<li class:past={d.past} class:today={d.today} aria-current={d.today ? 'date' : undefined} aria-label={longDay.format(d.time)}>
					<span aria-hidden="true">{narrow.format(d.time)}</span>
					<span aria-hidden="true">{d.day}</span>
				</li>
			{/each}
		</ol>
	</div>
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

	.wide {
		display: none;
		flex-direction: column;
		justify-content: space-between;
		gap: 10px;
		height: 100%;
		min-width: 0;
	}
	.top {
		display: flex;
		align-items: center;
		gap: 0.22em;
		min-width: 0;
		/* Day number: 32 px floor, 112 px at 1920×1080 (DESIGN §3). */
		font-size: max(32px, min(112px, 24cqw, calc(100cqh - 70px)));
	}
	.num.big {
		font-size: 1em;
		letter-spacing: -0.04em;
	}
	.wblock {
		display: flex;
		flex-direction: column;
		gap: 4px;
		min-width: 0;
	}
	.wday {
		font-size: max(15px, min(24px, 0.32em));
		font-weight: 600;
	}
	.wsub {
		font-size: max(12px, min(15px, 0.2em));
		color: var(--k-text-muted);
	}
	.wday,
	.wsub {
		white-space: nowrap;
		overflow: hidden;
		text-overflow: ellipsis;
	}
	/* Week strip: 7 equal cells "M 2", mono 13; today a filled accent pill; past days muted. */
	.strip {
		display: grid;
		grid-template-columns: repeat(7, 1fr);
		gap: 4px;
		margin: 0;
		padding: 0;
		list-style: none;
		flex-shrink: 0;
	}
	.strip li {
		display: flex;
		align-items: center;
		justify-content: center;
		gap: 0.4em;
		height: 30px;
		border-radius: 999px;
		font-family: var(--k-font-mono);
		font-variant-numeric: tabular-nums;
		font-size: 13px;
		white-space: nowrap;
	}
	.strip li.past {
		color: var(--k-text-muted);
	}
	.strip li.today {
		background: var(--k-accent);
		color: var(--k-on-accent);
		font-weight: 600;
	}
	@container (height >= 100px) and (width >= 220px) {
		.wide {
			display: flex;
		}
		.row {
			display: none;
		}
	}
</style>
