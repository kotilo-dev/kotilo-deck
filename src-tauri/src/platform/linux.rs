use super::{Capability, Platform};

/// Linux implementation (MPRIS, PipeWire/Pulse). Full support is Phase 4.
pub struct LinuxPlatform;

impl Platform for LinuxPlatform {
    fn capabilities(&self) -> Vec<Capability> {
        vec![Capability::Monitors, Capability::SystemInfo]
    }
}
