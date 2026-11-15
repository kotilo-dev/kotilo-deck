//! On-disk locations under `~/.kotilo/deck/` (ARCHITECTURE.md "Data").

use std::path::PathBuf;
use std::sync::OnceLock;

/// Env var that moves all deck data to another folder (the idle soak run, `scripts/soak.mjs`).
/// Unset in normal use. `dirs::home_dir()` ignores `%USERPROFILE%` on Windows, so a
/// throwaway profile cannot be made by redirecting the home folder.
pub const DATA_DIR_ENV: &str = "KOTILO_DECK_DATA_DIR";

/// The [`DATA_DIR_ENV`] folder, when set to an absolute path. Read once per process.
pub fn data_dir_override() -> Option<&'static PathBuf> {
    static DIR: OnceLock<Option<PathBuf>> = OnceLock::new();
    DIR.get_or_init(|| parse_override(std::env::var_os(DATA_DIR_ENV)))
        .as_ref()
}

fn parse_override(raw: Option<std::ffi::OsString>) -> Option<PathBuf> {
    let p = PathBuf::from(raw?);
    p.is_absolute().then_some(p)
}

/// The tauri-plugin-store settings file. A relative path resolves against the app data dir
/// (`settings.json` there in normal use); with [`DATA_DIR_ENV`] set it lives in that folder.
pub fn settings_store_file() -> PathBuf {
    match data_dir_override() {
        Some(dir) => dir.join("settings.json"),
        None => PathBuf::from("settings.json"),
    }
}

/// `~/.kotilo/deck`, or the [`DATA_DIR_ENV`] folder. Falls back to the working directory
/// when the OS reports no home directory, so the deck still starts (and logs) instead of
/// failing.
pub fn deck_dir() -> PathBuf {
    if let Some(dir) = data_dir_override() {
        return dir.clone();
    }
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".kotilo")
        .join("deck")
}

pub fn logs_dir() -> PathBuf {
    deck_dir().join("logs")
}

pub fn workspaces_dir() -> PathBuf {
    deck_dir().join("workspaces")
}

pub fn weather_cache_dir() -> PathBuf {
    deck_dir().join("cache").join("weather")
}

pub fn wallpapers_dir() -> PathBuf {
    deck_dir().join("wallpapers")
}

/// Shared with other Kotilo apps, hence not under `deck/`.
pub fn user_themes_dir() -> PathBuf {
    deck_dir()
        .parent()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
        .join("themes")
}

/// Todos (and, from Phase 3, pomodoro history).
pub fn database_file() -> PathBuf {
    deck_dir().join("deck.db")
}

/// Overlay window size/position (not a user setting, so not in settings.json).
pub fn window_state_file() -> PathBuf {
    deck_dir().join("window-state.json")
}

/// Phase 4 plugin widgets, one folder each (`docs/plugin-api.md`).
pub fn plugin_widgets_dir() -> PathBuf {
    deck_dir().join("widgets")
}

pub fn backups_dir() -> PathBuf {
    deck_dir().join("backups")
}

/// File name only, for logs (they must not carry the user's home path).
pub fn file_label(path: &std::path::Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// `~/.kotilo`, shared by every Kotilo app.
pub fn kotilo_dir() -> PathBuf {
    deck_dir()
        .parent()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

/// Companion shared secret (protocol.md §2), shared with the Kotilo desktop app.
pub fn companion_token_file() -> PathBuf {
    kotilo_dir().join("companion.token")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn override_needs_an_absolute_path() {
        assert_eq!(parse_override(None), None);
        assert_eq!(parse_override(Some("".into())), None);
        assert_eq!(parse_override(Some("relative/dir".into())), None);
        let abs = std::env::temp_dir().join("kotilo-soak").join("deck");
        assert_eq!(parse_override(Some(abs.clone().into())), Some(abs));
    }
}
