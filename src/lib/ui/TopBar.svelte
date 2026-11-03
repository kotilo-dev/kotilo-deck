<script lang="ts">
	/** Deck top bar, 56 px (DESIGN §4.2). Normal and edit variants. */
	import { t } from '#lib/i18n/index.ts';
	import { layout, nextDensity } from '#lib/grid/index.ts';
	import Icon from './Icon.svelte';
	import Mark from './Mark.svelte';
	import ThemePicker from './ThemePicker.svelte';
	import WorkspaceMenu from './WorkspaceMenu.svelte';
	import WorkspaceSwitcher from './WorkspaceSwitcher.svelte';
	import { getDeck, unplacedText, widgetName } from './deck.svelte';
	import { companionText, companionView, openSettingsAt } from './companion.svelte';

	const { store, ui } = getDeck();
	const companion = ui.companion;
	const ws = $derived(store.active);
	const view = $derived(companionView(store.settings.companion.enabled, companion.status));
	const text = $derived(companionText(view, companion.status));

	function openCompanion() {
		void openSettingsAt('companion').catch((e) => ui.say(String(e), 'warning'));
	}

	const unplaced = $derived(ws ? layout(ws.widgets, ws.grid).unplaced : []);
	/** Overlay mode is a borderless normal window: the empty parts of the top bar move it. */
	const drag = $derived(store.settings.displayMode === 'overlay' ? true : undefined);

	function cycleDensity() {
		if (!ws) return;
		const lost = store.setGrid(nextDensity(ws.grid, window.innerWidth, window.innerHeight));
		if (lost.length > 0) {
			const names = lost.map((id) => widgetName(ws.widgets.find((w) => w.instance === id)?.widget ?? id));
			ui.say(unplacedText(names), 'warning', 6000);
		}
	}
</script>

{#if !ui.editing}
	<header class="bar" data-tauri-drag-region={drag}>
		<div class="side start" data-tauri-drag-region={drag}>
			<Mark />
			<span class="brand" data-tauri-drag-region={drag}>{t('shell.appName')}</span>
		</div>
		<div class="center" data-tauri-drag-region={drag}>
			<WorkspaceSwitcher />
		</div>
		<div class="side end" data-tauri-drag-region={drag}>
			<button type="button" class="companion" title={text.title} aria-label={t('shell.topbar.companionOpen', { status: text.title })} onclick={openCompanion}>
				<span class="dot" class:on={view === 'connected'} aria-hidden="true"></span>
				<span class="companion-label">{text.label}</span>
			</button>
			<ThemePicker />
			<button type="button" class="k-btn k-btn-icon" aria-label={t('shell.topbar.edit')} title={t('shell.topbar.edit')} onclick={() => ui.enterEdit()}>
				<Icon name="edit" size={18} />
			</button>
		</div>
	</header>
{:else}
	<header class="bar editing">
		<div class="side start">
			<WorkspaceMenu />
			{#if ws}
				<button type="button" class="density" aria-label={t('shell.edit.density', { cols: ws.grid.cols, rows: ws.grid.rows })} onclick={cycleDensity}>
					<span class="k-mono">{ws.grid.cols} × {ws.grid.rows}</span>
				</button>
			{/if}
		</div>
		<p class="hint" class:warn={unplaced.length > 0} aria-live="polite">
			{#if unplaced.length > 0}
				<Icon name="alert" size={16} />
				{unplacedText(unplaced.map((w) => widgetName(w.widget)))}
			{:else}
				{t('shell.edit.hint')}
				<span class="k-sr-only">{t('shell.edit.hintKeys')}</span>
			{/if}
		</p>
		<div class="side end">
			<button type="button" class="k-btn add" aria-expanded={ui.drawerOpen} aria-controls="k-drawer" onclick={() => (ui.drawerOpen = !ui.drawerOpen)}>
				<Icon name="plus" size={16} stroke={2.4} />{t('shell.edit.addWidget')}
			</button>
			<button type="button" class="k-btn k-btn-primary" onclick={() => ui.exitEdit()}>{t('shell.edit.done')}</button>
		</div>
	</header>
{/if}

<style>
	.bar {
		box-sizing: border-box;
		height: 56px;
		flex: none;
		display: flex;
		align-items: center;
		justify-content: space-between;
		gap: 16px;
		padding: 0 24px;
		border-bottom: 1px solid transparent;
	}
	.bar.editing {
		border-bottom-color: var(--k-border);
	}
	.side {
		display: flex;
		align-items: center;
		gap: 10px;
		min-width: 220px;
	}
	.side.end {
		justify-content: flex-end;
		gap: 14px;
	}
	.editing .side.end {
		gap: 10px;
	}
	.editing .side {
		min-width: 0;
		flex: none;
	}
	.center {
		display: flex;
		justify-content: center;
		min-width: 0;
		flex: 0 1 auto;
	}
	.brand {
		font-size: 14px;
		font-weight: 600;
		letter-spacing: 0.01em;
		white-space: nowrap;
	}
	/* Status, not a control in look: no border; still a 44 px tap target that opens Settings ▸ Companion. */
	.companion {
		display: flex;
		align-items: center;
		gap: 8px;
		min-height: 44px;
		min-width: 44px;
		padding: 0 6px;
		margin: 0 -6px;
		justify-content: center;
		border: 0;
		border-radius: 10px;
		background: transparent;
		font-family: inherit;
		font-size: 12px;
		color: var(--k-text-muted);
		white-space: nowrap;
		cursor: pointer;
	}
	.companion:hover {
		color: var(--k-text);
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
	.density {
		display: inline-flex;
		align-items: center;
		min-height: 44px;
		min-width: 44px;
		padding: 0 10px;
		border: 1px solid var(--k-border);
		border-radius: 10px;
		background: transparent;
		color: var(--k-text-muted);
		font-size: 12px;
	}
	.density:hover {
		color: var(--k-text);
	}
	.hint {
		flex: 1;
		min-width: 0;
		margin: 0;
		display: flex;
		align-items: center;
		justify-content: center;
		gap: 6px;
		font-size: 12px;
		color: var(--k-text-muted);
		text-align: center;
		overflow: hidden;
		white-space: nowrap;
		text-overflow: ellipsis;
	}
	.hint.warn {
		/* Raw warning fails 4.5:1 on light bg (Latte ≈ 2.3:1); same mix the widgets use. */
		color: color-mix(in srgb, var(--k-warning) 35%, var(--k-text));
	}
	.add {
		font-weight: 500;
	}
	/* Below 1024 px wide: drop the companion label first (the dot stays). */
	@media (max-width: 1000px) {
		.companion-label {
			position: absolute;
			width: 1px;
			height: 1px;
			overflow: hidden;
			clip: rect(0 0 0 0);
		}
		.side {
			min-width: 0;
		}
	}
</style>
