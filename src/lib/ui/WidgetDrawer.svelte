<script lang="ts">
	/**
	 * "Add widget" drawer (DESIGN §6): slides from the right, 340 px; size chips place in the first free slot.
	 * Built-ins first, then an "External" section for plugins (docs/plugin-api.md), live on `plugin://changed`.
	 */
	import { t } from '#lib/i18n/index.ts';
	import { parseSize } from '../../widgets/index';
	import Icon from './Icon.svelte';
	import { iconTone } from './icons';
	import type { WidgetDefinition } from '../../widgets/index';
	import { builtinDefinitions, fmtSize, getDeck, pluginDefinitions, widgetName } from './deck.svelte';

	const { store, ui } = getDeck();
	const builtins = $derived(builtinDefinitions());
	const external = $derived(pluginDefinitions());
	/** Last placed chip per widget id (selected state). */
	let selected = $state<Record<string, string>>({});
	let closeBtn = $state<HTMLButtonElement>();
	let aside = $state<HTMLElement>();

	$effect(() => {
		if (ui.drawerOpen) closeBtn?.focus();
	});
	// Before the DOM update makes the drawer inert (which would drop focus to <body>),
	// hand focus back to the Add widget button.
	$effect.pre(() => {
		if (!ui.drawerOpen && aside?.contains(document.activeElement)) {
			document.querySelector<HTMLElement>('[aria-controls="k-drawer"]')?.focus();
		}
	});

	function place(id: string, size: string) {
		const s = parseSize(size);
		const placed = store.addWidget(id, s);
		const name = widgetName(id);
		if (!placed) {
			ui.say(t('shell.drawer.noRoom', { name, size: fmtSize(s) }), 'warning', 6000);
			return;
		}
		selected = { ...selected, [id]: size };
		ui.flash(placed.instance);
		ui.say(t('shell.drawer.placed', { name, size: fmtSize(s) }));
	}
</script>

<aside
	bind:this={aside}
	id="k-drawer"
	class="drawer"
	class:open={ui.drawerOpen}
	aria-label={t('shell.drawer.title')}
	inert={!ui.drawerOpen}
