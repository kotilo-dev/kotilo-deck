//! Appearance assets and user files: wallpapers (`~/.kotilo/deck/wallpapers/`, served to
//! the webview through the asset protocol, scoped to that folder in tauri.conf.json), user
//! themes (`~/.kotilo/themes/*.json`, hot-reloaded via a debounced watcher) and native
//! open/save dialogs (WebView2 blob downloads are unreliable). Logs carry file names only.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

use notify::RecommendedWatcher;
use notify_debouncer_mini::{new_debouncer, DebounceEventResult, Debouncer};
use serde::Serialize;
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter, Manager, WebviewWindow};
use tauri_plugin_dialog::{DialogExt, FilePath};

use crate::paths::{self, file_label};

pub const EVENT_USER_THEMES_CHANGED: &str = "theme://user-changed";
const IMAGE_EXT: &[&str] = &["png", "jpg", "jpeg", "webp", "gif"];
const VIDEO_EXT: &[&str] = &["mp4", "webm"];
const MAX_THEME_BYTES: u64 = 1024 * 1024;
const MAX_TEXT_BYTES: u64 = 10 * 1024 * 1024;
const THEME_DEBOUNCE: Duration = Duration::from_millis(400);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum WallpaperKind {
    Image,
    Video,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Wallpaper {
    pub name: String,
    pub kind: WallpaperKind,
    pub path: String,
}

fn extension(path: &Path) -> Option<String> {
    path.extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
}

pub fn wallpaper_kind(path: &Path) -> Option<WallpaperKind> {
    let ext = extension(path)?;
    if IMAGE_EXT.contains(&ext.as_str()) {
        Some(WallpaperKind::Image)
    } else if VIDEO_EXT.contains(&ext.as_str()) {
        Some(WallpaperKind::Video)
    } else {
        None
    }
}

fn wallpaper_at(path: &Path) -> Option<Wallpaper> {
    let name = path.file_name()?.to_str()?.to_string();
    if !crate::settings::valid_file_name(&name) {
        return None;
    }
    Some(Wallpaper {
        kind: wallpaper_kind(path)?,
        path: path.to_string_lossy().into_owned(),
        name,
    })
}

/// Supported files directly in `dir`, by name. A missing folder is an empty list.
pub fn list_wallpapers(dir: &Path) -> Result<Vec<Wallpaper>, String> {
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(format!("cannot read wallpapers folder: {e}")),
    };
    let mut out: Vec<Wallpaper> = entries
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
        .filter_map(|e| wallpaper_at(&e.path()))
        .collect();
    out.sort_by_key(|w| w.name.to_lowercase());
    Ok(out)
}

/// Safe, readable stem: letters/digits/`-`/`_`/space kept, anything else becomes `-`.
fn clean_stem(stem: &str) -> String {
    let s: String = stem
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || matches!(c, '-' | '_' | ' ') {
                c
            } else {
                '-'
            }
        })
        .collect();
    let s = s.trim().trim_matches('-').to_string();
    if s.is_empty() {
        "wallpaper".into()
    } else {
        s.chars().take(100).collect()
    }
}

/// `<stem>.<ext>`, or `<stem>-2.<ext>`, `-3`, … if taken.
pub fn unique_target(dir: &Path, source: &Path) -> Option<PathBuf> {
    let ext = extension(source)?;
    let stem = clean_stem(source.file_stem()?.to_str().unwrap_or_default());
    (1..10_000).find_map(|n| {
        let name = if n == 1 {
            format!("{stem}.{ext}")
        } else {
            format!("{stem}-{n}.{ext}")
        };
        let p = dir.join(name);
        (!p.exists()).then_some(p)
    })
}

/// Copies `source` into `dir` under a unique name (files already in `dir` are reused).
pub fn import_wallpaper(dir: &Path, source: &Path) -> Result<Wallpaper, String> {
    if wallpaper_kind(source).is_none() {
        return Err("unsupported file type (png, jpg, webp, gif, mp4, webm)".into());
    }
    if source.parent() == Some(dir) {
        if let Some(w) = wallpaper_at(source) {
            return Ok(w);
        }
    }
    fs::create_dir_all(dir).map_err(|e| format!("cannot create wallpapers folder: {e}"))?;
    let target = unique_target(dir, source).ok_or("cannot find a free file name")?;
    if let Err(e) = fs::copy(source, &target) {
        let _ = fs::remove_file(&target);
        return Err(format!("cannot copy wallpaper: {e}"));
    }
    tracing::info!(file = %file_label(&target), "wallpaper imported");
    wallpaper_at(&target).ok_or_else(|| "imported file name is not usable".into())
}

