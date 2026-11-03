//! Lenient iCalendar (RFC 5545) reader and occurrence expansion. Pure: no I/O, no clock.
//!
//! Reading never fails on a single bad line or component: unparseable lines are skipped,
//! unknown components ignored, and a VEVENT without DTSTART dropped. The only hard error
//! is a body without `BEGIN:VCALENDAR` ("Not a calendar file").
//!
//! Times keep their ICS semantics ([`IcsTime`]) so recurrences expand in the event's own
//! zone (DST-correct); only the final occurrences become Unix ms.

use std::collections::{HashMap, HashSet};

use chrono::{
    DateTime, Days, LocalResult, NaiveDate, NaiveDateTime, NaiveTime, TimeDelta, TimeZone,
};
use rrule::{RRule, RRuleSet, Tz, Unvalidated};

use super::zones::resolve_tzid;

/// Upper bound of recurrence instances scanned per series and query (a daily series
/// started decades ago still fits; a runaway `FREQ=SECONDLY` does not stall the deck).
const MAX_SCANNED: usize = 100_000;
/// Event lengths beyond this are clamped (bad `DURATION`/`DTEND` values).
const MAX_SPAN_DAYS: i64 = 3650;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IcsTime {
    /// `VALUE=DATE`: a calendar day, interpreted in the local zone.
    Date(NaiveDate),
    /// No `Z`, no (known) `TZID`: wall-clock time in the local zone.
    Floating(NaiveDateTime),
    Utc(NaiveDateTime),
    Zoned(NaiveDateTime, chrono_tz::Tz),
}

impl IcsTime {
    pub fn is_date(self) -> bool {
        matches!(self, IcsTime::Date(_))
    }

    /// The zone recurrences of this start expand in.
    fn zone(self, local: Tz) -> Tz {
        match self {
            IcsTime::Utc(_) => Tz::UTC,
            IcsTime::Zoned(_, tz) => Tz::Tz(tz),
            IcsTime::Date(_) | IcsTime::Floating(_) => local,
        }
    }

    pub fn resolve(self, local: Tz) -> Option<DateTime<Tz>> {
        match self {
            IcsTime::Date(d) => wall_clock(local, d.and_time(NaiveTime::MIN)),
            IcsTime::Floating(n) => wall_clock(local, n),
            IcsTime::Utc(n) => Some(Tz::UTC.from_utc_datetime(&n)),
            IcsTime::Zoned(n, tz) => wall_clock(Tz::Tz(tz), n),
        }
    }
}

