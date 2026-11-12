# Kotilo Deck — UI Design Handoff

> Companion to `kotilo-deck-spec.md`. This file is the visual/interaction contract for the `ui-widgets` agent.
> Reference mockups live in `reference/` (plain HTML, open in a browser). Theme JSONs in `themes/`.
> When this file and the product spec disagree on behaviour, the spec wins; on look, this file wins.

## 0. How to use with Claude Code

- Put this folder at `docs/design/` in the repo. Add to `CLAUDE.md`: *"Read `docs/design/DESIGN.md` before touching anything under `src/widgets/`, `src/lib/grid/` or `src/lib/theme/`."*
- `ui-widgets` agent: every widget PR must match the matching section in `## 5` for **each supported size**.
- `reviewer` agent: check PRs against `## 9` (checklist). Reject any hardcoded hex under `src/widgets/**`.
- `reference/*.html` are pixel references, not code to copy. They are 1280×720 (deck) and 960×680 (settings) static comps; the `deck-ops-*` refresh comps are 1920×1080.

---

## 1. Design principles

1. **Ambient first.** Readable at arm's length across a desk: big numerals, few words, high contrast. Nothing blinks.
2. **Theme is data.** Every colour is a `--k-*` token. A new theme JSON must make the whole deck look right without touching a widget.
3. **Touch and glance.** Hit targets ≥ 44×44 px. Primary action of a widget is one tap (pause pomodoro, approve agent, play/pause).
4. **Quiet surfaces.** Flat cards with a 1 px border; no gradients, no glow, no drop shadows except the *lifted* card in edit mode. Three exceptions: (a) sparkline area fills at ≤ 22 % of the line colour; (b) the media card tint from artwork (§5.7, flat look only); (c) the glass look's 1 px inner top highlight (§4.5).
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
| *(app.css)* | `--k-spark-fill` | sparkline area fill strength: 16 %, 22 % in glass. Use `color-mix(in srgb, currentColor var(--k-spark-fill), transparent)` |
| *(runtime)* | `--k-dim` | 1.0 → 0.3 multiplier for night/idle dim (applied as `opacity` on the content layer) |
| *(derived)* | `--k-inset` | chips, tracks, bars and fields **inside** a card. Flat: `bg-alt`. Glass: `color-mix(in srgb, bg-alt 45%, transparent)` |
| *(derived)* | `--k-inset-deep` | stat tiles and dials inside a card. Flat: `bg`. Glass: same as `--k-inset` |
| *(derived)* | `--k-inset-ring` | optional 1 px ring on insets, drawn as `box-shadow: inset 0 0 0 1px var(--k-inset-ring)` so it never changes layout. Flat: `transparent`. Glass: `color-mix(in srgb, text 8%, transparent)` |
| *(derived)* | `--k-card-border` | card border. Flat: `var(--k-border)`. Glass: `color-mix(in srgb, var(--k-text) 18%, transparent)` |
| *(derived)* | `--k-scrim` | wallpaper overlay. Flat: `transparent`. Glass: `color-mix(in srgb, bg <scrim>%, transparent)` (§4.5) |

Rules:
- Widgets use **only** `var(--k-…)`. CI lint: regex `#[0-9a-fA-F]{3,8}` under `src/widgets/**` fails the build (allow-list: `transparent`, `currentColor`).
- Text on an **accent-filled** button uses the theme's `bg-alt` (dark themes) or `#fff` (light themes). Expose this as `--k-on-accent`, computed by the theme loader: if `variant == "dark"` → `bg-alt`, else `#ffffff`.
- Tinted states (warning card, selected calendar row) are `color-mix(in srgb, var(--k-warning) 18%, var(--k-surface))` — never a second hex.
- Derived variables (`--k-surface-a`, `--k-on-accent`, `--k-inset*`, `--k-card-border`, `--k-scrim`, glass `--k-text-muted`) are computed once in `src/lib/theme/loader.ts` (`themeToCssVars`) or `app.css`, never per widget.
- Inner tiles, chips and tracks use `--k-inset` / `--k-inset-deep`, not opaque `--k-bg` / `--k-bg-alt`: opaque boxes punch holes in the glass look. Dialogs and sheets stay opaque.

