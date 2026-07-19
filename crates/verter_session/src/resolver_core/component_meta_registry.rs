//! Component-meta registry publication.
//!
//! This module owns registry queueing, route merging, and publication
//! policy for routed component-meta type publication.
//!
//! See architectural rule 10: "Component-meta publication stays in resolver_core."

use std::collections::VecDeque;
use std::sync::Arc;

use crate::resolver_core::ResolverContext;
use crate::resolver_core::RouteDemand;
use crate::types::FileAnalysisSnapshot;

/// Issue #7 / capture-token counters for route-demand
/// emission. Recorded inside [`enqueue_component_meta_registry_ref`]
/// so every enqueue site reports the route variant it pushed onto
/// the queue.
///
/// Test/debug instrumentation only — gated to match the capture-token
/// module (absent in release).
#[cfg(any(test, feature = "test-support"))]
pub(crate) const ROUTE_DEMAND_EMITTED_WHOLE_COUNTER: &str = "route_demand_emitted::Whole";
#[cfg(any(test, feature = "test-support"))]
pub(crate) const ROUTE_DEMAND_EMITTED_PICK_COUNTER: &str = "route_demand_emitted::Pick";
#[cfg(any(test, feature = "test-support"))]
pub(crate) const ROUTE_DEMAND_EMITTED_MEMBER_PATH_COUNTER: &str =
    "route_demand_emitted::MemberPath";
#[cfg(any(test, feature = "test-support"))]
pub(crate) const ROUTE_DEMAND_EMITTED_OMIT_COUNTER: &str = "route_demand_emitted::Omit";

/// Map a `RouteDemand` variant to its capture-token counter name.
/// Test/debug instrumentation only — gated to match the capture-token
/// module (absent in release).
#[cfg(any(test, feature = "test-support"))]
fn route_demand_counter_name(route: &RouteDemand) -> &'static str {
    match route {
        RouteDemand::Whole => ROUTE_DEMAND_EMITTED_WHOLE_COUNTER,
        RouteDemand::Pick(_) => ROUTE_DEMAND_EMITTED_PICK_COUNTER,
        RouteDemand::MemberPath(_) => ROUTE_DEMAND_EMITTED_MEMBER_PATH_COUNTER,
        RouteDemand::Omit(_) => ROUTE_DEMAND_EMITTED_OMIT_COUNTER,
    }
}

/// Work item for the unified registry publication queue.
///
/// Combines initial entries and transitive references into a single
/// queue that the resolver-core registry publisher processes uniformly.
#[derive(Debug, Clone)]
pub enum RegistryWorkItem {
    /// Initial registry entry from the component's direct type analysis.
    InitialEntry {
        index: usize,
        declaration_source: String,
        requested_name: String,
    },
    /// Transitive type reference discovered from props/emits/slots surfaces.
    TransitiveRef {
        name: String,
        source_hint: Option<String>,
        route: crate::resolver_core::RouteDemand,
    },
}

#[derive(Debug, Clone)]
pub(crate) struct PendingComponentMetaRegistryRef {
    pub(crate) name: String,
    pub(crate) source_hint: Option<String>,
    pub(crate) source_owner: verter_type_expr::TopLevelOwnerId,
    pub(crate) exported_name: Option<String>,
    pub(crate) route: RouteDemand,
    /// Authored USE-SITE slots for single-member (`len == 1` `MemberPath`)
    /// route discoveries: `(top-level member name, the authored annotation
    /// slot that expressed the indexed access)`. A route-scoped publication
    /// RETAINS the use-site slot as the selected member's payload when the
    /// member's declaring surface sits behind a generic substitution —
    /// replaying the use-site through the one shared dispatch re-derives
    /// navigation + substitution (never a serialized post-substitution graph
    /// node). Unsubstituted members keep their declaring contributor's
    /// prepared member slot instead.
    pub(crate) member_use_sites: Vec<(String, verter_type_expr::locators::TypeBodySlot)>,
}

/// Attach a single-member route USE-SITE slot to every pending queue entry
/// for `name` (see [`PendingComponentMetaRegistryRef::member_use_sites`]).
/// Pairs are consumed BY MEMBER NAME at publication, so attaching to every
/// same-name pending is idempotent for the published surface.
pub(crate) fn attach_component_meta_registry_member_use_site(
    referenced_names: &mut VecDeque<PendingComponentMetaRegistryRef>,
    name: &str,
    member: &str,
    slot: &verter_type_expr::locators::TypeBodySlot,
) {
    for pending in referenced_names
        .iter_mut()
        .filter(|pending| pending.name == name)
    {
        if !pending
            .member_use_sites
            .iter()
            .any(|(existing, _)| existing == member)
        {
            pending
                .member_use_sites
                .push((member.to_string(), slot.clone()));
        }
    }
}

pub(crate) fn upsert_component_meta_registry_entry(
    owner_canonical: &str,
    resolved_type_registry: &mut Vec<
        verter_semantic::analysis::component_meta::ResolvedTypeAnalysis,
    >,
    resolved_type_registry_meta: &mut Vec<crate::resolver_core::ResolvedTypeRegistryMeta>,
    published_names: &mut rustc_hash::FxHashSet<String>,
    queued_names: &mut rustc_hash::FxHashSet<String>,
    referenced_names: &mut VecDeque<PendingComponentMetaRegistryRef>,
    ctx: &dyn ResolverContext,
    name: String,
    type_source: verter_type_expr::facts::SemanticTypeSource,
    declaration: crate::resolver_core::ResolvedTypeDeclaration,
    cursor: crate::meta_resolve::projection_demand::ProjectionCursor<'_>,
) {
    let declaration_source_hint =
        (!declaration.canonical_source.is_empty()).then(|| declaration.canonical_source.clone());
    let declaration_owner = declaration.owner;
    let collect_nested_refs = should_collect_component_meta_registry_nested_refs(
        owner_canonical,
        declaration_source_hint.as_deref(),
    );
    // One dispatch for candidate preference and nested-ref discovery: the
    // published content-free source raises ONCE through the shared bridge;
    // preference between an existing and an incoming candidate is decided in
    // NODE DOMAIN (`compare_node_improvement`), and transitive references are
    // discovered by the node-domain walk — no materialised `TypeExpr`.
    let dispatch = crate::project_semantic_dispatch::ProjectSemanticDispatch::new(ctx);
    let transit_ctx =
        crate::semantic_query::ProjectionReductionContext::structural_transit_with_mode(
            crate::semantic_query::ProjectionMode::Navigate,
        );
    let raise_scope = declaration_source_hint
        .as_deref()
        .unwrap_or(owner_canonical);
    let raise = |source: &verter_type_expr::facts::SemanticTypeSource| {
        dispatch.raise_semantic_type_source_to_hot(
            source,
            crate::project_semantic_dispatch::semantic_source::SourceRaiseContext {
                scope_canonical_id: raise_scope,
                scope_owner: declaration_owner,
                context: transit_ctx,
                interior_failures: None,
            },
        )
    };
    if let Some(index) = resolved_type_registry
        .iter()
        .position(|entry| entry.name == name)
    {
        if resolved_type_registry[index].type_source.present() == Some(&type_source) {
            return;
        }
        // Monotonic route-scoped combination (route-demand publication
        // encoding): multiple route discoveries for one name COMBINE —
        // `Whole` dominates; otherwise the selected top-level members union
        // deterministically (existing surface order first, then new-only
        // members in candidate order).
        enum RouteCombine {
            /// Both sides are projected surfaces → publish the member union.
            Union(verter_type_expr::facts::SemanticTypeSource),
            /// Existing WHOLE authored body dominates the route-scoped
            /// candidate → keep the existing entry.
            KeepWhole,
            /// WHOLE authored candidate dominates the existing route-scoped
            /// surface → replace it.
            ReplaceWithWhole,
            /// Not a route-combination pair → fall through to the
            /// node-domain improvement comparator.
            Fallthrough,
        }
        let decision = {
            use verter_type_expr::facts::{ProjectedTypeFact, SemanticTypeSource};
            match (
                resolved_type_registry[index].type_source.present(),
                &type_source,
            ) {
                (
                    Some(SemanticTypeSource::Projected(ProjectedTypeFact::Surface(current))),
                    SemanticTypeSource::Projected(ProjectedTypeFact::Surface(candidate)),
                ) => {
                    let mut members = current.members.to_vec();
                    for member in candidate.members.iter() {
                        if !members.iter().any(|existing| existing.name == member.name) {
                            members.push(member.clone());
                        }
                    }
                    let union = verter_type_expr::facts::ProjectedSurfaceFact {
                        members: std::sync::Arc::from(members.into_boxed_slice()),
                        call_signatures: current.call_signatures.clone(),
                        construct_signatures: current.construct_signatures.clone(),
                        index_signatures: current.index_signatures.clone(),
                        has_index_signature: current.has_index_signature
                            || candidate.has_index_signature,
                    };
                    RouteCombine::Union(SemanticTypeSource::Projected(ProjectedTypeFact::Surface(
                        union,
                    )))
                }
                (
                    Some(SemanticTypeSource::Authored(_)),
                    SemanticTypeSource::Projected(ProjectedTypeFact::Surface(_)),
                ) => RouteCombine::KeepWhole,
                (
                    Some(SemanticTypeSource::Projected(ProjectedTypeFact::Surface(_))),
                    SemanticTypeSource::Authored(_),
                ) => RouteCombine::ReplaceWithWhole,
                _ => RouteCombine::Fallthrough,
            }
        };
        match decision {
            RouteCombine::Union(union_source) => {
                if resolved_type_registry[index].type_source.present() != Some(&union_source) {
                    resolved_type_registry[index].type_source =
                        verter_type_expr::facts::SourcePosition::Present(union_source.clone());
                    if collect_nested_refs {
                        if let Some(hot) = raise(&union_source) {
                            collect_component_meta_registry_refs_node(
                                ctx,
                                hot.node(),
                                published_names,
                                queued_names,
                                referenced_names,
                                declaration_source_hint.as_deref(),
                                declaration_owner,
                                RegistryMemberRefPolicy::PublicationBoundary,
                                cursor,
                            );
                        }
                    }
                }
                return;
            }
            RouteCombine::KeepWhole => return,
            RouteCombine::ReplaceWithWhole => {
                resolved_type_registry[index].type_source =
                    verter_type_expr::facts::SourcePosition::Present(type_source.clone());
                if let Some(meta) = resolved_type_registry_meta.get_mut(index) {
                    *meta = crate::resolver_core::ResolvedTypeRegistryMeta {
                        name: name.clone(),
                        declaration,
                    };
                }
                if collect_nested_refs {
                    if let Some(hot) = raise(&type_source) {
                        collect_component_meta_registry_refs_node(
                            ctx,
                            hot.node(),
                            published_names,
                            queued_names,
                            referenced_names,
                            declaration_source_hint.as_deref(),
                            declaration_owner,
                            RegistryMemberRefPolicy::PublicationBoundary,
                            cursor,
                        );
                    }
                }
                return;
            }
            RouteCombine::Fallthrough => {}
        }
        // An unraisable candidate never replaces the existing source; a
        // raisable candidate replaces an unraisable existing; both raisable
        // → the node-domain improvement comparator decides.
        let improves = match (
            raise(&type_source),
            resolved_type_registry[index]
                .type_source
                .present()
                .and_then(raise),
        ) {
            (Some(candidate), Some(current)) => {
                crate::meta_resolve::compare_node_improvement(ctx, candidate.node(), current.node())
            }
            (Some(_), None) => true,
            (None, _) => false,
        };
        if improves {
            resolved_type_registry[index].type_source =
                verter_type_expr::facts::SourcePosition::Present(type_source.clone());
            if let Some(meta) = resolved_type_registry_meta.get_mut(index) {
                *meta = crate::resolver_core::ResolvedTypeRegistryMeta {
                    name: name.clone(),
                    declaration,
                };
            }
            if collect_nested_refs {
                if let Some(hot) = raise(&type_source) {
                    collect_component_meta_registry_refs_node(
                        ctx,
                        hot.node(),
                        published_names,
                        queued_names,
                        referenced_names,
                        declaration_source_hint.as_deref(),
                        declaration_owner,
                        RegistryMemberRefPolicy::PublicationBoundary,
                        cursor,
                    );
                }
            }
        }
        return;
    }

    if collect_nested_refs {
        if let Some(hot) = raise(&type_source) {
            collect_component_meta_registry_refs_node(
                ctx,
                hot.node(),
                published_names,
                queued_names,
                referenced_names,
                declaration_source_hint.as_deref(),
                declaration_owner,
                RegistryMemberRefPolicy::PublicationBoundary,
                cursor,
            );
        }
    }
    resolved_type_registry.push(
        verter_semantic::analysis::component_meta::ResolvedTypeAnalysis {
            name: name.clone(),
            type_source: verter_type_expr::facts::SourcePosition::Present(type_source),
            type_expansion: None,
        },
    );
    resolved_type_registry_meta.push(crate::resolver_core::ResolvedTypeRegistryMeta {
        name: name.clone(),
        declaration,
    });
    published_names.insert(name);
}

