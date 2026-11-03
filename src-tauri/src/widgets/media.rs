//! Media widget backend (capability `media`). OS work goes through `platform::current()`.
//!
//! `media_subscribe` is ref-counted: while at least one widget is subscribed an OS watcher
//! pushes `media://state` on session/playback/metadata/timeline events (debounced, no polling).

use tauri::{AppHandle, Emitter, Manager};

use super::watch::{self, blocking, Live, Subscription};
use crate::platform::{self, MediaAction, MediaState, PlatformError};

pub const EVENT_STATE: &str = "media://state";

#[derive(Default)]
pub struct MediaHub(Subscription<Live>);

impl MediaHub {
    /// Called on webview (re)load; see `system::Sampler::reset`.
    pub fn reset(&self) {
        let n = self.0.reset();
        if n > 0 {
            tracing::debug!(subscribers = n, "page load: resetting media watcher");
        }
    }
}

#[tauri::command]
pub async fn media_state() -> Result<MediaState, String> {
    blocking(|| platform::current().media_state()).await
}

#[tauri::command]
pub async fn media_control(action: MediaAction) -> Result<(), String> {
    blocking(move || platform::current().media_control(action)).await
}

fn seek_target(seconds: f64) -> platform::Result<f64> {
    if seconds.is_finite() {
        Ok(seconds.max(0.0))
    } else {
        Err(PlatformError::Os("seek position must be a number".into()))
    }
}

#[tauri::command]
pub async fn media_seek(seconds: f64) -> Result<(), String> {
    blocking(move || platform::current().media_seek(seek_target(seconds)?)).await
}

fn start(app: AppHandle) -> Option<Live> {
    watch::start(
        "media",
        |on_change| platform::current().watch_media(on_change),
        move || match platform::current().media_state() {
            Ok(s) => {
                let _ = app.emit(EVENT_STATE, s);
            }
            Err(e) => tracing::debug!("media state after change: {e}"),
        },
    )
}

/// Starting/stopping the watcher registers/unregisters WinRT handlers, so it runs on the
/// blocking pool rather than an async worker.
#[tauri::command]
pub async fn media_subscribe(app: AppHandle) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let hub = app.state::<MediaHub>();
        hub.0.subscribe(|| start(app.clone()));
    })
    .await
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn media_unsubscribe(app: AppHandle) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || app.state::<MediaHub>().0.unsubscribe())
        .await
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seek_target_is_validated() {
        assert_eq!(seek_target(12.5).unwrap(), 12.5);
        assert_eq!(seek_target(-3.0).unwrap(), 0.0);
        assert!(seek_target(f64::NAN).is_err());
        assert!(seek_target(f64::INFINITY).is_err());
    }

    #[test]
    fn hub_reset_on_empty_is_noop() {
        let hub = MediaHub::default();
        hub.reset();
        assert_eq!(hub.0.count(), 0);
    }
}
