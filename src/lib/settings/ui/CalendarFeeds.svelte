<script lang="ts">
	/**
	 * ICS feed list (SPEC §4.4 Calendar, §7): add / edit / enable / reorder / delete, per-feed status from
	 * `calendarUpcoming(1)` errors, and "Refresh now". URLs are masked to the host everywhere in the list;
	 * the edit dialog never pre-fills the stored URL (it may contain a secret token).
	 */
	import { onMount } from 'svelte';
	import { calendarRefresh, calendarUpcoming, hasBackend, on, type CalendarFeed, type CalendarUpcoming, type FeedColor } from '#lib/ipc/index.ts';
	import { t } from '#lib/i18n/index.ts';
	import Icon from '#lib/ui/Icon.svelte';
	import { checkFeedUrl, FEED_COLORS, feedColorVar, maskFeedUrl, moveFeed, newFeedId, nextFeedColor, type FeedUrlProblem } from '../feeds';
	import { errText, getSettingsContext } from './context.svelte';
	import Switch from './Switch.svelte';

	const { store, ui, save } = getSettingsContext();
	const feeds = $derived(store.settings.calendarFeeds);
	const tauri = hasBackend();

	let upcoming = $state<CalendarUpcoming | null>(null);
	let statusError = $state<string | null>(null);
	let refreshing = $state(false);

	async function loadStatus() {
		if (!tauri) return;
		try {
			upcoming = await calendarUpcoming(1);
			statusError = null;
		} catch (e) {
			statusError = errText(e);
		}
	}

	async function refresh(announce = true) {
		if (!tauri) return;
		refreshing = true;
		try {
			await calendarRefresh();
			await loadStatus();
			if (announce) ui.say(t('settings.feeds.refreshed'));
		} catch (e) {
			ui.say(t('settings.feeds.refreshFailed', { message: errText(e) }), 'warning', 6000);
		} finally {
			refreshing = false;
		}
	}

	onMount(() => {
		void loadStatus();
		const off = on('calendar://changed', () => void loadStatus());
		return () => void off.then((f) => f());
	});

	const errorOf = (id: string) => upcoming?.errors.find((e) => e.feedId === id)?.message ?? null;

	function statusText(f: CalendarFeed): { text: string; tone: 'muted' | 'error' | 'ok' } {
		if (!f.enabled) return { text: t('settings.feeds.statusOff'), tone: 'muted' };
		if (!tauri) return { text: t('settings.common.desktopOnly'), tone: 'muted' };
		const err = errorOf(f.id);
		if (err) return { text: t('settings.feeds.statusError', { message: err }), tone: 'error' };
		if (!upcoming) return { text: t('settings.feeds.statusPending'), tone: 'muted' };
		return { text: upcoming.stale ? t('settings.feeds.statusStale') : t('settings.feeds.statusOk'), tone: 'ok' };
	}

	/** Saves the list; a URL / enable change also refetches. */
	async function write(next: CalendarFeed[], refetch: boolean) {
		if ((await save({ calendarFeeds: next })) && refetch) void refresh(false);
	}

	// ── add / edit dialog ───────────────────────────────────────────────────

	let dialog = $state<HTMLDialogElement>();
	let editing = $state<CalendarFeed | null>(null);
	let name = $state('');
	let url = $state('');
	let color = $state<FeedColor>('accent');
	let problem = $state<FeedUrlProblem | null>(null);
	let swatches = $state<HTMLElement>();

	function openAdd() {
		editing = null;
		name = '';
		url = '';
		color = nextFeedColor(feeds);
		problem = null;
		dialog?.showModal();
	}

	function openEdit(f: CalendarFeed) {
		editing = f;
		name = f.name;
		url = '';
		color = f.color;
		problem = null;
		dialog?.showModal();
	}

	function submit(e: SubmitEvent) {
		e.preventDefault();
		const clean = name.trim();
		if (!clean) return;
		// Editing with an empty URL keeps the stored one.
		let nextUrl = editing?.url ?? '';
		if (!editing || url.trim()) {
			const checked = checkFeedUrl(url);
			if ('problem' in checked) {
				problem = checked.problem;
				return;
			}
			nextUrl = checked.url;
		}
		if (editing) {
			const urlChanged = nextUrl !== editing.url;
			const id = editing.id;
			void write(
				feeds.map((f) => (f.id === id ? { ...f, name: clean, url: nextUrl, color } : f)),
				urlChanged && editing.enabled
			);
		} else {
			const feed: CalendarFeed = { id: newFeedId(clean, feeds), name: clean, url: nextUrl, color, enabled: true };
			void write([...feeds, feed], true);
			ui.say(t('settings.feeds.added', { name: clean }));
		}
		dialog?.close();
	}

	function onSwatchKey(e: KeyboardEvent) {
		const dir = e.key === 'ArrowRight' || e.key === 'ArrowDown' ? 1 : e.key === 'ArrowLeft' || e.key === 'ArrowUp' ? -1 : 0;
		if (!dir) return;
		e.preventDefault();
		const i = FEED_COLORS.indexOf(color);
		color = FEED_COLORS[(i + dir + FEED_COLORS.length) % FEED_COLORS.length];
		requestAnimationFrame(() => swatches?.querySelector<HTMLElement>('[aria-checked="true"]')?.focus());
	}

	// ── row actions ─────────────────────────────────────────────────────────

	function toggle(f: CalendarFeed, enabled: boolean) {
		void write(
			feeds.map((x) => (x.id === f.id ? { ...x, enabled } : x)),
			true
		);
	}

	function move(f: CalendarFeed, dir: -1 | 1) {
		void write(moveFeed(feeds, f.id, dir), false);
		// Keep focus on the same control of the moved row.
		requestAnimationFrame(() => document.getElementById(`k-feed-${f.id}-${dir < 0 ? 'up' : 'down'}`)?.focus());
	}

	function remove(f: CalendarFeed) {
		const index = feeds.findIndex((x) => x.id === f.id);
		void write(
			feeds.filter((x) => x.id !== f.id),
			false
		);
		ui.say(t('settings.feeds.removed', { name: f.name }), 'info', 5000, {
			label: t('shell.edit.undo'),
			run: () => {
				if (feeds.some((x) => x.id === f.id)) return;
				const next = [...feeds];
				next.splice(Math.min(index, next.length), 0, f);
				void write(next, f.enabled);
			}
		});
	}

	const problemText = (p: FeedUrlProblem) => t(`settings.feeds.urlErrors.${p}`);
