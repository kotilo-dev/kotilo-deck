//! Calendar widget backend: ICS feeds (SPEC §4.4 Calendar).
//!
//! Feeds come from the `calendarFeeds` key of the settings store (read raw and leniently,
//! re-read on `settings://changed`). Every enabled feed is fetched concurrently on start,
//! then every 15 min, with per-feed backoff (1, 2, 5, 10, 15 min) after failures. Bodies
//! are cached per feed under `~/.kotilo/deck/cache/calendar/<feedId>.json` (raw ICS +
//! validators) so a restart or an offline deck still shows events, flagged `stale`.
//!
//! Feed URLs often embed secret tokens: they are never logged, never put into error
//! messages and never written to the cache (only an FNV hash, to notice URL changes).
//! Logs identify feeds by id.

mod ics;
mod zones;

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use reqwest::header::{ACCEPT, ETAG, IF_MODIFIED_SINCE, IF_NONE_MATCH, LAST_MODIFIED};
use reqwest::{redirect, Client, StatusCode, Url};
use rrule::Tz;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter, Listener, Manager, State};
use tauri_plugin_store::StoreExt;
use tokio::sync::Notify;
use tokio::task::JoinSet;

use ics::VEvent;

pub const EVENT_CHANGED: &str = "calendar://changed";
/// Kept local (not `crate::settings`) so this module does not depend on the settings struct.
const SETTINGS_FILE: &str = "settings.json";
const SETTINGS_CHANGED: &str = "settings://changed";
const FEEDS_KEY: &str = "calendarFeeds";

const TIMEOUT: Duration = Duration::from_secs(15);
const MAX_BYTES: usize = 10 * 1024 * 1024;
const MAX_REDIRECTS: usize = 5;
const REFRESH_MS: i64 = 15 * 60 * 1000;
const BACKOFF_MIN: [i64; 5] = [1, 2, 5, 10, 15];
const CACHE_VERSION: u32 = 1;
const DEFAULT_LIMIT: u32 = 20;
const MAX_LIMIT: u32 = 500;
const DEFAULT_DAYS: u32 = 7;
const MAX_DAYS: u32 = 366;
const DAY_MS: i64 = 86_400_000;

// ── Contract types (src/lib/ipc/index.ts) ───────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CalendarEvent {
    pub id: String,
    pub feed_id: String,
    pub title: String,
    pub start: i64,
    pub end: i64,
    pub all_day: bool,
    pub location: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FeedErrorEntry {
    pub feed_id: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CalendarUpcoming {
    pub events: Vec<CalendarEvent>,
    pub fetched_at: Option<i64>,
    pub stale: bool,
    pub errors: Vec<FeedErrorEntry>,
}

// ── Feed configuration ──────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeedConfig {
    pub id: String,
    pub url: String,
    pub enabled: bool,
}

fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

/// `calendarFeeds` → feeds. Entries without a valid id or a url are skipped, duplicate
/// ids keep the first, `enabled` defaults to true. Name and colour are UI-only.
pub fn parse_feeds(value: Option<&Value>) -> Vec<FeedConfig> {
    let mut seen = HashSet::new();
    value
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[])
        .iter()
        .filter_map(|entry| {
            let id = entry.get("id")?.as_str()?.trim();
            let url = entry.get("url")?.as_str()?.trim();
            if !valid_id(id) || url.is_empty() || !seen.insert(id.to_string()) {
                return None;
            }
            Some(FeedConfig {
                id: id.to_string(),
                url: url.to_string(),
                enabled: entry
                    .get("enabled")
                    .and_then(Value::as_bool)
                    .unwrap_or(true),
            })
        })
        .collect()
}

// ── Errors (user-safe: never carry the URL) ─────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FeedError {
    InvalidUrl,
    UnsupportedScheme,
    Unreachable,
    Timeout,
    BadRedirect,
    Http(u16),
    TooLarge,
    NotCalendar,
}

