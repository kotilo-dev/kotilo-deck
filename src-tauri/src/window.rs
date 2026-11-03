//! Deck window placement on the chosen monitor, remembered by name, re-attached when it
//! reconnects. Two display modes (SPEC §5.3):
//! - fullscreen: borderless fullscreen on the target monitor (Phase 1 behaviour);
//! - overlay: a normal-sized, always-on-top borderless window whose size/position is kept in
//!   `~/.kotilo/deck/window-state.json` (not a user setting, so not in settings.json), with
//!   optional whole-window opacity via [`Platform::set_window_opacity`].
//!
//! Every show/hide path goes through [`show`] / [`hide`] so `deck://visibility` is emitted.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::{json, Map};
use tauri::{
    AppHandle, Emitter, LogicalSize, Manager, Monitor, PhysicalPosition, PhysicalSize, Size,
    WebviewWindow, WindowEvent,
};

use crate::platform::{self, MonitorInfo, NativeWindow, PlatformError};
use crate::settings::{DeckSettings, DisplayMode};

pub const DECK_LABEL: &str = "deck";
pub const EVENT_MONITOR_CHANGED: &str = "monitor://changed";
pub const EVENT_VISIBILITY: &str = "deck://visibility";
/// Monitor hot-plug check. Cheap (one EnumDisplayMonitors), so 5 s keeps re-attach snappy.
const WATCH_INTERVAL: Duration = Duration::from_secs(5);
/// Longest wait between placement retries after repeated failures.
const MAX_BACKOFF: Duration = Duration::from_secs(300);
/// Overlay geometry is written this long after the last move/resize.
const SAVE_DEBOUNCE: Duration = Duration::from_millis(1000);
const OVERLAY_MIN: LogicalSize<f64> = LogicalSize {
    width: 480.0,
    height: 300.0,
};
const WINDOW_STATE_VERSION: u32 = 1;

/// Monitor the deck was last placed on.
#[derive(Default)]
pub struct Placement(Mutex<Option<MonitorInfo>>);

impl Placement {
    pub fn current(&self) -> Option<MonitorInfo> {
        self.0.lock().unwrap_or_else(|p| p.into_inner()).clone()
    }
    fn set(&self, info: MonitorInfo) -> bool {
        let mut cur = self.0.lock().unwrap_or_else(|p| p.into_inner());
        let changed = cur.as_ref() != Some(&info);
        *cur = Some(info);
        changed
    }
}

/// Overlay window geometry in physical px, plus the monitor it was on.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OverlayRect {
    pub monitor: String,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WindowStateFile {
    schema_version: u32,
    overlay: Option<OverlayRect>,
}

/// Current display mode flags (read from window events without touching the store) and
/// the remembered overlay geometry.
pub struct DisplayState {
    file: PathBuf,
    overlay: AtomicBool,
    /// `f64` bits of the opacity to (re-)apply in overlay mode.
    opacity: AtomicU64,
    rect: Mutex<Option<OverlayRect>>,
    dirty: AtomicBool,
    generation: AtomicU64,
}

impl DisplayState {
    pub fn new(file: PathBuf) -> Self {
        let rect = read_state(&file);
        Self {
            file,
            overlay: AtomicBool::new(false),
            opacity: AtomicU64::new(1.0f64.to_bits()),
            rect: Mutex::new(rect),
            dirty: AtomicBool::new(false),
            generation: AtomicU64::new(0),
        }
    }

    fn rect(&self) -> Option<OverlayRect> {
        self.rect.lock().unwrap_or_else(|p| p.into_inner()).clone()
    }

    fn record(&self, rect: OverlayRect) -> bool {
        let mut cur = self.rect.lock().unwrap_or_else(|p| p.into_inner());
        if cur.as_ref() == Some(&rect) {
            return false;
        }
        *cur = Some(rect);
        self.dirty.store(true, Ordering::Release);
        true
    }

    /// Writes the overlay geometry if it changed since the last write.
    pub fn flush(&self) {
        if !self.dirty.swap(false, Ordering::AcqRel) {
            return;
        }
        let doc = WindowStateFile {
            schema_version: WINDOW_STATE_VERSION,
            overlay: self.rect(),
        };
        let write = || -> std::io::Result<()> {
            if let Some(dir) = self.file.parent() {
                std::fs::create_dir_all(dir)?;
            }
            let bytes = serde_json::to_vec_pretty(&doc).unwrap_or_default();
            crate::workspace::write_atomic(&self.file, &bytes)
        };
        if let Err(e) = write() {
            tracing::warn!(file = %crate::paths::file_label(&self.file), "cannot save window state: {e}");
        }
    }
}

