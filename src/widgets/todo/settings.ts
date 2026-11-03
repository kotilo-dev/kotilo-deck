import type { Appearance } from '#lib/ipc/index.ts';

export interface TodoSettings {
	/** New tasks with a parsed due time get a reminder at that time (backend notification). */
	remind: boolean;
	/** Completed tasks stay visible (muted, struck through, at the bottom). */
	showCompleted: boolean;
	appearance?: Appearance;
}

export const defaults: TodoSettings = {
	remind: true,
	showCompleted: true
};
