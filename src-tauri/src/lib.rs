pub mod platform;

mod files;
mod hotkeys;
mod logging;
mod migrations;
mod paths;
mod settings;
mod settings_window;
mod startup;
mod transfer;
mod tray;
mod widgets;
mod window;
mod workspace;

use platform::Capability;
use tauri::webview::PageLoadEvent;
use tauri::{Manager, RunEvent};
use tauri_plugin_autostart::MacosLauncher;

/// Capabilities the current OS provides; widgets needing a missing one render "Not available on this OS".
#[tauri::command]
fn platform_capabilities() -> Vec<Capability> {
    platform::current().capabilities()
}

pub fn run() {
    logging::init();
    let builder = tauri::Builder::default()
        // Must be first: a second launch focuses the running deck and exits.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            window::show(app)
        }))
        .plugin(tauri_plugin_store::Builder::default().build())
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_autostart::init(
            MacosLauncher::LaunchAgent,
            None,
        ))
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_dialog::init());
    let app = startup::manage(builder)
        .setup(startup::setup)
        .on_page_load(|webview, payload| {
            // Ref-counted subscriptions belong to the deck page; the settings window
            // loading must not reset them.
            if payload.event() == PageLoadEvent::Started && webview.label() == window::DECK_LABEL {
                webview.state::<widgets::system::Sampler>().reset();
                webview.state::<widgets::audio::AudioHub>().reset();
                webview.state::<widgets::media::MediaHub>().reset();
            }
        })
        .invoke_handler(tauri::generate_handler![
            platform_capabilities,
            settings::settings_get,
            settings::settings_set,
            workspace::workspace_list,
            workspace::workspace_save,
            workspace::workspace_delete,
            window::monitor_list,
            window::monitor_select,
            tray::tray_set_workspaces,
            widgets::system::system_info,
            widgets::system::system_subscribe,
            widgets::system::system_unsubscribe,
            widgets::system::system_top_processes,
            widgets::weather::weather_geocode,
            widgets::weather::weather_get,
            // Phase 2 core
            settings_window::settings_open,
            transfer::settings_export_all,
            transfer::settings_import_all,
            files::wallpaper_list,
            files::wallpaper_import,
            files::theme_user_list,
            files::file_save_as,
            files::file_open_text,
            widgets::todo::todo_list,
            widgets::todo::todo_add,
            widgets::todo::todo_update,
            widgets::todo::todo_delete,
            widgets::todo::todo_reorder,
            widgets::todo::todo_snooze,
            widgets::audio::audio_state,
            widgets::audio::audio_set_volume,
            widgets::audio::audio_set_muted,
            widgets::audio::audio_set_default_device,
            widgets::audio::audio_sessions,
            widgets::audio::audio_set_session_volume,
            widgets::audio::audio_set_session_muted,
            widgets::audio::audio_subscribe,
            widgets::audio::audio_unsubscribe,
            widgets::media::media_state,
            widgets::media::media_control,
            widgets::media::media_seek,
            widgets::media::media_subscribe,
            widgets::media::media_unsubscribe,
            // Phase 3 calendar
            widgets::calendar::calendar_upcoming,
            widgets::calendar::calendar_refresh,
        ])
        .build(tauri::generate_context!());
    match app {
        Ok(app) => app.run(|handle, event| {
            if let RunEvent::Exit = event {
                startup::on_exit(handle);
            }
        }),
        Err(e) => {
            tracing::error!("Kotilo Deck failed to start: {e}");
            logging::flush();
            std::process::exit(1);
        }
    }
}
