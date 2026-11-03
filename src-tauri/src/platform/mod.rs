//! Platform abstraction shared with Kotilo Shell. See ARCHITECTURE.md and the
//! `tauri-platform-layer` skill. Every OS-specific call goes through [`Platform`];
//! nothing outside this module may use `windows` / D-Bus / `pactl` directly.

use serde::Serialize;

#[cfg(target_os = "linux")]
mod linux;
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

#[derive(Debug, Clone, Serialize)]
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

/// OS integration surface. Methods default to `Unsupported` so a new platform
/// compiles as soon as it implements `capabilities()`; fill in per phase.
pub trait Platform: Send + Sync {
    fn capabilities(&self) -> Vec<Capability>;

    fn has(&self, capability: Capability) -> bool {
        self.capabilities().contains(&capability)
    }

    // Phase 2: Volume widget.
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

    // Phase 1: System widget GPU tile (CPU/RAM come from `sysinfo`, which is portable).
    /// Busiest GPU engine utilisation, 0–100 (Task Manager's "GPU" figure).
    fn gpu_usage(&self) -> Result<f32> {
        Err(PlatformError::Unsupported(Capability::Gpu))
    }
    /// Name of the primary hardware GPU adapter.
    fn gpu_name(&self) -> Result<String> {
        Err(PlatformError::Unsupported(Capability::Gpu))
    }

    // Phase 2: overlay mode. (A transparent *background* needs no OS call: the deck window
    // is created with Tauri `transparent: true` and the page paints its own background.)
    /// Whole-window opacity, 0–1 (1 removes the layered style again so fullscreen pays
    /// nothing). Must be re-applied after the window toolkit rewrites the window styles
    /// (fullscreen / always-on-top changes).
    fn set_window_opacity(&self, _window: NativeWindow, _opacity: f64) -> Result<()> {
        Err(PlatformError::Unsupported(Capability::Transparency))
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
    }

    #[test]
    fn window_opacity_defaults_to_unsupported() {
        assert!(matches!(
            Bare.set_window_opacity(NativeWindow(0), 0.5),
            Err(PlatformError::Unsupported(Capability::Transparency))
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