pub(crate) fn should_collect_component_meta_registry_nested_refs(
    owner_canonical: &str,
    source_hint: Option<&str>,
) -> bool {
    match source_hint.filter(|source| !source.is_empty()) {
        Some(source) => source == owner_canonical,
        None => true,
    }
}

pub(crate) fn owner_component_meta_registry_import_root(
    ctx: &dyn ResolverContext,
    owner_canonical: &str,
    owner: verter_type_expr::TopLevelOwnerId,
    _snapshot: &FileAnalysisSnapshot,
    local_name: &str,
) -> Option<(String, verter_type_expr::TopLevelOwnerId, String)> {
    let identity = crate::resolver_core::bare_name_resolve::resolve_bare_name_in_scope(
        ctx,
        owner_canonical,
        owner,
        None,
        local_name,
    )?;
    (identity.canonical_id.as_ref() != owner_canonical || identity.owner != owner).then(|| {
        (
            identity.canonical_id.to_string(),
            identity.owner,
            identity.symbol_name.to_string(),
        )
    })
}

/// Issue #7 / true when the named alias's prepared body
/// resolves (modulo single alias-of-alias indirection) to a
/// `ComponentConfig<...>` reference with a nonempty argument list —
/// classified NODE-DOMAIN off the prepared declaration's lowered body slot.
///
/// Returns `false` for:
/// - missing prepared decl / unraisable body
/// - body root is a type parameter
/// - body root is a non-generic reference to anything other than another
///   local alias whose body satisfies the rule (alias-of-alias depth 1)
/// - body root is a `ComponentConfig` reference with no type arguments
pub(crate) fn component_meta_registry_owner_local_component_config_alias_name(
    ctx: &dyn ResolverContext,
    owner_canonical: &str,
    owner: verter_type_expr::TopLevelOwnerId,
    name: &str,
) -> bool {
    fn body_head(
        ctx: &dyn ResolverContext,
        owner_canonical: &str,
        owner: verter_type_expr::TopLevelOwnerId,
        name: &str,
    ) -> Option<(String, Vec<SemanticNodeId>)> {
        let prepared = ctx.prepared_type_decl_return_only(owner_canonical, owner, name)?;
        let root = prepared_body_root_node(ctx, prepared.as_ref())?;
        if node_root_is_type_parameter(ctx, root) {
            return None;
        }
        component_meta_registry_node_ref_head(ctx, root)
    }

    let Some((ref_name, type_arguments)) = body_head(ctx, owner_canonical, owner, name) else {
        return false;
    };
    if ref_name == "ComponentConfig" && !type_arguments.is_empty() {
        return true;
    }
    // Single alias-of-alias indirection — follow once.
    if type_arguments.is_empty() {
        if let Some((nested_name, nested_args)) =
            body_head(ctx, owner_canonical, owner, ref_name.as_str())
        {
            return nested_name == "ComponentConfig" && !nested_args.is_empty();
        }
    }
    false
}

/// Issue #7 / predicate for the registry public-route
/// rewrite. Returns `Some(root_name)` when ALL of:
/// - the route root has no import binding on the owner (so the alias
///   resolves to a workspace-local declaration)
/// - the prepared body is not a `TypeParameter`
/// - the alias body is `ComponentConfig<...>` (or alias-of-alias to
///   that)
/// - `ComponentConfig` itself is not imported in the owner's scope
///
/// On owner-local hit, the caller MUST emit `RouteDemand::Whole`
/// instead of the standard `MemberPath`/`Pick` route.
pub(crate) fn component_meta_registry_public_route_owner_local_root(
    ctx: &dyn ResolverContext,
    owner_canonical: &str,
    owner: verter_type_expr::TopLevelOwnerId,
    snapshot: &FileAnalysisSnapshot,
    route_root_name: Option<&str>,
    source_hint: Option<&str>,
) -> Option<String> {
    // Source hint must be either None (component-local) or match the
    // owner — anything else is from a different owner's scope.
    if let Some(source) = source_hint.filter(|source| !source.is_empty()) {
        if source != owner_canonical {
            return None;
        }
    }

    // The route's root name (utility or indexed access), extracted by the
    // caller in its own carrier domain.
    let root_name = route_root_name?.to_string();

    // Owner-local rule: no import binding on `root_name`. If the
    // resolver routes the name to an external file, it is imported.
    if owner_component_meta_registry_import_root(
        ctx,
        owner_canonical,
        owner,
        snapshot,
        root_name.as_str(),
    )
    .is_some()
    {
        return None;
    }

    // ComponentConfig itself must NOT be imported in the owner's
    // scope.
    if owner_component_meta_registry_import_root(
        ctx,
        owner_canonical,
        owner,
        snapshot,
        "ComponentConfig",
    )
    .is_some()
    {
        return None;
    }

    // Alias body must be `ComponentConfig<...>`.
    if !component_meta_registry_owner_local_component_config_alias_name(
        ctx,
        owner_canonical,
        owner,
        root_name.as_str(),
    ) {
        return None;
    }

    Some(root_name)
}

/// Recover owner-local utility/indexed routes authored at a macro's type-arg
/// root before public-field expansion consumes that wrapper. Per-field
/// collection cannot recover `defineProps<Pick<Foo, K>>()` from the expanded
/// `Foo` members, while nested field annotations retain their own structural
/// locators and continue through the existing field collector.
pub(crate) fn collect_component_meta_registry_public_macro_root_refs(
    ctx: &dyn ResolverContext,
    owner_canonical: &str,
    snapshot: &FileAnalysisSnapshot,
    published_names: &rustc_hash::FxHashSet<String>,
    queued_names: &mut rustc_hash::FxHashSet<String>,
    output: &mut VecDeque<PendingComponentMetaRegistryRef>,
    source_hint: Option<&str>,
) {
    for (macro_index, macro_call) in snapshot.macros.iter().enumerate() {
        if !macro_call.is_type_based || macro_call.parsed_type_argument.is_none() {
            continue;
        }
        let Some(hot) = crate::structural_carrier_producer::macro_type_arg_hot_ref(
            ctx,
            owner_canonical,
            macro_index,
        ) else {
            continue;
        };
        let route_root_name = component_meta_registry_node_utility_route(ctx, hot.node())
            .or_else(|| component_meta_registry_node_indexed_access_route(ctx, hot.node()))
            .map(|(root, _)| root);
        let Some(owner_local_root) = component_meta_registry_public_route_owner_local_root(
            ctx,
            owner_canonical,
            macro_call.owner,
            snapshot,
            route_root_name.as_deref(),
            source_hint,
        ) else {
            continue;
        };
        enqueue_component_meta_registry_ref(
            published_names,
            queued_names,
            output,
            owner_local_root.as_str(),
            source_hint,
            macro_call.owner,
            None,
            RouteDemand::Whole,
        );
    }
}

