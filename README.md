# Kotilo Deck

A full-screen developer dashboard for your secondary display: clock, weather, calendar, system load, media, pomodoro, todos and your AI agents, in every popular developer colour theme. Part of [Kotilo](https://kotilo.dev).

> Status: Phase 0 (bootstrap). Nothing to use yet. See [SPEC.md](SPEC.md) for the plan.

## Develop

Requirements: Node 22+, pnpm 10, Rust stable, and the [Tauri 2 prerequisites](https://tauri.app/start/prerequisites/) for your OS (WebView2 + MSVC build tools on Windows).

```bash
pnpm install
pnpm tauri dev
```

| Command | What it does |
|---|---|
| `pnpm tauri dev` | Run the app with hot reload |
| `pnpm test` | Frontend unit tests (Vitest) |
| `pnpm lint` | Hex-colour lint + `svelte-check` |
| `cargo test` (in `src-tauri/`) | Backend tests |
| `pnpm tauri build` | Release build and installers |

## Docs

- [SPEC.md](SPEC.md): product specification
- [docs/design/DESIGN.md](docs/design/DESIGN.md): UI design handoff
- [ARCHITECTURE.md](ARCHITECTURE.md): module map and contracts
- [DECISIONS.md](DECISIONS.md): defaults chosen along the way

## Hardware ideas

A 7" to 15" USB-C portable monitor, or an old tablet (browser mode, Phase 4).

## License

MIT
