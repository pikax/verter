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
/// Percent-decodes the URI, strips the `file://` prefix, normalizes slashes, and
/// resolves the URI authority per RFC 8089:
/// - empty or `localhost` authority (case-insensitive) ⇒ LOCAL file:
///   `file:///C:/x`, `file://localhost/C:/x` and `file://localhost/home/x` all map
///   to the same local path (`C:/x`, `/home/x`), so a `localhost` URI never
///   produces a divergent canonical ID for a local file.
/// - any other authority ⇒ UNC host: `file://server/share/...` maps to
///   `//server/share/...`, preserving the leading `//` the canonical owner emits
///   for the same file reached via `\\server\share` / `//?/UNC/server/share`.
pub fn file_uri_to_path(uri: &str) -> String {
    let decoded = percent_decode(uri);
    let Some(rest) = decoded.strip_prefix("file://") else {
        return decoded.replace('\\', "/");
    };
    // Split the authority (up to the first `/`) from the path. For `file:///…`
    // the authority is empty and `path` starts with the leading `/`.
    let (authority, path) = match rest.find('/') {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, ""),
    };
    let path = path.replace('\\', "/");

    // A drive-letter "authority" (`file://C:/x`) is NOT a UNC host — it is the
    // local drive itself (some clients emit the two-slash drive form). Resolve
    // it to the local drive path `C:/x`, identical to `file:///C:/x`.
    let is_drive_authority = authority.len() == 2
        && authority.as_bytes()[1] == b':'
        && authority.as_bytes()[0].is_ascii_alphabetic();
    if is_drive_authority {
        return format!("{authority}{path}");
    }

    if authority.is_empty() || authority.eq_ignore_ascii_case("localhost") {
        // Local file. `path` starts with `/`; a `/X:/…` is a Windows drive path
        // (drop the leading slash), otherwise it is a Unix absolute path.
        let trimmed = path.strip_prefix('/').unwrap_or(&path);
        if trimmed.len() >= 2 && trimmed.as_bytes()[1] == b':' {
            return trimmed.to_string();
        }
        return path;
    }

    // UNC host authority.
    format!("//{authority}{path}")
}

/// Normalize a file URI for TSGO diagnostics-cache equivalence matching.
///
/// This is the TSGO URI-cache key ONLY — NOT the shared canonical file ID, and
/// intentionally NOT routed through `verter_span::path::canonicalize_path`. TSGO
/// may lowercase whole URI path segments on Windows (it sends
/// `file:///c%3A/users/...` where our `path_to_file_uri_string` emits
/// `file:///C:/Users/...`), so this folds the WHOLE decoded URI under
/// `cfg(windows)` to make both forms hit the same cache bucket on the
/// case-insensitive Windows filesystem. The owner only lowercases the drive
/// letter (it preserves case-sensitive Linux paths), so it cannot serve this
/// equivalence role. Shared file identity still uses `canonicalize_path`.
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
    fn file_uri_to_path_preserves_unc_authority() {
        // A two-slash `file://host/share` URI carries a UNC authority; it must
        // map to `//host/share`, matching the owner's canonicalization of the
        // same file reached via `\\host\share` / `//?/UNC/host/share`. Pre-fix
        // the authority was dropped (`host/share`), splitting UNC file identity.
        assert_eq!(
            file_uri_to_path("file://server/share/App.vue"),
            "//server/share/App.vue"
        );
        assert_ne!(
            file_uri_to_path("file://server/share/App.vue"),
            "server/share/App.vue"
        );
    }

    #[test]
    fn localhost_authority_is_local_not_unc() {
        // RFC 8089: `file://localhost/…` is the LOCAL file, identical to the
        // empty-authority `file:///…`. It must NOT be treated as a UNC host
        // (`//localhost/…`), which would split a local file's identity.
        assert_eq!(
            file_uri_to_path("file://localhost/C:/repo/App.vue"),
            "C:/repo/App.vue"
        );
        assert_eq!(
            file_uri_to_path("file://localhost/C:/repo/App.vue"),
            file_uri_to_path("file:///C:/repo/App.vue")
        );
        assert_eq!(
            file_uri_to_path("file://localhost/home/u/App.vue"),
            "/home/u/App.vue"
        );
        // Case-insensitive authority.
        assert_eq!(
            file_uri_to_path("file://LocalHost/C:/repo/App.vue"),
            "C:/repo/App.vue"
        );
        // NEGATIVE: never the UNC `//localhost/…` form.
        assert_ne!(
            file_uri_to_path("file://localhost/C:/repo/App.vue"),
            "//localhost/C:/repo/App.vue"
        );
        // A genuine (non-localhost) authority IS still a UNC host.
        assert_eq!(
            file_uri_to_path("file://server/share/App.vue"),
            "//server/share/App.vue"
        );
    }

    #[test]
    fn two_slash_drive_authority_is_local_drive_not_unc() {
        // A drive-letter "authority" (`file://C:/x`, the two-slash drive form
        // some clients emit) is the LOCAL drive, identical to `file:///C:/x` —
        // NOT a UNC host (`//C:/x`, which the owner can't drive-lower → split).
        assert_eq!(
            file_uri_to_path("file://C:/repo/App.vue"),
            "C:/repo/App.vue"
        );
        assert_eq!(
            file_uri_to_path("file://C:/repo/App.vue"),
            file_uri_to_path("file:///C:/repo/App.vue")
        );
        // NEGATIVE: never the UNC `//C:/…` form.
        assert_ne!(
            file_uri_to_path("file://C:/repo/App.vue"),
            "//C:/repo/App.vue"
        );
        // A real two-char host that is NOT a drive (no colon) stays UNC.
        assert_eq!(
            file_uri_to_path("file://nb/share/App.vue"),
            "//nb/share/App.vue"
        );
    }

    #[test]
    fn unc_path_round_trips_to_single_canonical_id() {
        // Both UNC URI forms must collapse to the SAME `//server/share/...` ID:
        // (1) the two-slash form an external LSP client sends, and (2) the
        // FOUR-slash form THIS module's own `path_to_file_uri_string` emits for a
        // `//server/share` path. There is exactly ONE canonical ID, never a
        // double-/triple-slash split.
        let external = file_uri_to_path("file://server/share/App.vue");
        let round_tripped = file_uri_to_path(&path_to_file_uri_string("//server/share/App.vue"));
        assert_eq!(
            path_to_file_uri_string("//server/share/App.vue"),
            "file:////server/share/App.vue"
        );
        assert_eq!(external, "//server/share/App.vue");
        assert_eq!(round_tripped, "//server/share/App.vue");
        assert_eq!(external, round_tripped);
        // Not a triple-slash and not an authority-dropped form.
        assert_ne!(round_tripped, "///server/share/App.vue");
        assert_ne!(round_tripped, "server/share/App.vue");
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
