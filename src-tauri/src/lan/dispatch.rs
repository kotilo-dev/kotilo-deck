//! In-process dispatch of allowlisted commands for LAN browsers: the same functions the
//! webview reaches through `invoke`, with the same camelCase argument names. Subscriptions
//! never get here (`server` owns them per client).

use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::{json, Map, Value};
use tauri::{AppHandle, Emitter, Manager, State};

use super::server::InvokeError;
use crate::{companion, files, plugins, settings, widgets, workspace};

struct Args(Map<String, Value>);

impl Args {
    /// Tauri semantics: a missing key deserialises from `null` (so `Option` → `None`).
    fn get<T: DeserializeOwned>(&mut self, key: &str) -> Result<T, InvokeError> {
        let v = self.0.remove(key).unwrap_or(Value::Null);
        serde_json::from_value(v)
            .map_err(|e| InvokeError::BadArgs(format!("invalid argument `{key}`: {e}")))
    }
}

fn state<T: Send + Sync + 'static>(app: &AppHandle) -> Result<State<'_, T>, InvokeError> {
    app.try_state::<T>()
        .ok_or_else(|| InvokeError::Failed("not available yet".into()))
}

fn done<T: Serialize>(r: Result<T, String>) -> Result<Value, InvokeError> {
    let v = r.map_err(InvokeError::Failed)?;
    serde_json::to_value(v).map_err(|e| InvokeError::Failed(e.to_string()))
}

/// Runs one allowlisted command. Keep the arms in sync with `allowlist::ALLOWED` (a test
/// checks that every allowlisted, non-subscription name has an arm here).
pub async fn run(
    app: AppHandle,
    command: &str,
    args: Map<String, Value>,
) -> Result<Value, InvokeError> {
    use widgets::{audio, calendar, media, pomodoro, system, todo, weather};
    let mut a = Args(args);
    let app = &app;
    match command {
        "platform_capabilities" => done(Ok(crate::platform::current().capabilities())),
        "settings_get" => done(settings::settings_get(app.clone()).await),
        "workspace_list" => done(workspace::workspace_list(state(app)?).await),
        "workspace_save" => {
            let r = done(workspace::workspace_save(state(app)?, a.get("workspace")?).await)?;
            // The host deck/settings windows (and other browsers) reload from disk.
            let _ = app.emit(crate::transfer::EVENT_WORKSPACE_CHANGED, json!({}));
            Ok(r)
        }
        "system_info" => done(system::system_info().await),
        "system_top_processes" => done(system::system_top_processes().await),
        "weather_geocode" => done(weather::weather_geocode(state(app)?, a.get("query")?).await),
        "weather_get" => done(
            weather::weather_get(
                state(app)?,
                a.get("latitude")?,
                a.get("longitude")?,
                a.get("unit")?,
            )
            .await,
        ),
        "wallpaper_list" => done(files::wallpaper_list().await),
        "theme_user_list" => done(files::theme_user_list().await),
        "todo_list" => done(todo::todo_list(state(app)?).await),
        "todo_add" => done(
            todo::todo_add(
                app.clone(),
                state(app)?,
                a.get("text")?,
                a.get("due")?,
                a.get("remindAt")?,
            )
            .await,
        ),
        "todo_update" => {
            done(todo::todo_update(app.clone(), state(app)?, a.get("id")?, a.get("patch")?).await)
        }
        "todo_delete" => done(todo::todo_delete(app.clone(), state(app)?, a.get("id")?).await),
        "todo_reorder" => done(todo::todo_reorder(app.clone(), state(app)?, a.get("ids")?).await),
        "todo_snooze" => {
            done(todo::todo_snooze(app.clone(), state(app)?, a.get("id")?, a.get("minutes")?).await)
        }
        "audio_state" => done(audio::audio_state().await),
        "audio_set_volume" => done(audio::audio_set_volume(a.get("volume")?).await),
        "audio_set_muted" => done(audio::audio_set_muted(a.get("muted")?).await),
        "audio_set_default_device" => done(audio::audio_set_default_device(a.get("id")?).await),
        "audio_sessions" => done(audio::audio_sessions().await),
        "audio_set_session_volume" => {
            done(audio::audio_set_session_volume(a.get("id")?, a.get("volume")?).await)
        }
        "audio_set_session_muted" => {
            done(audio::audio_set_session_muted(a.get("id")?, a.get("muted")?).await)
        }
        "media_state" => done(media::media_state().await),
        "media_control" => done(media::media_control(a.get("action")?).await),
        "media_seek" => done(media::media_seek(a.get("seconds")?).await),
        "pomodoro_state" => done(pomodoro::pomodoro_state(state(app)?).await),
        "pomodoro_start" => done(
            pomodoro::pomodoro_start(app.clone(), state(app)?, a.get("phase")?, a.get("minutes")?)
                .await,
        ),
        "pomodoro_pause" => done(pomodoro::pomodoro_pause(app.clone(), state(app)?).await),
        "pomodoro_resume" => done(pomodoro::pomodoro_resume(app.clone(), state(app)?).await),
        "pomodoro_skip" => done(pomodoro::pomodoro_skip(app.clone(), state(app)?).await),
        "pomodoro_reset" => done(pomodoro::pomodoro_reset(app.clone(), state(app)?).await),
        "pomodoro_today" => done(pomodoro::pomodoro_today(app.clone(), state(app)?).await),
        "companion_status" => done(companion::companion_status(state(app)?).await),
        "companion_snapshot" => done(companion::companion_snapshot(state(app)?).await),
        "companion_respond" => {
            done(companion::companion_respond(state(app)?, a.get("id")?, a.get("decision")?).await)
        }
        "calendar_upcoming" => {
            done(calendar::calendar_upcoming(state(app)?, a.get("limit")?, a.get("days")?).await)
        }
        "calendar_refresh" => done(calendar::calendar_refresh(app.clone(), state(app)?).await),
        "plugin_list" => done(plugins::plugin_list(app.clone()).await),
        _ => Err(InvokeError::Failed("command not available over LAN".into())),
    }
}

#[cfg(test)]
mod tests {
    use super::super::allowlist::{self, ALLOWED};

    #[test]
    fn every_allowlisted_command_has_an_arm() {
        let src = include_str!("dispatch.rs");
        let body = &src[..src.find("#[cfg(test)]").unwrap_or(src.len())];
        for cmd in ALLOWED {
            let has_arm = body.contains(&format!("\"{cmd}\" =>"));
            assert_eq!(
                has_arm,
                allowlist::subscription(cmd).is_none(),
                "{cmd}: dispatch arm present = {has_arm}"
            );
        }
    }

    #[test]
    fn missing_optional_args_are_none() {
        let mut a = super::Args(serde_json::Map::new());
        let v: Option<u32> = a.get("limit").unwrap();
        assert_eq!(v, None);
        let e = a.get::<String>("text").unwrap_err();
        assert!(matches!(e, super::InvokeError::BadArgs(m) if m.contains("`text`")));
    }
}
