//! File path ↔ URI conversion helpers.
//!
//! The pure string transforms live in the leaf `verter_span` crate
//! ([`verter_span::uri`]) so every consumer shares one implementation; this
//! module re-exports them, keeping the `verter_type_runtime::uri` public path
//! stable for the LSP and type-runtime backends.
pub use verter_span::uri::{
    file_uri_to_path, normalize_file_uri_for_cache, path_to_file_uri_string, percent_decode,
};
