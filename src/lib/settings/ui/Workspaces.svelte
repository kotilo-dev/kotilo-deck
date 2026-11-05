<script lang="ts">
	/**
	 * Workspaces (SPEC §4.1, §7): list with rename / duplicate / reset / export / delete, import, grid
	 * density, the default workspace per monitor and the auto-switch rules (SPEC §4.1).
	 * Every write reaches the deck through `workspace://changed` / `settings://changed`.
	 */
	import type { Workspace } from '#lib/ipc/index.ts';
	import { t } from '#lib/i18n/index.ts';
	import { DENSITIES_LANDSCAPE, DENSITIES_PORTRAIT, layout } from '#lib/grid/index.ts';
	import { PRESET_IDS, WorkspaceError, isPresetId, type PresetId } from '#lib/workspace/index.ts';
	import Icon from '#lib/ui/Icon.svelte';
	import { unplacedText, widgetName } from '#lib/ui/deck.svelte.ts';
	import { openTextFile, saveTextFile } from '../files';
	import { errText, getSettingsContext } from './context.svelte';
	import Head from './Head.svelte';
	import WorkspaceAppearance from './WorkspaceAppearance.svelte';
	import AutoSwitchRules from './AutoSwitchRules.svelte';

	const { store, ui, env } = getSettingsContext();

	let nameDialog = $state<HTMLDialogElement>();
	let resetDialog = $state<HTMLDialogElement>();
	let deleteDialog = $state<HTMLDialogElement>();
	let target = $state<Workspace | null>(null);
	let nameMode = $state<'new' | 'rename'>('new');
	let nameValue = $state('');
	let preset = $state<PresetId>('focus');
	/** Rows whose appearance override panel is expanded. */
	let open = $state<Record<string, boolean>>({});

	const monitors = $derived(env.monitorKeys(store));
	const densities = (ws: Workspace) => (ws.grid.rows > ws.grid.cols ? DENSITIES_PORTRAIT : DENSITIES_LANDSCAPE);
	const gridKey = (g: { cols: number; rows: number }) => `${g.cols}x${g.rows}`;

	function openName(mode: 'new' | 'rename', ws: Workspace | null = null) {
		nameMode = mode;
		target = ws;
		nameValue = ws?.name ?? '';
		nameDialog?.showModal();
	}

	function submitName(e: SubmitEvent) {
		e.preventDefault();
		if (!nameValue.trim()) return;
		if (nameMode === 'new') {
			const ws = store.create(nameValue);
			ui.say(t('settings.workspaces.created', { name: ws.name }));
		} else if (target) store.rename(target.id, nameValue);
		nameDialog?.close();
	}

	function openReset(ws: Workspace) {
		target = ws;
		preset = isPresetId(ws.id) ? ws.id : 'focus';
		resetDialog?.showModal();
	}

	function confirmReset(e: SubmitEvent) {
		e.preventDefault();
		if (target) store.resetToPreset(target.id, preset);
		resetDialog?.close();
	}

	function openDelete(ws: Workspace) {
		target = ws;
		deleteDialog?.showModal();
	}

	async function confirmDelete(e: SubmitEvent) {
		e.preventDefault();
		deleteDialog?.close();
		if (target && !(await store.remove(target.id))) ui.say(t('shell.workspace.deleteLast'), 'warning');
	}

	function duplicate(ws: Workspace) {
		const copy = store.duplicate(ws.id);
		if (copy) ui.say(t('settings.workspaces.created', { name: copy.name }));
	}

	async function exportOne(ws: Workspace) {
		const file = `${ws.id}.json`;
		try {
			await store.flush();
			if (await saveTextFile(file, store.exportJson(ws.id))) ui.say(t('shell.workspace.exported', { file }));
		} catch (err) {
			ui.say(errText(err), 'warning', 7000);
		}
	}

	async function importOne() {
		try {
			const text = await openTextFile(['json']);
			if (text === null) return;
			const ws = store.importJson(text);
			ui.say(t('shell.workspace.imported', { name: ws.name }));
		} catch (err) {
			ui.say(err instanceof WorkspaceError ? err.message : errText(err), 'warning', 7000);
		}
	}

	function setDensity(ws: Workspace, key: string) {
		const g = densities(ws).find((d) => gridKey(d) === key);
		if (!g) return;
		const lost = store.setGridFor(ws.id, g);
		if (lost.length) {
			const names = lost.map((id) => widgetName(ws.widgets.find((w) => w.instance === id)?.widget ?? id));
			ui.say(unplacedText(names), 'warning', 6000);
		}
	}

	const monitorLabel = (key: string) => (key === 'browser' ? t('settings.workspaces.browserMonitor') : key);
