pub mod platform;

mod hotkeys;
mod logging;
mod migrations;
mod paths;
mod settings;
mod startup;
mod tray;
mod widgets;
mod window;
mod workspace;

use platform::Capability;
use tauri::webview::PageLoadEvent;
use tauri::Manager;
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
        // Registered only; the toggle UI is Phase 2.
        .plugin(tauri_plugin_autostart::init(
            MacosLauncher::LaunchAgent,
            None,
        ));
    let result = startup::manage(builder)
        .setup(startup::setup)
        .on_page_load(|webview, payload| {
            if payload.event() == PageLoadEvent::Started {
                webview.state::<widgets::system::Sampler>().reset();
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
        ])
        .run(tauri::generate_context!());
    if let Err(e) = result {
        tracing::error!("Kotilo Deck exited with an error: {e}");
        std::process::exit(1);
    }
}
