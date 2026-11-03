//! The separate settings window (SPEC §7, DESIGN §7): label `settings`, 960×680, route
//! `/settings` of the same SPA. Borderless because the page draws its own 44 px title bar
//! (drag region + minimize/close via the core window permissions in capabilities/).
//! Single instance: opening again focuses the existing window.

use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};

pub const SETTINGS_LABEL: &str = "settings";

pub fn open(app: &AppHandle) -> Result<(), String> {
    if let Some(win) = app.get_webview_window(SETTINGS_LABEL) {
        let _ = win.unminimize();
        let _ = win.show();
        let _ = win.set_focus();
        return Ok(());
    }
    // An always-on-top deck on the same monitor would otherwise cover the settings window.
    let s = crate::settings::load(app);
    let on_top = s.always_on_top || s.display_mode == crate::settings::DisplayMode::Overlay;
    WebviewWindowBuilder::new(app, SETTINGS_LABEL, WebviewUrl::App("settings".into()))
        .title("Kotilo Deck — Settings")
        .inner_size(960.0, 680.0)
        .min_inner_size(720.0, 520.0)
        .decorations(false)
        .resizable(true)
        .always_on_top(on_top)
        .center()
        .focused(true)
        .build()
        .map(|_| ())
        .map_err(|e| {
            tracing::warn!("cannot open settings window: {e}");
            format!("cannot open settings window: {e}")
        })
}

/// Async so the window is not created on the main thread from inside an IPC handler
/// (that deadlocks on Windows).
#[tauri::command]
pub async fn settings_open(app: AppHandle) -> Result<(), String> {
    open(&app)
}
