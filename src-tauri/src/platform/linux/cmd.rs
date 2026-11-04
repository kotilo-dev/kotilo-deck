//! Short-lived helper processes (`pactl`, `wpctl`, `xdg-mime`). Callers are already off the
//! async runtime (`spawn_blocking`), but a hung sound server must still not hang a command
//! forever, so every run has a timeout after which the child is killed and reaped.

use std::ffi::OsStr;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

use crate::platform::{PlatformError, Result};

/// Upper bound for one helper invocation.
pub const TIMEOUT: Duration = Duration::from_secs(3);

/// Full path of `name` on `PATH`, if it is an (executable) file.
pub fn which(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    find_in(&path, name)
}

fn find_in(path: &OsStr, name: &str) -> Option<PathBuf> {
    std::env::split_paths(path)
        .filter(|dir| dir.is_absolute())
        .map(|dir| dir.join(name))
        .find(|p| is_executable(p))
}

#[cfg(unix)]
fn is_executable(p: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(p).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
}

#[cfg(not(unix))]
fn is_executable(p: &Path) -> bool {
    p.is_file()
}

/// A command with a C locale, so helper output (`pactl subscribe` lines, float formatting in
/// `pactl -f json`) is never translated or written with a decimal comma.
pub fn command(program: &Path) -> Command {
    let mut c = Command::new(program);
    c.env("LC_ALL", "C").env("LANG", "C").stdin(Stdio::null());
    c
}

/// Runs `program args…`, returning stdout on exit status 0; an `Os` error carrying stderr
/// (first line) otherwise.
pub fn run<I, S>(program: &Path, args: I) -> Result<String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let label = program
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let mut child = command(program)
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| PlatformError::Os(format!("cannot run {label}: {e}")))?;
    let mut stdout = child.stdout.take();
    let mut stderr = child.stderr.take();
    let (tx, rx) = mpsc::sync_channel::<(Vec<u8>, Vec<u8>)>(1);
    let reader = std::thread::Builder::new()
        .name(format!("{label}-output"))
        .spawn(move || {
            let mut out = Vec::new();
            let mut err = Vec::new();
            if let Some(s) = stdout.as_mut() {
                let _ = s.read_to_end(&mut out);
            }
            if let Some(s) = stderr.as_mut() {
                let _ = s.take(64 * 1024).read_to_end(&mut err);
            }
            let _ = tx.send((out, err));
        });
    if let Err(e) = reader {
        let _ = child.kill();
        let _ = child.wait();
        return Err(PlatformError::Os(format!(
            "cannot read {label} output: {e}"
        )));
    }
    let (out, err) = match rx.recv_timeout(TIMEOUT) {
        Ok(v) => v,
        Err(_) => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(PlatformError::Os(format!("{label} timed out")));
        }
    };
    let status = child
        .wait()
        .map_err(|e| PlatformError::Os(format!("{label} failed: {e}")))?;
    if status.success() {
        Ok(String::from_utf8_lossy(&out).into_owned())
    } else {
        let msg = String::from_utf8_lossy(&err);
        let msg = msg.lines().find(|l| !l.trim().is_empty()).unwrap_or("");
        Err(PlatformError::Os(if msg.is_empty() {
            format!("{label} failed ({status})")
        } else {
            format!("{label}: {}", msg.trim())
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn which_ignores_relative_dirs_and_missing_tools() {
        let dir = tempfile::tempdir().unwrap();
        let tool = dir.path().join("kotilo-fake-tool");
        std::fs::write(&tool, b"#!/bin/sh\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&tool, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let path =
            std::env::join_paths([PathBuf::from("relative"), dir.path().to_path_buf()]).unwrap();
        assert_eq!(find_in(&path, "kotilo-fake-tool"), Some(tool));
        assert_eq!(find_in(&path, "kotilo-missing-tool"), None);
        let rel = std::env::join_paths([PathBuf::from(".")]).unwrap();
        assert_eq!(find_in(&rel, "kotilo-fake-tool"), None);
    }

    #[test]
    fn missing_program_is_an_error() {
        assert!(matches!(
            run(Path::new("/nonexistent/kotilo-no-such-tool"), ["x"]),
            Err(PlatformError::Os(_))
        ));
    }
}
