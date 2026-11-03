<script lang="ts">
	/**
	 * Appearance (DESIGN §7, reference/settings-appearance.html): background layer, widget surfaces,
	 * dim toggles. Edits are a draft until Apply; "Reset to theme" drafts the theme defaults.
	 */
	import { convertFileSrc } from '@tauri-apps/api/core';
	import type { DeckAppearance } from '#lib/ipc/index.ts';
	import { t } from '#lib/i18n/index.ts';
	import { getTheme } from '#lib/theme/index.ts';
	import Icon from '#lib/ui/Icon.svelte';
	import { draftDirty, errText, getSettingsContext } from './context.svelte';
	import Head from './Head.svelte';
	import Switch from './Switch.svelte';

	const { store, ui, env, save } = getSettingsContext();
	const draft = $derived(env.draft);
	const saved = $derived(store.settings.appearance);
	const value = <K extends keyof DeckAppearance>(k: K): DeckAppearance[K] =>
		(k in draft.appearance ? draft.appearance[k] : saved[k]) as DeckAppearance[K];

	const theme = $derived(getTheme(store.settings.theme));
	const themePct = $derived(Math.round(theme.opacity.surface * 100));
	const background = $derived(value('background'));
	const wallpaperName = $derived(value('wallpaper'));
	const surfacePct = $derived(Math.round((value('surfaceOpacity') ?? theme.opacity.surface) * 100));
	const blur = $derived(value('blur'));
	const windowPct = $derived(Math.round(value('windowOpacity') * 100));
	const overlay = $derived(store.settings.displayMode === 'overlay');
	const transparencyOk = $derived(env.capabilities === null || !env.tauri || env.capabilities.has('transparency'));
	const night = $derived(store.settings.burnIn.night);
	const idle = $derived(store.settings.burnIn.idle);
	const nightOn = $derived(draft.night ?? night.enabled);
	const idleOn = $derived(draft.idle ?? idle.enabled);
	const dirty = $derived(draftDirty(draft));
	const selectedWallpaper = $derived(env.wallpapers.find((w) => w.name === wallpaperName));
	let applying = $state(false);

	function set<K extends keyof DeckAppearance>(k: K, v: DeckAppearance[K]) {
		const next = { ...draft.appearance, [k]: v };
		if (saved[k] === v) delete next[k];
		env.draft = { ...draft, appearance: next };
	}

	function setToggle(which: 'night' | 'idle', v: boolean) {
		const savedValue = which === 'night' ? night.enabled : idle.enabled;
		env.draft = { ...draft, [which]: v === savedValue ? undefined : v };
	}

	function resetToTheme() {
		const target: DeckAppearance = { ...saved, background: 'solid', surfaceOpacity: null, blur: 0, windowOpacity: 1 };
		const next: Partial<DeckAppearance> = {};
		for (const k of Object.keys(target) as (keyof DeckAppearance)[]) {
			if (target[k] !== saved[k]) (next as Record<string, unknown>)[k] = target[k];
		}
		env.draft = { ...draft, appearance: next };
	}

	async function apply() {
		applying = true;
		const patch: Parameters<typeof save>[0] = { appearance: { ...saved, ...draft.appearance } };
		if (draft.night !== undefined || draft.idle !== undefined) {
			const b = store.settings.burnIn;
			patch.burnIn = {
				...b,
				night: { ...b.night, enabled: draft.night ?? b.night.enabled },
				idle: { ...b.idle, enabled: draft.idle ?? b.idle.enabled }
			};
		}
		if (await save(patch)) {
			env.draft = { appearance: {} };
			ui.say(t('settings.appearance.applied'));
		}
		applying = false;
	}

	async function importWallpaper() {
		try {
			const w = await env.importWallpaper();
			if (w) {
				set('background', 'wallpaper');
				set('wallpaper', w.name);
			}
		} catch (e) {
			ui.say(t('settings.appearance.importFailed', { message: errText(e) }), 'warning', 7000);
		}
	}

	const options = $derived([
		{ id: 'solid', label: t('settings.appearance.solid'), meta: t('settings.appearance.solidMeta'), disabled: false },
		{
			id: 'wallpaper',
			label: t('settings.appearance.wallpaper'),
			meta: wallpaperName ?? t('settings.appearance.noWallpaper'),
			disabled: false
		},
		{ id: 'transparent', label: t('settings.appearance.transparent'), meta: t('settings.appearance.transparentMeta'), disabled: !transparencyOk }
	] as const);

	function onRadioKey(e: KeyboardEvent) {
		const dir = e.key === 'ArrowRight' || e.key === 'ArrowDown' ? 1 : e.key === 'ArrowLeft' || e.key === 'ArrowUp' ? -1 : 0;
		if (!dir) return;
		e.preventDefault();
		const enabled = options.filter((o) => !o.disabled);
		const i = enabled.findIndex((o) => o.id === background);
		const next = enabled[(i + dir + enabled.length) % enabled.length];
		set('background', next.id);
		const el = (e.currentTarget as HTMLElement).parentElement?.querySelector<HTMLElement>(`[data-bg="${next.id}"]`);
		el?.focus();
	}
