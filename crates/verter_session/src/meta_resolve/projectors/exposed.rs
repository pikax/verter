//! `defineExpose({ ... })` projector.
//!
//! `defineExpose` always takes an object expression argument (or no
//! argument). The exposed surface is computed by the parser's
//! [`extract_exposed_from_macro`] as the union of `mac.expose_fields`
//! (with follow-on binding lookups in `input.bindings`) and the
//! resolved type-argument surface members. The dispatch path
//! through `ResolveMacroPayload { kind: DefineExpose }` is documented
//! as 0 args → Miss, else type_args[0] unchanged — so the projector
//! mostly mirrors what the parser already provides, but routes
//! through dispatch when a generic type argument is present.
//!
//! Dispatch return is reduced to surface members via empty-path
//! Shallow `ProjectPath`. For the common `defineExpose({ a, b })`
//! object-literal case, the parser's analyzed `expose_fields` is the
//! authoritative source. This projector therefore returns
//! `Vec::new()` when the macro lacks a `parsed_type_argument`.

use verter_semantic::analysis::component_meta::{MacroExpansionDiagnostics, MacroExpansionKind};
use verter_semantic::analysis::type_expand::ExpandedField;
use verter_semantic::analysis::{AnalyzedMacro, AnalyzedMacroKind};

use crate::project_semantic_dispatch::ProjectSemanticDispatch;
use crate::resolver_core::ResolverContext;
use crate::semantic_query::DeclIdentity;
use crate::types::FileAnalysisSnapshot;

use super::output_sink::surface_member_to_expanded_field;
use super::publication_authority::{
    admit_published_member, read_surface_member_candidates, resolve_macro_payload,
    resolve_payload_surface,
};

pub(crate) fn project_exposed(
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
    // See `project_props` for the PublishedField origin-edge rationale —
    // recorded uniformly inside `admit_published_member`.
    let admitted = {
        let dispatch = ProjectSemanticDispatch::new(ctx);
        let payload = match resolve_macro_payload(
            &dispatch,
            owner,
            file,
            macro_index,
            mac,
            AnalyzedMacroKind::DefineExpose,
            // Reuse DefineProps as the closest expansion-kind for now;
            // future enrichment of `MacroExpansionKind` can split this.
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
            surface_member_to_expanded_field(query_engine, file, &admitted, None, None, None)
        })
        .collect()
}
