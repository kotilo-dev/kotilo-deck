/**
 * Workspace + deck settings store (SPEC §4.1). Svelte 5 runes; persistence goes through a DeckBackend
 * (Tauri IPC or localStorage). Every mutation marks the workspace dirty; saves are debounced.
 *
 * Ordering guarantees: saves and deletes of one workspace id run strictly one after another (a promise
 * chain per id), and all settings writes share one chain, so rapid switches never land out of order.
 */
import type { Appearance, DeckSettings, WidgetInstance, Workspace } from '#lib/ipc/index.ts';
import { t } from '#lib/i18n/index.ts';
import {
	defaultDensity,
	firstFreeSlot,
	fits,
	isAutoDensity,
	isPortrait,
	reflow,
	type GridSize,
	type Size
} from '#lib/grid/index.ts';
import { DEFAULT_SETTINGS, normalizeSettings, type DeckBackend } from './backend';
import { isPresetId, makePreset, PRESET_IDS, presetWidgets, type PresetId } from './presets';
import { instanceId, parseWorkspace, slugify, uniqueId, WorkspaceError } from './validate';

export const SAVE_DEBOUNCE_MS = 400;
/** How long after our own `activeWorkspace` write a different incoming value counts as a stale echo. */
export const ECHO_GUARD_MS = 1000;

export interface StoreOptions {
	/** Supported sizes of a widget id; undefined for widgets that aren't registered. */
	sizesOf: (widget: string) => readonly Size[] | undefined;
	viewport: () => { width: number; height: number };
	onError?: (message: string) => void;
	/**
	 * "deck" (default) owns the active workspace, the tray and viewport-driven density. "settings" (the
	 * settings window) only edits files and settings: no switching, no tray, no seeding writes.
	 */
	role?: 'deck' | 'settings';
}

/** Presets first in their SPEC order, then everything else by name. */
function sortWorkspaces(list: Workspace[]): Workspace[] {
	const rank = (w: Workspace) => (isPresetId(w.id) ? PRESET_IDS.indexOf(w.id) : PRESET_IDS.length);
	return list.sort((a, b) => rank(a) - rank(b) || a.name.localeCompare(b.name));
}

