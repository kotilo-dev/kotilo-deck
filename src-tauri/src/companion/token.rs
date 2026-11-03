//! `~/.kotilo/companion.token` (protocol.md §2): 64 lowercase hex chars, created by whichever
//! side starts first. Created via a temp file in the same folder, published without ever
//! replacing an existing valid token, user-only readable (POSIX 0600). Never logged.

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

pub const TOKEN_LEN: usize = 64;

/// Exactly 64 lowercase hex chars (callers trim first).
pub fn is_valid(s: &str) -> bool {
    s.len() == TOKEN_LEN && s.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// Constant-time comparison (no early exit on the first differing byte).
pub fn matches(expected: &str, given: &str) -> bool {
    let (a, b) = (expected.as_bytes(), given.as_bytes());
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// The valid token in `path`, `None` if the file is missing or holds anything else.
pub fn read(path: &Path) -> io::Result<Option<String>> {
    match fs::read_to_string(path) {
        Ok(s) => {
            let t = s.trim();
            if is_valid(t) {
                restrict(path);
                Ok(Some(t.to_string()))
            } else {
                Ok(None)
            }
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        // Non-UTF-8 content counts as invalid, like any other garbage.
        Err(e) if e.kind() == io::ErrorKind::InvalidData => Ok(None),
        Err(e) => Err(e),
    }
}

fn generate() -> io::Result<String> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).map_err(|e| io::Error::other(e.to_string()))?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

/// Best effort: tighten an existing token file to 0600 (POSIX only).
#[cfg(unix)]
fn restrict(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    if let Ok(m) = fs::metadata(path) {
        if m.permissions().mode() & 0o077 != 0 {
            let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o600));
        }
    }
}

#[cfg(not(unix))]
fn restrict(_path: &Path) {}

fn write_temp(dir: &Path, token: &str) -> io::Result<PathBuf> {
    let mut suffix = [0u8; 6];
    getrandom::fill(&mut suffix).map_err(|e| io::Error::other(e.to_string()))?;
    let name: String = suffix.iter().map(|b| format!("{b:02x}")).collect();
    let tmp = dir.join(format!(".companion.token.{name}.tmp"));
    let mut opts = fs::OpenOptions::new();
    opts.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    let mut f = opts.open(&tmp)?;
    let res = f.write_all(token.as_bytes()).and_then(|()| f.sync_all());
    drop(f);
    if let Err(e) = res {
        let _ = fs::remove_file(&tmp);
        return Err(e);
    }
    Ok(tmp)
}

/// Returns the shared token, creating it if the file is missing or invalid. A valid token
/// written by the desktop (even concurrently) always wins over ours.
pub fn load_or_create(path: &Path) -> io::Result<String> {
    if let Some(t) = read(path)? {
        return Ok(t);
    }
    let dir = path
        .parent()
        .ok_or_else(|| io::Error::other("token path has no parent folder"))?;
    fs::create_dir_all(dir)?;
    let token = generate()?;
    let tmp = write_temp(dir, &token)?;
    let result = publish(&tmp, path, &token);
    let _ = fs::remove_file(&tmp);
    result
}

fn publish(tmp: &Path, path: &Path, token: &str) -> io::Result<String> {
    // Hard link = atomic "create if absent": it fails instead of replacing a file the other
    // side created after our read.
    match fs::hard_link(tmp, path) {
        Ok(()) => return Ok(token.to_string()),
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
        // File systems without hard links: fall through to the rename path.
        Err(_) => {}
    }
    if let Some(t) = read(path)? {
        return Ok(t);
    }
    // Missing (no hard links) or invalid content: replace it.
    fs::rename(tmp, path)?;
    Ok(token.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    const VALID: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    #[test]
    fn validity_and_compare() {
        assert!(is_valid(VALID));
        assert!(!is_valid(&VALID.to_uppercase()));
        assert!(!is_valid(&VALID[1..]));
        assert!(!is_valid(&format!("{VALID}0")));
        assert!(!is_valid(&VALID.replace('a', "g")));
        assert!(matches(VALID, VALID));
        assert!(!matches(VALID, &VALID.replace('f', "e")));
        assert!(!matches(VALID, ""));
    }

    #[test]
    fn creates_once_and_never_overwrites_a_valid_token() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested").join("companion.token");
        let a = load_or_create(&path).unwrap();
        assert!(is_valid(&a));
        assert_eq!(load_or_create(&path).unwrap(), a);
        // The other side's token (with a trailing newline) is read, trimmed and kept.
        fs::write(&path, format!("{VALID}\n")).unwrap();
        assert_eq!(load_or_create(&path).unwrap(), VALID);
        assert_eq!(fs::read_to_string(&path).unwrap(), format!("{VALID}\n"));
        // No temp files are left behind.
        let names: Vec<_> = fs::read_dir(path.parent().unwrap())
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(names.len(), 1);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(&path).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }
    }

    #[test]
    fn replaces_an_invalid_token() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("companion.token");
        fs::write(&path, "not a token").unwrap();
        let t = load_or_create(&path).unwrap();
        assert!(is_valid(&t));
        assert_eq!(fs::read_to_string(&path).unwrap(), t);
        fs::write(&path, [0xff, 0xfe]).unwrap();
        assert!(is_valid(&load_or_create(&path).unwrap()));
    }
}