/// Wall-clock time in `tz`; the earlier instant when ambiguous (DST fall-back), shifted
/// past the gap when it does not exist (DST spring-forward).
fn wall_clock(tz: Tz, naive: NaiveDateTime) -> Option<DateTime<Tz>> {
    match tz.from_local_datetime(&naive) {
        LocalResult::Single(t) => Some(t),
        LocalResult::Ambiguous(a, _) => Some(a),
        LocalResult::None => {
            let later = naive.checked_add_signed(TimeDelta::try_hours(1)?)?;
            tz.from_local_datetime(&later).earliest()
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct VEvent {
    pub uid: String,
    pub summary: Option<String>,
    pub location: Option<String>,
    pub start: IcsTime,
    pub end: Option<IcsTime>,
    pub duration: Option<TimeDelta>,
    pub rrule: Option<String>,
    pub exdates: Vec<IcsTime>,
    pub rdates: Vec<IcsTime>,
    pub recurrence_id: Option<IcsTime>,
    pub cancelled: bool,
}

/// One concrete occurrence; `key_ms` is the original (pre-override) start for stable ids.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Occurrence {
    pub uid: String,
    pub key_ms: i64,
    pub start_ms: i64,
    pub end_ms: i64,
    pub all_day: bool,
    pub title: String,
    pub location: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NotCalendar;

// ── Content lines ───────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContentLine {
    pub name: String,
    /// Keys upper-cased; value is the first (unquoted) value of the parameter.
    pub params: Vec<(String, String)>,
    pub value: String,
}

impl ContentLine {
    fn param(&self, key: &str) -> Option<&str> {
        self.params
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }
}

/// RFC 5545 §3.1 unfolding: a line starting with a space or tab continues the previous
/// one (that single whitespace char is removed). Accepts CRLF and bare LF; drops a BOM
/// and blank lines.
pub fn unfold(text: &str) -> Vec<String> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut out: Vec<String> = Vec::new();
    for raw in text.split('\n') {
        let line = raw.strip_suffix('\r').unwrap_or(raw);
        if let Some(rest) = line.strip_prefix([' ', '\t']) {
            if let Some(last) = out.last_mut() {
                last.push_str(rest);
            }
        } else if !line.is_empty() {
            out.push(line.to_string());
        }
    }
    out
}

/// `NAME *(;PARAM=VALUE) : VALUE`, honouring quoted parameter values (which may contain
/// `:` and `;`, e.g. Outlook's `TZID="(UTC+01:00) Amsterdam, Berlin"`).
pub fn parse_line(line: &str) -> Option<ContentLine> {
    let name_end = line.find([';', ':'])?;
    let name = line[..name_end].trim().to_ascii_uppercase();
    if name.is_empty() {
        return None;
    }
    let mut params = Vec::new();
    let mut rest = &line[name_end..];
    while let Some(r) = rest.strip_prefix(';') {
        let bytes = r.as_bytes();
        let mut i = 0;
        let mut quoted = false;
        while i < bytes.len() {
            match bytes[i] {
                b'"' => quoted = !quoted,
                b';' | b':' if !quoted => break,
                _ => {}
            }
            i += 1;
        }
        if i >= bytes.len() {
            return None; // no value delimiter
        }
        let (param, tail) = r.split_at(i);
        if let Some((k, v)) = param.split_once('=') {
            params.push((k.trim().to_ascii_uppercase(), first_param_value(v)));
        }
        rest = tail;
    }
    let value = rest.strip_prefix(':')?;
    Some(ContentLine {
        name,
        params,
        value: value.to_string(),
    })
}

fn first_param_value(raw: &str) -> String {
    let raw = raw.trim();
    match raw.strip_prefix('"') {
        Some(q) => q.split('"').next().unwrap_or_default().to_string(),
        None => raw.split(',').next().unwrap_or_default().trim().to_string(),
    }
}

/// TEXT unescaping (§3.3.11); line breaks become spaces (titles render on one line).
pub fn unescape_text(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut chars = raw.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n' | 'N') => out.push(' '),
            Some(other) => out.push(other),
            None => out.push('\\'),
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

// ── Values ──────────────────────────────────────────────────────────────────

fn parse_date(s: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(s, "%Y%m%d").ok()
}

/// One DATE / DATE-TIME value with the property's `VALUE` and `TZID` parameters.
pub fn parse_time_value(
    value: &str,
    value_type: Option<&str>,
    tzid: Option<&str>,
) -> Option<IcsTime> {
    let v = value.trim();
    let is_date = value_type.is_some_and(|t| t.eq_ignore_ascii_case("DATE"))
        || (v.len() == 8 && v.bytes().all(|b| b.is_ascii_digit()));
    if is_date {
        return parse_date(v.get(..8)?).map(IcsTime::Date);
    }
    let (body, utc) = match v.strip_suffix(['Z', 'z']) {
        Some(b) => (b, true),
        None => (v, false),
    };
    let naive = NaiveDateTime::parse_from_str(body, "%Y%m%dT%H%M%S")
        .or_else(|_| NaiveDateTime::parse_from_str(body, "%Y%m%dT%H%M"))
        .ok()?;
    if utc {
        return Some(IcsTime::Utc(naive));
    }
    Some(match tzid.and_then(resolve_tzid) {
        Some(tz) => IcsTime::Zoned(naive, tz),
        None => IcsTime::Floating(naive),
    })
}

fn line_time(line: &ContentLine) -> Option<IcsTime> {
    parse_time_value(&line.value, line.param("VALUE"), line.param("TZID"))
}

fn line_times(line: &ContentLine) -> impl Iterator<Item = IcsTime> + '_ {
    let period = line
        .param("VALUE")
        .is_some_and(|t| t.eq_ignore_ascii_case("PERIOD"));
    line.value
        .split(',')
        .filter(move |v| !period && !v.contains('/'))
        .filter_map(|v| parse_time_value(v, line.param("VALUE"), line.param("TZID")))
}

/// `[+-]P[nW][nD][T[nH][nM][nS]]` (§3.3.6).
pub fn parse_duration(raw: &str) -> Option<TimeDelta> {
    let s = raw.trim().to_ascii_uppercase();
    let (neg, s) = match s.as_bytes().first()? {
        b'-' => (true, &s[1..]),
        b'+' => (false, &s[1..]),
        _ => (false, &s[..]),
    };
    let s = s.strip_prefix('P')?;
    let mut secs: i64 = 0;
    let mut num = String::new();
    let mut in_time = false;
    let mut any = false;
    for c in s.chars() {
        match c {
            '0'..='9' => num.push(c),
            'T' if num.is_empty() => in_time = true,
            'W' | 'D' | 'H' | 'M' | 'S' => {
                let n: i64 = num.parse().ok()?;
                num.clear();
                let unit = match (c, in_time) {
                    ('W', false) => 7 * 86_400,
                    ('D', false) => 86_400,
                    ('H', true) => 3_600,
                    ('M', true) => 60,
                    ('S', true) => 1,
                    _ => return None,
                };
                secs = secs.checked_add(n.checked_mul(unit)?)?;
                any = true;
            }
            _ => return None,
        }
    }
    if !any || !num.is_empty() {
        return None;
    }
    TimeDelta::try_seconds(if neg { -secs } else { secs })
}

// ── Components ──────────────────────────────────────────────────────────────

/// All VEVENTs of a calendar body, in file order.
pub fn parse_calendar(text: &str) -> Result<Vec<VEvent>, NotCalendar> {
    let mut saw_calendar = false;
    let mut stack: Vec<String> = Vec::new();
    let mut current: Option<Vec<ContentLine>> = None;
    let mut events = Vec::new();
    fn finish(props: Vec<ContentLine>, events: &mut Vec<VEvent>) {
        events.extend(VEvent::from_lines(&props));
    }

    for line in unfold(text) {
        let Some(cl) = parse_line(&line) else {
            continue;
        };
        match cl.name.as_str() {
            "BEGIN" => {
                let comp = cl.value.trim().to_ascii_uppercase();
                if comp == "VCALENDAR" {
                    saw_calendar = true;
                }
                if comp == "VEVENT" {
                    // A missing END:VEVENT must not swallow the next event.
                    if let Some(props) = current.replace(Vec::new()) {
                        finish(props, &mut events);
                    }
                }
                stack.push(comp);
            }
            "END" => {
                let comp = cl.value.trim().to_ascii_uppercase();
                if let Some(pos) = stack.iter().rposition(|c| *c == comp) {
                    let closed_event = stack.drain(pos..).any(|c| c == "VEVENT");
                    if closed_event {
                        if let Some(props) = current.take() {
                            finish(props, &mut events);
                        }
                    }
                }
            }
            _ => {
                if stack.last().is_some_and(|c| c == "VEVENT") {
                    if let Some(props) = current.as_mut() {
                        props.push(cl);
                    }
                }
            }
        }
    }
    if let Some(props) = current.take() {
        finish(props, &mut events);
    }
    if saw_calendar {
        Ok(events)
    } else {
        Err(NotCalendar)
    }
}

/// FNV-1a 64; stable across Rust versions (unlike `DefaultHasher`).
pub fn fnv1a(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |h, b| {
        (h ^ u64::from(*b)).wrapping_mul(0x0100_0000_01b3)
    })
}

