---
name: kotilo-theme
description: Kotilo theme token schema, bundled theme list with exact hex values, and rules for mapping tokens to --k-* CSS variables. Use when adding a theme, touching src/lib/theme/, or styling any widget. Shared with Kotilo Shell and launcher.
---

# Kotilo theme tokens

## Schema (`src/lib/theme/themes/<id>.json`, SPEC §5.1)

```json
{
  "id": "catppuccin-mocha", "name": "Catppuccin Mocha", "variant": "dark", "pair": "catppuccin-latte",
  "colors": { "bg", "bg-alt", "surface", "text", "text-muted", "border",
              "accent", "accent-2", "success", "warning", "danger", "info" },
  "radius": "16px",
  "opacity": { "surface": 0.85, "bg": 1.0 },
  "font": { "ui": "Inter, system-ui, sans-serif", "mono": "JetBrains Mono, ui-monospace, monospace" }
}
```

All 12 colours are required hex (`#rgb` or `#rrggbb`). `parseTheme` in `src/lib/theme/loader.ts` validates and fills defaults for `radius`, `opacity`, `font`.

## CSS variable mapping (`themeToCssVars`)

| Source | Variable |
|---|---|
| `colors.<token>` | `--k-<token>` |
| `surface` + `opacity.surface` (or override) | `--k-surface-a` = `rgba(surface, opacity)`; **widget cards use this** |
| `variant` | `--k-on-accent` = `bg-alt` for dark, `#ffffff` for light |
| `radius` | `--k-radius` |
| `font.ui` / `font.mono` | `--k-font-ui` / `--k-font-mono` |
| runtime | `--k-dim` (1.0 to 0.3, night/idle dim) |

Tinted states: `color-mix(in srgb, var(--k-warning) 18%, var(--k-surface))`, never a second hex.

## Bundled themes

