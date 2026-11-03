<script lang="ts">
	import { tick } from 'svelte';
	import { locale, t } from '#lib/i18n/index.ts';
	import type { Todo } from '#lib/ipc/index.ts';
	import { formatWeekday, isDeckVisible, onDeckVisibility, subscribeTick } from '#lib/time/index.ts';
	import type { WidgetProps } from '../types';
	import {
		applyOrder,
		dragTarget,
		dueChip,
		dueRow,
		dueSoon,
		groupByDay,
		moveBy,
		openCount,
		pickVisible,
		reorderVisible,
		rowsThatFit,
		sortTodos,
		SWIPE_REVEAL_PX,
		type GroupKey
	} from './logic';
	import { parseQuickAdd } from './parse';
	import { defaults, type TodoSettings } from './settings';
	import { todoApi } from './store';

	let { size, settings, editing }: WidgetProps<TodoSettings> = $props();

	const s = $derived({ ...defaults, ...settings });
	const api = todoApi();
	/** DOM id prefix; the random part keeps two instances apart even if their instance ids collide. */
	const uid = `todo-${Math.random().toString(36).slice(2, 9)}`;

	/** Row height and gap (36 px rows: secondary in-card controls, DESIGN §9). */
	const ROW = 36;
	const GAP = 4;
	/** Extra height of a row showing Snooze / Dismiss. */
	const ALERT_EXTRA = 40;
	/** DESIGN §5.9: 4×2 shows at most 6 rows in 2 columns. */
	const WIDE_MAX_PER_COL = 3;

	let todos = $state<Todo[]>([]);
	let loaded = $state(false);
	let failed = $state(false);
	let saveError = $state(false);
	let saveErrorTimer: ReturnType<typeof setTimeout> | null = null;

	let loadSeq = 0;
	async function load() {
		const seq = ++loadSeq;
		try {
			const list = await api.list();
			if (seq !== loadSeq) return;
			todos = list;
			loaded = true;
			failed = false;
		} catch {
			if (seq !== loadSeq) return;
			failed = !loaded;
		}
	}

	function flagSaveError() {
		saveError = true;
		if (saveErrorTimer) clearTimeout(saveErrorTimer);
		saveErrorTimer = setTimeout(() => (saveError = false), 4000);
		load();
	}

	// ── Reminders: pulse once + inline Snooze / Dismiss ─────────────────────
	let alerts = $state<number[]>([]);
	/** Per-row pulse counter; parity switches between two identical keyframes so it replays. */
	let pulses = $state<Record<number, number>>({});
	let visible = $state(isDeckVisible());
	/** Reminders that fired while hidden pulse when the deck is shown. */
	let pulseOnShow = new Set<number>();

	function pulse(id: number) {
		pulses = { ...pulses, [id]: (pulses[id] ?? 0) + 1 };
	}

	function onReminder(id: number) {
		if (!alerts.includes(id)) alerts = [...alerts, id];
		if (visible) pulse(id);
		else pulseOnShow.add(id);
		load();
	}

	$effect(() => {
		load();
		const offChanged = api.onChanged(load);
		const offReminder = api.onReminder(onReminder);
		return () => {
			offChanged();
			offReminder();
			loadSeq++;
			if (saveErrorTimer) clearTimeout(saveErrorTimer);
		};
	});

	$effect(() =>
		onDeckVisibility((v) => {
			visible = v;
			if (!v) return;
			load();
			for (const id of pulseOnShow) pulse(id);
			pulseOnShow = new Set();
		})
	);

	let now = $state(Date.now());
	$effect(() => subscribeTick((d) => (now = d.getTime())));

	const words = $derived({ today: t('widgets.todo.today'), tomorrow: t('widgets.todo.tomorrow'), yesterday: t('widgets.todo.yesterday') });

	const sorted = $derived(sortTodos(todos).filter((x) => s.showCompleted || !x.done));
	const left = $derived(openCount(todos));
	const liveAlerts = $derived(new Set(alerts.filter((id) => todos.some((x) => x.id === id && !x.done))));

	// ── Fitted rows (2×2, 4×2); 4×4 scrolls its grouped list ────────────────
	let listH = $state(0);
	const cap = $derived.by(() => {
		const extra = liveAlerts.size * ALERT_EXTRA;
		if (size === '4x2') return 2 * Math.min(WIDE_MAX_PER_COL, rowsThatFit(listH - extra, ROW, GAP));
		return rowsThatFit(listH - extra, ROW, GAP);
	});
	const shown = $derived(size === '4x4' ? sorted : pickVisible(sorted, cap, liveAlerts));
	const hidden = $derived(Math.max(0, sorted.length - shown.length));
	const groups = $derived(size === '4x4' ? groupByDay(sorted, now) : []);
	const wideRows = $derived(Math.max(1, Math.ceil(shown.length / 2)));

	function groupLabel(key: GroupKey, at?: number): string {
		if (key.startsWith('day:') && at !== undefined) return formatWeekday(new Date(at), locale, 'long');
		return t(`widgets.todo.group.${key}`);
	}

	const meta = $derived(
		[t('widgets.todo.left', { n: left }), hidden > 0 && size !== '4x4' ? t('widgets.todo.more', { n: hidden }) : null]
			.filter(Boolean)
			.join(' · ')
	);

	// ── Quick add ──────────────────────────────────────────────────────────
	let draft = $state('');
	const parsed = $derived(draft.trim() ? parseQuickAdd(draft, new Date()) : null);
	const chip = $derived(parsed?.due != null ? dueChip(parsed.due, Date.now(), locale, words) : '');
	let chipW = $state(0);

	async function submit(e: SubmitEvent) {
		e.preventDefault();
		if (editing) return;
		const raw = draft.trim();
		if (!raw) return;
		const p = parseQuickAdd(raw, new Date());
		// "bugün 15:00" alone: keep the typed words as the title rather than an empty task.
		const text = p.text || raw;
		const remindAt = s.remind && p.due !== null ? p.due : null;
		draft = '';
		try {
			const created = await api.add(text, p.due, remindAt);
			if (!todos.some((x) => x.id === created.id)) todos = [...todos, created];
		} catch {
			draft = raw;
			flagSaveError();
		}
	}

	function onDraftKey(e: KeyboardEvent) {
		if (e.key === 'Escape' && draft) {
			e.preventDefault();
			draft = '';
		}
	}

	// ── Row actions ─────────────────────────────────────────────────────────
	async function toggle(item: Todo, done: boolean) {
		todos = todos.map((x) => (x.id === item.id ? { ...x, done, completedAt: done ? Date.now() : null } : x));
		if (done) alerts = alerts.filter((id) => id !== item.id);
		try {
			await api.update(item.id, { done });
		} catch {
			flagSaveError();
		}
	}

	async function remove(item: Todo) {
		todos = todos.filter((x) => x.id !== item.id);
		alerts = alerts.filter((id) => id !== item.id);
		revealed = null;
		try {
			await api.remove(item.id);
		} catch {
			flagSaveError();
		}
	}

	async function commitOrder(ids: number[]) {
		todos = applyOrder(todos, ids);
		try {
			await api.reorder(ids);
		} catch {
			flagSaveError();
		}
	}

	async function snooze(id: number) {
		alerts = alerts.filter((x) => x !== id);
		try {
			await api.snooze(id, 10);
		} catch {
			flagSaveError();
		}
	}

	function dismiss(id: number) {
		alerts = alerts.filter((x) => x !== id);
		// Fired reminders shouldn't fire again after a restart.
		api.update(id, { remindAt: null }).catch(() => {});
	}

	const checkId = (id: number) => `${uid}-c${id}`;

	let announce = $state('');
	async function onCheckKey(e: KeyboardEvent, item: Todo) {
		if (editing) return;
		if (e.altKey && (e.key === 'ArrowUp' || e.key === 'ArrowDown')) {
			e.preventDefault();
			if (item.done) return;
			const ids = moveBy(todos, item.id, e.key === 'ArrowUp' ? -1 : 1);
			if (!ids) return;
			commitOrder(ids);
			announce = t('widgets.todo.moved', { n: ids.indexOf(item.id) + 1 });
			// Moving a focused node in the DOM drops focus; put it back.
			await tick();
			document.getElementById(checkId(item.id))?.focus();
		} else if (e.key === 'Delete') {
			e.preventDefault();
			revealed = item.id;
			await tick();
			document.getElementById(`${uid}-d${item.id}`)?.focus();
		} else if (e.key === 'Escape' && revealed !== null) {
			revealed = null;
		}
	}

	// ── Pointer gestures: swipe left reveals Delete; the grip drags to reorder ─
	let revealed = $state<number | null>(null);
	let swipe = $state<{ id: number; dx: number } | null>(null);
	let drag = $state<{ id: number; dy: number; target: number; from: number; ids: number[] } | null>(null);
	let gesture: { id: number; pointerId: number; x0: number; y0: number; mode: 'pending' | 'swipe' | 'scroll' } | null = null;
	let suppressClick = false;
	let gripStart: { y0: number; pointerId: number } | null = null;

	function rowDown(e: PointerEvent, item: Todo) {
		if (editing || (e.pointerType === 'mouse' && e.button !== 0)) return;
		if ((e.target as Element).closest('button, .grip')) return;
		gesture = { id: item.id, pointerId: e.pointerId, x0: e.clientX, y0: e.clientY, mode: 'pending' };
	}

	function rowMove(e: PointerEvent) {
		const g = gesture;
		if (!g || e.pointerId !== g.pointerId) return;
		const dx = e.clientX - g.x0;
		const dy = e.clientY - g.y0;
		if (g.mode === 'pending') {
			if (Math.abs(dx) > 10 && Math.abs(dx) > Math.abs(dy)) {
				g.mode = 'swipe';
				(e.currentTarget as Element).setPointerCapture?.(e.pointerId);
			} else if (Math.abs(dy) > 10) g.mode = 'scroll';
		}
		if (g.mode === 'swipe') {
			const base = revealed === g.id ? -SWIPE_REVEAL_PX : 0;
			swipe = { id: g.id, dx: Math.max(-SWIPE_REVEAL_PX * 1.5, Math.min(0, base + dx)) };
		}
	}

	function rowUp(e: PointerEvent) {
		const g = gesture;
		if (!g || e.pointerId !== g.pointerId) return;
		gesture = null;
		if (g.mode === 'swipe') {
			suppressClick = true;
			setTimeout(() => (suppressClick = false), 0);
			revealed = (swipe?.dx ?? 0) < -SWIPE_REVEAL_PX / 2 ? g.id : revealed === g.id ? null : revealed;
		} else if (g.mode === 'pending' && revealed !== null && revealed !== g.id) {
			revealed = null;
		}
		swipe = null;
	}

	function onRowClick(e: MouseEvent) {
		if (suppressClick) {
			e.preventDefault();
			e.stopPropagation();
		}
	}

	function gripDown(e: PointerEvent, item: Todo, ids: number[]) {
		if (editing || item.done || (e.pointerType === 'mouse' && e.button !== 0)) return;
		e.preventDefault();
		(e.currentTarget as Element).setPointerCapture?.(e.pointerId);
		const from = ids.indexOf(item.id);
		gripStart = { y0: e.clientY, pointerId: e.pointerId };
		drag = { id: item.id, dy: 0, target: from, from, ids };
	}

	function gripMove(e: PointerEvent) {
		if (!drag || !gripStart || e.pointerId !== gripStart.pointerId) return;
		const dy = e.clientY - gripStart.y0;
		drag = { ...drag, dy, target: dragTarget(drag.from, dy, ROW + GAP, drag.ids.length) };
	}

	function gripUp(e: PointerEvent) {
		if (!drag || !gripStart || e.pointerId !== gripStart.pointerId) return;
		const d = drag;
		drag = null;
		gripStart = null;
		const ids = reorderVisible(todos, d.ids, d.id, d.target);
		if (ids) commitOrder(ids);
	}

	function gripCancel() {
		drag = null;
		gripStart = null;
	}

	/** Visual drop marker: above the target row when moving up, below it when moving down. */
	const dropMark = (id: number): 'above' | 'below' | null => {
		if (!drag || drag.target === drag.from) return null;
		if (drag.ids[drag.target] !== id) return null;
		return drag.target < drag.from ? 'above' : 'below';
	};
