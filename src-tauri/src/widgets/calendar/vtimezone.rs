//! A feed's own `VTIMEZONE` (RFC 5545 §3.6.5), used only for `TZID`s that are neither IANA
//! nor Windows names (e.g. Outlook's "(UTC+03:00) Istanbul" or "Customized Time Zone").
//!
//! Each STANDARD/DAYLIGHT observance contributes onsets (its DTSTART, RRULE instances and
//! RDATEs, all in the observance's TZOFFSETFROM wall clock). They are expanded once, when
//! the zone is first needed, into a sorted UTC transition table, so resolving a time is a
//! binary search. A block without RRULE is a fixed offset.
//!
//! Local → UTC follows RFC 5545 §3.3.5: a wall-clock time that occurs twice (fall-back)
//! means the first occurrence; one that does not exist (spring-forward gap) is read with
//! the offset in effect before the gap, so it lands that far past the gap.

use chrono::{Datelike, NaiveDateTime, TimeDelta, TimeZone};
use rrule::{RRule, Tz, Unvalidated};

use super::ics::{split_until, ContentLine};

/// Onsets are expanded up to this year (yearly DST rules have no end).
const LAST_YEAR: i32 = 2200;
/// Per observance; a runaway rule cannot stall the parser.
const MAX_ONSETS: usize = 2_000;
/// Offsets beyond ±18 h are invalid; such observances are skipped.
const MAX_OFFSET_SECS: i32 = 18 * 3_600;
/// Window used to find candidate offsets around a wall-clock time. Transitions closer
/// together than this are not resolved exactly (real zones never do that).
const PROBE_SECS: i64 = 86_400;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CustomTz {
    pub tzid: String,
    /// Offset (seconds east of UTC) before the first transition.
    initial: i32,
    /// (UTC seconds, offset from then on), sorted by time.
    transitions: Vec<(i64, i32)>,
}

#[derive(Default)]
struct Observance {
    start: Option<NaiveDateTime>,
    from: Option<i32>,
    to: Option<i32>,
    rrule: Option<String>,
    rdates: Vec<NaiveDateTime>,
}

/// `±HHMM[SS]` (also tolerates `±HH:MM`).
pub fn parse_offset(raw: &str) -> Option<i32> {
    let s = raw.trim().replace(':', "");
    let (sign, digits) = match s.as_bytes().first()? {
        b'+' => (1, &s[1..]),
        b'-' => (-1, &s[1..]),
        _ => (1, &s[..]),
    };
    if !(digits.len() == 4 || digits.len() == 6) || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let h: i32 = digits[0..2].parse().ok()?;
    let m: i32 = digits[2..4].parse().ok()?;
    let sec: i32 = digits.get(4..6).map_or(Some(0), |x| x.parse().ok())?;
    if m >= 60 || sec >= 60 {
        return None;
    }
    let total = sign * (h * 3_600 + m * 60 + sec);
    (total.abs() <= MAX_OFFSET_SECS).then_some(total)
}

fn parse_local(raw: &str) -> Option<NaiveDateTime> {
    let v = raw.trim().trim_end_matches(['Z', 'z']);
    NaiveDateTime::parse_from_str(v, "%Y%m%dT%H%M%S")
        .or_else(|_| NaiveDateTime::parse_from_str(v, "%Y%m%dT%H%M"))
        .ok()
        .or_else(|| {
            chrono::NaiveDate::parse_from_str(v.get(..8)?, "%Y%m%d")
                .ok()?
                .and_hms_opt(0, 0, 0)
        })
}

