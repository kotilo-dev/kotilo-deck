<script lang="ts">
	import { t } from '#lib/i18n/index.ts';
	import type { PomodoroState, PomodoroToday } from '#lib/ipc/index.ts';
	import { isDeckVisible, onDeckVisibility, subscribeTick } from '#lib/time/index.ts';
	import type { WidgetProps } from '../types';
	import {
		circumference,
		formatCountdown,
		goalFraction,
		isBreak,
		phaseKey,
		phaseTone,
		primaryAction,
		remainingAt,
		ringOffset,
		suggestedMinutes,
		suggestionApplies,
		SUGGEST_MS
	} from './logic';
	import { defaults, type PomodoroWidgetSettings } from './settings';
	import { pomodoroApi, type Suggestion } from './store';

	let { size, settings, editing }: WidgetProps<PomodoroWidgetSettings> = $props();

	const s = $derived({ ...defaults, ...settings });
	const api = pomodoroApi();

	let timer = $state<PomodoroState | null>(null);
	let today = $state<PomodoroToday | null>(null);
	let failed = $state(false);
	let actionError = $state(false);
	let errorTimer: ReturnType<typeof setTimeout> | null = null;
	let now = $state(Date.now());
	let visible = $state(isDeckVisible());

	let loadSeq = 0;
	async function load() {
		const seq = ++loadSeq;
		try {
			const next = await api.state();
			if (seq !== loadSeq) return;
			apply(next);
			failed = false;
		} catch {
			if (seq === loadSeq && !timer) failed = true;
		}
	}

	async function loadToday() {
		if (size !== '4x2') return;
		try {
			today = await api.today();
		} catch {
			// Stats are secondary: keep the last value.
		}
	}

	function apply(next: PomodoroState) {
		timer = next;
		now = Date.now();
	}

	$effect(() => {
		load();
		const offState = api.onState((st) => {
			loadSeq++;
			apply(st);
			failed = false;
		});
		const offEnd = api.onPhaseEnd(() => loadToday());
		const offSuggest = api.onSuggest(onSuggest);
		return () => {
			offState();
			offEnd();
			offSuggest();
			loadSeq++;
			if (errorTimer) clearTimeout(errorTimer);
			if (suggestTimer) clearTimeout(suggestTimer);
		};
	});

	// 4×2 stats: on mount / size change; also re-read when the deck is shown again (day may have rolled over).
	$effect(() => {
		if (size === '4x2') loadToday();
	});

	$effect(() =>
		onDeckVisibility((v) => {
			visible = v;
			if (v) {
				load();
				loadToday();
			}
		})
	);

	// Countdown from `endsAt` on the shared 1 Hz tick, only while running and on screen (tick pauses when hidden).
	$effect(() => {
		if (!timer?.running || !visible) return;
		now = Date.now();
		return subscribeTick((d) => (now = d.getTime()), { seconds: true });
	});

	// ── Companion suggestion banner (60 s, dismissible; never starts a timer by itself) ──
	let suggestion = $state<Suggestion | null>(null);
	let suggestTimer: ReturnType<typeof setTimeout> | null = null;

	function onSuggest(next: Suggestion) {
		suggestion = next;
		if (suggestTimer) clearTimeout(suggestTimer);
		suggestTimer = setTimeout(() => {
			suggestTimer = null;
			suggestion = null;
		}, SUGGEST_MS);
	}

	function dismissSuggestion() {
		if (suggestTimer) clearTimeout(suggestTimer);
		suggestTimer = null;
		suggestion = null;
	}

	const showBanner = $derived(!!suggestion && s.suggestions && suggestionApplies(timer));
	const suggestMin = $derived(suggestedMinutes(suggestion?.minutes));
	const bannerText = $derived(suggestion?.reason?.trim() ? `${suggestion.reason.trim()} — ${t('widgets.pomodoro.suggestQuestion')}` : t('widgets.pomodoro.suggestFallback'));

	async function acceptSuggestion() {
		if (editing) return;
		const minutes = suggestMin;
		dismissSuggestion();
		await act(() => api.start('focus', minutes));
	}

	// ── Actions ──────────────────────────────────────────────────────────────
	function flagError() {
		actionError = true;
		if (errorTimer) clearTimeout(errorTimer);
		errorTimer = setTimeout(() => (actionError = false), 4000);
		load();
	}

	async function act(fn: () => Promise<PomodoroState>) {
		if (editing) return;
		try {
			apply(await fn());
		} catch {
			flagError();
		}
	}

	const primary = $derived(timer ? primaryAction(timer) : 'start');
	function onPrimary() {
		if (primary === 'pause') act(() => api.pause());
		else if (primary === 'resume') act(() => api.resume());
		else act(() => api.start());
	}

	// ── View ─────────────────────────────────────────────────────────────────
	const phase = $derived(timer?.phase ?? 'idle');
	const remaining = $derived(timer ? remainingAt(timer, now) : 0);
	const duration = $derived(timer?.duration ?? 0);
	const tone = $derived(phaseTone(phase));
	const title = $derived(t(`widgets.pomodoro.${phaseKey(phase)}`));
	const sessionText = $derived(timer ? `${timer.session} / ${timer.sessionsPerCycle}` : '');
	const timeText = $derived(formatCountdown(remaining));
	const subText = $derived(
		phase === 'idle'
			? t('widgets.pomodoro.ready')
			: timer && !timer.running
				? t('widgets.pomodoro.paused')
				: t('widgets.pomodoro.of', { time: formatCountdown(duration) })
	);
	const timerLabel = $derived(t('widgets.pomodoro.timerLabel', { phase: title, time: timeText }));
	const primaryLabel = $derived(t(`widgets.pomodoro.${primary}`));
	const skipLabel = $derived(t(isBreak(phase) ? 'widgets.pomodoro.skipToFocus' : 'widgets.pomodoro.skipToBreak'));
	/** Ring re-mounts per phase so the 1 s transition never sweeps backwards across a phase change. */
	const ringKey = $derived(`${phase}:${duration}:${timer?.session ?? 0}`);

	const R = 60;
	const C = circumference(R);
	const MINI_R = 20;
	const MINI_C = circumference(MINI_R);
	const DAY_R = 40;
	const DAY_C = circumference(DAY_R);
