use super::{Capability, Platform};

/// Windows implementation (windows-rs). Capabilities are added as each phase lands.
pub struct WindowsPlatform;

impl Platform for WindowsPlatform {
    fn capabilities(&self) -> Vec<Capability> {
        vec![Capability::Monitors, Capability::SystemInfo]
    }
}
