//! tsserver transport: newline-delimited JSON over stdio.

pub mod ipc;

#[cfg(test)]
mod completion_resolve_tests;

// Re-export the main types and functions
pub use ipc::{
    assemble_signature_label, build_completion_entry_details_request, build_entry_names_entry,
    byte_offset_to_tsserver_pos, combined_code_fix_args,
    completion_entry_details_to_resolve_result, concat_display_parts, dedup_error_codes,
    enrich_completion_with_entry_details, format_quickinfo_hover, merge_diagnostic_sets,
    parse_tsserver_code_action, parse_tsserver_combined_code_fix, parse_tsserver_completion,
    parse_tsserver_diagnostic, parse_tsserver_location, parse_tsserver_rename_span,
    stamp_tsserver_completion_offset, tsserver_pos_to_byte_offset, AssembledSignatureLabel,
    TsserverTypeProvider, CHILD_PROCESS_ENV_DENYLIST,
};
