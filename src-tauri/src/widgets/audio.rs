//! Volume widget backend (capability `audio`). OS work goes through `platform::current()`.
//!
//! `audio_subscribe` is ref-counted: while at least one widget is subscribed an OS watcher
//! pushes `audio://state` on volume/mute/device/session changes (debounced, no polling).

use tauri::{AppHandle, Emitter, Manager};

use super::watch::{self, blocking, Live, Owner, Subscription};
use crate::platform::{self, AudioSession, AudioState, PlatformError};

pub const EVENT_STATE: &str = "audio://state";

#[derive(Default)]
pub struct AudioHub(Subscription<Live>);

impl AudioHub {
    /// Called on webview (re)load; see `system::Sampler::reset`.
    pub fn reset(&self) {
        let n = self.0.reset();
        if n > 0 {
            tracing::debug!(subscribers = n, "page load: resetting audio watcher");
        }
    }
}

fn level(volume: f64) -> platform::Result<f32> {
    platform::unit_level(volume).ok_or_else(|| PlatformError::Os("volume must be a number".into()))
}

#[tauri::command]
pub async fn audio_state() -> Result<AudioState, String> {
    blocking(|| platform::current().audio_state()).await
}

#[tauri::command]
pub async fn audio_set_volume(volume: f64) -> Result<(), String> {
    blocking(move || platform::current().set_volume(level(volume)?)).await
}

#[tauri::command]
pub async fn audio_set_muted(muted: bool) -> Result<(), String> {
    blocking(move || platform::current().set_muted(muted)).await
}

#[tauri::command]
pub async fn audio_set_default_device(id: String) -> Result<(), String> {
    blocking(move || platform::current().set_default_audio_device(&id)).await
}

#[tauri::command]
pub async fn audio_sessions() -> Result<Vec<AudioSession>, String> {
    blocking(|| platform::current().audio_sessions()).await
}

#[tauri::command]
pub async fn audio_set_session_volume(id: String, volume: f64) -> Result<(), String> {
    blocking(move || platform::current().set_session_volume(&id, level(volume)?)).await
}

#[tauri::command]
pub async fn audio_set_session_muted(id: String, muted: bool) -> Result<(), String> {
    blocking(move || platform::current().set_session_muted(&id, muted)).await
}

fn start(app: AppHandle) -> Option<Live> {
    watch::start(
        "audio",
        |on_change| platform::current().watch_audio(on_change),
        move || match platform::current().audio_state() {
            Ok(s) => {
                let _ = app.emit(EVENT_STATE, s);
            }
            Err(e) => tracing::debug!("audio state after change: {e}"),
        },
    )
}

/// Starting/stopping the watcher registers/unregisters COM callbacks, so it runs on the
/// blocking pool rather than an async worker.
pub async fn subscribe_as(app: AppHandle, owner: Owner) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let hub = app.state::<AudioHub>();
        hub.0.subscribe_as(owner, || start(app.clone()));
    })
    .await
    .map_err(|e| e.to_string())
}

pub async fn unsubscribe_as(app: AppHandle, owner: Owner) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || app.state::<AudioHub>().0.unsubscribe_as(owner))
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn audio_subscribe(app: AppHandle) -> Result<(), String> {
    subscribe_as(app, Owner::Deck).await
}

#[tauri::command]
pub async fn audio_unsubscribe(app: AppHandle) -> Result<(), String> {
    unsubscribe_as(app, Owner::Deck).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn level_clamps_and_rejects_nan() {
        assert_eq!(level(2.0).unwrap(), 1.0);
        assert_eq!(level(-1.0).unwrap(), 0.0);
        assert!(level(f64::NAN).is_err());
    }

    #[test]
    fn hub_reset_on_empty_is_noop() {
        let hub = AudioHub::default();
        hub.reset();
        assert_eq!(hub.0.count(), 0);
    }
}