impl VEvent {
    fn from_lines(lines: &[ContentLine]) -> Option<VEvent> {
        let get = |name: &str| lines.iter().find(|l| l.name == name);
        let text = |name: &str| {
            get(name)
                .map(|l| unescape_text(&l.value))
                .filter(|s| !s.is_empty())
        };
        let start_line = get("DTSTART")?;
        let start = line_time(start_line)?;
        let summary = text("SUMMARY");
        let uid = text("UID").unwrap_or_else(|| {
            // No UID (rare, broken exporters): derive a stable one.
            let seed = format!("{}|{}", start_line.value, summary.as_deref().unwrap_or(""));
            format!("nouid-{:016x}", fnv1a(seed.as_bytes()))
        });
        let all = |name: &'static str| {
            lines
                .iter()
                .filter(move |l| l.name == name)
                .flat_map(line_times)
                .collect::<Vec<_>>()
        };
        Some(VEvent {
            uid,
            summary,
            location: text("LOCATION"),
            start,
            end: get("DTEND").and_then(line_time),
            duration: get("DURATION").and_then(|l| parse_duration(&l.value)),
            rrule: get("RRULE")
                .map(|l| l.value.trim().to_string())
                .filter(|r| !r.is_empty()),
            exdates: all("EXDATE"),
            rdates: all("RDATE"),
            recurrence_id: get("RECURRENCE-ID").and_then(line_time),
            cancelled: get("STATUS")
                .is_some_and(|l| l.value.trim().eq_ignore_ascii_case("CANCELLED")),
        })
    }
}

