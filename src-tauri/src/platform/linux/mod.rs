//! Linux implementation: `pactl`/`wpctl` audio, MPRIS media over `zbus`, `xdg-mime` /
//! `xdg-open` URL schemes and sysfs GPU load (ARCHITECTURE.md ▸ Linux platform).
//!
//! Written without a Linux machine (DECISIONS 59): the module is also compiled under
//! `cfg(test)` on Windows so every parser and mapping is type-checked and unit-tested there;
//! Unix-only APIs sit behind inner `#[cfg(unix)]`. Behaviour on a real desktop is unverified
//! until a Linux box or CI runs it.

use std::sync::OnceLock;

use super::{
    AudioDevice, AudioSession, AudioState, Capability, ChangeFn, MediaAction, MediaState, Platform,
    Result, Watch,
};

mod audio;
mod cmd;
mod gpu;
mod media;
mod shell;
mod worker;

pub struct LinuxPlatform;

/// Probed once (PATH lookups, one env/socket check, a sysfs scan): cheap enough for the
/// first `capabilities()` call, and stable for the process lifetime.
fn probed() -> &'static [Capability] {
    static CAPS: OnceLock<Vec<Capability>> = OnceLock::new();
    CAPS.get_or_init(|| {
        let mut caps = vec![
            Capability::Monitors,
            Capability::SystemInfo,
            // tauri-plugin-notification (notify-rust over D-Bus).
            Capability::Notifications,
            // Companion server is portable tokio; `kotilo://` via xdg-open.
            Capability::Companion,
            // Tauri `transparent: true` windows work under a compositor. Whole-window
            // opacity (`set_window_opacity`) is not implemented on Linux.
            Capability::Transparency,
        ];
        if audio::available() {
            caps.push(Capability::Audio);
        }
        if media::bus_available() {
            caps.push(Capability::Media);
        }
        if gpu::available() {
            caps.push(Capability::Gpu);
        }
        tracing::debug!(?caps, "linux capabilities");
        caps
    })
}

impl Platform for LinuxPlatform {
    fn capabilities(&self) -> Vec<Capability> {
        probed().to_vec()
    }

    fn open_url_scheme(&self, url: &str) -> Result<()> {
        shell::open_url_scheme(url)
    }

    fn audio_devices(&self) -> Result<Vec<AudioDevice>> {
        audio::state().map(|s| s.devices)
    }
    fn volume(&self) -> Result<f32> {
        audio::state().map(|s| s.volume)
    }
    fn set_volume(&self, level: f32) -> Result<()> {
        audio::set_volume(level)
    }
    fn set_muted(&self, muted: bool) -> Result<()> {
        audio::set_muted(muted)
    }
    fn audio_state(&self) -> Result<AudioState> {
        audio::state()
    }
    fn set_default_audio_device(&self, id: &str) -> Result<()> {
        audio::set_default_device(id)
    }
    fn audio_sessions(&self) -> Result<Vec<AudioSession>> {
        audio::sessions()
    }
    fn set_session_volume(&self, id: &str, level: f32) -> Result<()> {
        audio::set_session_volume(id, level)
    }
    fn set_session_muted(&self, id: &str, muted: bool) -> Result<()> {
        audio::set_session_muted(id, muted)
    }
    fn watch_audio(&self, on_change: ChangeFn) -> Result<Watch> {
        audio::watch(on_change)
    }

    fn media_state(&self) -> Result<MediaState> {
        media::run(media::state)
    }
    fn media_control(&self, action: MediaAction) -> Result<()> {
        media::run(move || media::control(action))
    }
    fn media_seek(&self, seconds: f64) -> Result<()> {
        media::run(move || media::seek(seconds))
    }
    fn watch_media(&self, on_change: ChangeFn) -> Result<Watch> {
        media::watch(on_change)
    }

    fn gpu_usage(&self) -> Result<f32> {
        gpu::usage()
    }
    fn gpu_name(&self) -> Result<String> {
        gpu::name()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::{NativeWindow, PlatformError};

    #[test]
    fn opacity_is_not_implemented() {
        assert!(matches!(
            LinuxPlatform.set_window_opacity(NativeWindow(0), 0.5),
            Err(PlatformError::Unsupported(Capability::Transparency))
        ));
    }

    /// Exercises the real probes on whatever OS runs the tests; on Windows they find nothing
    /// (no pactl, no sysfs) but must not fail.
    #[test]
    fn capabilities_always_include_portable_ones() {
        let caps = LinuxPlatform.capabilities();
        for c in [
            Capability::Monitors,
            Capability::SystemInfo,
            Capability::Notifications,
            Capability::Companion,
        ] {
            assert!(caps.contains(&c), "{c:?}");
        }
        #[cfg(not(target_os = "linux"))]
        assert!(!caps.contains(&Capability::Gpu));
    }

    #[test]
    #[cfg(target_os = "linux")]
    #[ignore = "reads the real sound server / media players (read-only)"]
    fn real_linux_state() {
        let p = LinuxPlatform;
        println!("capabilities: {:?}", p.capabilities());
        println!("audio: {:?}", p.audio_state());
        println!("sessions: {:?}", p.audio_sessions());
        println!(
            "media: {:?}",
            p.media_state().map(|m| m.session.map(|s| s.title))
        );
        println!("gpu: {:?} {:?}", p.gpu_name(), p.gpu_usage());
    }

    #[test]
    #[cfg(target_os = "linux")]
    #[ignore = "starts real pactl subscribe / MPRIS watchers"]
    fn real_linux_watchers_start_and_stop() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::Arc;
        let hits = Arc::new(AtomicUsize::new(0));
        let h = hits.clone();
        let cb: ChangeFn = Arc::new(move || {
            h.fetch_add(1, Ordering::Relaxed);
        });
        let a = LinuxPlatform.watch_audio(cb.clone()).expect("audio watch");
        let m = LinuxPlatform.watch_media(cb).expect("media watch");
        std::thread::sleep(std::time::Duration::from_millis(1500));
        drop(a);
        drop(m);
        println!(
            "change callbacks in 1.5 s: {}",
            hits.load(Ordering::Relaxed)
        );
    }
}
