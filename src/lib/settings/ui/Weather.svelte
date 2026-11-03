<script lang="ts">
	/** Weather & calendar: default units and the ICS calendar feeds (SPEC §7). */
	import { t } from '#lib/i18n/index.ts';
	import CalendarFeeds from './CalendarFeeds.svelte';
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
	<CalendarFeeds />
</section>

<style>
	.small {
		font-size: 12px;
	}
</style>
