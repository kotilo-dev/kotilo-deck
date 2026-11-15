//! `TZID` → zone, in this order:
//!
//! 1. Normalised name: IANA as is, a Windows zone name via CLDR ([`windows_zones`]), or
//!    an IANA name behind a vendor prefix (`/mozilla.org/20050126_1/Europe/Berlin`,
//!    `tzone://Microsoft/…`).
//! 2. The feed's own `VTIMEZONE` with that `TZID` ([`CustomTz`]), parsed lazily once.
//! 3. Unknown: `None`, the caller treats the time as floating (local); logged once per
//!    `TZID` per process.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, OnceLock};

use chrono_tz::Tz;

use super::ics::ContentLine;
use super::vtimezone::CustomTz;
use super::windows_zones::windows_to_iana;

/// Vendor prefixes in front of a zone name (compared case-insensitively).
const PREFIXES: &[&str] = &["tzone://Microsoft/"];
/// Bound on remembered unknown names (a hostile feed cannot grow it without limit).
const MAX_LOGGED: usize = 256;

/// A resolved `TZID`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Zone {
    Iana(Tz),
    Custom(Arc<CustomTz>),
}

fn clean(raw: &str) -> &str {
    raw.trim().trim_matches('"').trim()
}

fn strip_prefix_ci<'a>(name: &'a str, prefix: &str) -> Option<&'a str> {
    let head = name.get(..prefix.len())?;
    head.eq_ignore_ascii_case(prefix)
        .then(|| &name[prefix.len()..])
}

/// One name, no prefix handling: IANA, UTC aliases, Windows.
fn direct(name: &str) -> Option<Tz> {
    if let Ok(tz) = name.parse::<Tz>() {
        return Some(tz);
    }
    if ["utc", "gmt", "z", "etc/utc"]
        .iter()
        .any(|a| a.eq_ignore_ascii_case(name))
    {
        return Some(Tz::UTC);
    }
    windows_to_iana(name).and_then(|iana| iana.parse::<Tz>().ok())
}

/// Step 1: resolves a `TZID` by name only; `None` = not a known name.
pub fn resolve_tzid(raw: &str) -> Option<Tz> {
    let mut name = clean(raw);
    if name.is_empty() {
        return None;
    }
    for p in PREFIXES {
        if let Some(rest) = strip_prefix_ci(name, p) {
            name = rest;
        }
    }
    if let Some(tz) = direct(name) {
        return Some(tz);
    }
    // Path-like vendor prefixes: try ever shorter `/`-suffixes ("/mozilla.org/x/Europe/Berlin").
    let parts: Vec<&str> = name.split('/').filter(|p| !p.is_empty()).collect();
    (1..parts.len()).find_map(|skip| direct(&parts[skip..].join("/")))
}

fn log_unknown_once(name: &str) {
    static LOGGED: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    let Ok(mut seen) = LOGGED.get_or_init(Default::default).lock() else {
        return;
    };
    if seen.len() < MAX_LOGGED && seen.insert(name.to_string()) {
        tracing::warn!(tzid = %name, "calendar: unknown time zone, using local time");
    }
}

/// Per-calendar resolver: the feed's VTIMEZONE blocks plus a cache of resolved names, so
/// each distinct `TZID` is resolved (and a VTIMEZONE expanded) once per parse.
#[derive(Default)]
pub struct ZoneTable {
    vtimezones: HashMap<String, Vec<ContentLine>>,
    cache: HashMap<String, Option<Zone>>,
}

fn key(tzid: &str) -> String {
    clean(tzid).to_lowercase()
}

impl ZoneTable {
    /// Registers the content lines of one VTIMEZONE (nested BEGIN/END included). The
    /// first block wins when a feed repeats a `TZID`.
    pub fn add_vtimezone(&mut self, lines: Vec<ContentLine>) {
        let mut depth = 0usize;
        let tzid = lines.iter().find_map(|l| {
            match l.name.as_str() {
                "BEGIN" => depth += 1,
                "END" => depth = depth.saturating_sub(1),
                "TZID" if depth == 0 => return Some(key(&l.value)),
                _ => {}
            }
            None
        });
        if let Some(k) = tzid.filter(|k| !k.is_empty()) {
            self.vtimezones.entry(k).or_insert(lines);
        }
    }

    pub fn lookup(&mut self, tzid: &str) -> Option<Zone> {
        let k = key(tzid);
        if let Some(z) = self.cache.get(&k) {
            return z.clone();
        }
        let zone = resolve_tzid(tzid).map(Zone::Iana).or_else(|| {
            self.vtimezones
                .get(&k)
                .and_then(|lines| CustomTz::from_lines(lines))
                .map(|tz| Zone::Custom(Arc::new(tz)))
        });
        if zone.is_none() && !k.is_empty() {
            log_unknown_once(clean(tzid));
        }
        self.cache.insert(k, zone.clone());
        zone
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_iana_windows_and_prefixed_names() {
        assert_eq!(resolve_tzid("Europe/Istanbul"), Some(Tz::Europe__Istanbul));
        assert_eq!(
            resolve_tzid("W. Europe Standard Time"),
            Some(Tz::Europe__Berlin)
        );
        assert_eq!(
            resolve_tzid("\"Pacific Standard Time\""),
            Some(Tz::America__Los_Angeles)
        );
        assert_eq!(
            resolve_tzid("turkey standard time"),
            Some(Tz::Europe__Istanbul)
        );
        assert_eq!(
            resolve_tzid("/mozilla.org/20050126_1/Europe/Berlin"),
            Some(Tz::Europe__Berlin)
        );
        assert_eq!(
            resolve_tzid("/softwarestudio.org/Olson_20011030_5/America/New_York"),
            Some(Tz::America__New_York)
        );
        assert_eq!(
            resolve_tzid("/citadel.org/20190914_1/Asia/Tokyo"),
            Some(Tz::Asia__Tokyo)
        );
        assert_eq!(
            resolve_tzid("tzone://Microsoft/W. Europe Standard Time"),
            Some(Tz::Europe__Berlin)
        );
        assert_eq!(resolve_tzid("tzone://Microsoft/Utc"), Some(Tz::UTC));
        assert_eq!(resolve_tzid("Customized Time Zone"), None);
        assert_eq!(resolve_tzid("(UTC+03:00) Istanbul"), None);
        assert_eq!(resolve_tzid(""), None);
    }
}