Bundled reference values are in `themes/catppuccin-mocha.json` and `themes/catppuccin-latte.json`. The `kotilo-theme` skill holds the rest.

---

## 3. Typography & spacing

Fonts: **Inter** (ui), **JetBrains Mono** (mono). Bundle them (no Google Fonts at runtime — the deck may be offline).

| Role | Font | Size / weight | Notes |
|---|---|---|---|
| Clock 4×2 | mono | 128 / 500, `letter-spacing: -0.04em`, `line-height: 1` | seconds: 40 / 400 muted |
| Hero number (weather, CPU, timer) | mono | 48 / 500 (2×2) · container-scaled in 2×1 and 4×2 tiles (table below) · 32 / 500 in settings | |
| Timer in ring | mono | 30 / 600 | |
| Widget title | ui | 13 / 600 → 14 at 1920×1080 | top-left of every card |
| Widget meta (right of title) | ui | 11 / 400 muted → 12 at 1920×1080 | "6 min ago", "2 left", "60 s" |
| Body row | ui | 13 / 400 → 15 at 1920×1080 | todo item, calendar title |
| Tiny label | ui | 11 / 400 muted, uppercase, `letter-spacing: .08em` | "BERLIN", "CPU" |
| Button label | ui | 12–13 / 600 | |
| Size chip | mono | 11 / 400 | "2×2" |

**Container-scaled type.** Every card root has `container-type: size`. Hero text follows the 4×2 clock pattern
`font-size: max(<floor>, min(<cap>, <N>cqw, calc(100cqh - <reserved>px)))`: the floor keeps the 1024×600 layout,
the cap is the size at 1920×1080. Tune `cqw` / reserved per widget so nothing clips at any supported size.
Mono hero numerals use `letter-spacing: -0.04em` and `line-height: 1`. Card title, meta and body rows use the shared
`--k-fs-title` / `--k-fs-meta` / `--k-fs-body` variables from `app.css` (viewport-clamped 13→14, 11→12, 13→15).
Text in a flex column that may get short is `flex-shrink: 0`; a sparkline or spacer gives up the height instead.
Reference implementation: System 4×2 (`src/widgets/system/Widget.svelte`, `.val-l`).

| Element | Floor (1024×600) | Cap (1920×1080) |
|---|---|---|
| Clock 2×1 time | 32 | 112, seconds at 0.3 em muted |
| Date 2×1 day number | 32 | 112 |
| Weather 2×1 temperature | 32 | 84 |
| Volume 2×1 percent | 13 | 56, `%` at 0.36 em muted |
| System 4×2 tile value | 32 | 68, `%` at 0.35 em muted |
| Calendar next-event title | 15 | 24 |
| Calendar countdown chip | 14 | 22 |
| Media 4×2 title | 20 | 44, weight 600, `letter-spacing: -0.02em` |
| Card title / meta | 13 / 11 | 14 / 12 |
| Body rows (calendar, todo) | 13 | 15 |

Spacing scale: 4, 6, 8, 10, 12, 14, 16, 20, 24. Card padding: **18 px 20 px** (2×N cards), **22 px 28 px** (4×2 clock), **14 px 18 px** (1-row cards). Inner chips radius 10 px, buttons 12 px, pills 999 px.

---

## 4. Deck shell

### 4.1 Window
Fullscreen on the chosen monitor. Root is a column: **top bar 56 px** + **grid area** (`padding: 8px 24px 24px`, `gap: 16px`).

### 4.2 Top bar (56 px)
Three zones, `justify-content: space-between`:
- **Left (min 220 px):** Kotilo diamond mark (22 px, accent stroke, accent-2 cross) + "Kotilo Deck" 14/600.
- **Center:** workspace switcher — pill group on `--k-bg-alt`, 1 px border, radius 999, padding 4. Each pill 44 px tall (touch-target rule), padding 0 18, 13 px. Active: `--k-accent` fill, `--k-on-accent` text, 600. Inactive: transparent, muted, 500. Swipe/hotkey changes active pill with a 200 ms slide.
- **Right (min 220 px):** companion status (8 px dot: success = connected, border = not running; label 12 px muted) · theme chip (44 px tall, four 10×18 px swatches `--k-accent` / `--k-accent-2` / `--k-info` / `--k-warning`, then the name in mono 12, 1 px border, radius 8, e.g. `mocha`) · **Edit** icon button 44×44, 1 px border, radius 12.

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

