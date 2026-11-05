# Linux test plan (v1.0.0)

The Linux platform layer was written without a Linux machine (DECISIONS 59). CI compiles it and runs its unit tests on Ubuntu, but nobody has run the app on a Linux desktop yet. This checklist is for the first real run. It takes about 30–45 minutes per distribution.

Write the result next to each item: ✅ works, ⚠️ works with problems (say what), ❌ broken. At the end, collect the files listed in [What to send back](#what-to-send-back).

## 0. Before you start

Note down:

```sh
cat /etc/os-release | head -3          # distribution and version
echo $XDG_SESSION_TYPE                 # wayland or x11
echo $XDG_CURRENT_DESKTOP              # GNOME, KDE, ...
pactl info | grep -E "Server Name|Default Sink"   # PulseAudio or PipeWire (pipewire-pulse)
which pactl wpctl playerctl xdg-open xdg-mime
xrandr --listmonitors 2>/dev/null || true
```

If `pactl` is missing: Ubuntu/Debian `sudo apt install pulseaudio-utils`; Fedora `sudo dnf install pulseaudio-utils`. The volume widget needs it.

Good combinations to cover: **Ubuntu 24.04 (GNOME, Wayland, PipeWire)** first, then if possible a KDE or X11 session (Fedora KDE, Kubuntu, or "Ubuntu on Xorg" from the login screen).

## 1. Install

Downloads are on https://github.com/kotilo-dev/kotilo-deck/releases/tag/v1.0.0

### 1a. AppImage (test this first)

```sh
cd ~/Downloads
chmod +x Kotilo.Deck_1.0.0_amd64.AppImage
./Kotilo.Deck_1.0.0_amd64.AppImage --verbose
```

- [ ] The window opens; no crash in the terminal output.
- [ ] If it fails to start with a FUSE error: `sudo apt install libfuse2t64` (Ubuntu 24.04) or `libfuse2` (older), then retry.

### 1b. .deb (Ubuntu/Debian) — do this after the AppImage, with the AppImage closed

```sh
sudo apt install ./Kotilo.Deck_1.0.0_amd64.deb
kotilo-deck --verbose
```

- [ ] Installs without dependency errors.
- [ ] "Kotilo Deck" appears in the app launcher with its icon and starts from there.

### 1c. .rpm (Fedora)

```sh
sudo dnf install ./Kotilo.Deck-1.0.0-1.x86_64.rpm
kotilo-deck --verbose
```

- [ ] Installs and starts.

## 2. First run

Start from a clean state so onboarding shows (only on a machine where the deck never ran):

```sh
ls ~/.kotilo/deck ~/.local/share/dev.kotilo.deck 2>/dev/null   # should not exist yet
```

- [ ] Onboarding appears: Welcome → (Monitor, only with 2+ screens) → Theme → Workspace → Done.
- [ ] Monitor step: picking a screen moves the deck there, fullscreen.
- [ ] Theme step: the deck changes theme live.
- [ ] Workspace step: the chosen preset is active after "Open deck".
- [ ] Time from first launch to a working deck: ____ min (target < 2 min).

## 3. Window, tray, hotkeys

- [ ] The deck is borderless fullscreen on the chosen monitor (Settings ▸ Display & monitor).
- [ ] Tray icon appears. GNOME needs the "AppIndicator and KStatusNotifierItem Support" extension (Ubuntu has it by default). Menu: show/hide, workspaces, monitor, quit.
- [ ] `Ctrl+Alt+K` toggles the deck, `Ctrl+Alt+←/→` switches workspaces. (On Wayland, global shortcuts may not work at all — note ❌ and the session type; that's useful to know.)
- [ ] Unplug/replug the secondary monitor (if you have one): the deck comes back on it.
- [ ] Settings window opens (`Ctrl+,` or tray ▸ Settings).

## 4. Widgets

Add each to a workspace (E → drawer) if the preset doesn't have it.

| Widget | What to check | Result |
|---|---|---|
| Clock / Date | Local time and date correct, time zone right | |
| Weather | City search works, forecast loads | |
| System | CPU sparkline moves, memory right; GPU shows on AMD (sysfs), missing on NVIDIA/Intel is expected; top-5 process sheet opens | |
| Volume | Master slider changes system volume (and vice versa: change volume from the desktop, the widget follows within ~1 s); mute; output device list matches `pactl list short sinks`; switching output works; per-app mixer lists an app playing sound (Firefox/Spotify) | |
| Media | Play something in **Firefox** (YouTube) and **Spotify** if installed: title/artist/artwork show, play/pause/next/prev work, seek works, the widget switches to whichever player last started playing | |
| Todo | Add `call mom tomorrow 10:00`; set a reminder 1–2 min ahead, hide the deck to the tray → an OS notification fires | |
| Pomodoro | Start a 1-minute focus (Settings ▸ Pomodoro), hide the deck → notification at the end | |
| Calendar | Add any public `https://…ics` feed in Settings → events show | |
| Agents | Shows "desktop not running" without crashing | |

## 5. Settings ▸ About and support links

- [ ] Version shows 1.0.0.
- [ ] Updates: **AppImage** → "Check now" says you're up to date. **.deb/.rpm** → updates are shown as not available with a "use your package manager" hint (that's intended).
- [ ] "Open logs folder" opens `~/.kotilo/deck/logs/` in the file manager.
- [ ] "Report an issue" opens a prefilled GitHub issue in the browser (don't submit it — just check it opens and contains version/OS, no paths).
- [ ] Settings ▸ Widgets ▸ "Open plugins folder" opens `~/.kotilo/deck/widgets/` (this goes through `xdg-open`).

## 6. Tablet / browser mode (optional, needs a phone or tablet on the same Wi-Fi)

- [ ] Settings ▸ Tablet & browser ▸ enable → Show → scan the QR code → the deck appears on the phone.
- [ ] If not: is a firewall on? (`sudo ufw status` on Ubuntu, `firewall-cmd --state` on Fedora). Port 47832/tcp.

## 7. Plugins

```sh
mkdir -p ~/.kotilo/deck/widgets
# copy examples/widgets/hello from the repo into ~/.kotilo/deck/widgets/hello
```

- [ ] "Hello" appears in the widget drawer without a restart, and renders (theme colours, size).

## 8. Theme and appearance

- [ ] Switch 3–4 themes, including a light one: instant, no unreadable text.
- [ ] Settings ▸ Appearance ▸ wallpaper image: shows. Transparent background: note what happens (compositor-dependent).
- [ ] Same workspace file looks like on Windows: copy a file from Windows `%USERPROFILE%\.kotilo\deck\workspaces\` into `~/.kotilo/deck/workspaces/` (SPEC Phase 4 acceptance: "renders identically").

## 9. Quit and restart

- [ ] Tray ▸ Quit exits (no process left: `pgrep -a kotilo`).
- [ ] Restart: same workspace, theme and monitor; onboarding doesn't show again.
- [ ] Optional autostart: Settings ▸ General ▸ start with the OS → log out/in → deck starts.

## What to send back

1. This file with the results filled in (or just the ❌/⚠️ items).
2. The outputs from step 0.
3. The logs: `~/.kotilo/deck/logs/` (the `deck.log.*` file of the test day, plus any `crash-*.log`).
4. Terminal output of the `--verbose` run if something failed.
5. Screenshots of anything that looks wrong.

## Uninstall afterwards (optional)

```sh
sudo apt remove kotilo-deck        # .deb  (Fedora: sudo dnf remove kotilo-deck)
rm ~/Downloads/Kotilo.Deck_1.0.0_amd64.AppImage
rm -rf ~/.kotilo/deck ~/.local/share/dev.kotilo.deck ~/.config/dev.kotilo.deck   # all deck data
```
