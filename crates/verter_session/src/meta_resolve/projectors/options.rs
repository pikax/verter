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

use super::output_sink::{surface_member_to_expanded_field, MemberValuePosition};
use super::publication_authority::{
    admit_published_member, read_surface_member_candidates, resolve_macro_payload,
    resolve_payload_surface,
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
    // Each surface member descends via
    // `cursor.descend_published_member(name)`; the per-member type
    // body publishes as a carrier (`Navigate`) unless the consumer
    // walked a deep path.
    if !mac.is_type_based {
        return Vec::new();
    }

    let ctx: &dyn ResolverContext = query_engine.ctx;
    // Admission applies the public-visibility filter, the derived-kind/cursor
    // match, the `descend_published_member` gate, AND records the
    // published-field edge uniformly — previously the options projector
    // descended the member but did NOT record the edge (a drift versus
    // props/emits/slots/expose); routing through `admit_published_member`
    // closes that drift.
    let admitted = {
        let dispatch = ProjectSemanticDispatch::new(ctx);
        let payload = match resolve_macro_payload(
            &dispatch,
            owner,
            file,
            macro_index,
            mac,
            AnalyzedMacroKind::DefineOptions,
            MacroExpansionKind::DefineProps,
            diag_sink,
        ) {
            Some(payload) => payload,
            None => return Vec::new(),
        };

        let surface = match resolve_payload_surface(
            &dispatch,
            &payload,
            MacroExpansionKind::DefineProps,
            diag_sink,
        ) {
            Some(surface) => surface,
            None => return Vec::new(),
        };

        read_surface_member_candidates(ctx, &surface)
            .into_iter()
            .filter_map(|candidate| admit_published_member(candidate, &cursor, &dispatch))
            .collect::<Vec<_>>()
    };
    admitted
        .into_iter()
        .map(|admitted| {
            surface_member_to_expanded_field(
                query_engine,
                file,
                &admitted,
                None,
                None,
                mac.parsed_type_argument.as_ref(),
                MemberValuePosition::ShallowMember,
            )
        })
        .collect()
}
