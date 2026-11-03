//! System widget backend: `sysinfo` CPU/RAM (portable), GPU via `platform::current()`.
//!
//! Sampling is ref-counted: a single sampler thread runs at 1 Hz only while at least one
//! widget is subscribed, and exits as soon as the count drops to zero (idle budget).

use std::collections::HashMap;
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use serde::Serialize;
use sysinfo::{
    CpuRefreshKind, MemoryRefreshKind, ProcessRefreshKind, ProcessesToUpdate, System,
    MINIMUM_CPU_UPDATE_INTERVAL,
};
use tauri::{AppHandle, Emitter, State};

use crate::platform;

pub const EVENT_SAMPLE: &str = "system://sample";
const SAMPLE_INTERVAL: Duration = Duration::from_secs(1);
const TOP_N: usize = 5;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SystemSample {
    pub t: i64,
    pub cpu: f32,
    pub per_core: Vec<f32>,
    pub mem_used: u64,
    pub mem_total: u64,
    pub gpu: Option<f32>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SystemInfo {
    pub cpu_name: String,
    pub cores: usize,
    pub gpu_name: Option<String>,
    pub mem_total: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcessInfo {
    pub name: String,
    pub cpu: f32,
    pub memory: u64,
}

/// Subscriber count + the stop handle of the running sampler (dropping it stops the thread).
#[derive(Default)]
pub struct Sampler(Mutex<(u32, Option<Sender<()>>)>);

/// The PDH `\GPU Engine(*)` wildcard query is the costliest part of a sample, so the GPU
/// value is refreshed at most every [`GPU_INTERVAL`] and reused in between.
const GPU_INTERVAL: Duration = Duration::from_secs(3);

#[derive(Default)]
struct GpuThrottle {
    last: Option<f32>,
    at: Option<Instant>,
}

impl GpuThrottle {
    fn get(&mut self, now: Instant, read: impl FnOnce() -> Option<f32>) -> Option<f32> {
        let due = match self.at {
            Some(at) => now.saturating_duration_since(at) >= GPU_INTERVAL,
            None => true,
        };
        if due {
            self.last = read();
            self.at = Some(now);
        }
        self.last
    }
}

fn sample(sys: &mut System, gpu: Option<f32>) -> SystemSample {
    sys.refresh_cpu_usage();
    sys.refresh_memory_specifics(MemoryRefreshKind::nothing().with_ram());
    SystemSample {
        t: chrono::Utc::now().timestamp_millis(),
        cpu: sys.global_cpu_usage().clamp(0.0, 100.0),
        per_core: sys
            .cpus()
            .iter()
            .map(|c| c.cpu_usage().clamp(0.0, 100.0))
            .collect(),
        mem_used: sys.used_memory(),
        mem_total: sys.total_memory(),
        gpu,
    }
}

fn spawn_sampler(app: AppHandle) -> Option<Sender<()>> {
    let (tx, rx) = mpsc::channel::<()>();
    let res = std::thread::Builder::new()
        .name("system-sampler".into())
        .spawn(move || {
            tracing::debug!("system sampler started");
            let mut sys = System::new();
            sys.refresh_cpu_usage(); // baseline for the first delta
            let _ = platform::current().gpu_usage(); // prime the GPU rate counter
            let mut gpu = GpuThrottle::default();
            // Wakes every second; a disconnect (sender dropped) ends the loop at once.
            while let Err(RecvTimeoutError::Timeout) = rx.recv_timeout(SAMPLE_INTERVAL) {
                let g = gpu.get(Instant::now(), || platform::current().gpu_usage().ok());
                let _ = app.emit(EVENT_SAMPLE, sample(&mut sys, g));
            }
            // Close the PDH query so a later restart doesn't average over the idle gap.
            platform::current().gpu_release();
            tracing::debug!("system sampler stopped");
        });
    match res {
        Ok(_) => Some(tx),
        Err(e) => {
            tracing::warn!("cannot start system sampler: {e}");
            None
        }
    }
}

impl Sampler {
    /// Drops every subscription. Called when the webview (re)loads: a reload never runs the
    /// widgets' cleanup, so without this the count would leak and the 1 Hz thread run forever.
    pub fn reset(&self) {
        let mut s = self.0.lock().unwrap_or_else(|p| p.into_inner());
        if s.0 > 0 {
            tracing::debug!(subscribers = s.0, "page load: resetting system sampler");
        }
        *s = (0, None);
    }
}

#[tauri::command]
pub async fn system_subscribe(app: AppHandle, sampler: State<'_, Sampler>) -> Result<(), String> {
    let mut s = sampler.0.lock().unwrap_or_else(|p| p.into_inner());
    s.0 += 1;
    if s.1.is_none() {
        s.1 = spawn_sampler(app);
    }
    Ok(())
}

#[tauri::command]
pub async fn system_unsubscribe(sampler: State<'_, Sampler>) -> Result<(), String> {
    let mut s = sampler.0.lock().unwrap_or_else(|p| p.into_inner());
    s.0 = s.0.saturating_sub(1);
    if s.0 == 0 {
        s.1 = None; // disconnects the channel → thread exits immediately
    }
    Ok(())
}

static INFO: OnceLock<SystemInfo> = OnceLock::new();

#[tauri::command]
pub async fn system_info() -> Result<SystemInfo, String> {
    if let Some(info) = INFO.get() {
        return Ok(info.clone());
    }
    let info = tauri::async_runtime::spawn_blocking(|| {
        let mut sys = System::new();
        sys.refresh_cpu_list(CpuRefreshKind::nothing());
        sys.refresh_memory_specifics(MemoryRefreshKind::nothing().with_ram());
        let cpu_name = sys
            .cpus()
            .first()
            .map(|c| c.brand().trim().to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "Unknown CPU".into());
        SystemInfo {
            cpu_name,
            cores: sys.cpus().len(),
            gpu_name: platform::current().gpu_name().ok(),
            mem_total: sys.total_memory(),
        }
    })
    .await
    .map_err(|e| e.to_string())?;
    Ok(INFO.get_or_init(|| info).clone())
}

/// Sums same-name processes (e.g. many `chrome.exe`) and returns the top `n` by CPU.
/// `cpu` is normalised to 0–100 of the whole machine.
pub fn top_processes(
    raw: impl IntoIterator<Item = (String, f32, u64)>,
    cores: usize,
    n: usize,
) -> Vec<ProcessInfo> {
    let cores = cores.max(1) as f32;
    let mut by_name: HashMap<String, ProcessInfo> = HashMap::new();
    for (name, cpu, memory) in raw {
        let e = by_name.entry(name.clone()).or_insert(ProcessInfo {
            name,
            cpu: 0.0,
            memory: 0,
        });
        e.cpu += cpu / cores;
        e.memory += memory;
    }
    let mut list: Vec<ProcessInfo> = by_name.into_values().collect();
    for p in &mut list {
        p.cpu = p.cpu.clamp(0.0, 100.0);
    }
    list.sort_by(|a, b| {
        b.cpu
            .total_cmp(&a.cpu)
            .then(b.memory.cmp(&a.memory))
            .then(a.name.cmp(&b.name))
    });
    list.truncate(n);
    list
}

#[tauri::command]
pub async fn system_top_processes() -> Result<Vec<ProcessInfo>, String> {
    tauri::async_runtime::spawn_blocking(|| {
        let mut sys = System::new();
        sys.refresh_cpu_list(CpuRefreshKind::nothing());
        let kind = ProcessRefreshKind::nothing().with_cpu().with_memory();
        // CPU% is a delta, so refresh twice; on demand only, nothing is kept afterwards.
        sys.refresh_processes_specifics(ProcessesToUpdate::All, true, kind);
        std::thread::sleep(MINIMUM_CPU_UPDATE_INTERVAL.max(Duration::from_millis(300)));
        sys.refresh_processes_specifics(ProcessesToUpdate::All, true, kind);
        let raw = sys
            .processes()
            .iter()
            .filter(|(pid, _)| pid.as_u32() != 0) // Windows "System Idle Process"
            .map(|(_, p)| {
                (
                    p.name().to_string_lossy().into_owned(),
                    p.cpu_usage(),
                    p.memory(),
                )
            });
        top_processes(raw, sys.cpus().len(), TOP_N)
    })
    .await
    .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn top_processes_aggregates_and_normalises() {
        let raw = vec![
            ("chrome".to_string(), 80.0, 100),
            ("chrome".to_string(), 40.0, 50),
            ("code".to_string(), 100.0, 10),
            ("idle".to_string(), 0.0, 1),
        ];
        let top = top_processes(raw, 4, 2);
        assert_eq!(top.len(), 2);
        assert_eq!(top[0].name, "chrome");
        assert!((top[0].cpu - 30.0).abs() < 1e-4);
        assert_eq!(top[0].memory, 150);
        assert_eq!(top[1].name, "code");
    }

    #[test]
    fn gpu_is_read_at_most_every_interval() {
        let mut t = GpuThrottle::default();
        let start = Instant::now();
        let mut reads = 0;
        let mut at = |secs: u64, v: f32, t: &mut GpuThrottle| {
            t.get(start + Duration::from_secs(secs), || {
                reads += 1;
                Some(v)
            })
        };
        assert_eq!(at(0, 10.0, &mut t), Some(10.0));
        assert_eq!(at(1, 20.0, &mut t), Some(10.0), "reused");
        assert_eq!(at(2, 30.0, &mut t), Some(10.0), "reused");
        assert_eq!(at(3, 40.0, &mut t), Some(40.0), "refreshed after 3 s");
        assert_eq!(at(4, 50.0, &mut t), Some(40.0));
        assert_eq!(reads, 2);
    }

    #[test]
    fn sample_is_camel_case() {
        let mut sys = System::new();
        let v = serde_json::to_value(sample(&mut sys, Some(1.0))).unwrap();
        for k in ["t", "cpu", "perCore", "memUsed", "memTotal", "gpu"] {
            assert!(v.get(k).is_some(), "{k}");
        }
    }
}
