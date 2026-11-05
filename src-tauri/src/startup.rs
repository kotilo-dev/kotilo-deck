//! App setup: managed state, tray, hotkeys, initial placement and background tasks; and
//! the orderly quit path.

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use serde_json::json;
use tauri::{App, AppHandle, Emitter, Manager};

use crate::{companion, files, hotkeys, paths, settings, tray, widgets, window, workspace};

pub const EVENT_QUITTING: &str = "deck://quitting";
/// Time the webviews get after `deck://quitting` to flush pending saves.
const QUIT_GRACE: Duration = Duration::from_millis(500);

/// Registers managed state. Called on the builder before `setup`.
pub fn manage(builder: tauri::Builder<tauri::Wry>) -> tauri::Builder<tauri::Wry> {
    builder
        .manage(settings::SettingsLock::default())
        .manage(workspace::WorkspaceStore::new(paths::workspaces_dir()))
        .manage(window::Placement::default())
        .manage(window::DisplayState::new(paths::window_state_file()))
        .manage(tray::TrayState::default())
        .manage(widgets::system::Sampler::default())
        .manage(widgets::audio::AudioHub::default())
        .manage(widgets::media::MediaHub::default())
        .manage(widgets::weather::Weather::new(paths::weather_cache_dir()))
        .manage(widgets::todo::Todos::open(&paths::database_file()))
        .manage(files::ThemeWatcher::default())
        .manage(widgets::calendar::Calendar::new(
            widgets::calendar::cache_dir(),
        ))
        .manage(companion::Companion::default())
}

pub fn setup(app: &mut App) -> Result<(), Box<dyn std::error::Error>> {
    let handle = app.handle().clone();
    let s = settings::load(&handle);

    if let Err(e) = tray::setup(&handle) {
        tracing::warn!("tray icon unavailable: {e}");
    }
    if !window::attach(&handle) {
        window::apply_window_flags(&handle, &s);
    }
    window::show(&handle);
    hotkeys::setup(&handle, &s.hotkey);
    window::spawn_watcher(handle.clone());
    settings::sync_autostart(&handle, &s);
    widgets::todo::spawn_scheduler(handle.clone());
    // Pomodoro needs the stored cycle settings to restore its timer, so it is managed here.
    handle.manage(widgets::pomodoro::Pomodoro::open(
        &paths::database_file(),
        &s.pomodoro,
    ));
    widgets::pomodoro::spawn_scheduler(handle.clone());
    companion::setup(&handle, s.companion.enabled);
    files::watch_user_themes(&handle);
    widgets::calendar::start(&handle);
    // Phase 4
    crate::lan::start(&handle, &s.lan);
    crate::plugins::start(&handle);
    crate::update::start(&handle, &s.updates);

    // Closing the deck hides it to the tray ("Quit" lives in the tray menu); overlay
    // geometry is remembered.
    if let Some(win) = window::deck(&handle) {
        let app = handle.clone();
        win.on_window_event(move |event| window::on_deck_event(&app, event));
    }
    Ok(())
}

static QUITTING: AtomicBool = AtomicBool::new(false);

/// Tray "Quit": hides the windows at once, tells the webviews (`deck://quitting`) so they
/// can flush debounced saves, and exits after [`QUIT_GRACE`].
pub fn quit(app: &AppHandle) {
    if QUITTING.swap(true, Ordering::AcqRel) {
        return;
    }
    tracing::info!("quit requested");
    let _ = app.emit(EVENT_QUITTING, json!({}));
    // Closes the desktop connection with 1001 during the grace period.
    companion::shutdown(app);
    crate::lan::shutdown(app);
    window::hide(app);
    if let Some(w) = app.get_webview_window(crate::settings_window::SETTINGS_LABEL) {
        let _ = w.hide();
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(QUIT_GRACE).await;
        app.exit(0);
    });
}

/// `RunEvent::Exit`: persist window state and flush the log writer (the process ends
/// without running destructors).
pub fn on_exit(app: &AppHandle) {
    app.state::<window::DisplayState>().flush();
    companion::shutdown(app);
    crate::lan::shutdown(app);
    crate::crash::mark_clean_exit();
    tracing::info!("Kotilo Deck exiting");
    crate::logging::flush();
}
