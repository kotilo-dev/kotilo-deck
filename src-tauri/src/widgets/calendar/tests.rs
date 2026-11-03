//! No network: parsing is pure, HTTP tests use a loopback server, the rest an offline client.
//! Fixed clock 2026-11-03T08:00Z; "local" zone Europe/Berlin (UTC+1 since 25 Oct).

use std::io::{Read, Write};
use std::net::TcpListener;

use super::ics::{
    self, parse_calendar, parse_duration, parse_line, unescape_text, unfold, IcsTime,
};
use super::*;

const GOOGLE: &str = include_str!("../fixtures/calendar/google.ics");
const OUTLOOK: &str = include_str!("../fixtures/calendar/outlook.ics");
const ICLOUD: &str = include_str!("../fixtures/calendar/icloud.ics");
const BERLIN: Tz = Tz::Tz(chrono_tz::Europe::Berlin);

fn ms(rfc3339: &str) -> i64 {
    chrono::DateTime::parse_from_rfc3339(rfc3339)
        .unwrap()
        .timestamp_millis()
}

fn now() -> i64 {
    ms("2026-11-03T08:00:00Z")
}

/// (title, start, end, allDay) of `upcoming` for one fixture.
fn upcoming_of(ics_text: &str, days: u32, limit: u32) -> CalendarUpcoming {
    let cal = Calendar::with_client(std::env::temp_dir(), Err("offline".into()));
    let shared = &cal.0;
    shared.set_feeds(vec![feed("work", true)]);
    inject(
        shared,
        "work",
        parse_calendar(ics_text).unwrap(),
        now() - 1_000,
    );
    shared.upcoming(now(), limit, days, BERLIN)
}

fn rows(up: &CalendarUpcoming) -> Vec<(String, i64, i64, bool)> {
    up.events
        .iter()
        .map(|e| (e.title.clone(), e.start, e.end, e.all_day))
        .collect()
}

fn feed(id: &str, enabled: bool) -> FeedConfig {
    FeedConfig {
        id: id.into(),
        url: format!("https://example.com/{id}.ics?token=SECRET-{id}"),
        enabled,
    }
}

fn inject(shared: &Shared, id: &str, events: Vec<VEvent>, fetched_at: i64) {
    let mut inner = shared.lock();
    let st = inner.states.get_mut(id).unwrap();
    st.events = Arc::new(events);
    st.fetched_at = Some(fetched_at);
    st.disk_pending = false;
}

fn row(title: &str, start: &str, end: &str, all_day: bool) -> (String, i64, i64, bool) {
    (title.into(), ms(start), ms(end), all_day)
}

// ── Lines, values ───────────────────────────────────────────────────────────

#[test]
fn unfolds_crlf_lf_tabs_and_bom() {
    let text = "\u{feff}BEGIN:VCALENDAR\r\nSUMMARY:Hello \r\n World\r\nX-A:1\n\tcontinued\n\nEND:VCALENDAR";
    assert_eq!(
        unfold(text),
        vec![
            "BEGIN:VCALENDAR",
            "SUMMARY:Hello World",
            "X-A:1continued",
            "END:VCALENDAR"
        ]
    );
}

