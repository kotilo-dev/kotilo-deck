//! Custom URL schemes (`kotilo://`): registry lookup + `ShellExecuteW`.
//!
//! The lookup comes first so a missing handler is an error rather than Windows'
//! "How do you want to open this?" / Store prompt.

use windows::core::{w, HSTRING};
use windows::Win32::Foundation::{ERROR_SUCCESS, HWND};
use windows::Win32::System::Com::{
    CoInitializeEx, CoUninitialize, COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE,
};
use windows::Win32::System::Registry::{
    RegCloseKey, RegOpenKeyExW, RegQueryValueExW, HKEY, HKEY_CLASSES_ROOT, KEY_READ,
};
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

use crate::platform::shared::scheme_of;
use crate::platform::{PlatformError, Result};

/// True when `HKCR\<scheme>` exists and carries the `URL Protocol` marker value
/// (covers per-user `HKCU\Software\Classes` registrations too, HKCR merges them).
fn scheme_registered(scheme: &str) -> bool {
    let name = HSTRING::from(scheme);
    let mut key = HKEY::default();
    // SAFETY: `name` outlives the call; `key` is a valid out pointer and is closed below.
    unsafe {
        if RegOpenKeyExW(HKEY_CLASSES_ROOT, &name, Some(0), KEY_READ, &mut key) != ERROR_SUCCESS {
            return false;
        }
        let found =
            RegQueryValueExW(key, w!("URL Protocol"), None, None, None, None) == ERROR_SUCCESS;
        let _ = RegCloseKey(key);
        found
    }
}

pub fn open_url_scheme(url: &str) -> Result<()> {
    let scheme = scheme_of(url).ok_or_else(|| PlatformError::Os("not a URL".into()))?;
    if !scheme_registered(scheme) {
        return Err(PlatformError::Os(format!(
            "no app is registered for {scheme}:// links"
        )));
    }
    shell_open(HSTRING::from(url), "link")
}

/// Phase 4: opens a local folder in Explorer.
pub fn open_path(path: &std::path::Path) -> Result<()> {
    if !path.is_dir() {
        return Err(PlatformError::Os("folder does not exist".into()));
    }
    shell_open(HSTRING::from(path.as_os_str()), "folder")
}

/// `ShellExecuteW("open", target)`. `what` names the target in error messages.
fn shell_open(url: HSTRING, what: &'static str) -> Result<()> {
    // ShellExecute wants an STA with OLE1 DDE off; use a throwaway thread so no runtime
    // thread's COM apartment is touched.
    std::thread::Builder::new()
        .name("shell-execute".into())
        .spawn(move || {
            // SAFETY: COM init/uninit are paired on this thread; `url` outlives ShellExecuteW.
            unsafe {
                let com = CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE);
                let r = ShellExecuteW(None::<HWND>, w!("open"), &url, None, None, SW_SHOWNORMAL);
                if com.is_ok() {
                    CoUninitialize();
                }
                // > 32 means success (documented HINSTANCE pseudo-error codes).
                if r.0 as isize > 32 {
                    Ok(())
                } else {
                    Err(PlatformError::Os(format!(
                        "cannot open the {what} (code {})",
                        r.0 as isize
                    )))
                }
            }
        })
        .map_err(|e| PlatformError::Os(format!("cannot start shell thread: {e}")))?
        .join()
        .map_err(|_| PlatformError::Os("shell thread panicked".into()))?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unregistered_scheme_is_an_error_not_a_prompt() {
        assert!(!scheme_registered("kotilo-deck-test-unregistered-scheme"));
        assert!(open_url_scheme("kotilo-deck-test-unregistered-scheme://x").is_err());
    }

    #[test]
    fn open_path_rejects_missing_folder() {
        // The success path opens an Explorer window, so it is not exercised here.
        let dir = tempfile::tempdir().unwrap();
        assert!(open_path(&dir.path().join("missing")).is_err());
    }
}
