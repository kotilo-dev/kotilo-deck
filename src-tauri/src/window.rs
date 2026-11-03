//! Deck window placement: borderless fullscreen on the chosen monitor, remembered by
//! name, re-attached when it reconnects. Uses Tauri's portable window/monitor APIs only.

use std::sync::Mutex;
use std::time::Duration;

use serde_json::{json, Map};
use tauri::{AppHandle, Emitter, Manager, Monitor, PhysicalPosition, Size, WebviewWindow};

use crate::platform::MonitorInfo;

pub const DECK_LABEL: &str = "deck";
pub const EVENT_MONITOR_CHANGED: &str = "monitor://changed";
/// Monitor hot-plug check. Cheap (one EnumDisplayMonitors), so 5 s keeps re-attach snappy.
const WATCH_INTERVAL: Duration = Duration::from_secs(5);

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

pub fn deck(app: &AppHandle) -> Option<WebviewWindow> {
    app.get_webview_window(DECK_LABEL)
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

fn place(win: &WebviewWindow, monitor: &Monitor) -> tauri::Result<()> {
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

/// Moves the deck onto the target monitor (per settings) and emits `monitor://changed`
/// when the placement differs from the previous one.
pub fn attach(app: &AppHandle) {
    let Some(win) = deck(app) else {
        return;
    };
    let saved = crate::settings::load(app).monitor;
    let list = monitors(app);
    let infos: Vec<MonitorInfo> = list.iter().map(|(_, i)| i.clone()).collect();
    let Some(idx) = pick_target(&infos, saved.as_deref()) else {
        tracing::warn!("no monitors reported; leaving deck where it is");
        return;
    };
    let (monitor, info) = &list[idx];
    if let Err(e) = place(&win, monitor) {
        tracing::warn!("cannot place deck window: {e}");
        return;
    }
    if app.state::<Placement>().set(info.clone()) {
        tracing::info!(primary = info.primary, "deck attached to monitor");
        let _ = app.emit(EVENT_MONITOR_CHANGED, info.clone());
        crate::tray::refresh(app);
    }
}

/// Re-attaches when the wanted monitor (re)appears, the current one disappears, or its
/// resolution/scale changes.
pub fn spawn_watcher(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        let mut tick = tokio::time::interval(WATCH_INTERVAL);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        tick.tick().await;
        loop {
            tick.tick().await;
            let saved = crate::settings::load(&app).monitor;
            let infos: Vec<MonitorInfo> = monitors(&app).into_iter().map(|(_, i)| i).collect();
            let Some(idx) = pick_target(&infos, saved.as_deref()) else {
                continue;
            };
            let placed = app.state::<Placement>().current();
            let on_monitor = deck(&app)
                .and_then(|w| w.current_monitor().ok().flatten())
                .and_then(|m| m.name().cloned());
            let drifted = on_monitor.is_some_and(|n| n != infos[idx].name);
            if placed.as_ref() != Some(&infos[idx]) || drifted {
                tracing::debug!("monitor layout changed; re-attaching deck");
                attach(&app);
            }
        }
    });
}

pub fn apply_always_on_top(app: &AppHandle, on: bool) {
    if let Some(win) = deck(app) {
        if let Err(e) = win.set_always_on_top(on) {
            tracing::warn!("cannot set always-on-top: {e}");
        }
    }
}

pub fn show(app: &AppHandle) {
    if let Some(win) = deck(app) {
        let _ = win.show();
        let _ = win.unminimize();
        let _ = win.set_focus();
    }
}

pub fn toggle(app: &AppHandle) {
    let Some(win) = deck(app) else {
        return;
    };
    let visible = win.is_visible().unwrap_or(false) && !win.is_minimized().unwrap_or(false);
    if visible {
        let _ = win.hide();
    } else {
        show(app);
    }
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
    crate::settings::update(app, &patch)?;
    attach(app);
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
}
