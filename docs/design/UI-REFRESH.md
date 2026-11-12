# Kotilo Deck — UI refresh handoff

Task brief for Claude Code. Implements the "Ops redesign" and "glass over wallpaper" mockups.

Reference files, next to the existing ones:

```
docs/design/reference/deck-ops-refresh.html   ← 1920×1080, Ops workspace, flat look, Catppuccin Mocha
docs/design/reference/deck-ops-glass.html     ← 1920×1080, Ops workspace, glass look over a wallpaper
docs/design/reference/deck-ops-refresh.png    ← render of the first file
docs/design/reference/deck-ops-glass.png      ← render of the second file
docs/design/reference/wallpaper-example.svg   ← "Ember" gradient wallpaper made for this repo
```

The glass mockup was first drawn over a photo of unknown licence. That photo is **not** in the repo: the
committed HTML uses `wallpaper-example.svg` and the PNG was re-rendered over it. Anything that ships
(screenshots, a bundled demo wallpaper) uses this gradient, or an image we own or one with a clear licence.

## 0. How to work

- Read `CLAUDE.md`, `SPEC.md` and `docs/design/DESIGN.md` first. All existing rules still apply:
  colours only via `var(--k-*)`, no hex under `src/widgets/**` (`pnpm lint:hex`), 44 px touch targets,
  mono for numbers, `prefers-reduced-motion`, conventional commits, **one widget per PR**,
  `CHANGELOG.md` under `[Unreleased]`, defaults logged in `DECISIONS.md`.
- The reference HTML files are pixel references, not code to copy. They use literal Mocha hex values;
  map them to tokens with the table in §2. They are static: sample data, no logic.
- Work in the order of §9. Each step must leave `pnpm lint`, `pnpm test` and the Rust checks green.
- Every widget change must still render at **1024×600** and in **portrait (4×8)**. The refresh targets
  large displays; the small layouts must not get worse.
- After each widget PR run `pnpm screenshots` and compare against the reference at 1920×1080.

## 1. Why

At 1920×1080 a 2×1 card is about 456×236 px, but hero numbers are fixed at 32/48 px, so most of each card
is empty. Only the 4×2 clock scales with its container (`src/widgets/clock/Widget.svelte`, `.big`).
Every card also has the same visual weight, and the wallpaper/blur feature never appears in the
screenshots. The refresh does three things:

1. Type scales with the card.
2. Empty card area is filled with data (sparklines, day timeline, week strip, forecast).
3. A first-class **glass look** for wallpaper and transparent backgrounds.

## 2. Mockup colour → token map

| Mockup value | Token |
|---|---|
| `#89b4fa` (flat) / `#fab387` (glass accent) | `--k-accent` |
| `#cba6f7` | `--k-accent-2` |
| `#94e2d5` | `--k-info` |
| `#a6e3a1` | `--k-success` |
| `#f9e2af` | `--k-warning` |
| `#f38ba8` | `--k-danger` |
| `#cdd6f4`, `#f5f5fa`, `#ffffff` text | `--k-text` |
| `#a6adc8`, `#dcdde8`, `#e2e3ec` | `--k-text-muted` (see §4 for the glass lift) |
| `#181825` on accent fills | `--k-on-accent` |
| `rgba(17,17,27,.55)`, `rgba(0,0,0,.24)` inner tiles | new `--k-inset` (§3) |
| `rgba(49,50,68,.60)`, `rgba(24,24,37,.52)` cards | `--k-surface-a` |

The peach accent in the glass mockup is not a new theme. It only shows that a warm wallpaper wants a warm
accent; keep the theme's own `--k-accent`. (Picking the accent from the wallpaper is backlog, §10.)

## 3. DESIGN.md amendments (do this first, in a docs-only PR)

Update `docs/design/DESIGN.md` so the reviewer agent checks against the new contract:

