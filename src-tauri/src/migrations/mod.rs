//! `schemaVersion` upgrades for persisted JSON (SPEC §8 cross-cutting "Data & migrations").
//!
//! Each schema has a `CURRENT` version and an ordered list of steps; step `i` upgrades
//! version `i` to `i + 1`. A document without `schemaVersion` is v0. Documents from a
//! future version are rejected (callers skip them and leave the file untouched).

use serde_json::{Map, Value};

pub const WORKSPACE_VERSION: u64 = 1;
pub const SETTINGS_VERSION: u32 = 1;

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
    fn settings_v0_stamped() {
        let m = migrate_settings(Map::new());
        assert_eq!(m["schemaVersion"], 1);
    }
}
