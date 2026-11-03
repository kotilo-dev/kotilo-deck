/**
 * Workspace + deck settings store (SPEC §4.1). Svelte 5 runes; persistence goes through a DeckBackend
 * (Tauri IPC or localStorage). Every mutation marks the workspace dirty; saves are debounced.
 */
import type { DeckSettings, WidgetInstance, Workspace } from '#lib/ipc/index.ts';
import { t } from '#lib/i18n/index.ts';
import {
	defaultDensity,
	firstFreeSlot,
	isAutoDensity,
	isPortrait,
	reflow,
	type GridSize,
	type Size
} from '#lib/grid/index.ts';
import { DEFAULT_SETTINGS, type DeckBackend } from './backend';
import { isPresetId, makePreset, PRESET_IDS, presetWidgets, type PresetId } from './presets';
import { instanceId, parseWorkspace, slugify, uniqueId, WorkspaceError } from './validate';

export const SAVE_DEBOUNCE_MS = 400;

export interface StoreOptions {
	/** Supported sizes of a widget id; undefined for widgets that aren't registered. */
	sizesOf: (widget: string) => readonly Size[] | undefined;
	viewport: () => { width: number; height: number };
	onError?: (message: string) => void;
}

/** Presets first in their SPEC order, then everything else by name. */
function sortWorkspaces(list: Workspace[]): Workspace[] {
	const rank = (w: Workspace) => (isPresetId(w.id) ? PRESET_IDS.indexOf(w.id) : PRESET_IDS.length);
	return list.sort((a, b) => rank(a) - rank(b) || a.name.localeCompare(b.name));
}

export class WorkspaceStore {
	workspaces = $state<Workspace[]>([]);
	settings = $state<DeckSettings>({ ...DEFAULT_SETTINGS });
	activeId = $state('');
	monitor = $state('');
	ready = $state(false);
	/** Direction of the last switch, drives the 200 ms slide. */
	lastDir = $state<1 | -1>(1);
	/** Instance ids that didn't fit after the last density change / import, per workspace. */
	unplaced = $state<Record<string, string[]>>({});

	active = $derived(this.workspaces.find((w) => w.id === this.activeId) ?? this.workspaces[0]);

	#dirty = new Set<string>();
	#timer: ReturnType<typeof setTimeout> | null = null;
	#portrait: boolean | null = null;

	private backend: DeckBackend;
	private opts: StoreOptions;

	constructor(backend: DeckBackend, opts: StoreOptions) {
		this.backend = backend;
		this.opts = opts;
	}

	// ── lifecycle ───────────────────────────────────────────────────────────

	async init(): Promise<void> {
		try {
			this.settings = { ...DEFAULT_SETTINGS, ...(await this.backend.getSettings()) };
		} catch (e) {
			this.#report(e);
		}
		try {
			this.monitor = await this.backend.currentMonitor(this.settings);
		} catch {
			this.monitor = 'default';
		}

		let raw: unknown[] = [];
		let listed = false;
		try {
			raw = await this.backend.listWorkspaces();
			listed = true;
		} catch (e) {
			this.#report(e);
		}
		const list: Workspace[] = [];
		for (const entry of raw) {
			try {
				list.push(parseWorkspace(entry));
			} catch (e) {
				// Left on disk untouched; just not shown.
				console.warn('kotilo-deck: skipping invalid workspace', e);
			}
		}
		const seeded = list.length === 0;
		if (seeded) {
			// Never reuse an id that exists on disk but failed validation here: saving over it would lose it.
			const taken = new Set(raw.map((r) => (r as { id?: unknown } | null)?.id).filter((id) => typeof id === 'string'));
			for (const id of PRESET_IDS) {
				const preset = makePreset(id);
				let n = 2;
				while (taken.has(preset.id)) preset.id = `${id}-${n++}`;
				list.push(preset);
			}
		}
		this.workspaces = sortWorkspaces(list);
		// Persist presets only on a genuine first run (folder listed fine and held nothing). If the list
		// failed, keep them in memory only; they are written once the user actually edits them.
		if (seeded && listed && raw.length === 0) for (const w of this.workspaces) this.#touch(w.id);

		this.#portrait = null;
		this.applyViewport();

		const remembered = this.settings.activeWorkspace[this.monitor];
		this.activeId = this.workspaces.some((w) => w.id === remembered) ? remembered : this.workspaces[0].id;
		if (remembered !== this.activeId) {
			void this.#persistSettings({ activeWorkspace: { ...this.settings.activeWorkspace, [this.monitor]: this.activeId } });
		}
		this.ready = true;
		void this.#syncTray();
	}

