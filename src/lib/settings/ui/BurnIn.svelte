<script lang="ts">
	/** Burn-in & night (SPEC §2, §5.3): pixel shift interval, night dim schedule, idle dim + clock face. */
	import { rangeFill } from '#lib/ui/range.ts';
	import type { BurnInSettings } from '#lib/ipc/index.ts';
	import { t } from '#lib/i18n/index.ts';
	import { parseHm } from '#lib/theme/index.ts';
	import { IDLE_CHOICES, SHIFT_CHOICES } from '../look';
	import { getSettingsContext } from './context.svelte';
	import Head from './Head.svelte';
	import Switch from './Switch.svelte';

	const { store, env, save } = getSettingsContext();
	const b = $derived(store.settings.burnIn);
	/** The Appearance page may hold unapplied dim toggles; show that so the two pages don't disagree silently. */
	const pending = $derived(env.draft.night !== undefined || env.draft.idle !== undefined);

	function patch(next: Partial<BurnInSettings>) {
		void save({ burnIn: { ...b, ...next } });
	}
	const night = (p: Partial<BurnInSettings['night']>) => patch({ night: { ...b.night, ...p } });
	const idle = (p: Partial<BurnInSettings['idle']>) => patch({ idle: { ...b.idle, ...p } });
	const setNightTime = (key: 'from' | 'to', v: string) => parseHm(v) !== null && night({ [key]: v });

	// Sliders update the label while dragging and save once on release.
	let nightDim = $state<number | null>(null);
	let idleDim = $state<number | null>(null);
	const nightPct = $derived(nightDim ?? Math.round(b.night.dim * 100));
	const idlePct = $derived(idleDim ?? Math.round(b.idle.dim * 100));
	const shiftLabel = (m: number) => (m === 0 ? t('settings.common.off') : t('settings.burnin.every', { min: m }));
</script>

<section class="s-page">
	<Head title={t('settings.burnin.title')} text={t('settings.burnin.text')} />
	{#if pending}<p class="s-help warn">{t('settings.burnin.pending')}</p>{/if}

	<div class="s-group">
		<h2 class="s-eyebrow">{t('settings.burnin.shift')}</h2>
		<label class="s-card s-row">
			<span class="s-label">
				<span class="s-title">{t('settings.burnin.shiftLabel')}</span>
				<span class="s-help">{t('settings.burnin.shiftHelp')}</span>
			</span>
			<select class="k-select s-select-sm" value={String(b.shiftMinutes)} onchange={(e) => patch({ shiftMinutes: Number(e.currentTarget.value) })}>
				{#if !(SHIFT_CHOICES as readonly number[]).includes(b.shiftMinutes)}<option value={String(b.shiftMinutes)}>{shiftLabel(b.shiftMinutes)}</option>{/if}
				{#each SHIFT_CHOICES as m (m)}<option value={String(m)}>{shiftLabel(m)}</option>{/each}
			</select>
		</label>
	</div>

	<div class="s-group">
		<h2 class="s-eyebrow">{t('settings.burnin.night')}</h2>
		<div class="s-card s-stack">
			<div class="s-row">
				<div class="s-label">
					<span class="s-title">{t('settings.burnin.nightLabel')}</span>
					<span class="s-help" id="k-bn-night">{t('settings.burnin.nightHelp')}</span>
				</div>
				<Switch label={t('settings.burnin.nightLabel')} describedby="k-bn-night" checked={b.night.enabled} onchange={(v) => night({ enabled: v })} />
			</div>
			<div class="s-inline gap" class:s-disabled={!b.night.enabled}>
				<label class="s-inline">
					<span class="s-help">{t('settings.burnin.from')}</span>
					<input type="time" class="k-input s-time" value={b.night.from} disabled={!b.night.enabled} onchange={(e) => setNightTime('from', e.currentTarget.value)} />
				</label>
				<label class="s-inline">
					<span class="s-help">{t('settings.burnin.to')}</span>
					<input type="time" class="k-input s-time" value={b.night.to} disabled={!b.night.enabled} onchange={(e) => setNightTime('to', e.currentTarget.value)} />
				</label>
			</div>
			<div class="s-slider" class:s-disabled={!b.night.enabled}>
				<label class="s-label" for="k-bn-ndim"><span class="s-title">{t('settings.burnin.dimTo')}</span></label>
				<input
					id="k-bn-ndim"
					type="range"
					class="k-range" use:rangeFill={nightPct}
					min="30"
					max="100"
					step="5"
					value={nightPct}
					disabled={!b.night.enabled}
					oninput={(e) => (nightDim = Number(e.currentTarget.value))}
					onchange={(e) => {
						night({ dim: Number(e.currentTarget.value) / 100 });
						nightDim = null;
					}}
				/>
				<span class="s-value">{nightPct}%</span>
			</div>
		</div>
	</div>

	<div class="s-group">
		<h2 class="s-eyebrow">{t('settings.burnin.idle')}</h2>
		<div class="s-card s-stack">
			<div class="s-row">
				<div class="s-label">
					<span class="s-title">{t('settings.burnin.idleLabel')}</span>
					<span class="s-help" id="k-bn-idle">{t('settings.burnin.idleHelp')}</span>
				</div>
				<Switch label={t('settings.burnin.idleLabel')} describedby="k-bn-idle" checked={b.idle.enabled} onchange={(v) => idle({ enabled: v })} />
			</div>
			<label class="s-row" class:s-disabled={!b.idle.enabled}>
				<span class="s-title">{t('settings.burnin.after')}</span>
				<select class="k-select s-select-sm" value={String(b.idle.minutes)} disabled={!b.idle.enabled} onchange={(e) => idle({ minutes: Number(e.currentTarget.value) })}>
					{#if !(IDLE_CHOICES as readonly number[]).includes(b.idle.minutes)}<option value={String(b.idle.minutes)}>{t('settings.burnin.minutes', { min: b.idle.minutes })}</option>{/if}
					{#each IDLE_CHOICES as m (m)}<option value={String(m)}>{t('settings.burnin.minutes', { min: m })}</option>{/each}
				</select>
			</label>
			<div class="s-slider" class:s-disabled={!b.idle.enabled}>
				<label class="s-label" for="k-bn-idim"><span class="s-title">{t('settings.burnin.dimTo')}</span></label>
				<input
					id="k-bn-idim"
					type="range"
					class="k-range" use:rangeFill={idlePct}
					min="30"
					max="100"
					step="5"
					value={idlePct}
					disabled={!b.idle.enabled}
					oninput={(e) => (idleDim = Number(e.currentTarget.value))}
					onchange={(e) => {
						idle({ dim: Number(e.currentTarget.value) / 100 });
						idleDim = null;
					}}
				/>
				<span class="s-value">{idlePct}%</span>
			</div>
			<div class="s-row" class:s-disabled={!b.idle.enabled}>
				<div class="s-label">
					<span class="s-title">{t('settings.burnin.clockFace')}</span>
					<span class="s-help" id="k-bn-clock">{t('settings.burnin.clockFaceHelp')}</span>
				</div>
				<Switch
					label={t('settings.burnin.clockFace')}
					describedby="k-bn-clock"
					checked={b.idle.clockFace}
					disabled={!b.idle.enabled}
					onchange={(v) => idle({ clockFace: v })}
				/>
			</div>
		</div>
	</div>
</section>

<style>
	.gap {
		gap: 20px;
	}
</style>
