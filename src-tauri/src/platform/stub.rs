use super::{Capability, Platform};

/// Fallback for OSes without an implementation yet (macOS). Only portable features.
pub struct StubPlatform;

impl Platform for StubPlatform {
    fn capabilities(&self) -> Vec<Capability> {
        vec![Capability::Monitors, Capability::SystemInfo]
    }
}
