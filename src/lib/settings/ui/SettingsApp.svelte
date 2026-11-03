<script lang="ts">
	/**
	 * Settings window (DESIGN §7, 960×680): 44 px custom title bar (drag region + minimize/close), 220 px
	 * sidebar, one section at a time (`#hash`, so a browser can deep-link `/settings#appearance`).
	 * Never boots the deck: it uses its own WorkspaceStore in the "settings" role.
	 */
	import './settings.css';
	import { onMount } from 'svelte';
	import { getCurrentWindow } from '@tauri-apps/api/window';
	import { listen } from '@tauri-apps/api/event';
	import { isTauri, on, type DeckSettings } from '#lib/ipc/index.ts';
	import { t } from '#lib/i18n/index.ts';
	import { applyTheme, effectiveThemeId, getTheme, pairFor, themes } from '#lib/theme/index.ts';
	import { defaultBackend, WorkspaceStore } from '#lib/workspace/index.ts';
	import { DeckUi, setDeck, sizesOf } from '#lib/ui/deck.svelte.ts';
	import Toast from '#lib/ui/Toast.svelte';
	import Icon from '#lib/ui/Icon.svelte';
	import { companionView, SETTINGS_NAVIGATE_EVENT, takePendingSection } from '#lib/ui/companion.svelte.ts';
	import { Ambient } from '../ambient.svelte';
	import { refreshUserThemes } from '../user-themes';
	import { errText, isSection, SECTIONS, SettingsEnv, setSettingsContext, type SectionId } from './context.svelte';
	import General from './General.svelte';
	import Display from './Display.svelte';
	import AppearancePage from './Appearance.svelte';
	import Themes from './Themes.svelte';
	import Workspaces from './Workspaces.svelte';
	import Weather from './Weather.svelte';
	import Pomodoro from './Pomodoro.svelte';
	import Companion from './Companion.svelte';
	import BurnIn from './BurnIn.svelte';
	import About from './About.svelte';

	const backend = defaultBackend();
	const ui = new DeckUi();
	const env = new SettingsEnv();
	const store = new WorkspaceStore(backend, {
		sizesOf,
		viewport: () => ({ width: 1280, height: 720 }),
		onError: (message) => ui.say(message, 'warning', 6000),
		role: 'settings'
	});
	const ambient = new Ambient({ settings: () => store.settings, burnIn: false });

	let section = $state<SectionId>('general');
	let content = $state<HTMLElement>();

	function go(next: SectionId) {
		section = next;
		history.replaceState(null, '', `#${next}`);
		content?.scrollTo({ top: 0 });
		requestAnimationFrame(() => content?.querySelector<HTMLElement>('h1')?.focus());
	}

	setDeck({ store, ui });
	async function save(patch: Partial<DeckSettings>): Promise<boolean> {
		try {
			await store.updateSettings(patch);
			return true;
		} catch (e) {
			ui.say(t('settings.common.saveFailed', { message: errText(e) }), 'warning', 6000);
			return false;
		}
	}

	setSettingsContext({ store, ui, env, backend, go, save });

	// The settings window follows the global theme (+ auto switch), not a workspace override.
	$effect(() => {
		const base = getTheme(effectiveThemeId(null, store.settings.theme));
		const theme = pairFor(base, ambient.wanted, (id) => themes.get(id));
		applyTheme(theme, document.documentElement);
		document.documentElement.dataset.bg = 'solid';
	});

	onMount(() => {
		const fromHash = location.hash.slice(1);
		const pending = takePendingSection();
		if (pending && isSection(pending)) go(pending);
		else if (isSection(fromHash)) section = fromHash;
		const stopAmbient = ambient.start();
		const stopCompanion = ui.companion.start();
		// Deep links from the deck (top-bar companion status): an open window gets an event, a browser tab a hash change.
		const onHash = () => {
			const id = location.hash.slice(1);
			if (isSection(id) && id !== section) go(id);
		};
		window.addEventListener('hashchange', onHash);
		const offNavigate = isTauri()
			? listen<{ section: string }>(SETTINGS_NAVIGATE_EVENT, (e) => {
					takePendingSection();
					if (isSection(e.payload.section)) go(e.payload.section);
				})
			: Promise.resolve(() => {});
		void store.init();
		void env.load(() => backend.capabilities());
		void refreshUserThemes();

		let reloadTimer: ReturnType<typeof setTimeout> | undefined;
		const unwatch = backend.watch({
			settings: (s) => store.ready && store.applyRemoteSettings(s),
			workspaces: () => {
				clearTimeout(reloadTimer);
				reloadTimer = setTimeout(() => store.ready && void store.reload(), 150);
			}
		});
		const unlisten = [
			on('theme://user-changed', () => void refreshUserThemes()),
			on('monitor://changed', () => void env.loadMonitors()),
			// Tray Quit closes this window too: write pending workspace edits first.
			on('deck://quitting', () => void store.flush())
		];
		const onPageHide = () => void store.flush();
		window.addEventListener('pagehide', onPageHide);
		return () => {
			stopAmbient();
			stopCompanion();
			window.removeEventListener('hashchange', onHash);
			void offNavigate.then((f) => f());
			unwatch();
			for (const p of unlisten) void p.then((off) => off());
			clearTimeout(reloadTimer);
			window.removeEventListener('pagehide', onPageHide);
			void store.flush();
		};
	});

	async function minimize() {
		try {
			await getCurrentWindow().minimize();
		} catch (e) {
			console.warn(e);
		}
	}

	async function closeWindow() {
		await store.flush();
		try {
			await getCurrentWindow().close();
		} catch (e) {
			console.warn(e);
		}
	}

	const companion = $derived(companionView(store.settings.companion.enabled, ui.companion.status));
	const companionLabel = $derived(t(`settings.companion.view.${companion}`));
