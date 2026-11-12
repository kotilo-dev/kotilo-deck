import type { Workspace, WidgetInstance } from '#lib/ipc/index.ts';
import { t } from '#lib/i18n/index.ts';

/**
 * Bundled presets (SPEC §4.1), laid out on the 8×4 default. SPEC: Focus = clock + pomodoro + todo,
 * Ops = system + media + calendar, Agents = agent status + pomodoro + todo, Full = everything; each
 * fills the rest of the grid with what fits. Full places every widget once (all ten exactly fill 8×4).
 * Only new seeds and "Reset to preset" use these; existing workspace files are never rewritten.
 */
export const PRESET_IDS = ['focus', 'ops', 'agents', 'full'] as const;
export type PresetId = (typeof PRESET_IDS)[number];

type Slot = [widget: string, x: number, y: number, w: number, h: number];

const LAYOUTS: Record<PresetId, Slot[]> = {
	// Calm: big clock and the timer side by side, weather beside them; today's todos and the next meeting
	// below (the 4×2 clock already shows the date and week, so no Date card).
	focus: [
		['clock', 0, 0, 4, 2],
		['pomodoro', 4, 0, 2, 2],
		['weather', 6, 0, 2, 2],
		['todo', 0, 2, 4, 2],
		['calendar', 4, 2, 4, 2]
	],
	// Machine and schedule first: system and calendar on top, media and small tiles below.
	ops: [
		['system', 0, 0, 4, 2],
		['calendar', 4, 0, 4, 2],
		['media', 0, 2, 4, 2],
		['clock', 4, 2, 2, 1],
		['date', 6, 2, 2, 1],
		['weather', 4, 3, 2, 1],
		['volume', 6, 3, 2, 1]
	],
	// Agents while they run, the timer beside them, todos and machine load below.
	agents: [
		['agents', 0, 0, 4, 2],
		['pomodoro', 4, 0, 2, 2],
		['clock', 6, 0, 2, 1],
		['date', 6, 1, 2, 1],
		['todo', 0, 2, 4, 2],
		['system', 4, 2, 2, 2],
		['weather', 6, 2, 2, 2]
	],
	// Every widget once, filling the 8×4 grid exactly.
	full: [
		['clock', 0, 0, 2, 1],
		['date', 2, 0, 2, 1],
		['weather', 4, 0, 2, 1],
		['volume', 6, 0, 2, 1],
		['calendar', 0, 1, 4, 2],
		['system', 4, 1, 2, 2],
		['todo', 6, 1, 2, 2],
		['media', 0, 3, 4, 1],
		['pomodoro', 4, 3, 2, 1],
		['agents', 6, 3, 2, 1]
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
