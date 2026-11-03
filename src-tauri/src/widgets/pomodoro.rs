//! Pomodoro: one timer owned by the backend, so it keeps running, notifies and talks to the
//! companion while the deck is hidden (SPEC §4.4). Shape mirrors `PomodoroState` in
//! `src/lib/ipc/index.ts` and `pomodoro.state` in `protocol.md`.
//!
//! The live timer is persisted (`pomodoro_timer`, one JSON row) on every change, so a
//! restart resumes it: a running phase keeps its wall-clock `endsAt`, a paused one its
//! `remaining`. History goes to `pomodoro_sessions`. Both live in `deck.db`.
//!
//! Scheduling works like the todo reminders: one task sleeps until `endsAt` (at most
//! [`MAX_SLEEP`]) and is woken by every change; a phase that ended while the app was closed
//! is completed once at startup. Every change emits `pomodoro://state` and broadcasts
//! `pomodoro.state` to the companion; nothing is sent per tick.

use std::path::Path;
use std::sync::Mutex;
use std::time::Duration;

use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_notification::NotificationExt;
use tokio::sync::Notify;

use crate::migrations;
use crate::settings::PomodoroSettings;

pub const EVENT_STATE: &str = "pomodoro://state";
pub const EVENT_PHASE_END: &str = "pomodoro://phase-end";
/// Upper bound for one scheduler sleep while a phase runs (suspend / clock changes).
const MAX_SLEEP: Duration = Duration::from_secs(60);
/// A phase end noticed later than this (app closed, PC asleep) never auto-starts the next
/// phase: the user was away.
const LATE_LIMIT_MS: i64 = 60_000;
const TIMER_SCHEMA_VERSION: u32 = 1;
const MAX_MINUTES: u32 = 180;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Phase {
    Idle,
    Focus,
    ShortBreak,
    LongBreak,
}

impl Phase {
    pub fn as_str(self) -> &'static str {
        match self {
            Phase::Idle => "idle",
            Phase::Focus => "focus",
            Phase::ShortBreak => "short-break",
            Phase::LongBreak => "long-break",
        }
    }
}

/// `PomodoroState` (TS) = protocol.md `pomodoro.state` payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PomodoroState {
    pub phase: Phase,
    pub running: bool,
    /// Unix ms while running, else null.
    pub ends_at: Option<i64>,
    /// Seconds left; authoritative while paused.
    pub remaining: u32,
    /// Full length of the phase in seconds.
    pub duration: u32,
    /// 1-based index of the current/next focus session in the cycle.
    pub session: u32,
    pub sessions_per_cycle: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PomodoroToday {
    pub sessions: u32,
    pub focus_minutes: u32,
    pub goal: u32,
}

/// One finished or abandoned phase for `pomodoro_sessions`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Record {
    pub started_at: i64,
    pub ended_at: i64,
    pub phase: Phase,
    pub completed: bool,
    /// Seconds actually run (pauses excluded).
    pub duration_s: u32,
}

/// What a timer operation produced besides the new state.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Outcome {
    pub records: Vec<Record>,
    /// `(ended, next)` when a phase ran to its end.
    pub phase_end: Option<(Phase, Phase)>,
    /// The next phase was started automatically.
    pub auto_started: bool,
}

fn length_s(phase: Phase, cfg: &PomodoroSettings) -> u32 {
    60 * match phase {
        Phase::Idle | Phase::Focus => cfg.focus,
        Phase::ShortBreak => cfg.short_break,
        Phase::LongBreak => cfg.long_break,
    }
}

/// The live timer (persisted as JSON).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Timer {
    #[serde(default)]
    pub schema_version: u32,
    pub phase: Phase,
    pub running: bool,
    pub ends_at: Option<i64>,
    pub remaining: u32,
    pub duration: u32,
    pub session: u32,
    pub sessions_per_cycle: u32,
    /// When the current phase was first started; `None` = not started yet (idle or ready).
    pub started_at: Option<i64>,
}

impl Timer {
    pub fn idle(cfg: &PomodoroSettings) -> Self {
        let d = length_s(Phase::Idle, cfg);
        Self {
            schema_version: TIMER_SCHEMA_VERSION,
            phase: Phase::Idle,
            running: false,
            ends_at: None,
            remaining: d,
            duration: d,
            session: 1,
            sessions_per_cycle: cfg.sessions_per_cycle,
            started_at: None,
        }
    }

