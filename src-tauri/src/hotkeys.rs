//! Global shortcuts: settings.hotkey toggles the deck, Ctrl+Alt+←/→ cycle workspaces.
//! A shortcut already taken by another app is logged and skipped, never fatal.

use serde_json::json;
use tauri::{AppHandle, Emitter};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};

use crate::window;

pub const EVENT_WORKSPACE_CYCLE: &str = "workspace://cycle";
const PREV: &str = "Ctrl+Alt+ArrowLeft";
const NEXT: &str = "Ctrl+Alt+ArrowRight";

fn try_register_toggle(app: &AppHandle, accel: &str) -> Result<(), String> {
    app.global_shortcut()
        .on_shortcut(accel, |app, _shortcut, event| {
            if event.state == ShortcutState::Pressed {
                window::toggle(app);
            }
        })
        .map_err(|e| e.to_string())
}

fn register_toggle(app: &AppHandle, accel: &str) {
    if let Err(e) = try_register_toggle(app, accel) {
        tracing::warn!(accel, "cannot register toggle hotkey: {e}");
    }
}

/// True if `accel` parses as a Tauri accelerator.
pub fn is_valid(accel: &str) -> bool {
    accel.parse::<Shortcut>().is_ok()
}

fn register_cycle(app: &AppHandle, accel: &str, dir: i8) {
    let res = app
        .global_shortcut()
        .on_shortcut(accel, move |app, _shortcut, event| {
            if event.state == ShortcutState::Pressed {
                let _ = app.emit(EVENT_WORKSPACE_CYCLE, json!({ "dir": dir }));
            }
        });
    if let Err(e) = res {
        tracing::warn!(accel, "cannot register workspace hotkey: {e}");
    }
}

pub fn setup(app: &AppHandle, toggle_accel: &str) {
    register_toggle(app, toggle_accel);
    register_cycle(app, PREV, -1);
    register_cycle(app, NEXT, 1);
}

/// Swaps the toggle shortcut. If `new` cannot be registered (e.g. taken by another app)
/// the old one is restored and an error is returned so the setting is not persisted.
pub fn rebind_toggle(app: &AppHandle, old: &str, new: &str) -> Result<(), String> {
    if let Err(e) = app.global_shortcut().unregister(old) {
        tracing::debug!(old, "unregister old hotkey: {e}");
    }
    match try_register_toggle(app, new) {
        Ok(()) => Ok(()),
        Err(e) => {
            tracing::warn!(new, "cannot register toggle hotkey: {e}");
            register_toggle(app, old);
            Err(format!("cannot register hotkey {new}: {e}"))
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn accelerators_parse() {
        assert!(super::is_valid(crate::settings::DEFAULT_HOTKEY));
        assert!(super::is_valid(super::PREV));
        assert!(super::is_valid(super::NEXT));
        assert!(!super::is_valid("Ctrl+Nope"));
    }
}
