//! Todo widget: SQLite `~/.kotilo/deck/deck.db` owned by the backend (rusqlite), so the
//! reminder scheduler fires OS notifications while the deck is hidden to the tray.
//!
//! Every mutation emits `todo://changed` and re-arms the scheduler. The scheduler is one
//! task that sleeps until the next `remindAt` (at most [`MAX_SLEEP`], so a suspended PC or
//! a wall-clock change is noticed); a fired reminder's `remindAt` is cleared so it never
//! refires, and reminders that fell due while the app was closed fire once at startup.
//! Todo text is user content: never log it.

use std::path::Path;
use std::sync::Mutex;
use std::time::Duration;

use rusqlite::{params, Connection, OptionalExtension, Row};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::json;
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_notification::NotificationExt;
use tokio::sync::Notify;

use crate::migrations;

pub const EVENT_CHANGED: &str = "todo://changed";
pub const EVENT_REMINDER: &str = "todo://reminder";
const MAX_TEXT: usize = 1000;
const MAX_SNOOZE_MINUTES: u32 = 7 * 24 * 60;
/// Upper bound for one scheduler sleep.
const MAX_SLEEP: Duration = Duration::from_secs(60);
/// More due reminders than this in one batch (e.g. after a long shutdown) are summarised
/// in a single notification.
const MAX_INDIVIDUAL_NOTIFICATIONS: usize = 3;

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Todo {
    pub id: i64,
    pub text: String,
    pub done: bool,
    pub due: Option<i64>,
    pub remind_at: Option<i64>,
    pub position: i64,
    pub created_at: i64,
    pub completed_at: Option<i64>,
}

/// Unix ms; JSON numbers that are whole but sent as floats are accepted.
fn to_ms(v: f64) -> Result<i64, String> {
    if (0.0..1e15).contains(&v) {
        Ok(v.round() as i64)
    } else {
        Err("timestamp must be a non-negative Unix ms value".into())
    }
}

