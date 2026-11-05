# Releasing Kotilo Deck

Maintainer guide. The short version is in `.claude/skills/release/SKILL.md`. User-facing install notes are in [install.md](install.md).

## What a release contains

`.github/workflows/release.yml` builds everything from a `vX.Y.Z` tag and attaches it to a **draft** GitHub Release. GitHub turns the space in "Kotilo Deck" into a dot in asset names.

| Asset | Built on | Notes |
|---|---|---|
| `Kotilo.Deck_X.Y.Z_x64-setup.exe` (+ `.sig`) | windows-latest | NSIS, per-user install (no admin), English/Turkish by OS language. Default download, winget source, updater target. |
| `Kotilo.Deck_X.Y.Z_x64_en-US.msi`, `…_tr-TR.msi` (+ `.sig`) | windows-latest | WiX, per-machine. For admins and GPO deployment. |
| `Kotilo.Deck_X.Y.Z_amd64.AppImage` (+ `.sig`) | ubuntu-22.04 | Portable; the Linux updater target. No GStreamer bundled (`bundleMediaFramework: false`). |
| `Kotilo.Deck_X.Y.Z_amd64.deb` | ubuntu-22.04 | Debian/Ubuntu; also the Flatpak source. |
| `Kotilo.Deck-X.Y.Z-1.x86_64.rpm` | ubuntu-22.04 | Fedora/openSUSE. |
| `latest.json` | both | Updater manifest written by `tauri-action` (`windows-x86_64` → NSIS, `linux-x86_64` → AppImage). |

Ubuntu 22.04 is used on purpose: the AppImage and .deb then run on glibc 2.35 and newer.

The app polls `https://github.com/kotilo-dev/kotilo-deck/releases/latest/download/latest.json`. That URL only resolves to the newest **published, non-prerelease** release, so nothing reaches users until the draft is published.

## One-time setup

Repository ▸ Settings ▸ Secrets and variables ▸ Actions:

| Secret | Required | Value |
|---|---|---|
| `TAURI_SIGNING_PRIVATE_KEY` | yes | The **contents** of `~/.tauri/kotilo-deck.key` (minisign; its public key is `plugins.updater.pubkey` in `tauri.conf.json`). Without it the build fails, because `createUpdaterArtifacts` is on. |
| `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` | no | Leave it unset. The key has an empty password and GitHub cannot store an empty secret; an unset secret resolves to `""`. |
| `WINDOWS_CERTIFICATE` | no | Base64 of an Authenticode `.pfx` (`[Convert]::ToBase64String([IO.File]::ReadAllBytes("cert.pfx"))`). |
| `WINDOWS_CERTIFICATE_PASSWORD` | with the above | The `.pfx` password. |

Never commit, print or upload the private keys. `~/.tauri/kotilo.key` is a different key (other Kotilo apps) and must not be used here.

## Cutting a release

1. `main` is green in CI, and the Linux smoke test below has passed at least once for this release.
2. Bump the version in `package.json`, `src-tauri/Cargo.toml` and `src-tauri/tauri.conf.json` (`node scripts/check-version.mjs` checks that they agree), and run `cargo check` so `Cargo.lock` follows.
3. In `CHANGELOG.md`, move `## [Unreleased]` under `## [X.Y.Z] - YYYY-MM-DD`. `node scripts/changelog-section.mjs X.Y.Z` prints exactly what becomes the release body.
4. Add a `<release version="X.Y.Z" date="…">` entry to `packaging/flatpak/dev.kotilo.deck.metainfo.xml`.
5. Commit `chore(release): vX.Y.Z`, tag `vX.Y.Z`, push both: `git push origin main vX.Y.Z`.

The workflow then:

1. **verify-version**: the tag must equal all three versions, and CHANGELOG must have the section.
2. **create-release**: creates the draft `vX.Y.Z` with the changelog section as body (plus an "installers are not code-signed" note when `WINDOWS_CERTIFICATE` is missing). A re-run edits an existing draft and refuses to touch a published release.
3. **build** (Windows + Linux matrix): `tauri-action` builds, uploads the installers and `.sig` files, and merges its platform into `latest.json` on the draft.
4. **verify-release**: downloads `latest.json` and fails unless both `windows-x86_64` (`-setup.exe`) and `linux-x86_64` (`.AppImage`) are present and signed. The two matrix jobs can race when both update `latest.json`; if this check fails, re-run the failed build job (or the whole workflow; it reuses the draft).

