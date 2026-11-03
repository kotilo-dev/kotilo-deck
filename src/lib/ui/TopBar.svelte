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

	const { store, ui } = getDeck();
	const ws = $derived(store.active);
	/** Phase 3 wires the companion server; until then the deck always reports "not running". */
	const companionConnected = false;

	const unplaced = $derived(ws ? layout(ws.widgets, ws.grid).unplaced : []);

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
	<header class="bar">
		<div class="side start">
			<Mark />
			<span class="brand">{t('shell.appName')}</span>
		</div>
		<div class="center">
			<WorkspaceSwitcher />
		</div>
		<div class="side end">
			<span class="companion">
				<span class="dot" class:on={companionConnected} aria-hidden="true"></span>
				<span class="companion-label">
					{companionConnected ? t('shell.topbar.companionOn') : t('shell.topbar.companionOff')}
				</span>
			</span>
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
	.companion {
		display: flex;
		align-items: center;
		gap: 8px;
		font-size: 12px;
		color: var(--k-text-muted);
		white-space: nowrap;
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