| id | variant | bg | bg-alt | surface | text | text-muted | border | accent | accent-2 | success | warning | danger | info |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| catppuccin-mocha | dark | #1e1e2e | #181825 | #313244 | #cdd6f4 | #a6adc8 | #45475a | #89b4fa | #cba6f7 | #a6e3a1 | #f9e2af | #f38ba8 | #94e2d5 |
| catppuccin-latte | light | #eff1f5 | #e6e9ef | #ffffff | #4c4f69 | #5c5f77 | #ccd0da | #1e66f5 | #8839ef | #40a02b | #df8e1d | #d20f39 | #179299 |
| dracula | dark | #282a36 | #21222c | #44475a | #f8f8f2 | #aeb6d0 * | #6272a4 | #bd93f9 | #ff79c6 | #50fa7b | #ffb86c | #ff5555 | #8be9fd |
| nord | dark | #2e3440 | #3b4252 | #434c5e | #eceff4 | #d8dee9 | #4c566a | #88c0d0 | #b48ead | #a3be8c | #ebcb8b | #bf616a | #8fbcbb |
| tokyo-night | dark | #1a1b26 | #16161e | #292e42 | #c0caf5 | #a9b1d6 | #3b4261 | #7aa2f7 | #bb9af7 | #9ece6a | #e0af68 | #f7768e | #7dcfff |
| catppuccin-frappe | dark | #303446 | #292c3c | #414559 | #c6d0f5 | #acb3d2 | #51576d | #8caaee | #ca9ee6 | #a6d189 | #e5c890 | #e78284 | #81c8be |
| catppuccin-macchiato | dark | #24273a | #1e2030 | #363a4f | #cad3f5 | #a5adcb | #494d64 | #8aadf4 | #c6a0f6 | #a6da95 | #eed49f | #ed8796 | #8bd5ca |
| gruvbox-dark | dark | #282828 | #1d2021 | #3c3836 | #ebdbb2 | #bdae93 | #504945 | #83a598 | #d3869b | #b8bb26 | #fabd2f | #fb4934 | #8ec07c |
| gruvbox-light | light | #fbf1c7 | #ebdbb2 | #f9f5d7 | #3c3836 | #665c54 | #d5c4a1 | #076678 | #8f3f71 | #79740e | #b57614 | #9d0006 | #427b58 |
| tokyo-night-storm | dark | #24283b | #1f2335 | #292e42 | #c0caf5 | #a9b1d6 | #3b4261 | #7aa2f7 | #bb9af7 | #9ece6a | #e0af68 | #f7768e | #7dcfff |
| tokyo-night-day | light | #e1e2e7 | #d0d5e3 | #e9e9ed | #3760bf | #4f609e | #a8aecb | #1c72e7 | #9854f1 | #587539 | #8c6c3e | #f52a65 | #007197 |
| one-dark | dark | #282c34 | #21252b | #2c313a | #abb2bf | #9399a5 | #3e4451 | #61afef | #c678dd | #98c379 | #e5c07b | #e06c75 | #56b6c2 |
| solarized-dark | dark | #002b36 | #002b36 | #073642 | #93a1a1 | #8b9b9d | #586e75 | #3295da | #6c71c4 | #859900 | #b58900 | #dc322f | #2aa198 |
| solarized-light | light | #fdf6e3 | #eee8d5 | #eee8d5 | #073642 | #566b72 | #93a1a1 | #217ab8 | #6c71c4 | #859900 | #b58900 | #dc322f | #2aa198 |
| rose-pine | dark | #191724 | #1f1d2e | #26233a | #e0def4 | #908caa | #403d52 | #9ccfd8 | #c4a7e7 | #31748f | #f6c177 | #eb6f92 | #ebbcba |
| rose-pine-moon | dark | #232136 | #2a273f | #393552 | #e0def4 | #a4a1b9 | #44415a | #9ccfd8 | #c4a7e7 | #3e8fb0 | #f6c177 | #eb6f92 | #ea9a97 |
| rose-pine-dawn | light | #faf4ed | #f2e9e1 | #fffaf3 | #575279 | #716d8c | #dfdad9 | #286983 | #907aa9 | #56949f | #ea9d34 | #b4637a | #d7827e |
| everforest-dark | dark | #2d353b | #232a2e | #343f44 | #d3c6aa | #a0aba3 | #475258 | #7fbbb3 | #d699b6 | #a7c080 | #dbbc7f | #e67e80 | #83c092 |
| everforest-light | light | #f4f0d9 | #efebd4 | #fdf6e3 | #5c6a72 | #626f61 | #e6e2cc | #317ca5 | #df69ba | #8da101 | #dfa000 | #f85552 | #35a77c |
| kanagawa-wave | dark | #1f1f28 | #16161d | #2a2a37 | #dcd7ba | #c8c093 | #54546d | #7e9cd8 | #957fb8 | #98bb6c | #e6c384 | #e46876 | #7aa89f |
| kanagawa-dragon | dark | #181616 | #0d0c0c | #282727 | #c5c9c5 | #a6a69c | #625e5a | #8ba4b0 | #8992a7 | #8a9a7b | #c4b28a | #c4746e | #8ea4a2 |
| monokai-pro | dark | #2d2a2e | #221f22 | #403e41 | #fcfcfa | #c1c0c0 | #5b595c | #78dce8 | #ab9df2 | #a9dc76 | #ffd866 | #ff6188 | #78dce8 |
| github-dark | dark | #0d1117 | #010409 | #161b22 | #e6edf3 | #7d8590 | #30363d | #2f81f7 | #a371f7 | #3fb950 | #d29922 | #f85149 | #39c5cf |
| github-light | light | #f6f8fa | #eaeef2 | #ffffff | #1f2328 | #656d76 | #d0d7de | #0969da | #8250df | #1a7f37 | #9a6700 | #d1242f | #1b7c83 |
| ayu-dark | dark | #0d1017 | #0b0e14 | #131721 | #bfbdb6 | #7a808f | #1b1f29 | #59c2ff | #d2a6ff | #aad94c | #ffb454 | #d95757 | #95e6cb |
| ayu-mirage | dark | #242936 | #1f2430 | #282e3b | #cccac2 | #8f97a5 | #171b24 | #73d0ff | #dfbfff | #d5ff80 | #ffd173 | #ff6666 | #95e6cb |
| ayu-light | light | #f8f9fa | #f3f4f5 | #fcfcfc | #5c6166 | #6b727b | #e7eaed | #187abf | #a37acc | #86b300 | #f2ae49 | #e65050 | #4cbf99 |
| nightfox | dark | #192330 | #131a24 | #212e3f | #cdcecf | #aeafb0 | #29394f | #719cd6 | #9d79d6 | #81b29a | #dbc074 | #c94f6d | #63cdcf |
| material-palenight | dark | #292d3e | #202331 | #34324a | #a6accd | #979cb7 | #444267 | #82aaff | #c792ea | #c3e88d | #ffcb6b | #f07178 | #89ddff |
| material-ocean | dark | #0f111a | #090b10 | #1a1c25 | #8f93a2 | #7a83a4 | #3b3f51 | #82aaff | #c792ea | #c3e88d | #ffcb6b | #f07178 | #89ddff |
| oxocarbon | dark | #161616 | #161616 | #262626 | #f2f4f8 | #dde1e6 | #393939 | #78a9ff | #be95ff | #42be65 | #ff7eb6 | #ee5396 | #3ddbd9 |

