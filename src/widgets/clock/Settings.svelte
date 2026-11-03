<script lang="ts">
	import { t } from '#lib/i18n/index.ts';
	import { isValidTimeZone, localTimeZone } from '#lib/time/index.ts';
	import type { WidgetSettingsProps } from '../types';
	import { sanitizeZones, zoneLabel, zoneOffsetLabel } from './logic';
	import { MAX_ZONES, defaults, type ClockSettings, type ExtraZone } from './settings';
	import ZonePicker from './ZonePicker.svelte';

	let { settings, onchange }: WidgetSettingsProps<ClockSettings> = $props();

	const s = $derived({ ...defaults, ...settings });
	const zones = $derived(sanitizeZones(s.zones));
	const primary = $derived(isValidTimeZone(s.timeZone) ? s.timeZone : '');

	/** Which picker is open: the main zone, a new extra zone, or none. */
	let picking = $state<'primary' | 'extra' | null>(null);

	const update = (patch: Partial<ClockSettings>) => onchange({ ...s, ...patch });
	const setZones = (next: ExtraZone[]) => update({ zones: next });

	function pick(tz: string) {
		if (picking === 'primary') update({ timeZone: tz });
		else if (picking === 'extra' && zones.length < MAX_ZONES) setZones([...zones, { timeZone: tz, label: '' }]);
		picking = null;
	}
</script>

<div class="clock-settings">
	<fieldset>
		<legend>{t('widgets.clock.settings.primary')}</legend>
		<div class="zone-row">
			<span class="zone">
				{primary
					? `${primary.replace(/_/g, ' ')}`
					: t('widgets.clock.settings.local', { zone: localTimeZone().replace(/_/g, ' ') })}
				<span class="off">{zoneOffsetLabel(primary || localTimeZone())}</span>
			</span>
			{#if primary}
				<button type="button" class="btn ghost" onclick={() => update({ timeZone: '' })}>
					{t('widgets.clock.settings.useLocal')}
				</button>
			{/if}
			<button type="button" class="btn ghost" onclick={() => (picking = picking === 'primary' ? null : 'primary')}>
				{t('widgets.clock.settings.change')}
			</button>
		</div>
		{#if picking === 'primary'}
			<ZonePicker onpick={pick} oncancel={() => (picking = null)} />
		{/if}
	</fieldset>

	<fieldset>
		<legend>{t('widgets.clock.settings.extra')}</legend>
		<p class="hint">{t('widgets.clock.settings.extraHint', { max: MAX_ZONES })}</p>
		{#each zones as z, i (z.timeZone)}
			<div class="zone-row">
				<label class="label-field">
					<span class="sr-only">{t('widgets.clock.settings.label', { zone: z.timeZone })}</span>
					<input
						class="field"
						type="text"
						value={z.label}
						placeholder={zoneLabel({ timeZone: z.timeZone, label: '' })}
						oninput={(e) =>
							setZones(zones.map((x, j) => (j === i ? { ...x, label: e.currentTarget.value } : x)))}
					/>
				</label>
				<span class="off">{z.timeZone.replace(/_/g, ' ')} · {zoneOffsetLabel(z.timeZone)}</span>
				<button
					type="button"
					class="icon-btn"
					aria-label={t('widgets.clock.settings.remove', { name: zoneLabel(z) })}
					onclick={() => setZones(zones.filter((_, j) => j !== i))}
				>
					<svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" aria-hidden="true">
						<path d="M18 6 6 18M6 6l12 12" />
					</svg>
				</button>
			</div>
		{/each}
		{#if picking === 'extra'}
			<ZonePicker exclude={zones.map((z) => z.timeZone)} onpick={pick} oncancel={() => (picking = null)} />
		{:else}
			<button type="button" class="btn ghost add" disabled={zones.length >= MAX_ZONES} onclick={() => (picking = 'extra')}>
				<svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" aria-hidden="true">
					<path d="M12 5v14M5 12h14" />
				</svg>
				{t('widgets.clock.settings.add')}
			</button>
		{/if}
	</fieldset>
</div>

<style>
	.clock-settings {
		display: flex;
		flex-direction: column;
		gap: 16px;
		font-family: var(--k-font-ui);
		color: var(--k-text);
	}
	fieldset {
		margin: 0;
		padding: 0;
		border: 0;
		display: flex;
		flex-direction: column;
		gap: 8px;
		min-width: 0;
	}
	legend {
		padding: 0;
		margin-bottom: 8px;
		font-size: 12px;
		font-weight: 600;
		text-transform: uppercase;
		letter-spacing: 0.08em;
		color: var(--k-text-muted);
	}
	.hint {
		margin: 0;
		font-size: 12px;
		color: var(--k-text-muted);
	}
	.zone-row {
		display: flex;
		align-items: center;
		gap: 8px;
		min-width: 0;
	}
	.zone {
		flex: 1;
		min-width: 0;
		font-size: 13px;
		display: flex;
		flex-direction: column;
		gap: 2px;
	}
	.off {
		font: 11px var(--k-font-mono);
		color: var(--k-text-muted);
		white-space: nowrap;
		overflow: hidden;
		text-overflow: ellipsis;
		min-width: 0;
	}
	.label-field {
		flex: 1;
		min-width: 0;
		display: flex;
	}
	.field {
		flex: 1;
		min-width: 0;
		height: 44px;
		box-sizing: border-box;
		padding: 0 12px;
		border-radius: 10px;
		border: 1px solid var(--k-border);
		background: var(--k-bg-alt);
		color: var(--k-text);
		font: 13px var(--k-font-ui);
	}
	.btn {
		height: 44px;
		padding: 0 14px;
		border-radius: 12px;
		font: 600 13px var(--k-font-ui);
		cursor: pointer;
		white-space: nowrap;
	}
	.ghost {
		border: 1px solid var(--k-border);
		background: transparent;
		color: var(--k-text);
	}
	.add {
		align-self: flex-start;
		display: inline-flex;
		align-items: center;
		gap: 6px;
	}
	.btn:disabled {
		opacity: 0.55;
		cursor: default;
	}
	.icon-btn {
		width: 44px;
		height: 44px;
		flex-shrink: 0;
		display: flex;
		align-items: center;
		justify-content: center;
		border-radius: 12px;
		border: 1px solid var(--k-border);
		background: transparent;
		color: var(--k-text-muted);
		cursor: pointer;
	}
	.sr-only {
		position: absolute;
		width: 1px;
		height: 1px;
		overflow: hidden;
		clip-path: inset(50%);
		white-space: nowrap;
	}
</style>
