//! TSGO transport — re-exported from `verter_type_runtime`.
//!
//! All transport code now lives in `verter_type_runtime::tsgo::ipc`.
//! This module re-exports for backward compatibility within `verter_lsp`.
//! tsgo binary DISCOVERY lives in `verter_tsgo_api::toolchain::discovery`
//! (the 4-tier resolver).

pub use verter_type_runtime::tsgo::{
    offset_to_position_with_encoding, position_to_offset_with_encoding, TsgoOwnedProvider,
    TsgoTypeProvider,
};