</script>

{#snippet trashIcon()}
	<svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
		<path d="M3 6h18M8 6V4h8v2M19 6l-1 14H6L5 6" />
	</svg>
{/snippet}

{#snippet row(item: Todo, ids: number[])}
	{@const alerting = liveAlerts.has(item.id)}
	{@const soon = dueSoon(item, now)}
	{@const dr = item.due !== null ? dueRow(item.due, now, locale) : null}
	{@const pulseN = pulses[item.id] ?? 0}
	{@const showDelete = size === '4x4' || revealed === item.id}
	<!-- data-no-gesture: rows own swipe/drag; the deck's long-press and workspace swipe start elsewhere on the card. -->
	<li
		data-no-gesture
		class="row"
		class:done={item.done}
		class:alerting
		class:dragging={drag?.id === item.id}
		class:swiping={swipe?.id === item.id}
		data-drop={dropMark(item.id)}
		data-pulse={pulseN ? (pulseN % 2 ? 'a' : 'b') : undefined}
		style:transform={drag?.id === item.id ? `translateY(${drag.dy}px)` : undefined}
		onpointerdown={(e) => rowDown(e, item)}
		onpointermove={rowMove}
		onpointerup={rowUp}
		onpointercancel={rowUp}
		onclickcapture={onRowClick}
	>
		<div class="line" style:transform={swipe?.id === item.id ? `translateX(${swipe.dx}px)` : undefined}>
			{#if !item.done}
				<span
					class="grip"
					role="presentation"
					title={t('widgets.todo.reorder', { text: item.text })}
					onpointerdown={(e) => gripDown(e, item, ids)}
					onpointermove={gripMove}
					onpointerup={gripUp}
					onpointercancel={gripCancel}
				>
					<svg width="10" height="16" viewBox="0 0 10 16" fill="currentColor" aria-hidden="true">
						<circle cx="3" cy="3" r="1.4" /><circle cx="7" cy="3" r="1.4" /><circle cx="3" cy="8" r="1.4" /><circle cx="7" cy="8" r="1.4" /><circle cx="3" cy="13" r="1.4" /><circle cx="7" cy="13" r="1.4" />
					</svg>
				</span>
			{:else}
				<span class="grip-space" aria-hidden="true"></span>
			{/if}
			<label class="main">
				<input
					id={checkId(item.id)}
					type="checkbox"
					class="check"
					checked={item.done}
					disabled={editing}
					aria-describedby={`${uid}-keys`}
					onchange={(e) => toggle(item, e.currentTarget.checked)}
					onkeydown={(e) => onCheckKey(e, item)}
				/>
				<span class="text">{item.text}</span>
			</label>
			{#if dr && !item.done && !(showDelete && size !== '4x4')}
				<span class="due mono" class:soon class:has-day={dr.day && dr.time}>
					{#if dr.day}<span class="due-day">{dr.day}</span>{/if}
					{#if dr.time}<span class="due-time">{dr.time}</span>{/if}
				</span>
			{/if}
			{#if showDelete}
				<button
					id={`${uid}-d${item.id}`}
					type="button"
					class="del"
					class:revealed={size !== '4x4'}
					aria-label={t('widgets.todo.deleteItem', { text: item.text })}
					disabled={editing}
					onclick={() => remove(item)}
					onkeydown={(e) => e.key === 'Escape' && (revealed = null)}
				>
					{@render trashIcon()}
				</button>
			{/if}
		</div>
		{#if alerting}
			<div class="alert" role="group" aria-label={t('widgets.todo.reminder')}>
				<span class="alert-label">{t('widgets.todo.reminder')}</span>
				<button type="button" class="act" disabled={editing} onclick={() => snooze(item.id)}>{t('widgets.todo.snooze')}</button>
				<button type="button" class="act ghost" disabled={editing} onclick={() => dismiss(item.id)}>{t('widgets.todo.dismiss')}</button>
			</div>
		{/if}
	</li>
{/snippet}

<section class="k-card todo" data-size={size} data-attention={liveAlerts.size ? 'true' : undefined} aria-label={t('widgets.todo.list')}>
	<header class="hdr">
		<span class="title">{t('widgets.todo.title')}</span>
		{#if loaded}
			<span class="meta" class:warn-text={saveError}>{saveError ? t('widgets.todo.saveFailed') : meta}</span>
		{/if}
	</header>

	<div class="body" bind:clientHeight={listH}>
		{#if failed}
			<div class="state">
				<span class="state-title">{t('widgets.todo.error')}</span>
				<button type="button" class="k-btn" disabled={editing} onclick={load}>{t('widgets.common.retry')}</button>
			</div>
		{:else if !loaded}
			<div class="skeleton" aria-busy="true" aria-label={t('widgets.common.loading')}>
				<span class="bar"></span><span class="bar"></span><span class="bar" style="width: 60%"></span>
			</div>
		{:else if sorted.length === 0}
			<div class="state">
				<svg width="22" height="22" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
					<path d="M9 11l3 3L22 4" /><path d="M21 12v7a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h11" />
				</svg>
				<span class="state-title">{t('widgets.todo.empty')}</span>
				<span class="state-text">{t('widgets.todo.emptyHint')}</span>
			</div>
		{:else if size === '4x4'}
			<div class="scroll">
				{#each groups as g (g.key)}
					{@const ids = g.items.filter((x) => !x.done).map((x) => x.id)}
					<section class="group" aria-label={groupLabel(g.key, g.at)}>
						<h3 class="ghead">{groupLabel(g.key, g.at)}</h3>
						<ul class="rows">
							{#each g.items as item (item.id)}
								{@render row(item, ids)}
							{/each}
						</ul>
					</section>
				{/each}
			</div>
		{:else}
			{@const ids = shown.filter((x) => !x.done).map((x) => x.id)}
			<ul class="rows" class:wide={size === '4x2'} style:--rows={wideRows}>
				{#each shown as item (item.id)}
					{@render row(item, ids)}
				{/each}
			</ul>
		{/if}
	</div>

	<form class="add" onsubmit={submit}>
		<input
			class="field"
			type="text"
			enterkeyhint="done"
			autocomplete="off"
			spellcheck="false"
			bind:value={draft}
			placeholder={t('widgets.todo.placeholder')}
			aria-label={t('widgets.todo.addLabel')}
			aria-describedby={chip ? `${uid}-chip` : undefined}
			disabled={editing}
			onkeydown={onDraftKey}
			style:padding-right={chip ? `${chipW + 16}px` : undefined}
		/>
		{#if chip}
			<span id={`${uid}-chip`} class="chip mono" bind:offsetWidth={chipW} aria-live="polite">
				<span class="k-sr-only">{t('widgets.todo.dueChip', { when: '' })}</span>{chip}
			</span>
		{/if}
	</form>
	<p id={`${uid}-keys`} class="k-sr-only">{t('widgets.todo.keysHint')}</p>
	<p class="k-sr-only" aria-live="polite">{announce}</p>
</section>

<style>
	.todo {
		--warn-text: color-mix(in srgb, var(--k-warning) 35%, var(--k-text));
		container-type: size;
		padding: 18px 20px;
		gap: 10px;
		font-family: var(--k-font-ui);
	}
	.todo[data-size='4x2'],
	.todo[data-size='4x4'] {
		padding: 18px 22px;
	}
	.mono {
		font-family: var(--k-font-mono);
		font-variant-numeric: tabular-nums;
	}
	.hdr {
		display: flex;
		justify-content: space-between;
		align-items: center;
		gap: 12px;
		min-width: 0;
		flex-shrink: 0;
	}
	.title {
		font-size: 13px;
		font-weight: 600;
		flex-shrink: 0;
	}
	.meta {
		font-size: 11px;
		color: var(--k-text-muted);
		white-space: nowrap;
		overflow: hidden;
		text-overflow: ellipsis;
		min-width: 0;
	}
	.warn-text {
		color: var(--warn-text);
	}

	.body {
		flex: 1;
		min-height: 0;
		position: relative;
		display: flex;
		flex-direction: column;
	}
	.rows {
		margin: 0;
		padding: 0;
		list-style: none;
		display: flex;
		flex-direction: column;
		gap: 4px;
		min-width: 0;
	}
	.rows.wide {
		display: grid;
		grid-auto-flow: column;
		grid-template-columns: repeat(2, minmax(0, 1fr));
		grid-template-rows: repeat(var(--rows), auto);
		align-content: start;
		column-gap: 20px;
		row-gap: 4px;
	}

	.row {
		position: relative;
		border: 1px solid transparent;
		border-radius: 10px;
		min-width: 0;
		touch-action: pan-y;
	}
	.row.dragging {
		z-index: 2;
		background: var(--k-surface);
		border-color: var(--k-accent);
	}
	.row[data-drop='above']::before,
	.row[data-drop='below']::after {
		content: '';
		position: absolute;
		left: 8px;
		right: 8px;
		height: 2px;
		border-radius: 1px;
		background: var(--k-accent);
	}
	.row[data-drop='above']::before {
		top: -3px;
	}
	.row[data-drop='below']::after {
		bottom: -3px;
	}
	.row.alerting {
		background: color-mix(in srgb, var(--k-warning) 12%, transparent);
	}
	/* Reminder fired: warning border for 300 ms, once (DESIGN §5.9). Two names so a repeat replays. */
	.row[data-pulse='a'] {
		animation: pulse-a 300ms ease-out 1;
	}
	.row[data-pulse='b'] {
		animation: pulse-b 300ms ease-out 1;
	}
	@keyframes pulse-a {
		0%,
		70% {
			border-color: var(--k-warning);
		}
		100% {
			border-color: transparent;
		}
	}
	@keyframes pulse-b {
		0%,
		70% {
			border-color: var(--k-warning);
		}
		100% {
			border-color: transparent;
		}
	}

	.line {
		display: flex;
		align-items: center;
		gap: 6px;
		height: 36px;
		min-width: 0;
		padding-right: 2px;
		transition: transform 150ms ease;
	}
	.row.swiping .line {
		transition: none;
	}
	.grip,
	.grip-space {
		flex-shrink: 0;
		width: 24px;
		height: 36px;
	}
	.grip {
		position: relative;
		display: flex;
		align-items: center;
		justify-content: center;
		color: var(--k-text-muted);
		opacity: 0.6;
		cursor: grab;
		touch-action: none;
	}
	/* Dots stay 10 px; the hit area is 36×36 (spills 6 px into the row edge and the gap). */
	.grip::before {
		content: '';
		position: absolute;
		inset: 0 -6px;
	}
	.row.dragging .grip {
		cursor: grabbing;
		opacity: 1;
	}
	.main {
		flex: 1;
		min-width: 0;
		height: 100%;
		display: flex;
		align-items: center;
		gap: 10px;
		font-size: 13px;
		cursor: pointer;
	}
	.check {
		flex-shrink: 0;
		width: 18px;
		height: 18px;
		margin: 0;
		accent-color: var(--k-accent);
		cursor: pointer;
	}
	.text {
		min-width: 0;
		white-space: nowrap;
		overflow: hidden;
		text-overflow: ellipsis;
	}
	.row.done .main {
		color: var(--k-text-muted);
	}
	.row.done .text {
		text-decoration: line-through;
	}
	.due {
		flex-shrink: 0;
		font-size: 11px;
		color: var(--k-text-muted);
		white-space: nowrap;
	}
	.due.soon {
		color: var(--warn-text);
	}
	.due.has-day .due-time {
		margin-left: 0.5ch;
	}
	.del {
		flex-shrink: 0;
		width: 36px;
		height: 36px;
		display: flex;
		align-items: center;
		justify-content: center;
		border-radius: 10px;
		border: 1px solid transparent;
		background: transparent;
		color: var(--k-text-muted);
	}
	.del:hover:not(:disabled) {
		color: var(--k-danger);
		border-color: var(--k-border);
	}
	.del.revealed {
		color: var(--k-danger);
		border-color: var(--k-danger);
		background: color-mix(in srgb, var(--k-danger) 14%, transparent);
	}

	.alert {
		display: flex;
		align-items: center;
		gap: 6px;
		padding: 0 4px 4px 30px;
		min-width: 0;
	}
	.alert-label {
		flex: 1;
		min-width: 0;
		font-size: 11px;
		color: var(--warn-text);
		white-space: nowrap;
		overflow: hidden;
		text-overflow: ellipsis;
	}
	.act {
		flex-shrink: 0;
		height: 36px;
		padding: 0 10px;
		border-radius: 10px;
		border: 1px solid var(--k-warning);
		background: transparent;
		color: var(--k-text);
		font-size: 12px;
		font-weight: 600;
		white-space: nowrap;
	}
	.act.ghost {
		border-color: var(--k-border);
		color: var(--k-text-muted);
	}

	/* 4×4: grouped, scrolling inside the card body with sticky day headers. */
	.scroll {
		flex: 1;
		min-height: 0;
		overflow-y: auto;
		overscroll-behavior: contain;
		scrollbar-width: thin;
		margin: 0 -10px 0 -6px;
		padding: 0 10px 0 6px;
	}
	.group + .group {
		margin-top: 6px;
	}
	.ghead {
		position: sticky;
		top: 0;
		z-index: 1;
		margin: 0;
		padding: 4px 0 6px;
		/* Same layer as the card (so no block over a wallpaper); blur keeps scrolled rows from reading through. */
		background: var(--k-surface-a);
		backdrop-filter: blur(8px);
		font-size: 11px;
		font-weight: 400;
		color: var(--k-text-muted);
		text-transform: uppercase;
		letter-spacing: 0.08em;
	}

	@supports not (backdrop-filter: blur(1px)) {
		.ghead {
			background: color-mix(in srgb, var(--k-surface) 92%, transparent);
		}
	}

	/* Quick add (DESIGN §5.9): 40 px field on bg-alt, chip inside while a date is recognised. */
	.add {
		position: relative;
		flex-shrink: 0;
		margin: 0;
	}
	.field {
		box-sizing: border-box;
		width: 100%;
		height: 40px;
		padding: 0 12px;
		border-radius: 10px;
		border: 1px solid var(--k-border);
		background: var(--k-bg-alt);
		color: var(--k-text);
		font: 13px var(--k-font-ui);
	}
	.field::placeholder {
		color: var(--k-text-muted);
	}
	.field:disabled {
		opacity: 0.55;
	}
	.chip {
		position: absolute;
		right: 6px;
		top: 50%;
		transform: translateY(-50%);
		max-width: 55%;
		height: 26px;
		display: flex;
		align-items: center;
		padding: 0 8px;
		border-radius: 8px;
		background: color-mix(in srgb, var(--k-accent) 16%, var(--k-surface));
		border: 1px solid color-mix(in srgb, var(--k-accent) 45%, transparent);
		color: var(--k-text);
		font-size: 11px;
		white-space: nowrap;
		overflow: hidden;
		text-overflow: ellipsis;
		pointer-events: none;
	}

	/* States */
	.state {
		flex: 1;
		display: flex;
		flex-direction: column;
		align-items: center;
		justify-content: center;
		gap: 6px;
		text-align: center;
		color: var(--k-text-muted);
	}
	.state-title {
		font-size: 13px;
		font-weight: 600;
		color: var(--k-text);
	}
	.state-text {
		font-size: 12px;
	}
	.skeleton {
		display: flex;
		flex-direction: column;
		gap: 10px;
		padding-top: 4px;
	}
	.bar {
		display: block;
		height: 22px;
		border-radius: 8px;
		background: var(--k-bg-alt);
	}

	/* Narrow 2×2 (1024×600): rows keep the day, the time goes; the reminder keeps only its buttons. */
	@container (width < 250px) {
		.todo[data-size='2x2'] .due.has-day .due-time {
			display: none;
		}
	}
	@container (width < 520px) {
		.todo[data-size='4x2'] .due.has-day .due-time,
		.todo[data-size='4x2'] .alert-label {
			display: none;
		}
		.todo[data-size='4x2'] .alert {
			justify-content: flex-end;
			padding-left: 4px;
		}
	}
	@container (width < 300px) {
		.todo[data-size='2x2'] .alert-label {
			display: none;
		}
		.todo[data-size='2x2'] .alert {
			justify-content: flex-end;
			padding-left: 4px;
		}
	}
	/* 1024×600: the empty state drops its icon and hint first. */
	@container (height < 230px) {
		.todo[data-size='2x2'] .state svg,
		.todo[data-size='2x2'] .state-text {
			display: none;
		}
	}

	/* The pulse is a colour change, not motion, so it stays; the swipe snap-back does not. */
	@media (prefers-reduced-motion: reduce) {
		.line {
			transition: none;
		}
	}
</style>
