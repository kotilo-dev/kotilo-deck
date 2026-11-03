<script lang="ts">
	/** Whole-number field with 44 px − / + buttons (touch friendly); commits on change, clamped to min–max. */
	import Icon from '#lib/ui/Icon.svelte';
	import { t } from '#lib/i18n/index.ts';

	let {
		id,
		value,
		min,
		max,
		label,
		unit,
		describedby,
		onchange
	}: {
		id: string;
		value: number;
		min: number;
		max: number;
		label: string;
		/** Short unit shown after the number, e.g. "min". */
		unit?: string;
		describedby?: string;
		onchange: (next: number) => void;
	} = $props();

	const clamp = (n: number) => Math.min(max, Math.max(min, Math.round(n)));

	function commit(raw: number, input?: HTMLInputElement) {
		const next = Number.isFinite(raw) ? clamp(raw) : value;
		if (input) input.value = String(next);
		if (next !== value) onchange(next);
	}
</script>

<div class="stepper">
	<button type="button" class="k-btn k-btn-icon" aria-label={t('settings.common.decrease', { name: label })} disabled={value <= min} onclick={() => commit(value - 1)}>
		<svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" aria-hidden="true"><path d="M5 12h14" /></svg>
	</button>
	<input
		{id}
		class="k-input k-mono num"
		type="number"
		inputmode="numeric"
		{min}
		{max}
		step="1"
		{value}
		aria-describedby={describedby}
		onchange={(e) => commit(e.currentTarget.valueAsNumber, e.currentTarget)}
	/>
	{#if unit}<span class="unit" aria-hidden="true">{unit}</span>{/if}
	<button type="button" class="k-btn k-btn-icon" aria-label={t('settings.common.increase', { name: label })} disabled={value >= max} onclick={() => commit(value + 1)}>
		<Icon name="plus" size={16} />
	</button>
</div>

<style>
	.stepper {
		display: inline-flex;
		align-items: center;
		gap: 6px;
		flex: none;
	}
	.num {
		width: 72px;
		text-align: center;
		-moz-appearance: textfield;
		appearance: textfield;
	}
	.num::-webkit-outer-spin-button,
	.num::-webkit-inner-spin-button {
		-webkit-appearance: none;
		margin: 0;
	}
	.unit {
		min-width: 28px;
		font-size: 12px;
		color: var(--k-text-muted);
	}
	.k-btn-icon:disabled {
		opacity: 0.55;
		cursor: default;
	}
</style>
