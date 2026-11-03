import type { Appearance } from '#lib/ipc/index.ts';

export interface SystemSettings {
	/** Percent; a metric above it for 3 consecutive samples turns the warning on. */
	threshold: number;
	appearance?: Appearance;
}

export const defaults: SystemSettings = {
	threshold: 85
};