#[test]
fn unescapes_text() {
    assert_eq!(unescape_text(r"a\, b\; c\\d"), r"a, b; c\d");
    assert_eq!(unescape_text(r"Room 4\nBuilding B\N"), "Room 4 Building B");
    assert_eq!(unescape_text("  spaced   out  "), "spaced out");
    assert_eq!(unescape_text(r"trailing\"), r"trailing\");
}

#[test]
fn parses_content_lines_with_quoted_params() {
    let l = parse_line(r#"dtstart;tzid="(UTC+01:00) Amsterdam; Berlin":20261104T120000"#).unwrap();
    assert_eq!(l.name, "DTSTART");
    assert_eq!(
        l.params,
        vec![("TZID".into(), "(UTC+01:00) Amsterdam; Berlin".into())]
    );
    assert_eq!(l.value, "20261104T120000");

    let l = parse_line("ATTENDEE;ROLE=REQ-PARTICIPANT;CN=Ana:mailto:ana@example.com").unwrap();
    assert_eq!(l.params.len(), 2);
    assert_eq!(l.value, "mailto:ana@example.com");

    assert!(parse_line("no delimiter at all").is_none());
    assert!(parse_line(":value-without-name").is_none());
    assert!(parse_line(r#"X;P="unterminated:value"#).is_none());
}

#[test]
fn parses_times() {
    use chrono::NaiveDate;
    let d = NaiveDate::from_ymd_opt(2026, 11, 4).unwrap();
    assert_eq!(
        ics::parse_time_value("20261104", None, None),
        Some(IcsTime::Date(d))
    );
    assert_eq!(
        ics::parse_time_value("20261104", Some("DATE"), Some("Europe/Berlin")),
        Some(IcsTime::Date(d))
    );
    let n = d.and_hms_opt(9, 30, 0).unwrap();
    assert_eq!(
        ics::parse_time_value("20261104T093000Z", None, Some("Europe/Berlin")),
        Some(IcsTime::Utc(n))
    );
    assert_eq!(
        ics::parse_time_value("20261104T093000", None, None),
        Some(IcsTime::Floating(n))
    );
    assert_eq!(
        ics::parse_time_value("20261104T0930", None, None),
        Some(IcsTime::Floating(n))
    );
    assert_eq!(
        ics::parse_time_value("20261104T093000", None, Some("Turkey Standard Time")),
        Some(IcsTime::Zoned(n, chrono_tz::Europe::Istanbul))
    );
    assert_eq!(
        ics::parse_time_value("20261104T093000", None, Some("Nowhere/Special")),
        Some(IcsTime::Floating(n))
    );
    assert_eq!(ics::parse_time_value("garbage", None, None), None);
}

#[test]
fn timezone_conversion_including_dst() {
    use chrono::NaiveDate;
    let at = |y, m, d, h| {
        NaiveDate::from_ymd_opt(y, m, d)
            .unwrap()
            .and_hms_opt(h, 0, 0)
            .unwrap()
    };
    let berlin = chrono_tz::Europe::Berlin;
    // Summer (CEST, +2) and winter (CET, +1).
    assert_eq!(
        IcsTime::Zoned(at(2026, 7, 1, 12), berlin)
            .resolve(BERLIN)
            .unwrap()
            .timestamp_millis(),
        ms("2026-07-01T10:00:00Z")
    );
    assert_eq!(
        IcsTime::Zoned(at(2026, 11, 3, 12), berlin)
            .resolve(BERLIN)
            .unwrap()
            .timestamp_millis(),
        ms("2026-11-03T11:00:00Z")
    );
    // Spring-forward gap (02:30 does not exist on 29 Mar 2026) → shifted past the gap.
    assert_eq!(
        IcsTime::Zoned(at(2026, 3, 29, 2), berlin)
            .resolve(BERLIN)
            .unwrap()
            .timestamp_millis(),
        ms("2026-03-29T01:00:00Z")
    );
    // Fall-back ambiguity (02:00 twice on 25 Oct 2026) → the earlier instant.
    assert_eq!(
        IcsTime::Zoned(at(2026, 10, 25, 2), berlin)
            .resolve(BERLIN)
            .unwrap()
            .timestamp_millis(),
        ms("2026-10-25T00:00:00Z")
    );
    // Floating and DATE use the local zone; UTC ignores it.
    assert_eq!(
        IcsTime::Floating(at(2026, 11, 3, 12))
            .resolve(BERLIN)
            .unwrap()
            .timestamp_millis(),
        ms("2026-11-03T11:00:00Z")
    );
    assert_eq!(
        IcsTime::Utc(at(2026, 11, 3, 12))
            .resolve(BERLIN)
            .unwrap()
            .timestamp_millis(),
        ms("2026-11-03T12:00:00Z")
    );
}

#[test]
fn parses_durations() {
    let s = |x: i64| Some(chrono::TimeDelta::try_seconds(x).unwrap());
    assert_eq!(parse_duration("PT45M"), s(45 * 60));
    assert_eq!(parse_duration("P1D"), s(86_400));
    assert_eq!(parse_duration("P1W"), s(7 * 86_400));
    assert_eq!(parse_duration("P1DT2H3M4S"), s(86_400 + 7_384));
    assert_eq!(parse_duration("-PT15M"), s(-900));
    assert_eq!(parse_duration("-P0DT0H10M0S"), s(-600));
    assert_eq!(parse_duration("P"), None);
    assert_eq!(parse_duration("PT1D"), None);
    assert_eq!(parse_duration("1H"), None);
}

#[test]
fn rejects_non_calendars_but_tolerates_junk() {
    assert!(parse_calendar("<!doctype html><html>Sign in</html>").is_err());
    assert!(parse_calendar("").is_err());
    let events = parse_calendar(
        "BEGIN:VCALENDAR\nthis line is junk\nBEGIN:VEVENT\nUID:a\nDTSTART:20261103T100000Z\n\
         BEGIN:VEVENT\nUID:b\nDTSTART:20261103T110000Z\nEND:VEVENT\n\
         BEGIN:VEVENT\nUID:no-start\nEND:VEVENT\nEND:VCALENDAR",
    )
    .unwrap();
    let uids: Vec<_> = events.iter().map(|e| e.uid.as_str()).collect();
    assert_eq!(uids, vec!["a", "b"]);
}

// ── Fixtures ────────────────────────────────────────────────────────────────

#[test]
fn google_fixture() {
    let up = upcoming_of(GOOGLE, 7, 100);
    let standup = |d: u32| {
        row(
            "Daily standup",
            &format!("2026-11-{d:02}T08:30:00Z"),
            &format!("2026-11-{d:02}T08:45:00Z"),
            false,
        )
    };
    let planning =
        "Planning, review ; retro - a deliberately long title that Google folds onto the next line";
    assert_eq!(
        rows(&up),
        vec![
            // Started before now, still running.
            row(
                "Focus block",
                "2026-11-03T07:00:00Z",
                "2026-11-03T09:00:00Z",
                false
            ),
            // DST: 09:30 Berlin was 07:30Z at DTSTART (20 Oct), 08:30Z now.
            standup(3),
            row(
                "Lunch with Ana",
                "2026-11-03T11:00:00Z",
                "2026-11-03T12:00:00Z",
                false
            ),
            row(
                planning,
                "2026-11-03T14:00:00Z",
                "2026-11-03T15:00:00Z",
                false
            ),
            // All-day: local midnight to local midnight, two days. 4 Nov standup: EXDATE.
            row(
                "Team offsite",
                "2026-11-03T23:00:00Z",
                "2026-11-05T23:00:00Z",
                true
            ),
            // RECURRENCE-ID override moves the 5 Nov instance.
            row(
                "Daily standup (moved)",
                "2026-11-05T09:00:00Z",
                "2026-11-05T09:15:00Z",
                false
            ),
            standup(6),
            standup(7),
            standup(8),
            standup(9),
        ]
    );
    let planning_ev = up.events.iter().find(|e| e.title == planning).unwrap();
    assert_eq!(planning_ev.location.as_deref(), Some("Room 4 Building B"));
    let moved = up
        .events
        .iter()
        .find(|e| e.title == "Daily standup (moved)")
        .unwrap();
    // Override keeps the id of the instance it replaces.
    assert_eq!(
        moved.id,
        format!("work:standup-1@google.com:{}", ms("2026-11-05T08:30:00Z"))
    );
    assert_eq!(
        up.events[1].id,
        format!("work:standup-1@google.com:{}", ms("2026-11-03T08:30:00Z"))
    );
    assert!(!up.stale);
    assert!(up.errors.is_empty());

    // VALARM properties do not leak into the event.
    let events = parse_calendar(GOOGLE).unwrap();
    assert_eq!(events[0].summary.as_deref(), Some("Daily standup"));
    // CRLF line endings parse identically.
    assert_eq!(
        parse_calendar(&GOOGLE.replace('\n', "\r\n")).unwrap(),
        events
    );
}

#[test]
fn outlook_fixture_with_windows_zones() {
    let up = upcoming_of(OUTLOOK, 30, 100);
    assert_eq!(
        rows(&up),
        vec![
            // Override of the 14:00 instance (13:00Z) moved to 15:00 W. Europe.
            row(
                "Weekly 1:1 (moved)",
                "2026-11-03T14:00:00Z",
                "2026-11-03T14:30:00Z",
                false
            ),
            // 09:00 "Pacific Standard Time" (PST, -8 after 1 Nov).
            row(
                "Call with SF",
                "2026-11-03T17:00:00Z",
                "2026-11-03T18:00:00Z",
                false
            ),
            // Unknown zone names fall back to local wall-clock time.
            row(
                "Custom zone review",
                "2026-11-04T07:00:00Z",
                "2026-11-04T08:00:00Z",
                false
            ),
            row(
                "Display-name zone",
                "2026-11-04T11:00:00Z",
                "2026-11-04T11:30:00Z",
                false
            ),
            row(
                "Public holiday",
                "2026-11-05T23:00:00Z",
                "2026-11-06T23:00:00Z",
                true
            ),
            // 10 Nov: EXDATE; 17 Nov: cancelled override.
            row(
                "Weekly 1:1",
                "2026-11-24T13:00:00Z",
                "2026-11-24T13:30:00Z",
                false
            ),
            row(
                "Weekly 1:1",
                "2026-12-01T13:00:00Z",
                "2026-12-01T13:30:00Z",
                false
            ),
        ]
    );
    assert_eq!(
        up.events[0].location.as_deref(),
        Some("Microsoft Teams Meeting")
    );
}

#[test]
fn icloud_fixture() {
    let up = upcoming_of(ICLOUD, 7, 100);
    assert_eq!(
        rows(&up),
        vec![
            // Floating 18:00 → local (Berlin) 17:00Z; second instance (10 Nov) is outside 7 days.
            row(
                "Yoga",
                "2026-11-03T17:00:00Z",
                "2026-11-03T18:00:00Z",
                false
            ),
            // America/New_York + DURATION.
            row(
                "Dentist",
                "2026-11-05T15:00:00Z",
                "2026-11-05T15:45:00Z",
                false
            ),
            // RDATE-only series.
            row(
                "Book club",
                "2026-11-06T18:00:00Z",
                "2026-11-06T19:00:00Z",
                false
            ),
            // Yearly all-day since 1985.
            row(
                "Mum's birthday",
                "2026-11-06T23:00:00Z",
                "2026-11-07T23:00:00Z",
                true
            ),
        ]
    );
    let dentist = &up.events[1];
    assert_eq!(dentist.location.as_deref(), Some("12 Main St, Springfield"));
    // COUNT=2 stops the weekly series.
    let long = upcoming_of(ICLOUD, 60, 100);
    assert_eq!(long.events.iter().filter(|e| e.title == "Yoga").count(), 2);
}

// ── Recurrence rules ────────────────────────────────────────────────────────

fn cal(body: &str) -> String {
    format!("BEGIN:VCALENDAR\nVERSION:2.0\n{body}\nEND:VCALENDAR\n")
}

#[test]
fn all_day_weekly_with_date_exdate_and_until() {
    let text = cal(
        "BEGIN:VEVENT\nUID:gym\nSUMMARY:Gym\nDTSTART;VALUE=DATE:20261102\nDTEND;VALUE=DATE:20261103\n\
         RRULE:FREQ=WEEKLY;BYDAY=MO,WE;UNTIL=20261111\nEXDATE;VALUE=DATE:20261104\nEND:VEVENT",
    );
    let up = upcoming_of(&text, 30, 100);
    assert_eq!(
        rows(&up),
        vec![
            row("Gym", "2026-11-08T23:00:00Z", "2026-11-09T23:00:00Z", true),
            // UNTIL as DATE is inclusive.
            row("Gym", "2026-11-10T23:00:00Z", "2026-11-11T23:00:00Z", true),
        ]
    );
}

#[test]
fn daily_utc_series_with_multi_value_exdate_and_cancelled_override() {
    let text = cal(
        "BEGIN:VEVENT\nUID:d\nSUMMARY:Sync\nDTSTART:20261101T090000Z\nDURATION:PT30M\n\
         RRULE:FREQ=DAILY;COUNT=6\nEXDATE:20261103T090000Z,20261105T090000Z\nEND:VEVENT\n\
         BEGIN:VEVENT\nUID:d\nRECURRENCE-ID:20261104T090000Z\nDTSTART:20261104T090000Z\n\
         DURATION:PT30M\nSTATUS:CANCELLED\nSUMMARY:Sync\nEND:VEVENT",
    );
    let up = upcoming_of(&text, 30, 100);
    // 1, 2 Nov ended; 3, 5 excluded; 4 cancelled; COUNT=6 ends at 6 Nov.
    assert_eq!(
        rows(&up),
        vec![row(
            "Sync",
            "2026-11-06T09:00:00Z",
            "2026-11-06T09:30:00Z",
            false
        )]
    );
}

#[test]
fn cancelled_series_and_bad_rules() {
    let text = cal(
        "BEGIN:VEVENT\nUID:x\nSUMMARY:Gone\nSTATUS:CANCELLED\nDTSTART:20261101T090000Z\n\
         RRULE:FREQ=DAILY\nEND:VEVENT\n\
         BEGIN:VEVENT\nUID:y\nSUMMARY:Broken rule\nDTSTART:20261104T090000Z\n\
         DTEND:20261104T100000Z\nRRULE:FREQ=FORTNIGHTLY\nEND:VEVENT",
    );
    // An unusable RRULE still yields DTSTART.
    assert_eq!(
        rows(&upcoming_of(&text, 7, 100)),
        vec![row(
            "Broken rule",
            "2026-11-04T09:00:00Z",
            "2026-11-04T10:00:00Z",
            false
        )]
    );
}

#[test]
fn old_daily_series_expands_within_window_only() {
    let text = cal("BEGIN:VEVENT\nUID:old\nSUMMARY:Pills\nDTSTART:20000101T200000Z\nRRULE:FREQ=DAILY\nDURATION:PT5M\nEND:VEVENT");
    let up = upcoming_of(&text, 2, 100);
    assert_eq!(
        rows(&up),
        vec![
            row(
                "Pills",
                "2026-11-03T20:00:00Z",
                "2026-11-03T20:05:00Z",
                false
            ),
            row(
                "Pills",
                "2026-11-04T20:00:00Z",
                "2026-11-04T20:05:00Z",
                false
            ),
        ]
    );
}

// ── Window, limit, sort, aggregation ────────────────────────────────────────

#[test]
fn window_limit_sort_and_feed_aggregation() {
    let cal_ = Calendar::with_client(std::env::temp_dir(), Err("offline".into()));
    let shared = &cal_.0;
    shared.set_feeds(vec![
        feed("a", true),
        feed("b", true),
        feed("off", false),
        feed("broken", true),
    ]);
    let a = parse_calendar(&cal(
        "BEGIN:VEVENT\nUID:t\nSUMMARY:Midnight call\nDTSTART:20261103T230000Z\nDTEND:20261103T233000Z\nEND:VEVENT\n\
         BEGIN:VEVENT\nUID:ended\nSUMMARY:Ended\nDTSTART:20261103T060000Z\nDTEND:20261103T080000Z\nEND:VEVENT\n\
         BEGIN:VEVENT\nUID:far\nSUMMARY:Too far\nDTSTART:20261110T080000Z\nDTEND:20261110T090000Z\nEND:VEVENT\n\
         BEGIN:VEVENT\nUID:edge\nSUMMARY:Last in window\nDTSTART:20261110T075959Z\nDTEND:20261110T090000Z\nEND:VEVENT",
    ))
    .unwrap();
    let b = parse_calendar(&cal(
        "BEGIN:VEVENT\nUID:ad\nSUMMARY:Holiday\nDTSTART;VALUE=DATE:20261104\nEND:VEVENT\n\
         BEGIN:VEVENT\nUID:early\nSUMMARY:Early\nDTSTART:20261103T090000Z\nDTEND:20261103T100000Z\nEND:VEVENT",
    ))
    .unwrap();
    let off = parse_calendar(&cal(
        "BEGIN:VEVENT\nUID:hidden\nSUMMARY:Disabled feed\nDTSTART:20261103T090000Z\nEND:VEVENT",
    ))
    .unwrap();
    inject(shared, "a", a, now() - 60_000);
    inject(shared, "b", b, now() - 5_000);
    inject(shared, "off", off, now() - 999_999);
    shared.lock().states.get_mut("broken").unwrap().error = Some(FeedError::Http(404));

    let up = shared.upcoming(now(), 20, 7, BERLIN);
    let titles: Vec<_> = up.events.iter().map(|e| e.title.as_str()).collect();
    // 23:00Z on 3 Nov is 00:00 on 4 Nov in Berlin: the all-day event of that day goes first.
    assert_eq!(
        titles,
        vec!["Early", "Holiday", "Midnight call", "Last in window"]
    );
    assert_eq!(up.events[0].feed_id, "b");
    assert_eq!(
        up.events[1].id,
        format!("b:ad:{}", ms("2026-11-03T23:00:00Z"))
    );
    // Oldest successful fetch among enabled feeds.
    assert_eq!(up.fetched_at, Some(now() - 60_000));
    assert!(!up.stale);
    assert_eq!(
        up.errors,
        vec![FeedErrorEntry {
            feed_id: "broken".into(),
            message: "HTTP 404".into()
        }]
    );

    let limited = shared.upcoming(now(), 2, 7, BERLIN);
    assert_eq!(limited.events.len(), 2);
    assert_eq!(limited.events[1].title, "Holiday");

    let json = serde_json::to_value(&up).unwrap();
    assert_eq!(json["fetchedAt"], now() - 60_000);
    assert_eq!(json["events"][1]["allDay"], true);
    assert_eq!(json["events"][1]["feedId"], "b");
    assert_eq!(json["events"][0]["location"], Value::Null);
    assert_eq!(json["errors"][0]["feedId"], "broken");
}

// ── Feeds, URLs, errors ─────────────────────────────────────────────────────

#[test]
fn parses_feeds_leniently() {
    let v = json!([
        {"id": "work", "name": "Work", "url": "https://a/x.ics", "color": "accent", "enabled": true},
        {"id": "Bad Id", "url": "https://a/y.ics"},
        {"id": "nourl"},
        {"id": "home", "url": "webcal://b/z.ics", "enabled": false},
        {"id": "work", "url": "https://dupe"},
        {"id": "implicit", "url": "https://c"},
        42,
        {"id": "../escape", "url": "https://d"}
    ]);
    let feeds = parse_feeds(Some(&v));
    let ids: Vec<_> = feeds.iter().map(|f| (f.id.as_str(), f.enabled)).collect();
    assert_eq!(
        ids,
        vec![("work", true), ("home", false), ("implicit", true)]
    );
    assert!(parse_feeds(Some(&json!({"not": "an array"}))).is_empty());
    assert!(parse_feeds(None).is_empty());
}

#[test]
fn normalizes_urls() {
    assert_eq!(
        normalize_url("webcal://p01-caldav.icloud.com/published/2/abc")
            .unwrap()
            .as_str(),
        "https://p01-caldav.icloud.com/published/2/abc"
    );
    assert_eq!(
        normalize_url("WEBCALS://x.example/a.ics").unwrap().scheme(),
        "https"
    );
    assert!(
        normalize_url("https://calendar.google.com/calendar/ical/x/private-abc/basic.ics").is_ok()
    );
    assert_eq!(
        normalize_url("http://x.example/a.ics"),
        Err(FeedError::UnsupportedScheme)
    );
    assert_eq!(
        normalize_url("file:///C:/secret.ics"),
        Err(FeedError::UnsupportedScheme)
    );
    assert_eq!(normalize_url("not a url"), Err(FeedError::InvalidUrl));
}

#[test]
fn error_messages_never_contain_urls() {
    let all = [
        FeedError::InvalidUrl,
        FeedError::UnsupportedScheme,
        FeedError::Unreachable,
        FeedError::Timeout,
        FeedError::BadRedirect,
        FeedError::Http(404),
        FeedError::TooLarge,
        FeedError::NotCalendar,
    ];
    for e in all {
        let m = e.to_string();
        assert!(!m.contains("example") && !m.contains("SECRET"), "{m}");
        assert!(!m.is_empty());
    }
    assert_eq!(FeedError::Http(404).to_string(), "HTTP 404");
    assert_eq!(FeedError::NotCalendar.to_string(), "Not a calendar file");
    assert_eq!(FeedError::Unreachable.to_string(), "Feed unreachable");
}

#[test]
fn backoff_schedule() {
    let mins: Vec<i64> = (1..=7).map(|n| backoff_ms(n) / 60_000).collect();
    assert_eq!(mins, vec![1, 2, 5, 10, 15, 15, 15]);
    assert_eq!(backoff_ms(0), 60_000);
}

// ── Cache ───────────────────────────────────────────────────────────────────

fn cache_file(id: &str, url: &str, fetched_at: i64, ics_text: &str) -> CacheFile {
    CacheFile {
        schema_version: CACHE_VERSION,
        feed_id: id.into(),
        url_hash: url_hash(url),
        fetched_at,
        etag: Some("\"v1\"".into()),
        last_modified: None,
        ics: ics_text.into(),
    }
}

#[test]
fn cache_round_trip_and_invalidation() {
    let dir = tempfile::tempdir().unwrap();
    let f = feed("work", true);
    let file = cache_file("work", &f.url, 1234, GOOGLE);
    write_cache(dir.path(), &file);

    let raw: Value =
        serde_json::from_slice(&fs::read(dir.path().join("work.json")).unwrap()).unwrap();
    assert_eq!(raw["schemaVersion"], 1);
    assert_eq!(raw["fetchedAt"], 1234);
    assert!(
        !raw.to_string().contains("SECRET"),
        "cache must not store the URL"
    );

    assert_eq!(
        read_cache(dir.path(), "work", &url_hash(&f.url)),
        Some(file.clone())
    );
    // webcal:// and https:// spellings of one URL share the cache.
    let webcal = f.url.replace("https://", "webcal://");
    assert!(read_cache(dir.path(), "work", &url_hash(&webcal)).is_some());
    // Another URL under the same id, or another schema version: miss.
    assert!(read_cache(dir.path(), "work", &url_hash("https://other/x.ics")).is_none());
    let mut v2 = raw.clone();
    v2["schemaVersion"] = json!(2);
    fs::write(dir.path().join("work.json"), v2.to_string()).unwrap();
    assert!(read_cache(dir.path(), "work", &url_hash(&f.url)).is_none());
    fs::write(dir.path().join("work.json"), "{corrupt").unwrap();
    assert!(read_cache(dir.path(), "work", &url_hash(&f.url)).is_none());
}

#[tokio::test]
async fn restart_offline_serves_cache_as_stale_with_safe_error() {
    let dir = tempfile::tempdir().unwrap();
    let f = feed("work", true);
    write_cache(
        dir.path(),
        &cache_file("work", &f.url, now() - 3_600_000, GOOGLE),
    );

    let cal_ = Calendar::with_client(dir.path().to_path_buf(), Err("offline".into()));
    let shared = &cal_.0;
    shared.set_feeds(vec![f, feed("fresh", true)]);
    assert!(
        shared.refresh(false).await,
        "cache load + failures are visible changes"
    );

    let up = shared.upcoming(now(), 20, 7, BERLIN);
    assert!(up.stale);
    assert_eq!(up.fetched_at, Some(now() - 3_600_000));
    assert!(up.events.iter().any(|e| e.title == "Lunch with Ana"));
    let errors: Vec<_> = up
        .errors
        .iter()
        .map(|e| (e.feed_id.as_str(), e.message.as_str()))
        .collect();
    assert_eq!(
        errors,
        vec![("work", "Feed unreachable"), ("fresh", "Feed unreachable")]
    );
    assert!(!serde_json::to_string(&up).unwrap().contains("SECRET"));

    // Backoff: not due again right away; a forced refresh still runs (and changes nothing visible).
    {
        let inner = shared.lock();
        let st = &inner.states["work"];
        assert_eq!(st.failures, 1);
        assert!(st.next_due > now_ms() + 50_000);
    }
    assert!(!shared.refresh(false).await);
    assert!(!shared.refresh(true).await);
    assert_eq!(shared.lock().states["work"].failures, 2);
}

#[tokio::test]
async fn unsupported_scheme_is_reported_per_feed() {
    let cal_ = Calendar::with_client(std::env::temp_dir(), Err("offline".into()));
    let shared = &cal_.0;
    shared.set_feeds(vec![FeedConfig {
        id: "plain".into(),
        url: "http://insecure.example/cal.ics?token=SECRET".into(),
        enabled: true,
    }]);
    shared.refresh(false).await;
    let up = shared.upcoming(now(), 20, 7, BERLIN);
    assert_eq!(
        up.errors[0].message,
        "Only https:// and webcal:// feeds are supported"
    );
    assert!(!up.stale);
}

#[test]
fn feed_changes_reset_state_and_drop_caches() {
    let dir = tempfile::tempdir().unwrap();
    let cal_ = Calendar::with_client(dir.path().to_path_buf(), Err("offline".into()));
    let shared = &cal_.0;
    let (a, b) = (feed("a", true), feed("b", true));
    write_cache(dir.path(), &cache_file("a", &a.url, 1, GOOGLE));
    write_cache(dir.path(), &cache_file("b", &b.url, 1, GOOGLE));
    assert!(shared.set_feeds(vec![a.clone(), b.clone()]));
    assert!(
        !shared.set_feeds(vec![a.clone(), b.clone()]),
        "same list is not a change"
    );

    // Disable then re-enable "a": due immediately again.
    shared.lock().states.get_mut("a").unwrap().next_due = i64::MAX;
    let a_off = FeedConfig {
        enabled: false,
        ..a.clone()
    };
    assert!(shared.set_feeds(vec![a_off, b.clone()]));
    assert!(shared.set_feeds(vec![a.clone(), b.clone()]));
    assert_eq!(shared.lock().states["a"].next_due, 0);

    // New URL for "a" and "b" removed: both caches go.
    let a2 = FeedConfig {
        url: "https://elsewhere/a.ics".into(),
        ..a
    };
    assert!(shared.set_feeds(vec![a2]));
    assert!(!dir.path().join("a.json").exists());
    assert!(!dir.path().join("b.json").exists());
    assert_eq!(shared.lock().states.len(), 1);
}

// ── HTTP (loopback only) ────────────────────────────────────────────────────

/// Serves `responses` in order, one connection each; returns the base URL and the raw
/// requests received.
fn serve(responses: Vec<String>) -> (String, std::thread::JoinHandle<Vec<String>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let handle = std::thread::spawn(move || {
        let mut requests = Vec::new();
        for resp in responses {
            let (mut sock, _) = listener.accept().unwrap();
            let mut buf = Vec::new();
            let mut chunk = [0u8; 1024];
            while !buf.windows(4).any(|w| w == b"\r\n\r\n") {
                let n = sock.read(&mut chunk).unwrap();
                if n == 0 {
                    break;
                }
                buf.extend_from_slice(&chunk[..n]);
            }
            requests.push(String::from_utf8_lossy(&buf).to_lowercase());
            let _ = sock.write_all(resp.as_bytes());
        }
        requests
    });
    (base, handle)
}

fn http(status: &str, headers: &str, body: &str) -> String {
    format!(
        "HTTP/1.1 {status}\r\nConnection: close\r\nContent-Length: {}\r\n{headers}\r\n{body}",
        body.len()
    )
}

fn test_client() -> Client {
    client_builder().no_proxy().build().unwrap()
}

#[tokio::test]
async fn fetch_sends_validators_and_handles_304() {
    let (base, server) = serve(vec![
        http(
            "200 OK",
            "ETag: \"abc\"\r\nLast-Modified: Tue, 03 Nov 2026 07:00:00 GMT\r\n",
            GOOGLE,
        ),
        http("304 Not Modified", "", ""),
    ]);
    let url = Url::parse(&format!("{base}/cal.ics")).unwrap();
    let client = test_client();
    let Fetched::Body { text, validators } = fetch(&client, url.clone(), &Validators::default())
        .await
        .unwrap()
    else {
        panic!("expected a body");
    };
    assert!(parse_calendar(&text).is_ok());
    assert_eq!(validators.etag.as_deref(), Some("\"abc\""));
    assert_eq!(
        validators.last_modified.as_deref(),
        Some("Tue, 03 Nov 2026 07:00:00 GMT")
    );

    let second = fetch(&client, url, &validators).await.unwrap();
    assert!(matches!(second, Fetched::NotModified));
    let requests = server.join().unwrap();
    assert!(!requests[0].contains("if-none-match"));
    assert!(requests[1].contains("if-none-match: \"abc\""));
    assert!(requests[1].contains("if-modified-since: tue, 03 nov 2026 07:00:00 gmt"));
}

#[tokio::test]
async fn fetch_errors_are_url_free() {
    let (base, server) = serve(vec![
        http("404 Not Found", "", "nope"),
        "HTTP/1.1 200 OK\r\nConnection: close\r\nContent-Length: 20000000\r\n\r\n".to_string(),
    ]);
    let client = test_client();
    let secret = |b: &str| Url::parse(&format!("{b}/cal.ics?token=SECRET")).unwrap();
    assert_eq!(
        fetch(&client, secret(&base), &Validators::default())
            .await
            .unwrap_err(),
        FeedError::Http(404)
    );
    assert_eq!(
        fetch(&client, secret(&base), &Validators::default())
            .await
            .unwrap_err(),
        FeedError::TooLarge
    );
    server.join().unwrap();

    // Closed port → "Feed unreachable", no URL anywhere.
    let closed = {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        format!("http://{}", l.local_addr().unwrap())
    };
    let err = fetch(&client, secret(&closed), &Validators::default())
        .await
        .unwrap_err();
    assert_eq!(err, FeedError::Unreachable);
    assert!(!err.to_string().contains("SECRET"));
}
