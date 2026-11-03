# Kotilo Deck — Product Specification

> Full-screen developer dashboard for a secondary display. Part of the Kotilo brand (kotilo.dev).
> Target: Windows first, Linux second (shares platform abstraction with Kotilo Shell), macOS later.

---

## 0. How to use this spec with Claude Code

Read `## 1`–`## 9` fully before writing code. Work phase by phase (`## 8`); do not start a phase until the previous one builds and its acceptance checks pass. Ask only when a decision is genuinely blocked; otherwise pick the option marked **(default)** and note it in `DECISIONS.md`.

### Required agents (define in `.claude/agents/`)

| Agent | Role | Tools |
|---|---|---|
| `architect` | Owns `ARCHITECTURE.md`, module boundaries, IPC contracts. Reviews every new widget before implementation. | Read, Grep, Glob |
| `rust-platform` | Rust/Tauri backend: platform layer (`src-tauri/src/platform/*`), sysinfo, audio, media, notifications. Windows-rs expert. | Read, Edit, Write, Bash |
| `ui-widgets` | Frontend widgets, grid layout, theme engine. Follows `## 5` theme token contract strictly. | Read, Edit, Write, Bash |
| `companion` | Local WebSocket protocol between Deck and the Kotilo desktop app (`## 6`). Keeps `protocol.md` as single source of truth. | Read, Edit, Write |
| `reviewer` | Read-only. Checks each PR/phase against this spec, theme contract, and platform abstraction rules. Reports findings; does not edit. | Read, Grep, Glob |

### Required skills (define in `.claude/skills/`)

| Skill | Purpose |
|---|---|
| `kotilo-theme` | Theme token schema, list of bundled themes with exact hex values, rules for mapping tokens to CSS variables. Shared with Kotilo Shell / launcher. |
| `widget-authoring` | Step-by-step template for adding a widget: manifest, component, Rust command (if any), settings schema, tests. |
| `tauri-platform-layer` | Conventions for `platform::windows` / `platform::linux` modules, trait definitions, how to stub a platform not yet implemented. |
| `companion-protocol` | Message schema, versioning, reconnect rules, how to add a new message type on both sides. |
| `release` | Build, sign, version bump, changelog, GitHub Release, update manifest. |

### Repo conventions
- `pnpm` + `cargo`; `pnpm tauri dev` must run after Phase 1.
- Conventional commits. One widget per PR.
- `DECISIONS.md` for every default chosen without asking.
- `CHANGELOG.md` kept by `release` skill.

---

## 1. Vision

A second-screen "deck" that stays on all day next to the developer's main monitor: ambient information (time, weather, calendar, system load) plus a few controls (volume, media, pomodoro, todo) that are faster to tap on a touch display than to reach via the main OS. Zero-config, beautiful in every popular developer color theme, and aware of the user's AI agents through the Kotilo desktop companion.

### Non-goals (v1)
- Not a Stream Deck plugin; not a macro pad. No arbitrary button/shortcut grid in v1.
- No cloud account, no sync, no telemetry.
- No mobile app. (A browser/tablet mode is a stretch goal, `## 8` Phase 4.)

---

## 2. Target hardware & window behavior

- Secondary monitor of any size; designed for 7"–15" landscape and 1920×1080 primary layout; must degrade gracefully to 1024×600 and portrait.
- Touch-first hit targets (min 44×44 px) but fully mouse/keyboard usable.
- Window: borderless, fullscreen on a user-selected monitor, `always_on_top` optional, remembers monitor by name, re-attaches if the monitor reconnects.
- Burn-in protection: subtle pixel shift every N minutes (configurable), optional dim/night schedule, optional "screensaver" clock face after inactivity.
- Hotkey to toggle visibility (default `Ctrl+Alt+K`).
- Starts with OS (optional), single instance, tray icon with "Show / Settings / Quit".

---

## 3. Tech stack

- **Tauri 2** (Rust backend) **(default)**. Rationale: tiny binary, native API access, same stack reusable for Kotilo Shell helpers.
- Frontend: **SvelteKit + TypeScript** **(default)** (alternative: React; choose Svelte for lower idle CPU on an always-on screen). Tailwind for layout; theme via CSS custom properties only.
- State: `tauri-plugin-store` for settings, **SQLite via `tauri-plugin-sql`** for todos/pomodoro history.
- Crates: `sysinfo`, `windows` (windows-rs), `tokio`, `tokio-tungstenite`, `serde`, `chrono`, `reqwest`, `ical`.
- Notifications: `tauri-plugin-notification`.
- Target idle: < 1% CPU, < 120 MB RAM with all widgets enabled.

---

## 4. Widget system