/// `undefined` → None (leave), `null` → Some(None) (clear), number → Some(Some(ms)).
fn patch_ms<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Option<f64>>, D::Error> {
    Option::<f64>::deserialize(d).map(Some)
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TodoPatch {
    pub text: Option<String>,
    pub done: Option<bool>,
    #[serde(default, deserialize_with = "patch_ms")]
    pub due: Option<Option<f64>>,
    #[serde(default, deserialize_with = "patch_ms")]
    pub remind_at: Option<Option<f64>>,
}

fn clean_text(text: &str) -> Result<String, String> {
    let t = text.trim();
    if t.is_empty() {
        return Err("todo text is empty".into());
    }
    if t.chars().count() > MAX_TEXT {
        return Err(format!("todo text is longer than {MAX_TEXT} characters"));
    }
    Ok(t.to_string())
}

fn opt_ms(v: Option<f64>) -> Result<Option<i64>, String> {
    v.map(to_ms).transpose()
}

fn sql(e: rusqlite::Error) -> String {
    format!("database error: {e}")
}

const COLUMNS: &str = "id, text, done, due, remind_at, position, created_at, completed_at";

fn row(r: &Row<'_>) -> rusqlite::Result<Todo> {
    Ok(Todo {
        id: r.get(0)?,
        text: r.get(1)?,
        done: r.get::<_, i64>(2)? != 0,
        due: r.get(3)?,
        remind_at: r.get(4)?,
        position: r.get(5)?,
        created_at: r.get(6)?,
        completed_at: r.get(7)?,
    })
}

pub struct TodoDb {
    conn: Connection,
}

impl TodoDb {
    pub fn open(path: &Path) -> Result<Self, String> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| format!("cannot create data dir: {e}"))?;
        }
        let conn = Connection::open(path).map_err(sql)?;
        // WAL keeps a crash from corrupting the file; busy_timeout covers a second handle.
        conn.pragma_update(None, "journal_mode", "WAL")
            .map_err(sql)?;
        conn.busy_timeout(Duration::from_secs(2)).map_err(sql)?;
        Self::init(conn)
    }

    #[cfg(test)]
    pub fn open_in_memory() -> Result<Self, String> {
        Self::init(Connection::open_in_memory().map_err(sql)?)
    }

    fn init(mut conn: Connection) -> Result<Self, String> {
        let from = migrations::migrate_db(&mut conn)?;
        if from != migrations::DB_VERSION {
            tracing::info!(from, to = migrations::DB_VERSION, "migrated database");
        }
        Ok(Self { conn })
    }

    /// Not done by position, then done by most recently completed.
    pub fn list(&self) -> Result<Vec<Todo>, String> {
        let mut stmt = self
            .conn
            .prepare_cached(&format!(
                "SELECT {COLUMNS} FROM todos ORDER BY done ASC,
                 CASE WHEN done = 0 THEN position END ASC,
                 completed_at DESC, id ASC"
            ))
            .map_err(sql)?;
        let rows = stmt.query_map([], row).map_err(sql)?;
        rows.collect::<rusqlite::Result<Vec<_>>>().map_err(sql)
    }

    pub fn get(&self, id: i64) -> Result<Todo, String> {
        self.conn
            .query_row(
                &format!("SELECT {COLUMNS} FROM todos WHERE id = ?1"),
                [id],
                row,
            )
            .optional()
            .map_err(sql)?
            .ok_or_else(|| "todo not found".to_string())
    }

    /// Appends at the end of the list.
    pub fn add(
        &self,
        text: &str,
        due: Option<i64>,
        remind_at: Option<i64>,
        now: i64,
    ) -> Result<Todo, String> {
        let text = clean_text(text)?;
        self.conn
            .execute(
                "INSERT INTO todos (text, done, due, remind_at, position, created_at)
                 VALUES (?1, 0, ?2, ?3, (SELECT COALESCE(MAX(position), -1) + 1 FROM todos), ?4)",
                params![text, due, remind_at, now],
            )
            .map_err(sql)?;
        self.get(self.conn.last_insert_rowid())
    }

    /// Patch; `done` stamps (false → true) or clears (→ false) `completedAt`.
    pub fn update(&self, id: i64, patch: &TodoPatch, now: i64) -> Result<Todo, String> {
        let mut t = self.get(id)?;
        if let Some(text) = &patch.text {
            t.text = clean_text(text)?;
        }
        if let Some(due) = patch.due {
            t.due = opt_ms(due)?;
        }
        if let Some(remind_at) = patch.remind_at {
            t.remind_at = opt_ms(remind_at)?;
        }
        if let Some(done) = patch.done {
            if done && !t.done {
                t.completed_at = Some(now);
            } else if !done {
                t.completed_at = None;
            }
            t.done = done;
        }
        self.conn
            .execute(
                "UPDATE todos SET text = ?2, done = ?3, due = ?4, remind_at = ?5, completed_at = ?6
                 WHERE id = ?1",
                params![id, t.text, t.done, t.due, t.remind_at, t.completed_at],
            )
            .map_err(sql)?;
        Ok(t)
    }

    /// Idempotent.
    pub fn delete(&self, id: i64) -> Result<(), String> {
        self.conn
            .execute("DELETE FROM todos WHERE id = ?1", [id])
            .map_err(sql)?;
        Ok(())
    }

    /// `ids` become positions 0…n in that order; unknown ids are ignored (deleted meanwhile)
    /// and rows missing from `ids` keep their relative order after them.
    pub fn reorder(&mut self, ids: &[i64]) -> Result<(), String> {
        let mut seen = std::collections::HashSet::new();
        if !ids.iter().all(|id| seen.insert(*id)) {
            return Err("duplicate id in new order".into());
        }
        let tx = self.conn.transaction().map_err(sql)?;
        let existing: Vec<i64> = {
            let mut stmt = tx
                .prepare("SELECT id FROM todos ORDER BY position ASC, id ASC")
                .map_err(sql)?;
            let rows = stmt.query_map([], |r| r.get(0)).map_err(sql)?;
            rows.collect::<rusqlite::Result<_>>().map_err(sql)?
        };
        let known: std::collections::HashSet<i64> = existing.iter().copied().collect();
        let order = ids
            .iter()
            .copied()
            .filter(|id| known.contains(id))
            .chain(existing.iter().copied().filter(|id| !seen.contains(id)));
        {
            let mut stmt = tx
                .prepare("UPDATE todos SET position = ?2 WHERE id = ?1")
                .map_err(sql)?;
            for (pos, id) in order.enumerate() {
                stmt.execute(params![id, pos as i64]).map_err(sql)?;
            }
        }
        tx.commit().map_err(sql)
    }

    pub fn snooze(&self, id: i64, minutes: u32, now: i64) -> Result<Todo, String> {
        if !(1..=MAX_SNOOZE_MINUTES).contains(&minutes) {
            return Err(format!(
                "snooze must be between 1 and {MAX_SNOOZE_MINUTES} minutes"
            ));
        }
        let at = now + i64::from(minutes) * 60_000;
        let changed = self
            .conn
            .execute(
                "UPDATE todos SET remind_at = ?2 WHERE id = ?1",
                params![id, at],
            )
            .map_err(sql)?;
        if changed == 0 {
            return Err("todo not found".into());
        }
        self.get(id)
    }

    /// Earliest pending reminder of an open todo.
    pub fn next_reminder(&self) -> Result<Option<i64>, String> {
        self.conn
            .query_row(
                "SELECT MIN(remind_at) FROM todos WHERE done = 0 AND remind_at IS NOT NULL",
                [],
                |r| r.get(0),
            )
            .map_err(sql)
    }

    /// Open todos whose reminder is due at `now`, oldest first. Their `remindAt` is cleared
    /// in the same transaction so they cannot fire twice. Returned rows keep the old value.
    pub fn take_due(&mut self, now: i64) -> Result<Vec<Todo>, String> {
        let tx = self.conn.transaction().map_err(sql)?;
        let due: Vec<Todo> = {
            let mut stmt = tx
                .prepare(&format!(
                    "SELECT {COLUMNS} FROM todos
                     WHERE done = 0 AND remind_at IS NOT NULL AND remind_at <= ?1
                     ORDER BY remind_at ASC, id ASC"
                ))
                .map_err(sql)?;
            let rows = stmt.query_map([now], row).map_err(sql)?;
            rows.collect::<rusqlite::Result<_>>().map_err(sql)?
        };
        tx.execute(
            "UPDATE todos SET remind_at = NULL
             WHERE done = 0 AND remind_at IS NOT NULL AND remind_at <= ?1",
            [now],
        )
        .map_err(sql)?;
        tx.commit().map_err(sql)?;
        Ok(due)
    }
}