/** JSON with sorted keys, to compare workspaces regardless of key order. */
function stable(v: unknown): string {
	if (Array.isArray(v)) return `[${v.map(stable).join(',')}]`;
	if (v && typeof v === 'object') {
		const o = v as Record<string, unknown>;
		return `{${Object.keys(o)
			.filter((k) => o[k] !== undefined)
			.sort()
			.map((k) => `${JSON.stringify(k)}:${stable(o[k])}`)
			.join(',')}}`;
	}
	return JSON.stringify(v);
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
	/** Per workspace id: tail of its save/delete chain. */
	#chains = new Map<string, Promise<void>>();
	/** Last disk version we listed or wrote, per id (normalized JSON), to detect changes made elsewhere. */
	#known = new Map<string, string>();
	#settingsChain: Promise<void> = Promise.resolve();
	#settingsPending = 0;
	#lastActiveWrite = 0;
	#notifyTimer: ReturnType<typeof setTimeout> | null = null;

	private backend: DeckBackend;
	private opts: StoreOptions;

	constructor(backend: DeckBackend, opts: StoreOptions) {
		this.backend = backend;
		this.opts = opts;
	}

	get role(): 'deck' | 'settings' {
		return this.opts.role ?? 'deck';
	}

	#isDeck(): boolean {
		return this.role === 'deck';
	}

	// ── lifecycle ───────────────────────────────────────────────────────────

	async init(): Promise<void> {
		try {
			this.settings = normalizeSettings(await this.backend.getSettings());
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
				const ws = parseWorkspace(entry);
				list.push(ws);
				this.#known.set(ws.id, stable(ws));
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
		// The settings window never seeds files: the deck does that.
		if (this.#isDeck() && seeded && listed && raw.length === 0) for (const w of this.workspaces) this.#touch(w.id);

		const remembered = this.settings.activeWorkspace[this.monitor];
		this.activeId = this.workspaces.some((w) => w.id === remembered) ? remembered : this.workspaces[0].id;
		if (this.#isDeck()) {
			this.#portrait = null;
			this.applyViewport();
			if (remembered !== this.activeId) {
				void this.#persistSettings({ activeWorkspace: { ...this.settings.activeWorkspace, [this.monitor]: this.activeId } });
			}
		}
		this.ready = true;
		void this.#syncTray();
	}

	/** Writes pending changes now (also called on page hide / quit) and waits for every queued write. */
	async flush(): Promise<void> {
		if (this.#timer) clearTimeout(this.#timer);
		this.#timer = null;
		const ids = [...this.#dirty];
		this.#dirty.clear();
		for (const id of ids) {
			void this.#enqueue(id, async () => {
				// Snapshot when the job runs, so a queued save always writes the newest state.
				const ws = this.#find(id);
				if (!ws) return;
				const snap = $state.snapshot(ws) as Workspace;
				await this.backend.saveWorkspace(snap);
				this.#remember(snap);
				this.#wrote();
			});
		}
		await Promise.all([...this.#chains.values(), this.#settingsChain]);
	}

	/**
	 * Workspace files changed elsewhere (`workspace://changed`): writes our pending edits first, then
	 * re-reads. Workspaces edited here meanwhile keep the local version; unchanged ones keep their object
	 * identity so widgets don't re-run effects (e.g. refetch weather).
	 */
	async reload(): Promise<void> {
		// Let writes already in flight land, but don't start new ones: pending edits are reconciled below.
		await Promise.all(this.#chains.values());
		let raw: unknown[];
		try {
			raw = await this.backend.listWorkspaces();
		} catch (e) {
			this.#report(e);
			return;
		}
		const next: Workspace[] = [];
		const seen = new Set<string>();
		for (const entry of raw) {
			let ws: Workspace;
			try {
				ws = parseWorkspace(entry);
			} catch {
				continue;
			}
			if (seen.has(ws.id)) continue;
			seen.add(ws.id);
			const disk = stable(ws);
			const local = this.#find(ws.id);
			// A local unsaved edit survives only if nobody else changed the file since we last saw it.
			const keepLocal = local && this.#dirty.has(ws.id) && this.#known.get(ws.id) === disk;
			if (local && this.#dirty.has(ws.id) && !keepLocal) this.#dirty.delete(ws.id);
			if (keepLocal) next.push(local);
			else if (local && stable($state.snapshot(local)) === disk) next.push(local);
			else next.push(ws);
			this.#known.set(ws.id, disk);
		}
		for (const id of [...this.#dirty]) {
			const local = this.#find(id);
			if (!local || seen.has(id)) continue;
			// Known on disk before and gone now = deleted (or replaced by an import) elsewhere: never resurrect.
			// Never written yet (created here moments ago) = keep, it is saved by the pending flush.
			if (this.#known.has(id)) {
				this.#dirty.delete(id);
				this.#known.delete(id);
			} else next.push(local);
		}
		for (const id of [...this.#known.keys()]) if (!seen.has(id)) this.#known.delete(id);
		if (next.length === 0) return; // never leave the deck without a workspace
		this.workspaces = sortWorkspaces(next);
		if (!this.workspaces.some((w) => w.id === this.activeId)) {
			const remembered = this.settings.activeWorkspace[this.monitor];
			const id = this.workspaces.some((w) => w.id === remembered) ? remembered : this.workspaces[0].id;
			if (this.#isDeck()) this.switchTo(id);
			else this.activeId = id;
		}
		void this.#syncTray();
	}

	/** Re-reads settings from the backend (after an import). */
	async reloadSettings(): Promise<void> {
		try {
			this.applyRemoteSettings(await this.backend.getSettings());
		} catch (e) {
			this.#report(e);
		}
	}

	/**
	 * Settings written by another window (`settings://changed`). Ignored while our own writes are in flight:
	 * the last of them adopts the backend's merged result. A different `activeWorkspace` for this monitor
	 * switches the deck, unless it arrives right after our own switch (a stale echo).
	 */
	applyRemoteSettings(input: DeckSettings): void {
		if (this.#settingsPending > 0) return;
		const next = normalizeSettings(input);
		const want = next.activeWorkspace[this.monitor];
		if (this.#isDeck() && want && want !== this.activeId) {
			if (Date.now() - this.#lastActiveWrite < ECHO_GUARD_MS || !this.#find(want)) {
				next.activeWorkspace = { ...next.activeWorkspace, [this.monitor]: this.activeId };
			} else {
				this.settings = next;
				this.switchTo(want, undefined, false);
				return;
			}
		}
		this.settings = next;
	}

	/**
	 * Writes a settings patch (optimistic, serialized). Rejects with the backend's error, e.g. a hotkey
	 * that is already taken; local state then re-syncs from the backend. Not reported as a toast.
	 */
	updateSettings(patch: Partial<DeckSettings>): Promise<void> {
		return this.#persistSettings(patch, false);
	}

	// ── switching ───────────────────────────────────────────────────────────

	switchTo(id: string, dir?: 1 | -1, persist = true): void {
		if (!this.workspaces.some((w) => w.id === id) || id === this.activeId) return;
		const from = this.workspaces.findIndex((w) => w.id === this.activeId);
		const to = this.workspaces.findIndex((w) => w.id === id);
		this.lastDir = dir ?? (to >= from ? 1 : -1);
		this.activeId = id;
		if (persist && this.#isDeck()) {
			this.#lastActiveWrite = Date.now();
			void this.#persistSettings({ activeWorkspace: { ...this.settings.activeWorkspace, [this.monitor]: id } });
		}
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

	/** Settings window: which workspace `monitor` shows (the deck follows via `settings://changed`). */
	setDefaultFor(monitor: string, id: string): void {
		if (!this.#find(id)) return;
		void this.#persistSettings({ activeWorkspace: { ...this.settings.activeWorkspace, [monitor]: id } });
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
		const fallback = (this.workspaces[idx + 1] ?? this.workspaces[idx - 1]).id;
		if (id === this.activeId) {
			if (this.#isDeck()) this.switchTo(fallback);
			else this.activeId = fallback;
		}
		this.workspaces = this.workspaces.filter((w) => w.id !== id);
		this.#dirty.delete(id);
		const map = { ...this.settings.activeWorkspace };
		let changed = false;
		for (const [mon, ws] of Object.entries(map)) {
			if (ws === id) {
				map[mon] = this.#isDeck() ? this.activeId : fallback;
				changed = true;
			}
		}
		if (changed) void this.#persistSettings({ activeWorkspace: map });
		void this.#syncTray();
		// Queued behind any save of the same id, so a debounced save can't resurrect the file.
		await this.#enqueue(id, async () => {
			await this.backend.deleteWorkspace(id);
			this.#known.delete(id);
			this.#wrote();
		});
		return true;
	}

	/** Replaces layout, theme and appearance with a preset's; keeps id and name. */
	resetToPreset(id: string, preset: PresetId): void {
		const ws = this.#find(id);
		if (!ws) return;
		const portrait = ws.grid.rows > ws.grid.cols;
		ws.theme = null;
		delete ws.appearance;
		ws.grid = { cols: 8, rows: 4 };
		ws.widgets = presetWidgets(preset);
		// The settings window's own size says nothing about the deck: keep the workspace's orientation there.
		const { width, height } = this.#isDeck()
			? this.opts.viewport()
			: portrait
				? { width: 600, height: 1024 }
				: { width: 1024, height: 600 };
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

	/** Returns what was removed (for Undo), or null. */
	removeWidget(instance: string): { workspace: string; widget: WidgetInstance } | null {
		const ws = this.active;
		const found = ws?.widgets.find((w) => w.instance === instance);
		if (!ws || !found) return null;
		const widget = structuredClone($state.snapshot(found)) as WidgetInstance;
		ws.widgets = ws.widgets.filter((w) => w.instance !== instance);
		this.#dropUnplaced(ws.id, instance);
		this.#touch(ws.id);
		return { workspace: ws.id, widget };
	}

	/** Undo of `removeWidget`: back in its old cells if they are still free, else the first free slot. Call once. */
	restoreWidget(workspace: string, widget: WidgetInstance): boolean {
		const ws = this.#find(workspace);
		if (!ws) return false;
		const slot = fits(widget, ws.widgets, ws.grid) ? { x: widget.x, y: widget.y } : firstFreeSlot(widget, ws.widgets, ws.grid);
		if (!slot) return false;
		// A widget added since may have reused the instance id: the restored one then gets a fresh id.
		const taken = ws.widgets.map((w) => w.instance);
		const instance = taken.includes(widget.instance) ? instanceId(widget.widget, taken) : widget.instance;
		ws.widgets.push({ ...widget, ...slot, instance });
		this.#touch(ws.id);
		return true;
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
		return this.active ? this.setGridFor(this.active.id, grid) : [];
	}

	setGridFor(id: string, grid: GridSize): string[] {
		const ws = this.#find(id);
		if (!ws) return [];
		const out = this.#setGrid(ws, grid);
		this.#touch(ws.id);
		return out;
	}

	/** Merges a workspace-level appearance override (SPEC §5.3); `undefined` values clear a key. */
	setWorkspaceAppearance(id: string, patch: Partial<Appearance>): void {
		const ws = this.#find(id);
		if (!ws) return;
		const next: Record<string, unknown> = { ...(ws.appearance ?? {}) };
		for (const [k, v] of Object.entries(patch)) {
			if (v === undefined) delete next[k];
			else next[k] = v;
		}
		if (Object.keys(next).length) ws.appearance = next as Appearance;
		else delete ws.appearance;
		this.#touch(id);
	}

	/**
	 * Window resized / monitor changed. When the orientation flips, workspaces still on the automatic
	 * density (8×4 ↔ 4×8) follow it; user-chosen densities are left alone. Deck only.
	 */
	applyViewport(): void {
		if (!this.#isDeck()) return;
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
		if (this.#isDeck()) this.switchTo(ws.id);
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

	#remember(ws: Workspace) {
		try {
			// Normalized like a listed file, so "unchanged on disk" compares equal after a round trip.
			this.#known.set(ws.id, stable(parseWorkspace(structuredClone(ws))));
		} catch {
			this.#known.delete(ws.id);
		}
	}

	#enqueue(id: string, job: () => Promise<void>): Promise<void> {
		const run = async () => {
			try {
				await job();
			} catch (e) {
				this.#report(e);
			}
		};
		const tail = (this.#chains.get(id) ?? Promise.resolve()).then(run);
		this.#chains.set(id, tail);
		void tail.then(() => {
			if (this.#chains.get(id) === tail) this.#chains.delete(id);
		});
		return tail;
	}

	/** Coalesced "files changed" ping to the other window. */
	#wrote() {
		if (this.#notifyTimer) clearTimeout(this.#notifyTimer);
		this.#notifyTimer = setTimeout(() => {
			this.#notifyTimer = null;
			void this.backend.notifyWorkspacesChanged().catch(() => {});
		}, 120);
	}

	#persistSettings(patch: Partial<DeckSettings>, report = true): Promise<void> {
		// Optimistic: local state is the truth for this session; the backend shallow-merges the same patch.
		this.settings = { ...this.settings, ...patch };
		this.#settingsPending++;
		let failure: unknown = null;
		const run = async () => {
			try {
				const merged = await this.backend.setSettings(patch);
				// Last write in the queue: adopt the backend's merged view (includes other windows' changes).
				if (this.#settingsPending === 1 && merged) this.settings = normalizeSettings(merged);
			} catch (e) {
				failure = e;
				if (report) this.#report(e);
				if (this.#settingsPending === 1) {
					try {
						this.settings = normalizeSettings(await this.backend.getSettings());
					} catch {
						// Keep the optimistic state.
					}
				}
			} finally {
				this.#settingsPending--;
			}
		};
		const p = this.#settingsChain.then(run);
		this.#settingsChain = p;
		return p.then(() => {
			if (failure !== null) throw failure;
		});
	}

	async #syncTray() {
		if (!this.#isDeck()) return;
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
