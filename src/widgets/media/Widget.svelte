<script lang="ts">
	import { t } from '#lib/i18n/index.ts';
	import {
		hasBackend,
		mediaControl,
		mediaSeek,
		mediaState,
		mediaSubscribe,
		mediaUnsubscribe,
		on,
		type MediaAction,
		type MediaSession
	} from '#lib/ipc/index.ts';
	import { getTheme } from '#lib/theme/index.ts';
	import { isDeckVisible, onDeckVisibility, subscribeTick } from '#lib/time/index.ts';
	import type { WidgetProps } from '../types';
	import {
		applyOverrides,
		averageColor,
		byline,
		currentPosition,
		formatTime,
		HOLD_MS,
		nextRepeat,
		progress,
		repeatKey,
		rgbCss,
		seekSeconds,
		tintColor,
		type Overrides,
		type Rgb
	} from './logic';
	import { defaults, type MediaSettings } from './settings';

	let { size, settings, editing, available }: WidgetProps<MediaSettings> = $props();

	const s = $derived({ ...defaults, ...settings });
	const tauri = hasBackend();
	/** Seek slider resolution. */
	const SCRUB_MAX = 1000;

	let session = $state<MediaSession | null>(null);
	let loaded = $state(false);
	let failed = $state(false);
	let retryKey = $state(0);
	let overrides = $state<Overrides>({});
	let now = $state(Date.now());
	let settleTimer: ReturnType<typeof setTimeout> | null = null;

	function receive(next: MediaSession | null) {
		session = next;
		loaded = true;
		failed = false;
		now = Date.now();
	}

	let visible = $state(isDeckVisible());
	$effect(() => onDeckVisibility((v) => (visible = v)));

	// Backend pushes `media://state` on session/playback/property events while subscribed. Hidden → unsubscribe.
	$effect(() => {
		void retryKey;
		if (!tauri || !available || !visible) return;
		let disposed = false;
		let unlisten: (() => void) | null = null;
		on('media://state', (st) => {
			if (!disposed) receive(st.session);
		})
			.then((u) => (disposed ? u() : (unlisten = u)))
			.catch(() => {});
		const subscribed = mediaSubscribe().then(
			() => true,
			() => false
		);
		mediaState()
			.then((st) => {
				if (!disposed) receive(st.session);
			})
			.catch(() => {
				if (!disposed && !loaded) failed = true;
			});
		return () => {
			disposed = true;
			unlisten?.();
			subscribed.then((ok) => {
				if (ok) mediaUnsubscribe().catch(() => {});
			});
		};
	});

	$effect(() => () => {
		if (settleTimer) clearTimeout(settleTimer);
	});

	const view = $derived(session ? applyOverrides(session, overrides, now) : null);
	const position = $derived(view ? currentPosition(view, now) : null);
	const duration = $derived(view?.duration ?? null);
	const frac = $derived(progress(position, duration));
	const ticking = $derived(!!view?.playing && position !== null);

	// One shared 1 Hz tick (no rAF) only while something is actually moving.
	$effect(() => {
		if (!ticking) return;
		return subscribeTick((d) => (now = d.getTime()), { seconds: true });
	});

	/** Re-evaluate overrides once they expire, even when paused (no tick running). */
	function scheduleSettle() {
		if (settleTimer) clearTimeout(settleTimer);
		settleTimer = setTimeout(() => {
			settleTimer = null;
			now = Date.now();
		}, HOLD_MS + 20);
	}

	function control(action: MediaAction) {
		if (editing || !view) return;
		const at = Date.now();
		if (action === 'play-pause') overrides = { ...overrides, playing: { value: !view.playing, at } };
		else if (action === 'toggle-shuffle' && view.shuffle !== null) overrides = { ...overrides, shuffle: { value: !view.shuffle, at } };
		else if (action === 'cycle-repeat' && view.repeat !== null) overrides = { ...overrides, repeat: { value: nextRepeat(view.repeat), at } };
		now = at;
		scheduleSettle();
		mediaControl(action).catch(() => {});
	}

	// ── Seek (4×2): native range, preview while dragging, commit on change ──
	let scrub = $state<number | null>(null);
	const shownFrac = $derived(scrub !== null ? scrub / SCRUB_MAX : (frac ?? 0));
	const shownPos = $derived(scrub !== null && duration ? seekSeconds(scrub / SCRUB_MAX, duration) : position);

	function onScrub(e: Event & { currentTarget: HTMLInputElement }) {
		scrub = Number(e.currentTarget.value);
	}

	function commitSeek(e: Event & { currentTarget: HTMLInputElement }) {
		const value = Number(e.currentTarget.value);
		scrub = null;
		if (editing || !duration) return;
		const target = seekSeconds(value / SCRUB_MAX, duration);
		const at = Date.now();
		overrides = { ...overrides, seek: { position: target, at } };
		now = at;
		scheduleSettle();
		mediaSeek(target).catch(() => {});
	}

	const idle = $derived(loaded && !view);
	const sub4x1 = $derived(view ? byline(view.artist, view.source) : '');
	const sub4x2 = $derived(view ? byline(view.artist, s.showAlbum ? view.album : null) : '');
	const timeLabel = $derived(t('widgets.media.position', { pos: formatTime(shownPos), dur: formatTime(duration) }));

	// ── Artwork tint (DESIGN §5.7, flat look, 4×2) ──
	// The root's data-theme / data-look attributes are the theme signal (set by applyTheme / the page).
	const root = typeof document !== 'undefined' ? document.documentElement : null;
	let themeId = $state(root?.dataset.theme ?? '');
	let look = $state(root?.dataset.look ?? 'flat');
	$effect(() => {
		if (!root || size !== '4x2') return;
		const obs = new MutationObserver(() => {
			themeId = root.dataset.theme ?? '';
			look = root.dataset.look ?? 'flat';
		});
		obs.observe(root, { attributes: true, attributeFilter: ['data-theme', 'data-look'] });
		return () => obs.disconnect();
	});

	/**
	 * Average artwork colour, once per track: the artwork is a `data:` URL on every platform (the backend
	 * inlines it), so drawing it on a canvas never taints it. 8×8 downscale, alpha-weighted.
	 */
	const artSrc = $derived(size === '4x2' ? (view?.artwork ?? null) : null);
	let artRgb = $state<Rgb | null>(null);
	$effect(() => {
		const src = artSrc;
		artRgb = null;
		if (!src) return;
		let off = false;
		const img = new Image();
		img.onload = () => {
			if (off) return;
			try {
				const c = document.createElement('canvas');
				c.width = c.height = 8;
				const ctx = c.getContext('2d', { willReadFrequently: true });
				if (!ctx) return;
				ctx.drawImage(img, 0, 0, 8, 8);
				artRgb = averageColor(ctx.getImageData(0, 0, 8, 8).data);
			} catch {
				artRgb = null;
			}
		};
		img.src = src;
		return () => {
			off = true;
		};
	});
	const tint = $derived.by(() => {
		if (!artRgb || look === 'glass') return undefined;
		const c = getTheme(themeId).colors;
		return rgbCss(tintColor(artRgb, { surface: c.surface, text: c.text, muted: c['text-muted'] }));
	});
