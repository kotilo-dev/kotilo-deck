---
name: widget-authoring
description: Step-by-step template for adding a Kotilo Deck widget - manifest, Svelte component per size, settings defaults, optional Rust command, tests. Use whenever creating or extending a widget under src/widgets/.
---

# Adding a widget

One widget per PR. Get an `architect` review of the plan first.

## 1. Folder
```
src/widgets/<id>/
  manifest.json
  Widget.svelte
  settings.ts
  <id>.test.ts        (logic only: parsers, formatters, state machines)
src-tauri/src/widgets/<id>.rs   (only if it needs the backend)
```

## 2. manifest.json
```json
{
  "id": "clock",
  "name": "Clock",
  "icon": "clock",
  "sizes": ["2x1", "2x2", "4x2"],
  "defaultSize": "4x2",
  "capabilities": [],
  "multiInstance": true,
  "settings": { "type": "object", "properties": { "seconds": { "type": "boolean" } } }
}
```
`sizes` must match the DESIGN.md §5 entry for this widget exactly.

## 3. Widget.svelte
- Props: `size` (`"2x2"` etc.), `settings`, `instance`.
- One `{#if}` branch per supported size, each following DESIGN.md §5.
- Card: `background: var(--k-surface-a); border: 1px solid var(--k-border); border-radius: var(--k-radius)`. Header: title 13/600 + meta 11 muted.
- Colours only from `var(--k-*)`; `pnpm lint:hex` fails otherwise.
- Missing capability: render the muted "Not available on this OS" state.
- Loading: skeleton bars on `--k-bg-alt`. Error: inline message + Retry button; never throw out of the component.
- Strings from `src/i18n/en.json` and `tr.json`.

## 4. settings.ts
Export `defaults` matching the manifest schema, plus `appearance: { opacity?: number, blur?: number }` support.

## 5. Backend (optional)
Add `src-tauri/src/widgets/<id>.rs` with `#[tauri::command]` functions named `<id>_<verb>`; OS calls go through `platform::current()`. Register in `lib.rs`.

## 6. Done when
- Every size renders at 1280x720 and 1024x600 in Mocha and Latte.
- `pnpm lint`, `pnpm test`, `cargo test` pass.
- `reviewer` checklist (DESIGN.md §9) passes.
