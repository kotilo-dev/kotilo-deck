<script lang="ts">
	/**
	 * Settings ▸ Widgets (docs/plugin-api.md): installed external widgets (valid and rejected), the
	 * widgets folder and how to add one. Live on `plugin://changed` through the shared registry.
	 */
	import { isTauri, pluginOpenFolder, type PluginWidget } from '#lib/ipc/index.ts';
	import { t } from '#lib/i18n/index.ts';
	import Icon from '#lib/ui/Icon.svelte';
	import { plugins } from '#lib/plugins/registry.svelte.ts';
	import { errText, getSettingsContext } from './context.svelte';
	import Head from './Head.svelte';

	const { env } = getSettingsContext();
	plugins.start();

	const entries = $derived([...plugins.entries].sort((a, b) => a.folder.localeCompare(b.folder)));
	const valid = $derived(entries.filter((p) => p.manifest && !p.error).length);
	/** Same root as the workspaces folder (`~/.kotilo/deck/`), real path once SettingsEnv resolved it. */
	const folderPath = $derived(env.workspacesPath.replace(/workspaces[\\/]?$/, 'widgets'));
	let openError = $state<string | null>(null);

	const sizeLabel = (s: string) => s.replace('x', '×');
	const meta = (p: PluginWidget) =>
		[p.manifest?.version ? `v${p.manifest.version}` : null, p.manifest?.author ? t('plugins.page.by', { author: p.manifest.author }) : null]
			.filter(Boolean)
			.join(' · ');

	async function openFolder() {
		openError = null;
		try {
			await pluginOpenFolder();
		} catch (e) {
			openError = errText(e);
		}
	}
</script>

<section class="s-page">
	<Head title={t('plugins.page.title')} text={t('plugins.page.text')} />

	<div class="s-group">
		<h2 class="s-eyebrow">
			{t('plugins.page.installed')}
			{#if plugins.loaded && entries.length}<span class="count k-mono">{valid}/{entries.length}</span>{/if}
		</h2>
		{#if plugins.error}
			<p class="s-help error" role="alert">{t('plugins.page.loadError', { message: plugins.error })}</p>
		{/if}
		{#if !plugins.loaded}
			<div class="s-card skeleton" aria-busy="true" aria-label={t('plugins.page.loading')}></div>
		{:else if entries.length === 0}
			<p class="s-card empty">{t('plugins.page.empty')}</p>
		{:else}
			<ul class="s-list" aria-live="polite">
				{#each entries as p (p.id)}
					{@const m = p.manifest}
					{@const bad = !!p.error || !m}
					<li class="s-card plugin" class:bad>
						<span class="tile" aria-hidden="true"><Icon name={bad ? 'alert' : (m?.icon ?? 'puzzle')} size={18} /></span>
						<div class="s-label body">
							<div class="line">
								<span class="name">{m?.name ?? p.folder}</span>
								<span class="s-path">{p.id}</span>
								{#if !bad && meta(p)}<span class="s-help">{meta(p)}</span>{/if}
							</div>
							{#if bad}
								<span class="s-help error">{p.error ?? t('plugins.page.noManifest')}</span>
							{:else if m}
								{#if m.description}<span class="s-help">{m.description}</span>{/if}
								<div class="facts">
									<span class="s-help">{t('plugins.page.sizes')}</span>
									{#each m.sizes as s (s)}<span class="s-badge k-mono">{sizeLabel(s)}</span>{/each}
									<span class="s-help gap">{t('plugins.page.topics')}</span>
									{#each m.topics as topic (topic)}
										<span class="s-badge">{t(`plugins.topics.${topic}`)}</span>
									{:else}
										<span class="s-help">{t('plugins.page.noTopics')}</span>
									{/each}
								</div>
							{/if}
						</div>
					</li>
				{/each}
			</ul>
		{/if}
	</div>

	<div class="s-group">
		<h2 class="s-eyebrow">{t('plugins.page.add')}</h2>
		<div class="s-card s-stack">
			<p class="s-help how">{t('plugins.page.addHelp')}</p>
			<label class="s-stack path-field">
				<span class="s-help">{t('plugins.page.folder')}</span>
				<input class="k-input k-mono path" readonly value={folderPath} spellcheck="false" />
			</label>
			<p class="s-help how">{t('plugins.page.sandbox')}</p>
			<p class="s-help how">{t('plugins.page.docs')} <span class="s-path">docs/plugin-api.md</span></p>
			{#if isTauri()}
				<div class="s-inline">
					<button type="button" class="k-btn" onclick={openFolder}>
						<Icon name="folder" size={16} />{t('plugins.page.openFolder')}
					</button>
				</div>
				{#if openError}<p class="s-help error" role="alert">{openError}</p>{/if}
			{:else}
				<p class="s-help">{t('settings.common.desktopOnly')}</p>
			{/if}
		</div>
	</div>
</section>

<style>
	.count {
		margin-left: 6px;
		font-weight: 400;
		letter-spacing: 0;
	}
	.skeleton {
		height: 64px;
		background: var(--k-bg-alt);
	}
	.empty {
		margin: 0;
		font-size: 13px;
		color: var(--k-text-muted);
	}
	.plugin {
		display: flex;
		align-items: flex-start;
		gap: 12px;
	}
	.plugin.bad {
		border-color: color-mix(in srgb, var(--k-danger) 50%, var(--k-border));
	}
	.tile {
		flex: none;
		width: 36px;
		height: 36px;
		border-radius: 10px;
		background: var(--k-bg-alt);
		color: var(--k-accent);
		display: flex;
		align-items: center;
		justify-content: center;
	}
	.bad .tile {
		color: var(--k-danger);
	}
	.body {
		flex: 1;
		gap: 4px;
	}
	.line {
		display: flex;
		align-items: baseline;
		flex-wrap: wrap;
		gap: 4px 10px;
		min-width: 0;
	}
	.name {
		font-size: 13px;
		font-weight: 600;
	}
	.facts {
		display: flex;
		align-items: center;
		flex-wrap: wrap;
		gap: 6px;
		margin-top: 2px;
	}
	.gap {
		margin-left: 8px;
	}
	.how {
		margin: 0;
		font-size: 12px;
	}
	.path-field {
		gap: 4px;
	}
	.path {
		font-size: 12px;
		color: var(--k-text-muted);
	}
</style>
