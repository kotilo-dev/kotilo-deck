# Decisions

Defaults chosen without asking (SPEC §0). Newest last.

| # | Date | Decision | Why |
|---|---|---|---|
| 1 | 2026-11-03 | Tauri 2 + SvelteKit + TypeScript + Tailwind (spec defaults). | SPEC §3 **(default)**. |
| 2 | 2026-11-03 | SvelteKit 3 with `adapter-static`, `ssr = false`, page prerendered. Kit config lives in `vite.config.ts` (`svelte.config.js` is gone in Kit 3). | Tauri has no Node server; current stable Kit. |
| 3 | 2026-11-03 | Shared frontend code imported as `#lib/<path>/index.ts` via `package.json` `"imports": { "#lib/*": "./src/lib/*" }`. | Kit 3 removed `$lib`; TypeScript needs the explicit file for subpath imports. |
| 4 | 2026-11-03 | TypeScript pinned to `^6`. | Kit 3.1 and svelte-check peer ranges stop at 6; TS 7 is out but not yet supported by them. |
| 5 | 2026-11-03 | Tailwind 4 via `@tailwindcss/vite`, no config file. | Tailwind for layout only; colours stay in `--k-*` variables. |
| 6 | 2026-11-03 | Fonts bundled from `@fontsource/inter` and `@fontsource/jetbrains-mono`. | DESIGN §3: no Google Fonts at runtime, deck may be offline. |
| 7 | 2026-11-03 | Bundled themes live in `src/lib/theme/themes/`, loaded with `import.meta.glob`. | Adding a theme = dropping a JSON; no registry edits. |
| 8 | 2026-11-03 | Vitest for frontend unit tests; `cargo test` for Rust. | Ships with Vite; no extra runner. |
| 9 | 2026-11-03 | App identifier `dev.kotilo.deck`; dev server on port 1420. | Matches kotilo.dev; Tauri's conventional port. |
| 10 | 2026-11-03 | Phase 0 window is 1280×720 borderless, not yet fullscreen. | Fullscreen-on-chosen-monitor is Phase 1 scope. |
| 11 | 2026-11-03 | `Platform` trait methods default to `Unsupported`; `current()` picks the impl via `cfg`. | Lets Linux/macOS compile before they are implemented (SPEC §4.3: never crash). |
