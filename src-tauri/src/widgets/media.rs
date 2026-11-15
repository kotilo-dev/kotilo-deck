//! Media widget backend (capability `media`). OS work goes through `platform::current()`.
//!
//! `media_subscribe` is ref-counted: while at least one widget is subscribed an OS watcher
//! pushes `media://state` on session/playback/metadata/timeline events (debounced, no polling).

use std::sync::Mutex;

use tauri::{AppHandle, Emitter, Manager};

use super::watch::{self, blocking, Live, Owner, Subscription};
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

/// Leaves out artwork this watcher already sent (≈ 200 KB per event otherwise): listeners
/// keep the last artwork while `artworkKey` is unchanged. `last` is per watcher, so a fresh
/// watcher always sends it once; late subscribers and reloaded pages get it from
/// `media_state`, which is never stripped.
fn strip_sent_artwork(state: &mut MediaState, last: &mut Option<String>) {
    let key = state.session.as_ref().and_then(|s| s.artwork_key.clone());
    if key.is_some() && key == *last {
        if let Some(s) = state.session.as_mut() {
            s.artwork = None;
        }
    } else {
        *last = key;
    }
}

fn start(app: AppHandle) -> Option<Live> {
    let last_artwork = Mutex::new(None::<String>);
    watch::start(
        "media",
        |on_change| platform::current().watch_media(on_change),
        move || match platform::current().media_state() {
            Ok(mut s) => {
                strip_sent_artwork(
                    &mut s,
                    &mut last_artwork.lock().unwrap_or_else(|p| p.into_inner()),
                );
                let _ = app.emit(EVENT_STATE, s);
            }
            Err(e) => tracing::debug!("media state after change: {e}"),
        },
    )
}

/// Starting/stopping the watcher registers/unregisters WinRT handlers, so it runs on the
/// blocking pool rather than an async worker.
pub async fn subscribe_as(app: AppHandle, owner: Owner) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let hub = app.state::<MediaHub>();
        hub.0.subscribe_as(owner, || start(app.clone()));
    })
    .await
    .map_err(|e| e.to_string())
}

pub async fn unsubscribe_as(app: AppHandle, owner: Owner) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || app.state::<MediaHub>().0.unsubscribe_as(owner))
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn media_subscribe(app: AppHandle) -> Result<(), String> {
    subscribe_as(app, Owner::Deck).await
}

#[tauri::command]
pub async fn media_unsubscribe(app: AppHandle) -> Result<(), String> {
    unsubscribe_as(app, Owner::Deck).await
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

    fn state(art: Option<&str>) -> MediaState {
        MediaState {
            session: Some(platform::MediaSession {
                source: "S".into(),
                title: "T".into(),
                artist: "A".into(),
                album: None,
                artwork: art.map(Into::into),
                artwork_key: art.map(|a| format!("k-{a}")),
                playing: true,
                position: None,
                duration: None,
                position_at: 0,
                shuffle: None,
                repeat: None,
                can_prev: false,
                can_next: false,
                can_play_pause: true,
            }),
        }
    }

    fn art_of(s: &MediaState) -> (Option<&str>, Option<&str>) {
        let s = s.session.as_ref().unwrap();
        (s.artwork.as_deref(), s.artwork_key.as_deref())
    }

    #[test]
    fn unchanged_artwork_is_sent_once() {
        let mut last = None;
        let mut first = state(Some("a"));
        strip_sent_artwork(&mut first, &mut last);
        assert_eq!(art_of(&first), (Some("a"), Some("k-a")));
        let mut again = state(Some("a"));
        strip_sent_artwork(&mut again, &mut last);
        assert_eq!(art_of(&again), (None, Some("k-a")), "key only");
        let mut next = state(Some("b"));
        strip_sent_artwork(&mut next, &mut last);
        assert_eq!(art_of(&next), (Some("b"), Some("k-b")));
        // No artwork, then the old one again: sent again in full.
        let mut none = state(None);
        strip_sent_artwork(&mut none, &mut last);
        assert_eq!(art_of(&none), (None, None));
        let mut back = state(Some("b"));
        strip_sent_artwork(&mut back, &mut last);
        assert_eq!(art_of(&back), (Some("b"), Some("k-b")));
        // Nothing playing resets too.
        let mut idle = MediaState { session: None };
        strip_sent_artwork(&mut idle, &mut last);
        assert_eq!(last, None);
    }

    #[test]
    fn hub_reset_on_empty_is_noop() {
        let hub = MediaHub::default();
        hub.reset();
        assert_eq!(hub.0.count(), 0);
    }
}