To rebuild an existing tag, run the workflow manually (Actions ▸ Release ▸ Run workflow) with `tag: vX.Y.Z`.

## Verify the draft, then publish

- Assets: the six installers listed above, their `.sig` files and `latest.json`.
- `latest.json`: `version` has no `v`, the URLs point at `releases/download/vX.Y.Z/…`, and the signatures are file contents, not paths.
- **Windows**, on a fresh VM (SPEC Phase 5 acceptance: a working deck in under 2 minutes without docs): download the setup.exe, install, run onboarding. Without a certificate, expect SmartScreen's "Windows protected your PC" dialog.
- **Linux smoke test**: the Linux platform layer had never run on real Linux before 1.0 (DECISIONS 59). On Ubuntu 24.04 (GNOME, PipeWire) and one more distro (Fedora KDE or similar), check that the AppImage and the .deb start, the volume widget lists devices (`pactl`), the media widget follows Spotify/Firefox (MPRIS), the tray works, and logs appear in `~/.kotilo/deck/logs/`.
- **Updater**: with an older release build installed, Settings ▸ About ▸ Check now only finds the update after publishing. To test before publishing, upload the draft's `latest.json` to a scratch pre-release in a fork, or test right after publishing and be ready to pull the release.

Publish with `gh release edit vX.Y.Z --draft=false` (or the web UI). Then do winget and Flatpak below.

To pull a bad release, mark it as a draft again (`gh release edit vX.Y.Z --draft=true`). `latest/download/latest.json` then falls back to the previous release, so users stop seeing the update.

## Local fallback build

If CI is unavailable, build on each OS and upload by hand:

```sh
export TAURI_SIGNING_PRIVATE_KEY="$HOME/.tauri/kotilo-deck.key"   # path or contents
export TAURI_SIGNING_PRIVATE_KEY_PASSWORD=""
pnpm tauri build --bundles nsis,msi            # Windows
pnpm tauri build --bundles appimage,deb,rpm    # Linux (same apt packages as ci.yml)
```

Output goes to `src-tauri/target/release/bundle/<type>/`, with a `.sig` next to each updater artifact. Write `latest.json` by hand in the format shown in the release skill, then `gh release upload vX.Y.Z <files> latest.json`.

## Code signing (Windows Authenticode)

**Current state:** there is no certificate, so the Windows installers ship unsigned. Updates are still safe, because the in-app updater checks every download against the minisign public key compiled into the app. Unsigned only affects first install (SmartScreen, some antivirus heuristics).

**How CI signs:** when `WINDOWS_CERTIFICATE` and `WINDOWS_CERTIFICATE_PASSWORD` exist, the build job imports the PFX into `Cert:\CurrentUser\My` and passes a generated `--config` override with `bundle.windows.certificateThumbprint`, `digestAlgorithm: sha256` and `timestampUrl: http://timestamp.digicert.com`. The Tauri bundler then signs the app exe, the NSIS installer and the MSIs with signtool *before* creating the minisign `.sig` files, so the updater signatures match the signed files.

Since 2023, publicly trusted code-signing keys must live on hardware (HSM/token), so a plain downloadable PFX is no longer sold. The PFX path above only fits certificates whose provider still exports one, or self-signed test certificates. The two recommended routes do not use a PFX:

