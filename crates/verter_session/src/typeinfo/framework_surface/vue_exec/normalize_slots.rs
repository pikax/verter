//! The `.vue` slots macro NORMALIZER — the slot half of the per-surface
//! macro normalizers (`normalize` owns props / emits / expose / options /
//! index signatures). Same contract: a thin projection of the ONE shared
//! resolver's macro surface — callable realization, binding projection, and
//! display minting through the sealed output capability; never a resolver.

use std::sync::Arc;

use verter_semantic::analysis::types::{AnalyzedSlotField, AnalyzedSlotFieldBinding};
use verter_type_expr::{LiteralValue, TypeExpr};

use super::{member_jsdoc_from_spans, raise_member_value, slice_canonical_span};
use crate::meta_resolve::callable_view::{ArmCombineNode, CallableNodeView};
use crate::project_semantic_dispatch::output_materialization::OutputProjector;
use crate::project_semantic_dispatch::{node_data_for, ProjectSemanticDispatch};
use crate::resolver_core::surface_projector::render_type_expr_display;
use crate::semantic_query::{
    PathSegment, ProjectionMode, ProjectionReductionContext, SemanticNodeData, SemanticNodeId,
};
use crate::typeinfo::framework_surface::resolved_surface_access::ResolvedSurfaceAccess;
use crate::typeinfo::surface::{CanonicalSpan, TypeInfoSurfaceMember};

