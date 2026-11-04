//! Media via MPRIS (`org.mpris.MediaPlayer2.*`) on the session D-Bus, using `zbus`.
//!
//! Reads/controls run on the `kotilo-media` worker with a blocking connection, which also
//! owns the artwork cache and a small current-thread tokio runtime for `http(s)` cover art
//! (Spotify). The watcher has its own connection on its own thread.
//!
//! The D-Bus → [`MediaSession`] mapping and the session pick rule are pure functions,
//! unit-tested against hand-built MPRIS property maps.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use zbus::zvariant::{ObjectPath, OwnedValue, Value};

use super::worker::Worker;
use crate::platform::shared::{artwork_data_url, ARTWORK_MAX_BYTES};
use crate::platform::{
    ChangeFn, MediaAction, MediaSession, MediaState, PlatformError, RepeatMode, Result, Watch,
};

static WORKER: Worker = Worker::new("kotilo-media");

/// Runs `f` on the media worker.
pub fn run<T: Send + 'static>(f: impl FnOnce() -> Result<T> + Send + 'static) -> Result<T> {
    WORKER.run(f)
}

pub const PLAYER_PREFIX: &str = "org.mpris.MediaPlayer2.";
const MPRIS_PATH: &str = "/org/mpris/MediaPlayer2";
const ROOT_IFACE: &str = "org.mpris.MediaPlayer2";
const PLAYER_IFACE: &str = "org.mpris.MediaPlayer2.Player";
const PROPS_IFACE: &str = "org.freedesktop.DBus.Properties";
const NO_TRACK: &str = "/org/mpris/MediaPlayer2/TrackList/NoTrack";
/// A D-Bus call to a player never blocks the worker longer than this.
const METHOD_TIMEOUT: Duration = Duration::from_secs(3);
/// Cover art download budget (Spotify's CDN answers in ~100 ms).
const ART_FETCH_TIMEOUT: Duration = Duration::from_secs(3);
/// A failed http(s) cover is retried after this long, not on every state read.
const ART_RETRY: Duration = Duration::from_secs(30);
const ART_CACHE_LEN: usize = 4;

/// Proxies that mirror another player (would show every session twice).
fn ignored(name: &str) -> bool {
    name == "org.mpris.MediaPlayer2.playerctld"
}

/// True when a session bus looks reachable (address in the environment or the standard
/// `$XDG_RUNTIME_DIR/bus` socket). No connection is made.
pub fn bus_available() -> bool {
    if std::env::var_os("DBUS_SESSION_BUS_ADDRESS").is_some_and(|a| !a.is_empty()) {
        return true;
    }
    std::env::var_os("XDG_RUNTIME_DIR")
        .map(|d| PathBuf::from(d).join("bus"))
        .is_some_and(|p| p.exists())
}

// ── Pure mapping ────────────────────────────────────────────────────────────

/// `xesam:` / `mpris:` metadata we use.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Metadata {
    pub trackid: Option<String>,
    pub title: String,
    pub artists: Vec<String>,
    pub album: Option<String>,
    pub length_us: Option<i64>,
    pub art_url: Option<String>,
    pub url: Option<String>,
}

/// `org.mpris.MediaPlayer2.Player` properties we use.
#[derive(Debug, Clone, PartialEq)]
pub struct PlayerProps {
    pub status: String,
    pub loop_status: Option<String>,
    pub shuffle: Option<bool>,
    pub position_us: Option<i64>,
    pub can_next: bool,
    pub can_prev: bool,
    pub can_play: bool,
    pub can_pause: bool,
    pub can_control: bool,
    pub metadata: Metadata,
}

/// Strips nested variants (`v` inside `a{sv}`).
fn inner<'a, 'v>(v: &'a Value<'v>) -> &'a Value<'v> {
    match v {
        Value::Value(b) => inner(b),
        other => other,
    }
}

fn v_str(v: &Value) -> Option<String> {
    match inner(v) {
        Value::Str(s) => Some(s.as_str().to_string()),
        Value::ObjectPath(p) => Some(p.as_str().to_string()),
        _ => None,
    }
}

/// `as` (spec) or a plain `s` (some players).
fn v_strs(v: &Value) -> Vec<String> {
    match inner(v) {
        Value::Array(a) => a.iter().filter_map(v_str).collect(),
        other => v_str(other).into_iter().collect(),
    }
}

/// `x` per spec; players also send `t`, `i`, `u` or `d`.
fn v_i64(v: &Value) -> Option<i64> {
    match inner(v) {
        Value::I64(n) => Some(*n),
        Value::U64(n) => i64::try_from(*n).ok(),
        Value::I32(n) => Some(i64::from(*n)),
        Value::U32(n) => Some(i64::from(*n)),
        Value::I16(n) => Some(i64::from(*n)),
        Value::U16(n) => Some(i64::from(*n)),
        Value::U8(n) => Some(i64::from(*n)),
        Value::F64(f) if f.is_finite() => Some(*f as i64),
        _ => None,
    }
}

fn v_bool(v: &Value) -> Option<bool> {
    match inner(v) {
        Value::Bool(b) => Some(*b),
        _ => None,
    }
}

