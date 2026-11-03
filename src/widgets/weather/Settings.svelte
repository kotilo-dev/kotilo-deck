<script lang="ts">
	import { t } from '#lib/i18n/index.ts';
	import { isTauri, weatherGeocode, type Place } from '#lib/ipc/index.ts';
	import type { WidgetSettingsProps } from '../types';
	import { asLocation, fromPlace, placeLine, validCoords } from './logic';
	import { defaults, type WeatherSettings } from './settings';

	let { settings, onchange }: WidgetSettingsProps<WeatherSettings> = $props();

	const s = $derived({ ...defaults, ...settings });
	const current = $derived(asLocation(s.location));
	const tauri = isTauri();
	const uid = Math.random().toString(36).slice(2, 8);

	let query = $state('');
	let results = $state<Place[]>([]);
	let status = $state<'idle' | 'searching' | 'done' | 'failed'>('idle');
	let debounce: ReturnType<typeof setTimeout> | null = null;
	let seq = 0;

	function onQuery() {
		if (debounce) clearTimeout(debounce);
		const q = query.trim();
		if (q.length < 2 || !tauri) {
			results = [];
			status = 'idle';
			return;
		}
		debounce = setTimeout(() => search(q), 350);
	}

	async function search(q: string) {
		const mine = ++seq;
		status = 'searching';
		try {
			const found = await weatherGeocode(q);
			if (mine !== seq) return;
			results = found;
			status = 'done';
		} catch {
			if (mine !== seq) return;
			results = [];
			status = 'failed';
		}
	}

	$effect(() => () => {
		if (debounce) clearTimeout(debounce);
		seq++;
	});

	function choose(p: Place) {
		onchange({ ...s, location: fromPlace(p) });
		query = '';
		results = [];
		status = 'idle';
	}

	let lat = $state('');
	let lon = $state('');
	let label = $state('');
	const parsed = $derived({ lat: Number(lat.replace(',', '.')), lon: Number(lon.replace(',', '.')) });
	const coordsOk = $derived(lat.trim() !== '' && lon.trim() !== '' && validCoords(parsed.lat, parsed.lon));

	function useCoords(e: SubmitEvent) {
		e.preventDefault();
		if (!coordsOk) return;
		const loc = asLocation({ latitude: parsed.lat, longitude: parsed.lon, name: label, country: '', admin1: null });
		if (loc) onchange({ ...s, location: loc });
	}
</script>

<div class="wx-settings">
	<fieldset>
		<legend>{t('widgets.weather.settings.location')}</legend>
		<div class="current">
			<span class="current-text">
				{current ? t('widgets.weather.settings.current', { name: placeLine(current) }) : t('widgets.weather.settings.none')}
				{#if current}
					<span class="mono muted">{current.latitude.toFixed(2)}, {current.longitude.toFixed(2)}</span>
				{/if}
			</span>
			{#if current}
				<button type="button" class="btn ghost" onclick={() => onchange({ ...s, location: null })}>
					{t('widgets.weather.settings.clear')}
				</button>
			{/if}
		</div>

		<label class="lbl" for="wx-q-{uid}">{t('widgets.weather.settings.search')}</label>
		<input
			id="wx-q-{uid}"
			class="field"
			type="search"
			autocomplete="off"
			placeholder={t('widgets.weather.settings.searchPlaceholder')}
			disabled={!tauri}
			bind:value={query}
			oninput={onQuery}
		/>
		{#if !tauri}
			<p class="hint">{t('widgets.weather.settings.browserPreview')}</p>
		{:else if status === 'searching'}
			<p class="hint" aria-live="polite">{t('widgets.weather.settings.searching')}</p>
		{:else if status === 'failed'}
			<p class="hint" aria-live="polite">{t('widgets.weather.settings.searchFailed')}</p>
		{:else if status === 'done' && results.length === 0}
			<p class="hint" aria-live="polite">{t('widgets.weather.settings.noResults')}</p>
		{/if}
		{#if results.length}
			<ul class="results">
				{#each results as p (`${p.latitude},${p.longitude}`)}
					<li>
						<button type="button" class="result" onclick={() => choose(p)}>
							<span class="result-name">{placeLine(p)}</span>
							<span class="mono muted">{p.latitude.toFixed(2)}, {p.longitude.toFixed(2)}</span>
						</button>
					</li>
				{/each}
			</ul>
		{/if}
	</fieldset>

	<form class="manual" onsubmit={useCoords}>
		<fieldset>
			<legend>{t('widgets.weather.settings.manual')}</legend>
			<div class="coords">
				<label class="lbl">
					{t('widgets.weather.settings.latitude')}
					<input class="field mono" type="text" inputmode="decimal" placeholder="41.01" bind:value={lat} />
				</label>
				<label class="lbl">
					{t('widgets.weather.settings.longitude')}
					<input class="field mono" type="text" inputmode="decimal" placeholder="28.95" bind:value={lon} />
				</label>
			</div>
			<label class="lbl">
				{t('widgets.weather.settings.name')}
				<input class="field" type="text" bind:value={label} />
			</label>
			{#if lat.trim() && lon.trim() && !coordsOk}
				<p class="hint">{t('widgets.weather.settings.invalidCoords')}</p>
			{/if}
			<button type="submit" class="btn ghost" disabled={!coordsOk}>{t('widgets.weather.settings.useCoords')}</button>
		</fieldset>
	</form>
</div>

<style>
	.wx-settings {
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
	.current {
		display: flex;
		align-items: center;
		gap: 8px;
	}
	.current-text {
		flex: 1;
		min-width: 0;
		display: flex;
		flex-direction: column;
		gap: 2px;
		font-size: 13px;
	}
	.mono {
		font-family: var(--k-font-mono);
	}
	.muted {
		font-size: 11px;
		color: var(--k-text-muted);
	}
	.lbl {
		display: flex;
		flex-direction: column;
		gap: 4px;
		font-size: 12px;
		color: var(--k-text-muted);
		min-width: 0;
		flex: 1;
	}
	.field {
		height: 44px;
		box-sizing: border-box;
		padding: 0 12px;
		border-radius: 10px;
		border: 1px solid var(--k-border);
		background: var(--k-bg-alt);
		color: var(--k-text);
		font-size: 13px;
		min-width: 0;
	}
	.field:not(.mono) {
		font-family: var(--k-font-ui);
	}
	.field:disabled {
		opacity: 0.55;
	}
	.hint {
		margin: 0;
		font-size: 12px;
		color: var(--k-text-muted);
	}
	.results {
		margin: 0;
		padding: 4px;
		list-style: none;
		max-height: 264px;
		overflow-y: auto;
		border: 1px solid var(--k-border);
		border-radius: 10px;
		background: var(--k-bg-alt);
	}
	.result {
		width: 100%;
		min-height: 44px;
		display: flex;
		align-items: center;
		justify-content: space-between;
		gap: 12px;
		padding: 0 10px;
		border: 0;
		border-radius: 8px;
		background: transparent;
		color: var(--k-text);
		font: 13px var(--k-font-ui);
		text-align: left;
		cursor: pointer;
	}
	.result:hover {
		background: var(--k-surface);
	}
	.result-name {
		min-width: 0;
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}
	.coords {
		display: flex;
		gap: 8px;
	}
	.btn {
		align-self: flex-start;
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
	.btn:disabled {
		opacity: 0.55;
		cursor: default;
	}
</style>