pub(crate) fn enqueue_component_meta_registry_ref(
    published_names: &rustc_hash::FxHashSet<String>,
    queued_names: &mut rustc_hash::FxHashSet<String>,
    referenced_names: &mut VecDeque<PendingComponentMetaRegistryRef>,
    name: &str,
    source_hint: Option<&str>,
    source_owner: verter_type_expr::TopLevelOwnerId,
    exported_name: Option<&str>,
    route: RouteDemand,
) {
    if published_names.contains(name) {
        return;
    }
    // Issue #7 / record the route-demand variant on every
    // queue admission. Tests assert
    // `route_demand_emitted::Whole >= 1` and
    // `route_demand_emitted::Pick + ::MemberPath == 0` for owner-local
    // ComponentConfig aliases (and the inverse for external imports).
    // Route-demand counter recording — test/debug instrumentation only;
    // gated to match the capture-token module (absent in release).
    #[cfg(any(test, feature = "test-support"))]
    let counter_name = route_demand_counter_name(&route);
    #[cfg(any(test, feature = "test-support"))]
    crate::capture_token::with_active_capture(|t| t.record_counter(counter_name, 1));
    let source_hint = source_hint
        .filter(|source| !source.is_empty())
        .map(str::to_string);
    let exported_name = exported_name
        .filter(|exported| !exported.is_empty())
        .map(str::to_string);
    if !queued_names.insert(name.to_string()) {
        if let Some(existing) = referenced_names.iter_mut().find(|pending| {
            pending.name == name
                && pending.source_hint == source_hint
                && pending.source_owner == source_owner
                && pending.exported_name == exported_name
                && component_meta_registry_can_merge_pending_route(&pending.route, &route)
        }) {
            existing.route = crate::resolver_core::merge_route_demands(&existing.route, &route);
        } else {
            referenced_names.push_back(PendingComponentMetaRegistryRef {
                name: name.to_string(),
                source_hint,
                source_owner,
                exported_name,
                route,
                member_use_sites: Vec::new(),
            });
        }
        return;
    }
    referenced_names.push_back(PendingComponentMetaRegistryRef {
        name: name.to_string(),
        source_hint,
        source_owner,
        exported_name,
        route,
        member_use_sites: Vec::new(),
    });
}

fn component_meta_registry_can_merge_pending_route(
    existing: &RouteDemand,
    incoming: &RouteDemand,
) -> bool {
    let merged = crate::resolver_core::merge_route_demands(existing, incoming);
    route_demand_keeps_exact_deep_member_path(existing, &merged)
        && route_demand_keeps_exact_deep_member_path(incoming, &merged)
}

fn route_demand_keeps_exact_deep_member_path(
    requested: &RouteDemand,
    merged: &RouteDemand,
) -> bool {
    match requested {
        RouteDemand::MemberPath(path) if path.len() > 1 => match merged {
            RouteDemand::Whole => true,
            RouteDemand::MemberPath(merged_path) => merged_path == path,
            _ => false,
        },
        _ => true,
    }
}

pub(crate) fn collect_component_meta_registry_public_field_refs(
    ctx: &dyn ResolverContext,
    owner_canonical: &str,
    owner: verter_type_expr::TopLevelOwnerId,
    snapshot: &FileAnalysisSnapshot,
    field: &verter_semantic::analysis::type_expand::ExpandedField,
    published_names: &rustc_hash::FxHashSet<String>,
    queued_names: &mut rustc_hash::FxHashSet<String>,
    output: &mut VecDeque<PendingComponentMetaRegistryRef>,
    source_hint: Option<&str>,
) {
    // One dispatch: the field's published SOURCE and its authored shallow
    // locator raise through the shared bridge; every route decision below
    // runs NODE-DOMAIN off the raised carriers.
    let dispatch = crate::project_semantic_dispatch::ProjectSemanticDispatch::new(ctx);
    let transit_ctx =
        crate::semantic_query::ProjectionReductionContext::structural_transit_with_mode(
            crate::semantic_query::ProjectionMode::Navigate,
        );
    let type_node = field
        .r#type
        .present()
        .and_then(|source| {
            dispatch.raise_semantic_type_source_to_hot(
                source,
                crate::project_semantic_dispatch::semantic_source::SourceRaiseContext {
                    scope_canonical_id: owner_canonical,
                    scope_owner: owner,
                    context: transit_ctx,
                    interior_failures: None,
                },
            )
        })
        .map(|hot| hot.node());

    // When the post-expansion source carries no actionable route (no
    // reference / utility / indexed-access head the registry can route on),
    // fall back to the analyzer-populated authored SHALLOW locator — the
    // bare annotation the user wrote. A deep indexed member path (len > 1)
    // recovers only when the resolved source is an explicit object surface.
    let shallow_node = (!type_node
        .is_some_and(|node| component_meta_registry_node_has_actionable_route(ctx, node)))
    .then(|| {
        field.shallow_source.as_ref().and_then(|locator| {
            dispatch
                .raise_authored_locator_to_hot(locator, transit_ctx)
                .map(|hot| hot.node())
        })
    })
    .flatten()
    .filter(|shallow| {
        let deep_indexed_path = component_meta_registry_node_indexed_access_route(ctx, *shallow)
            .is_some_and(|(_, route)| {
                matches!(
                    route,
                    RouteDemand::MemberPath(ref path) if path.len() > 1,
                )
            });
        !deep_indexed_path
            || type_node.is_some_and(|node| node_root_has_explicit_object_surface(ctx, node))
    });
    let Some(node) = shallow_node.or(type_node) else {
        return;
    };

    // Issue #7 / owner-local ComponentConfig alias rewrite.
    // When the indexed-access or utility route's root resolves to an
    // owner-local alias whose body is `ComponentConfig<...>`, emit a
    // `RouteDemand::Whole(root)` instead of `MemberPath`/`Pick`. The
    // registry materialises the alias once and reuses the result for
    // every later projection.
    //
    // External imports preserve `MemberPath`/`Pick` (the predicate
    // declines when the route root has an import binding).
    let route_root_name = component_meta_registry_node_utility_route(ctx, node)
        .or_else(|| component_meta_registry_node_indexed_access_route(ctx, node))
        .map(|(root, _)| root);
    if let Some(owner_local_root) = component_meta_registry_public_route_owner_local_root(
        ctx,
        owner_canonical,
        owner,
        snapshot,
        route_root_name.as_deref(),
        source_hint,
    ) {
        enqueue_component_meta_registry_ref(
            published_names,
            queued_names,
            output,
            owner_local_root.as_str(),
            source_hint,
            owner,
            None,
            RouteDemand::Whole,
        );
        return;
    }

    // Prepared-body classifications run NODE-DOMAIN off the declaration's
    // lowered body slot (never an embedded body).
    let prepared_body_root = |canonical_id: &str,
                              declaration_owner: verter_type_expr::TopLevelOwnerId,
                              symbol_name: &str| {
        ctx.prepared_type_decl_return_only(canonical_id, declaration_owner, symbol_name)
            .and_then(|prepared| prepared_body_root_node(ctx, prepared.as_ref()))
    };
    let skip_direct_plain_ref =
        component_meta_registry_node_ref_name(ctx, node).is_some_and(|name| {
            let name = name.as_str();
            prepared_body_root(owner_canonical, owner, name)
                .is_some_and(|root| node_root_is_type_parameter(ctx, root))
                || owner_component_meta_registry_import_root(
                    ctx,
                    owner_canonical,
                    owner,
                    snapshot,
                    name,
                )
                .and_then(|(canonical_id, target_owner, exported_name)| {
                    (!canonical_id.is_empty()
                        && !ctx.workspace_is_package_backed(canonical_id.as_str()))
                    .then(|| {
                        prepared_body_root(
                            canonical_id.as_str(),
                            target_owner,
                            exported_name.as_str(),
                        )
                    })
                })
                .flatten()
                .is_some_and(|root| {
                    node_root_has_non_object_top_level_surface(ctx, root)
                        && !node_root_has_explicit_object_surface(ctx, root)
                })
                || owner_component_meta_registry_import_root(
                    ctx,
                    owner_canonical,
                    owner,
                    snapshot,
                    name,
                )
                .is_some_and(|(canonical_id, _, _)| {
                    ctx.workspace_is_package_backed(canonical_id.as_str())
                })
                || ctx.workspace_is_package_backed(
                    ctx.resolve_type_declaration_for_dep(owner_canonical, owner, name)
                        .canonical_source
                        .as_str(),
                )
        });
    let direct_ref = component_meta_registry_node_ref_head(ctx, node);
    let skip_imported_generic_non_object_ref =
        direct_ref.as_ref().is_some_and(|(name, type_arguments)| {
            if type_arguments.is_empty() {
                return false;
            }
            let Some((canonical_id, target_owner, exported_name)) =
                owner_component_meta_registry_import_root(
                    ctx,
                    owner_canonical,
                    owner,
                    snapshot,
                    name.as_str(),
                )
            else {
                return false;
            };
            if canonical_id.is_empty() || ctx.workspace_is_package_backed(canonical_id.as_str()) {
                return false;
            }
            prepared_body_root(canonical_id.as_str(), target_owner, exported_name.as_str())
                .is_some_and(|root| {
                    node_root_has_non_object_top_level_surface(ctx, root)
                        && !node_root_has_explicit_object_surface(ctx, root)
                })
        });
    if (skip_direct_plain_ref || skip_imported_generic_non_object_ref)
        && direct_ref.as_ref().is_some_and(|(name, _)| {
            let name = name.as_str();
            let local_type_parameter = prepared_body_root(owner_canonical, owner, name)
                .is_some_and(|root| node_root_is_type_parameter(ctx, root));
            let import_root = owner_component_meta_registry_import_root(
                ctx,
                owner_canonical,
                owner,
                snapshot,
                name,
            );
            let package_backed = import_root.as_ref().is_some_and(|(canonical_id, _, _)| {
                ctx.workspace_is_package_backed(canonical_id.as_str())
            }) || ctx.workspace_is_package_backed(
                ctx.resolve_type_declaration_for_dep(owner_canonical, owner, name)
                    .canonical_source
                    .as_str(),
            );
            !local_type_parameter && !package_backed
        })
    {
        if let Some((name, _)) = direct_ref.as_ref() {
            enqueue_component_meta_registry_ref(
                published_names,
                queued_names,
                output,
                name.as_str(),
                source_hint,
                owner,
                None,
                RouteDemand::Whole,
            );
        }
    }
    if !skip_direct_plain_ref && !skip_imported_generic_non_object_ref {
        collect_component_meta_registry_public_surface_refs_node(
            ctx,
            node,
            published_names,
            queued_names,
            output,
            source_hint,
            owner,
        );
    }

    // Indexed-access roots whose owner-local prepared body is not a type
    // parameter enqueue their member-path route.
    if let Some((root_name, route)) = component_meta_registry_node_indexed_access_route(ctx, node) {
        if prepared_body_root(owner_canonical, owner, root_name.as_str())
            .is_some_and(|root| !node_root_is_type_parameter(ctx, root))
        {
            // A single-member route records the field's authored annotation
            // slot as the member USE-SITE: when the member's declaring
            // surface sits behind a generic substitution, the route-scoped
            // publication retains this slot as the member payload — replaying
            // it re-derives navigation + substitution.
            let use_site = match &route {
                RouteDemand::MemberPath(path) if path.len() == 1 => {
                    match field.shallow_source.as_ref() {
                        Some(verter_type_expr::locators::AuthoredBodyLocator::DeclBody(slot)) => {
                            Some((path[0].clone(), slot.clone()))
                        }
                        _ => None,
                    }
                }
                _ => None,
            };
            enqueue_component_meta_registry_ref(
                published_names,
                queued_names,
                output,
                root_name.as_str(),
                source_hint,
                owner,
                None,
                route,
            );
            if let Some((member, slot)) = use_site {
                attach_component_meta_registry_member_use_site(
                    output,
                    root_name.as_str(),
                    member.as_str(),
                    &slot,
                );
            }
        }
    } else if let Some((head_name, resolved_identity, route)) =
        component_meta_registry_node_instantiated_indexed_access_route(ctx, node)
    {
        // Instantiated-root indexed access (`LocalConfig<string>['slot']`):
        // enqueue under the AUTHORED owner-scope name — the resolved base
        // identity reverse-maps through the owner's import bindings to the
        // renamed local alias; an unresolved bare head already carries the
        // authored name.
        let local_name = resolved_identity
            .as_ref()
            .and_then(|(canonical, decl_name)| {
                snapshot.imports.iter().find_map(|import| {
                    let resolved = import.resolved_canonical_id.as_deref()?;
                    if resolved != canonical.as_ref() {
                        return None;
                    }
                    import.bindings.iter().find_map(|binding| {
                        let imported = binding.imported_name.as_deref().unwrap_or(&binding.name);
                        (imported == decl_name.as_ref()).then(|| binding.name.clone())
                    })
                })
            })
            .unwrap_or(head_name);
        let use_site = match &route {
            RouteDemand::MemberPath(path) if path.len() == 1 => {
                match field.shallow_source.as_ref() {
                    Some(verter_type_expr::locators::AuthoredBodyLocator::DeclBody(slot)) => {
                        Some((path[0].clone(), slot.clone()))
                    }
                    _ => None,
                }
            }
            _ => None,
        };
        enqueue_component_meta_registry_ref(
            published_names,
            queued_names,
            output,
            local_name.as_str(),
            source_hint,
            owner,
            None,
            route,
        );
        if let Some((member, slot)) = use_site {
            attach_component_meta_registry_member_use_site(
                output,
                local_name.as_str(),
                member.as_str(),
                &slot,
            );
        }
    }
}

