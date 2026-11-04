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
5. Build with the updater signing key in the environment (`bundle.createUpdaterArtifacts` is on, so the build fails without it):
   ```sh
   # path or contents of the minisign private key; it has an empty password
   export TAURI_SIGNING_PRIVATE_KEY="$HOME/.tauri/kotilo-deck.key"
   export TAURI_SIGNING_PRIVATE_KEY_PASSWORD=""
   pnpm tauri build
   ```
   Output (Windows): MSI + NSIS installers in `src-tauri/target/release/bundle/{msi,nsis}/`, each with a `.sig` next to it. AppImage/Flatpak on Linux from Phase 5. Never commit, print or upload `kotilo-deck.key`; only `kotilo-deck.key.pub` is in `tauri.conf.json` (`plugins.updater.pubkey`). It is a different key from `~/.tauri/kotilo.key`.
6. Sign the Windows installers (code signing set up in Phase 5).
7. Create the GitHub Release `vx.y.z` on `kotilo-dev/kotilo-deck` with the changelog section and the installers attached. Then publish the updater manifest. The app polls `https://github.com/kotilo-dev/kotilo-deck/releases/latest/download/latest.json`, so the file must be an asset named exactly `latest.json` on the newest non-draft, non-prerelease release:
   ```json
   {
     "version": "x.y.z",
     "notes": "<changelog section, plain text/markdown>",
     "pub_date": "2026-11-04T12:00:00Z",
     "platforms": {
       "windows-x86_64": {
         "signature": "<contents of Kotilo Deck_x.y.z_x64-setup.exe.sig>",
         "url": "https://github.com/kotilo-dev/kotilo-deck/releases/download/vx.y.z/Kotilo.Deck_x.y.z_x64-setup.exe"
       }
     }
   }
   ```
   - `signature` is the text of the `.sig` file, not a path. `url` must point at the same installer the `.sig` belongs to. Use NSIS (`-setup.exe`) by default. GitHub replaces spaces in asset names with `.`, so copy the URL from the uploaded asset.
   - `pub_date` is RFC 3339 and `version` has no `v` prefix. Add `linux-x86_64` (AppImage + its `.sig`) once Linux builds ship.
   - Upload it with `gh release upload vx.y.z latest.json`, then check that `curl -L <endpoint>` returns it. Settings ▸ About ▸ Check now on an older release build should then report the update. Debug builds always report `unsupported`.
8. Update the winget manifest (Phase 5).

Code signing (step 6) and winget (step 8) stay placeholders until Phase 5; skip them with a note in the release body.
