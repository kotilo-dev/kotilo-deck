<script lang="ts">
	/**
	 * Settings ▸ Tablet & browser (ARCHITECTURE "Phase 4 ▸ LAN mode"): on/off, port, live server status,
	 * the deck's addresses (token masked until "Show") and a QR code for the first one. Off by default.
	 */
	import { onMount, untrack } from 'svelte';
	import { t, tCode } from '#lib/i18n/index.ts';
	import { lanStatus, on, type LanStatus } from '#lib/ipc/index.ts';
	import Icon from '#lib/ui/Icon.svelte';
	import { getSettingsContext } from './context.svelte';
	import Head from './Head.svelte';
	import Switch from './Switch.svelte';
	import { maskUrl, PORT_MAX, PORT_MIN, portProblem, svgDataUrl } from './lan';

	const { store, ui, env, save } = getSettingsContext();

	const lan = $derived(store.settings.lan);
	let status = $state<LanStatus | null>(null);
	let reveal = $state(false);
	let portText = $state('');
	let savingPort = $state(false);

	// Follow the stored port (initially and when it changes elsewhere); typing never gets overwritten.
	$effect(() => {
		const p = String(lan.port);
		untrack(() => {
			if (!savingPort) portText = p;
		});
	});

	const problem = $derived(portProblem(portText));
	const portDirty = $derived(problem === null && Number(portText.trim()) !== lan.port);
	const problemText = $derived(
		problem === 'number' || problem === 'range'
			? t('lan.portRange', { min: PORT_MIN, max: PORT_MAX })
			: problem === 'companion'
				? t('lan.portCompanion')
				: ''
	);

	onMount(() => {
		if (!env.tauri) return;
		let alive = true;
		lanStatus()
			.then((s) => alive && (status = s))
			.catch(() => {});
		const off = on('lan://status', (s) => (status = s));
		return () => {
			alive = false;
			void off.then((f) => f());
		};
	});

	type View = 'off' | 'starting' | 'listening' | 'error';
	const view = $derived<View>(
		!lan.enabled ? 'off' : status?.listening ? 'listening' : status?.error ? 'error' : 'starting'
	);
	const headline = $derived(
		view === 'listening' ? t('lan.listening', { port: status?.port ?? lan.port }) : t(`lan.view.${view}`)
	);
	const detail = $derived.by(() => {
		if (!env.tauri) return t('settings.common.desktopOnly');
		if (view === 'off') return t('lan.offHelp');
		if (view === 'error') return t('lan.errorHelp', { message: tCode('lan.errors', status?.errorCode, status?.error ?? '') });
		if (view === 'starting') return t('lan.startingHelp');
		return t('lan.listeningHelp');
	});
	const urls = $derived(view === 'listening' ? (status?.urls ?? []) : []);
	const qr = $derived(view === 'listening' && status?.qrSvg ? svgDataUrl(status.qrSvg) : null);

	function setEnabled(enabled: boolean) {
		void save({ lan: { ...lan, enabled } });
	}

	async function savePort() {
		if (!portDirty) return;
		savingPort = true;
		const port = Number(portText.trim());
		const ok = await save({ lan: { ...lan, port } });
		savingPort = false;
		if (ok) ui.say(t('lan.portSaved', { port }));
		else portText = String(lan.port);
	}

	function onPortKey(e: KeyboardEvent) {
		if (e.key === 'Enter') {
			e.preventDefault();
			void savePort();
		} else if (e.key === 'Escape') {
			portText = String(lan.port);
		}
	}

	async function copy(url: string) {
		try {
			await navigator.clipboard.writeText(url);
			ui.say(t('lan.copied'));
		} catch {
			ui.say(t('lan.copyFailed'), 'warning');
		}
	}
</script>

