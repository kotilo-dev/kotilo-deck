import type { Capability, SizeKey } from '#lib/ipc/index.ts';

/** `src/widgets/<id>/manifest.json` (SPEC §4.3). */
export interface WidgetManifest {
	id: string;
	name: string;
	/** Icon key understood by the shell's icon set (e.g. "clock", "calendar", "cloud", "cpu"). */
	icon: string;
	sizes: SizeKey[];
	defaultSize: SizeKey;
	capabilities: Capability[];
	multiInstance: boolean;
	/** JSON schema (object with `properties`); the shell renders boolean / enum / number / string fields generically. */
	settings: { type: 'object'; properties: Record<string, SettingSchema> };
	/** Marks companion widgets in the drawer (DESIGN §6). */
	companion?: boolean;
}

export type SettingSchema =
	| { type: 'boolean'; title?: string; default?: boolean }
	| { type: 'string'; title?: string; enum?: string[]; default?: string; /** "time": an "HH:MM" picker. */ format?: 'time' }
	| { type: 'number'; title?: string; minimum?: number; maximum?: number; step?: number; default?: number }
	| { type: 'array' | 'object'; title?: string };

/** Props every `Widget.svelte` receives from the grid. */
export interface WidgetProps<S = Record<string, unknown>> {
	size: SizeKey;
	/** Instance settings merged over `settings.ts` defaults. */
	settings: S;
	instance: string;
	/** True in edit mode: widgets should pause interactions (no taps, no drag conflicts). */
	editing: boolean;
	/** True when every capability in the manifest is available. */
	available: boolean;
}

/**
 * Optional `src/widgets/<id>/Settings.svelte` for settings the generic form cannot express
 * (city search, time-zone list). Rendered inside the gear sheet above the generic fields.
 */
export interface WidgetSettingsProps<S = Record<string, unknown>> {
	settings: S;
	onchange: (next: S) => void;
}
