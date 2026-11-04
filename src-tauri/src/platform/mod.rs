//! Platform abstraction shared with Kotilo Shell. See ARCHITECTURE.md and the
//! `tauri-platform-layer` skill. Every OS-specific call goes through [`Platform`];
//! nothing outside this module may use `windows` / D-Bus / `pactl` directly.

use std::sync::Arc;

use serde::{Deserialize, Serialize};

// Also compiled (not selected) in tests on every OS so the Linux parsers and D-Bus mapping
// are type-checked and unit-tested on the Windows dev machine (ARCHITECTURE.md).
#[cfg(any(target_os = "linux", test))]
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
mod linux;
#[cfg(any(target_os = "windows", target_os = "linux", test))]
#[cfg_attr(not(any(target_os = "windows", target_os = "linux")), allow(dead_code))]
mod shared;
#[cfg(not(any(target_os = "windows", target_os = "linux")))]
mod stub;
#[cfg(target_os = "windows")]
mod windows;

/// Platform features a widget can require in its manifest (`capabilities`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Capability {
    Monitors,
    SystemInfo,
    Gpu,
    Audio,
    Media,
    Notifications,
    Transparency,
    Companion,
}

#[derive(Debug, thiserror::Error)]
pub enum PlatformError {
    #[error("{0:?} is not available on this OS")]
    Unsupported(Capability),
    #[error("platform call failed: {0}")]
    Os(String),
}

pub type Result<T> = std::result::Result<T, PlatformError>;

/// Sizes are physical pixels; divide by `scale_factor` for CSS pixels.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MonitorInfo {
    /// Stable name used to re-attach when the monitor reconnects.
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub scale_factor: f64,
    pub primary: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioDevice {
    pub id: String,
    pub name: String,
    pub is_default: bool,
}

/// Opaque native top-level window handle (`HWND` on Windows). Obtained from Tauri in
/// `window.rs`; only the platform modules interpret it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NativeWindow(pub isize);

/// Mirrors `AudioState` in `src/lib/ipc/index.ts`.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioState {
    /// Default output device; `None` when there is none.
    pub device: Option<AudioDevice>,
    /// 0–1 master volume of the default output.
    pub volume: f32,
    pub muted: bool,
    pub devices: Vec<AudioDevice>,
}

/// One app's audio session on the default output (per-app mixer). Mirrors `AudioSession`.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioSession {
    pub id: String,
    pub name: String,
    pub volume: f32,
    pub muted: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum RepeatMode {
    None,
    Track,
    List,
}

impl RepeatMode {
    /// `cycle-repeat` order, same as Spotify: off → all (list) → one (track) → off.
    pub fn next(self) -> Self {
        match self {
            RepeatMode::None => RepeatMode::List,
            RepeatMode::List => RepeatMode::Track,
            RepeatMode::Track => RepeatMode::None,
        }
    }
}

/// Mirrors `MediaSession` in `src/lib/ipc/index.ts`.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaSession {
    pub source: String,
    pub title: String,
    pub artist: String,
    pub album: Option<String>,
    /// `data:` URL (png/jpeg), size-capped by the platform.
    pub artwork: Option<String>,
    pub playing: bool,
    /// Seconds; `None` when the source reports no timeline.
    pub position: Option<f64>,
    pub duration: Option<f64>,
    /// Unix ms when `position` was read.
    pub position_at: i64,
    pub shuffle: Option<bool>,
    pub repeat: Option<RepeatMode>,
    pub can_prev: bool,
    pub can_next: bool,
    pub can_play_pause: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaState {
    /// `None` = nothing playing.
    pub session: Option<MediaSession>,
}

/// `MediaAction` in `src/lib/ipc/index.ts`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum MediaAction {
    PlayPause,
    Next,
    Prev,
    ToggleShuffle,
    CycleRepeat,
}

/// "Something changed" signal from an OS watcher. Invoked on arbitrary OS threads, possibly
/// in bursts; it must be cheap and non-blocking (callers debounce and re-read the state).
pub type ChangeFn = Arc<dyn Fn() + Send + Sync>;

