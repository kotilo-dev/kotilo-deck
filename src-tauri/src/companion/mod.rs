//! Companion: the Deck side of `protocol.md` (Kotilo desktop ↔ Deck), app-wide and
//! independent of any window. [`server`] is Tauri-free; this module wires it to events,
//! notifications, settings and the pomodoro timer, and exposes the `companion_*` commands.
//!
//! Events: `companion://status` on every status change, `companion://snapshot` on every
//! snapshot change, `companion://suggest` for `pomodoro.suggest`; `workspace.switch` is
//! forwarded as the existing `workspace://switch {id}` when the user allows it.

pub mod codec;
pub mod server;
pub mod token;

#[cfg(test)]
mod tests;

use std::sync::{Arc, OnceLock};

use serde_json::json;
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_notification::NotificationExt;

use codec::{Agent, Approval, Decision, Message, PomodoroState, TodoAdded};
pub use server::{CompanionSnapshot, CompanionStatus};
use server::{Config, Host, Server};

/// protocol.md §1.
pub const PORT: u16 = 47831;
pub const EVENT_STATUS: &str = "companion://status";
pub const EVENT_SNAPSHOT: &str = "companion://snapshot";
pub const EVENT_SUGGEST: &str = "companion://suggest";
/// Handed to the OS URL handler by `companion_open_desktop`.
pub const DESKTOP_URL: &str = "kotilo://open";

/// Managed state; the server is created in `setup` (it needs the app handle).
#[derive(Default)]
pub struct Companion(OnceLock<Server>);

struct TauriHost(AppHandle);

impl Host for TauriHost {
    fn status_changed(&self, status: &CompanionStatus) {
        let _ = self.0.emit(EVENT_STATUS, status);
    }

    fn snapshot_changed(&self, snapshot: &CompanionSnapshot) {
        let _ = self.0.emit(EVENT_SNAPSHOT, snapshot);
    }

    fn approval_requested(&self, approval: &Approval, agent: Option<&Agent>) {
        let s = crate::settings::load(&self.0);
        if !s.companion.notify_approvals {
            return;
        }
        let who = agent.map_or_else(
            || "An agent".to_string(),
            |a| format!("{} · {}", a.repo, a.name),
        );
        let body = format!("{who} needs approval: {}", approval.summary);
        let mut n = self
            .0
            .notification()
            .builder()
            .title("Kotilo Deck")
            .body(body);
        if s.notification_sound {
            n = n.sound(if cfg!(windows) {
                "Reminder"
            } else {
                "message-new-instant"
            });
        }
        if let Err(e) = n.show() {
            tracing::warn!("cannot show approval notification: {e}");
        }
    }

    fn suggest(&self, reason: &str, minutes: Option<u32>) {
        let _ = self.0.emit(
            EVENT_SUGGEST,
            json!({ "reason": reason, "minutes": minutes }),
        );
    }

    fn workspace_switch(&self, slug: &str) {
        if !crate::settings::load(&self.0)
            .companion
            .allow_workspace_switch
        {
            tracing::debug!("ignoring workspace.switch: not allowed in settings");
            return;
        }
        let known = self
            .0
            .state::<crate::workspace::WorkspaceStore>()
            .list()
            .is_ok_and(|all| all.iter().any(|w| w.id == slug));
        if !known {
            tracing::debug!("ignoring workspace.switch: unknown workspace");
            return;
        }
        let _ = self
            .0
            .emit(crate::tray::EVENT_WORKSPACE_SWITCH, json!({ "id": slug }));
    }

    fn pomodoro_state(&self) -> PomodoroState {
        match self.0.try_state::<crate::widgets::pomodoro::Pomodoro>() {
            Some(p) => p.state(),
            None => crate::widgets::pomodoro::Timer::idle(&crate::settings::load(&self.0).pomodoro)
                .state(crate::widgets::pomodoro::now_ms()),
        }
    }
}

fn server(app: &AppHandle) -> Option<Server> {
    app.try_state::<Companion>()?.0.get().cloned()
}

/// Creates the server and starts it if enabled. Call once from `setup`.
pub fn setup(app: &AppHandle, enabled: bool) {
    let Some(state) = app.try_state::<Companion>() else {
        return;
    };
    let config = Config::new(PORT, crate::paths::companion_token_file());
    let srv = Server::new(config, Arc::new(TauriHost(app.clone())));
    if state.0.set(srv.clone()).is_err() {
        return;
    }
    if enabled {
        srv.set_enabled(true);
    }
}

/// Settings hook: `companion.enabled` toggled.
pub fn set_enabled(app: &AppHandle, on: bool) {
    if let Some(s) = server(app) {
        s.set_enabled(on);
    }
}

/// Quit path: closes the desktop connection with `1001`.
pub fn shutdown(app: &AppHandle) {
    if let Some(s) = server(app) {
        s.shutdown();
    }
}

/// `pomodoro.state` to the desktop (no-op when none is connected).
pub fn broadcast_pomodoro(app: &AppHandle, state: &PomodoroState) {
    if let Some(s) = server(app) {
        s.send(&Message::PomodoroState(state.clone()));
    }
}

/// `todo.added` to the desktop, fire-and-forget.
pub fn todo_added(app: &AppHandle, id: i64, text: &str, due: Option<i64>) {
    if let Some(s) = server(app) {
        s.send(&Message::TodoAdded(TodoAdded {
            id,
            text: text.to_string(),
            due,
        }));
    }
}

fn disabled_status() -> CompanionStatus {
    CompanionStatus {
        enabled: false,
        listening: false,
        connected: false,
        client: None,
        port: PORT,
        error: None,
    }
}

#[tauri::command]
pub async fn companion_status(c: State<'_, Companion>) -> Result<CompanionStatus, String> {
    Ok(c.0.get().map_or_else(disabled_status, Server::status))
}

#[tauri::command]
pub async fn companion_snapshot(c: State<'_, Companion>) -> Result<CompanionSnapshot, String> {
    Ok(c.0.get().map(Server::snapshot).unwrap_or_default())
}

#[tauri::command]
pub async fn companion_respond(
    c: State<'_, Companion>,
    id: String,
    decision: Decision,
) -> Result<(), String> {
    c.0.get()
        .ok_or_else(|| "Kotilo desktop is not connected".to_string())?
        .respond(&id, decision)
}

#[tauri::command]
pub async fn companion_open_desktop() -> Result<(), String> {
    let result = tauri::async_runtime::spawn_blocking(|| {
        crate::platform::current().open_url_scheme(DESKTOP_URL)
    })
    .await
    .map_err(|_| "cannot open the Kotilo desktop app".to_string())?;
    result.map_err(|e| match e {
        crate::platform::PlatformError::Unsupported(_) => {
            "Opening the Kotilo desktop app is not available on this OS".to_string()
        }
        // Platform messages for this call are user-safe ("no app is registered for …").
        crate::platform::PlatformError::Os(msg) => {
            tracing::info!("cannot open the Kotilo desktop app: {msg}");
            msg
        }
    })
}
