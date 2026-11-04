<script lang="ts">
	/**
	 * Cell grid (DESIGN §4.3) + edit-mode interactions (DESIGN §6) with pointer events (touch + mouse):
	 * long-press 500 ms → edit + lift, drag → snap + shift/reject, corner handle → size cycling,
	 * bottom-edge trash, swipe left/right → switch workspace. Keyboard: arrows move, S sizes, Delete removes.
	 */
	import type { WidgetInstance } from '#lib/ipc/index.ts';
	import { t } from '#lib/i18n/index.ts';
	import {
		cellAt,
		closestSize,
		layout,
		moveItem,
		nextSize,
		resizeItem,
		snapDrag,
		type GridGeometry,
		type Size
	} from '#lib/grid/index.ts';
	import Icon from './Icon.svelte';
	import WidgetFrame from './WidgetFrame.svelte';
	import { fmtSize, getDeck, removeWithUndo, sizesOf, widgetName } from './deck.svelte';
	import { plugins } from '#lib/plugins/registry.svelte.ts';

	const LONG_PRESS_MS = 500;
	const SWIPE_PX = 64;
	const DRAG_START_PX = 6;
	/** Pointer this close to the grid's bottom edge = over the trash zone. */
	const TRASH_BAND_PX = 44;
	/** Gesture opt-out: widgets can mark any subtree with `data-no-gesture`. */
	const NO_GESTURE = '[data-no-gesture], input[type="range"], dialog, [popover]';

	// External widgets (docs/plugin-api.md): load the list and follow `plugin://changed` while the deck lives.
	plugins.start();

	const deck = getDeck();
	const { store, ui } = deck;
	const ws = $derived(store.active);

	type MoveDrag = {
		kind: 'move';
		id: string;
		pointerId: number;
		x0: number;
		y0: number;
		dx: number;
		dy: number;
		started: boolean;
		target: { x: number; y: number } | null;
		preview: WidgetInstance[] | null;
		overTrash: boolean;
	};
	type ResizeDrag = { kind: 'resize'; id: string; pointerId: number; x0: number; y0: number; moved: boolean; size: Size };

	let gridEl = $state<HTMLElement>();
	let drag = $state<MoveDrag | ResizeDrag | null>(null);
	let shakeId = $state<string | null>(null);
	let suppressHandleClick = false;
	/** Normal-mode press: long-press timer + swipe tracking. Not reactive. */
	let press: { id?: string; pointerId: number; x0: number; y0: number; x: number; y: number; t0: number; timer: ReturnType<typeof setTimeout> | null } | null = null;

	const items = $derived(drag?.kind === 'move' && drag.started && drag.preview ? drag.preview : ws.widgets);
	const placed = $derived(layout(items, ws.grid).placed);
	const original = (id: string) => ws.widgets.find((w) => w.instance === id)!;
	const snapshot = () => $state.snapshot(ws.widgets) as WidgetInstance[];
	const supported = (inst: WidgetInstance): Size[] => sizesOf(inst.widget) ?? [{ w: inst.w, h: inst.h }];

	const ghost = $derived.by(() => {
		if (!drag) return null;
		const o = original(drag.id);
		if (!o) return null;
		if (drag.kind === 'move') {
			return drag.started && drag.preview && drag.target && !drag.overTrash ? { ...drag.target, w: o.w, h: o.h } : null;
		}
		return drag.moved && (drag.size.w !== o.w || drag.size.h !== o.h) ? { x: o.x, y: o.y, ...drag.size } : null;
	});

	function pillFor(inst: WidgetInstance): string {
		if (!drag || drag.id !== inst.instance) return '';
		if (drag.kind === 'move' && !drag.started) return '';
		const cur = drag.kind === 'resize' ? drag.size : { w: inst.w, h: inst.h };
		const others = supported(inst).filter((s) => s.w !== cur.w || s.h !== cur.h);
		return others.length ? `${fmtSize(cur)} → ${others.map(fmtSize).join(' · ')}` : fmtSize(cur);
	}

	function geometry(): GridGeometry {
		const r = gridEl!.getBoundingClientRect();
		const cs = getComputedStyle(gridEl!);
		const pl = parseFloat(cs.paddingLeft);
		const pr = parseFloat(cs.paddingRight);
		const pt = parseFloat(cs.paddingTop);
		const pb = parseFloat(cs.paddingBottom);
		return {
			cols: ws.grid.cols,
			rows: ws.grid.rows,
			left: r.left + pl,
			top: r.top + pt,
			width: r.width - pl - pr,
			height: r.height - pt - pb,
			gap: parseFloat(cs.columnGap) || 16
		};
	}

	function shake(id: string) {
		shakeId = null;
		requestAnimationFrame(() => {
			shakeId = id;
			setTimeout(() => {
				if (shakeId === id) shakeId = null;
			}, 320);
		});
	}

	function capture(el: Element | undefined, pointerId: number) {
		try {
			el?.setPointerCapture(pointerId);
		} catch {
			// Pointer already released.
		}
	}

	function clearPress() {
		if (press?.timer) clearTimeout(press.timer);
		press = null;
	}

	function startMove(id: string, pointerId: number, x: number, y: number) {
		drag = { kind: 'move', id, pointerId, x0: x, y0: y, dx: 0, dy: 0, started: false, target: null, preview: null, overTrash: false };
		capture(gridEl, pointerId);
	}

	function onpointerdown(e: PointerEvent) {
		if (e.pointerType === 'mouse' && e.button !== 0) return;
		const target = e.target as Element;
		// Sliders, sheets and menus inside widgets never start a long-press or a swipe.
		if (target.closest(NO_GESTURE)) return;
		const frame = target.closest<HTMLElement>('[data-instance]');
		if (!ui.editing) {
			clearPress();
			const p = {
				id: frame?.dataset.instance,
				pointerId: e.pointerId,
				x0: e.clientX,
				y0: e.clientY,
				x: e.clientX,
				y: e.clientY,
				t0: performance.now(),
				timer: null as ReturnType<typeof setTimeout> | null
			};
			if (p.id) {
				p.timer = setTimeout(() => {
					// Long-press: enter edit mode and keep the card under the finger lifted.
					p.timer = null;
					press = null;
					ui.enterEdit();
					navigator.vibrate?.(12);
					startMove(p.id!, p.pointerId, p.x0, p.y0);
				}, LONG_PRESS_MS);
			}
			press = p;
			return;
		}
		if (!frame || !target.closest('.shield')) return;
		startMove(frame.dataset.instance!, e.pointerId, e.clientX, e.clientY);
	}

	function onpointermove(e: PointerEvent) {
		if (press && e.pointerId === press.pointerId) {
			press.x = e.clientX;
			press.y = e.clientY;
			if (press.timer && Math.hypot(press.x - press.x0, press.y - press.y0) > 10) {
				clearTimeout(press.timer);
				press.timer = null;
			}
		}
		if (!drag || e.pointerId !== drag.pointerId) return;
		const dx = e.clientX - drag.x0;
		const dy = e.clientY - drag.y0;

		if (drag.kind === 'resize') {
			if (!drag.moved && Math.hypot(dx, dy) < DRAG_START_PX) return;
			const o = original(drag.id);
			const cell = cellAt(e.clientX, e.clientY, geometry());
			const desired = { w: Math.max(1, cell.x - o.x + 1), h: Math.max(1, cell.y - o.y + 1) };
			const size = closestSize(ws.widgets, drag.id, supported(o), desired, ws.grid) ?? drag.size;
			drag = { ...drag, moved: true, size };
			return;
		}

		if (!drag.started && Math.hypot(dx, dy) < DRAG_START_PX) return;
		const geo = geometry();
		const o = original(drag.id);
		const tgt = snapDrag(o, dx, dy, geo);
		const overTrash = e.clientY >= gridEl!.getBoundingClientRect().bottom - TRASH_BAND_PX;
		const same = drag.target && drag.target.x === tgt.x && drag.target.y === tgt.y && drag.overTrash === overTrash;
		const preview = same ? drag.preview : overTrash ? null : moveItem(snapshot(), drag.id, tgt.x, tgt.y, ws.grid);
		drag = { ...drag, started: true, dx, dy, target: tgt, preview, overTrash };
	}

	function onpointerup(e: PointerEvent) {
		if (press && e.pointerId === press.pointerId) {
			const p = press;
			clearPress();
			const dx = e.clientX - p.x0;
			const dy = e.clientY - p.y0;
			if (!ui.editing && Math.abs(dx) >= SWIPE_PX && Math.abs(dx) > 1.5 * Math.abs(dy) && performance.now() - p.t0 < 1000) {
				store.cycle(dx < 0 ? 1 : -1);
			}
		}
		if (!drag || e.pointerId !== drag.pointerId) return;
		const d = drag;
		drag = null;

		if (d.kind === 'resize') {
			if (!d.moved) return; // tap: the click handler cycles
			suppressHandleClick = true;
			setTimeout(() => (suppressHandleClick = false), 0);
			const o = original(d.id);
			if (d.size.w === o.w && d.size.h === o.h) return;
			const out = resizeItem(snapshot(), d.id, d.size, ws.grid);
			if (out) store.setWidgets(out);
			else shake(d.id);
			return;
		}

		if (!d.started) return;
		if (d.overTrash) {
			removeWithUndo(deck, d.id);
			return;
		}
		if (d.preview) store.setWidgets(d.preview);
		else {
			shake(d.id);
			ui.say(t('shell.edit.noRoomMove', { name: widgetName(original(d.id).widget) }), 'warning');
		}
	}

	function onpointercancel() {
		clearPress();
		drag = null;
	}

	function cycleSize(inst: WidgetInstance) {
		const next = nextSize(ws.widgets, inst.instance, supported(inst), ws.grid);
		const out = next && resizeItem(snapshot(), inst.instance, next, ws.grid);
		if (out) store.setWidgets(out);
		else {
			shake(inst.instance);
			ui.say(t('shell.edit.noRoomSize', { name: widgetName(inst.widget) }), 'warning');
		}
	}

	function handleDown(inst: WidgetInstance, e: PointerEvent) {
		if (e.pointerType === 'mouse' && e.button !== 0) return;
		e.stopPropagation();
		capture(e.currentTarget as Element, e.pointerId);
		drag = { kind: 'resize', id: inst.instance, pointerId: e.pointerId, x0: e.clientX, y0: e.clientY, moved: false, size: { w: inst.w, h: inst.h } };
	}

	function handleClick(inst: WidgetInstance) {
		if (suppressHandleClick) {
			suppressHandleClick = false;
			return;
		}
		cycleSize(inst);
	}

	function onkey(inst: WidgetInstance, e: KeyboardEvent) {
		const step: Record<string, [number, number]> = { ArrowLeft: [-1, 0], ArrowRight: [1, 0], ArrowUp: [0, -1], ArrowDown: [0, 1] };
		if (e.key in step) {
			e.preventDefault();
			const [dx, dy] = step[e.key];
			const out = moveItem(snapshot(), inst.instance, inst.x + dx, inst.y + dy, ws.grid);
			if (out) store.setWidgets(out);
			else shake(inst.instance);
		} else if (e.key === 's' || e.key === 'S') {
			e.preventDefault();
			cycleSize(inst);
		} else if (e.key === 'Delete' || e.key === 'Backspace') {
			e.preventDefault();
			// Undo toast (5 s, also Ctrl+Z) instead of instant loss; focus moves to the grid so keys keep working.
			removeWithUndo(deck, inst.instance);
			gridEl?.focus();
		} else if (e.key === 'Enter') {
			e.preventDefault();
			ui.settingsFor = inst.instance;
		}
	}
