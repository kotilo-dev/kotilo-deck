<script lang="ts">
	import { locale, t } from '#lib/i18n/index.ts';
	import { hasBackend, weatherGet, type WeatherReport } from '#lib/ipc/index.ts';
	import { isDeckVisible, minutesSince, onDeckVisibility, subscribeTick } from '#lib/time/index.ts';
	import type { WidgetProps } from '../types';
	import {
		agoKey,
		asLocation,
		backoffDelay,
		chipDays,
		dayLabel,
		formatTemp,
		hourLabel,
		REFRESH_MS,
		wmoFamily,
		wmoText
	} from './logic';
	import { defaults, type WeatherSettings } from './settings';
	import WeatherIcon from './WeatherIcon.svelte';

	let { size, settings, editing }: WidgetProps<WeatherSettings> = $props();

	const s = $derived({ ...defaults, ...settings });
	const loc = $derived(asLocation(s.location));
	const unit = $derived<'c' | 'f'>(s.unit === 'f' ? 'f' : 'c');
	// Primitives, so unrelated settings edits (e.g. appearance) don't refetch.
	const lat = $derived(loc?.latitude ?? null);
	const lon = $derived(loc?.longitude ?? null);
	const tauri = hasBackend();

	let report = $state<WeatherReport | null>(null);
	let failed = $state(false);
	let loading = $state(false);

	let timer: ReturnType<typeof setTimeout> | null = null;
	let attempt = 0;
	/** Bumped whenever the location/unit changes or the widget unmounts; late responses are dropped. */
	let generation = 0;

	/** Next refresh (wall-clock ms) and what it runs; the timer is only armed while the deck is visible. */
	let dueAt = 0;
	let pending: (() => void) | null = null;
	let visible = isDeckVisible();

	function clearTimer() {
		if (timer !== null) clearTimeout(timer);
		timer = null;
	}

	function arm() {
		clearTimer();
		if (!pending || !visible) return;
		// Overdue after being hidden → refresh right away.
		timer = setTimeout(() => {
			const run = pending;
			pending = null;
			timer = null;
			run?.();
		}, Math.max(0, dueAt - Date.now()));
	}

	function scheduleNext(delay: number, run: () => void) {
		dueAt = Date.now() + delay;
		pending = run;
		arm();
	}

	// Hidden deck: no refresh timer (idle budget). On show, refresh if the 15 min (or backoff) are up.
	$effect(() =>
		onDeckVisibility((v) => {
			visible = v;
			if (v) arm();
			else clearTimer();
		})
	);

	async function load(lat: number, lon: number, u: 'c' | 'f', gen: number) {
		clearTimer();
		pending = null;
		loading = true;
		let next = REFRESH_MS;
		try {
			const r = await weatherGet(lat, lon, u);
			if (gen !== generation) return;
			report = r;
			failed = false;
			// Backend served its cache because upstream failed: keep retrying with backoff.
			attempt = r.stale ? attempt + 1 : 0;
			if (r.stale) next = backoffDelay(attempt);
		} catch {
			if (gen !== generation) return;
			failed = true;
			attempt += 1;
			next = backoffDelay(attempt);
		} finally {
			if (gen === generation) loading = false;
		}
		if (gen === generation) scheduleNext(next, () => load(lat, lon, u, gen));
	}

	$effect(() => {
		const la = lat;
		const lo = lon;
		const u = unit;
		const gen = ++generation;
		report = null;
		failed = false;
		attempt = 0;
		if (tauri && la !== null && lo !== null) load(la, lo, u, gen);
		return () => {
			generation++;
			pending = null;
			clearTimer();
		};
	});

	function retry() {
		if (lat === null || lon === null || editing) return;
		attempt = 0;
		load(lat, lon, unit, generation);
	}

	let now = $state(Date.now());
	$effect(() => subscribeTick((d) => (now = d.getTime())));

	const ago = $derived.by(() => {
		if (!report) return '';
		const a = agoKey(minutesSince(report.fetchedAt, now));
		return t(a.key, a.params);
	});
	const stale = $derived(!!report && (report.stale || failed));
	const cond = $derived(report ? t(`widgets.weather.cond.${wmoText(report.current.code)}`) : '');
	const family = $derived(report ? wmoFamily(report.current.code) : 'cloud');
	const hiLo = $derived(
		report
			? `${t('widgets.weather.high', { t: formatTemp(report.current.high) })} ${t('widgets.weather.low', {
					t: formatTemp(report.current.low)
				})}`
			: ''
	);
	const title = $derived(loc?.name ?? t('widgets.weather.title'));
	/** Large 2×1 (DESIGN §5.3): "H 19° · L 12°". */
	const hiLoDot = $derived(
		report
			? `${t('widgets.weather.high', { t: formatTemp(report.current.high) })} · ${t('widgets.weather.low', {
					t: formatTemp(report.current.low)
				})}`
			: ''
	);
