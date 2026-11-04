//! Audio via `pactl` (PulseAudio, or PipeWire through pipewire-pulse) with `-f json` output
//! (pactl ≥ 16). `wpctl` (WirePlumber) is the fallback for the default output's volume/mute
//! when pactl is missing or cannot reach a server. Per-app sessions are sink-inputs on the
//! default sink. Changes come from one long-lived `pactl subscribe` child (see [`watch`]).
//!
//! Every parser here is a pure function over the tool's text output so it is unit-tested on
//! any OS against fixtures.

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdout, Stdio};
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::time::{Duration, Instant};

use serde_json::Value;

use super::cmd;
use crate::platform::{
    unit_level, AudioDevice, AudioSession, AudioState, Capability, ChangeFn, PlatformError, Result,
    Watch,
};

/// PulseAudio `PA_VOLUME_NORM` (100 %).
const VOLUME_NORM: f64 = 65536.0;
/// `wpctl` names for the default output.
const WP_DEFAULT_SINK: &str = "@DEFAULT_AUDIO_SINK@";
const PA_DEFAULT_SINK: &str = "@DEFAULT_SINK@";

struct Tools {
    pactl: Option<PathBuf>,
    wpctl: Option<PathBuf>,
}

fn tools() -> &'static Tools {
    static TOOLS: OnceLock<Tools> = OnceLock::new();
    TOOLS.get_or_init(|| Tools {
        pactl: cmd::which("pactl"),
        wpctl: cmd::which("wpctl"),
    })
}

/// True when at least one supported audio tool is on `PATH`.
pub fn available() -> bool {
    let t = tools();
    t.pactl.is_some() || t.wpctl.is_some()
}

fn unsupported() -> PlatformError {
    PlatformError::Unsupported(Capability::Audio)
}

fn pactl() -> Result<&'static Path> {
    tools().pactl.as_deref().ok_or_else(|| {
        PlatformError::Os("this needs pactl (pulseaudio-utils / pipewire-pulse)".into())
    })
}

// ── Parsing (pure) ──────────────────────────────────────────────────────────

/// One `pactl -f json list sinks` entry.
#[derive(Debug, Clone, PartialEq)]
pub struct Sink {
    pub index: u32,
    pub name: String,
    pub description: String,
    pub volume: f32,
    pub muted: bool,
}

/// One `pactl -f json list sink-inputs` entry.
#[derive(Debug, Clone, PartialEq)]
pub struct SinkInput {
    pub index: u32,
    pub sink: u32,
    pub name: String,
    pub volume: f32,
    pub muted: bool,
    pub has_volume: bool,
}

/// Unsigned integer that pactl may print as a number or (PipeWire, some fields) a string.
fn as_u32(v: &Value) -> Option<u32> {
    match v {
        Value::Number(n) => n.as_u64().and_then(|n| u32::try_from(n).ok()),
        Value::String(s) => s.trim().parse().ok(),
        _ => None,
    }
}

fn as_bool(v: &Value) -> Option<bool> {
    match v {
        Value::Bool(b) => Some(*b),
        Value::String(s) => match s.trim() {
            "yes" | "true" | "1" => Some(true),
            "no" | "false" | "0" => Some(false),
            _ => None,
        },
        Value::Number(n) => n.as_u64().map(|n| n != 0),
        _ => None,
    }
}

/// 0–1+ level of one channel: raw `value` (65536 = 100 %, number or string) or, when that is
/// missing, the `value_percent` string ("40%").
fn channel_level(ch: &Value) -> Option<f64> {
    let raw = match ch.get("value") {
        Some(Value::Number(n)) => n.as_f64(),
        Some(Value::String(s)) => s.trim().parse::<f64>().ok(),
        _ => None,
    };
    if let Some(r) = raw {
        return Some(r / VOLUME_NORM);
    }
    let pct = ch.get("value_percent")?.as_str()?.trim();
    pct.strip_suffix('%')
        .unwrap_or(pct)
        .trim()
        .parse::<f64>()
        .ok()
        .map(|p| p / 100.0)
}

