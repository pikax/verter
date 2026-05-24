//! `defineSlots<T>()` projector — slot-shape level only.
//!
//! Resolves the slot surface (the slot names and their function-shape
//! signatures) through `ResolveMacroPayload` + empty-path Shallow
//! `ProjectPath`. Each surface member is one slot, with its TypeExpr
//! preserving the function signature (which downstream consumers
//! introspect for `bindings`).
//!
//! Slot bindings (the `defineSlots<{ name(props: { item: string }): VNode[] }>`
//! parameter introspection) live in
//! [`crate::meta_resolve::slot_binding_graph`]; this projector
//! intentionally stops at the slot-shape level so the
//! cooperative-admission caches behind both projectors and
//! `resolve_slot_bindings_graph_native` populate the same dispatch
//! family memo.

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

pub(crate) fn project_slots(
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
    // Block 6.j R18 — see `project_props` for the PublishedField
    // origin-edge emit rationale.
    let admitted: Vec<_> = {
        let dispatch = ProjectSemanticDispatch::new(ctx);
        let payload_node = match resolve_macro_payload(
            &dispatch,
            owner,
            file,
            macro_index,
            mac,
            AnalyzedMacroKind::DefineSlots,
            MacroExpansionKind::DefineSlots,
            diag_sink,
        ) {
            Some(node) => node,
            None => return Vec::new(),
        };

        let surface_node = match resolve_payload_surface(
            &dispatch,
            payload_node,
            macro_index,
            MacroExpansionKind::DefineSlots,
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
                dispatch.record_published_field_edge(
                    owner,
                    surface_node,
                    member.value,
                    &member.name,
                );
                Some((member, member_cursor))
            })
            .collect()
    };
    // Slot fields don't carry a payload-style raw_type per member;
    // their parser-side annotation lives on bindings. The slot
    // surface itself is left without raw_type so the merge layer
    // (parser-side `extract_component_meta`) can populate the slot
    // structure via its own slot_fields traversal.
    admitted
        .into_iter()
        .map(|(member, member_cursor)| {
            surface_member_to_expanded_field(
                query_engine,
                file,
                &member,
                None,
                None,
                None,
                member_cursor,
            )
        })
        .collect()
}
