use std::collections::HashMap;
use std::sync::Mutex;

use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{COLORREF, HWND};
use windows::Win32::Graphics::Dxgi::{
    CreateDXGIFactory1, IDXGIFactory1, DXGI_ADAPTER_FLAG_SOFTWARE,
};
use windows::Win32::System::Performance::{
    PdhAddEnglishCounterW, PdhCloseQuery, PdhCollectQueryData, PdhGetFormattedCounterArrayW,
    PdhOpenQueryW, PDH_CSTATUS_NEW_DATA, PDH_CSTATUS_VALID_DATA, PDH_FMT_COUNTERVALUE_ITEM_W,
    PDH_FMT_DOUBLE, PDH_HCOUNTER, PDH_HQUERY, PDH_MORE_DATA,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetWindowLongW, IsWindow, SetLayeredWindowAttributes, SetWindowLongW, GWL_EXSTYLE, LWA_ALPHA,
    WS_EX_LAYERED,
};

use super::{
    AudioDevice, AudioSession, AudioState, Capability, ChangeFn, MediaAction, MediaState,
    NativeWindow, Platform, PlatformError, Result, Watch,
};

mod audio;
mod com;
mod media;

/// Windows implementation (windows-rs). Capabilities are added as each phase lands.
pub struct WindowsPlatform;

impl Platform for WindowsPlatform {
    fn capabilities(&self) -> Vec<Capability> {
        vec![
            Capability::Monitors,
            Capability::SystemInfo,
            Capability::Gpu,
            Capability::Transparency,
            Capability::Audio,
            Capability::Media,
        ]
    }

    fn set_window_opacity(&self, window: NativeWindow, opacity: f64) -> Result<()> {
        let hwnd = HWND(window.0 as *mut core::ffi::c_void);
        let layered = WS_EX_LAYERED.0 as i32;
        // SAFETY: the handle is validated with IsWindow; style reads/writes and
        // SetLayeredWindowAttributes have no memory-safety preconditions beyond a valid HWND.
        unsafe {
            if !IsWindow(Some(hwnd)).as_bool() {
                return Err(PlatformError::Os("not a window".into()));
            }
            let ex = GetWindowLongW(hwnd, GWL_EXSTYLE);
            if opacity >= 1.0 || !opacity.is_finite() {
                if ex & layered != 0 {
                    SetWindowLongW(hwnd, GWL_EXSTYLE, ex & !layered);
                }
                return Ok(());
            }
            if ex & layered == 0 {
                SetWindowLongW(hwnd, GWL_EXSTYLE, ex | layered);
                if GetWindowLongW(hwnd, GWL_EXSTYLE) & layered == 0 {
                    return Err(PlatformError::Os("cannot make the window layered".into()));
                }
            }
            SetLayeredWindowAttributes(hwnd, COLORREF(0), alpha(opacity), LWA_ALPHA).map_err(os)
        }
    }

    fn audio_devices(&self) -> Result<Vec<AudioDevice>> {
        audio::run(audio::devices)
    }
    fn volume(&self) -> Result<f32> {
        audio::run(audio::volume)
    }
    fn set_volume(&self, level: f32) -> Result<()> {
        audio::run(move || audio::set_volume(level))
    }
    fn set_muted(&self, muted: bool) -> Result<()> {
        audio::run(move || audio::set_muted(muted))
    }
    fn audio_state(&self) -> Result<AudioState> {
        audio::run(audio::state)
    }
    fn set_default_audio_device(&self, id: &str) -> Result<()> {
        let id = id.to_string();
        audio::run(move || audio::set_default_device(&id))
    }
    fn audio_sessions(&self) -> Result<Vec<AudioSession>> {
        audio::run(audio::sessions)
    }
    fn set_session_volume(&self, id: &str, level: f32) -> Result<()> {
        let id = id.to_string();
        audio::run(move || audio::set_session_volume(&id, level))
    }
    fn set_session_muted(&self, id: &str, muted: bool) -> Result<()> {
        let id = id.to_string();
        audio::run(move || audio::set_session_muted(&id, muted))
    }
    fn watch_audio(&self, on_change: ChangeFn) -> Result<Watch> {
        audio::watch(on_change)
    }

    fn media_state(&self) -> Result<MediaState> {
        media::run(media::state)
    }
    fn media_control(&self, action: MediaAction) -> Result<()> {
        media::run(move || media::control(action))
    }
    fn media_seek(&self, seconds: f64) -> Result<()> {
        media::run(move || media::seek(seconds))
    }
    fn watch_media(&self, on_change: ChangeFn) -> Result<Watch> {
        media::watch(on_change)
    }