/// Master level of a pactl `volume` object = loudest channel (what pavucontrol's lock slider
/// shows), clamped to 0–1 (Pulse allows > 100 %).
pub fn volume_level(volume: &Value) -> Option<f32> {
    let channels = volume.as_object()?;
    let max = channels
        .values()
        .filter_map(channel_level)
        .fold(None, |acc: Option<f64>, v| {
            Some(acc.map_or(v, |a| a.max(v)))
        })?;
    unit_level(max)
}

fn parse_array(json: &str, what: &str) -> Result<Vec<Value>> {
    match serde_json::from_str::<Value>(json.trim()) {
        Ok(Value::Array(a)) => Ok(a),
        Ok(_) => Err(PlatformError::Os(format!("unexpected pactl {what} output"))),
        Err(e) => Err(PlatformError::Os(format!(
            "cannot parse pactl {what} JSON (pactl 16+ needed): {e}"
        ))),
    }
}

pub fn parse_sinks(json: &str) -> Result<Vec<Sink>> {
    Ok(parse_array(json, "sinks")?
        .iter()
        .filter_map(|s| {
            let index = as_u32(s.get("index")?)?;
            let name = s.get("name")?.as_str()?.to_string();
            fn text(v: Option<&Value>) -> Option<&str> {
                v.and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|d| !d.is_empty())
            }
            let description = text(s.get("description"))
                .or_else(|| text(s.pointer("/properties/device.description")))
                .unwrap_or(&name)
                .to_string();
            Some(Sink {
                index,
                description,
                volume: s.get("volume").and_then(volume_level).unwrap_or(0.0),
                muted: s.get("mute").and_then(as_bool).unwrap_or(false),
                name,
            })
        })
        .collect())
}

/// Readable app name of a sink-input from its properties.
fn input_name(props: Option<&Value>) -> String {
    let prop = |k: &str| {
        props
            .and_then(|p| p.get(k))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
    };
    prop("application.name")
        .or_else(|| prop("application.process.binary"))
        .or_else(|| prop("node.description"))
        .or_else(|| prop("media.name"))
        .unwrap_or("Unknown app")
        .to_string()
}

pub fn parse_sink_inputs(json: &str) -> Result<Vec<SinkInput>> {
    Ok(parse_array(json, "sink-inputs")?
        .iter()
        .filter_map(|s| {
            Some(SinkInput {
                index: as_u32(s.get("index")?)?,
                sink: as_u32(s.get("sink")?)?,
                name: input_name(s.get("properties")),
                volume: s.get("volume").and_then(volume_level).unwrap_or(1.0),
                muted: s.get("mute").and_then(as_bool).unwrap_or(false),
                has_volume: s.get("has_volume").and_then(as_bool).unwrap_or(true),
            })
        })
        .collect())
}

pub fn build_state(sinks: &[Sink], default_name: &str) -> AudioState {
    let devices: Vec<AudioDevice> = sinks
        .iter()
        .map(|s| AudioDevice {
            id: s.name.clone(),
            name: s.description.clone(),
            is_default: s.name == default_name,
        })
        .collect();
    match sinks.iter().find(|s| s.name == default_name) {
        Some(def) => AudioState {
            device: devices.iter().find(|d| d.is_default).cloned(),
            volume: def.volume,
            muted: def.muted,
            devices,
        },
        None => AudioState {
            device: None,
            volume: 0.0,
            muted: false,
            devices,
        },
    }
}

/// Sink-inputs playing to `sink`, sorted by name (like the Windows mixer).
pub fn sessions_on(inputs: &[SinkInput], sink: Option<u32>) -> Vec<AudioSession> {
    let Some(sink) = sink else {
        return Vec::new();
    };
    let mut out: Vec<AudioSession> = inputs
        .iter()
        .filter(|i| i.sink == sink && i.has_volume)
        .map(|i| AudioSession {
            id: i.index.to_string(),
            name: i.name.clone(),
            volume: i.volume,
            muted: i.muted,
        })
        .collect();
    out.sort_by_key(|a| a.name.to_lowercase());
    out
}

/// `pactl set-*-volume` argument: raw volume (65536 = 100 %), clamped to 0–100 %.
pub fn raw_volume(level: f32) -> String {
    let l = if level.is_finite() {
        f64::from(level).clamp(0.0, 1.0)
    } else {
        0.0
    };
    ((l * VOLUME_NORM).round() as u32).to_string()
}

