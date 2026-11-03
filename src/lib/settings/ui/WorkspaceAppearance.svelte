<script lang="ts">
	/**
	 * Per-workspace appearance override (SPEC §5.3): background, surface opacity, blur. Each one either
	 * inherits the global Appearance setting or sets its own value. Saved with the workspace file, so the
	 * deck picks it up through `workspace://changed`.
	 */
	import type { Workspace } from '#lib/ipc/index.ts';
	import { t } from '#lib/i18n/index.ts';
	import { getTheme } from '#lib/theme/index.ts';
	import { clamp } from '../look';
	import { getSettingsContext } from './context.svelte';
	import Switch from './Switch.svelte';

	let { ws }: { ws: Workspace } = $props();
	const { store, env } = getSettingsContext();

	const a = $derived(ws.appearance ?? {});
	const global = $derived(store.settings.appearance);
	const globalOpacity = $derived(global.surfaceOpacity ?? getTheme(ws.theme ?? store.settings.theme).opacity.surface);
	const opacityPct = $derived(Math.round((typeof a.opacity === 'number' ? a.opacity : globalOpacity) * 100));
	const blur = $derived(typeof a.blur === 'number' ? clamp(Math.round(a.blur), 0, 40) : global.blur);
	const bg = $derived(typeof a.background === 'string' ? a.background : '');
	const wallpaperOptions = $derived.by(() => {
		const names = env.wallpapers.map((w) => w.name);
		const current = bg.startsWith('wallpaper:') ? bg.slice('wallpaper:'.length) : null;
		if (current && !names.includes(current)) names.push(current);
		return names;
	});
	const id = $derived(`k-wsa-${ws.id}`);

	const set = (patch: Parameters<typeof store.setWorkspaceAppearance>[1]) => store.setWorkspaceAppearance(ws.id, patch);
</script>

<div class="panel" role="group" aria-label={t('settings.workspaces.appearanceOf', { name: ws.name })}>
	<label class="s-row">
		<span class="s-label">
			<span class="s-title">{t('settings.appearance.background')}</span>
			<span class="s-help">{t('settings.workspaces.inheritHelp')}</span>
		</span>
		<select class="k-select s-select-sm bg" value={bg} onchange={(e) => set({ background: e.currentTarget.value || undefined })}>
			<option value="">{t('settings.workspaces.inherit')}</option>
			<option value="solid">{t('settings.appearance.solid')}</option>
			<option value="transparent">{t('settings.appearance.transparent')}</option>
			{#each wallpaperOptions as name (name)}
				<option value="wallpaper:{name}">{t('settings.appearance.wallpaper')} · {name}</option>
			{/each}
		</select>
	</label>

	<div class="s-slider">
		<div class="s-label">
			<label class="s-title" for="{id}-op">{t('settings.appearance.opacity')}</label>
			<span class="own">
				<Switch
					label={t('settings.workspaces.ownValue', { what: t('settings.appearance.opacity') })}
					checked={typeof a.opacity === 'number'}
					onchange={(on) => set({ opacity: on ? globalOpacity : undefined })}
				/>
				<span class="s-help">{typeof a.opacity === 'number' ? t('settings.workspaces.own') : t('settings.workspaces.inherit')}</span>
			</span>
		</div>
		<input
			id="{id}-op"
			type="range"
			class="k-range"
			min="0"
			max="100"
			step="1"
			value={opacityPct}
			disabled={typeof a.opacity !== 'number'}
			oninput={(e) => set({ opacity: Number(e.currentTarget.value) / 100 })}
		/>
		<span class="s-value">{opacityPct}%</span>
	</div>

	<div class="s-slider">
		<div class="s-label">
			<label class="s-title" for="{id}-blur">{t('settings.appearance.blur')}</label>
			<span class="own">
				<Switch
					label={t('settings.workspaces.ownValue', { what: t('settings.appearance.blur') })}
					checked={typeof a.blur === 'number'}
					onchange={(on) => set({ blur: on ? global.blur : undefined })}
				/>
				<span class="s-help">{typeof a.blur === 'number' ? t('settings.workspaces.own') : t('settings.workspaces.inherit')}</span>
			</span>
		</div>
		<input
			id="{id}-blur"
			type="range"
			class="k-range"
			min="0"
			max="40"
			step="1"
			value={blur}
			disabled={typeof a.blur !== 'number'}
			oninput={(e) => set({ blur: Number(e.currentTarget.value) })}
		/>
		<span class="s-value">{blur > 0 ? `${blur} px` : t('settings.common.off')}</span>
	</div>
</div>

<style>
	.panel {
		display: flex;
		flex-direction: column;
		gap: 10px;
		padding: 12px 14px;
		border-top: 1px solid var(--k-border);
	}
	.own {
		display: flex;
		align-items: center;
		gap: 8px;
		margin-top: 4px;
	}
	.bg {
		min-width: 220px;
	}
	.k-range:disabled {
		opacity: 0.55;
	}
</style>