/// Normalize a `.vue` slots macro surface into the published
/// [`AnalyzedSlotField`] set.
///
/// Keep FUNCTION-LIKE members only (the value realizes to a callable;
/// non-function members are filtered); the slot's `bindings` come from resolving
/// the function's first-parameter type to its object surface (a literal object,
/// a `Pick<…>`, or a named alias — see [`binding_fields_from_param_node`]); the
/// display `return_type` comes from the function's return type. The published
/// field is SHALLOW: a resolved-surface slot has no flat authored macro-payload
/// position, so `payload` stays the honest `None` (paired with a `None` scope)
/// and typed slot demand is host-raised through the graph surface.
///
/// Merged-contributor fail-close: a member VALUE that does not
/// demand-validate through the shared structural-fact primitive (an
/// unresolvable contributor — alone, or merged behind a resolvable same-name
/// sibling) publishes NO slot and marks the request's materialization-cache
/// suppress (the raise-miss fence): the realized-callable filter would
/// otherwise drop the failed arm and publish the resolvable sibling's
/// callable as a completed concrete slot.
#[must_use]
pub(crate) fn slots_from_typeinfo_surface(
    ctx: &dyn crate::resolver_core::ResolverContext,
    resolved: &impl ResolvedSurfaceAccess,
) -> Vec<AnalyzedSlotField> {
    let macro_surface = resolved.macro_surface();
    // View-sensitive slot type resolution flows through the active `ctx`.
    // Host-level reads (JSDoc / return-type source slicing, node scope) use the
    // host the `ctx` is installed against.
    let host = ctx.host_for_fact_tracer_install();
    let dispatch = ctx.dispatch();
    // Node-domain demand identity: `Navigate` carrier-resolves an aliased /
    // generic slot callable before the function-like filter.
    let context = ProjectionReductionContext::published(ProjectionMode::Navigate);
    macro_surface
        .surface
        .members
        .iter()
        // Public-only publication: a `private` / `protected` class member must
        // NOT leak as a published slot.
        .filter(|member| member.visibility.is_public())
        .filter_map(|member| {
            // Merged-contributor fail-close: a slot member value that does
            // NOT demand-validate through the shared structural-fact
            // primitive (an unresolvable contributor — alone, or merged
            // behind a resolvable same-name sibling in an intersection) must
            // never publish the resolvable arm's callable as a completed
            // concrete slot. The published set carries no per-slot typed
            // source position, so the honest degraded form is the
            // raise-miss fence: publish NO fabricated slot and mark the
            // request's materialization-cache suppress so the result is
            // never admitted warm as complete metadata.
            if dispatch
                .demand_validated_structural_node(
                    member.value,
                    ProjectionReductionContext::structural_transit_with_mode(
                        ProjectionMode::Navigate,
                    ),
                )
                .is_none()
            {
                crate::request_context::mark_request_result_partial();
                return None;
            }
            // The slot callable decisions are made ENTIRELY in the node domain
            // through the shared `CallableNodeView`; the display return `TypeExpr`
            // is minted ONCE at the terminal sink. A slot member may be a
            // non-`Function` carrier shell under the transit-shallow surface (a
            // generic slot alias lowering to an `InstantiationRef` / alias
            // carrier) — the view realizes it BEFORE the function-like filter.
            let view = CallableNodeView::new(&dispatch, member.value);
            // Function-like FILTER + return-combiner selection (node-domain): a
            // member that does not realize to a callable is not a slot (dropped).
            // The realized root's top-level kind selects the return combiner — a
            // `Union` of function arms unions returns; a single `Function` or an
            // `Intersection` of function arms intersects them. (The first params
            // are ALWAYS intersected — a template binding must hold across arms.)
            let realized_root = view.realized_callable_root(context)?;
            let combine = match node_data_for(dispatch.ctx, realized_root).as_deref() {
                Some(SemanticNodeData::Union(_)) => ArmCombineNode::Union,
                _ => ArmCombineNode::Intersection,
            };
            // The across-arms first-param + return NODES (fails closed to drop
            // the slot when any arm is non-callable).
            let parts = view.slot_param_and_return_by_arm(combine, context)?;
            let scope = macro_surface.member_expr_scope(host, member);
            // Bindings from the (across-arms-intersected) first-param NODE.
            let bindings = parts
                .first_param
                .map(|first_param| binding_fields_from_param_node(ctx, first_param))
                .unwrap_or_default();
            // Display `return_type`: prefer the EXACT source text sliced from the
            // return-type annotation span (single-arm) — this preserves a name the
            // typed return cannot surface (an unresolved imported `VNode`). Fall
            // back to rendering the return NODE materialized ONCE at the terminal
            // sink (composed multi-arm — no single span). Display-only; the
            // by-name `.and_then` render form makes no decision on the
            // materialized value.
            let return_type = parts
                .return_type_span
                .map(|span| CanonicalSpan::new(scope.as_str().into(), span))
                .and_then(|cspan| slice_canonical_span(host, &cspan))
                .map(|text| text.trim().to_string())
                .filter(|text| !text.is_empty())
                .or_else(|| {
                    parts
                        .return_type
                        .map(|return_node| materialize_slot_return_node(ctx, return_node))
                        .as_ref()
                        .and_then(render_type_expr_display)
                });
            let (description, tags) = member_jsdoc_from_spans(host, member);
            Some(AnalyzedSlotField {
                name: member.name.as_ref().to_string(),
                is_required: !member.optional,
                span: verter_span::Span::default(),
                bindings,
                return_type,
                // A resolved-surface slot has no flat authored macro-payload
                // position — the honest locator-less form, paired with a
                // `None` scope; typed demand is host-raised.
                payload: None,
                return_expr_scope: None,
                description,
                tags,
            })
        })
        .collect()
}

/// Materialize a Vue slot RETURN node into its display `TypeExpr` — a GENUINE
/// decide-free terminal one-shot sink (the single-node twin of
/// [`materialize_payload_tuple`]). The return `SemanticNodeId` is minted ONCE
/// through the sealed output cap; it makes NO decision on the materialized value
/// and takes NO `&TypeExpr` param (a node id + the active `ctx`). The mint cap is
/// constructed INTERNALLY from `ctx` (the `raise_member_value` pattern).
fn materialize_slot_return_node(
    ctx: &dyn crate::resolver_core::ResolverContext,
    return_node: SemanticNodeId,
) -> TypeExpr {
    let dispatch = ctx.dispatch();
    let cap = super::TypeinfoVueSurfaceOutputCap::new(&dispatch);
    // A node that does not materialize keeps the opaque `Unknown` raise-miss value
    // (the shared raise-miss convention); a realized slot's return node always
    // mints, so the fallback is robustness only.
    cap.materialize_output_type_expr(return_node)
        .map(|raised| raised.into_type_expr(&cap))
        .unwrap_or(TypeExpr::Unknown { raw: String::new() })
}

