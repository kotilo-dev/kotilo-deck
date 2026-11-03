import type { Appearance } from '#lib/ipc/index.ts';

/** Per-instance settings. Feeds (URL, name, colour) are global: Settings ▸ Weather & calendar. */
export interface CalendarSettings {
	/** List all-day events (rows show "All day"). */
	allDay: boolean;
	appearance?: Appearance;
}

export const defaults: CalendarSettings = {
	allDay: true
};
