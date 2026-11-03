import type { Workspace, WidgetInstance } from '#lib/ipc/index.ts';
import { t } from '#lib/i18n/index.ts';

/**
 * Bundled presets (SPEC §4.1), laid out on the 8×4 default. Phase 1 only ships clock, date, weather and
 * system, so each preset uses those; pomodoro / todo / media / calendar / agent status join in later
 * phases. Full places clock and weather twice at different sizes (Phase 1 acceptance).
 */
export const PRESET_IDS = ['focus', 'ops', 'agents', 'full'] as const;
export type PresetId = (typeof PRESET_IDS)[number];

type Slot = [widget: string, x: number, y: number, w: number, h: number];

const LAYOUTS: Record<PresetId, Slot[]> = {
	// Calm: one big clock, weather and date beside it, bottom half free for focus widgets.
	focus: [
		['clock', 0, 0, 4, 2],
		['weather', 4, 0, 2, 2],
		['date', 6, 0, 2, 1]
	],
	// Machine first: wide system tile, compact clock/date/weather column.
	ops: [
		['system', 0, 0, 4, 2],
		['weather', 4, 0, 2, 2],
		['clock', 6, 0, 2, 1],
		['date', 6, 1, 2, 1]
	],
	// Glanceable status strip while agents run.
	agents: [
		['clock', 0, 0, 2, 1],
		['date', 0, 1, 2, 1],
		['system', 2, 0, 2, 2],
		['weather', 4, 0, 2, 1]
	],
	full: [
		['clock', 0, 0, 4, 2],
		['weather', 4, 0, 4, 2],
		['system', 0, 2, 4, 2],
		['clock', 4, 2, 2, 2],
		['date', 6, 2, 2, 1],
		['weather', 6, 3, 2, 1]
	]
};

export const isPresetId = (id: string): id is PresetId => (PRESET_IDS as readonly string[]).includes(id);

export function presetWidgets(id: PresetId): WidgetInstance[] {
	const counts = new Map<string, number>();
	return LAYOUTS[id].map(([widget, x, y, w, h]) => {
		const n = (counts.get(widget) ?? 0) + 1;
		counts.set(widget, n);
		return { instance: `${widget}-${n}`, widget, x, y, w, h, settings: {} };
	});
}

/** A fresh copy of a preset on the 8×4 grid (caller reflows for portrait). */
export function makePreset(id: PresetId): Workspace {
	return {
		schemaVersion: 1,
		id,
		name: t(`shell.presets.${id}`),
		theme: null,
		grid: { cols: 8, rows: 4 },
		widgets: presetWidgets(id)
	};
}