/// Reconstruct a slot's binding fields from its function's first-parameter NODE.
/// Each member of the parameter's OBJECT surface becomes one
/// [`AnalyzedSlotFieldBinding`] carrying that member's display
/// `type_annotation` (bindings are locator-less by convention — the flat
/// field-position vocabulary cannot address a nested (slot, binding) position
/// honestly, so typed binding demand is host-raised through the graph-native
/// slot-binding walk).
///
/// The first parameter is the slot-props object. It can be written several ways —
/// a literal object, a `Pick<T, 'k'>` / `Omit<…>` over a named type, an aliased
/// `Ref`, or a parenthesized form — plus the multi-arm intersected node a `Union`
/// / `Intersection` slot produces. To cover all of them WITHOUT a nominal
/// shape-sniff, the binding object is the first-param node's one-level SHALLOW
/// object surface, projected through the shared carrier-preserving
/// [`VerterHost::project_shallow_surface_from_base`]; each surface member becomes
/// a binding.
///
/// NODE-DOMAIN: the first param is a `SemanticNodeId` (the across-arms-intersected
/// slot first-param node from [`CallableNodeView::slot_param_and_return_by_arm`]),
/// NOT a materialised `TypeExpr`. The shallow projection uniformly covers a
/// literal object, a `Pick<…>`, an aliased param, AND the multi-arm intersected
/// node (which [`CallableNodeView::first_param_object_surface`] does NOT cover),
/// applying the SAME open-generic gate (`slot_param_root_is_symbolic_only`) both
/// binding paths use. A first-param node that does not project to an object
/// surface yields no bindings. Each per-member binding `TypeExpr` is minted ONCE
/// at the registered terminal [`slot_binding_field`]; this navigator holds NO
/// mint.
fn binding_fields_from_param_node(
    ctx: &dyn crate::resolver_core::ResolverContext,
    first_param: SemanticNodeId,
) -> Vec<AnalyzedSlotFieldBinding> {
    let dispatch = ctx.dispatch();
    let host = ctx.host_for_fact_tracer_install();
    // Open-generic gate: a symbolic-only param root (an open Conditional / mapped
    // / indexed / free `TypeParam`) must NOT be materialised into a committed
    // object surface — the SAME gate `navigate_param_to_object_surface` /
    // `first_param_object_surface` apply, keeping every binding path in agreement.
    if crate::meta_resolve::slot_binding_graph::slot_param_root_is_symbolic_only(
        &dispatch,
        first_param,
        0,
    ) {
        return Vec::new();
    }
    // Project the first-param node's one-level SHALLOW object surface (STAYS
    // Shallow / carrier-preserving — the root is NOT carrier-resolved, preserving
    // the `AppProps['avatar']` symbolic-access policy).
    let Some(surface) = host.project_shallow_surface_from_base(
        ctx,
        &dispatch,
        first_param,
        Arc::from(Vec::<PathSegment>::new().into_boxed_slice()),
        ProjectionReductionContext::published(ProjectionMode::Shallow),
        None,
    ) else {
        return Vec::new();
    };
    // Shallow-by-default Pick member publication: when the slot param is a
    // `Pick<NamedRoot, K>` the picked members stay SYMBOLIC at the published
    // binding surface — the terminal sink builds each binding's value as the typed
    // indexed access `NamedRoot['member']`. The Pick source-root is read
    // NODE-DOMAIN from the first-param node (a thin structural `InstantiationRef`
    // read under the Vue Pick DTO policy — never a `"Pick<"` text sniff).
    let pick_root = pick_source_root_node(&dispatch, first_param);
    surface
        .members
        .iter()
        // Public-only publication.
        .filter(|member| member.visibility.is_public())
        .map(|member| slot_binding_field(ctx, member, pick_root))
        .collect()
}

