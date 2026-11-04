//! Phase 4 external widgets (`docs/plugin-api.md`, ARCHITECTURE.md "Widget plugins").
//!
//! Scans `~/.kotilo/deck/widgets/<folder>/manifest.json`, validates each manifest, serves
//! the folder's files through the `kotilo-widget` URI scheme with a no-network CSP, and
//! watches the directory so a new folder loads without a rebuild (`plugin://changed`).
//! Plugin code only ever runs inside a sandboxed iframe on the deck.
//!
//! Owner: rust-platform (plugins + updater worktree).

use serde::Serialize;
use serde_json::Value;
use tauri::AppHandle;

pub const EVENT_PLUGIN_CHANGED: &str = "plugin://changed";
/// URI scheme for plugin files: `http://kotilo-widget.localhost/<folder>/<path>` on
/// Windows, `kotilo-widget://localhost/<folder>/<path>` elsewhere.
pub const SCHEME: &str = "kotilo-widget";

/// Mirrors `PluginWidget` in `src/lib/ipc/index.ts`.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginWidget {
    /// `ext.<folder>`; the `widget` value in workspace files.
    pub id: String,
    pub folder: String,
    /// Validated manifest (`PluginManifest`); `None` when invalid.
    pub manifest: Option<Value>,
    /// Why the folder was rejected (shown in Settings ▸ Widgets).
    pub error: Option<String>,
    /// iframe `src` for the entry file inside the Tauri webview; `None` when invalid.
    pub entry_url: Option<String>,
}

/// CSP sent with every plugin file (URI scheme and LAN `/plugins/...`). No `connect-src`.
pub const PLUGIN_CSP: &str = "default-src 'none'; script-src 'self' 'unsafe-inline'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; font-src 'self' data:; media-src 'self'";

/// One served plugin file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginFile {
    pub bytes: Vec<u8>,
    /// Content-Type, e.g. "text/html; charset=utf-8".
    pub mime: &'static str,
}

/// Reads `<folder>/<path>` of a valid plugin, or the SDK when `folder == "_sdk"`.
/// `None` = 404 (unknown/invalid plugin, traversal, symlink escape, over limits). Shared by
/// the URI-scheme handler and the LAN server, which both add [`PLUGIN_CSP`].
pub fn read_file(_app: &AppHandle, _folder: &str, _path: &str) -> Option<PluginFile> {
    None
}

/// Startup: initial scan + directory watcher.
pub fn start(_app: &AppHandle) {}

#[tauri::command]
pub async fn plugin_list(_app: AppHandle) -> Result<Vec<PluginWidget>, String> {
    Ok(Vec::new())
}

/// Opens `~/.kotilo/deck/widgets/` (created if missing) in the OS file manager.
#[tauri::command]
pub async fn plugin_open_folder(_app: AppHandle) -> Result<(), String> {
    Err("not implemented".into())
}