pub(crate) fn component_meta_registry_string_literal_keys(
    expr: &verter_type_expr::TypeExpr,
) -> Option<Vec<String>> {
    use verter_type_expr::{LiteralValue, TypeExpr};

    match expr {
        TypeExpr::Literal(LiteralValue::String(value)) => Some(vec![value.clone()]),
        TypeExpr::Union(types) => {
            let mut keys = Vec::new();
            for ty in types.iter() {
                keys.extend(component_meta_registry_string_literal_keys(ty)?);
            }
            keys.sort();
            keys.dedup();
            Some(keys)
        }
        TypeExpr::Parenthesized(inner) => component_meta_registry_string_literal_keys(inner),
        _ => None,
    }
}

pub(crate) fn component_meta_registry_public_utility_route(
    expr: &verter_type_expr::TypeExpr,
) -> Option<(String, RouteDemand)> {
    use verter_type_expr::TypeExpr;

    match expr {
        TypeExpr::Parenthesized(inner) => component_meta_registry_public_utility_route(inner),
        TypeExpr::Ref {
            name,
            type_arguments,
        } if type_arguments.len() == 2 && matches!(name.as_ref(), "Pick" | "Omit") => {
            // Allow `Pick`/`Omit` over BOTH bare Refs (`Pick<Foo, K>`)
            // and INSTANTIATED generic Refs (`Pick<Foo<T>, K>`). The
            // route consumer surfaces the inner ref's declared
            // members under `symbol_name` — for bare and instantiated
            // forms alike, the published prop NAMES come from the
            // declaration body. Type-parameter substitutions affect
            // member VALUES (e.g. `items?: T` body shape), not member
            // names, and the route applies a name-set filter (Pick /
            // Omit). Allowing the inner Ref to carry `type_arguments`
            // closes the cross-package + instantiated-generic gap
            // (CP1) — pre-fix `component_meta_registry_ref_name`
            // required `type_arguments.is_empty()` and the route
            // extractor returned `None` for every utility wrap over
            // an instantiated generic, falling through to a path
            // that flattened the inner generic's body without
            // applying the Pick/Omit filter.
            let root_name =
                component_meta_registry_utility_inner_ref_name(&type_arguments[0])?.to_string();
            let members = component_meta_registry_string_literal_keys(&type_arguments[1])?;
            if members.is_empty() {
                return None;
            }
            let route = if name.as_ref() == "Pick" {
                RouteDemand::pick(members)
            } else {
                RouteDemand::omit(members)
            };
            Some((root_name, route))
        }
        _ => None,
    }
}

/// Extract the inner Ref's name for `Pick<X, K>` / `Omit<X, K>` route
/// extraction. Mirrors [`component_meta_registry_ref_name`] but
/// permits the Ref to carry `type_arguments` — the route consumer
/// dispatches the inner ref's declared members under `symbol_name`,
/// and the Pick/Omit filter operates on member NAMES (not member
/// types), so the type arguments do not affect route discriminability.
/// Used exclusively by the public utility route extractor (above).
/// Other callers of `component_meta_registry_ref_name` enforce the
/// `type_arguments.is_empty()` invariant for different reasons —
/// e.g. indexed-access chain roots that MUST be bare; those
/// constraints stay verbatim.
///
/// Iterative walk through `Parenthesized` shells — the loop is bounded
/// by the number of nested parentheses (a structural property of the
/// caller-supplied AST). No self-recursion, so the
/// `no_unbounded_recursion_in_resolver_core` architecture guard does
/// not need an entry for this helper.
fn component_meta_registry_utility_inner_ref_name(
    expr: &verter_type_expr::TypeExpr,
) -> Option<&str> {
    use verter_type_expr::TypeExpr;

    let mut cursor = expr;
    loop {
        match cursor {
            TypeExpr::Ref { name, .. } => return Some(name.as_ref()),
            TypeExpr::Parenthesized(inner) => cursor = inner.as_ref(),
            _ => return None,
        }
    }
}

pub(crate) fn component_meta_registry_public_indexed_access_route(
    expr: &verter_type_expr::TypeExpr,
) -> Option<(String, RouteDemand)> {
    use verter_type_expr::TypeExpr;

    fn collect_path(expr: &TypeExpr, path: &mut Vec<String>) -> Option<String> {
        use verter_type_expr::{LiteralValue, TypeExpr};

        match expr {
            TypeExpr::IndexedAccess { object, index } => {
                let root = collect_path(object, path)?;
                let TypeExpr::Literal(LiteralValue::String(member)) = index.as_ref() else {
                    return None;
                };
                path.push(member.clone());
                Some(root)
            }
            TypeExpr::Parenthesized(inner) => collect_path(inner, path),
            TypeExpr::Ref {
                name,
                type_arguments,
            } if type_arguments.is_empty() => Some(name.to_string()),
            _ => None,
        }
    }

    let mut path = Vec::new();
    let root = collect_path(expr, &mut path)?;
    if path.is_empty() {
        return None;
    }
    Some((root, RouteDemand::member_path(path)))
}

// + §6.10 sub-task 4 — both legacy
// member-path helpers retired. The shared object-member navigation
// logic is inlined into the body of
// `component_meta_registry_raw_member_path_surface` (its only
// surviving caller). The retired symbols are listed in the
// `RETIRED_SYMBOLS` array of the static-grep gate test.

// ===========================================================================
// Node-domain discovery: the registry BFS over raised semantic-graph nodes
// ===========================================================================
//
// The registry publishes content-free SOURCES (authored body locators /
// closed leaf facts) and discovers transitive references by walking the
// semantic-graph NODES those sources raise to through the one shared
// dispatch. These are the node-domain siblings of the `TypeExpr` route
// extractors above: route DISCOVERY (which names enqueue, with which
// `RouteDemand`) stays path-precise, while publication stays a shallow
// content-free source the consumer re-raises on demand.
//
// Node walks carry a `visited` set: graph nodes are interned and may be
// shared or cyclic (recursive types), unlike the tree-shaped `TypeExpr`
// inputs of the sibling walkers.

