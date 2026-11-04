//! Phase 4 auto-update via `tauri-plugin-updater` (ARCHITECTURE.md "Auto-update").
//!
//! Backend-owned: checks at startup and every 24 h when `updates.autoCheck`, never
//! installs without the user clicking Install. State is pushed as `update://status`.
//!
//! The state machine ([`Engine`]) is independent of the network: it talks to a
//! [`Backend`], which is `tauri-plugin-updater` in the app and a fake in the tests.
//!
//! Owner: rust-platform (plugins + updater worktree).

use std::future::Future;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::async_runtime::JoinHandle;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_updater::UpdaterExt;

use crate::settings::UpdateSettings;

pub const EVENT_UPDATE_STATUS: &str = "update://status";
/// First automatic check after startup (keeps launch quiet).
const STARTUP_DELAY: Duration = Duration::from_secs(30);
const CHECK_INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);
/// At most ~5 progress events per second while downloading.
const PROGRESS_EVERY: Duration = Duration::from_millis(200);
const MAX_ERROR_CHARS: usize = 160;

/// Mirrors `UpdateState` in `src/lib/ipc/index.ts`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum UpdateState {
    #[default]
    Idle,
    Checking,
    Available,
    Downloading,
    /// Installed; relaunch pending (Windows installers quit the app themselves).
    Ready,
    Error,
    /// Dev build or no updater configured.
    Unsupported,
}

/// Mirrors `UpdateInfo`.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInfo {
    pub version: String,
    /// RFC 3339 publish date from the manifest.
    pub date: Option<String>,
    pub notes: Option<String>,
}

/// Mirrors `UpdateStatus`.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateStatus {
    pub current: String,
    pub state: UpdateState,
    pub available: Option<UpdateInfo>,
    /// 0–1 while downloading.
    pub progress: Option<f64>,
    pub error: Option<String>,
    /// Unix ms of the last finished check.
    pub last_checked: Option<i64>,
}

/// Download progress callback: `(bytes in this chunk, total length if known)`.
pub type ProgressFn<'a> = Box<dyn FnMut(u64, Option<u64>) + Send + 'a>;

/// The network side of updating.
pub trait Backend: Send + Sync + 'static {
    /// `Ok(None)` = up to date.
    fn check(&self) -> impl Future<Output = Result<Option<UpdateInfo>, String>> + Send;
    /// Downloads and verifies the update found by the last successful `check`.
    fn download<'a>(
        &'a self,
        on_progress: ProgressFn<'a>,
    ) -> impl Future<Output = Result<Vec<u8>, String>> + Send + 'a;
    /// Installs the downloaded package and relaunches; may not return (Windows installers
    /// exit the app).
    fn install(&self, bytes: Vec<u8>) -> Result<(), String>;
}

type EmitFn = Box<dyn Fn(&UpdateStatus) + Send + Sync>;

/// Update state machine. Every transition is emitted.
pub struct Engine<B> {
    status: Mutex<UpdateStatus>,
    backend: B,
    emit: EmitFn,
}

fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

fn short(msg: &str) -> String {
    let mut s: String = msg.chars().take(MAX_ERROR_CHARS).collect();
    if msg.chars().count() > MAX_ERROR_CHARS {
        s.push('…');
    }
    s
}