// ── Expansion ───────────────────────────────────────────────────────────────

/// Length of each occurrence.
#[derive(Debug, Clone, Copy)]
enum Span {
    /// All-day: N calendar days (DST-safe, ends at local midnight).
    Days(u64),
    Fixed(TimeDelta),
}

impl Span {
    fn of(ev: &VEvent, start: DateTime<Tz>, local: Tz) -> Span {
        let max = TimeDelta::try_days(MAX_SPAN_DAYS).unwrap_or(TimeDelta::MAX);
        if let IcsTime::Date(sd) = ev.start {
            let days = match ev.end {
                Some(IcsTime::Date(ed)) => (ed - sd).num_days(),
                Some(other) => other
                    .resolve(local)
                    .map(|e| (e.date_naive() - sd).num_days())
                    .unwrap_or(1),
                None => ev.duration.map(|d| d.num_days()).unwrap_or(1),
            };
            return Span::Days(days.clamp(1, MAX_SPAN_DAYS) as u64);
        }
        let len = match (ev.end.and_then(|e| e.resolve(local)), ev.duration) {
            (Some(end), _) => end.signed_duration_since(start),
            (None, Some(d)) => d,
            (None, None) => TimeDelta::zero(),
        };
        Span::Fixed(len.clamp(TimeDelta::zero(), max))
    }

    fn at(self, start: DateTime<Tz>, local: Tz) -> Option<(i64, i64)> {
        match self {
            Span::Days(n) => {
                let day = start.date_naive();
                let s = wall_clock(local, day.and_time(NaiveTime::MIN))?;
                let e = wall_clock(
                    local,
                    day.checked_add_days(Days::new(n))?.and_time(NaiveTime::MIN),
                )?;
                Some((s.timestamp_millis(), e.timestamp_millis()))
            }
            Span::Fixed(d) => {
                let s = start.timestamp_millis();
                Some((s, s.saturating_add(d.num_milliseconds())))
            }
        }
    }
}

/// `RRULE` value → rule set anchored at `start`. `UNTIL` is re-expressed in UTC (allowed
/// for every DTSTART kind) so floating/DATE/zoned starts all validate.
fn build_rrule(rule: &str, start: DateTime<Tz>) -> Option<RRuleSet> {
    let mut until = None;
    let parts: Vec<&str> = rule
        .split(';')
        .map(str::trim)
        .filter(|p| match p.split_once('=') {
            Some((k, v)) if k.eq_ignore_ascii_case("UNTIL") => {
                until = Some(v);
                false
            }
            _ => !p.is_empty(),
        })
        .collect();
    let mut parsed: RRule<Unvalidated> = parts.join(";").parse().ok()?;
    if let Some(u) = until {
        let tz = start.timezone();
        let until_dt = match parse_time_value(u, None, None)? {
            // Inclusive: the whole day counts.
            IcsTime::Date(d) => wall_clock(tz, d.and_hms_opt(23, 59, 59)?)?,
            IcsTime::Floating(n) => wall_clock(tz, n)?,
            t => t.resolve(tz)?,
        };
        parsed = parsed.until(until_dt.with_timezone(&Tz::UTC));
    }
    parsed.build(start).ok().map(RRuleSet::limit)
}

