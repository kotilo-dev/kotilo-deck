//! Phase 5 crash logs and "Report an issue" (`src/lib/ipc/index.ts`, "crash log + report
//! issue").
//!
//! - A panic hook writes `~/.kotilo/deck/logs/crash-<UTC>.log` synchronously (version, OS,
//!   arch, thread, message, location, backtrace) with the home directory replaced by `~`,
//!   then runs the previous hook (stderr) and logs through `tracing`.
//! - `crash_last` returns the newest crash log from an earlier run newer than the
//!   "dismissed up to" marker `logs/.crash-seen` (unix ms; kept out of `settings.json`).
//!   A clean exit advances the marker too ([`mark_clean_exit`]), so panics that tokio
//!   caught inside a command or task (the app kept running) are not reported as a crash on
//!   the next start; only runs that did not exit cleanly leave a crash to show.
//! - `report_issue_url` prefills a GitHub issue with version/OS/arch/WebView and an optional
//!   crash summary only: never logs, paths, settings or user names.
//!
//! None of these commands are on the LAN allowlist (`crate::lan::allowlist`).

use std::fmt::Write as _;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicI64, AtomicU32, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::Serialize;

use crate::paths;
use crate::platform::{self, PlatformError};

pub const ISSUES_URL: &str = "https://github.com/kotilo-dev/kotilo-deck/issues/new";
const CRASH_PREFIX: &str = "crash-";
const CRASH_SUFFIX: &str = ".log";
/// UTC, file-name safe (no `:`), e.g. `2026-11-05T10-12-03Z`.
const STAMP_FORMAT: &str = "%Y-%m-%dT%H-%M-%SZ";
const SEEN_FILE: &str = ".crash-seen";
const SUMMARY_LINE: &str = "summary: ";
const SUMMARY_MAX: usize = 200;
/// A panicking loop in a background task must not fill the disk.
const MAX_CRASH_FILES_PER_RUN: u32 = 5;
/// Older crash logs are pruned at startup.
const KEEP_CRASH_FILES: usize = 10;
const FRONTEND_MESSAGE_MAX: usize = 500;
const FRONTEND_STACK_MAX: usize = 4000;
const FRONTEND_PER_MINUTE: u32 = 20;
/// Kept well below the 4000-char contract limit: `ShellExecute` and some browsers balk at
/// longer URLs.
const MAX_URL_LEN: usize = 2000;
const TITLE_SUMMARY_MAX: usize = 100;

/// Unix ms when this process installed the hook; crash files from this run are not "last".
static STARTED_AT: AtomicI64 = AtomicI64::new(i64::MAX);
static WRITTEN: AtomicU32 = AtomicU32::new(0);

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CrashReport {
    pub file: String,
    pub at: i64,
    pub summary: String,
}

fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

// ── Redaction / text helpers ────────────────────────────────────────────────

fn home() -> Option<String> {
    dirs::home_dir().map(|h| h.to_string_lossy().into_owned())
}

/// Replaces every occurrence of `needle` (ASCII case-insensitive on Windows, where paths
/// are) with `with`. ASCII lowercasing keeps byte offsets, so indices map back to `text`.
fn replace_all(text: &str, needle: &str, with: &str) -> String {
    if needle.is_empty() {
        return text.to_string();
    }
    let (hay, pat) = if cfg!(windows) {
        (text.to_ascii_lowercase(), needle.to_ascii_lowercase())
    } else {
        (text.to_string(), needle.to_string())
    };
    let mut out = String::with_capacity(text.len());
    let mut last = 0;
    for (i, _) in hay.match_indices(&pat) {
        out.push_str(&text[last..i]);
        out.push_str(with);
        last = i + pat.len();
    }
    out.push_str(&text[last..]);
    out
}

