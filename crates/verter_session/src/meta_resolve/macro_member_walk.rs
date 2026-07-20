//! Helpers used by the per-macro projector decomposition:
//! `defineProps<T>()` root-name collection and the slot-binding
//! registry-collection skip predicate. Production routes through
//! `meta_resolve::projectors::project_evaluated_types`; per-member
//! reduction is owned by the projector via
//! `materialize_component_meta_type_expr_until_stable`.

use crate::types::FileAnalysisSnapshot;

/// Capture-token counter name recorded every time the slot-binding
/// registry-collection skip predicate fires for a slot binding rooted
/// in the owner's own `defineProps<T>()` interface. Used by
/// `component_meta_slot_binding_skip_tests` to discriminate the
/// positive case (counter > 0) from the counterfixtures (counter == 0)
/// via `CaptureToken::start_for_query` / `CaptureToken::end()`.
///
/// Test/debug instrumentation only — gated to match the capture-token
/// module (absent in release).
#[cfg(any(test, feature = "test-support"))]
pub(crate) const SLOT_BINDING_REGISTRY_COLLECTION_SKIP_COUNTER: &str =
    "slot_binding_registry_collection_skips";

/// Issue #10 / capture-token counter for Pick member-route callable
/// parameter descent. Registry publication is shallow (content-free
/// sources; no eager member-route materialisation), so no production
/// path descends into a callable parameter type and the counter reads
/// zero. `component_meta_pick_omit_tests` pins `== 0` on the
/// package-backed fixtures — a non-zero reading means an eager
/// callable-parameter descent path re-appeared.
///
/// Test/debug instrumentation only — gated to match the capture-token
/// module (absent in release).
#[cfg(test)]
pub(crate) const PICK_MEMBER_ROUTE_CALLABLE_DESCENT_COUNTER: &str =
    "pick_member_route_callable_descent_count";

/// Collect the source-level root type names referenced by every
/// type-based `defineProps<T>()` macro in `snapshot.macros`. The
/// result is the set of names that — when found at the root of a
/// slot binding's raw type — make the binding's contribution to the
/// component-meta registry redundant (the defineProps interface is
/// already authoritative for that surface).
///
/// Only top-level `Ref { name }` roots are collected. Inline
/// object-literal type arguments, intersections, unions, and other
/// composite shapes do not produce a root name (the predicate
/// downstream falls back to `false` for those cases).
pub(crate) fn collect_define_props_root_names(
    ctx: &dyn crate::resolver_core::ResolverContext,
    owner_canonical: &str,
    snapshot: &FileAnalysisSnapshot,
) -> rustc_hash::FxHashSet<String> {
    use verter_semantic::analysis::AnalyzedMacroKind;

    let mut names: rustc_hash::FxHashSet<String> = rustc_hash::FxHashSet::default();
    for (macro_index, mac) in snapshot.macros.iter().enumerate() {
        if mac.kind != AnalyzedMacroKind::DefineProps || !mac.is_type_based {
            continue;
        }
        if mac.parsed_type_argument.is_none() {
            continue;
        }
        // The type argument's root reference name is read off its structural
        // mirror node (the ONE sanctioned type-arg producer; parens are
        // structurally transparent there) — never a stored body.
        let Some(handle) = crate::structural_carrier_producer::macro_type_arg_hot_ref(
            ctx,
            owner_canonical,
            macro_index,
        ) else {
            continue;
        };
        let Some(data) = crate::project_semantic_dispatch::node_data_for(ctx, handle.node()) else {
            continue;
        };
        if let Some((name, _)) = data.bare_ref_head() {
            names.insert(name.as_ref().to_string());
        }
    }
    names
}