</script>

<section class="s-page">
	<Head title={t('settings.appearance.title')} text={t('settings.appearance.text')} />

	<div class="s-group">
		<h2 class="s-eyebrow" id="k-bg-label">{t('settings.appearance.background')}</h2>
		<div class="bgs" role="radiogroup" aria-labelledby="k-bg-label">
			{#each options as o (o.id)}
				<button
					type="button"
					role="radio"
					class="bg-card"
					data-bg={o.id}
					aria-checked={background === o.id}
					tabindex={background === o.id ? 0 : -1}
					disabled={o.disabled}
					title={o.disabled ? t('settings.appearance.transparentUnavailable') : undefined}
					onkeydown={onRadioKey}
					onclick={() => set('background', o.id)}
				>
					{#if o.id === 'solid'}
						<span class="strip solid"></span>
					{:else if o.id === 'wallpaper'}
						<span
							class="strip wall"
							style:background-image={selectedWallpaper?.kind === 'image' ? `url("${convertFileSrc(selectedWallpaper.path)}")` : undefined}
						>
							{#if selectedWallpaper?.kind !== 'image'}<Icon name="image" size={16} />{/if}
						</span>
					{:else}
						<span class="strip clear"></span>
					{/if}
					<span class="bg-label">{o.label} <span class="meta">· {o.meta}</span></span>
				</button>
			{/each}
		</div>

		{#if background === 'wallpaper'}
			<div class="s-card wallpapers">
				{#if !env.tauri}
					<p class="s-note">{t('settings.common.desktopOnly')}</p>
				{:else}
					<ul class="thumbs" aria-label={t('settings.appearance.wallpapers')}>
						{#each env.wallpapers as w (w.name)}
							<li>
								<button
									type="button"
									class="thumb"
									aria-pressed={w.name === wallpaperName}
									aria-label={w.name}
									title={w.name}
									onclick={() => set('wallpaper', w.name)}
								>
									{#if w.kind === 'image'}
										<img src={convertFileSrc(w.path)} alt="" loading="lazy" />
									{:else}
										<Icon name="video" size={18} />
									{/if}
								</button>
							</li>
						{:else}
							<li class="s-help">{t('settings.appearance.noWallpapers', { path: env.wallpapersPath })}</li>
						{/each}
					</ul>
					<button type="button" class="k-btn s-btn-sm" onclick={importWallpaper}>
						<Icon name="upload" size={16} />{t('settings.appearance.import')}
					</button>
				{/if}
			</div>
		{/if}
		{#if background === 'transparent' && !overlay}
			<p class="s-help warn">{t('settings.appearance.transparentHint')}</p>
		{/if}
	</div>

	<div class="s-group">
		<h2 class="s-eyebrow">{t('settings.appearance.surfaces')}</h2>

		<div class="s-card s-slider">
			<label for="k-opacity" class="s-label">
				<span class="s-title">{t('settings.appearance.opacity')}</span>
				<span class="s-help">{t('settings.appearance.opacityHelp', { pct: themePct })}</span>
			</label>
			<input
				id="k-opacity"
				type="range"
				class="k-range"
				min="0"
				max="100"
				step="1"
				value={surfacePct}
				oninput={(e) => {
					const pct = Number(e.currentTarget.value);
					set('surfaceOpacity', pct === themePct && saved.surfaceOpacity === null ? null : pct / 100);
				}}
			/>
			<span class="s-value">{surfacePct}%</span>
		</div>

		<div class="s-card s-slider">
			<label for="k-blur" class="s-label">
				<span class="s-title">{t('settings.appearance.blur')}</span>
				<span class="s-help warn">{t('settings.appearance.blurHelp')}</span>
			</label>
			<input
				id="k-blur"
				type="range"
				class="k-range"
				min="0"
				max="40"
				step="1"
				value={blur}
				oninput={(e) => set('blur', Number(e.currentTarget.value))}
			/>
			<span class="s-value">{blur > 0 ? `${blur} px` : t('settings.common.off')}</span>
		</div>

		<div class="s-card s-slider" class:s-disabled={!overlay}>
			<label for="k-winop" class="s-label">
				<span class="s-title">{t('settings.appearance.windowOpacity')}</span>
				<span class="s-help">{overlay ? t('settings.appearance.windowOpacityOverlay') : t('settings.appearance.windowOpacityHelp')}</span>
			</label>
			<input
				id="k-winop"
				type="range"
				class="k-range"
				min="20"
				max="100"
				step="1"
				value={windowPct}
				disabled={!overlay}
				oninput={(e) => set('windowOpacity', Number(e.currentTarget.value) / 100)}
			/>
			<span class="s-value">{windowPct}%</span>
		</div>
	</div>

	<div class="s-group">
		<h2 class="s-eyebrow">{t('settings.appearance.dim')}</h2>
		<div class="s-grid2">
			<div class="s-card s-row">
				<div class="s-label">
					<span class="s-title">{t('settings.burnin.night')}</span>
					<span class="s-help" id="k-night-sum">
						{t('settings.appearance.nightSummary', { from: night.from, to: night.to, pct: Math.round(night.dim * 100) })}
					</span>
				</div>
				<Switch label={t('settings.burnin.night')} describedby="k-night-sum" checked={nightOn} onchange={(v) => setToggle('night', v)} />
			</div>
			<div class="s-card s-row">
				<div class="s-label">
					<span class="s-title">{t('settings.burnin.idle')}</span>
					<span class="s-help" id="k-idle-sum">
						{idle.clockFace
							? t('settings.appearance.idleSummaryClock', { min: idle.minutes })
							: t('settings.appearance.idleSummary', { min: idle.minutes })}
					</span>
				</div>
				<Switch label={t('settings.burnin.idle')} describedby="k-idle-sum" checked={idleOn} onchange={(v) => setToggle('idle', v)} />
			</div>
		</div>
	</div>

	<footer class="s-footer">
		<span class="s-path" title={env.settingsPath}>{env.settingsPath}</span>
		<div class="s-actions">
			{#if dirty}<span class="unsaved" aria-live="polite">{t('settings.appearance.unsaved')}</span>{/if}
			<button type="button" class="k-btn s-btn-sm" onclick={resetToTheme}>{t('settings.appearance.reset')}</button>
			<button type="button" class="k-btn k-btn-primary s-btn-sm apply" disabled={!dirty || applying} onclick={apply}>
				{t('settings.appearance.apply')}
			</button>
		</div>
	</footer>
</section>

<style>
	.bgs {
		display: grid;
		grid-template-columns: repeat(3, minmax(0, 1fr));
		gap: 12px;
	}
	/* 96 px option cards with a 36 px preview strip; selected = 2 px accent border (DESIGN §7). */
	.bg-card {
		box-sizing: border-box;
		height: 96px;
		padding: 12px;
		border-radius: 12px;
		border: 1px solid var(--k-border);
		background: var(--k-surface);
		color: var(--k-text);
		display: flex;
		flex-direction: column;
		align-items: flex-start;
		justify-content: space-between;
		text-align: left;
		min-width: 0;
	}
	.bg-card[aria-checked='true'] {
		border: 2px solid var(--k-accent);
		padding: 11px;
	}
	.strip {
		box-sizing: border-box;
		width: 100%;
		height: 36px;
		border-radius: 6px;
		flex: none;
	}
	.strip.solid {
		background: var(--k-bg);
	}
	.strip.wall {
		display: flex;
		align-items: center;
		justify-content: center;
		background-color: color-mix(in srgb, var(--k-text) 22%, var(--k-surface));
		background-size: cover;
		background-position: center;
		color: var(--k-text);
	}
	.strip.clear {
		border: 1px dashed var(--k-border);
		background-image: repeating-linear-gradient(45deg, var(--k-border) 0 6px, transparent 6px 12px);
	}
	.bg-label {
		font-size: 13px;
		font-weight: 500;
		white-space: nowrap;
		overflow: hidden;
		text-overflow: ellipsis;
		max-width: 100%;
	}
	.meta {
		color: var(--k-text-muted);
		font-weight: 400;
	}
	.wallpapers {
		display: flex;
		align-items: center;
		gap: 12px;
	}
	.thumbs {
		flex: 1;
		min-width: 0;
		list-style: none;
		margin: 0;
		padding: 0;
		display: flex;
		gap: 8px;
		overflow-x: auto;
	}
	.thumb {
		box-sizing: border-box;
		width: 72px;
		height: 44px;
		padding: 0;
		overflow: hidden;
		border-radius: 8px;
		border: 1px solid var(--k-border);
		background: var(--k-bg);
		color: var(--k-text-muted);
		display: flex;
		align-items: center;
		justify-content: center;
	}
	.thumb[aria-pressed='true'] {
		border: 2px solid var(--k-accent);
	}
	.thumb img {
		width: 100%;
		height: 100%;
		object-fit: cover;
	}
	.s-slider .k-range {
		margin: 0;
	}
	.unsaved {
		align-self: center;
		font-size: 12px;
		color: var(--k-text-muted);
	}
	.apply {
		padding: 0 16px;
		font-weight: 600;
	}
	.s-footer {
		min-width: 0;
	}
	.s-footer .s-path {
		min-width: 0;
		white-space: nowrap;
		overflow: hidden;
		text-overflow: ellipsis;
	}
</style>
