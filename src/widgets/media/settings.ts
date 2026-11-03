import type { Appearance } from '#lib/ipc/index.ts';

export interface MediaSettings {
	/** 4×2: "Artist · Album" under the title instead of just the artist. */
	showAlbum: boolean;
	appearance?: Appearance;
}

export const defaults: MediaSettings = {
	showAlbum: true
};