/// The Pick source-root NODE the Vue Pick DTO policy publishes each picked member
/// against. Peels the first-param node through the carrier-PRESERVING
/// [`ProjectSemanticDispatch::peel_node_for_uninstantiated_carrier_fact_demand`]
/// to reach an un-instantiated `Pick<Root, K>` `InstantiationRef`, then returns
/// its source-root arg (`args[0]`) ONLY when BOTH hold:
///
/// - **Builtin-Pick identity** — the carrier is the BUILTIN `Pick`
///   (`base.canonical_id == "__builtin__"` AND `base.decl_name == "Pick"`, two
///   args): the SAME builtin-utility identity the resolver's route extractors read
///   (`extract_route_root_identity_node` / `node_root_identity` in
///   `meta_resolve::graph_predicates`). A USERLAND `type Pick<T, K>` that shadows
///   the builtin is NOT a builtin `Pick` — its carrier base is the declaring file,
///   not `__builtin__`, so it fails here and each member mints its own concrete
///   (userland-Pick body) value instead of a symbolic access.
/// - **Nominal source root** — `args[0]` is a NOMINAL named reference (`DeclRef` /
///   `InstantiationRef` / the unresolved macro-carrier `BareRef`) — the reachable
///   breadth of an authored named-reference source. An INLINE macro-authored
///   `Pick<Source, K>` (in the `defineSlots` payload itself) lowers its source
///   root to `SemanticNodeData::BareRef` — the node-domain mirror of an authored
///   named reference — and the published binding shape must NOT depend on
///   inline-vs-named authorship, so `BareRef` is in the nominal set. A
///   STRUCTURAL source (`Pick<{ foo: string }, "foo">`) lowers `args[0]` to an
///   object/structural node — NOT nominal — so publishing a symbolic
///   `<object>['foo']` access would be bogus: return `None` and let each member
///   mint its own concrete member value.
///
/// NOTE: this predicate does NOT mirror `extract_pick_omit_route`'s arm set. In
/// that route extractor a non-nominal root returns `None` to PRESERVE the
/// carrier; here `None` means CONCRETE materialization — the OPPOSITE
/// consequence — so the arm sets are intentionally different.
///
/// A typed-IR STRUCTURAL match (node-domain `canonical_id` / `decl_name` + the
/// source-root shape) — NOT a `"Pick<"` text sniff and NOT a
/// materialise-then-decide. Any other shape (a literal object, a multi-arm
/// `Intersection` first param, a userland or non-Pick alias) returns `None`.
fn pick_source_root_node(
    dispatch: &ProjectSemanticDispatch<'_>,
    first_param: SemanticNodeId,
) -> Option<SemanticNodeId> {
    // A PARTIAL peel yields `None` — the existing safe concrete-materialization
    // fallback (each member mints its own concrete value). The builtin-Pick
    // identity checks below apply ONLY to a Complete peel: a truncated peel
    // must never publish a symbolic `NamedRoot['member']` access off an
    // operationally-incomplete carrier read.
    let peeled = dispatch
        .peel_node_for_uninstantiated_carrier_fact_demand(
            first_param,
            ProjectionReductionContext::published(ProjectionMode::Navigate),
        )
        .into_complete_node()?;
    match node_data_for(dispatch.ctx, peeled).as_deref() {
        Some(SemanticNodeData::InstantiationRef { base, args })
            if base.canonical_id.as_ref() == "__builtin__"
                && base.decl_name.as_ref() == "Pick"
                && args.len() == 2 =>
        {
            // Nominal-root restriction: publish the symbolic
            // `NamedRoot['member']` access for a nominal named-reference source
            // root — `DeclRef` / `InstantiationRef` / the unresolved
            // macro-carrier `BareRef` (an inline macro-authored source); a
            // structural source mints each member's own concrete value at the
            // sink instead. NOT `extract_pick_omit_route`'s arm set — there
            // `None` PRESERVES the carrier, here `None` means CONCRETE
            // materialization.
            let root = args[0];
            match node_data_for(dispatch.ctx, root).as_deref() {
                Some(
                    SemanticNodeData::DeclRef { .. }
                    | SemanticNodeData::InstantiationRef { .. }
                    | SemanticNodeData::BareRef(_),
                ) => Some(root),
                _ => None,
            }
        }
        _ => None,
    }
}