fn non_empty(s: Option<String>) -> Option<String> {
    s.map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

/// `Metadata` (`a{sv}`) → [`Metadata`].
pub fn parse_metadata(v: &Value) -> Metadata {
    let mut m = Metadata::default();
    let Value::Dict(dict) = inner(v) else {
        return m;
    };
    for (k, val) in dict.iter() {
        let Some(key) = v_str(k) else { continue };
        match key.as_str() {
            "mpris:trackid" => m.trackid = non_empty(v_str(val)),
            "xesam:title" => m.title = v_str(val).unwrap_or_default().trim().to_string(),
            "xesam:artist" => {
                m.artists = v_strs(val)
                    .into_iter()
                    .map(|a| a.trim().to_string())
                    .filter(|a| !a.is_empty())
                    .collect();
            }
            "xesam:album" => m.album = non_empty(v_str(val)),
            "mpris:length" => m.length_us = v_i64(val),
            "mpris:artUrl" => m.art_url = non_empty(v_str(val)),
            "xesam:url" => m.url = non_empty(v_str(val)),
            _ => {}
        }
    }
    m
}

/// `GetAll(org.mpris.MediaPlayer2.Player)` → [`PlayerProps`]. Missing optional properties
/// (`Shuffle`, `LoopStatus`) stay `None`; missing `Can*` flags default to false, except
/// `CanControl` (absent on some players) which defaults to true.
pub fn parse_player(map: &HashMap<String, OwnedValue>) -> PlayerProps {
    let get = |k: &str| map.get(k).map(|v| &**v);
    let flag = |k: &str| get(k).and_then(v_bool).unwrap_or(false);
    PlayerProps {
        status: get("PlaybackStatus")
            .and_then(v_str)
            .unwrap_or_else(|| "Stopped".into()),
        loop_status: get("LoopStatus").and_then(v_str),
        shuffle: get("Shuffle").and_then(v_bool),
        position_us: get("Position").and_then(v_i64),
        can_next: flag("CanGoNext"),
        can_prev: flag("CanGoPrevious"),
        can_play: flag("CanPlay"),
        can_pause: flag("CanPause"),
        can_control: get("CanControl").and_then(v_bool).unwrap_or(true),
        metadata: get("Metadata").map(parse_metadata).unwrap_or_default(),
    }
}

pub fn repeat_from_mpris(s: &str) -> Option<RepeatMode> {
    match s {
        "None" => Some(RepeatMode::None),
        "Track" => Some(RepeatMode::Track),
        "Playlist" => Some(RepeatMode::List),
        _ => None,
    }
}

pub fn repeat_to_mpris(r: RepeatMode) -> &'static str {
    match r {
        RepeatMode::None => "None",
        RepeatMode::Track => "Track",
        RepeatMode::List => "Playlist",
    }
}

/// Decodes `%XX` escapes (invalid escapes are kept literally).
pub fn percent_decode(s: &str) -> Vec<u8> {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            let hex = |c: u8| (c as char).to_digit(16);
            if let (Some(h), Some(l)) = (hex(b[i + 1]), hex(b[i + 2])) {
                out.push((h * 16 + l) as u8);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    out
}

/// Title for players that publish only `xesam:url` (local files in mpv/VLC): the file name.
fn title_from_url(url: &str) -> Option<String> {
    let path = url.split(['?', '#']).next().unwrap_or(url);
    let last = path.trim_end_matches('/').rsplit('/').next()?;
    let name = String::from_utf8_lossy(&percent_decode(last))
        .trim()
        .to_string();
    (!name.is_empty() && !name.contains(':')).then_some(name)
}

/// (position, duration) in seconds; both `None` without a usable track length.
pub fn timeline(position_us: Option<i64>, length_us: Option<i64>) -> (Option<f64>, Option<f64>) {
    match length_us {
        Some(len) if len > 0 => {
            let pos = position_us.unwrap_or(0).clamp(0, len);
            (Some(pos as f64 / 1e6), Some(len as f64 / 1e6))
        }
        _ => (None, None),
    }
}

/// [`PlayerProps`] → [`MediaSession`] (artwork resolved separately).
pub fn to_session(
    p: &PlayerProps,
    source: String,
    artwork: Option<String>,
    now_ms: i64,
) -> MediaSession {
    let m = &p.metadata;
    let title = if m.title.is_empty() {
        m.url
            .as_deref()
            .and_then(title_from_url)
            .unwrap_or_default()
    } else {
        m.title.clone()
    };
    let (position, duration) = timeline(p.position_us, m.length_us);
    MediaSession {
        source,
        title,
        artist: m.artists.join(", "),
        album: m.album.clone(),
        artwork,
        playing: p.status == "Playing",
        position,
        duration,
        position_at: now_ms,
        shuffle: p.shuffle,
        repeat: p.loop_status.as_deref().and_then(repeat_from_mpris),
        can_prev: p.can_control && p.can_prev,
        can_next: p.can_control && p.can_next,
        can_play_pause: p.can_control && (p.can_play || p.can_pause),
    }
}

/// Readable name from a bus name when the player has no `Identity`:
/// `org.mpris.MediaPlayer2.firefox.instance_1_84` → "Firefox".
pub fn pretty_source(bus_name: &str) -> String {
    let rest = bus_name.strip_prefix(PLAYER_PREFIX).unwrap_or(bus_name);
    let base = rest.split('.').next().unwrap_or(rest);
    let known = match base.to_ascii_lowercase().as_str() {
        "spotify" => Some("Spotify"),
        "firefox" => Some("Firefox"),
        "chromium" => Some("Chromium"),
        "chrome" | "google-chrome" => Some("Google Chrome"),
        "brave" => Some("Brave"),
        "vivaldi" => Some("Vivaldi"),
        "vlc" => Some("VLC media player"),
        "mpv" => Some("mpv"),
        "rhythmbox" => Some("Rhythmbox"),
        "elisa" => Some("Elisa"),
        _ => None,
    };
    if let Some(k) = known {
        return k.to_string();
    }
    let mut c = base.chars();
    match c.next() {
        Some(f) => f.to_uppercase().chain(c).collect(),
        None => "Unknown app".into(),
    }
}

/// A player as seen by the pick rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Candidate {
    pub playing: bool,
    /// Logical time the player last started or stopped playing; `None` = never seen active.
    pub active_at: Option<u64>,
}