use crate::semantic_query::{IndexKey, SemanticNodeData, SemanticNodeId};

fn registry_node_data(
    ctx: &dyn ResolverContext,
    node: SemanticNodeId,
) -> Option<Arc<SemanticNodeData>> {
    crate::project_semantic_dispatch::node_data_for(ctx, node)
}

/// Unwrap ONE `Alias` hop (the node-domain analog of stripping a
/// `Parenthesized` wrapper), mirroring the root-kind classifier convention
/// in `meta_resolve::exactness::node_root_should_stay_symbolic`.
fn registry_unalias(ctx: &dyn ResolverContext, node: SemanticNodeId) -> SemanticNodeId {
    match registry_node_data(ctx, node).as_deref() {
        Some(SemanticNodeData::Alias(target)) => *target,
        _ => node,
    }
}

/// The node's reference HEAD: `(name, type-argument nodes)` for the three
/// reference carriers (`BareRef` / `InstantiationRef` / `DeclRef`).
pub(crate) fn component_meta_registry_node_ref_head(
    ctx: &dyn ResolverContext,
    node: SemanticNodeId,
) -> Option<(String, Vec<SemanticNodeId>)> {
    let data = registry_node_data(ctx, registry_unalias(ctx, node))?;
    if let Some((name, _scope)) = data.bare_ref_head() {
        return Some((name.to_string(), data.carrier_type_args().to_vec()));
    }
    match data.as_ref() {
        SemanticNodeData::DeclRef { identity } => {
            Some((identity.decl_name.to_string(), Vec::new()))
        }
        SemanticNodeData::InstantiationRef { base, args } => {
            Some((base.decl_name.to_string(), args.to_vec()))
        }
        _ => None,
    }
}

/// The node-domain sibling of [`component_meta_registry_ref_name`]: the
/// bare (argument-free) reference head name.
pub(crate) fn component_meta_registry_node_ref_name(
    ctx: &dyn ResolverContext,
    node: SemanticNodeId,
) -> Option<String> {
    component_meta_registry_node_ref_head(ctx, node)
        .filter(|(_, args)| args.is_empty())
        .map(|(name, _)| name)
}

/// The node-domain sibling of [`component_meta_registry_string_literal_keys`].
pub(crate) fn component_meta_registry_node_string_literal_keys(
    ctx: &dyn ResolverContext,
    node: SemanticNodeId,
) -> Option<Vec<String>> {
    fn keys_of(
        ctx: &dyn ResolverContext,
        node: SemanticNodeId,
        out: &mut Vec<String>,
    ) -> Option<()> {
        match registry_node_data(ctx, registry_unalias(ctx, node)).as_deref() {
            Some(SemanticNodeData::Literal(verter_type_expr::LiteralValue::String(value))) => {
                out.push(value.clone());
                Some(())
            }
            Some(SemanticNodeData::Union(arms)) => {
                for arm in arms.iter() {
                    keys_of(ctx, *arm, out)?;
                }
                Some(())
            }
            _ => None,
        }
    }
    let mut keys = Vec::new();
    keys_of(ctx, node, &mut keys)?;
    keys.sort();
    keys.dedup();
    Some(keys)
}

/// The node-domain sibling of [`component_meta_registry_public_utility_route`]:
/// a `Pick<Inner, K>` / `Omit<Inner, K>` reference head whose key argument is
/// a string-literal set routes as `Pick`/`Omit` on the inner reference name.
pub(crate) fn component_meta_registry_node_utility_route(
    ctx: &dyn ResolverContext,
    node: SemanticNodeId,
) -> Option<(String, RouteDemand)> {
    let (name, args) = component_meta_registry_node_ref_head(ctx, node)?;
    if args.len() != 2 || !matches!(name.as_str(), "Pick" | "Omit") {
        return None;
    }
    // The inner reference may carry type arguments (`Pick<Foo<T>, K>`) —
    // the route filter operates on member NAMES, matching the `TypeExpr`
    // sibling's contract.
    let (root_name, _) = component_meta_registry_node_ref_head(ctx, args[0])?;
    let members = component_meta_registry_node_string_literal_keys(ctx, args[1])?;
    if members.is_empty() {
        return None;
    }
    let route = if name.as_str() == "Pick" {
        RouteDemand::pick(members)
    } else {
        RouteDemand::omit(members)
    };
    Some((root_name, route))
}

/// The node-domain sibling of
/// [`component_meta_registry_public_indexed_access_route`]: an
/// `IndexedAccess` spine of string keys rooted at a bare reference routes as
/// a `MemberPath`.
pub(crate) fn component_meta_registry_node_indexed_access_route(
    ctx: &dyn ResolverContext,
    node: SemanticNodeId,
) -> Option<(String, RouteDemand)> {
    let (path_rev, cursor) = indexed_access_string_key_spine(ctx, node)?;
    let root = component_meta_registry_node_ref_name(ctx, cursor)?;
    Some((root, member_path_from_rev(path_rev)))
}

/// Walk an `IndexedAccess` spine of STRING keys from `node` to its root:
/// `Some((keys outer-to-inner, root cursor))` for a non-empty spine whose
/// every index is a string literal; `None` otherwise.
fn indexed_access_string_key_spine(
    ctx: &dyn ResolverContext,
    node: SemanticNodeId,
) -> Option<(Vec<String>, SemanticNodeId)> {
    let mut path_rev: Vec<String> = Vec::new();
    let mut cursor = registry_unalias(ctx, node);
    while let Some(SemanticNodeData::IndexedAccess { object, index }) =
        registry_node_data(ctx, cursor).as_deref()
    {
        let IndexKey::String(member) = index else {
            return None;
        };
        path_rev.push(member.to_string());
        cursor = registry_unalias(ctx, *object);
    }
    (!path_rev.is_empty()).then_some((path_rev, cursor))
}

/// Reverse the outer-to-inner spine keys into the inner-to-outer
/// `MemberPath` route.
fn member_path_from_rev(mut path_rev: Vec<String>) -> RouteDemand {
    path_rev.reverse();
    RouteDemand::member_path(path_rev)
}

/// Instantiated-root sibling of
/// [`component_meta_registry_node_indexed_access_route`]: an indexed-access
/// spine whose root is a GENERIC APPLICATION (`LocalConfig<string>['slot']`)
/// rather than a bare reference. Returns the root head name (the authored
/// name for an unresolved bare head, the resolved declaration name for an
/// `InstantiationRef`), the resolved base identity when available
/// (`(canonical, decl_name)` — the discovery site reverse-maps it through the
/// owner's import bindings to recover the authored local alias), and the
/// member-path route.
pub(crate) fn component_meta_registry_node_instantiated_indexed_access_route(
    ctx: &dyn ResolverContext,
    node: SemanticNodeId,
) -> Option<InstantiatedIndexedAccessRoute> {
    let (path_rev, cursor) = indexed_access_string_key_spine(ctx, node)?;
    let (name, args) = component_meta_registry_node_ref_head(ctx, cursor)?;
    if args.is_empty() {
        // A bare root belongs to the plain extractor above.
        return None;
    }
    let resolved_identity = match registry_node_data(ctx, cursor).as_deref() {
        Some(SemanticNodeData::InstantiationRef { base, .. }) => Some((
            std::sync::Arc::clone(&base.canonical_id),
            std::sync::Arc::clone(&base.decl_name),
        )),
        _ => None,
    };
    Some((name, resolved_identity, member_path_from_rev(path_rev)))
}

/// The instantiated-root route extraction result: the root head name, the
/// resolved base identity when available (`(canonical, decl_name)`), and the
/// member-path route.
pub(crate) type InstantiatedIndexedAccessRoute = (
    String,
    Option<(std::sync::Arc<str>, std::sync::Arc<str>)>,
    RouteDemand,
);

/// The node-domain sibling of
/// [`component_meta_registry_field_expr_has_actionable_route`].
pub(crate) fn component_meta_registry_node_has_actionable_route(
    ctx: &dyn ResolverContext,
    node: SemanticNodeId,
) -> bool {
    component_meta_registry_node_ref_head(ctx, node).is_some()
        || component_meta_registry_node_utility_route(ctx, node).is_some()
        || component_meta_registry_node_indexed_access_route(ctx, node).is_some()
}

/// The node-domain sibling of
/// [`collect_component_meta_registry_public_surface_refs`]: a top-level
/// reference head enqueues a `Whole` route; every other root enqueues
/// nothing.
pub(crate) fn collect_component_meta_registry_public_surface_refs_node(
    ctx: &dyn ResolverContext,
    node: SemanticNodeId,
    published_names: &rustc_hash::FxHashSet<String>,
    queued_names: &mut rustc_hash::FxHashSet<String>,
    output: &mut VecDeque<PendingComponentMetaRegistryRef>,
    source_hint: Option<&str>,
    source_owner: verter_type_expr::TopLevelOwnerId,
) {
    if let Some((name, _)) = component_meta_registry_node_ref_head(ctx, node) {
        enqueue_component_meta_registry_ref(
            published_names,
            queued_names,
            output,
            name.as_str(),
            source_hint,
            source_owner,
            None,
            RouteDemand::Whole,
        );
    }
}

