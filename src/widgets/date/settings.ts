import type { Appearance } from '#lib/ipc/index.ts';

export interface DateSettings {
	dayOfYear: boolean;
	appearance?: Appearance;
}

export const defaults: DateSettings = {
	dayOfYear: false
};