/// `wpctl get-volume` → (level, muted). Output: `Volume: 0.40` or `Volume: 0.40 [MUTED]`.
pub fn parse_wpctl_volume(out: &str) -> Option<(f32, bool)> {
    let line = out
        .lines()
        .find(|l| l.trim_start().starts_with("Volume:"))?;
    let rest = line.trim_start().strip_prefix("Volume:")?.trim();
    let num = rest.split_whitespace().next()?.parse::<f64>().ok()?;
    Some((unit_level(num)?, rest.contains("[MUTED]")))
}

/// `wpctl inspect` → `node.description` (falls back to `node.nick` / `node.name`).
pub fn parse_wpctl_description(out: &str) -> Option<String> {
    let value = |key: &str| {
        out.lines().find_map(|l| {
            let l = l.trim_start().trim_start_matches('*').trim_start();
            let rest = l.strip_prefix(key)?.trim_start().strip_prefix('=')?.trim();
            let v = rest.trim_matches('"').trim();
            (!v.is_empty()).then(|| v.to_string())
        })
    };
    value("node.description")
        .or_else(|| value("node.nick"))
        .or_else(|| value("node.name"))
}

/// True for `pactl subscribe` lines that can change [`AudioState`] or the session list:
/// `Event 'change' on sink #56`, `Event 'new' on sink-input #91`, `… on server #-1`, cards.
pub fn is_audio_event(line: &str) -> bool {
    let Some(rest) = line.trim().strip_prefix("Event '") else {
        return false;
    };
    let Some((_, on)) = rest.split_once("' on ") else {
        return false;
    };
    let facility = on.split(" #").next().unwrap_or("").trim();
    matches!(facility, "sink" | "sink-input" | "server" | "card")
}

// ── pactl / wpctl calls ─────────────────────────────────────────────────────

struct Snapshot {
    sinks: Vec<Sink>,
    default_name: String,
}

fn pactl_snapshot(p: &Path) -> Result<Snapshot> {
    let sinks = parse_sinks(&cmd::run(p, ["-f", "json", "list", "sinks"])?)?;
    let default_name = cmd::run(p, ["get-default-sink"])?.trim().to_string();
    Ok(Snapshot {
        sinks,
        default_name,
    })
}

fn wpctl_state(w: &Path) -> Result<AudioState> {
    let (volume, muted) = parse_wpctl_volume(&cmd::run(w, ["get-volume", WP_DEFAULT_SINK])?)
        .ok_or_else(|| PlatformError::Os("unexpected wpctl output".into()))?;
    let name = cmd::run(w, ["inspect", WP_DEFAULT_SINK])
        .ok()
        .and_then(|o| parse_wpctl_description(&o))
        .unwrap_or_else(|| "Default output".into());
    let device = AudioDevice {
        id: WP_DEFAULT_SINK.into(),
        name,
        is_default: true,
    };
    Ok(AudioState {
        device: Some(device.clone()),
        volume,
        muted,
        devices: vec![device],
    })
}

/// pactl first; wpctl when pactl is missing or fails (e.g. PipeWire without pipewire-pulse).
fn with_fallback<T>(
    pa: impl FnOnce(&Path) -> Result<T>,
    wp: impl FnOnce(&Path) -> Result<T>,
) -> Result<T> {
    let t = tools();
    match (&t.pactl, &t.wpctl) {
        (Some(p), Some(w)) => pa(p).or_else(|e| {
            tracing::debug!("pactl failed ({e}); trying wpctl");
            wp(w)
        }),
        (Some(p), None) => pa(p),
        (None, Some(w)) => wp(w),
        (None, None) => Err(unsupported()),
    }
}

pub fn state() -> Result<AudioState> {
    with_fallback(
        |p| pactl_snapshot(p).map(|s| build_state(&s.sinks, &s.default_name)),
        wpctl_state,
    )
}

pub fn set_volume(level: f32) -> Result<()> {
    with_fallback(
        |p| cmd::run(p, ["set-sink-volume", PA_DEFAULT_SINK, &raw_volume(level)]).map(drop),
        |w| {
            let l = unit_level(f64::from(level)).unwrap_or(0.0);
            cmd::run(w, ["set-volume", WP_DEFAULT_SINK, &format!("{l:.3}")]).map(drop)
        },
    )
}