/// How long the scheduler may sleep before checking again.
pub fn sleep_for(next: Option<i64>, now: i64) -> Option<Duration> {
    next.map(|t| {
        let ms = u64::try_from(t.saturating_sub(now)).unwrap_or(0);
        Duration::from_millis(ms).min(MAX_SLEEP)
    })
}

pub fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

/// Notification body: the due time in local time ("Due 14:30", or with the date when it
/// is not today). English until backend strings are localised.
pub fn reminder_body(due: Option<i64>, now: i64) -> String {
    use chrono::{Local, TimeZone};
    let Some(due) = due.and_then(|d| Local.timestamp_millis_opt(d).single()) else {
        return "Reminder".into();
    };
    let today = Local
        .timestamp_millis_opt(now)
        .single()
        .map(|n| n.date_naive());
    if today == Some(due.date_naive()) {
        format!("Due {}", due.format("%H:%M"))
    } else {
        format!("Due {}", due.format("%a %-d %b, %H:%M"))
    }
}

// ── State, commands, scheduler ──────────────────────────────────────────────

pub struct Todos {
    db: Result<Mutex<TodoDb>, String>,
    wake: Notify,
}

impl Todos {
    pub fn open(path: &Path) -> Self {
        let db = TodoDb::open(path).map(Mutex::new).map_err(|e| {
            tracing::error!(file = %crate::paths::file_label(path), "todo database unavailable: {e}");
            e
        });
        Self {
            db,
            wake: Notify::new(),
        }
    }