impl std::fmt::Display for FeedError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FeedError::InvalidUrl => f.write_str("Invalid feed URL"),
            FeedError::UnsupportedScheme => {
                f.write_str("Only https:// and webcal:// feeds are supported")
            }
            FeedError::Unreachable => f.write_str("Feed unreachable"),
            FeedError::Timeout => f.write_str("Feed timed out"),
            FeedError::BadRedirect => f.write_str("Feed redirected to an unsupported address"),
            FeedError::Http(code) => write!(f, "HTTP {code}"),
            FeedError::TooLarge => f.write_str("Feed too large"),
            FeedError::NotCalendar => f.write_str("Not a calendar file"),
        }
    }
}

/// `webcal://` / `webcals://` → `https://`; anything but https is rejected.
pub fn normalize_url(raw: &str) -> Result<Url, FeedError> {
    let raw = raw.trim();
    let lower = raw.to_ascii_lowercase();
    let rewritten = ["webcal://", "webcals://"]
        .iter()
        .find(|p| lower.starts_with(*p))
        .map(|p| format!("https://{}", &raw[p.len()..]));
    let url = Url::parse(rewritten.as_deref().unwrap_or(raw)).map_err(|_| FeedError::InvalidUrl)?;
    if url.scheme() != "https" {
        return Err(FeedError::UnsupportedScheme);
    }
    if url.host_str().filter(|h| !h.is_empty()).is_none() {
        return Err(FeedError::InvalidUrl);
    }
    Ok(url)
}

fn url_hash(url: &str) -> String {
    let normalized = normalize_url(url)
        .map(String::from)
        .unwrap_or_else(|_| url.to_string());
    format!("{:016x}", ics::fnv1a(normalized.as_bytes()))
}

fn classify(e: &reqwest::Error) -> FeedError {
    if e.is_timeout() {
        FeedError::Timeout
    } else if e.is_redirect() {
        FeedError::BadRedirect
    } else {
        FeedError::Unreachable
    }
}

/// 1, 2, 5, 10, then 15 min after the 1st, 2nd, … consecutive failure.
pub fn backoff_ms(failures: u32) -> i64 {
    let i = (failures.max(1) as usize - 1).min(BACKOFF_MIN.len() - 1);
    BACKOFF_MIN[i] * 60_000
}

// ── Fetch ───────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct Validators {
    etag: Option<String>,
    last_modified: Option<String>,
}

#[derive(Debug)]
enum Fetched {
    NotModified,
    Body {
        text: String,
        validators: Validators,
    },
}

fn client_builder() -> reqwest::ClientBuilder {
    Client::builder()
        .timeout(TIMEOUT)
        .user_agent(concat!("KotiloDeck/", env!("CARGO_PKG_VERSION")))
        .redirect(redirect::Policy::custom(|attempt| {
            if attempt.previous().len() >= MAX_REDIRECTS {
                attempt.error("too many redirects")
            } else if attempt.url().scheme() != "https" {
                attempt.error("redirect to a non-https address")
            } else {
                attempt.follow()
            }
        }))
}

async fn fetch(client: &Client, url: Url, validators: &Validators) -> Result<Fetched, FeedError> {
    let mut req = client
        .get(url)
        .header(ACCEPT, "text/calendar, text/plain;q=0.9, */*;q=0.5");
    if let Some(etag) = &validators.etag {
        req = req.header(IF_NONE_MATCH, etag);
    }
    if let Some(lm) = &validators.last_modified {
        req = req.header(IF_MODIFIED_SINCE, lm);
    }
    let mut resp = req.send().await.map_err(|e| classify(&e))?;
    let status = resp.status();
    if status == StatusCode::NOT_MODIFIED {
        return Ok(Fetched::NotModified);
    }
    if !status.is_success() {
        return Err(FeedError::Http(status.as_u16()));
    }
    if resp.content_length().is_some_and(|n| n > MAX_BYTES as u64) {
        return Err(FeedError::TooLarge);
    }
    let header = |name| {
        resp.headers()
            .get(name)
            .and_then(|v: &reqwest::header::HeaderValue| v.to_str().ok())
            .map(str::to_string)
    };
    let validators = Validators {
        etag: header(ETAG),
        last_modified: header(LAST_MODIFIED),
    };
    let mut body: Vec<u8> = Vec::new();
    while let Some(chunk) = resp.chunk().await.map_err(|e| classify(&e))? {
        if body.len() + chunk.len() > MAX_BYTES {
            return Err(FeedError::TooLarge);
        }
        body.extend_from_slice(&chunk);
    }
    Ok(Fetched::Body {
        text: String::from_utf8_lossy(&body).into_owned(),
        validators,
    })
}

