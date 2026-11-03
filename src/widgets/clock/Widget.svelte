<script lang="ts">
	import { locale, t } from '#lib/i18n/index.ts';
	import {
		cityFromZone,
		dayOfYear,
		formatClock,
		formatLongDate,
		formatShortDate,
		formatUtcOffset,
		isValidTimeZone,
		isoWeek,
		localTimeZone,
		subscribeTick,
		utcOffsetMinutes,
		zonedParts
	} from '#lib/time/index.ts';
	import type { WidgetProps } from '../types';
	import { handAngles, handEnd, sanitizeZones, zoneLabel } from './logic';
	import { defaults, type ClockSettings } from './settings';

	let { size, settings }: WidgetProps<ClockSettings> = $props();

	const s = $derived({ ...defaults, ...settings });
	const tz = $derived(isValidTimeZone(s.timeZone) ? s.timeZone : undefined);
	const analog = $derived(size === '2x2' && s.face === 'analog');
	// The analog face has no second hand: tick per minute there (idle CPU on an always-on screen).
	const wantSeconds = $derived(s.seconds && !analog);

	let now = $state(new Date());
	$effect(() => subscribeTick((d) => (now = d), { seconds: wantSeconds }));

	const clock = $derived(formatClock(now, { locale, hour12: s.hour12, timeZone: tz }));
	const parts = $derived(zonedParts(now, tz));
	const week = $derived(isoWeek(parts.year, parts.month, parts.day));
	const city = $derived(s.city.trim() || cityFromZone(tz ?? localTimeZone()));
	const offset = $derived(formatUtcOffset(utcOffsetMinutes(now, tz)));
	const zones = $derived(
		sanitizeZones(s.zones).map((z) => {
			const c = formatClock(now, { locale, hour12: s.hour12, timeZone: z.timeZone });
			return { key: z.timeZone, label: zoneLabel(z), time: c.period ? `${c.hm} ${c.period}` : c.hm };
		})
	);

	const hands = $derived.by(() => {
		const a = handAngles(parts.hour, parts.minute);
		return { hour: handEnd(a.hour, 40), minute: handEnd(a.minute, 61) };
	});
	const spoken = $derived(clock.period ? `${clock.hm} ${clock.period}` : clock.hm);
</script>

