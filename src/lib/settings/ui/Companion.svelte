<script lang="ts">
	/**
	 * Companion (SPEC §6, protocol.md): server on/off, live status, what the desktop may do, and where the
	 * shared token lives (path only; the token itself is never shown). The deck works fully without it.
	 */
	import { t } from '#lib/i18n/index.ts';
	import { companionView } from '#lib/ui/companion.svelte.ts';
	import { getSettingsContext } from './context.svelte';
	import Head from './Head.svelte';
	import Switch from './Switch.svelte';

	const { store, ui, env, save } = getSettingsContext();
	/** protocol.md §1 default, shown until the backend reports its port. */
	const DEFAULT_PORT = 47831;

	const c = $derived(store.settings.companion);
	const status = $derived(ui.companion.status);
	const view = $derived(companionView(c.enabled, status));
	const port = $derived(status?.port ?? DEFAULT_PORT);

	const set = (patch: Partial<typeof c>) => void save({ companion: { ...c, ...patch } });

	const headline = $derived.by(() => {
		if (view === 'connected')
			return status?.client ? t('settings.companion.connectedVersion', { version: status.client }) : t('settings.companion.connected');
		return t(`settings.companion.view.${view}`);
	});
	const detail = $derived.by(() => {
		if (!env.tauri) return t('settings.common.desktopOnly');
		if (view === 'off') return t('settings.companion.offHelp');
		if (view === 'error') return t('settings.companion.errorHelp', { message: status?.error ?? '' });
		if (view === 'waiting') return t('settings.companion.waitingHelp');
		return t('settings.companion.connectedHelp');
	});
</script>

<section class="s-page">
	<Head title={t('settings.companion.title')} text={t('settings.companion.text')} />

	<div class="s-group">
		<h2 class="s-eyebrow">{t('settings.companion.server')}</h2>
		<div class="s-card s-row">
			<div class="s-label">
				<span class="s-title">{t('settings.companion.enable')}</span>
				<span class="s-help" id="k-cp-enable">{t('settings.companion.enableHelp')}</span>
			</div>
			<Switch label={t('settings.companion.enable')} describedby="k-cp-enable" checked={c.enabled} onchange={(v) => set({ enabled: v })} />
		</div>

		<div class="s-card status" aria-live="polite">
			<div class="head">
				<span class="dot" class:on={view === 'connected'} aria-hidden="true"></span>
				<div class="s-label">
					<span class="s-title strong">{headline}</span>
					<span class="s-help" class:error={view === 'error'}>{detail}</span>
				</div>
			</div>
			<dl class="facts">
				<div>
					<dt>{t('settings.companion.listening')}</dt>
					<dd>
						{#if status?.listening}
							<span class="k-mono">127.0.0.1:{port}</span>
						{:else}
							{t('settings.companion.notListening')} · <span class="k-mono">{port}</span>
						{/if}
					</dd>
				</div>
				<div>
					<dt>{t('settings.companion.client')}</dt>
					<dd>
						{#if status?.connected}
							{t('shell.topbar.companionOn')}{#if status.client}&nbsp;<span class="k-mono">{status.client}</span>{/if}
						{:else}
							{t('settings.companion.noClient')}
						{/if}
					</dd>
				</div>
			</dl>
		</div>
	</div>

	<div class="s-group" class:s-disabled={!c.enabled}>
		<h2 class="s-eyebrow">{t('settings.companion.permissions')}</h2>
		<div class="s-card s-row">
			<div class="s-label">
				<span class="s-title">{t('settings.companion.allowSwitch')}</span>
				<span class="s-help" id="k-cp-switch">{t('settings.companion.allowSwitchHelp')}</span>
			</div>
			<Switch
				label={t('settings.companion.allowSwitch')}
				describedby="k-cp-switch"
				checked={c.allowWorkspaceSwitch}
				disabled={!c.enabled}
				onchange={(v) => set({ allowWorkspaceSwitch: v })}
			/>
		</div>
		<div class="s-card s-row">
			<div class="s-label">
				<span class="s-title">{t('settings.companion.notify')}</span>
				<span class="s-help" id="k-cp-notify">{t('settings.companion.notifyHelp')}</span>
			</div>
			<Switch
				label={t('settings.companion.notify')}
				describedby="k-cp-notify"
				checked={c.notifyApprovals}
				disabled={!c.enabled}
				onchange={(v) => set({ notifyApprovals: v })}
			/>
		</div>
	</div>

	<div class="s-group">
		<h2 class="s-eyebrow">{t('settings.companion.pairing')}</h2>
		<div class="s-card s-stack">
			<span class="s-title">{t('settings.companion.auto')}</span>
			<p class="s-help how">{t('settings.companion.autoHelp')}</p>
			<label class="s-stack token">
				<span class="s-help">{t('settings.companion.tokenFile')}</span>
				<input class="k-input k-mono path" readonly value={env.tokenPath} spellcheck="false" />
			</label>
			<p class="s-help">{t('settings.companion.tokenSecret')}</p>
		</div>
	</div>
</section>

<style>
	.status {
		display: flex;
		flex-direction: column;
		gap: 12px;
	}
	.head {
		display: flex;
		align-items: flex-start;
		gap: 12px;
	}
	.dot {
		flex: none;
		width: 8px;
		height: 8px;
		margin-top: 5px;
		border-radius: 50%;
		background: var(--k-border);
	}
	.dot.on {
		background: var(--k-success);
	}
	.strong {
		font-weight: 600;
	}
	.facts {
		margin: 0;
		display: grid;
		grid-template-columns: repeat(2, minmax(0, 1fr));
		gap: 10px;
	}
	.facts > div {
		padding: 10px 12px;
		border-radius: 10px;
		background: var(--k-bg-alt);
		min-width: 0;
	}
	dt {
		font-size: 11px;
		text-transform: uppercase;
		letter-spacing: 0.08em;
		color: var(--k-text-muted);
	}
	dd {
		margin: 4px 0 0;
		font-size: 13px;
		overflow-wrap: anywhere;
	}
	.how {
		margin: 0;
		font-size: 12px;
	}
	.token {
		gap: 4px;
	}
	.path {
		font-size: 12px;
		color: var(--k-text-muted);
	}
</style>
