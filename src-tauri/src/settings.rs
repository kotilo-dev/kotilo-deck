//! Deck settings in tauri-plugin-store `settings.json`, one top-level key per field.
//! Shape mirrors `DeckSettings` in `src/lib/ipc/index.ts`.
//!
//! Corruption handling: every stored top-level key is applied on top of the defaults only
//! if it still deserialises and validates. The nested objects (`appearance`, `burnIn`,
//! `themeAuto`) are checked leaf by leaf, so one bad nested value only resets itself.

use std::collections::HashMap;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_autostart::ManagerExt;
use tauri_plugin_store::StoreExt;

use crate::migrations;

pub const STORE_FILE: &str = "settings.json";
pub const DEFAULT_HOTKEY: &str = "Ctrl+Alt+K";
pub const EVENT_SETTINGS_CHANGED: &str = "settings://changed";

/// Top-level keys holding plain structs: patches merge into them key by key and stored
/// values fall back leaf by leaf. (`activeWorkspace` is a map and is replaced as a whole.)
const NESTED_KEYS: &[&str] = &["appearance", "burnIn", "themeAuto"];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DisplayMode {
    #[default]
    Fullscreen,
    Overlay,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Background {
    #[default]
    Solid,
    Wallpaper,
    Transparent,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeckAppearance {
    pub background: Background,
    pub wallpaper: Option<String>,
    pub surface_opacity: Option<f64>,
    pub blur: f64,
    pub window_opacity: f64,
}

impl Default for DeckAppearance {
    fn default() -> Self {
        Self {
            background: Background::Solid,
            wallpaper: None,
            surface_opacity: None,
            blur: 0.0,
            window_opacity: 1.0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NightDim {
    pub enabled: bool,
    pub from: String,
    pub to: String,
    pub dim: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IdleDim {
    pub enabled: bool,
    pub minutes: u32,
    pub dim: f64,
    pub clock_face: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BurnInSettings {
    pub shift_minutes: u32,
    pub night: NightDim,
    pub idle: IdleDim,
}

impl Default for BurnInSettings {
    fn default() -> Self {
        Self {
            shift_minutes: 10,
            night: NightDim {
                enabled: false,
                from: "22:00".into(),
                to: "07:00".into(),
                dim: 0.5,
            },
            idle: IdleDim {
                enabled: false,
                minutes: 15,
                dim: 0.4,
                clock_face: true,
            },
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ThemeAutoMode {
    #[default]
    Off,
    Os,
    Schedule,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThemeAuto {
    pub mode: ThemeAutoMode,
    pub light_from: String,
    pub dark_from: String,
}

impl Default for ThemeAuto {
    fn default() -> Self {
        Self {
            mode: ThemeAutoMode::Off,
            light_from: "07:00".into(),
            dark_from: "19:00".into(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TemperatureUnit {
    #[default]
    C,
    F,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeckSettings {
    pub schema_version: u32,
    pub theme: Option<String>,
    pub monitor: Option<String>,
    pub active_workspace: HashMap<String, String>,
    pub always_on_top: bool,
    pub hotkey: String,
    // Phase 2 (additive, still schemaVersion 1).
    pub autostart: bool,
    pub display_mode: DisplayMode,
    pub appearance: DeckAppearance,
    pub burn_in: BurnInSettings,
    pub theme_auto: ThemeAuto,
    pub temperature_unit: TemperatureUnit,
    pub notification_sound: bool,
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
            autostart: false,
            display_mode: DisplayMode::Fullscreen,
            appearance: DeckAppearance::default(),
            burn_in: BurnInSettings::default(),
            theme_auto: ThemeAuto::default(),
            temperature_unit: TemperatureUnit::C,
            notification_sound: true,
        }
    }
}

/// "HH:MM", 24 h, zero-padded.
pub fn valid_hhmm(s: &str) -> bool {
    let b = s.as_bytes();
    if b.len() != 5 || b[2] != b':' {
        return false;
    }
    let digits = [b[0], b[1], b[3], b[4]];
    if !digits.iter().all(u8::is_ascii_digit) {
        return false;
    }
    let hh = (b[0] - b'0') * 10 + (b[1] - b'0');
    let mm = (b[3] - b'0') * 10 + (b[4] - b'0');
    hh < 24 && mm < 60
}

/// A bare file name (no directories, no hidden/relative names). Used for wallpapers.
pub fn valid_file_name(s: &str) -> bool {
    const RESERVED: &[char] = &['/', '\\', ':', '*', '?', '"', '<', '>', '|'];
    !s.is_empty()
        && s.len() <= 255
        && !s.starts_with('.')
        && !s.chars().any(|c| RESERVED.contains(&c) || c.is_control())
}

/// Range and format checks serde cannot express.
pub fn validate(s: &DeckSettings) -> Result<(), String> {
    fn range(name: &str, v: f64, lo: f64, hi: f64) -> Result<(), String> {
        if v.is_finite() && (lo..=hi).contains(&v) {
            Ok(())
        } else {
            Err(format!("{name} must be between {lo} and {hi}"))
        }
    }
    fn hhmm(name: &str, v: &str) -> Result<(), String> {
        if valid_hhmm(v) {
            Ok(())
        } else {
            Err(format!("{name} must be \"HH:MM\""))
        }
    }
    if !crate::hotkeys::is_valid(&s.hotkey) {
        return Err(format!("invalid hotkey: {}", s.hotkey));
    }
    let a = &s.appearance;
    if let Some(o) = a.surface_opacity {
        range("appearance.surfaceOpacity", o, 0.0, 1.0)?;
    }
    range("appearance.blur", a.blur, 0.0, 40.0)?;
    range("appearance.windowOpacity", a.window_opacity, 0.2, 1.0)?;
    if let Some(w) = &a.wallpaper {
        if !valid_file_name(w) {
            return Err("appearance.wallpaper must be a file name".into());
        }
    }
    let b = &s.burn_in;
    if b.shift_minutes > 1440 {
        return Err("burnIn.shiftMinutes must be between 0 and 1440".into());
    }
    hhmm("burnIn.night.from", &b.night.from)?;
    hhmm("burnIn.night.to", &b.night.to)?;
    range("burnIn.night.dim", b.night.dim, 0.3, 1.0)?;
    if !(1..=1440).contains(&b.idle.minutes) {
        return Err("burnIn.idle.minutes must be between 1 and 1440".into());
    }
    range("burnIn.idle.dim", b.idle.dim, 0.3, 1.0)?;
    hhmm("themeAuto.lightFrom", &s.theme_auto.light_from)?;
    hhmm("themeAuto.darkFrom", &s.theme_auto.dark_from)?;
    Ok(())
}

pub fn to_map(s: &DeckSettings) -> Map<String, Value> {
    match serde_json::to_value(s) {
        Ok(Value::Object(m)) => m,
        _ => Map::new(),
    }
}

fn is_valid(candidate: &Map<String, Value>) -> bool {
    serde_json::from_value::<DeckSettings>(Value::Object(candidate.clone()))
        .is_ok_and(|s| validate(&s).is_ok())
}

type Leaf = (Vec<String>, Value);

/// Leaves of `stored`, descending only where the default has an object too.
fn leaves(default: &Value, stored: Value, path: Vec<String>, out: &mut Vec<Leaf>) {
    match (default, stored) {
        (Value::Object(d), Value::Object(s)) => {
            for (k, v) in s {
                if let Some(dv) = d.get(&k) {
                    let mut p = path.clone();
                    p.push(k);
                    leaves(dv, v, p, out);
                }
            }
        }
        (_, v) => out.push((path, v)),
    }
}

fn set_path(map: &mut Map<String, Value>, path: &[String], value: Value) {
    let Some((last, parents)) = path.split_last() else {
        return;
    };
    let mut cur = map;
    for p in parents {
        match cur.get_mut(p) {
            Some(Value::Object(m)) => cur = m,
            _ => return,
        }
    }
    cur.insert(last.clone(), value);
}

/// Builds settings from raw store entries. Each stored key (each leaf, for the nested
/// objects) is applied on top of the defaults only if it still deserialises and validates,
/// so one corrupt value never resets the rest.
pub fn from_stored(entries: Map<String, Value>) -> DeckSettings {
    let entries = migrations::migrate_settings(entries);
    let defaults = to_map(&DeckSettings::default());
    let mut merged = defaults.clone();
    for (key, value) in entries {
        if key == "schemaVersion" || !merged.contains_key(&key) {
            continue;
        }
        let mut parts = Vec::new();
        if NESTED_KEYS.contains(&key.as_str()) {
            leaves(&defaults[&key], value, vec![key.clone()], &mut parts);
        } else {
            parts.push((vec![key.clone()], value));
        }
        for (path, value) in parts {
            let mut candidate = merged.clone();
            set_path(&mut candidate, &path, value);
            if is_valid(&candidate) {
                merged = candidate;
            } else {
                tracing::warn!(key = path.join("."), "ignoring invalid stored setting");
            }
        }
    }
    serde_json::from_value(Value::Object(merged)).unwrap_or_default()
}

/// Recursive object merge (patch wins; non-objects replace).
fn deep_merge(base: &mut Value, patch: &Value) {
    match (base, patch) {
        (Value::Object(b), Value::Object(p)) => {
            for (k, v) in p {
                match b.get_mut(k) {
                    Some(bv) if bv.is_object() && v.is_object() => deep_merge(bv, v),
                    _ => {
                        b.insert(k.clone(), v.clone());
                    }
                }
            }
        }
        (b, p) => *b = p.clone(),
    }
}

/// Shallow merge of `patch` into `base` (the nested struct keys merge deeper, so a partial
/// `burnIn` patch keeps the other burn-in fields; a full object behaves exactly like a
/// shallow replace). Unknown keys and `schemaVersion` are ignored; a known key with a
/// wrong type or out-of-range value rejects the whole patch.
pub fn merge(base: &DeckSettings, patch: &Map<String, Value>) -> Result<DeckSettings, String> {
    let mut merged = to_map(base);
    for (key, value) in patch {
        if key == "schemaVersion" {
            continue;
        }
        let Some(current) = merged.get_mut(key) else {
            tracing::debug!(key, "ignoring unknown setting");
            continue;
        };
        if NESTED_KEYS.contains(&key.as_str()) && value.is_object() {
            deep_merge(current, value);
        } else {
            *current = value.clone();
        }
    }
    let merged: DeckSettings = serde_json::from_value(Value::Object(merged))
        .map_err(|e| format!("invalid settings: {e}"))?;
    validate(&merged)?;
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

/// Tells every window (deck + settings) the current settings.
pub fn broadcast(app: &AppHandle, settings: &DeckSettings) {
    if let Err(e) = app.emit(EVENT_SETTINGS_CHANGED, settings) {
        tracing::warn!("cannot emit settings change: {e}");
    }
}

fn set_autostart(app: &AppHandle, on: bool) -> Result<(), String> {
    let launcher = app.autolaunch();
    let res = if on {
        launcher.enable()
    } else {
        launcher.disable()
    };
    res.map_err(|e| {
        tracing::warn!(on, "cannot change autostart: {e}");
        format!("cannot change start-with-OS: {e}")
    })
}

/// Startup: re-register autostart when enabled so the entry follows the current exe path.
pub fn sync_autostart(app: &AppHandle, settings: &DeckSettings) {
    if settings.autostart {
        let _ = set_autostart(app, true);
    }
}

/// `settings_set` without the IPC wrapper. Side effects that can fail (hotkey, autostart)
/// are applied before saving and rolled back if saving fails; then window side effects are
/// applied and `settings://changed` is broadcast.
pub fn apply_patch(app: &AppHandle, patch: &Map<String, Value>) -> Result<DeckSettings, String> {
    let before = load(app);
    let wanted = merge(&before, patch)?;
    let hotkey_changed = wanted.hotkey != before.hotkey;
    let autostart_changed = wanted.autostart != before.autostart;
    if hotkey_changed {
        crate::hotkeys::rebind_toggle(app, &before.hotkey, &wanted.hotkey)?;
    }
    let rollback = |autostart: bool| {
        if hotkey_changed {
            let _ = crate::hotkeys::rebind_toggle(app, &wanted.hotkey, &before.hotkey);
        }
        if autostart {
            let _ = set_autostart(app, before.autostart);
        }
    };
    if autostart_changed {
        if let Err(e) = set_autostart(app, wanted.autostart) {
            rollback(false);
            return Err(e);
        }
    }
    let (old, new) = match update(app, patch) {
        Ok(v) => v,
        Err(e) => {
            rollback(autostart_changed);
            return Err(e);
        }
    };
    if old.monitor != new.monitor || old.display_mode != new.display_mode {
        crate::window::attach(app);
    } else if old.always_on_top != new.always_on_top
        || old.appearance.window_opacity != new.appearance.window_opacity
    {
        crate::window::apply_window_flags(app, &new);
    }
    broadcast(app, &new);
    Ok(new)
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
    apply_patch(&app, &patch)
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
                "activeWorkspace": {}, "alwaysOnTop": false, "hotkey": "Ctrl+Alt+K",
                "autostart": false,
                "displayMode": "fullscreen",
                "appearance": {
                    "background": "solid", "wallpaper": null, "surfaceOpacity": null,
                    "blur": 0.0, "windowOpacity": 1.0
                },
                "burnIn": {
                    "shiftMinutes": 10,
                    "night": {"enabled": false, "from": "22:00", "to": "07:00", "dim": 0.5},
                    "idle": {"enabled": false, "minutes": 15, "dim": 0.4, "clockFace": true}
                },
                "themeAuto": {"mode": "off", "lightFrom": "07:00", "darkFrom": "19:00"},
                "temperatureUnit": "c",
                "notificationSound": true
            })
        );
        assert!(validate(&DeckSettings::default()).is_ok());
    }

    #[test]
    fn phase1_store_gets_phase2_defaults() {
        let s = from_stored(obj(
            json!({"schemaVersion": 1, "theme": "nord", "hotkey": "Ctrl+Alt+D"}),
        ));
        assert_eq!(s.theme.as_deref(), Some("nord"));
        assert_eq!(s.hotkey, "Ctrl+Alt+D");
        assert_eq!(s.display_mode, DisplayMode::Fullscreen);
        assert_eq!(s.burn_in, BurnInSettings::default());
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
        // Maps are replaced, not merged, so entries can be removed.
        let s3 = merge(&s2, &obj(json!({"activeWorkspace": {}}))).unwrap();
        assert!(s3.active_workspace.is_empty());
    }

    #[test]
    fn nested_patches_merge_deeper() {
        let s = merge(
            &DeckSettings::default(),
            &obj(json!({"burnIn": {"night": {"enabled": true}}, "appearance": {"blur": 12}})),
        )
        .unwrap();
        assert!(s.burn_in.night.enabled);
        assert_eq!(s.burn_in.night.from, "22:00");
        assert_eq!(s.burn_in.shift_minutes, 10);
        assert_eq!(s.appearance.blur, 12.0);
        assert_eq!(s.appearance.window_opacity, 1.0);
    }

    #[test]
    fn merge_rejects_wrong_types_and_ranges() {
        let d = DeckSettings::default();
        for bad in [
            json!({"alwaysOnTop": "yes"}),
            json!({"hotkey": "Ctrl+Nope"}),
            json!({"displayMode": "window"}),
            json!({"appearance": {"windowOpacity": 0.1}}),
            json!({"appearance": {"surfaceOpacity": 1.5}}),
            json!({"appearance": {"blur": 41}}),
            json!({"appearance": {"background": "video"}}),
            json!({"appearance": {"wallpaper": "../x.png"}}),
            json!({"burnIn": {"night": {"dim": 0.2}}}),
            json!({"burnIn": {"idle": {"minutes": 0}}}),
            json!({"burnIn": {"shiftMinutes": -1}}),
            json!({"burnIn": {"night": {"from": "24:00"}}}),
            json!({"themeAuto": {"lightFrom": "7:00"}}),
            json!({"themeAuto": {"mode": "auto"}}),
            json!({"temperatureUnit": "k"}),
            json!({"burnIn": 5}),
        ] {
            assert!(merge(&d, &obj(bad.clone())).is_err(), "{bad}");
        }
        let ok = merge(
            &d,
            &obj(json!({"appearance": {"windowOpacity": 0.2, "surfaceOpacity": 0, "wallpaper": "a b.webm"}, "displayMode": "overlay"})),
        )
        .unwrap();
        assert_eq!(ok.display_mode, DisplayMode::Overlay);
        assert_eq!(ok.appearance.surface_opacity, Some(0.0));
    }

    #[test]
    fn stored_corrupt_key_falls_back_to_default() {
        let s = from_stored(obj(json!({"hotkey": 42, "theme": "dracula"})));
        assert_eq!(s.hotkey, DEFAULT_HOTKEY);
        assert_eq!(s.theme.as_deref(), Some("dracula"));
    }

    #[test]
    fn stored_corrupt_nested_value_falls_back_alone() {
        let s = from_stored(obj(json!({
            "burnIn": {
                "shiftMinutes": 5,
                "night": {"enabled": true, "from": "nope", "to": "06:30", "dim": 0.1},
                "idle": "garbage"
            },
            "appearance": "garbage",
            "themeAuto": {"mode": "schedule", "lightFrom": "06:00", "darkFrom": 7},
            "notificationSound": false
        })));
        assert_eq!(s.burn_in.shift_minutes, 5);
        assert!(s.burn_in.night.enabled);
        assert_eq!(s.burn_in.night.from, "22:00");
        assert_eq!(s.burn_in.night.to, "06:30");
        assert_eq!(s.burn_in.night.dim, 0.5);
        assert_eq!(s.burn_in.idle, BurnInSettings::default().idle);
        assert_eq!(s.appearance, DeckAppearance::default());
        assert_eq!(s.theme_auto.mode, ThemeAutoMode::Schedule);
        assert_eq!(s.theme_auto.light_from, "06:00");
        assert_eq!(s.theme_auto.dark_from, "19:00");
        assert!(!s.notification_sound);
    }

    #[test]
    fn hhmm_and_file_names() {
        for ok in ["00:00", "23:59", "07:05"] {
            assert!(valid_hhmm(ok), "{ok}");
        }
        for bad in [
            "24:00", "12:60", "7:00", "07:0", "ab:cd", "07-00", "", "007:00",
        ] {
            assert!(!valid_hhmm(bad), "{bad}");
        }
        assert!(valid_file_name("mountains.jpg"));
        for bad in ["", ".hidden", "a/b.png", "a\\b.png", "C:x.png", ".."] {
            assert!(!valid_file_name(bad), "{bad}");
        }
    }
}