</script>

<div class="window">
	<header class="titlebar" data-tauri-drag-region>
		<div class="brand" data-tauri-drag-region>
			<svg width="16" height="16" viewBox="0 0 24 24" fill="none" aria-hidden="true" data-tauri-drag-region>
				<path d="M12 2 22 12 12 22 2 12Z" stroke="var(--k-accent)" stroke-width="2" stroke-linejoin="round" />
			</svg>
			<span class="title" data-tauri-drag-region>{t('settings.window.title')}</span>
			<span class="version k-mono" data-tauri-drag-region>v{env.version}</span>
		</div>
		{#if env.tauri}
			<div class="controls">
				<button type="button" class="wc" aria-label={t('settings.window.minimize')} onclick={minimize}>
					<svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" aria-hidden="true"><path d="M5 12h14" /></svg>
				</button>
				<button type="button" class="wc" aria-label={t('settings.window.close')} onclick={closeWindow}>
					<Icon name="x" size={12} stroke={2.5} />
				</button>
			</div>
		{/if}
	</header>

	<div class="body">
		<nav class="nav" aria-label={t('settings.nav.label')}>
			{#each SECTIONS as id (id)}
				<a
					href="#{id}"
					class="link"
					class:bottom={id === 'about'}
					aria-current={section === id ? 'page' : undefined}
					onclick={(e) => {
						e.preventDefault();
						go(id);
					}}
				>
					<span>{t(`settings.nav.${id}`)}</span>
					{#if id === 'companion'}
						<span class="dot" class:on={companion === 'connected'} aria-hidden="true"></span>
						<span class="k-sr-only">{companionLabel}</span>
					{/if}
				</a>
			{/each}
		</nav>

		<div class="content" bind:this={content}>
			{#if store.ready}
				{#if section === 'general'}<General />
				{:else if section === 'display'}<Display />
				{:else if section === 'appearance'}<AppearancePage />
				{:else if section === 'themes'}<Themes />
				{:else if section === 'workspaces'}<Workspaces />
				{:else if section === 'weather'}<Weather />
				{:else if section === 'pomodoro'}<Pomodoro />
				{:else if section === 'companion'}<Companion />
				{:else if section === 'burnin'}<BurnIn />
				{:else}<About />
				{/if}
			{/if}
		</div>
	</div>
	<Toast />
</div>

<style>
	.window {
		box-sizing: border-box;
		height: 100vh;
		height: 100dvh;
		display: flex;
		flex-direction: column;
		overflow: hidden;
		background: var(--k-bg);
		color: var(--k-text);
		font-family: var(--k-font-ui);
	}
	.titlebar {
		flex: none;
		box-sizing: border-box;
		height: 44px;
		padding: 0 16px;
		display: flex;
		align-items: center;
		justify-content: space-between;
		border-bottom: 1px solid var(--k-border);
		background: var(--k-bg-alt);
		user-select: none;
	}
	.brand {
		flex: 1;
		height: 100%;
		display: flex;
		align-items: center;
		gap: 10px;
	}
	.title {
		font-size: 13px;
		font-weight: 600;
	}
	.version {
		font-size: 11px;
		color: var(--k-text-muted);
	}
	.controls {
		display: flex;
		gap: 6px;
	}
	/* 36×32 ghost title-bar buttons (DESIGN §7). */
	.wc {
		width: 36px;
		height: 32px;
		display: flex;
		align-items: center;
		justify-content: center;
		border-radius: 8px;
		border: 0;
		background: transparent;
		color: var(--k-text-muted);
	}
	.wc:hover {
		background: var(--k-surface);
		color: var(--k-text);
	}
	.body {
		flex: 1;
		min-height: 0;
		display: flex;
	}
	.nav {
		flex: none;
		box-sizing: border-box;
		width: 220px;
		padding: 14px 10px;
		display: flex;
		flex-direction: column;
		gap: 2px;
		border-right: 1px solid var(--k-border);
		background: var(--k-bg-alt);
		overflow-y: auto;
	}
	/* 40 px nav links, radius 10 (DESIGN §7). */
	.link {
		box-sizing: border-box;
		min-height: 40px;
		padding: 0 12px;
		border-radius: 10px;
		display: flex;
		align-items: center;
		justify-content: space-between;
		gap: 10px;
		font-size: 13px;
		font-weight: 500;
		color: var(--k-text-muted);
		text-decoration: none;
	}
	.link:hover {
		color: var(--k-text);
	}
	.link[aria-current='page'] {
		background: var(--k-surface);
		color: var(--k-text);
		font-weight: 600;
	}
	.link.bottom {
		margin-top: auto;
	}
	.dot {
		width: 8px;
		height: 8px;
		border-radius: 50%;
		background: var(--k-border);
	}
	.dot.on {
		background: var(--k-success);
	}
	.content {
		flex: 1;
		min-width: 0;
		box-sizing: border-box;
		padding: 24px 28px;
		display: flex;
		flex-direction: column;
		overflow-y: auto;
	}
	.content > :global(.s-page) {
		flex: 1;
	}
	.content :global(h1:focus) {
		outline: none;
	}
</style>