1. **Azure Trusted Signing** (now also called Azure Artifact Signing), about USD 10/month. Microsoft-managed certificate with good SmartScreen standing. Identity validation is limited to some countries and organisations with a verifiable history, so check eligibility first. Tauri integrates it through `bundle.windows.signCommand` with [`trusted-signing-cli`](https://v2.tauri.app/distribute/sign/windows/#azure-trusted-signing): add the Azure client id/secret/tenant as secrets, install the CLI in the Windows job and set `signCommand` in the same generated `--config` override instead of `certificateThumbprint`. Signing still happens inside the bundler, so the `.sig` files stay valid.
2. **SignPath Foundation**: free for OSI-licensed open-source projects (MIT qualifies). They review the project, then sign artifacts built by a GitHub Actions workflow in this repository (the certificate is issued to "SignPath Foundation"). Signing happens *after* the build, so the order changes: build unsigned → upload the artifacts to SignPath with `signpath/github-action-submit-signing-request` → download the signed files → re-create the updater signatures with `pnpm tauri signer sign <file>` → upload them and regenerate `latest.json`. This needs a separate job; don't sign after `tauri-action` has already uploaded, or the `.sig` files will not match.

Either way, SmartScreen reputation builds up per certificate over downloads, so the first signed release may still warn for a while.

## winget

Package `Kotilo.KotiloDeck`. The manifest set (ManifestVersion 1.6.0, `nullsoft` installer, user scope) is in `packaging/winget/` and is generated by `scripts/winget-manifest.mjs`. Edit the script, not the YAML.

After publishing the release:

```sh
node scripts/winget-manifest.mjs X.Y.Z https://github.com/kotilo-dev/kotilo-deck/releases/download/vX.Y.Z/Kotilo.Deck_X.Y.Z_x64-setup.exe
winget validate --manifest packaging/winget
winget install --manifest packaging/winget       # needs: winget settings --enable LocalManifestFiles; ideally in Windows Sandbox
```

Commit the regenerated `packaging/winget/` (`chore(winget): X.Y.Z`). Then submit:

- **First submission:** fork `microsoft/winget-pkgs`, copy the three files to `manifests/k/Kotilo/KotiloDeck/X.Y.Z/`, open a PR. Or run `wingetcreate submit --token <PAT> packaging/winget`. A bot validates the manifests and runs the installer; a moderator merges it, usually within a few days.
- **Later versions:** `wingetcreate update Kotilo.KotiloDeck --version X.Y.Z --urls <setup.exe URL> --submit --token <PAT>`, or the PR route with the regenerated files.

An installer URL can never be replaced once winget has its hash. If an asset is re-uploaded, its SHA256 changes and winget installs fail, so cut a new patch version instead.

## Flatpak

`packaging/flatpak/` holds `dev.kotilo.deck.yml` (GNOME 46 runtime, repackages the release `.deb`, following Tauri 2's Flatpak guide), the AppStream `dev.kotilo.deck.metainfo.xml` and `dev.kotilo.deck.desktop`.

Per release: update the `.deb` URL and `sha256` (`sha256sum Kotilo.Deck_X.Y.Z_amd64.deb`) in the manifest, and the `<release>` entry in the metainfo.

Build and test locally on Linux:

```sh
cd packaging/flatpak
git clone --depth 1 https://github.com/flathub/shared-modules   # tray: libayatana-appindicator
flatpak install -y flathub org.gnome.Platform//46 org.gnome.Sdk//46
flatpak-builder --user --install --force-clean build-dir dev.kotilo.deck.yml
flatpak run dev.kotilo.deck
flatpak run --command=flatpak-builder-lint org.flatpak.Builder manifest dev.kotilo.deck.yml
```

Flathub submission: fork `flathub/flathub`, branch from `new-pr`, add the manifest, metainfo, desktop file and `shared-modules` as a git submodule, and open a PR against `new-pr`. After acceptance Flathub creates `flathub/dev.kotilo.deck`, and each release is a PR there (Flathub's bot can open it automatically with an `x-checker-data` entry on the `.deb` source). Flathub also requires real screenshots: the metainfo points at `docs/screenshots/deck-focus.png` and `deck-edit.png` on `main`, so add those first.

Known sandbox limits to check in the smoke test:
- `pactl` must exist inside the runtime for the volume widget. If it is missing, add a module that builds PulseAudio's client utilities (`-Ddaemon=false`), or ship `wpctl`. Without either the widget reports "unsupported".
- The System widget's top-process list only sees processes inside the sandbox (PID namespace). CPU, memory and GPU totals are unaffected.
- Global hotkeys and window placement on Wayland depend on the compositor (no Flatpak-specific fix).
- Autostart writes `~/.config/autostart`, which the sandbox cannot reach; it needs the Background portal (follow-up).

## Follow-up: updater in package-managed installs

The in-app updater should step aside when a package manager owns the install:
- **Flatpak:** `FLATPAK_ID` is set (and `/.flatpak-info` exists). Updates come from Flathub, and the AppImage in `latest.json` cannot replace a Flatpak.
- **deb/rpm:** the binary is in `/usr/bin` and `APPIMAGE` is unset. The updater on Linux only replaces AppImages.
- **winget:** installs the same NSIS package, so in-app updates work, but winget then reports a stale version until the manifest catches up. It can't be detected reliably; keep the updater on.

Suggested change (not done yet; it touches `src-tauri/src/`): report `unsupported` with a "managed by your package manager" reason in those cases, as debug builds already do.
