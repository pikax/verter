//! tsserver transport: newline-delimited JSON over stdio.

pub mod ipc;

#[cfg(test)]
mod completion_resolve_tests;

// Re-export the main types and functions
pub use ipc::{
    build_completion_entry_details_request, build_entry_names_entry, byte_offset_to_tsserver_pos,
    completion_entry_details_to_resolve_result, concat_display_parts,
    enrich_completion_with_entry_details, format_quickinfo_hover, parse_tsserver_code_action,
    parse_tsserver_completion, parse_tsserver_diagnostic, parse_tsserver_location,
    parse_tsserver_rename_span, stamp_tsserver_completion_offset, tsserver_pos_to_byte_offset,
    TsserverTypeProvider, CHILD_PROCESS_ENV_DENYLIST,
};