	/** Writes pending changes now (also called on page hide). */
	async flush(): Promise<void> {
		if (this.#timer) clearTimeout(this.#timer);
		this.#timer = null;
		const ids = [...this.#dirty];
		this.#dirty.clear();
		for (const id of ids) {
			const ws = this.workspaces.find((w) => w.id === id);
			if (!ws) continue;
			try {
				await this.backend.saveWorkspace($state.snapshot(ws) as Workspace);
			} catch (e) {
				this.#report(e);
			}
		}
	}

	// ── switching ───────────────────────────────────────────────────────────

	switchTo(id: string, dir?: 1 | -1): void {
		if (!this.workspaces.some((w) => w.id === id) || id === this.activeId) return;
		const from = this.workspaces.findIndex((w) => w.id === this.activeId);
		const to = this.workspaces.findIndex((w) => w.id === id);
		this.lastDir = dir ?? (to >= from ? 1 : -1);
		this.activeId = id;
		void this.#persistSettings({ activeWorkspace: { ...this.settings.activeWorkspace, [this.monitor]: id } });
		void this.#syncTray();
	}

	cycle(dir: 1 | -1): void {
		const n = this.workspaces.length;
		if (n < 2) return;
		const i = this.workspaces.findIndex((w) => w.id === this.activeId);
		this.switchTo(this.workspaces[(i + dir + n) % n].id, dir);
	}

	/** Deck moved to another monitor: remember per monitor, restore that monitor's last workspace. */
	setMonitor(name: string): void {
		if (name === this.monitor) return;
		this.monitor = name;
		const remembered = this.settings.activeWorkspace[name];
		if (remembered) this.switchTo(remembered);
		else void this.#persistSettings({ activeWorkspace: { ...this.settings.activeWorkspace, [name]: this.activeId } });
	}

	// ── workspace CRUD ──────────────────────────────────────────────────────

	create(name: string): Workspace {
		const clean = name.trim() || t('shell.workspace.untitled');
		const { width, height } = this.opts.viewport();
		const ws: Workspace = {
			schemaVersion: 1,
			id: uniqueId(slugify(clean), this.workspaces.map((w) => w.id)),
			name: clean,
			theme: null,
			grid: defaultDensity(width, height),
			widgets: []
		};
		this.#insert(ws);
		return ws;
	}

	rename(id: string, name: string): boolean {
		const clean = name.trim();
		const ws = this.#find(id);
		if (!ws || !clean) return false;
		// The id (file name) stays stable so per-monitor memory and companion `workspace.switch` keep working.
		ws.name = clean;
		this.workspaces = sortWorkspaces([...this.workspaces]);
		this.#touch(id);
		void this.#syncTray();
		return true;
	}

	duplicate(id: string): Workspace | null {
		const src = this.#find(id);
		if (!src) return null;
		const copy = structuredClone($state.snapshot(src)) as Workspace;
		copy.name = t('shell.workspace.copyName', { name: src.name });
		copy.id = uniqueId(slugify(copy.name), this.workspaces.map((w) => w.id));
		this.#insert(copy);
		return copy;
	}

	/** Never deletes the last workspace. */
	async remove(id: string): Promise<boolean> {
		if (this.workspaces.length <= 1 || !this.#find(id)) return false;
		const idx = this.workspaces.findIndex((w) => w.id === id);
		if (id === this.activeId) {
			const next = this.workspaces[idx + 1] ?? this.workspaces[idx - 1];
			this.switchTo(next.id);
		}
		this.workspaces = this.workspaces.filter((w) => w.id !== id);
		this.#dirty.delete(id);
		const map = { ...this.settings.activeWorkspace };
		let changed = false;
		for (const [mon, ws] of Object.entries(map)) {
			if (ws === id) {
				map[mon] = this.activeId;
				changed = true;
			}
		}
		if (changed) void this.#persistSettings({ activeWorkspace: map });
		void this.#syncTray();
		try {
			await this.backend.deleteWorkspace(id);
		} catch (e) {
			this.#report(e);
		}
		return true;
	}

	/** Replaces layout, theme and appearance with a preset's; keeps id and name. */
	resetToPreset(id: string, preset: PresetId): void {
		const ws = this.#find(id);
		if (!ws) return;
		ws.theme = null;
		delete ws.appearance;
		ws.grid = { cols: 8, rows: 4 };
		ws.widgets = presetWidgets(preset);
		const { width, height } = this.opts.viewport();
		this.#setGrid(ws, defaultDensity(width, height));
		this.#touch(id);
	}

	/** Parses, validates and adds an exported workspace. Throws WorkspaceError with a user message. */
	importJson(text: string): Workspace {
		let data: unknown;
		try {
			data = JSON.parse(text);
		} catch {
			throw new WorkspaceError('shell.errors.json');
		}
		const ws = parseWorkspace(data);
		ws.id = uniqueId(ws.id, this.workspaces.map((w) => w.id));
		this.#insert(ws);
		this.#setGrid(this.#find(ws.id)!, ws.grid);
		return ws;
	}

	exportJson(id: string): string {
		const ws = this.#find(id);
		if (!ws) throw new WorkspaceError('shell.errors.notFound');
		return JSON.stringify($state.snapshot(ws), null, 2) + '\n';
	}

	// ── theme ───────────────────────────────────────────────────────────────

	/** `workspace` scope sets/clears the active workspace override; `global` sets settings.theme. */
	setTheme(scope: 'global' | 'workspace', themeId: string | null): void {
		if (scope === 'global') {
			void this.#persistSettings({ theme: themeId });
			return;
		}
		const ws = this.active;
		if (!ws) return;
		ws.theme = themeId;
		this.#touch(ws.id);
	}

	// ── layout (active workspace) ───────────────────────────────────────────

	setWidgets(widgets: WidgetInstance[]): void {
		const ws = this.active;
		if (!ws) return;
		ws.widgets = widgets;
		this.#touch(ws.id);
	}

	/** Places a new instance of `widget` at `size` in the first free slot; null when there's no room. */
	addWidget(widget: string, size: Size, settings: Record<string, unknown> = {}): WidgetInstance | null {
		const ws = this.active;
		if (!ws) return null;
		const slot = firstFreeSlot(size, ws.widgets, ws.grid);
		if (!slot) return null;
		const inst: WidgetInstance = {
			instance: instanceId(widget, ws.widgets.map((w) => w.instance)),
			widget,
			...slot,
			w: size.w,
			h: size.h,
			settings
		};
		ws.widgets.push(inst);
		this.#touch(ws.id);
		return inst;
	}

	removeWidget(instance: string): void {
		const ws = this.active;
		if (!ws) return;
		ws.widgets = ws.widgets.filter((w) => w.instance !== instance);
		this.#dropUnplaced(ws.id, instance);
		this.#touch(ws.id);
	}

	updateWidgetSettings(instance: string, settings: Record<string, unknown>): void {
		const ws = this.active;
		const inst = ws?.widgets.find((w) => w.instance === instance);
		if (!ws || !inst) return;
		inst.settings = settings;
		this.#touch(ws.id);
	}

	/** Changes the active workspace's density and reflows; returns instance ids that didn't fit. */
	setGrid(grid: GridSize): string[] {
		const ws = this.active;
		if (!ws) return [];
		const out = this.#setGrid(ws, grid);
		this.#touch(ws.id);
		return out;
	}

	/**
	 * Window resized / monitor changed. When the orientation flips, workspaces still on the automatic
	 * density (8×4 ↔ 4×8) follow it; user-chosen densities are left alone.
	 */
	applyViewport(): void {
		const { width, height } = this.opts.viewport();
		const portrait = isPortrait(width, height);
		if (this.#portrait === portrait) return;
		this.#portrait = portrait;
		const target = defaultDensity(width, height);
		for (const ws of this.workspaces) {
			if (!isAutoDensity(ws.grid) || (ws.grid.cols === target.cols && ws.grid.rows === target.rows)) continue;
			this.#setGrid(ws, target);
			this.#touch(ws.id);
		}
	}

	// ── internals ───────────────────────────────────────────────────────────

	#setGrid(ws: Workspace, grid: GridSize): string[] {
		const sizesOf = (it: WidgetInstance) => this.opts.sizesOf(it.widget) ?? [{ w: it.w, h: it.h }];
		const out = reflow($state.snapshot(ws.widgets) as WidgetInstance[], grid, sizesOf);
		ws.grid = { cols: grid.cols, rows: grid.rows };
		ws.widgets = out.items;
		this.unplaced = { ...this.unplaced, [ws.id]: out.unplaced };
		return out.unplaced;
	}

	#dropUnplaced(wsId: string, instance: string) {
		const list = this.unplaced[wsId];
		if (list?.includes(instance)) this.unplaced = { ...this.unplaced, [wsId]: list.filter((i) => i !== instance) };
	}

	#insert(ws: Workspace) {
		this.workspaces = sortWorkspaces([...this.workspaces, ws]);
		this.#touch(ws.id);
		this.switchTo(ws.id);
		void this.#syncTray();
	}

	#find(id: string) {
		return this.workspaces.find((w) => w.id === id);
	}

	#touch(id: string) {
		this.#dirty.add(id);
		if (this.#timer) clearTimeout(this.#timer);
		this.#timer = setTimeout(() => void this.flush(), SAVE_DEBOUNCE_MS);
	}

	async #persistSettings(patch: Partial<DeckSettings>) {
		// Optimistic: local state is the truth for this session; the backend shallow-merges the same patch.
		this.settings = { ...this.settings, ...patch };
		try {
			await this.backend.setSettings(patch);
		} catch (e) {
			this.#report(e);
		}
	}

	async #syncTray() {
		try {
			await this.backend.setTray(
				this.workspaces.map((w) => ({ id: w.id, name: w.name })),
				this.activeId
			);
		} catch {
			// Tray is optional (no tray on some Linux desktops).
		}
	}

	#report(e: unknown) {
		const message = e instanceof Error ? e.message : String(e);
		console.error('kotilo-deck:', message);
		this.opts.onError?.(t('shell.errors.storage', { message }));
	}
}
