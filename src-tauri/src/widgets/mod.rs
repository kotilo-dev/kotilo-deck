//! Per-widget Rust command modules (SPEC §4.3). Frontend contract: `src/lib/ipc/index.ts`.

pub mod audio;
pub mod calendar;
pub mod media;
pub mod system;
pub mod todo;

mod watch;
pub mod weather;
