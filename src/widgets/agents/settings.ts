import type { Appearance } from '#lib/ipc/index.ts';

export interface AgentsSettings {
	/** List idle agents too (2×2 / 4×2); waiting and working agents are always shown. */
	showIdle: boolean;
	appearance?: Appearance;
}

export const defaults: AgentsSettings = {
	showIdle: true
};
