//! Core walker types and rewrite helpers.
//!
//! Hosts the `PolicyRegistry` / `PolicyCtx` / `DeclLookup` types that
//! coordinate the policy walk and the node-domain `rewrite_*` helpers
//! consumed by the entrypoint in `mod.rs`.
//!
//! The policy operates on content-free SOURCES: every driver field is a
//! [`SemanticTypeSource`], each decision raises the source to a
//! semantic-graph node through the ONE shared dispatch bridge and
//! classifies node-domain, and a fired rule publishes a REPLACEMENT source
//! (never a materialized `TypeExpr` — materialization happens only at the
//! sealed output sink).

use std::sync::Arc;

use rustc_hash::{FxHashMap, FxHashSet};
use verter_semantic::analysis::component_meta::ResolvedTypeAnalysis;
use verter_semantic::analysis::type_solver::host::ResolvedRootIdentity;
use verter_type_expr::facts::SemanticTypeSource;

use crate::host_manage::component_meta_extract::resolve_ref_to_root_identity;
use crate::project_semantic_dispatch::semantic_source::SourceRaiseContext;
use crate::project_semantic_dispatch::ProjectSemanticDispatch;
use crate::resolver_core::component_meta::ResolvedTypeRegistryMeta;
use crate::resolver_core::component_meta_registry::{
    component_meta_registry_node_ref_head, source_bare_ref_name,
};
use crate::resolver_core::{ComponentMetaQueryEngine, ResolverContext};
use crate::semantic_query::{
    DeclIdentity, HotTypeRef, ProjectionMode, ProjectionReductionContext, SemanticNodeData,
    SemanticNodeId,
};
use crate::VerterHost;

use super::cycle_guard::NormalizedTypeArgs;

/// Build O(1)-lookup tables over the resolved type registry / meta.
/// `resolved_type_registry` and `resolved_type_registry_meta` are aligned by
/// `name` — the meta entry for `name` lives in the parallel meta vec.
pub(super) struct PolicyRegistry<'a> {
    /// `name → &SemanticTypeSource` of the resolved registry entry.
    source_by_name: FxHashMap<&'a str, &'a SemanticTypeSource>,
    /// `name → canonical_source` of the declaration.
    canonical_source_by_name: FxHashMap<&'a str, &'a str>,
    /// Distinct, non-empty declaration scopes drawn from the registry meta.
    /// Used as fallback scopes for cross-file declaration lookup of
    /// transitively-referenced types (e.g. types referenced from a macro
    /// argument's declaration but not registered as a top-level macro arg).
    fallback_scopes: Vec<&'a str>,
}

impl<'a> PolicyRegistry<'a> {
    pub(super) fn build(
        type_registry: &'a [ResolvedTypeAnalysis],
        type_registry_meta: &'a [ResolvedTypeRegistryMeta],
    ) -> Self {
        let mut source_by_name = FxHashMap::default();
        for entry in type_registry.iter() {
            // Registry rows resolve present sources by construction; a
            // non-present position carries no policy-consultable body.
            if let Some(source) = entry.type_source.present() {
                source_by_name.insert(entry.name.as_str(), source);
            }
        }
        let mut canonical_source_by_name = FxHashMap::default();
        let mut fallback_scopes: Vec<&'a str> = Vec::new();
        for meta in type_registry_meta.iter() {
            let canonical = meta.declaration.canonical_source.as_str();
            canonical_source_by_name.insert(meta.name.as_str(), canonical);
            if !canonical.is_empty() && !fallback_scopes.contains(&canonical) {
                fallback_scopes.push(canonical);
            }
        }
        Self {
            source_by_name,
            canonical_source_by_name,
            fallback_scopes,
        }
    }