fn single(
    ev: &VEvent,
    key: Option<DateTime<Tz>>,
    local: Tz,
    from_ms: i64,
    to_ms: i64,
) -> Option<Occurrence> {
    let start = ev.start.resolve(local)?;
    let (s, e) = Span::of(ev, start, local).at(start, local)?;
    (e > from_ms && s < to_ms).then(|| Occurrence {
        uid: ev.uid.clone(),
        key_ms: key.map(|k| k.timestamp_millis()).unwrap_or(s),
        start_ms: s,
        end_ms: e,
        all_day: ev.start.is_date(),
        title: ev.summary.clone().unwrap_or_default(),
        location: ev.location.clone(),
    })
}

/// Occurrences overlapping `[from_ms, to_ms)`: recurring series expanded (RRULE + RDATE −
/// EXDATE − overridden instances), `RECURRENCE-ID` overrides applied, cancelled ones
/// dropped. Not sorted.
pub fn expand(events: &[VEvent], from_ms: i64, to_ms: i64, local: Tz) -> Vec<Occurrence> {
    let mut overridden: HashMap<&str, HashSet<i64>> = HashMap::new();
    for ev in events {
        if let Some(rid) = ev.recurrence_id.and_then(|r| r.resolve(local)) {
            overridden
                .entry(ev.uid.as_str())
                .or_default()
                .insert(rid.timestamp_millis());
        }
    }

    let mut out = Vec::new();
    for ev in events {
        if let Some(rid) = ev.recurrence_id {
            if !ev.cancelled {
                out.extend(single(ev, rid.resolve(local), local, from_ms, to_ms));
            }
            continue;
        }
        if ev.cancelled {
            continue;
        }
        let recurring = ev.rrule.is_some() || !ev.rdates.is_empty();
        if !recurring {
            out.extend(single(ev, None, local, from_ms, to_ms));
            continue;
        }
        let Some(start) = ev.start.resolve(local) else {
            continue;
        };
        let zone = ev.start.zone(local);
        let span = Span::of(ev, start, local);
        let mut excluded: HashSet<i64> =
            overridden.get(ev.uid.as_str()).cloned().unwrap_or_default();
        let mut excluded_days: HashSet<NaiveDate> = HashSet::new();
        for ex in &ev.exdates {
            match (ex, ev.start.is_date()) {
                // A DATE exdate on a timed series removes that day's instance.
                (IcsTime::Date(d), false) => {
                    excluded_days.insert(*d);
                }
                _ => {
                    if let Some(t) = ex.resolve(local) {
                        excluded.insert(t.timestamp_millis());
                    }
                }
            }
        }
        let emit = |occ: DateTime<Tz>, out: &mut Vec<Occurrence>| {
            let key = occ.timestamp_millis();
            if excluded.contains(&key) || excluded_days.contains(&occ.date_naive()) {
                return;
            }
            if let Some((s, e)) = span.at(occ, local) {
                if e > from_ms && s < to_ms {
                    out.push(Occurrence {
                        uid: ev.uid.clone(),
                        key_ms: s,
                        start_ms: s,
                        end_ms: e,
                        all_day: ev.start.is_date(),
                        title: ev.summary.clone().unwrap_or_default(),
                        location: ev.location.clone(),
                    });
                }
            }
        };

        let mut seen: HashSet<i64> = HashSet::new();
        match ev
            .rrule
            .as_deref()
            .and_then(|r| build_rrule(r, start.with_timezone(&zone)))
        {
            Some(set) => {
                for occ in (&set).into_iter().take(MAX_SCANNED) {
                    if occ.timestamp_millis() >= to_ms {
                        break;
                    }
                    seen.insert(occ.timestamp_millis());
                    emit(occ, &mut out);
                }
            }
            // Unparseable rule (or RDATE-only): DTSTART is still the first instance.
            None => {
                seen.insert(start.timestamp_millis());
                emit(start, &mut out);
            }
        }
        for rd in &ev.rdates {
            if let Some(t) = rd.resolve(local) {
                if seen.insert(t.timestamp_millis()) {
                    emit(t.with_timezone(&zone), &mut out);
                }
            }
        }
    }
    out
}