- **§1.4 Quiet surfaces** — keep "no glow, no drop shadows except the lifted card". Allow three exceptions:
  (a) sparkline area fills at ≤ 22 % of the line colour, (b) the media card tint from artwork (§6.3),
  (c) the glass look's 1 px inner top highlight (§4).
- **§2 tokens** — add derived variables, computed in `src/lib/theme/loader.ts` (`themeToCssVars`) or
  `app.css`, never per widget:
  - `--k-inset`: background of tiles/chips/tracks that sit *inside* a card.
    Flat look: `var(--k-bg)` for stat tiles and `var(--k-bg-alt)` for chips (today's behaviour).
    Glass look: `color-mix(in srgb, var(--k-bg-alt) 45%, transparent)`.
    Widgets must use `--k-inset` instead of opaque `--k-bg` / `--k-bg-alt` for inner tiles, otherwise
    opaque boxes punch holes in the glass.
  - `--k-card-border`: `var(--k-border)` flat; `color-mix(in srgb, var(--k-text) 18%, transparent)` glass.
  - `--k-scrim`: wallpaper overlay colour, glass only (§4).
- **§3 typography** — replace the fixed hero sizes with the scale in §5 of this file.
- **§5** — update the per-size specs for System 4×2, Calendar 4×2, Media 4×2, Clock 2×1, Date 2×1,
  Weather 2×1, Volume 2×1 with the content described in §6 below.
- **§10 files** — list the two new reference files.

## 4. Glass look (shell + theme, no widget logic)

What exists already: `DeckAppearance.background` (`solid | wallpaper | transparent`), `surfaceOpacity`,
`blur` (0–40 px), per-workspace and per-instance overrides (`src/lib/settings/look.ts`,
`src/routes/+page.svelte`, `WidgetFrame.svelte`). Glass is a preset on top of these, not a new renderer.

Add `style: 'flat' | 'glass'` to `DeckAppearance` and to the workspace-level `Appearance`
(workspace overrides global; default `'flat'`). This touches `src/lib/ipc/index.ts`,
`src-tauri/src/settings.rs` (a serde default of `flat` is enough: `migrate_settings` in
`src-tauri/src/migrations/mod.rs` only stamps the schema version, so add a test that an older settings file
without `style` loads as `flat` instead of a new migration step), `src/lib/ipc/demo.ts`,
and the Appearance settings page (`src/lib/settings/ui/Appearance.svelte`,
`WorkspaceAppearance.svelte`): a two-option segmented control "Flat / Glass". Selecting Glass while
background is `solid` shows the helper text "Glass needs a wallpaper or transparent background".

Expose it as `html[data-look='glass']` and style from `app.css`:

| Part | Glass treatment |
|---|---|
| Wallpaper scrim | a layer above the wallpaper, below content: `color-mix(in srgb, var(--k-bg) 34%, transparent)`. Needed because bright wallpapers kill text contrast. Strength as a setting 0–60 %, default 34 |
| Card (`.k-card`) | `background: var(--k-surface-a)`; defaults when the user has not set them: surface opacity **0.52**, blur **32 px** |
| Card backdrop | `blur(Npx) saturate(150%)` — extend `backdropValue()` in `look.ts` (and its test) to append the saturate when the look is glass |
| Card border | `1px solid var(--k-card-border)` plus `box-shadow: inset 0 1px 0 color-mix(in srgb, var(--k-text) 22%, transparent)` |
| Inner tiles, chips, tracks | `--k-inset`, 1 px border `color-mix(in srgb, var(--k-text) 8%, transparent)` |
| Muted text | lift for contrast: `--k-text-muted: color-mix(in srgb, var(--k-text-muted) 45%, var(--k-text))`, set once under `[data-look='glass']` |
| Top bar | brand, workspace switcher, companion status, theme chip and edit button each become a glass pill (same background, border and backdrop as cards) instead of sitting on the bare wallpaper |
| Attention border | unchanged (`--k-warning`) |
| Edit mode | unchanged: surfaces go to 100 % opacity as today |

Rules:

- Flat look must be pixel-identical to today apart from the widget changes in §6.
- Blur is expensive. Keep `--k-backdrop: none` when blur is 0, and keep the existing GPU helper text.
- Contrast: body text ≥ 4.5:1 and large numerals ≥ 3:1 against the *worst case* behind the card.
  Verify in Mocha and Latte with a bright and a dark wallpaper. If Latte fails at 0.52 opacity, raise
  the glass default opacity for light themes rather than weakening the rule.
- Video wallpapers keep working; the scrim sits above them too.

Reference: `deck-ops-glass.html`.

## 5. Container-scaled type

Every card root already has `container-type: size`. Replace fixed hero sizes with the pattern the 4×2
clock uses: `font-size: max(<floor>, min(<cap>, <N>cqw, calc(100cqh - <reserved>px)))`.
Floors keep the 1024×600 layout as it is today; caps are the mockup sizes at 1920×1080.

| Element | Today | Floor (1024×600) | Cap (1920×1080) |
|---|---|---|---|
| Clock 2×1 time | 32 | 32 | 112, with seconds at 0.3 em muted |
| Date 2×1 day number | 32 | 32 | 112 |
| Weather 2×1 temperature | 32 | 32 | 84 |
| Volume 2×1 percent | 13 | 13 | 56, `%` at 0.36 em muted |
| System 4×2 tile value | 32 | 32 | 68, `%` at 0.35 em muted |
| Calendar next-event title | 15 | 15 | 24 |
| Calendar countdown chip | 14 | 14 | 22 |
| Media 4×2 title | 20 | 20 | 44, weight 600, `letter-spacing: -0.02em` |
| Card title / meta | 13 / 11 | 13 / 11 | 14 / 12 |
| Body rows (calendar, todo) | 13 | 13 | 15 |

Use `letter-spacing: -0.04em` and `line-height: 1` on mono hero numerals, as the clock does.
Tune the `cqw` / reserved values per widget so nothing clips at any supported size; check with
`pnpm screenshots` at both resolutions and in portrait.

## 6. Widgets (one PR each)

### 6.1 System 4×2 — `src/widgets/system/`
- All three tiles get a sparkline, not only CPU. Keep `history` for memory % and GPU % as well
  (`logic.ts` `pushHistory`, same 60-sample cap). Line colours: CPU `--k-accent`, memory `--k-info`,
  GPU `--k-accent-2`; warn state keeps `--k-warning` as today.
- Sparkline: 2.5 px line plus an area fill underneath at 16 % of the line colour (22 % in glass).
  It bleeds to the tile's left, right and bottom edges (tile `overflow: hidden`, no bottom padding).
- CPU tile sub-label: `peak N% · 60 s` (max of the history window). Memory and GPU keep their labels.
- Per-core strip: 64 px tall at 1080p, 8 px gap, 6 px radius, with a small header row
  `CORES` (tiny label) and `busiest: core N · X%` (mono 11, right).
  Bar colour by load: below 25 % `--k-info`, 25 % up to the warning threshold `--k-accent`,
  at or above the threshold `--k-warning`. Bar height stays proportional, so colour is never the only cue.
- Tiles use `--k-inset`, radius 14.
- 2×2: no layout change; only apply the area fill to its sparkline.

### 6.2 Calendar 4×2 — `src/widgets/calendar/`
- Next-event card: padding 16 18, feed bar 5 px, title and countdown sizes from §5. In glass the
  countdown chip is filled `--k-accent` with `--k-on-accent` text; in flat it stays on `--k-inset`.
- Rows: 38 px, feed bar in the feed's colour (`feedColor()` already returns a token), time column
  mono, right column shows the **feed name** (today the mock data repeats "Work").
- **Day timeline** in the remaining height (new):
  - Range from settings `timelineFrom` / `timelineTo`, defaults `09:00` / `18:00`, added to
    `manifest.json` and `settings.ts`. Grid of 15-minute columns.
  - Top lane (20 px): all-day events as a tinted bar with the title.
  - Main lane: one block per timed event, positioned by start/end, feed colour at ~30 % with a 4 px
    solid left edge, title 12/500 ellipsised. Blocks under 30 minutes show no label.
    Overlapping events stack in up to two sub-lanes; more than two collapse into a `+N` block.
  - "Now" marker: 2 px vertical line in `--k-text` with a small mono time label; hidden when now is
    outside the range. Updates on the shared minute tick (`#lib/time`), no new timers.
  - Hour labels under the lane, mono 11 muted.
  - Hidden by container query when the card is too short (keep today's 1024×600 layout: next + 3 rows).
- Put the positioning maths in `logic.ts` as pure functions with tests (event → column span, lanes,
  now position, clipping events that start before or end after the range).
- 4×4: add the timeline above the row list; otherwise unchanged.

### 6.3 Media 4×2 — `src/widgets/media/`
- Artwork is a square that fills the card height (card padding 26), radius 18. Placeholder as today.
- Text column: source tiny label, title (§5), "Artist · Album" at 19 px, spacer, progress row
  (8 px track on `--k-inset`, mono 14 times), controls centred underneath.
- Controls: play/pause is the only filled button — 84 px circle, `--k-accent-2`, `--k-on-accent` icon.
  Previous/next: 60 px, no border, no background. Shuffle/repeat: 52 px, no border, muted; when
  active, a 22 % `--k-accent-2` tint circle and `--k-accent-2` icon. All have `aria-label`s and
  `aria-pressed` where they toggle.
- **Artwork tint (flat look only):** card background
  `color-mix(in srgb, <art colour> 30%, var(--k-surface-a))` and border
  `color-mix(in srgb, <art colour> 30%, var(--k-border))`, transition 200 ms.
  `<art colour>` is the average/dominant colour of the artwork, computed once per track (downscale to a
  few pixels on a canvas in the frontend, or in Rust next to where artwork is produced — choose the one
  that avoids tainted-canvas problems with how artwork is delivered, and log it in `DECISIONS.md`).
  Pass it as an inline custom property (`--art`), which keeps `lint:hex` clean. No artwork → `--k-accent-2`.
  Clamp lightness so text contrast holds. In the glass look the card stays neutral glass and the
  progress fill and play button use `--k-text` instead of `--k-accent-2`.
- 4×1: only the control restyle (borderless prev/next, round filled play).

### 6.4 Clock 2×1 — `src/widgets/clock/`
- Time on one baseline with muted seconds (when seconds are enabled), sizes from §5.
- Bottom row: left "Thursday, November 5" 16/600, right "Istanbul UTC+3" 13 muted.
- Below the container-query breakpoint keep today's single-row layout.

### 6.5 Date 2×1 — `src/widgets/date/`
- Top: day number (§5) with weekday 24/600 and "November · Week 45 · Day 309" 15 muted beside it
  ("Day N" only when `dayOfYear` is on).
- Bottom: week strip, 7 equal cells `M 2 … S 8`, mono 13; today is a filled `--k-accent` pill with
  `--k-on-accent` text; past days muted. Week start follows the locale. Hidden when the card is short.

### 6.6 Weather 2×1 — `src/widgets/weather/`
- Header: city + "N min ago" meta (stale badge unchanged).
- Middle: temperature (§5), condition 16/500 over mono 13 "H 19° · L 12°", icon 84 px.
- **Filled icon variant** for sizes ≥ 56 px in `WeatherIcon.svelte`: sun disc filled `--k-warning`
  with rays, cloud filled `--k-text` with a thin `--k-border` outline. Stroke icons stay for small sizes.
- Bottom: the 3-day chips from the 2×2 layout in a compact one-line form
  (`Fri  16° 11°`), on `--k-inset`. Hidden when the card is short.

### 6.7 Volume 2×1 — `src/widgets/volume/`
- Header: speaker icon 20 + device name 14/600, percent hero on the right (§5).
- Custom-styled native range (keep `<input type="range">` for keyboard and screen readers):
  track 14 px, fully rounded, filled part `--k-accent`, rest `--k-inset`; thumb 28 px circle in
  `--k-text` with a 4 px `--k-bg` ring. Style both `::-webkit-slider-*` and `::-moz-range-*`.
  Fill position via an inline `--p` custom property, as the media progress already does.
- Buttons "Output device" and "Mixer" become 44 px tall.
- Use the same range styling for the shared `.k-range` in `app.css` so the mixer sheet matches.

### 6.8 Top bar — `src/lib/ui/TopBar.svelte`, `WorkspaceSwitcher.svelte`, `ThemePicker.svelte`
- Theme chip (`ThemePicker.svelte`): add four 10×18 px swatches (`--k-accent`, `--k-accent-2`, `--k-info`, `--k-warning`)
  before the theme name; chip height 44 px.
- Workspace pills (`WorkspaceSwitcher.svelte`) 44 px tall (today 36) to meet the touch-target rule.

## 7. Presets and screenshots

- `src/lib/workspace/presets.ts`: the **Focus** preset leaves six cells empty at 8×4: the bottom-right
  4×2 block holds only the 2×1 Date. Fill the grid (for example grow the Date card or add the Calendar 4×2 and rearrange).
- `scripts/screenshots.mjs`: add a glass capture per preset (`deck-<preset>-glass.png`) using
  `wallpaper-example.svg` (or another image with a clear licence) as the demo wallpaper. Make the README hero image and `deck.gif` use the glass Ops capture.
- Regenerate every screenshot; update the README table.

## 8. Acceptance checklist

- [ ] `pnpm lint`, `pnpm test`, `cargo fmt --check && cargo clippy -- -D warnings && cargo test` pass.
- [ ] No hex under `src/widgets/**`; inner tiles use `--k-inset`; cards use `--k-surface-a`.
- [ ] 1920×1080 captures match the two reference files in layout, sizes and hierarchy.
- [ ] 1024×600 and portrait captures are no worse than today's; no clipped text at any `manifest.sizes`.
- [ ] Flat look without a wallpaper is unchanged outside the widget changes listed here.
- [ ] Glass: contrast ≥ 4.5:1 (3:1 for ≥ 24 px) in Mocha and Latte over a bright and a dark wallpaper.
- [ ] Settings round-trip: `style` persists, migrates from older settings files, works per workspace.
- [ ] New pure logic (timeline layout, history for memory/GPU, core colour buckets, artwork colour
      clamp) has unit tests.
- [ ] Reduced motion respected for every new transition.
- [ ] `DESIGN.md`, `CHANGELOG.md` and `DECISIONS.md` updated; English and Turkish strings added for
      every new label (`src/i18n/en.json`, `tr.json`).

## 9. Suggested PR order

1. `docs`: DESIGN.md amendments + reference files (§3).
2. `feat(theme)`: `--k-inset`, `--k-card-border`, glass look, settings + migration (§4).
3. `feat(ui)`: top bar (§6.8) and shared `.k-range` styling.
4. `feat(system)` (§6.1) — also the first user of §5; settle the type-scale pattern here.
5. `feat(calendar)` (§6.2).
6. `feat(media)` (§6.3).
7. `feat(clock)`, `feat(date)`, `feat(weather)`, `feat(volume)` (§6.4–6.7), one PR each.
8. `feat(workspace)`: Focus preset; `docs`: screenshots and README (§7).

## 10. Not in this task (backlog)

- Motion: digit roll on clock/percent changes, staggered card entry on workspace switch, slow
  "breathing" attention border for pending approvals, sparkline slide on new samples.
- Auto-hiding top bar.
- Accent colour picked from the wallpaper.
- Agents 4×2: recent-activity feed or per-agent cost bars in the empty area.
- Custom todo checkbox.

## 11. What is made up in the mockups

Sample data only, not requirements: the "Design" feed name, `peak 61%`, the seconds value, the week
strip dates, and the gradient blobs standing in for a wallpaper in the flat reference.
