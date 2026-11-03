# Kotilo Deck — Architecture

Owner: `architect` agent. Behaviour: `SPEC.md`. Look: `docs/design/DESIGN.md`.

## Module map

```
kotilo-deck/
├── src/                          SvelteKit 3 SPA (Svelte 5, Tailwind 4), prerendered by adapter-static
│   ├── app.css                   global CSS: fonts (bundled @fontsource), focus ring, reduced motion
│   ├── routes/                   +layout (applies theme) · +page (deck shell)
│   ├── lib/                      imported as #lib/... (package.json "imports")
│   │   ├── theme/                theme loader, token → --k-* mapping, bundled themes/*.json
│   │   ├── grid/        (P1)     cell grid, placement, edit mode, drawer
│   │   ├── workspace/   (P1)     workspace store, presets, switching
│   │   └── ipc/         (P1)     typed wrappers around Tauri invoke/events
│   ├── widgets/<id>/    (P1+)    manifest.json · Widget.svelte · settings.ts  (no hex colours)
│   └── i18n/                     en.json · tr.json
├── src-tauri/                    Tauri 2 backend (Rust)
│   └── src/
│       ├── main.rs / lib.rs      app builder, command registration
│       ├── platform/             Platform trait + windows / linux / stub impls (only place for OS APIs)
│       ├── widgets/     (P1+)    per-widget commands, e.g. system.rs, weather.rs
│       ├── workspace/   (P1)     ~/.kotilo/deck/workspaces/<slug>.json read/write
│       ├── migrations/  (P1)     schemaVersion upgrades, run at startup
│       └── companion/   (P3)     WebSocket server on 127.0.0.1:47831, see protocol.md
├── scripts/lint-hex.mjs          CI: no hardcoded hex under src/widgets/**
├── docs/design/                  design handoff (DESIGN.md, reference comps, theme JSON)
└── .claude/                      agents + skills (SPEC §0)
```

## Platform trait

```rust
pub trait Platform: Send + Sync {
    fn capabilities(&self) -> Vec<Capability>;           // required
    fn has(&self, c: Capability) -> bool;                // derived
    fn audio_devices(&self) -> Result<Vec<AudioDevice>>; // default: Unsupported
    fn volume(&self) -> Result<f32>;
    fn set_volume(&self, level: f32) -> Result<()>;
    fn set_muted(&self, muted: bool) -> Result<()>;
    fn gpu_usage(&self) -> Result<f32>;
    fn set_window_transparent(&self, on: bool) -> Result<()>;
}
pub fn current() -> &'static dyn Platform;              // cfg-selected impl
```

`Capability` = `monitors | system-info | gpu | audio | media | notifications | transparency | companion` (kebab-case on the wire, same strings as widget manifests' `capabilities`). Methods grow per phase; each new one gets an `Unsupported` default so unimplemented OSes keep compiling.

## IPC contract

- Frontend → backend: `invoke('<area>_<verb>', args)`; args/results are serde types, never OS handles.
  - Source of truth for names and shapes: `src/lib/ipc/index.ts`. Rust mirrors it with `#[serde(rename_all = "camelCase")]`.
  - `platform_capabilities() -> Capability[]` (Phase 0)
  - `settings_get() / settings_set(patch)` — `DeckSettings` in `tauri-plugin-store` (`settings.json`)
  - `workspace_list() / workspace_save(workspace) / workspace_delete(id)` — files under `~/.kotilo/deck/workspaces/`
  - `monitor_list() / monitor_select(name)`, `tray_set_workspaces(items, active)`
  - `system_info() / system_subscribe() / system_unsubscribe() / system_top_processes()` — sampler runs only while subscribed
  - `weather_geocode(query) / weather_get(latitude, longitude, unit)` — Open-Meteo, 15 min cache, stale fallback
- Backend → frontend: Tauri events named `<area>://<event>` for pushed data, so the UI never polls:
  `system://sample`, `workspace://cycle`, `workspace://switch`, `monitor://changed`.
- Widgets: `src/widgets/index.ts` globs every `<id>/manifest.json` + `Widget.svelte` (+ optional `settings.ts`, `Settings.svelte`); contract types in `src/widgets/types.ts`.
- Companion (Phase 3): `protocol.md`.

## Data

| Data | Where | Format |
|---|---|---|
| Settings | `tauri-plugin-store` | JSON, `schemaVersion` |
| Workspaces | `~/.kotilo/deck/workspaces/<slug>.json` | JSON (SPEC §4.1), `schemaVersion` |
| Todos, pomodoro history | SQLite via `tauri-plugin-sql` | migrations in `src-tauri/src/migrations/` |
| User themes | `~/.kotilo/themes/*.json` | theme contract, hot-reloaded |
| Logs | `~/.kotilo/deck/logs/` | `tracing`, daily rotation |

## Rendering layers (SPEC §5.3)

background layer → widget surfaces (`--k-surface-a`) → content (`opacity: var(--k-dim)`).