impl<B: Backend> Engine<B> {
    /// `supported == false` (debug builds) pins the state to `unsupported`; the backend is
    /// never called.
    pub fn new(backend: B, current: String, supported: bool, emit: EmitFn) -> Self {
        let state = if supported {
            UpdateState::Idle
        } else {
            UpdateState::Unsupported
        };
        Self {
            status: Mutex::new(UpdateStatus {
                current,
                state,
                ..Default::default()
            }),
            backend,
            emit,
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, UpdateStatus> {
        self.status.lock().unwrap_or_else(|p| p.into_inner())
    }

    pub fn status(&self) -> UpdateStatus {
        self.lock().clone()
    }

    /// Applies `f` and emits the result.
    fn transition(&self, f: impl FnOnce(&mut UpdateStatus)) -> UpdateStatus {
        let snapshot = {
            let mut s = self.lock();
            f(&mut s);
            s.clone()
        };
        (self.emit)(&snapshot);
        snapshot
    }

    fn busy(state: UpdateState) -> bool {
        matches!(
            state,
            UpdateState::Checking
                | UpdateState::Downloading
                | UpdateState::Ready
                | UpdateState::Unsupported
        )
    }

    /// Checks now. A check while busy (or unsupported) just returns the current status.
    pub async fn check(&self) -> UpdateStatus {
        {
            let s = self.lock();
            if Self::busy(s.state) {
                return s.clone();
            }
        }
        self.transition(|s| {
            s.state = UpdateState::Checking;
            s.error = None;
            s.progress = None;
        });
        let result = self.backend.check().await;
        self.transition(|s| {
            s.last_checked = Some(now_ms());
            s.progress = None;
            match result {
                Ok(Some(info)) => {
                    tracing::info!(version = %info.version, "update available");
                    s.state = UpdateState::Available;
                    s.available = Some(info);
                    s.error = None;
                }
                Ok(None) => {
                    tracing::info!("no update available");
                    s.state = UpdateState::Idle;
                    s.available = None;
                    s.error = None;
                }
                Err(e) => {
                    tracing::warn!("update check failed: {e}");
                    s.state = UpdateState::Error;
                    s.available = None;
                    s.error = Some(short(&e));
                }
            }
        })
    }

    /// Downloads (with throttled progress), then installs and relaunches.
    pub async fn install(&self) -> Result<(), String> {
        {
            let s = self.lock();
            if s.state == UpdateState::Unsupported {
                return Err("updates are not available in this build".into());
            }
            if Self::busy(s.state) {
                return Err("an update is already in progress".into());
            }
            if s.available.is_none() {
                return Err("no update available".into());
            }
        }
        self.transition(|s| {
            s.state = UpdateState::Downloading;
            s.progress = Some(0.0);
            s.error = None;
        });

        let mut downloaded = 0u64;
        let mut last_emit: Option<Instant> = None;
        let on_progress: ProgressFn<'_> = Box::new(|chunk, total| {
            downloaded += chunk;
            let Some(total) = total.filter(|t| *t > 0) else {
                return;
            };
            let done = downloaded >= total;
            if !done && last_emit.is_some_and(|t| t.elapsed() < PROGRESS_EVERY) {
                return;
            }
            last_emit = Some(Instant::now());
            let p = (downloaded as f64 / total as f64).clamp(0.0, 1.0);
            self.transition(|s| s.progress = Some(p));
        });
        let bytes = match self.backend.download(on_progress).await {
            Ok(b) => b,
            Err(e) => {
                tracing::warn!("update download failed: {e}");
                let msg = short(&e);
                // `available` stays so the user can retry.
                self.transition(|s| {
                    s.state = UpdateState::Error;
                    s.progress = None;
                    s.error = Some(msg.clone());
                });
                return Err(msg);
            }
        };
        self.transition(|s| {
            s.state = UpdateState::Ready;
            s.progress = Some(1.0);
        });
        tracing::info!("update downloaded; installing");
        if let Err(e) = self.backend.install(bytes) {
            tracing::warn!("update install failed: {e}");
            let msg = short(&e);
            self.transition(|s| {
                s.state = UpdateState::Error;
                s.progress = None;
                s.error = Some(msg.clone());
            });
            return Err(msg);
        }
        Ok(())
    }
}

/// Short, URL-free message for an updater error.
fn describe(e: &tauri_plugin_updater::Error, during: &str) -> String {
    use tauri_plugin_updater::Error as E;
    match e {
        E::ReleaseNotFound => "no release information is published yet".into(),
        E::Reqwest(r) if r.is_timeout() => format!("the update server timed out ({during})"),
        E::Reqwest(r) if r.is_connect() => {
            format!("cannot reach the update server ({during})")
        }
        E::Reqwest(_) | E::Network(_) => format!("network error ({during})"),
        E::Minisign(_) | E::SignedVersionMismatch { .. } | E::MissingSignedVersion => {
            "the update signature is invalid".into()
        }
        E::TargetNotFound(_) | E::TargetsNotFound(_) => {
            "no update is published for this platform".into()
        }
        E::Serialization(_) => "the update manifest is malformed".into(),
        other => format!("{during} failed: {other}"),
    }
}

/// `tauri-plugin-updater` (endpoint + pubkey from `tauri.conf.json`).
pub struct TauriBackend {
    app: AppHandle,
    pending: Mutex<Option<tauri_plugin_updater::Update>>,
}

