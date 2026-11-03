# Decisions

Defaults chosen without asking (SPEC §0). Newest last.

| # | Date | Decision | Why |
|---|---|---|---|
| 1 | 2026-11-03 | Tauri 2 + SvelteKit + TypeScript + Tailwind (spec defaults). | SPEC §3 **(default)**. |
| 2 | 2026-11-03 | SvelteKit 3 with `adapter-static`, `ssr = false`, page prerendered. Kit config lives in `vite.config.ts` (`svelte.config.js` is gone in Kit 3). | Tauri has no Node server; current stable Kit. |
| 3 | 2026-11-03 | Shared frontend code imported as `#lib/<path>/index.ts` via `package.json` `"imports": { "#lib/*": "./src/lib/*" }`. | Kit 3 removed `$lib`; TypeScript needs the explicit file for subpath imports. |
| 4 | 2026-11-03 | TypeScript pinned to `^6`. | Kit 3.1 and svelte-check peer ranges stop at 6; TS 7 is out but not yet supported by them. |
| 5 | 2026-11-03 | Tailwind 4 via `@tailwindcss/vite`, no config file. | Tailwind for layout only; colours stay in `--k-*` variables. |
| 6 | 2026-11-03 | Fonts bundled from `@fontsource/inter` and `@fontsource/jetbrains-mono`. | DESIGN §3: no Google Fonts at runtime, deck may be offline. |
| 7 | 2026-11-03 | Bundled themes live in `src/lib/theme/themes/`, loaded with `import.meta.glob`. | Adding a theme = dropping a JSON; no registry edits. |
| 8 | 2026-11-03 | Vitest for frontend unit tests; `cargo test` for Rust. | Ships with Vite; no extra runner. |
| 9 | 2026-11-03 | App identifier `dev.kotilo.deck`; dev server on port 1420. | Matches kotilo.dev; Tauri's conventional port. |
| 10 | 2026-11-03 | Phase 0 window is 1280×720 borderless, not yet fullscreen. | Fullscreen-on-chosen-monitor is Phase 1 scope. |
| 11 | 2026-11-03 | `Platform` trait methods default to `Unsupported`; `current()` picks the impl via `cfg`. | Lets Linux/macOS compile before they are implemented (SPEC §4.3: never crash). |
| 12 | 2026-11-03 | Phase 1 frontend/backend contract lives in `src/lib/ipc/index.ts`; Rust mirrors it with camelCase serde. | One source of truth for three parallel workstreams. |
| 13 | 2026-11-03 | Settings: one top-level key per field in `settings.json` (tauri-plugin-store); a corrupt value falls back to its own default. Hotkey changes are registered before saving and rolled back if the key is taken. | Partial corruption never resets everything. |
| 14 | 2026-11-03 | Workspace migrations upgrade in memory; disk is rewritten on next save. Future `schemaVersion` files are skipped and never overwritten. Ids `^[a-z0-9-]{1,64}$`, file name wins over inner id, atomic write via temp file + fsync + rename. | SPEC cross-cutting: never break a workspace file. |
| 15 | 2026-11-03 | Monitor: saved name if connected, else first non-primary, else primary; a 5 s watcher re-applies the rule (so with no saved monitor a newly plugged secondary display is adopted). `MonitorInfo` sizes are physical px. | SPEC §2 re-attach. |
| 16 | 2026-11-03 | Closing the deck window hides it to the tray; only tray Quit exits. Tray labels English until tray i18n. | Always-on deck. |
| 17 | 2026-11-03 | System sampler: dedicated thread at 1 Hz only while subscribed; the widget drops its subscription while `document.hidden`. Top processes = two snapshots ~300 ms apart, same-name summed, CPU as share of machine. GPU = busiest PDH GPU engine; name via DXGI. | Idle budget < 1 % CPU. |
| 18 | 2026-11-03 | Weather: backend makes one attempt per call and caches 15 min (memory + `~/.kotilo/deck/cache/weather/`); frontend retries 30 s, 1, 2, 5, 10 min, then every 15 min. Coordinates and queries are never logged. | SPEC §4.4, no PII in logs. |
| 19 | 2026-11-03 | Logs: `~/.kotilo/deck/logs/deck.log.<date>`, info for our crate, warn for deps; `--verbose` → debug; `RUST_LOG` overrides. | SPEC cross-cutting logging. |
| 20 | 2026-11-03 | Clock/date run on the frontend (`Intl`, one shared tick aligned to second/minute, paused while hidden) rather than `chrono` over IPC. | No IPC per tick; OS locale formatting. |
| 21 | 2026-11-03 | Widget defaults: clock 24 h, seconds off, analog 2×2; date day-of-year off; weather °C, no location; system warn threshold 85 % (3 samples over / 3 at-or-under). Percent shown as `23%` in every locale. | Spec/design defaults where given. |
| 22 | 2026-11-03 | Themes added: `dracula`, `nord`, `tokyo-night` (no `pair` yet). Dracula text-muted lightened from Comment `#6272a4` to `#aeb6d0` (same hue) to reach 4.5:1 on surface; Comment stays the border. | DESIGN §9 contrast. |
| 23 | 2026-11-03 | Effective theme = workspace override ?? global setting ?? Mocha; theme picker offers both scopes. Chip label = lower-cased name minus family word for paired themes. | SPEC §4.1 workspace theme override. |
| 24 | 2026-11-03 | Grid: move onto occupied cells swaps or shifts neighbours to the nearest free slot, else rejects with a shake. Density ladder 6×3/8×4/10×5/12×6 (portrait transposed); "auto density" = exactly 8×4 or 4×8 and only those flip on orientation change. Widgets that no longer fit after a density change stay in the file, hidden, with a warning. | Never lose data; no schema field needed. |
| 25 | 2026-11-03 | Workspace order: presets first, then alphabetical. Rename keeps the id. Reset to preset keeps id and name. Import gets a fresh id on collision, missing `schemaVersion` = 1, unknown widgets/fields kept. Saves debounced 400 ms and flushed on Done/page hide. Preset names localised at creation. | SPEC §4.1. |
| 26 | 2026-11-03 | Outside Tauri (browser `pnpm dev`): settings/workspaces in localStorage, no capabilities, Ctrl+Alt+←/→ handled in-page. | Usable browser preview for design work. |
| 27 | 2026-11-03 | Gestures: long-press 500 ms (cancel after 10 px), drag after 6 px, swipe ≥ 64 px horizontal within 1 s, trash = bottom 44 px. Edit-mode keyboard: arrows move, S cycles size, Delete removes, Enter opens settings. | DESIGN §6, keyboard reachability. |
| 28 | 2026-11-03 | Presets are written to disk only on a genuine first run (workspace folder listed fine and empty). `workspace_list` errors on an unreadable folder; seeded ids avoid ids present on disk; `workspace_save` keeps a timestamped `<id>.json.<stamp>.bak` before overwriting a file it could not fully read. The System sampler's subscriber count resets on every webview page load. | Phase 1 review: a bad boot must never overwrite real workspaces; a reload must not leak the sampler. |
| 29 | 2026-11-03 | The 10-minute idle CPU/RAM soak CI job (SPEC §8 cross-cutting) is deferred until a git remote and CI runner exist; Phase 1 idle CPU was measured by hand instead (0.04 % over 30 s, debug build). | No remote yet, so CI has never run. Tracked as an open item. |
