<script lang="ts">
	import { locale, t } from '#lib/i18n/index.ts';
	import {
		calendarRefresh,
		calendarUpcoming,
		hasBackend,
		isTauri,
		on,
		settingsOpen,
		type CalendarEvent,
		type CalendarFeed,
		type CalendarUpcoming
	} from '#lib/ipc/index.ts';
	import { formatShortDate, isDeckVisible, onDeckVisibility, subscribeTick } from '#lib/time/index.ts';
	import type { WidgetProps } from '../types';
	import {
		agenda,
		countdown,
		countdownPhrase,
		dayWord,
		enabledFeeds,
		feedById,
		feedColor,
		feedsKey,
		fitRows,
		groupRows,
		listDay,
		RELOAD_MS,
		ROW_CAP,
		shortDay,
		timeRange,
		UPCOMING_DAYS,
		UPCOMING_LIMIT,
		type Row
	} from './logic';
	import { defaults, type CalendarSettings } from './settings';
	import { loadFeeds, watchFeeds } from './store';

	let { size, settings, editing }: WidgetProps<CalendarSettings> = $props();

	const s = $derived({ ...defaults, ...settings });
	const tauri = hasBackend();

	/** Row 32 px (8 px padding + 16 px line), day heading 18 px, 4 px gap. */
	const ROW = 32;
	const HEAD = 18;
	const GAP = 4;

	let feeds = $state<CalendarFeed[]>([]);
	let feedsLoaded = $state(false);
	let data = $state<CalendarUpcoming | null>(null);
	let failed = $state(false);
	/** Last call failed but older data is still shown → stale badge. */
	let refreshFailed = $state(false);
	let loading = $state(false);
	let loadedAt = 0;
	let now = $state(Date.now());
	let visible = $state(isDeckVisible());

	let seq = 0;
	async function load() {
		if (!tauri) return;
		const mine = ++seq;
		loading = true;
		try {
			const next = await calendarUpcoming(UPCOMING_LIMIT, UPCOMING_DAYS);
			if (mine !== seq) return;
			data = next;
			failed = false;
			refreshFailed = false;
			loadedAt = Date.now();
			now = loadedAt;
		} catch {
			if (mine !== seq) return;
			if (data) refreshFailed = true;
			else failed = true;
		} finally {
			if (mine === seq) loading = false;
		}
	}

	async function retry() {
		if (editing) return;
		try {
			await calendarRefresh();
		} catch {
			// The reload below reports the outcome.
		}
		await load();
	}

	function openSettings() {
		if (editing) return;
		if (isTauri()) void settingsOpen().catch(() => {});
		else window.open('/settings', 'kotilo-deck-settings', 'width=960,height=680');
	}

	$effect(() => {
		loadFeeds()
			.then((f) => {
				feeds = f;
				feedsLoaded = true;
			})
			.catch(() => (feedsLoaded = true));
		const offFeeds = watchFeeds((f) => {
			const changed = feedsKey(f) !== feedsKey(feeds);
			feeds = f;
			feedsLoaded = true;
			if (changed && visible) load();
		});
		return () => {
			offFeeds();
			seq++;
		};
	});

	// Data: on show (also the first mount) and on `calendar://changed`; nothing runs while hidden.
	$effect(() => {
		if (!visible || !tauri) return;
		load();
		let disposed = false;
		let off: (() => void) | null = null;
		on('calendar://changed', () => load())
			.then((u) => (disposed ? u() : (off = u)))
			.catch(() => {});
		return () => {
			disposed = true;
			off?.();
		};
	});

	$effect(() => onDeckVisibility((v) => (visible = v)));

	// Countdown chip + "now" boundaries on the shared minute tick; also a 15 min safety reload.
	$effect(() =>
		subscribeTick((d) => {
			now = d.getTime();
			if (tauri && loadedAt && now - loadedAt >= RELOAD_MS && !loading) load();
		})
	);

	// ── View ───────────────────────────────────────────────────────────────
	const active = $derived(enabledFeeds(feeds));
	const view = $derived(agenda(data?.events ?? [], now, s.allDay));
	const stale = $derived(!!data && (data.stale || refreshFailed));
	const errors = $derived(data?.errors ?? []);
	const meta = $derived(
		[active.length ? t(active.length === 1 ? 'widgets.calendar.feedsOne' : 'widgets.calendar.feedsMany', { n: active.length }) : null, formatShortDate(new Date(now), locale)]
			.filter(Boolean)
			.join(' · ')
	);

	let rowsH = $state(0);
	const grouped = $derived(groupRows(view.rest, now, ROW_CAP[size === '4x4' ? '4x4' : '4x2']));
	const rows = $derived(fitRows(grouped, rowsH, ROW, HEAD, GAP));

	const whenCh = $derived(Math.max(0, ...rows.map((r) => (r.kind === 'event' ? whenText(r.event).length : 0))));

	const feedName = (id: string) => feedById(feeds, id)?.name ?? '';
	const colorOf = (id: string) => feedColor(feedById(feeds, id)?.color);

	function dayLabel(day: number): string {
		const w = dayWord(day, now);
		return w ? t(`widgets.calendar.${w}`) : shortDay(day, locale);
	}

	function whenText(e: CalendarEvent): string {
		return timeRange(e, locale) ?? t('widgets.calendar.allDay');
	}

	const next = $derived(view.next);
	const nextCd = $derived(next ? countdown(next, now) : null);
	const chip = $derived.by(() => {
		if (!nextCd) return '';
		const p = countdownPhrase(nextCd);
		return t(`widgets.calendar.${p.key}`, p.params);
	});
	const nextSub = $derived.by(() => {
		if (!next) return '';
		const day = listDay(next, now);
		const when = dayWord(day, now) === 'today' ? whenText(next) : `${dayLabel(day)} ${whenText(next)}`;
		return [when, next.location, feedName(next.feedId)].filter(Boolean).join(' · ');
	});
	const errorText = $derived.by(() => {
		if (errors.length === 0) return '';
		if (errors.length === 1) {
			const name = feedName(errors[0].feedId) || t('widgets.calendar.feed');
			return t('widgets.calendar.feedError', { name });
		}
		return t('widgets.calendar.feedErrors', { n: errors.length });
	});
	const errorTitle = $derived(errors.map((e) => `${feedName(e.feedId) || e.feedId}: ${e.message}`).join('\n'));