    /// Restores a persisted timer, repairing anything out of range.
    fn sane(mut self, cfg: &PomodoroSettings) -> Self {
        self.schema_version = TIMER_SCHEMA_VERSION;
        self.sessions_per_cycle = cfg.sessions_per_cycle;
        self.session = self.session.clamp(1, cfg.sessions_per_cycle);
        if self.phase == Phase::Idle || self.duration == 0 {
            return Timer {
                session: self.session,
                ..Timer::idle(cfg)
            };
        }
        self.remaining = self.remaining.min(self.duration);
        if self.running && self.ends_at.is_none() {
            self.running = false;
        }
        if !self.running {
            self.ends_at = None;
        }
        self
    }

    fn remaining_at(&self, now: i64) -> u32 {
        match (self.running, self.ends_at) {
            (true, Some(end)) => {
                let ms = (end - now).max(0);
                u32::try_from((ms + 999) / 1000)
                    .unwrap_or(u32::MAX)
                    .min(self.duration)
            }
            _ => self.remaining,
        }
    }

    pub fn state(&self, now: i64) -> PomodoroState {
        PomodoroState {
            phase: self.phase,
            running: self.running,
            ends_at: if self.running { self.ends_at } else { None },
            remaining: self.remaining_at(now),
            duration: self.duration,
            session: self.session,
            sessions_per_cycle: self.sessions_per_cycle,
        }
    }

    /// History row for the current phase if it was started.
    fn record(&self, now: i64, completed: bool) -> Option<Record> {
        let started_at = self.started_at?;
        if self.phase == Phase::Idle {
            return None;
        }
        let ran = if completed {
            self.duration
        } else {
            self.duration - self.remaining_at(now)
        };
        Some(Record {
            started_at,
            ended_at: if completed {
                self.ends_at.unwrap_or(now)
            } else {
                now
            },
            phase: self.phase,
            completed,
            duration_s: ran,
        })
    }

    /// Phase and session that follow the current phase.
    fn next(&self) -> (Phase, u32) {
        let spc = self.sessions_per_cycle.max(1);
        match self.phase {
            Phase::Focus if self.session >= spc => (Phase::LongBreak, 1),
            Phase::Focus => (Phase::ShortBreak, self.session + 1),
            Phase::ShortBreak | Phase::LongBreak => (Phase::Focus, self.session),
            Phase::Idle => (Phase::Focus, self.session),
        }
    }

    fn begin(&mut self, phase: Phase, duration: u32, now: i64) {
        self.phase = phase;
        self.duration = duration;
        self.remaining = duration;
        self.running = true;
        self.ends_at = Some(now + i64::from(duration) * 1000);
        self.started_at = Some(now);
    }

    /// Moves to the next phase: started if `auto`, else ready (not running, full length).
    fn advance(&mut self, cfg: &PomodoroSettings, now: i64, auto: bool) -> Phase {
        let (phase, session) = self.next();
        self.session = session;
        let d = length_s(phase, cfg);
        if auto {
            self.begin(phase, d, now);
        } else {
            self.phase = phase;
            self.duration = d;
            self.remaining = d;
            self.running = false;
            self.ends_at = None;
            self.started_at = None;
        }
        phase
    }

    /// Starts `phase` (default: focus when idle, else the current phase) from its full
    /// length, or `minutes`. A phase already in progress is recorded as not completed.
    pub fn start(
        &mut self,
        cfg: &PomodoroSettings,
        phase: Option<Phase>,
        minutes: Option<u32>,
        now: i64,
    ) -> Result<Outcome, String> {
        if phase == Some(Phase::Idle) {
            return Err("cannot start the idle phase".into());
        }
        if let Some(m) = minutes {
            if !(1..=MAX_MINUTES).contains(&m) {
                return Err(format!("minutes must be between 1 and {MAX_MINUTES}"));
            }
        }
        let target = phase.unwrap_or(match self.phase {
            Phase::Idle => Phase::Focus,
            p => p,
        });
        let mut out = Outcome::default();
        out.records.extend(self.record(now, false));
        let d = minutes.map_or_else(|| length_s(target, cfg), |m| m * 60);
        self.begin(target, d, now);
        Ok(out)
    }

    pub fn pause(&mut self, now: i64) {
        if self.running {
            self.remaining = self.remaining_at(now);
            self.running = false;
            self.ends_at = None;
        }
    }

