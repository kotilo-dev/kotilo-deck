<script lang="ts">
	/** About: version, license, where files live (no telemetry, SPEC §1). */
	import { t } from '#lib/i18n/index.ts';
	import { getSettingsContext } from './context.svelte';
	import Head from './Head.svelte';

	const { env } = getSettingsContext();
	const rows = $derived([
		{ label: t('settings.about.settings'), path: env.settingsPath },
		{ label: t('settings.about.workspaces'), path: env.workspacesPath },
		{ label: t('settings.about.wallpapers'), path: env.wallpapersPath },
		{ label: t('settings.about.themes'), path: env.themesPath },
		{ label: t('settings.about.logs'), path: env.logsPath }
	]);
</script>

<section class="s-page">
	<Head title={t('settings.about.title')} text={t('settings.about.text')} />
	<div class="s-group">
		<div class="s-card s-row">
			<div class="s-label">
				<span class="s-title">{t('shell.appName')}</span>
				<span class="s-help">{t('settings.about.tagline')}</span>
			</div>
			<span class="s-value">v{env.version}</span>
		</div>
		<div class="s-card s-row">
			<div class="s-label">
				<span class="s-title">{t('settings.about.license')}</span>
				<span class="s-help">{t('settings.about.licenseText')}</span>
			</div>
			<span class="s-badge">MIT</span>
		</div>
	</div>
	<div class="s-group">
		<h2 class="s-eyebrow">{t('settings.about.files')}</h2>
		<dl class="s-card files">
			{#each rows as r (r.label)}
				<div class="file">
					<dt class="s-help">{r.label}</dt>
					<dd class="s-path">{r.path}</dd>
				</div>
			{/each}
		</dl>
		<p class="s-note small">{t('settings.about.privacy')}</p>
	</div>
</section>

<style>
	.files {
		margin: 0;
		display: flex;
		flex-direction: column;
		gap: 10px;
	}
	.file {
		display: grid;
		grid-template-columns: 140px minmax(0, 1fr);
		gap: 12px;
		align-items: baseline;
	}
	dd {
		margin: 0;
		user-select: text;
	}
	.small {
		font-size: 12px;
	}
</style>
