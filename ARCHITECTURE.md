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
    fn capabilities(&self) -> Vec<Capability>;                         // required
    fn has(&self, c: Capability) -> bool;                              // derived
    // every method below defaults to Err(Unsupported(cap))
    fn audio_state(&self) -> Result<AudioState>;                       // + volume/mute/devices
    fn set_default_audio_device(&self, id: &str) -> Result<()>;
    fn audio_sessions(&self) -> Result<Vec<AudioSession>>;             // + per-session volume/mute
    fn watch_audio(&self, on_change: ChangeFn) -> Result<Watch>;       // drop = unregister
    fn media_state(&self) -> Result<MediaState>;
    fn media_control(&self, a: MediaAction) -> Result<()>;
    fn media_seek(&self, seconds: f64) -> Result<()>;
    fn watch_media(&self, on_change: ChangeFn) -> Result<Watch>;
    fn gpu_usage(&self) -> Result<f32>;  fn gpu_name(&self) -> Result<String>;  fn gpu_release(&self);
    fn set_window_opacity(&self, w: NativeWindow, opacity: f64) -> Result<()>;
}
pub fn current() -> &'static dyn Platform;                            // cfg-selected impl
```

Windows: COM/WinRT calls run on dedicated MTA worker threads (`platform/windows/com.rs`); watchers own their own thread and only signal a debounced "changed" — callers re-read state.

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
  - Phase 2: `settings_open / settings_export_all / settings_import_all`, `wallpaper_list / wallpaper_import`, `theme_user_list`, `file_save_as / file_open_text`, `audio_*`, `media_*`, `todo_*` (todos live in a backend-owned SQLite so reminders fire while the UI is hidden)
  - Phase 3: `pomodoro_*` (backend-owned timer + SQLite history), `companion_status / companion_snapshot / companion_respond / companion_open_desktop`, `calendar_upcoming / calendar_refresh`
  - Phase 4: `lan_status`, `plugin_list / plugin_open_folder`, `update_status / update_check / update_install`
- Backend → frontend: Tauri events named `<area>://<event>` for pushed data, so the UI never polls:
  `system://sample`, `workspace://cycle`, `workspace://switch`, `monitor://changed`; Phase 2: `settings://changed`, `deck://visibility`, `audio://state`, `media://state`, `todo://changed`, `todo://reminder`, `theme://user-changed`, `workspace://changed`, `deck://quitting`; Phase 3: `pomodoro://state`, `pomodoro://phase-end`, `companion://status`, `companion://snapshot`, `companion://suggest`, `calendar://changed`; Phase 4: `lan://status`, `plugin://changed`, `update://status`.
- Windows: `deck` (fullscreen/overlay) and `settings` (960×680 normal window, route `/settings`), same SPA.
- Widgets: `src/widgets/index.ts` globs every `<id>/manifest.json` + `Widget.svelte` (+ optional `settings.ts`, `Settings.svelte`); contract types in `src/widgets/types.ts`.
- Companion (Phase 3): `protocol.md` (repo root) is the single source of truth; server in `src-tauri/src/companion/`, reference client `scripts/mock-desktop.mjs`.

## Phase 4

