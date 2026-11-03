//! Export / import of everything user-configurable as one JSON document
//! `{ schemaVersion: 1, settings, workspaces }` (SPEC §7).
//!
//! Import is all-or-nothing up to the first write: the whole document is validated, then
//! the current settings and workspace files are copied to
//! `~/.kotilo/deck/backups/import-<stamp>/`, then settings are applied (hotkey/autostart
//! may still refuse), then the workspace folder is replaced. Workspace files written by a
//! newer build are never touched.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use tauri::{AppHandle, Emitter, State};

use crate::migrations::{self, WORKSPACE_VERSION};
use crate::paths::{self, file_label};
use crate::settings::{self, DeckSettings};
use crate::workspace::{Workspace, WorkspaceStore};

pub const EXPORT_VERSION: u64 = 1;
pub const EVENT_WORKSPACE_CHANGED: &str = "workspace://changed";

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportDoc {
    pub schema_version: u64,
    pub settings: DeckSettings,
    pub workspaces: Vec<Workspace>,
}

pub fn export(settings: DeckSettings, workspaces: Vec<Workspace>) -> Result<String, String> {
    serde_json::to_string_pretty(&ExportDoc {
        schema_version: EXPORT_VERSION,
        settings,
        workspaces,
    })
    .map_err(|e| e.to_string())
}

/// Parses and validates an export. Settings are strict (a bad value rejects the import);
/// missing keys take defaults. Workspaces are migrated and checked like `workspace_save`.
pub fn parse(json: &str) -> Result<(Map<String, Value>, Vec<Workspace>), String> {
    let doc: Value = serde_json::from_str(json).map_err(|e| format!("not valid JSON: {e}"))?;
    let obj = doc.as_object().ok_or("export must be a JSON object")?;
    match obj.get("schemaVersion").and_then(Value::as_u64) {
        Some(v) if v <= EXPORT_VERSION && v >= 1 => {}
        Some(v) if v > EXPORT_VERSION => {
            return Err(format!(
                "export was made by a newer Kotilo Deck (schemaVersion {v})"
            ))
        }
        _ => return Err("not a Kotilo Deck export (schemaVersion missing)".into()),
    }
    let settings = obj
        .get("settings")
        .and_then(Value::as_object)
        .cloned()
        .ok_or("export has no settings object")?;
    settings::merge(&DeckSettings::default(), &settings)?;

    let raw = obj
        .get("workspaces")
        .and_then(Value::as_array)
        .ok_or("export has no workspaces list")?;
    let mut ids = std::collections::HashSet::new();
    let mut workspaces = Vec::with_capacity(raw.len());
    for (i, w) in raw.iter().enumerate() {
        let (doc, _) =
            migrations::migrate_workspace(w.clone()).map_err(|e| format!("workspace #{i}: {e}"))?;
        let ws: Workspace =
            serde_json::from_value(doc).map_err(|e| format!("workspace #{i}: {e}"))?;
        WorkspaceStore::check(&ws).map_err(|e| format!("workspace #{i}: {e}"))?;
        if !ids.insert(ws.id.clone()) {
            return Err(format!("workspace id \"{}\" appears twice", ws.id));
        }
        workspaces.push(ws);
    }
    Ok((settings, workspaces))
}

/// Copies the current settings (as JSON) and every workspace file into a fresh folder.
pub fn backup(
    settings: &DeckSettings,
    workspaces_dir: &Path,
    root: &Path,
) -> Result<PathBuf, String> {
    let stamp = chrono::Local::now().format("%Y%m%dT%H%M%S%.3f");
    let dir = root.join(format!("import-{stamp}"));
    let ws_dir = dir.join("workspaces");
    fs::create_dir_all(&ws_dir).map_err(|e| format!("cannot create backup folder: {e}"))?;
    let s = serde_json::to_vec_pretty(settings).map_err(|e| e.to_string())?;
    fs::write(dir.join("settings.json"), s).map_err(|e| format!("cannot back up settings: {e}"))?;
    if let Ok(entries) = fs::read_dir(workspaces_dir) {
        for p in entries.filter_map(Result::ok).map(|e| e.path()) {
            if p.is_file() && p.extension().is_some_and(|x| x == "json") {
                if let Some(name) = p.file_name() {
                    fs::copy(&p, ws_dir.join(name))
                        .map_err(|e| format!("cannot back up {}: {e}", file_label(&p)))?;
                }
            }
        }
    }
    Ok(dir)
}

fn is_future_file(path: &Path) -> bool {
    fs::read(path)
        .ok()
        .and_then(|b| serde_json::from_slice::<Value>(&b).ok())
        .and_then(|d| migrations::version_of(&d).ok())
        .is_some_and(|v| v > WORKSPACE_VERSION)
}

fn workspace_files(dir: &Path) -> Vec<PathBuf> {
    fs::read_dir(dir)
        .map(|e| {
            e.filter_map(Result::ok)
                .map(|e| e.path())
                .filter(|p| p.is_file() && p.extension().is_some_and(|x| x == "json"))
                .collect()
        })
        .unwrap_or_default()
}

/// Ids of workspace files written by a newer build.
fn future_ids(files: &[PathBuf]) -> Vec<String> {
    files
        .iter()
        .filter(|p| is_future_file(p))
        .filter_map(|p| p.file_stem().and_then(|s| s.to_str()).map(String::from))
        .collect()
}

/// Refuses an import that would overwrite a workspace file from a newer build.
pub fn check_replaceable(dir: &Path, workspaces: &[Workspace]) -> Result<(), String> {
    let future = future_ids(&workspace_files(dir));
    match workspaces.iter().find(|w| future.contains(&w.id)) {
        Some(w) => Err(format!(
            "workspace \"{}\" was saved by a newer Kotilo Deck; not replacing it",
            w.id
        )),
        None => Ok(()),
    }
}

