<script lang="ts">
	/**
	 * One widget instance on the grid: explicit placement, merged settings, capability check,
	 * per-instance surface opacity, error boundary, and the edit-mode affordances (DESIGN §4.3, §6).
	 */
	import { hasBackend, type Appearance, type WidgetInstance } from '#lib/ipc/index.ts';
	import { backdropValue, clamp } from '#lib/settings/look.ts';
	import type { Rect } from '#lib/grid/index.ts';
	import { t } from '#lib/i18n/index.ts';
	import FallbackCard from './FallbackCard.svelte';
	import Icon from './Icon.svelte';
	import { definition, fmtSize, getDeck, mergedSettings, surfaceDefault, widgetName } from './deck.svelte';
	import PluginFrame from '#lib/plugins/PluginFrame.svelte';
	import { isPluginDefinition, pluginPending } from '#lib/plugins/registry.svelte.ts';

	let {
		inst,
		rect,
		lifted = false,
		offset = { dx: 0, dy: 0 },
		pill = '',
		shaking = false,
		onhandledown,
		onhandleclick,
		onkey,
		ongear
	}: {
		inst: WidgetInstance;
		rect: Rect;
		lifted?: boolean;
		offset?: { dx: number; dy: number };
		pill?: string;
		shaking?: boolean;
		onhandledown: (e: PointerEvent) => void;
		onhandleclick: (e: MouseEvent) => void;
		onkey: (e: KeyboardEvent) => void;
		ongear: () => void;
	} = $props();

	const { ui } = getDeck();
	const def = $derived(definition(inst.widget));
	const settings = $derived(mergedSettings(inst));
	/** Central capability gate: a widget is only mounted once its capabilities are known to exist. */
	const availability = $derived(ui.availability(def));
	const size = $derived(`${rect.w}x${rect.h}` as const);
	const name = $derived(widgetName(inst.widget));
	const compact = $derived(rect.h === 1);

	/**
	 * Per-instance appearance.opacity (0–1). Edit mode forces 100 % via the grid, except for the instance
	 * whose settings sheet is open, so the opacity slider previews live.
	 */
	const own = $derived((inst.settings?.appearance ?? undefined) as Appearance | undefined);
	const opacity = $derived.by(() => {
		const o = own?.opacity;
		return typeof o === 'number' && o >= 0 && o <= 1 ? o : null;
	});
	const previewing = $derived(ui.settingsFor === inst.instance);
	const surfaceA = $derived.by(() => {
		const o = previewing ? (opacity ?? surfaceDefault(ui)) : ui.editing ? null : opacity;
		return o === null ? undefined : `color-mix(in srgb, var(--k-surface) ${Math.round(o * 100)}%, transparent)`;
	});
	/** Instance blur override only; the workspace/global value is inherited from :root. */
	const backdrop = $derived(typeof own?.blur === 'number' ? backdropValue(clamp(Math.round(own.blur), 0, 40), ui.look.look) : undefined);

	const transform = $derived(
		lifted
			? `translate(${offset.dx}px, ${offset.dy}px) scale(1.04)${ui.reducedMotion ? '' : ' rotate(-1deg)'}`
			: undefined
	);
</script>

<div
	class="frame"
	class:lifted
	class:editing={ui.editing}
	class:flash={ui.highlight === inst.instance}
	class:k-shake={shaking}
	data-instance={inst.instance}
	style:grid-column="{rect.x + 1} / span {rect.w}"
	style:grid-row="{rect.y + 1} / span {rect.h}"
	style:transform
	style:--k-surface-a={surfaceA}
	style:--k-backdrop={backdrop}