### Linux platform (`platform/linux/`)
- Audio: `pactl` (works on PulseAudio and on PipeWire via pipewire-pulse) with `-f json` output, `wpctl` as fallback for volume/mute; sessions = sink-inputs; `watch_audio` = one long-lived `pactl subscribe` child, killed on drop. Media: MPRIS over the session D-Bus (`zbus`, blocking API on the platform's own threads); `watch_media` = `PropertiesChanged` + `NameOwnerChanged` signals. `open_url_scheme` = `xdg-mime query default x-scheme-handler/<scheme>` then `xdg-open`. GPU: AMD `gpu_busy_percent` in sysfs when present, otherwise unsupported.
- The module is compiled for `target_os = "linux"` **and** for `cfg(test)` on every OS, so the parsers and D-Bus mapping are type-checked and unit-tested on the Windows dev machine (fixtures of real `pactl`/MPRIS output). Unix-only APIs stay behind an inner `#[cfg(unix)]`.

### LAN mode (`src-tauri/src/lan/`, `src/lib/ipc/transport.ts`)
- Off by default. When `lan.enabled`, an HTTP server binds `0.0.0.0:<lan.port>` (default 47832) and serves the prerendered SPA (embedded assets; `../build` from disk in debug builds), `/plugins/<folder>/…` (same CSP as the URI scheme) and `/wallpapers/<file>` (streamed; every `Range` answer is capped at 4 MiB). The page CSP has `connect-src 'self' ws://<Host>` (never a bare `ws:`) and `frame-src 'self'`.
- Auth: the companion token. `GET /?token=<t>` sets an `HttpOnly; SameSite=Strict` cookie and redirects to `/`; every other request needs that cookie (or `Authorization: Bearer <t>`), else 401. Constant-time compare; 10 failed attempts per IP per minute → 429. No CORS headers.
- Exception: `GET/HEAD /plugins/<folder>/…` and `/plugins/_sdk/…` need no credential (and never count toward the 429 limit), because the `sandbox="allow-scripts"` plugin iframe has an opaque origin and browsers withhold the SameSite=Strict cookie on its sub-resources; these are the user's read-only, PLUGIN_CSP'd plugin files with no secrets, and the path checks still apply.
- `POST /api/invoke/<command>` with the JSON args object → `200` JSON result or `4xx/5xx {"error": "..."}`. `GET /api/events` upgrades to a WebSocket that pushes `{ "event": "<area>://<name>", "payload": … }` for every `DeckEvents` name. `GET /api/hello` → `{ "app": "kotilo-deck", "version": "…", "remote": true }` (the SPA uses it to detect remote mode).
- Allowlist (everything else → 403): `platform_capabilities, settings_get, workspace_list, workspace_save, system_info, system_subscribe, system_unsubscribe, system_top_processes, weather_geocode, weather_get, wallpaper_list, theme_user_list, todo_*, audio_*, media_*, pomodoro_*, companion_status, companion_snapshot, companion_respond, calendar_upcoming, calendar_refresh, plugin_list`. Not reachable: settings_set/open/import/export, workspace_delete, monitor_*, tray_*, file_*, wallpaper_import, companion_open_desktop, lan_*, update_*.
- Ref-counted subscriptions (`*_subscribe`) made by a browser are tracked per events-socket and released when it closes: the socket's first frame is `{event:"lan://hello", payload:{socketId}}`, and `*_subscribe`/`*_unsubscribe` invokes send `X-Kotilo-Socket: <socketId>` (missing/unknown → 400). Hub subscriptions keep separate deck and remote owner counts. Host-only events (`workspace://switch|cycle`, `monitor://changed`, `deck://visibility|quitting`, and `lan://status`, which carries the token URLs and QR code) are not forwarded; a remote `workspace_save` emits `workspace://changed`.
- Frontend: `transport.ts` exports `invoke`, `listen`, `isTauri()`, `isRemote()` and `hasBackend()`; `ipc/index.ts` imports from it instead of `@tauri-apps/api`. Remote mode is the deck only (`/settings` shows "Open settings on the host"); per-device state (active workspace, theme override) lives in localStorage, never in `settings_set`. `convertFileSrc` → `/wallpapers/<file>` in remote mode.

### Widget plugins (`src-tauri/src/plugins/`, `src/lib/plugins/`)
- Contract: `docs/plugin-api.md`. Backend scans `~/.kotilo/deck/widgets/`, validates manifests, watches the folder (`plugin://changed`), serves files via the `kotilo-widget` URI scheme with a no-network CSP and the SDK at `_sdk/kotilo-widget.js`.
- Frontend: a reactive registry merges built-in widgets with `ext.<folder>` plugin definitions; a plugin's component is the generic `PluginFrame.svelte` (sandboxed iframe + postMessage bridge). The deck CSP gains `frame-src http://kotilo-widget.localhost kotilo-widget:`.

### Auto-update (`src-tauri/src/update.rs`)
- `tauri-plugin-updater` + `tauri-plugin-process`; endpoint `https://github.com/kotilo-dev/kotilo-deck/releases/latest/download/latest.json`; minisign public key in `tauri.conf.json`, private key outside the repo. Backend checks at startup (+30 s) and every 24 h when `updates.autoCheck`; never installs without `update_install`. Debug builds report `unsupported`. Settings ▸ About shows status, Check now, Install.

## Data

| Data | Where | Format |
|---|---|---|
| Settings | `tauri-plugin-store` | JSON, `schemaVersion` |
| Workspaces | `~/.kotilo/deck/workspaces/<slug>.json` | JSON (SPEC §4.1), `schemaVersion` |
| Todos, pomodoro history | SQLite via `tauri-plugin-sql` | migrations in `src-tauri/src/migrations/` |
| User themes | `~/.kotilo/themes/*.json` | theme contract, hot-reloaded |
| Plugin widgets | `~/.kotilo/deck/widgets/<folder>/` | `docs/plugin-api.md`, hot-reloaded |
| Logs | `~/.kotilo/deck/logs/` | `tracing`, daily rotation |

## Rendering layers (SPEC §5.3)

background layer → widget surfaces (`--k-surface-a`) → content (`opacity: var(--k-dim)`).