/// Eligibility policy for enqueuing PLAIN named refs (bare `Ref { name }`
/// heads with no utility / indexed-access route) discovered on member paths
/// of a registry seed's raised surface.
///
/// A plain named ref reached on an ACTIVELY DEMANDED member path of an
/// owner-local seed is a registry dependency, regardless of transparent
/// wrappers (`Section<T>` and `Section<T>[]` have IDENTICAL discovery
/// eligibility — an array is not a publication boundary). Transparent
/// composites (arrays / tuples / unions / intersections / aliases / mapped
/// and conditional operands) thread the policy UNCHANGED; only genuine
/// publication boundaries (function parameter / return positions) force
/// [`Self::PublicationBoundary`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RegistryMemberRefPolicy {
    /// The walk is inside the actively demanded member surface of an
    /// owner-local seed (or a route-root operand position): a plain named
    /// ref head IS a registry dependency — enqueue the carrier and stop.
    DemandedOwnerLocalSurface,
    /// Publication boundary: a plain named ref head stays inline in the
    /// published surface (the consumer re-resolves it on demand); only
    /// utility / indexed-access routes enqueue.
    PublicationBoundary,
}

impl RegistryMemberRefPolicy {
    /// Whether a plain named ref head on the current path enqueues as a
    /// registry dependency.
    pub(crate) fn allows_plain_member_refs(self) -> bool {
        matches!(self, Self::DemandedOwnerLocalSurface)
    }
}

/// Whether an intersection's arms form the lone-`extends` heritage shape:
/// `DeclRef` heritage arms (bare, no type arguments through alias peeling)
/// plus exactly ONE own-member `Object` arm. This is the shape the
/// heritage-merged surface observation composes into ONE published surface,
/// absorbing the heritage bases.
fn intersection_is_lone_extends_heritage(
    ctx: &dyn ResolverContext,
    arms: &[SemanticNodeId],
) -> bool {
    let mut decl_ref_arms = 0usize;
    let mut object_arms = 0usize;
    for arm in arms {
        let mut node = *arm;
        loop {
            match registry_node_data(ctx, node).as_deref() {
                Some(SemanticNodeData::Alias(inner)) => node = *inner,
                Some(SemanticNodeData::DeclRef { .. }) => {
                    decl_ref_arms += 1;
                    break;
                }
                Some(SemanticNodeData::Object(_)) => {
                    object_arms += 1;
                    break;
                }
                _ => return false,
            }
        }
    }
    decl_ref_arms >= 1 && object_arms == 1
}

/// The node-domain sibling of [`collect_component_meta_registry_refs`]:
/// walk a raised registry-entry body node and enqueue its transitive
/// references, path-precise under `cursor`.
pub(crate) fn collect_component_meta_registry_refs_node(
    ctx: &dyn ResolverContext,
    node: SemanticNodeId,
    published_names: &rustc_hash::FxHashSet<String>,
    queued_names: &mut rustc_hash::FxHashSet<String>,
    output: &mut VecDeque<PendingComponentMetaRegistryRef>,
    source_hint: Option<&str>,
    source_owner: verter_type_expr::TopLevelOwnerId,
    member_ref_policy: RegistryMemberRefPolicy,
    cursor: crate::meta_resolve::projection_demand::ProjectionCursor<'_>,
) {
    let mut visited: rustc_hash::FxHashSet<SemanticNodeId> = rustc_hash::FxHashSet::default();
    collect_registry_refs_node_inner(
        ctx,
        node,
        published_names,
        queued_names,
        output,
        source_hint,
        source_owner,
        member_ref_policy,
        cursor,
        &mut visited,
    );
}

#[allow(clippy::too_many_arguments)]
fn collect_registry_refs_node_inner(
    ctx: &dyn ResolverContext,
    node: SemanticNodeId,
    published_names: &rustc_hash::FxHashSet<String>,
    queued_names: &mut rustc_hash::FxHashSet<String>,
    output: &mut VecDeque<PendingComponentMetaRegistryRef>,
    source_hint: Option<&str>,
    source_owner: verter_type_expr::TopLevelOwnerId,
    member_ref_policy: RegistryMemberRefPolicy,
    cursor: crate::meta_resolve::projection_demand::ProjectionCursor<'_>,
    visited: &mut rustc_hash::FxHashSet<SemanticNodeId>,
) {
    if !visited.insert(node) {
        return;
    }
    if let Some((root_name, route)) = component_meta_registry_node_utility_route(ctx, node)
        .or_else(|| component_meta_registry_node_indexed_access_route(ctx, node))
    {
        enqueue_component_meta_registry_ref(
            published_names,
            queued_names,
            output,
            root_name.as_str(),
            source_hint,
            source_owner,
            None,
            route,
        );
        return;
    }
    if let Some((name, _)) = component_meta_registry_node_ref_head(ctx, node) {
        enqueue_component_meta_registry_ref(
            published_names,
            queued_names,
            output,
            name.as_str(),
            source_hint,
            source_owner,
            None,
            RouteDemand::Whole,
        );
        return;
    }
    let Some(data) = registry_node_data(ctx, node) else {
        return;
    };
    match data.as_ref() {
        SemanticNodeData::Alias(target) => {
            collect_registry_refs_node_inner(
                ctx,
                *target,
                published_names,
                queued_names,
                output,
                source_hint,
                source_owner,
                member_ref_policy,
                cursor,
                visited,
            );
        }
        SemanticNodeData::Array { element, .. } | SemanticNodeData::KeyOf { base: element } => {
            collect_registry_refs_node_inner(
                ctx,
                *element,
                published_names,
                queued_names,
                output,
                source_hint,
                source_owner,
                member_ref_policy,
                cursor,
                visited,
            );
        }
        SemanticNodeData::Tuple { elements, .. } => {
            for element in elements.iter() {
                collect_registry_refs_node_inner(
                    ctx,
                    element.value,
                    published_names,
                    queued_names,
                    output,
                    source_hint,
                    source_owner,
                    member_ref_policy,
                    cursor,
                    visited,
                );
            }
        }
        SemanticNodeData::Union(arms) => {
            if !member_ref_policy.allows_plain_member_refs() {
                return;
            }
            for arm in arms.iter() {
                collect_registry_refs_node_inner(
                    ctx,
                    *arm,
                    published_names,
                    queued_names,
                    output,
                    source_hint,
                    source_owner,
                    member_ref_policy,
                    cursor,
                    visited,
                );
            }
        }
        SemanticNodeData::Intersection(arms) => {
            if !member_ref_policy.allows_plain_member_refs() {
                return;
            }
            // A lone-`extends` heritage intersection (`DeclRef` heritage arms
            // plus exactly ONE own-member `Object` arm) is ABSORBED into the
            // derived entry's published MERGED surface (heritage composed at
            // surface observation) — the heritage bases are NOT whole registry
            // dependencies of their own. Their members' routes (indexed
            // access / utilities) are discovered from the merged surface at
            // publication, so only route-scoped demands publish the base.
            let heritage_shape = intersection_is_lone_extends_heritage(ctx, arms);
            for arm in arms.iter() {
                if heritage_shape
                    && component_meta_registry_node_ref_head(ctx, *arm)
                        .is_some_and(|(_, args)| args.is_empty())
                {
                    continue;
                }
                collect_registry_refs_node_inner(
                    ctx,
                    *arm,
                    published_names,
                    queued_names,
                    output,
                    source_hint,
                    source_owner,
                    member_ref_policy,
                    cursor,
                    visited,
                );
            }
        }
        SemanticNodeData::TemplateLiteral { expressions, .. } => {
            if !member_ref_policy.allows_plain_member_refs() {
                return;
            }
            for expr in expressions.iter() {
                collect_registry_refs_node_inner(
                    ctx,
                    *expr,
                    published_names,
                    queued_names,
                    output,
                    source_hint,
                    source_owner,
                    member_ref_policy,
                    cursor,
                    visited,
                );
            }
        }
        // Registry publication stays shallow: object member VALUES route
        // through the member-surface walker under the same G2 path-precision
        // cursor gates as the `TypeExpr` sibling.
        SemanticNodeData::Object(surface) => {
            for member in surface.members.iter() {
                if !cursor.admits_key(member.name.as_ref()) {
                    continue;
                }
                collect_registry_member_surface_refs_node(
                    ctx,
                    member.value,
                    published_names,
                    queued_names,
                    output,
                    source_hint,
                    source_owner,
                    member_ref_policy,
                    visited,
                );
            }
            if cursor.is_whole_surface() {
                for signature in surface
                    .call_signatures
                    .iter()
                    .chain(surface.construct_signatures.iter())
                {
                    collect_registry_member_surface_refs_node(
                        ctx,
                        *signature,
                        published_names,
                        queued_names,
                        output,
                        source_hint,
                        source_owner,
                        member_ref_policy,
                        visited,
                    );
                }
                for signature in surface.index_signatures.iter() {
                    collect_registry_member_surface_refs_node(
                        ctx,
                        signature.key_type,
                        published_names,
                        queued_names,
                        output,
                        source_hint,
                        source_owner,
                        member_ref_policy,
                        visited,
                    );
                    collect_registry_member_surface_refs_node(
                        ctx,
                        signature.value_type,
                        published_names,
                        queued_names,
                        output,
                        source_hint,
                        source_owner,
                        member_ref_policy,
                        visited,
                    );
                }
            }
        }
        SemanticNodeData::Function {
            params,
            return_type,
            ..
        } => {
            for param in params.iter() {
                collect_registry_member_surface_refs_node(
                    ctx,
                    param.ty,
                    published_names,
                    queued_names,
                    output,
                    source_hint,
                    source_owner,
                    RegistryMemberRefPolicy::PublicationBoundary,
                    visited,
                );
            }
            collect_registry_member_surface_refs_node(
                ctx,
                *return_type,
                published_names,
                queued_names,
                output,
                source_hint,
                source_owner,
                RegistryMemberRefPolicy::PublicationBoundary,
                visited,
            );
        }
        SemanticNodeData::ConstructorType { signature } => {
            collect_registry_refs_node_inner(
                ctx,
                *signature,
                published_names,
                queued_names,
                output,
                source_hint,
                source_owner,
                member_ref_policy,
                cursor,
                visited,
            );
        }
        SemanticNodeData::IndexedAccess { object, index } => {
            collect_registry_refs_node_inner(
                ctx,
                *object,
                published_names,
                queued_names,
                output,
                source_hint,
                source_owner,
                member_ref_policy,
                cursor,
                visited,
            );
            if let IndexKey::TypeNode(index_node) = index {
                collect_registry_refs_node_inner(
                    ctx,
                    *index_node,
                    published_names,
                    queued_names,
                    output,
                    source_hint,
                    source_owner,
                    member_ref_policy,
                    cursor,
                    visited,
                );
            }
        }
        _ => {}
    }
}

