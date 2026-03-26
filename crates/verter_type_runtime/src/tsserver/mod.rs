//! tsserver transport: newline-delimited JSON over stdio.

pub mod ipc;

// Re-export the main types and functions
pub use ipc::{
    byte_offset_to_tsserver_pos, concat_display_parts, format_quickinfo_hover,
    parse_tsserver_code_action, parse_tsserver_completion, parse_tsserver_diagnostic,
    parse_tsserver_location, parse_tsserver_rename_span, tsserver_pos_to_byte_offset,
    TsserverTypeProvider, CHILD_PROCESS_ENV_DENYLIST,
};