/// Session pick (DECISIONS 35, Linux flavour): the most recently started playing player,
/// else the most recently active one, else the first. Ties keep list order.
pub fn pick(cands: &[Candidate]) -> Option<usize> {
    let best = |playing_only: bool| {
        cands
            .iter()
            .enumerate()
            .filter(|(_, c)| !playing_only || c.playing)
            .fold(
                None,
                |best: Option<(usize, Option<u64>)>, (i, c)| match best {
                    Some((_, at)) if at >= c.active_at => best,
                    _ => Some((i, c.active_at)),
                },
            )
    };
    if let Some((i, _)) = best(true) {
        return Some(i);
    }
    match best(false) {
        Some((i, Some(_))) => Some(i),
        _ => (!cands.is_empty()).then_some(0),
    }
}

/// Tracks when each player last changed between playing and not playing ("recently
/// active"), across state reads.
#[derive(Debug, Default)]
pub struct Activity {
    tick: u64,
    seen: HashMap<String, (bool, Option<u64>)>,
}

impl Activity {
    /// Records the current statuses (`(bus name, playing)`, already sorted) and returns the
    /// pick candidates in the same order. Players that disappeared are forgotten.
    pub fn update(&mut self, players: &[(String, bool)]) -> Vec<Candidate> {
        self.tick += 1;
        let tick = self.tick;
        self.seen
            .retain(|name, _| players.iter().any(|(n, _)| n == name));
        players
            .iter()
            .map(|(name, playing)| {
                let e = self
                    .seen
                    .entry(name.clone())
                    .or_insert((*playing, playing.then_some(tick)));
                if e.0 != *playing {
                    *e = (*playing, Some(tick));
                }
                Candidate {
                    playing: *playing,
                    active_at: e.1,
                }
            })
            .collect()
    }
}

// ── Artwork ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub enum ArtSource {
    File(PathBuf),
    Http(String),
    /// Already a png/jpeg data URL within the cap.
    Data(String),
}

#[cfg(unix)]
fn path_from_bytes(b: Vec<u8>) -> PathBuf {
    use std::os::unix::ffi::OsStringExt;
    PathBuf::from(std::ffi::OsString::from_vec(b))
}

#[cfg(not(unix))]
fn path_from_bytes(b: Vec<u8>) -> PathBuf {
    PathBuf::from(String::from_utf8_lossy(&b).into_owned())
}

/// Spotify's `ab67616d0000b273` covers are 640 px (often > 150 KB); `ab67616d00001e02` is
/// the 300 px variant of the same image. Old clients publish `open.spotify.com/image/<id>`.
pub fn spotify_small(url: &str) -> String {
    let url = url.replace(
        "https://open.spotify.com/image/",
        "https://i.scdn.co/image/",
    );
    if url.starts_with("https://i.scdn.co/image/") {
        url.replace("ab67616d0000b273", "ab67616d00001e02")
    } else {
        url
    }
}

pub fn art_source(url: &str) -> Option<ArtSource> {
    if let Some(rest) = url.strip_prefix("file://") {
        let path = rest.strip_prefix("localhost").unwrap_or(rest);
        let path = path.split(['?', '#']).next().unwrap_or(path);
        return path
            .starts_with('/')
            .then(|| ArtSource::File(path_from_bytes(percent_decode(path))));
    }
    if url.starts_with("https://") || url.starts_with("http://") {
        return Some(ArtSource::Http(spotify_small(url)));
    }
    let data_ok = (url.starts_with("data:image/png;base64,")
        || url.starts_with("data:image/jpeg;base64,"))
        && url.len() <= ARTWORK_MAX_BYTES.div_ceil(3) * 4 + 32;
    data_ok.then(|| ArtSource::Data(url.to_string()))
}

fn read_file_art(path: &std::path::Path) -> Option<String> {
    let meta = std::fs::metadata(path).ok()?;
    // Never read big files; a too-big cover is dropped (no image decoder in this build).
    if !meta.is_file() || meta.len() == 0 || meta.len() > ARTWORK_MAX_BYTES as u64 {
        return None;
    }
    artwork_data_url(&std::fs::read(path).ok()?)
}

struct Http {
    rt: tokio::runtime::Runtime,
    client: reqwest::Client,
}

