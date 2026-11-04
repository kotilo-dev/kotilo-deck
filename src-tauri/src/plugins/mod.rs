//! Phase 4 external widgets (`docs/plugin-api.md`, ARCHITECTURE.md "Widget plugins").
//!
//! Scans `~/.kotilo/deck/widgets/<folder>/manifest.json`, validates each manifest, serves
//! the folder's files through the `kotilo-widget` URI scheme with a no-network CSP, and
//! watches the directory so a new folder loads without a rebuild (`plugin://changed`).
//! Plugin code only ever runs inside a sandboxed iframe on the deck.
//!
//! Owner: rust-platform (plugins + updater worktree).

mod manifest;
mod serve;

use std::fs;
use std::path::Path;
use std::sync::Mutex;
use std::time::Duration;

use notify::RecommendedWatcher;
use notify_debouncer_mini::{new_debouncer, DebounceEventResult, Debouncer};
use serde::Serialize;
use serde_json::{json, Value};
use tauri::http::{header, Method, Request, Response, StatusCode};
use tauri::{AppHandle, Emitter, Manager, UriSchemeContext, UriSchemeResponder};

use crate::paths;
use crate::platform::{self, PlatformError};

pub const EVENT_PLUGIN_CHANGED: &str = "plugin://changed";
/// URI scheme for plugin files: `http://kotilo-widget.localhost/<folder>/<path>` on
/// Windows, `kotilo-widget://localhost/<folder>/<path>` elsewhere.
pub const SCHEME: &str = "kotilo-widget";

/// At most this many plugin folders are loaded (by folder name); the rest are listed with
/// an error.
pub const MAX_PLUGINS: usize = 64;
/// Pseudo-folder that serves the SDK.
const SDK_FOLDER: &str = "_sdk";
const SDK_FILE: &str = "kotilo-widget.js";
const SDK_JS: &str = include_str!("sdk/kotilo-widget.js");
const WATCH_DEBOUNCE: Duration = Duration::from_millis(400);

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

/// Last scan plus the folder watcher. Managed from `lib.rs`.
#[derive(Default)]
pub struct Plugins {
    list: Mutex<Vec<PluginWidget>>,
    watcher: Mutex<Option<Debouncer<RecommendedWatcher>>>,
}

impl Plugins {
    fn store(&self, list: Vec<PluginWidget>) -> bool {
        let mut cur = self.list.lock().unwrap_or_else(|p| p.into_inner());
        let changed = *cur != list;
        *cur = list;
        changed
    }

    fn snapshot(&self) -> Vec<PluginWidget> {
        self.list.lock().unwrap_or_else(|p| p.into_inner()).clone()
    }
}

fn invalid(folder: &str, error: String) -> PluginWidget {
    PluginWidget {
        id: format!("ext.{folder}"),
        folder: folder.to_string(),
        manifest: None,
        error: Some(error),
        entry_url: None,
    }
}

/// Validates one plugin folder (name already checked).
fn load(dir: &Path, folder: &str) -> PluginWidget {
    match load_manifest(dir, folder) {
        Ok(m) => {
            let entry = m["entry"].as_str().unwrap_or(manifest::DEFAULT_ENTRY);
            PluginWidget {
                id: format!("ext.{folder}"),
                folder: folder.to_string(),
                entry_url: Some(serve::entry_url(folder, entry)),
                manifest: Some(m),
                error: None,
            }
        }
        Err(e) => invalid(folder, e),
    }
}

fn load_manifest(dir: &Path, folder: &str) -> Result<Value, String> {
    serve::check_limits(dir)?;
    let path = dir.join("manifest.json");
    let meta = match fs::symlink_metadata(&path) {
        Ok(m) if m.is_file() => m,
        Ok(_) => return Err("manifest.json must be a regular file".into()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Err("manifest.json is missing".into())
        }
        Err(e) => return Err(format!("cannot read manifest.json: {e}")),
    };
    if meta.len() > manifest::MAX_MANIFEST_BYTES {
        return Err("manifest.json is larger than 64 KB".into());
    }
    let bytes = fs::read(&path).map_err(|e| format!("cannot read manifest.json: {e}"))?;
    let raw: Value = serde_json::from_slice(&bytes)
        .map_err(|e| format!("manifest.json is not valid JSON: {e}"))?;
    manifest::validate(folder, &raw, |entry| serve::resolve(dir, entry).is_some())
}