// ── Disk cache ──────────────────────────────────────────────────────────────

/// `cache/calendar/<feedId>.json`. Raw ICS rather than parsed events, so parser fixes
/// apply to cached feeds too.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CacheFile {
    schema_version: u32,
    feed_id: String,
    /// FNV-1a of the normalised URL (not the URL: it may carry a secret).
    url_hash: String,
    fetched_at: i64,
    etag: Option<String>,
    last_modified: Option<String>,
    ics: String,
}

fn cache_path(dir: &std::path::Path, feed_id: &str) -> PathBuf {
    dir.join(format!("{feed_id}.json"))
}

fn read_cache(dir: &std::path::Path, feed_id: &str, url_hash: &str) -> Option<CacheFile> {
    let bytes = fs::read(cache_path(dir, feed_id)).ok()?;
    serde_json::from_slice::<CacheFile>(&bytes)
        .ok()
        .filter(|c| {
            c.schema_version == CACHE_VERSION && c.feed_id == feed_id && c.url_hash == url_hash
        })
}

fn write_cache(dir: &std::path::Path, file: &CacheFile) {
    let write = || -> std::io::Result<()> {
        fs::create_dir_all(dir)?;
        let bytes = serde_json::to_vec(file).map_err(std::io::Error::other)?;
        crate::workspace::write_atomic(&cache_path(dir, &file.feed_id), &bytes)
    };
    if let Err(e) = write() {
        tracing::warn!(feed = %file.feed_id, "cannot write calendar cache: {e}");
    }
}

// ── State ───────────────────────────────────────────────────────────────────

struct FeedState {
    url: String,
    url_hash: String,
    events: Arc<Vec<VEvent>>,
    /// Last successful download (or cache file time); `None` = no data yet.
    fetched_at: Option<i64>,
    validators: Validators,
    error: Option<FeedError>,
    failures: u32,
    next_due: i64,
    /// Data is from the disk cache or older than a failed refresh.
    stale: bool,
    /// Disk cache not consulted yet (done off the setup thread, before the first fetch).
    disk_pending: bool,
}

impl FeedState {
    fn new(url: &str) -> Self {
        Self {
            url: url.to_string(),
            url_hash: url_hash(url),
            events: Arc::new(Vec::new()),
            fetched_at: None,
            validators: Validators::default(),
            error: None,
            failures: 0,
            next_due: 0,
            stale: false,
            disk_pending: true,
        }
    }

    /// What `calendar_upcoming` exposes; a change triggers `calendar://changed`.
    fn visible(&self) -> (Option<FeedError>, bool, bool) {
        (self.error, self.stale, self.fetched_at.is_some())
    }
}

#[derive(Default)]
struct Inner {
    feeds: Vec<FeedConfig>,
    states: HashMap<String, FeedState>,
}

struct Job {
    id: String,
    url: String,
    validators: Validators,
}

struct Shared {
    client: Result<Client, String>,
    cache_dir: PathBuf,
    inner: Mutex<Inner>,
    /// One refresh at a time (timer loop vs. `calendar_refresh`).
    refresh_lock: tokio::sync::Mutex<()>,
    wake: Notify,
    started: AtomicBool,
}

pub struct Calendar(Arc<Shared>);

pub fn cache_dir() -> PathBuf {
    crate::paths::deck_dir().join("cache").join("calendar")
}

fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

impl Calendar {
    pub fn new(cache_dir: PathBuf) -> Self {
        let client = client_builder().build().map_err(|e| {
            tracing::warn!("cannot build calendar HTTP client: {e}");
            e.to_string()
        });
        Self::with_client(cache_dir, client)
    }

