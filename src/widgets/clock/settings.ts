import type { Appearance } from '#lib/ipc/index.ts';

export interface ExtraZone {
	/** IANA zone id, e.g. "Europe/Berlin". */
	timeZone: string;
	/** Optional display label; empty = derived from the zone ("Berlin"). */
	label: string;
}

export interface ClockSettings {
	hour12: boolean;
	seconds: boolean;
	/** 2×2 only. */
	face: 'analog' | 'digital';
	/** Label shown for the main zone; empty = derived from the zone. */
	city: string;
	/** Main IANA zone; empty = system zone. */
	timeZone: string;
	zones: ExtraZone[];
	appearance?: Appearance;
}

export const MAX_ZONES = 3;

export const defaults: ClockSettings = {
	hour12: false,
	seconds: false,
	face: 'analog',
	city: '',
	timeZone: '',
	zones: []
};
