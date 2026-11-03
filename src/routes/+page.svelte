<script lang="ts">
	/** Deck shell (DESIGN §4): top bar + workspace grid + edit-mode overlays. */
	import { onMount, untrack } from 'svelte';
	import { fly } from 'svelte/transition';
	import { isTauri, on } from '#lib/ipc/index.ts';
	import { applyTheme, effectiveThemeId, getTheme } from '#lib/theme/index.ts';
	import { defaultBackend, WorkspaceStore } from '#lib/workspace/index.ts';
	import { DeckUi, setDeck, sizesOf } from '#lib/ui/deck.svelte.ts';
	import DeckGrid from '#lib/ui/DeckGrid.svelte';
	import SettingsSheet from '#lib/ui/SettingsSheet.svelte';
	import Toast from '#lib/ui/Toast.svelte';
	import TopBar from '#lib/ui/TopBar.svelte';
	import WidgetDrawer from '#lib/ui/WidgetDrawer.svelte';

	const backend = defaultBackend();
	const ui = new DeckUi();
	const store = new WorkspaceStore(backend, {
		sizesOf,
		viewport: () => ({ width: window.innerWidth, height: window.innerHeight }),
		onError: (message) => ui.say(message, 'warning', 6000)
	});
	setDeck({ store, ui });

	// Effective theme = workspace override → global setting → default; workspace opacity via the loader.
	// Only CSS custom properties change, so switching is instant.
	$effect(() => {
		if (!store.ready) return;
		const ws = store.active;
		applyTheme(getTheme(effectiveThemeId(ws?.theme, store.settings.theme)), document.documentElement, ws?.appearance?.opacity);
	});

	// Persist immediately when leaving edit mode instead of waiting for the debounce.
	$effect(() => {
		if (store.ready && !ui.editing) untrack(() => void store.flush());
	});

	onMount(() => {
		const motion = matchMedia('(prefers-reduced-motion: reduce)');
		const onMotion = () => (ui.reducedMotion = motion.matches);
		onMotion();
		motion.addEventListener('change', onMotion);

		void store.init();
		backend
			.capabilities()
			.then((caps) => (ui.capabilities = new Set(caps)))
			.catch(() => (ui.capabilities = new Set()));

		const unlisten = [
			on('workspace://switch', ({ id }) => store.switchTo(id)),
			on('workspace://cycle', ({ dir }) => store.cycle(dir)),
			on('monitor://changed', (m) => {
				store.setMonitor(m.name);
				store.applyViewport();
			})
		];

		let resizeTimer: ReturnType<typeof setTimeout> | undefined;
		const onResize = () => {
			clearTimeout(resizeTimer);
			resizeTimer = setTimeout(() => store.applyViewport(), 150);
		};
		const onHide = () => {
			if (document.visibilityState === 'hidden') void store.flush();
		};
		const onPageHide = () => void store.flush();
		window.addEventListener('resize', onResize);
		window.addEventListener('pagehide', onPageHide);
		document.addEventListener('visibilitychange', onHide);

		return () => {
			motion.removeEventListener('change', onMotion);
			for (const p of unlisten) void p.then((off) => off());
			clearTimeout(resizeTimer);
			window.removeEventListener('resize', onResize);
			window.removeEventListener('pagehide', onPageHide);
			document.removeEventListener('visibilitychange', onHide);
			void store.flush();
		};
	});

	function overlayOpen(): boolean {
		if (document.querySelector('dialog[open]')) return true;
		try {
			return document.querySelector(':popover-open') !== null;
		} catch {
			return false;
		}
	}

	function onkeydown(e: KeyboardEvent) {
		if (e.defaultPrevented || !store.ready) return;
		const el = e.target as HTMLElement | null;
		const typing = !!el && (el.isContentEditable || ['INPUT', 'TEXTAREA', 'SELECT'].includes(el.tagName));
		if (e.key === 'Escape') {
			if (overlayOpen()) return; // the dialog / popover closes itself
			if (ui.drawerOpen) ui.drawerOpen = false;
			else if (ui.editing) ui.exitEdit();
			return;
		}
		if (typing || overlayOpen()) return;
		// Inside Tauri the global shortcut emits workspace://cycle; handling it here too would double-step.
		if (!isTauri() && e.ctrlKey && e.altKey && (e.key === 'ArrowLeft' || e.key === 'ArrowRight')) {
			e.preventDefault();
			store.cycle(e.key === 'ArrowLeft' ? -1 : 1);
			return;
		}
		if ((e.key === 'e' || e.key === 'E') && !e.ctrlKey && !e.altKey && !e.metaKey && !ui.editing) {
			e.preventDefault();
			ui.enterEdit();
		}
	}
</script>

<svelte:window {onkeydown} />

<main class="deck">
	{#if store.ready && store.active}
		<TopBar />
		<div class="stage">
			{#key store.activeId}
				<div class="slide" in:fly={{ x: 48 * store.lastDir, opacity: 0, duration: ui.reducedMotion ? 0 : 200 }}>
					<DeckGrid />
				</div>
			{/key}
		</div>
		{#if ui.editing}
			<WidgetDrawer />
		{/if}
		<SettingsSheet />
		<Toast />
	{/if}
</main>

<style>
	.deck {
		position: relative;
		height: 100vh;
		height: 100dvh;
		display: flex;
		flex-direction: column;
		overflow: hidden;
		opacity: var(--k-dim);
	}
	.stage {
		position: relative;
		flex: 1;
		min-height: 0;
		display: flex;
		overflow: hidden;
	}
	.slide {
		flex: 1;
		min-width: 0;
		display: flex;
		flex-direction: column;
	}
</style>
