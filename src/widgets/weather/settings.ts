import type { Appearance } from '#lib/ipc/index.ts';

export interface WeatherLocation {
	/** Display name ("İstanbul"). */
	name: string;
	country: string;
	admin1: string | null;
	latitude: number;
	longitude: number;
}

export interface WeatherSettings {
	unit: 'c' | 'f';
	/** null = not set yet → empty state prompting for a city. */
	location: WeatherLocation | null;
	appearance?: Appearance;
}

export const defaults: WeatherSettings = {
	unit: 'c',
	location: null
};