### 4.5 Look: flat or glass
`style: 'flat' | 'glass'` in the global and the workspace appearance (workspace wins; default `flat`), exposed as
`html[data-look='glass']` and styled from `app.css`. Glass is a preset over the existing background / surface
opacity / blur settings, not a new renderer. It needs a wallpaper or transparent background; with a solid background
the setting shows "Glass needs a wallpaper or transparent background". Reference: `reference/deck-ops-glass.html`.

| Part | Glass treatment |
|---|---|
| Wallpaper scrim | layer above the wallpaper (image or video), below content: `color-mix(in srgb, var(--k-bg) 34%, transparent)`; strength setting 0–60 %, default 34 |
| Card | `--k-surface-a`. Without a user opacity the default is `glassSurfaceOpacity(theme, scrim)`: the lowest value from 0.52 up at which text and muted text reach 4.5:1 over a pure white **and** a pure black wallpaper (Mocha 0.77, Latte 0.71 at scrim 34 %). Picking Glass while blur is off proposes 32 px |
| Card backdrop | `blur(Npx) saturate(150%)`; `--k-backdrop: none` when blur is 0 |
| Card border | `1px solid var(--k-card-border)` + `box-shadow: inset 0 1px 0 color-mix(in srgb, var(--k-text) 22%, transparent)` |
| Inner tiles, chips, tracks | `--k-inset` / `--k-inset-deep`, ring `--k-inset-ring` |
| Muted text | `--k-text-muted` = theme muted mixed 45 % with text (computed by the theme loader) |
| Top bar | brand, workspace switcher, companion status, theme chip and Edit each become a glass pill (card background, border, backdrop) |
| Attention border, edit mode | unchanged |

Flat must stay pixel-identical to the pre-refresh look apart from the §5 widget changes. Contrast rules (§9) hold
against the worst case behind the card; the per-theme glass default above guarantees it for every bundled theme
(unit-tested), and a user-chosen opacity is the user's call.

---

## 5. Widgets — per-size specification

Sizes are `w×h` in cells. "Compact" = 1-row variants.

### 5.1 Clock — `2×1, 2×2, 4×2`
- **4×2:** time 128 mono + seconds 40 muted on one baseline. Bottom row: left = "Tuesday, 3 November" 18/600 over "Week 45 · Day 307 · Istanbul UTC+3" 13 muted; right = up to 3 extra time zones, each tiny-label city + mono 20 time, right-aligned, 20 px apart.
- **2×2:** header "City" + mono 12 time; analog face 150 px (face `--k-bg`, 2 px border, 4 quarter ticks 3 px, hour hand 5 px `--k-text`, minute hand 3 px `--k-accent`, 4 px accent hub); footer "Tue 3 Nov · W45" 12 muted. Digital option: 48 mono centered.
- **2×1:** time container-scaled (§3) on one baseline with seconds 0.3 em muted (when enabled). Bottom row: left "Thursday, November 5" 16/600, right "Istanbul UTC+3" 13 muted. Below the container-query breakpoint: mono time left, date 12 muted right on one row (the 1024×600 layout). Breakpoint: content box ≥ 220×100 px (1280×720 and up); under 320 px wide the zone label is dropped so the date stays whole.

### 5.2 Date — `2×1`
- Top: day number container-scaled (§3); beside it weekday 24/600 over "November · Week 45 · Day 309" 15 muted ("Day N" only when `dayOfYear` is on).
- Bottom: week strip, 7 equal cells `M 2 … S 8`, mono 13; today a filled `--k-accent` pill with `--k-on-accent` text; past days muted. Week start follows the locale. Hidden when the card is short (then day number + weekday + ISO week, as before).

