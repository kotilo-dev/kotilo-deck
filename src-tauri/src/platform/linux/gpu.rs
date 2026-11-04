//! GPU utilisation from sysfs: `/sys/class/drm/card*/device/gpu_busy_percent` (amdgpu; also
//! exposed by recent i915/xe builds). NVIDIA's proprietary driver has no sysfs counter, so
//! those machines report `Unsupported`.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use crate::platform::{Capability, PlatformError, Result};

const DRM: &str = "/sys/class/drm";

/// A card with a readable busy counter, found once at startup.
struct Card {
    busy: PathBuf,
    name: String,
}

fn cards() -> &'static [Card] {
    static CARDS: OnceLock<Vec<Card>> = OnceLock::new();
    CARDS.get_or_init(|| scan(Path::new(DRM)))
}

pub fn available() -> bool {
    !cards().is_empty()
}

/// `card0`, `card1` — not connectors (`card0-DP-1`) or render nodes.
pub fn is_card_dir(name: &str) -> bool {
    name.strip_prefix("card")
        .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
}

pub fn parse_busy(s: &str) -> Option<f32> {
    let v = s.trim().parse::<f32>().ok()?;
    v.is_finite().then(|| v.clamp(0.0, 100.0))
}

fn read_trim(p: &Path) -> Option<String> {
    let s = fs::read_to_string(p).ok()?;
    let s = s.trim();
    (!s.is_empty()).then(|| s.to_string())
}

/// `PCI_ID=1002:73BF` from a `uevent` file → ("0x1002", "0x73bf").
pub fn pci_id_from_uevent(uevent: &str) -> Option<(String, String)> {
    let id = uevent
        .lines()
        .find_map(|l| l.trim().strip_prefix("PCI_ID="))?;
    let (v, d) = id.split_once(':')?;
    Some((
        format!("0x{}", v.to_ascii_lowercase()),
        format!("0x{}", d.to_ascii_lowercase()),
    ))
}

/// Best-effort display name: the driver's `product_name` when present, else vendor + PCI
/// device id (`AMD GPU 0x73bf`).
pub fn gpu_label(product: Option<&str>, vendor: Option<&str>, device: Option<&str>) -> String {
    if let Some(p) = product.map(str::trim).filter(|p| !p.is_empty()) {
        return p.to_string();
    }
    let vendor_name = match vendor.map(|v| v.trim().to_ascii_lowercase()).as_deref() {
        Some("0x1002") => "AMD",
        Some("0x10de") => "NVIDIA",
        Some("0x8086") => "Intel",
        _ => "",
    };
    let dev = device.map(str::trim).filter(|d| !d.is_empty());
    match (vendor_name, dev) {
        ("", None) => "GPU".to_string(),
        ("", Some(d)) => format!("GPU {d}"),
        (v, None) => format!("{v} GPU"),
        (v, Some(d)) => format!("{v} GPU {d}"),
    }
}

fn card_name(dev: &Path) -> String {
    let product = read_trim(&dev.join("product_name"));
    let mut vendor = read_trim(&dev.join("vendor"));
    let mut device = read_trim(&dev.join("device"));
    if vendor.is_none() || device.is_none() {
        if let Some((v, d)) = read_trim(&dev.join("uevent")).and_then(|u| pci_id_from_uevent(&u)) {
            vendor = vendor.or(Some(v));
            device = device.or(Some(d));
        }
    }
    gpu_label(product.as_deref(), vendor.as_deref(), device.as_deref())
}

fn scan(drm: &Path) -> Vec<Card> {
    let Ok(entries) = fs::read_dir(drm) else {
        return Vec::new();
    };
    let mut names: Vec<String> = entries
        .filter_map(|e| e.ok())
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|n| is_card_dir(n))
        .collect();
    names.sort_by_key(|n| n[4..].parse::<u32>().unwrap_or(u32::MAX));
    names
        .into_iter()
        .filter_map(|n| {
            let dev = drm.join(&n).join("device");
            let busy = dev.join("gpu_busy_percent");
            read_trim(&busy).as_deref().and_then(parse_busy)?;
            Some(Card {
                name: card_name(&dev),
                busy,
            })
        })
        .collect()
}

/// Busiest card, 0–100.
pub fn usage() -> Result<f32> {
    let cards = cards();
    if cards.is_empty() {
        return Err(PlatformError::Unsupported(Capability::Gpu));
    }
    cards
        .iter()
        .filter_map(|c| read_trim(&c.busy).as_deref().and_then(parse_busy))
        .reduce(f32::max)
        .ok_or_else(|| PlatformError::Os("gpu_busy_percent unreadable".into()))
}

pub fn name() -> Result<String> {
    cards()
        .first()
        .map(|c| c.name.clone())
        .ok_or(PlatformError::Unsupported(Capability::Gpu))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn card_dirs() {
        assert!(is_card_dir("card0"));
        assert!(is_card_dir("card12"));
        assert!(!is_card_dir("card0-DP-1"));
        assert!(!is_card_dir("card"));
        assert!(!is_card_dir("renderD128"));
    }

    #[test]
    fn busy_values() {
        assert_eq!(parse_busy("37\n"), Some(37.0));
        assert_eq!(parse_busy("140"), Some(100.0));
        assert_eq!(parse_busy(""), None);
        assert_eq!(parse_busy("NaN"), None);
    }

    #[test]
    fn labels() {
        assert_eq!(
            gpu_label(Some("AMD Radeon RX 6800 XT"), Some("0x1002"), None),
            "AMD Radeon RX 6800 XT"
        );
        assert_eq!(
            gpu_label(Some(" "), Some("0x1002\n"), Some("0x73bf")),
            "AMD GPU 0x73bf"
        );
        assert_eq!(gpu_label(None, Some("0x8086"), None), "Intel GPU");
        assert_eq!(gpu_label(None, Some("0x1234"), Some("0x1")), "GPU 0x1");
        assert_eq!(gpu_label(None, None, None), "GPU");
        assert_eq!(
            pci_id_from_uevent("DRIVER=amdgpu\nPCI_CLASS=30000\nPCI_ID=1002:73BF\n"),
            Some(("0x1002".into(), "0x73bf".into()))
        );
        assert_eq!(pci_id_from_uevent("DRIVER=amdgpu"), None);
    }

    #[test]
    fn scans_a_fake_sysfs_tree() {
        let root = tempfile::tempdir().unwrap();
        let drm = root.path();
        let mk = |card: &str, files: &[(&str, &str)]| {
            let dev = drm.join(card).join("device");
            fs::create_dir_all(&dev).unwrap();
            for (f, v) in files {
                fs::write(dev.join(f), v).unwrap();
            }
        };
        // NVIDIA (no counter), then an AMD dGPU, a connector dir and an AMD card via uevent.
        mk("card0", &[("vendor", "0x10de\n"), ("device", "0x2204\n")]);
        mk(
            "card1",
            &[
                ("vendor", "0x1002\n"),
                ("device", "0x73bf\n"),
                ("gpu_busy_percent", "12\n"),
            ],
        );
        mk("card1-DP-1", &[("gpu_busy_percent", "99\n")]);
        mk(
            "card10",
            &[
                ("uevent", "DRIVER=amdgpu\nPCI_ID=1002:164E\n"),
                ("gpu_busy_percent", "3\n"),
            ],
        );
        let cards = scan(drm);
        let names: Vec<&str> = cards.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["AMD GPU 0x73bf", "AMD GPU 0x164e"]);
        assert!(scan(&drm.join("missing")).is_empty());
    }
}