/// Onset wall-clock times of one observance (DTSTART is always the first, §3.8.5.3).
fn onsets(o: &Observance, start: NaiveDateTime, from: i32) -> Vec<NaiveDateTime> {
    let mut out = vec![start];
    out.extend(o.rdates.iter().copied());
    let Some(rule) = o.rrule.as_deref() else {
        return out;
    };
    let (rest, until) = split_until(rule);
    let Ok(mut parsed) = rest.parse::<RRule<Unvalidated>>() else {
        return out;
    };
    if let Some(u) = until {
        let utc = u.trim().ends_with(['Z', 'z']);
        let Some(mut until_wall) = parse_local(u) else {
            return out;
        };
        if utc {
            // UNTIL is the UTC instant of the last onset; onsets are in `from` wall clock.
            until_wall += TimeDelta::seconds(i64::from(from));
        }
        parsed = parsed.until(Tz::UTC.from_utc_datetime(&until_wall));
    }
    // Wall-clock frame: naive times tagged as UTC, so no zone rules interfere.
    let Ok(set) = parsed.build(Tz::UTC.from_utc_datetime(&start)) else {
        return out;
    };
    out.extend(
        (&set.limit())
            .into_iter()
            .take(MAX_ONSETS)
            .map(|d| d.naive_utc())
            .take_while(|d| d.year() <= LAST_YEAR),
    );
    out
}

impl CustomTz {
    /// Builds a zone from the content lines of one VTIMEZONE (its nested BEGIN/END lines
    /// included). `None` when no observance has a usable TZOFFSETTO.
    pub fn from_lines(lines: &[ContentLine]) -> Option<CustomTz> {
        let mut tzid = String::new();
        let mut observances = Vec::new();
        let mut current: Option<Observance> = None;
        for l in lines {
            let comp = || l.value.trim().to_ascii_uppercase();
            match l.name.as_str() {
                "BEGIN" if matches!(comp().as_str(), "STANDARD" | "DAYLIGHT") => {
                    current = Some(Observance::default());
                }
                "END" if matches!(comp().as_str(), "STANDARD" | "DAYLIGHT") => {
                    observances.extend(current.take());
                }
                "TZID" if current.is_none() => tzid = l.value.trim().to_string(),
                name => {
                    let Some(o) = current.as_mut() else {
                        continue;
                    };
                    match name {
                        "DTSTART" => o.start = parse_local(&l.value),
                        "TZOFFSETFROM" => o.from = parse_offset(&l.value),
                        "TZOFFSETTO" => o.to = parse_offset(&l.value),
                        "RRULE" => o.rrule = Some(l.value.trim().to_string()),
                        "RDATE" => o.rdates.extend(l.value.split(',').filter_map(parse_local)),
                        _ => {}
                    }
                }
            }
        }
        // An observance left open by a truncated feed still counts.
        observances.extend(current);

        // (UTC seconds, offset before, offset after)
        let mut all: Vec<(i64, i32, i32)> = Vec::new();
        for o in &observances {
            let Some(to) = o.to else {
                continue;
            };
            let from = o.from.unwrap_or(to);
            let start = o.start.unwrap_or_default(); // 1970: lenient for a missing DTSTART
            for onset in onsets(o, start, from) {
                all.push((onset.and_utc().timestamp() - i64::from(from), from, to));
            }
        }
        if all.is_empty() {
            return None;
        }
        all.sort_by_key(|t| t.0);
        all.dedup_by_key(|t| t.0);
        Some(CustomTz {
            tzid,
            initial: all[0].1,
            transitions: all.into_iter().map(|(t, _, to)| (t, to)).collect(),
        })
    }

    /// Offset (seconds east of UTC) in effect at a UTC instant.
    pub fn offset_at(&self, utc_secs: i64) -> i32 {
        let i = self.transitions.partition_point(|t| t.0 <= utc_secs);
        match i {
            0 => self.initial,
            _ => self.transitions[i - 1].1,
        }
    }

    /// Wall-clock time at a UTC instant.
    pub fn to_local(&self, utc: NaiveDateTime) -> NaiveDateTime {
        let off = self.offset_at(utc.and_utc().timestamp());
        utc + TimeDelta::seconds(i64::from(off))
    }