### 5.3 Weather — `2×1, 2×2, 4×2`
- **2×2:** header city + "N min ago"; hero 48 mono `14°` with "Partly cloudy · H 16° L 9°" 13 muted; condition icon 56 px (stroke 1.6, sun in `--k-warning`, cloud in `--k-text-muted`); 3-day row: three chips on `--k-bg-alt`, radius 10, "Wed" 11 muted over mono 13 `16° 9°` (low muted).
- **2×1:** header city + "N min ago" (stale badge unchanged). Middle: temperature container-scaled (§3), condition 16/500 over mono 13 "H 19° · L 12°", icon 84 px. Bottom: the 3-day chips in one-line form (`Fri  16° 11°`) on `--k-inset`, hidden when the card is short. Short card: city + stale badge, condition 12 muted | hero + 36 px icon.
- **Icons ≥ 56 px** use the filled variant: sun disc filled `--k-warning` with rays, cloud filled `--k-text` with a thin `--k-border` outline. Smaller icons keep the stroke style.
- **4×2:** 2×2 content + 6-hour strip (6 columns: hour, icon 20 px, temp).
- **Stale state:** badge "stale" 11/500, warning tint chip; subtitle "last update N min ago".

### 5.4 Calendar — `4×2, 4×4`
- Header "Up next" + "2 feeds · Tue 3 Nov".
- **Next event card:** accent-tinted (`color-mix` 14% accent on surface), 1 px accent border, 4 px feed-colour bar, title 15/600, "10:00 – 10:45 · Teams · Work" 12 muted, countdown chip mono 14/600 on `--k-surface` → "in 12 min" (switches to "now" at 0, danger text if overrunning).
- Following rows: 4 px feed bar · mono 12 time range (92 px col) · title 13 · feed name 11 muted. All-day shows "All day".
- 4×2 shows next + 3 rows; 4×4 next + 8 rows.
- **4×2 at large sizes:** next-event card padding 16 18, feed bar 5 px, title and countdown container-scaled (§3); in glass the countdown chip is filled `--k-accent` with `--k-on-accent` text, in flat it sits on `--k-inset`. Rows 38 px, feed bar in the feed's colour, mono time column, right column = feed name.
- **Day timeline** fills the remaining height (4×2) or sits above the rows (4×4); hidden by container query when the card is short (1024×600 keeps next + 3 rows):
  - Range from settings `timelineFrom` / `timelineTo` (default `09:00` / `18:00`), 15-minute columns.
  - Top lane 20 px: all-day events as a tinted bar with the title.
  - Main lane: one block per timed event by start/end, feed colour ~30 % with a 4 px solid left edge, title 12/500 ellipsised; blocks under 30 min have no label. Overlaps stack in up to two sub-lanes; more collapse into a `+N` block. Events crossing the range edges are clipped.
  - "Now": 2 px `--k-text` line with a small mono time label, hidden outside the range, moved by the shared minute tick.
  - Hour labels under the lane, mono 11 muted (hour number only, the locale's 12/24 h).
  - Layout maths in `src/widgets/calendar/timeline.ts` (tested); 4×2 hides the timeline below a 400 px card height.
  - Range settings use the manifest's `"format": "time"` string field, rendered as `<input type="time">`.

### 5.5 System — `2×2, 4×2`
- **2×2:** header + "60 s"; CPU row: label 12 muted / mono 20 value, sparkline 36 px tall (2 px accent line, 60 samples, no fill); Memory & GPU: label/value row + 6 px bar on `--k-bg-alt` (fill `--k-info` memory, `--k-warning` when in warn state).
- **2×2 sparkline** gets the area fill described under 4×2; layout unchanged.
- **4×2:** header + CPU name; three stat tiles on `--k-inset` radius 14: tiny label, value container-scaled (§3), sub-label (CPU `peak N% · 60 s` = max of the history window; RAM "11.2 of 32 GB"; GPU model), and a sparkline in every tile (60 samples; CPU `--k-accent`, memory `--k-info`, GPU `--k-accent-2`, `--k-warning` in warn state). Sparkline: 2.5 px line + area fill at 16 % of the line colour (22 % in glass), bleeding to the tile's left, right and bottom edges (`overflow: hidden`, no bottom padding). Warn tile: warning tint bg + warning border, text darkened for contrast.
- **Per-core strip** below the tiles: header row tiny label `CORES` + `busiest: core N · X%` mono 11 right; bars 64 px tall at 1080p (40 px floor), 8 px gap, 6 px radius, height proportional to load; colour below 25 % `--k-info`, 25 % to threshold `--k-accent`, at/above threshold `--k-warning`.
- **Warn rule** (from SysPulse): value > threshold for 3 consecutive samples → tile/bar turns warning, header meta shows "N samples over X%". Clears after 3 samples under.
- Tap anywhere → modal sheet with top-5 processes (name, CPU %, RAM).

### 5.6 Volume — `2×1, 2×2`
- **2×1:** speaker icon 20 + device name 14/600 | percent container-scaled (§3), `%` 0.36 em muted. Native `<input type=range>` (kept for keyboard and screen readers) styled as the shared `.k-range`. "Output device ▾" and "Mixer" buttons 44 px tall.
- **2×2:** same + device list (radio rows 40 px) and mute toggle.
- Muted state: icon with slash, percent replaced by "Muted" muted.
- **Shared `.k-range`** (`app.css`, used by every slider including settings and the mixer sheet): 28 px tall, track 14 px fully rounded, filled part `--k-accent`, rest `--k-inset`, thumb 28 px `--k-text` circle with a 4 px `--k-bg` ring. Fill = inline `--p` kept by the `rangeFill` action (`#lib/ui/range.ts`); Firefox uses `::-moz-range-progress`.

### 5.7 Media — `4×1, 4×2`
- **4×1:** artwork 84 px radius 12 (fallback: `--k-accent-2` square with note icon in `--k-bg`); title 15/600 ellipsis + "Artist · Source" 12 muted; progress 4 px with mono 11 times; 3 buttons 44 px (prev borderless · play/pause round filled `--k-accent-2` · next borderless).
- **4×2:** card padding 26; artwork a square filling the card height, radius 18 (placeholder as 4×1). Text column: source tiny label, title container-scaled (§3), "Artist · Album" 19 px, spacer, progress row (8 px track on `--k-inset`, mono 14 times), controls centred under it. Play/pause is the only filled button: 84 px circle `--k-accent-2`, `--k-on-accent` icon. Prev/next 60 px, no border or background. Shuffle/repeat 52 px, no border, muted; active = 22 % `--k-accent-2` tint circle + `--k-accent-2` icon, `aria-pressed`. Every control has an `aria-label`.
- **Artwork tint (flat look only):** card background `color-mix(in srgb, var(--art) 30%, var(--k-surface-a))`, border `color-mix(in srgb, var(--art) 30%, var(--k-border))`, 200 ms transition. `--art` is the artwork's average/dominant colour, computed once per track, lightness-clamped for contrast, set as an inline custom property; no artwork → `--k-accent-2`. In glass the card stays neutral and the progress fill and play button use `--k-text`.
- Tint maths in `src/widgets/media/logic.ts` (`averageColor`, `tintColor`, tested over every bundled theme): saturation capped at 0.75, then pulled toward the surface until text keeps 4.5:1 and muted text 3:1 on the tinted card. Below 700 px card width or 300 px height (1024×600, 1280×720) 4×2 keeps the smaller scale: padding 18, artwork ≤ 30 % of the width, play 56 and other controls 44.
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

- [ ] No hex colours under `src/widgets/**`; cards use `--k-surface-a`; inner tiles, chips and tracks use `--k-inset`.
- [ ] Hero text is container-scaled (§3): floor at 1024×600, cap at 1920×1080, nothing clipped in portrait (4×8).
- [ ] Glass look (§4.5) checked over a bright and a dark wallpaper; flat look unchanged outside the listed widget changes.
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
├── UI-REFRESH.md                 ← task brief for the 1920×1080 refresh and the glass look
├── themes/
│   ├── catppuccin-mocha.json     ← spec §5.1 schema, exact official palette
│   └── catppuccin-latte.json
└── reference/
    ├── deck-focus-mocha.html     ← 1280×720, Focus workspace, all widgets
    ├── deck-edit-mode.html       ← edit mode + widget drawer
    ├── deck-ops-latte.html       ← Ops workspace, light theme, 4×2 variants
    ├── settings-appearance.html  ← 960×680 settings window
    ├── deck-ops-refresh.html/.png ← 1920×1080, Ops workspace, flat look, Mocha
    ├── deck-ops-glass.html/.png  ← 1920×1080, Ops workspace, glass look over a wallpaper
    └── wallpaper-example.svg     ← "Ember" gradient wallpaper made for this repo (safe to ship)
```
