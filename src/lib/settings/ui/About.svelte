<script lang="ts">
	/**
	 * About: version, updates (ARCHITECTURE "Auto-update": check, install on click only), license, where
	 * files live (no telemetry, SPEC §1).
	 */
	import { onMount } from 'svelte';
	import { locale, t, tCode } from '#lib/i18n/index.ts';
	import { on, openLogsFolder, openReportIssue, updateCheck, updateInstall, updateStatus, type UpdateStatus } from '#lib/ipc/index.ts';
	import { resetPatch } from '#lib/onboarding/index.ts';
	import Icon from '#lib/ui/Icon.svelte';
	import { errText, getSettingsContext } from './context.svelte';
	import Head from './Head.svelte';
	import Switch from './Switch.svelte';
	import { canInstall, updateBusy } from './lan';

	const { env, store, ui, save } = getSettingsContext();

	let update = $state<UpdateStatus | null>(null);
	let requesting = $state(false);
	const updates = $derived(store.settings.updates);
	const phase = $derived(update?.state ?? 'idle');
	const unsupported = $derived(phase === 'unsupported');
	const busy = $derived(requesting || updateBusy(phase));
	const percent = $derived(Math.round(Math.min(1, Math.max(0, update?.progress ?? 0)) * 100));
	const fmtChecked = new Intl.DateTimeFormat(locale, { dateStyle: 'medium', timeStyle: 'short' });

	onMount(() => {
		if (!env.tauri) return;
		let alive = true;
		updateStatus()
			.then((s) => alive && (update = s))
			.catch(() => {});
		const off = on('update://status', (s) => (update = s));
		return () => {
			alive = false;
			void off.then((f) => f());
		};
	});

	const stateText = $derived.by(() => {
		if (!env.tauri) return t('settings.common.desktopOnly');
		switch (phase) {
			case 'checking':
				return t('update.state.checking');
			case 'available':
				return t('update.state.available', { version: update?.available?.version ?? '' });
			case 'downloading':
				return t('update.state.downloading', { version: update?.available?.version ?? '' });
			case 'ready':
				return t('update.state.ready');
			case 'error':
				return t('update.state.error', { message: tCode('update.errors', update?.errorCode, update?.error ?? '') });
			case 'unsupported':
				return t('update.state.unsupported');
			default:
				return update?.lastChecked ? t('update.state.upToDate') : t('update.state.never');
		}
	});

	async function check() {
		requesting = true;
		try {
			update = await updateCheck();
		} catch (e) {
			ui.say(t('update.checkFailed', { message: errText(e) }), 'warning', 6000);
		} finally {
			requesting = false;
		}
	}

	async function install() {
		requesting = true;
		try {
			await updateInstall();
		} catch (e) {
			ui.say(t('update.installFailed', { message: errText(e) }), 'warning', 6000);
		} finally {
			requesting = false;
		}
	}
	async function reportIssue() {
		try {
			await openReportIssue(null);
		} catch (e) {
			ui.say(t('settings.about.actionFailed', { message: errText(e) }), 'warning', 6000);
		}
	}

	async function openLogs() {
		try {
			await openLogsFolder();
		} catch (e) {
			ui.say(t('settings.about.actionFailed', { message: errText(e) }), 'warning', 6000);
		}
	}

	async function runSetupAgain() {
		// The deck reacts to settings://changed and opens onboarding right away.
		if (await save(resetPatch(store.settings.onboarding ?? { completed: true, version: 0 }))) {
			ui.say(t('settings.about.setupAgainDone'));
		}
	}

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
			<span class="s-value">v{update?.current ?? env.version}</span>
		</div>
	</div>
	<div class="s-group">
		<h2 class="s-eyebrow">{t('update.title')}</h2>
		<div class="s-card upd" aria-live="polite">
			<div class="s-row">
				<div class="s-label">
					<span class="s-title strong">{stateText}</span>
					<span class="s-help">
						{#if update?.lastChecked}
							{t('update.lastChecked')} <span class="k-mono">{fmtChecked.format(update.lastChecked)}</span>
						{:else}
							{t('update.current')} <span class="k-mono">v{update?.current ?? env.version}</span>
						{/if}
					</span>
				</div>
				<div class="s-inline actions">
					<button type="button" class="k-btn s-btn-sm" disabled={!env.tauri || unsupported || busy} onclick={check}>
						<Icon name="refresh" size={16} />{t('update.check')}
					</button>
					{#if canInstall(phase) || phase === 'downloading'}
						<button type="button" class="k-btn k-btn-primary s-btn-sm" disabled={busy} onclick={install}>
							<Icon name="download" size={16} />{phase === 'ready' ? t('update.restart') : t('update.install')}
						</button>
					{/if}
				</div>
			</div>
			{#if phase === 'downloading'}
				<div class="progress">
					<div
						class="track"
						role="progressbar"
						aria-label={t('update.progress')}
						aria-valuemin="0"
						aria-valuemax="100"
						aria-valuenow={percent}
					>
						<div class="fill" style:width="{percent}%"></div>
					</div>
					<span class="k-mono pct">{percent}%</span>
				</div>
			{/if}
			{#if update?.available?.notes && canInstall(phase)}
				<p class="s-help notes">{update.available.notes}</p>
			{/if}
		</div>
		<div class="s-card s-row" class:s-disabled={!env.tauri || unsupported}>
			<div class="s-label">
				<span class="s-title">{t('update.autoCheck')}</span>
				<span class="s-help" id="k-upd-auto">{t('update.autoCheckHelp')}</span>
			</div>
			<Switch
				label={t('update.autoCheck')}
				describedby="k-upd-auto"
				checked={updates.autoCheck}
				disabled={!env.tauri || unsupported}
				onchange={(v) => void save({ updates: { ...updates, autoCheck: v } })}
			/>
		</div>
	</div>
	<div class="s-group">
		<h2 class="s-eyebrow">{t('settings.about.support')}</h2>
		<div class="s-card s-row" class:s-disabled={!env.tauri}>
			<div class="s-label">
				<span class="s-title">{t('settings.about.reportIssue')}</span>
				<span class="s-help">{env.tauri ? t('settings.about.reportIssueHelp') : t('settings.common.desktopOnly')}</span>
			</div>
			<div class="s-inline actions">
				<button type="button" class="k-btn s-btn-sm" disabled={!env.tauri} onclick={openLogs}>
					<Icon name="folder" size={16} />{t('settings.about.openLogs')}
				</button>
				<button type="button" class="k-btn s-btn-sm" disabled={!env.tauri} onclick={reportIssue}>
					<Icon name="bug" size={16} />{t('settings.about.reportIssue')}
				</button>
			</div>
		</div>
		<div class="s-card s-row" class:s-disabled={!env.tauri}>
			<div class="s-label">
				<span class="s-title">{t('settings.about.setupAgain')}</span>
				<span class="s-help">{t('settings.about.setupAgainHelp')}</span>
			</div>
			<button type="button" class="k-btn s-btn-sm" disabled={!env.tauri} onclick={runSetupAgain}>
				<Icon name="refresh" size={16} />{t('settings.about.setupAgain')}
			</button>
		</div>
	</div>
	<div class="s-group">
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
	.upd {
		display: flex;
		flex-direction: column;
		gap: 12px;
	}
	.strong {
		font-weight: 600;
	}
	.actions {
		flex: none;
		flex-wrap: nowrap;
	}
	.progress {
		display: flex;
		align-items: center;
		gap: 12px;
	}
	.track {
		flex: 1;
		height: 6px;
		border-radius: 999px;
		background: var(--k-bg-alt);
		overflow: hidden;
	}
	.fill {
		height: 100%;
		border-radius: inherit;
		background: var(--k-accent);
		transition: width 200ms ease;
	}
	@media (prefers-reduced-motion: reduce) {
		.fill {
			transition: none;
		}
	}
	.pct {
		font-size: 12px;
		min-width: 40px;
		text-align: right;
	}
	.notes {
		margin: 0;
		white-space: pre-line;
		max-height: 96px;
		overflow: hidden;
	}
</style>
