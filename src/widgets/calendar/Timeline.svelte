<script lang="ts">
	/**
	 * Day timeline (DESIGN §5.4): all-day lane, timed blocks in up to two sub-lanes (+N beyond), a "now"
	 * marker and hour labels. Purely visual: the same events are listed as rows, so it is aria-hidden.
	 * Positions come from `timeline.ts`; `now` is the widget's shared minute tick (no timers here).
	 */
	import type { CalendarEvent } from '#lib/ipc/index.ts';
	import { dtf } from '#lib/time/index.ts';
	import { hourTicks, layoutTimeline, nowAt, timelineRange } from './timeline';

	let {
		events,
		now,
		from,
		to,
		allDay,
		locale,
		colorOf,
		moreLabel
	}: {
		events: readonly CalendarEvent[];
		now: number;
		from: unknown;
		to: unknown;
		allDay: boolean;
		locale: string;
		colorOf: (feedId: string) => string;
		moreLabel: (n: number) => string;
	} = $props();

	const range = $derived(timelineRange(now, from, to));
	const layout = $derived(layoutTimeline(events, range, allDay));
	const nowPos = $derived(nowAt(now, range));
	const ticks = $derived(hourTicks(range).filter((t) => t.at < 1));
	const pct = (f: number) => `${(f * 100).toFixed(3)}%`;

	/** Hour number only ("9", "13" or "1" in 12 h locales), as the mockup's axis. */
	function hourLabel(time: number): string {
		const parts = dtf(locale, { hour: 'numeric' }).formatToParts(new Date(time));
		return parts.find((p) => p.type === 'hour')?.value ?? '';
	}
	const nowLabel = $derived(dtf(locale, { hour: '2-digit', minute: '2-digit' }).format(new Date(now)));
</script>

<div class="tl" aria-hidden="true">
	<div class="panel">
		{#if layout.allDay.length}
			<div class="allday">
				{#each layout.allDay as e (e.id)}
					<span class="ad" style:--fc={colorOf(e.feedId)}>{e.title}</span>
				{/each}
			</div>
		{/if}
		<div class="lane">
			{#each layout.blocks as b (b.event.id)}
				<div
					class="blk"
					class:half={b.lanes === 2}
					class:low={b.lanes === 2 && b.lane === 1}
					style:--fc={colorOf(b.event.feedId)}
					style:left={pct(b.left)}
					style:width={pct(b.width)}
				>
					{#if b.label}<span class="blk-title">{b.event.title}</span>{/if}
				</div>
			{/each}
			{#each layout.more as m (m.left)}
				<div class="blk half low more" style:left={pct(m.left)} style:width={pct(m.width)}>
					<span class="blk-title">{moreLabel(m.count)}</span>
				</div>
			{/each}
		</div>
		{#if nowPos !== null}
			<!-- Same horizontal box as the lane (inside the panel's 8 px padding), full panel height. -->
			<div class="now-track">
				<div class="now" style:left={pct(nowPos)}>
					<span class="now-label">{nowLabel}</span>
				</div>
			</div>
		{/if}
	</div>
	<div class="hours">
		{#each ticks as tk (tk.time)}
			<span style:left={pct(tk.at)}>{hourLabel(tk.time)}</span>
		{/each}
	</div>
</div>

<style>
	.tl {
		flex: 1;
		min-height: 0;
		display: flex;
		flex-direction: column;
		gap: 6px;
	}
	.panel {
		position: relative;
		flex: 1;
		min-height: 0;
		display: flex;
		flex-direction: column;
		gap: 6px;
		padding: 8px;
		border-radius: 12px;
		background: var(--k-inset-deep);
		box-shadow: inset 0 0 0 1px var(--k-inset-ring);
		overflow: hidden;
	}
	/* Top lane, 20 px: all-day events as tinted bars side by side. */
	.allday {
		display: flex;
		gap: 4px;
		height: 20px;
		flex-shrink: 0;
	}
	.ad {
		flex: 1;
		min-width: 0;
		padding: 0 10px;
		border-radius: 6px;
		background: color-mix(in srgb, var(--fc) 30%, transparent);
		color: var(--k-text);
		font-size: 11px;
		font-weight: 500;
		line-height: 20px;
		white-space: nowrap;
		overflow: hidden;
		text-overflow: ellipsis;
	}
	.lane {
		position: relative;
		flex: 1;
		min-height: 0;
	}
	/* Timed block: feed colour ~30 % with a 4 px solid left edge; 2 px between columns and sub-lanes. */
	.blk {
		position: absolute;
		top: 0;
		box-sizing: border-box;
		height: 100%;
		padding: 6px 8px 0 10px;
		border-left: 4px solid var(--fc);
		border-radius: 6px;
		background: color-mix(in srgb, var(--fc) 30%, transparent);
		background-clip: padding-box;
		border-right: 2px solid transparent;
		overflow: hidden;
	}
	.blk.half {
		height: calc(50% - 1px);
	}
	.blk.low {
		top: calc(50% + 1px);
	}
	.blk.more {
		--fc: var(--k-text-muted);
	}
	.blk-title {
		display: block;
		font-size: 12px;
		font-weight: 500;
		color: var(--k-text);
		white-space: nowrap;
		overflow: hidden;
		text-overflow: ellipsis;
	}
	/* "Now": 2 px line in --k-text across the panel, small mono label at its foot. */
	.now-track {
		position: absolute;
		inset: 0 8px;
		pointer-events: none;
	}
	.now {
		position: absolute;
		top: 0;
		bottom: 0;
		width: 2px;
		margin-left: -1px;
		background: var(--k-text);
		pointer-events: none;
	}
	.now-label {
		position: absolute;
		bottom: 6px;
		right: 6px;
		padding: 1px 5px;
		border-radius: 5px;
		background: var(--k-bg-alt);
		border: 1px solid var(--k-border);
		color: var(--k-text);
		font-family: var(--k-font-mono);
		font-size: 11px;
		font-weight: 600;
		white-space: nowrap;
	}
	/* Hour labels under the lane, mono 11 muted, left-aligned on their tick (the panel's 8 px inset). */
	.hours {
		position: relative;
		height: 14px;
		margin: 0 8px;
		flex-shrink: 0;
	}
	.hours span {
		position: absolute;
		top: 0;
		font-family: var(--k-font-mono);
		font-size: 11px;
		line-height: 14px;
		color: var(--k-text-muted);
	}
</style>