fn read_state(file: &Path) -> Option<OverlayRect> {
    let bytes = std::fs::read(file).ok()?;
    match serde_json::from_slice::<WindowStateFile>(&bytes) {
        Ok(s) if s.schema_version == WINDOW_STATE_VERSION => s.overlay,
        Ok(_) | Err(_) => {
            tracing::warn!(file = %crate::paths::file_label(file), "ignoring unreadable window state");
            None
        }
    }
}

pub fn deck(app: &AppHandle) -> Option<WebviewWindow> {
    app.get_webview_window(DECK_LABEL)
}

/// Native handle for the platform layer (Tauri API; the platform module interprets it).
fn native(win: &WebviewWindow) -> Option<NativeWindow> {
    #[cfg(target_os = "windows")]
    {
        win.hwnd().ok().map(|h| NativeWindow(h.0 as isize))
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = win;
        None
    }
}

fn monitors(app: &AppHandle) -> Vec<(Monitor, MonitorInfo)> {
    let Some(win) = deck(app) else {
        return Vec::new();
    };
    let primary = win
        .primary_monitor()
        .ok()
        .flatten()
        .and_then(|m| m.name().cloned());
    let list = win.available_monitors().unwrap_or_else(|e| {
        tracing::warn!("cannot enumerate monitors: {e}");
        Vec::new()
    });
    list.into_iter()
        .enumerate()
        .map(|(i, m)| {
            let name = m.name().cloned().unwrap_or_else(|| format!("monitor-{i}"));
            let info = MonitorInfo {
                primary: primary.as_deref() == Some(name.as_str()),
                name,
                width: m.size().width,
                height: m.size().height,
                scale_factor: m.scale_factor(),
            };
            (m, info)
        })
        .collect()
}

/// Saved monitor if connected, else first non-primary, else primary, else first.
pub fn pick_target(monitors: &[MonitorInfo], saved: Option<&str>) -> Option<usize> {
    saved
        .and_then(|name| monitors.iter().position(|m| m.name == name))
        .or_else(|| monitors.iter().position(|m| !m.primary))
        .or_else(|| monitors.iter().position(|m| m.primary))
        .or(if monitors.is_empty() { None } else { Some(0) })
}

/// Monitor rectangle in physical px: (x, y, width, height).
type Rect = (i32, i32, u32, u32);

fn monitor_rect(m: &Monitor) -> Rect {
    (
        m.position().x,
        m.position().y,
        m.size().width,
        m.size().height,
    )
}

/// 60 % of the monitor, centred.
pub fn default_overlay(monitor: Rect) -> (i32, i32, u32, u32) {
    let (mx, my, mw, mh) = monitor;
    let w = (f64::from(mw) * 0.6).round() as u32;
    let h = (f64::from(mh) * 0.6).round() as u32;
    (mx + ((mw - w) / 2) as i32, my + ((mh - h) / 2) as i32, w, h)
}

/// A remembered rect is reused when its centre is on `monitor`; it is shrunk to fit.
pub fn fit_overlay(rect: &OverlayRect, monitor: Rect) -> Option<(i32, i32, u32, u32)> {
    let (mx, my, mw, mh) = monitor;
    let cx = i64::from(rect.x) + i64::from(rect.width) / 2;
    let cy = i64::from(rect.y) + i64::from(rect.height) / 2;
    let inside = (i64::from(mx)..i64::from(mx) + i64::from(mw)).contains(&cx)
        && (i64::from(my)..i64::from(my) + i64::from(mh)).contains(&cy);
    if !inside || rect.width < 200 || rect.height < 150 {
        return None;
    }
    Some((rect.x, rect.y, rect.width.min(mw), rect.height.min(mh)))
}

fn place_fullscreen(win: &WebviewWindow, monitor: &Monitor) -> tauri::Result<()> {
    if win.is_fullscreen().unwrap_or(false) {
        win.set_fullscreen(false)?;
    }
    // Small decks (7" 1024×600, portrait) must not be clamped by the config's min size.
    win.set_min_size(None::<Size>)?;
    let pos = monitor.position();
    win.set_position(PhysicalPosition::new(pos.x, pos.y))?;
    win.set_size(*monitor.size())?;
    win.set_fullscreen(true)
}

