<script lang="ts">
	/** General: startup, hotkey, temperature unit, notification sound, always on top, backup (SPEC §7). */
	import { t } from '#lib/i18n/index.ts';
	import Icon from '#lib/ui/Icon.svelte';
	import { openTextFile, saveTextFile } from '../files';
	import { acceleratorFrom, errText, getSettingsContext, hotkeyProblem } from './context.svelte';
	import Head from './Head.svelte';
	import Switch from './Switch.svelte';
	import UnitControl from './UnitControl.svelte';

	const { store, ui, env, save, backend } = getSettingsContext();
	/** The backend copies the current files to `import-<stamp>` in here before an import-all. */
	const BACKUPS_DIR = '~/.kotilo/deck/backups/';

	let hotkey = $state(store.settings.hotkey);
	let hotkeyError = $state<string | null>(null);
	let hotkeySaving = $state(false);
	let busy = $state(false);
	// Follow changes from elsewhere while the field isn't being edited.
	let editingHotkey = $state(false);
	$effect(() => {
		const saved = store.settings.hotkey;
		if (!editingHotkey) hotkey = saved;
	});

	function onHotkeyKey(e: KeyboardEvent) {
		if (e.key === 'Enter') {
			e.preventDefault();
			void saveHotkey();
			return;
		}
		if (e.key === 'Tab' || e.key === 'Escape') return;
		// Recording: a combination with a modifier replaces the field; plain typing still works.
		if (e.ctrlKey || e.altKey || e.metaKey) {
			const acc = acceleratorFrom(e);
			e.preventDefault();
			if (acc) {
				hotkey = acc;
				hotkeyError = null;
			}
		}
	}

	async function saveHotkey() {
		const value = hotkey.trim();
		const problem = hotkeyProblem(value);
		if (problem) {
			hotkeyError = t(`settings.general.hotkeyErrors.${problem}`);
			return;
		}
		if (value === store.settings.hotkey) {
			hotkeyError = null;
			editingHotkey = false;
			return;
		}
		hotkeySaving = true;
		try {
			await store.updateSettings({ hotkey: value });
			hotkeyError = null;
			editingHotkey = false;
			ui.say(t('settings.general.hotkeySaved', { hotkey: value }));
		} catch (e) {
			// e.g. "already registered by another app": the backend rolled back to the old key.
			hotkeyError = t('settings.general.hotkeyErrors.backend', { message: errText(e) });
		} finally {
			hotkeySaving = false;
		}
	}

	async function exportAll() {
		busy = true;
		try {
			await store.flush();
			const json = await backend.exportAll();
			const date = new Date().toISOString().slice(0, 10);
			if (await saveTextFile(`kotilo-deck-${date}.json`, json)) ui.say(t('settings.backup.exported'));
		} catch (e) {
			ui.say(t('settings.backup.failed', { message: errText(e) }), 'warning', 7000);
		} finally {
			busy = false;
		}
	}

	let confirmDialog = $state<HTMLDialogElement>();
	let pendingImport: string | null = null;

	/** Pick the file first, then confirm: importing replaces every setting and workspace. */
	async function chooseImport() {
		try {
			const json = await openTextFile(['json']);
			if (json === null) return;
			pendingImport = json;
			confirmDialog?.showModal();
		} catch (e) {
			ui.say(t('settings.backup.failed', { message: errText(e) }), 'warning', 8000);
		}
	}

	async function confirmImport(e: SubmitEvent) {
		e.preventDefault();
		const json = pendingImport;
		pendingImport = null;
		confirmDialog?.close();
		if (json !== null) await importAll(json);
	}

	async function importAll(json: string) {
		busy = true;
		try {
			await store.flush();
			await backend.importAll(json);
			await store.reloadSettings();
			await store.reload();
			// The deck reloads too (the backend may also emit these after an import; reloading twice is harmless).
			await backend.notifySettingsChanged(store.settings);
			await backend.notifyWorkspacesChanged();
			ui.say(env.tauri ? t('settings.backup.imported', { path: BACKUPS_DIR }) : t('settings.backup.importedBrowser'), 'info', 8000);
		} catch (e) {
			ui.say(t('settings.backup.failed', { message: errText(e) }), 'warning', 8000);
		} finally {
			busy = false;
		}
	}
</script>