/// `text` with `home` (and its `/` and `\` spellings) replaced by `~`.
pub fn redact_with(text: &str, home: Option<&str>) -> String {
    let Some(home) = home.map(|h| h.trim_end_matches(['/', '\\'])) else {
        return text.to_string();
    };
    // Too short to be a real home directory (`/`, `C:`): replacing it would mangle text.
    if home.len() < 3 {
        return text.to_string();
    }
    let mut out = replace_all(text, home, "~");
    for variant in [home.replace('\\', "/"), home.replace('/', "\\")] {
        if variant != home {
            out = replace_all(&out, &variant, "~");
        }
    }
    out
}

pub fn redact(text: &str) -> String {
    redact_with(text, home().as_deref())
}

/// First chars up to `max`, by char (never splits UTF-8).
fn truncate_chars(s: &str, max: usize) -> &str {
    match s.char_indices().nth(max) {
        Some((i, _)) => &s[..i],
        None => s,
    }
}

/// First non-empty line, control characters dropped, ≤ [`SUMMARY_MAX`] chars, redacted.
pub fn summarize_with(message: &str, home: Option<&str>) -> String {
    let redacted = redact_with(message, home);
    let line = redacted
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("");
    let clean: String = line.chars().filter(|c| !c.is_control()).collect();
    truncate_chars(&clean, SUMMARY_MAX).to_string()
}

fn summarize(message: &str) -> String {
    summarize_with(message, home().as_deref())
}

// ── System description (no PII) ─────────────────────────────────────────────

fn os_description() -> String {
    let name = sysinfo::System::name().unwrap_or_else(|| std::env::consts::OS.to_string());
    match sysinfo::System::os_version() {
        Some(v) => format!("{name} {v}"),
        None => name,
    }
}

fn webview_version() -> Option<String> {
    tauri::webview_version().ok()
}

// ── Panic hook ──────────────────────────────────────────────────────────────

fn crash_file_name(at: chrono::DateTime<chrono::Utc>) -> String {
    format!("{CRASH_PREFIX}{}{CRASH_SUFFIX}", at.format(STAMP_FORMAT))
}

fn payload_message(payload: &(dyn std::any::Any + Send)) -> String {
    if let Some(s) = payload.downcast_ref::<&str>() {
        (*s).to_string()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "Box<dyn Any>".to_string()
    }
}

struct PanicDetails<'a> {
    thread: &'a str,
    message: &'a str,
    location: Option<String>,
    backtrace: &'a str,
}

/// The crash file body, already redacted.
fn crash_text(at: chrono::DateTime<chrono::Utc>, d: &PanicDetails, home: Option<&str>) -> String {
    let mut s = String::new();
    let _ = writeln!(s, "Kotilo Deck crash report");
    let _ = writeln!(s, "{SUMMARY_LINE}{}", summarize_with(d.message, home));
    let _ = writeln!(
        s,
        "time: {}",
        at.to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
    );
    let _ = writeln!(s, "version: {}", env!("CARGO_PKG_VERSION"));
    let _ = writeln!(s, "os: {}", os_description());
    let _ = writeln!(s, "arch: {}", std::env::consts::ARCH);
    let _ = writeln!(s, "thread: {}", d.thread);
    let _ = writeln!(
        s,
        "location: {}",
        d.location.as_deref().unwrap_or("<unknown>")
    );
    let _ = writeln!(
        s,
        "\nmessage:\n{}\n\nbacktrace:\n{}",
        d.message, d.backtrace
    );
    redact_with(&s, home)
}

/// Appends (same-second panics share a file). Errors are swallowed: the hook must not panic.
fn write_crash_file(dir: &Path, name: &str, text: &str) -> Option<PathBuf> {
    fs::create_dir_all(dir).ok()?;
    let path = dir.join(name);
    let mut f = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .ok()?;
    f.write_all(text.as_bytes()).ok()?;
    f.write_all(b"\n").ok()?;
    f.sync_all().ok()?;
    Some(path)
}

