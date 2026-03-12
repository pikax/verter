// Pre-parsed sentinel URIs used across feature modules.
//
// These LazyLock statics parse the URI once at first access, eliminating
// repeated `.parse::<Uri>().unwrap()` calls in production code paths.

use std::sync::LazyLock;

use tower_lsp_server::ls_types::Uri;

/// Sentinel URI for same-file navigation (definition, references, rename).
/// The server replaces this with the actual document URI before returning to the client.
pub const SAME_FILE_URI_STR: &str = "verter-internal:same-file";

/// Pre-parsed [`SAME_FILE_URI_STR`] for use in feature modules.
pub static SAME_FILE_URI: LazyLock<Uri> = LazyLock::new(|| {
    SAME_FILE_URI_STR
        .parse()
        .expect("SAME_FILE_URI_STR is a valid URI")
});

/// Sentinel URI for code actions that edit the current file.
/// Must be replaced with the actual document URI via [`action_utils::fix_placeholder_uris`].
pub const PLACEHOLDER_URI_STR: &str = "file:///placeholder";

/// Pre-parsed [`PLACEHOLDER_URI_STR`] for use in code action modules.
pub static PLACEHOLDER_URI: LazyLock<Uri> = LazyLock::new(|| {
    PLACEHOLDER_URI_STR
        .parse()
        .expect("PLACEHOLDER_URI_STR is a valid URI")
});

/// Fallback URI used when a canonical ID cannot be parsed into a valid `file://` URI.
pub static UNKNOWN_FILE_URI: LazyLock<Uri> = LazyLock::new(|| {
    "file:///unknown"
        .parse()
        .expect("file:///unknown is a valid URI")
});