pub fn set_muted(muted: bool) -> Result<()> {
    let flag = if muted { "1" } else { "0" };
    with_fallback(
        |p| cmd::run(p, ["set-sink-mute", PA_DEFAULT_SINK, flag]).map(drop),
        |w| cmd::run(w, ["set-mute", WP_DEFAULT_SINK, flag]).map(drop),
    )
}

pub fn set_default_device(id: &str) -> Result<()> {
    if !available() {
        return Err(unsupported());
    }
    let p = pactl()?;
    // Only accept a listed sink; never hand arbitrary strings to pactl.
    let snap = pactl_snapshot(p)?;
    let sink = snap
        .sinks
        .iter()
        .find(|s| s.name == id)
        .ok_or_else(|| PlatformError::Os("unknown audio device".into()))?;
    cmd::run(p, ["set-default-sink", &sink.name]).map(drop)
}

pub fn sessions() -> Result<Vec<AudioSession>> {
    if !available() {
        return Err(unsupported());
    }
    // Without pactl (wpctl only) there is no per-app list: an empty mixer, not an error.
    let Some(p) = tools().pactl.as_deref() else {
        return Ok(Vec::new());
    };
    let snap = pactl_snapshot(p)?;
    let inputs = parse_sink_inputs(&cmd::run(p, ["-f", "json", "list", "sink-inputs"])?)?;
    let default = snap
        .sinks
        .iter()
        .find(|s| s.name == snap.default_name)
        .map(|s| s.index);
    Ok(sessions_on(&inputs, default))
}

/// A session id is a sink-input index; anything else is rejected before reaching pactl.
pub fn session_index(id: &str) -> Result<u32> {
    id.parse::<u32>()
        .map_err(|_| PlatformError::Os("audio session not found".into()))
}

pub fn set_session_volume(id: &str, level: f32) -> Result<()> {
    if !available() {
        return Err(unsupported());
    }
    let idx = session_index(id)?.to_string();
    cmd::run(
        pactl()?,
        ["set-sink-input-volume", &idx, &raw_volume(level)],
    )
    .map(drop)
}

pub fn set_session_muted(id: &str, muted: bool) -> Result<()> {
    if !available() {
        return Err(unsupported());
    }
    let idx = session_index(id)?.to_string();
    let flag = if muted { "1" } else { "0" };
    cmd::run(pactl()?, ["set-sink-input-mute", &idx, flag]).map(drop)
}

// ── Change notifications (`pactl subscribe`) ────────────────────────────────

/// Longest wait before respawning a `pactl subscribe` that exited (sound server restart).
const RESPAWN_MAX: Duration = Duration::from_secs(30);

struct WatchState {
    stopped: bool,
    child: Option<Child>,
}

struct Shared {
    state: Mutex<WatchState>,
    wake: Condvar,
}

impl Shared {
    fn lock(&self) -> std::sync::MutexGuard<'_, WatchState> {
        self.state.lock().unwrap_or_else(|p| p.into_inner())
    }
}

/// Dropping it kills and reaps the `pactl subscribe` child; the reader thread then sees EOF
/// and exits.
struct StopOnDrop(Arc<Shared>);

impl Drop for StopOnDrop {
    fn drop(&mut self) {
        let child = {
            let mut s = self.0.lock();
            s.stopped = true;
            s.child.take()
        };
        self.0.wake.notify_all();
        if let Some(mut c) = child {
            let _ = c.kill();
            let _ = c.wait();
        }
    }
}

fn spawn_subscribe(p: &Path) -> std::io::Result<(Child, ChildStdout)> {
    let mut child = cmd::command(p)
        .arg("subscribe")
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;
    match child.stdout.take() {
        Some(out) => Ok((child, out)),
        None => {
            let _ = child.kill();
            let _ = child.wait();
            Err(std::io::Error::other("no stdout"))
        }
    }
}

fn read_events(out: ChildStdout, on_change: &ChangeFn) {
    for line in BufReader::new(out).lines() {
        match line {
            Ok(l) if is_audio_event(&l) => on_change(),
            Ok(_) => {}
            Err(_) => break,
        }
    }
}

