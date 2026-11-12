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
	import {
		busiestCore,
		coreBucket,
		gb,
		gbTotal,
		initialWarn,
		peak,
		percent,
		pushHistory,
		sparklineArea,
		sparklinePoints,
		stepWarn,
		type WarnState
	} from './logic';
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
	/** 60-sample windows (DESIGN §5.5): every 4×2 tile has a sparkline. */
	let history = $state<number[]>([]);
	let memHistory = $state<number[]>([]);
	let gpuHistory = $state<number[]>([]);
	let cpuWarn = $state<WarnState>(initialWarn);
	let memWarn = $state<WarnState>(initialWarn);
	let gpuWarn = $state<WarnState>(initialWarn);
	let failed = $state(false);

	function onSample(next: SystemSample) {
		sample = next;
		history = pushHistory(history, next.cpu);
		memHistory = pushHistory(memHistory, percent(next.memUsed, next.memTotal));
		gpuHistory = next.gpu === null ? [] : pushHistory(gpuHistory, next.gpu);
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
	const busiest = $derived(busiestCore(cores));
	const cpuPeak = $derived(peak(history));
	const cpuName = $derived(info?.cpuName ?? '');
	const wideMeta = $derived(
		[cpuName, info ? t('widgets.system.cores', { n: info.cores }) : null, t('widgets.system.tapHint')]
			.filter(Boolean)
			.join(' · ')
	);
</script>

{#snippet spark(values: number[], height: number, warn: boolean)}
	<!-- 2×2: fixed-height line with its area fill. -->
	<svg class="spark cpu" class:warn style:height="{height}px" viewBox="0 0 240 {height}" preserveAspectRatio="none" aria-hidden="true">
		<polygon points={sparklineArea(values, 240, height)} />
		<polyline points={sparklinePoints(values, 240, height)} />
	</svg>
{/snippet}

{#snippet tileSpark(values: number[], kind: 'cpu' | 'mem' | 'gpu', warn: boolean)}
	<!-- 4×2: fills the tile's free height and bleeds to its left, right and bottom edges. -->
	<svg class="spark tile-spark {kind}" class:warn viewBox="0 0 240 100" preserveAspectRatio="none" aria-hidden="true">
		<polygon points={sparklineArea(values, 240, 100, undefined, 2)} />
		<polyline points={sparklinePoints(values, 240, 100, undefined, 2)} />
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
					<span class="sub">{cpuPeak === null ? '' : t('widgets.system.peak', { n: cpuPeak })}</span>
					{@render tileSpark(history, 'cpu', cpuWarn.warn)}
				</div>
				<div class="tile" class:warn={memWarn.warn}>
					<span class="tiny">{tileLabel('widgets.system.memory', memWarn.warn)}</span>
					<span class="mono val-l">{Math.round(memPct)}<span class="unit">%</span></span>
					<span class="sub">{t('widgets.system.memOf', { used: memUsed, total: memTotal })}</span>
					{@render tileSpark(memHistory, 'mem', memWarn.warn)}
				</div>
				{#if gpuPct !== null}
					<div class="tile" class:warn={gpuWarn.warn}>
						<span class="tiny">{tileLabel('widgets.system.gpu', gpuWarn.warn)}</span>
						<span class="mono val-l">{gpuPct}<span class="unit">%</span></span>
						<span class="sub">{info?.gpuName ?? ''}</span>
						{@render tileSpark(gpuHistory, 'gpu', gpuWarn.warn)}
					</div>
				{/if}
			</div>
			<div class="cores-block">
				<div class="cores-head">
					<span class="tiny">{t('widgets.system.coresLabel')}</span>
					{#if busiest}<span class="mono busiest">{t('widgets.system.busiest', { n: busiest.core, x: busiest.load })}</span>{/if}
				</div>
				<!-- Height carries the load; colour (info / accent / warning) is a second cue, never the only one. -->
				<div class="cores" class:dense={cores.length > 32} role="img" aria-label={t('widgets.system.perCore')}>
					{#each cores as c, i (i)}
						<div class="core {coreBucket(c, threshold)}" style="height: {Math.max(4, Math.min(100, c))}%"></div>
					{/each}
				</div>
			</div>
		{:else}
			<div class="cpu">
				<div class="row">
					<span class="label">{t('widgets.system.cpu')}</span>
					<span class="mono val-m" class:warn-text={cpuWarn.warn}>{cpuPct}%</span>
				</div>
				{@render spark(history, 36, cpuWarn.warn)}
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
		font-size: var(--k-fs-title);
		font-weight: 600;
		flex-shrink: 0;
	}
	.meta {
		font-size: var(--k-fs-meta);
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
	/* Line colour via `color`: CPU accent, memory info, GPU accent-2, warning in warn state (DESIGN §5.5). */
	.spark {
		display: block;
		width: 100%;
		flex-shrink: 0;
		color: var(--k-accent);
	}
	.spark.mem {
		color: var(--k-info);
	}
	.spark.gpu {
		color: var(--k-accent-2);
	}
	.spark.warn {
		color: var(--k-warning);
	}
	.spark polyline {
		fill: none;
		stroke: currentColor;
		stroke-width: 2;
		stroke-linejoin: round;
		stroke-linecap: round;
		vector-effect: non-scaling-stroke;
	}
	.spark polygon {
		fill: color-mix(in srgb, currentColor var(--k-spark-fill), transparent);
		stroke: none;
	}
	.tile-spark {
		flex: 1 1 auto;
		min-height: 24px;
		height: auto;
		/* Bleed to the tile's left, right and bottom edges (the tile clips). */
		width: calc(100% + 2 * var(--tile-px));
		margin: 2px calc(-1 * var(--tile-px)) 0;
	}
	.tile-spark polyline {
		stroke-width: 2.5;
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
		/* Horizontal padding, shared with the sparkline's bleed. */
		--tile-px: clamp(14px, 2cqw, 18px);
		display: flex;
		flex-direction: column;
		gap: 6px;
		min-width: 0;
		min-height: 0;
		overflow: hidden;
		padding: clamp(12px, 3.6cqh, 18px) var(--tile-px) 0;
		border-radius: 14px;
		background: var(--k-inset-deep);
		border: 1px solid var(--k-inset-ring);
	}
	/* Text never shrinks (it would overlap); only the sparkline gives up height. */
	.tile > :not(svg) {
		flex-shrink: 0;
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
	/* Container-scaled hero (DESIGN §3): 32 px at 1024×600, 68 px at 1920×1080. */
	.val-l {
		font-size: max(32px, min(68px, 7.4cqw, calc(15cqh - 5px)));
		font-weight: 500;
		line-height: 1;
		letter-spacing: -0.04em;
	}
	.unit {
		font-size: 0.35em;
		letter-spacing: 0;
		color: var(--k-text-muted);
	}
	.sub {
		font-size: 12px;
		color: var(--k-text-muted);
		white-space: nowrap;
		overflow: hidden;
		text-overflow: ellipsis;
	}
	.cores-block {
		display: flex;
		flex-direction: column;
		gap: 8px;
		flex-shrink: 0;
	}
	.cores-head {
		display: flex;
		justify-content: space-between;
		align-items: baseline;
		gap: 12px;
		min-width: 0;
	}
	.busiest {
		font-size: 11px;
		color: var(--k-text-muted);
		white-space: nowrap;
		overflow: hidden;
		text-overflow: ellipsis;
	}
	.cores {
		display: flex;
		align-items: flex-end;
		/* 4 px gap / 40 px tall at 1280×720, 8 px / 64 px at 1920×1080 (DESIGN §5.5). */
		gap: clamp(4px, 0.9cqw, 8px);
		height: clamp(40px, calc((100cqh - 220px) / 4), 64px);
	}
	.cores.dense {
		gap: 2px;
	}
	.core {
		flex: 1;
		min-width: 1px;
		border-radius: clamp(3px, 0.65cqw, 6px);
		background: var(--k-accent);
		transition: height 200ms ease;
	}
	.core.low {
		background: var(--k-info);
	}
	.core.high {
		background: var(--k-warning);
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
	/* Short 4×2 (1024×600 up to ~1280×720): no cores header, thinner sparklines, so the tiles keep their text. */
	@container (height < 320px) {
		.system[data-size='4x2'] .cores-head {
			display: none;
		}
		.system[data-size='4x2'] .tile-spark {
			min-height: 12px;
		}
	}
	@container (height < 190px) {
		.system[data-size='2x2'] .meter .track {
			height: 4px;
		}
	}
</style>