/// Installs the panic hook (after `logging::init`) and prunes old crash logs.
pub fn install() {
    STARTED_AT.store(now_ms(), Ordering::Release);
    prune(&paths::logs_dir(), KEEP_CRASH_FILES);
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let message = payload_message(info.payload());
        let location = info
            .location()
            .map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column()));
        let thread = std::thread::current();
        let thread = thread.name().unwrap_or("<unnamed>");
        let mut file = None;
        if WRITTEN.fetch_add(1, Ordering::AcqRel) < MAX_CRASH_FILES_PER_RUN {
            let backtrace = std::backtrace::Backtrace::force_capture().to_string();
            let at = chrono::Utc::now();
            let home = home();
            let text = crash_text(
                at,
                &PanicDetails {
                    thread,
                    message: &message,
                    location: location.clone(),
                    backtrace: &backtrace,
                },
                home.as_deref(),
            );
            file = write_crash_file(&paths::logs_dir(), &crash_file_name(at), &text);
        }
        tracing::error!(
            target: "panic",
            thread,
            location = %location.as_deref().map(redact).unwrap_or_default(),
            crash_file = %file.as_deref().map(paths::file_label).unwrap_or_default(),
            "panic: {}",
            summarize(&message)
        );
        previous(info);
    }));
}

/// Deletes all but the newest `keep` crash logs.
fn prune(dir: &Path, keep: usize) {
    let mut files = crash_files(dir);
    files.sort_by_key(|f| std::cmp::Reverse(f.1));
    for (name, _) in files.into_iter().skip(keep) {
        let _ = fs::remove_file(dir.join(name));
    }
}

// ── crash_last / crash_dismiss ──────────────────────────────────────────────

/// Unix ms encoded in a `crash-<UTC>.log` file name.
fn parse_crash_name(name: &str) -> Option<i64> {
    let stamp = name
        .strip_prefix(CRASH_PREFIX)?
        .strip_suffix(CRASH_SUFFIX)?;
    chrono::NaiveDateTime::parse_from_str(stamp, STAMP_FORMAT)
        .ok()
        .map(|t| t.and_utc().timestamp_millis())
}

fn crash_files(dir: &Path) -> Vec<(String, i64)> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().into_string().ok()?;
            let at = parse_crash_name(&name)?;
            e.file_type().ok()?.is_file().then_some((name, at))
        })
        .collect()
}

fn read_seen(dir: &Path) -> i64 {
    fs::read_to_string(dir.join(SEEN_FILE))
        .ok()
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(i64::MIN)
}

/// The `summary:` header line (redacted again, in case the file was written elsewhere).
fn read_summary(path: &Path) -> String {
    let mut head = Vec::new();
    if let Ok(f) = fs::File::open(path) {
        let _ = f.take(8 * 1024).read_to_end(&mut head);
    }
    let head = String::from_utf8_lossy(&head);
    head.lines()
        .find_map(|l| l.strip_prefix(SUMMARY_LINE))
        .map(summarize)
        .unwrap_or_default()
}

/// Newest crash log in `dir` with `seen < at < before`.
///
/// File names only carry whole seconds, so `before` (the run start, in ms) is truncated to
/// its second first: a panic in the first second of this run (stamped `…:SS.000`) must not
/// count as an earlier run's crash.
pub fn last_in(dir: &Path, before: i64) -> Option<CrashReport> {
    let seen = read_seen(dir);
    let before = before.div_euclid(1000).saturating_mul(1000);
    let (file, at) = crash_files(dir)
        .into_iter()
        .filter(|(_, at)| *at > seen && *at < before)
        .max_by(|a, b| a.1.cmp(&b.1).then_with(|| a.0.cmp(&b.0)))?;
    let summary = read_summary(&dir.join(&file));
    Some(CrashReport { file, at, summary })
}

/// Clean exit: advances the marker to `now` (never backwards), so this run's crash files
/// (panics caught while the app kept running) are not shown on the next start.
pub fn clean_exit_in(dir: &Path, now: i64) -> Result<(), String> {
    if read_seen(dir) >= now {
        return Ok(());
    }
    dismiss_in(dir, now)
}

