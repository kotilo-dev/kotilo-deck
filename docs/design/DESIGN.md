# Kotilo Deck — UI Design Handoff

> Companion to `kotilo-deck-spec.md`. This file is the visual/interaction contract for the `ui-widgets` agent.
> Reference mockups live in `reference/` (plain HTML, open in a browser). Theme JSONs in `themes/`.
> When this file and the product spec disagree on behaviour, the spec wins; on look, this file wins.

## 0. How to use with Claude Code

- Put this folder at `docs/design/` in the repo. Add to `CLAUDE.md`: *"Read `docs/design/DESIGN.md` before touching anything under `src/widgets/`, `src/lib/grid/` or `src/lib/theme/`."*
- `ui-widgets` agent: every widget PR must match the matching section in `## 5` for **each supported size**.
- `reviewer` agent: check PRs against `## 9` (checklist). Reject any hardcoded hex under `src/widgets/**`.
- `reference/*.html` are pixel references, not code to copy. They are 1280×720 (deck) and 960×680 (settings) static comps.

---

## 1. Design principles

1. **Ambient first.** Readable at arm's length across a desk: big numerals, few words, high contrast. Nothing blinks.
2. **Theme is data.** Every colour is a `--k-*` token. A new theme JSON must make the whole deck look right without touching a widget.
3. **Touch and glance.** Hit targets ≥ 44×44 px. Primary action of a widget is one tap (pause pomodoro, approve agent, play/pause).
4. **Quiet surfaces.** Flat cards with a 1 px border; no gradients, no glow, no drop shadows except the *lifted* card in edit mode.
5. **Mono for numbers, sans for words.** Time, percentages, temperatures, sizes → `--k-font-mono`. Everything else → `--k-font-ui`.

---

## 2. Theme tokens → CSS variables

Theme JSON (`themes/<id>.json`, schema from spec §5.1) maps 1:1 to CSS custom properties on `:root`:

| JSON key | CSS var | Used for |
|---|---|---|
| `colors.bg` | `--k-bg` | deck background (solid mode) |
| `colors.bg-alt` | `--k-bg-alt` | top-bar pills, inputs, inner chips, sidebar |
| `colors.surface` | `--k-surface` | widget card (opaque) |
| *(derived)* | `--k-surface-a` | `rgba(surface, opacity.surface)` — **widget cards use this one** |
| `colors.text` | `--k-text` | primary text |
| `colors.text-muted` | `--k-text-muted` | labels, secondary text (must still pass 4.5:1 on surface) |
| `colors.border` | `--k-border` | card and control borders, ring track |
| `colors.accent` | `--k-accent` | active workspace pill, primary buttons, pomodoro ring, sliders, focus ring |
| `colors.accent-2` | `--k-accent-2` | media widget accent (artwork placeholder, play button, progress) |
| `colors.success` | `--k-success` | companion connected dot, Approve button, memory bar |
| `colors.warning` | `--k-warning` | threshold warnings, "stale" badge, due-soon times, approval-pending border |
| `colors.danger` | `--k-danger` | trash zone, destructive confirms |
| `colors.info` | `--k-info` | memory/secondary meters |
| `radius` | `--k-radius` | card radius (default 16 px) |
| `font.ui` / `font.mono` | `--k-font-ui` / `--k-font-mono` | |
| *(runtime)* | `--k-dim` | 1.0 → 0.3 multiplier for night/idle dim (applied as `opacity` on the content layer) |

Rules:
- Widgets use **only** `var(--k-…)`. CI lint: regex `#[0-9a-fA-F]{3,8}` under `src/widgets/**` fails the build (allow-list: `transparent`, `currentColor`).
- Text on an **accent-filled** button uses the theme's `bg-alt` (dark themes) or `#fff` (light themes). Expose this as `--k-on-accent`, computed by the theme loader: if `variant == "dark"` → `bg-alt`, else `#ffffff`.
- Tinted states (warning card, selected calendar row) are `color-mix(in srgb, var(--k-warning) 18%, var(--k-surface))` — never a second hex.