</script>

{#snippet note(px: number)}
	<svg width={px} height={px} viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
		<path d="M9 18V5l12-2v13" />
		<circle cx="6" cy="18" r="3" />
		<circle cx="18" cy="16" r="3" />
	</svg>
{/snippet}

{#snippet artwork(cls: string, iconPx: number)}
	<div class="art {cls}">
		{#if view?.artwork}
			<img src={view.artwork} alt="" draggable="false" />
		{:else}
			<span class="placeholder">{@render note(iconPx)}</span>
		{/if}
	</div>
{/snippet}

{#snippet prevBtn()}
	<button type="button" class="btn ghost" aria-label={t('widgets.media.prev')} disabled={editing || !view?.canPrev} onclick={() => control('prev')}>
		<svg width="16" height="16" viewBox="0 0 24 24" fill="currentColor" aria-hidden="true"><path d="M19 20 9 12l10-8z" /><rect x="4" y="4" width="3" height="16" rx="1" /></svg>
	</button>
{/snippet}

{#snippet nextBtn()}
	<button type="button" class="btn ghost" aria-label={t('widgets.media.next')} disabled={editing || !view?.canNext} onclick={() => control('next')}>
		<svg width="16" height="16" viewBox="0 0 24 24" fill="currentColor" aria-hidden="true"><path d="M5 4l10 8-10 8z" /><rect x="17" y="4" width="3" height="16" rx="1" /></svg>
	</button>
{/snippet}

{#snippet playBtn(wide: boolean)}
	<button
		type="button"
		class="btn play"
		class:wide
		aria-label={view?.playing ? t('widgets.media.pause') : t('widgets.media.play')}
		disabled={editing || !view?.canPlayPause}
		onclick={() => control('play-pause')}
	>
		{#if view?.playing}
			<svg width={wide ? 18 : 16} height={wide ? 18 : 16} viewBox="0 0 24 24" fill="currentColor" aria-hidden="true"><rect x="6" y="4" width="4" height="16" rx="1" /><rect x="14" y="4" width="4" height="16" rx="1" /></svg>
		{:else}
			<svg width={wide ? 18 : 16} height={wide ? 18 : 16} viewBox="0 0 24 24" fill="currentColor" aria-hidden="true"><path d="M7 4.5v15a1 1 0 0 0 1.5.86l12-7.5a1 1 0 0 0 0-1.72l-12-7.5A1 1 0 0 0 7 4.5Z" /></svg>
		{/if}
	</button>
{/snippet}

{#snippet stateIcon()}
	<svg width="22" height="22" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
		<path d="M9 18V5l12-2v13" /><circle cx="6" cy="18" r="3" /><circle cx="18" cy="16" r="3" /><path d="M2 2l20 20" />
	</svg>
{/snippet}

<section class="k-card media" class:idle data-size={size} style:--art={tint} aria-label={t('widgets.media.title')}>
	{#if !tauri || !available}
		<div class="state">
			{@render stateIcon()}
			<span>{!tauri ? t('widgets.common.browserPreview') : t('widgets.common.notAvailable')}</span>
		</div>
	{:else if failed}
		<div class="state">
			{@render stateIcon()}
			<span class="state-title">{t('widgets.media.error')}</span>
			<button type="button" class="k-btn" disabled={editing} onclick={() => retryKey++}>{t('widgets.common.retry')}</button>
		</div>
	{:else if !loaded}
		<div class="skeleton" aria-busy="true" aria-label={t('widgets.common.loading')}>
			<span class="bar art-s"></span>
			<span class="sk-lines"><span class="bar" style="width: 60%; height: 16px"></span><span class="bar" style="width: 40%; height: 12px"></span></span>
		</div>
	{:else if size === '4x2'}
		{@render artwork('art-l', 56)}
		<div class="col">
			<div class="meta-block">
				{#if view}
					<span class="tiny">{view.source}</span>
					<span class="title-l">{view.title}</span>
					{#if sub4x2}<span class="sub-l">{sub4x2}</span>{/if}
				{:else}
					<span class="title-l muted">{t('widgets.media.nothingPlaying')}</span>
				{/if}
			</div>
			{#if view && position !== null}
				<div class="timeline">
					<span class="time mono">{formatTime(shownPos)}</span>
					{#if duration}
						<input
							type="range"
							class="seek"
							min="0"
							max={SCRUB_MAX}
							step="1"
							value={Math.round(shownFrac * SCRUB_MAX)}
							style:--p="{shownFrac * 100}%"
							aria-label={t('widgets.media.seek')}
							aria-valuetext={timeLabel}
							disabled={editing}
							oninput={onScrub}
							onchange={commitSeek}
						/>
					{:else}
						<div class="track l"><div class="fill" style:width="0%"></div></div>
					{/if}
					<span class="time mono">{formatTime(duration)}</span>
				</div>
			{/if}
			<div class="btns">
				{#if view?.shuffle != null}
					<button
						type="button"
						class="btn ghost toggle"
						aria-label={t('widgets.media.shuffle')}
						aria-pressed={view.shuffle}
						disabled={editing}
						onclick={() => control('toggle-shuffle')}
					>
						<svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M16 3h5v5M4 20 21 3M21 16v5h-5M15 15l6 6M4 4l5 5" /></svg>
					</button>
				{/if}
				{@render prevBtn()}
				{@render playBtn(true)}
				{@render nextBtn()}
				{#if view?.repeat != null}
					<button
						type="button"
						class="btn ghost toggle"
						aria-label={t(repeatKey(view.repeat))}
						aria-pressed={view.repeat !== 'none'}
						disabled={editing}
						onclick={() => control('cycle-repeat')}
					>
						<svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
							<path d="M17 1l4 4-4 4M3 11V9a4 4 0 0 1 4-4h14M7 23l-4-4 4-4M21 13v2a4 4 0 0 1-4 4H3" />
							{#if view.repeat === 'track'}<path d="M11 10h1v4" stroke-width="1.8" />{/if}
						</svg>
					</button>
				{/if}
			</div>
		</div>
	{:else}
		{@render artwork('art-s', 34)}
		<div class="info">
			<div class="meta-block tight">
				{#if view}
					<span class="title-s">{view.title}</span>
					{#if sub4x1}<span class="sub-s">{sub4x1}</span>{/if}
				{:else}
					<span class="title-s muted">{t('widgets.media.nothingPlaying')}</span>
				{/if}
			</div>
			{#if view && position !== null}
				<div class="timeline" role="img" aria-label={timeLabel}>
					<span class="time mono">{formatTime(position)}</span>
					<div class="track s"><div class="fill" style:width="{(frac ?? 0) * 100}%"></div></div>
					<span class="time mono">{formatTime(duration)}</span>
				</div>
			{/if}
		</div>
		<div class="btns">
			{@render prevBtn()}
			{@render playBtn(false)}
			{@render nextBtn()}
		</div>
	{/if}
</section>

<style>
	.media {
		container-type: size;
		flex-direction: row;
		align-items: center;
		gap: 16px;
		padding: 14px 18px;
		font-family: var(--k-font-ui);
		/* Progress fill and play button: accent-2 in flat, text in glass (DESIGN §5.7). */
		--fill: var(--k-accent-2);
		--on-fill: var(--k-on-accent);
	}
	:global(html[data-look='glass']) .media {
		--fill: var(--k-text);
		--on-fill: var(--k-bg);
	}
	/* 4×2 (DESIGN §5.7): padding 26, artwork a square filling the card height, text column beside it.
	   Flat: the card takes 30 % of the artwork colour (--art, inline), accent-2 without artwork. */
	.media[data-size='4x2'] {
		--art-size: min(calc(100cqh - 52px), 47cqw);
		padding: 26px;
		gap: 28px;
		align-items: stretch;
		background-color: color-mix(in srgb, var(--art, var(--k-accent-2)) 30%, var(--k-surface-a));
		border-color: color-mix(in srgb, var(--art, var(--k-accent-2)) 30%, var(--k-card-border, var(--k-border)));
		transition:
			background-color 200ms ease,
			border-color 200ms ease;
	}
	:global(html[data-look='glass']) .media[data-size='4x2'] {
		background-color: var(--k-surface-a);
		border-color: var(--k-card-border);
	}
	.mono {
		font-family: var(--k-font-mono);
		font-variant-numeric: tabular-nums;
	}
	.muted {
		color: var(--k-text-muted);
	}

	.art {
		flex-shrink: 0;
		overflow: hidden;
		background: var(--k-accent-2);
		display: flex;
		align-items: center;
		justify-content: center;
	}
	.art-s {
		width: min(84px, 100cqh);
		height: min(84px, 100cqh);
		border-radius: 12px;
	}
	.art-l {
		width: var(--art-size);
		height: var(--art-size);
		align-self: center;
		border-radius: 18px;
	}
	.art img {
		width: 100%;
		height: 100%;
		object-fit: cover;
		display: block;
	}
	.placeholder {
		display: flex;
		color: var(--k-bg);
	}

	.info {
		flex: 1;
		min-width: 0;
		display: flex;
		flex-direction: column;
		gap: 8px;
	}
	.col {
		flex: 1;
		min-width: 0;
		display: flex;
		flex-direction: column;
		gap: 12px;
	}
	/* Meta at the top, then a spacer: progress sits right above the controls (DESIGN §5.7). */
	.col .meta-block {
		margin-bottom: auto;
	}
	.meta-block {
		display: flex;
		flex-direction: column;
		gap: 3px;
		min-width: 0;
	}
	.col .meta-block {
		gap: 6px;
	}
	.meta-block.tight {
		gap: 2px;
	}
	.title-s,
	.title-l,
	.sub-s,
	.sub-l,
	.tiny {
		white-space: nowrap;
		overflow: hidden;
		text-overflow: ellipsis;
	}
	.title-s {
		font-size: 15px;
		font-weight: 600;
	}
	.sub-s {
		font-size: 12px;
		color: var(--k-text-muted);
	}
	.tiny {
		font-size: var(--k-fs-meta);
		color: var(--k-text-muted);
		text-transform: uppercase;
		letter-spacing: 0.08em;
	}
	/* Container-scaled (DESIGN §3): 20 px at 1024×600, 44 px at 1920×1080. */
	.title-l {
		font-size: max(20px, min(44px, 4.74cqw, calc(13cqh - 19px)));
		font-weight: 600;
		letter-spacing: -0.02em;
		line-height: 1.15;
	}
	.sub-l {
		font-size: 19px;
		color: var(--k-text-muted);
	}

	.timeline {
		display: flex;
		align-items: center;
		gap: 10px;
		min-width: 0;
	}
	.time {
		flex-shrink: 0;
		font-size: 11px;
		color: var(--k-text-muted);
	}
	.col .time {
		font-size: 14px;
	}
	.track {
		flex: 1;
		min-width: 0;
		background: var(--k-inset);
		overflow: hidden;
	}
	.track.s {
		height: 4px;
		border-radius: 2px;
	}
	.track.l {
		height: 8px;
		border-radius: 4px;
	}
	.fill {
		height: 100%;
		background: var(--fill);
	}

	/* 4×2 seek bar: a native range drawn as the 8 px progress bar, 24 px tall to hit. */
	.seek {
		--p: 0%;
		flex: 1;
		min-width: 0;
		height: 24px;
		margin: 0;
		background: transparent;
		appearance: none;
		-webkit-appearance: none;
		cursor: pointer;
		touch-action: none;
	}
	.seek::-webkit-slider-runnable-track {
		height: 8px;
		border-radius: 4px;
		background: linear-gradient(to right, var(--fill) var(--p), var(--k-inset) var(--p));
	}
	.seek::-webkit-slider-thumb {
		-webkit-appearance: none;
		appearance: none;
		width: 16px;
		height: 16px;
		margin-top: -4px;
		border-radius: 50%;
		background: var(--fill);
		border: 2px solid var(--k-surface);
		opacity: 0;
		transition: opacity 150ms ease;
	}
	.seek:hover::-webkit-slider-thumb,
	.seek:focus-visible::-webkit-slider-thumb,
	.seek:active::-webkit-slider-thumb {
		opacity: 1;
	}
	.seek::-moz-range-track {
		height: 8px;
		border-radius: 4px;
		background: linear-gradient(to right, var(--fill) var(--p), var(--k-inset) var(--p));
	}
	.seek::-moz-range-thumb {
		width: 14px;
		height: 14px;
		border-radius: 50%;
		background: var(--fill);
		border: 2px solid var(--k-surface);
	}

	/* Controls: play/pause is the only filled button (round); prev/next and toggles have no border or fill. */
	.btns {
		display: flex;
		align-items: center;
		gap: 8px;
		flex-shrink: 0;
	}
	.media[data-size='4x2'] .btns {
		justify-content: center;
		gap: 14px;
	}
	.btn {
		--ico: 16px;
		width: 44px;
		height: 44px;
		flex-shrink: 0;
		display: flex;
		align-items: center;
		justify-content: center;
		padding: 0;
		border-radius: 50%;
		border: 0;
		background: transparent;
		color: var(--k-text);
	}
	.btn :global(svg) {
		width: var(--ico);
		height: var(--ico);
	}
	.btn.play {
		background: var(--fill);
		color: var(--on-fill);
	}
	.btn.toggle {
		color: var(--k-text-muted);
	}
	.btn.toggle[aria-pressed='true'] {
		color: var(--k-accent-2);
		background: color-mix(in srgb, var(--k-accent-2) 22%, transparent);
	}
	/* 4×2 sizes: play 84, prev/next 60, shuffle/repeat 52. */
	.media[data-size='4x2'] .btn.ghost {
		width: 60px;
		height: 60px;
		--ico: 26px;
	}
	.media[data-size='4x2'] .btn.toggle {
		width: 52px;
		height: 52px;
		--ico: 20px;
	}
	.media[data-size='4x2'] .btn.play {
		width: 84px;
		height: 84px;
		--ico: 32px;
	}
	/* Small 4×2 cards (1024×600, 1280×720): the pre-refresh scale, touch targets stay ≥ 44 px. */
	@container (width < 700px) or (height < 300px) {
		.media[data-size='4x2'] {
			--art-size: min(calc(100cqh - 36px), 30cqw);
			padding: 18px;
			gap: 18px;
		}
		.media[data-size='4x2'] .btns {
			gap: 6px;
		}
		.media[data-size='4x2'] .btn.ghost,
		.media[data-size='4x2'] .btn.toggle {
			width: 44px;
			height: 44px;
			--ico: 16px;
		}
		.media[data-size='4x2'] .btn.play {
			width: 56px;
			height: 56px;
			--ico: 20px;
		}
		.sub-l {
			font-size: 13px;
		}
		.col .time {
			font-size: 11px;
		}
		.col {
			gap: 8px;
		}
	}
	/* DESIGN §5.7: no session → disabled at .4. */
	.media.idle .btn:disabled {
		opacity: 0.4;
	}

	/* States */
	.state {
		flex: 1;
		display: flex;
		align-items: center;
		justify-content: center;
		gap: 10px;
		font-size: 12px;
		color: var(--k-text-muted);
		text-align: center;
	}
	.media[data-size='4x2'] .state {
		flex-direction: column;
		gap: 6px;
	}
	.state-title {
		font-size: 13px;
		font-weight: 600;
		color: var(--k-text);
	}
	.skeleton {
		flex: 1;
		display: flex;
		align-items: center;
		gap: 16px;
		min-width: 0;
	}
	.sk-lines {
		flex: 1;
		display: flex;
		flex-direction: column;
		gap: 8px;
	}
	.bar {
		display: block;
		border-radius: 8px;
		background: var(--k-inset);
	}

	/* Narrow (portrait grids, 1024×600): drop the times on 4×1, then the artwork. */
	@container (width < 470px) {
		.media[data-size='4x1'] .time {
			display: none;
		}
	}
	@container (width < 400px) {
		.media[data-size='4x1'] .art {
			display: none;
		}
		.media[data-size='4x2'] .art {
			display: none;
		}
	}
	@media (prefers-reduced-motion: reduce) {
		.seek::-webkit-slider-thumb,
		.media[data-size='4x2'] {
			transition: none;
		}
	}
</style>
