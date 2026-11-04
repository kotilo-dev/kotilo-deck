//! Media backend via WinRT `GlobalSystemMediaTransportControlsSessionManager` (GSMTC): the
//! session Windows shows in its media flyout (Spotify, Edge/Chrome, …).
//!
//! Reads/controls run on the `kotilo-media` MTA worker, which caches the manager and the last
//! artwork. The watcher owns its own manager + event registrations on a dedicated thread.

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::mpsc::{self, Sender};

use windows::core::{Interface, HSTRING};
use windows::ApplicationModel::AppInfo;
use windows::Foundation::TypedEventHandler;
use windows::Graphics::Imaging::{
    BitmapAlphaMode, BitmapDecoder, BitmapEncoder, BitmapInterpolationMode, BitmapPixelFormat,
};
use windows::Media::Control::{
    CurrentSessionChangedEventArgs, GlobalSystemMediaTransportControlsSession as Session,
    GlobalSystemMediaTransportControlsSessionManager as Manager,
    GlobalSystemMediaTransportControlsSessionMediaProperties as Props,
    GlobalSystemMediaTransportControlsSessionPlaybackStatus as Status,
    MediaPropertiesChangedEventArgs, PlaybackInfoChangedEventArgs, SessionsChangedEventArgs,
    TimelinePropertiesChangedEventArgs,
};
use windows::Media::MediaPlaybackAutoRepeatMode;
use windows::Storage::Streams::{
    Buffer, DataReader, IRandomAccessStream, InMemoryRandomAccessStream, InputStreamOptions,
};

use super::com::{os, ComGuard, Worker};
use crate::platform::shared::artwork_data_url;
use crate::platform::{
    ChangeFn, MediaAction, MediaSession, MediaState, PlatformError, RepeatMode, Result, Watch,
};

static WORKER: Worker = Worker::new("kotilo-media");

/// Runs `f` on the media worker.
pub fn run<T: Send + 'static>(f: impl FnOnce() -> Result<T> + Send + 'static) -> Result<T> {
    WORKER.run(f)
}

/// Longest artwork edge after downscaling.
pub const ARTWORK_MAX_EDGE: u32 = 300;
/// Thumbnails bigger than this are not even read.
const ARTWORK_MAX_INPUT: u64 = 16 * 1024 * 1024;

const TICKS_PER_SEC: f64 = 10_000_000.0;
/// 1601-01-01 → 1970-01-01 in 100 ns ticks (WinRT `DateTime` epoch).
const UNIX_EPOCH_TICKS: i64 = 116_444_736_000_000_000;

thread_local! {
    static MANAGER: RefCell<Option<Manager>> = const { RefCell::new(None) };
    /// (track key, data URL) of the last artwork, so timeline/playback events don't re-encode it.
    static ARTWORK: RefCell<Option<(String, String)>> = const { RefCell::new(None) };
    static SOURCE_NAMES: RefCell<HashMap<String, String>> = RefCell::new(HashMap::new());
}

fn request_manager() -> Result<Manager> {
    Manager::RequestAsync()
        .and_then(|op| op.join())
        .map_err(|e| PlatformError::Os(format!("media sessions unavailable: {}", e.message())))
}

fn manager() -> Result<Manager> {
    MANAGER.with(|cell| {
        let mut cell = cell.borrow_mut();
        if let Some(m) = cell.as_ref() {
            return Ok(m.clone());
        }
        let m = request_manager()?;
        *cell = Some(m.clone());
        Ok(m)
    })
}

fn is_playing(s: &Session) -> bool {
    s.GetPlaybackInfo()
        .and_then(|i| i.PlaybackStatus())
        .is_ok_and(|st| st == Status::Playing)
}