    /// Continues a paused phase, or starts a ready one. No-op when idle or running.
    pub fn resume(&mut self, now: i64) {
        if self.running || self.phase == Phase::Idle {
            return;
        }
        if self.remaining == 0 {
            self.remaining = self.duration;
        }
        self.running = true;
        self.ends_at = Some(now + i64::from(self.remaining) * 1000);
        if self.started_at.is_none() {
            self.started_at = Some(now);
        }
    }

    /// Ends the current phase now (recorded as not completed, so a skipped focus never
    /// counts) and moves to the next, which auto-starts like a natural end would.
    pub fn skip(&mut self, cfg: &PomodoroSettings, now: i64) -> Outcome {
        let mut out = Outcome::default();
        if self.phase == Phase::Idle {
            return out;
        }
        out.records.extend(self.record(now, false));
        out.auto_started = cfg.auto_start_next;
        self.advance(cfg, now, cfg.auto_start_next);
        out
    }

    /// Back to idle, session 1.
    pub fn reset(&mut self, cfg: &PomodoroSettings, now: i64) -> Outcome {
        let mut out = Outcome::default();
        out.records.extend(self.record(now, false));
        *self = Timer::idle(cfg);
        out
    }

    /// Completes the running phase if its end has passed.
    pub fn tick(&mut self, cfg: &PomodoroSettings, now: i64) -> Outcome {
        let mut out = Outcome::default();
        let Some(end) = self.ends_at.filter(|_| self.running) else {
            return out;
        };
        if end > now {
            return out;
        }
        out.records.extend(self.record(now, true));
        let ended = self.phase;
        let auto = cfg.auto_start_next && now - end <= LATE_LIMIT_MS;
        let next = self.advance(cfg, now, auto);
        out.phase_end = Some((ended, next));
        out.auto_started = auto;
        out
    }

    /// Settings changed: a phase in progress keeps its length (new lengths apply to the next
    /// phase); idle/ready phases take the new length. Returns whether the state changed.
    pub fn apply_settings(&mut self, cfg: &PomodoroSettings) -> bool {
        let before = self.clone();
        self.sessions_per_cycle = cfg.sessions_per_cycle;
        self.session = self.session.clamp(1, cfg.sessions_per_cycle);
        if !self.running && self.started_at.is_none() {
            let d = length_s(self.phase, cfg);
            self.duration = d;
            self.remaining = d;
        }
        *self != before
    }

    /// When the scheduler must look again.
    pub fn next_deadline(&self) -> Option<i64> {
        self.ends_at.filter(|_| self.running)
    }
}

// ── Storage ────────────────────────────────────────────────────────────────

fn sql(e: rusqlite::Error) -> String {
    format!("database error: {e}")
}

pub struct PomodoroDb {
    conn: Connection,
}

impl PomodoroDb {
    pub fn open(path: &Path) -> Result<Self, String> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| format!("cannot create data dir: {e}"))?;
        }
        let conn = Connection::open(path).map_err(sql)?;
        conn.pragma_update(None, "journal_mode", "WAL")
            .map_err(sql)?;
        conn.busy_timeout(Duration::from_secs(2)).map_err(sql)?;
        Self::init(conn)
    }

    #[cfg(test)]
    pub fn open_in_memory() -> Result<Self, String> {
        Self::init(Connection::open_in_memory().map_err(sql)?)
    }

    fn init(mut conn: Connection) -> Result<Self, String> {
        migrations::migrate_db(&mut conn)?;
        Ok(Self { conn })
    }

    pub fn load_timer(&self) -> Result<Option<Timer>, String> {
        let raw: Option<String> = self
            .conn
            .query_row("SELECT state FROM pomodoro_timer WHERE id = 1", [], |r| {
                r.get(0)
            })
            .optional()
            .map_err(sql)?;
        Ok(raw.and_then(|s| match serde_json::from_str::<Timer>(&s) {
            Ok(t) if t.schema_version <= TIMER_SCHEMA_VERSION => Some(t),
            Ok(_) => {
                tracing::warn!("pomodoro timer row is from a newer build; starting idle");
                None
            }
            Err(_) => {
                tracing::warn!("pomodoro timer row is corrupt; starting idle");
                None
            }
        }))
    }

    pub fn save_timer(&self, t: &Timer) -> Result<(), String> {
        let json = serde_json::to_string(t).map_err(|e| e.to_string())?;
        self.conn
            .execute(
                "INSERT INTO pomodoro_timer (id, state) VALUES (1, ?1)
                 ON CONFLICT(id) DO UPDATE SET state = excluded.state",
                [json],
            )
            .map_err(sql)?;
        Ok(())
    }

    pub fn insert(&self, r: &Record) -> Result<(), String> {
        self.conn
            .execute(
                "INSERT INTO pomodoro_sessions (started_at, ended_at, phase, completed, duration_s)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    r.started_at,
                    r.ended_at,
                    r.phase.as_str(),
                    r.completed,
                    r.duration_s
                ],
            )
            .map_err(sql)?;
        Ok(())
    }

    /// Completed focus sessions and their seconds ending in `[from, to)`.
    pub fn focus_between(&self, from: i64, to: i64) -> Result<(u32, u32), String> {
        self.conn
            .query_row(
                "SELECT COUNT(*), COALESCE(SUM(duration_s), 0) FROM pomodoro_sessions
                 WHERE phase = 'focus' AND completed = 1 AND ended_at >= ?1 AND ended_at < ?2",
                params![from, to],
                |r| Ok((r.get::<_, u32>(0)?, r.get::<_, u32>(1)?)),
            )
            .map_err(sql)
    }
}