    fn gpu_release(&self) {
        // Closes the PDH query (Drop); the next gpu_usage() re-primes from scratch.
        *GPU_QUERY.lock().unwrap_or_else(|p| p.into_inner()) = None;
    }

    fn gpu_usage(&self) -> Result<f32> {
        let mut slot = GPU_QUERY.lock().unwrap_or_else(|p| p.into_inner());
        if slot.is_none() {
            // A rate counter needs two collections; the first call primes it.
            *slot = Some(GpuQuery::open()?);
            return Err(PlatformError::Os("gpu counter warming up".into()));
        }
        match slot.as_ref().map(GpuQuery::sample) {
            Some(Ok(v)) => Ok(v),
            Some(Err(e)) => {
                // Drop the query so the next call starts fresh (e.g. after a driver reset).
                *slot = None;
                Err(e)
            }
            None => Err(PlatformError::Os("gpu query unavailable".into())),
        }
    }

    fn gpu_name(&self) -> Result<String> {
        // SAFETY: plain DXGI factory/adapter enumeration; no COM apartment needed.
        unsafe {
            let factory: IDXGIFactory1 = CreateDXGIFactory1().map_err(os)?;
            let mut i = 0;
            while let Ok(adapter) = factory.EnumAdapters1(i) {
                i += 1;
                let Ok(desc) = adapter.GetDesc1() else {
                    continue;
                };
                if desc.Flags & (DXGI_ADAPTER_FLAG_SOFTWARE.0 as u32) != 0 {
                    continue;
                }
                let len = desc.Description.iter().position(|&c| c == 0).unwrap_or(128);
                let name = String::from_utf16_lossy(&desc.Description[..len]);
                let name = name.trim();
                if !name.is_empty() {
                    return Ok(name.to_string());
                }
            }
        }
        Err(PlatformError::Os("no hardware GPU adapter".into()))
    }
}

/// 0–1 → 0–255 alpha.
fn alpha(opacity: f64) -> u8 {
    (opacity.clamp(0.0, 1.0) * 255.0).round() as u8
}

fn os(e: windows::core::Error) -> PlatformError {
    PlatformError::Os(e.message())
}

fn pdh(status: u32, what: &str) -> Result<()> {
    if status == 0 {
        Ok(())
    } else {
        Err(PlatformError::Os(format!("{what} failed: 0x{status:08x}")))
    }
}

static GPU_QUERY: Mutex<Option<GpuQuery>> = Mutex::new(None);

/// PDH query on `\GPU Engine(*)\Utilization Percentage`. Wildcard instances are
/// re-expanded on every collection, so processes coming and going are picked up.
struct GpuQuery {
    query: PDH_HQUERY,
    counter: PDH_HCOUNTER,
}

// SAFETY: PDH handles are process-wide and may be used from any thread; access is
// serialised by the `GPU_QUERY` mutex.
unsafe impl Send for GpuQuery {}

impl GpuQuery {
    fn open() -> Result<Self> {
        let mut query = PDH_HQUERY::default();
        // SAFETY: out-pointers are valid; once wrapped, the query is closed in Drop.
        unsafe {
            pdh(PdhOpenQueryW(PCWSTR::null(), 0, &mut query), "PdhOpenQuery")?;
            let mut q = Self {
                query,
                counter: PDH_HCOUNTER::default(),
            };
            pdh(
                PdhAddEnglishCounterW(
                    q.query,
                    w!(r"\GPU Engine(*)\Utilization Percentage"),
                    0,
                    &mut q.counter,
                ),
                "PdhAddEnglishCounter",
            )?;
            pdh(PdhCollectQueryData(q.query), "PdhCollectQueryData")?;
            Ok(q)
        }
    }