fn http() -> Option<Http> {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| tracing::debug!("artwork runtime unavailable: {e}"))
        .ok()?;
    let client = reqwest::Client::builder()
        .timeout(ART_FETCH_TIMEOUT)
        .connect_timeout(Duration::from_secs(2))
        .redirect(reqwest::redirect::Policy::limited(3))
        .user_agent(concat!("KotiloDeck/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|e| tracing::debug!("artwork HTTP client unavailable: {e}"))
        .ok()?;
    Some(Http { rt, client })
}

async fn fetch(client: &reqwest::Client, url: &str) -> Option<Vec<u8>> {
    let mut resp = client.get(url).send().await.ok()?.error_for_status().ok()?;
    if resp
        .content_length()
        .is_some_and(|n| n > ARTWORK_MAX_BYTES as u64)
    {
        return None;
    }
    let mut buf = Vec::new();
    while let Some(chunk) = resp.chunk().await.ok()? {
        buf.extend_from_slice(&chunk);
        if buf.len() > ARTWORK_MAX_BYTES {
            return None;
        }
    }
    Some(buf)
}

struct ArtEntry {
    url: String,
    art: Option<String>,
    at: Instant,
}

thread_local! {
    static CONN: RefCell<Option<zbus::blocking::Connection>> = const { RefCell::new(None) };
    static HTTP: RefCell<Option<Option<Http>>> = const { RefCell::new(None) };
    static ART: RefCell<Vec<ArtEntry>> = const { RefCell::new(Vec::new()) };
    static ACTIVITY: RefCell<Activity> = RefCell::new(Activity::default());
    static IDENTITIES: RefCell<HashMap<String, String>> = RefCell::new(HashMap::new());
}

fn fetch_http(url: &str) -> Option<String> {
    HTTP.with(|h| {
        let mut h = h.borrow_mut();
        let http = h.get_or_insert_with(http).as_ref()?;
        let bytes = http.rt.block_on(fetch(&http.client, url))?;
        artwork_data_url(&bytes)
    })
}

/// Cover art for `art_url`, cached by URL. File covers are cached only on success (players
/// often write the file a moment after publishing the URL); failed downloads are retried
/// after [`ART_RETRY`].
fn artwork(art_url: &str) -> Option<String> {
    let cached = ART.with(|c| {
        c.borrow()
            .iter()
            .find(|e| e.url == art_url)
            .filter(|e| e.art.is_some() || e.at.elapsed() < ART_RETRY)
            .map(|e| e.art.clone())
    });
    if let Some(art) = cached {
        return art;
    }
    let (art, cache_failure) = match art_source(art_url)? {
        ArtSource::Data(d) => (Some(d), true),
        ArtSource::File(p) => (read_file_art(&p), false),
        ArtSource::Http(u) => (fetch_http(&u), true),
    };
    if art.is_some() || cache_failure {
        ART.with(|c| {
            let mut c = c.borrow_mut();
            c.retain(|e| e.url != art_url);
            c.insert(
                0,
                ArtEntry {
                    url: art_url.to_string(),
                    art: art.clone(),
                    at: Instant::now(),
                },
            );
            c.truncate(ART_CACHE_LEN);
        });
    }
    art
}

// ── D-Bus calls (media worker) ──────────────────────────────────────────────

fn dbus_err(e: zbus::Error) -> PlatformError {
    PlatformError::Os(format!("D-Bus: {e}"))
}

fn connect() -> Result<zbus::blocking::Connection> {
    zbus::blocking::connection::Builder::session()
        .map_err(dbus_err)?
        .method_timeout(METHOD_TIMEOUT)
        .build()
        .map_err(|e| PlatformError::Os(format!("session D-Bus unavailable: {e}")))
}

fn conn() -> Result<zbus::blocking::Connection> {
    CONN.with(|c| {
        let mut c = c.borrow_mut();
        if let Some(conn) = c.as_ref() {
            return Ok(conn.clone());
        }
        let conn = connect()?;
        *c = Some(conn.clone());
        Ok(conn)
    })
}

/// Drops the cached connection after a transport error so the next call reconnects.
fn reset_on_transport_error<T>(r: Result<T>, e: &zbus::Error) -> Result<T> {
    if matches!(e, zbus::Error::InputOutput(_)) {
        CONN.with(|c| *c.borrow_mut() = None);
    }
    r
}

fn call<B, R>(
    c: &zbus::blocking::Connection,
    dest: &str,
    iface: &str,
    method: &str,
    body: &B,
) -> Result<R>
where
    B: serde::Serialize + zbus::zvariant::DynamicType,
    R: for<'d> serde::Deserialize<'d> + zbus::zvariant::Type,
{
    let msg = match c.call_method(Some(dest), MPRIS_PATH, Some(iface), method, body) {
        Ok(m) => m,
        Err(e) => {
            let r = Err(dbus_err(e.clone()));
            return reset_on_transport_error(r, &e);
        }
    };
    msg.body().deserialize::<R>().map_err(dbus_err)
}

fn players(c: &zbus::blocking::Connection) -> Result<Vec<String>> {
    let msg = c
        .call_method(
            Some("org.freedesktop.DBus"),
            "/org/freedesktop/DBus",
            Some("org.freedesktop.DBus"),
            "ListNames",
            &(),
        )
        .map_err(|e| {
            let r = Err::<(), _>(dbus_err(e.clone()));
            let _ = reset_on_transport_error(r, &e);
            dbus_err(e)
        })?;
    let mut names: Vec<String> = msg
        .body()
        .deserialize::<Vec<String>>()
        .map_err(dbus_err)?
        .into_iter()
        .filter(|n| n.starts_with(PLAYER_PREFIX) && !ignored(n))
        .collect();
    names.sort();
    Ok(names)
}

fn get_prop(
    c: &zbus::blocking::Connection,
    dest: &str,
    iface: &str,
    name: &str,
) -> Result<OwnedValue> {
    call(c, dest, PROPS_IFACE, "Get", &(iface, name))
}

fn is_playing(c: &zbus::blocking::Connection, dest: &str) -> bool {
    get_prop(c, dest, PLAYER_IFACE, "PlaybackStatus")
        .ok()
        .and_then(|v| v_str(&v))
        .is_some_and(|s| s == "Playing")
}

fn identity(c: &zbus::blocking::Connection, dest: &str) -> String {
    if let Some(n) = IDENTITIES.with(|m| m.borrow().get(dest).cloned()) {
        return n;
    }
    let name = get_prop(c, dest, ROOT_IFACE, "Identity")
        .ok()
        .and_then(|v| non_empty(v_str(&v)))
        .unwrap_or_else(|| pretty_source(dest));
    IDENTITIES.with(|m| m.borrow_mut().insert(dest.to_string(), name.clone()));
    name
}

fn player_props(c: &zbus::blocking::Connection, dest: &str) -> Result<PlayerProps> {
    let map: HashMap<String, OwnedValue> = call(c, dest, PROPS_IFACE, "GetAll", &(PLAYER_IFACE,))?;
    Ok(parse_player(&map))
}

/// The bus name of the player to show/control (pick rule over every MPRIS player).
fn active_player(c: &zbus::blocking::Connection) -> Result<Option<String>> {
    let names = players(c)?;
    let statuses: Vec<(String, bool)> = names
        .iter()
        .map(|n| (n.clone(), is_playing(c, n)))
        .collect();
    let cands = ACTIVITY.with(|a| a.borrow_mut().update(&statuses));
    IDENTITIES.with(|m| m.borrow_mut().retain(|k, _| names.contains(k)));
    Ok(pick(&cands).map(|i| names[i].clone()))
}

pub fn state() -> Result<MediaState> {
    let c = conn()?;
    let Some(dest) = active_player(&c)? else {
        return Ok(MediaState { session: None });
    };
    let props = player_props(&c, &dest)?;
    let art = props.metadata.art_url.as_deref().and_then(artwork);
    let source = identity(&c, &dest);
    Ok(MediaState {
        session: Some(to_session(
            &props,
            source,
            art,
            chrono::Utc::now().timestamp_millis(),
        )),
    })
}

fn active(c: &zbus::blocking::Connection) -> Result<String> {
    active_player(c)?.ok_or_else(|| PlatformError::Os("nothing is playing".into()))
}

fn set_prop(
    c: &zbus::blocking::Connection,
    dest: &str,
    name: &str,
    value: Value<'_>,
) -> Result<()> {
    call::<_, ()>(c, dest, PROPS_IFACE, "Set", &(PLAYER_IFACE, name, value))
}

pub fn control(action: MediaAction) -> Result<()> {
    let c = conn()?;
    let dest = active(&c)?;
    let method = |m: &str| call::<_, ()>(&c, &dest, PLAYER_IFACE, m, &());
    match action {
        MediaAction::PlayPause => method("PlayPause"),
        MediaAction::Next => method("Next"),
        MediaAction::Prev => method("Previous"),
        MediaAction::ToggleShuffle => {
            let cur = get_prop(&c, &dest, PLAYER_IFACE, "Shuffle")
                .ok()
                .and_then(|v| v_bool(&v))
                .ok_or_else(|| PlatformError::Os("this app has no shuffle".into()))?;
            set_prop(&c, &dest, "Shuffle", Value::from(!cur))
        }
        MediaAction::CycleRepeat => {
            let cur = get_prop(&c, &dest, PLAYER_IFACE, "LoopStatus")
                .ok()
                .and_then(|v| v_str(&v))
                .and_then(|s| repeat_from_mpris(&s))
                .ok_or_else(|| PlatformError::Os("this app has no repeat".into()))?;
            set_prop(
                &c,
                &dest,
                "LoopStatus",
                Value::from(repeat_to_mpris(cur.next())),
            )
        }
    }
}

/// Absolute seek target in µs, clamped to the track.
pub fn seek_target_us(seconds: f64, length_us: Option<i64>) -> i64 {
    let t = (seconds.max(0.0) * 1e6) as i64;
    match length_us {
        Some(len) if len > 0 => t.min(len),
        _ => t,
    }
}

pub fn seek(seconds: f64) -> Result<()> {
    let c = conn()?;
    let dest = active(&c)?;
    let p = player_props(&c, &dest)?;
    let target = seek_target_us(seconds, p.metadata.length_us);
    match p.metadata.trackid.as_deref().filter(|t| *t != NO_TRACK) {
        Some(id) => {
            let path = ObjectPath::try_from(id)
                .map_err(|_| PlatformError::Os("the media app sent a bad track id".into()))?;
            call::<_, ()>(&c, &dest, PLAYER_IFACE, "SetPosition", &(path, target))
        }
        // No track id (some browsers): relative `Seek` from the current position.
        None => {
            let cur = p
                .position_us
                .ok_or_else(|| PlatformError::Os("this app cannot seek".into()))?;
            call::<_, ()>(&c, &dest, PLAYER_IFACE, "Seek", &(target - cur,))
        }
    }
}

// ── Change notifications ────────────────────────────────────────────────────

fn rules() -> zbus::Result<Vec<zbus::MatchRule<'static>>> {
    use zbus::message::Type;
    Ok(vec![
        // Playback / metadata / shuffle / loop changes of any player.
        zbus::MatchRule::builder()
            .msg_type(Type::Signal)
            .interface(PROPS_IFACE)?
            .member("PropertiesChanged")?
            .path(MPRIS_PATH)?
            .arg(0, PLAYER_IFACE)?
            .build(),
        // Position jumps (Position itself is never signalled).
        zbus::MatchRule::builder()
            .msg_type(Type::Signal)
            .interface(PLAYER_IFACE)?
            .member("Seeked")?
            .path(MPRIS_PATH)?
            .build(),
        // Players appearing / quitting.
        zbus::MatchRule::builder()
            .msg_type(Type::Signal)
            .sender("org.freedesktop.DBus")?
            .interface("org.freedesktop.DBus")?
            .member("NameOwnerChanged")?
            .arg0ns("org.mpris.MediaPlayer2")?
            .build(),
    ])
}