Bundled reference values are in `themes/catppuccin-mocha.json` and `themes/catppuccin-latte.json`. The `kotilo-theme` skill holds the rest.

---

## 3. Typography & spacing

Fonts: **Inter** (ui), **JetBrains Mono** (mono). Bundle them (no Google Fonts at runtime — the deck may be offline).

| Role | Font | Size / weight | Notes |
|---|---|---|---|
| Clock 4×2 | mono | 128 / 500, `letter-spacing: -0.04em`, `line-height: 1` | seconds: 40 / 400 muted |
| Hero number (weather, CPU, timer) | mono | 48 / 500 (2×2) · 32 / 500 (2×1 & settings) | |
| Timer in ring | mono | 30 / 600 | |
| Widget title | ui | 13 / 600 | top-left of every card |
| Widget meta (right of title) | ui | 11 / 400 muted | "6 min ago", "2 left", "60 s" |
| Body row | ui | 13 / 400 | todo item, calendar title |
| Tiny label | ui | 11 / 400 muted, uppercase, `letter-spacing: .08em` | "BERLIN", "CPU" |
| Button label | ui | 12–13 / 600 | |
| Size chip | mono | 11 / 400 | "2×2" |

Spacing scale: 4, 6, 8, 10, 12, 14, 16, 20, 24. Card padding: **18 px 20 px** (2×N cards), **22 px 28 px** (4×2 clock), **14 px 18 px** (1-row cards). Inner chips radius 10 px, buttons 12 px, pills 999 px.

---

## 4. Deck shell

### 4.1 Window
Fullscreen on the chosen monitor. Root is a column: **top bar 56 px** + **grid area** (`padding: 8px 24px 24px`, `gap: 16px`).

### 4.2 Top bar (56 px)
Three zones, `justify-content: space-between`:
- **Left (min 220 px):** Kotilo diamond mark (22 px, accent stroke, accent-2 cross) + "Kotilo Deck" 14/600.
- **Center:** workspace switcher — pill group on `--k-bg-alt`, 1 px border, radius 999, padding 4. Each pill 36 px tall, padding 0 18, 13 px. Active: `--k-accent` fill, `--k-on-accent` text, 600. Inactive: transparent, muted, 500. Swipe/hotkey changes active pill with a 200 ms slide.
- **Right (min 220 px):** companion status (8 px dot: success = connected, border = not running; label 12 px muted) · theme chip (mono 12, 1 px border, radius 8, e.g. `mocha`) · **Edit** icon button 44×44, 1 px border, radius 12.

In **edit mode** the top bar becomes: accent dot + "Editing *Focus*" + grid density chip (`8 × 4`) | hint text | **Add widget** (secondary) + **Done** (primary). Bottom border 1 px.

### 4.3 Grid
`display:grid; grid-template-columns: repeat(cols, minmax(0,1fr)); grid-template-rows: repeat(rows, minmax(0,1fr)); gap:16px`. Each widget instance is placed with explicit `grid-column: x+1 / span w; grid-row: y+1 / span h` (never auto-placement). At 1280×720 with 8×4 a cell is ≈140×142 px; widgets must stay legible down to **1024×600** (cell ≈ 106×112) — hide the meta label and secondary rows first, never shrink the hero number below 32 px.

### 4.4 Widget card anatomy
```
┌ card: bg --k-surface-a · 1px --k-border · radius --k-radius · box-sizing border-box · min-height 0 ┐
│ header row: title 13/600 ─────────────────────────── meta 11 muted │
│ body (flex column, justify space-between)                          │
│ footer / actions (44px buttons)                                    │
└────────────────────────────────────────────────────────────────────┘
```
Cards never scroll; a size variant shows what fits. Attention border: `border-color: var(--k-warning)` when the widget needs the user (pending approval, threshold breached).

---

## 5. Widgets — per-size specification

Sizes are `w×h` in cells. "Compact" = 1-row variants.