/// Windows' current session if it is playing, else the first playing one, else the current
/// (paused) one, else any. Avoids showing a paused tab while Spotify plays.
fn pick_session(m: &Manager) -> Option<Session> {
    let current = m.GetCurrentSession().ok();
    if current.as_ref().is_some_and(is_playing) {
        return current;
    }
    let all: Vec<Session> = m
        .GetSessions()
        .map(|list| {
            let n = list.Size().unwrap_or(0);
            (0..n).filter_map(|i| list.GetAt(i).ok()).collect()
        })
        .unwrap_or_default();
    if let Some(s) = all.iter().find(|s| is_playing(s)) {
        return Some(s.clone());
    }
    current.or_else(|| all.into_iter().next())
}

pub fn repeat_from_winrt(m: MediaPlaybackAutoRepeatMode) -> Option<RepeatMode> {
    match m {
        MediaPlaybackAutoRepeatMode::None => Some(RepeatMode::None),
        MediaPlaybackAutoRepeatMode::Track => Some(RepeatMode::Track),
        MediaPlaybackAutoRepeatMode::List => Some(RepeatMode::List),
        _ => None,
    }
}

pub fn repeat_to_winrt(r: RepeatMode) -> MediaPlaybackAutoRepeatMode {
    match r {
        RepeatMode::None => MediaPlaybackAutoRepeatMode::None,
        RepeatMode::Track => MediaPlaybackAutoRepeatMode::Track,
        RepeatMode::List => MediaPlaybackAutoRepeatMode::List,
    }
}

/// (position, duration) in seconds from GSMTC ticks; both `None` without a usable timeline.
pub fn timeline(start: i64, end: i64, position: i64) -> (Option<f64>, Option<f64>) {
    let dur = end.saturating_sub(start);
    if dur <= 0 {
        return (None, None);
    }
    let pos = position.saturating_sub(start).clamp(0, dur);
    (
        Some(pos as f64 / TICKS_PER_SEC),
        Some(dur as f64 / TICKS_PER_SEC),
    )
}

/// WinRT `DateTime` ticks → Unix ms; `None` for unset/pre-1970 values.
pub fn ticks_to_unix_ms(ticks: i64) -> Option<i64> {
    (ticks > UNIX_EPOCH_TICKS).then(|| (ticks - UNIX_EPOCH_TICKS) / 10_000)
}

/// Absolute seek target in ticks for `seconds` into a track spanning `start..end`.
pub fn seek_ticks(start: i64, end: i64, seconds: f64) -> i64 {
    let target = start.saturating_add((seconds.max(0.0) * TICKS_PER_SEC) as i64);
    if end > start {
        target.clamp(start, end)
    } else {
        target.max(start)
    }
}