</script>

<section
	bind:this={gridEl}
	class="grid"
	class:k-dots={ui.editing}
	class:editing={ui.editing}
	class:dragging={drag !== null}
	tabindex="-1"
	style:grid-template-columns="repeat({ws.grid.cols}, minmax(0, 1fr))"
	style:grid-template-rows="repeat({ws.grid.rows}, minmax(0, 1fr))"
	aria-label={t('shell.grid.label', { name: ws.name })}
	aria-describedby="k-swipe-hint"
	{onpointerdown}
	{onpointermove}
	{onpointerup}
	{onpointercancel}
	oncontextmenu={(e) => e.preventDefault()}
>
	{#each placed as p (p.instance)}
		{@const lifted = drag?.kind === 'move' && drag.started && drag.id === p.instance}
		<WidgetFrame
			inst={original(p.instance) ?? p}
			rect={lifted ? original(p.instance) : p}
			{lifted}
			offset={lifted && drag?.kind === 'move' ? { dx: drag.dx, dy: drag.dy } : undefined}
			pill={ui.editing ? pillFor(original(p.instance) ?? p) : ''}
			shaking={shakeId === p.instance}
			onhandledown={(e) => handleDown(original(p.instance), e)}
			onhandleclick={() => handleClick(original(p.instance))}
			onkey={(e) => onkey(original(p.instance), e)}
			ongear={() => (ui.settingsFor = p.instance)}
		/>
	{/each}

	{#if ghost}
		<div
			class="k-ghost"
			aria-hidden="true"
			style:grid-column="{ghost.x + 1} / span {ghost.w}"
			style:grid-row="{ghost.y + 1} / span {ghost.h}"
		></div>
	{/if}

	{#if placed.length === 0 && !ui.editing}
		<div class="empty" style:grid-column="1 / -1" style:grid-row="1 / -1">
			<p>{t('shell.grid.empty')}</p>
			<button
				type="button"
				class="k-btn k-btn-primary"
				onclick={() => {
					ui.enterEdit();
					ui.drawerOpen = true;
				}}
			>
				<Icon name="plus" size={16} stroke={2.4} />{t('shell.edit.addWidget')}
			</button>
		</div>
	{/if}

	{#if ui.editing}
		<div class="trash" class:hot={drag?.kind === 'move' && drag.overTrash} aria-hidden="true">
			<Icon name="trash" size={16} />{t('shell.edit.trash')}
		</div>
	{/if}
</section>
<p id="k-swipe-hint" class="k-sr-only">{t('shell.grid.swipeHint')}</p>
<p id="k-edit-keys" class="k-sr-only">{t('shell.edit.hintKeys')}</p>

<style>
	.grid {
		position: relative;
		flex: 1;
		min-height: 0;
		box-sizing: border-box;
		display: grid;
		gap: 16px;
		padding: 8px 24px 24px;
		touch-action: none;
		user-select: none;
		-webkit-user-select: none;
	}
	/* Edit mode: every surface at 100 % so cell edges are visible (SPEC §5.3). */
	.grid.editing {
		--k-surface-a: var(--k-surface);
	}
	.dragging {
		cursor: grabbing;
	}
	.empty {
		display: flex;
		flex-direction: column;
		align-items: center;
		justify-content: center;
		gap: 14px;
		color: var(--k-text-muted);
		font-size: 13px;
	}
	.empty p {
		margin: 0;
	}
	.trash {
		position: absolute;
		left: 50%;
		bottom: 0;
		z-index: 4;
		box-sizing: border-box;
		height: 44px;
		padding: 0 22px;
		display: flex;
		align-items: center;
		justify-content: center;
		gap: 8px;
		transform: translateX(-50%);
		width: max-content;
		border-radius: 14px 14px 0 0;
		border: 1px solid var(--k-danger);
		border-bottom: 0;
		background: color-mix(in srgb, var(--k-danger) 16%, var(--k-bg));
		color: var(--k-danger);
		font-size: 12px;
		font-weight: 600;
		pointer-events: none;
		transition:
			width 150ms ease,
			background-color 150ms ease;
	}
	.trash.hot {
		width: 100%;
		border-radius: 0;
		background: color-mix(in srgb, var(--k-danger) 28%, var(--k-bg));
	}
</style>
