---
name: release
description: Kotilo Deck release procedure - version bump, CHANGELOG, tag push, CI draft release with installers and latest.json, verification, publishing, winget and Flatpak, code signing. Use when cutting any release or pre-release.
---

# Release

Full maintainer guide: `docs/releasing.md`. Versions follow SemVer; the same version lives in `package.json`, `src-tauri/Cargo.toml` and `src-tauri/tauri.conf.json`.

## Prepare (local)

1. `main` is green in CI. Before 1.0 and after any Linux platform change, run the Linux smoke test from `docs/releasing.md` (the Linux layer was written without a Linux machine, DECISIONS 59).
2. Bump the version in all three files, then `cargo check` in `src-tauri/` so `Cargo.lock` follows. `node scripts/check-version.mjs` must print three `ok` lines.
3. Move `## [Unreleased]` in `CHANGELOG.md` under `## [x.y.z] - YYYY-MM-DD` (Keep a Changelog), built from the conventional commits since the last tag, and leave an empty `## [Unreleased]` above it. `node scripts/changelog-section.mjs x.y.z` prints the release body.
4. Add `<release version="x.y.z" date="YYYY-MM-DD">` to `packaging/flatpak/dev.kotilo.deck.metainfo.xml`.
5. Commit `chore(release): vx.y.z`, tag `vx.y.z`, then `git push origin main vx.y.z`.

## CI (`.github/workflows/release.yml`, on the tag push)

- `verify-version`: the tag must equal the three versions, and CHANGELOG must have the section.
- `create-release`: creates a **draft** release `vx.y.z` whose body is the changelog section. It adds an "unsigned" note when the `WINDOWS_CERTIFICATE` secret is missing.
- `build`: `tauri-action` (pinned to v1.0.0 by SHA) on windows-latest (`nsis,msi`) and ubuntu-22.04 (`appimage,deb,rpm`). It uploads the installers and `.sig` files and merges `latest.json`: `windows-x86_64` points at the NSIS `-setup.exe` and `linux-x86_64` at the AppImage. Needs the `TAURI_SIGNING_PRIVATE_KEY` secret (contents of `~/.tauri/kotilo-deck.key`); leave `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` unset (empty password).
- `verify-release`: fails unless `latest.json` has both platforms with signatures. The build jobs run one at a time so `latest.json` isn't overwritten; if the check still fails, delete `latest.json` from the draft and re-run the whole workflow. Only plain `vX.Y.Z` tags trigger a release (no pre-releases).
- To rebuild an existing tag: Actions ▸ Release ▸ Run workflow with `tag: vx.y.z` (reuses the draft; refuses a published release).

## Verify and publish

1. Check the draft's assets: setup.exe, the en-US MSI, AppImage, deb, rpm, `.sig` files and `latest.json`.
2. Fresh Windows VM: install the setup.exe and reach a working deck in under 2 minutes without docs. Without a certificate, SmartScreen warns ("More info ▸ Run anyway").
3. Linux: the AppImage and .deb start; volume (`pactl`), media (MPRIS), tray and logs work.
4. Publish: `gh release edit vx.y.z --draft=false`. Only then does `https://github.com/kotilo-dev/kotilo-deck/releases/latest/download/latest.json` (the updater endpoint) serve the new version. Check it with `curl -L`, then use Settings ▸ About ▸ Check now on an older release build. Debug builds always report `unsupported`.
5. To pull a bad release, make it a draft again; the endpoint falls back to the previous release.

## After publishing

- **winget**: `node scripts/winget-manifest.mjs x.y.z <setup.exe release URL>` regenerates `packaging/winget/` with the real SHA256. Run `winget validate --manifest packaging/winget`, commit, then submit with `wingetcreate submit` (first time) or `wingetcreate update Kotilo.KotiloDeck --version x.y.z --urls <url> --submit`, or a PR to `microsoft/winget-pkgs` under `manifests/k/Kotilo/KotiloDeck/x.y.z/`. Never re-upload a release asset after winget has its hash.
- **Flatpak**: put the new `.deb` URL and `sha256` in `packaging/flatpak/dev.kotilo.deck.yml`, build it with `flatpak-builder`, and open the update PR on `flathub/dev.kotilo.deck` (first time: a PR against `flathub/flathub` `new-pr`).

## Code signing

Authenticode is optional and driven by secrets. With `WINDOWS_CERTIFICATE` (base64 .pfx) and `WINDOWS_CERTIFICATE_PASSWORD` set, CI imports the certificate and passes `bundle.windows.certificateThumbprint` (sha256, DigiCert timestamp) through a generated `--config` override, so the bundler signs before it writes the `.sig` files. Without them the build is unsigned and the release body says so. The recommended routes are Azure Trusted Signing (`signCommand`) or SignPath Foundation (free for OSS; signs after the build, so the `.sig` files and `latest.json` must be regenerated). See `docs/releasing.md`.

## Local fallback build

```sh
export TAURI_SIGNING_PRIVATE_KEY="$HOME/.tauri/kotilo-deck.key"   # path or contents; empty password
export TAURI_SIGNING_PRIVATE_KEY_PASSWORD=""
pnpm tauri build --bundles nsis,msi            # Windows
pnpm tauri build --bundles appimage,deb,rpm    # Linux
```

Output: `src-tauri/target/release/bundle/<type>/`, with a `.sig` next to each updater artifact. Upload with `gh release upload vx.y.z …` and write `latest.json` by hand:

```json
{
  "version": "x.y.z",
  "notes": "<changelog section>",
  "pub_date": "2026-11-04T12:00:00Z",
  "platforms": {
    "windows-x86_64": {
      "signature": "<contents of Kotilo Deck_x.y.z_x64-setup.exe.sig>",
      "url": "https://github.com/kotilo-dev/kotilo-deck/releases/download/vx.y.z/Kotilo.Deck_x.y.z_x64-setup.exe"
    },
    "linux-x86_64": {
      "signature": "<contents of Kotilo Deck_x.y.z_amd64.AppImage.sig>",
      "url": "https://github.com/kotilo-dev/kotilo-deck/releases/download/vx.y.z/Kotilo.Deck_x.y.z_amd64.AppImage"
    }
  }
}
```

`signature` is the text of the `.sig` file, not a path. `version` has no `v`, `pub_date` is RFC 3339, and GitHub turns spaces in asset names into `.`.

Never commit, print or upload `kotilo-deck.key`; only its public key is in `tauri.conf.json` (`plugins.updater.pubkey`). It is a different key from `~/.tauri/kotilo.key`.