/// A live OS change watcher. Dropping it unregisters the OS callbacks and ends its thread.
pub struct Watch(#[allow(dead_code)] Box<dyn Send>);

impl Watch {
    pub fn new(guard: impl Send + 'static) -> Self {
        Self(Box::new(guard))
    }
}

/// Clamps a 0–1 level; `None` for NaN/infinite input.
pub fn unit_level(v: f64) -> Option<f32> {
    v.is_finite().then(|| v.clamp(0.0, 1.0) as f32)
}

/// OS integration surface. Methods default to `Unsupported` so a new platform
/// compiles as soon as it implements `capabilities()`; fill in per phase.
pub trait Platform: Send + Sync {
    fn capabilities(&self) -> Vec<Capability>;

    fn has(&self, capability: Capability) -> bool {
        self.capabilities().contains(&capability)
    }

    // Phase 2: Volume widget. Levels are 0–1 and clamped by the implementation.
    fn audio_devices(&self) -> Result<Vec<AudioDevice>> {
        Err(PlatformError::Unsupported(Capability::Audio))
    }
    fn volume(&self) -> Result<f32> {
        Err(PlatformError::Unsupported(Capability::Audio))
    }
    fn set_volume(&self, _level: f32) -> Result<()> {
        Err(PlatformError::Unsupported(Capability::Audio))
    }
    fn set_muted(&self, _muted: bool) -> Result<()> {
        Err(PlatformError::Unsupported(Capability::Audio))
    }
    /// Default output, its volume/mute and every active output device.
    fn audio_state(&self) -> Result<AudioState> {
        Err(PlatformError::Unsupported(Capability::Audio))
    }
    /// Makes `id` (an [`AudioDevice::id`]) the default output.
    fn set_default_audio_device(&self, _id: &str) -> Result<()> {
        Err(PlatformError::Unsupported(Capability::Audio))
    }
    /// Per-app sessions on the default output.
    fn audio_sessions(&self) -> Result<Vec<AudioSession>> {
        Err(PlatformError::Unsupported(Capability::Audio))
    }
    fn set_session_volume(&self, _id: &str, _level: f32) -> Result<()> {
        Err(PlatformError::Unsupported(Capability::Audio))
    }
    fn set_session_muted(&self, _id: &str, _muted: bool) -> Result<()> {
        Err(PlatformError::Unsupported(Capability::Audio))
    }
    /// Calls `on_change` on volume/mute, device add/remove/default and session changes.
    fn watch_audio(&self, _on_change: ChangeFn) -> Result<Watch> {
        Err(PlatformError::Unsupported(Capability::Audio))
    }

    // Phase 2: Media widget.
    fn media_state(&self) -> Result<MediaState> {
        Err(PlatformError::Unsupported(Capability::Media))
    }
    fn media_control(&self, _action: MediaAction) -> Result<()> {
        Err(PlatformError::Unsupported(Capability::Media))
    }
    /// Seeks the active session to `seconds` from the start of the track.
    fn media_seek(&self, _seconds: f64) -> Result<()> {
        Err(PlatformError::Unsupported(Capability::Media))
    }
    /// Calls `on_change` on session add/remove/switch, playback, metadata and timeline changes.
    fn watch_media(&self, _on_change: ChangeFn) -> Result<Watch> {
        Err(PlatformError::Unsupported(Capability::Media))
    }

    // Phase 1: System widget GPU tile (CPU/RAM come from `sysinfo`, which is portable).
    /// Busiest GPU engine utilisation, 0–100 (Task Manager's "GPU" figure).
    fn gpu_usage(&self) -> Result<f32> {
        Err(PlatformError::Unsupported(Capability::Gpu))
    }
    /// Name of the primary hardware GPU adapter.
    fn gpu_name(&self) -> Result<String> {
        Err(PlatformError::Unsupported(Capability::Gpu))
    }
    /// Frees GPU counter state when sampling stops, so a restart doesn't average the idle gap.
    fn gpu_release(&self) {}

    // Phase 2: overlay mode. (A transparent *background* needs no OS call: the deck window
    // is created with Tauri `transparent: true` and the page paints its own background.)
    /// Whole-window opacity, 0–1 (1 removes the layered style again so fullscreen pays
    /// nothing). Must be re-applied after the window toolkit rewrites the window styles
    /// (fullscreen / always-on-top changes).
    fn set_window_opacity(&self, _window: NativeWindow, _opacity: f64) -> Result<()> {
        Err(PlatformError::Unsupported(Capability::Transparency))
    }

    // Phase 3: Agent Status "open Kotilo desktop".
    /// Opens a custom-scheme URL (e.g. `kotilo://open`) with its registered handler. Errors
    /// (`Os`) when no app is registered for the scheme instead of showing an OS picker.
    fn open_url_scheme(&self, _url: &str) -> Result<()> {
        Err(PlatformError::Unsupported(Capability::Companion))
    }
}

/// The implementation for the OS this binary was built for.
pub fn current() -> &'static dyn Platform {
    #[cfg(target_os = "windows")]
    {
        &windows::WindowsPlatform
    }
    #[cfg(target_os = "linux")]
    {
        &linux::LinuxPlatform
    }
    #[cfg(not(any(target_os = "windows", target_os = "linux")))]
    {
        &stub::StubPlatform
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Bare;
    impl Platform for Bare {
        fn capabilities(&self) -> Vec<Capability> {
            vec![Capability::Monitors]
        }
    }

    #[test]
    fn unimplemented_methods_report_unsupported() {
        let p = Bare;
        assert!(p.has(Capability::Monitors));
        assert!(!p.has(Capability::Audio));
        assert!(matches!(
            p.volume(),
            Err(PlatformError::Unsupported(Capability::Audio))
        ));
    }

    #[test]
    fn gpu_defaults_to_unsupported() {
        assert!(matches!(
            Bare.gpu_usage(),
            Err(PlatformError::Unsupported(Capability::Gpu))
        ));
        assert!(matches!(
            Bare.gpu_name(),
            Err(PlatformError::Unsupported(Capability::Gpu))
        ));
        Bare.gpu_release(); // no-op default
    }

    #[test]
    fn audio_and_media_default_to_unsupported() {
        let noop: ChangeFn = Arc::new(|| {});
        assert!(matches!(
            Bare.audio_state(),
            Err(PlatformError::Unsupported(Capability::Audio))
        ));
        assert!(matches!(
            Bare.audio_sessions(),
            Err(PlatformError::Unsupported(Capability::Audio))
        ));
        assert!(matches!(
            Bare.watch_audio(noop.clone()),
            Err(PlatformError::Unsupported(Capability::Audio))
        ));
        assert!(matches!(
            Bare.set_default_audio_device("x"),
            Err(PlatformError::Unsupported(Capability::Audio))
        ));
        assert!(matches!(
            Bare.media_state(),
            Err(PlatformError::Unsupported(Capability::Media))
        ));
        assert!(matches!(
            Bare.media_control(MediaAction::Next),
            Err(PlatformError::Unsupported(Capability::Media))
        ));
        assert!(matches!(
            Bare.media_seek(1.0),
            Err(PlatformError::Unsupported(Capability::Media))
        ));
        assert!(matches!(
            Bare.watch_media(noop),
            Err(PlatformError::Unsupported(Capability::Media))
        ));
    }

    #[test]
    fn unit_level_clamps_and_rejects_nan() {
        assert_eq!(unit_level(1.7), Some(1.0));
        assert_eq!(unit_level(-0.2), Some(0.0));
        assert_eq!(unit_level(0.25), Some(0.25));
        assert_eq!(unit_level(f64::NAN), None);
        assert_eq!(unit_level(f64::INFINITY), None);
    }

    #[test]
    fn repeat_cycles_like_spotify() {
        assert_eq!(RepeatMode::None.next(), RepeatMode::List);
        assert_eq!(RepeatMode::List.next(), RepeatMode::Track);
        assert_eq!(RepeatMode::Track.next(), RepeatMode::None);
    }

    #[test]
    fn media_action_parses_ts_strings() {
        let parse = |s: &str| serde_json::from_value::<MediaAction>(serde_json::json!(s)).ok();
        assert_eq!(parse("play-pause"), Some(MediaAction::PlayPause));
        assert_eq!(parse("next"), Some(MediaAction::Next));
        assert_eq!(parse("prev"), Some(MediaAction::Prev));
        assert_eq!(parse("toggle-shuffle"), Some(MediaAction::ToggleShuffle));
        assert_eq!(parse("cycle-repeat"), Some(MediaAction::CycleRepeat));
        assert_eq!(parse("stop"), None);
    }

    #[test]
    fn audio_state_matches_ts_contract() {
        let dev = AudioDevice {
            id: "{0.0.0}".into(),
            name: "Speakers".into(),
            is_default: true,
        };
        let v = serde_json::to_value(AudioState {
            device: Some(dev.clone()),
            volume: 0.5,
            muted: false,
            devices: vec![dev],
        })
        .unwrap();
        assert_eq!(
            v,
            serde_json::json!({
                "device": {"id": "{0.0.0}", "name": "Speakers", "isDefault": true},
                "volume": 0.5,
                "muted": false,
                "devices": [{"id": "{0.0.0}", "name": "Speakers", "isDefault": true}],
            })
        );
        let empty = serde_json::to_value(AudioState {
            device: None,
            volume: 0.0,
            muted: false,
            devices: vec![],
        })
        .unwrap();
        assert!(empty["device"].is_null());
        let s = serde_json::to_value(AudioSession {
            id: "s".into(),
            name: "Spotify".into(),
            volume: 1.0,
            muted: true,
        })
        .unwrap();
        assert_eq!(
            s,
            serde_json::json!({"id": "s", "name": "Spotify", "volume": 1.0, "muted": true})
        );
    }

    #[test]
    fn media_state_matches_ts_contract() {
        let session = MediaSession {
            source: "Spotify".into(),
            title: "T".into(),
            artist: "A".into(),
            album: None,
            artwork: None,
            playing: true,
            position: Some(1.5),
            duration: Some(200.0),
            position_at: 42,
            shuffle: Some(false),
            repeat: Some(RepeatMode::Track),
            can_prev: true,
            can_next: true,
            can_play_pause: true,
        };
        let v = serde_json::to_value(MediaState {
            session: Some(session),
        })
        .unwrap();
        let s = &v["session"];
        let mut keys: Vec<&str> = s.as_object().unwrap().keys().map(|k| k.as_str()).collect();
        keys.sort_unstable();
        assert_eq!(
            keys,
            [
                "album",
                "artist",
                "artwork",
                "canNext",
                "canPlayPause",
                "canPrev",
                "duration",
                "playing",
                "position",
                "positionAt",
                "repeat",
                "shuffle",
                "source",
                "title"
            ]
        );
        assert_eq!(s["repeat"], "track");
        assert!(s["album"].is_null());
        assert_eq!(serde_json::json!(RepeatMode::None), "none");
        assert_eq!(serde_json::json!(RepeatMode::List), "list");
        let none = serde_json::to_value(MediaState { session: None }).unwrap();
        assert_eq!(none, serde_json::json!({"session": null}));
    }

    #[test]
    fn window_opacity_defaults_to_unsupported() {
        assert!(matches!(
            Bare.set_window_opacity(NativeWindow(0), 0.5),
            Err(PlatformError::Unsupported(Capability::Transparency))
        ));
    }

    #[test]
    fn open_url_scheme_defaults_to_unsupported() {
        assert!(matches!(
            Bare.open_url_scheme("kotilo://open"),
            Err(PlatformError::Unsupported(Capability::Companion))
        ));
    }

    #[test]
    fn monitor_info_is_camel_case() {
        let m = MonitorInfo {
            name: "D".into(),
            width: 1,
            height: 1,
            scale_factor: 1.5,
            primary: true,
        };
        let v = serde_json::to_value(m).unwrap();
        assert_eq!(v["scaleFactor"], 1.5);
    }

    #[test]
    fn current_platform_has_monitors() {
        assert!(current().has(Capability::Monitors));
    }

    #[test]
    fn capability_serializes_kebab_case() {
        let json = serde_json::to_string(&Capability::SystemInfo).unwrap();
        assert_eq!(json, "\"system-info\"");
    }
}