/// Readable app name from an AppUserModelID when `AppInfo` can't resolve it:
/// `Spotify.exe` → "Spotify", `MSEdge` → "Microsoft Edge", `Pkg.Name_hash!App` → "Name".
pub fn pretty_source(aumid: &str) -> String {
    let (family, app) = match aumid.split_once('!') {
        Some((f, a)) => (f, a),
        None => ("", aumid),
    };
    let mut base = app.rsplit(['\\', '/']).next().unwrap_or(app);
    // `get` (not indexing) so a non-ASCII name can't panic on a char boundary.
    if let Some(stem) = base
        .len()
        .checked_sub(4)
        .filter(|&i| {
            i > 0
                && base
                    .get(i..)
                    .is_some_and(|ext| ext.eq_ignore_ascii_case(".exe"))
        })
        .and_then(|i| base.get(..i))
    {
        base = stem;
    }
    if base.eq_ignore_ascii_case("app") && !family.is_empty() {
        base = family.split('_').next().unwrap_or(family);
    }
    // `Microsoft.ZuneMusic` → `ZuneMusic`
    base = base.rsplit('.').next().unwrap_or(base);
    let known = match base.to_ascii_lowercase().as_str() {
        "msedge" => Some("Microsoft Edge"),
        "chrome" => Some("Google Chrome"),
        "firefox" => Some("Firefox"),
        "spotify" => Some("Spotify"),
        "brave" => Some("Brave"),
        "opera" => Some("Opera"),
        "vlc" => Some("VLC media player"),
        "zunemusic" => Some("Media Player"),
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

fn source_name(aumid: &str) -> String {
    if let Some(n) = SOURCE_NAMES.with(|m| m.borrow().get(aumid).cloned()) {
        return n;
    }
    let resolved = AppInfo::GetFromAppUserModelId(&HSTRING::from(aumid))
        .and_then(|a| a.DisplayInfo())
        .and_then(|d| d.DisplayName())
        .map(|h| h.to_string())
        .ok()
        .filter(|s| !s.trim().is_empty());
    let name = resolved.unwrap_or_else(|| pretty_source(aumid));
    SOURCE_NAMES.with(|m| m.borrow_mut().insert(aumid.to_string(), name.clone()));
    name
}

// ── Artwork ─────────────────────────────────────────────────────────────────

/// Longest edge scaled down to `max` keeping aspect; never upscales; never 0.
pub fn fit_within(w: u32, h: u32, max: u32) -> (u32, u32) {
    if w == 0 || h == 0 || (w <= max && h <= max) {
        return (w.max(1), h.max(1));
    }
    let scale = max as f64 / w.max(h) as f64;
    (
        ((w as f64 * scale).round() as u32).clamp(1, max),
        ((h as f64 * scale).round() as u32).clamp(1, max),
    )
}

fn read_all(rs: &IRandomAccessStream) -> windows::core::Result<Vec<u8>> {
    let size = rs.Size()?.min(ARTWORK_MAX_INPUT) as u32;
    rs.Seek(0)?;
    let buf = Buffer::Create(size)?;
    let ib = rs.ReadAsync(&buf, size, InputStreamOptions::None)?.join()?;
    let mut v = vec![0u8; ib.Length()? as usize];
    DataReader::FromBuffer(&ib)?.ReadBytes(&mut v)?;
    Ok(v)
}

/// Decodes any WIC-supported image and re-encodes it as a ≤ ARTWORK_MAX_EDGE px JPEG.
fn transcode(rs: &IRandomAccessStream) -> windows::core::Result<Vec<u8>> {
    rs.Seek(0)?;
    let dec = BitmapDecoder::CreateAsync(rs)?.join()?;
    let (w, h) = fit_within(dec.PixelWidth()?, dec.PixelHeight()?, ARTWORK_MAX_EDGE);
    let bmp = dec
        .GetSoftwareBitmapConvertedAsync(BitmapPixelFormat::Bgra8, BitmapAlphaMode::Ignore)?
        .join()?;
    let out = InMemoryRandomAccessStream::new()?;
    let enc = BitmapEncoder::CreateAsync(BitmapEncoder::JpegEncoderId()?, &out)?.join()?;
    enc.SetSoftwareBitmap(&bmp)?;
    let t = enc.BitmapTransform()?;
    t.SetScaledWidth(w)?;
    t.SetScaledHeight(h)?;
    t.SetInterpolationMode(BitmapInterpolationMode::Fant)?;
    enc.FlushAsync()?.join()?;
    read_all(&out.cast::<IRandomAccessStream>()?)
}

fn load_artwork(props: &Props) -> Option<String> {
    let stream = props.Thumbnail().ok()?.OpenReadAsync().ok()?.join().ok()?;
    let rs: IRandomAccessStream = stream.cast().ok()?;
    let size = rs.Size().ok()?;
    if size == 0 || size > ARTWORK_MAX_INPUT {
        return None;
    }
    let bytes = read_all(&rs).ok()?;
    // Small png/jpeg (Spotify's 300 px covers) pass through untouched; everything else is
    // downscaled to JPEG.
    if let Some(url) = artwork_data_url(&bytes) {
        return Some(url);
    }
    match transcode(&rs) {
        Ok(jpeg) => artwork_data_url(&jpeg),
        Err(e) => {
            tracing::debug!("artwork transcode failed: {}", e.message());
            None
        }
    }
}

fn artwork(key: &str, props: &Props) -> Option<String> {
    if let Some(url) = ARTWORK.with(|a| {
        a.borrow()
            .as_ref()
            .filter(|(k, _)| k == key)
            .map(|(_, u)| u.clone())
    }) {
        return Some(url);
    }
    // Only successes are cached: apps often publish the thumbnail a moment after the title.
    let url = load_artwork(props)?;
    ARTWORK.with(|a| *a.borrow_mut() = Some((key.to_string(), url.clone())));
    Some(url)
}

// ── State & controls ────────────────────────────────────────────────────────

fn read_session(s: &Session) -> Result<MediaSession> {
    let aumid = s.SourceAppUserModelId().map_err(os)?.to_string();
    let props = s.TryGetMediaPropertiesAsync().and_then(|op| op.join()).ok();
    let text = |f: fn(&Props) -> windows::core::Result<HSTRING>| {
        props
            .as_ref()
            .and_then(|p| f(p).ok())
            .map(|h| h.to_string().trim().to_string())
            .unwrap_or_default()
    };
    let title = text(Props::Title);
    let artist = text(Props::Artist);
    let album = Some(text(Props::AlbumTitle)).filter(|a| !a.is_empty());
    let key = format!(
        "{aumid}\u{1}{title}\u{1}{artist}\u{1}{}",
        album.as_deref().unwrap_or("")
    );
    let artwork = props.as_ref().and_then(|p| artwork(&key, p));

    let info = s.GetPlaybackInfo().map_err(os)?;
    let playing = info.PlaybackStatus().is_ok_and(|st| st == Status::Playing);
    let shuffle = info.IsShuffleActive().and_then(|r| r.Value()).ok();
    let repeat = info
        .AutoRepeatMode()
        .and_then(|r| r.Value())
        .ok()
        .and_then(repeat_from_winrt);
    let controls = info.Controls().ok();
    let flag = |f: fn(
        &windows::Media::Control::GlobalSystemMediaTransportControlsSessionPlaybackControls,
    ) -> windows::core::Result<bool>| {
        controls.as_ref().is_some_and(|c| f(c).unwrap_or(false))
    };
    let can_play_pause = flag(|c| c.IsPlayPauseToggleEnabled())
        || flag(|c| c.IsPlayEnabled())
        || flag(|c| c.IsPauseEnabled());

    let now = chrono::Utc::now().timestamp_millis();
    let (position, duration, position_at) = match s.GetTimelineProperties() {
        Ok(t) => {
            let ticks = |r: windows::core::Result<windows::Foundation::TimeSpan>| {
                r.map(|v| v.Duration).unwrap_or(0)
            };
            let (pos, dur) = timeline(
                ticks(t.StartTime()),
                ticks(t.EndTime()),
                ticks(t.Position()),
            );
            let at = t
                .LastUpdatedTime()
                .ok()
                .and_then(|d| ticks_to_unix_ms(d.UniversalTime))
                .filter(|&ms| ms <= now + 1_000)
                .unwrap_or(now);
            (pos, dur, if pos.is_some() { at } else { now })
        }
        Err(_) => (None, None, now),
    };

    Ok(MediaSession {
        source: source_name(&aumid),
        title,
        artist,
        album,
        artwork,
        playing,
        position,
        duration,
        position_at,
        shuffle,
        repeat,
        can_prev: flag(|c| c.IsPreviousEnabled()),
        can_next: flag(|c| c.IsNextEnabled()),
        can_play_pause,
    })
}

pub fn state() -> Result<MediaState> {
    let m = manager()?;
    let session = match pick_session(&m) {
        Some(s) => Some(read_session(&s)?),
        None => None,
    };
    Ok(MediaState { session })
}

fn active() -> Result<Session> {
    pick_session(&manager()?).ok_or_else(|| PlatformError::Os("nothing is playing".into()))
}

fn accepted(op: windows::core::Result<bool>, what: &str) -> Result<()> {
    match op {
        Ok(true) => Ok(()),
        Ok(false) => Err(PlatformError::Os(format!("the media app ignored {what}"))),
        Err(e) => Err(os(e)),
    }
}

pub fn control(action: MediaAction) -> Result<()> {
    let s = active()?;
    let info = s.GetPlaybackInfo().map_err(os)?;
    match action {
        MediaAction::PlayPause => {
            let toggle = info
                .Controls()
                .and_then(|c| c.IsPlayPauseToggleEnabled())
                .unwrap_or(false);
            let op = if toggle {
                s.TryTogglePlayPauseAsync()
            } else if is_playing(&s) {
                s.TryPauseAsync()
            } else {
                s.TryPlayAsync()
            };
            accepted(op.and_then(|o| o.join()), "play/pause")
        }
        MediaAction::Next => accepted(s.TrySkipNextAsync().and_then(|o| o.join()), "next"),
        MediaAction::Prev => accepted(s.TrySkipPreviousAsync().and_then(|o| o.join()), "previous"),
        MediaAction::ToggleShuffle => {
            let cur = info
                .IsShuffleActive()
                .and_then(|r| r.Value())
                .map_err(|_| PlatformError::Os("this app has no shuffle".into()))?;
            accepted(
                s.TryChangeShuffleActiveAsync(!cur).and_then(|o| o.join()),
                "shuffle",
            )
        }
        MediaAction::CycleRepeat => {
            let cur = info
                .AutoRepeatMode()
                .and_then(|r| r.Value())
                .ok()
                .and_then(repeat_from_winrt)
                .ok_or_else(|| PlatformError::Os("this app has no repeat".into()))?;
            accepted(
                s.TryChangeAutoRepeatModeAsync(repeat_to_winrt(cur.next()))
                    .and_then(|o| o.join()),
                "repeat",
            )
        }
    }
}

pub fn seek(seconds: f64) -> Result<()> {
    let s = active()?;
    let t = s.GetTimelineProperties().map_err(os)?;
    let start = t.StartTime().map(|v| v.Duration).unwrap_or(0);
    let end = t.EndTime().map(|v| v.Duration).unwrap_or(0);
    accepted(
        s.TryChangePlaybackPositionAsync(seek_ticks(start, end, seconds))
            .and_then(|o| o.join()),
        "seek",
    )
}

// ── Change notifications ────────────────────────────────────────────────────

enum Msg {
    /// Sessions added/removed or the current one switched: re-attach per-session handlers.
    Rebind,
    Stop,
}

struct Bound {
    session: Session,
    playback: i64,
    props: i64,
    timeline: i64,
}

fn bind_all(m: &Manager, on_change: &ChangeFn) -> Vec<Bound> {
    let Ok(list) = m.GetSessions() else {
        return Vec::new();
    };
    let n = list.Size().unwrap_or(0);
    let mut out = Vec::new();
    for i in 0..n {
        let Ok(session) = list.GetAt(i) else { continue };
        let (a, b, c) = (on_change.clone(), on_change.clone(), on_change.clone());
        let playback = session.PlaybackInfoChanged(&TypedEventHandler::<
            Session,
            PlaybackInfoChangedEventArgs,
        >::new(move |_, _| {
            a();
            Ok(())
        }));
        let props = session.MediaPropertiesChanged(&TypedEventHandler::<
            Session,
            MediaPropertiesChangedEventArgs,
        >::new(move |_, _| {
            b();
            Ok(())
        }));
        let timeline = session.TimelinePropertiesChanged(&TypedEventHandler::<
            Session,
            TimelinePropertiesChangedEventArgs,
        >::new(move |_, _| {
            c();
            Ok(())
        }));
        let bound = Bound {
            playback: playback.unwrap_or(0),
            props: props.unwrap_or(0),
            timeline: timeline.unwrap_or(0),
            session,
        };
        out.push(bound);
    }
    out
}

fn unbind_all(bound: &mut Vec<Bound>) {
    for b in bound.drain(..) {
        if b.playback != 0 {
            let _ = b.session.RemovePlaybackInfoChanged(b.playback);
        }
        if b.props != 0 {
            let _ = b.session.RemoveMediaPropertiesChanged(b.props);
        }
        if b.timeline != 0 {
            let _ = b.session.RemoveTimelinePropertiesChanged(b.timeline);
        }
    }
}

struct StopOnDrop(Sender<Msg>);

impl Drop for StopOnDrop {
    fn drop(&mut self) {
        let _ = self.0.send(Msg::Stop);
    }
}

pub fn watch(on_change: ChangeFn) -> Result<Watch> {
    let (tx, rx) = mpsc::channel::<Msg>();
    let (ready_tx, ready_rx) = mpsc::sync_channel::<Result<()>>(1);
    let thread_tx = tx.clone();
    std::thread::Builder::new()
        .name("kotilo-media-watch".into())
        .spawn(move || {
            let _com = match ComGuard::init() {
                Ok(g) => g,
                Err(e) => {
                    let _ = ready_tx.send(Err(e));
                    return;
                }
            };
            let setup = request_manager().and_then(|m| {
                let (t1, t2) = (thread_tx.clone(), thread_tx.clone());
                let sessions = m
                    .SessionsChanged(
                        &TypedEventHandler::<Manager, SessionsChangedEventArgs>::new(
                            move |_, _| {
                                let _ = t1.send(Msg::Rebind);
                                Ok(())
                            },
                        ),
                    )
                    .map_err(os)?;
                let current = m.CurrentSessionChanged(&TypedEventHandler::<
                    Manager,
                    CurrentSessionChangedEventArgs,
                >::new(move |_, _| {
                    let _ = t2.send(Msg::Rebind);
                    Ok(())
                }));
                let current = match current {
                    Ok(t) => t,
                    Err(e) => {
                        let _ = m.RemoveSessionsChanged(sessions);
                        return Err(os(e));
                    }
                };
                Ok((m, sessions, current))
            });
            let (m, sessions_tok, current_tok) = match setup {
                Ok(v) => {
                    let _ = ready_tx.send(Ok(()));
                    v
                }
                Err(e) => {
                    let _ = ready_tx.send(Err(e));
                    return;
                }
            };
            tracing::debug!("media watcher started");
            let mut bound = bind_all(&m, &on_change);
            while let Ok(msg) = rx.recv() {
                match msg {
                    Msg::Stop => break,
                    Msg::Rebind => {
                        unbind_all(&mut bound);
                        bound = bind_all(&m, &on_change);
                        on_change();
                    }
                }
            }
            unbind_all(&mut bound);
            let _ = m.RemoveSessionsChanged(sessions_tok);
            let _ = m.RemoveCurrentSessionChanged(current_tok);
            tracing::debug!("media watcher stopped");
        })
        .map_err(|e| PlatformError::Os(format!("cannot start media watcher: {e}")))?;
    ready_rx
        .recv()
        .map_err(|_| PlatformError::Os("media watcher exited during start".into()))??;
    Ok(Watch::new(StopOnDrop(tx)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    #[test]
    fn repeat_maps_both_ways() {
        for r in [RepeatMode::None, RepeatMode::Track, RepeatMode::List] {
            assert_eq!(repeat_from_winrt(repeat_to_winrt(r)), Some(r));
        }
        assert_eq!(
            repeat_from_winrt(MediaPlaybackAutoRepeatMode::List),
            Some(RepeatMode::List)
        );
        assert_eq!(repeat_from_winrt(MediaPlaybackAutoRepeatMode(7)), None);
    }

    #[test]
    fn timeline_converts_ticks() {
        assert_eq!(
            timeline(0, 2_000_000_000, 15_000_000),
            (Some(1.5), Some(200.0))
        );
        // Offset start, position past the end is clamped.
        assert_eq!(timeline(10, 20_000_010, 99_999_999), (Some(2.0), Some(2.0)));
        assert_eq!(timeline(0, 0, 5), (None, None));
        assert_eq!(timeline(5, 1, 3), (None, None));
    }

    #[test]
    fn datetime_ticks_to_unix_ms() {
        assert_eq!(ticks_to_unix_ms(0), None);
        assert_eq!(ticks_to_unix_ms(UNIX_EPOCH_TICKS + 10_000), Some(1));
        // 2024-01-01T00:00:00Z
        assert_eq!(
            ticks_to_unix_ms(UNIX_EPOCH_TICKS + 1_704_067_200_000 * 10_000),
            Some(1_704_067_200_000)
        );
    }

    #[test]
    fn seek_is_clamped_to_track() {
        assert_eq!(seek_ticks(0, 100_000_000, 3.0), 30_000_000);
        assert_eq!(seek_ticks(0, 100_000_000, 99.0), 100_000_000);
        assert_eq!(seek_ticks(5, 100_000_000, -4.0), 5);
        assert_eq!(seek_ticks(0, 0, 2.0), 20_000_000);
    }

    #[test]
    fn pretty_source_names() {
        assert_eq!(pretty_source("Spotify.exe"), "Spotify");
        assert_eq!(pretty_source("MSEdge"), "Microsoft Edge");
        assert_eq!(pretty_source("Chrome"), "Google Chrome");
        assert_eq!(
            pretty_source("SpotifyAB.SpotifyMusic_zpdnekdrzrea0!Spotify"),
            "Spotify"
        );
        assert_eq!(
            pretty_source("Microsoft.ZuneMusic_8wekyb3d8bbwe!Microsoft.ZuneMusic"),
            "Media Player"
        );
        assert_eq!(pretty_source("Vendor.CoolPlayer_abc!App"), "CoolPlayer");
        assert_eq!(pretty_source(r"C:\Apps\foobar2000.exe"), "Foobar2000");
        assert_eq!(pretty_source(""), "Unknown app");
        assert_eq!(
            pretty_source("Müzikçalar"),
            "Müzikçalar",
            "non-ASCII never panics"
        );
        assert_eq!(pretty_source("çç"), "Çç");
        assert_eq!(pretty_source(".exe"), "Exe");
    }

    #[test]
    fn fit_within_keeps_aspect_and_never_upscales() {
        assert_eq!(fit_within(1280, 720, 300), (300, 169));
        assert_eq!(fit_within(720, 1280, 300), (169, 300));
        assert_eq!(fit_within(200, 100, 300), (200, 100));
        assert_eq!(fit_within(5000, 1, 300), (300, 1));
        assert_eq!(fit_within(0, 0, 300), (1, 1));
    }

    #[test]
    #[ignore = "reads the real media session (read-only)"]
    fn real_media_state() {
        let s = run(state).expect("media state");
        match &s.session {
            None => println!("no media session"),
            Some(m) => {
                println!(
                    "{} — {:?} by {:?} (album {:?}); playing {}; pos {:?}/{:?}; shuffle {:?}; repeat {:?}; prev {} next {} pp {}; artwork {} bytes",
                    m.source,
                    m.title,
                    m.artist,
                    m.album,
                    m.playing,
                    m.position,
                    m.duration,
                    m.shuffle,
                    m.repeat,
                    m.can_prev,
                    m.can_next,
                    m.can_play_pause,
                    m.artwork.as_ref().map_or(0, |a| a.len())
                );
                if let Some(a) = &m.artwork {
                    assert!(a.len() <= 205 * 1024);
                }
            }
        }
        let list = run(|| {
            let m = manager()?;
            let l = m.GetSessions().map_err(os)?;
            let mut v = Vec::new();
            for i in 0..l.Size().map_err(os)? {
                let s = l.GetAt(i).map_err(os)?;
                let id = s.SourceAppUserModelId().map_err(os)?.to_string();
                v.push((source_name(&id), id, is_playing(&s)));
            }
            Ok(v)
        })
        .expect("sessions");
        for (name, id, playing) in list {
            println!("  session {name:?} ({id}) playing={playing}");
        }
    }

    #[test]
    #[ignore = "registers real GSMTC event handlers (read-only)"]
    fn real_media_watch_starts_and_stops() {
        let hits = Arc::new(AtomicUsize::new(0));
        let h = hits.clone();
        let w = watch(Arc::new(move || {
            h.fetch_add(1, Ordering::Relaxed);
        }))
        .expect("watch");
        std::thread::sleep(std::time::Duration::from_millis(1500));
        drop(w);
        println!(
            "media change callbacks in 1.5 s: {}",
            hits.load(Ordering::Relaxed)
        );
    }
}
