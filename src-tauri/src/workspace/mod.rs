//! Workspaces: `~/.kotilo/deck/workspaces/<id>.json` (SPEC §4.1).
//!
//! Widget entries stay opaque `serde_json::Value`s (only their basic shape is checked) so
//! per-widget settings round-trip untouched. Unknown top-level fields are preserved too.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use tauri::State;

use crate::migrations::{self, MigrationError, WORKSPACE_VERSION};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Grid {
    pub cols: u32,
    pub rows: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Workspace {
    pub schema_version: u64,
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub theme: Option<String>,
    pub grid: Grid,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub appearance: Option<Value>,
    #[serde(default)]
    pub widgets: Vec<Value>,
    /// Fields this build does not know about; written back as-is.
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// `^[a-z0-9-]+$`, max 64 chars. Doubles as the file name, so this is the path-traversal guard.
pub fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

/// Minimal shape check for a widget entry (`WidgetInstance` in the IPC contract).
pub fn valid_widget(v: &Value) -> bool {
    let Some(o) = v.as_object() else {
        return false;
    };
    let non_empty_str = |k: &str| {
        o.get(k)
            .and_then(Value::as_str)
            .is_some_and(|s| !s.is_empty())
    };
    let uint = |k: &str| o.get(k).and_then(Value::as_u64);
    non_empty_str("instance")
        && non_empty_str("widget")
        && uint("x").is_some()
        && uint("y").is_some()
        && uint("w").is_some_and(|w| w >= 1)
        && uint("h").is_some_and(|h| h >= 1)
        && !matches!(o.get("settings"), Some(s) if !(s.is_object() || s.is_null()))
}

pub struct WorkspaceStore {
    dir: PathBuf,
}

impl WorkspaceStore {
    pub fn new(dir: PathBuf) -> Self {
        Self { dir }
    }

    fn path_for(&self, id: &str) -> PathBuf {
        self.dir.join(format!("{id}.json"))
    }

    /// Every readable workspace, migrated to the current version, sorted by id.
    /// Bad files are logged and skipped, never modified. An existing but unreadable
    /// directory is an error, so the UI never mistakes it for a first run and seeds presets.
    pub fn list(&self) -> Result<Vec<Workspace>, String> {
        let entries = match fs::read_dir(&self.dir) {
            Ok(e) => e,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => {
                tracing::warn!("cannot read workspaces dir: {e}");
                return Err(format!("cannot read workspaces folder: {e}"));
            }
        };
        let mut out: Vec<Workspace> = entries
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|x| x == "json"))
            .filter_map(|p| match read_workspace(&p) {
                Ok(ws) => Some(ws),
                Err(reason) => {
                    tracing::warn!(file = %file_label(&p), "skipping workspace: {reason}");
                    None
                }
            })
            .collect();
        out.sort_by(|a, b| a.id.cmp(&b.id));
        Ok(out)
    }

    pub fn save(&self, ws: &Workspace) -> Result<(), String> {
        if !valid_id(&ws.id) {
            return Err("invalid workspace id (expected [a-z0-9-]+)".into());
        }
        if ws.name.trim().is_empty() {
            return Err("workspace name is empty".into());
        }
        if ws.grid.cols == 0 || ws.grid.rows == 0 {
            return Err("grid must be at least 1x1".into());
        }
        if let Some(i) = ws.widgets.iter().position(|w| !valid_widget(w)) {
            return Err(format!("widget #{i} is malformed"));
        }
        let path = self.path_for(&ws.id);
        self.refuse_if_future(&path)?;
        backup_if_lossy(&path)?;

        let mut ws = ws.clone();
        ws.schema_version = WORKSPACE_VERSION;
        let json = serde_json::to_vec_pretty(&ws).map_err(|e| e.to_string())?;
        fs::create_dir_all(&self.dir).map_err(|e| format!("cannot create workspaces dir: {e}"))?;
        write_atomic(&path, &json).map_err(|e| format!("cannot write workspace: {e}"))
    }

    pub fn delete(&self, id: &str) -> Result<(), String> {
        if !valid_id(id) {
            return Err("invalid workspace id (expected [a-z0-9-]+)".into());
        }
        let path = self.path_for(id);
        self.refuse_if_future(&path)?;
        match fs::remove_file(&path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(format!("cannot delete workspace: {e}")),
        }
    }

    /// A file written by a newer build must never be overwritten or deleted by this one.
    fn refuse_if_future(&self, path: &Path) -> Result<(), String> {
        let Ok(bytes) = fs::read(path) else {
            return Ok(());
        };
        let Ok(doc) = serde_json::from_slice::<Value>(&bytes) else {
            return Ok(());
        };
        match migrations::version_of(&doc) {
            Ok(v) if v > WORKSPACE_VERSION => Err(format!(
                "workspace was saved by a newer Kotilo Deck (schemaVersion {v}); not modifying it"
            )),
            _ => Ok(()),
        }
    }
}

