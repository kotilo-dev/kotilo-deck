//! Phase 4 auto-update via `tauri-plugin-updater` (ARCHITECTURE.md "Auto-update").
//!
//! Backend-owned: checks at startup and every 24 h when `updates.autoCheck`, never
//! installs without the user clicking Install. State is pushed as `update://status`.
//!
//! Owner: rust-platform (plugins + updater worktree).

use serde::Serialize;
use tauri::AppHandle;

use crate::settings::UpdateSettings;

pub const EVENT_UPDATE_STATUS: &str = "update://status";

/// Mirrors `UpdateState` in `src/lib/ipc/index.ts`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum UpdateState {
    #[default]
    Idle,
    Checking,
    Available,
    Downloading,
    /// Installed; relaunch pending (Windows installers quit the app themselves).
    Ready,
    Error,
    /// Dev build or no updater configured.
    Unsupported,
}

/// Mirrors `UpdateInfo`.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInfo {
    pub version: String,
    /// RFC 3339 publish date from the manifest.
    pub date: Option<String>,
    pub notes: Option<String>,
}

/// Mirrors `UpdateStatus`.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateStatus {
    pub current: String,
    pub state: UpdateState,
    pub available: Option<UpdateInfo>,
    /// 0–1 while downloading.
    pub progress: Option<f64>,
    pub error: Option<String>,
    /// Unix ms of the last finished check.
    pub last_checked: Option<i64>,
}

/// Startup: schedules the periodic check when `updates.autoCheck`.
pub fn start(_app: &AppHandle, _updates: &UpdateSettings) {}

/// Settings hook: `updates` changed.
pub fn settings_changed(_app: &AppHandle, _updates: &UpdateSettings) {}

#[tauri::command]
pub async fn update_status(_app: AppHandle) -> Result<UpdateStatus, String> {
    Ok(UpdateStatus::default())
}

/// Checks now (also when autoCheck is off).
#[tauri::command]
pub async fn update_check(_app: AppHandle) -> Result<UpdateStatus, String> {
    Ok(UpdateStatus::default())
}

/// Downloads and installs the available update, then relaunches.
#[tauri::command]
pub async fn update_install(_app: AppHandle) -> Result<(), String> {
    Err("no update available".into())
}
