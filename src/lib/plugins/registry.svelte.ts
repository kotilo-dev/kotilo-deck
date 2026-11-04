/**
 * Reactive widget registry (ARCHITECTURE "Phase 4 ▸ Widget plugins"): the static built-ins from
 * `src/widgets/` merged with external plugin definitions (`ext.<folder>`, docs/plugin-api.md) that
 * load at runtime and update on `plugin://changed` without a reload.
 *
 * Reads go through `$state`, so `getWidget` / `allWidgets` inside `$derived` / templates re-run when
 * the plugin list changes. Instances of a missing or invalid plugin resolve to `undefined` (the
 * "Unknown widget" card) and keep their data.
 */
import type { Component } from 'svelte';
import { hasBackend, isRemote, on, pluginList, type PluginTopic, type PluginWidget, type SizeKey } from '#lib/ipc/index.ts';
import { widgets, type SettingSchema, type WidgetDefinition, type WidgetManifest, type WidgetProps } from '../../widgets/index';
import PluginFrame from './PluginFrame.svelte';

/** Prefix of every plugin widget id; built-in ids never contain a dot. */
export const PLUGIN_PREFIX = 'ext.';
export const isPluginId = (id: string): boolean => id.startsWith(PLUGIN_PREFIX);

/** What PluginFrame needs beyond the generic widget props. */
export interface PluginInfo {
	/** `ext.<folder>`. */
	id: string;
	folder: string;
	entry: string;
	/** Backend-provided iframe src (Tauri webview); null if the backend gave none. */
	entryUrl: string | null;
	topics: readonly PluginTopic[];
	version?: string;
	author?: string;
	description?: string;
}

export interface PluginDefinition extends WidgetDefinition {
	plugin: PluginInfo;
}

const TOPICS: readonly PluginTopic[] = ['system', 'media', 'pomodoro'];
const SIZE = /^([1-9]|1[0-2])x([1-9]|1[0-2])$/;

/** Keeps only fields the generic settings form understands; defaults come from each field's `default`. */
export function settingsFromSchema(raw: unknown): { schema: WidgetManifest['settings']; defaults: Record<string, unknown> } {
	const properties: Record<string, SettingSchema> = {};
	const defaults: Record<string, unknown> = {};
	const props = raw && typeof raw === 'object' ? (raw as { properties?: unknown }).properties : undefined;
	if (props && typeof props === 'object' && !Array.isArray(props)) {
		for (const [key, value] of Object.entries(props as Record<string, unknown>)) {
			if (key === 'appearance' || !value || typeof value !== 'object') continue;
			const s = value as Record<string, unknown>;
			const title = typeof s.title === 'string' ? s.title : undefined;
			const num = (k: string) => (typeof s[k] === 'number' && Number.isFinite(s[k]) ? (s[k] as number) : undefined);
			if (s.type === 'boolean') {
				properties[key] = { type: 'boolean', title, default: typeof s.default === 'boolean' ? s.default : undefined };
			} else if (s.type === 'string') {
				const en = Array.isArray(s.enum) ? s.enum.filter((v): v is string => typeof v === 'string') : undefined;
				const def = typeof s.default === 'string' && (!en || en.includes(s.default)) ? s.default : undefined;
				properties[key] = { type: 'string', title, enum: en && en.length ? en : undefined, default: def };
			} else if (s.type === 'number') {
				properties[key] = { type: 'number', title, minimum: num('minimum'), maximum: num('maximum'), step: num('step'), default: num('default') };
			} else continue;
			const d = (properties[key] as { default?: unknown }).default;
			if (d !== undefined) defaults[key] = d;
		}
	}
	return { schema: { type: 'object', properties }, defaults };
}

/** A valid `PluginWidget` → registry definition; null for rejected folders. */
export function toDefinition(p: PluginWidget, component: Component<WidgetProps<any>> = PluginFrame as Component<WidgetProps<any>>): PluginDefinition | null {
	const m = p.manifest;
	if (p.error || !m || !isPluginId(p.id)) return null;
	const sizes = (m.sizes ?? []).filter((s): s is SizeKey => SIZE.test(s));
	if (sizes.length === 0) return null;
	const { schema, defaults } = settingsFromSchema(m.settings);
	const manifest: WidgetManifest = {
		id: p.id,
		name: m.name || p.folder,
		icon: m.icon || 'puzzle',
		sizes,
		defaultSize: sizes.includes(m.defaultSize) ? m.defaultSize : sizes[0],
		capabilities: m.capabilities ?? [],
		multiInstance: m.multiInstance ?? true,
		settings: schema
	};
	return {
		manifest,
		component,
		defaults,
		plugin: {
			id: p.id,
			folder: p.folder,
			entry: m.entry || 'index.html',
			entryUrl: p.entryUrl,
			topics: (m.topics ?? []).filter((t) => TOPICS.includes(t)),
			version: m.version,
			author: m.author,
			description: m.description
		}
	};
}

