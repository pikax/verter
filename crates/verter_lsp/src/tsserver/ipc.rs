//! tsserver transport — re-exported from `verter_type_runtime`.
//!
//! All transport code now lives in `verter_type_runtime::tsserver::ipc`.
//! This module re-exports for backward compatibility within `verter_lsp`.

pub use verter_type_runtime::tsserver::{
    build_completion_entry_details_request, build_entry_names_entry, byte_offset_to_tsserver_pos,
    completion_entry_details_to_resolve_result, concat_display_parts,
    enrich_completion_with_entry_details, format_quickinfo_hover, merge_diagnostic_sets,
    parse_tsserver_code_action, parse_tsserver_completion, parse_tsserver_diagnostic,
    parse_tsserver_location, parse_tsserver_rename_span, stamp_tsserver_completion_offset,
    tsserver_pos_to_byte_offset, TsserverTypeProvider, CHILD_PROCESS_ENV_DENYLIST,
};