<section class="k-card clock" data-size={size} aria-label={t('widgets.clock.title')}>
	{#if size === '4x2'}
		<div class="big">
			<span class="hm">{clock.hm}</span>
			{#if s.seconds}<span class="sec">{clock.seconds}</span>{/if}
			{#if clock.period}<span class="sec">{clock.period}</span>{/if}
		</div>
		<div class="bottom">
			<div class="dateblock">
				<span class="long">{formatLongDate(now, locale, tz)}</span>
				<span class="meta">
					{t('widgets.common.week', { week })} · {t('widgets.common.day', {
						day: dayOfYear(parts.year, parts.month, parts.day)
					})} · {city}
					{offset}
				</span>
			</div>
			{#if zones.length}
				<ul class="zones" aria-label={t('widgets.clock.zonesLabel')}>
					{#each zones as z (z.key)}
						<li>
							<span class="tiny">{z.label}</span>
							<span class="ztime">{z.time}</span>
						</li>
					{/each}
				</ul>
			{/if}
		</div>
	{:else if size === '2x2'}
		<header class="hdr">
			<span class="title">{city}</span>
			<span class="hdr-meta">{analog ? spoken : offset}</span>
		</header>
		{#if analog}
			<svg class="face" viewBox="0 0 150 150" role="img" aria-label={t('widgets.clock.analogLabel', { time: spoken })}>
				<circle class="dial" cx="75" cy="75" r="70" />
				<line class="tick" x1="75" y1="9" x2="75" y2="18" />
				<line class="tick" x1="141" y1="75" x2="132" y2="75" />
				<line class="tick" x1="75" y1="141" x2="75" y2="132" />
				<line class="tick" x1="9" y1="75" x2="18" y2="75" />
				<line class="hour" x1="75" y1="75" x2={hands.hour.x} y2={hands.hour.y} />
				<line class="minute" x1="75" y1="75" x2={hands.minute.x} y2={hands.minute.y} />
				<circle class="hub" cx="75" cy="75" r="4" />
			</svg>
		{:else}
			<div class="digital">
				<span class="hm">{clock.hm}</span>
				{#if s.seconds}<span class="sec">{clock.seconds}</span>{/if}
				{#if clock.period}<span class="sec">{clock.period}</span>{/if}
			</div>
		{/if}
		<span class="foot">{formatShortDate(now, locale, tz)} · {t('widgets.common.weekShort', { week })}</span>
	{:else}
		<div class="compact">
			<span class="time">
				<span class="hm">{clock.hm}</span>
				{#if s.seconds}<span class="sec">{clock.seconds}</span>{/if}
				{#if clock.period}<span class="sec">{clock.period}</span>{/if}
			</span>
			<span class="cdate">
				<span>{formatShortDate(now, locale, tz)}</span>
				<span>{t('widgets.common.weekShort', { week })}</span>
			</span>
		</div>
	{/if}
</section>

<style>
	.clock {
		container-type: size;
		justify-content: space-between;
		font-family: var(--k-font-ui);
	}
	.clock[data-size='4x2'] {
		padding: 22px 28px;
	}
	.clock[data-size='2x2'] {
		padding: 16px 18px;
		align-items: center;
	}
	.clock[data-size='2x1'] {
		padding: 14px 18px;
		justify-content: center;
	}

	.hm,
	.sec,
	.ztime,
	.hdr-meta {
		font-family: var(--k-font-mono);
		font-variant-numeric: tabular-nums;
	}

	/* 4×2 ---------------------------------------------------------------- */
	.big {
		display: flex;
		align-items: baseline;
		gap: 0.08em;
		/* 128 px at 1280×720; shrinks with the card, never below 32 px (DESIGN §4.3). */
		font-size: max(32px, min(128px, 26cqw, calc(100cqh - 64px)));
		line-height: 1;
		white-space: nowrap;
	}
	.big .hm {
		font-weight: 500;
		letter-spacing: -0.04em;
	}
	.big .sec {
		font-size: 0.3125em; /* 40 / 128 */
		font-weight: 400;
		color: var(--k-text-muted);
		margin-left: 0.25em;
	}
	.bottom {
		display: flex;
		align-items: flex-end;
		justify-content: space-between;
		gap: 16px;
		min-width: 0;
	}
	.dateblock {
		display: flex;
		flex-direction: column;
		gap: 4px;
		min-width: 0;
	}
	.long {
		font-size: 18px;
		font-weight: 600;
	}
	.long,
	.meta {
		white-space: nowrap;
		overflow: hidden;
		text-overflow: ellipsis;
	}
	.meta {
		font-size: 13px;
		color: var(--k-text-muted);
	}
	.zones {
		display: flex;
		gap: 20px;
		margin: 0;
		padding: 0;
		list-style: none;
		flex-shrink: 0;
	}
	.zones li {
		display: flex;
		flex-direction: column;
		align-items: flex-end;
		gap: 2px;
	}
	.tiny {
		font-size: 11px;
		color: var(--k-text-muted);
		text-transform: uppercase;
		letter-spacing: 0.08em;
		white-space: nowrap;
	}
	.ztime {
		font-size: 20px;
		font-weight: 500;
		white-space: nowrap;
	}
	/* 1024×600: drop the third zone, then the secondary date line. */
	@container (width < 500px) {
		.zones li:nth-child(n + 3) {
			display: none;
		}
	}
	@container (height < 170px) {
		.meta {
			display: none;
		}
	}

	/* 2×2 ---------------------------------------------------------------- */
	.hdr {
		width: 100%;
		display: flex;
		justify-content: space-between;
		align-items: center;
		gap: 8px;
	}
	.title {
		font-size: 13px;
		font-weight: 600;
		white-space: nowrap;
		overflow: hidden;
		text-overflow: ellipsis;
	}
	.hdr-meta {
		font-size: 12px;
		color: var(--k-text-muted);
		white-space: nowrap;
	}
	.face {
		width: min(150px, calc(100cqh - 56px), 100cqw);
		height: min(150px, calc(100cqh - 56px), 100cqw);
		flex-shrink: 0;
	}
	.dial {
		fill: var(--k-bg);
		stroke: var(--k-border);
		stroke-width: 2;
	}
	.tick,
	.hour,
	.minute {
		stroke-linecap: round;
	}
	.tick {
		stroke: var(--k-text);
		stroke-width: 3;
	}
	.hour {
		stroke: var(--k-text);
		stroke-width: 5;
	}
	.minute {
		stroke: var(--k-accent);
		stroke-width: 3;
	}
	.hub {
		fill: var(--k-accent);
	}
	.digital {
		display: flex;
		align-items: baseline;
		justify-content: center;
		gap: 6px;
		white-space: nowrap;
		line-height: 1;
	}
	.digital .hm {
		font-size: max(32px, min(48px, 22cqw));
		font-weight: 500;
		letter-spacing: -0.03em;
	}
	.digital .sec {
		font-size: 20px;
		color: var(--k-text-muted);
	}
	.foot {
		font-size: 12px;
		color: var(--k-text-muted);
		white-space: nowrap;
	}
	@container (height < 190px) {
		.clock[data-size='2x2'] .foot {
			display: none;
		}
	}

	/* 2×1 ---------------------------------------------------------------- */
	.compact {
		display: flex;
		align-items: center;
		justify-content: space-between;
		gap: 10px;
		min-width: 0;
	}
	.time {
		display: flex;
		align-items: baseline;
		gap: 4px;
		white-space: nowrap;
		line-height: 1;
	}
	.compact .hm {
		font-size: 32px;
		font-weight: 500;
		letter-spacing: -0.03em;
	}
	.compact .sec {
		font-size: 14px;
		color: var(--k-text-muted);
	}
	.cdate {
		display: flex;
		flex-direction: column;
		align-items: flex-end;
		gap: 2px;
		font-size: 12px;
		color: var(--k-text-muted);
		text-align: right;
		min-width: 0;
	}
	.cdate span {
		white-space: nowrap;
		overflow: hidden;
		text-overflow: ellipsis;
		max-width: 100%;
	}
	@container (width < 170px) {
		.cdate {
			display: none;
		}
	}
</style>
