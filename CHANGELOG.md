# Changelog

All notable changes to this project are documented here. Format: [Keep a Changelog](https://keepachangelog.com/en/1.1.0/); versioning: [SemVer](https://semver.org/). Maintained by the `release` skill.

## [Unreleased]

### Added
- Linux platform layer: volume, devices and per-app mixer via `pactl` (PulseAudio / PipeWire) with a `wpctl` fallback; media via MPRIS (D-Bus); AMD GPU load from sysfs; `kotilo://` links and folders via `xdg-open`. Not yet run on a real Linux machine.
- Tablet/browser mode: serve the deck on the LAN (off by default, token-protected, QR code in Settings ▸ Tablet & browser); remote browsers get the deck with per-device theme and workspace.
- External widget plugins: drop a folder into `~/.kotilo/deck/widgets/` and it appears in the drawer without a rebuild; sandboxed iframe, no network, theme tokens and system/media/pomodoro data via `docs/plugin-api.md`; example in `examples/widgets/hello/`; Settings ▸ Widgets page.
- Auto-update via GitHub Releases: check at startup and every 24 h (can be turned off), install only on click; Settings ▸ About shows status and progress.
- Volume widget (2×1, 2×2): master slider, mute, output device switcher, per-app mixer (Windows Core Audio).
- Media widget (4×1, 4×2): artwork, title/artist/source, progress with seek, prev/play-pause/next, shuffle/repeat (Windows media transport controls; works with Spotify and browsers).
- Todo widget (2×2, 4×2, 4×4): add/complete/reorder/delete, due dates, English + Turkish quick-add parser, reminders as OS notifications that fire while the deck is hidden, snooze 10 min.
- Settings window (General, Display & monitor, Appearance, Themes, Workspaces, Weather, Burn-in & night, About) with import/export of all settings.
- Appearance layers: solid / wallpaper (image or video) / transparent background, surface opacity per instance/workspace/global, backdrop blur, overlay mode with window opacity.
- Burn-in protection: pixel shift, night schedule and idle dim, screensaver clock face.
- 26 more bundled themes (31 total), light/dark auto switch (OS or schedule), user themes from `~/.kotilo/themes/` with hot reload.
- Undo for widget removal; gesture opt-out for sliders, sheets and menus.
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
- Companion server (`ws://127.0.0.1:47831`, protocol v1 in `protocol.md`) with token auth, single-client replace, keep-alive and a reference client `scripts/mock-desktop.mjs`.
- Pomodoro widget (2×1/2×2/4×2) on a backend-owned timer with session history, daily goal ring, OS notifications and companion suggestions.
- Agent Status widget (2×1/2×2/4×2): agents, approval queue with Approve/Deny reaching the desktop, usage/cost bar, graceful "desktop not running" state.
- Calendar widget (4×2/4×4) backed by ICS feeds (https/webcal) with recurrence, Windows/IANA time zones, offline cache and per-feed errors.
- Settings: Companion page, ICS feed manager, Pomodoro page, workspace auto-switch rules (time, pomodoro start/end, calendar event start).
- Top bar companion status; presets updated with the Phase 3 widgets.

### Changed
- GPU is sampled every 3 s; deck widgets and timers pause while the deck is hidden; logs keep 14 days.
- System 4×2: the CPU sparkline fills the tile and the per-core strip grows on large displays.
- Settings dialogs are centred (Tailwind preflight zeroed dialog margins).
- Tablet/browser mode: plugin widgets load their scripts, styles and images; large video wallpapers stream instead of loading whole; `lan://status` (token URLs, QR) stays on the host. Install in the "ready" state relaunches the app. LAN, update and plugin errors carry an `errorCode` for translation.
