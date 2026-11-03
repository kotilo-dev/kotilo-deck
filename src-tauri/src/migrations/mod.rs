//! `schemaVersion` upgrades for persisted JSON (SPEC §8 cross-cutting "Data & migrations").
//!
//! Each schema has a `CURRENT` version and an ordered list of steps; step `i` upgrades
//! version `i` to `i + 1`. A document without `schemaVersion` is v0. Documents from a
//! future version are rejected (callers skip them and leave the file untouched).
//!
//! The SQLite database (`deck.db`, todos) uses the same idea with `PRAGMA user_version`
//! as its schemaVersion and [`DB_STEPS`] as the ordered list of SQL migrations.

use serde_json::{Map, Value};

pub const WORKSPACE_VERSION: u64 = 1;
pub const SETTINGS_VERSION: u32 = 1;

/// SQL migrations for `deck.db`; step `i` upgrades `user_version` `i` to `i + 1`.
/// Append only — never edit a released step.
pub const DB_STEPS: &[&str] = &[
    // v1: todos (Phase 2).
    "CREATE TABLE todos (
        id           INTEGER PRIMARY KEY AUTOINCREMENT,
        text         TEXT    NOT NULL,
        done         INTEGER NOT NULL DEFAULT 0,
        due          INTEGER,
        remind_at    INTEGER,
        position     INTEGER NOT NULL,
        created_at   INTEGER NOT NULL,
        completed_at INTEGER
    );
    CREATE INDEX todos_remind_at ON todos (remind_at) WHERE remind_at IS NOT NULL AND done = 0;",
];

pub const DB_VERSION: i64 = DB_STEPS.len() as i64;

/// Brings `conn` to [`DB_VERSION`], one transaction per step. A database written by a
/// newer build is refused (and left untouched). Returns the version it started from.
pub fn migrate_db(conn: &mut rusqlite::Connection) -> Result<i64, String> {
    let from: i64 = conn
        .pragma_query_value(None, "user_version", |r| r.get(0))
        .map_err(|e| format!("cannot read database version: {e}"))?;
    if from > DB_VERSION {
        return Err(format!(
            "database schemaVersion {from} is newer than this build supports"
        ));
    }
    for v in from.max(0)..DB_VERSION {
        let mut step = || -> rusqlite::Result<()> {
            let tx = conn.transaction()?;
            tx.execute_batch(DB_STEPS[v as usize])?;
            tx.pragma_update(None, "user_version", v + 1)?;
            tx.commit()
        };
        step().map_err(|e| format!("database migration from v{v} failed: {e}"))?;
    }
    Ok(from)
}

type Step = fn(Map<String, Value>) -> Result<Map<String, Value>, String>;

/// v0 → v1: pre-release files had no `schemaVersion`; the shape is otherwise identical.
fn workspace_v0_to_v1(doc: Map<String, Value>) -> Result<Map<String, Value>, String> {
    Ok(doc)
}

const WORKSPACE_STEPS: &[Step] = &[workspace_v0_to_v1];

#[derive(Debug, PartialEq)]
pub enum MigrationError {
    NotAnObject,
    BadVersion,
    Future(u64),
    Step { from: u64, reason: String },
}

impl std::fmt::Display for MigrationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotAnObject => write!(f, "document is not a JSON object"),
            Self::BadVersion => write!(f, "schemaVersion is not a non-negative integer"),
            Self::Future(v) => write!(f, "schemaVersion {v} is newer than this build supports"),
            Self::Step { from, reason } => write!(f, "migration from v{from} failed: {reason}"),
        }
    }
}

/// Reads `schemaVersion` (missing → 0).
pub fn version_of(doc: &Value) -> Result<u64, MigrationError> {
    let obj = doc.as_object().ok_or(MigrationError::NotAnObject)?;
    match obj.get("schemaVersion") {
        None => Ok(0),
        Some(v) => v.as_u64().ok_or(MigrationError::BadVersion),
    }
}

fn run(doc: Value, current: u64, steps: &[Step]) -> Result<(Value, bool), MigrationError> {
    debug_assert_eq!(steps.len() as u64, current);
    let from = version_of(&doc)?;
    if from > current {
        return Err(MigrationError::Future(from));
    }
    let Value::Object(mut obj) = doc else {
        return Err(MigrationError::NotAnObject);
    };
    for v in from..current {
        obj = steps[v as usize](obj).map_err(|reason| MigrationError::Step { from: v, reason })?;
        obj.insert("schemaVersion".into(), Value::from(v + 1));
    }
    Ok((Value::Object(obj), from != current))
}

/// Upgrades a workspace document to [`WORKSPACE_VERSION`]. Returns `(doc, changed)`.
pub fn migrate_workspace(doc: Value) -> Result<(Value, bool), MigrationError> {
    run(doc, WORKSPACE_VERSION, WORKSPACE_STEPS)
}

/// Settings live in the plugin store and are read field-by-field with defaults, so the
/// only v0 → v1 change is stamping the version. Future versions are read best-effort.
pub fn migrate_settings(mut entries: Map<String, Value>) -> Map<String, Value> {
    if !entries.contains_key("schemaVersion") {
        entries.insert("schemaVersion".into(), Value::from(SETTINGS_VERSION));
    }
    entries
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn v0_gets_schema_version() {
        let (doc, changed) = migrate_workspace(json!({"id": "a", "widgets": []})).unwrap();
        assert!(changed);
        assert_eq!(doc["schemaVersion"], 1);
        assert_eq!(doc["id"], "a");
    }

    #[test]
    fn current_is_untouched() {
        let input = json!({"schemaVersion": 1, "id": "a"});
        let (doc, changed) = migrate_workspace(input.clone()).unwrap();
        assert!(!changed);
        assert_eq!(doc, input);
    }

    #[test]
    fn future_version_is_rejected() {
        assert_eq!(
            migrate_workspace(json!({"schemaVersion": 99})),
            Err(MigrationError::Future(99))
        );
    }

    #[test]
    fn garbage_is_rejected() {
        assert_eq!(
            migrate_workspace(json!([1, 2])),
            Err(MigrationError::NotAnObject)
        );
        assert_eq!(
            migrate_workspace(json!({"schemaVersion": "1"})),
            Err(MigrationError::BadVersion)
        );
    }

    #[test]
    fn db_migrates_once_and_refuses_future() {
        let mut conn = rusqlite::Connection::open_in_memory().unwrap();
        assert_eq!(migrate_db(&mut conn).unwrap(), 0);
        assert_eq!(migrate_db(&mut conn).unwrap(), DB_VERSION);
        let v: i64 = conn
            .pragma_query_value(None, "user_version", |r| r.get(0))
            .unwrap();
        assert_eq!(v, DB_VERSION);
        conn.pragma_update(None, "user_version", DB_VERSION + 1)
            .unwrap();
        assert!(migrate_db(&mut conn).is_err());
    }

    #[test]
    fn settings_v0_stamped() {
        let m = migrate_settings(Map::new());
        assert_eq!(m["schemaVersion"], 1);
    }
}
