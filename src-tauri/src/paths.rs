//! On-disk locations under `~/.kotilo/deck/` (ARCHITECTURE.md "Data").

use std::path::PathBuf;

/// `~/.kotilo/deck`. Falls back to the working directory when the OS reports no home
/// directory, so the deck still starts (and logs) instead of failing.
pub fn deck_dir() -> PathBuf {
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

pub fn backups_dir() -> PathBuf {
    deck_dir().join("backups")
}

/// File name only, for logs (they must not carry the user's home path).
pub fn file_label(path: &std::path::Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}
