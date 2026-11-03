---
name: release
description: Kotilo Deck release procedure - version bump, CHANGELOG, build, signing, GitHub Release and update manifest. Use when cutting any release or pre-release.
---

# Release

Versions follow SemVer; the same version lives in `package.json`, `src-tauri/Cargo.toml` and `src-tauri/tauri.conf.json`.

1. Make sure `main` is green in CI.
2. Bump the version in all three files.
3. Move `## [Unreleased]` in `CHANGELOG.md` under a new `## [x.y.z] - YYYY-MM-DD` heading (Keep a Changelog format), built from conventional commits since the last tag.
4. Commit `chore(release): vx.y.z`, tag `vx.y.z`.
5. Build: `pnpm tauri build` (MSI + NSIS on Windows; AppImage/Flatpak on Linux from Phase 5).
6. Sign the Windows installers (code signing set up in Phase 5).
7. Create the GitHub Release with the changelog section and installers attached; publish the updater manifest (Phase 4).
8. Update the winget manifest (Phase 5).

Signing and updater steps are placeholders until those phases land; skip them with a note in the release body.