/// Raw JSON values of `dir/*.json`, by file name; unreadable files are skipped.
pub fn list_user_themes(dir: &Path) -> Vec<Value> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut files: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| extension(p).as_deref() == Some("json") && p.is_file())
        .collect();
    files.sort();
    files
        .into_iter()
        .filter_map(|p| {
            let too_big = fs::metadata(&p).is_ok_and(|m| m.len() > MAX_THEME_BYTES);
            if too_big {
                tracing::warn!(file = %file_label(&p), "skipping user theme: file too large");
                return None;
            }
            match fs::read(&p)
                .map_err(|e| e.to_string())
                .and_then(|b| serde_json::from_slice::<Value>(&b).map_err(|e| e.to_string()))
            {
                Ok(v) => Some(v),
                Err(e) => {
                    tracing::warn!(file = %file_label(&p), "skipping user theme: {e}");
                    None
                }
            }
        })
        .collect()
}

/// Keeps the user-theme folder watcher alive.
#[derive(Default)]
pub struct ThemeWatcher(Mutex<Option<Debouncer<RecommendedWatcher>>>);

/// Watches `~/.kotilo/themes/` (created if missing) and emits `theme://user-changed`
/// after a quiet period. The watcher thread blocks on OS notifications (no polling).
pub fn watch_user_themes(app: &AppHandle) {
    let dir = paths::user_themes_dir();
    if let Err(e) = fs::create_dir_all(&dir) {
        tracing::warn!("cannot create user themes folder: {e}");
        return;
    }
    let handle = app.clone();
    let debouncer = new_debouncer(THEME_DEBOUNCE, move |res: DebounceEventResult| match res {
        Ok(events) => {
            let relevant = events
                .iter()
                .any(|e| extension(&e.path).as_deref() == Some("json"));
            if relevant {
                tracing::debug!("user themes changed");
                let _ = handle.emit(EVENT_USER_THEMES_CHANGED, json!({}));
            }
        }
        Err(e) => tracing::warn!("user theme watcher error: {e}"),
    });
    let mut debouncer = match debouncer {
        Ok(d) => d,
        Err(e) => {
            tracing::warn!("cannot start user theme watcher: {e}");
            return;
        }
    };
    if let Err(e) = debouncer
        .watcher()
        .watch(&dir, notify::RecursiveMode::NonRecursive)
    {
        tracing::warn!("cannot watch user themes folder: {e}");
        return;
    }
    *app.state::<ThemeWatcher>()
        .0
        .lock()
        .unwrap_or_else(|p| p.into_inner()) = Some(debouncer);
}

/// `["json", ".txt"]` → `["json", "txt"]`, dropping anything that is not a plain extension.
fn clean_extensions(extensions: &[String]) -> Vec<String> {
    extensions
        .iter()
        .map(|e| e.trim().trim_start_matches('.').to_ascii_lowercase())
        .filter(|e| !e.is_empty() && e.chars().all(|c| c.is_ascii_alphanumeric()))
        .collect()
}

async fn wait_for_path(
    start: impl FnOnce(Box<dyn FnOnce(Option<FilePath>) + Send>),
) -> Result<Option<PathBuf>, String> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    start(Box::new(move |p| {
        let _ = tx.send(p);
    }));
    match rx.await {
        Ok(Some(p)) => p
            .into_path()
            .map(Some)
            .map_err(|e| format!("unusable path: {e}")),
        Ok(None) | Err(_) => Ok(None),
    }
}

async fn blocking<T: Send + 'static>(
    f: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(f)
        .await
        .map_err(|e| format!("background task failed: {e}"))?
}

#[tauri::command]
pub async fn wallpaper_list() -> Result<Vec<Wallpaper>, String> {
    blocking(|| list_wallpapers(&paths::wallpapers_dir())).await
}

#[tauri::command]
pub async fn wallpaper_import(window: WebviewWindow) -> Result<Option<Wallpaper>, String> {
    let mut all: Vec<&str> = IMAGE_EXT.to_vec();
    all.extend_from_slice(VIDEO_EXT);
    let builder = window
        .dialog()
        .file()
        .set_parent(&window)
        .set_title("Import wallpaper")
        .add_filter("Images and videos", &all)
        .add_filter("Images", IMAGE_EXT)
        .add_filter("Videos", VIDEO_EXT);
    let Some(source) = wait_for_path(|cb| builder.pick_file(cb)).await? else {
        return Ok(None);
    };
    blocking(move || import_wallpaper(&paths::wallpapers_dir(), &source).map(Some)).await
}

#[tauri::command]
pub async fn theme_user_list() -> Result<Vec<Value>, String> {
    blocking(|| Ok(list_user_themes(&paths::user_themes_dir()))).await
}