fn read_workspace(path: &Path) -> Result<Workspace, String> {
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .filter(|s| valid_id(s))
        .ok_or("file name is not a valid workspace id")?
        .to_string();
    let bytes = fs::read(path).map_err(|e| e.to_string())?;
    let doc: Value = serde_json::from_slice(&bytes).map_err(|e| format!("invalid JSON: {e}"))?;
    let (doc, migrated) = migrations::migrate_workspace(doc).map_err(|e| match e {
        MigrationError::Future(_) => format!("{e}; left untouched"),
        other => other.to_string(),
    })?;
    if migrated {
        tracing::info!(file = %file_label(path), "migrated workspace in memory");
    }
    let mut ws: Workspace =
        serde_json::from_value(doc).map_err(|e| format!("unexpected shape: {e}"))?;
    if ws.id != stem {
        tracing::warn!(file = %file_label(path), "workspace id differs from file name; using file name");
        ws.id = stem;
    }
    let before = ws.widgets.len();
    ws.widgets.retain(valid_widget);
    if ws.widgets.len() != before {
        tracing::warn!(
            file = %file_label(path),
            dropped = before - ws.widgets.len(),
            "dropped malformed widget entries"
        );
    }
    Ok(ws)
}

/// File name only (logs must not carry the user's home path).
fn file_label(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// Before overwriting a file we could not fully read (unparseable, or with widget entries
/// `read_workspace` would drop), keep a timestamped copy so a save never destroys user data.
fn backup_if_lossy(path: &Path) -> Result<(), String> {
    if !path.exists() {
        return Ok(());
    }
    let lossless = fs::read(path)
        .ok()
        .and_then(|b| serde_json::from_slice::<Value>(&b).ok())
        .is_some_and(|doc| {
            let widgets_ok = doc
                .get("widgets")
                .and_then(Value::as_array)
                .is_some_and(|ws| ws.iter().all(valid_widget));
            widgets_ok && read_workspace(path).is_ok()
        });
    if lossless {
        return Ok(());
    }
    let stamp = chrono::Utc::now().format("%Y%m%dT%H%M%S");
    let backup = path.with_extension(format!("json.{stamp}.bak"));
    fs::copy(path, &backup).map_err(|e| format!("cannot back up unreadable workspace: {e}"))?;
    tracing::warn!(file = %file_label(path), "backed up unreadable workspace before overwrite");
    Ok(())
}

/// Write to a unique sibling temp file, fsync, then rename over the target (replaces on Windows too).
pub(crate) fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::sync::atomic::{AtomicU64, Ordering};
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let file_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("workspace.json");
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    let tmp = path.with_file_name(format!(".{file_name}.{}.{seq}.tmp", std::process::id()));
    let result = (|| {
        let mut f = fs::File::create(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
        drop(f);
        fs::rename(&tmp, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result
}

#[tauri::command]
pub async fn workspace_list(store: State<'_, WorkspaceStore>) -> Result<Vec<Workspace>, String> {
    store.list()
}

#[tauri::command]
pub async fn workspace_save(
    store: State<'_, WorkspaceStore>,
    workspace: Workspace,
) -> Result<(), String> {
    store.save(&workspace)
}

#[tauri::command]
pub async fn workspace_delete(store: State<'_, WorkspaceStore>, id: String) -> Result<(), String> {
    store.delete(&id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn sample(id: &str) -> Workspace {
        serde_json::from_value(json!({
            "schemaVersion": 1, "id": id, "name": "Focus", "theme": null,
            "grid": {"cols": 8, "rows": 4},
            "appearance": {"opacity": 0.8},
            "widgets": [
                {"instance": "clock-1", "widget": "clock", "x": 0, "y": 0, "w": 4, "h": 2,
                 "settings": {"seconds": true, "appearance": {"opacity": 0.6}}},
                {"instance": "clock-2", "widget": "clock", "x": 4, "y": 0, "w": 2, "h": 1}
            ],
            "futureField": {"keep": true}
        }))
        .unwrap()
    }

    #[test]
    fn id_validation() {
        for ok in ["focus", "ops-2", "a", "0"] {
            assert!(valid_id(ok), "{ok}");
        }
        for bad in [
            "",
            "Focus",
            "../evil",
            "a/b",
            "a\\b",
            "a.json",
            "a b",
            "é",
            "..",
            &"a".repeat(65),
        ] {
            assert!(!valid_id(bad), "{bad}");
        }
    }

    #[test]
    fn save_list_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let store = WorkspaceStore::new(dir.path().join("workspaces"));
        assert!(store.list().unwrap().is_empty());
        let ws = sample("focus");
        store.save(&ws).unwrap();
        store.save(&sample("ops")).unwrap();
        let listed = store.list().unwrap();
        assert_eq!(listed.len(), 2);
        assert_eq!(listed[0], ws);
        assert_eq!(listed[0].extra["futureField"], json!({"keep": true}));
        // No temp files left behind.
        let names: Vec<_> = fs::read_dir(dir.path().join("workspaces"))
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        assert!(names.iter().all(|n| !n.ends_with(".tmp")), "{names:?}");

        // Overwrite replaces.
        let mut renamed = ws.clone();
        renamed.name = "Deep focus".into();
        store.save(&renamed).unwrap();
        assert_eq!(store.list().unwrap()[0].name, "Deep focus");

        store.delete("focus").unwrap();
        store.delete("focus").unwrap(); // idempotent
        assert_eq!(store.list().unwrap().len(), 1);
    }

    #[test]
    fn save_rejects_traversal_and_bad_widgets() {
        let dir = tempfile::tempdir().unwrap();
        let store = WorkspaceStore::new(dir.path().to_path_buf());
        assert!(store.save(&sample("../evil")).is_err());
        assert!(store.delete("../evil").is_err());
        let mut ws = sample("ok");
        ws.widgets.push(json!({"widget": "clock"}));
        assert!(store.save(&ws).is_err());
    }

    #[test]
    fn list_migrates_skips_and_never_touches_bad_files() {
        let dir = tempfile::tempdir().unwrap();
        let d = dir.path();
        // v0: no schemaVersion.
        fs::write(
            d.join("legacy.json"),
            r#"{"id":"legacy","name":"Legacy","grid":{"cols":8,"rows":4},"widgets":[{"instance":"c","widget":"clock","x":0,"y":0,"w":2,"h":1},{"bad":true}]}"#,
        )
        .unwrap();
        let future = r#"{"schemaVersion":7,"id":"future","name":"F","grid":{"cols":8,"rows":4},"widgets":[]}"#;
        fs::write(d.join("future.json"), future).unwrap();
        fs::write(d.join("broken.json"), "{not json").unwrap();
        fs::write(d.join("Bad Name.json"), "{}").unwrap();
        fs::write(d.join("notes.txt"), "ignore me").unwrap();

        let store = WorkspaceStore::new(d.to_path_buf());
        let listed = store.list().unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].id, "legacy");
        assert_eq!(listed[0].schema_version, 1);
        assert_eq!(listed[0].widgets.len(), 1);

        // Future file is neither overwritten nor deleted.
        assert!(store.save(&sample("future")).is_err());
        assert!(store.delete("future").is_err());
        assert_eq!(fs::read_to_string(d.join("future.json")).unwrap(), future);
        assert_eq!(
            fs::read_to_string(d.join("broken.json")).unwrap(),
            "{not json"
        );
    }

    #[test]
    fn file_name_wins_over_inner_id() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("evening.json"),
            r#"{"schemaVersion":1,"id":"other","name":"E","grid":{"cols":8,"rows":4},"widgets":[]}"#,
        )
        .unwrap();
        let listed = WorkspaceStore::new(dir.path().to_path_buf())
            .list()
            .unwrap();
        assert_eq!(listed[0].id, "evening");
    }

    #[test]
    fn save_backs_up_a_file_it_cannot_fully_read() {
        let dir = tempfile::tempdir().unwrap();
        let store = WorkspaceStore::new(dir.path().to_path_buf());
        fs::write(dir.path().join("focus.json"), b"{ not json").unwrap();
        assert!(store.list().unwrap().is_empty());

        let ws: Workspace = serde_json::from_value(serde_json::json!({
            "schemaVersion": 1, "id": "focus", "name": "Focus", "theme": null,
            "grid": { "cols": 8, "rows": 4 }, "widgets": []
        }))
        .unwrap();
        store.save(&ws).unwrap();

        let backups: Vec<_> = fs::read_dir(dir.path())
            .unwrap()
            .filter_map(Result::ok)
            .filter(|e| e.file_name().to_string_lossy().ends_with(".bak"))
            .collect();
        assert_eq!(backups.len(), 1);
        assert_eq!(fs::read(backups[0].path()).unwrap(), b"{ not json");
        // A second save over the now-valid file makes no further backup.
        store.save(&ws).unwrap();
        assert_eq!(
            fs::read_dir(dir.path())
                .unwrap()
                .filter_map(Result::ok)
                .filter(|e| e.file_name().to_string_lossy().ends_with(".bak"))
                .count(),
            1
        );
    }
}
