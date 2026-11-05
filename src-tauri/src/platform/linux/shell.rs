//! Custom URL schemes (`kotilo://`): `xdg-mime` lookup, then a detached `xdg-open`; also
//! folders (`open_path`).
//!
//! For custom schemes the lookup comes first so a missing handler is an error rather than a
//! desktop "choose an application" dialog. `http`/`https` skip it: every desktop has a
//! browser, and inside Flatpak (flatpak-xdg-utils ships `xdg-open` via the portal but no
//! `xdg-mime`) or on minimal desktops the lookup fails even though `xdg-open` works.

use std::path::Path;
use std::process::Stdio;

use super::cmd;
use crate::platform::shared::scheme_of;
use crate::platform::{PlatformError, Result};

/// `xdg-mime query default` output → the handler's `.desktop` id, if any.
pub fn handler_from(out: &str) -> Option<&str> {
    out.lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .filter(|l| l.ends_with(".desktop"))
}

fn handler(xdg_mime: &Path, scheme: &str) -> Option<String> {
    let mime = format!("x-scheme-handler/{scheme}");
    let out = cmd::run(xdg_mime, ["query", "default", mime.as_str()]).ok()?;
    handler_from(&out).map(str::to_string)
}

/// Whether `scheme` needs a registered handler before `xdg-open` (custom schemes do; web
/// links go straight to `xdg-open`).
pub fn needs_handler_check(scheme: &str) -> bool {
    !(scheme.eq_ignore_ascii_case("http") || scheme.eq_ignore_ascii_case("https"))
}

pub fn open_url_scheme(url: &str) -> Result<()> {
    let scheme = scheme_of(url).ok_or_else(|| PlatformError::Os("not a URL".into()))?;
    if !needs_handler_check(scheme) {
        // The URL starts with a letter (`scheme_of`), so it can never be parsed as an option.
        return xdg_open(std::ffi::OsStr::new(url));
    }
    let not_registered =
        || PlatformError::Os(format!("no app is registered for {scheme}:// links"));
    let xdg_mime = cmd::which("xdg-mime").ok_or_else(not_registered)?;
    if handler(&xdg_mime, scheme).is_none() {
        return Err(not_registered());
    }
    // The URL starts with a letter (`scheme_of`), so it can never be parsed as an option.
    xdg_open(std::ffi::OsStr::new(url))
}

/// Opens a local folder in the file manager (`xdg-open <dir>`).
pub fn open_path(path: &Path) -> Result<()> {
    if !path.is_absolute() {
        // Absolute paths start with `/`, so they can never be parsed as an option.
        return Err(PlatformError::Os("path must be absolute".into()));
    }
    xdg_open(path.as_os_str())
}

/// Spawns `xdg-open <target>` detached and reaps it in the background.
fn xdg_open(target: &std::ffi::OsStr) -> Result<()> {
    let xdg_open = cmd::which("xdg-open")
        .ok_or_else(|| PlatformError::Os("xdg-open is not installed".into()))?;
    let mut c = cmd::command(&xdg_open);
    c.arg(target).stdout(Stdio::null()).stderr(Stdio::null());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // Own process group: the opened app does not get our terminal's signals.
        c.process_group(0);
    }
    let mut child = c
        .spawn()
        .map_err(|e| PlatformError::Os(format!("cannot open: {e}")))?;
    // Detached: reap it in the background so no zombie is left behind.
    let _ = std::thread::Builder::new()
        .name("xdg-open-reaper".into())
        .spawn(move || {
            let _ = child.wait();
        });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xdg_mime_output() {
        assert_eq!(handler_from("kotilo.desktop\n"), Some("kotilo.desktop"));
        assert_eq!(
            handler_from("\n  org.kotilo.Desktop.desktop  \n"),
            Some("org.kotilo.Desktop.desktop")
        );
        assert_eq!(handler_from(""), None, "no handler → never open a picker");
        assert_eq!(handler_from("\n"), None);
        assert_eq!(handler_from("xdg-mime: unknown mode"), None);
    }

    #[test]
    fn web_links_skip_the_handler_check() {
        assert!(!needs_handler_check("http"));
        assert!(!needs_handler_check("https"));
        assert!(!needs_handler_check("HTTPS"));
        assert!(needs_handler_check("kotilo"));
        assert!(needs_handler_check("httpx"));
        assert!(needs_handler_check("mailto"));
    }

    #[test]
    fn invalid_url_is_an_error() {
        assert!(matches!(
            open_url_scheme("not a url"),
            Err(PlatformError::Os(_))
        ));
    }
}