fn place_overlay(win: &WebviewWindow, geometry: (i32, i32, u32, u32)) -> tauri::Result<()> {
    let (x, y, w, h) = geometry;
    if win.is_fullscreen().unwrap_or(false) {
        win.set_fullscreen(false)?;
    }
    win.set_min_size(Some(Size::Logical(OVERLAY_MIN)))?;
    win.set_position(PhysicalPosition::new(x, y))?;
    win.set_size(PhysicalSize::new(w, h))
}

/// Applies the display mode on the target monitor (per settings) and emits
/// `monitor://changed` when the placement differs from the previous one.
/// Returns false when the window could not be placed.
pub fn attach(app: &AppHandle) -> bool {
    let Some(win) = deck(app) else {
        return false;
    };
    let settings = crate::settings::load(app);
    let overlay = settings.display_mode == DisplayMode::Overlay;
    let display = app.state::<DisplayState>();
    display.overlay.store(overlay, Ordering::Release);
    let list = monitors(app);
    let infos: Vec<MonitorInfo> = list.iter().map(|(_, i)| i.clone()).collect();
    let Some(target) = pick_target(&infos, settings.monitor.as_deref()) else {
        tracing::warn!("no monitors reported; leaving deck where it is");
        return false;
    };

    let (idx, result) = if overlay {
        // Read the remembered rect before un-fullscreening fires move/resize events.
        let remembered = display.rect().and_then(|r| {
            let i = list.iter().position(|(_, info)| info.name == r.monitor)?;
            Some((i, fit_overlay(&r, monitor_rect(&list[i].0))?))
        });
        let (idx, geometry) =
            remembered.unwrap_or_else(|| (target, default_overlay(monitor_rect(&list[target].0))));
        (idx, place_overlay(&win, geometry))
    } else {
        (target, place_fullscreen(&win, &list[target].0))
    };
    if let Err(e) = result {
        tracing::warn!(overlay, "cannot place deck window: {e}");
        return false;
    }
    apply_window_flags(app, &settings);
    let info = &list[idx].1;
    if app.state::<Placement>().set(info.clone()) {
        tracing::info!(primary = info.primary, overlay, "deck attached to monitor");
        let _ = app.emit(EVENT_MONITOR_CHANGED, info.clone());
        crate::tray::refresh(app);
    }
    true
}

/// Retry delay after `failures` consecutive placement failures: 10 s, 20 s, … ≤ 5 min.
pub fn backoff(failures: u32) -> Duration {
    WATCH_INTERVAL
        .saturating_mul(2u32.saturating_pow(failures.min(16)))
        .min(MAX_BACKOFF)
}

/// Re-attaches when the wanted monitor (re)appears, the current one disappears, or its
/// resolution/scale changes. In overlay mode only a monitor layout change re-attaches (the
/// user may move the overlay anywhere). Placement failures back off exponentially instead
/// of retrying (and toggling fullscreen) every tick; a layout change resets the backoff.
pub fn spawn_watcher(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        let mut tick = tokio::time::interval(WATCH_INTERVAL);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        tick.tick().await;
        let mut layout: Vec<MonitorInfo> = monitors(&app).into_iter().map(|(_, i)| i).collect();
        let mut failures = 0u32;
        let mut retry_at: Option<Instant> = None;
        loop {
            tick.tick().await;
            let infos: Vec<MonitorInfo> = monitors(&app).into_iter().map(|(_, i)| i).collect();
            let layout_changed = infos != layout;
            if layout_changed {
                layout = infos.clone();
                failures = 0;
                retry_at = None;
            }
            if retry_at.is_some_and(|t| Instant::now() < t) {
                continue;
            }
            let overlay = app.state::<DisplayState>().overlay.load(Ordering::Acquire);
            let wanted = if overlay {
                layout_changed || failures > 0
            } else {
                let saved = crate::settings::load(&app).monitor;
                let Some(idx) = pick_target(&infos, saved.as_deref()) else {
                    continue;
                };
                let placed = app.state::<Placement>().current();
                let on_monitor = deck(&app)
                    .and_then(|w| w.current_monitor().ok().flatten())
                    .and_then(|m| m.name().cloned());
                let drifted = on_monitor.is_some_and(|n| n != infos[idx].name);
                placed.as_ref() != Some(&infos[idx]) || drifted
            };
            if !wanted {
                continue;
            }
            tracing::debug!("monitor layout changed; re-attaching deck");
            if attach(&app) {
                failures = 0;
                retry_at = None;
            } else {
                failures = failures.saturating_add(1);
                let wait = backoff(failures);
                tracing::warn!(
                    failures,
                    wait_s = wait.as_secs(),
                    "deck placement failed; backing off"
                );
                retry_at = Some(Instant::now() + wait);
            }
        }
    });
}

