<script lang="ts">
	/** Weather & calendar: default units now; ICS calendar feeds arrive with the calendar widget (Phase 3). */
	import { t } from '#lib/i18n/index.ts';
	import Head from './Head.svelte';
	import UnitControl from './UnitControl.svelte';
</script>

<section class="s-page">
	<Head title={t('settings.weather.title')} text={t('settings.weather.text')} />
	<div class="s-group">
		<h2 class="s-eyebrow">{t('settings.weather.weather')}</h2>
		<UnitControl />
		<p class="s-note small">{t('settings.weather.location')}</p>
	</div>
	<div class="s-group">
		<h2 class="s-eyebrow">{t('settings.weather.calendar')}</h2>
		<div class="s-card s-row s-disabled" aria-disabled="true">
			<div class="s-label">
				<span class="s-title">{t('settings.weather.feeds')}</span>
				<span class="s-help">{t('settings.common.later')}</span>
			</div>
		</div>
	</div>
</section>

<style>
	.small {
		font-size: 12px;
	}
</style>