    fn with_client(cache_dir: PathBuf, client: Result<Client, String>) -> Self {
        Self(Arc::new(Shared {
            client,
            cache_dir,
            inner: Mutex::new(Inner::default()),
            refresh_lock: tokio::sync::Mutex::new(()),
            wake: Notify::new(),
            started: AtomicBool::new(false),
        }))
    }
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// Applies a new feed list. New feeds / changed URLs start empty and are due now; a
    /// re-enabled feed is due now; removed feeds lose their state and disk cache.
    /// Returns whether anything changed.
    fn set_feeds(&self, feeds: Vec<FeedConfig>) -> bool {
        let mut removed = Vec::new();
        {
            let mut inner = self.lock();
            if inner.feeds == feeds {
                return false;
            }
            let old: HashMap<String, FeedConfig> =
                inner.feeds.drain(..).map(|f| (f.id.clone(), f)).collect();
            let mut states = std::mem::take(&mut inner.states);
            for f in &feeds {
                match states.remove(&f.id) {
                    Some(st) if st.url == f.url => {
                        let mut st = st;
                        let was_enabled = old.get(&f.id).is_some_and(|o| o.enabled);
                        if f.enabled && !was_enabled {
                            st.next_due = 0;
                        }
                        inner.states.insert(f.id.clone(), st);
                    }
                    Some(_) => {
                        // Same id, new URL: the old cache belongs to another calendar.
                        removed.push(f.id.clone());
                        inner.states.insert(f.id.clone(), FeedState::new(&f.url));
                    }
                    None => {
                        inner.states.insert(f.id.clone(), FeedState::new(&f.url));
                    }
                }
            }
            removed.extend(states.into_keys());
            inner.feeds = feeds;
        }
        for id in removed {
            let path = cache_path(&self.cache_dir, &id);
            if let Err(e) = fs::remove_file(&path) {
                if e.kind() != std::io::ErrorKind::NotFound {
                    tracing::warn!(feed = %id, "cannot remove calendar cache: {e}");
                }
            }
        }
        self.wake.notify_one();
        true
    }

    /// Loads disk caches of feeds not consulted yet. Returns whether any data appeared.
    async fn load_disk_caches(&self) -> bool {
        let pending: Vec<(String, String)> = {
            let mut inner = self.lock();
            inner
                .states
                .iter_mut()
                .filter(|(_, st)| st.disk_pending)
                .map(|(id, st)| {
                    st.disk_pending = false;
                    (id.clone(), st.url_hash.clone())
                })
                .collect()
        };
        let mut changed = false;
        for (id, hash) in pending {
            let dir = self.cache_dir.clone();
            let (fid, h) = (id.clone(), hash.clone());
            let loaded = tokio::task::spawn_blocking(move || {
                let file = read_cache(&dir, &fid, &h)?;
                let events = ics::parse_calendar(&file.ics).ok()?;
                Some((file, events))
            })
            .await
            .ok()
            .flatten();
            let Some((file, events)) = loaded else {
                continue;
            };
            let mut inner = self.lock();
            if let Some(st) = inner.states.get_mut(&id) {
                if st.url_hash == hash && st.fetched_at.is_none() {
                    st.events = Arc::new(events);
                    st.fetched_at = Some(file.fetched_at);
                    st.validators = Validators {
                        etag: file.etag,
                        last_modified: file.last_modified,
                    };
                    st.stale = true;
                    changed = true;
                }
            }
        }
        changed
    }

    /// Fetches due (or, with `force`, all) enabled feeds concurrently. Returns whether
    /// anything `calendar_upcoming` reports changed.
    async fn refresh(&self, force: bool) -> bool {
        let _guard = self.refresh_lock.lock().await;
        let mut changed = self.load_disk_caches().await;
        let now = now_ms();
        let jobs: Vec<Job> = {
            let inner = self.lock();
            inner
                .feeds
                .iter()
                .filter(|f| f.enabled)
                .filter_map(|f| {
                    let st = inner.states.get(&f.id)?;
                    (force || st.next_due <= now).then(|| Job {
                        id: f.id.clone(),
                        url: f.url.clone(),
                        // Without data a 304 would leave the feed empty.
                        validators: if st.fetched_at.is_some() {
                            st.validators.clone()
                        } else {
                            Validators::default()
                        },
                    })
                })
                .collect()
        };
        if jobs.is_empty() {
            return changed;
        }

        let mut set = JoinSet::new();
        for job in jobs {
            let client = self.client.clone();
            set.spawn(async move {
                let result = match (client, normalize_url(&job.url)) {
                    (_, Err(e)) => Err(e),
                    (Err(_), _) => Err(FeedError::Unreachable),
                    (Ok(c), Ok(url)) => fetch(&c, url, &job.validators).await,
                };
                // Parse off the async workers (feeds can be megabytes).
                let parsed = match result {
                    Ok(Fetched::Body { text, validators }) => {
                        tokio::task::spawn_blocking(move || {
                            ics::parse_calendar(&text)
                                .map(|events| (events, text, validators))
                                .map_err(|_| FeedError::NotCalendar)
                        })
                        .await
                        .unwrap_or(Err(FeedError::NotCalendar))
                        .map(Some)
                    }
                    Ok(Fetched::NotModified) => Ok(None),
                    Err(e) => Err(e),
                };
                (job, parsed)
            });
        }
        while let Some(joined) = set.join_next().await {
            let Ok((job, parsed)) = joined else {
                continue;
            };
            changed |= self.apply(job, parsed, now_ms()).await;
        }
        changed
    }

