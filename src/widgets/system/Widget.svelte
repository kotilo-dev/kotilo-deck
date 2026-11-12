<script lang="ts">
	import { locale, t } from '#lib/i18n/index.ts';
	import {
		hasBackend,
		on,
		systemInfo,
		systemSubscribe,
		systemUnsubscribe,
		type SystemInfo,
		type SystemSample
	} from '#lib/ipc/index.ts';
	import { isDeckVisible, onDeckVisibility } from '#lib/time/index.ts';
	import type { WidgetProps } from '../types';
	import { gb, gbTotal, initialWarn, percent, pushHistory, sparklinePoints, stepWarn, type WarnState } from './logic';
	import ProcessSheet from './ProcessSheet.svelte';
	import { defaults, type SystemSettings } from './settings';

	let { size, settings, editing, available }: WidgetProps<SystemSettings> = $props();

	const s = $derived({ ...defaults, ...settings });
	const threshold = $derived(
		Number.isFinite(Number(s.threshold)) ? Math.min(99, Math.max(1, Number(s.threshold))) : defaults.threshold
	);
	const tauri = hasBackend();

	let info = $state<SystemInfo | null>(null);
	let sample = $state<SystemSample | null>(null);
	let history = $state<number[]>([]);
	let cpuWarn = $state<WarnState>(initialWarn);
	let memWarn = $state<WarnState>(initialWarn);
	let gpuWarn = $state<WarnState>(initialWarn);
	let failed = $state(false);

	function onSample(next: SystemSample) {
		sample = next;
		history = pushHistory(history, next.cpu);
		cpuWarn = stepWarn(cpuWarn, next.cpu, threshold);
		memWarn = stepWarn(memWarn, percent(next.memUsed, next.memTotal), threshold);
		gpuWarn = next.gpu === null ? initialWarn : stepWarn(gpuWarn, next.gpu, threshold);
	}

	// Deck hidden to tray / minimised → drop the subscription so the backend sampler stops (idle budget).
	// `deck://visibility` is the primary signal (WebView2 may not flip document.hidden), document.hidden secondary.
	let visible = $state(isDeckVisible());
	$effect(() => onDeckVisibility((v) => (visible = v)));

	// Backend samples only while subscribed (ref-counted): pair every subscribe with an unsubscribe.
	$effect(() => {
		if (!available || !tauri || !visible) return;
		let disposed = false;
		let unlisten: (() => void) | null = null;
		failed = false;
		systemInfo()
			.then((i) => {
				if (!disposed) info = i;
			})
			.catch(() => {});
		on('system://sample', onSample)
			.then((u) => (disposed ? u() : (unlisten = u)))
			.catch(() => {});
		const subscribed = systemSubscribe().then(
			() => true,
			() => {
				if (!disposed) failed = true;
				return false;
			}
		);
		return () => {
			disposed = true;
			unlisten?.();
			subscribed.then((ok) => {
				if (ok) systemUnsubscribe().catch(() => {});
			});
		};
	});

	let sheet: ProcessSheet | undefined = $state();

	const tileLabel = (key: string, warn: boolean) => (warn ? `${t(key)} · ${t('widgets.system.warn')}` : t(key));

	const memPct = $derived(sample ? percent(sample.memUsed, sample.memTotal) : 0);
	const cpuPct = $derived(sample ? Math.round(sample.cpu) : 0);
	const gpuPct = $derived(sample?.gpu != null ? Math.round(sample.gpu) : null);
	const anyWarn = $derived(cpuWarn.warn || memWarn.warn || (gpuPct !== null && gpuWarn.warn));
	const worst = $derived(
		[cpuWarn, memWarn, gpuWarn].filter((w) => w.warn).reduce((a, b) => (b.over > a.over ? b : a), initialWarn)
	);
	const warnMeta = $derived(t('widgets.system.over', { n: worst.over, x: threshold }));
	const memUsed = $derived(sample ? gb(sample.memUsed, locale) : '');
	const memTotal = $derived(sample ? gbTotal(sample.memTotal, locale) : '');
	const cores = $derived(sample?.perCore ?? []);
	const cpuName = $derived(info?.cpuName ?? '');
	const wideMeta = $derived(
		[cpuName, info ? t('widgets.system.cores', { n: info.cores }) : null, t('widgets.system.tapHint')]
			.filter(Boolean)
			.join(' · ')
	);
</script>