/// Build ONE published slot binding for a surface member — a GENUINE decide-free
/// terminal one-shot sink. It takes NO `&TypeExpr` param (a surface member and
/// the node-domain `Option<SemanticNodeId>` Pick source-root) and makes NO
/// decision on any materialised value:
///
/// - a `Pick` member (`pick_root == Some`) renders the SYMBOLIC
///   `NamedRoot['member']` indexed-access DISPLAY text — the source root is
///   minted ONCE (internally) and the `IndexedAccess` is a pure syntactic
///   display build (NOT a reverse-materialisation), the shallow-by-default
///   Pick policy;
/// - any other member mints its own value ONCE through the registered
///   [`raise_member_value`] sink for the DISPLAY `type_annotation`. On a raise
///   miss — the member's value node does not materialize, a torn graph read —
///   the binding fabricates NO display text and marks the request's
///   materialization-cache suppress so the torn result is never admitted warm
///   as complete metadata (the no-poison completion fence).
///
/// The published binding is locator-less (`payload: None`, paired with a
/// `None` scope): the flat field-position vocabulary cannot address a nested
/// (slot, binding) position honestly, so typed binding demand is host-raised
/// through the graph-native slot-binding walk. The `pick_root` branch is a
/// NODE-DOMAIN `Option` match, never a `TypeExpr` decide; the display renders
/// through the by-name `.and_then` form. The mint cap is constructed
/// INTERNALLY from `ctx` (the `raise_member_value` pattern).
fn slot_binding_field(
    ctx: &dyn crate::resolver_core::ResolverContext,
    member: &TypeInfoSurfaceMember,
    pick_root: Option<SemanticNodeId>,
) -> AnalyzedSlotFieldBinding {
    let Some(root_node) = pick_root else {
        // Non-Pick member: mint the member's own value ONCE through the
        // registered [`raise_member_value`] sink for DISPLAY ONLY.
        let raised = raise_member_value(ctx, member);
        // Raise-miss fence: an absent mint is a torn graph read — mark the
        // request's materialization-cache suppress so the torn result is never
        // admitted warm as complete metadata (a pure absence check, never a
        // variant decide on the materialized value).
        if raised.is_none() {
            crate::request_context::mark_request_result_partial();
        }
        // Display renders through the by-name `.and_then` form, so an
        // unraisable value fabricates no display text.
        let type_annotation = raised.as_ref().and_then(render_type_expr_display);
        return AnalyzedSlotFieldBinding {
            name: member.name.as_ref().to_string(),
            type_annotation,
            payload: None,
            binding_expr_scope: None,
            span: verter_span::Span::default(),
        };
    };
    // Mint the Pick source-root ONCE, then build the symbolic
    // `NamedRoot['member']` display access (a pure syntactic build).
    let dispatch = ctx.dispatch();
    let cap = super::TypeinfoVueSurfaceOutputCap::new(&dispatch);
    let named_root = cap
        .materialize_output_type_expr(root_node)
        .map(|raised| raised.into_type_expr(&cap))
        .unwrap_or(TypeExpr::Unknown { raw: String::new() });
    let symbolic = TypeExpr::IndexedAccess {
        object: Arc::new(named_root),
        index: Arc::new(TypeExpr::Literal(LiteralValue::String(
            member.name.as_ref().to_string(),
        ))),
    };
    let type_annotation = render_type_expr_display(&symbolic);
    AnalyzedSlotFieldBinding {
        name: member.name.as_ref().to_string(),
        type_annotation,
        payload: None,
        binding_expr_scope: None,
        span: verter_span::Span::default(),
    }
}

