//! Plugin file serving helpers: URL parsing for both scheme forms, path resolution that
//! refuses traversal and symlink escapes, folder limits and MIME types.

use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use super::manifest::{code, valid_relative_path, Rejection};
use super::SCHEME;

/// Files per plugin folder that may be served (plugin-api.md ▸ Folder).
pub const MAX_FILES: usize = 1000;
/// Total bytes per plugin folder.
pub const MAX_FOLDER_BYTES: u64 = 20 * 1024 * 1024;
/// Directories visited per folder before the walk gives up (pathological trees).
const MAX_DIRS: usize = 1000;

/// The iframe origin of the plugin scheme inside the Tauri webview.
pub fn scheme_base() -> String {
    if cfg!(windows) {
        format!("http://{SCHEME}.localhost")
    } else {
        format!("{SCHEME}://localhost")
    }
}

/// `scheme_base()/<folder>/<entry>`, each path segment percent-encoded.
pub fn entry_url(folder: &str, entry: &str) -> String {
    let path: Vec<String> = entry.split('/').map(encode_segment).collect();
    format!("{}/{folder}/{}", scheme_base(), path.join("/"))
}

fn encode_segment(seg: &str) -> String {
    let mut out = String::with_capacity(seg.len());
    for b in seg.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// Strict percent-decoding; `None` for malformed escapes or non-UTF-8 results.
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

/// Maps a request URL to `(folder, path)`. Accepts
/// `http(s)://kotilo-widget.localhost/<folder>/<path>` (Windows/WebView2) and
/// `kotilo-widget://localhost/<folder>/<path>` (Linux/macOS). Query and fragment are ignored;
/// the path is percent-decoded but not yet validated (see [`resolve`]).
pub fn parse_url(url: &str) -> Option<(String, String)> {
    let prefixes = [
        format!("http://{SCHEME}.localhost/"),
        format!("https://{SCHEME}.localhost/"),
        format!("{SCHEME}://localhost/"),
    ];
    let prefix = prefixes.iter().find(|p| {
        url.len() >= p.len() && url.as_bytes()[..p.len()].eq_ignore_ascii_case(p.as_bytes())
    })?;
    // The matched prefix is ASCII, so this is a char boundary.
    let rest = &url[prefix.len()..];
    let rest = rest.split(['?', '#']).next().unwrap_or_default();
    let (folder, path) = rest.split_once('/').unwrap_or((rest, ""));
    Some((percent_decode(folder)?, percent_decode(path)?))
}

/// Windows reserved device names (`con`, `nul.txt`, …) must never reach the filesystem.
fn is_device_name(seg: &str) -> bool {
    let stem = seg.split('.').next().unwrap_or_default().trim_end();
    let upper = stem.to_ascii_uppercase();
    matches!(
        upper.as_str(),
        "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$"
    ) || ((upper.starts_with("COM") || upper.starts_with("LPT"))
        && upper.len() == 4
        && upper.as_bytes()[3].is_ascii_digit())
}

/// Canonical path of the regular file `rel` inside `dir`, or `None` when `rel` is not a
/// valid relative path, does not exist, is not a file, or resolves (through `..` or a
/// symlink anywhere on the way) to somewhere outside `dir`.
pub fn resolve(dir: &Path, rel: &str) -> Option<PathBuf> {
    if !valid_relative_path(rel) || rel.split('/').any(is_device_name) {
        return None;
    }
    let root = fs::canonicalize(dir).ok()?;
    let mut joined = root.clone();
    for seg in rel.split('/') {
        joined.push(seg);
    }
    let target = fs::canonicalize(&joined).ok()?;
    (target.starts_with(&root) && target.is_file()).then_some(target)
}

/// Reads the file `rel` in `dir` (see [`resolve`]), at most [`MAX_FOLDER_BYTES`].
pub fn read(dir: &Path, rel: &str) -> Option<Vec<u8>> {
    let path = resolve(dir, rel)?;
    let file = fs::File::open(path).ok()?;
    if file.metadata().ok()?.len() > MAX_FOLDER_BYTES {
        return None;
    }
    let mut bytes = Vec::new();
    file.take(MAX_FOLDER_BYTES + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    (bytes.len() as u64 <= MAX_FOLDER_BYTES).then_some(bytes)
}

/// Checks the per-folder limits without following symlinked directories.
pub fn check_limits(dir: &Path) -> Result<(), Rejection> {
    let too_large = |msg: String| Rejection::new(code::TOO_LARGE, msg);
    let mut files = 0usize;
    let mut bytes = 0u64;
    let mut dirs = 0usize;
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        dirs += 1;
        if dirs > MAX_DIRS {
            return Err(too_large(format!("more than {MAX_DIRS} sub-folders")));
        }
        let entries = fs::read_dir(&d).map_err(|e| format!("cannot read folder: {e}"))?;
        for entry in entries.filter_map(Result::ok) {
            let Ok(ft) = entry.file_type() else { continue };
            if ft.is_dir() {
                stack.push(entry.path());
                continue;
            }
            files += 1;
            if files > MAX_FILES {
                return Err(too_large(format!("more than {MAX_FILES} files")));
            }
            if ft.is_file() {
                bytes += entry.metadata().map(|m| m.len()).unwrap_or(0);
                if bytes > MAX_FOLDER_BYTES {
                    return Err(too_large("folder is larger than 20 MB".into()));
                }
            }
        }
    }
    Ok(())
}

/// Content-Type by extension; unknown types are served as opaque bytes (with `nosniff`).
pub fn mime_for(path: &str) -> &'static str {
    let ext = path
        .rsplit_once('.')
        .map(|(_, e)| e.to_ascii_lowercase())
        .unwrap_or_default();
    match ext.as_str() {
        "html" | "htm" => "text/html; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "json" | "map" => "application/json; charset=utf-8",
        "txt" | "md" => "text/plain; charset=utf-8",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "avif" => "image/avif",
        "ico" => "image/x-icon",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        "ttf" => "font/ttf",
        "otf" => "font/otf",
        "mp3" => "audio/mpeg",
        "ogg" | "oga" => "audio/ogg",
        "wav" => "audio/wav",
        "mp4" => "video/mp4",
        "webm" => "video/webm",
        "wasm" => "application/wasm",
        _ => "application/octet-stream",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_both_url_forms() {
        let p = |u: &str| parse_url(u);
        let want = Some(("hello".to_string(), "index.html".to_string()));
        assert_eq!(p("http://kotilo-widget.localhost/hello/index.html"), want);
        assert_eq!(p("https://kotilo-widget.localhost/hello/index.html"), want);
        assert_eq!(p("kotilo-widget://localhost/hello/index.html"), want);
        assert_eq!(p("HTTP://Kotilo-Widget.LOCALHOST/hello/index.html"), want);
        assert_eq!(
            p("kotilo-widget://localhost/hello/index.html?x=1#top"),
            want
        );
        assert_eq!(
            p("kotilo-widget://localhost/hello/img/a%20b.png"),
            Some(("hello".into(), "img/a b.png".into()))
        );
        assert_eq!(
            p("http://kotilo-widget.localhost/_sdk/kotilo-widget.js"),
            Some(("_sdk".into(), "kotilo-widget.js".into()))
        );
        assert_eq!(
            p("http://kotilo-widget.localhost/hello"),
            Some(("hello".into(), "".into()))
        );
        // Encoded traversal decodes to `..`, which `resolve` refuses.
        assert_eq!(
            p("http://kotilo-widget.localhost/hello/%2e%2e/x"),
            Some(("hello".into(), "../x".into()))
        );
        assert_eq!(p("http://other.localhost/hello/index.html"), None);
        assert_eq!(p("http://tauri.localhost/index.html"), None);
        assert_eq!(p("http://kotilo-widget.localhost/hello/%zz"), None);
        assert_eq!(p("http://kotilo-widget.localhost/hello/%ff"), None);
    }

    #[test]
    fn entry_urls_are_per_os_and_encoded() {
        let url = entry_url("hello", "pages/my page.html");
        if cfg!(windows) {
            assert_eq!(
                url,
                "http://kotilo-widget.localhost/hello/pages/my%20page.html"
            );
        } else {
            assert_eq!(url, "kotilo-widget://localhost/hello/pages/my%20page.html");
        }
        assert_eq!(
            parse_url(&url),
            Some(("hello".into(), "pages/my page.html".into()))
        );
    }

    #[test]
    fn mime_mapping() {
        assert_eq!(mime_for("index.html"), "text/html; charset=utf-8");
        assert_eq!(mime_for("a/B.HTM"), "text/html; charset=utf-8");
        assert_eq!(mime_for("app.js"), "text/javascript; charset=utf-8");
        assert_eq!(mime_for("m.mjs"), "text/javascript; charset=utf-8");
        assert_eq!(mime_for("s.css"), "text/css; charset=utf-8");
        assert_eq!(mime_for("d.json"), "application/json; charset=utf-8");
        assert_eq!(mime_for("i.svg"), "image/svg+xml");
        assert_eq!(mime_for("i.PNG"), "image/png");
        assert_eq!(mime_for("i.jpeg"), "image/jpeg");
        assert_eq!(mime_for("f.woff2"), "font/woff2");
        assert_eq!(mime_for("v.webm"), "video/webm");
        assert_eq!(mime_for("noext"), "application/octet-stream");
        assert_eq!(mime_for("x.exe"), "application/octet-stream");
    }

    #[test]
    fn resolve_rejects_traversal() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("hello");
        fs::create_dir_all(dir.join("sub")).unwrap();
        fs::write(dir.join("index.html"), "<p>hi</p>").unwrap();
        fs::write(dir.join("sub/a.js"), "1").unwrap();
        fs::write(tmp.path().join("secret.txt"), "s").unwrap();

        assert!(resolve(&dir, "index.html").is_some());
        assert_eq!(read(&dir, "sub/a.js").as_deref(), Some(&b"1"[..]));
        for bad in [
            "../secret.txt",
            "sub/../../secret.txt",
            "/secret.txt",
            "sub\\..\\..\\secret.txt",
            "sub",
            "missing.html",
            "",
            "nul",
            "con.html",
            "COM1.txt",
        ] {
            assert!(read(&dir, bad).is_none(), "{bad:?}");
        }
        let abs = tmp.path().join("secret.txt").to_string_lossy().into_owned();
        assert!(read(&dir, &abs).is_none());
    }

    #[cfg(unix)]
    fn symlink_file(src: &Path, dst: &Path) -> bool {
        std::os::unix::fs::symlink(src, dst).is_ok()
    }

    #[cfg(windows)]
    fn symlink_file(src: &Path, dst: &Path) -> bool {
        // Needs Developer Mode or admin; the test skips itself otherwise.
        std::os::windows::fs::symlink_file(src, dst).is_ok()
    }

    #[test]
    fn resolve_rejects_symlink_escape() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("hello");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("inside.txt"), "ok").unwrap();
        fs::write(tmp.path().join("secret.txt"), "s").unwrap();
        if !symlink_file(&tmp.path().join("secret.txt"), &dir.join("leak.txt")) {
            eprintln!("symlinks unavailable here; skipping");
            return;
        }
        assert!(read(&dir, "leak.txt").is_none());
        if symlink_file(&dir.join("inside.txt"), &dir.join("alias.txt")) {
            assert_eq!(read(&dir, "alias.txt").as_deref(), Some(&b"ok"[..]));
        }
    }

    /// Directory junctions need no privileges, so this escape test always runs on Windows.
    #[cfg(windows)]
    #[test]
    fn resolve_rejects_junction_escape() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("hello");
        let outside = tmp.path().join("outside");
        fs::create_dir_all(&dir).unwrap();
        fs::create_dir_all(&outside).unwrap();
        fs::write(outside.join("secret.txt"), "s").unwrap();
        let status = std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(dir.join("j"))
            .arg(&outside)
            .output()
            .unwrap();
        assert!(status.status.success(), "mklink /J failed");
        assert!(dir.join("j/secret.txt").is_file());
        assert!(read(&dir, "j/secret.txt").is_none());
    }

    #[test]
    fn folder_limits() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        fs::write(dir.join("index.html"), "x").unwrap();
        assert!(check_limits(dir).is_ok());
        let many = dir.join("many");
        fs::create_dir(&many).unwrap();
        for i in 0..MAX_FILES {
            fs::write(many.join(format!("{i}.txt")), "").unwrap();
        }
        let err = check_limits(dir).unwrap_err();
        assert!(err.message.contains("1000 files"), "{err:?}");
        assert_eq!(err.code, code::TOO_LARGE);

        let big = tempfile::tempdir().unwrap();
        let f = fs::File::create(big.path().join("big.bin")).unwrap();
        f.set_len(MAX_FOLDER_BYTES + 1).unwrap();
        let err = check_limits(big.path()).unwrap_err();
        assert!(err.message.contains("20 MB"), "{err:?}");
        assert_eq!(err.code, code::TOO_LARGE);
    }

    #[test]
    fn percent_decoding() {
        assert_eq!(percent_decode("a%20b").as_deref(), Some("a b"));
        assert_eq!(percent_decode("%C3%A7").as_deref(), Some("ç"));
        assert_eq!(percent_decode("%"), None);
        assert_eq!(percent_decode("%4"), None);
    }
}