    /// The registry entry's published body SOURCE, when it carries body
    /// knowledge. A shallow SELF-referential seed (`Closed(Leaf(Ref(name)))`
    /// for the entry's own name) carries none — the entry says "resolve me
    /// on demand" — so the lookup falls through to the engine's
    /// declaration-body route.
    fn registry_body(&self, name: &str) -> Option<&'a SemanticTypeSource> {
        self.source_by_name
            .get(name)
            .copied()
            .filter(|source| source_bare_ref_name(source) != Some(name))
    }

    fn canonical_source(&self, name: &str) -> Option<&'a str> {
        self.canonical_source_by_name
            .get(name)
            .copied()
            .filter(|s| !s.is_empty())
    }
}

pub(super) struct PolicyCtx<'a, 'h> {
    pub(super) registry: &'a PolicyRegistry<'a>,
    pub(super) engine: &'a mut ComponentMetaQueryEngine<'h>,
    pub(super) owner_canonical: &'a str,
    pub(super) host: &'a VerterHost,
    /// Set of `ResolvedRootIdentity` values that participate in
    /// type-role-bearing Vue SFC macros (`defineProps`, `defineEmits`,
    /// `defineModel`, `defineSlots`, `withDefaults`) on the owner SFC.
    ///
    /// A type reference is classified "role-bearing" — and thus kept
    /// symbolic per Rules 2 / 4 + the raw-restoration helpers — IFF its
    /// resolved root identity appears in this set. This is the §3.4
    /// (Typed-IR-Only Resolver Rule) structural macro-participation
    /// classifier: type-role is conferred by macro consumption, not by
    /// nominal name suffix.
    ///
    /// Built once per `apply_component_meta_resolution_policy` call by
    /// `build_policy_macro_role_identities` raising
    /// `AnalyzedMacro.parsed_type_argument` (skeleton-only node walk) and
    /// `AnalyzedMacro.resolved_local_types[i].name` / `.shape` (full-body
    /// node walk for named alias closures). Names resolve through
    /// `resolve_ref_to_root_identity` (scope-aware: local declarations
    /// shadow imports, imports route through
    /// `resolve_local_import_symbol_target`).
    pub(super) macro_participating_idents: &'a FxHashSet<ResolvedRootIdentity>,
    /// Cycle-guard active set keyed on `(DeclIdentity, NormalizedTypeArgs)`
    /// per Invariant #20. Bare-name keying is forbidden because:
    /// * Two `Foo`s in different files would collide under name keying.
    /// * `Pick<X, 'a'>` and `Pick<X, 'b'>` would collide under name-only
    ///   keying — but they are distinct instantiations and must navigate
    ///   independently.
    /// * Anonymous-type cycles re-entering through identical structural
    ///   shapes need an identity that is stable across independent raises.
    pub(super) active_refs: FxHashSet<(DeclIdentity, NormalizedTypeArgs)>,
    /// Greatest observed depth of `active_refs` during the walk. Used
    /// to surface a `policy_active_refs_max_depth` counter under
    /// `CaptureToken` for cycle-guard fixtures.
    pub(super) active_refs_max_depth: u64,
}