/// Replaces the workspace folder contents with `workspaces`. Files from a newer build are
/// kept (and importing over one of them is refused before anything is written).
pub fn replace_workspaces(
    store: &WorkspaceStore,
    dir: &Path,
    workspaces: &[Workspace],
) -> Result<(), String> {
    check_replaceable(dir, workspaces)?;
    let existing = workspace_files(dir);
    let future = future_ids(&existing);
    for ws in workspaces {
        store.save(ws)?;
    }
    let keep: Vec<&str> = workspaces.iter().map(|w| w.id.as_str()).collect();
    for p in existing {
        let Some(stem) = p.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        if keep.contains(&stem) || future.iter().any(|f| f == stem) {
            continue;
        }
        if let Err(e) = fs::remove_file(&p) {
            tracing::warn!(file = %file_label(&p), "cannot remove replaced workspace: {e}");
        }
    }
    Ok(())
}

#[tauri::command]
pub async fn settings_export_all(
    app: AppHandle,
    store: State<'_, WorkspaceStore>,
) -> Result<String, String> {
    export(settings::load(&app), store.list()?)
}

#[tauri::command]
pub async fn settings_import_all(
    app: AppHandle,
    store: State<'_, WorkspaceStore>,
    json: String,
) -> Result<(), String> {
    let (settings_patch, workspaces) = parse(&json)?;
    let dir = paths::workspaces_dir();
    check_replaceable(&dir, &workspaces)?;
    let backup_dir = backup(&settings::load(&app), &dir, &paths::backups_dir())?;
    tracing::info!(backup = %file_label(&backup_dir), workspaces = workspaces.len(), "importing settings");
    // Missing keys in the export take defaults, so import = full replace.
    let mut full = settings::to_map(&DeckSettings::default());
    full.extend(settings_patch);
    settings::apply_patch(&app, &full)?;
    let result = replace_workspaces(&store, &dir, &workspaces);
    // Even a partial workspace write changed files on disk.
    let _ = app.emit(EVENT_WORKSPACE_CHANGED, json!({}));
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ws(id: &str) -> Value {
        json!({"schemaVersion": 1, "id": id, "name": id, "theme": null,
               "grid": {"cols": 8, "rows": 4}, "widgets": []})
    }

    #[test]
    fn export_parse_round_trip() {
        let workspaces: Vec<Workspace> = vec![serde_json::from_value(ws("focus")).unwrap()];
        let s = DeckSettings {
            theme: Some("nord".into()),
            ..DeckSettings::default()
        };
        let text = export(s, workspaces.clone()).unwrap();
        let (settings, parsed) = parse(&text).unwrap();
        assert_eq!(settings["theme"], "nord");
        assert_eq!(parsed, workspaces);
    }

    #[test]
    fn parse_rejects_bad_documents() {
        let good_settings = json!({"theme": null});
        for bad in [
            json!([]),
            json!({"settings": good_settings, "workspaces": []}),
            json!({"schemaVersion": 2, "settings": good_settings, "workspaces": []}),
            json!({"schemaVersion": 1, "workspaces": []}),
            json!({"schemaVersion": 1, "settings": {"appearance": {"blur": 99}}, "workspaces": []}),
            json!({"schemaVersion": 1, "settings": good_settings}),
            json!({"schemaVersion": 1, "settings": good_settings, "workspaces": [ws("a"), ws("a")]}),
            json!({"schemaVersion": 1, "settings": good_settings, "workspaces": [ws("../x")]}),
            json!({"schemaVersion": 1, "settings": good_settings, "workspaces": [{"schemaVersion": 9}]}),
        ] {
            assert!(parse(&bad.to_string()).is_err(), "{bad}");
        }
        assert!(parse("{nope").is_err());
        // v0 workspaces are migrated.
        let mut legacy = ws("legacy");
        legacy.as_object_mut().unwrap().remove("schemaVersion");
        let doc = json!({"schemaVersion": 1, "settings": {}, "workspaces": [legacy]});
        assert_eq!(parse(&doc.to_string()).unwrap().1[0].schema_version, 1);
    }

    #[test]
    fn backup_then_replace() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("workspaces");
        let store = WorkspaceStore::new(dir.clone());
        for id in ["old", "keep"] {
            store
                .save(&serde_json::from_value(ws(id)).unwrap())
                .unwrap();
        }
        let future = r#"{"schemaVersion":7,"id":"future","name":"F","grid":{"cols":8,"rows":4},"widgets":[]}"#;
        fs::write(dir.join("future.json"), future).unwrap();

        let b = backup(&DeckSettings::default(), &dir, &root.path().join("backups")).unwrap();
        assert!(b.join("settings.json").is_file());
        assert!(b.join("workspaces").join("old.json").is_file());
        assert!(b.join("workspaces").join("future.json").is_file());

        let new: Vec<Workspace> = vec![
            serde_json::from_value(ws("keep")).unwrap(),
            serde_json::from_value(ws("fresh")).unwrap(),
        ];
        replace_workspaces(&store, &dir, &new).unwrap();
        let mut names: Vec<String> = fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        names.sort();
        assert_eq!(names, ["fresh.json", "future.json", "keep.json"]);
        assert_eq!(fs::read_to_string(dir.join("future.json")).unwrap(), future);

        let clash: Vec<Workspace> = vec![serde_json::from_value(ws("future")).unwrap()];
        assert!(replace_workspaces(&store, &dir, &clash).is_err());
    }
}