#[tauri::command]
pub async fn file_save_as(
    window: WebviewWindow,
    default_name: String,
    contents: String,
) -> Result<bool, String> {
    let mut builder = window
        .dialog()
        .file()
        .set_parent(&window)
        .set_file_name(&default_name);
    let ext = clean_extensions(&[extension(Path::new(&default_name)).unwrap_or_default()]);
    if let Some(ext) = ext.first() {
        builder = builder.add_filter(ext.to_uppercase(), &[ext.as_str()]);
    }
    let Some(target) = wait_for_path(|cb| builder.save_file(cb)).await? else {
        return Ok(false);
    };
    blocking(move || {
        crate::workspace::write_atomic(&target, contents.as_bytes())
            .map_err(|e| format!("cannot write file: {e}"))?;
        tracing::info!(file = %file_label(&target), "file saved");
        Ok(true)
    })
    .await
}

#[tauri::command]
pub async fn file_open_text(
    window: WebviewWindow,
    extensions: Vec<String>,
) -> Result<Option<String>, String> {
    let exts = clean_extensions(&extensions);
    let mut builder = window.dialog().file().set_parent(&window);
    if !exts.is_empty() {
        let refs: Vec<&str> = exts.iter().map(String::as_str).collect();
        builder = builder.add_filter(exts.join(", ").to_uppercase(), &refs);
    }
    let Some(source) = wait_for_path(|cb| builder.pick_file(cb)).await? else {
        return Ok(None);
    };
    blocking(move || {
        let len = fs::metadata(&source).map(|m| m.len()).unwrap_or(0);
        if len > MAX_TEXT_BYTES {
            return Err("file is larger than 10 MB".into());
        }
        let bytes = fs::read(&source).map_err(|e| format!("cannot read file: {e}"))?;
        String::from_utf8(bytes)
            .map(Some)
            .map_err(|_| "file is not UTF-8 text".into())
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kinds_by_extension() {
        assert_eq!(
            wallpaper_kind(Path::new("a.JPG")),
            Some(WallpaperKind::Image)
        );
        assert_eq!(
            wallpaper_kind(Path::new("a.webm")),
            Some(WallpaperKind::Video)
        );
        assert_eq!(wallpaper_kind(Path::new("a.bmp")), None);
        assert_eq!(wallpaper_kind(Path::new("noext")), None);
    }

    #[test]
    fn import_copies_with_unique_names_and_lists() {
        let src = tempfile::tempdir().unwrap();
        let dest = tempfile::tempdir().unwrap();
        let wp = dest.path().join("wallpapers");
        let file = src.path().join("My Mountains!.JPG");
        fs::write(&file, b"img").unwrap();
        fs::write(src.path().join("notes.txt"), b"x").unwrap();

        assert!(list_wallpapers(&wp).unwrap().is_empty());
        let a = import_wallpaper(&wp, &file).unwrap();
        assert_eq!(a.name, "My Mountains.jpg");
        assert_eq!(a.kind, WallpaperKind::Image);
        let b = import_wallpaper(&wp, &file).unwrap();
        assert_eq!(b.name, "My Mountains-2.jpg");
        // Importing a file that already lives in the folder reuses it.
        let c = import_wallpaper(&wp, Path::new(&a.path)).unwrap();
        assert_eq!(c.name, a.name);
        assert!(import_wallpaper(&wp, &src.path().join("notes.txt")).is_err());

        fs::write(wp.join("clip.mp4"), b"v").unwrap();
        fs::write(wp.join("readme.md"), b"x").unwrap();
        let names: Vec<String> = list_wallpapers(&wp)
            .unwrap()
            .into_iter()
            .map(|w| w.name)
            .collect();
        assert_eq!(
            names,
            ["clip.mp4", "My Mountains-2.jpg", "My Mountains.jpg"]
        );
    }

    #[test]
    fn stems_are_cleaned() {
        assert_eq!(clean_stem("../../evil"), "evil");
        assert_eq!(clean_stem("???"), "wallpaper");
        assert_eq!(clean_stem("çay bahçesi"), "çay bahçesi");
    }

    #[test]
    fn user_themes_skip_unparseable() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("b.json"), r#"{"id":"b"}"#).unwrap();
        fs::write(dir.path().join("a.json"), r#"{"id":"a"}"#).unwrap();
        fs::write(dir.path().join("broken.json"), "{nope").unwrap();
        fs::write(dir.path().join("other.txt"), "{}").unwrap();
        let ids: Vec<Value> = list_user_themes(dir.path())
            .into_iter()
            .map(|v| v["id"].clone())
            .collect();
        assert_eq!(ids, [json!("a"), json!("b")]);
        assert!(list_user_themes(&dir.path().join("missing")).is_empty());
    }

    #[test]
    fn extensions_are_normalised() {
        let input = [".JSON", "txt", "", "a/b", " md "].map(String::from);
        assert_eq!(clean_extensions(&input), ["json", "txt", "md"]);
    }
}
