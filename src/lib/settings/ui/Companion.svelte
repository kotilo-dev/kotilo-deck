<script lang="ts">
	/** Companion (Phase 3): muted placeholder; the deck works fully without the Kotilo desktop app. */
	import { t } from '#lib/i18n/index.ts';
	import Head from './Head.svelte';
</script>

<section class="s-page">
	<Head title={t('settings.companion.title')} text={t('settings.companion.text')} />
	<div class="s-card s-row s-disabled" aria-disabled="true">
		<div class="s-label">
			<span class="s-title">{t('settings.companion.status')}</span>
			<span class="s-help">{t('settings.companion.statusOff')}</span>
		</div>
	</div>
	<p class="s-note">{t('settings.common.later')}</p>
</section>