impl<'a, 'h> PolicyCtx<'a, 'h> {
    /// The request-bound resolver context every raise / node read routes
    /// through (overlay-aware under a session context).
    pub(super) fn resolver_ctx(&self) -> &'h dyn ResolverContext {
        self.engine.ctx
    }

    /// Raise a content-free source to a transient graph handle through the
    /// ONE shared dispatch bridge, under the owner's name-resolution scope
    /// (authored locators self-anchor; only producer-local empty anchors
    /// absolutize against the scope).
    pub(super) fn raise_source(&self, source: &SemanticTypeSource) -> Option<HotTypeRef> {
        self.raise_source_in_scope(source, self.owner_canonical)
    }

    /// [`Self::raise_source`] under an explicit name-resolution scope — the
    /// declaring file for a located declaration body.
    pub(super) fn raise_source_in_scope(
        &self,
        source: &SemanticTypeSource,
        scope_canonical_id: &str,
    ) -> Option<HotTypeRef> {
        let dispatch = ProjectSemanticDispatch::new(self.resolver_ctx());
        dispatch.raise_semantic_type_source_to_hot(
            source,
            SourceRaiseContext {
                scope_canonical_id,
                context: ProjectionReductionContext::structural_transit_with_mode(
                    ProjectionMode::Navigate,
                ),
                interior_failures: None,
            },
        )
    }

    /// Node data reader (the shared dispatch-owned arena read).
    pub(super) fn node_data(&self, node: SemanticNodeId) -> Option<Arc<SemanticNodeData>> {
        crate::project_semantic_dispatch::node_data_for(self.resolver_ctx(), node)
    }

    /// The node's reference HEAD: `(name, type-argument nodes)` for the
    /// three reference carriers.
    pub(super) fn node_ref_head(
        &self,
        node: SemanticNodeId,
    ) -> Option<(String, Vec<SemanticNodeId>)> {
        component_meta_registry_node_ref_head(self.resolver_ctx(), node)
    }

    /// Locate `name`'s declaration body SOURCE. The body lookup itself is
    /// the authoritative signal for "this declaration exists" —
    /// registry-meta `canonical_source` only tells us the macro arg's home,
    /// not whether a transitively-referenced type is reachable.
    ///
    /// Returns `(canonical_source, body source)`. The first scope that
    /// produces a body wins; the resolver scopes are the owner first then
    /// registry-meta declaration sources — `Status` is referenced from
    /// `ExternalProps.status` whose declaration lives in `/types.ts`, so
    /// resolving `Status` in `/App.vue` returns nothing but resolving in
    /// `/types.ts` succeeds.
    pub(super) fn locate_declaration(&mut self, name: &str) -> Option<DeclLookup> {
        // Registry first — pre-resolved data, zero engine work. A shallow
        // self-referential seed carries no body knowledge and falls
        // through to the engine's declaration-body route below.
        if let Some(body) = self.registry.registry_body(name) {
            let canonical = self
                .registry
                .canonical_source(name)
                .unwrap_or(self.owner_canonical)
                .to_string();
            return Some(DeclLookup {
                canonical_source: canonical,
                body: body.clone(),
            });
        }
        let mut scopes: Vec<String> = vec![self.owner_canonical.to_string()];
        for fallback in self.registry.fallback_scopes.iter() {
            if !scopes.iter().any(|existing| existing.as_str() == *fallback) {
                scopes.push(fallback.to_string());
            }
        }
        for scope in scopes {
            let decl = self.engine.resolve_type_declaration(&scope, name);
            if decl.canonical_source.is_empty() {
                continue;
            }
            let resolved_name = if decl.resolved_name.is_empty() {
                name.to_string()
            } else {
                decl.resolved_name.clone()
            };
            if let Some(locator) = self
                .engine
                .named_decl_body(&decl.canonical_source, &resolved_name)
            {
                return Some(DeclLookup {
                    canonical_source: decl.canonical_source,
                    body: SemanticTypeSource::Authored(locator),
                });
            }
        }
        None
    }

    /// Build a `DeclIdentity` for `(canonical_source, name)` by reading
    /// the file's `whole_hash` from the host's shallow file state. Files
    /// that have no shallow state (e.g. synthetic test fixtures, the
    /// owner SFC before parse, or registry-meta entries pointing at
    /// not-yet-loaded paths) fall back to `HashValue::default()` — the
    /// `(canonical_source, name)` pair is still a deterministic identity
    /// for the cycle guard within a single policy invocation.
    pub(super) fn decl_identity_for(&self, canonical_source: &str, name: &str) -> DeclIdentity {
        let whole_hash = self
            .host
            .shallow_file_state(canonical_source)
            .map(|state| state.whole_hash)
            .unwrap_or_default();
        DeclIdentity {
            canonical_id: Arc::from(canonical_source),
            whole_hash,
            decl_name: Arc::from(name),
        }
    }

    /// §3.4 structural macro-participation predicate.
    ///
    /// Resolves the bare-name reference `name` to its
    /// `ResolvedRootIdentity` and checks whether that identity is in
    /// the macro-participating set built by
    /// `apply_component_meta_resolution_policy`.
    ///
    /// Two resolution paths are consulted, in order:
    ///
    /// 1. **Host scope resolution** (production path):
    ///    `resolve_ref_to_root_identity` consults the host's local-type
    ///    declarations and the cached import-target resolver. This is
    ///    the path that pairs with the set-construction path in
    ///    `build_policy_macro_role_identities`, so warm host hits
    ///    always agree.
    /// 2. **Registry fallback** (unit-test path): when host state has
    ///    not been seeded, derive the identity from the policy
    ///    registry's `canonical_source_by_name`. The registry's
    ///    canonical_source is the file declaring the type — the same
    ///    identity host resolution would have computed had the file
    ///    been loaded.
    ///
    /// Returns `true` if `name` resolves to a type consumed by one of
    /// the owner's type-role-bearing macros (`defineProps`,
    /// `defineEmits`, `defineModel`, `defineSlots`, `withDefaults`).
    ///
    /// Type-role classification is structural, not nominal — never a
    /// name-suffix check.
    pub(super) fn is_macro_participating(&self, name: &str) -> bool {
        if let Some(identity) = resolve_ref_to_root_identity(self.host, self.owner_canonical, name)
        {
            if self.macro_participating_idents.contains(&identity) {
                return true;
            }
        }
        if let Some(canonical) = self.registry.canonical_source(name) {
            let identity = ResolvedRootIdentity::new(canonical, name);
            return self.macro_participating_idents.contains(&identity);
        }
        false
    }
}