/// The node-domain sibling of
/// [`collect_component_meta_registry_member_surface_refs`].
#[allow(clippy::too_many_arguments)]
fn collect_registry_member_surface_refs_node(
    ctx: &dyn ResolverContext,
    node: SemanticNodeId,
    published_names: &rustc_hash::FxHashSet<String>,
    queued_names: &mut rustc_hash::FxHashSet<String>,
    output: &mut VecDeque<PendingComponentMetaRegistryRef>,
    source_hint: Option<&str>,
    source_owner: verter_type_expr::TopLevelOwnerId,
    member_ref_policy: RegistryMemberRefPolicy,
    visited: &mut rustc_hash::FxHashSet<SemanticNodeId>,
) {
    if !visited.insert(node) {
        return;
    }
    if let Some((root_name, route)) = component_meta_registry_node_utility_route(ctx, node)
        .or_else(|| component_meta_registry_node_indexed_access_route(ctx, node))
    {
        enqueue_component_meta_registry_ref(
            published_names,
            queued_names,
            output,
            root_name.as_str(),
            source_hint,
            source_owner,
            None,
            route,
        );
        return;
    }
    if component_meta_registry_node_instantiated_indexed_access_route(ctx, node).is_some() {
        // Instantiated-root indexed access: the authored-name discovery
        // (with the owner's import-binding reverse map) happens at the
        // public-field collector, which has the owner snapshot. Recursing
        // into the object here would misclassify the RESOLVED base name as
        // a whole registry dependency of the owner.
        return;
    }
    if let Some((name, _)) = component_meta_registry_node_ref_head(ctx, node) {
        if member_ref_policy.allows_plain_member_refs() {
            enqueue_component_meta_registry_ref(
                published_names,
                queued_names,
                output,
                name.as_str(),
                source_hint,
                source_owner,
                None,
                RouteDemand::Whole,
            );
        }
        return;
    }
    let Some(data) = registry_node_data(ctx, node) else {
        return;
    };
    let recurse = |ctx: &dyn ResolverContext,
                   child: SemanticNodeId,
                   queued_names: &mut rustc_hash::FxHashSet<String>,
                   output: &mut VecDeque<PendingComponentMetaRegistryRef>,
                   policy: RegistryMemberRefPolicy,
                   visited: &mut rustc_hash::FxHashSet<SemanticNodeId>| {
        collect_registry_member_surface_refs_node(
            ctx,
            child,
            published_names,
            queued_names,
            output,
            source_hint,
            source_owner,
            policy,
            visited,
        );
    };
    match data.as_ref() {
        SemanticNodeData::Alias(target) => {
            recurse(
                ctx,
                *target,
                queued_names,
                output,
                member_ref_policy,
                visited,
            );
        }
        SemanticNodeData::IndexedAccess { object, index } => {
            // Route-root operand positions: a plain ref head here IS the
            // root of an actively demanded projection, so it is always
            // dependency-eligible regardless of the inherited policy.
            recurse(
                ctx,
                *object,
                queued_names,
                output,
                RegistryMemberRefPolicy::DemandedOwnerLocalSurface,
                visited,
            );
            if let IndexKey::TypeNode(index_node) = index {
                recurse(
                    ctx,
                    *index_node,
                    queued_names,
                    output,
                    RegistryMemberRefPolicy::DemandedOwnerLocalSurface,
                    visited,
                );
            }
        }
        SemanticNodeData::Array { element, .. } | SemanticNodeData::KeyOf { base: element } => {
            recurse(
                ctx,
                *element,
                queued_names,
                output,
                member_ref_policy,
                visited,
            );
        }
        SemanticNodeData::Tuple { elements, .. } => {
            for element in elements.iter() {
                recurse(
                    ctx,
                    element.value,
                    queued_names,
                    output,
                    member_ref_policy,
                    visited,
                );
            }
        }
        SemanticNodeData::Union(arms) | SemanticNodeData::Intersection(arms) => {
            for arm in arms.iter() {
                recurse(ctx, *arm, queued_names, output, member_ref_policy, visited);
            }
        }
        SemanticNodeData::TemplateLiteral { expressions, .. } => {
            for expr in expressions.iter() {
                recurse(ctx, *expr, queued_names, output, member_ref_policy, visited);
            }
        }
        SemanticNodeData::Conditional {
            check,
            extends,
            true_branch_ref,
            false_branch_ref,
            ..
        } => {
            recurse(
                ctx,
                *check,
                queued_names,
                output,
                member_ref_policy,
                visited,
            );
            recurse(
                ctx,
                *extends,
                queued_names,
                output,
                member_ref_policy,
                visited,
            );
            recurse(
                ctx,
                *true_branch_ref,
                queued_names,
                output,
                member_ref_policy,
                visited,
            );
            recurse(
                ctx,
                *false_branch_ref,
                queued_names,
                output,
                member_ref_policy,
                visited,
            );
        }
        SemanticNodeData::Mapped { source, .. } => {
            recurse(
                ctx,
                *source,
                queued_names,
                output,
                member_ref_policy,
                visited,
            );
        }
        SemanticNodeData::Function {
            params,
            return_type,
            ..
        } => {
            for param in params.iter() {
                recurse(
                    ctx,
                    param.ty,
                    queued_names,
                    output,
                    RegistryMemberRefPolicy::PublicationBoundary,
                    visited,
                );
            }
            recurse(
                ctx,
                *return_type,
                queued_names,
                output,
                RegistryMemberRefPolicy::PublicationBoundary,
                visited,
            );
        }
        SemanticNodeData::ConstructorType { signature } => {
            recurse(
                ctx,
                *signature,
                queued_names,
                output,
                member_ref_policy,
                visited,
            );
        }
        SemanticNodeData::Object(surface) => {
            for member in surface.members.iter() {
                recurse(
                    ctx,
                    member.value,
                    queued_names,
                    output,
                    member_ref_policy,
                    visited,
                );
            }
            for signature in surface
                .call_signatures
                .iter()
                .chain(surface.construct_signatures.iter())
            {
                recurse(
                    ctx,
                    *signature,
                    queued_names,
                    output,
                    member_ref_policy,
                    visited,
                );
            }
            for signature in surface.index_signatures.iter() {
                recurse(
                    ctx,
                    signature.key_type,
                    queued_names,
                    output,
                    member_ref_policy,
                    visited,
                );
                recurse(
                    ctx,
                    signature.value_type,
                    queued_names,
                    output,
                    member_ref_policy,
                    visited,
                );
            }
        }
        _ => {}
    }
}

/// Collect every reference-head NAME reachable from `node` (bounded,
/// visited-guarded). Used for seeded-dependency-name accounting over
/// raised registry sources in the host registry BFS.
pub(crate) fn collect_node_ref_names(
    ctx: &dyn ResolverContext,
    node: SemanticNodeId,
    names: &mut rustc_hash::FxHashSet<String>,
) {
    let mut visited: rustc_hash::FxHashSet<SemanticNodeId> = rustc_hash::FxHashSet::default();
    let mut worklist: Vec<SemanticNodeId> = vec![node];
    while let Some(node) = worklist.pop() {
        if !visited.insert(node) {
            continue;
        }
        let Some(data) = registry_node_data(ctx, node) else {
            continue;
        };
        if let Some((name, args)) = component_meta_registry_node_ref_head(ctx, node) {
            names.insert(name);
            worklist.extend(args);
            continue;
        }
        match data.as_ref() {
            SemanticNodeData::Alias(target) => worklist.push(*target),
            SemanticNodeData::Array { element, .. } | SemanticNodeData::KeyOf { base: element } => {
                worklist.push(*element)
            }
            SemanticNodeData::Tuple { elements, .. } => {
                worklist.extend(elements.iter().map(|element| element.value));
            }
            SemanticNodeData::Union(arms) | SemanticNodeData::Intersection(arms) => {
                worklist.extend(arms.iter().copied());
            }
            SemanticNodeData::TemplateLiteral { expressions, .. } => {
                worklist.extend(expressions.iter().copied());
            }
            SemanticNodeData::Object(surface) => {
                worklist.extend(surface.members.iter().map(|member| member.value));
                worklist.extend(surface.call_signatures.iter().copied());
                worklist.extend(surface.construct_signatures.iter().copied());
                for signature in surface.index_signatures.iter() {
                    worklist.push(signature.key_type);
                    worklist.push(signature.value_type);
                }
            }
            SemanticNodeData::Function {
                params,
                return_type,
                ..
            } => {
                worklist.extend(params.iter().map(|param| param.ty));
                worklist.push(*return_type);
            }
            SemanticNodeData::ConstructorType { signature } => worklist.push(*signature),
            SemanticNodeData::IndexedAccess { object, index } => {
                worklist.push(*object);
                if let IndexKey::TypeNode(index_node) = index {
                    worklist.push(*index_node);
                }
            }
            SemanticNodeData::Conditional {
                check,
                extends,
                true_branch_ref,
                false_branch_ref,
                ..
            } => {
                worklist.push(*check);
                worklist.push(*extends);
                worklist.push(*true_branch_ref);
                worklist.push(*false_branch_ref);
            }
            SemanticNodeData::Mapped { source, .. } => worklist.push(*source),
            SemanticNodeData::MergedDecl { contributors } => {
                worklist.extend(contributors.iter().copied());
            }
            _ => {}
        }
    }
}

