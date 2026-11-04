//! Connection URLs and the QR code shown in Settings ▸ LAN. Both carry the token, so they
//! are only ever returned to the host UI and authenticated LAN clients, never logged.

use std::net::Ipv4Addr;

use qrcode::render::svg;
use qrcode::QrCode;

/// Non-loopback, non-link-local IPv4 addresses of interfaces that are up; private LAN
/// ranges first so `urls[0]` (the QR code) is the one a tablet can most likely reach.
pub fn local_ipv4s() -> Vec<Ipv4Addr> {
    let ifaces = match if_addrs::get_if_addrs() {
        Ok(v) => v,
        Err(e) => {
            tracing::debug!(kind = ?e.kind(), "cannot list network interfaces");
            return Vec::new();
        }
    };
    let ips = ifaces.into_iter().filter_map(|i| {
        let up = i.is_oper_up();
        match i.addr {
            if_addrs::IfAddr::V4(v4) if up => Some(v4.ip),
            _ => None,
        }
    });
    order(ips)
}

fn order(ips: impl IntoIterator<Item = Ipv4Addr>) -> Vec<Ipv4Addr> {
    let mut v: Vec<Ipv4Addr> = ips
        .into_iter()
        .filter(|ip| !ip.is_loopback() && !ip.is_link_local() && !ip.is_unspecified())
        .collect();
    v.sort_by_key(|ip| (!ip.is_private(), *ip));
    v.dedup();
    v
}

pub fn urls(ips: &[Ipv4Addr], port: u16, token: &str) -> Vec<String> {
    ips.iter()
        .map(|ip| format!("http://{ip}:{port}/?token={token}"))
        .collect()
}

/// Pixels per QR module.
const QR_MODULE: u32 = 6;

/// Black-on-white SVG with its own white background rect and the standard 4-module quiet
/// zone, so it scans on dark theme surfaces (it is shown as an image, never themed).
pub fn qr_svg(data: &str) -> Option<String> {
    let code = QrCode::new(data.as_bytes()).ok()?;
    Some(
        code.render::<svg::Color>()
            .module_dimensions(QR_MODULE, QR_MODULE)
            .dark_color(svg::Color("#000000"))
            .light_color(svg::Color("#ffffff"))
            .quiet_zone(true)
            .build(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn private_addresses_first_without_loopback_or_link_local() {
        let got = order([
            Ipv4Addr::new(127, 0, 0, 1),
            Ipv4Addr::new(169, 254, 3, 4),
            Ipv4Addr::new(84, 1, 2, 3),
            Ipv4Addr::new(192, 168, 1, 20),
            Ipv4Addr::new(10, 0, 0, 5),
            Ipv4Addr::new(10, 0, 0, 5),
        ]);
        assert_eq!(
            got,
            [
                Ipv4Addr::new(10, 0, 0, 5),
                Ipv4Addr::new(192, 168, 1, 20),
                Ipv4Addr::new(84, 1, 2, 3)
            ]
        );
    }

    #[test]
    fn urls_and_qr() {
        let u = urls(&[Ipv4Addr::new(10, 0, 0, 5)], 47832, "abc");
        assert_eq!(u, ["http://10.0.0.5:47832/?token=abc"]);
        let svg = qr_svg(&u[0]).unwrap();
        assert!(svg.contains("<svg") && svg.contains("#000000"));
        // White background covering the whole image.
        let w = svg
            .split("width=\"")
            .nth(1)
            .and_then(|s| s.split('"').next())
            .unwrap()
            .to_string();
        assert!(svg.contains(&format!(
            "<rect x=\"0\" y=\"0\" width=\"{w}\" height=\"{w}\" fill=\"#ffffff\"/>"
        )));
        // No dark module inside the 4-module quiet zone.
        let d = svg.split(" d=\"").nth(1).unwrap();
        let min = d
            .split('M')
            .filter_map(|p| {
                let mut it = p.split(['h', ' ']);
                let x: u32 = it.next()?.parse().ok()?;
                let y: u32 = it.next()?.parse().ok()?;
                Some(x.min(y))
            })
            .min()
            .unwrap();
        assert!(min >= 4 * QR_MODULE, "quiet zone {min}px");
        let w: u32 = w.parse().unwrap();
        assert_eq!(w % QR_MODULE, 0);
    }
}