async fn watch_signals(
    on_change: ChangeFn,
    ready: std::sync::mpsc::SyncSender<Result<()>>,
    mut stop: tokio::sync::oneshot::Receiver<()>,
) {
    use futures_util::StreamExt;
    let setup = async {
        let conn = zbus::Connection::session()
            .await
            .map_err(|e| PlatformError::Os(format!("session D-Bus unavailable: {e}")))?;
        let mut streams = Vec::new();
        for rule in rules().map_err(dbus_err)? {
            streams.push(
                zbus::MessageStream::for_match_rule(rule, &conn, Some(64))
                    .await
                    .map_err(dbus_err)?,
            );
        }
        Ok::<_, PlatformError>((conn, futures_util::stream::select_all(streams)))
    };
    let (conn, mut events) = match setup.await {
        Ok(v) => {
            let _ = ready.send(Ok(()));
            v
        }
        Err(e) => {
            let _ = ready.send(Err(e));
            return;
        }
    };
    tracing::debug!("media watcher started");
    loop {
        tokio::select! {
            _ = &mut stop => break,
            msg = events.next() => match msg {
                Some(Ok(_)) => on_change(),
                Some(Err(e)) => tracing::debug!("media watcher: {e}"),
                None => break,
            },
        }
    }
    // Dropping the streams removes their match rules; then the connection closes.
    drop(events);
    drop(conn);
    tracing::debug!("media watcher stopped");
}

