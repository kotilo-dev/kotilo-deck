# Changelog

All notable changes to this project are documented here. Format: [Keep a Changelog](https://keepachangelog.com/en/1.1.0/); versioning: [SemVer](https://semver.org/). Maintained by the `release` skill.

## [Unreleased]

## [1.1.0] - 2026-11-12

### Added
- Glass card style (Settings ▸ Appearance ▸ Card style, also per workspace): frosted, saturated cards and top-bar pills over a wallpaper or transparent background, with a "Wallpaper dimming" slider. The default card opacity in Glass is worked out per theme so text stays readable even over a white or black wallpaper; picking Glass with blur off turns blur on at 32 px.
- Calendar day timeline on large cards: today's events as blocks on an hour axis (09:00–18:00 by default, set per widget), all-day events in a top lane, overlapping events in two lanes with a "+N" block beyond that, and a line for now. The next-event card and rows get bigger on large screens, and in the glass look the countdown is a filled accent chip.
- Media 4×2 takes on the colour of the album artwork in the flat look, toned down per theme so text stays readable.
- Widget settings can offer a time picker (`"format": "time"` in a manifest).

### Changed
- Sliders have a new look: a thick rounded track with an accent fill and a large round thumb, easier to grab on a touch screen.
- Top bar: workspace pills are 44 px tall (touch-target rule) and the theme chip shows four colour swatches of the current theme.
- System widget (4×2): bigger numbers on large screens, a sparkline with a soft fill under it in every tile (CPU, memory, GPU), a "peak N% · 60 s" label for CPU, and a per-core strip coloured by load with the busiest core named.
- Media: artwork fills the card height on large screens, the title grows with the card, and the controls are round: a large filled play button, borderless previous/next, and shuffle/repeat that show a tinted circle when on. In the glass look the play button and progress use the text colour.
- Clock 2×1: on larger cards the time grows with the card, with the full date and the city with its UTC offset underneath. Small cards keep the single-row layout.
- Date 2×1: on larger cards a big day number beside the weekday and "November · Week 45", with a strip of this week's days below (today highlighted, past days dimmed, first day of the week from your locale). Small cards keep the compact layout.
- Weather 2×1: on larger cards a big temperature with the condition and today's high/low, a large filled icon and a one-line 3-day forecast. Large weather icons (56 px and up) are now filled. Small cards keep the compact layout.
- Volume 2×1: the percentage grows with the card, the slider uses the new rounded style, and the Output device and Mixer buttons are 44 px tall (short cards keep them slim but still 44 px to tap).
- The Focus preset now fills the whole grid: Calendar takes the bottom-right block in place of the Date card (the big clock already shows the date). Existing workspaces are not changed; use "Reset to preset" to get it.
- README screenshots and the animated preview are refreshed and show the glass look.

### Fixed
- A media or audio call could fail with "call aborted" right after another call crashed on the background worker (Windows and Linux). The worker now survives a failing job.

## [1.0.1] - 2026-11-06

### Security
- Companion handshake is now mutual: the Deck and Kotilo desktop prove the shared token to each other with HMAC-SHA256 over fresh nonces, and the token itself is never sent over the socket. A program squatting the companion port while the Deck is closed can no longer collect the token or approve agent commands. The old one-step handshake is still accepted for older clients.

### Added
- Companion approvals can be resolved with an "unknown" outcome (answered in the agent's own terminal); the Deck clears them like any other resolved approval.

## [1.0.0] - 2026-11-05

### Added
- First-run setup on the deck: pick the monitor, theme and starting workspace (Focus / Ops / Agents / Full) with live preview; skippable any time (Skip setup / Esc); Settings ▸ About ▸ "Run setup again". Users upgrading from an earlier version skip it.
- Crash logs: a crash writes `~/.kotilo/deck/logs/crash-<UTC>.log` (version, OS, backtrace; home folder redacted) and the next start shows "Kotilo Deck closed unexpectedly last time." with Report issue, Open logs and Dismiss.
- "Report an issue" opens a prefilled GitHub issue (version, OS, arch, WebView, crash summary; no logs or paths are sent), and "Open logs folder" in Settings ▸ About. Uncaught errors in the deck and settings windows are written to the app log (rate-limited).
- Installers: per-user NSIS setup (English/Turkish, no admin rights, installs WebView2 if missing), MSI (English, for administrators), AppImage, .deb and .rpm.
- Release workflow: pushing a `v*` tag builds every installer, its updater signature and `latest.json` into a draft GitHub Release with the changelog as notes; optional Windows code signing from repository secrets.
- winget manifest (`Kotilo.KotiloDeck`) and Flatpak manifest (`dev.kotilo.deck`) with AppStream metadata.
- Bug report issue template; `docs/install.md` and `docs/releasing.md`.
- Demo mode: open the deck in a browser with `?demo` (optionally `&theme=<id>&workspace=<id>`) to try it with sample data; nothing is saved.
- `pnpm screenshots` regenerates the README screenshots (every preset and widget in four themes, 7" and portrait layouts) and the GIF; CI uploads them as an artifact.
- Linux platform layer: volume, devices and per-app mixer via `pactl` (PulseAudio / PipeWire) with a `wpctl` fallback; media via MPRIS (D-Bus); AMD GPU load from sysfs; `kotilo://` links and folders via `xdg-open`. Not yet run on a real Linux machine.
- Tablet/browser mode: serve the deck on the LAN (off by default, token-protected, QR code in Settings ▸ Tablet & browser); remote browsers get the deck with per-device theme and workspace. Rotating a tablet reflows the layout on that device only and never rewrites the host's workspaces.
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

### Fixed
- Pomodoro: removed the square outline around the timer ring (2×2, 4×2); a CSS class clashed with Tailwind's `ring` utility.
- Agents: the 2×2 approval queue is no longer forced to `position: fixed` by a Tailwind class clash. Every custom CSS class that matched a Tailwind utility was renamed, and a test fails if a new one appears.
- Agents: the 2×1 card lists agent rows when it has room instead of a blank area above the approval row.
- Media: the 4×2 card keeps artwork, title and controls together on tall screens; the artwork grows instead of leaving a gap.
- Edit mode: the per-card settings gear no longer covers header info ("2 / 4", "6 min ago", "4 left").
- Ctrl+Z no longer undoes a deck action behind an open dialog.
- Linux: the in-app updater no longer offers updates it cannot install; Flatpak and deb/rpm installs say to update through the software center or package manager (only AppImages update themselves).
- Linux: "Report issue" and other web links open with `xdg-open` even without `xdg-mime` (Flatpak, minimal desktops).

### Changed
- README rewritten for v1.0 with GIF, screenshot gallery, install summary, hardware ideas, shortcuts and privacy notes.
- GPU is sampled every 3 s; deck widgets and timers pause while the deck is hidden; logs keep 14 days.
- System 4×2: the CPU sparkline fills the tile and the per-core strip grows on large displays.
- Settings dialogs are centred (Tailwind preflight zeroed dialog margins).
- Tablet/browser mode: plugin widgets load their scripts, styles and images; large video wallpapers stream instead of loading whole; `lan://status` (token URLs, QR) stays on the host. Install in the "ready" state relaunches the app. LAN, update and plugin errors carry an `errorCode` for translation.

[Unreleased]: https://github.com/kotilo-dev/kotilo-deck/compare/v1.0.0...HEAD
[1.0.0]: https://github.com/kotilo-dev/kotilo-deck/releases/tag/v1.0.0
