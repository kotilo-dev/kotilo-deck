//! Deck settings in tauri-plugin-store `settings.json`, one top-level key per field.
//! Shape mirrors `DeckSettings` in `src/lib/ipc/index.ts`.
//!
//! Corruption handling: every stored top-level key is applied on top of the defaults only
//! if it still deserialises and validates. The nested objects (`appearance`, `burnIn`,
//! `themeAuto`, `companion`, `pomodoro`) are checked leaf by leaf, so one bad nested value
//! only resets itself. Arrays (`calendarFeeds`, `autoSwitch`) are replaced on patch and
//! fall back as a whole.

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
const NESTED_KEYS: &[&str] = &[
    "appearance",
    "burnIn",
    "themeAuto",
    "companion",
    "pomodoro",
    "lan",
    "updates",
    "onboarding",
];

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

/// Companion server (protocol.md).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompanionSettings {
    pub enabled: bool,
    pub allow_workspace_switch: bool,
    pub notify_approvals: bool,
}

impl Default for CompanionSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            allow_workspace_switch: false,
            notify_approvals: true,
        }
    }
}

/// Phase 4 tablet/browser mode: the deck UI served on the LAN (`crate::lan`). Off by default;
/// every request needs the companion token.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LanSettings {
    pub enabled: bool,
    /// 1024–65535, never the companion port 47831.
    pub port: u16,
}

pub const DEFAULT_LAN_PORT: u16 = 47832;

impl Default for LanSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            port: DEFAULT_LAN_PORT,
        }
    }
}

/// Phase 4 auto-update (`crate::update`). Checks never install without a click.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateSettings {
    /// Check at startup and every 24 h.
    pub auto_check: bool,
}

impl Default for UpdateSettings {
    fn default() -> Self {
        Self { auto_check: true }
    }
}

/// Phase 5 first-run onboarding. A fresh install (no settings file) starts at
/// [`Default`] (`completed: false`); a settings file written before this key existed loads
/// as [`OnboardingSettings::UPGRADED`] so upgrading users never see onboarding.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OnboardingSettings {
    pub completed: bool,
    /// Onboarding flow version the user finished (0 = none).
    pub version: u32,
}

/// Upper bound for `onboarding.version` (sanity check, flows are numbered 1, 2, ...).
pub const MAX_ONBOARDING_VERSION: u32 = 1000;

impl OnboardingSettings {
    /// Settings files from before Phase 5: treated as done, without claiming a flow version.
    pub const UPGRADED: Self = Self {
        completed: true,
        version: 0,
    };
}

/// Lengths in minutes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PomodoroSettings {
    pub focus: u32,
    pub short_break: u32,
    pub long_break: u32,
    pub sessions_per_cycle: u32,
    pub auto_start_next: bool,
    pub daily_goal: u32,
}

impl Default for PomodoroSettings {
    fn default() -> Self {
        Self {
            focus: 25,
            short_break: 5,
            long_break: 15,
            sessions_per_cycle: 4,
            auto_start_next: false,
            daily_goal: 8,
        }
    }
}

/// Theme token names (never a hex).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum FeedColor {
    Accent,
    #[serde(rename = "accent-2")]
    Accent2,
    Success,
    Warning,
    Danger,
    Info,
}

/// The URL may carry a secret: never log it (`Debug` omits it).
#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CalendarFeed {
    pub id: String,
    pub name: String,
    pub url: String,
    pub color: FeedColor,
    pub enabled: bool,
}