### 4.1 Workspaces (saved dashboards)
- A **workspace** = a named, saved dashboard: widget placement + per-widget settings + optional theme override. Users keep many (e.g. *Focus*, *Ops*, *Meeting*, *Evening*) and switch between them.
- Switching: swipe left/right on the deck, tap the workspace pill in the top bar, hotkey `Ctrl+Alt+←/→`, tray menu, or companion message `workspace.switch` (e.g. desktop app switches to *Agents* when an approval is pending — optional rule in settings).
- Optional auto-switch rules: by time of day, when pomodoro starts, when a calendar event starts (Phase 3).
- Storage: `~/.kotilo/deck/workspaces/<slug>.json` (schema below); last active workspace per monitor remembered. Import/export a single workspace as JSON; "Duplicate" and "Reset to preset" actions.
- Ships with presets: *Focus* (clock + pomodoro + todo), *Ops* (system + media + calendar), *Agents* (agent status + pomodoro + todo), *Full*.

```json
{
  "id": "focus", "name": "Focus", "theme": null, "grid": { "cols": 8, "rows": 4 },
  "appearance": { "background": "wallpaper:mountains.jpg", "opacity": 0.8, "blur": 0 },
  "widgets": [
    { "instance": "clock-1", "widget": "clock", "x": 0, "y": 0, "w": 4, "h": 2, "settings": { "seconds": true } },
    { "instance": "pomo-1",  "widget": "pomodoro", "x": 4, "y": 0, "w": 2, "h": 2, "settings": { "appearance": { "opacity": 0.6 } } },
    { "instance": "todo-1",  "widget": "todo", "x": 0, "y": 2, "w": 4, "h": 2 }
  ]
}
```

### 4.2 Grid & placement (Android-launcher style)
- Fixed cell grid per workspace: default **8 × 4** landscape (computed from monitor aspect ratio; portrait → 4 × 8); user can change density (6×3 … 12×6). Cells are square-ish; widgets snap to cells.
- Each widget declares the **sizes it supports** as a list of `w×h` cells (like Android widget sizes), e.g. Clock `2×1, 2×2, 4×2`, System `2×2, 4×2`, Todo `2×2, 4×2, 4×4`, Media `4×2, 4×1`. The widget renders a different layout for each size (compact/normal/large); no free-form stretching.
- Edit mode (long-press or `E`): long-press a widget → lift + drag to any free cells; drag the corner handle → cycles only through supported sizes; drop onto occupied cells → surrounding widgets shift if there is room, otherwise the drop is rejected with a shake animation. Empty cells show a faint dot grid in edit mode.
- "+" opens a widget drawer with size previews per widget (same widget may be placed multiple times, each an independent instance with its own settings).
- Per-instance settings via gear icon in edit mode. Remove = drag to trash zone at the bottom edge.

### 4.3 Widget contract
Every widget lives in `src/widgets/<id>/` with:
- `manifest.json` — id, name, icon, **`sizes: ["2x1","2x2","4x2"]`**, default size, settings JSON-schema, required platform capabilities (e.g. `media`, `audio`, `companion`), `multiInstance: true|false`.
- `Widget.svelte` — receives `size` prop and renders the matching variant; uses only theme tokens.
- `settings.ts` — defaults.
- Optional Rust command module `src-tauri/src/widgets/<id>.rs`.
- Widgets requiring a capability the platform doesn't provide render a muted "Not available on this OS" state, never crash.

### 4.4 Widgets (v1)

| Widget | Data source / implementation | Notes |
|---|---|---|
| **Clock** | local time, `chrono` | 12/24h, seconds toggle, analog/digital, multiple time zones (up to 3) |
| **Date** | local | ISO week number, day-of-year optional |
| **Weather** | Open-Meteo (no API key) | Current + 6h + 3-day, °C/°F, location by city search (Open-Meteo geocoding) or manual lat/lon; cache 15 min; offline → last known + "stale" badge |
| **Calendar** | ICS feed URLs (Google/Outlook/iCloud export) **(default)**; OAuth (Graph/Google) Phase 3 | Next 5 events, "starts in 12 min" countdown, all-day events, multiple feeds with per-feed color |
| **System** | `sysinfo`: CPU total + per-core, RAM, optional GPU (Windows: `windows` perf counters) | Sparkline 60 s history; threshold warning color (reuse SysPulse logic: warn when > threshold for 3 consecutive samples); tap → top 5 processes |
| **Volume** | Windows: Core Audio `IAudioEndpointVolume` + default device switcher; Linux: PipeWire/Pulse via `pactl`/`wpctl` | Slider, mute, output device picker, per-app mixer (Windows `IAudioSessionManager2`) Phase 2 |
| **Media** | Windows: `GlobalSystemMediaTransportControlsSessionManager`; Linux: MPRIS (D-Bus) | Artwork, title/artist, play/pause/next/prev, progress bar, source app name |
| **Pomodoro** | local timer, SQLite history | 25/5/15 defaults, custom; auto-start next; daily stat ring; notification + optional sound; **companion-aware** (`## 6`) |
| **Todo** | SQLite | Add/complete/reorder, due date+time, reminder → OS notification at due time and optional "snooze 10 min"; quick-add parser (`buy milk tomorrow 9am`); optional import/export JSON compatible with the Kotilo Todo List cmdpal extension format |
| **Agent Status** (companion) | Kotilo desktop via WebSocket | Running agents, status (idle/working/waiting approval), pending approvals count, today's token/cost; tap approval → sends approve/deny |

