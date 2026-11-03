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

Sources and mapping notes:
- **Dracula** (draculatheme.com/spec + the official VS Code theme's `BGDark`): bg = Background, bg-alt = BG Dark, surface = Current Line, border = Comment, accent = Purple, accent-2 = Pink, success = Green, warning = Orange, danger = Red, info = Cyan.
  `*` text-muted is **adjusted**: the official Comment `#6272a4` is only 1.94:1 on Current Line. It is lightened in HSL (same hue/saturation, L 51 % → 75 %) to `#aeb6d0`, the first step reaching 4.5:1 (4.53:1 on surface, 7.05:1 on bg). Comment stays as the border.
- **Nord** (nordtheme.com): bg nord0, bg-alt nord1, surface nord2, border nord3, text nord6, text-muted nord4, accent nord8, accent-2 nord15, success nord14, warning nord13, danger nord11, info nord7. Uses Nord's own text tiers (nord4 base, nord6 elevated); nord3 (comments) fails contrast as text.
- **Tokyo Night — Night** (enkia/tokyo-night-vscode-theme, folke/tokyonight.nvim `night`): bg, bg_dark, bg_highlight, fg, fg_dark, fg_gutter, blue, magenta, green, yellow, red, cyan.
- No `pair` yet for these three: their light variants (Tokyo Night Day) arrive in Phase 2; `pair` must point at a bundled theme (tested).

Phase 2 adds the rest of SPEC §5.2. Take values from each project's official palette, add a row here, and keep `text` and `text-muted` at >= 4.5:1 on both `surface` and `bg` (enforced per theme by `loader.test.ts`, along with `--k-on-accent` on `accent`). If an official muted colour fails, lighten/darken it in HSL by the minimum step and note it here.

## Adding a theme
1. Drop `<id>.json` into `src/lib/theme/themes/` (picked up by `import.meta.glob`).
2. Set `pair` on both light and dark variants.
3. Add the row above; update the registry test in `loader.test.ts`.