</script>

<div class="s-group">
	<div class="s-inline between">
		<h2 class="s-eyebrow">{t('settings.feeds.list', { count: feeds.length })}</h2>
		<div class="s-inline">
			<button type="button" class="k-btn s-btn-sm" disabled={!tauri || refreshing || !feeds.some((f) => f.enabled)} onclick={() => refresh()}>
				<Icon name="refresh" size={16} />{t('settings.feeds.refresh')}
			</button>
			<button type="button" class="k-btn k-btn-primary s-btn-sm" onclick={openAdd}>
				<Icon name="plus" size={16} stroke={2.4} />{t('settings.feeds.add')}
			</button>
		</div>
	</div>

	{#if statusError}<p class="s-help error">{t('settings.feeds.statusError', { message: statusError })}</p>{/if}

	{#if feeds.length === 0}
		<p class="s-card s-note">{t('settings.feeds.empty')}</p>
	{:else}
		<ul class="s-list">
			{#each feeds as f, i (f.id)}
				{@const st = statusText(f)}
				<li class="s-card item" class:off={!f.enabled}>
					<span class="bar" style:background={feedColorVar(f.color)} aria-hidden="true"></span>
					<div class="s-label grow">
						<span class="s-title name">{f.name}</span>
						<span class="s-path masked">{maskFeedUrl(f.url)}</span>
						<span class="s-help" class:error={st.tone === 'error'} aria-live="polite">{st.text}</span>
					</div>
					<div class="actions">
						<Switch label={t('settings.feeds.enableOne', { name: f.name })} checked={f.enabled} onchange={(v) => toggle(f, v)} />
						<button
							id="k-feed-{f.id}-up"
							type="button"
							class="k-btn k-btn-icon"
							aria-label={t('settings.feeds.moveUp', { name: f.name })}
							title={t('settings.feeds.moveUp', { name: f.name })}
							disabled={i === 0}
							onclick={() => move(f, -1)}
						>
							<Icon name="chevronUp" size={16} />
						</button>
						<button
							id="k-feed-{f.id}-down"
							type="button"
							class="k-btn k-btn-icon"
							aria-label={t('settings.feeds.moveDown', { name: f.name })}
							title={t('settings.feeds.moveDown', { name: f.name })}
							disabled={i === feeds.length - 1}
							onclick={() => move(f, 1)}
						>
							<Icon name="chevronDown" size={16} />
						</button>
						<button type="button" class="k-btn k-btn-icon" aria-label={t('settings.feeds.editOne', { name: f.name })} title={t('settings.feeds.edit')} onclick={() => openEdit(f)}>
							<Icon name="edit" size={16} />
						</button>
						<button type="button" class="k-btn k-btn-icon danger" aria-label={t('settings.feeds.deleteOne', { name: f.name })} title={t('settings.feeds.delete')} onclick={() => remove(f)}>
							<Icon name="trash" size={16} />
						</button>
					</div>
				</li>
			{/each}
		</ul>
	{/if}
	<p class="s-note small">{t('settings.feeds.help')}</p>
</div>

<dialog bind:this={dialog} class="k-dialog" aria-labelledby="k-feed-title">
	<form class="k-dialog-body" onsubmit={submit} novalidate>
		<h2 id="k-feed-title" class="k-dialog-title">{editing ? t('settings.feeds.editTitle', { name: editing.name }) : t('settings.feeds.addTitle')}</h2>
		<label class="k-label">
			{t('settings.feeds.name')}
			<!-- svelte-ignore a11y_autofocus -->
			<input class="k-input" bind:value={name} maxlength="40" required autofocus autocomplete="off" />
		</label>
		<label class="k-label">
			{t('settings.feeds.url')}
			<input
				class="k-input k-mono url"
				type="url"
				bind:value={url}
				required={!editing}
				placeholder={editing ? t('settings.feeds.urlKeep', { host: maskFeedUrl(editing.url) }) : 'https://… .ics'}
				spellcheck="false"
				autocomplete="off"
				aria-invalid={problem ? 'true' : undefined}
				aria-describedby="k-feed-url-help"
				oninput={() => (problem = null)}
			/>
			<span id="k-feed-url-help" class="s-help" class:error={!!problem} aria-live="polite">
				{problem ? problemText(problem) : t('settings.feeds.urlHelp')}
			</span>
		</label>
		<div class="k-label">
			<span id="k-feed-color">{t('settings.feeds.color')}</span>
			<div bind:this={swatches} class="swatches" role="radiogroup" aria-labelledby="k-feed-color">
				{#each FEED_COLORS as c (c)}
					<button
						type="button"
						role="radio"
						class="swatch"
						aria-checked={color === c}
						aria-label={t(`settings.feeds.colors.${c}`)}
						title={t(`settings.feeds.colors.${c}`)}
						tabindex={color === c ? 0 : -1}
						onkeydown={onSwatchKey}
						onclick={() => (color = c)}
					>
						<span style:background={feedColorVar(c)}></span>
					</button>
				{/each}
			</div>
		</div>
		<div class="k-dialog-actions">
			<button type="button" class="k-btn" onclick={() => dialog?.close()}>{t('shell.workspace.cancel')}</button>
			<button type="submit" class="k-btn k-btn-primary" disabled={!name.trim() || (!editing && !url.trim())}>
				{editing ? t('shell.workspace.save') : t('settings.feeds.add')}
			</button>
		</div>
	</form>
</dialog>

<style>
	.between {
		justify-content: space-between;
	}
	.small {
		font-size: 12px;
	}
	.item {
		display: flex;
		align-items: center;
		gap: 12px;
		padding: 8px 8px 8px 0;
		overflow: hidden;
	}
	.item.off .grow {
		opacity: 0.55;
	}
	.bar {
		align-self: stretch;
		flex: none;
		width: 4px;
		border-radius: 0 4px 4px 0;
	}
	.grow {
		flex: 1;
	}
	.name {
		font-weight: 600;
		white-space: nowrap;
		overflow: hidden;
		text-overflow: ellipsis;
	}
	.masked {
		white-space: nowrap;
		overflow: hidden;
		text-overflow: ellipsis;
	}
	.actions {
		display: flex;
		align-items: center;
		gap: 2px;
		flex: none;
	}
	.actions :global(.k-switch) {
		margin-right: 6px;
	}
	.actions .k-btn-icon {
		border-color: transparent;
	}
	.actions .k-btn-icon:disabled {
		opacity: 0.35;
		cursor: default;
	}
	.actions .danger:hover {
		color: var(--k-danger);
	}
	.url {
		font-size: 12px;
	}
	.swatches {
		display: flex;
		gap: 6px;
		flex-wrap: wrap;
	}
	.swatch {
		width: 44px;
		height: 44px;
		padding: 0;
		display: flex;
		align-items: center;
		justify-content: center;
		border-radius: 12px;
		border: 1px solid var(--k-border);
		background: var(--k-bg-alt);
	}
	.swatch span {
		width: 22px;
		height: 22px;
		border-radius: 50%;
	}
	.swatch[aria-checked='true'] {
		border: 2px solid var(--k-accent);
	}
</style>