/// Always-on-top (forced in overlay mode) and overlay window opacity.
pub fn apply_window_flags(app: &AppHandle, settings: &DeckSettings) {
    let Some(win) = deck(app) else {
        return;
    };
    let overlay = settings.display_mode == DisplayMode::Overlay;
    if let Err(e) = win.set_always_on_top(settings.always_on_top || overlay) {
        tracing::warn!("cannot set always-on-top: {e}");
    }
    let opacity = if overlay {
        settings.appearance.window_opacity
    } else {
        1.0
    };
    let display = app.state::<DisplayState>();
    display.overlay.store(overlay, Ordering::Release);
    display.opacity.store(opacity.to_bits(), Ordering::Release);
    reapply_opacity(app);
}

/// The window toolkit rewrites the extended window style on show/hide, fullscreen and
/// always-on-top changes, which drops the layered style; call this after those.
fn reapply_opacity(app: &AppHandle) {
    let Some(win) = deck(app) else {
        return;
    };
    let opacity = f64::from_bits(app.state::<DisplayState>().opacity.load(Ordering::Acquire));
    let Some(handle) = native(&win) else {
        return;
    };
    match platform::current().set_window_opacity(handle, opacity) {
        Ok(()) => {}
        Err(PlatformError::Unsupported(_)) if opacity >= 1.0 => {}
        Err(e) => tracing::warn!("cannot set window opacity: {e}"),
    }
}

fn emit_visibility(app: &AppHandle, visible: bool) {
    let _ = app.emit(EVENT_VISIBILITY, json!({ "visible": visible }));
}

pub fn show(app: &AppHandle) {
    if let Some(win) = deck(app) {
        let _ = win.show();
        let _ = win.unminimize();
        let _ = win.set_focus();
        reapply_opacity(app);
        emit_visibility(app, true);
    }
}

pub fn hide(app: &AppHandle) {
    if let Some(win) = deck(app) {
        let _ = win.hide();
        emit_visibility(app, false);
    }
}

pub fn toggle(app: &AppHandle) {
    let Some(win) = deck(app) else {
        return;
    };
    let visible = win.is_visible().unwrap_or(false) && !win.is_minimized().unwrap_or(false);
    if visible {
        hide(app);
    } else {
        show(app);
    }
}

/// Deck window events: close hides to the tray; overlay moves/resizes are remembered.
pub fn on_deck_event(app: &AppHandle, event: &WindowEvent) {
    match event {
        WindowEvent::CloseRequested { api, .. } => {
            api.prevent_close();
            hide(app);
        }
        WindowEvent::Moved(_) | WindowEvent::Resized(_) => remember_overlay(app),
        WindowEvent::Focused(true)
            if app.state::<DisplayState>().overlay.load(Ordering::Acquire) =>
        {
            reapply_opacity(app)
        }
        _ => {}
    }
}

fn remember_overlay(app: &AppHandle) {
    let display = app.state::<DisplayState>();
    if !display.overlay.load(Ordering::Acquire) {
        return;
    }
    let Some(win) = deck(app) else {
        return;
    };
    if win.is_fullscreen().unwrap_or(false)
        || win.is_minimized().unwrap_or(false)
        || !win.is_visible().unwrap_or(false)
    {
        return;
    }
    let (Ok(pos), Ok(size), Ok(Some(monitor))) = (
        win.outer_position(),
        win.inner_size(),
        win.current_monitor(),
    ) else {
        return;
    };
    let Some(name) = monitor.name().cloned() else {
        return;
    };
    let rect = OverlayRect {
        monitor: name,
        x: pos.x,
        y: pos.y,
        width: size.width,
        height: size.height,
    };
    if !display.record(rect) {
        return;
    }
    let generation = display.generation.fetch_add(1, Ordering::AcqRel) + 1;
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(SAVE_DEBOUNCE).await;
        let display = app.state::<DisplayState>();
        if display.generation.load(Ordering::Acquire) != generation {
            return;
        }
        display.flush();
        // The overlay may have been dragged to another monitor.
        let Some(rect) = display.rect() else {
            return;
        };
        let info = monitors(&app)
            .into_iter()
            .map(|(_, i)| i)
            .find(|i| i.name == rect.monitor);
        if let Some(info) = info {
            if app.state::<Placement>().set(info.clone()) {
                let _ = app.emit(EVENT_MONITOR_CHANGED, info);
                crate::tray::refresh(&app);
            }
        }
    });
}