pub(super) struct DeclLookup {
    pub(super) canonical_source: String,
    pub(super) body: SemanticTypeSource,
}

/// Returns true if the published source position was replaced. Only a
/// PRESENT source is rule-walked: a proven absence and a typed failure pass
/// through the policy untouched (the policy refines present sources; it
/// never fabricates one and never launders a failure).
pub(super) fn rewrite_source_in_place(
    slot: &mut verter_type_expr::facts::SourcePosition,
    ctx: &mut PolicyCtx<'_, '_>,
) -> bool {
    let Some(current) = slot.present() else {
        return false;
    };
    let Some(next) = rewrite_source(current, ctx) else {
        return false;
    };
    if slot.present() == Some(&next) {
        return false;
    }
    *slot = verter_type_expr::facts::SourcePosition::Present(next);
    true
}

/// Decide a published source's replacement, node-domain: raise the source
/// ONCE through the shared bridge and run the rule walk off the raised
/// node. `None` = keep the existing published source (unraisable sources
/// stay shallow verbatim — never a fabricated stand-in).
pub(super) fn rewrite_source(
    source: &SemanticTypeSource,
    ctx: &mut PolicyCtx<'_, '_>,
) -> Option<SemanticTypeSource> {
    let hot = ctx.raise_source(source)?;
    rewrite_node(hot.node(), ctx)
}

/// The node-domain rule walk over a raised published-source node. Returns
/// `Some(replacement source)` when a rule fired; `None` keeps the existing
/// published source.
pub(super) fn rewrite_node(
    node: SemanticNodeId,
    ctx: &mut PolicyCtx<'_, '_>,
) -> Option<SemanticTypeSource> {
    if let Some((name, args)) = ctx.node_ref_head(node) {
        return rewrite_ref_node(name.as_str(), &args, ctx);
    }
    // Rule 2: a member-path on a macro-participating type stays symbolic
    // (e.g. `MyProps['avatar']`). Structural §3.4 classification — the
    // root must resolve to an identity consumed by one of the owner's
    // role-bearing macros. Every other structural root keeps its source:
    // interior positions materialize shallow at the sealed output sink and
    // consumers re-resolve them on demand.
    None
}

