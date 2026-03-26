//! File path ↔ URI conversion helpers.
//!
//! Extracted from `verter_lsp::uri` with no tower_lsp_server dependency.
//! All functions operate on plain strings.

/// Convert a filesystem path to a `file://` URI string.
///
/// Normalizes backslashes to forward slashes.
/// Produces `file:///C:/...` (Windows) or `file:///home/...` (Unix).
pub fn path_to_file_uri_string(path: &str) -> String {
    let normalized = path.replace('\\', "/");
    if normalized.starts_with('/') {
        format!("file://{normalized}")
    } else {
        format!("file:///{normalized}")
    }
}

/// Convert a `file://` URI string to a filesystem path.
///
/// Percent-decodes the URI, strips `file:///` prefix, normalizes slashes.
/// Handles both Windows drive letters and Unix paths.
pub fn file_uri_to_path(uri: &str) -> String {
    let decoded = percent_decode(uri);
    if let Some(rest) = decoded.strip_prefix("file:///") {
        if rest.len() >= 2 && rest.as_bytes()[1] == b':' {
            return rest.replace('\\', "/");
        }
        return format!("/{}", rest.replace('\\', "/"));
    }
    if let Some(rest) = decoded.strip_prefix("file://") {
        return rest.replace('\\', "/");
    }

    decoded.replace('\\', "/")
}

/// Normalize a file URI for cache key matching.
///
/// Percent-decodes the URI. On Windows, lowercases the result
/// (case-insensitive filesystem). On Unix, leaves as-is.
pub fn normalize_file_uri_for_cache(uri: &str) -> String {
    let decoded = percent_decode(uri);
    #[cfg(windows)]
    {
        decoded.to_lowercase()
    }
    #[cfg(not(windows))]
    {
        decoded
    }
}

/// Standard percent-decoding (`%XX` hex → byte).
pub fn percent_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(hi), Some(lo)) = (hex_digit(bytes[i + 1]), hex_digit(bytes[i + 2])) {
                decoded.push((hi << 4) | lo);
                i += 3;
                continue;
            }
        }
        decoded.push(bytes[i]);
        i += 1;
    }

    String::from_utf8(decoded).unwrap_or_else(|_| input.to_string())
}

fn hex_digit(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_uri_to_path_handles_windows_drive_uris() {
        assert_eq!(
            file_uri_to_path("file:///c%3A/Users/test/file.ts"),
            "c:/Users/test/file.ts"
        );
    }

    #[test]
    fn file_uri_to_path_handles_unix_uris() {
        assert_eq!(
            file_uri_to_path("file:///home/user/project/file.ts"),
            "/home/user/project/file.ts"
        );
    }

    #[test]
    fn path_to_file_uri_string_normalizes_separators() {
        assert_eq!(
            path_to_file_uri_string(r"C:\Users\dev\App.vue"),
            "file:///C:/Users/dev/App.vue"
        );
    }

    #[test]
    fn path_to_file_uri_string_unix() {
        assert_eq!(
            path_to_file_uri_string("/home/user/App.vue"),
            "file:///home/user/App.vue"
        );
    }

    #[test]
    fn percent_decode_handles_multibyte_sequences() {
        assert_eq!(percent_decode("caf%C3%A9"), "caf\u{00E9}");
    }

    #[test]
    fn percent_decode_handles_colon() {
        assert_eq!(percent_decode("c%3A/Users"), "c:/Users");
    }

    #[test]
    fn normalize_file_uri_for_cache_decodes_percent() {
        let result = normalize_file_uri_for_cache("file:///c%3A/Users/test");
        // On Windows this would be lowercased, on Unix it stays as-is
        assert!(result.contains("c:/Users/test") || result.contains("c:/users/test"));
    }
}