    async fn apply(
        &self,
        job: Job,
        parsed: Result<Option<(Vec<VEvent>, String, Validators)>, FeedError>,
        now: i64,
    ) -> bool {
        let mut to_write = None;
        let changed = {
            let mut inner = self.lock();
            let Some(st) = inner.states.get_mut(&job.id).filter(|st| st.url == job.url) else {
                return false; // feed removed or re-pointed while fetching
            };
            let before = st.visible();
            let mut events_changed = false;
            match parsed {
                Ok(fresh) => {
                    if let Some((events, text, validators)) = fresh {
                        events_changed = *st.events != events;
                        st.events = Arc::new(events);
                        st.validators = validators.clone();
                        to_write = Some(CacheFile {
                            schema_version: CACHE_VERSION,
                            feed_id: job.id.clone(),
                            url_hash: st.url_hash.clone(),
                            fetched_at: now,
                            etag: validators.etag,
                            last_modified: validators.last_modified,
                            ics: text,
                        });
                    }
                    st.fetched_at = Some(now);
                    st.error = None;
                    st.failures = 0;
                    st.stale = false;
                    st.next_due = now + REFRESH_MS;
                    tracing::debug!(feed = %job.id, events = st.events.len(), "calendar feed refreshed");
                }
                Err(e) => {
                    st.failures = st.failures.saturating_add(1);
                    st.error = Some(e);
                    st.stale = st.fetched_at.is_some();
                    st.next_due = now + backoff_ms(st.failures);
                    tracing::warn!(feed = %job.id, failures = st.failures, "calendar fetch failed: {e}");
                }
            }
            events_changed || st.visible() != before
        };
        if let Some(file) = to_write {
            let dir = self.cache_dir.clone();
            let _ = tokio::task::spawn_blocking(move || write_cache(&dir, &file)).await;
        }
        changed
    }

    /// Time until the next feed is due (≥ 1 s, ≤ the refresh interval).
    fn next_wait(&self, now: i64) -> Duration {
        let inner = self.lock();
        let next = inner
            .feeds
            .iter()
            .filter(|f| f.enabled)
            .filter_map(|f| inner.states.get(&f.id).map(|st| st.next_due))
            .min()
            .unwrap_or(now + REFRESH_MS);
        Duration::from_millis((next - now).clamp(1_000, REFRESH_MS) as u64)
    }

    fn upcoming(&self, now: i64, limit: u32, days: u32, local: Tz) -> CalendarUpcoming {
        let mut errors = Vec::new();
        let mut sources = Vec::new();
        let mut fetched_at: Option<i64> = None;
        let mut stale = false;
        {
            let inner = self.lock();
            for f in inner.feeds.iter().filter(|f| f.enabled) {
                let Some(st) = inner.states.get(&f.id) else {
                    continue;
                };
                if let Some(e) = st.error {
                    errors.push(FeedErrorEntry {
                        feed_id: f.id.clone(),
                        message: e.to_string(),
                    });
                }
                if let Some(t) = st.fetched_at {
                    fetched_at = Some(fetched_at.map_or(t, |o| o.min(t)));
                    stale |= st.stale;
                    sources.push((f.id.clone(), st.events.clone()));
                }
            }
        }
        let to = now.saturating_add(i64::from(days) * DAY_MS);
        let mut events: Vec<CalendarEvent> = Vec::new();
        let mut ids = HashSet::new();
        for (feed_id, feed_events) in sources {
            for o in ics::expand(&feed_events, now, to, local) {
                let id = format!("{feed_id}:{}:{}", o.uid, o.key_ms);
                if ids.insert(id.clone()) {
                    events.push(CalendarEvent {
                        id,
                        feed_id: feed_id.clone(),
                        title: o.title,
                        start: o.start_ms,
                        end: o.end_ms,
                        all_day: o.all_day,
                        location: o.location,
                    });
                }
            }
        }
        sort_events(&mut events, local);
        events.truncate(limit as usize);
        CalendarUpcoming {
            events,
            fetched_at,
            stale,
            errors,
        }
    }
}

