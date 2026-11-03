<script lang="ts">
	/**
	 * Deck shell (DESIGN §4): background layer → top bar + workspace grid (content layer) → overlays.
	 * Appearance layers and burn-in follow SPEC §5.3 / §2; settings and workspace edits made in the
	 * settings window arrive via `settings://changed` / `workspace://changed` and apply live.
	 */
	import { onMount, untrack } from 'svelte';
	import { fly } from 'svelte/transition';
	import { convertFileSrc } from '@tauri-apps/api/core';
	import { isTauri, on, settingsOpen, wallpaperList, type Wallpaper } from '#lib/ipc/index.ts';
	import { t } from '#lib/i18n/index.ts';
	import { applyTheme, effectiveThemeId, getTheme, pairFor, themes } from '#lib/theme/index.ts';
	import { defaultBackend, WorkspaceStore } from '#lib/workspace/index.ts';
	import {
		Ambient,
		backdropValue,
		refreshUserThemes,
		resolveBackground,
		resolveBlur,
		resolveSurfaceOpacity
	} from '#lib/settings/index.ts';
	import { DeckUi, setDeck, sizesOf } from '#lib/ui/deck.svelte.ts';
	import ClockFace from '#lib/ui/ClockFace.svelte';
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
	const ambient = new Ambient({ settings: () => store.settings });
	setDeck({ store, ui });

	let wallpapers = $state<Wallpaper[]>([]);

	// Effective theme = workspace override → global setting → default, then the light/dark pair when the
	// auto switch asks for the other variant. Only CSS custom properties change, so switching is instant.
	const theme = $derived.by(() => {
		const base = getTheme(effectiveThemeId(store.active?.theme, store.settings.theme));
		return pairFor(base, ambient.wanted, (id) => themes.get(id));
	});
	const background = $derived(resolveBackground(store.active?.appearance, store.settings.appearance));
	const wallpaper = $derived.by(() => {
		if (background.mode !== 'wallpaper') return null;
		const w = wallpapers.find((x) => x.name === background.wallpaper);
		return w ? { kind: w.kind, src: convertFileSrc(w.path) } : null;
	});

	$effect(() => {
		if (!store.ready) return;
		const ws = store.active;
		const surface = resolveSurfaceOpacity(ws?.appearance, store.settings.appearance, theme);
		const blur = resolveBlur(undefined, ws?.appearance, store.settings.appearance);
		const root = document.documentElement;
		applyTheme(theme, root, surface);
		root.style.setProperty('--k-backdrop', backdropValue(blur));
		ui.look = { surfaceOpacity: surface, blur };
	});

	$effect(() => {
		document.documentElement.style.setProperty('--k-dim', String(ambient.dim));
	});

	$effect(() => {
		// Transparent = html/body transparent too (the window itself is made transparent by the backend).
		document.documentElement.dataset.bg = store.ready ? background.mode : 'solid';
	});

	// A wallpaper named in settings but not in our list (just imported in the settings window) → re-list.
	$effect(() => {
		const name = background.mode === 'wallpaper' ? background.wallpaper : null;
		if (!name || !isTauri()) return;
		untrack(() => {
			if (!wallpapers.some((w) => w.name === name)) void loadWallpapers();
		});
	});

	// Persist immediately when leaving edit mode instead of waiting for the debounce.
	$effect(() => {
		if (store.ready && !ui.editing) untrack(() => void store.flush());
	});

	let videoEl = $state<HTMLVideoElement>();
	$effect(() => {
		// Looping wallpaper video: paused while the deck is hidden, under the opaque clock face, or with
		// reduced motion, so nothing decodes frames nobody sees.
		if (!videoEl) return;
		if (ambient.visible && !ambient.clockFace && !ui.reducedMotion) void videoEl.play().catch(() => {});
		else videoEl.pause();
	});

	async function loadWallpapers() {
		if (!isTauri()) return;
		try {
			wallpapers = await wallpaperList();
		} catch (e) {
			console.warn('kotilo-deck: wallpapers unavailable', e);
		}
	}

	async function loadUserThemes() {
		const errors = await refreshUserThemes();
		if (errors.length) {
			ui.say(t('shell.theme.userSkipped', { count: errors.length, names: errors.map((e) => e.label).join(', ') }), 'warning', 8000);
		}
	}

	onMount(() => {
		const motion = matchMedia('(prefers-reduced-motion: reduce)');
		const onMotion = () => (ui.reducedMotion = motion.matches);
		onMotion();
		motion.addEventListener('change', onMotion);
		const stopAmbient = ambient.start();

		void store.init();
		void loadUserThemes();
		void loadWallpapers();
		backend
			.capabilities()
			.then((caps) => (ui.capabilities = new Set(caps)))
			.catch(() => (ui.capabilities = new Set()));

		let reloadTimer: ReturnType<typeof setTimeout> | undefined;
		const unwatch = backend.watch({
			settings: (s) => store.ready && store.applyRemoteSettings(s),
			workspaces: () => {
				clearTimeout(reloadTimer);
				reloadTimer = setTimeout(() => store.ready && void store.reload(), 150);
			}
		});

		const unlisten = [
			on('workspace://switch', ({ id }) => store.switchTo(id)),
			on('workspace://cycle', ({ dir }) => store.cycle(dir)),
			on('monitor://changed', (m) => {
				store.setMonitor(m.name);
				store.applyViewport();
			}),
			on('theme://user-changed', () => void loadUserThemes()),
			// Tray Quit: the backend hides the windows and exits ~500 ms later; write pending edits now.
			on('deck://quitting', () => void store.flush())
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
			stopAmbient();
			unwatch();
			for (const p of unlisten) void p.then((off) => off());
			clearTimeout(resizeTimer);
			clearTimeout(reloadTimer);
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

	function openSettings() {
		if (isTauri()) void settingsOpen().catch((e) => ui.say(String(e), 'warning'));
		else window.open('/settings', 'kotilo-deck-settings', 'width=960,height=680');
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
		if (typing) return;
		if ((e.ctrlKey || e.metaKey) && !e.altKey && (e.key === 'z' || e.key === 'Z') && ui.runAction()) {
			e.preventDefault();
			return;
		}
		if ((e.ctrlKey || e.metaKey) && e.key === ',') {
			e.preventDefault();
			openSettings();
			return;
		}
		if (overlayOpen()) return;
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

<div class="dim-base" data-mode={background.mode}>
<div class="stack">
	<!-- Background layer (SPEC §5.3): solid --k-bg, wallpaper image/video, or nothing (transparent). -->
	<div class="bg" data-mode={background.mode} aria-hidden="true">
		{#if wallpaper?.kind === 'image'}
			<img src={wallpaper.src} alt="" />
		{:else if wallpaper?.kind === 'video'}
			<video bind:this={videoEl} src={wallpaper.src} muted loop playsinline disablepictureinpicture></video>
		{/if}
	</div>

	<main class="deck" style:transform={ambient.shift.x || ambient.shift.y ? `translate(${ambient.shift.x}px, ${ambient.shift.y}px)` : undefined}>
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

		{#if ambient.clockFace}
			<ClockFace now={ambient.now} onwake={() => ambient.wake()} />
		{:else if ambient.idle}
			<!-- Idle dim: the first tap only wakes, it never reaches a widget underneath. -->
			<button type="button" class="wake" data-wake aria-label={t('shell.screensaver.wake')} onclick={() => ambient.wake()}></button>
		{/if}
	</main>
</div>
</div>

<style>
	/*
	 * --k-dim lowers background and content together over 2 s (DESIGN §8), fading toward black so a solid
	 * background and light themes really get darker. Transparent mode fades toward the desktop instead.
	 */
	.dim-base {
		height: 100vh;
		height: 100dvh;
		background: var(--k-dim-base);
	}
	.dim-base[data-mode='transparent'] {
		background: transparent;
	}
	.stack {
		position: relative;
		height: 100%;
		overflow: hidden;
		opacity: var(--k-dim);
		transition: opacity 2s ease;
	}
	.bg {
		position: absolute;
		inset: 0;
		background: var(--k-bg);
	}
	.bg[data-mode='transparent'] {
		background: transparent;
	}
	.bg img,
	.bg video {
		width: 100%;
		height: 100%;
		object-fit: cover;
		display: block;
	}
	.deck {
		position: relative;
		height: 100%;
		display: flex;
		flex-direction: column;
		overflow: hidden;
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
	.wake {
		position: absolute;
		inset: 0;
		z-index: 40;
		border: 0;
		padding: 0;
		background: transparent;
		cursor: default;
	}
	.wake:focus-visible {
		outline-offset: -4px;
	}
</style>
