<script lang="ts">
	/** Global temperature unit (shared by General and Weather & calendar). */
	import { t } from '#lib/i18n/index.ts';
	import { getSettingsContext } from './context.svelte';
	import Segmented from './Segmented.svelte';

	const { store, save } = getSettingsContext();
</script>

<div class="s-card s-row">
	<div class="s-label">
		<span class="s-title" id="k-unit-label">{t('settings.general.unit')}</span>
		<span class="s-help">{t('settings.general.unitHelp')}</span>
	</div>
	<Segmented
		label={t('settings.general.unit')}
		value={store.settings.temperatureUnit}
		options={[
			{ value: 'c', label: '°C', mono: true },
			{ value: 'f', label: '°F', mono: true }
		]}
		onchange={(v) => void save({ temperatureUnit: v })}
	/>
</div>