fn watch_loop(p: PathBuf, shared: Arc<Shared>, first: ChildStdout, on_change: ChangeFn) {
    let mut out = Some(first);
    let mut backoff = Duration::from_secs(1);
    loop {
        if let Some(o) = out.take() {
            let started = Instant::now();
            read_events(o, &on_change);
            if started.elapsed() > Duration::from_secs(10) {
                backoff = Duration::from_secs(1);
            }
        }
        // EOF: pactl exited (server restart) or Drop killed it. Reap it here if Drop didn't.
        let mut s = shared.lock();
        if let Some(mut c) = s.child.take() {
            let _ = c.kill();
            let _ = c.wait();
        }
        if s.stopped {
            break;
        }
        let deadline = Instant::now() + backoff;
        while !s.stopped {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                break;
            }
            s = shared
                .wake
                .wait_timeout(s, left)
                .unwrap_or_else(|p| p.into_inner())
                .0;
        }
        if s.stopped {
            break;
        }
        backoff = (backoff * 2).min(RESPAWN_MAX);
        match spawn_subscribe(&p) {
            Ok((child, o)) => {
                s.child = Some(child);
                out = Some(o);
                drop(s);
                tracing::debug!("pactl subscribe restarted");
                // The server may have changed while we weren't listening.
                on_change();
            }
            Err(e) => tracing::debug!("cannot restart pactl subscribe: {e}"),
        }
    }
    tracing::debug!("audio watcher stopped");
}