/** iframe src: the backend's URL in the Tauri webview, `/plugins/<folder>/<entry>` over the LAN server. */
export function pluginSrc(info: Pick<PluginInfo, 'folder' | 'entry' | 'entryUrl'>, remote = isRemote()): string | null {
	if (!remote) return info.entryUrl;
	const path = info.entry.split('/').map(encodeURIComponent).join('/');
	return `/plugins/${encodeURIComponent(info.folder)}/${path}`;
}

export interface RegistryDeps {
	list: () => Promise<PluginWidget[]>;
	onChanged: (cb: () => void) => Promise<() => void>;
	enabled: () => boolean;
}

const defaultDeps: RegistryDeps = {
	list: () => pluginList(),
	onChanged: (cb) => on('plugin://changed', cb),
	enabled: () => hasBackend()
};

export class PluginRegistry {
	/** Every folder the backend reported, valid or not (Settings ▸ Widgets). */
	entries = $state<PluginWidget[]>([]);
	/** False until the first pluginList answer (or immediately when there is no backend). */
	loaded = $state(false);
	/** Last pluginList failure, if any. */
	error = $state<string | null>(null);
	readonly definitions: ReadonlyMap<string, PluginDefinition> = $derived(
		new Map(
			this.entries.flatMap((p) => {
				const d = toDefinition(p, this.#component);
				return d ? [[d.manifest.id, d] as const] : [];
			})
		)
	);

	#deps: RegistryDeps;
	#component: Component<WidgetProps<any>> | undefined;
	#started = false;
	#seq = 0;
	#unlisten: (() => void) | null = null;

	constructor(deps: RegistryDeps = defaultDeps, component?: Component<WidgetProps<any>>) {
		this.#deps = deps;
		this.#component = component;
	}

	/**
	 * Idempotent: first call loads the list and follows `plugin://changed` for the window's lifetime.
	 * May run inside a `$derived` (lazy start from `getWidget`), so it never writes state synchronously.
	 */
	start(): void {
		if (this.#started) return;
		this.#started = true;
		if (!this.#deps.enabled()) {
			queueMicrotask(() => (this.loaded = true));
			return;
		}
		this.#deps
			.onChanged(() => void this.refresh())
			.then((u) => (this.#unlisten = u))
			.catch(() => {});
		void this.refresh();
	}

	/** Re-reads the list; out-of-order answers are dropped. A failed refresh keeps the last list. */
	async refresh(): Promise<void> {
		const seq = ++this.#seq;
		try {
			const list = await this.#deps.list();
			if (seq !== this.#seq) return;
			this.entries = Array.isArray(list) ? list : [];
			this.error = null;
		} catch (e) {
			if (seq !== this.#seq) return;
			this.error = e instanceof Error ? e.message : String(e);
		} finally {
			if (seq === this.#seq) this.loaded = true;
		}
	}

	/** Tests / teardown. */
	stop(): void {
		this.#unlisten?.();
		this.#unlisten = null;
		this.#started = false;
	}

	get(id: string): PluginDefinition | undefined {
		return this.definitions.get(id);
	}
}

/** The app-wide registry (one per window). */
export const plugins = new PluginRegistry();

/** Built-in or plugin definition; touching an `ext.*` id starts the plugin registry lazily. */
export function getWidget(id: string, registry: PluginRegistry = plugins): WidgetDefinition | undefined {
	const builtin = widgets.get(id);
	if (builtin) return builtin;
	if (!isPluginId(id)) return undefined;
	registry.start();
	return registry.get(id);
}

export const builtinWidgets = (): WidgetDefinition[] => [...widgets.values()];
export const pluginWidgets = (registry: PluginRegistry = plugins): PluginDefinition[] => [...registry.definitions.values()];
export const allWidgets = (registry: PluginRegistry = plugins): WidgetDefinition[] => [...builtinWidgets(), ...pluginWidgets(registry)];

/** True while an `ext.*` instance may still resolve (list not loaded yet): show a skeleton, not "Unknown". */
export const pluginPending = (id: string, registry: PluginRegistry = plugins): boolean => isPluginId(id) && !registry.loaded;

export const isPluginDefinition = (d: WidgetDefinition | undefined): d is PluginDefinition => !!d && 'plugin' in d && !!(d as PluginDefinition).plugin;