>
	<div class="content" inert={ui.editing}>
		{#if (def && availability === 'unknown') || (!def && pluginPending(inst.widget))}
			<div class="k-card" aria-busy="true" aria-label={name}></div>
		{:else if def && availability === 'no'}
			<FallbackCard
				{compact}
				title={name}
				meta={fmtSize(size)}
				icon={def.manifest.icon}
				body={hasBackend() ? t('shell.widget.notAvailable') : t('shell.widget.browserPreview')}
			/>
		{:else if isPluginDefinition(def)}
			<PluginFrame {size} {settings} instance={inst.instance} editing={ui.editing} available={true} plugin={def.plugin} {name} />
		{:else if def}
			{@const Widget = def.component}
			<svelte:boundary onerror={(e) => console.error(`kotilo-deck: widget ${inst.instance} failed`, e)}>
				<Widget {size} {settings} instance={inst.instance} editing={ui.editing} available={true} />
				{#snippet failed(_error, reset)}
					<FallbackCard {compact} icon="alert" title={name} meta={fmtSize(size)} body={t('shell.widget.error')} onretry={reset} />
				{/snippet}
			</svelte:boundary>
		{:else}
			<FallbackCard {compact} title={t('shell.widget.unknown')} meta={fmtSize(size)} body={t('shell.widget.unknownBody', { id: inst.widget })} />
		{/if}
	</div>

	{#if ui.editing}
		<button
			type="button"
			class="shield"
			aria-label={t('shell.edit.widgetLabel', { name, size: fmtSize(size) })}
			aria-describedby="k-edit-keys"
			onkeydown={onkey}
		></button>
		<button type="button" class="gear" aria-label={t('shell.edit.settings', { name })} onclick={ongear}>
			<Icon name="gear" size={16} />
		</button>
		<button
			type="button"
			class="handle"
			aria-label={t('shell.edit.resize', { name })}
			onpointerdown={onhandledown}
			onclick={onhandleclick}
		>
			<span class="knob"><Icon name="resize" size={12} stroke={3} /></span>
		</button>
		{#if pill}<span class="pill k-mono" aria-hidden="true">{pill}</span>{/if}
	{/if}
</div>

<style>
	.frame {
		position: relative;
		min-width: 0;
		min-height: 0;
		border-radius: var(--k-radius);
	}
	.content {
		height: 100%;
		min-height: 0;
	}
	.content > :global(*) {
		height: 100%;
	}
	/* Edit mode: the 36 px gear (top 10, right 10) sits over the right end of the card header (DESIGN §6).
	   Every card header is a <header> child of .k-card; reserve the gear's width so the meta
	   ("2 / 4", "6 min ago", "4 left") ellipsizes before it instead of running underneath. */
	.editing .content :global(.k-card > header) {
		box-sizing: border-box;
		padding-right: 32px;
	}
	.flash {
		outline: 2px solid var(--k-accent);
		outline-offset: 3px;
	}
	.lifted {
		z-index: 5;
		/* The only shadow in the app (DESIGN §6). */
		box-shadow: 0 24px 48px rgb(0 0 0 / 0.6);
		cursor: grabbing;
	}
	.lifted :global(.k-card) {
		border: 2px solid var(--k-accent);
	}
	.shield {
		position: absolute;
		inset: 0;
		z-index: 1;
		border: 0;
		border-radius: var(--k-radius);
		background: transparent;
		cursor: grab;
		touch-action: none;
	}
	.shield:focus-visible {
		outline-offset: 2px;
	}
	.gear {
		position: absolute;
		top: 10px;
		right: 10px;
		z-index: 2;
		width: 36px;
		height: 36px;
		display: flex;
		align-items: center;
		justify-content: center;
		border-radius: 10px;
		border: 1px solid var(--k-border);
		background: var(--k-bg-alt);
		color: var(--k-text-muted);
	}
	.gear:hover {
		color: var(--k-text);
	}
	/* 28 px visual knob inside a 44 px hit target, overlapping the corner by 14 px. */
	.handle {
		position: absolute;
		right: -22px;
		bottom: -22px;
		z-index: 3;
		width: 44px;
		height: 44px;
		padding: 0;
		border: 0;
		border-radius: 50%;
		background: transparent;
		display: flex;
		align-items: center;
		justify-content: center;
		touch-action: none;
		cursor: nwse-resize;
	}
	.knob {
		box-sizing: border-box;
		width: 28px;
		height: 28px;
		border-radius: 50%;
		background: var(--k-accent);
		border: 3px solid var(--k-bg);
		color: var(--k-on-accent);
		display: flex;
		align-items: center;
		justify-content: center;
	}
	.pill {
		position: absolute;
		left: 50%;
		top: -16px;
		z-index: 4;
		transform: translateX(-50%);
		padding: 4px 10px;
		border-radius: 999px;
		background: var(--k-accent);
		color: var(--k-on-accent);
		font-size: 11px;
		font-weight: 600;
		white-space: nowrap;
		pointer-events: none;
	}
</style>