/// Every plugin folder under `root`, sorted by folder name. `.`/`_`-prefixed entries and
/// plain files are ignored; a missing `root` is an empty list.
pub fn scan(root: &Path) -> Vec<PluginWidget> {
    let Ok(entries) = fs::read_dir(root) else {
        return Vec::new();
    };
    let mut folders: Vec<String> = entries
        .filter_map(Result::ok)
        .filter_map(|e| {
            let name = e.file_name().into_string().ok()?;
            let hidden = name.starts_with('.') || name.starts_with('_');
            // `is_dir` follows a symlinked plugin folder (handy while developing one).
            (!hidden && e.path().is_dir()).then_some(name)
        })
        .collect();
    folders.sort();
    folders
        .iter()
        .enumerate()
        .map(|(i, name)| {
            if i >= MAX_PLUGINS {
                invalid(
                    name,
                    format!("more than {MAX_PLUGINS} plugins installed; this one is not loaded"),
                )
            } else if !manifest::valid_folder_id(name) {
                invalid(
                    name,
                    "folder name must be 1–32 lowercase letters, digits or '-' (not starting with '-')"
                        .into(),
                )
            } else {
                load(&root.join(name), name)
            }
        })
        .collect()
}

/// [`read_file`] against an explicit root and plugin list.
fn read_from(root: &Path, list: &[PluginWidget], folder: &str, path: &str) -> Option<PluginFile> {
    if folder == SDK_FOLDER {
        return (path == SDK_FILE).then(|| PluginFile {
            bytes: SDK_JS.as_bytes().to_vec(),
            mime: serve::mime_for(SDK_FILE),
        });
    }
    list.iter()
        .find(|p| p.folder == folder && p.manifest.is_some())?;
    let bytes = serve::read(&root.join(folder), path)?;
    Some(PluginFile {
        bytes,
        mime: serve::mime_for(path),
    })
}

/// Reads `<folder>/<path>` of a valid plugin, or the SDK when `folder == "_sdk"`.
/// `None` = 404 (unknown/invalid plugin, traversal, symlink escape, over limits). Shared by
/// the URI-scheme handler and the LAN server, which both add [`PLUGIN_CSP`].
pub fn read_file(app: &AppHandle, folder: &str, path: &str) -> Option<PluginFile> {
    let list = app
        .try_state::<Plugins>()
        .map(|s| s.snapshot())
        .unwrap_or_default();
    read_from(&paths::plugin_widgets_dir(), &list, folder, path)
}

fn respond(method: &Method, file: Option<PluginFile>) -> Response<Vec<u8>> {
    let builder = Response::builder()
        .header(header::CONTENT_SECURITY_POLICY, PLUGIN_CSP)
        .header(header::X_CONTENT_TYPE_OPTIONS, "nosniff")
        .header(header::CACHE_CONTROL, "no-cache");
    let res = if method != Method::GET && method != Method::HEAD {
        builder
            .status(StatusCode::METHOD_NOT_ALLOWED)
            .header(header::ALLOW, "GET, HEAD")
            .body(Vec::new())
    } else if let Some(f) = file {
        let body = if method == Method::HEAD {
            Vec::new()
        } else {
            f.bytes
        };
        builder
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, f.mime)
            .body(body)
    } else {
        builder
            .status(StatusCode::NOT_FOUND)
            .header(header::CONTENT_TYPE, "text/plain; charset=utf-8")
            .body(b"not found".to_vec())
    };
    res.unwrap_or_else(|_| {
        let mut r = Response::new(Vec::new());
        *r.status_mut() = StatusCode::INTERNAL_SERVER_ERROR;
        r
    })
}

/// `kotilo-widget` URI-scheme handler (registered on the Builder in `lib.rs`). File IO runs
/// on the blocking pool so the webview's protocol thread never waits on disk.
pub fn protocol(
    ctx: UriSchemeContext<'_, tauri::Wry>,
    request: Request<Vec<u8>>,
    responder: UriSchemeResponder,
) {
    let app = ctx.app_handle().clone();
    let method = request.method().clone();
    let url = request.uri().to_string();
    tauri::async_runtime::spawn_blocking(move || {
        let file =
            serve::parse_url(&url).and_then(|(folder, path)| read_file(&app, &folder, &path));
        responder.respond(respond(&method, file));
    });
}

fn rescan(app: &AppHandle) -> (Vec<PluginWidget>, bool) {
    let list = scan(&paths::plugin_widgets_dir());
    let changed = app.state::<Plugins>().store(list.clone());
    (list, changed)
}