<section class="s-page">
	<Head title={t('lan.title')} text={t('lan.text')} />

	<div class="s-group">
		<h2 class="s-eyebrow">{t('lan.server')}</h2>
		<div class="s-card s-row" class:s-disabled={!env.tauri}>
			<div class="s-label">
				<span class="s-title">{t('lan.enable')}</span>
				<span class="s-help" id="k-lan-enable">{t('lan.enableHelp')}</span>
			</div>
			<Switch label={t('lan.enable')} describedby="k-lan-enable" checked={lan.enabled} disabled={!env.tauri} onchange={setEnabled} />
		</div>

		<div class="s-card s-row" class:s-disabled={!env.tauri}>
			<label class="s-label" for="k-lan-port">
				<span class="s-title">{t('lan.port')}</span>
				<span class="s-help" class:error={problem !== null} id="k-lan-port-help">
					{problem !== null ? problemText : t('lan.portHelp')}
				</span>
			</label>
			<div class="s-inline">
				<input
					id="k-lan-port"
					class="k-input k-mono port"
					type="text"
					inputmode="numeric"
					autocomplete="off"
					spellcheck="false"
					maxlength="5"
					aria-describedby="k-lan-port-help"
					aria-invalid={problem !== null}
					disabled={!env.tauri || savingPort}
					bind:value={portText}
					onkeydown={onPortKey}
				/>
				<button type="button" class="k-btn s-btn-sm" disabled={!env.tauri || !portDirty || savingPort} onclick={savePort}>
					{t('settings.common.save')}
				</button>
			</div>
		</div>

		<div class="s-card status" aria-live="polite">
			<div class="head">
				<span class="dot" class:on={view === 'listening'} class:bad={view === 'error'} aria-hidden="true"></span>
				<div class="s-label">
					<span class="s-title strong">{headline}</span>
					<span class="s-help" class:error={view === 'error'}>{detail}</span>
				</div>
				{#if view === 'listening'}
					<span class="clients">
						<span class="k-mono n">{status?.clients ?? 0}</span>
						<span class="s-help">{t('lan.clients')}</span>
					</span>
				{/if}
			</div>
		</div>
	</div>

	{#if view === 'listening'}
		<div class="s-group">
			<div class="s-row bar">
				<h2 class="s-eyebrow">{t('lan.addresses')}</h2>
				<button type="button" class="k-btn s-btn-sm" aria-pressed={reveal} onclick={() => (reveal = !reveal)}>
					{reveal ? t('lan.hide') : t('lan.show')}
				</button>
			</div>
			<div class="s-card links">
				{#if urls.length === 0}
					<p class="s-help none">{t('lan.noAddresses')}</p>
				{:else}
					<ul class="s-list">
						{#each urls as url, i (url)}
							<li class="url">
								<span class="s-path addr" class:masked={!reveal}>{reveal ? url : maskUrl(url)}</span>
								<button
									type="button"
									class="k-btn k-btn-icon"
									aria-label={t('lan.copy', { n: i + 1 })}
									title={t('lan.copy', { n: i + 1 })}
									onclick={() => copy(url)}
								>
									<Icon name="copy" size={16} />
								</button>
							</li>
						{/each}
					</ul>
				{/if}
				{#if qr}
					<div class="qr">
						{#if reveal}
							<img src={qr} alt={t('lan.qrAlt')} width="168" height="168" />
						{:else}
							<div class="qr-hidden" aria-hidden="true">
								<Icon name="link" size={20} />
							</div>
						{/if}
						<span class="s-help">{reveal ? t('lan.qrHelp') : t('lan.qrHidden')}</span>
					</div>
				{/if}
			</div>
			<p class="s-help warn line"><Icon name="alert" size={16} />{t('lan.warning')}</p>
		</div>
	{/if}
</section>

<style>
	.port {
		width: 96px;
		text-align: right;
	}
	.port[aria-invalid='true'] {
		border-color: var(--k-danger);
	}
	.status .head {
		display: flex;
		align-items: flex-start;
		gap: 12px;
	}
	.status .s-label {
		flex: 1;
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
	.dot.bad {
		background: var(--k-danger);
	}
	.strong {
		font-weight: 600;
	}
	.clients {
		display: flex;
		align-items: baseline;
		gap: 6px;
		flex: none;
	}
	.n {
		font-size: 20px;
		font-weight: 500;
	}
	.bar {
		min-height: 0;
	}
	.links {
		display: grid;
		grid-template-columns: minmax(0, 1fr) auto;
		gap: 16px;
		align-items: start;
	}
	.none {
		margin: 0;
	}
	.url {
		display: flex;
		align-items: center;
		gap: 8px;
	}
	.addr {
		flex: 1;
		min-width: 0;
		font-size: 12px;
		color: var(--k-text);
		user-select: text;
	}
	.addr.masked {
		user-select: none;
	}
	.qr {
		display: flex;
		flex-direction: column;
		align-items: center;
		gap: 6px;
		width: 168px;
		text-align: center;
	}
	.qr img,
	.qr-hidden {
		width: 168px;
		height: 168px;
		border-radius: 10px;
	}
	.qr-hidden {
		display: grid;
		place-items: center;
		background: var(--k-bg-alt);
		border: 1px dashed var(--k-border);
		color: var(--k-text-muted);
	}
	.line {
		margin: 0;
		display: flex;
		align-items: center;
		gap: 8px;
		font-size: 12px;
	}
</style>
