use tower_lsp_server::ls_types::Uri;

// Re-export shared URI helpers from verter_type_runtime.
pub(crate) use verter_type_runtime::uri::{
    file_uri_to_path, path_to_file_uri_string, percent_decode,
};

/// Convert a path to a tower_lsp_server `Uri`.
///
/// This LSP-specific function wraps the shared `path_to_file_uri_string()`
/// and parses into the tower_lsp_server `Uri` type.
pub(crate) fn path_to_file_uri(path: &str) -> Option<Uri> {
    path_to_file_uri_string(path).parse().ok()
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
    fn percent_decode_handles_multibyte_sequences() {
        assert_eq!(percent_decode("caf%C3%A9"), "caf\u{00E9}");
    }
}
