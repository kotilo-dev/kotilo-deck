//! Pure helpers shared by the Windows and Linux backends (no OS calls).

/// Max raw image bytes in an artwork data URL (base64 adds a third → ≈ 200 KB of JSON).
pub const ARTWORK_MAX_BYTES: usize = 150 * 1024;

/// `scheme` from `scheme:rest`, if it is a syntactically valid URL scheme.
pub fn scheme_of(url: &str) -> Option<&str> {
    let (scheme, _) = url.split_once(':')?;
    let mut chars = scheme.chars();
    let first_ok = chars.next().is_some_and(|c| c.is_ascii_alphabetic());
    (first_ok && chars.all(|c| c.is_ascii_alphanumeric() || "+-.".contains(c))).then_some(scheme)
}

pub fn sniff_mime(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(&[0x89, b'P', b'N', b'G']) {
        Some("image/png")
    } else if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some("image/jpeg")
    } else {
        None
    }
}

pub fn base64(bytes: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            chunk.get(1).copied().unwrap_or(0),
            chunk.get(2).copied().unwrap_or(0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        out.push(T[(n >> 18) as usize & 63] as char);
        out.push(T[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 {
            T[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            T[n as usize & 63] as char
        } else {
            '='
        });
    }
    out
}

/// `data:` URL for png/jpeg bytes within [`ARTWORK_MAX_BYTES`]; `None` otherwise.
pub fn artwork_data_url(bytes: &[u8]) -> Option<String> {
    let mime = sniff_mime(bytes)?;
    (bytes.len() <= ARTWORK_MAX_BYTES).then(|| format!("data:{mime};base64,{}", base64(bytes)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scheme_parsing() {
        assert_eq!(scheme_of("kotilo://open"), Some("kotilo"));
        assert_eq!(scheme_of("ms-settings:sound"), Some("ms-settings"));
        assert_eq!(scheme_of("no scheme"), None);
        assert_eq!(scheme_of("1abc://x"), None);
        assert_eq!(scheme_of("://x"), None);
        assert_eq!(scheme_of("-x://y"), None, "never looks like a CLI flag");
    }

    #[test]
    fn base64_matches_rfc4648_vectors() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn artwork_is_capped_and_typed() {
        let mut png = vec![0x89, b'P', b'N', b'G'];
        png.resize(1000, 0);
        let url = artwork_data_url(&png).unwrap();
        assert!(url.starts_with("data:image/png;base64,iVBOR"));
        let mut jpeg = vec![0xFF, 0xD8, 0xFF];
        jpeg.resize(ARTWORK_MAX_BYTES, 1);
        let url = artwork_data_url(&jpeg).unwrap();
        assert!(url.starts_with("data:image/jpeg;base64,"));
        assert!(url.len() <= 205 * 1024, "data URL stays ≈ 200 KB");
        jpeg.push(0);
        assert_eq!(artwork_data_url(&jpeg), None, "over the cap");
        assert_eq!(artwork_data_url(b"GIF89a"), None, "unknown type");
    }
}
