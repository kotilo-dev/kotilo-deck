//! `tracing` to `~/.kotilo/deck/logs/deck.log.<date>` with daily rotation.
//! `--verbose` raises our level to debug; `RUST_LOG` overrides both.
//! No PII: never log file contents, settings values, coordinates or process names.

use std::sync::OnceLock;

use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

static GUARD: OnceLock<WorkerGuard> = OnceLock::new();

pub fn init() {
    let verbose = std::env::args().any(|a| a == "--verbose");
    let default = if verbose {
        "warn,kotilo_deck_lib=debug,kotilo_deck=debug"
    } else {
        "warn,kotilo_deck_lib=info,kotilo_deck=info"
    };
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(default));

    let dir = crate::paths::logs_dir();
    let file_layer = match std::fs::create_dir_all(&dir) {
        Ok(()) => {
            let appender = tracing_appender::rolling::daily(&dir, "deck.log");
            let (writer, guard) = tracing_appender::non_blocking(appender);
            let _ = GUARD.set(guard);
            Some(fmt::layer().with_ansi(false).with_writer(writer))
        }
        Err(e) => {
            eprintln!("kotilo-deck: cannot create log dir: {e}");
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