/// Rewrite a reference-headed node per rules 1, 3, 4, 5.
fn rewrite_ref_node(
    name: &str,
    arg_nodes: &[SemanticNodeId],
    ctx: &mut PolicyCtx<'_, '_>,
) -> Option<SemanticTypeSource> {
    // Rule 4: macro-participating bare alias / generic stays symbolic.
    // Type-role classification is structural (the reference resolves to a
    // type consumed by one of the owner's `defineProps` / `defineEmits` /
    // `defineModel` / `defineSlots` / `withDefaults` macros), NOT nominal
    // (the identifier ends in `"Props"`). See §3.4 of the Typed-IR-Only
    // Resolver Rule.
    if ctx.is_macro_participating(name) {
        return None;
    }

    // For non-participating refs, check whether the declaration is
    // reachable. If not (builtin utilities like `Pick` / `Omit` included),
    // the carrier stays symbolic — the sealed output sink's reducer owns
    // path-precise selective materialization of utility carriers (L1).
    let lookup = ctx.locate_declaration(name)?;

    // Rule 1: package-backed refs stay symbolic.
    if ctx
        .host
        .workspace_is_package_backed(&lookup.canonical_source)
    {
        return None;
    }

    // Build the `(DeclIdentity, NormalizedTypeArgs)` cycle-guard key. The
    // identity is keyed on resolved declaration source, not the bare
    // name — two `Foo`s in different files produce different identities;
    // `Pick<X, 'a'>` and `Pick<X, 'b'>` produce different normalized
    // type-args so they navigate independently.
    let decl_identity = ctx.decl_identity_for(&lookup.canonical_source, name);
    let normalized_args = NormalizedTypeArgs::normalize_nodes(arg_nodes, ctx);
    let guard_key = (decl_identity, normalized_args);

    // Re-entry on the same key keeps the symbolic carrier: the prior
    // invocation is still resolving this declaration, and continuing would
    // recurse forever. The preserved shallow carrier IS the recursion
    // back-edge — the consumer re-resolves it on demand and reaches the
    // same shallow published form. Generic substitutions are part of
    // identity — `Foo<A>` and `Foo<B>` are distinct guard keys and only
    // stop on a back edge when the *same* substitution recurs.
    if ctx.active_refs.contains(&guard_key) {
        return None;
    }

    ctx.active_refs.insert(guard_key.clone());
    if (ctx.active_refs.len() as u64) > ctx.active_refs_max_depth {
        ctx.active_refs_max_depth = ctx.active_refs.len() as u64;
        #[cfg(any(test, debug_assertions))]
        crate::capture_token::with_active_capture(|t| {
            t.record_counter("policy_active_refs_max_depth", 1)
        });
    }

    let result = rewrite_ref_body_with_guard(&lookup, arg_nodes, ctx);

    ctx.active_refs.remove(&guard_key);
    result
}

/// Body-chase logic invoked under the active-refs cycle guard. Raises the
/// located declaration body and either publishes it (Rule 3 /
/// project-local non-participating bare alias with a structurally
/// resolvable body) or descends the alias SPINE one reference at a time
/// (Rule 5) — each hop re-enters [`rewrite_ref_node`] under the guard, so a
/// self-referential alias (`type Self = Pick<Self>`, `type A = B; type B =
/// A`) registers on the active set and terminates on the back-edge.
fn rewrite_ref_body_with_guard(
    lookup: &DeclLookup,
    arg_nodes: &[SemanticNodeId],
    ctx: &mut PolicyCtx<'_, '_>,
) -> Option<SemanticTypeSource> {
    // Rule 3 applies only to BARE references: a generic instantiation's
    // published carrier keeps the substitution (publishing the uninstantiated
    // declaration body would lose it).
    if !arg_nodes.is_empty() {
        return None;
    }
    let body_hot = ctx.raise_source_in_scope(&lookup.body, &lookup.canonical_source)?;
    let body_node = body_hot.node();
    if body_root_is_resolvable(body_node, ctx) {
        // Publish the located declaration's body SOURCE. Nested positions
        // stay shallow: the sealed output sink materializes the body and
        // consumers re-resolve interior references on demand.
        return Some(lookup.body.clone());
    }
    // Alias-spine descent: `type A = B` / `type A = Pick<Self, 'x'>` — the
    // body root is itself a reference head. Descend the DECISION through
    // the guard so a cyclic spine terminates on the back-edge, and adopt
    // the inner chase's publication when one resolves.
    if let Some((inner_name, inner_args)) = ctx.node_ref_head(body_node) {
        return rewrite_ref_node(inner_name.as_str(), &inner_args, ctx);
    }
    None
}