/// Dropping it stops the watcher thread (which unregisters its match rules).
struct StopOnDrop(Option<tokio::sync::oneshot::Sender<()>>);

impl Drop for StopOnDrop {
    fn drop(&mut self) {
        if let Some(tx) = self.0.take() {
            let _ = tx.send(());
        }
    }
}

pub fn watch(on_change: ChangeFn) -> Result<Watch> {
    let (stop_tx, stop_rx) = tokio::sync::oneshot::channel::<()>();
    let (ready_tx, ready_rx) = std::sync::mpsc::sync_channel::<Result<()>>(1);
    std::thread::Builder::new()
        .name("kotilo-media-watch".into())
        .spawn(move || {
            let rt = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(rt) => rt,
                Err(e) => {
                    let _ = ready_tx.send(Err(PlatformError::Os(format!(
                        "cannot start media watcher runtime: {e}"
                    ))));
                    return;
                }
            };
            rt.block_on(watch_signals(on_change, ready_tx, stop_rx));
        })
        .map_err(|e| PlatformError::Os(format!("cannot start media watcher: {e}")))?;
    ready_rx
        .recv()
        .map_err(|_| PlatformError::Os("media watcher exited during start".into()))??;
    Ok(Watch::new(StopOnDrop(Some(stop_tx))))
}

#[cfg(test)]
mod tests {
    use super::*;
    use zbus::zvariant::{Dict, Signature};

    fn owned(v: Value<'_>) -> OwnedValue {
        v.try_to_owned().unwrap()
    }

