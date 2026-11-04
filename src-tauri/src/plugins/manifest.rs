//! `manifest.json` validation (`docs/plugin-api.md` ▸ manifest.json).
//!
//! The result is a normalised manifest: known fields only, defaults filled in, so the
//! frontend can trust every field of `PluginManifest`.

use serde_json::{json, Map, Value};

use crate::platform::Capability;

pub const MAX_MANIFEST_BYTES: u64 = 64 * 1024;
pub const DEFAULT_ENTRY: &str = "index.html";
const TOPICS: &[&str] = &["system", "media", "pomodoro"];
const MAX_SIZES: usize = 8;
const MAX_GRID: u32 = 12;
const MAX_NAME_CHARS: usize = 64;
const MAX_SETTING_KEY_CHARS: usize = 64;

/// `^[a-z0-9][a-z0-9-]{0,31}$`.
pub fn valid_folder_id(id: &str) -> bool {
    let b = id.as_bytes();
    !b.is_empty()
        && b.len() <= 32
        && (b[0].is_ascii_lowercase() || b[0].is_ascii_digit())
        && b[1..]
            .iter()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'-')
}

/// `"WxH"` in canonical form (no sign, no leading zeros), 1 ≤ W,H ≤ 12.
pub fn valid_size_key(s: &str) -> bool {
    let Some((w, h)) = s.split_once('x') else {
        return false;
    };
    let dim = |p: &str| {
        p.bytes().all(|c| c.is_ascii_digit())
            && p.parse::<u32>()
                .is_ok_and(|n| (1..=MAX_GRID).contains(&n) && n.to_string() == p)
    };
    dim(w) && dim(h)
}

/// Relative path inside the plugin folder: `/`-separated, no empty / `.` / `..` segments,
/// no leading `/`, no backslashes, drive letters, NULs or other control characters.
pub fn valid_relative_path(path: &str) -> bool {
    !path.is_empty()
        && path.len() <= 512
        && path.split('/').all(|seg| {
            !seg.is_empty()
                && seg != "."
                && seg != ".."
                && !seg.chars().any(|c| {
                    c.is_control() || matches!(c, '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|')
                })
        })
}

fn string_field<'a>(m: &'a Map<String, Value>, key: &str) -> Result<Option<&'a str>, String> {
    match m.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) => Ok(Some(s)),
        Some(_) => Err(format!("\"{key}\" must be a string")),
    }
}

fn string_array(m: &Map<String, Value>, key: &str) -> Result<Option<Vec<String>>, String> {
    match m.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Array(items)) => items
            .iter()
            .map(|v| {
                v.as_str()
                    .map(str::to_string)
                    .ok_or_else(|| format!("\"{key}\" must be an array of strings"))
            })
            .collect::<Result<Vec<_>, _>>()
            .map(Some),
        Some(_) => Err(format!("\"{key}\" must be an array of strings")),
    }
}

fn dedup(v: Vec<String>) -> Vec<String> {
    let mut out: Vec<String> = Vec::with_capacity(v.len());
    for s in v {
        if !out.contains(&s) {
            out.push(s);
        }
    }
    out
}

