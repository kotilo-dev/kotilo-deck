//! App setup: managed state, tray, hotkeys, initial placement and background watchers.

use tauri::{App, WindowEvent};

use crate::{hotkeys, paths, settings, tray, widgets, window, workspace};

/// Registers managed state. Called on the builder before `setup`.
pub fn manage(builder: tauri::Builder<tauri::Wry>) -> tauri::Builder<tauri::Wry> {
    builder
        .manage(settings::SettingsLock::default())
        .manage(workspace::WorkspaceStore::new(paths::workspaces_dir()))
        .manage(window::Placement::default())
        .manage(tray::TrayState::default())
        .manage(widgets::system::Sampler::default())
        .manage(widgets::audio::AudioHub::default())
        .manage(widgets::media::MediaHub::default())
        .manage(widgets::weather::Weather::new(paths::weather_cache_dir()))
}

pub fn setup(app: &mut App) -> Result<(), Box<dyn std::error::Error>> {
    let handle = app.handle().clone();
    let s = settings::load(&handle);

    if let Err(e) = tray::setup(&handle) {
        tracing::warn!("tray icon unavailable: {e}");
    }
    window::apply_always_on_top(&handle, s.always_on_top);
    window::attach(&handle);
    window::show(&handle);
    hotkeys::setup(&handle, &s.hotkey);
    window::spawn_watcher(handle.clone());

    // Closing the deck hides it to the tray; "Quit" lives in the tray menu.
    if let Some(win) = window::deck(&handle) {
        let w = win.clone();
        win.on_window_event(move |event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = w.hide();
            }
        });
    }
    Ok(())
}