    fn with<T>(&self, f: impl FnOnce(&mut TodoDb) -> Result<T, String>) -> Result<T, String> {
        let db = self.db.as_ref().map_err(Clone::clone)?;
        let mut guard = db.lock().unwrap_or_else(|p| p.into_inner());
        f(&mut guard)
    }
}

fn changed(app: &AppHandle, todos: &Todos) {
    todos.wake.notify_one();
    let _ = app.emit(EVENT_CHANGED, json!({}));
}

#[tauri::command]
pub async fn todo_list(todos: State<'_, Todos>) -> Result<Vec<Todo>, String> {
    todos.with(|db| db.list())
}

#[tauri::command]
pub async fn todo_add(
    app: AppHandle,
    todos: State<'_, Todos>,
    text: String,
    due: Option<f64>,
    remind_at: Option<f64>,
) -> Result<Todo, String> {
    let (due, remind_at) = (opt_ms(due)?, opt_ms(remind_at)?);
    let t = todos.with(|db| db.add(&text, due, remind_at, now_ms()))?;
    changed(&app, &todos);
    // protocol.md `todo.added`; no-op without a connected desktop.
    crate::companion::todo_added(&app, t.id, &t.text, t.due);
    Ok(t)
}

#[tauri::command]
pub async fn todo_update(
    app: AppHandle,
    todos: State<'_, Todos>,
    id: i64,
    patch: TodoPatch,
) -> Result<Todo, String> {
    let t = todos.with(|db| db.update(id, &patch, now_ms()))?;
    changed(&app, &todos);
    Ok(t)
}

#[tauri::command]
pub async fn todo_delete(app: AppHandle, todos: State<'_, Todos>, id: i64) -> Result<(), String> {
    todos.with(|db| db.delete(id))?;
    changed(&app, &todos);
    Ok(())
}

#[tauri::command]
pub async fn todo_reorder(
    app: AppHandle,
    todos: State<'_, Todos>,
    ids: Vec<i64>,
) -> Result<(), String> {
    todos.with(|db| db.reorder(&ids))?;
    changed(&app, &todos);
    Ok(())
}

#[tauri::command]
pub async fn todo_snooze(
    app: AppHandle,
    todos: State<'_, Todos>,
    id: i64,
    minutes: u32,
) -> Result<Todo, String> {
    let t = todos.with(|db| db.snooze(id, minutes, now_ms()))?;
    changed(&app, &todos);
    Ok(t)
}

fn notify(app: &AppHandle, title: &str, body: &str, sound: bool) {
    let mut n = app.notification().builder().title(title).body(body);
    if sound {
        n = n.sound(if cfg!(windows) {
            "Reminder"
        } else {
            "message-new-instant"
        });
    }
    if let Err(e) = n.show() {
        tracing::warn!("cannot show reminder notification: {e}");
    }
}

fn fire(app: &AppHandle, due: &[Todo], now: i64) {
    let sound = crate::settings::load(app).notification_sound;
    for (i, t) in due.iter().enumerate() {
        if i < MAX_INDIVIDUAL_NOTIFICATIONS {
            notify(app, &t.text, &reminder_body(t.due, now), sound && i == 0);
        }
        let _ = app.emit(EVENT_REMINDER, json!({ "id": t.id }));
    }
    if due.len() > MAX_INDIVIDUAL_NOTIFICATIONS {
        let more = due.len() - MAX_INDIVIDUAL_NOTIFICATIONS;
        notify(app, "Kotilo Deck", &format!("{more} more reminders"), false);
    }
    tracing::info!(count = due.len(), "todo reminders fired");
    let _ = app.emit(EVENT_CHANGED, json!({}));
}