/// One generic-form setting: boolean / string (+enum) / number.
fn validate_setting(key: &str, schema: &Value) -> Result<Value, String> {
    let bad = |why: &str| Err(format!("setting \"{key}\": {why}"));
    let Some(s) = schema.as_object() else {
        return bad("must be an object");
    };
    let mut out = Map::new();
    let ty = s.get("type").and_then(Value::as_str).unwrap_or_default();
    out.insert("type".into(), json!(ty));
    match string_field(s, "title") {
        Ok(Some(t)) => {
            out.insert("title".into(), json!(t));
        }
        Ok(None) => {}
        Err(_) => return bad("\"title\" must be a string"),
    }
    let default = s.get("default").filter(|v| !v.is_null());
    match ty {
        "boolean" => {
            if let Some(d) = default {
                if !d.is_boolean() {
                    return bad("default must be a boolean");
                }
                out.insert("default".into(), d.clone());
            }
        }
        "string" => {
            let options = match string_array(s, "enum") {
                Ok(o) => o,
                Err(_) => return bad("\"enum\" must be an array of strings"),
            };
            if let Some(options) = &options {
                if options.is_empty() {
                    return bad("\"enum\" must not be empty");
                }
                out.insert("enum".into(), json!(dedup(options.clone())));
            }
            if let Some(d) = default {
                let Some(d) = d.as_str() else {
                    return bad("default must be a string");
                };
                if options.as_ref().is_some_and(|o| !o.iter().any(|x| x == d)) {
                    return bad("default must be one of \"enum\"");
                }
                out.insert("default".into(), json!(d));
            }
        }
        "number" => {
            for k in ["minimum", "maximum", "step", "default"] {
                match s.get(k) {
                    None | Some(Value::Null) => {}
                    Some(v) if v.is_number() => {
                        out.insert(k.into(), v.clone());
                    }
                    Some(_) => return bad(&format!("\"{k}\" must be a number")),
                }
            }
            let num = |k: &str| out.get(k).and_then(Value::as_f64);
            if let (Some(lo), Some(hi)) = (num("minimum"), num("maximum")) {
                if lo > hi {
                    return bad("\"minimum\" is greater than \"maximum\"");
                }
            }
            if num("step").is_some_and(|st| st <= 0.0) {
                return bad("\"step\" must be positive");
            }
        }
        _ => return bad("type must be \"boolean\", \"string\" or \"number\""),
    }
    Ok(Value::Object(out))
}

fn validate_settings(v: Option<&Value>) -> Result<Value, String> {
    let Some(v) = v.filter(|v| !v.is_null()) else {
        return Ok(json!({ "type": "object", "properties": {} }));
    };
    let obj = v
        .as_object()
        .ok_or("\"settings\" must be an object schema")?;
    if obj.get("type").and_then(Value::as_str) != Some("object") {
        return Err("\"settings.type\" must be \"object\"".into());
    }
    let props = match obj.get("properties") {
        None | Some(Value::Null) => Map::new(),
        Some(Value::Object(p)) => p.clone(),
        Some(_) => return Err("\"settings.properties\" must be an object".into()),
    };
    let mut out = Map::new();
    for (key, schema) in &props {
        if key.is_empty() || key.chars().count() > MAX_SETTING_KEY_CHARS {
            return Err(format!(
                "setting names must be 1–{MAX_SETTING_KEY_CHARS} characters"
            ));
        }
        out.insert(key.clone(), validate_setting(key, schema)?);
    }
    Ok(json!({ "type": "object", "properties": out }))
}