Sources and mapping notes:
- **Dracula** (draculatheme.com/spec + the official VS Code theme's `BGDark`): bg = Background, bg-alt = BG Dark, surface = Current Line, border = Comment, accent = Purple, accent-2 = Pink, success = Green, warning = Orange, danger = Red, info = Cyan.
  `*` text-muted is **adjusted**: the official Comment `#6272a4` is only 1.94:1 on Current Line. It is lightened in HSL (same hue/saturation, L 51 % → 75 %) to `#aeb6d0`, the first step reaching 4.5:1 (4.53:1 on surface, 7.05:1 on bg). Comment stays as the border.
- **Nord** (nordtheme.com): bg nord0, bg-alt nord1, surface nord2, border nord3, text nord6, text-muted nord4, accent nord8, accent-2 nord15, success nord14, warning nord13, danger nord11, info nord7. Uses Nord's own text tiers (nord4 base, nord6 elevated); nord3 (comments) fails contrast as text.
- **Tokyo Night — Night** (enkia/tokyo-night-vscode-theme, folke/tokyonight.nvim `night`): bg, bg_dark, bg_highlight, fg, fg_dark, fg_gutter, blue, magenta, green, yellow, red, cyan.
- **Pairs** (both ways, tested): catppuccin-latte/mocha, tokyo-night/tokyo-night-day, gruvbox-dark/light, solarized-dark/light, rose-pine/rose-pine-dawn, everforest-dark/light, github-dark/light, ayu-dark/light.

Phase 2 themes (official palettes; same mapping order bg, bg-alt, surface, text, text-muted, border, accent, accent-2, success, warning, danger, info). Values marked as *adjusted* were moved in HSL lightness only (hue/saturation kept) by the minimum 1 % step that passes; ratios are text-muted on surface/bg, or `--k-on-accent` on accent.
- **Catppuccin Frappé / Macchiato** (catppuccin/palette): base, mantle, surface0, text, subtext0, surface1, blue, mauve, green, yellow, red, teal. Frappé subtext0 `#a5adce` *adjusted* to `#acb3d2` (L 73 -> 75 %, 4.56/5.94); subtext1 is too close to text.
- **Gruvbox Dark** (morhetz/gruvbox): bg0, bg0_h, bg1, fg1, fg3, bg2, bright blue/purple/green/yellow/red/aqua. **Gruvbox Light**: bg0, bg1, bg0_h (card), fg1, fg3, bg2, faded blue/purple/green/yellow/red/aqua (faded tier so white on-accent passes, 6.60).
- **Tokyo Night Storm / Day** (folke/tokyonight.nvim): bg, bg_dark, bg_highlight (Day: `#e9e9ed` card, lighter than bg), fg, fg_dark, fg_gutter, blue, magenta, green, yellow, red, cyan. Day *adjusted*: fg_dark `#6172b0` -> `#4f609e` (L 54 -> 47 %, 4.96/4.64); blue `#2e7de9` -> `#1c72e7` (L 55 -> 51 %, white 4.56).
- **One Dark** (atom/one-dark-syntax): syntax-bg, ui bg `#21252b`, cursor-line `#2c313a`, mono-1, mono-2, `#3e4451`, hue-2, hue-3, hue-4, hue-6-2, hue-5, hue-1. mono-2 `#828997` *adjusted* to `#9399a5` (L 55 -> 61 %, 4.57/4.89); mono-3 (comments) fails worse.
- **Solarized Dark** (ethanschoonover.com/solarized): base03, base03 (no darker tone exists), base02, base1, base0, base01, blue, violet, green, yellow, red, cyan. *adjusted*: base0 `#839496` -> `#8b9b9d` (L 55 -> 58 %, 4.50/5.20); blue `#268bd2` -> `#3295da` (L 49 -> 53 %, on base03 4.61). **Solarized Light**: base3, base2, base2, base02, base01, base1, same accents. *adjusted*: base01 `#586e75` -> `#566b72` (L 40 -> 39 %, 4.58/5.20); blue `#268bd2` -> `#217ab8` (L 49 -> 43 %, white 4.62).
- **Rosé Pine / Moon** (rosepinetheme.com): base, surface, overlay, text, subtle, highlight med, foam (accent), iris, pine (success, no green in palette), gold, love, rose (info; foam is already the accent). Moon subtle `#908caa` *adjusted* to `#a4a1b9` (L 61 -> 68 %, 4.64/6.24). **Dawn**: base, overlay, surface (card), text, subtle, highlight med, pine (accent, foam fails white), iris, foam (success), gold, love, rose. Dawn subtle `#797593` -> `#716d8c` (L 52 -> 49 %, 4.74/4.50).
- **Everforest Dark** (sainnhe/everforest, medium): bg0, bg_dim, bg1, fg, grey2, bg3, blue, purple, green, yellow, red, aqua. grey2 `#9da9a0` -> `#a0aba3` (L 64 -> 65 %, 4.56/5.25). **Light** (medium): bg1, bg_dim, bg0 (card), fg, grey2, bg3, ... *adjusted*: grey2 `#829181` -> `#626f61` (L 54 -> 41 %, 4.91/4.62); blue `#3a94c5` -> `#317ca5` (L 50 -> 42 %, white 4.61; no darker official blue).
- **Kanagawa Wave** (rebelot/kanagawa.nvim): sumiInk3, sumiInk0, sumiInk4, fujiWhite, oldWhite, sumiInk6, crystalBlue, oniViolet, springGreen, carpYellow, waveRed, waveAqua2. **Dragon**: dragonBlack3, dragonBlack0, dragonBlack4, dragonWhite, dragonGray, dragonBlack6, dragonBlue2, dragonViolet, dragonGreen2, dragonYellow, dragonRed, dragonAqua.
- **Monokai Pro** (monokai.pro, classic filter): background, dark1, dimmed5, text, dimmed1, dimmed4, blue, purple, green, yellow, red, blue (no separate cyan).
- **GitHub Dark / Light** (primer/primitives): canvas.default/inset/subtle (Light: canvas.subtle bg, neutral `#eaeef2` alt, canvas.default card), fg.default, fg.muted, border.default, accent.fg, done.fg, success.fg, attention.fg, danger.fg, ansi cyan.
- **Ayu Dark / Mirage / Light** (ayu-theme/ayu-colors): editor.bg, ui.bg, line/panel, editor.fg, ui.fg, gutter/line, syntax.entity, constant, string, func, common.error, regexp. *adjusted* ui.fg (comment tier fails too): Dark `#565b66` -> `#7a808f` (L 37 -> 52 %, 4.53/4.81); Mirage `#707a8c` -> `#8f97a5` (L 49 -> 60 %, 4.62/4.94); Light `#8a9199` -> `#6b727b` (L 57 -> 45 %, 4.74/4.61). Light entity `#399ee6` -> `#187abf` (L 56 -> 42 %, white 4.59).
- **Nightfox** (EdenEast/nightfox.nvim): bg1, bg0, bg2, fg1, fg2, bg3, blue, magenta, green, yellow, red, cyan.
- **Material Palenight / Ocean** (material-theme): background, contrast, second background, foreground, text, highlight/line-numbers, blue, purple, green, yellow, red, cyan. *adjusted* "text": Palenight `#676e95` -> `#979cb7` (L 49 -> 65 %, 4.56/5.04); Ocean `#4b526d` -> `#7a83a4` (L 36 -> 56 %, 4.54/5.03).
- **Oxocarbon** (nyoom-engineering/oxocarbon.nvim): base00, base00 (no darker tone), base01, base05, base04, base02, base09, base0E, base0D, base0C (pink; palette has no yellow/orange), base0A, base08.

Phase 2 adds the rest of SPEC §5.2. Take values from each project's official palette, add a row here, and keep `text` and `text-muted` at >= 4.5:1 on both `surface` and `bg` (enforced per theme by `loader.test.ts`, along with `--k-on-accent` on `accent`). If an official muted colour fails, lighten/darken it in HSL by the minimum step and note it here.

## Adding a theme
1. Drop `<id>.json` into `src/lib/theme/themes/` (picked up by `import.meta.glob`).
2. Set `pair` on both light and dark variants.
3. Add the row above; update the registry test in `loader.test.ts`.
