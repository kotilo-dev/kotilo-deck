//! Backend events forwarded to LAN browsers over `/api/events`: every key of `DeckEvents`
//! in `src/lib/ipc/index.ts` except the host-only ones (a test keeps the lists in sync).

/// About the host's deck window (hotkeys, tray, monitor, visibility, quit); a browser has
/// its own workspace state, so these never leave the machine. (Documentation + test only.)
#[cfg(test)]
pub const HOST_ONLY: &[&str] = &[
    "workspace://cycle",
    "workspace://switch",
    "monitor://changed",
    "deck://visibility",
    "deck://quitting",
];

pub const FORWARDED: &[&str] = &[
    "system://sample",
    "settings://changed",
    "audio://state",
    "media://state",
    "todo://changed",
    "todo://reminder",
    "theme://user-changed",
    "workspace://changed",
    "pomodoro://state",
    "pomodoro://phase-end",
    "companion://status",
    "companion://snapshot",
    "companion://suggest",
    "calendar://changed",
    "lan://status",
    "plugin://changed",
    "update://status",
];

/// The `/api/events` frame: `{"event": "<name>", "payload": <payload JSON>}`. `payload` is
/// already JSON (Tauri serialises it before handing it to Rust listeners).
pub fn frame(event: &str, payload_json: &str) -> String {
    let name = serde_json::to_string(event).unwrap_or_else(|_| "\"\"".into());
    let payload = if payload_json.trim().is_empty() {
        "null"
    } else {
        payload_json
    };
    format!("{{\"event\":{name},\"payload\":{payload}}}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_deck_events_in_ipc_index() {
        let src = include_str!("../../../src/lib/ipc/index.ts");
        let start = src
            .find("export interface DeckEvents {")
            .expect("DeckEvents");
        let body = &src[start..];
        let body = &body[..body.find("\n}").expect("end of DeckEvents")];
        let mut names: Vec<&str> = body
            .lines()
            .filter_map(|l| {
                let l = l.trim();
                let rest = l.strip_prefix('\'')?;
                let end = rest.find('\'')?;
                rest[end..].starts_with("':").then(|| &rest[..end])
            })
            .collect();
        let mut ours = FORWARDED.to_vec();
        ours.extend_from_slice(HOST_ONLY);
        names.sort_unstable();
        ours.sort_unstable();
        assert_eq!(ours, names);
        assert!(FORWARDED.iter().all(|e| !HOST_ONLY.contains(e)));
    }

    #[test]
    fn frame_shape() {
        let f: serde_json::Value =
            serde_json::from_str(&frame("todo://reminder", r#"{"id":3}"#)).unwrap();
        assert_eq!(
            f,
            serde_json::json!({"event": "todo://reminder", "payload": {"id": 3}})
        );
        let f: serde_json::Value = serde_json::from_str(&frame("x://y", "")).unwrap();
        assert_eq!(f["payload"], serde_json::Value::Null);
    }
}
