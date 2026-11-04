//! Commands a LAN browser may call (ARCHITECTURE.md "LAN mode"). Everything else is 403:
//! no settings writes, files, monitors, tray, updates, LAN control or desktop launching.
//! The `*_subscribe` pairs are handled by the server itself (per-client ownership, see
//! `server::Subs`); the rest go to `dispatch`.

/// A ref-counted live feed a browser can subscribe to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Topic {
    System,
    Audio,
    Media,
}

impl Topic {
    pub const ALL: [Topic; 3] = [Topic::System, Topic::Audio, Topic::Media];

    pub fn index(self) -> usize {
        match self {
            Topic::System => 0,
            Topic::Audio => 1,
            Topic::Media => 2,
        }
    }
}

/// `Some((topic, true))` for `<topic>_subscribe`, `Some((topic, false))` for unsubscribe.
pub fn subscription(command: &str) -> Option<(Topic, bool)> {
    Some(match command {
        "system_subscribe" => (Topic::System, true),
        "system_unsubscribe" => (Topic::System, false),
        "audio_subscribe" => (Topic::Audio, true),
        "audio_unsubscribe" => (Topic::Audio, false),
        "media_subscribe" => (Topic::Media, true),
        "media_unsubscribe" => (Topic::Media, false),
        _ => return None,
    })
}

/// Concrete names, so a new `todo_*`-style command is never exposed without being added
/// (and dispatched) here.
pub const ALLOWED: &[&str] = &[
    "platform_capabilities",
    "settings_get",
    "workspace_list",
    "workspace_save",
    "system_info",
    "system_subscribe",
    "system_unsubscribe",
    "system_top_processes",
    "weather_geocode",
    "weather_get",
    "wallpaper_list",
    "theme_user_list",
    "todo_list",
    "todo_add",
    "todo_update",
    "todo_delete",
    "todo_reorder",
    "todo_snooze",
    "audio_state",
    "audio_set_volume",
    "audio_set_muted",
    "audio_set_default_device",
    "audio_sessions",
    "audio_set_session_volume",
    "audio_set_session_muted",
    "audio_subscribe",
    "audio_unsubscribe",
    "media_state",
    "media_control",
    "media_seek",
    "media_subscribe",
    "media_unsubscribe",
    "pomodoro_state",
    "pomodoro_start",
    "pomodoro_pause",
    "pomodoro_resume",
    "pomodoro_skip",
    "pomodoro_reset",
    "pomodoro_today",
    "companion_status",
    "companion_snapshot",
    "companion_respond",
    "calendar_upcoming",
    "calendar_refresh",
    "plugin_list",
];

pub fn allowed(command: &str) -> bool {
    ALLOWED.contains(&command)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The ARCHITECTURE.md rule, wildcards included.
    fn architecture_allows(cmd: &str) -> bool {
        const EXACT: &[&str] = &[
            "platform_capabilities",
            "settings_get",
            "workspace_list",
            "workspace_save",
            "system_info",
            "system_subscribe",
            "system_unsubscribe",
            "system_top_processes",
            "weather_geocode",
            "weather_get",
            "wallpaper_list",
            "theme_user_list",
            "companion_status",
            "companion_snapshot",
            "companion_respond",
            "calendar_upcoming",
            "calendar_refresh",
            "plugin_list",
        ];
        EXACT.contains(&cmd)
            || ["todo_", "audio_", "media_", "pomodoro_"]
                .iter()
                .any(|p| cmd.starts_with(p))
    }

    fn registered_commands() -> Vec<String> {
        let src = include_str!("../lib.rs");
        let start = src.find("generate_handler![").expect("generate_handler");
        let body = &src[start + "generate_handler![".len()..];
        let body = &body[..body.find(']').expect("end of handler list")];
        body.lines()
            .map(|l| {
                l.split("//")
                    .next()
                    .unwrap_or("")
                    .trim()
                    .trim_end_matches(',')
            })
            .filter(|l| !l.is_empty())
            .map(|l| l.rsplit("::").next().unwrap_or(l).to_string())
            .collect()
    }

    #[test]
    fn allowlist_matches_architecture_for_every_registered_command() {
        let registered = registered_commands();
        assert!(registered.len() > 50, "parsed {registered:?}");
        for cmd in &registered {
            assert_eq!(allowed(cmd), architecture_allows(cmd), "{cmd}");
        }
        for cmd in ALLOWED {
            assert!(
                registered.iter().any(|r| r == cmd),
                "{cmd} is not a command"
            );
        }
    }

    #[test]
    fn dangerous_commands_are_denied() {
        for cmd in [
            "settings_set",
            "settings_open",
            "settings_export_all",
            "settings_import_all",
            "workspace_delete",
            "monitor_list",
            "monitor_select",
            "tray_set_workspaces",
            "file_save_as",
            "file_open_text",
            "wallpaper_import",
            "companion_open_desktop",
            "lan_status",
            "update_check",
            "update_install",
            "plugin_open_folder",
            "",
            "todo_",
        ] {
            assert!(!allowed(cmd), "{cmd}");
        }
    }

    #[test]
    fn every_subscription_command_is_allowed() {
        for cmd in ALLOWED.iter().filter(|c| c.contains("subscribe")) {
            assert!(subscription(cmd).is_some(), "{cmd}");
        }
    }
}
