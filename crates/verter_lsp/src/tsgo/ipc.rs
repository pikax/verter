//! TSGO transport — re-exported from `verter_type_runtime`.
//!
//! All transport code now lives in `verter_type_runtime::tsgo::ipc`.
//! This module re-exports for backward compatibility within `verter_lsp`.

pub use verter_type_runtime::tsgo::{
    find_tsgo_binary, offset_to_position_with_encoding, position_to_offset_with_encoding,
    TsgoBinaryLookupError, TsgoTypeProvider,
};
