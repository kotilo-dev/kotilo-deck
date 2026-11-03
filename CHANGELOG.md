# Changelog

All notable changes to this project are documented here. Format: [Keep a Changelog](https://keepachangelog.com/en/1.1.0/); versioning: [SemVer](https://semver.org/). Maintained by the `release` skill.

## [Unreleased]

### Added
- Phase 0 bootstrap: Tauri 2 + SvelteKit 3 + Tailwind 4 scaffold with an empty themed deck window.
- Theme loader with the shared Kotilo token contract and Catppuccin Mocha/Latte.
- `Platform` trait with Windows, Linux and stub implementations.
- CI: hex-colour lint, type check, Vitest, `cargo fmt`/`clippy`/`test`, Windows build.
- Claude Code agents and skills from SPEC §0.
- Clock widget (2×1, 2×2 analog/digital, 4×2): 12/24 h, seconds, main + up to 3 extra time zones with a searchable picker, ISO week and day of year.
- Date widget (2×1): day number, weekday, ISO week, optional day of year.
- Weather widget (2×1, 2×2, 4×2): Open-Meteo current / 6-hour / 3-day, °C/°F, city search or manual coordinates, stale badge, retry with backoff.
- System widget (2×2, 4×2): CPU sparkline, memory, optional GPU, per-core strip, 3-sample threshold warning, top-5 process sheet.
- Shared wall-clock tick (`src/lib/time`) aligned to second/minute boundaries and paused while the deck is hidden.
- Deck shell: 56 px top bar (workspace pills with sliding indicator, companion status, theme picker, Edit), cell grid with explicit placement, per-widget error boundary with Retry, "Unknown widget" card that keeps the instance's data.
- Edit mode: long-press / `E` / Edit button, dot grid, lift + drag with neighbour shifting or shake-reject, corner handle size cycling, trash zone, widget drawer, per-instance settings sheet (schema form + Appearance opacity), density chip 6×3…12×6, keyboard moves (arrows / S / Delete).
- Workspaces: presets Focus, Ops, Agents, Full; create, rename, duplicate, delete, reset to preset, JSON import/export with validation; debounced saves; active workspace per monitor; swipe and Ctrl+Alt+←/→ switching with a 200 ms slide; localStorage fallback outside Tauri.
- Backend: borderless fullscreen on the chosen monitor (remembered by name, re-attaches on reconnect), tray menu (show/hide, workspaces, monitor, quit), global hotkeys (Ctrl+Alt+K toggle, Ctrl+Alt+←/→ workspace), single instance, settings store, workspace files under `~/.kotilo/deck/workspaces/` with atomic saves and `schemaVersion` migrations, `tracing` logs with daily rotation and `--verbose`.
- System sampler (1 Hz, runs only while subscribed and the deck is visible) with Windows GPU utilisation via PDH; Open-Meteo weather and geocoding with a 15 min memory + disk cache and stale fallback.
- Themes: Dracula, Nord, Tokyo Night; per-workspace theme override; contrast test (text and text-muted ≥ 4.5:1 on surface and bg) for every bundled theme.