/// Startup: initial scan + directory watcher.
pub fn start(app: &AppHandle) {
    let dir = paths::plugin_widgets_dir();
    if let Err(e) = fs::create_dir_all(&dir) {
        tracing::warn!("cannot create plugin widgets folder: {e}");
        return;
    }
    let (list, _) = rescan(app);
    let valid = list.iter().filter(|p| p.manifest.is_some()).count();
    tracing::info!(
        valid,
        invalid = list.len() - valid,
        "plugin widgets scanned"
    );

    let handle = app.clone();
    let debouncer = new_debouncer(WATCH_DEBOUNCE, move |res: DebounceEventResult| match res {
        Ok(_) => {
            // Rescan on any change; only tell the UI when the list actually differs
            // (folder added/removed, manifest edited, entry appeared/disappeared).
            if rescan(&handle).1 {
                tracing::debug!("plugin widgets changed");
                let _ = handle.emit(EVENT_PLUGIN_CHANGED, json!({}));
            }
        }
        Err(e) => tracing::warn!("plugin watcher error: {e}"),
    });
    let mut debouncer = match debouncer {
        Ok(d) => d,
        Err(e) => {
            tracing::warn!("cannot start plugin watcher: {e}");
            return;
        }
    };
    if let Err(e) = debouncer
        .watcher()
        .watch(&dir, notify::RecursiveMode::Recursive)
    {
        tracing::warn!("cannot watch plugin widgets folder: {e}");
        return;
    }
    *app.state::<Plugins>()
        .watcher
        .lock()
        .unwrap_or_else(|p| p.into_inner()) = Some(debouncer);
}

#[tauri::command]
pub async fn plugin_list(app: AppHandle) -> Result<Vec<PluginWidget>, String> {
    tauri::async_runtime::spawn_blocking(move || rescan(&app).0)
        .await
        .map_err(|e| format!("background task failed: {e}"))
}