</script>

{#snippet calendarIcon()}
	<svg width="22" height="22" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
		<path d="M5 5h14a2 2 0 0 1 2 2v12a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V7a2 2 0 0 1 2-2Z" /><path d="M16 3v4M8 3v4M3 11h18" />
	</svg>
{/snippet}

{#snippet row(r: Row)}
	{#if r.kind === 'day'}
		<li class="day" aria-hidden="true">{dayLabel(r.day)}</li>
	{:else}
		{@const e = r.event}
		<li class="row">
			<span class="fbar" style:background={colorOf(e.feedId)}></span>
			<span class="mono when">{whenText(e)}</span>
			<span class="what" title={e.location ? `${e.title} · ${e.location}` : e.title}>{e.title}</span>
			<span class="feed">{feedName(e.feedId)}</span>
		</li>
	{/if}
{/snippet}

<section class="k-card cal" data-size={size} aria-label={t('widgets.calendar.name')}>
	<header class="hdr">
		<span class="title">{t('widgets.calendar.title')}</span>
		<span class="meta-wrap">
			{#if stale}<span class="badge">{t('widgets.calendar.stale')}</span>{/if}
			<span class="meta">{meta}</span>
		</span>
	</header>

	{#if !feedsLoaded}
		<div class="skeleton" aria-busy="true" aria-label={t('widgets.common.loading')}>
			<span class="bar tall"></span><span class="bar"></span><span class="bar" style="width: 70%"></span>
		</div>
	{:else if active.length === 0}
		<div class="state">
			{@render calendarIcon()}
			<span class="state-title">{t('widgets.calendar.noFeeds')}</span>
			<span class="state-text">{t('widgets.calendar.noFeedsHint')}</span>
			<button type="button" class="k-btn" disabled={editing} onclick={openSettings}>{t('widgets.calendar.openSettings')}</button>
		</div>
	{:else if !tauri}
		<div class="state">
			{@render calendarIcon()}
			<span class="state-text">{t('widgets.calendar.browserPreview')}</span>
		</div>
	{:else if failed}
		<div class="state">
			<span class="state-title">{t('widgets.calendar.error')}</span>
			<button type="button" class="k-btn" disabled={editing} onclick={retry}>{t('widgets.common.retry')}</button>
		</div>
	{:else if !data}
		<div class="skeleton" aria-busy="true" aria-label={t('widgets.common.loading')}>
			<span class="bar tall"></span><span class="bar"></span><span class="bar" style="width: 70%"></span>
		</div>
	{:else if !next}
		<div class="state">
			{@render calendarIcon()}
			<span class="state-title">{t('widgets.calendar.empty')}</span>
			<span class="state-text">{t('widgets.calendar.emptyHint', { n: UPCOMING_DAYS })}</span>
		</div>
	{:else}
		<div class="next">
			<span class="fbar" style:background={colorOf(next.feedId)}></span>
			<div class="next-text">
				<span class="next-title" title={next.title}>{next.title}</span>
				<span class="next-sub">{nextSub}</span>
			</div>
			<span class="mono chip" class:over={nextCd?.kind === 'over'} class:now={nextCd?.kind === 'now'}>{chip}</span>
		</div>
		<div class="rows-wrap" bind:clientHeight={rowsH}>
			<ul class="rows" style:--when-ch={whenCh} aria-label={t('widgets.calendar.events')}>
				{#each rows as r (r.kind === 'day' ? `d${r.day}` : r.event.id)}
					{@render row(r)}
				{/each}
			</ul>
		</div>
	{/if}

	{#if data && errors.length && active.length && tauri}
		<footer class="foot" data-no-gesture>
			<span class="foot-text" title={errorTitle}>{errorText}</span>
			<button type="button" class="retry" disabled={editing || loading} onclick={retry}>{t('widgets.common.retry')}</button>
		</footer>
	{/if}
</section>

<style>
	.cal {
		container-type: size;
		padding: 18px 22px;
		gap: 10px;
		font-family: var(--k-font-ui);
	}
	.mono {
		font-family: var(--k-font-mono);
		font-variant-numeric: tabular-nums;
	}
	.hdr {
		display: flex;
		justify-content: space-between;
		align-items: center;
		gap: 12px;
		min-width: 0;
		flex-shrink: 0;
	}
	.title {
		font-size: 13px;
		font-weight: 600;
		flex-shrink: 0;
	}
	.meta-wrap {
		display: flex;
		align-items: center;
		gap: 8px;
		min-width: 0;
	}
	.meta {
		font-size: 11px;
		color: var(--k-text-muted);
		white-space: nowrap;
		overflow: hidden;
		text-overflow: ellipsis;
	}
	/* Stale badge, DESIGN §5.3 style. */
	.badge {
		flex-shrink: 0;
		font-size: 11px;
		font-weight: 500;
		padding: 2px 6px;
		border-radius: 6px;
		background: color-mix(in srgb, var(--k-warning) 18%, var(--k-surface));
		color: color-mix(in srgb, var(--k-warning) 35%, var(--k-text));
	}

	.fbar {
		width: 4px;
		align-self: stretch;
		border-radius: 2px;
		flex-shrink: 0;
	}

	/* Next event card: 14 % accent tint, 1 px accent border. */
	.next {
		display: flex;
		align-items: center;
		gap: 14px;
		padding: 12px 14px;
		border-radius: 12px;
		background: color-mix(in srgb, var(--k-accent) 14%, var(--k-surface));
		border: 1px solid var(--k-accent);
		min-width: 0;
		flex-shrink: 0;
	}
	.next-text {
		flex: 1;
		min-width: 0;
		display: flex;
		flex-direction: column;
		gap: 2px;
	}
	.next-title {
		font-size: 15px;
		font-weight: 600;
		white-space: nowrap;
		overflow: hidden;
		text-overflow: ellipsis;
	}
	.next-sub {
		font-size: 12px;
		color: var(--k-text-muted);
		white-space: nowrap;
		overflow: hidden;
		text-overflow: ellipsis;
	}
	.chip {
		flex-shrink: 0;
		font-size: 14px;
		font-weight: 600;
		padding: 6px 10px;
		border-radius: 8px;
		background: var(--k-surface);
		color: color-mix(in srgb, var(--k-accent) 70%, var(--k-text));
		white-space: nowrap;
	}
	.chip.now {
		color: var(--k-text);
	}
	.chip.over {
		color: var(--k-danger);
	}

	/* Following rows: bar · mono time (92 px column) · title · feed. Subgrid keeps the columns aligned. */
	.rows-wrap {
		flex: 1;
		min-height: 0;
		overflow: hidden;
	}
	.rows {
		list-style: none;
		margin: 0;
		padding: 0;
		display: flex;
		flex-direction: column;
		gap: 4px;
	}
	.row {
		display: flex;
		align-items: center;
		gap: 14px;
		padding: 8px 14px;
		line-height: 16px;
		min-width: 0;
	}
	/* 92 px column (DESIGN §5.4); mono, so the longest label in `ch` aligns every row (12 h locales run longer). */
	.when {
		flex-shrink: 0;
		width: max(92px, calc(var(--when-ch, 0) * 1ch));
		font-size: 12px;
		color: var(--k-text-muted);
		white-space: nowrap;
	}
	.what {
		flex: 1;
		font-size: 13px;
		white-space: nowrap;
		overflow: hidden;
		text-overflow: ellipsis;
		min-width: 0;
	}
	.feed {
		font-size: 11px;
		color: var(--k-text-muted);
		white-space: nowrap;
		max-width: 120px;
		overflow: hidden;
		text-overflow: ellipsis;
	}
	.day {
		height: 18px;
		line-height: 18px;
		padding: 0 14px;
		font-size: 11px;
		color: var(--k-text-muted);
		text-transform: uppercase;
		letter-spacing: 0.08em;
	}

	/* Per-feed errors: muted footer line + Retry (36 px secondary). */
	.foot {
		display: flex;
		align-items: center;
		justify-content: space-between;
		gap: 10px;
		min-width: 0;
		flex-shrink: 0;
	}
	.foot-text {
		font-size: 11px;
		color: var(--k-text-muted);
		white-space: nowrap;
		overflow: hidden;
		text-overflow: ellipsis;
	}
	.retry {
		flex-shrink: 0;
		height: 36px;
		padding: 0 12px;
		border-radius: 10px;
		border: 1px solid var(--k-border);
		background: transparent;
		color: var(--k-text);
		font-size: 12px;
		font-weight: 600;
	}
	.retry:hover:not(:disabled) {
		background: color-mix(in srgb, var(--k-text) 6%, transparent);
	}

	/* States */
	.state {
		flex: 1;
		display: flex;
		flex-direction: column;
		align-items: center;
		justify-content: center;
		gap: 6px;
		text-align: center;
		color: var(--k-text-muted);
		min-height: 0;
	}
	.state-title {
		font-size: 13px;
		font-weight: 600;
		color: var(--k-text);
	}
	.state-text {
		font-size: 12px;
	}
	.state .k-btn {
		margin-top: 4px;
	}
	.skeleton {
		flex: 1;
		display: flex;
		flex-direction: column;
		gap: 10px;
	}
	.bar {
		display: block;
		height: 22px;
		border-radius: 8px;
		background: var(--k-bg-alt);
	}
	.bar.tall {
		height: 60px;
		border-radius: 12px;
	}

	/* 1024×600: tighter next card, drop the empty-state icon first. */
	@container (height < 230px) {
		.next {
			padding: 9px 12px;
		}
		.state svg {
			display: none;
		}
	}
</style>
