//! Phase 4 tablet/browser mode (SPEC §8 Phase 4, ARCHITECTURE.md "LAN mode").
//!
//! Serves the deck SPA plus an HTTP/WebSocket IPC bridge on `0.0.0.0:<lan.port>` so a
//! tablet or a browser on the LAN can show the deck. Off by default; every request needs
//! the companion token (`~/.kotilo/companion.token`). Only the commands on the allowlist
//! in ARCHITECTURE.md are reachable; backend events are forwarded over `/api/events`.
//!
//! [`server`] is Tauri-free; this module wires it to the app: embedded assets, the
//! wallpapers folder, plugin files, in-process command dispatch ([`dispatch`]), the
//! remote side of the ref-counted subscriptions, and every `DeckEvents` event (Rust-side
//! `listen_any`, which `app.emit` reaches) fanned out to the open sockets.

mod allowlist;
mod assets;
mod auth;
mod dispatch;
mod events;
pub mod server;
mod status;

#[cfg(test)]
mod tests;

use std::path::PathBuf;
use std::sync::{Arc, OnceLock};

use serde::Serialize;
use serde_json::{Map, Value};
use tauri::{AppHandle, Emitter, Listener, Manager};

use crate::settings::LanSettings;
use crate::widgets::watch::Owner;
use allowlist::Topic;
use server::{Backend, BoxFuture, Config, InvokeError, Server};

pub const EVENT_LAN_STATUS: &str = "lan://status";

/// Mirrors `LanStatus` in `src/lib/ipc/index.ts`.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LanStatus {
    pub enabled: bool,
    pub listening: bool,
    pub port: u16,
    /// `http://<ipv4>:<port>/?token=<token>` for every non-loopback IPv4 address.
    pub urls: Vec<String>,
    /// SVG markup of a QR code for `urls[0]`; `None` when not listening.
    pub qr_svg: Option<String>,
    /// Open `/api/events` connections.
    pub clients: u32,
    /// Last bind/serve error, e.g. "port 47832 is in use".
    pub error: Option<String>,
    /// Machine-readable `error` for translation: "port-in-use", "permission-denied",
    /// "bind-failed", "token-unreadable".
    pub error_code: Option<&'static str>,
}

/// Managed state; created by [`start`] (it needs the app handle).
#[derive(Default)]
struct Lan(OnceLock<Server>);

struct TauriBackend(AppHandle);

impl Backend for TauriBackend {
    fn status_changed(&self, status: &LanStatus) {
        let _ = self.0.emit(EVENT_LAN_STATUS, status);
    }

    fn asset(&self, key: &str) -> Option<assets::Asset> {
        let a = self.0.asset_resolver().get(key.to_string())?;
        let mime = if a.mime_type.is_empty() || a.mime_type == "application/octet-stream" {
            assets::mime_for(key).to_string()
        } else {
            a.mime_type
        };
        Some(assets::Asset {
            bytes: a.bytes,
            mime,
        })
    }

    fn wallpapers_dir(&self) -> PathBuf {
        crate::paths::wallpapers_dir()
    }

    fn plugin_file(&self, folder: &str, path: &str) -> Option<crate::plugins::PluginFile> {
        crate::plugins::read_file(&self.0, folder, path)
    }

    fn invoke(
        &self,
        command: String,
        args: Map<String, Value>,
    ) -> BoxFuture<Result<Value, InvokeError>> {
        let app = self.0.clone();
        Box::pin(async move { dispatch::run(app, &command, args).await })
    }

    fn subscription(&self, topic: Topic, on: bool) -> BoxFuture<()> {
        use crate::widgets::{audio, media, system};
        let app = self.0.clone();
        Box::pin(async move {
            let result = match (topic, on) {
                (Topic::System, on) => {
                    if let Some(s) = app.try_state::<system::Sampler>() {
                        if on {
                            s.subscribe(app.clone(), Owner::Remote);
                        } else {
                            s.unsubscribe(Owner::Remote);
                        }
                    }
                    Ok(())
                }
                (Topic::Audio, true) => audio::subscribe_as(app, Owner::Remote).await,
                (Topic::Audio, false) => audio::unsubscribe_as(app, Owner::Remote).await,
                (Topic::Media, true) => media::subscribe_as(app, Owner::Remote).await,
                (Topic::Media, false) => media::unsubscribe_as(app, Owner::Remote).await,
            };
            if let Err(e) = result {
                tracing::debug!(?topic, on, "LAN subscription change failed: {e}");
            }
        })
    }
}

fn server(app: &AppHandle) -> Option<Server> {
    app.try_state::<Lan>()?.0.get().cloned()
}

/// Startup (from `startup::setup`): starts the server when `lan.enabled`.
pub fn start(app: &AppHandle, lan: &LanSettings) {
    if app.try_state::<Lan>().is_none() {
        app.manage(Lan::default());
    }
    let Some(state) = app.try_state::<Lan>() else {
        return;
    };
    let config = Config::new(crate::paths::companion_token_file());
    let srv = Server::new(config, Arc::new(TauriBackend(app.clone())));
    if state.0.set(srv.clone()).is_err() {
        return;
    }
    // `emit` hands every event to Rust listeners too; `publish` is a no-op without sockets.
    for name in events::FORWARDED {
        let s = srv.clone();
        app.listen_any(*name, move |e| s.publish(name, e.payload()));
    }
    srv.apply(lan.enabled, lan.port);
}

/// Settings hook: `lan` changed (enable/disable/port). Rebinds; emits `lan://status`.
pub fn settings_changed(app: &AppHandle, lan: &LanSettings) {
    if let Some(s) = server(app) {
        s.apply(lan.enabled, lan.port);
    }
}

/// Quit path: closes every LAN connection.
pub fn shutdown(app: &AppHandle) {
    if let Some(s) = server(app) {
        s.shutdown();
    }
}

#[tauri::command]
pub async fn lan_status(app: AppHandle) -> Result<LanStatus, String> {
    match server(&app) {
        Some(s) => {
            s.refresh_urls();
            Ok(s.status())
        }
        None => {
            let lan = crate::settings::load(&app).lan;
            Ok(LanStatus {
                enabled: lan.enabled,
                port: lan.port,
                ..LanStatus::default()
            })
        }
    }
}
