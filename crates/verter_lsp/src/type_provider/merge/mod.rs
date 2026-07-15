//! Merge logic for combining verter analysis results with TypeProvider results.
//!
//! Each merge function takes verter-only results and TypeProvider results,
//! producing enhanced output. All functions handle the case where either
//! source may be absent (graceful fallback).
//!
//! The logic is grouped into cohesive submodules — position/range mapping
//! ([`position`]), and one module per LSP feature family ([`hover`],
//! [`completion`], [`diagnostics`], [`definition`], [`feature_merges`]) — and
//! re-exported here so callers keep using `crate::type_provider::merge::<item>`.

mod completion;
mod definition;
mod diagnostics;
mod feature_merges;
mod hover;
mod position;

#[cfg(test)]
mod tests;

pub(crate) use completion::provider_completion_to_lsp_item;
pub use completion::{jsx_prop_to_vue_attr, merge_completions};

pub(crate) use definition::resolve_external_target_range;
pub use definition::{
    file_path_to_uri, merge_definitions, merge_definitions_with_barrel_resolver,
    normalize_carrier_path_owned,
};

pub use diagnostics::merge_diagnostics;

pub use feature_merges::{
    merge_code_actions, merge_document_highlights, merge_inlay_hints, merge_references,
    merge_rename_locations, merge_semantic_tokens, merge_signature_help,
};

pub use hover::merge_hover;

pub(crate) use position::carrier_completion_member_boundary_offset;
pub use position::{
    api_surface_range_to_carrier_range, carrier_position_to_tsx_offset,
    carrier_position_to_tsx_offset_validated, tsx_range_to_carrier_range, ApiSurfaceResolution,
    BarrelResolver, ExternalApiResolver, ExternalIdeContext, ExternalIdeResolver,
    ExternalSourceReader,
};