    /// UTC instant of a wall-clock time: first occurrence when ambiguous, offset before
    /// the gap when it does not exist (RFC 5545 §3.3.5).
    pub fn to_utc(&self, local: NaiveDateTime) -> NaiveDateTime {
        let l = local.and_utc().timestamp();
        let before = self.offset_at(l - PROBE_SECS);
        let after = self.offset_at(l + PROBE_SECS);
        let valid = |off: i32| {
            let u = l - i64::from(off);
            (self.offset_at(u) == off).then_some(u)
        };
        let utc = match (valid(before), valid(after)) {
            (Some(a), Some(b)) => a.min(b),
            (Some(a), None) | (None, Some(a)) => a,
            (None, None) => l - i64::from(before),
        };
        chrono::DateTime::from_timestamp(utc, 0).map_or(local, |d| d.naive_utc())
    }
}

#[cfg(test)]
mod tests {
    use super::super::ics::parse_line;
    use super::*;

    fn zone(body: &str) -> Option<CustomTz> {
        let lines: Vec<ContentLine> = body.lines().filter_map(parse_line).collect();
        CustomTz::from_lines(&lines)
    }

    fn at(s: &str) -> NaiveDateTime {
        parse_local(s).unwrap()
    }

    #[test]
    fn parses_offsets() {
        assert_eq!(parse_offset("+0200"), Some(7_200));
        assert_eq!(parse_offset("-0330"), Some(-12_600));
        assert_eq!(parse_offset("+053045"), Some(19_845));
        assert_eq!(parse_offset("+05:45"), Some(20_700));
        assert_eq!(parse_offset("0100"), Some(3_600));
        assert_eq!(parse_offset("+1900"), None);
        assert_eq!(parse_offset("+0160"), None);
        assert_eq!(parse_offset("junk"), None);
        assert_eq!(parse_offset(""), None);
    }

    #[test]
    fn gap_and_overlap_follow_rfc_5545() {
        // Central European rules around both 2026 transitions.
        let tz = zone(
            "BEGIN:VTIMEZONE\nTZID:Mitteleuropa\n\
             BEGIN:STANDARD\nDTSTART:19961027T030000\nRRULE:FREQ=YEARLY;BYMONTH=10;BYDAY=-1SU\n\
             TZOFFSETFROM:+0200\nTZOFFSETTO:+0100\nEND:STANDARD\n\
             BEGIN:DAYLIGHT\nDTSTART:19810329T020000\nRRULE:FREQ=YEARLY;BYMONTH=3;BYDAY=-1SU\n\
             TZOFFSETFROM:+0100\nTZOFFSETTO:+0200\nEND:DAYLIGHT\nEND:VTIMEZONE",
        )
        .unwrap();
        assert_eq!(tz.tzid, "Mitteleuropa");
        // Gap (29 Mar 02:00–03:00): offset before the gap (+1), so 02:30 → 01:30Z.
        assert_eq!(tz.to_utc(at("20260329T013000")), at("20260329T003000"));
        assert_eq!(tz.to_utc(at("20260329T023000")), at("20260329T013000"));
        assert_eq!(tz.to_utc(at("20260329T030000")), at("20260329T010000"));
        // Overlap (25 Oct 02:00–03:00 twice): the first, i.e. still +2.
        assert_eq!(tz.to_utc(at("20261025T023000")), at("20261025T003000"));
        assert_eq!(tz.to_utc(at("20261025T030000")), at("20261025T020000"));
        // Before the first transition: the earliest observance's TZOFFSETFROM.
        assert_eq!(
            tz.offset_at(at("19700101T000000").and_utc().timestamp()),
            3_600
        );
        assert_eq!(tz.to_local(at("20260701T100000")), at("20260701T120000"));
    }

    #[test]
    fn needs_a_usable_offset() {
        assert!(zone("BEGIN:VTIMEZONE\nTZID:Empty\nEND:VTIMEZONE").is_none());
        let fixed = zone(
            "TZID:Kathmandu-ish\nBEGIN:STANDARD\nDTSTART:19860101T000000\nTZOFFSETTO:+0545\nEND:STANDARD",
        )
        .unwrap();
        assert_eq!(fixed.to_utc(at("20261104T120000")), at("20261104T061500"));
    }
}
