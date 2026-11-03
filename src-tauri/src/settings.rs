//! Deck settings in tauri-plugin-store `settings.json`, one top-level key per field.
//! Shape mirrors `DeckSettings` in `src/lib/ipc/index.ts`.

use std::collections::HashMap;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use tauri::{AppHandle, Manager};
use tauri_plugin_store::StoreExt;

use crate::migrations;

pub const STORE_FILE: &str = "settings.json";
pub const DEFAULT_HOTKEY: &str = "Ctrl+Alt+K";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeckSettings {
    pub schema_version: u32,
    pub theme: Option<String>,
    pub monitor: Option<String>,
    pub active_workspace: HashMap<String, String>,
    pub always_on_top: bool,
    pub hotkey: String,
}

impl Default for DeckSettings {
    fn default() -> Self {
        Self {
            schema_version: migrations::SETTINGS_VERSION,
            theme: None,
            monitor: None,
            active_workspace: HashMap::new(),
            always_on_top: false,
            hotkey: DEFAULT_HOTKEY.to_string(),
        }
    }
}

fn to_map(s: &DeckSettings) -> Map<String, Value> {
    match serde_json::to_value(s) {
        Ok(Value::Object(m)) => m,
        _ => Map::new(),
    }
}

/// Builds settings from raw store entries. Each stored key is applied on top of the
/// defaults only if it still deserialises, so one corrupt value never resets the rest.
pub fn from_stored(entries: Map<String, Value>) -> DeckSettings {
    let entries = migrations::migrate_settings(entries);
    let mut merged = to_map(&DeckSettings::default());
    for (key, value) in entries {
        if key == "schemaVersion" || !merged.contains_key(&key) {
            continue;
        }
        let mut candidate = merged.clone();
        candidate.insert(key.clone(), value);
        if serde_json::from_value::<DeckSettings>(Value::Object(candidate.clone())).is_ok() {
            merged = candidate;
        } else {
            tracing::warn!(key, "ignoring invalid stored setting");
        }
    }
    serde_json::from_value(Value::Object(merged)).unwrap_or_default()
}

/// Shallow merge of `patch` into `base`. Unknown keys and `schemaVersion` are ignored;
/// a known key with the wrong type rejects the whole patch.
pub fn merge(base: &DeckSettings, patch: &Map<String, Value>) -> Result<DeckSettings, String> {
    let mut merged = to_map(base);
    for (key, value) in patch {
        if key == "schemaVersion" {
            continue;
        }
        if !merged.contains_key(key) {
            tracing::debug!(key, "ignoring unknown setting");
            continue;
        }
        merged.insert(key.clone(), value.clone());
    }
    let merged: DeckSettings = serde_json::from_value(Value::Object(merged))
        .map_err(|e| format!("invalid settings: {e}"))?;
    if !crate::hotkeys::is_valid(&merged.hotkey) {
        return Err(format!("invalid hotkey: {}", merged.hotkey));
    }
    Ok(merged)
}

/// Serialises read-modify-write cycles on the store.
#[derive(Default)]
pub struct SettingsLock(Mutex<()>);

pub fn load(app: &AppHandle) -> DeckSettings {
    match app.store(STORE_FILE) {
        Ok(store) => from_stored(store.entries().into_iter().collect()),
        Err(e) => {
            tracing::warn!("settings store unavailable, using defaults: {e}");
            DeckSettings::default()
        }
    }
}

/// Merges, persists and returns `(old, new)`. Side effects are the caller's job.
pub fn update(
    app: &AppHandle,
    patch: &Map<String, Value>,
) -> Result<(DeckSettings, DeckSettings), String> {
    let lock = app.state::<SettingsLock>();
    let _guard = lock.0.lock().unwrap_or_else(|p| p.into_inner());
    let store = app.store(STORE_FILE).map_err(|e| e.to_string())?;
    let old = from_stored(store.entries().into_iter().collect());
    let new = merge(&old, patch)?;
    let map = to_map(&new);
    for (key, value) in map {
        store.set(key, value);
    }
    store.save().map_err(|e| e.to_string())?;
    Ok((old, new))
}

#[tauri::command]
pub async fn settings_get(app: AppHandle) -> Result<DeckSettings, String> {
    Ok(load(&app))
}

#[tauri::command]
pub async fn settings_set(
    app: AppHandle,
    patch: Map<String, Value>,
) -> Result<DeckSettings, String> {
    // Bind a changed hotkey first so a taken/invalid one is rejected before it is saved.
    let before = load(&app);
    let wanted = merge(&before, &patch)?;
    if wanted.hotkey != before.hotkey {
        crate::hotkeys::rebind_toggle(&app, &before.hotkey, &wanted.hotkey)?;
    }
    let (old, new) = match update(&app, &patch) {
        Ok(v) => v,
        Err(e) => {
            if wanted.hotkey != before.hotkey {
                let _ = crate::hotkeys::rebind_toggle(&app, &wanted.hotkey, &before.hotkey);
            }
            return Err(e);
        }
    };
    if old.always_on_top != new.always_on_top {
        crate::window::apply_always_on_top(&app, new.always_on_top);
    }
    if old.monitor != new.monitor {
        crate::window::attach(&app);
    }
    Ok(new)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn obj(v: Value) -> Map<String, Value> {
        v.as_object().cloned().unwrap()
    }

    #[test]
    fn defaults_match_contract() {
        let v = serde_json::to_value(DeckSettings::default()).unwrap();
        assert_eq!(
            v,
            json!({
                "schemaVersion": 1, "theme": null, "monitor": null,
                "activeWorkspace": {}, "alwaysOnTop": false, "hotkey": "Ctrl+Alt+K"
            })
        );
    }

    #[test]
    fn merge_is_shallow_and_ignores_unknown() {
        let base = DeckSettings::default();
        let s = merge(
            &base,
            &obj(json!({"theme": "nord", "bogus": 1, "schemaVersion": 9})),
        )
        .unwrap();
        assert_eq!(s.theme.as_deref(), Some("nord"));
        assert_eq!(s.schema_version, 1);
        assert_eq!(s.hotkey, DEFAULT_HOTKEY);

        let s2 = merge(&s, &obj(json!({"activeWorkspace": {"DISPLAY2": "ops"}}))).unwrap();
        assert_eq!(s2.active_workspace.get("DISPLAY2").unwrap(), "ops");
        assert_eq!(s2.theme.as_deref(), Some("nord"));
    }

    #[test]
    fn merge_rejects_wrong_types() {
        assert!(merge(
            &DeckSettings::default(),
            &obj(json!({"alwaysOnTop": "yes"}))
        )
        .is_err());
        assert!(merge(
            &DeckSettings::default(),
            &obj(json!({"hotkey": "Ctrl+Nope"}))
        )
        .is_err());
    }

    #[test]
    fn stored_corrupt_key_falls_back_to_default() {
        let s = from_stored(obj(json!({"hotkey": 42, "theme": "dracula"})));
        assert_eq!(s.hotkey, DEFAULT_HOTKEY);
        assert_eq!(s.theme.as_deref(), Some("dracula"));
    }
}
