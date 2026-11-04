//! Phase 4 tablet/browser mode (SPEC §8 Phase 4, ARCHITECTURE.md "LAN mode").
//!
//! Serves the deck SPA plus an HTTP/WebSocket IPC bridge on `0.0.0.0:<lan.port>` so a
//! tablet or a browser on the LAN can show the deck. Off by default; every request needs
//! the companion token (`~/.kotilo/companion.token`). Only the commands on the allowlist
//! in ARCHITECTURE.md are reachable; backend events are forwarded over `/api/events`.
//!
//! Owner: rust-platform (LAN worktree). The functions below are the contract the rest of
//! the app calls; their bodies are filled in by that work.

use serde::Serialize;
use tauri::AppHandle;

use crate::settings::LanSettings;

pub const EVENT_LAN_STATUS: &str = "lan://status";

/// Mirrors `LanStatus` in `src/lib/ipc/index.ts`.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LanStatus {
    pub enabled: bool,
    pub listening: bool,
    pub port: u16,
    /// `http://<ipv4>:<port>/?token=<token>` for every non-loopback IPv4 address.
    pub urls: Vec<String>,
    /// SVG markup of a QR code for `urls[0]`; `None` when not listening.
    pub qr_svg: Option<String>,
    /// Open `/api/events` connections.
    pub clients: u32,
    /// Last bind/serve error, e.g. "port 47832 is in use".
    pub error: Option<String>,
}

/// Startup (from `startup::setup`): starts the server when `lan.enabled`.
pub fn start(_app: &AppHandle, _lan: &LanSettings) {}

/// Settings hook: `lan` changed (enable/disable/port). Rebinds; emits `lan://status`.
pub fn settings_changed(_app: &AppHandle, _lan: &LanSettings) {}

/// Quit path: closes every LAN connection.
pub fn shutdown(_app: &AppHandle) {}

#[tauri::command]
pub async fn lan_status(_app: AppHandle) -> Result<LanStatus, String> {
    Ok(LanStatus::default())
}