impl TauriBackend {
    fn pending(&self) -> Option<tauri_plugin_updater::Update> {
        self.pending
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
    }
}

impl Backend for TauriBackend {
    async fn check(&self) -> Result<Option<UpdateInfo>, String> {
        let updater = self
            .app
            .updater()
            .map_err(|e| describe(&e, "update check"))?;
        let found = updater
            .check()
            .await
            .map_err(|e| describe(&e, "update check"))?;
        let info = found.as_ref().map(|u| UpdateInfo {
            version: u.version.clone(),
            // The manifest's own string; `Update::date` would need the `time` crate.
            date: u
                .raw_json
                .get("pub_date")
                .and_then(|v| v.as_str())
                .map(str::to_string),
            notes: u.body.clone().filter(|b| !b.trim().is_empty()),
        });
        *self.pending.lock().unwrap_or_else(|p| p.into_inner()) = found;
        Ok(info)
    }

    async fn download<'a>(&'a self, mut on_progress: ProgressFn<'a>) -> Result<Vec<u8>, String> {
        let update = self.pending().ok_or("no update available")?;
        update
            .download(|chunk, total| on_progress(chunk as u64, total), || {})
            .await
            .map_err(|e| describe(&e, "download"))
    }

    fn install(&self, bytes: Vec<u8>) -> Result<(), String> {
        let update = self.pending().ok_or("no update available")?;
        update.install(bytes).map_err(|e| describe(&e, "install"))?;
        // Windows: the installer already quit the app and relaunches it itself.
        self.app.restart()
    }
}

/// Managed updater state: the engine plus the periodic-check task.
pub struct UpdateHub {
    engine: Engine<TauriBackend>,
    schedule: Mutex<Option<JoinHandle<()>>>,
}

fn hub(app: &AppHandle) -> Option<tauri::State<'_, UpdateHub>> {
    app.try_state::<UpdateHub>()
}

/// Delay before the next automatic check: 30 s after startup, otherwise what is left of
/// 24 h since the last check.
fn next_delay(last_checked: Option<i64>, now: i64, startup: bool) -> Duration {
    if startup {
        return STARTUP_DELAY;
    }
    match last_checked {
        Some(t) => {
            let since = Duration::from_millis(now.saturating_sub(t).max(0) as u64);
            CHECK_INTERVAL.saturating_sub(since).max(STARTUP_DELAY)
        }
        None => STARTUP_DELAY,
    }
}

fn reschedule(app: &AppHandle, updates: &UpdateSettings, startup: bool) {
    let Some(state) = hub(app) else { return };
    let mut slot = state.schedule.lock().unwrap_or_else(|p| p.into_inner());
    if let Some(task) = slot.take() {
        task.abort();
    }
    let status = state.engine.status();
    if !updates.auto_check || status.state == UpdateState::Unsupported {
        return;
    }
    let delay = next_delay(status.last_checked, now_ms(), startup);
    let app = app.clone();
    *slot = Some(tauri::async_runtime::spawn(async move {
        tokio::time::sleep(delay).await;
        loop {
            if let Some(hub) = hub(&app) {
                hub.engine.check().await;
            }
            tokio::time::sleep(CHECK_INTERVAL).await;
        }
    }));
}

/// Startup: schedules the periodic check when `updates.autoCheck`.
pub fn start(app: &AppHandle, updates: &UpdateSettings) {
    let supported = !cfg!(debug_assertions);
    let emitter = app.clone();
    let engine = Engine::new(
        TauriBackend {
            app: app.clone(),
            pending: Mutex::new(None),
        },
        app.package_info().version.to_string(),
        supported,
        Box::new(move |s| {
            let _ = emitter.emit(EVENT_UPDATE_STATUS, s);
        }),
    );
    app.manage(UpdateHub {
        engine,
        schedule: Mutex::new(None),
    });
    if !supported {
        tracing::info!("auto-update disabled in debug builds");
    }
    reschedule(app, updates, true);
}

/// Settings hook: `updates` changed.
pub fn settings_changed(app: &AppHandle, updates: &UpdateSettings) {
    reschedule(app, updates, false);
}

fn fallback_status(app: &AppHandle) -> UpdateStatus {
    UpdateStatus {
        current: app.package_info().version.to_string(),
        state: UpdateState::Unsupported,
        ..Default::default()
    }
}

