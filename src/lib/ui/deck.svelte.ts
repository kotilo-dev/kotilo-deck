/** Transient deck UI state (not persisted) + registry helpers shared by the shell components. */
import { getContext, setContext } from 'svelte';
import type { Capability, SizeKey, WidgetInstance } from '#lib/ipc/index.ts';
import { t } from '#lib/i18n/index.ts';
import { parseSize, widgets, type WidgetDefinition } from '../../widgets/index';
import type { Size } from '#lib/grid/index.ts';
import type { WorkspaceStore } from '#lib/workspace/index.ts';

export interface NoticeAction {
	label: string;
	run: () => void;
}

export interface Notice {
	id: number;
	text: string;
	tone: 'info' | 'warning';
	action?: NoticeAction;
}

export class DeckUi {
	editing = $state(false);
	drawerOpen = $state(false);
	/** Instance whose settings sheet is open. */
	settingsFor = $state<string | null>(null);
	notice = $state<Notice | null>(null);
	/** null until the backend answered, so widgets don't flash "Not available" at boot. */
	capabilities = $state<ReadonlySet<Capability> | null>(null);
	/** Resolved appearance for cards without their own override (written by the deck page). */
	look = $state({ surfaceOpacity: 0.85, blur: 0 });
	/** Instance to flash after placing it from the drawer. */
	highlight = $state<string | null>(null);
	reducedMotion = $state(false);

	#noticeTimer: ReturnType<typeof setTimeout> | null = null;
	#highlightTimer: ReturnType<typeof setTimeout> | null = null;

	say(text: string, tone: 'info' | 'warning' = 'info', ms = 4000, action?: NoticeAction) {
		if (this.#noticeTimer) clearTimeout(this.#noticeTimer);
		const notice: Notice = { id: Date.now(), text, tone, action };
		this.notice = notice;
		this.#noticeTimer = setTimeout(() => {
			if (this.notice === notice) this.notice = null;
		}, ms);
	}

	dismiss() {
		if (this.#noticeTimer) clearTimeout(this.#noticeTimer);
		this.notice = null;
	}

	/** Runs the visible toast's action (Undo); also bound to Ctrl+Z while the toast shows. */
	runAction(): boolean {
		const action = this.notice?.action;
		if (!action) return false;
		this.dismiss();
		action.run();
		return true;
	}

	flash(instance: string) {
		if (this.#highlightTimer) clearTimeout(this.#highlightTimer);
		this.highlight = instance;
		this.#highlightTimer = setTimeout(() => (this.highlight = null), 1600);
	}

	enterEdit() {
		this.editing = true;
	}

	exitEdit() {
		this.editing = false;
		this.drawerOpen = false;
		this.settingsFor = null;
	}

	/** "unknown" while capabilities are still loading and the widget needs some. */
	availability(def: WidgetDefinition | undefined): 'yes' | 'no' | 'unknown' {
		if (!def) return 'no';
		const needs = def.manifest.capabilities;
		if (needs.length === 0) return 'yes';
		const caps = this.capabilities;
		if (caps === null) return 'unknown';
		return needs.every((c) => caps.has(c)) ? 'yes' : 'no';
	}

	available(def: WidgetDefinition | undefined): boolean {
		return this.availability(def) === 'yes';
	}
}

export interface DeckContext {
	store: WorkspaceStore;
	ui: DeckUi;
}

const KEY = Symbol('kotilo-deck');
export const setDeck = (ctx: DeckContext) => setContext(KEY, ctx);
export const getDeck = () => getContext<DeckContext>(KEY);

/** Toast duration for removals with Undo. */
export const UNDO_MS = 5000;

/** Removes a widget instance and offers Undo for 5 s (edit-mode Delete, trash zone, sheet button). */
export function removeWithUndo({ store, ui }: DeckContext, instance: string): void {
	const removed = store.removeWidget(instance);
	if (!removed) return;
	const name = widgetName(removed.widget.widget);
	if (ui.settingsFor === instance) ui.settingsFor = null;
	ui.say(t('shell.edit.removed', { name }), 'info', UNDO_MS, {
		label: t('shell.edit.undo'),
		run: () => {
			if (store.restoreWidget(removed.workspace, removed.widget)) {
				const back = store.active?.widgets.at(-1)?.instance;
				if (back) ui.flash(back);
			} else {
				ui.say(t('shell.edit.undoNoRoom', { name }), 'warning', 6000);
			}
		}
	});
}

// ── registry helpers ────────────────────────────────────────────────────────

export const definition = (widget: string): WidgetDefinition | undefined => widgets.get(widget);

export const sizesOf = (widget: string): Size[] | undefined => widgets.get(widget)?.manifest.sizes.map(parseSize);

/** "2×2" (DESIGN size chip). */
export const fmtSize = (s: Size | SizeKey): string => {
	const { w, h } = typeof s === 'string' ? parseSize(s) : s;
	return `${w}×${h}`;
};

const tOr = (key: string, fallback: string) => {
	const v = t(key);
	return v === key ? fallback : v;
};

/** Localised widget name: `widgets.<id>.name`, else the manifest name, else the raw id. */
export const widgetName = (widget: string): string => tOr(`widgets.${widget}.name`, widgets.get(widget)?.manifest.name ?? widget);

/** Generic settings label: `widgets.<id>.settings.<key>`, else schema title, else the key. */
export const settingLabel = (widget: string, key: string, title?: string) => tOr(`widgets.${widget}.settings.${key}`, title ?? key);

/** Enum option label: `widgets.<id>.options.<key>.<value>`, else the value. */
export const optionLabel = (widget: string, key: string, value: string) => tOr(`widgets.${widget}.options.${key}.${value}`, value);

export const mergedSettings = (inst: WidgetInstance): Record<string, unknown> => ({
	...(widgets.get(inst.widget)?.defaults ?? {}),
	...(inst.settings ?? {})
});

export const allDefinitions = (): WidgetDefinition[] =>
	[...widgets.values()].sort((a, b) => widgetName(a.manifest.id).localeCompare(widgetName(b.manifest.id)));

/** Surface opacity a widget gets without its own override (workspace > global setting > theme). */
export const surfaceDefault = (ui: DeckUi): number => ui.look.surfaceOpacity;

/** "2 widgets don't fit this grid: Clock, Weather" (singular/plural). */
export const unplacedText = (names: string[]): string =>
	t(names.length === 1 ? 'shell.edit.unplacedOne' : 'shell.edit.unplaced', { count: names.length, names: names.join(', ') });
