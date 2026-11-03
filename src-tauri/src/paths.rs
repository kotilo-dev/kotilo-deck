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