</script>

<section class="s-page">
	<Head title={t('settings.workspaces.title')} text={t('settings.workspaces.text')} />

	<div class="s-group">
		<div class="s-inline between">
			<h2 class="s-eyebrow">{t('settings.workspaces.list', { count: store.workspaces.length })}</h2>
			<div class="s-inline">
				<button type="button" class="k-btn s-btn-sm" onclick={importOne}><Icon name="upload" size={16} />{t('shell.workspace.import')}</button>
				<button type="button" class="k-btn k-btn-primary s-btn-sm" onclick={() => openName('new')}>
					<Icon name="plus" size={16} stroke={2.4} />{t('shell.workspace.new')}
				</button>
			</div>
		</div>
		<ul class="s-list">
			{#each store.workspaces as ws (ws.id)}
				{@const hidden = layout(ws.widgets, ws.grid).unplaced.length}
				<li class="s-card item">
					<div class="row">
					<div class="s-label main">
						<span class="s-title name">{ws.name}</span>
						<span class="s-help">
							<span class="k-mono">{ws.id}</span> · {t('settings.workspaces.widgets', { count: ws.widgets.length })}{#if hidden}
								· <span class="warn">{t('settings.workspaces.hidden', { count: hidden })}</span>{/if}
						</span>
					</div>
					<label class="density">
						<span class="k-sr-only">{t('settings.workspaces.density', { name: ws.name })}</span>
						<select class="k-select k-mono" value={gridKey(ws.grid)} onchange={(e) => setDensity(ws, e.currentTarget.value)}>
							{#if !densities(ws).some((d) => gridKey(d) === gridKey(ws.grid))}
								<option value={gridKey(ws.grid)}>{ws.grid.cols} × {ws.grid.rows}</option>
							{/if}
							{#each densities(ws) as d (gridKey(d))}
								<option value={gridKey(d)}>{d.cols} × {d.rows}</option>
							{/each}
						</select>
					</label>
					<div class="actions">
						<button
							type="button"
							class="k-btn k-btn-icon"
							class:on={!!ws.appearance}
							aria-expanded={!!open[ws.id]}
							aria-label={t('settings.workspaces.appearanceOf', { name: ws.name })}
							title={t('settings.workspaces.appearance')}
							onclick={() => (open = { ...open, [ws.id]: !open[ws.id] })}
						>
							<Icon name="palette" size={16} />
						</button>
						<button type="button" class="k-btn k-btn-icon" aria-label={t('settings.workspaces.renameOne', { name: ws.name })} title={t('shell.workspace.rename')} onclick={() => openName('rename', ws)}>
							<Icon name="edit" size={16} />
						</button>
						<button type="button" class="k-btn k-btn-icon" aria-label={t('settings.workspaces.duplicateOne', { name: ws.name })} title={t('shell.workspace.duplicate')} onclick={() => duplicate(ws)}>
							<Icon name="copy" size={16} />
						</button>
						<button type="button" class="k-btn k-btn-icon" aria-label={t('settings.workspaces.resetOne', { name: ws.name })} title={t('shell.workspace.reset')} onclick={() => openReset(ws)}>
							<Icon name="refresh" size={16} />
						</button>
						<button type="button" class="k-btn k-btn-icon" aria-label={t('settings.workspaces.exportOne', { name: ws.name })} title={t('shell.workspace.export')} onclick={() => exportOne(ws)}>
							<Icon name="download" size={16} />
						</button>
						<button
							type="button"
							class="k-btn k-btn-icon danger"
							aria-label={t('settings.workspaces.deleteOne', { name: ws.name })}
							title={store.workspaces.length <= 1 ? t('shell.workspace.deleteLast') : t('shell.workspace.delete')}
							disabled={store.workspaces.length <= 1}
							onclick={() => openDelete(ws)}
						>
							<Icon name="trash" size={16} />
						</button>
					</div>
					</div>
					{#if open[ws.id]}<WorkspaceAppearance {ws} />{/if}
				</li>
			{/each}
		</ul>
	</div>

	<AutoSwitchRules />

	<div class="s-group">
		<h2 class="s-eyebrow">{t('settings.workspaces.perMonitor')}</h2>
		{#if monitors.length === 0}
			<p class="s-card s-note">{t('settings.display.none')}</p>
		{:else}
			{#each monitors as mon (mon)}
				{@const current = store.settings.activeWorkspace[mon] ?? store.workspaces[0]?.id}
				<label class="s-card s-row">
					<span class="s-label">
						<span class="s-title">{monitorLabel(mon)}</span>
						<span class="s-help">{t('settings.workspaces.perMonitorHelp')}</span>
					</span>
					<select class="k-select s-select-sm" value={current} onchange={(e) => store.setDefaultFor(mon, e.currentTarget.value)}>
						{#each store.workspaces as ws (ws.id)}
							<option value={ws.id}>{ws.name}</option>
						{/each}
					</select>
				</label>
			{/each}
		{/if}
	</div>
</section>

<dialog bind:this={nameDialog} class="k-dialog" aria-labelledby="k-sw-name">
	<form class="k-dialog-body" onsubmit={submitName}>
		<h2 id="k-sw-name" class="k-dialog-title">{nameMode === 'new' ? t('shell.workspace.new') : t('shell.workspace.rename')}</h2>
		<label class="k-label">
			{t('shell.workspace.nameLabel')}
			<!-- svelte-ignore a11y_autofocus -->
			<input class="k-input" bind:value={nameValue} maxlength="40" required autofocus />
		</label>
		<div class="k-dialog-actions">
			<button type="button" class="k-btn" onclick={() => nameDialog?.close()}>{t('shell.workspace.cancel')}</button>
			<button type="submit" class="k-btn k-btn-primary" disabled={!nameValue.trim()}>
				{nameMode === 'new' ? t('shell.workspace.create') : t('shell.workspace.save')}
			</button>
		</div>
	</form>
</dialog>

<dialog bind:this={resetDialog} class="k-dialog" aria-labelledby="k-sw-reset">
	<form class="k-dialog-body" onsubmit={confirmReset}>
		<h2 id="k-sw-reset" class="k-dialog-title">{t('shell.workspace.resetTitle', { name: target?.name ?? '' })}</h2>
		<p class="k-dialog-text">{t('shell.workspace.resetBody')}</p>
		<fieldset class="presets">
			<legend class="k-sr-only">{t('shell.workspace.reset')}</legend>
			{#each PRESET_IDS as id (id)}
				<label class="preset">
					<input type="radio" name="sw-preset" value={id} bind:group={preset} class="k-check" />
					{t(`shell.presets.${id}`)}
				</label>
			{/each}
		</fieldset>
		<div class="k-dialog-actions">
			<button type="button" class="k-btn" onclick={() => resetDialog?.close()}>{t('shell.workspace.cancel')}</button>
			<button type="submit" class="k-btn k-btn-primary">{t('shell.workspace.reset')}</button>
		</div>
	</form>
</dialog>

<dialog bind:this={deleteDialog} class="k-dialog" aria-labelledby="k-sw-del">
	<form class="k-dialog-body" onsubmit={confirmDelete}>
		<h2 id="k-sw-del" class="k-dialog-title">{t('shell.workspace.deleteTitle', { name: target?.name ?? '' })}</h2>
		<p class="k-dialog-text">{t('shell.workspace.deleteBody')}</p>
		<div class="k-dialog-actions">
			<button type="button" class="k-btn" onclick={() => deleteDialog?.close()}>{t('shell.workspace.cancel')}</button>
			<button type="submit" class="k-btn k-btn-danger k-btn-filled">{t('shell.workspace.delete')}</button>
		</div>
	</form>
</dialog>

<style>
	.between {
		justify-content: space-between;
	}
	.item {
		padding: 0;
	}
	.actions .on {
		color: var(--k-accent);
	}
	.row {
		display: flex;
		align-items: center;
		gap: 12px;
		padding: 8px 8px 8px 14px;
	}
	.main {
		flex: 1;
	}
	.name {
		font-weight: 600;
		white-space: nowrap;
		overflow: hidden;
		text-overflow: ellipsis;
	}
	.warn {
		color: color-mix(in srgb, var(--k-warning) 45%, var(--k-text));
	}
	.density select {
		width: 104px;
	}
	.actions {
		display: flex;
		gap: 2px;
	}
	.actions .k-btn-icon {
		border-color: transparent;
	}
	.actions .danger:not(:disabled):hover {
		color: var(--k-danger);
	}
	.presets {
		display: grid;
		grid-template-columns: 1fr 1fr;
		gap: 6px;
		margin: 0;
		padding: 0;
		border: 0;
	}
	.preset {
		display: flex;
		align-items: center;
		gap: 10px;
		min-height: 44px;
		padding: 0 12px;
		border-radius: 10px;
		border: 1px solid var(--k-border);
		font-size: 13px;
		cursor: pointer;
	}
	.preset:has(input:checked) {
		border-color: var(--k-accent);
		background: color-mix(in srgb, var(--k-accent) 16%, transparent);
	}
</style>