#[tauri::command]
pub async fn update_status(app: AppHandle) -> Result<UpdateStatus, String> {
    Ok(hub(&app)
        .map(|h| h.engine.status())
        .unwrap_or_else(|| fallback_status(&app)))
}

/// Checks now (also when autoCheck is off).
#[tauri::command]
pub async fn update_check(app: AppHandle) -> Result<UpdateStatus, String> {
    match hub(&app) {
        Some(h) => Ok(h.engine.check().await),
        None => Ok(fallback_status(&app)),
    }
}

/// Downloads and installs the available update, then relaunches.
#[tauri::command]
pub async fn update_install(app: AppHandle) -> Result<(), String> {
    match hub(&app) {
        Some(h) => h.engine.install().await,
        None => Err("updates are not available in this build".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    #[derive(Default)]
    struct Fake {
        check: Mutex<Vec<Result<Option<UpdateInfo>, String>>>,
        download: Mutex<Option<Result<Vec<u8>, String>>>,
        install: Mutex<Option<Result<(), String>>>,
        /// Chunks reported by `download` (bytes each), total = sum.
        chunks: Vec<u64>,
        calls: AtomicUsize,
    }

    impl Backend for Fake {
        fn check(&self) -> impl Future<Output = Result<Option<UpdateInfo>, String>> + Send {
            self.calls.fetch_add(1, Ordering::SeqCst);
            let r = self.check.lock().unwrap().remove(0);
            async move { r }
        }
        fn download<'a>(
            &'a self,
            mut on_progress: ProgressFn<'a>,
        ) -> impl Future<Output = Result<Vec<u8>, String>> + Send + 'a {
            self.calls.fetch_add(1, Ordering::SeqCst);
            async move {
                let total: u64 = self.chunks.iter().sum();
                for c in &self.chunks {
                    on_progress(*c, Some(total));
                }
                self.download.lock().unwrap().take().unwrap_or(Ok(vec![1]))
            }
        }
        fn install(&self, _bytes: Vec<u8>) -> Result<(), String> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.install.lock().unwrap().take().unwrap_or(Ok(()))
        }
    }

    fn info(v: &str) -> UpdateInfo {
        UpdateInfo {
            version: v.into(),
            date: Some("2026-11-04T10:00:00Z".into()),
            notes: Some("Fixes".into()),
        }
    }

    type Log = Arc<Mutex<Vec<UpdateStatus>>>;

    fn engine(fake: Fake, supported: bool) -> (Engine<Fake>, Log) {
        let log: Log = Arc::default();
        let sink = log.clone();
        let e = Engine::new(
            fake,
            "0.0.1".into(),
            supported,
            Box::new(move |s| sink.lock().unwrap().push(s.clone())),
        );
        (e, log)
    }

    fn states(log: &Log) -> Vec<UpdateState> {
        log.lock().unwrap().iter().map(|s| s.state).collect()
    }

    #[tokio::test]
    async fn check_finds_update_then_installs() {
        let fake = Fake {
            check: Mutex::new(vec![Ok(Some(info("0.1.0")))]),
            chunks: vec![10, 10, 10, 10],
            ..Default::default()
        };
        let (e, log) = engine(fake, true);
        let s = e.check().await;
        assert_eq!(s.state, UpdateState::Available);
        assert_eq!(s.available, Some(info("0.1.0")));
        assert!(s.last_checked.is_some());
        assert_eq!(
            states(&log),
            [UpdateState::Checking, UpdateState::Available]
        );

        e.install().await.unwrap();
        let st = states(&log);
        assert_eq!(st[2], UpdateState::Downloading);
        assert_eq!(*st.last().unwrap(), UpdateState::Ready);
        let progress: Vec<f64> = log.lock().unwrap()[3..]
            .iter()
            .filter(|s| s.state == UpdateState::Downloading)
            .filter_map(|s| s.progress)
            .collect();
        // First chunk and the final one are emitted; the rest fall inside the 200 ms window.
        assert_eq!(progress, [0.25, 1.0]);
        assert_eq!(e.status().progress, Some(1.0));
    }

    #[tokio::test]
    async fn up_to_date_and_errors() {
        let fake = Fake {
            check: Mutex::new(vec![
                Ok(None),
                Err("no release information is published yet".into()),
                Err("x".repeat(500)),
            ]),
            ..Default::default()
        };
        let (e, _log) = engine(fake, true);
        assert_eq!(e.check().await.state, UpdateState::Idle);
        let s = e.check().await;
        assert_eq!(s.state, UpdateState::Error);
        assert_eq!(
            s.error.as_deref(),
            Some("no release information is published yet")
        );
        // Errors are retryable and kept short.
        let s = e.check().await;
        assert_eq!(s.state, UpdateState::Error);
        assert!(s.error.unwrap().chars().count() <= MAX_ERROR_CHARS + 1);
        assert!(e.install().await.is_err(), "nothing to install");
    }

    #[tokio::test]
    async fn download_failure_keeps_the_offer() {
        let fake = Fake {
            check: Mutex::new(vec![Ok(Some(info("0.2.0")))]),
            download: Mutex::new(Some(Err("network error (download)".into()))),
            ..Default::default()
        };
        let (e, _log) = engine(fake, true);
        e.check().await;
        assert_eq!(e.install().await.unwrap_err(), "network error (download)");
        let s = e.status();
        assert_eq!(s.state, UpdateState::Error);
        assert!(s.available.is_some());
        assert!(s.progress.is_none());
        // Retry succeeds.
        e.install().await.unwrap();
        assert_eq!(e.status().state, UpdateState::Ready);
    }

    #[tokio::test]
    async fn install_failure_is_reported() {
        let fake = Fake {
            check: Mutex::new(vec![Ok(Some(info("0.2.0")))]),
            install: Mutex::new(Some(Err("the update signature is invalid".into()))),
            ..Default::default()
        };
        let (e, _log) = engine(fake, true);
        e.check().await;
        assert!(e.install().await.is_err());
        assert_eq!(e.status().state, UpdateState::Error);
    }

    #[tokio::test]
    async fn ready_blocks_further_checks() {
        let fake = Fake {
            check: Mutex::new(vec![Ok(Some(info("0.2.0")))]),
            ..Default::default()
        };
        let (e, _log) = engine(fake, true);
        e.check().await;
        e.install().await.unwrap();
        // A scheduled check after install does nothing (and would panic the fake).
        assert_eq!(e.check().await.state, UpdateState::Ready);
        assert!(e.install().await.is_err());
    }

    #[tokio::test]
    async fn unsupported_never_touches_the_backend() {
        let (e, log) = engine(Fake::default(), false);
        assert_eq!(e.status().state, UpdateState::Unsupported);
        assert_eq!(e.check().await.state, UpdateState::Unsupported);
        assert!(e.install().await.is_err());
        assert_eq!(e.backend.calls.load(Ordering::SeqCst), 0);
        assert!(log.lock().unwrap().is_empty());
    }

    #[test]
    fn schedule_delays() {
        let h = 60 * 60 * 1000;
        assert_eq!(next_delay(None, 0, true), STARTUP_DELAY);
        assert_eq!(next_delay(Some(0), 0, true), STARTUP_DELAY);
        assert_eq!(next_delay(None, 0, false), STARTUP_DELAY);
        assert_eq!(
            next_delay(Some(0), 4 * h, false),
            Duration::from_secs(20 * 60 * 60)
        );
        assert_eq!(next_delay(Some(0), 30 * h, false), STARTUP_DELAY);
    }

    #[test]
    fn status_matches_ts_contract() {
        let v = serde_json::to_value(UpdateStatus {
            current: "0.0.1".into(),
            state: UpdateState::Downloading,
            available: Some(info("0.1.0")),
            progress: Some(0.5),
            error: None,
            last_checked: Some(1),
        })
        .unwrap();
        assert_eq!(
            v,
            serde_json::json!({
                "current": "0.0.1", "state": "downloading",
                "available": {"version": "0.1.0", "date": "2026-11-04T10:00:00Z", "notes": "Fixes"},
                "progress": 0.5, "error": null, "lastChecked": 1
            })
        );
        assert_eq!(
            serde_json::to_value(UpdateState::Unsupported).unwrap(),
            "unsupported"
        );
    }

    #[test]
    fn short_truncates() {
        assert_eq!(short("abc"), "abc");
        assert_eq!(short(&"é".repeat(200)).chars().count(), MAX_ERROR_CHARS + 1);
    }
}