/// The reminder scheduler. Runs for the app's lifetime, independent of any window.
pub fn spawn_scheduler(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        let todos = app.state::<Todos>();
        if todos.db.is_err() {
            return;
        }
        loop {
            let now = now_ms();
            match todos.with(|db| db.take_due(now)) {
                Ok(due) if !due.is_empty() => fire(&app, &due, now),
                Ok(_) => {}
                Err(e) => tracing::warn!("reminder check failed: {e}"),
            }
            let next = todos.with(|db| db.next_reminder()).unwrap_or_else(|e| {
                tracing::warn!("cannot read next reminder: {e}");
                Some(now + MAX_SLEEP.as_millis() as i64)
            });
            // No pending reminder still re-checks every MAX_SLEEP: rows can appear without a
            // wake (import, another process, clock change); one indexed query a minute is free.
            let d = sleep_for(next, now_ms()).unwrap_or(MAX_SLEEP);
            tokio::select! {
                _ = tokio::time::sleep(d) => {}
                _ = todos.wake.notified() => {}
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn db() -> TodoDb {
        TodoDb::open_in_memory().unwrap()
    }

    fn texts(db: &TodoDb) -> Vec<String> {
        db.list().unwrap().into_iter().map(|t| t.text).collect()
    }

    fn patch(v: serde_json::Value) -> TodoPatch {
        serde_json::from_value(v).unwrap()
    }

    #[test]
    fn crud_round_trip() {
        let db = db();
        let a = db.add("  buy milk ", Some(1000), Some(900), 1).unwrap();
        assert_eq!(a.text, "buy milk");
        assert_eq!((a.due, a.remind_at, a.position), (Some(1000), Some(900), 0));
        assert!(!a.done && a.completed_at.is_none());
        assert!(db.add("   ", None, None, 1).is_err());
        assert!(db.add(&"x".repeat(MAX_TEXT + 1), None, None, 1).is_err());

        let b = db.add("deploy", None, None, 2).unwrap();
        assert_eq!(b.position, 1);

        let a2 = db
            .update(
                a.id,
                &patch(serde_json::json!({"text": "buy oat milk", "due": null})),
                5,
            )
            .unwrap();
        assert_eq!(a2.text, "buy oat milk");
        assert_eq!(a2.due, None);
        assert_eq!(a2.remind_at, Some(900)); // absent = unchanged

        db.delete(b.id).unwrap();
        db.delete(b.id).unwrap();
        assert_eq!(texts(&db), ["buy oat milk"]);
        assert!(db.update(b.id, &TodoPatch::default(), 5).is_err());
    }

    #[test]
    fn done_stamps_and_clears_completed_at() {
        let db = db();
        let t = db.add("a", None, None, 1).unwrap();
        let done = db
            .update(t.id, &patch(serde_json::json!({"done": true})), 50)
            .unwrap();
        assert_eq!(done.completed_at, Some(50));
        // Re-marking done keeps the first stamp.
        let again = db
            .update(t.id, &patch(serde_json::json!({"done": true})), 60)
            .unwrap();
        assert_eq!(again.completed_at, Some(50));
        let open = db
            .update(t.id, &patch(serde_json::json!({"done": false})), 70)
            .unwrap();
        assert!(!open.done && open.completed_at.is_none());
    }

    #[test]
    fn list_order_open_by_position_then_done_by_recency() {
        let db = db();
        let ids: Vec<i64> = ["a", "b", "c", "d"]
            .iter()
            .map(|t| db.add(t, None, None, 1).unwrap().id)
            .collect();
        let done = |id, at| {
            db.update(id, &patch(serde_json::json!({"done": true})), at)
                .unwrap()
        };
        done(ids[0], 10);
        done(ids[2], 20);
        assert_eq!(texts(&db), ["b", "d", "c", "a"]);
    }

    #[test]
    fn reorder_full_partial_and_invalid() {
        let mut db = db();
        let ids: Vec<i64> = ["a", "b", "c"]
            .iter()
            .map(|t| db.add(t, None, None, 1).unwrap().id)
            .collect();
        db.reorder(&[ids[2], ids[0], ids[1]]).unwrap();
        assert_eq!(texts(&db), ["c", "a", "b"]);
        let positions: Vec<i64> = db.list().unwrap().iter().map(|t| t.position).collect();
        assert_eq!(positions, [0, 1, 2]);
        // Unknown ids ignored, missing ones keep their order at the end.
        db.reorder(&[ids[1], 999]).unwrap();
        assert_eq!(texts(&db), ["b", "c", "a"]);
        assert!(db.reorder(&[ids[0], ids[0]]).is_err());
        // New items still append at the end.
        db.add("d", None, None, 1).unwrap();
        assert_eq!(texts(&db), ["b", "c", "a", "d"]);
    }

    #[test]
    fn snooze_sets_remind_at_from_now() {
        let db = db();
        let t = db.add("a", None, None, 1).unwrap();
        let s = db.snooze(t.id, 10, 1_000).unwrap();
        assert_eq!(s.remind_at, Some(1_000 + 600_000));
        assert!(db.snooze(t.id, 0, 1).is_err());
        assert!(db.snooze(t.id, MAX_SNOOZE_MINUTES + 1, 1).is_err());
        assert!(db.snooze(424_242, 10, 1).is_err());
    }

    #[test]
    fn scheduler_next_due_and_take_due() {
        let mut db = db();
        assert_eq!(db.next_reminder().unwrap(), None);
        let a = db.add("a", None, Some(5_000), 1).unwrap();
        let b = db.add("b", None, Some(2_000), 1).unwrap();
        let c = db.add("c", None, Some(1_000), 1).unwrap();
        db.add("no reminder", None, None, 1).unwrap();
        // Done todos never fire.
        db.update(c.id, &patch(serde_json::json!({"done": true})), 1)
            .unwrap();
        assert_eq!(db.next_reminder().unwrap(), Some(2_000));

        assert!(db.take_due(1_999).unwrap().is_empty());
        let fired = db.take_due(2_000).unwrap();
        assert_eq!(fired.iter().map(|t| t.id).collect::<Vec<_>>(), [b.id]);
        assert_eq!(fired[0].remind_at, Some(2_000));
        assert_eq!(db.get(b.id).unwrap().remind_at, None);
        // Does not refire.
        assert!(db.take_due(2_500).unwrap().is_empty());
        assert_eq!(db.next_reminder().unwrap(), Some(5_000));

        // Missed while closed: everything in the past fires at once, oldest first.
        db.snooze(b.id, 1, 0).unwrap(); // remind at 60_000
        let fired = db.take_due(100_000).unwrap();
        assert_eq!(fired.iter().map(|t| t.id).collect::<Vec<_>>(), [a.id, b.id]);
        assert_eq!(db.next_reminder().unwrap(), None);
    }

    #[test]
    fn sleep_is_capped_and_never_negative() {
        assert_eq!(sleep_for(None, 0), None);
        assert_eq!(sleep_for(Some(500), 0), Some(Duration::from_millis(500)));
        assert_eq!(sleep_for(Some(0), 1_000), Some(Duration::ZERO));
        assert_eq!(sleep_for(Some(10_000_000), 0), Some(MAX_SLEEP));
    }

    #[test]
    fn patch_distinguishes_absent_and_null() {
        let p = patch(serde_json::json!({"due": null, "remindAt": 1.0e12}));
        assert_eq!(p.due, Some(None));
        assert_eq!(p.remind_at, Some(Some(1.0e12)));
        assert!(patch(serde_json::json!({})).due.is_none());
        assert!(to_ms(-1.0).is_err());
        assert!(to_ms(f64::NAN).is_err());
        assert_eq!(to_ms(1.6).unwrap(), 2);
    }

    #[test]
    fn body_formats_due() {
        assert_eq!(reminder_body(None, 0), "Reminder");
        let now = now_ms();
        assert!(reminder_body(Some(now), now).starts_with("Due "));
        assert_eq!(reminder_body(Some(now), now).len(), "Due 14:30".len());
    }

    #[test]
    fn file_database_persists() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("deck.db");
        {
            let db = TodoDb::open(&path).unwrap();
            db.add("persist me", None, None, 1).unwrap();
        }
        assert_eq!(texts(&TodoDb::open(&path).unwrap()), ["persist me"]);
    }
}
