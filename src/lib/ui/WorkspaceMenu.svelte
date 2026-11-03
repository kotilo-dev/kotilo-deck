<script lang="ts">
	/**
	 * "Editing <Name> ▾" trigger + workspace actions (SPEC §4.1): new, rename, duplicate,
	 * reset to preset, import, export, delete (never the last one).
	 */
	import { t } from '#lib/i18n/index.ts';
	import { PRESET_IDS, WorkspaceError, type PresetId } from '#lib/workspace/index.ts';
	import Icon from './Icon.svelte';
	import Popover from './Popover.svelte';
	import { getDeck } from './deck.svelte';
	import { openTextFile, saveTextFile } from '#lib/settings/files.ts';

	const { store, ui } = getDeck();
	const ws = $derived(store.active);

	let menu = $state<HTMLDivElement>();
	let nameDialog = $state<HTMLDialogElement>();
	let deleteDialog = $state<HTMLDialogElement>();
	let resetDialog = $state<HTMLDialogElement>();

	let nameMode = $state<'new' | 'rename'>('new');
	let nameValue = $state('');
	let preset = $state<PresetId>('focus');

	function close() {
		menu?.hidePopover();
	}

	function openName(mode: 'new' | 'rename') {
		close();
		nameMode = mode;
		nameValue = mode === 'rename' ? (ws?.name ?? '') : '';
		nameDialog?.showModal();
	}

	function submitName(e: SubmitEvent) {
		e.preventDefault();
		if (!nameValue.trim()) return;
		if (nameMode === 'new') store.create(nameValue);
		else if (ws) store.rename(ws.id, nameValue);
		nameDialog?.close();
	}

	function duplicate() {
		close();
		if (ws) store.duplicate(ws.id);
	}

	function openReset() {
		close();
		preset = ws && (PRESET_IDS as readonly string[]).includes(ws.id) ? (ws.id as PresetId) : 'focus';
		resetDialog?.showModal();
	}

	function confirmReset(e: SubmitEvent) {
		e.preventDefault();
		if (ws) store.resetToPreset(ws.id, preset);
		resetDialog?.close();
	}

	function openDelete() {
		close();
		deleteDialog?.showModal();
	}

	async function confirmDelete(e: SubmitEvent) {
		e.preventDefault();
		deleteDialog?.close();
		if (ws && !(await store.remove(ws.id))) ui.say(t('shell.workspace.deleteLast'), 'warning');
	}

	async function chooseImport() {
		close();
		try {
			const text = await openTextFile(['json']);
			if (text === null) return;
			const imported = store.importJson(text);
			ui.say(t('shell.workspace.imported', { name: imported.name }));
		} catch (err) {
			ui.say(err instanceof WorkspaceError ? err.message : err instanceof Error ? err.message : String(err), 'warning', 7000);
		}
	}

	/** Native save dialog inside Tauri (`file_save_as`), Blob download in a browser. */
	async function exportActive() {
		close();
		if (!ws) return;
		const file = `${ws.id}.json`;
		try {
			if (await saveTextFile(file, store.exportJson(ws.id))) ui.say(t('shell.workspace.exported', { file }));
		} catch (err) {
			ui.say(err instanceof Error ? err.message : String(err), 'warning', 7000);
		}
	}
</script>

<button type="button" class="trigger" popovertarget="k-ws-menu" aria-haspopup="menu">
	<span class="dot" aria-hidden="true"></span>
	<span class="label">{t('shell.edit.editing')} <span class="name">{ws?.name}</span></span>
	<Icon name="chevronDown" size={16} />
</button>

