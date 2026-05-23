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

use super::{
    read_surface_members, resolve_macro_payload, resolve_payload_surface_with_scope,
    surface_member_to_expanded_field, PayloadSurfaceScope,
};

/// Project a `defineEmits<T>()` macro to a `Vec<ExpandedField>` for
/// publication into `evaluated_types.emits`.
///
/// See [`super::props::project_props`] for the per-field semantics
/// — the only differences are:
/// - the macro kind passed to `ResolveMacroPayload` is `DefineEmits`
/// - the diagnostic envelope's `MacroExpansionKind` is `DefineEmits`
/// - the raw_type comes from `mac.emit_fields[i].payload_type`
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
    let members_with_raw = {
        let dispatch = ProjectSemanticDispatch::new(ctx);
        let payload_node = match resolve_macro_payload(
            &dispatch,
            owner,
            file,
            macro_index,
            mac,
            AnalyzedMacroKind::DefineEmits,
            MacroExpansionKind::DefineEmits,
            diag_sink,
        ) {
            Some(node) => node,
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
        let surface_node = match resolve_payload_surface_with_scope(
            &dispatch,
            payload_node,
            macro_index,
            MacroExpansionKind::DefineEmits,
            PayloadSurfaceScope::EmitClassMacroObject,
            diag_sink,
        ) {
            Some(node) => node,
            None => return Vec::new(),
        };

        let members = read_surface_members(ctx, surface_node);
        members
            .into_iter()
            .map(|member| {
                let analyzed = mac
                    .emit_fields
                    .iter()
                    .find(|e| e.name == member.name.as_ref());
                let raw_type = analyzed.and_then(|e| e.payload_type.clone());
                let shallow_type_expr = analyzed.and_then(|e| e.payload_expr.clone());
                let shallow_type_expr_scope = analyzed.and_then(|e| e.payload_expr_scope.clone());
                (member, raw_type, shallow_type_expr, shallow_type_expr_scope)
            })
            .collect::<Vec<_>>()
    };
    members_with_raw
        .into_iter()
        .filter_map(
            |(member, raw_type, shallow_type_expr, shallow_type_expr_scope)| {
                let member_cursor = cursor.descend_published_member(member.name.as_ref())?;
                Some(surface_member_to_expanded_field(
                    query_engine,
                    file,
                    &member,
                    raw_type,
                    shallow_type_expr,
                    shallow_type_expr_scope,
                    member_cursor,
                ))
            },
        )
        .collect()
}