### 5.1 Clock — `2×1, 2×2, 4×2`
- **4×2:** time 128 mono + seconds 40 muted on one baseline. Bottom row: left = "Tuesday, 3 November" 18/600 over "Week 45 · Day 307 · Istanbul UTC+3" 13 muted; right = up to 3 extra time zones, each tiny-label city + mono 20 time, right-aligned, 20 px apart.
- **2×2:** header "City" + mono 12 time; analog face 150 px (face `--k-bg`, 2 px border, 4 quarter ticks 3 px, hour hand 5 px `--k-text`, minute hand 3 px `--k-accent`, 4 px accent hub); footer "Tue 3 Nov · W45" 12 muted. Digital option: 48 mono centered.
- **2×1:** mono 32 time left, date 12 muted right.

### 5.2 Date — `2×1`
Big day number mono 32, weekday + ISO week on the right. (Same style as clock 2×1.)

### 5.3 Weather — `2×1, 2×2, 4×2`
- **2×2:** header city + "N min ago"; hero 48 mono `14°` with "Partly cloudy · H 16° L 9°" 13 muted; condition icon 56 px (stroke 1.6, sun in `--k-warning`, cloud in `--k-text-muted`); 3-day row: three chips on `--k-bg-alt`, radius 10, "Wed" 11 muted over mono 13 `16° 9°` (low muted).
- **2×1:** city + stale badge, condition text 12 muted | hero 32 + 36 px icon.
- **4×2:** 2×2 content + 6-hour strip (6 columns: hour, icon 20 px, temp).
- **Stale state:** badge "stale" 11/500, warning tint chip; subtitle "last update N min ago".

### 5.4 Calendar — `4×2, 4×4`
- Header "Up next" + "2 feeds · Tue 3 Nov".
- **Next event card:** accent-tinted (`color-mix` 14% accent on surface), 1 px accent border, 4 px feed-colour bar, title 15/600, "10:00 – 10:45 · Teams · Work" 12 muted, countdown chip mono 14/600 on `--k-surface` → "in 12 min" (switches to "now" at 0, danger text if overrunning).
- Following rows: 4 px feed bar · mono 12 time range (92 px col) · title 13 · feed name 11 muted. All-day shows "All day".
- 4×2 shows next + 3 rows; 4×4 next + 8 rows.

### 5.5 System — `2×2, 4×2`
- **2×2:** header + "60 s"; CPU row: label 12 muted / mono 20 value, sparkline 36 px tall (2 px accent line, 60 samples, no fill); Memory & GPU: label/value row + 6 px bar on `--k-bg-alt` (fill `--k-info` memory, `--k-warning` when in warn state).
- **4×2:** header + CPU name; three stat tiles on `--k-bg` radius 12: tiny label, mono 32 value, sparkline (CPU) / "11.2 of 32 GB" (RAM) / model (GPU). Warn tile: warning tint bg + warning border, text darkened for contrast. Below: per-core bar strip 40 px tall, 4 px gap, 3 px radius, accent.
- **Warn rule** (from SysPulse): value > threshold for 3 consecutive samples → tile/bar turns warning, header meta shows "N samples over X%". Clears after 3 samples under.
- Tap anywhere → modal sheet with top-5 processes (name, CPU %, RAM).

### 5.6 Volume — `2×1, 2×2`
- **2×1:** speaker icon 18 + device name 13/600 | mono 13 percent; native `<input type=range>` 28 px tall with `accent-color: var(--k-accent)`; optional "Output device ▾" ghost button 28 px.
- **2×2:** same + device list (radio rows 40 px) and mute toggle.
- Muted state: icon with slash, percent replaced by "Muted" muted.

### 5.7 Media — `4×1, 4×2`
- **4×1:** artwork 84 px radius 12 (fallback: `--k-accent-2` square with note icon in `--k-bg`); title 15/600 ellipsis + "Artist · Source" 12 muted; progress 4 px with mono 11 times; 3 buttons 44 px (prev ghost · pause filled `--k-accent-2` · next ghost).
- **4×2:** artwork 120 px radius 14; source as tiny label, title 20/600, artist 13 muted; like button top-right; progress 6 px; 5 buttons (shuffle · prev · play/pause 56×44 filled · next · repeat), shuffle/repeat muted when off.
- No session → artwork placeholder + "Nothing playing" muted, buttons disabled (opacity .4).