---

## 5. Theme engine

### 5.1 Token contract (shared across Kotilo products)
`themes/<id>.json`:
```json
{
  "id": "catppuccin-mocha",
  "name": "Catppuccin Mocha",
  "variant": "dark",
  "colors": {
    "bg": "#1e1e2e", "bg-alt": "#181825", "surface": "#313244",
    "text": "#cdd6f4", "text-muted": "#a6adc8", "border": "#45475a",
    "accent": "#89b4fa", "accent-2": "#cba6f7",
    "success": "#a6e3a1", "warning": "#f9e2af", "danger": "#f38ba8", "info": "#94e2d5"
  },
  "radius": "12px",
  "opacity": { "surface": 0.85, "bg": 1.0 },
  "font": { "ui": "Inter, system-ui", "mono": "JetBrains Mono, monospace" }
}
```
- Widgets use **only** `var(--k-<token>)`. Lint rule: any hardcoded hex in `src/widgets/**` fails CI.
- Surface colors are exposed as both `--k-surface` (opaque) and `--k-surface-a` (with alpha applied); widget cards use `--k-surface-a` so opacity works without any widget code.

### 5.3 Opacity & background layers
Rendering stack, bottom to top: **background layer** → **widget surfaces** → **widget content**. Each has its own opacity so the deck can look solid, glassy, or nearly invisible over a wallpaper.
- **Background layer**: solid theme `bg` **(default)**, or a user image/video wallpaper (`~/.kotilo/deck/wallpapers/`), or *transparent* (Windows: `WS_EX_LAYERED` + DWM transparency; Linux: compositor alpha; shows the desktop/other windows behind the deck — useful when the deck is an overlay rather than fullscreen).
- **Widget surface opacity**: global slider 0–100 % (default from theme `opacity.surface`, typically 85 %), overridable **per workspace** and **per widget instance** (edit-mode gear → *Appearance*). Optional **blur** behind surfaces (backdrop-filter 0–40 px; Windows: Mica/Acrylic when a wallpaper is set) — off by default because it costs GPU on an always-on screen.
- **Content opacity / dim**: night schedule and idle dim lower a global `--k-dim` multiplier (100 % → 30 %) rather than screen brightness, so the deck stays readable on displays with no brightness control. Burn-in protection (`## 2`) uses the same mechanism.
- **Whole-window opacity**: 20–100 % for overlay mode only (Tauri `set_opacity` / `SetLayeredWindowAttributes`); disabled in fullscreen mode where it makes no sense.
- Edit mode temporarily raises all surfaces to 100 % so cell edges are visible; restores on *Done*.
- Transitions: opacity changes animate over 200 ms; respect `prefers-reduced-motion`.
- Persisted: `opacity.surface` in theme JSON (theme default), `appearance.opacity` in workspace JSON and in each widget instance's `settings`.
- Light/dark auto-switch by schedule or OS setting; "pair" field links a light and dark variant.
- User themes: drop a JSON in `~/.kotilo/themes/`; hot-reloads.

### 5.2 Bundled themes (exact official palettes — the `kotilo-theme` skill holds the values)
Catppuccin (Latte, Frappé, Macchiato, Mocha), Gruvbox (Dark, Light), Dracula, Nord, Tokyo Night (Night, Storm, Day), One Dark, Solarized (Dark, Light), Rosé Pine (Main, Moon, Dawn), Everforest (Dark, Light), Kanagawa (Wave, Dragon), Monokai Pro, GitHub (Dark, Light), Ayu (Dark, Mirage, Light), Nightfox, Material (Palenight, Ocean), Oxocarbon.

---

## 6. Companion protocol (Kotilo desktop ↔ Deck)