#[cfg(test)]
mod raise_miss_normalization_tests {
    use std::sync::Arc;

    use verter_type_expr::MemberVisibility;

    use super::slot_binding_field;
    use crate::request_context::{current_cold_compute_completeness, ColdComputeCompletenessScope};
    use crate::semantic_query::{MemberMergeRole, SemanticNodeId};
    use crate::typeinfo::surface::{JsdocTagSpan, SurfaceMemberOrigin, TypeInfoSurfaceMember};
    use crate::types::HostConfig;
    use crate::VerterHost;

    fn make_host() -> Arc<VerterHost> {
        Arc::new(VerterHost::new_standalone(HostConfig::default()))
    }

    /// A synthetic surface member whose `value` node id was never interned in
    /// the host's semantic graph, so `raise_member_value` genuinely MISSES
    /// (`materialize_output_type_expr` returns `None` for an absent node).
    fn raise_miss_member() -> TypeInfoSurfaceMember {
        TypeInfoSurfaceMember {
            name: Arc::from("item"),
            name_span: None,
            value: SemanticNodeId(u64::MAX),
            type_annotation_span: None,
            optional: false,
            readonly: false,
            is_method: false,
            visibility: MemberVisibility::Public,
            declared_in_macro_type_arg: false,
            jsdoc_description_span: None,
            jsdoc_tag_spans: Arc::from(Vec::<JsdocTagSpan>::new().into_boxed_slice()),
            origin: SurfaceMemberOrigin {
                canonical_file: None,
                declaration_span: None,
                merge_role: MemberMergeRole::Authored,
            },
        }
    }

    /// The resolver-published slot-binding invariant on a raise MISS: the
    /// binding fabricates NO display text, stays locator-less (`payload: None`
    /// paired with a `None` scope), and marks the cold compute PARTIAL so the
    /// torn graph read is never warmed as complete metadata (the no-poison
    /// fence). Exercised through the non-Pick arm of the registered
    /// [`slot_binding_field`] terminal (`pick_root: None`).
    #[test]
    fn slot_binding_raise_miss_fabricates_no_display_and_suppresses_warm_admission() {
        let host = make_host();
        let member = raise_miss_member();

        let guard = ColdComputeCompletenessScope::enter();
        let binding = slot_binding_field(&*host, &member, None);
        let completeness = current_cold_compute_completeness();
        drop(guard);

        assert!(
            binding.type_annotation.is_none(),
            "no display text is fabricated for an unraisable binding value"
        );
        assert!(
            binding.payload.is_none() && binding.binding_expr_scope.is_none(),
            "a slot binding stays locator-less (payload: None paired with a None scope)"
        );
        assert!(
            completeness.is_partial(),
            "a raise miss must mark the cold compute partial so the torn \
             result is never admitted warm"
        );
    }