</script>

{#snippet staleBadge()}
	<span class="badge">{t('widgets.weather.stale')}</span>
{/snippet}

{#snippet header()}
	<header class="hdr">
		<span class="title">
			<span class="name">{title}</span>
			{#if stale}{@render staleBadge()}{/if}
		</span>
		{#if report}
			<span class="meta">{stale ? t('widgets.weather.lastUpdate', { ago }) : ago}</span>
		{/if}
	</header>
{/snippet}

{#snippet chips(r: WeatherReport)}
	<ul class="chips" aria-label={t('widgets.weather.daily')}>
		{#each chipDays(r.daily) as d (d.date)}
			<li class="chip">
				<span class="chip-day">{dayLabel(d.date, locale)}</span>
				<span class="mono chip-temp">{formatTemp(d.max)} <span class="muted">{formatTemp(d.min)}</span></span>
			</li>
		{/each}
	</ul>
{/snippet}

{#snippet hero(r: WeatherReport, iconSize: number)}
	<div class="hero">
		<div class="hero-text">
			<span class="mono temp-xl">{formatTemp(r.current.temp)}</span>
			<span class="cond">{cond} · {hiLo}</span>
		</div>
		<WeatherIcon {family} isDay={r.current.isDay} size={iconSize} />
	</div>
{/snippet}

{#snippet stateIcon(kind: 'pin' | 'off' | 'alert')}
	<svg width="22" height="22" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
		{#if kind === 'pin'}
			<path d="M20 10c0 5-8 12-8 12s-8-7-8-12a8 8 0 0 1 16 0Z" />
			<circle cx="12" cy="10" r="3" />
		{:else if kind === 'off'}
			<path d="M2 2l20 20M5.8 5.8A7 7 0 0 0 9 19h8.5c.5 0 1-.06 1.4-.17M21.7 16.3A4.5 4.5 0 0 0 17.5 10h-1.8A7 7 0 0 0 10 5.2" />
		{:else}
			<circle cx="12" cy="12" r="10" />
			<path d="M12 8v4M12 16h.01" />
		{/if}
	</svg>
{/snippet}

<section class="k-card weather" data-size={size} aria-label={t('widgets.weather.title')}>
	{#if !tauri}
		<div class="state">
			{@render stateIcon('off')}
			<span class="state-text">{t('widgets.weather.browserPreview')}</span>
		</div>
	{:else if !loc}
		<div class="state">
			{@render stateIcon('pin')}
			<span class="state-body">
				<span class="state-title">{t('widgets.weather.emptyTitle')}</span>
				<span class="state-text">{t('widgets.weather.emptyHint')}</span>
			</span>
		</div>
	{:else if report}
		{#if size === '2x1'}
			<!-- Large 2×1: header, scaled temperature + condition + 84 px filled icon, one-line 3-day chips. -->
			<div class="w21">
				{@render header()}
				<div class="w21-mid">
					<span class="mono w21-temp">{formatTemp(report.current.temp)}</span>
					<span class="w21-text">
						<span class="w21-cond">{cond}</span>
						<span class="mono w21-hl">{hiLoDot}</span>
					</span>
					<span class="w21-icon"><WeatherIcon {family} isDay={report.current.isDay} size={84} /></span>
				</div>
				<ul class="w21-chips" aria-label={t('widgets.weather.daily')}>
					{#each chipDays(report.daily) as d (d.date)}
						<li>
							<span class="w21-day">{dayLabel(d.date, locale)}</span>
							<span class="mono">{formatTemp(d.max)} <span class="muted">{formatTemp(d.min)}</span></span>
						</li>
					{/each}
				</ul>
			</div>
			<div class="compact">
				<div class="compact-text">
					<span class="title">
						<span class="name">{title}</span>
						{#if stale}{@render staleBadge()}{/if}
					</span>
					<span class="sub">{stale ? `${cond} · ${t('widgets.weather.lastUpdate', { ago })}` : `${cond} · ${ago}`}</span>
				</div>
				<div class="compact-hero">
					<span class="mono temp-l">{formatTemp(report.current.temp)}</span>
					<span class="compact-icon"><WeatherIcon {family} isDay={report.current.isDay} size={36} stroke={1.8} /></span>
				</div>
			</div>
		{:else if size === '4x2'}
			{@render header()}
			<div class="wide">
				{@render hero(report, 56)}
				<ol class="hours" aria-label={t('widgets.weather.hourly')}>
					{#each report.hourly.slice(0, 6) as h (h.time)}
						<li class="hour">
							<span class="mono hour-label">{hourLabel(h.time).slice(0, 2)}<span class="mins">{hourLabel(h.time).slice(2)}</span></span>
							<WeatherIcon family={wmoFamily(h.code)} size={20} stroke={1.8} />
							<span class="mono hour-temp">{formatTemp(h.temp)}</span>
						</li>
					{/each}
				</ol>
			</div>
			{@render chips(report)}
		{:else}
			{@render header()}
			{@render hero(report, 56)}
			{@render chips(report)}
		{/if}
	{:else if failed && !loading}
		<div class="state">
			{@render stateIcon('alert')}
			<span class="state-body"><span class="state-title">{t('widgets.weather.error')}</span></span>
			<button type="button" class="btn" onclick={retry} disabled={editing}>{t('widgets.common.retry')}</button>
		</div>
	{:else}
		<div class="skeleton" aria-busy="true" aria-label={t('widgets.common.loading')}>
			<span class="bar" style="width: 45%; height: 13px"></span>
			{#if size === '2x1'}
				<span class="bar" style="width: 70%; height: 32px"></span>
			{:else}
				<span class="bar" style="width: 40%; height: 48px"></span>
				<span class="bar" style="width: 100%; height: 44px"></span>
			{/if}
		</div>
	{/if}
</section>

<style>
	.weather {
		container-type: size;
		padding: 18px 20px;
		justify-content: space-between;
		gap: 10px;
		font-family: var(--k-font-ui);
	}
	.weather[data-size='2x1'] {
		padding: 14px 18px;
		justify-content: center;
	}
	.weather[data-size='4x2'] {
		padding: 18px 22px;
	}
	.mono {
		font-family: var(--k-font-mono);
		font-variant-numeric: tabular-nums;
	}
	.muted {
		color: var(--k-text-muted);
	}

	.hdr {
		display: flex;
		justify-content: space-between;
		align-items: center;
		gap: 8px;
		min-width: 0;
	}
	.title {
		display: flex;
		align-items: center;
		gap: 6px;
		min-width: 0;
		font-size: 13px;
		font-weight: 600;
	}
	.name {
		white-space: nowrap;
		overflow: hidden;
		text-overflow: ellipsis;
	}
	.meta {
		font-size: 11px;
		color: var(--k-text-muted);
		white-space: nowrap;
	}
	.badge {
		flex-shrink: 0;
		font-size: 11px;
		font-weight: 500;
		padding: 2px 6px;
		border-radius: 6px;
		background: color-mix(in srgb, var(--k-warning) 18%, var(--k-surface));
		/* Darkened toward text so it stays legible on the light tint (Latte). */
		color: color-mix(in srgb, var(--k-warning) 35%, var(--k-text));
	}

	.hero {
		display: flex;
		align-items: center;
		justify-content: space-between;
		gap: 8px;
		min-width: 0;
	}
	.hero-text {
		display: flex;
		flex-direction: column;
		gap: 2px;
		min-width: 0;
	}
	.temp-xl {
		font-size: 48px;
		font-weight: 500;
		line-height: 1;
		letter-spacing: -0.03em;
	}
	.cond {
		font-size: 13px;
		color: var(--k-text-muted);
		display: -webkit-box;
		-webkit-line-clamp: 2;
		line-clamp: 2;
		-webkit-box-orient: vertical;
		overflow: hidden;
	}

	.chips {
		display: grid;
		grid-template-columns: repeat(3, minmax(0, 1fr));
		gap: 8px;
		margin: 0;
		padding: 0;
		list-style: none;
	}
	.chip {
		display: flex;
		flex-direction: column;
		align-items: center;
		gap: 3px;
		padding: 8px 0;
		border-radius: 10px;
		background: var(--k-inset);
		min-width: 0;
	}
	.chip-day {
		font-size: 11px;
		color: var(--k-text-muted);
	}
	.chip-temp {
		font-size: 13px;
		white-space: nowrap;
	}

	/* 4×2 */
	.wide {
		display: grid;
		grid-template-columns: minmax(0, 1fr) minmax(0, 1.25fr);
		align-items: center;
		gap: 20px;
	}
	.hours {
		display: grid;
		grid-template-columns: repeat(6, minmax(0, 1fr));
		gap: 4px;
		margin: 0;
		padding: 0;
		list-style: none;
	}
	.hour {
		display: flex;
		flex-direction: column;
		align-items: center;
		gap: 4px;
	}
	.hour-label {
		font-size: 11px;
		color: var(--k-text-muted);
	}
	.hour-temp {
		font-size: 13px;
	}

	/* 2×1 */
	.compact {
		display: flex;
		align-items: center;
		justify-content: space-between;
		gap: 12px;
		min-width: 0;
	}
	.compact-text {
		display: flex;
		flex-direction: column;
		gap: 2px;
		min-width: 0;
	}
	.sub {
		font-size: 12px;
		color: var(--k-text-muted);
		white-space: nowrap;
		overflow: hidden;
		text-overflow: ellipsis;
	}
	.compact-hero {
		display: flex;
		align-items: center;
		gap: 10px;
		flex-shrink: 0;
	}
	.compact-icon {
		display: flex;
	}
	.temp-l {
		font-size: 32px;
		font-weight: 500;
		line-height: 1;
		letter-spacing: -0.03em;
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
	.state-body {
		display: flex;
		flex-direction: column;
		align-items: center;
		gap: 2px;
		min-width: 0;
	}
	.weather[data-size='2x1'] .state {
		flex-direction: row;
		gap: 12px;
		text-align: left;
	}
	.weather[data-size='2x1'] .state-body {
		align-items: flex-start;
	}
	.state-title {
		font-size: 13px;
		font-weight: 600;
		color: var(--k-text);
	}
	.state-text {
		font-size: 12px;
	}
	.btn {
		height: 44px;
		padding: 0 16px;
		border-radius: 12px;
		border: 1px solid var(--k-border);
		background: transparent;
		color: var(--k-text);
		font: 600 13px var(--k-font-ui);
		cursor: pointer;
	}
	.btn:disabled {
		opacity: 0.55;
		cursor: default;
	}
	.skeleton {
		flex: 1;
		display: flex;
		flex-direction: column;
		justify-content: space-between;
		gap: 10px;
	}
	.bar {
		display: block;
		border-radius: 8px;
		background: var(--k-inset);
	}

	/* 1024×600: secondary rows go first, the hero number stays. */
	@container (height < 200px) {
		.weather[data-size='2x2'] .cond,
		.weather[data-size='4x2'] .cond {
			-webkit-line-clamp: 1;
			line-clamp: 1;
		}
	}
	@container (width < 230px) {
		.weather[data-size='2x2'] .meta {
			display: none;
		}
	}
	@container (width < 520px) {
		.mins {
			display: none;
		}
	}
	@container (width < 210px) {
		.compact-icon {
			display: none;
		}
	}

	/* Large 2×1 ----------------------------------------------------------- */
	.w21 {
		display: none;
		flex-direction: column;
		justify-content: space-between;
		gap: 8px;
		height: 100%;
		min-width: 0;
		/* Temperature and icon share one size: 32 px floor, 84 px at 1920×1080 (DESIGN §3). */
		--hero: max(32px, min(84px, 20cqw, calc(100cqh - 40px)));
	}
	.w21-mid {
		display: flex;
		align-items: center;
		gap: 14px;
		min-width: 0;
		flex: 1;
		min-height: 0;
	}
	.w21-temp {
		font-size: var(--hero);
		font-weight: 500;
		letter-spacing: -0.04em;
		line-height: 1;
		white-space: nowrap;
	}
	.w21-text {
		flex: 1;
		min-width: 0;
		display: flex;
		flex-direction: column;
		gap: 4px;
	}
	.w21-cond {
		font-size: 16px;
		font-weight: 500;
	}
	.w21-hl {
		font-size: 13px;
		color: var(--k-text-muted);
	}
	.w21-cond,
	.w21-hl {
		white-space: nowrap;
		overflow: hidden;
		text-overflow: ellipsis;
	}
	.w21-icon {
		display: flex;
		flex-shrink: 0;
	}
	.w21-icon :global(svg) {
		width: var(--hero);
		height: var(--hero);
	}
	/* Compact 3-day chips: "Fri  16° 11°" on one line. */
	.w21-chips {
		display: none;
		grid-template-columns: repeat(3, minmax(0, 1fr));
		gap: 8px;
		margin: 0;
		padding: 0;
		list-style: none;
		flex-shrink: 0;
	}
	.w21-chips li {
		display: flex;
		align-items: baseline;
		justify-content: center;
		gap: 8px;
		padding: 6px 8px;
		border-radius: 10px;
		background: var(--k-inset);
		font-size: 13px;
		white-space: nowrap;
		overflow: hidden;
		min-width: 0;
	}
	.w21-day {
		color: var(--k-text-muted);
	}
	@container (height >= 100px) and (width >= 220px) {
		.w21 {
			display: flex;
		}
		.compact {
			display: none;
		}
	}
	/* 1280×720: the condition text wins over the icon. */
	@container (width < 300px) {
		.w21-icon {
			display: none;
		}
	}
	/* Room for the chips (1920×1080, portrait); the hero gives up their height. */
	@container (height >= 150px) and (width >= 300px) {
		.w21 {
			--hero: max(32px, min(84px, 20cqw, calc(100cqh - 116px)));
		}
		.w21-chips {
			display: grid;
		}
	}
	@container (width < 400px) {
		.wide {
			grid-template-columns: minmax(0, 1fr);
		}
		.hours {
			display: none;
		}
	}
</style>