>
	<div class="head">
		<h2>{t('shell.drawer.title')}</h2>
		<button bind:this={closeBtn} type="button" class="close" aria-label={t('shell.drawer.close')} onclick={() => (ui.drawerOpen = false)}>
			<Icon name="x" size={14} stroke={2.4} />
		</button>
	</div>
	<p class="help">{t('shell.drawer.help')}</p>

	<div class="rows">
		<ul class="list">
			{#each builtins as def (def.manifest.id)}
				{@render row(def, false)}
			{:else}
				<li class="help">{t('shell.drawer.empty')}</li>
			{/each}
		</ul>
		{#if external.length}
			<h3 class="eyebrow" id="k-drawer-external">{t('plugins.drawer.section')}</h3>
			<ul class="list" aria-labelledby="k-drawer-external">
				{#each external as def (def.manifest.id)}
					{@render row(def, true)}
				{/each}
			</ul>
		{/if}
	</div>
</aside>

{#snippet row(def: WidgetDefinition, plugin: boolean)}
	{@const m = def.manifest}
	{@const taken = !m.multiInstance && (store.active?.widgets.some((w) => w.widget === m.id) ?? false)}
	<li class="row">
		<span class="tile" style:color={iconTone(m.icon)}><Icon name={m.icon} size={20} /></span>
		<div class="info">
			<span class="name">
				{widgetName(m.id)}
				{#if m.companion}<span class="tag">· {t('shell.drawer.companion')}</span>{/if}
				{#if plugin}<span class="tag">· {t('plugins.drawer.tag')}</span>{/if}
			</span>
			<div class="chips" role="group" aria-label={widgetName(m.id)}>
				{#each m.sizes as size (size)}
					<button
						type="button"
						class="chip"
						aria-pressed={selected[m.id] === size}
						aria-label="{widgetName(m.id)} {fmtSize(size)}"
						disabled={taken}
						onclick={() => place(m.id, size)}
					>
						<span class="k-mono">{fmtSize(size)}</span>
					</button>
				{/each}
			</div>
			{#if taken}<span class="note">{t('shell.drawer.single')}</span>{/if}
		</div>
	</li>
{/snippet}

<style>
	.drawer {
		position: absolute;
		top: 56px;
		right: 0;
		bottom: 0;
		z-index: 20;
		box-sizing: border-box;
		width: 340px;
		max-width: 100vw;
		display: flex;
		flex-direction: column;
		gap: 14px;
		padding: 20px;
		background: var(--k-bg-alt);
		border-left: 1px solid var(--k-border);
		box-shadow: -16px 0 40px rgb(0 0 0 / 0.5);
		transform: translateX(calc(100% + 48px));
		visibility: hidden;
		transition:
			transform 200ms ease,
			visibility 0s linear 200ms;
	}
	.drawer.open {
		transform: none;
		visibility: visible;
		transition:
			transform 200ms ease,
			visibility 0s;
	}
	.head {
		display: flex;
		justify-content: space-between;
		align-items: center;
	}
	h2 {
		margin: 0;
		font-size: 15px;
		font-weight: 600;
	}
	.close {
		width: 36px;
		height: 36px;
		display: flex;
		align-items: center;
		justify-content: center;
		border-radius: 10px;
		border: 1px solid var(--k-border);
		background: transparent;
		color: var(--k-text-muted);
	}
	.help {
		margin: 0;
		font-size: 12px;
		color: var(--k-text-muted);
	}
	.rows {
		display: flex;
		flex-direction: column;
		gap: 10px;
		overflow-y: auto;
		min-height: 0;
	}
	.list {
		list-style: none;
		margin: 0;
		padding: 0;
		display: flex;
		flex-direction: column;
		gap: 10px;
	}
	.eyebrow {
		margin: 6px 0 0;
		font-size: 11px;
		font-weight: 600;
		text-transform: uppercase;
		letter-spacing: 0.08em;
		color: var(--k-text-muted);
	}
	.row {
		display: flex;
		align-items: center;
		gap: 12px;
		padding: 8px 12px;
		border-radius: 12px;
		background: var(--k-surface);
		border: 1px solid var(--k-border);
	}
	.tile {
		flex: none;
		width: 40px;
		height: 40px;
		border-radius: 10px;
		background: var(--k-bg);
		display: flex;
		align-items: center;
		justify-content: center;
	}
	.info {
		flex: 1;
		min-width: 0;
		display: flex;
		flex-direction: column;
		gap: 2px;
	}
	.name {
		font-size: 13px;
		font-weight: 600;
	}
	.tag {
		font-size: 10px;
		font-weight: 500;
		color: var(--k-text-muted);
	}
	.chips {
		display: flex;
		flex-wrap: wrap;
		gap: 2px;
	}
	/* 44 px hit target around the 28 px visual chip (DESIGN §6). */
	.chip {
		min-width: 44px;
		height: 44px;
		padding: 0 2px;
		border: 0;
		background: transparent;
		display: flex;
		align-items: center;
		justify-content: center;
	}
	.chip > span {
		box-sizing: border-box;
		height: 28px;
		padding: 0 9px;
		display: flex;
		align-items: center;
		border-radius: 7px;
		border: 1px solid var(--k-border);
		color: var(--k-text);
		font-size: 11px;
	}
	.chip:hover:not(:disabled) > span {
		border-color: var(--k-text-muted);
	}
	.chip[aria-pressed='true'] > span {
		border-color: var(--k-accent);
		background: color-mix(in srgb, var(--k-accent) 16%, transparent);
	}
	.chip:focus-visible {
		outline: none;
	}
	.chip:focus-visible > span {
		outline: 2px solid var(--k-accent);
		outline-offset: 2px;
	}
	.note {
		font-size: 11px;
		color: var(--k-text-muted);
	}
	@media (prefers-reduced-motion: reduce) {
		.drawer,
		.drawer.open {
			transition: none;
		}
	}
</style>