    /// `a{sv}` metadata the way Spotify publishes it (variants inside the dict).
    fn spotify_metadata() -> Value<'static> {
        let mut d = Dict::new(&Signature::Str, &Signature::Variant);
        let mut add = |k: &'static str, v: Value<'static>| {
            d.append(Value::from(k), Value::Value(Box::new(v))).unwrap();
        };
        add(
            "mpris:trackid",
            Value::from(ObjectPath::try_from("/com/spotify/track/4uLU6hMCjMI75M1A2tKUQC").unwrap()),
        );
        add("mpris:length", Value::U64(213_573_000));
        add(
            "mpris:artUrl",
            Value::from("https://i.scdn.co/image/ab67616d0000b273e319baafd16e84f0408af2a0"),
        );
        add("xesam:album", Value::from("Whenever You Need Somebody"));
        add("xesam:albumArtist", Value::from(vec!["Rick Astley"]));
        add("xesam:artist", Value::from(vec!["Rick Astley", " "]));
        add("xesam:autoRating", Value::F64(0.8));
        add("xesam:discNumber", Value::I32(1));
        add("xesam:title", Value::from("Never Gonna Give You Up "));
        add("xesam:trackNumber", Value::I32(1));
        add(
            "xesam:url",
            Value::from("https://open.spotify.com/track/4uLU6hMCjMI75M1A2tKUQC"),
        );
        Value::Dict(d)
    }

    fn spotify_props() -> HashMap<String, OwnedValue> {
        let mut m = HashMap::new();
        m.insert("PlaybackStatus".into(), owned(Value::from("Playing")));
        m.insert("LoopStatus".into(), owned(Value::from("Playlist")));
        m.insert("Rate".into(), owned(Value::F64(1.0)));
        m.insert("Shuffle".into(), owned(Value::Bool(true)));
        m.insert("Metadata".into(), owned(spotify_metadata()));
        m.insert("Volume".into(), owned(Value::F64(1.0)));
        m.insert("Position".into(), owned(Value::I64(42_500_000)));
        m.insert("MinimumRate".into(), owned(Value::F64(1.0)));
        m.insert("MaximumRate".into(), owned(Value::F64(1.0)));
        m.insert("CanGoNext".into(), owned(Value::Bool(true)));
        m.insert("CanGoPrevious".into(), owned(Value::Bool(true)));
        m.insert("CanPlay".into(), owned(Value::Bool(true)));
        m.insert("CanPause".into(), owned(Value::Bool(true)));
        m.insert("CanSeek".into(), owned(Value::Bool(true)));
        m.insert("CanControl".into(), owned(Value::Bool(true)));
        m
    }

    #[test]
    fn maps_spotify_properties() {
        let p = parse_player(&spotify_props());
        assert_eq!(
            p.metadata.trackid.as_deref(),
            Some("/com/spotify/track/4uLU6hMCjMI75M1A2tKUQC")
        );
        assert_eq!(
            p.metadata.length_us,
            Some(213_573_000),
            "u64 length accepted"
        );
        let s = to_session(&p, "Spotify".into(), None, 1_000);
        assert_eq!(s.title, "Never Gonna Give You Up");
        assert_eq!(s.artist, "Rick Astley", "blank artists dropped");
        assert_eq!(s.album.as_deref(), Some("Whenever You Need Somebody"));
        assert!(s.playing);
        assert_eq!(s.position, Some(42.5));
        assert_eq!(s.duration, Some(213.573));
        assert_eq!(s.position_at, 1_000);
        assert_eq!(s.shuffle, Some(true));
        assert_eq!(s.repeat, Some(RepeatMode::List));
        assert!(s.can_prev && s.can_next && s.can_play_pause);
        assert_eq!(s.source, "Spotify");
    }

    /// Firefox: no Shuffle/LoopStatus, no trackid-less length, `s` artist, CanControl only.
    #[test]
    fn maps_a_sparse_browser_player() {
        let mut d = Dict::new(&Signature::Str, &Signature::Variant);
        d.append(
            Value::from("xesam:title"),
            Value::Value(Box::new(Value::from("Lofi beats"))),
        )
        .unwrap();
        d.append(
            Value::from("xesam:artist"),
            Value::Value(Box::new(Value::from("Some Channel"))),
        )
        .unwrap();
        d.append(
            Value::from("mpris:artUrl"),
            Value::Value(Box::new(Value::from(
                "file:///home/u/.mozilla/firefox/firefox-mpris/1234_5.png",
            ))),
        )
        .unwrap();
        let mut m = HashMap::new();
        m.insert("PlaybackStatus".into(), owned(Value::from("Paused")));
        m.insert("Metadata".into(), owned(Value::Dict(d)));
        m.insert("Position".into(), owned(Value::I64(5_000_000)));
        m.insert("CanPlay".into(), owned(Value::Bool(true)));
        m.insert("CanGoNext".into(), owned(Value::Bool(true)));
        m.insert("CanControl".into(), owned(Value::Bool(false)));
        let p = parse_player(&m);
        assert_eq!(p.metadata.artists, ["Some Channel"]);
        let s = to_session(&p, "Firefox".into(), None, 7);
        assert!(!s.playing);
        assert_eq!(s.shuffle, None);
        assert_eq!(s.repeat, None);
        assert_eq!(
            (s.position, s.duration),
            (None, None),
            "no length → no timeline"
        );
        assert!(
            !s.can_next && !s.can_prev && !s.can_play_pause,
            "CanControl=false disables controls"
        );
        assert_eq!(s.album, None);
    }

    #[test]
    fn empty_or_garbage_properties_do_not_panic() {
        let p = parse_player(&HashMap::new());
        assert_eq!(p.status, "Stopped");
        assert!(p.can_control);
        let s = to_session(&p, "X".into(), None, 0);
        assert!(!s.playing && s.title.is_empty() && s.artist.is_empty());
        assert!(!s.can_play_pause);
        let mut m = HashMap::new();
        m.insert("Metadata".into(), owned(Value::from("not a dict")));
        m.insert("Shuffle".into(), owned(Value::from("yes")));
        m.insert("Position".into(), owned(Value::from("1")));
        let p = parse_player(&m);
        assert_eq!(p.metadata, Metadata::default());
        assert_eq!(p.shuffle, None);
        assert_eq!(p.position_us, None);
    }

    #[test]
    fn title_falls_back_to_the_file_name() {
        let mut p = parse_player(&HashMap::new());
        p.metadata.url = Some("file:///home/u/Music/My%20Song.flac".into());
        assert_eq!(to_session(&p, "mpv".into(), None, 0).title, "My Song.flac");
        p.metadata.url = Some("https://example.com/".into());
        assert_eq!(to_session(&p, "mpv".into(), None, 0).title, "example.com");
    }

    #[test]
    fn loop_status_maps_both_ways() {
        for r in [RepeatMode::None, RepeatMode::Track, RepeatMode::List] {
            assert_eq!(repeat_from_mpris(repeat_to_mpris(r)), Some(r));
        }
        assert_eq!(repeat_from_mpris("Playlist"), Some(RepeatMode::List));
        assert_eq!(repeat_from_mpris("playlist"), None);
        // cycle-repeat from MPRIS: None → Playlist → Track → None
        assert_eq!(repeat_to_mpris(RepeatMode::None.next()), "Playlist");
        assert_eq!(repeat_to_mpris(RepeatMode::List.next()), "Track");
        assert_eq!(repeat_to_mpris(RepeatMode::Track.next()), "None");
    }

    #[test]
    fn timeline_and_seek() {
        assert_eq!(
            timeline(Some(1_500_000), Some(200_000_000)),
            (Some(1.5), Some(200.0))
        );
        assert_eq!(
            timeline(Some(-5), Some(10_000_000)),
            (Some(0.0), Some(10.0))
        );
        assert_eq!(
            timeline(Some(99_000_000), Some(10_000_000)),
            (Some(10.0), Some(10.0))
        );
        assert_eq!(timeline(None, Some(2_000_000)), (Some(0.0), Some(2.0)));
        assert_eq!(timeline(Some(5), None), (None, None));
        assert_eq!(timeline(Some(5), Some(0)), (None, None));
        assert_eq!(seek_target_us(3.0, Some(100_000_000)), 3_000_000);
        assert_eq!(seek_target_us(999.0, Some(100_000_000)), 100_000_000);
        assert_eq!(seek_target_us(-1.0, Some(100)), 0);
        assert_eq!(seek_target_us(2.0, None), 2_000_000);
    }

    #[test]
    fn pick_rule() {
        let c = |playing, active_at| Candidate { playing, active_at };
        assert_eq!(pick(&[]), None);
        // Playing wins over a more recently paused one.
        assert_eq!(pick(&[c(false, Some(9)), c(true, Some(2))]), Some(1));
        // Several playing: the one that started last.
        assert_eq!(
            pick(&[c(true, Some(3)), c(true, Some(5)), c(false, Some(9))]),
            Some(1)
        );
        // Tie keeps list order.
        assert_eq!(pick(&[c(true, Some(1)), c(true, Some(1))]), Some(0));
        // Nothing playing: most recently active.
        assert_eq!(
            pick(&[c(false, None), c(false, Some(4)), c(false, Some(2))]),
            Some(1)
        );
        // Never active: any (the first).
        assert_eq!(pick(&[c(false, None), c(false, None)]), Some(0));
    }

    #[test]
    fn activity_tracks_start_and_stop() {
        let mut a = Activity::default();
        let s = |v: &[(&str, bool)]| -> Vec<(String, bool)> {
            v.iter().map(|(n, p)| (n.to_string(), *p)).collect()
        };
        // Spotify plays, Firefox paused (never seen active).
        let c = a.update(&s(&[("firefox", false), ("spotify", true)]));
        assert_eq!(pick(&c), Some(1));
        // Spotify pauses, then Firefox starts: Firefox.
        let c = a.update(&s(&[("firefox", false), ("spotify", false)]));
        assert_eq!(pick(&c), Some(1), "just-paused Spotify is the most recent");
        let c = a.update(&s(&[("firefox", true), ("spotify", false)]));
        assert_eq!(pick(&c), Some(0));
        // Firefox pauses: it stays (most recently active), not Spotify.
        let c = a.update(&s(&[("firefox", false), ("spotify", false)]));
        assert_eq!(pick(&c), Some(0));
        // Firefox quits: forgotten.
        let c = a.update(&s(&[("spotify", false)]));
        assert_eq!(pick(&c), Some(0));
        assert_eq!(a.seen.len(), 1);
    }

    #[test]
    fn source_names() {
        assert_eq!(pretty_source("org.mpris.MediaPlayer2.spotify"), "Spotify");
        assert_eq!(
            pretty_source("org.mpris.MediaPlayer2.firefox.instance_1_84"),
            "Firefox"
        );
        assert_eq!(
            pretty_source("org.mpris.MediaPlayer2.chromium.instance12345"),
            "Chromium"
        );
        assert_eq!(
            pretty_source("org.mpris.MediaPlayer2.strawberry"),
            "Strawberry"
        );
        assert_eq!(pretty_source("org.mpris.MediaPlayer2."), "Unknown app");
        assert!(ignored("org.mpris.MediaPlayer2.playerctld"));
    }

    #[test]
    fn artwork_sources() {
        assert_eq!(
            art_source("file:///home/u/My%20Music/cover%2B1.jpg"),
            Some(ArtSource::File(PathBuf::from(
                "/home/u/My Music/cover+1.jpg"
            )))
        );
        assert_eq!(
            art_source("file://localhost/tmp/a.png"),
            Some(ArtSource::File(PathBuf::from("/tmp/a.png")))
        );
        assert_eq!(art_source("file://host/share/a.png"), None);
        assert_eq!(
            art_source("https://i.scdn.co/image/ab67616d0000b273e319baafd16e84f0408af2a0"),
            Some(ArtSource::Http(
                "https://i.scdn.co/image/ab67616d00001e02e319baafd16e84f0408af2a0".into()
            )),
            "Spotify 640 px → 300 px"
        );
        assert_eq!(
            art_source("https://open.spotify.com/image/ab67616d0000b273abc"),
            Some(ArtSource::Http(
                "https://i.scdn.co/image/ab67616d00001e02abc".into()
            ))
        );
        assert_eq!(
            art_source("http://radio.example/cover.jpg"),
            Some(ArtSource::Http("http://radio.example/cover.jpg".into()))
        );
        assert_eq!(
            art_source("data:image/png;base64,iVBOR"),
            Some(ArtSource::Data("data:image/png;base64,iVBOR".into()))
        );
        assert_eq!(art_source("data:image/gif;base64,R0lG"), None);
        assert_eq!(art_source("ftp://x/y.png"), None);
        assert_eq!(art_source(""), None);
    }

    #[test]
    fn percent_decoding() {
        assert_eq!(percent_decode("a%20b"), b"a b");
        assert_eq!(percent_decode("%C3%BC"), "ü".as_bytes());
        assert_eq!(percent_decode("100%"), b"100%");
        assert_eq!(percent_decode("%zz%4"), b"%zz%4");
        assert_eq!(percent_decode("%41"), b"A");
    }

    #[test]
    fn file_artwork_is_capped() {
        let dir = tempfile::tempdir().unwrap();
        let small = dir.path().join("small.png");
        let mut png = vec![0x89, b'P', b'N', b'G'];
        png.resize(2048, 0);
        std::fs::write(&small, &png).unwrap();
        assert!(read_file_art(&small)
            .unwrap()
            .starts_with("data:image/png;base64,"));
        let big = dir.path().join("big.jpg");
        let mut jpeg = vec![0xFF, 0xD8, 0xFF];
        jpeg.resize(ARTWORK_MAX_BYTES + 1, 0);
        std::fs::write(&big, &jpeg).unwrap();
        assert_eq!(read_file_art(&big), None, "over the cap is dropped");
        assert_eq!(read_file_art(&dir.path().join("missing.png")), None);
        assert_eq!(
            read_file_art(dir.path()),
            None,
            "directories are not covers"
        );
    }

    #[test]
    fn match_rules_build() {
        let r = rules().unwrap();
        assert_eq!(r.len(), 3);
        assert!(r[0]
            .to_string()
            .contains("arg0='org.mpris.MediaPlayer2.Player'"));
        assert!(r[2]
            .to_string()
            .contains("arg0namespace='org.mpris.MediaPlayer2'"));
    }
}
