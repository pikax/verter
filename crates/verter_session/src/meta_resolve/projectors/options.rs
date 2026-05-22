//! `defineOptions({ ... })` projector.
//!
//! `defineOptions` carries component-level configuration (most
//! importantly `inheritAttrs: false`). The relevant flags live on
//! [`AnalyzedMacro::has_inherit_attrs_false`] — the parser populates
//! these during shallow analysis, before any cross-file resolution.
//!
//! The dispatch path through `ResolveMacroPayload { kind: DefineOptions }`
//! is documented as 0 args → Miss; else type_args[0] unchanged
//! ([`build_resolve_macro_payload`](crate::project_semantic_dispatch::build::build_resolve_macro_payload)
//! arms). The projector enumerates the resolved object surface for
//! callers that need a member view. The flag-derivation step
//! (`inheritAttrs`) stays parser-side because it operates on the
//! source-text annotation, not on the dispatched type.

use verter_semantic::analysis::component_meta::{MacroExpansionDiagnostics, MacroExpansionKind};
use verter_semantic::analysis::type_expand::ExpandedField;
use verter_semantic::analysis::{AnalyzedMacro, AnalyzedMacroKind};

use crate::project_semantic_dispatch::ProjectSemanticDispatch;
use crate::resolver_core::ResolverContext;
use crate::semantic_query::DeclIdentity;
use crate::types::FileAnalysisSnapshot;

use super::{
    read_surface_members, resolve_macro_payload, resolve_payload_surface,
    surface_member_to_expanded_field,
};

/// Project a `defineOptions<T>()` macro to a `Vec<ExpandedField>`.
///
/// Returns `Vec::new()` when the macro has no `parsed_type_argument`
/// — the parser-side analysis already covers the runtime
/// `defineOptions({ ... })` form via flags on `AnalyzedMacro`.
pub(crate) fn project_options(
    query_engine: &mut crate::resolver_core::ComponentMetaQueryEngine<'_>,
    owner: &DeclIdentity,
    file: &str,
    macro_index: usize,
    mac: &AnalyzedMacro,
    _snapshot: &FileAnalysisSnapshot,
    diag_sink: &mut Vec<MacroExpansionDiagnostics>,
    cursor: crate::meta_resolve::projection_demand::ProjectionCursor<'_>,
) -> Vec<ExpandedField> {
    // Block 6.i Commit AX — each surface member descends via
    // `cursor.descend_published_member(name)`; the per-member type
    // body publishes as a carrier (`Navigate`) unless the consumer
    // walked a deep path.
    if !mac.is_type_based {
        return Vec::new();
    }

    let ctx: &dyn ResolverContext = query_engine.ctx;
    let members = {
        let dispatch = ProjectSemanticDispatch::new(ctx);
        let payload_node = match resolve_macro_payload(
            &dispatch,
            owner,
            file,
            macro_index,
            mac,
            AnalyzedMacroKind::DefineOptions,
            MacroExpansionKind::DefineProps,
            diag_sink,
        ) {
            Some(node) => node,
            None => return Vec::new(),
        };

        let surface_node = match resolve_payload_surface(
            &dispatch,
            payload_node,
            macro_index,
            MacroExpansionKind::DefineProps,
            diag_sink,
        ) {
            Some(node) => node,
            None => return Vec::new(),
        };

        read_surface_members(ctx, surface_node)
    };
    members
        .into_iter()
        .filter_map(|member| {
            let member_cursor = cursor.descend_published_member(member.name.as_ref())?;
            Some(surface_member_to_expanded_field(
                query_engine,
                file,
                &member,
                None,
                None,
                None,
                member_cursor,
            ))
        })
        .collect()
}