<section class="s-page">
	<Head title={t('settings.general.title')} text={t('settings.general.text')} />

	<div class="s-group">
		<h2 class="s-eyebrow">{t('settings.general.startup')}</h2>
		<div class="s-card s-row">
			<div class="s-label">
				<span class="s-title">{t('settings.general.autostart')}</span>
				<span class="s-help" id="k-autostart-help">{t('settings.general.autostartHelp')}</span>
			</div>
			<Switch
				label={t('settings.general.autostart')}
				describedby="k-autostart-help"
				checked={store.settings.autostart}
				onchange={(v) => void save({ autostart: v })}
			/>
		</div>
		<div class="s-card s-row">
			<div class="s-label">
				<span class="s-title">{t('settings.general.alwaysOnTop')}</span>
				<span class="s-help" id="k-ontop-help">{t('settings.general.alwaysOnTopHelp')}</span>
			</div>
			<Switch
				label={t('settings.general.alwaysOnTop')}
				describedby="k-ontop-help"
				checked={store.settings.alwaysOnTop}
				onchange={(v) => void save({ alwaysOnTop: v })}
			/>
		</div>
	</div>

	<div class="s-group">
		<h2 class="s-eyebrow">{t('settings.general.hotkeyGroup')}</h2>
		<div class="s-card s-row">
			<label class="s-label" for="k-hotkey">
				<span class="s-title">{t('settings.general.hotkey')}</span>
				<span class="s-help" class:error={!!hotkeyError} id="k-hotkey-help" aria-live="polite">
					{hotkeyError ?? t('settings.general.hotkeyHelp')}
				</span>
			</label>
			<div class="s-inline">
				<input
					id="k-hotkey"
					class="k-input k-mono hotkey"
					value={hotkey}
					aria-invalid={hotkeyError ? 'true' : undefined}
					aria-describedby="k-hotkey-help"
					spellcheck="false"
					autocomplete="off"
					oninput={(e) => {
						hotkey = e.currentTarget.value;
						editingHotkey = true;
						hotkeyError = null;
					}}
					onfocus={() => (editingHotkey = true)}
					onkeydown={onHotkeyKey}
				/>
				<button type="button" class="k-btn" disabled={hotkeySaving || hotkey.trim() === store.settings.hotkey} onclick={saveHotkey}>
					{t('settings.common.save')}
				</button>
			</div>
		</div>
	</div>

	<div class="s-group">
		<h2 class="s-eyebrow">{t('settings.general.units')}</h2>
		<UnitControl />
		<div class="s-card s-row">
			<div class="s-label">
				<span class="s-title">{t('settings.general.sound')}</span>
				<span class="s-help" id="k-sound-help">{t('settings.general.soundHelp')}</span>
			</div>
			<Switch
				label={t('settings.general.sound')}
				describedby="k-sound-help"
				checked={store.settings.notificationSound}
				onchange={(v) => void save({ notificationSound: v })}
			/>
		</div>
	</div>

	<div class="s-group">
		<h2 class="s-eyebrow">{t('settings.backup.title')}</h2>
		<div class="s-card s-row">
			<div class="s-label">
				<span class="s-title">{t('settings.backup.label')}</span>
				<span class="s-help">{t('settings.backup.help')}</span>
			</div>
			<div class="s-inline">
				<button type="button" class="k-btn" disabled={busy} onclick={chooseImport}><Icon name="upload" size={16} />{t('settings.backup.import')}</button>
				<button type="button" class="k-btn" disabled={busy} onclick={exportAll}><Icon name="download" size={16} />{t('settings.backup.export')}</button>
			</div>
		</div>
	</div>
</section>

<dialog bind:this={confirmDialog} class="k-dialog" aria-labelledby="k-import-title" onclose={() => (pendingImport = null)}>
	<form class="k-dialog-body" onsubmit={confirmImport}>
		<h2 id="k-import-title" class="k-dialog-title">{t('settings.backup.confirmTitle')}</h2>
		<p class="k-dialog-text">{env.tauri ? t('settings.backup.confirmBody', { path: BACKUPS_DIR }) : t('settings.backup.confirmBodyBrowser')}</p>
		<div class="k-dialog-actions">
			<button type="button" class="k-btn" onclick={() => confirmDialog?.close()}>{t('shell.workspace.cancel')}</button>
			<button type="submit" class="k-btn k-btn-danger k-btn-filled">{t('settings.backup.confirm')}</button>
		</div>
	</form>
</dialog>

<style>
	.hotkey {
		width: 200px;
	}
</style>
