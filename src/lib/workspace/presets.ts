import type { Workspace, WidgetInstance } from '#lib/ipc/index.ts';
import { t } from '#lib/i18n/index.ts';

/**
 * Bundled presets (SPEC §4.1), laid out on the 8×4 default. SPEC: Focus = clock + pomodoro + todo,
 * Ops = system + media + calendar, Agents = agent status + pomodoro + todo, Full = everything.
 * Pomodoro, calendar and agent status arrive in Phase 3; until then each preset uses what exists.
 * Full places clock twice at different sizes (Phase 1 acceptance). Only new seeds and
 * "Reset to preset" use these; existing workspace files are never rewritten.
 */
export const PRESET_IDS = ['focus', 'ops', 'agents', 'full'] as const;
export type PresetId = (typeof PRESET_IDS)[number];

type Slot = [widget: string, x: number, y: number, w: number, h: number];

const LAYOUTS: Record<PresetId, Slot[]> = {
	// Calm: one big clock, weather and date beside it, today's todos below.
	focus: [
		['clock', 0, 0, 4, 2],
		['weather', 4, 0, 2, 2],
		['date', 6, 0, 2, 1],
		['todo', 0, 2, 4, 2]
	],
	// Machine first: wide system tile, compact clock/date/weather column, media below.
	ops: [
		['system', 0, 0, 4, 2],
		['weather', 4, 0, 2, 2],
		['clock', 6, 0, 2, 1],
		['date', 6, 1, 2, 1],
		['media', 0, 2, 4, 2]
	],
	// Glanceable status strip while agents run, todos below.
	agents: [
		['clock', 0, 0, 2, 1],
		['date', 0, 1, 2, 1],
		['system', 2, 0, 2, 2],
		['weather', 4, 0, 2, 1],
		['todo', 0, 2, 4, 2]
	],
	// Every Phase 2 widget, filling the 8×4 grid.
	full: [
		['clock', 0, 0, 4, 2],
		['weather', 4, 0, 2, 2],
		['system', 6, 0, 2, 2],
		['media', 0, 2, 4, 1],
		['volume', 0, 3, 2, 1],
		['date', 2, 3, 2, 1],
		['todo', 4, 2, 2, 2],
		['clock', 6, 2, 2, 2]
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