pub fn watch(on_change: ChangeFn) -> Result<Watch> {
    if !available() {
        return Err(unsupported());
    }
    let p = pactl()?.to_path_buf();
    let (child, out) = spawn_subscribe(&p)
        .map_err(|e| PlatformError::Os(format!("cannot run pactl subscribe: {e}")))?;
    let shared = Arc::new(Shared {
        state: Mutex::new(WatchState {
            stopped: false,
            child: Some(child),
        }),
        wake: Condvar::new(),
    });
    let guard = StopOnDrop(shared.clone());
    std::thread::Builder::new()
        .name("kotilo-audio-watch".into())
        .spawn(move || watch_loop(p, shared, out, on_change))
        .map_err(|e| PlatformError::Os(format!("cannot start audio watcher: {e}")))?;
    tracing::debug!("audio watcher started");
    Ok(Watch::new(guard))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `pactl -f json list sinks` from pactl 16.1 on PipeWire 1.0 (pipewire-pulse), trimmed.
    /// The HDMI sink carries only `value_percent` strings (seen with some pipewire-pulse
    /// builds) and a string index.
    const SINKS: &str = r#"[{"index":56,"state":"RUNNING","name":"alsa_output.pci-0000_00_1f.3.analog-stereo","description":"Built-in Audio Analog Stereo","driver":"PipeWire","sample_specification":"s32le 2ch 48000Hz","channel_map":"front-left,front-right","owner_module":4294967295,"mute":false,"volume":{"front-left":{"value":26214,"value_percent":"40%","db":"-23.88 dB"},"front-right":{"value":22938,"value_percent":"35%","db":"-27.36 dB"}},"balance":-0.125000,"base_volume":{"value":65536,"value_percent":"100%","db":"0.00 dB"},"monitor_source":"alsa_output.pci-0000_00_1f.3.analog-stereo.monitor","latency":{"actual":0.000000,"configured":0.000000},"flags":["HARDWARE","HW_MUTE_CTRL","HW_VOLUME_CTRL","DECIBEL_VOLUME","LATENCY"],"properties":{"alsa.card":"0","device.description":"Built-in Audio Analog Stereo","node.name":"alsa_output.pci-0000_00_1f.3.analog-stereo","object.serial":"57","media.class":"Audio/Sink"},"ports":[{"name":"analog-output-speaker","description":"Speakers","type":"Speaker","priority":10000,"availability_group":"Legacy 3","availability":"availability unknown"}],"active_port":"analog-output-speaker","formats":["pcm"]},
{"index":"61","state":"SUSPENDED","name":"alsa_output.pci-0000_01_00.1.hdmi-stereo","description":"","driver":"PipeWire","mute":true,"volume":{"front-left":{"value_percent":"120%"},"front-right":{"value_percent":"118%"}},"properties":{"device.description":"GA102 HDMI Audio Controller Digital Stereo (HDMI)"},"ports":[],"formats":["pcm"]},
{"name":"broken entry without index"}]"#;

    /// `pactl -f json list sink-inputs` on pipewire-pulse: Firefox and Spotify on the analog
    /// sink, a nameless stream, a volume-less passive stream and one on HDMI.
    const SINK_INPUTS: &str = r#"[{"index":89,"driver":"PipeWire","owner_module":"4294967295","client":"88","sink":56,"sample_specification":"float32le 2ch 48000Hz","channel_map":"front-left,front-right","format":"pcm, format.sample_format = \"\\\"float32le\\\"\"  format.rate = \"48000\"  format.channels = \"2\"  format.channel_map = \"\\\"front-left,front-right\\\"\"","corked":false,"mute":false,"volume":{"front-left":{"value":65536,"value_percent":"100%","db":"0.00 dB"},"front-right":{"value":65536,"value_percent":"100%","db":"0.00 dB"}},"balance":0.000000,"buffer_latency":0.000000,"sink_latency":0.000000,"resample_method":"PipeWire","properties":{"application.name":"Firefox","application.process.binary":"firefox","application.process.id":"4242","media.name":"AudioStream","media.class":"Stream/Output/Audio","node.name":"Firefox","object.serial":"512"}},
{"index":95,"driver":"PipeWire","client":"94","sink":56,"corked":true,"mute":true,"has_volume":true,"volume_writable":true,"volume":{"front-left":{"value":32768,"value_percent":"50%","db":"-18.06 dB"},"front-right":{"value":32768,"value_percent":"50%","db":"-18.06 dB"}},"properties":{"application.name":"spotify","application.process.binary":"spotify","media.name":"Spotify"}},
{"index":97,"sink":56,"mute":false,"volume":{"mono":{"value":49152,"value_percent":"75%","db":"-7.50 dB"}},"properties":{"application.name":"","application.process.binary":"mpv"}},
{"index":98,"sink":56,"mute":false,"has_volume":false,"volume":{},"properties":{"application.name":"Passive"}},
{"index":99,"sink":61,"mute":false,"volume":{"mono":{"value":65536}},"properties":{"application.name":"Chromium"}}]"#;

    #[test]
    fn parses_pipewire_pulse_sinks() {
        let sinks = parse_sinks(SINKS).unwrap();
        assert_eq!(sinks.len(), 2, "entries without an index are skipped");
        let a = &sinks[0];
        assert_eq!(a.index, 56);
        assert_eq!(a.name, "alsa_output.pci-0000_00_1f.3.analog-stereo");
        assert_eq!(a.description, "Built-in Audio Analog Stereo");
        assert!(
            (a.volume - 0.4).abs() < 1e-3,
            "loudest channel: {}",
            a.volume
        );
        assert!(!a.muted);
        let hdmi = &sinks[1];
        assert_eq!(hdmi.index, 61, "string index accepted");
        assert_eq!(
            hdmi.description, "GA102 HDMI Audio Controller Digital Stereo (HDMI)",
            "empty description falls back to device.description"
        );
        assert_eq!(hdmi.volume, 1.0, "value_percent fallback, clamped to 100 %");
        assert!(hdmi.muted);
    }

    #[test]
    fn state_marks_the_default_sink() {
        let sinks = parse_sinks(SINKS).unwrap();
        let s = build_state(&sinks, "alsa_output.pci-0000_01_00.1.hdmi-stereo");
        let dev = s.device.unwrap();
        assert_eq!(dev.id, "alsa_output.pci-0000_01_00.1.hdmi-stereo");
        assert!(dev.is_default);
        assert!(s.muted);
        assert_eq!(s.devices.len(), 2);
        assert_eq!(s.devices.iter().filter(|d| d.is_default).count(), 1);
        let none = build_state(&sinks, "auto_null");
        assert!(none.device.is_none());
        assert_eq!(none.volume, 0.0);
        assert!(none.devices.iter().all(|d| !d.is_default));
    }

    #[test]
    fn sessions_are_sink_inputs_on_the_default_sink() {
        let inputs = parse_sink_inputs(SINK_INPUTS).unwrap();
        assert_eq!(inputs.len(), 5);
        let s = sessions_on(&inputs, Some(56));
        let names: Vec<&str> = s.iter().map(|a| a.name.as_str()).collect();
        assert_eq!(
            names,
            ["Firefox", "mpv", "spotify"],
            "sorted, passive skipped"
        );
        assert_eq!(s[0].id, "89");
        assert_eq!(s[0].volume, 1.0);
        assert_eq!(s[1].volume, 0.75, "mono channel");
        assert!(s[2].muted);
        assert_eq!(s[2].volume, 0.5);
        assert_eq!(sessions_on(&inputs, Some(61))[0].name, "Chromium");
        assert!(sessions_on(&inputs, None).is_empty());
    }

    #[test]
    fn sink_input_name_fallbacks() {
        assert_eq!(input_name(None), "Unknown app");
        let p = serde_json::json!({"media.name": "Playback"});
        assert_eq!(input_name(Some(&p)), "Playback");
    }

    #[test]
    fn bad_json_is_an_error_not_a_panic() {
        assert!(parse_sinks("Connection failure: Connection refused").is_err());
        assert!(parse_sinks("{}").is_err());
        assert_eq!(parse_sinks("[]").unwrap(), vec![]);
        assert!(parse_sink_inputs("").is_err());
    }

    #[test]
    fn channel_levels() {
        let v = serde_json::json!({"a": {"value": "32768"}, "b": {"value_percent": " 60 % "}});
        assert_eq!(volume_level(&v), Some(0.6));
        assert_eq!(volume_level(&serde_json::json!({})), None);
        assert_eq!(volume_level(&serde_json::json!("50%")), None);
    }

    #[test]
    fn raw_volume_is_clamped() {
        assert_eq!(raw_volume(0.5), "32768");
        assert_eq!(raw_volume(1.0), "65536");
        assert_eq!(raw_volume(2.0), "65536");
        assert_eq!(raw_volume(-1.0), "0");
        assert_eq!(raw_volume(f32::NAN), "0");
    }

    #[test]
    fn wpctl_output() {
        assert_eq!(parse_wpctl_volume("Volume: 0.40\n"), Some((0.4, false)));
        assert_eq!(
            parse_wpctl_volume("Volume: 1.25 [MUTED]\n"),
            Some((1.0, true))
        );
        assert_eq!(parse_wpctl_volume("Translate error"), None);
        let inspect = "id 52, type PipeWire:Interface:Node\n    alsa.card = \"0\"\n  * node.description = \"Built-in Audio Analog Stereo\"\n  * node.name = \"alsa_output.pci-0000_00_1f.3.analog-stereo\"\n    node.nick = \"ALC257 Analog\"\n";
        assert_eq!(
            parse_wpctl_description(inspect).as_deref(),
            Some("Built-in Audio Analog Stereo")
        );
        assert_eq!(
            parse_wpctl_description("  * node.name = \"x\"\n").as_deref(),
            Some("x")
        );
        assert_eq!(parse_wpctl_description(""), None);
    }

    #[test]
    fn subscribe_lines() {
        assert!(is_audio_event("Event 'change' on sink #56"));
        assert!(is_audio_event("Event 'new' on sink-input #91\n"));
        assert!(is_audio_event("Event 'remove' on sink-input #91"));
        assert!(is_audio_event("Event 'change' on server #-1"));
        assert!(is_audio_event("Event 'change' on card #42"));
        assert!(!is_audio_event("Event 'change' on source #57"));
        assert!(!is_audio_event("Event 'new' on source-output #7"));
        assert!(!is_audio_event("Event 'remove' on client #90"));
        assert!(!is_audio_event("Ereignis 'change' auf sink #56"));
        assert!(!is_audio_event(""));
    }

    #[test]
    fn session_ids_must_be_indices() {
        assert_eq!(session_index("89").unwrap(), 89);
        assert!(session_index("-1").is_err());
        assert!(session_index("@DEFAULT_SINK@").is_err());
        assert!(session_index("").is_err());
    }
}