impl std::fmt::Debug for CalendarFeed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CalendarFeed")
            .field("id", &self.id)
            .field("color", &self.color)
            .field("enabled", &self.enabled)
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase"
)]
pub enum AutoSwitchTrigger {
    /// `at` "HH:MM"; `days` 0 = Sun … 6 = Sat, empty = every day.
    Time {
        at: String,
        days: Vec<u8>,
    },
    PomodoroStart,
    PomodoroEnd,
    /// `None` = any feed.
    CalendarStart {
        feed_id: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AutoSwitchRule {
    pub id: String,
    pub enabled: bool,
    pub trigger: AutoSwitchTrigger,
    pub workspace: String,
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
    // Phase 3 (additive, still schemaVersion 1).
    pub companion: CompanionSettings,
    pub pomodoro: PomodoroSettings,
    pub calendar_feeds: Vec<CalendarFeed>,
    pub auto_switch: Vec<AutoSwitchRule>,
    // Phase 4 (additive, still schemaVersion 1).
    pub lan: LanSettings,
    pub updates: UpdateSettings,
    // Phase 5 (additive, still schemaVersion 1).
    pub onboarding: OnboardingSettings,
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
            companion: CompanionSettings::default(),
            pomodoro: PomodoroSettings::default(),
            calendar_feeds: Vec::new(),
            auto_switch: Vec::new(),
            lan: LanSettings::default(),
            updates: UpdateSettings::default(),
            onboarding: OnboardingSettings::default(),
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

/// Calendar feed URL: `https://` or `webcal://` (any case) with a host, no whitespace.
pub fn valid_feed_url(url: &str) -> bool {
    let lower = url.get(..9).unwrap_or(url).to_ascii_lowercase();
    let rest = if lower.starts_with("https://") {
        &url[8..]
    } else if lower.starts_with("webcal://") {
        &url[9..]
    } else {
        return false;
    };
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    let host = authority.rsplit('@').next().unwrap_or("");
    url.len() <= 2048
        && !host.is_empty()
        && !url.chars().any(|c| c.is_whitespace() || c.is_control())
}

fn validate_phase3(s: &DeckSettings) -> Result<(), String> {
    let p = &s.pomodoro;
    for (name, v, lo, hi) in [
        ("pomodoro.focus", p.focus, 1, 180),
        ("pomodoro.shortBreak", p.short_break, 1, 60),
        ("pomodoro.longBreak", p.long_break, 1, 120),
        ("pomodoro.sessionsPerCycle", p.sessions_per_cycle, 1, 12),
        ("pomodoro.dailyGoal", p.daily_goal, 1, 24),
    ] {
        if !(lo..=hi).contains(&v) {
            return Err(format!("{name} must be between {lo} and {hi}"));
        }
    }
    let mut ids = std::collections::HashSet::new();
    for (i, f) in s.calendar_feeds.iter().enumerate() {
        if !crate::workspace::valid_id(&f.id) {
            return Err(format!("calendarFeeds[{i}].id must be a slug [a-z0-9-]"));
        }
        if !ids.insert(f.id.as_str()) {
            return Err(format!("calendarFeeds[{i}].id is a duplicate"));
        }
        if f.name.chars().count() > 200 {
            return Err(format!("calendarFeeds[{i}].name is too long"));
        }
        if !valid_feed_url(&f.url) {
            // The URL itself is never echoed (it may carry a secret).
            return Err(format!(
                "calendarFeeds[{i}].url must start with https:// or webcal://"
            ));
        }
    }
    let mut ids = std::collections::HashSet::new();
    for (i, r) in s.auto_switch.iter().enumerate() {
        if r.id.is_empty() || r.id.chars().count() > 64 {
            return Err(format!("autoSwitch[{i}].id must be 1-64 characters"));
        }
        if !ids.insert(r.id.as_str()) {
            return Err(format!("autoSwitch[{i}].id is a duplicate"));
        }
        if !crate::workspace::valid_id(&r.workspace) {
            return Err(format!("autoSwitch[{i}].workspace must be a workspace id"));
        }
        match &r.trigger {
            AutoSwitchTrigger::Time { at, days } => {
                if !valid_hhmm(at) {
                    return Err(format!("autoSwitch[{i}].trigger.at must be \"HH:MM\""));
                }
                if days.iter().any(|d| *d > 6) {
                    return Err(format!("autoSwitch[{i}].trigger.days must be 0-6"));
                }
            }
            AutoSwitchTrigger::CalendarStart { feed_id: Some(id) } => {
                if !crate::workspace::valid_id(id) {
                    return Err(format!("autoSwitch[{i}].trigger.feedId must be a feed id"));
                }
            }
            AutoSwitchTrigger::PomodoroStart
            | AutoSwitchTrigger::PomodoroEnd
            | AutoSwitchTrigger::CalendarStart { feed_id: None } => {}
        }
    }
    validate_phase4(s)
}

fn validate_phase4(s: &DeckSettings) -> Result<(), String> {
    let port = s.lan.port;
    if port < 1024 || port == crate::companion::PORT {
        return Err(format!(
            "lan.port must be between 1024 and 65535 and not {}",
            crate::companion::PORT
        ));
    }
    if s.onboarding.version > MAX_ONBOARDING_VERSION {
        return Err(format!(
            "onboarding.version must be between 0 and {MAX_ONBOARDING_VERSION}"
        ));
    }
    Ok(())
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
    validate_phase3(s)
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

/// [`from_stored_on_disk`] without a settings file on disk (tests).
#[cfg(test)]
pub fn from_stored(entries: Map<String, Value>) -> DeckSettings {
    from_stored_on_disk(entries, false)
}

/// Builds settings from raw store entries. Each stored key (each leaf, for the nested
/// objects) is applied on top of the defaults only if it still deserialises and validates,
/// so one corrupt value never resets the rest.
///
/// An empty store with no `settings.json` on disk is a fresh install. A non-empty store
/// without `onboarding` (or with a corrupt one) was written by an earlier version, so
/// onboarding starts as completed. So does an existing file that loaded as an empty store:
/// the store plugin silently treats an unreadable (corrupt) file that way, and that is not
/// a fresh install.
pub fn from_stored_on_disk(entries: Map<String, Value>, file_exists: bool) -> DeckSettings {
    let existing = file_exists || !entries.is_empty();
    let entries = migrations::migrate_settings(entries);
    let mut defaults = to_map(&DeckSettings::default());
    if existing {
        if let Ok(v) = serde_json::to_value(OnboardingSettings::UPGRADED) {
            defaults.insert("onboarding".into(), v);
        }
    }
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

/// Whether `settings.json` is on disk (readable or not).
fn store_file_exists(app: &AppHandle) -> bool {
    tauri_plugin_store::resolve_store_path(app, STORE_FILE).is_ok_and(|p| p.is_file())
}

pub fn load(app: &AppHandle) -> DeckSettings {
    let on_disk = store_file_exists(app);
    match app.store(STORE_FILE) {
        Ok(store) => {
            let entries: Map<String, Value> = store.entries().into_iter().collect();
            if on_disk && entries.is_empty() {
                tracing::warn!("settings file is unreadable, using defaults");
            }
            from_stored_on_disk(entries, on_disk)
        }
        Err(e) => {
            tracing::warn!("settings store unavailable, using defaults: {e}");
            from_stored_on_disk(Map::new(), on_disk)
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
    let old = from_stored_on_disk(
        store.entries().into_iter().collect(),
        store_file_exists(app),
    );
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
    if old.companion.enabled != new.companion.enabled {
        crate::companion::set_enabled(app, new.companion.enabled);
    }
    if old.lan != new.lan {
        crate::lan::settings_changed(app, &new.lan);
    }
    if old.updates != new.updates {
        crate::update::settings_changed(app, &new.updates);
    }
    if old.pomodoro != new.pomodoro {
        crate::widgets::pomodoro::settings_changed(app, &new.pomodoro);
    }
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
                "notificationSound": true,
                "companion": {"enabled": true, "allowWorkspaceSwitch": false, "notifyApprovals": true},
                "pomodoro": {
                    "focus": 25, "shortBreak": 5, "longBreak": 15, "sessionsPerCycle": 4,
                    "autoStartNext": false, "dailyGoal": 8
                },
                "calendarFeeds": [],
                "autoSwitch": [],
                "lan": {"enabled": false, "port": 47832},
                "updates": {"autoCheck": true},
                "onboarding": {"completed": false, "version": 0}
            })
        );
        assert!(validate(&DeckSettings::default()).is_ok());
    }

    #[test]
    fn fresh_install_starts_onboarding() {
        let s = from_stored(Map::new());
        assert_eq!(s.onboarding, OnboardingSettings::default());
        assert!(!s.onboarding.completed);
        assert_eq!(s.onboarding.version, 0);
    }

    #[test]
    fn pre_phase5_store_skips_onboarding() {
        let s = from_stored(obj(json!({"schemaVersion": 1, "theme": "nord"})));
        assert_eq!(s.onboarding, OnboardingSettings::UPGRADED);
        assert_eq!(s.theme.as_deref(), Some("nord"));
        // A corrupt value in an existing store also counts as completed.
        for bad in [json!("garbage"), json!({"completed": "no", "version": -1})] {
            let s = from_stored(obj(json!({"theme": "nord", "onboarding": bad.clone()})));
            assert!(s.onboarding.completed, "{bad}");
        }
    }

    #[test]
    fn unreadable_settings_file_skips_onboarding() {
        // A corrupt settings.json loads as an empty store: not a fresh install.
        let s = from_stored_on_disk(Map::new(), true);
        assert_eq!(s.onboarding, OnboardingSettings::UPGRADED);
        assert_eq!(s.theme, DeckSettings::default().theme);
        // No file and no entries: fresh install.
        assert_eq!(
            from_stored_on_disk(Map::new(), false).onboarding,
            OnboardingSettings::default()
        );
        // A stored onboarding value still wins.
        let pending = obj(json!({"onboarding": {"completed": false, "version": 0}}));
        assert!(!from_stored_on_disk(pending, true).onboarding.completed);
    }

    #[test]
    fn onboarding_round_trips_and_validates() {
        // Fresh install, user finishes onboarding, the store is written and read back.
        let fresh = from_stored(Map::new());
        let done = merge(
            &fresh,
            &obj(json!({"onboarding": {"completed": true, "version": 1}})),
        )
        .unwrap();
        assert_eq!(
            done.onboarding,
            OnboardingSettings {
                completed: true,
                version: 1
            }
        );
        assert_eq!(from_stored(to_map(&done)).onboarding, done.onboarding);
        // A store written while onboarding was still pending keeps it pending.
        let pending = from_stored(to_map(&fresh));
        assert!(!pending.onboarding.completed);
        // Partial patch merges into the nested object.
        let partial = merge(&fresh, &obj(json!({"onboarding": {"completed": true}}))).unwrap();
        assert!(partial.onboarding.completed);
        assert_eq!(partial.onboarding.version, 0);
        for bad in [
            json!({"onboarding": {"completed": "yes"}}),
            json!({"onboarding": {"version": -1}}),
            json!({"onboarding": {"version": 1.5}}),
            json!({"onboarding": {"version": 1001}}),
            json!({"onboarding": true}),
        ] {
            assert!(merge(&fresh, &obj(bad.clone())).is_err(), "{bad}");
        }
    }

    #[test]
    fn lan_port_is_validated() {
        let base = DeckSettings::default();
        for bad in [80, 1023, 47831] {
            assert!(
                merge(&base, &obj(json!({"lan": {"port": bad}}))).is_err(),
                "{bad}"
            );
        }
        let s = merge(&base, &obj(json!({"lan": {"enabled": true}}))).unwrap();
        assert!(s.lan.enabled);
        assert_eq!(s.lan.port, DEFAULT_LAN_PORT);
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

    fn feed(id: &str, url: &str) -> Value {
        json!({"id": id, "name": "Work", "url": url, "color": "accent-2", "enabled": true})
    }

    #[test]
    fn phase3_partial_patches_and_ranges() {
        let d = DeckSettings::default();
        let s = merge(
            &d,
            &obj(json!({"pomodoro": {"focus": 50}, "companion": {"allowWorkspaceSwitch": true}})),
        )
        .unwrap();
        assert_eq!(s.pomodoro.focus, 50);
        assert_eq!(s.pomodoro.short_break, 5);
        assert!(s.companion.enabled && s.companion.allow_workspace_switch);
        for bad in [
            json!({"pomodoro": {"focus": 0}}),
            json!({"pomodoro": {"focus": 181}}),
            json!({"pomodoro": {"shortBreak": 61}}),
            json!({"pomodoro": {"longBreak": 121}}),
            json!({"pomodoro": {"sessionsPerCycle": 13}}),
            json!({"pomodoro": {"sessionsPerCycle": 0}}),
            json!({"pomodoro": {"dailyGoal": 25}}),
            json!({"pomodoro": {"dailyGoal": 0}}),
            json!({"pomodoro": {"focus": 2.5}}),
            json!({"pomodoro": 5}),
            json!({"companion": {"enabled": "yes"}}),
        ] {
            assert!(merge(&d, &obj(bad.clone())).is_err(), "{bad}");
        }
        let edge = merge(
            &d,
            &obj(
                json!({"pomodoro": {"focus": 180, "shortBreak": 60, "longBreak": 120,
                "sessionsPerCycle": 12, "dailyGoal": 24}}),
            ),
        )
        .unwrap();
        assert_eq!(edge.pomodoro.sessions_per_cycle, 12);
    }

    #[test]
    fn calendar_feeds_are_validated_and_replaced() {
        let d = DeckSettings::default();
        let s = merge(
            &d,
            &obj(json!({"calendarFeeds": [
                feed("work", "https://example.com/a.ics"),
                feed("home-2", "webcal://cal.example.org/x?token=s3cret"),
            ]})),
        )
        .unwrap();
        assert_eq!(s.calendar_feeds.len(), 2);
        assert_eq!(s.calendar_feeds[0].color, FeedColor::Accent2);
        // Arrays are replaced wholesale.
        let s2 = merge(
            &s,
            &obj(json!({"calendarFeeds": [feed("x", "HTTPS://e.com/c.ics")]})),
        )
        .unwrap();
        assert_eq!(s2.calendar_feeds.len(), 1);
        assert_eq!(s2.calendar_feeds[0].id, "x");
        for bad in [
            json!([feed("Work", "https://e.com/a.ics")]),
            json!([feed("", "https://e.com/a.ics")]),
            json!([feed("a", "http://e.com/a.ics?k=s3cret")]),
            json!([feed("a", "file:///c:/a.ics")]),
            json!([feed("a", "https://")]),
            json!([feed("a", "https://user@/x")]),
            json!([feed("a", "https://e.com/a b.ics")]),
            json!([
                feed("a", "https://e.com/a.ics"),
                feed("a", "https://e.com/b.ics")
            ]),
            json!([{"id": "a", "name": "x", "url": "https://e.com", "color": "#ff0000", "enabled": true}]),
            json!([{"id": "a", "name": "x", "url": "https://e.com", "color": "purple", "enabled": true}]),
            json!({"id": "a"}),
        ] {
            let e = merge(&d, &obj(json!({ "calendarFeeds": bad.clone() })));
            assert!(e.is_err(), "{bad}");
            assert!(!e.unwrap_err().contains("s3cret"));
        }
        // Debug output never carries the URL.
        assert!(!format!("{:?}", s.calendar_feeds).contains("s3cret"));
    }

    #[test]
    fn auto_switch_rules_are_validated() {
        let d = DeckSettings::default();
        let s = merge(
            &d,
            &obj(json!({"autoSwitch": [
                {"id": "r1", "enabled": true, "trigger": {"type": "time", "at": "09:00", "days": [1, 2, 3, 4, 5]}, "workspace": "focus"},
                {"id": "r2", "enabled": false, "trigger": {"type": "pomodoro-start"}, "workspace": "focus"},
                {"id": "r3", "enabled": true, "trigger": {"type": "pomodoro-end"}, "workspace": "ops"},
                {"id": "r4", "enabled": true, "trigger": {"type": "calendar-start", "feedId": null}, "workspace": "ops"},
                {"id": "r5", "enabled": true, "trigger": {"type": "calendar-start", "feedId": "work"}, "workspace": "ops"},
            ]})),
        )
        .unwrap();
        assert_eq!(s.auto_switch.len(), 5);
        assert_eq!(
            s.auto_switch[4].trigger,
            AutoSwitchTrigger::CalendarStart {
                feed_id: Some("work".into())
            }
        );
        // Serialises back to the TS shape.
        let v = serde_json::to_value(&s.auto_switch).unwrap();
        assert_eq!(
            v[0]["trigger"],
            json!({"type": "time", "at": "09:00", "days": [1, 2, 3, 4, 5]})
        );
        assert_eq!(
            v[3]["trigger"],
            json!({"type": "calendar-start", "feedId": null})
        );
        assert_eq!(v[1]["trigger"], json!({"type": "pomodoro-start"}));
        let rule =
            |t: Value| json!([{"id": "r", "enabled": true, "trigger": t, "workspace": "focus"}]);
        for bad in [
            rule(json!({"type": "time", "at": "9:00", "days": []})),
            rule(json!({"type": "time", "at": "09:00", "days": [7]})),
            rule(json!({"type": "time", "at": "09:00", "days": [-1]})),
            rule(json!({"type": "time", "at": "09:00"})),
            rule(json!({"type": "sunrise"})),
            rule(json!({"type": "calendar-start", "feedId": "Bad Id"})),
            json!([{"id": "", "enabled": true, "trigger": {"type": "pomodoro-end"}, "workspace": "focus"}]),
            json!([{"id": "r", "enabled": true, "trigger": {"type": "pomodoro-end"}, "workspace": "Not A Slug"}]),
            json!([
                {"id": "r", "enabled": true, "trigger": {"type": "pomodoro-end"}, "workspace": "a"},
                {"id": "r", "enabled": true, "trigger": {"type": "pomodoro-end"}, "workspace": "b"}
            ]),
        ] {
            assert!(
                merge(&d, &obj(json!({ "autoSwitch": bad.clone() }))).is_err(),
                "{bad}"
            );
        }
    }

    #[test]
    fn phase3_stored_corruption_falls_back_per_key() {
        let s = from_stored(obj(json!({
            "pomodoro": {"focus": 500, "shortBreak": 10, "autoStartNext": "x"},
            "companion": {"enabled": false, "notifyApprovals": 3},
            "calendarFeeds": [feed("ok", "https://e.com/a.ics"), feed("BAD", "https://e.com")],
            "autoSwitch": "garbage",
            "theme": "nord"
        })));
        assert_eq!(s.pomodoro.focus, 25);
        assert_eq!(s.pomodoro.short_break, 10);
        assert!(!s.pomodoro.auto_start_next);
        assert!(!s.companion.enabled);
        assert!(s.companion.notify_approvals);
        // A bad array element resets the whole array.
        assert!(s.calendar_feeds.is_empty());
        assert!(s.auto_switch.is_empty());
        assert_eq!(s.theme.as_deref(), Some("nord"));
        // A Phase 2 store gets the Phase 3 defaults.
        let p2 = from_stored(obj(json!({"schemaVersion": 1, "notificationSound": false})));
        assert_eq!(p2.pomodoro, PomodoroSettings::default());
        assert_eq!(p2.companion, CompanionSettings::default());
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
