/**
 * Widget registry: every `src/widgets/<id>/` folder with a manifest.json + Widget.svelte is picked up
 * automatically (no edits here when adding a widget). Optional: settings.ts (`defaults`), Settings.svelte.
 */
import type { Component } from 'svelte';
import type { WidgetManifest, WidgetProps, WidgetSettingsProps } from './types';

export * from './types';

export interface WidgetDefinition {
	manifest: WidgetManifest;
	component: Component<WidgetProps<any>>;
	defaults: Record<string, unknown>;
	settingsComponent?: Component<WidgetSettingsProps<any>>;
}

const manifests = import.meta.glob<WidgetManifest>('./*/manifest.json', { eager: true, import: 'default' });
const components = import.meta.glob<Component<WidgetProps<any>>>('./*/Widget.svelte', { eager: true, import: 'default' });
const settingsModules = import.meta.glob<{ defaults?: Record<string, unknown> }>('./*/settings.ts', { eager: true });
const settingsComponents = import.meta.glob<Component<WidgetSettingsProps<any>>>('./*/Settings.svelte', {
	eager: true,
	import: 'default'
});

const dirOf = (path: string) => path.split('/')[1];

export const widgets: ReadonlyMap<string, WidgetDefinition> = new Map(
	Object.entries(manifests).flatMap(([path, manifest]) => {
		const dir = dirOf(path);
		const component = components[`./${dir}/Widget.svelte`];
		if (!component) return [];
		return [
			[
				manifest.id,
				{
					manifest,
					component,
					defaults: settingsModules[`./${dir}/settings.ts`]?.defaults ?? {},
					settingsComponent: settingsComponents[`./${dir}/Settings.svelte`]
				}
			] as const
		];
	})
);

export const parseSize = (size: string): { w: number; h: number } => {
	const [w, h] = size.split('x').map(Number);
	return { w, h };
};