{#snippet spark(height: number, warn: boolean, fill = false)}
	<!-- `fill`: the 4×2 tile sparkline stretches to the tile's free height (non-scaling 2 px stroke). -->
	<svg
		class="spark"
		class:warn
		class:fill
		style:height={fill ? undefined : `${height}px`}
		viewBox="0 0 240 {height}"
		preserveAspectRatio="none"
		aria-hidden="true"
	>
		<polyline points={sparklinePoints(history, 240, height)} />
	</svg>
{/snippet}

{#snippet meter(label: string, value: string, pct: number, kind: 'mem' | 'gpu', warn: boolean)}
	<div class="meter">
		<div class="row">
			<span class="label">{label}</span>
			<span class="mono val-s" class:warn-text={warn}>
				{value}{#if warn}<span class="warn-tag">{t('widgets.system.warn')}</span>{/if}
			</span>
		</div>
		<div class="track"><div class="fill {kind}" class:warn style="width: {pct}%"></div></div>
	</div>
{/snippet}

<section
	class="k-card system"
	data-size={size}
	data-attention={anyWarn ? 'true' : undefined}
	aria-label={t('widgets.system.title')}
>
	{#if !available || !tauri}
		<header class="hdr">
			<span class="title">{t('widgets.system.title')}</span>
		</header>
		<div class="state">
			<svg width="22" height="22" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
				<rect x="4" y="4" width="16" height="16" rx="2" />
				<rect x="9" y="9" width="6" height="6" />
				<path d="M9 1v3M15 1v3M9 20v3M15 20v3M20 9h3M20 14h3M1 9h3M1 14h3" />
			</svg>
			<span>{!available ? t('widgets.common.notAvailable') : t('widgets.common.browserPreview')}</span>
		</div>
	{:else}
		<header class="hdr">
			<span class="title">{t('widgets.system.title')}</span>
			<span class="meta" class:warn-text={anyWarn}>
				{anyWarn ? warnMeta : size === '4x2' ? wideMeta : t('widgets.system.window')}
			</span>
		</header>

		{#if !sample}
			<div class="skeleton" aria-busy="true" aria-label={t('widgets.common.loading')}>
				{#if failed}
					<span class="state-inline">{t('widgets.common.notAvailable')}</span>
				{:else}
					<span class="bar" style="height: 56px"></span>
					<span class="bar" style="height: 22px"></span>
					<span class="bar" style="height: 22px"></span>
				{/if}
			</div>
		{:else if size === '4x2'}
			<div class="tiles" style="grid-template-columns: repeat({gpuPct === null ? 2 : 3}, minmax(0, 1fr))">
				<div class="tile" class:warn={cpuWarn.warn}>
					<span class="tiny">{tileLabel('widgets.system.cpu', cpuWarn.warn)}</span>
					<span class="mono val-l">{cpuPct}<span class="unit">%</span></span>
					{@render spark(40, cpuWarn.warn, true)}
				</div>
				<div class="tile" class:warn={memWarn.warn}>
					<span class="tiny">{tileLabel('widgets.system.memory', memWarn.warn)}</span>
					<span class="mono val-l">{Math.round(memPct)}<span class="unit">%</span></span>
					<span class="sub">{t('widgets.system.memOf', { used: memUsed, total: memTotal })}</span>
				</div>
				{#if gpuPct !== null}
					<div class="tile" class:warn={gpuWarn.warn}>
						<span class="tiny">{tileLabel('widgets.system.gpu', gpuWarn.warn)}</span>
						<span class="mono val-l">{gpuPct}<span class="unit">%</span></span>
						<span class="sub">{info?.gpuName ?? ''}</span>
					</div>
				{/if}
			</div>
			<div class="cores" class:dense={cores.length > 32} role="img" aria-label={t('widgets.system.perCore')}>
				{#each cores as c, i (i)}
					<div class="core" style="height: {Math.max(4, Math.min(100, c))}%"></div>
				{/each}
			</div>
		{:else}
			<div class="cpu">
				<div class="row">
					<span class="label">{t('widgets.system.cpu')}</span>
					<span class="mono val-m" class:warn-text={cpuWarn.warn}>{cpuPct}%</span>
				</div>
				{@render spark(36, cpuWarn.warn)}
			</div>
			{@render meter(
				t('widgets.system.memory'),
				t('widgets.system.memSlash', { used: memUsed, total: memTotal }),
				memPct,
				'mem',
				memWarn.warn
			)}
			{#if gpuPct !== null}
				{@render meter(t('widgets.system.gpu'), `${gpuPct}%`, gpuPct, 'gpu', gpuWarn.warn)}
			{/if}
		{/if}

		{#if !editing && sample}
			<button type="button" class="cover" aria-haspopup="dialog" aria-label={t('widgets.system.open')} onclick={() => sheet?.open()}
			></button>
		{/if}
		<ProcessSheet bind:this={sheet} />
	{/if}
</section>

<style>
	.system {
		--warn-text: color-mix(in srgb, var(--k-warning) 35%, var(--k-text));
		container-type: size;
		position: relative;
		padding: 18px 20px;
		gap: 12px;
		font-family: var(--k-font-ui);
	}
	.system[data-size='4x2'] {
		padding: 18px 22px;
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
	}
	.title {
		font-size: 13px;
		font-weight: 600;
		flex-shrink: 0;
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
	.row {
		display: flex;
		justify-content: space-between;
		align-items: baseline;
		gap: 8px;
	}
	.label {
		font-size: 12px;
		color: var(--k-text-muted);
	}
	.cpu,
	.meter {
		display: flex;
		flex-direction: column;
		gap: 4px;
		min-height: 0;
	}
	.meter {
		gap: 6px;
	}
	.val-m {
		font-size: 20px;
		font-weight: 500;
	}
	.val-s {
		font-size: 13px;
		white-space: nowrap;
	}
	.warn-tag {
		margin-left: 6px;
		font-family: var(--k-font-ui);
	}
	.spark {
		display: block;
		width: 100%;
		flex-shrink: 0;
	}
	.spark.fill {
		flex: 1 1 auto;
		min-height: 24px;
		height: auto;
	}
	.spark polyline {
		fill: none;
		stroke: var(--k-accent);
		stroke-width: 2;
		stroke-linejoin: round;
		stroke-linecap: round;
		vector-effect: non-scaling-stroke;
	}
	.spark.warn polyline {
		stroke: var(--k-warning);
	}
	.track {
		height: 6px;
		border-radius: 3px;
		background: var(--k-inset);
		overflow: hidden;
	}
	.fill {
		height: 100%;
		border-radius: 3px;
		transition: width 200ms ease;
	}
	.fill.mem {
		background: var(--k-info);
	}
	.fill.gpu {
		background: var(--k-accent);
	}
	.fill.warn {
		background: var(--k-warning);
	}

	/* 4×2 */
	.tiles {
		flex: 1;
		min-height: 0;
		display: grid;
		gap: 14px;
	}
	.tile {
		display: flex;
		flex-direction: column;
		gap: 6px;
		min-width: 0;
		min-height: 0;
		overflow: hidden;
		padding: 12px 14px;
		border-radius: 12px;
		background: var(--k-inset-deep);
		border: 1px solid transparent;
	}
	.tile.warn {
		background: color-mix(in srgb, var(--k-warning) 18%, var(--k-surface));
		border-color: var(--k-warning);
		color: var(--warn-text);
	}
	.tiny {
		font-size: 11px;
		color: var(--k-text-muted);
		text-transform: uppercase;
		letter-spacing: 0.08em;
		white-space: nowrap;
		overflow: hidden;
		text-overflow: ellipsis;
	}
	.tile.warn .tiny,
	.tile.warn .sub,
	.tile.warn .unit {
		color: var(--warn-text);
	}
	.val-l {
		font-size: 32px;
		font-weight: 500;
		line-height: 1;
	}
	.unit {
		font-size: 16px;
		color: var(--k-text-muted);
	}
	.sub {
		font-size: 12px;
		color: var(--k-text-muted);
		white-space: nowrap;
		overflow: hidden;
		text-overflow: ellipsis;
	}
	.cores {
		display: flex;
		align-items: flex-end;
		gap: 4px;
		/* 40 px at 1280×720 (DESIGN §5.5); grows with taller cards up to ~25 % of the card (1920×1080). */
		height: clamp(40px, calc((100cqh - 220px) / 2), 25cqh);
		flex-shrink: 0;
	}
	.cores.dense {
		gap: 2px;
	}
	.core {
		flex: 1;
		min-width: 1px;
		border-radius: 3px;
		background: var(--k-accent);
		transition: height 200ms ease;
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
		font-size: 12px;
		color: var(--k-text-muted);
	}
	.skeleton {
		flex: 1;
		display: flex;
		flex-direction: column;
		gap: 12px;
		min-height: 0;
	}
	.bar {
		display: block;
		border-radius: 8px;
		background: var(--k-inset);
	}
	.state-inline {
		margin: auto;
		font-size: 12px;
		color: var(--k-text-muted);
	}

	/* Whole card is the tap target for the process sheet. */
	.cover {
		position: absolute;
		inset: 0;
		margin: 0;
		padding: 0;
		border: 0;
		border-radius: inherit;
		background: transparent;
		cursor: pointer;
	}
	.cover:focus-visible {
		outline-offset: -3px;
	}

	/* 1024×600: shrink the secondary pieces, keep the numbers. */
	@container (height < 200px) {
		.system[data-size='4x2'] .cores {
			height: 24px;
		}
		.system[data-size='4x2'] .tile {
			padding: 10px 12px;
		}
	}
	@container (height < 190px) {
		.system[data-size='2x2'] .meter .track {
			height: 4px;
		}
	}
</style>