/// Validates the parsed manifest of `folder`. `entry_ok` checks that the (syntactically
/// valid) entry exists as a file inside the folder. Returns the normalised manifest.
pub fn validate(
    folder: &str,
    raw: &Value,
    entry_ok: impl FnOnce(&str) -> bool,
) -> Result<Value, String> {
    let m = raw
        .as_object()
        .ok_or("manifest.json must be a JSON object")?;

    // `as_u64` is `None` for floats, so `1.0` and `"1"` are rejected too.
    if m.get("apiVersion").and_then(Value::as_u64) != Some(1) {
        return Err("\"apiVersion\" must be 1".into());
    }

    let id = string_field(m, "id")?.ok_or("\"id\" is missing")?;
    if id != folder {
        return Err(format!(
            "\"id\" (\"{}\") must equal the folder name",
            id.chars().take(40).collect::<String>()
        ));
    }

    let name = string_field(m, "name")?.ok_or("\"name\" is missing")?;
    let name_len = name.chars().count();
    if name.trim().is_empty() || name_len > MAX_NAME_CHARS {
        return Err(format!("\"name\" must be 1–{MAX_NAME_CHARS} characters"));
    }

    let icon = string_field(m, "icon")?.unwrap_or("puzzle");

    let sizes = string_array(m, "sizes")?.ok_or("\"sizes\" is missing")?;
    if sizes.is_empty() || sizes.len() > MAX_SIZES {
        return Err(format!("\"sizes\" must list 1–{MAX_SIZES} sizes"));
    }
    for (i, s) in sizes.iter().enumerate() {
        if !valid_size_key(s) {
            return Err(format!(
                "size \"{}\" is not \"WxH\" with 1 ≤ W,H ≤ {MAX_GRID}",
                s.chars().take(16).collect::<String>()
            ));
        }
        if sizes[..i].contains(s) {
            return Err(format!("size \"{s}\" is listed twice"));
        }
    }

    let default_size = string_field(m, "defaultSize")?.ok_or("\"defaultSize\" is missing")?;
    if !sizes.iter().any(|s| s == default_size) {
        return Err("\"defaultSize\" must be one of \"sizes\"".into());
    }

    let capabilities = dedup(string_array(m, "capabilities")?.unwrap_or_default());
    for c in &capabilities {
        if serde_json::from_value::<Capability>(json!(c)).is_err() {
            return Err(format!(
                "unknown capability \"{}\"",
                c.chars().take(32).collect::<String>()
            ));
        }
    }

    let multi_instance = match m.get("multiInstance") {
        None | Some(Value::Null) => true,
        Some(Value::Bool(b)) => *b,
        Some(_) => return Err("\"multiInstance\" must be a boolean".into()),
    };

    let entry = string_field(m, "entry")?.unwrap_or(DEFAULT_ENTRY);
    if !valid_relative_path(entry) {
        return Err("\"entry\" must be a relative path inside the folder".into());
    }
    if !entry_ok(entry) {
        return Err(format!(
            "entry file \"{}\" does not exist",
            entry.chars().take(64).collect::<String>()
        ));
    }

    let topics = dedup(string_array(m, "topics")?.unwrap_or_default());
    if let Some(t) = topics.iter().find(|t| !TOPICS.contains(&t.as_str())) {
        return Err(format!(
            "unknown topic \"{}\" (system, media, pomodoro)",
            t.chars().take(32).collect::<String>()
        ));
    }

    let settings = validate_settings(m.get("settings"))?;

    let mut out = json!({
        "apiVersion": 1,
        "id": id,
        "name": name,
        "icon": icon,
        "sizes": sizes,
        "defaultSize": default_size,
        "capabilities": capabilities,
        "multiInstance": multi_instance,
        "settings": settings,
        "entry": entry,
        "topics": topics,
    });
    for key in ["description", "author", "version"] {
        if let Some(s) = string_field(m, key)? {
            out[key] = json!(s);
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> Value {
        json!({
            "apiVersion": 1,
            "id": "hello",
            "name": "Hello",
            "icon": "puzzle",
            "sizes": ["2x1", "2x2"],
            "defaultSize": "2x2",
            "capabilities": [],
            "multiInstance": true,
            "entry": "index.html",
            "topics": ["system"],
            "settings": {
                "type": "object",
                "properties": {
                    "greeting": { "type": "string", "title": "Greeting", "default": "Hello" }
                }
            },
            "description": "Example widget",
            "author": "you",
            "version": "1.0.0"
        })
    }

    fn check(v: &Value) -> Result<Value, String> {
        validate("hello", v, |_| true)
    }

    fn with(key: &str, value: Value) -> Value {
        let mut v = base();
        v[key] = value;
        v
    }

    fn without(key: &str) -> Value {
        let mut v = base();
        v.as_object_mut().unwrap().remove(key);
        v
    }

    #[test]
    fn folder_ids() {
        for ok in ["hello", "a", "0", "my-widget-2", &"a".repeat(32)] {
            assert!(valid_folder_id(ok), "{ok}");
        }
        for bad in ["", "-a", "Hello", "a_b", "a.b", "ä", &"a".repeat(33), "a b"] {
            assert!(!valid_folder_id(bad), "{bad}");
        }
    }

    #[test]
    fn size_keys() {
        for ok in ["1x1", "2x2", "12x12", "4x2"] {
            assert!(valid_size_key(ok), "{ok}");
        }
        for bad in [
            "0x1", "13x1", "1x13", "02x1", "+1x1", "2X2", "2x", "x2", "2x2x2", "",
        ] {
            assert!(!valid_size_key(bad), "{bad}");
        }
    }

    #[test]
    fn relative_paths() {
        for ok in ["index.html", "a/b.js", "img/logo.svg", "with space.css"] {
            assert!(valid_relative_path(ok), "{ok}");
        }
        for bad in [
            "",
            "/index.html",
            "../x",
            "a/../b",
            "a//b",
            "./a",
            "a\\b",
            "C:/x",
            "a\0b",
            "a/",
        ] {
            assert!(!valid_relative_path(bad), "{bad:?}");
        }
    }

    #[test]
    fn full_manifest_is_normalised() {
        let mut raw = base();
        raw["unknownField"] = json!(1);
        let m = check(&raw).unwrap();
        assert!(m.get("unknownField").is_none());
        assert_eq!(m["settings"]["properties"]["greeting"]["default"], "Hello");
        assert_eq!(m["topics"], json!(["system"]));
        assert_eq!(m["version"], "1.0.0");
    }

    #[test]
    fn defaults_are_filled() {
        let raw = json!({
            "apiVersion": 1, "id": "hello", "name": "Hello",
            "sizes": ["2x2"], "defaultSize": "2x2"
        });
        let m = check(&raw).unwrap();
        assert_eq!(m["icon"], "puzzle");
        assert_eq!(m["capabilities"], json!([]));
        assert_eq!(m["multiInstance"], true);
        assert_eq!(m["entry"], "index.html");
        assert_eq!(m["topics"], json!([]));
        assert_eq!(m["settings"], json!({"type": "object", "properties": {}}));
        assert!(m.get("description").is_none());
    }

    /// One row per rule in plugin-api.md: (case, manifest, expected error fragment).
    #[test]
    fn validation_table() {
        let cases: Vec<(&str, Value, &str)> = vec![
            ("not an object", json!([1]), "JSON object"),
            ("apiVersion missing", without("apiVersion"), "apiVersion"),
            ("apiVersion 2", with("apiVersion", json!(2)), "apiVersion"),
            (
                "apiVersion string",
                with("apiVersion", json!("1")),
                "apiVersion",
            ),
            (
                "apiVersion float",
                with("apiVersion", json!(1.0)),
                "apiVersion",
            ),
            ("id missing", without("id"), "\"id\""),
            ("id mismatch", with("id", json!("other")), "folder name"),
            ("name missing", without("name"), "\"name\""),
            ("name empty", with("name", json!("")), "1–64"),
            ("name blank", with("name", json!("   ")), "1–64"),
            ("name long", with("name", json!("x".repeat(65))), "1–64"),
            ("name type", with("name", json!(5)), "string"),
            ("icon type", with("icon", json!(true)), "\"icon\""),
            ("sizes missing", without("sizes"), "\"sizes\""),
            ("sizes empty", with("sizes", json!([])), "1–8"),
            (
                "sizes too many",
                with(
                    "sizes",
                    json!(["1x1", "1x2", "1x3", "1x4", "1x5", "1x6", "1x7", "1x8", "1x9"]),
                ),
                "1–8",
            ),
            ("size zero", with("sizes", json!(["0x1"])), "WxH"),
            ("size big", with("sizes", json!(["13x1"])), "WxH"),
            ("size garbage", with("sizes", json!(["big"])), "WxH"),
            ("size dup", with("sizes", json!(["2x2", "2x2"])), "twice"),
            ("size type", with("sizes", json!([2])), "array of strings"),
            ("defaultSize missing", without("defaultSize"), "defaultSize"),
            (
                "defaultSize not in sizes",
                with("defaultSize", json!("4x4")),
                "one of",
            ),
            (
                "capability unknown",
                with("capabilities", json!(["camera"])),
                "unknown capability",
            ),
            (
                "capability type",
                with("capabilities", json!("audio")),
                "array of strings",
            ),
            (
                "multiInstance type",
                with("multiInstance", json!("yes")),
                "boolean",
            ),
            (
                "entry absolute",
                with("entry", json!("/index.html")),
                "relative path",
            ),
            (
                "entry traversal",
                with("entry", json!("../x.html")),
                "relative path",
            ),
            (
                "entry backslash",
                with("entry", json!("a\\b.html")),
                "relative path",
            ),
            ("entry type", with("entry", json!(1)), "\"entry\""),
            (
                "topic unknown",
                with("topics", json!(["network"])),
                "unknown topic",
            ),
            ("topics type", with("topics", json!({})), "array of strings"),
            (
                "settings not object",
                with("settings", json!([])),
                "object schema",
            ),
            (
                "settings type",
                with("settings", json!({"type": "array"})),
                "settings.type",
            ),
            (
                "settings properties type",
                with("settings", json!({"type": "object", "properties": []})),
                "settings.properties",
            ),
            (
                "setting array type",
                with(
                    "settings",
                    json!({"type": "object", "properties": {"z": {"type": "array"}}}),
                ),
                "type must be",
            ),
            (
                "setting bad default",
                with(
                    "settings",
                    json!({"type": "object", "properties": {"b": {"type": "boolean", "default": 1}}}),
                ),
                "boolean",
            ),
            (
                "setting enum default",
                with(
                    "settings",
                    json!({"type": "object", "properties": {"e": {"type": "string", "enum": ["a"], "default": "b"}}}),
                ),
                "one of \"enum\"",
            ),
            (
                "setting empty enum",
                with(
                    "settings",
                    json!({"type": "object", "properties": {"e": {"type": "string", "enum": []}}}),
                ),
                "not be empty",
            ),
            (
                "setting number min>max",
                with(
                    "settings",
                    json!({"type": "object", "properties": {"n": {"type": "number", "minimum": 5, "maximum": 1}}}),
                ),
                "greater",
            ),
            (
                "setting number default type",
                with(
                    "settings",
                    json!({"type": "object", "properties": {"n": {"type": "number", "default": "1"}}}),
                ),
                "number",
            ),
            (
                "description type",
                with("description", json!(1)),
                "description",
            ),
        ];
        for (case, manifest, fragment) in cases {
            let err = check(&manifest).expect_err(case);
            assert!(err.contains(fragment), "{case}: {err}");
        }
    }

    #[test]
    fn known_capabilities_and_dedup() {
        let m = check(&with(
            "capabilities",
            json!(["media", "system-info", "media"]),
        ))
        .unwrap();
        assert_eq!(m["capabilities"], json!(["media", "system-info"]));
        let m = check(&with("topics", json!(["media", "pomodoro", "media"]))).unwrap();
        assert_eq!(m["topics"], json!(["media", "pomodoro"]));
    }

    #[test]
    fn missing_entry_is_rejected() {
        let err = validate("hello", &base(), |_| false).unwrap_err();
        assert!(err.contains("does not exist"), "{err}");
    }

    #[test]
    fn generic_setting_types_pass() {
        let m = check(&with(
            "settings",
            json!({"type": "object", "properties": {
                "b": {"type": "boolean", "default": true},
                "s": {"type": "string", "enum": ["a", "b"], "default": "b"},
                "n": {"type": "number", "minimum": 0, "maximum": 10, "step": 1, "default": 3},
                "t": {"type": "string"}
            }}),
        ))
        .unwrap();
        assert_eq!(m["settings"]["properties"]["n"]["maximum"], 10);
        assert_eq!(m["settings"]["properties"]["s"]["enum"], json!(["a", "b"]));
    }
}
