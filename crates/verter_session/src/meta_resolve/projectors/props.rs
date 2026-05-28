//! `defineProps<T>()` projector.
//!
//! Resolves the macro's parsed type argument to a payload node through
//! `ResolveMacroPayload`, then enumerates the prop surface via an
//! empty-path `ProjectPath` in `Shallow` mode. Each surface member
//! becomes one `ExpandedField` with its `raw_type` populated from the
//! parser-side `AnalyzedPropField.type_annotation` when available
//! (preserves §7.4b parity).

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

pub(crate) fn project_props(
    query_engine: &mut crate::resolver_core::ComponentMetaQueryEngine<'_>,
    owner: &DeclIdentity,
    file: &str,
    macro_index: usize,
    mac: &AnalyzedMacro,
    _snapshot: &FileAnalysisSnapshot,
    diag_sink: &mut Vec<MacroExpansionDiagnostics>,
    cursor: crate::meta_resolve::projection_demand::ProjectionCursor<'_>,
) -> Vec<ExpandedField> {
    // `cursor` carries the publication-boundary
    // demand. Each surface member is descended via
    // `cursor.descend_published_member(name)`: the macro publishes
    // every member NAME, and each member's type body is published as
    // a CARRIER (`Navigate` mode) unless the consumer explicitly
    // walked a deep path. This closes the Rule-5 depth leak where a
    // `Tool<INPUT, OUTPUT>`-typed prop would breadth-enumerate
    // `Tool`'s `outputSchema` / `execute` members into the surface.
    if !mac.is_type_based {
        return Vec::new();
    }

    let ctx: &dyn ResolverContext = query_engine.ctx;
    // Admit members under the cursor and emit
    // `MemberEdgeProvenance::PublishedField` origin edges for every
    // admitted name BEFORE dispatch drops, so the Rule-5 compliance
    // validator can attest that every published member is a declared
    // surface member.
    let admitted: Vec<_> = {
        let dispatch = ProjectSemanticDispatch::new(ctx);
        let payload_node = match resolve_macro_payload(
            &dispatch,
            owner,
            file,
            macro_index,
            mac,
            AnalyzedMacroKind::DefineProps,
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
            // Props payload surface carries macro-T own-body provenance
            // so the DeclPlaceholder unwrap stamps own-body members
            // `declared_in_macro_type_arg = true` (codex BINDING design).
            super::macro_payload_surface_provenance(AnalyzedMacroKind::DefineProps),
            diag_sink,
        ) {
            Some(node) => node,
            None => return Vec::new(),
        };

        let members = read_surface_members(ctx, surface_node);
        members
            .into_iter()
            .filter_map(|member| {
                let member_cursor = cursor.descend_published_member(member.name.as_ref())?;
                // Emit `PublishedField` origin edges for every
                // member the projector admits onto the macro
                // surface. PublishedField is the SEMANTIC
                // PROVENANCE rail — it records the producer's raw
                // truth without downstream-projection filtering.
                // `PublishedSurfacePolicy::{Compat, Refined}`
                // consumers read the Native graph and apply their
                // own structural filters (Vue intrinsics,
                // `onX`-shadows-emit, global-attrs).
                //
                // Ref-carrier surfaces (cross-file generic payloads
                // like `defineProps<AccordionProps<T>>()`) lower
                // to a `SemanticNodeData::Ref` shell where
                // `read_surface_members` returns empty, so this
                // emit fires ZERO times. The orchestrator's
                // [`crate::meta_resolve::materialize::macro_shapes::record_published_field_edges_for_macro_shape`]
                // covers Ref carriers via the cross-file-resolved
                // `shape.value.properties`.
                dispatch.record_published_field_edge(
                    owner,
                    surface_node,
                    member.value,
                    &member.name,
                );
                let analyzed = mac
                    .prop_fields
                    .iter()
                    .find(|p| p.name == member.name.as_ref());
                let raw_type = analyzed.and_then(|p| p.type_annotation.clone());
                let shallow_type_expr = analyzed.and_then(|p| p.type_expr.clone());
                let shallow_type_expr_scope = analyzed.and_then(|p| p.type_expr_scope.clone());
                Some((
                    member,
                    raw_type,
                    shallow_type_expr,
                    shallow_type_expr_scope,
                    member_cursor,
                ))
            })
            .collect()
    };
    admitted
        .into_iter()
        .map(
            |(member, raw_type, shallow_type_expr, shallow_type_expr_scope, member_cursor)| {
                surface_member_to_expanded_field(
                    query_engine,
                    file,
                    &member,
                    raw_type,
                    shallow_type_expr,
                    shallow_type_expr_scope,
                    member_cursor,
                )
            },
        )
        .collect()
}
