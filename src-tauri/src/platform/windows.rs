use std::collections::HashMap;
use std::sync::Mutex;

use windows::core::{w, PCWSTR};
use windows::Win32::Graphics::Dxgi::{
    CreateDXGIFactory1, IDXGIFactory1, DXGI_ADAPTER_FLAG_SOFTWARE,
};
use windows::Win32::System::Performance::{
    PdhAddEnglishCounterW, PdhCloseQuery, PdhCollectQueryData, PdhGetFormattedCounterArrayW,
    PdhOpenQueryW, PDH_CSTATUS_NEW_DATA, PDH_CSTATUS_VALID_DATA, PDH_FMT_COUNTERVALUE_ITEM_W,
    PDH_FMT_DOUBLE, PDH_HCOUNTER, PDH_HQUERY, PDH_MORE_DATA,
};

use super::{Capability, Platform, PlatformError, Result};

/// Windows implementation (windows-rs). Capabilities are added as each phase lands.
pub struct WindowsPlatform;

impl Platform for WindowsPlatform {
    fn capabilities(&self) -> Vec<Capability> {
        vec![
            Capability::Monitors,
            Capability::SystemInfo,
            Capability::Gpu,
        ]
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
    }
}