// ---------------------------------------------------------------------------
// Predicates
// ---------------------------------------------------------------------------

/// Walk through wrapper nodes; return true if any leaf is a reference whose
/// resolved root identity participates in one of the owner's
/// type-role-bearing Vue SFC macros.
///
/// Structural §3.4 classification — never a nominal name-suffix filter.
pub(super) fn indexed_access_targets_macro_participating(
    object: SemanticNodeId,
    ctx: &PolicyCtx<'_, '_>,
) -> bool {
    if let Some((name, _)) = ctx.node_ref_head(object) {
        return ctx.is_macro_participating(name.as_str());
    }
    match ctx.node_data(object).as_deref() {
        Some(SemanticNodeData::Alias(target)) => {
            indexed_access_targets_macro_participating(*target, ctx)
        }
        Some(SemanticNodeData::Union(arms)) | Some(SemanticNodeData::Intersection(arms)) => arms
            .iter()
            .any(|arm| indexed_access_targets_macro_participating(*arm, ctx)),
        _ => false,
    }
}

/// A located declaration body is "resolvable" if its raised ROOT is a
/// structural shape (Object / Union / Intersection / Array / Tuple /
/// Function / Primitive / Literal / Conditional / Mapped / TemplateLiteral
/// / KeyOf / merged declaration / etc.) — anything other than a reference
/// head (which would just chase to another symbolic; the alias-spine
/// descent owns that hop).
///
/// Bodies that are themselves an IndexedAccess on a macro-participating
/// alias should NOT resolve eagerly — they are kept symbolic because that
/// is the registry authoritative form.
pub(super) fn body_root_is_resolvable(body: SemanticNodeId, ctx: &PolicyCtx<'_, '_>) -> bool {
    if ctx.node_ref_head(body).is_some() {
        return false;
    }
    match ctx.node_data(body).as_deref() {
        Some(SemanticNodeData::Alias(target)) => body_root_is_resolvable(*target, ctx),
        Some(SemanticNodeData::IndexedAccess { object, .. }) => {
            !indexed_access_targets_macro_participating(*object, ctx)
        }
        // A cross-file import carrier is a symbolic reference (like a bare
        // reference head), an opaque / raw-fallback node carries no
        // publishable shape, and open type structure (type parameters /
        // infer placeholders / synthetic bindings) is not a body to chase.
        Some(SemanticNodeData::ImportType(_))
        | Some(SemanticNodeData::Opaque(_))
        | Some(SemanticNodeData::RawFallback { .. })
        | Some(SemanticNodeData::TypeParam { .. })
        | Some(SemanticNodeData::Infer { .. })
        | Some(SemanticNodeData::SyntheticBinding { .. })
        | None => false,
        Some(
            SemanticNodeData::Object(_)
            | SemanticNodeData::Union(_)
            | SemanticNodeData::Intersection(_)
            | SemanticNodeData::Primitive(_)
            | SemanticNodeData::Literal(_)
            | SemanticNodeData::Array { .. }
            | SemanticNodeData::Tuple { .. }
            | SemanticNodeData::TemplateLiteral { .. }
            | SemanticNodeData::KeyOf { .. }
            | SemanticNodeData::Mapped { .. }
            | SemanticNodeData::TypeOf(_)
            | SemanticNodeData::Conditional { .. }
            | SemanticNodeData::Function { .. }
            | SemanticNodeData::ConstructorType { .. }
            | SemanticNodeData::MergedDecl { .. },
        ) => true,
        // Reference carriers are rejected by the head check above.
        Some(
            SemanticNodeData::BareRef(_)
            | SemanticNodeData::DeclRef { .. }
            | SemanticNodeData::InstantiationRef { .. },
        ) => false,
    }
}