    /// Direct partial-`Pick` consumer discriminator. A builtin
    /// `Pick<Source, "foo">` over a NOMINAL source root (`DeclRef(Source)`) is
    /// reached by `pick_source_root_node`'s carrier-preserving peel: when the
    /// peel is `Complete` it selects the SYMBOLIC source root (`Some(Source)` ⇒
    /// the sink publishes the shallow-by-default `Source['foo']` access), but
    /// when the peel is `Partial` (an operationally-truncated nested read) the
    /// node-hiding `into_complete_node()?` MUST yield `None` so the sink falls
    /// back to CONCRETE materialization instead of publishing a symbolic access
    /// off an incomplete carrier read.
    ///
    /// DISCRIMINATING: the SAME `Pick<Source, "foo">` fixture is driven twice.
    ///   - ARMED (the forced-result-partial hook makes the nested
    ///     `ResolveDecl(PickAlias)` build `result_is_partial`) the peel is
    ///     `Partial` and `pick_source_root_node` returns `None`.
    ///   - UNARMED (a genuinely `Complete` peel — the armed run never warmed the
    ///     partial-refused `ResolveDecl`, so this re-runs cold) it returns
    ///     `Some(Source)`, PROVING the fixture genuinely reaches a builtin Pick
    ///     with a nominal source root (so the armed `None` is the node-hiding,
    ///     NOT a fixture always-miss).
    ///
    /// The pre-change bare-node peel returned the reached Pick node EVEN on
    /// truncation, so it would have selected `Some(Source)` in the ARMED case
    /// too. Revert the `Partial => Partial(reasons)` arm of
    /// `resolve_structural_fact_demand` to `Partial => Complete(n)` (the OLD
    /// bare-node behaviour) and the ARMED assertion returns `Some` — the test
    /// FAILS pre-change, PASSES post-change.
    #[test]
    fn pick_source_root_node_on_partial_peel_returns_none_for_concrete_fallback() {
        use crate::project_semantic_dispatch::ProjectSemanticDispatch;
        use crate::semantic_query::{DeclIdentity, NodeScopeId, SemanticNodeData};
        use crate::types::{FileLanguage, UpsertRequest};

        let host = make_host();
        let _ = host
            .upsert(UpsertRequest {
                canonical_id: None,
                input_id: "/dep.ts".to_string(),
                source: Arc::from(
                    "export type Source = { foo: string; bar: number };\n\
                 export type PickAlias = Pick<Source, 'foo'>;\n",
                ),
                file_language: FileLanguage::script_ts(),
                aliases: Vec::new(),
            })
            .unwrap();
        let dispatch = ProjectSemanticDispatch::new(&*host);
        let graph = host.project_type_store().semantic_graph();
        let shallow = dispatch
            .ctx
            .shallow_file_state("/dep.ts")
            .expect("/dep.ts must index");
        let scope = NodeScopeId::File {
            canonical_id: Arc::from("/dep.ts"),
            owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
            whole_hash: shallow.whole_hash,
            local_scope: None,
        };
        // `first_param = DeclRef(PickAlias)` — the peel resolves it via
        // `ResolveDecl` to the builtin `Pick<Source, "foo">` `InstantiationRef`.
        let first_param = graph.intern_node(SemanticNodeData::DeclRef {
            identity: DeclIdentity::from_scope(&scope, Arc::from("PickAlias")),
        });

        // ARMED FIRST (cold `ResolveDecl` ⇒ result_is_partial ⇒ peel Partial ⇒
        // node-hiding None). Armed BEFORE the Complete control so the
        // partial-refused `ResolveDecl` is never warmed into a Complete hit. The
        // knob is PER-HOST (not process-global), so a concurrently-running test
        // on another host cannot be contaminated.
        let armed_root = {
            host.test_force
                .force_result_partial_for_tests
                .store(true, std::sync::atomic::Ordering::Relaxed);
            let root = super::pick_source_root_node(&dispatch, first_param);
            host.test_force
                .force_result_partial_for_tests
                .store(false, std::sync::atomic::Ordering::Relaxed);
            root
        };
        assert!(
            armed_root.is_none(),
            "a Partial (operationally-truncated) peel MUST yield None (concrete-materialization \
             fallback), never a symbolic source root off an incomplete carrier read — the \
             pre-change bare-node peel returned the reached Pick node even on truncation and would \
             select Some(Source) here; the node-hiding into_complete_node() is what makes it None"
        );

        // UNARMED positive control: a Complete peel of the SAME fixture selects
        // the nominal symbolic source root, proving the armed None is the
        // node-hiding, not an always-miss (the armed partial ResolveDecl was
        // never warmed, so this re-runs cold ⇒ Complete).
        let complete_root = super::pick_source_root_node(&dispatch, first_param);
        assert!(
            complete_root.is_some(),
            "FIXTURE INVALID: a Complete peel of Pick<Source, \"foo\"> must reach the builtin Pick \
             and select the nominal source root (the symbolic access) — else the armed None above \
             is a fixture always-miss, not the node-hiding"
        );
    }
}
