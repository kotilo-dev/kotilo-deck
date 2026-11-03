/** Transient deck UI state (not persisted) + registry helpers shared by the shell components. */
import { getContext, setContext } from 'svelte';
import type { Capability, SizeKey, WidgetInstance } from '#lib/ipc/index.ts';
import { t } from '#lib/i18n/index.ts';
import { parseSize, widgets, type WidgetDefinition } from '../../widgets/index';
import type { Size } from '#lib/grid/index.ts';
import type { WorkspaceStore } from '#lib/workspace/index.ts';
import { effectiveThemeId, getTheme } from '#lib/theme/index.ts';

export class DeckUi {
	editing = $state(false);
	drawerOpen = $state(false);
	/** Instance whose settings sheet is open. */
	settingsFor = $state<string | null>(null);
	notice = $state<{ id: number; text: string; tone: 'info' | 'warning' } | null>(null);
	capabilities = $state<ReadonlySet<Capability>>(new Set());
	/** Instance to flash after placing it from the drawer. */
	highlight = $state<string | null>(null);
	reducedMotion = $state(false);

	#noticeTimer: ReturnType<typeof setTimeout> | null = null;
	#highlightTimer: ReturnType<typeof setTimeout> | null = null;

	say(text: string, tone: 'info' | 'warning' = 'info', ms = 4000) {
		if (this.#noticeTimer) clearTimeout(this.#noticeTimer);
		this.notice = { id: Date.now(), text, tone };
		this.#noticeTimer = setTimeout(() => (this.notice = null), ms);
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

	available(def: WidgetDefinition | undefined): boolean {
		return !!def && def.manifest.capabilities.every((c) => this.capabilities.has(c));
	}
}

export interface DeckContext {
	store: WorkspaceStore;
	ui: DeckUi;
}

const KEY = Symbol('kotilo-deck');
export const setDeck = (ctx: DeckContext) => setContext(KEY, ctx);
export const getDeck = () => getContext<DeckContext>(KEY);

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

/** Surface opacity a widget gets without its own override: workspace appearance, else the theme default. */
export function surfaceDefault(store: WorkspaceStore): number {
	const ws = store.active;
	const o = ws?.appearance?.opacity;
	return typeof o === 'number' ? o : getTheme(effectiveThemeId(ws?.theme, store.settings.theme)).opacity.surface;
}

/** "2 widgets don't fit this grid: Clock, Weather" (singular/plural). */
export const unplacedText = (names: string[]): string =>
	t(names.length === 1 ? 'shell.edit.unplacedOne' : 'shell.edit.unplaced', { count: names.length, names: names.join(', ') });