### 5.8 Pomodoro — `2×1, 2×2, 4×2`
- **2×2:** header "Focus" / "Break" + mono "2 / 4" sessions; ring 136 px (track 10 px `--k-border`, progress 10 px `--k-accent` round caps, starts at 12 o'clock, `stroke-dashoffset` animates 1 s linear); center mono 30/600 remaining + "of 25:00" 11 muted; footer: pause/play 44 px filled accent + skip 44 px ghost. Break ring uses `--k-success`.
- **2×1:** mini ring 48 px left, time + phase middle, one 44 px button right.
- **4×2:** 2×2 + daily stat ring (sessions done / goal) and today's focus minutes.
- Companion suggestion (`pomodoro.suggest`): a 36 px inline banner "Agent started a long task — start a focus block?" with **Start** ghost button; auto-hides after 60 s.

### 5.9 Todo — `2×2, 4×2, 4×4`
- Header "Today" + "N left". Rows 13 px: native checkbox 18 px (`accent-color: var(--k-accent)`), text, due time mono 11 right (warning colour when due within 1 h, muted otherwise). Completed: muted + line-through, sinks to bottom.
- Quick-add input 40 px, `--k-bg-alt`, 1 px border, radius 10, placeholder `Add… "deploy tomorrow 9am"`; parsed date/time shows as a chip inside the field while typing.
- 4×2 shows 6 rows in 2 columns; 4×4 shows grouped by day with sticky headers.
- Reminder fired → row pulses once (border warning 300 ms), OS notification handled by backend.

### 5.10 Agent Status (companion) — `2×1, 2×2, 4×2`
- **2×1:** header: success dot + "3 agents running" 13/600 | "$4.12 today" mono 11 muted. Body: left "repo · agent" 12/500 ellipsis over status 11 (warning when "Waiting approval: …"); right **Approve** 36 px filled `--k-success` + **Deny** 36 px ghost. Card border becomes `--k-warning` while an approval is pending.
- **2×2:** list of agents (dot per state: idle = border, working = accent, waiting = warning) + approval queue.
- **4×2:** 2×2 + today's token/cost bar.
- **Disconnected:** muted icon + "Kotilo desktop not running — open it" with **Open** ghost button. Never an error colour.

---

## 6. Edit mode

Enter: long-press any card 500 ms, or `E`, or top-bar Edit. Exit: **Done** / `Esc`.

- Grid area gets a dot pattern: `radial-gradient(circle, var(--k-border) 1.5px, transparent 1.6px)` on a 24 px cell, offset 12 px. Surfaces go to 100 % opacity.
- Every card shows a **gear** 36 px (top-right, `--k-bg-alt`, 1 px border) → per-instance settings sheet (schema from manifest + *Appearance* section: opacity, blur).
- **Lifted card** (being dragged): `transform: translate(…) scale(1.04) rotate(-1deg)`, 2 px `--k-accent` border, shadow `0 24px 48px rgba(0,0,0,.6)` (only shadow in the app), `z-index` above siblings. Above it a pill (accent fill, mono 11/600) shows `2×2 → 2×1 · 4×2` (current → other supported sizes).
- **Corner handle:** 28 px circle, accent fill, 3 px `--k-bg` ring, at bottom-right overlapping −14 px. Drag → cycles through `manifest.sizes` only; the ghost target redraws per step.
- **Drop target ghost:** 2 px dashed accent border, 8 % accent tint, radius `--k-radius`.
- **Rejected drop:** card shakes (`translateX ±6px`, 3 cycles, 300 ms) and returns.
- **Trash zone:** bottom-center tab 44 px tall, radius 14 14 0 0, danger tint 16 %, 1 px danger border, danger text 12/600 + trash icon; expands to full width band when a card hovers it.
- **Widget drawer:** slides from right, 340 px, `--k-bg-alt`, 1 px left border, shadow `-16px 0 40px rgba(0,0,0,.5)`. Header "Add widget" + close 36 px. Rows: 40 px icon tile (`--k-bg`, icon in a per-widget token colour) · name 13/600 (companion widgets get "· companion" 10 muted) · size chips 28 px mono 11 (1 px border; selected = accent border + 16 % accent tint). Tapping a chip places that size in the first free slot and highlights it.

Respect `prefers-reduced-motion`: no lift rotation, no shake, instant drawer.

---

## 7. Settings window (960×680, normal OS window)

- Title bar 44 px on `--k-bg-alt`: mark + "Kotilo Deck — Settings" + version mono 11; minimize/close 36×32 ghost.
- **Sidebar 220 px** on `--k-bg-alt`, right border: nav links 40 px, radius 10, 13 px; active = `--k-surface` fill + 600; "Companion" shows a status dot; "About · MIT" pinned to bottom. Sections: General · Display & monitor · Appearance · Themes · Workspaces · Weather & calendar · Companion · Burn-in & night · About.
- **Content** padding 24 28: H1 20/600 + one-line description 13 muted. Section eyebrow 12/600 uppercase muted.
- **Appearance** page (reference `reference/settings-appearance.html`):
  - Background layer: 3 option cards 96 px tall (Solid / Wallpaper / Transparent) with a 36 px preview strip; selected = 2 px accent border.
  - Slider rows: grid `200px 1fr 64px`, on `--k-surface`, radius 12 — Surface opacity (0–100, default from theme), Backdrop blur (0–40 px, "Off" at 0, helper text in warning colour about GPU), Window opacity (20–100, **disabled at 55 % opacity with helper "Overlay mode only — disabled in fullscreen"**).
  - Dim: two toggle cards (Night schedule with times + target %, Idle dim with minutes + "show clock face"). Switch: 44×26, accent when on with `--k-on-accent` knob, `--k-bg-alt` + border when off.
  - Footer: settings path mono 11 muted left; **Reset to theme** ghost + **Apply** primary right.
- Other pages reuse the same row/card primitives. Themes page = grid of 160 px theme swatch cards (bg + surface + accent stripes, name, light/dark badge, "pair" link).

---

## 8. States & motion

| State | Treatment |
|---|---|
| Focus (keyboard) | 2 px `--k-accent` outline, 2 px offset, on every interactive element |
| Disabled | `opacity: .55`, `cursor: default` |
| Loading | skeleton bars in `--k-bg-alt`, no spinners |
| Offline/stale data | warning badge, data stays visible |
| Capability missing | muted icon + "Not available on this OS" centered |
| Opacity change | 200 ms ease |
| Workspace switch | 200 ms horizontal slide of the grid |
| Pomodoro tick | 1 s linear ring update |
| Night/idle dim | `--k-dim` animates over 2 s |

Icons: inline stroke SVG (Lucide-style, 2 px stroke, round caps), sized 16/18/20; never emoji.

---

## 9. Review checklist (for `reviewer`)

- [ ] No hex colours under `src/widgets/**`; cards use `--k-surface-a`.
- [ ] Every size in `manifest.sizes` has a layout matching §5 and renders at 1024×600.
- [ ] All tap targets ≥ 44 px (36 px allowed only for secondary in-card buttons, as specified).
- [ ] Numbers in mono, text in ui font; title 13/600 + meta 11 muted header on every card.
- [ ] Contrast ≥ 4.5:1 for text (3:1 for ≥ 24 px) in Mocha **and** Latte.
- [ ] Keyboard: Tab reaches every control; visible focus ring; real `<button>`/`<input>`/`<label>`.
- [ ] Reduced motion respected.
- [ ] Edit-mode affordances (gear, handle, trash, ghost) only appear in edit mode.
- [ ] Companion widgets render the disconnected state without error colours.

---

## 10. Files

```
docs/design/
├── DESIGN.md                     ← this file
├── themes/
│   ├── catppuccin-mocha.json     ← spec §5.1 schema, exact official palette
│   └── catppuccin-latte.json
└── reference/
    ├── deck-focus-mocha.html     ← 1280×720, Focus workspace, all widgets
    ├── deck-edit-mode.html       ← edit mode + widget drawer
    ├── deck-ops-latte.html       ← Ops workspace, light theme, 4×2 variants
    └── settings-appearance.html  ← 960×680 settings window
```
