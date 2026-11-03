# Kotilo Deck

Read `SPEC.md` first; work phase by phase (SPEC §8). Pick **(default)** options without asking and log them in `DECISIONS.md`.

Read `docs/design/DESIGN.md` before touching anything under `src/widgets/`, `src/lib/grid/` or `src/lib/theme/`.

- Stack: Tauri 2 (Rust, `src-tauri/`) + SvelteKit 3 / Svelte 5 + Tailwind 4 (`src/`). Package manager: pnpm.
- Kit 3: config is in `vite.config.ts`; import shared code as `#lib/<dir>/index.ts`, not `$lib`.
- Colours only via `var(--k-*)`; `pnpm lint:hex` fails on hex under `src/widgets/**`.
- OS APIs only inside `src-tauri/src/platform/` behind the `Platform` trait.
- Before calling work done: `pnpm lint`, `pnpm test`, and `cargo fmt --check && cargo clippy -- -D warnings && cargo test` in `src-tauri/`.
- Conventional commits; one widget per PR. Update `CHANGELOG.md` under `[Unreleased]`.
- Agents in `.claude/agents/`, skills in `.claude/skills/` (SPEC §0).