- Transport: local WebSocket, Deck is **server** on `127.0.0.1:47831` **(default)**, desktop app connects; token in `~/.kotilo/companion.token` (created by whichever side starts first). Reconnect with backoff.
- JSON messages, `{ "v": 1, "type": "...", "payload": {...} }`.
- Desktop → Deck: `agents.state` (list), `agent.approval.requested`, `usage.today`, `pomodoro.suggest` (e.g. agent started a long task → suggest focus block), `workspace.switch` (by slug; only if the user enabled it).
- Deck → Desktop: `approval.respond`, `pomodoro.state` (desktop can show the timer in its status bar / mute notifications during focus), `todo.added` (optional: create a task in Kotilo tasks).
- Deck must be fully functional with no companion connected; Agent Status widget shows "Kotilo desktop not running — open it" state.
- `protocol.md` is the single source of truth and is copied verbatim into the desktop repo.

---

## 7. Settings

Settings window (separate normal window, not on the deck): monitor selection, startup, hotkey, burn-in options, theme, **appearance (background mode, wallpaper, surface opacity, blur, overlay window opacity)**, units, weather location, ICS feeds, notification sound, companion on/off (incl. "allow desktop to switch workspace"), grid density, workspace list with auto-switch rules, import/export all settings and workspaces.

---

## 8. Phases & acceptance

**Phase 0 — Bootstrap** (one session): scaffold Tauri 2 + SvelteKit + Tailwind repo; create the agents and skills from `## 0` (skill bodies may start minimal and grow per phase); `ARCHITECTURE.md` with module map and the `Platform` trait; CI (lint, `cargo test`, `pnpm test`, hardcoded-hex lint, Windows build); `DECISIONS.md`, `CHANGELOG.md`, MIT `LICENSE`, README stub.
*Accept:* `pnpm tauri dev` opens an empty window; CI green on a PR.

**Phase 1 — Skeleton**: Tauri app, fullscreen on chosen monitor, tray, hotkey, cell grid + edit mode (drag, size cycling, drawer), workspaces (create/rename/duplicate/delete/switch, presets), theme engine with 5 themes, Clock, Date, Weather, System widgets.
*Accept:* runs 8 h idle < 1% CPU; theme switch instant; workspaces persist and survive restart; a widget can be placed twice with different sizes; no hardcoded colors.

**Phase 2 — Controls**: Volume, Media, Todo with reminders, all remaining bundled themes, burn-in protection, opacity & background layers (`## 5.3`: wallpaper, surface/per-widget opacity, blur, overlay transparency), settings window.
*Accept:* media controls work with Spotify + browser; todo reminder fires when app is minimized to tray.

**Phase 3 — Companion & calendar**: Pomodoro, Agent Status widget, companion server + `protocol.md`, ICS calendar; OAuth optional.
*Accept:* approval from Deck reaches the desktop app; desktop shows pomodoro state; Deck works standalone when desktop closed.

**Phase 4 — Platform & reach**: Linux (MPRIS, PipeWire), tablet/browser mode (serve UI on LAN with the same token), widget plugin API (external widgets as folders), auto-update.
*Accept:* same workspace file renders identically on Windows and Linux; an external widget folder loads without a rebuild.

**Phase 5 — Release**: first-run onboarding (pick monitor, theme, preset workspace), installer (MSI/NSIS via Tauri bundler, winget manifest; AppImage/Flatpak for Linux), code signing, crash log + "report issue" link, README with screenshots/GIF, `v1.0.0` GitHub Release via the `release` skill.
*Accept:* clean install on a fresh Windows VM to a working deck in < 2 min without docs.

### Cross-cutting (every phase)
- **Data & migrations**: every persisted JSON/SQLite schema carries a `schemaVersion`; migrations live in `src-tauri/src/migrations/` and run at startup; never break an existing workspace file.
- **Logging**: `tracing` to `~/.kotilo/deck/logs/` with daily rotation; `--verbose` flag; no PII in logs.
- **Error handling**: a failing widget shows an inline error state with "retry"; never crashes the deck. Network widgets (weather, calendar) retry with backoff and show stale data.
- **i18n**: all UI strings in `src/i18n/<lang>.json`; ship `en` and `tr`; date/number formatting via `Intl` with the OS locale.
- **Multi-monitor/DPI**: respect per-monitor DPI scaling; relayout on resolution or monitor change; portrait handled.
- **Security**: companion token file is user-only readable; LAN mode (Phase 4) is off by default and requires the token; no remote code loading in plugins (widgets are declarative + sandboxed iframe in Phase 4).
- **Performance budget** is a CI check from Phase 1 on: idle CPU and RAM measured on a 10-minute soak run.

---

## 9. Quality bar

- Unit tests for quick-add parser, ICS parsing, theme loader, protocol codec.
- Integration test: companion handshake with a mock desktop.
- Accessibility: keyboard reachable, visible focus, contrast ≥ 4.5:1 checked per theme in CI.
- Screenshots of every widget in 4 themes generated in CI for the README.
- License: MIT. README with hardware suggestions (7" USB display, old tablet) and GIF.