/// `[local midnight, next local midnight)` around `now`, in Unix ms.
pub fn local_day(now: i64) -> (i64, i64) {
    use chrono::{Days, Local, LocalResult, TimeZone};
    let Some(today) = Local
        .timestamp_millis_opt(now)
        .single()
        .map(|t| t.date_naive())
    else {
        return (now - 86_400_000, now + 1);
    };
    let midnight = |d: chrono::NaiveDate| -> Option<i64> {
        let naive = d.and_hms_opt(0, 0, 0)?;
        match Local.from_local_datetime(&naive) {
            LocalResult::Single(t) | LocalResult::Ambiguous(t, _) => Some(t.timestamp_millis()),
            // DST gap at midnight: the day starts an hour later.
            LocalResult::None => Local
                .from_local_datetime(&(naive + chrono::Duration::hours(1)))
                .earliest()
                .map(|t| t.timestamp_millis()),
        }
    };
    let start = midnight(today).unwrap_or(now - 86_400_000);
    let end = today
        .checked_add_days(Days::new(1))
        .and_then(midnight)
        .unwrap_or(start + 86_400_000);
    (start, end)
}

// ── State, commands, scheduler ──────────────────────────────────────────────

struct Core {
    db: Option<PomodoroDb>,
    timer: Timer,
}

impl Core {
    fn persist(&self, out: &Outcome) {
        let Some(db) = &self.db else { return };
        for r in &out.records {
            if let Err(e) = db.insert(r) {
                tracing::warn!("cannot record pomodoro session: {e}");
            }
        }
        if let Err(e) = db.save_timer(&self.timer) {
            tracing::warn!("cannot save pomodoro timer: {e}");
        }
    }
}

pub struct Pomodoro {
    core: Mutex<Core>,
    wake: Notify,
}

impl Pomodoro {
    /// Opens the timer from `deck.db`. Without a database the timer still works, it just
    /// does not survive a restart.
    pub fn open(path: &Path, cfg: &PomodoroSettings) -> Self {
        let db = match PomodoroDb::open(path) {
            Ok(db) => Some(db),
            Err(e) => {
                tracing::error!(file = %crate::paths::file_label(path), "pomodoro database unavailable: {e}");
                None
            }
        };
        Self::with_db(db, cfg)
    }