    fn sample(&self) -> Result<f32> {
        // SAFETY: handles are live (owned by self); the buffer is sized by the first call
        // and PDH writes at most `size` bytes into it.
        unsafe {
            pdh(PdhCollectQueryData(self.query), "PdhCollectQueryData")?;
            let (mut size, mut count) = (0u32, 0u32);
            let status = PdhGetFormattedCounterArrayW(
                self.counter,
                PDH_FMT_DOUBLE,
                &mut size,
                &mut count,
                None,
            );
            if status != PDH_MORE_DATA {
                pdh(status, "PdhGetFormattedCounterArray(size)")?;
                return Ok(0.0); // no GPU engine instances
            }
            let item = std::mem::size_of::<PDH_FMT_COUNTERVALUE_ITEM_W>();
            let mut buf: Vec<PDH_FMT_COUNTERVALUE_ITEM_W> =
                vec![PDH_FMT_COUNTERVALUE_ITEM_W::default(); (size as usize).div_ceil(item) + 1];
            pdh(
                PdhGetFormattedCounterArrayW(
                    self.counter,
                    PDH_FMT_DOUBLE,
                    &mut size,
                    &mut count,
                    Some(buf.as_mut_ptr()),
                ),
                "PdhGetFormattedCounterArray",
            )?;
            let mut per_engine: HashMap<String, f64> = HashMap::new();
            for it in buf.iter().take(count as usize) {
                let st = it.FmtValue.CStatus;
                if st != PDH_CSTATUS_VALID_DATA && st != PDH_CSTATUS_NEW_DATA {
                    continue;
                }
                let name = it.szName.to_string().unwrap_or_default();
                *per_engine.entry(engine_key(&name).to_string()).or_default() +=
                    it.FmtValue.Anonymous.doubleValue;
            }
            Ok(busiest(&per_engine))
        }
    }
}

impl Drop for GpuQuery {
    fn drop(&mut self) {
        // SAFETY: the query handle was opened by PdhOpenQueryW and is closed exactly once.
        unsafe {
            PdhCloseQuery(self.query);
        }
    }
}

/// Instance names look like `pid_1234_luid_0x0_0xD1C8_phys_0_eng_3_engtype_3D`; strip the
/// pid so utilisation is summed per physical engine across processes.
fn engine_key(instance: &str) -> &str {
    instance.find("luid_").map_or(instance, |i| &instance[i..])
}

fn busiest(per_engine: &HashMap<String, f64>) -> f32 {
    per_engine
        .values()
        .copied()
        .fold(0.0_f64, f64::max)
        .clamp(0.0, 100.0) as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opacity_to_alpha() {
        assert_eq!(alpha(0.2), 51);
        assert_eq!(alpha(1.0), 255);
        assert_eq!(alpha(-1.0), 0);
    }

    #[test]
    fn opacity_on_invalid_window_is_an_error() {
        assert!(matches!(
            WindowsPlatform.set_window_opacity(NativeWindow(0), 0.5),
            Err(PlatformError::Os(_))
        ));
    }

    #[test]
    fn engine_key_strips_pid() {
        assert_eq!(
            engine_key("pid_1234_luid_0x0_0xD1C8_phys_0_eng_3_engtype_3D"),
            "luid_0x0_0xD1C8_phys_0_eng_3_engtype_3D"
        );
        assert_eq!(engine_key("odd"), "odd");
    }

    #[test]
    fn busiest_engine_is_clamped() {
        let mut m = HashMap::new();
        m.insert("a".to_string(), 30.0);
        m.insert("b".to_string(), 140.0);
        assert_eq!(busiest(&m), 100.0);
        assert_eq!(busiest(&HashMap::new()), 0.0);
    }

    #[test]
    #[ignore = "reads real GPU counters"]
    fn real_gpu_counters() {
        let p = WindowsPlatform;
        let _ = p.gpu_usage();
        std::thread::sleep(std::time::Duration::from_millis(500));
        let v = p.gpu_usage().expect("gpu usage");
        assert!((0.0..=100.0).contains(&v));
        assert!(!p.gpu_name().expect("gpu name").is_empty());
        p.gpu_release();
        assert!(
            GPU_QUERY.lock().unwrap().is_none(),
            "query closed on release"
        );
    }

    /// Rough cost of one `\GPU Engine(*)` collection (wall time ≈ CPU time; it's CPU-bound).
    #[test]
    #[ignore = "measures real PDH GPU query cost"]
    fn real_gpu_query_cost() {
        let p = WindowsPlatform;
        let t = std::time::Instant::now();
        let _ = p.gpu_usage(); // open + prime
        let open = t.elapsed();
        let n = 10;
        let mut total = std::time::Duration::ZERO;
        for _ in 0..n {
            std::thread::sleep(std::time::Duration::from_millis(200));
            let t = std::time::Instant::now();
            let _ = p.gpu_usage();
            total += t.elapsed();
        }
        p.gpu_release();
        println!(
            "PDH GPU query: open+prime {:?}, sample avg {:?} over {n}",
            open,
            total / n
        );
    }
}