<Popover id="k-ws-menu" label={t('shell.workspace.menu')} width={240} bind:el={menu}>
	<div role="menu" aria-label={t('shell.workspace.menu')}>
		<button type="button" role="menuitem" class="k-menu-item" onclick={() => openName('new')}>
			<Icon name="plus" size={16} />{t('shell.workspace.new')}
		</button>
		<button type="button" role="menuitem" class="k-menu-item" onclick={() => openName('rename')}>
			<Icon name="edit" size={16} />{t('shell.workspace.rename')}
		</button>
		<button type="button" role="menuitem" class="k-menu-item" onclick={duplicate}>
			<Icon name="copy" size={16} />{t('shell.workspace.duplicate')}
		</button>
		<button type="button" role="menuitem" class="k-menu-item" onclick={openReset}>
			<Icon name="refresh" size={16} />{t('shell.workspace.reset')}
		</button>
		<div class="k-menu-sep" role="separator"></div>
		<button type="button" role="menuitem" class="k-menu-item" onclick={chooseImport}>
			<Icon name="upload" size={16} />{t('shell.workspace.import')}
		</button>
		<button type="button" role="menuitem" class="k-menu-item" onclick={exportActive}>
			<Icon name="download" size={16} />{t('shell.workspace.export')}
		</button>
		<div class="k-menu-sep" role="separator"></div>
		<button
			type="button"
			role="menuitem"
			class="k-menu-item"
			data-tone="danger"
			disabled={store.workspaces.length <= 1}
			title={store.workspaces.length <= 1 ? t('shell.workspace.deleteLast') : undefined}
			onclick={openDelete}
		>
			<Icon name="trash" size={16} />{t('shell.workspace.delete')}
		</button>
	</div>
</Popover>


<dialog bind:this={nameDialog} class="k-dialog" aria-labelledby="k-ws-name-title">
	<form class="k-dialog-body" onsubmit={submitName}>
		<h2 id="k-ws-name-title" class="k-dialog-title">
			{nameMode === 'new' ? t('shell.workspace.new') : t('shell.workspace.rename')}
		</h2>
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

<dialog bind:this={resetDialog} class="k-dialog" aria-labelledby="k-ws-reset-title">
	<form class="k-dialog-body" onsubmit={confirmReset}>
		<h2 id="k-ws-reset-title" class="k-dialog-title">{t('shell.workspace.resetTitle', { name: ws?.name ?? '' })}</h2>
		<p class="k-dialog-text">{t('shell.workspace.resetBody')}</p>
		<fieldset class="presets">
			<legend class="k-sr-only">{t('shell.workspace.reset')}</legend>
			{#each PRESET_IDS as id (id)}
				<label class="preset">
					<input type="radio" name="preset" value={id} bind:group={preset} class="k-check" />
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

<dialog bind:this={deleteDialog} class="k-dialog" aria-labelledby="k-ws-del-title">
	<form class="k-dialog-body" onsubmit={confirmDelete}>
		<h2 id="k-ws-del-title" class="k-dialog-title">{t('shell.workspace.deleteTitle', { name: ws?.name ?? '' })}</h2>
		<p class="k-dialog-text">{t('shell.workspace.deleteBody')}</p>
		<div class="k-dialog-actions">
			<button type="button" class="k-btn" onclick={() => deleteDialog?.close()}>{t('shell.workspace.cancel')}</button>
			<button type="submit" class="k-btn k-btn-danger k-btn-filled">{t('shell.workspace.delete')}</button>
		</div>
	</form>
</dialog>

<style>
	.trigger {
		display: inline-flex;
		align-items: center;
		gap: 10px;
		min-height: 44px;
		padding: 0 10px 0 4px;
		border: 0;
		border-radius: 12px;
		background: transparent;
		color: var(--k-text-muted);
		min-width: 0;
	}
	.trigger:hover {
		background: color-mix(in srgb, var(--k-text) 6%, transparent);
	}
	.dot {
		flex: none;
		width: 8px;
		height: 8px;
		border-radius: 50%;
		background: var(--k-accent);
	}
	.label {
		font-size: 14px;
		font-weight: 600;
		color: var(--k-text);
		white-space: nowrap;
		overflow: hidden;
		text-overflow: ellipsis;
	}
	.name {
		color: var(--k-accent);
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
