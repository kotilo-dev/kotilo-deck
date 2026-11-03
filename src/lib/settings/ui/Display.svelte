<script lang="ts">
	/** Display & monitor: which monitor the deck lives on, fullscreen vs overlay (SPEC §2, §5.3). */
	import { monitorSelect } from '#lib/ipc/index.ts';
	import { t } from '#lib/i18n/index.ts';
	import Icon from '#lib/ui/Icon.svelte';
	import { errText, getSettingsContext } from './context.svelte';
	import Head from './Head.svelte';
	import Segmented from './Segmented.svelte';

	const { store, ui, env, save } = getSettingsContext();
	let selecting = $state<string | null>(null);

	/** Same rule as the backend: pinned name, else first non-primary, else primary. */
	const current = $derived(
		env.monitors.find((m) => m.name === store.settings.monitor) ?? env.monitors.find((m) => !m.primary) ?? env.monitors[0]
	);

	async function select(name: string) {
		selecting = name;
		try {
			await monitorSelect(name);
			await store.reloadSettings();
			ui.say(t('settings.display.moved', { name }));
		} catch (e) {
			ui.say(t('settings.common.saveFailed', { message: errText(e) }), 'warning', 6000);
		} finally {
			selecting = null;
		}
	}
</script>

<section class="s-page">
	<Head title={t('settings.display.title')} text={t('settings.display.text')} />

	<div class="s-group">
		<div class="s-inline between">
			<h2 class="s-eyebrow">{t('settings.display.monitor')}</h2>
			{#if env.tauri}
				<button type="button" class="k-btn s-btn-sm" onclick={() => env.loadMonitors()}>
					<Icon name="refresh" size={16} />{t('settings.display.refresh')}
				</button>
			{/if}
		</div>
		{#if !env.tauri}
			<p class="s-card s-note">{t('settings.common.desktopOnly')}</p>
		{:else if env.monitorsLoaded && env.monitors.length === 0}
			<p class="s-card s-note">{t('settings.display.none')}</p>
		{:else}
			<div class="monitors" role="radiogroup" aria-label={t('settings.display.monitor')}>
				{#each env.monitors as m (m.name)}
					{@const on = current?.name === m.name}
					<button
						type="button"
						role="radio"
						aria-checked={on}
						class="monitor"
						disabled={selecting !== null}
						onclick={() => !on && select(m.name)}
					>
						<span class="screen" aria-hidden="true" style:aspect-ratio="{m.width} / {m.height}"></span>
						<span class="s-label">
							<span class="s-title">{m.name}</span>
							<span class="s-help k-mono">{m.width} × {m.height} · {Math.round(m.scaleFactor * 100)}%</span>
						</span>
						<span class="badges">
							{#if m.primary}<span class="s-badge">{t('settings.display.primary')}</span>{/if}
							{#if on}<span class="s-badge active">{store.settings.monitor ? t('settings.display.pinned') : t('settings.display.auto')}</span>{/if}
						</span>
					</button>
				{/each}
			</div>
			<p class="s-note small">{t('settings.display.rule')}</p>
		{/if}
	</div>

	<div class="s-group">
		<h2 class="s-eyebrow">{t('settings.display.mode')}</h2>
		<div class="s-card s-row">
			<div class="s-label">
				<span class="s-title">{t('settings.display.modeLabel')}</span>
				<span class="s-help">
					{store.settings.displayMode === 'overlay' ? t('settings.display.overlayHelp') : t('settings.display.fullscreenHelp')}
				</span>
			</div>
			<Segmented
				label={t('settings.display.modeLabel')}
				value={store.settings.displayMode}
				options={[
					{ value: 'fullscreen', label: t('settings.display.fullscreen') },
					{ value: 'overlay', label: t('settings.display.overlay') }
				]}
				onchange={(v) => void save({ displayMode: v })}
			/>
		</div>
	</div>
</section>

<style>
	.between {
		justify-content: space-between;
	}
	.monitors {
		display: grid;
		grid-template-columns: repeat(2, minmax(0, 1fr));
		gap: 12px;
	}
	.monitor {
		box-sizing: border-box;
		min-height: 72px;
		display: flex;
		align-items: center;
		gap: 12px;
		padding: 12px 14px;
		border-radius: 12px;
		border: 1px solid var(--k-border);
		background: var(--k-surface);
		color: var(--k-text);
		text-align: left;
	}
	.monitor[aria-checked='true'] {
		border: 2px solid var(--k-accent);
		padding: 11px 13px;
	}
	.monitor:disabled {
		opacity: 1;
	}
	.screen {
		flex: none;
		width: 44px;
		max-height: 44px;
		border-radius: 4px;
		border: 2px solid var(--k-text-muted);
		background: var(--k-bg);
	}
	.badges {
		margin-left: auto;
		display: flex;
		flex-direction: column;
		align-items: flex-end;
		gap: 4px;
	}
	.s-badge.active {
		border-color: var(--k-accent);
		color: var(--k-text);
	}
	.small {
		font-size: 12px;
	}
</style>
