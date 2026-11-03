import type { Appearance } from '#lib/ipc/index.ts';

/**
 * Per-instance settings. Durations, cycle length, auto-start and the daily goal are global
 * (`DeckSettings.pomodoro`, Settings window): every instance shows the one shared timer.
 */
export interface PomodoroWidgetSettings {
	/** Show the 60 s "start a focus block?" banner when the Kotilo desktop suggests one. */
	suggestions: boolean;
	appearance?: Appearance;
}

export const defaults: PomodoroWidgetSettings = {
	suggestions: true
};