/// Decide whether the slot-binding's raw-type root is the owner's
/// own `defineProps<T>()` interface — in which case the binding's
/// registry-collection contribution is redundant and can be
/// skipped.
///
/// The predicate fires only when the binding's raw type root
/// resolves to a name in `define_props_roots` for the same owner
/// AND the binding does NOT introduce a new prop surface beyond
/// what the defineProps root already exposes:
///
/// - `Props['avatar']` (indexed-access route) → fires when
///   `Props ∈ define_props_roots`.
/// - `Pick<Props, 'avatar' | 'label'>` / `Omit<Props, 'count'>`
///   (utility route) → fires when `Props ∈ define_props_roots`.
/// - `Props & Extra` (intersection broadens surface) → does NOT
///   fire. The intersection's `Extra` arm is reachable only
///   through the registry-collection call.
/// - `Props | Other` (union) → does NOT fire. Same reasoning as
///   intersection.
/// - `ButtonProps['avatar']` where `ButtonProps` is imported (not
///   the owner's defineProps root) → does NOT fire.
/// - `Primitive(_)` / `Object(_)` / fully-expanded fields whose
///   raw type was None → does NOT fire (no work to skip).
pub(crate) fn slot_binding_targets_define_props_root(
    ctx: &dyn crate::resolver_core::ResolverContext,
    owner_canonical: &str,
    owner: verter_type_expr::TopLevelOwnerId,
    field: &verter_semantic::analysis::type_expand::ExpandedField,
    define_props_roots: &rustc_hash::FxHashSet<String>,
) -> bool {
    use crate::semantic_query::{IndexKey, SemanticNodeData};

    if define_props_roots.is_empty() {
        return false;
    }

    // Prefer the shallow AUTHORED source when the analyzer stamped one (the
    // bare annotation the user wrote), otherwise the post-expansion resolved
    // source. Both raise through the shared dispatch bridge — the root
    // extraction below runs in NODE DOMAIN off the raised carrier.
    let dispatch = crate::project_semantic_dispatch::ProjectSemanticDispatch::new(ctx);
    let transit_ctx =
        crate::semantic_query::ProjectionReductionContext::structural_transit_with_mode(
            crate::semantic_query::ProjectionMode::Navigate,
        );
    let raised = field
        .shallow_source
        .as_ref()
        .and_then(|locator| dispatch.raise_authored_locator_to_hot(locator, transit_ctx))
        .or_else(|| {
            dispatch.raise_semantic_type_source_to_hot(
                field.r#type.present()?,
                crate::project_semantic_dispatch::semantic_source::SourceRaiseContext {
                    scope_canonical_id: owner_canonical,
                    scope_owner: owner,
                    context: transit_ctx,
                    interior_failures: None,
                },
            )
        });
    let Some(raised) = raised else {
        return false;
    };
    // A graph-raised binding row published against the first-class synthetic
    // carrier classifies through its SAME-GENERATION value-node seed (the
    // carrier's value-side provenance): the seed IS the lowered binding value
    // (`Props['avatar']`), exactly the node the root extraction below walks.
    let subject =
        match crate::project_semantic_dispatch::node_data_for(ctx, raised.node()).as_deref() {
            Some(SemanticNodeData::SyntheticBinding { value_node, .. }) => {
                crate::semantic_query::SemanticNodeId(*value_node)
            }
            _ => raised.node(),
        };
    let Some(data) = crate::project_semantic_dispatch::node_data_for(ctx, subject) else {
        return false;
    };

    // Broadening shapes (intersection / union) must NOT skip — extra
    // arms beyond the defineProps surface would be lost.
    if matches!(
        data.as_ref(),
        SemanticNodeData::Intersection(_) | SemanticNodeData::Union(_)
    ) {
        return false;
    }

    // Node-domain indexed-access / utility route root extraction: the
    // source-level root name (`Props` for `Props['avatar']` or
    // `Pick<Props, …>`) when the raised carrier is structurally a path
    // projection rooted at a single reference. Parens are structurally
    // transparent in the graph.
    let root_name = |node: crate::semantic_query::SemanticNodeId| -> Option<String> {
        let data = crate::project_semantic_dispatch::node_data_for(ctx, node)?;
        // A builtin object-filter utility application (`Pick<Props, …>` /
        // `Omit<Props, …>`): the root is the SOURCE argument's reference head.
        if let Some((name, _)) = data.bare_ref_head() {
            let args = data.carrier_type_args();
            let is_utility =
                verter_semantic::analysis::type_solver::builtin::BuiltinUtility::from_name(
                    name.as_ref(),
                )
                .is_some();
            if is_utility && !args.is_empty() {
                let source = crate::project_semantic_dispatch::node_data_for(ctx, args[0])?;
                let (source_name, _) = source.bare_ref_head()?;
                return Some(source_name.as_ref().to_string());
            }
            return None;
        }
        None
    };
    let indexed_access_root = |node: crate::semantic_query::SemanticNodeId| -> Option<String> {
        let mut current = node;
        loop {
            let data = crate::project_semantic_dispatch::node_data_for(ctx, current)?;
            match data.as_ref() {
                SemanticNodeData::IndexedAccess { object, index } => {
                    if !matches!(index, IndexKey::String(_) | IndexKey::Number(_)) {
                        return None;
                    }
                    current = *object;
                }
                _ => {
                    if let Some((name, _)) = data.bare_ref_head() {
                        return data
                            .carrier_type_args()
                            .is_empty()
                            .then(|| name.as_ref().to_string());
                    }
                    // A root the lowering already RESOLVED interns the
                    // `DeclRef` identity carrier — same root, same name.
                    if let SemanticNodeData::DeclRef { identity } = data.as_ref() {
                        return Some(identity.decl_name.as_ref().to_string());
                    }
                    return None;
                }
            }
        }
    };

    let root = root_name(subject).or_else(|| {
        // The indexed-access route fires only for a genuine access chain, not
        // a bare reference (parity with the former route extractors).
        matches!(data.as_ref(), SemanticNodeData::IndexedAccess { .. })
            .then(|| indexed_access_root(subject))
            .flatten()
    });
    root.is_some_and(|root| define_props_roots.contains(&root))
}