/// Called from `RunEvent::Exit` (tray Quit and every other orderly exit).
pub fn mark_clean_exit() {
    if let Err(e) = clean_exit_in(&paths::logs_dir(), now_ms()) {
        tracing::warn!("cannot record the clean exit: {e}");
    }
}

pub fn dismiss_in(dir: &Path, now: i64) -> Result<(), String> {
    fs::create_dir_all(dir).map_err(|e| format!("cannot create the logs folder: {e}"))?;
    fs::write(dir.join(SEEN_FILE), now.to_string())
        .map_err(|e| format!("cannot save the crash marker: {e}"))
}

#[tauri::command]
pub async fn crash_last() -> Result<Option<CrashReport>, String> {
    let before = STARTED_AT.load(Ordering::Acquire);
    tauri::async_runtime::spawn_blocking(move || last_in(&paths::logs_dir(), before))
        .await
        .map_err(|e| format!("background task failed: {e}"))
}

#[tauri::command]
pub async fn crash_dismiss() -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(|| dismiss_in(&paths::logs_dir(), now_ms()))
        .await
        .map_err(|e| format!("background task failed: {e}"))?
}

// ── Frontend errors ─────────────────────────────────────────────────────────

/// Fixed one-minute window. Dropped calls are counted and reported once when the next
/// window opens.
#[derive(Debug, Default)]
pub struct RateLimiter {
    window: Option<Instant>,
    count: u32,
    suppressed: u32,
}

#[derive(Debug, PartialEq, Eq)]
pub struct Admit {
    pub allowed: bool,
    /// Calls dropped in the previous window, to log once.
    pub suppressed: u32,
}

impl RateLimiter {
    pub const fn new() -> Self {
        Self {
            window: None,
            count: 0,
            suppressed: 0,
        }
    }

    pub fn admit(&mut self, now: Instant, limit: u32, period: Duration) -> Admit {
        let mut suppressed = 0;
        let expired = match self.window {
            Some(start) => now.saturating_duration_since(start) >= period,
            None => true,
        };
        if expired {
            suppressed = std::mem::take(&mut self.suppressed);
            self.window = Some(now);
            self.count = 0;
        }
        if self.count < limit {
            self.count += 1;
            Admit {
                allowed: true,
                suppressed,
            }
        } else {
            self.suppressed += 1;
            Admit {
                allowed: false,
                suppressed,
            }
        }
    }
}

static FRONTEND_LIMITER: Mutex<RateLimiter> = Mutex::new(RateLimiter::new());

#[tauri::command]
pub fn log_frontend_error(message: String, stack: Option<String>) {
    let admit = FRONTEND_LIMITER
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .admit(Instant::now(), FRONTEND_PER_MINUTE, Duration::from_secs(60));
    if admit.suppressed > 0 {
        tracing::error!(
            target: "frontend",
            "suppressed {} frontend errors (rate limit {FRONTEND_PER_MINUTE}/min)",
            admit.suppressed
        );
    }
    if !admit.allowed {
        return;
    }
    let message = redact(truncate_chars(&message, FRONTEND_MESSAGE_MAX));
    let stack = stack
        .as_deref()
        .map(|s| redact(truncate_chars(s, FRONTEND_STACK_MAX)))
        .unwrap_or_default();
    tracing::error!(target: "frontend", stack = %stack, "{message}");
}

// ── Report an issue ─────────────────────────────────────────────────────────

/// RFC 3986 percent-encoding (everything but unreserved characters).
pub fn percent_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 3);
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"-._~".contains(&b) {
            out.push(b as char);
        } else {
            let _ = write!(out, "%{b:02X}");
        }
    }
    out
}

pub struct IssueEnv {
    pub version: String,
    pub os: String,
    pub arch: String,
    pub webview: Option<String>,
}