#[tauri::command]
pub async fn monitor_list(app: AppHandle) -> Result<Vec<MonitorInfo>, String> {
    Ok(monitors(&app).into_iter().map(|(_, i)| i).collect())
}

#[tauri::command]
pub async fn monitor_select(app: AppHandle, name: String) -> Result<(), String> {
    select(&app, &name)
}

pub fn select(app: &AppHandle, name: &str) -> Result<(), String> {
    if !monitors(app).iter().any(|(_, i)| i.name == name) {
        return Err("monitor is not connected".into());
    }
    let mut patch = Map::new();
    patch.insert("monitor".into(), json!(name));
    let (_, new) = crate::settings::update(app, &patch)?;
    // An explicit choice moves the overlay too, even if it was remembered elsewhere.
    let display = app.state::<DisplayState>();
    let moved_away = display.rect().is_some_and(|r| r.monitor != name);
    if moved_away {
        *display.rect.lock().unwrap_or_else(|p| p.into_inner()) = None;
        display.dirty.store(true, Ordering::Release);
        display.flush();
    }
    attach(app);
    crate::settings::broadcast(app, &new);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(name: &str, primary: bool) -> MonitorInfo {
        MonitorInfo {
            name: name.into(),
            width: 1920,
            height: 1080,
            scale_factor: 1.0,
            primary,
        }
    }

    #[test]
    fn target_selection() {
        let both = [m("A", true), m("B", false)];
        assert_eq!(pick_target(&both, Some("A")), Some(0));
        assert_eq!(pick_target(&both, None), Some(1));
        assert_eq!(pick_target(&both, Some("gone")), Some(1));
        assert_eq!(pick_target(&[m("A", true)], Some("B")), Some(0));
        assert_eq!(pick_target(&[m("X", false)], None), Some(0));
        assert_eq!(pick_target(&[], None), None);
    }

    #[test]
    fn overlay_geometry() {
        let mon = (1920, 0, 1000, 500);
        assert_eq!(default_overlay(mon), (1920 + 200, 100, 600, 300));
        let r = |x, y, w, h| OverlayRect {
            monitor: "B".into(),
            x,
            y,
            width: w,
            height: h,
        };
        assert_eq!(
            fit_overlay(&r(2000, 50, 400, 300), mon),
            Some((2000, 50, 400, 300))
        );
        // Centre off the monitor → not reused.
        assert_eq!(fit_overlay(&r(0, 0, 400, 300), mon), None);
        // Too large → shrunk to the monitor.
        assert_eq!(
            fit_overlay(&r(1900, -10, 1200, 600), mon),
            Some((1900, -10, 1000, 500))
        );
        assert_eq!(fit_overlay(&r(2000, 50, 100, 100), mon), None);
    }

    #[test]
    fn backoff_grows_and_caps() {
        assert_eq!(backoff(1), Duration::from_secs(10));
        assert_eq!(backoff(2), Duration::from_secs(20));
        assert_eq!(backoff(10), MAX_BACKOFF);
        assert_eq!(backoff(u32::MAX), MAX_BACKOFF);
    }

    #[test]
    fn window_state_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("window-state.json");
        let s = DisplayState::new(file.clone());
        assert!(s.rect().is_none());
        let rect = OverlayRect {
            monitor: r"\\.\DISPLAY2".into(),
            x: -5,
            y: 10,
            width: 800,
            height: 600,
        };
        assert!(s.record(rect.clone()));
        assert!(!s.record(rect.clone()));
        s.flush();
        assert_eq!(DisplayState::new(file.clone()).rect(), Some(rect));
        std::fs::write(&file, b"{\"schemaVersion\":9,\"overlay\":null}").unwrap();
        assert!(DisplayState::new(file).rect().is_none());
    }
}
