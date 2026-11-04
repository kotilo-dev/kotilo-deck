//! Static files for LAN browsers: SvelteKit prerender paths, MIME types, the page CSP
//! (with hashes of the SPA's inline bootstrap script) and safe path decoding.

use base64::Engine;
use sha2::{Digest, Sha256};

/// One resolved file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Asset {
    pub bytes: Vec<u8>,
    pub mime: String,
}

/// Page CSP for LAN browsers; `{scripts}` gets the inline-script hashes. Everything is
/// same-origin: the backend does every network call, `ws:` is for `/api/events`, plugins
/// load from `/plugins/…` in sandboxed iframes.
const PAGE_CSP: &str = "default-src 'self'; script-src 'self'{scripts}; style-src 'self' 'unsafe-inline'; font-src 'self' data:; img-src 'self' data:; media-src 'self'; connect-src 'self' ws:; frame-src 'self'; object-src 'none'; base-uri 'none'; form-action 'none'; frame-ancestors 'none'";

/// Decodes `%XX` escapes; `None` for malformed escapes or non-UTF-8 results.
pub fn percent_decode(s: &str) -> Option<String> {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' {
            let hex = s.get(i + 1..i + 3)?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

/// Splits an URL path into decoded segments, rejecting anything that could leave the
/// served folder: empty, `.`/`..`, separators or NUL/control chars inside a segment,
/// drive prefixes (`C:`).
pub fn safe_segments(path: &str) -> Option<Vec<String>> {
    let mut out = Vec::new();
    for raw in path.split('/') {
        let seg = percent_decode(raw)?;
        let bad = seg.is_empty()
            || seg == "."
            || seg == ".."
            || seg.contains(['/', '\\', ':'])
            || seg.chars().any(char::is_control);
        if bad {
            return None;
        }
        out.push(seg);
    }
    (!out.is_empty()).then_some(out)
}

fn extension(path: &str) -> Option<&str> {
    let name = path.rsplit('/').next()?;
    name.rsplit_once('.').map(|(_, e)| e)
}

/// Content-Type by extension (the SPA build, wallpapers).
pub fn mime_for(path: &str) -> &'static str {
    match extension(path).map(str::to_ascii_lowercase).as_deref() {
        Some("html") => "text/html; charset=utf-8",
        Some("js" | "mjs") => "text/javascript; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("json" | "map") => "application/json",
        Some("webmanifest") => "application/manifest+json",
        Some("txt") => "text/plain; charset=utf-8",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("webp") => "image/webp",
        Some("gif") => "image/gif",
        Some("ico") => "image/x-icon",
        Some("woff") => "font/woff",
        Some("woff2") => "font/woff2",
        Some("ttf") => "font/ttf",
        Some("otf") => "font/otf",
        Some("mp4") => "video/mp4",
        Some("webm") => "video/webm",
        _ => "application/octet-stream",
    }
}

/// Asset keys to try for an URL path, in order (adapter-static prerenders `/` →
/// `index.html` and `/settings` → `settings.html`). Extension-less paths fall back to the
/// SPA shell `index.html`; a missing file with an extension is a 404.
pub fn candidates(path: &str) -> Option<Vec<String>> {
    let trimmed = path.trim_matches('/');
    if trimmed.is_empty() {
        return Some(vec!["index.html".into()]);
    }
    let key = safe_segments(trimmed)?.join("/");
    if extension(&key).is_some() {
        return Some(vec![key]);
    }
    Some(vec![
        format!("{key}.html"),
        format!("{key}/index.html"),
        "index.html".into(),
    ])
}

/// `Cache-Control` for a static key: hashed build output is immutable, the rest revalidates.
pub fn cache_control(key: &str) -> &'static str {
    if key.starts_with("_app/immutable/") {
        "private, max-age=31536000, immutable"
    } else {
        "no-cache"
    }
}

/// `'sha256-…'` for every inline `<script>` (no `src`) in an HTML page.
fn inline_script_hashes(html: &str) -> Vec<String> {
    let mut out = Vec::new();
    let lower = html.to_ascii_lowercase();
    let mut at = 0;
    while let Some(open) = lower[at..].find("<script") {
        let open = at + open;
        let Some(tag_end) = lower[open..].find('>').map(|i| open + i + 1) else {
            break;
        };
        let Some(close) = lower[tag_end..].find("</script").map(|i| tag_end + i) else {
            break;
        };
        if !lower[open..tag_end].contains(" src") {
            let digest = Sha256::digest(&html.as_bytes()[tag_end..close]);
            let b64 = base64::engine::general_purpose::STANDARD.encode(digest);
            out.push(format!("'sha256-{b64}'"));
        }
        at = close;
    }
    out
}

/// CSP header for an HTML page.
pub fn page_csp(html: &[u8]) -> String {
    let html = String::from_utf8_lossy(html);
    let scripts: String = inline_script_hashes(&html)
        .into_iter()
        .map(|h| format!(" {h}"))
        .collect();
    PAGE_CSP.replace("{scripts}", &scripts)
}

/// Reads `<build>/<key>` from the frontend build folder (debug builds only: dev uses the
/// vite devUrl, so nothing is embedded). `key` comes from [`candidates`].
#[cfg(debug_assertions)]
pub fn from_build_dir(key: &str) -> Option<Asset> {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../build");
    let bytes = std::fs::read(dir.join(key)).ok()?;
    Some(Asset {
        bytes,
        mime: mime_for(key).to_string(),
    })
}

#[cfg(not(debug_assertions))]
pub fn from_build_dir(_key: &str) -> Option<Asset> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prerender_paths() {
        assert_eq!(candidates("/").unwrap(), ["index.html"]);
        assert_eq!(
            candidates("/settings").unwrap(),
            ["settings.html", "settings/index.html", "index.html"]
        );
        assert_eq!(
            candidates("/_app/immutable/entry/start.js").unwrap(),
            ["_app/immutable/entry/start.js"]
        );
    }

    #[test]
    fn traversal_is_rejected() {
        for bad in [
            "/../secret",
            "/a/../../b.js",
            "/%2e%2e/x.js",
            "/a%2fb.js",
            "/a%5cb.js",
            "/C:/x.js",
            "/a//b.js",
            "/%zz.js",
            "/a%00.js",
        ] {
            assert!(candidates(bad).is_none(), "{bad}");
        }
        assert_eq!(safe_segments("a/b%20c.png").unwrap(), ["a", "b c.png"]);
    }

    #[test]
    fn csp_hashes_inline_scripts_only() {
        let html = br#"<head><script src="/a.js"></script><SCRIPT>boot()</SCRIPT></head>"#;
        let csp = page_csp(html);
        let expected = base64::engine::general_purpose::STANDARD.encode(Sha256::digest(b"boot()"));
        assert!(
            csp.contains(&format!("script-src 'self' 'sha256-{expected}';")),
            "{csp}"
        );
        assert_eq!(csp.matches("sha256-").count(), 1);
        assert!(csp.contains("connect-src 'self' ws:"));
        assert!(csp.contains("frame-src 'self'"));
        assert!(csp.contains("frame-ancestors 'none'"));
    }

    #[test]
    fn mime_and_cache() {
        assert_eq!(mime_for("x/Y.JPG"), "image/jpeg");
        assert_eq!(mime_for("index.html"), "text/html; charset=utf-8");
        assert_eq!(mime_for("noext"), "application/octet-stream");
        assert!(cache_control("_app/immutable/a.js").contains("immutable"));
        assert_eq!(cache_control("index.html"), "no-cache");
    }
}