impl IssueEnv {
    fn current() -> Self {
        Self {
            version: env!("CARGO_PKG_VERSION").to_string(),
            os: os_description(),
            arch: std::env::consts::ARCH.to_string(),
            webview: webview_version(),
        }
    }
}

fn issue_url_for(env: &IssueEnv, summary: Option<&str>) -> String {
    let mut body = String::new();
    let _ = writeln!(body, "**Version:** {}", env.version);
    let _ = writeln!(body, "**OS:** {}", env.os);
    let _ = writeln!(body, "**Arch:** {}", env.arch);
    if let Some(w) = &env.webview {
        let _ = writeln!(body, "**WebView:** {w}");
    }
    if let Some(s) = summary {
        let _ = writeln!(body, "\n**Crash:** {s}");
    }
    let _ = writeln!(body, "\n**What happened?**\n\n\n**Steps to reproduce**\n\n");
    let _ = write!(
        body,
        "Please attach the log from Settings \u{25B8} About \u{25B8} Open logs folder."
    );
    let mut url = format!("{ISSUES_URL}?template=bug_report.md");
    if let Some(s) = summary {
        let title = format!("Crash: {}", truncate_chars(s, TITLE_SUMMARY_MAX));
        let _ = write!(url, "&title={}", percent_encode(&title));
    }
    let _ = write!(url, "&body={}", percent_encode(&body));
    url
}

/// The prefilled issue URL. `crash` is sanitised to a one-line summary and shortened until
/// the URL fits in [`MAX_URL_LEN`].
pub fn issue_url(env: &IssueEnv, crash: Option<&str>, home: Option<&str>) -> String {
    let summary = crash
        .map(|c| summarize_with(c, home))
        .filter(|s| !s.is_empty());
    let Some(mut summary) = summary else {
        return issue_url_for(env, None);
    };
    loop {
        let url = issue_url_for(env, Some(&summary));
        if url.len() <= MAX_URL_LEN || summary.is_empty() {
            return url;
        }
        let keep = summary.chars().count().saturating_sub(10);
        summary = truncate_chars(&summary, keep).to_string();
    }
}

fn report_url(crash: Option<String>) -> String {
    issue_url(&IssueEnv::current(), crash.as_deref(), home().as_deref())
}

#[tauri::command]
pub async fn report_issue_url(crash: Option<String>) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || report_url(crash))
        .await
        .map_err(|e| format!("background task failed: {e}"))
}

#[tauri::command]
pub async fn open_report_issue(crash: Option<String>) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let url = report_url(crash);
        if !url.starts_with(&format!("{ISSUES_URL}?")) {
            return Err("refusing to open a non-GitHub-issues URL".to_string());
        }
        platform::current()
            .open_url_scheme(&url)
            .map_err(|e| match e {
                PlatformError::Unsupported(_) => {
                    format!("opening links is not supported on this OS; visit {ISSUES_URL}")
                }
                PlatformError::Os(m) => m,
            })
    })
    .await
    .map_err(|e| format!("background task failed: {e}"))?
}