    fn with_db(db: Option<PomodoroDb>, cfg: &PomodoroSettings) -> Self {
        let timer = db
            .as_ref()
            .and_then(|db| db.load_timer().unwrap_or_default())
            .map_or_else(|| Timer::idle(cfg), |t| t.sane(cfg));
        Self {
            core: Mutex::new(Core { db, timer }),
            wake: Notify::new(),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Core> {
        self.core.lock().unwrap_or_else(|p| p.into_inner())
    }

    pub fn state(&self) -> PomodoroState {
        self.lock().timer.state(now_ms())
    }

    /// Runs `f` on the timer, persists, and returns the new state plus what happened.
    fn mutate(
        &self,
        f: impl FnOnce(&mut Timer, i64) -> Result<Outcome, String>,
    ) -> Result<(PomodoroState, Outcome), String> {
        let now = now_ms();
        let mut core = self.lock();
        let out = f(&mut core.timer, now)?;
        core.persist(&out);
        let state = core.timer.state(now);
        drop(core);
        self.wake.notify_one();
        Ok((state, out))
    }

    pub fn today(&self, goal: u32) -> Result<PomodoroToday, String> {
        let (from, to) = local_day(now_ms());
        let core = self.lock();
        let (sessions, secs) = match &core.db {
            Some(db) => db.focus_between(from, to)?,
            None => (0, 0),
        };
        Ok(PomodoroToday {
            sessions,
            focus_minutes: secs / 60,
            goal,
        })
    }
}

pub fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

fn cfg(app: &AppHandle) -> PomodoroSettings {
    crate::settings::load(app).pomodoro
}

/// `pomodoro://state` to the UI and `pomodoro.state` to the desktop.
fn broadcast(app: &AppHandle, state: &PomodoroState) {
    if let Err(e) = app.emit(EVENT_STATE, state) {
        tracing::warn!("cannot emit pomodoro state: {e}");
    }
    crate::companion::broadcast_pomodoro(app, state);
}

fn minutes_label(seconds: u32) -> String {
    format!("{} min", seconds.div_ceil(60))
}

/// OS notification + `pomodoro://phase-end`. English until backend strings are localised.
fn announce(app: &AppHandle, ended: Phase, next: Phase, auto: bool, state: &PomodoroState) {
    let title = match ended {
        Phase::Focus => "Focus session complete",
        _ => "Break over",
    };
    let len = minutes_label(state.duration);
    let body = match (next, auto) {
        (Phase::ShortBreak, false) => format!("Time for a short break ({len})"),
        (Phase::LongBreak, false) => format!("Time for a long break ({len})"),
        (Phase::ShortBreak, true) => format!("Short break started ({len})"),
        (Phase::LongBreak, true) => format!("Long break started ({len})"),
        (_, false) => format!("Ready for focus session {}", state.session),
        (_, true) => format!("Focus session {} started ({len})", state.session),
    };
    let sound = crate::settings::load(app).notification_sound;
    let mut n = app.notification().builder().title(title).body(body);
    if sound {
        n = n.sound(if cfg!(windows) {
            "Reminder"
        } else {
            "message-new-instant"
        });
    }
    if let Err(e) = n.show() {
        tracing::warn!("cannot show pomodoro notification: {e}");
    }
    let _ = app.emit(
        EVENT_PHASE_END,
        serde_json::json!({ "phase": ended, "next": next }),
    );
    tracing::info!(
        phase = ended.as_str(),
        next = next.as_str(),
        auto,
        "pomodoro phase ended"
    );
}

fn command(
    app: &AppHandle,
    pomo: &Pomodoro,
    f: impl FnOnce(&mut Timer, &PomodoroSettings, i64) -> Result<Outcome, String>,
) -> Result<PomodoroState, String> {
    let c = cfg(app);
    let (state, _) = pomo.mutate(|t, now| f(t, &c, now))?;
    broadcast(app, &state);
    Ok(state)
}

/// Settings hook: lengths/cycle size changed (`settings_set`, import).
pub fn settings_changed(app: &AppHandle, cfg: &PomodoroSettings) {
    let Some(pomo) = app.try_state::<Pomodoro>() else {
        return;
    };
    let mut changed = false;
    let result = pomo.mutate(|t, _| {
        changed = t.apply_settings(cfg);
        Ok(Outcome::default())
    });
    if let (true, Ok((state, _))) = (changed, result) {
        broadcast(app, &state);
    }
}

#[tauri::command]
pub async fn pomodoro_state(pomo: State<'_, Pomodoro>) -> Result<PomodoroState, String> {
    Ok(pomo.state())
}

#[tauri::command]
pub async fn pomodoro_start(
    app: AppHandle,
    pomo: State<'_, Pomodoro>,
    phase: Option<Phase>,
    minutes: Option<u32>,
) -> Result<PomodoroState, String> {
    command(&app, &pomo, |t, c, now| t.start(c, phase, minutes, now))
}

#[tauri::command]
pub async fn pomodoro_pause(
    app: AppHandle,
    pomo: State<'_, Pomodoro>,
) -> Result<PomodoroState, String> {
    command(&app, &pomo, |t, _, now| {
        t.pause(now);
        Ok(Outcome::default())
    })
}

#[tauri::command]
pub async fn pomodoro_resume(
    app: AppHandle,
    pomo: State<'_, Pomodoro>,
) -> Result<PomodoroState, String> {
    command(&app, &pomo, |t, _, now| {
        t.resume(now);
        Ok(Outcome::default())
    })
}

#[tauri::command]
pub async fn pomodoro_skip(
    app: AppHandle,
    pomo: State<'_, Pomodoro>,
) -> Result<PomodoroState, String> {
    command(&app, &pomo, |t, c, now| Ok(t.skip(c, now)))
}

#[tauri::command]
pub async fn pomodoro_reset(
    app: AppHandle,
    pomo: State<'_, Pomodoro>,
) -> Result<PomodoroState, String> {
    command(&app, &pomo, |t, c, now| Ok(t.reset(c, now)))
}

#[tauri::command]
pub async fn pomodoro_today(
    app: AppHandle,
    pomo: State<'_, Pomodoro>,
) -> Result<PomodoroToday, String> {
    pomo.today(cfg(&app).daily_goal)
}

/// The phase-end scheduler. Runs for the app's lifetime, independent of any window; the
/// first pass completes a phase that ended while the app was closed.
pub fn spawn_scheduler(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        let pomo = app.state::<Pomodoro>();
        loop {
            let due = pomo
                .lock()
                .timer
                .next_deadline()
                .is_some_and(|end| end <= now_ms());
            if due {
                let c = cfg(&app);
                if let Ok((state, out)) = pomo.mutate(|t, now| Ok(t.tick(&c, now))) {
                    if let Some((ended, next)) = out.phase_end {
                        announce(&app, ended, next, out.auto_started, &state);
                        broadcast(&app, &state);
                    }
                }
            }
            let deadline = pomo.lock().timer.next_deadline();
            match deadline {
                Some(end) => {
                    let ms = u64::try_from(end.saturating_sub(now_ms())).unwrap_or(0);
                    let d = Duration::from_millis(ms).min(MAX_SLEEP);
                    tokio::select! {
                        _ = tokio::time::sleep(d) => {}
                        _ = pomo.wake.notified() => {}
                    }
                }
                // Nothing running: no timer wake-ups at all until a command changes state.
                None => pomo.wake.notified().await,
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> PomodoroSettings {
        PomodoroSettings::default()
    }

    const T0: i64 = 1_793_733_000_000;

    #[test]
    fn idle_state_matches_contract() {
        let t = Timer::idle(&cfg());
        let v = serde_json::to_value(t.state(T0)).unwrap();
        assert_eq!(
            v,
            serde_json::json!({
                "phase": "idle", "running": false, "endsAt": null, "remaining": 1500,
                "duration": 1500, "session": 1, "sessionsPerCycle": 4
            })
        );
        let today = serde_json::to_value(PomodoroToday {
            sessions: 2,
            focus_minutes: 50,
            goal: 8,
        })
        .unwrap();
        assert_eq!(
            today,
            serde_json::json!({"sessions": 2, "focusMinutes": 50, "goal": 8})
        );
    }

    #[test]
    fn phase_strings() {
        for p in [
            Phase::Idle,
            Phase::Focus,
            Phase::ShortBreak,
            Phase::LongBreak,
        ] {
            assert_eq!(serde_json::json!(p), p.as_str());
        }
    }

    #[test]
    fn start_defaults_and_overrides() {
        let c = cfg();
        let mut t = Timer::idle(&c);
        t.start(&c, None, None, T0).unwrap();
        let s = t.state(T0);
        assert_eq!(s.phase, Phase::Focus);
        assert!(s.running);
        assert_eq!(s.ends_at, Some(T0 + 1_500_000));
        assert_eq!((s.remaining, s.duration), (1500, 1500));
        assert_eq!(t.state(T0 + 1_001).remaining, 1499);

        // Restarting mid-phase records the abandoned part.
        let out = t
            .start(&c, Some(Phase::ShortBreak), Some(2), T0 + 60_000)
            .unwrap();
        assert_eq!(
            out.records,
            [Record {
                started_at: T0,
                ended_at: T0 + 60_000,
                phase: Phase::Focus,
                completed: false,
                duration_s: 60
            }]
        );
        assert_eq!(t.phase, Phase::ShortBreak);
        assert_eq!(t.duration, 120);
        assert!(t.start(&c, Some(Phase::Idle), None, T0).is_err());
        assert!(t.start(&c, None, Some(0), T0).is_err());
        assert!(t.start(&c, None, Some(181), T0).is_err());
        // No phase given while in a break restarts that break.
        t.start(&c, None, None, T0).unwrap();
        assert_eq!((t.phase, t.duration), (Phase::ShortBreak, 300));
    }

    #[test]
    fn pause_resume_keeps_remaining() {
        let c = cfg();
        let mut t = Timer::idle(&c);
        t.start(&c, None, None, T0).unwrap();
        t.pause(T0 + 100_500);
        let s = t.state(T0 + 999_999);
        assert!(!s.running && s.ends_at.is_none());
        assert_eq!(s.remaining, 1400); // ceil(1399.5)
        t.pause(T0 + 200_000); // no-op
        assert_eq!(t.remaining, 1400);
        t.resume(T0 + 500_000);
        assert_eq!(t.ends_at, Some(T0 + 500_000 + 1_400_000));
        assert_eq!(t.started_at, Some(T0));
        // Idle resume is a no-op.
        let mut idle = Timer::idle(&c);
        idle.resume(T0);
        assert!(!idle.running);
    }

    #[test]
    fn cycle_focus_breaks_and_long_break() {
        let c = PomodoroSettings {
            sessions_per_cycle: 2,
            ..cfg()
        };
        let mut t = Timer::idle(&c);
        let mut now = T0;
        let mut seen = Vec::new();
        t.start(&c, None, None, now).unwrap();
        for _ in 0..5 {
            now = t.ends_at.unwrap();
            let out = t.tick(&c, now);
            let (ended, next) = out.phase_end.unwrap();
            assert!(out.records[0].completed);
            seen.push((ended, next, t.session));
            assert!(!t.running, "no auto start by default");
            assert_eq!(t.remaining, t.duration);
            t.resume(now);
        }
        assert_eq!(
            seen,
            [
                (Phase::Focus, Phase::ShortBreak, 2),
                (Phase::ShortBreak, Phase::Focus, 2),
                (Phase::Focus, Phase::LongBreak, 1),
                (Phase::LongBreak, Phase::Focus, 1),
                (Phase::Focus, Phase::ShortBreak, 2),
            ]
        );
    }

    #[test]
    fn tick_before_end_does_nothing_and_auto_start_chains() {
        let c = PomodoroSettings {
            auto_start_next: true,
            ..cfg()
        };
        let mut t = Timer::idle(&c);
        t.start(&c, None, None, T0).unwrap();
        assert_eq!(t.tick(&c, T0 + 1_499_999), Outcome::default());
        let end = T0 + 1_500_000;
        let out = t.tick(&c, end + 10);
        assert!(out.auto_started);
        assert_eq!(out.records[0].ended_at, end);
        assert_eq!(out.records[0].duration_s, 1500);
        assert_eq!(t.phase, Phase::ShortBreak);
        assert!(t.running);
        assert_eq!(t.ends_at, Some(end + 10 + 300_000));
    }

    #[test]
    fn missed_end_at_startup_records_once_and_does_not_auto_start() {
        let c = PomodoroSettings {
            auto_start_next: true,
            ..cfg()
        };
        let mut t = Timer::idle(&c);
        t.start(&c, None, None, T0).unwrap();
        // App closed for hours.
        let out = t.tick(&c, T0 + 5 * 3_600_000);
        assert_eq!(out.phase_end, Some((Phase::Focus, Phase::ShortBreak)));
        assert!(!out.auto_started && !t.running);
        assert_eq!(out.records.len(), 1);
        assert!(t.tick(&c, T0 + 6 * 3_600_000).records.is_empty());
    }

    #[test]
    fn skip_and_reset() {
        let c = cfg();
        let mut t = Timer::idle(&c);
        assert_eq!(t.skip(&c, T0), Outcome::default());
        t.start(&c, None, None, T0).unwrap();
        let out = t.skip(&c, T0 + 30_000);
        assert!(!out.records[0].completed);
        assert_eq!(out.records[0].duration_s, 30);
        assert_eq!(out.phase_end, None);
        assert_eq!(
            (t.phase, t.session, t.running),
            (Phase::ShortBreak, 2, false)
        );
        // Skipping a ready (never started) phase records nothing.
        let out = t.skip(&c, T0 + 31_000);
        assert!(out.records.is_empty());
        assert_eq!(t.phase, Phase::Focus);
        t.resume(T0 + 40_000);
        let out = t.reset(&c, T0 + 100_000);
        assert_eq!(out.records[0].duration_s, 60);
        assert_eq!(t, Timer::idle(&c));
    }

    #[test]
    fn settings_apply_to_next_phase_only() {
        let mut c = cfg();
        let mut t = Timer::idle(&c);
        c.focus = 50;
        assert!(t.apply_settings(&c));
        assert_eq!(t.duration, 3000, "idle shows the new focus length");
        t.start(&c, None, None, T0).unwrap();
        c.focus = 10;
        c.short_break = 7;
        t.apply_settings(&c);
        assert_eq!(t.duration, 3000, "running phase keeps its length");
        assert_eq!(t.ends_at, Some(T0 + 3_000_000));
        t.pause(T0 + 1_000);
        t.apply_settings(&c);
        assert_eq!(t.duration, 3000, "paused phase keeps its length");
        t.resume(T0 + 2_000);
        t.tick(&c, t.ends_at.unwrap());
        assert_eq!(t.duration, 420, "next phase uses the new length");
        // Cycle size shrinks below the current session.
        t.session = 4;
        c.sessions_per_cycle = 2;
        assert!(t.apply_settings(&c));
        assert_eq!((t.session, t.sessions_per_cycle), (2, 2));
        assert!(!t.apply_settings(&c));
    }

    #[test]
    fn persisted_timer_resumes_and_today_counts_local_day() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("deck.db");
        let c = cfg();
        let now = now_ms();
        {
            let db = PomodoroDb::open(&path).unwrap();
            let mut t = Timer::idle(&c);
            t.start(&c, None, None, now).unwrap();
            db.save_timer(&t).unwrap();
            db.insert(&Record {
                started_at: now - 1_500_000,
                ended_at: now,
                phase: Phase::Focus,
                completed: true,
                duration_s: 1500,
            })
            .unwrap();
            for (phase, completed) in [(Phase::Focus, false), (Phase::ShortBreak, true)] {
                db.insert(&Record {
                    started_at: now - 1000,
                    ended_at: now,
                    phase,
                    completed,
                    duration_s: 300,
                })
                .unwrap();
            }
            // Yesterday.
            db.insert(&Record {
                started_at: now - 3 * 86_400_000,
                ended_at: now - 2 * 86_400_000,
                phase: Phase::Focus,
                completed: true,
                duration_s: 1500,
            })
            .unwrap();
        }
        let p = Pomodoro::open(&path, &c);
        let s = p.state();
        assert_eq!(s.phase, Phase::Focus);
        assert!(s.running);
        assert_eq!(s.ends_at, Some(now + 1_500_000));
        assert_eq!(
            p.today(8).unwrap(),
            PomodoroToday {
                sessions: 1,
                focus_minutes: 25,
                goal: 8
            }
        );
    }

    #[test]
    fn corrupt_or_future_timer_row_starts_idle() {
        let c = cfg();
        let db = PomodoroDb::open_in_memory().unwrap();
        db.conn
            .execute(
                "INSERT INTO pomodoro_timer (id, state) VALUES (1, 'nope')",
                [],
            )
            .unwrap();
        assert_eq!(db.load_timer().unwrap(), None);
        let mut future = Timer::idle(&c);
        future.schema_version = 99;
        db.save_timer(&future).unwrap();
        assert_eq!(db.load_timer().unwrap(), None);
        // Out-of-range values are repaired.
        let mut odd = Timer::idle(&c);
        odd.phase = Phase::Focus;
        odd.session = 40;
        odd.running = true;
        odd.ends_at = None;
        odd.remaining = 99_999;
        let fixed = odd.sane(&c);
        assert_eq!(fixed.session, 4);
        assert!(!fixed.running);
        assert_eq!(fixed.remaining, fixed.duration);
    }

    #[test]
    fn local_day_brackets_now() {
        let now = now_ms();
        let (a, b) = local_day(now);
        assert!(a <= now && now < b);
        assert!((82_800_000..=90_000_000).contains(&(b - a)));
    }
}
