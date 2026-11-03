import type { Appearance } from '#lib/ipc/index.ts';

export interface VolumeSettings {
	/** 2×1: "Output device ▾" menu; 2×2: the radio list. */
	devicePicker: boolean;
	/** "Mixer" button opening the per-app sheet. */
	mixer: boolean;
	appearance?: Appearance;
}

export const defaults: VolumeSettings = {
	devicePicker: true,
	mixer: true
};
