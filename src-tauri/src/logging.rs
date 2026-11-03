//! `tracing` to `~/.kotilo/deck/logs/deck.log.<date>` with daily rotation, keeping the
//! newest [`KEEP_DAYS`] files. `--verbose` raises our level to debug; `RUST_LOG` overrides.
//! No PII: never log file contents, settings values, coordinates, todo text, process names
//! or full paths (file names only).

use std::sync::Mutex;

use tracing_appender::non_blocking::WorkerGuard;
use tracing_appender::rolling::{RollingFileAppender, Rotation};
use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

const KEEP_DAYS: usize = 14;

/// Dropping the guard flushes the background writer. The event loop ends the process
/// without running destructors, so [`flush`] must be called explicitly on exit.
static GUARD: Mutex<Option<WorkerGuard>> = Mutex::new(None);

fn appender(dir: &std::path::Path) -> Result<RollingFileAppender, String> {
    RollingFileAppender::builder()
        .rotation(Rotation::DAILY)
        .filename_prefix("deck.log")
        .max_log_files(KEEP_DAYS)
        .build(dir)
        .map_err(|e| e.to_string())
}

pub fn init() {
    let verbose = std::env::args().any(|a| a == "--verbose");
    let default = if verbose {
        "warn,kotilo_deck_lib=debug,kotilo_deck=debug"
    } else {
        "warn,kotilo_deck_lib=info,kotilo_deck=info"
    };
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(default));

    let dir = crate::paths::logs_dir();
    let file_layer = match std::fs::create_dir_all(&dir)
        .map_err(|e| e.to_string())
        .and_then(|()| appender(&dir))
    {
        Ok(appender) => {
            let (writer, guard) = tracing_appender::non_blocking(appender);
            *GUARD.lock().unwrap_or_else(|p| p.into_inner()) = Some(guard);
            Some(fmt::layer().with_ansi(false).with_writer(writer))
        }
        Err(e) => {
            eprintln!("kotilo-deck: cannot open log file: {e}");
            None
        }
    };
    let stderr_layer = cfg!(debug_assertions).then(|| fmt::layer().with_writer(std::io::stderr));

    let _ = tracing_subscriber::registry()
        .with(filter)
        .with(file_layer)
        .with(stderr_layer)
        .try_init();
    tracing::info!(
        version = env!("CARGO_PKG_VERSION"),
        verbose,
        "Kotilo Deck starting"
    );
}

/// Writes out buffered log lines. Later log calls are dropped, so call this last.
pub fn flush() {
    let guard = GUARD.lock().unwrap_or_else(|p| p.into_inner()).take();
    drop(guard);
}