/// Opens `~/.kotilo/deck/logs/` (created if missing) in the OS file manager.
#[tauri::command]
pub async fn open_logs_folder() -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(|| {
        let dir = paths::logs_dir();
        fs::create_dir_all(&dir).map_err(|e| format!("cannot create the logs folder: {e}"))?;
        platform::current().open_path(&dir).map_err(|e| match e {
            PlatformError::Unsupported(_) => {
                "opening folders is not supported on this OS; open ~/.kotilo/deck/logs manually"
                    .to_string()
            }
            PlatformError::Os(m) => m,
        })
    })
    .await
    .map_err(|e| format!("background task failed: {e}"))?
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    const WIN_HOME: &str = r"C:\Users\alice";

    #[test]
    fn redaction_replaces_home_in_all_spellings() {
        let text = r"at C:\Users\alice\.cargo\x.rs and C:/Users/alice/src and C:\Users\alicex";
        let out = redact_with(text, Some(WIN_HOME));
        assert!(!out.contains("alice\\") && !out.contains("alice/"), "{out}");
        assert!(out.starts_with(r"at ~\.cargo\x.rs and ~/src"), "{out}");
        if cfg!(windows) {
            assert_eq!(
                redact_with(r"c:\users\ALICE\a", Some(WIN_HOME)),
                r"~\a",
                "case-insensitive on Windows"
            );
        }
        assert_eq!(
            redact_with("/home/bob/x and /home/bob", Some("/home/bob/")),
            "~/x and ~"
        );
        assert_eq!(redact_with("a / b", Some("/")), "a / b");
        assert_eq!(redact_with("ünï /home/bob", Some("/home/bob")), "ünï ~");
        assert_eq!(redact_with("x", None), "x");
    }

    #[test]
    fn summary_is_first_line_truncated_and_redacted() {
        let long = "é".repeat(300);
        let s = summarize_with(&format!("\n  {long}\nsecond"), None);
        assert_eq!(s.chars().count(), 200);
        assert!(s.chars().all(|c| c == 'é'));
        assert_eq!(
            summarize_with("open /home/bob/x failed\tnow\nline2", Some("/home/bob")),
            "open ~/x failednow"
        );
        assert_eq!(summarize_with("", None), "");
    }

    #[test]
    fn crash_text_has_fields_and_no_home() {
        let at = chrono::Utc
            .with_ymd_and_hms(2026, 11, 5, 10, 12, 3)
            .unwrap();
        assert_eq!(crash_file_name(at), "crash-2026-11-05T10-12-03Z.log");
        assert_eq!(
            parse_crash_name("crash-2026-11-05T10-12-03Z.log"),
            Some(at.timestamp_millis())
        );
        let text = crash_text(
            at,
            &PanicDetails {
                thread: "main",
                message: "boom at /home/bob/f.txt\nmore",
                location: Some("src/x.rs:1:2".into()),
                backtrace: "0: /home/bob/.cargo/registry/foo",
            },
            Some("/home/bob"),
        );
        assert!(!text.contains("/home/bob"), "{text}");
        for needle in [
            "summary: boom at ~/f.txt\n",
            "version: ",
            "os: ",
            "arch: ",
            "thread: main",
            "location: src/x.rs:1:2",
            "backtrace:\n0: ~/.cargo",
        ] {
            assert!(text.contains(needle), "{needle} in {text}");
        }
    }

    #[test]
    fn crash_last_picks_newest_undismissed_from_earlier_runs() {
        let dir = tempfile::tempdir().unwrap();
        let d = dir.path();
        assert_eq!(last_in(d, i64::MAX), None);
        let t = |s: u32| chrono::Utc.with_ymd_and_hms(2026, 11, 5, 10, 0, s).unwrap();
        for (s, msg) in [(1, "old"), (3, "newest"), (2, "middle")] {
            let text = format!("Kotilo Deck crash report\nsummary: {msg}\n");
            write_crash_file(d, &crash_file_name(t(s)), &text).unwrap();
        }
        // Noise: not crash logs.
        fs::write(d.join("deck.log.2026-11-05"), "x").unwrap();
        fs::write(d.join("crash-garbage.log"), "summary: no").unwrap();
        fs::create_dir(d.join("crash-2026-11-05T10-00-09Z.log")).unwrap();

        let last = last_in(d, i64::MAX).unwrap();
        assert_eq!(last.file, "crash-2026-11-05T10-00-03Z.log");
        assert_eq!(last.at, t(3).timestamp_millis());
        assert_eq!(last.summary, "newest");
        // Crashes from this run (at >= start) are excluded.
        assert_eq!(
            last_in(d, t(3).timestamp_millis()).unwrap().summary,
            "middle"
        );

        // Dismissing hides everything up to now, a later crash shows again.
        dismiss_in(d, t(3).timestamp_millis()).unwrap();
        assert_eq!(last_in(d, i64::MAX), None);
        write_crash_file(d, &crash_file_name(t(5)), "summary: again\n").unwrap();
        assert_eq!(last_in(d, i64::MAX).unwrap().summary, "again");
        // A file without a summary line still reports.
        write_crash_file(d, &crash_file_name(t(6)), "partial").unwrap();
        assert_eq!(last_in(d, i64::MAX).unwrap().summary, "");
        // Corrupt marker = nothing dismissed.
        fs::write(d.join(SEEN_FILE), "nope").unwrap();
        assert_eq!(last_in(d, t(2).timestamp_millis()).unwrap().summary, "old");

        prune(d, 2);
        let mut left: Vec<_> = crash_files(d).into_iter().map(|f| f.1).collect();
        left.sort();
        assert_eq!(left, vec![t(5).timestamp_millis(), t(6).timestamp_millis()]);
    }

    #[test]
    fn panic_in_the_first_second_of_a_run_is_not_last() {
        let dir = tempfile::tempdir().unwrap();
        let d = dir.path();
        let t = |s: u32| chrono::Utc.with_ymd_and_hms(2026, 11, 5, 10, 0, s).unwrap();
        // Run started at 10:00:05.700; a panic at 10:00:05.900 is stamped 10:00:05.
        let started = t(5).timestamp_millis() + 700;
        write_crash_file(
            d,
            &crash_file_name(t(5)),
            "summary: this run
",
        )
        .unwrap();
        assert_eq!(last_in(d, started), None);
        // An earlier second is still an earlier run.
        write_crash_file(
            d,
            &crash_file_name(t(4)),
            "summary: last run
",
        )
        .unwrap();
        assert_eq!(last_in(d, started).unwrap().summary, "last run");
    }

    #[test]
    fn clean_exit_hides_this_runs_crashes() {
        let dir = tempfile::tempdir().unwrap();
        let d = dir.path();
        let t = |s: u32| chrono::Utc.with_ymd_and_hms(2026, 11, 5, 10, 0, s).unwrap();
        // A panic caught by tokio at :02; the app kept running and quit cleanly at :09.
        write_crash_file(
            d,
            &crash_file_name(t(2)),
            "summary: caught
",
        )
        .unwrap();
        clean_exit_in(d, t(9).timestamp_millis()).unwrap();
        assert_eq!(last_in(d, i64::MAX), None);
        // The next run crashes hard (no clean exit): shown on the run after.
        write_crash_file(
            d,
            &crash_file_name(t(20)),
            "summary: hard
",
        )
        .unwrap();
        assert_eq!(last_in(d, i64::MAX).unwrap().summary, "hard");
        // The marker never moves backwards.
        dismiss_in(d, t(30).timestamp_millis()).unwrap();
        clean_exit_in(d, t(25).timestamp_millis()).unwrap();
        assert_eq!(read_seen(d), t(30).timestamp_millis());
    }

    #[test]
    fn rate_limiter_drops_extras_and_reports_once() {
        let mut l = RateLimiter::new();
        let t0 = Instant::now();
        let minute = Duration::from_secs(60);
        for _ in 0..20 {
            assert!(l.admit(t0, 20, minute).allowed);
        }
        for _ in 0..5 {
            assert_eq!(
                l.admit(t0 + Duration::from_secs(30), 20, minute),
                Admit {
                    allowed: false,
                    suppressed: 0
                }
            );
        }
        assert_eq!(
            l.admit(t0 + minute, 20, minute),
            Admit {
                allowed: true,
                suppressed: 5
            }
        );
        assert_eq!(l.admit(t0 + minute, 20, minute).suppressed, 0);
    }

    #[test]
    fn truncation_is_char_based() {
        assert_eq!(truncate_chars("héllo", 2), "hé");
        assert_eq!(truncate_chars("hi", 10), "hi");
        assert_eq!(truncate_chars(&"ş".repeat(600), 500).chars().count(), 500);
    }

    fn env() -> IssueEnv {
        IssueEnv {
            version: "1.2.3".into(),
            os: "Windows 11 (26300)".into(),
            arch: "x86_64".into(),
            webview: Some("131.0.2903.70".into()),
        }
    }

    fn decode(s: &str) -> String {
        let b = s.as_bytes();
        let mut out = Vec::new();
        let mut i = 0;
        while i < b.len() {
            if b[i] == b'%' {
                out.push(u8::from_str_radix(&s[i + 1..i + 3], 16).unwrap());
                i += 3;
            } else {
                out.push(b[i]);
                i += 1;
            }
        }
        String::from_utf8(out).unwrap()
    }

    fn param<'a>(url: &'a str, key: &str) -> Option<&'a str> {
        url.split_once('?')?
            .1
            .split('&')
            .find_map(|kv| kv.strip_prefix(&format!("{key}=")))
    }

    #[test]
    fn percent_encoding_is_rfc3986() {
        assert_eq!(percent_encode("a-Z_0.~"), "a-Z_0.~");
        assert_eq!(percent_encode("a b&c=d/?#+"), "a%20b%26c%3Dd%2F%3F%23%2B");
        assert_eq!(percent_encode("▸"), "%E2%96%B8");
        assert_eq!(decode(&percent_encode("x ▸ y\n")), "x ▸ y\n");
    }

    #[test]
    fn issue_url_has_env_and_no_paths() {
        let url = issue_url(&env(), None, Some("/home/bob"));
        assert!(url.starts_with(&format!("{ISSUES_URL}?template=bug_report.md&")));
        assert_eq!(param(&url, "title"), None);
        let body = decode(param(&url, "body").unwrap());
        for needle in [
            "1.2.3",
            "Windows 11 (26300)",
            "x86_64",
            "131.0.2903.70",
            "Settings \u{25B8} About \u{25B8} Open logs folder",
        ] {
            assert!(body.contains(needle), "{needle} in {body}");
        }
        assert!(!body.contains("Crash"));

        let url = issue_url(
            &env(),
            Some("thread panicked at /home/bob/src/x.rs\nsecret second line"),
            Some("/home/bob"),
        );
        let title = decode(param(&url, "title").unwrap());
        assert_eq!(title, "Crash: thread panicked at ~/src/x.rs");
        let body = decode(param(&url, "body").unwrap());
        assert!(body.contains("**Crash:** thread panicked at ~/src/x.rs"));
        assert!(!url.contains("bob") && !body.contains("secret"));
        // No raw reserved characters outside the separators.
        assert!(!url.contains(' ') && !url.contains('\n') && url.matches('&').count() == 2);
    }

    #[test]
    fn issue_url_stays_short() {
        let mut big = env();
        big.os = "Ö".repeat(50);
        for crash in ["ğ".repeat(5000), "a".repeat(5000), "€😀".repeat(400)] {
            let url = issue_url(&big, Some(&crash), None);
            assert!(url.len() <= MAX_URL_LEN, "{}", url.len());
            assert!(url.len() < 4000);
            // Still carries a (shortened) summary.
            assert!(param(&url, "title").is_some());
            decode(param(&url, "body").unwrap());
        }
        // Empty / whitespace-only crash = no crash section.
        assert_eq!(
            issue_url(&env(), Some("  \n "), None),
            issue_url(&env(), None, None)
        );
    }

    #[test]
    fn crash_commands_are_not_lan_allowed() {
        for cmd in [
            "crash_last",
            "crash_dismiss",
            "log_frontend_error",
            "report_issue_url",
            "open_report_issue",
            "open_logs_folder",
        ] {
            assert!(!crate::lan::allowlist::allowed(cmd), "{cmd}");
        }
    }
}