/// Fact-domain classification of a published registry SOURCE as an explicit
/// object surface: a closed / synthesized / projected object-shape fact IS
/// one; a leaf, function, tuple, indexed-access, or authored-locator source
/// is NOT (an authored body's shape is only knowable by lowering, and every
/// consumer of this predicate treats "unknown" as "not an explicit object").
pub(crate) fn source_has_explicit_object_surface_fact(
    source: &verter_type_expr::facts::SemanticTypeSource,
) -> bool {
    use verter_type_expr::facts::{
        ClosedTypeFact, ProjectedTypeFact, ResolvedLocalShape, SemanticTypeSource,
    };
    matches!(
        source,
        SemanticTypeSource::Closed(ClosedTypeFact::Object(_))
            | SemanticTypeSource::Synthesized(ResolvedLocalShape::Object(_))
            | SemanticTypeSource::Projected(ProjectedTypeFact::Surface(_))
    )
}

/// Source-domain sibling of [`component_meta_registry_ref_name`]: the bare
/// named-reference LEAF a shallow registry seed publishes
/// (`Closed(Leaf(Ref(name)))`), by fact identity — no lowering.
pub(crate) fn source_bare_ref_name(
    source: &verter_type_expr::facts::SemanticTypeSource,
) -> Option<&str> {
    use verter_type_expr::facts::{ClosedTypeFact, LeafTypeFact, SemanticTypeSource};
    match source {
        SemanticTypeSource::Closed(ClosedTypeFact::Leaf(LeafTypeFact::Ref(name))) => {
            Some(name.as_str())
        }
        _ => None,
    }
}

/// Lower a prepared declaration's content-free body slot through the ONE
/// shared dispatch and return the raised body-root node. `None` =
/// unraisable under the live view (unloaded / evicted producing canonical).
pub(crate) fn prepared_body_root_node(
    ctx: &dyn ResolverContext,
    prepared: &verter_semantic::analysis::type_solver::PreparedTypeDecl,
) -> Option<SemanticNodeId> {
    let dispatch = crate::project_semantic_dispatch::ProjectSemanticDispatch::new(ctx);
    dispatch
        .raise_authored_locator_to_hot(
            &verter_type_expr::locators::AuthoredBodyLocator::DeclBody(
                prepared.body_facts.body_slot.clone(),
            ),
            crate::semantic_query::ProjectionReductionContext::structural_transit_with_mode(
                crate::semantic_query::ProjectionMode::Navigate,
            ),
        )
        .map(|hot| hot.node())
}

/// Node-domain sibling of the `matches!(body, TypeExpr::TypeParameter(_))`
/// prepared-body classification: the raised body ROOT is a type parameter.
pub(crate) fn node_root_is_type_parameter(ctx: &dyn ResolverContext, node: SemanticNodeId) -> bool {
    matches!(
        registry_node_data(ctx, registry_unalias(ctx, node)).as_deref(),
        Some(SemanticNodeData::TypeParam { .. })
    )
}

/// Node-domain sibling of
/// [`component_meta_registry_has_non_object_top_level_surface`]: the raised
/// root carries a non-object top-level surface (a reference / indexed-access
/// / conditional / mapped root, or a union / intersection with a non-object
/// arm).
pub(crate) fn node_root_has_non_object_top_level_surface(
    ctx: &dyn ResolverContext,
    node: SemanticNodeId,
) -> bool {
    let node = registry_unalias(ctx, node);
    let Some(data) = registry_node_data(ctx, node) else {
        return false;
    };
    if component_meta_registry_node_ref_head(ctx, node).is_some() {
        return true;
    }
    match data.as_ref() {
        SemanticNodeData::Union(arms) | SemanticNodeData::Intersection(arms) => {
            arms.iter()
                .any(|arm| node_root_has_non_object_top_level_surface(ctx, *arm))
                || arms.iter().any(|arm| {
                    !matches!(
                        registry_node_data(ctx, registry_unalias(ctx, *arm)).as_deref(),
                        Some(SemanticNodeData::Object(_))
                    )
                })
        }
        SemanticNodeData::IndexedAccess { .. }
        | SemanticNodeData::Conditional { .. }
        | SemanticNodeData::Mapped { .. } => true,
        _ => false,
    }
}

/// Node-domain sibling of
/// [`component_meta_registry_has_explicit_object_surface`]: the raised root
/// is an object surface, or a union / intersection carrying one.
pub(crate) fn node_root_has_explicit_object_surface(
    ctx: &dyn ResolverContext,
    node: SemanticNodeId,
) -> bool {
    let node = registry_unalias(ctx, node);
    match registry_node_data(ctx, node).as_deref() {
        Some(SemanticNodeData::Object(_)) => true,
        Some(SemanticNodeData::Union(arms)) | Some(SemanticNodeData::Intersection(arms)) => arms
            .iter()
            .any(|arm| node_root_has_explicit_object_surface(ctx, *arm)),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::sync::Arc;

    use super::{
        component_meta_registry_public_indexed_access_route, enqueue_component_meta_registry_ref,
        owner_component_meta_registry_import_root, RouteDemand,
    };
    use crate::types::{AnalysisLevel, DependencyResolution, HostConfig};
    use crate::VerterHost;
    #[test]
    fn indexed_access_route_preserves_full_member_path() {
        let expr = verter_semantic::analysis::jsdoc::parse_jsdoc_tag_type_payload(
            "Button['variants']['color']",
            None,
        );

        assert_eq!(
            component_meta_registry_public_indexed_access_route(&expr),
            Some((
                "Button".to_string(),
                RouteDemand::member_path(vec!["variants".to_string(), "color".to_string()]),
            ))
        );
    }

    #[test]
    fn enqueue_registry_ref_keeps_deep_member_paths_separate_from_top_level_picks() {
        let published_names = rustc_hash::FxHashSet::default();
        let mut queued_names = rustc_hash::FxHashSet::default();
        let mut output = VecDeque::new();

        enqueue_component_meta_registry_ref(
            &published_names,
            &mut queued_names,
            &mut output,
            "Button",
            Some("/src/Button.vue"),
            verter_type_expr::TopLevelOwnerId::instance(0),
            None,
            RouteDemand::pick(vec!["slots".to_string()]),
        );
        enqueue_component_meta_registry_ref(
            &published_names,
            &mut queued_names,
            &mut output,
            "Button",
            Some("/src/Button.vue"),
            verter_type_expr::TopLevelOwnerId::instance(0),
            None,
            RouteDemand::member_path(vec!["variants".to_string(), "color".to_string()]),
        );

        assert_eq!(
            output.len(),
            2,
            "deep member-path requests must not be collapsed into a top-level pick for the same root"
        );
        assert_eq!(
            output[0].route,
            RouteDemand::pick(vec!["slots".to_string()]),
        );
        assert_eq!(
            output[1].route,
            RouteDemand::member_path(vec!["variants".to_string(), "color".to_string()]),
        );
    }

    #[test]
    fn owner_import_root_resolves_named_imports_through_barrels() {
        let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
            verter_workspace::MemoryOptions::default(),
        ));
        ws.inject_file(
            "/src/App.vue".to_string(),
            Arc::from(
                r#"<script setup lang="ts">
import type { AvatarProps } from './types'

export interface Props {
  avatar?: AvatarProps
}

defineProps<Props>()
</script>
<template><div /></template>"#,
            ),
        );
        ws.inject_file(
            "/src/types.ts".to_string(),
            Arc::from("export * from './Alert.vue'\nexport * from './Avatar.vue'\n"),
        );
        ws.inject_file(
            "/src/Alert.vue".to_string(),
            Arc::from(
                r#"<script lang="ts">
export interface AlertProps {
  title?: string
}
</script>
<template><div /></template>"#,
            ),
        );
        ws.inject_file(
            "/src/Avatar.vue".to_string(),
            Arc::from(
                r#"<script lang="ts">
export interface AvatarProps {
  src?: string
}
</script>
<template><div /></template>"#,
            ),
        );

        let host = VerterHost::new(
            HostConfig {
                analysis_level: AnalysisLevel::Full,
                ..HostConfig::default()
            },
            ws,
        );
        assert!(host.ensure_loaded("/src/App.vue"));
        host.set_import_dependencies(
            "/src/App.vue",
            vec![DependencyResolution {
                specifier: "./types".to_string(),
                resolved_canonical_id: Some("/src/types.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            }],
        );
        host.set_import_dependencies(
            "/src/types.ts",
            vec![
                DependencyResolution {
                    specifier: "./Alert.vue".to_string(),
                    resolved_canonical_id: Some("/src/Alert.vue".to_string()),
                    possible_canonical_ids: Vec::new(),
                },
                DependencyResolution {
                    specifier: "./Avatar.vue".to_string(),
                    resolved_canonical_id: Some("/src/Avatar.vue".to_string()),
                    possible_canonical_ids: Vec::new(),
                },
            ],
        );

        let snapshot = host
            .get_raw_analysis_snapshot("/src/App.vue")
            .expect("app snapshot should exist");

        let resolved = owner_component_meta_registry_import_root(
            &host,
            "/src/App.vue",
            verter_type_expr::TopLevelOwnerId::instance(0),
            &snapshot,
            "AvatarProps",
        );

        assert_eq!(
            resolved,
            Some((
                "/src/Avatar.vue".to_string(),
                verter_type_expr::TopLevelOwnerId::module(0),
                "AvatarProps".to_string(),
            )),
            "registry import roots must preserve the exact module-script owner from the defining Vue file instead of substituting the consumer instance owner",
        );
        assert_ne!(
            resolved.as_ref().map(|(_, owner, _)| *owner),
            Some(verter_type_expr::TopLevelOwnerId::instance(0)),
            "the consumer's instance owner is not authoritative for a declaration authored in the dependency's normal script block",
        );
    }
}