</script>

{#snippet playIcon()}
	<svg width="16" height="16" viewBox="0 0 24 24" fill="currentColor" aria-hidden="true"><path d="M7 4.5v15a1 1 0 0 0 1.5.86l12-7.5a1 1 0 0 0 0-1.72l-12-7.5A1 1 0 0 0 7 4.5Z" /></svg>
{/snippet}
{#snippet pauseIcon()}
	<svg width="16" height="16" viewBox="0 0 24 24" fill="currentColor" aria-hidden="true"><rect x="6" y="4" width="4" height="16" rx="1" /><rect x="14" y="4" width="4" height="16" rx="1" /></svg>
{/snippet}
{#snippet skipIcon()}
	<svg width="16" height="16" viewBox="0 0 24 24" fill="currentColor" aria-hidden="true"><path d="M5 4l10 8-10 8z" /><rect x="17" y="4" width="3" height="16" rx="1" /></svg>
{/snippet}
{#snippet resetIcon()}
	<svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M3 12a9 9 0 1 0 2.6-6.4L3 8" /><path d="M3 3v5h5" /></svg>
{/snippet}
{#snippet closeIcon()}
	<svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M18 6 6 18M6 6l12 12" /></svg>
{/snippet}

{#snippet primaryButton()}
	<button type="button" class="btn primary" aria-label={primaryLabel} title={primaryLabel} disabled={editing} onclick={onPrimary}>
		{#if primary === 'pause'}{@render pauseIcon()}{:else}{@render playIcon()}{/if}
	</button>
{/snippet}

{#snippet controls(withReset: boolean)}
	<div class="controls">
		{@render primaryButton()}
		<button type="button" class="btn ghost" aria-label={skipLabel} title={skipLabel} disabled={editing || phase === 'idle'} onclick={() => act(() => api.skip())}>
			{@render skipIcon()}
		</button>
		{#if withReset}
			<button
				type="button"
				class="btn ghost"
				aria-label={t('widgets.pomodoro.reset')}
				title={t('widgets.pomodoro.reset')}
				disabled={editing || phase === 'idle'}
				onclick={() => act(() => api.reset())}
			>
				{@render resetIcon()}
			</button>
		{/if}
	</div>
{/snippet}

{#snippet banner(compact: boolean)}
	<div class="banner" class:compact role="status" data-no-gesture>
		<span class="banner-text" title={bannerText}>{bannerText}</span>
		<button
			type="button"
			class="btn ghost small"
			aria-label={suggestMin ? t('widgets.pomodoro.suggestStartMinutes', { n: suggestMin }) : t('widgets.pomodoro.suggestStartLabel')}
			disabled={editing}
			onclick={acceptSuggestion}
		>
			{t('widgets.pomodoro.start')}
		</button>
		<button type="button" class="btn ghost small icon" aria-label={t('widgets.pomodoro.suggestDismiss')} title={t('widgets.pomodoro.suggestDismiss')} onclick={dismissSuggestion}>
			{@render closeIcon()}
		</button>
	</div>
{/snippet}

{#snippet bigRing()}
	<div class="ring" role="timer" aria-label={timerLabel}>
		<svg viewBox="0 0 136 136" aria-hidden="true">
			<circle class="track" cx="68" cy="68" r={R} stroke-width="10" />
			{#key ringKey}
				<circle
					class="progress"
					class:running={timer?.running}
					cx="68"
					cy="68"
					r={R}
					stroke-width="10"
					stroke-dasharray={C}
					stroke-dashoffset={ringOffset(C, remaining, duration)}
					style:stroke="var(--k-{tone})"
				/>
			{/key}
		</svg>
		<div class="center">
			<span class="mono time">{timeText}</span>
			<span class="sub">{subText}</span>
		</div>
	</div>
{/snippet}

<section
	class="k-card pomo"
	class:has-banner={showBanner}
	data-size={size}
	aria-label={t('widgets.pomodoro.title')}
>
	{#if failed}
		<div class="state">
			<span class="state-title">{t('widgets.pomodoro.error')}</span>
			<button type="button" class="k-btn" disabled={editing} onclick={load}>{t('widgets.common.retry')}</button>
		</div>
	{:else if !timer}
		<div class="skeleton" aria-busy="true" aria-label={t('widgets.common.loading')}>
			<span class="bar" style="width: 40%"></span>
			<span class="bar grow"></span>
			<span class="bar" style="width: 50%"></span>
		</div>
	{:else if size === '2x1'}
		{#if showBanner}
			{@render banner(true)}
		{:else}
			<div class="compact-row">
				<div class="mini" role="timer" aria-label={timerLabel}>
					<svg viewBox="0 0 48 48" aria-hidden="true">
						<circle class="track" cx="24" cy="24" r={MINI_R} stroke-width="5" />
						{#key ringKey}
							<circle
								class="progress"
								class:running={timer.running}
								cx="24"
								cy="24"
								r={MINI_R}
								stroke-width="5"
								stroke-dasharray={MINI_C}
								stroke-dashoffset={ringOffset(MINI_C, remaining, duration)}
								style:stroke="var(--k-{tone})"
							/>
						{/key}
					</svg>
				</div>
				<div class="mid">
					<span class="mono time-l">{timeText}</span>
					<span class="phase-line" class:warn-text={actionError}>
						{actionError ? t('widgets.pomodoro.actionFailed') : `${title} · ${sessionText}`}
					</span>
				</div>
				{@render primaryButton()}
			</div>
		{/if}
	{:else}
		<header class="hdr">
			<span class="title">{title}</span>
			<span class="meta mono" class:warn-text={actionError}>
				{#if actionError}<span class="ui">{t('widgets.pomodoro.actionFailed')}</span>{:else}{sessionText}{/if}
			</span>
		</header>
		{#if showBanner}{@render banner(false)}{/if}
		{#if size === '4x2'}
			<div class="wide">
				<div class="main">
					{@render bigRing()}
					{@render controls(true)}
				</div>
				<div class="stats" aria-label={t('widgets.pomodoro.today')}>
					{#if today}
						{@const frac = goalFraction(today)}
						<div class="day-ring" role="img" aria-label={t('widgets.pomodoro.sessionsLabel', { n: today.sessions, goal: today.goal })}>
							<svg viewBox="0 0 96 96" aria-hidden="true">
								<circle class="track" cx="48" cy="48" r={DAY_R} stroke-width="8" />
								<circle
									class="progress"
									cx="48"
									cy="48"
									r={DAY_R}
									stroke-width="8"
									stroke-dasharray={DAY_C}
									stroke-dashoffset={DAY_C * (1 - frac)}
									style:stroke="var(--k-success)"
								/>
							</svg>
							<span class="center"><span class="mono day-num">{today.sessions}<span class="of">/{today.goal}</span></span></span>
						</div>
						<span class="tiny">{t('widgets.pomodoro.sessionsToday')}</span>
						<div class="focus-min">
							<span class="mono focus-num">{today.focusMinutes}<span class="unit">{t('widgets.pomodoro.min')}</span></span>
							<span class="tiny">{t('widgets.pomodoro.focusToday')}</span>
						</div>
					{:else}
						<span class="bar" style="width: 96px; height: 96px; border-radius: 50%"></span>
						<span class="bar" style="width: 60%"></span>
					{/if}
				</div>
			</div>
		{:else}
			{@render bigRing()}
			{@render controls(false)}
		{/if}
	{/if}
</section>

<style>
	.pomo {
		--warn-text: color-mix(in srgb, var(--k-warning) 35%, var(--k-text));
		container-type: size;
		padding: 18px 20px;
		gap: 10px;
		align-items: stretch;
		font-family: var(--k-font-ui);
	}
	.pomo[data-size='2x1'] {
		padding: 14px 18px;
		justify-content: center;
	}
	.mono {
		font-family: var(--k-font-mono);
		font-variant-numeric: tabular-nums;
	}
	.ui {
		font-family: var(--k-font-ui);
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
		white-space: nowrap;
	}
	.meta {
		font-size: 11px;
		color: var(--k-text-muted);
		white-space: nowrap;
		overflow: hidden;
		text-overflow: ellipsis;
		min-width: 0;
	}
	.warn-text {
		color: var(--warn-text);
	}

	/* Ring (DESIGN §5.8): 136 px, 10 px track/progress, starts at 12 o'clock, 1 s linear. */
	.ring {
		/* Header 16 + 2 gaps 10 + controls 44 = 80; the banner takes 36 + 10 more. 136 px up to 1280×720,
		   larger only on tall cards (1920×1080) so the ring doesn't sit tiny in a huge card. */
		--ring: clamp(84px, calc(100cqh - 80px - var(--banner-h, 0px)), max(136px, calc(100cqh - 200px)));
		position: relative;
		width: var(--ring);
		height: var(--ring);
		align-self: center;
		flex-shrink: 0;
	}
	.has-banner {
		--banner-h: 46px;
	}
	.ring svg,
	.mini svg,
	.day-ring svg {
		position: absolute;
		inset: 0;
		width: 100%;
		height: 100%;
		transform: rotate(-90deg);
	}
	.track {
		fill: none;
		stroke: var(--k-border);
	}
	.progress {
		fill: none;
		stroke-linecap: round;
	}
	.progress.running {
		transition: stroke-dashoffset 1s linear;
	}
	.center {
		position: absolute;
		inset: 0;
		display: flex;
		flex-direction: column;
		align-items: center;
		justify-content: center;
		gap: 2px;
	}
	.time {
		font-size: clamp(20px, calc(var(--ring) * 0.22), 48px);
		font-weight: 600;
		letter-spacing: -0.02em;
		line-height: 1.1;
	}
	.sub {
		font-size: 11px;
		color: var(--k-text-muted);
		white-space: nowrap;
	}

	.controls {
		display: flex;
		justify-content: center;
		gap: 10px;
		flex-shrink: 0;
	}
	.btn {
		box-sizing: border-box;
		display: inline-flex;
		align-items: center;
		justify-content: center;
		width: 44px;
		height: 44px;
		flex-shrink: 0;
		padding: 0;
		border-radius: 12px;
		border: 1px solid var(--k-border);
		background: transparent;
		color: var(--k-text);
		font-size: 12px;
		font-weight: 600;
	}
	.btn.primary {
		border-color: var(--k-accent);
		background: var(--k-accent);
		color: var(--k-on-accent);
	}
	.btn.primary:hover:not(:disabled) {
		background: color-mix(in srgb, var(--k-accent) 88%, var(--k-text));
	}
	.btn.ghost:hover:not(:disabled) {
		background: color-mix(in srgb, var(--k-text) 6%, transparent);
	}
	/* Banner buttons: 36 px secondary in-card controls (DESIGN §5.8 / §9). */
	.btn.small {
		width: auto;
		height: 36px;
		padding: 0 12px;
		border-radius: 10px;
	}
	.btn.small.icon {
		width: 36px;
		padding: 0;
		border-color: transparent;
		color: var(--k-text-muted);
	}

	/* Suggestion banner: 36 px inline row. */
	.banner {
		display: flex;
		align-items: center;
		gap: 6px;
		height: 36px;
		flex-shrink: 0;
		min-width: 0;
		padding-left: 10px;
		border-radius: 10px;
		background: color-mix(in srgb, var(--k-accent) 12%, var(--k-surface));
	}
	.banner-text {
		flex: 1;
		min-width: 0;
		font-size: 12px;
		white-space: nowrap;
		overflow: hidden;
		text-overflow: ellipsis;
	}
	.banner.compact {
		height: auto;
		min-height: 44px;
		padding: 4px 4px 4px 10px;
	}
	.banner.compact .banner-text {
		white-space: normal;
		display: -webkit-box;
		-webkit-line-clamp: 2;
		line-clamp: 2;
		-webkit-box-orient: vertical;
	}

	/* 2×1: mini ring · time + phase · one button. */
	.compact-row {
		display: flex;
		align-items: center;
		gap: 12px;
		min-width: 0;
	}
	.mini {
		position: relative;
		width: 48px;
		height: 48px;
		flex-shrink: 0;
	}
	.mid {
		flex: 1;
		min-width: 0;
		display: flex;
		flex-direction: column;
		gap: 2px;
	}
	.time-l {
		font-size: 32px;
		font-weight: 500;
		line-height: 1;
		letter-spacing: -0.02em;
	}
	.phase-line {
		font-size: 11px;
		color: var(--k-text-muted);
		white-space: nowrap;
		overflow: hidden;
		text-overflow: ellipsis;
	}

	/* 4×2: ring + controls | daily stats. */
	.wide {
		flex: 1;
		min-height: 0;
		display: grid;
		grid-template-columns: minmax(0, 1fr) minmax(0, 1fr);
		gap: 20px;
	}
	.main {
		display: flex;
		flex-direction: column;
		justify-content: center;
		gap: 10px;
		min-height: 0;
	}
	.stats {
		display: flex;
		flex-direction: column;
		align-items: center;
		justify-content: center;
		gap: 6px;
		min-height: 0;
		border-left: 1px solid var(--k-border);
	}
	.day-ring {
		--day: clamp(64px, calc(100cqh - 120px), max(96px, calc(100cqh - 300px)));
		position: relative;
		width: var(--day);
		height: var(--day);
		flex-shrink: 0;
	}
	.day-num {
		font-size: 20px;
		font-weight: 600;
	}
	.of {
		font-size: 13px;
		color: var(--k-text-muted);
	}
	.tiny {
		font-size: 11px;
		color: var(--k-text-muted);
		text-transform: uppercase;
		letter-spacing: 0.08em;
		white-space: nowrap;
	}
	.focus-min {
		display: flex;
		flex-direction: column;
		align-items: center;
		gap: 2px;
		margin-top: 6px;
	}
	.focus-num {
		font-size: 20px;
		font-weight: 500;
	}
	.unit {
		margin-left: 4px;
		font-family: var(--k-font-ui);
		font-size: 12px;
		color: var(--k-text-muted);
	}

	/* States */
	.state {
		flex: 1;
		display: flex;
		flex-direction: column;
		align-items: center;
		justify-content: center;
		gap: 8px;
		text-align: center;
	}
	.state-title {
		font-size: 13px;
		font-weight: 600;
	}
	.skeleton {
		flex: 1;
		display: flex;
		flex-direction: column;
		align-items: center;
		gap: 10px;
		min-height: 0;
	}
	.bar {
		display: block;
		height: 14px;
		width: 100%;
		border-radius: 8px;
		background: var(--k-bg-alt);
	}
	.bar.grow {
		flex: 1;
		width: auto;
		aspect-ratio: 1;
		border-radius: 50%;
	}

	/* 1024×600: secondary text goes first, the numbers stay. */
	@container (width < 220px) {
		.compact-row {
			gap: 8px;
		}
		.mini {
			width: 40px;
			height: 40px;
		}
	}
	@container (height < 260px) {
		.has-banner .sub {
			display: none;
		}
	}
	@container (height < 230px) {
		.focus-min .tiny {
			display: none;
		}
		.focus-min {
			margin-top: 0;
		}
	}
</style>
