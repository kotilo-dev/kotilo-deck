pub mod platform;

use platform::Capability;

/// Capabilities the current OS provides; widgets needing a missing one render "Not available on this OS".
#[tauri::command]
fn platform_capabilities() -> Vec<Capability> {
    platform::current().capabilities()
}

pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![platform_capabilities])
        .run(tauri::generate_context!())
        .expect("error while running Kotilo Deck");
}
