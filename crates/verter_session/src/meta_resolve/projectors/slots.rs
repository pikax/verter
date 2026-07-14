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

use super::output_sink::{surface_member_to_expanded_field, MemberValuePosition};
use super::publication_authority::{
    admit_published_member, read_surface_member_candidates, resolve_macro_payload,
    resolve_payload_surface,
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
            AnalyzedMacroKind::DefineSlots,
            MacroExpansionKind::DefineSlots,
            diag_sink,
        ) {
            Some(payload) => payload,
            None => return Vec::new(),
        };

        let surface = match resolve_payload_surface(
            &dispatch,
            &payload,
            MacroExpansionKind::DefineSlots,
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
    // Slot fields don't carry a payload-style raw_type per member;
    // their parser-side annotation lives on bindings. The slot
    // surface itself is left without raw_type so the merge layer
    // (parser-side `extract_component_meta`) can populate the slot
    // structure via its own slot_fields traversal.
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