/// Opens `~/.kotilo/deck/widgets/` (created if missing) in the OS file manager.
#[tauri::command]
pub async fn plugin_open_folder(_app: AppHandle) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(|| {
        let dir = paths::plugin_widgets_dir();
        fs::create_dir_all(&dir).map_err(|e| format!("cannot create the widgets folder: {e}"))?;
        platform::current().open_path(&dir).map_err(|e| match e {
            PlatformError::Unsupported(_) => {
                "opening folders is not supported on this OS; open ~/.kotilo/deck/widgets manually"
                    .to_string()
            }
            PlatformError::Os(m) => m,
        })
    })
    .await
    .map_err(|e| format!("background task failed: {e}"))?
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_plugin(root: &Path, folder: &str, manifest: Value) {
        let dir = root.join(folder);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("manifest.json"), manifest.to_string()).unwrap();
        fs::write(dir.join("index.html"), "<!doctype html><p>hi</p>").unwrap();
    }

    fn manifest_for(id: &str) -> Value {
        json!({
            "apiVersion": 1, "id": id, "name": id,
            "sizes": ["2x1", "2x2"], "defaultSize": "2x2"
        })
    }

    #[test]
    fn scan_lists_sorted_with_errors() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        write_plugin(root, "zeta", manifest_for("zeta"));
        write_plugin(root, "alpha", manifest_for("alpha"));
        write_plugin(root, "mismatch", manifest_for("other"));
        fs::create_dir(root.join("Bad Name")).unwrap();
        fs::create_dir(root.join("empty")).unwrap();
        fs::create_dir(root.join(".git")).unwrap();
        fs::create_dir(root.join("_drafts")).unwrap();
        fs::write(root.join("README.md"), "x").unwrap();

        let list = scan(root);
        let folders: Vec<&str> = list.iter().map(|p| p.folder.as_str()).collect();
        assert_eq!(folders, ["Bad Name", "alpha", "empty", "mismatch", "zeta"]);

        let alpha = &list[1];
        assert_eq!(alpha.id, "ext.alpha");
        assert!(alpha.error.is_none());
        assert_eq!(alpha.manifest.as_ref().unwrap()["entry"], "index.html");
        assert!(alpha
            .entry_url
            .as_deref()
            .unwrap()
            .ends_with("/alpha/index.html"));

        for (i, want) in [(0, "folder name"), (2, "missing"), (3, "folder name")] {
            let p = &list[i];
            assert!(p.manifest.is_none() && p.entry_url.is_none());
            let err = p.error.as_deref().unwrap();
            assert!(err.contains(want), "{}: {err}", p.folder);
        }
        assert!(scan(&root.join("nope")).is_empty());
    }

    #[test]
    fn manifest_size_and_json_errors() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let big = root.join("big");
        fs::create_dir(&big).unwrap();
        fs::write(big.join("manifest.json"), " ".repeat(64 * 1024 + 1)).unwrap();
        let broken = root.join("broken");
        fs::create_dir(&broken).unwrap();
        fs::write(broken.join("manifest.json"), "{nope").unwrap();
        let noentry = root.join("noentry");
        fs::create_dir(&noentry).unwrap();
        fs::write(
            noentry.join("manifest.json"),
            manifest_for("noentry").to_string(),
        )
        .unwrap();

        let list = scan(root);
        let err = |i: usize| list[i].error.clone().unwrap();
        assert!(err(0).contains("64 KB"), "{}", err(0));
        assert!(err(1).contains("not valid JSON"), "{}", err(1));
        assert!(err(2).contains("does not exist"), "{}", err(2));
    }

    #[test]
    fn plugin_count_is_capped() {
        let tmp = tempfile::tempdir().unwrap();
        for i in 0..=MAX_PLUGINS {
            let id = format!("p{i:03}");
            write_plugin(tmp.path(), &id, manifest_for(&id));
        }
        let list = scan(tmp.path());
        assert_eq!(list.len(), MAX_PLUGINS + 1);
        assert!(list[..MAX_PLUGINS].iter().all(|p| p.manifest.is_some()));
        let last = list.last().unwrap();
        assert!(last.manifest.is_none());
        assert!(last.error.as_deref().unwrap().contains("64 plugins"));
    }

    #[test]
    fn serves_valid_plugins_and_sdk_only() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        write_plugin(root, "hello", manifest_for("hello"));
        write_plugin(root, "broken", manifest_for("other"));
        fs::write(root.join("secret.txt"), "s").unwrap();
        let list = scan(root);

        let f = read_from(root, &list, "hello", "index.html").unwrap();
        assert_eq!(f.mime, "text/html; charset=utf-8");
        assert!(read_from(root, &list, "hello", "../secret.txt").is_none());
        assert!(read_from(root, &list, "hello", "../broken/index.html").is_none());
        assert!(read_from(root, &list, "broken", "index.html").is_none());
        assert!(read_from(root, &list, "unknown", "index.html").is_none());

        let sdk = read_from(root, &list, "_sdk", "kotilo-widget.js").unwrap();
        assert_eq!(sdk.mime, "text/javascript; charset=utf-8");
        let js = String::from_utf8(sdk.bytes).unwrap();
        assert!(js.contains("type: 'ready'") && js.contains("window.kotilo"));
        assert!(read_from(root, &list, "_sdk", "other.js").is_none());
    }

    #[test]
    fn responses_carry_csp_and_nosniff() {
        let ok = respond(
            &Method::GET,
            Some(PluginFile {
                bytes: b"x".to_vec(),
                mime: "text/html; charset=utf-8",
            }),
        );
        assert_eq!(ok.status(), StatusCode::OK);
        assert_eq!(ok.headers()[header::CONTENT_SECURITY_POLICY], PLUGIN_CSP);
        assert_eq!(ok.headers()[header::X_CONTENT_TYPE_OPTIONS], "nosniff");
        assert_eq!(
            ok.headers()[header::CONTENT_TYPE],
            "text/html; charset=utf-8"
        );
        assert_eq!(ok.body(), b"x");

        let missing = respond(&Method::GET, None);
        assert_eq!(missing.status(), StatusCode::NOT_FOUND);
        assert_eq!(
            missing.headers()[header::CONTENT_SECURITY_POLICY],
            PLUGIN_CSP
        );

        assert_eq!(
            respond(&Method::POST, None).status(),
            StatusCode::METHOD_NOT_ALLOWED
        );
        let head = respond(
            &Method::HEAD,
            Some(PluginFile {
                bytes: b"x".to_vec(),
                mime: "text/css; charset=utf-8",
            }),
        );
        assert!(head.body().is_empty());
        assert!(!PLUGIN_CSP.contains("connect-src"));
    }

    #[test]
    fn example_plugin_validates() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../examples/widgets");
        let list = scan(&dir);
        let hello = list.iter().find(|p| p.folder == "hello").expect("example");
        assert!(hello.error.is_none(), "{:?}", hello.error);
        assert_eq!(
            hello.manifest.as_ref().unwrap()["topics"],
            json!(["system"])
        );
    }

    #[test]
    fn serialises_like_the_ts_contract() {
        let v = serde_json::to_value(invalid("x", "bad".into())).unwrap();
        assert_eq!(
            v,
            json!({"id": "ext.x", "folder": "x", "manifest": null, "error": "bad", "entryUrl": null})
        );
    }
}
