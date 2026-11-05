# Installing Kotilo Deck

Downloads are on the [Releases page](https://github.com/kotilo-dev/kotilo-deck/releases/latest). Kotilo Deck runs on Windows 10/11 (x64) and on 64-bit Linux.

## Windows

### Installer (recommended)

1. Download `Kotilo.Deck_<version>_x64-setup.exe`.
2. Run it. It installs for your user only, so it needs no administrator rights, and it uses your Windows display language (English or Turkish).
3. Kotilo Deck starts and walks you through picking a monitor, a theme and a starting workspace.

Windows 11 already has the WebView2 runtime the app needs. On Windows 10 the installer downloads it if it is missing, which needs an internet connection.

**"Windows protected your PC":** the installers are not code-signed yet, so SmartScreen may warn about an unrecognised app. Click **More info ▸ Run anyway**. Your browser may also ask you to confirm the download. Updates installed from inside the app are always checked against the Kotilo Deck signing key, whether or not the installer is signed.

### winget

```powershell
winget install Kotilo.KotiloDeck
```

This installs the same per-user installer. `winget upgrade Kotilo.KotiloDeck` updates it, and so does the in-app updater.

### MSI (administrators)

`Kotilo.Deck_<version>_x64_en-US.msi` (or `_tr-TR.msi`) installs for all users into Program Files and needs administrator rights. Use it for managed deployment. Most people should use the setup.exe.

### Uninstall

Settings ▸ Apps ▸ Installed apps ▸ Kotilo Deck ▸ Uninstall, or `winget uninstall Kotilo.KotiloDeck`. Your workspaces and settings are kept; see [Your data](#your-data).

## Linux

The Linux build is new in 1.0. If something doesn't work on your distribution, please [open an issue](https://github.com/kotilo-dev/kotilo-deck/issues/new?template=bug_report.md) with the log attached.

### AppImage (any distribution)

```sh
chmod +x Kotilo.Deck_<version>_amd64.AppImage
./Kotilo.Deck_<version>_amd64.AppImage
```

AppImages need FUSE 2: `sudo apt install libfuse2` (Ubuntu 22.04), `sudo apt install libfuse2t64` (Ubuntu 24.04+), `sudo dnf install fuse-libs` (Fedora). The AppImage is the Linux build that updates itself from inside the app.

### Debian / Ubuntu (.deb)

```sh
sudo apt install ./Kotilo.Deck_<version>_amd64.deb
```

Remove it with `sudo apt remove kotilo-deck`. To update, install the newer `.deb` the same way.

### Fedora / openSUSE (.rpm)

```sh
sudo dnf install ./Kotilo.Deck-<version>-1.x86_64.rpm
```

Remove it with `sudo dnf remove kotilo-deck`.

### Flatpak

Once Kotilo Deck is on Flathub:

```sh
flatpak install flathub dev.kotilo.deck
```

Flatpak updates come through `flatpak update` (or your software centre), not through the in-app updater. Inside the sandbox, the System widget's process list only shows processes in the sandbox.

### What the widgets need on Linux

| Feature | Needs | Package |
|---|---|---|
| Volume widget (master, devices, per-app) | `pactl`, with PulseAudio or PipeWire (`pipewire-pulse`) | `pulseaudio-utils` (Debian, Ubuntu, Fedora), `libpulse` (Arch). Fallback for volume and mute only: `wpctl` from `wireplumber`. |
| Media widget | An MPRIS-capable player on the session D-Bus | Spotify, Firefox, Chromium, VLC, mpv (with `mpv-mpris`) and most others |
| Tray icon | libayatana-appindicator (pulled in by the .deb/.rpm) | On GNOME, also the "AppIndicator and KStatusNotifierItem Support" extension |
| Notifications | A notification daemon | Every desktop environment ships one |

Global hotkeys (Ctrl+Alt+K, Ctrl+Alt+←/→) work on X11. Many Wayland compositors block global shortcuts; use the tray menu or bind the shortcut in your desktop's settings instead.

## Your data

Kotilo Deck keeps your data in your home folder, not in the install folder:

| Path | Contents |
|---|---|
| `~/.kotilo/deck/` (Windows: `%USERPROFILE%\.kotilo\deck\`) | Workspaces, todos and pomodoro history (`deck.db`), wallpapers, widget plugins (`widgets/`), backups, weather cache and logs (`logs/`) |
| `~/.kotilo/themes/` | Your own themes, shared with other Kotilo apps |
| `~/.kotilo/companion.token` | Token for the Kotilo desktop companion and LAN mode, shared with other Kotilo apps |
| Windows: `%APPDATA%\dev.kotilo.deck\`<br>Linux: `~/.local/share/dev.kotilo.deck/` (Flatpak: `~/.var/app/dev.kotilo.deck/`) | `settings.json` (app settings) and the web view's storage |

Uninstalling leaves these folders in place, so a reinstall picks up where you left off. To remove everything, delete `~/.kotilo/deck/` and the `dev.kotilo.deck` folder after uninstalling. Only delete `~/.kotilo/` itself if you don't use other Kotilo apps.

## Reporting a problem

Settings ▸ About ▸ Open logs folder shows the log files (`deck.log.<date>`). They hold no passwords or tokens, but may contain monitor, workspace and app names; skim them before attaching. Attach today's file to a [bug report](https://github.com/kotilo-dev/kotilo-deck/issues/new?template=bug_report.md).
