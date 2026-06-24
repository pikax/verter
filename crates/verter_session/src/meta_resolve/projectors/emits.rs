//! `defineEmits<T>()` projector.
//!
//! Mirrors [`super::props::project_props`] but for emit fields. The
//! parser-side `AnalyzedEmitField.payload_type` provides the raw_type
//! when available.

use verter_semantic::analysis::component_meta::{MacroExpansionDiagnostics, MacroExpansionKind};
use verter_semantic::analysis::type_expand::ExpandedField;
use verter_semantic::analysis::{AnalyzedMacro, AnalyzedMacroKind};

use crate::project_semantic_dispatch::ProjectSemanticDispatch;
use crate::resolver_core::ResolverContext;
use crate::semantic_query::DeclIdentity;
use crate::types::FileAnalysisSnapshot;

use super::macro_payload_substrate::PayloadSurfaceScope;
use super::output_sink::surface_member_to_expanded_field;
use super::publication_authority::{
    admit_published_member, read_surface_member_candidates, resolve_macro_payload,
    resolve_payload_surface_with_scope, AdmittedPublishedMember,
};

pub(crate) fn project_emits(
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
    let admitted: Vec<(AdmittedPublishedMember<'_>, _, _, _)> = {
        let dispatch = ProjectSemanticDispatch::new(ctx);
        let payload = match resolve_macro_payload(
            &dispatch,
            owner,
            file,
            macro_index,
            mac,
            AnalyzedMacroKind::DefineEmits,
            MacroExpansionKind::DefineEmits,
            diag_sink,
        ) {
            Some(payload) => payload,
            None => return Vec::new(),
        };

        // Branch-merged shallow semantics for emit-class macro object
        // payloads. When the payload is an undecided `Conditional`
        // (e.g. inherited emits via
        // `defineEmits<Mode extends 'editor' ? EditorEmits : ViewerEmits>()`),
        // [`PayloadSurfaceScope::EmitClassMacroObject`] projects BOTH
        // branches under `Published(Shallow)` and merges their
        // top-level Object members so the inherited `accepted_events`
        // set publishes without forcing the inheritance reducer onto
        // an `Expanded`-only escape hatch. Non-conditional payloads
        // pass through to the default single-dispatch path verbatim.
        let surface = match resolve_payload_surface_with_scope(
            &dispatch,
            &payload,
            MacroExpansionKind::DefineEmits,
            PayloadSurfaceScope::EmitClassMacroObject,
            diag_sink,
        ) {
            Some(surface) => surface,
            None => return Vec::new(),
        };

        read_surface_member_candidates(ctx, &surface)
            .into_iter()
            .filter_map(|candidate| {
                let analyzed = mac
                    .emit_fields
                    .iter()
                    .find(|e| e.name == candidate.member().name.as_ref());
                let raw_type = analyzed.and_then(|e| e.payload_type.clone());
                let shallow_type_expr = analyzed.and_then(|e| e.payload_expr.clone());
                let shallow_type_expr_scope = analyzed.and_then(|e| e.payload_expr_scope.clone());
                let admitted = admit_published_member(candidate, &cursor, &dispatch)?;
                Some((
                    admitted,
                    raw_type,
                    shallow_type_expr,
                    shallow_type_expr_scope,
                ))
            })
            .collect()
    };
    admitted
        .into_iter()
        .map(
            |(admitted, raw_type, shallow_type_expr, shallow_type_expr_scope)| {
                surface_member_to_expanded_field(
                    query_engine,
                    file,
                    &admitted,
                    raw_type,
                    shallow_type_expr,
                    shallow_type_expr_scope,
                )
            },
        )
        .collect()
}