/// By start; within a local day all-day events come first.
fn sort_events(events: &mut [CalendarEvent], local: Tz) {
    use chrono::TimeZone;
    let day = |ms: i64| {
        local
            .timestamp_millis_opt(ms)
            .single()
            .map(|d| d.date_naive())
    };
    events.sort_by(|a, b| {
        day(a.start)
            .cmp(&day(b.start))
            .then(b.all_day.cmp(&a.all_day))
            .then(a.start.cmp(&b.start))
            .then(a.end.cmp(&b.end))
            .then_with(|| a.title.cmp(&b.title))
            .then_with(|| a.id.cmp(&b.id))
    });
}

// ── App wiring ──────────────────────────────────────────────────────────────

/// `None` when the store cannot be read (keep the current feeds rather than drop them).
fn read_feeds(app: &AppHandle) -> Option<Vec<FeedConfig>> {
    match app.store(SETTINGS_FILE) {
        Ok(store) => Some(parse_feeds(store.get(FEEDS_KEY).as_ref())),
        Err(e) => {
            tracing::warn!("calendar: settings store unavailable: {e}");
            None
        }
    }
}

fn emit_changed(app: &AppHandle) {
    if let Err(e) = app.emit(EVENT_CHANGED, json!({})) {
        tracing::warn!("cannot emit calendar change: {e}");
    }
}

/// Setup hook: loads the feed list, follows `settings://changed` and starts the refresh
/// loop. Idempotent.
pub fn start(app: &AppHandle) {
    let shared = app.state::<Calendar>().0.clone();
    if shared.started.swap(true, Ordering::AcqRel) {
        return;
    }
    if let Some(feeds) = read_feeds(app) {
        shared.set_feeds(feeds);
    }
    {
        let (shared, handle) = (shared.clone(), app.clone());
        app.listen(SETTINGS_CHANGED, move |_| {
            if let Some(feeds) = read_feeds(&handle) {
                if shared.set_feeds(feeds) {
                    emit_changed(&handle);
                }
            }
        });
    }
    let handle = app.clone();
    tauri::async_runtime::spawn(async move {
        loop {
            if shared.refresh(false).await {
                emit_changed(&handle);
            }
            let wait = shared.next_wait(now_ms());
            tokio::select! {
                _ = tokio::time::sleep(wait) => {}
                _ = shared.wake.notified() => {}
            }
        }
    });
}

#[tauri::command]
pub async fn calendar_upcoming(
    calendar: State<'_, Calendar>,
    limit: Option<u32>,
    days: Option<u32>,
) -> Result<CalendarUpcoming, String> {
    let limit = limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT);
    let days = days.unwrap_or(DEFAULT_DAYS).clamp(1, MAX_DAYS);
    let shared = calendar.0.clone();
    tokio::task::spawn_blocking(move || shared.upcoming(now_ms(), limit, days, Tz::LOCAL))
        .await
        .map_err(|e| format!("calendar query failed: {e}"))
}

#[tauri::command]
pub async fn calendar_refresh(app: AppHandle, calendar: State<'_, Calendar>) -> Result<(), String> {
    let shared = calendar.0.clone();
    let mut changed = read_feeds(&app).is_some_and(|feeds| shared.set_feeds(feeds));
    changed |= shared.refresh(true).await;
    if changed {
        emit_changed(&app);
    }
    shared.wake.notify_one();
    Ok(())
}

#[cfg(test)]
mod tests;
