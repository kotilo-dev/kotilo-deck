import type { Appearance } from '#lib/ipc/index.ts';

/** Per-instance settings. Feeds (URL, name, colour) are global: Settings ▸ Weather & calendar. */
export interface CalendarSettings {
	/** List all-day events (rows show "All day"). */
	allDay: boolean;
	/** Day timeline range "HH:MM" (DESIGN §5.4); bad or backwards values fall back to 09:00–18:00. */
	timelineFrom: string;
	timelineTo: string;
	appearance?: Appearance;
}

export const defaults: CalendarSettings = {
	allDay: true,
	timelineFrom: '09:00',
	timelineTo: '18:00'
};
