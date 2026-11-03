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

Phase 1 adds three more; Phase 2 adds the rest of SPEC §5.2. Take values from each project's official palette, add a row here, and keep `text-muted` at >= 4.5:1 on `surface`.

## Adding a theme
1. Drop `<id>.json` into `src/lib/theme/themes/` (picked up by `import.meta.glob`).
2. Set `pair` on both light and dark variants.
3. Add the row above; update the registry test in `loader.test.ts`.
