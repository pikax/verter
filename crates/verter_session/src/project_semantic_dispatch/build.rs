//! Dispatch-layer builders. Every semantic query
//! variant that produces a new [`SemanticNodeId`] does so through one of
//! the `build_*` methods collected here. Kept on `ProjectSemanticDispatch`
//! via an `impl` block so the inner helpers share private accessors
//! (`graph`, `opaque`, `dep_signature_for`, etc.) without widening their
//! visibility beyond `pub(super)`.

use std::sync::Arc;

use rustc_hash::{FxHashMap, FxHashSet};
use verter_semantic::analysis::type_solver::host::{ResolvedRootIdentity, UtilitySource};
use verter_semantic::analysis::type_solver::PreparedTypeDecl;
use verter_type_expr::{FunctionExpr, ObjectExpr, ObjectMember, ObjectProperty, TypeExpr};

use super::walk::PathWalker;
use super::{
    empty_signature, utility_param_names, ConditionalBranchSelection, DispatchHost,
    InferPatternSelection, ProjectSemanticDispatch, SessionDispatchHost, ShallowRelation,
};
use crate::semantic_query::demand::{Demand, MaterializedPoint, MaterializedSet, ProjectionPath};
use crate::semantic_query::{
    BranchSelection, DepSignature, HostResolvedNamedTypeKey, IndexKey, IndexSignature,
    LiteralValue, NodeScopeId, OriginEdgeKind, OriginMeta, PathSegment, PrimitiveKind,
    ProjectionMode, QueryError, QueryResult, ReductionDemand, ResolveDeclKey, SemanticNodeData,
    SemanticNodeId, SemanticQueryKey, SurfaceMember, SurfaceView, ValueRootKey,
};

/// One folded cross-file augmentation contributor: its version self-root
/// (`canonical` + the `FileWholeHash` the body was lowered from) TOGETHER
/// WITH the exact artifact key the contributor's locator-backed body read
/// served from. One carrier BY CONSTRUCTION: a contributor cannot enter the
/// parent's `self_root_canonicals` without its source-env identity — the
/// version-root fields and the `artifact_key` are inseparable, and the fact
/// emission records the contributor's `FileWholeHash` observation AND its
/// `FileSourceEnv` observation from the SAME element in one step.
struct AugmentationContributorRoot {
    canonical: Arc<str>,
    whole_hash: crate::semantic_query::HashValue,
    artifact_key: crate::file_artifact_store::FileArtifactKey,
}

/// Result of a cross-file declaration-augmentation stitch
/// ([`ProjectSemanticDispatch::stitch_module_augmentations`]): the folded
/// [`SemanticNodeData::MergedDecl`] node plus the per-augmenter
/// [`AugmentationContributorRoot`]s whose `(canonical, whole_hash)` version
/// roots must enter the cached value's `self_root_canonicals`.
struct AugmentationStitch {
    merged: SemanticNodeId,
    contributor_roots: Vec<AugmentationContributorRoot>,
    /// `true` when a contributor's coherent source-env identity could not
    /// be observed (a torn / unhealable augmenter entry): the parent result
    /// is served but never warm-admitted.
    source_env_unobservable: bool,
}

/// Shared return shape of the augmenter-fold path
/// ([`ProjectSemanticDispatch::collect_augmentation_contributions`]): the
/// ordered augmenter contributor nodes, one [`AugmentationContributorRoot`]
/// per contributing augmenter, and whether any contributor's source-env
/// identity was unobservable (torn state ⇒ no warm admission).
struct AugmentationContributions {
    contributor_nodes: Vec<SemanticNodeId>,
    contributor_roots: Vec<AugmentationContributorRoot>,
    source_env_unobservable: bool,
}

/// One resolved heritage base from a class's `extends` clause
/// ([`ProjectSemanticDispatch::class_heritage_bases`]): the base decl's
/// `(canonical, symbol)` identity plus the heritage clause's authored
/// type-arguments (`extends Base<string>`), still un-lowered `TypeExpr`s.
type HeritageBase = (Arc<str>, Arc<str>, Arc<[TypeExpr]>);

/// Upper bound on the template-literal keyspace product width
/// `∏ |choice_set_i|` enumerated by
/// [`ProjectSemanticDispatch::reduce_template_literal_nodes`]. A finite
/// template whose enumerated product would exceed this cap carrier-stops to
/// the deferred [`SemanticNodeData::TemplateLiteral`] shell instead of
/// materialising (and possibly warm-publishing) an explosive union. The cap
/// sits well above any realistic component template keyspace (event / slot /
/// prop-name enumerations are far below it) while bounding allocation on the
/// pathological tail. This is a PRODUCT-WIDTH bound, distinct from the
/// deferred evaluator's per-arg recursion depth ceiling — that ceiling bounds
/// how deep one argument resolves, not how wide the cartesian product grows.
pub(super) const TEMPLATE_LITERAL_KEYSPACE_CAP: usize = 1024;

/// Outcome of [`ProjectSemanticDispatch::reduce_template_literal_nodes`]: the
/// folded surface node plus whether the keyspace product-width budget was
/// exceeded. A `keyspace_budget_exceeded == true` outcome carries the deferred
/// `TemplateLiteral` carrier-stop shell as `node`, and the live producer marks
/// the build non-cacheable / budget-tainted so it is never warm-admitted.
pub(super) struct TemplateReduceOutcome {
    pub(super) node: SemanticNodeId,
    pub(super) keyspace_budget_exceeded: bool,
}

/// Canonical TypeScript stringification of a literal interpolated into a
/// template-literal type (`` `${...}` ``). Typed-IR only — it reads the
/// interned [`LiteralValue`], never source text. Mirrors TS lexing: a string
/// literal contributes its text; a numeric literal its JS `Number`→string
/// form; a boolean `"true"` / `"false"`; a bigint its base-10 digits (the
/// `n` suffix is literal syntax, not part of the interpolated string).
fn literal_value_template_text(value: &LiteralValue) -> String {
    match value {
        LiteralValue::String(text) => text.clone(),
        LiteralValue::Number(number) => js_number_to_string(*number),
        LiteralValue::Boolean(flag) => if *flag { "true" } else { "false" }.to_string(),
        // `LiteralValue::BigInt` stores the signed base-10 magnitude with no
        // `n` suffix (see `verter_type_expr_oxc::lower_literal`), which is
        // exactly TS's interpolated form of a bigint literal.
        LiteralValue::BigInt(digits) => digits.clone(),
    }
}

// The canonical numeric-spelling cluster (`js_number_to_string`, the
// `integer_convention_index_key` admission fold, the ECMA-262 even
// tie-break) lives in `crate::semantic_query::index_key` — the module
// that OWNS the `IndexKey::Number` payload (`CanonicalIndexInt`,
// private field, blessed constructors only). Re-exported here because
// the dispatch submodules are its main consumers.
pub(super) use crate::semantic_query::index_key::{
    integer_convention_index_key, js_number_to_string,
};

/// Encode a [`ProjectionReductionContext`] as a compact u32 bit
/// pattern used in the mapped-member-materialization
/// identity tuple. Layout: bits 0–1 (demand tag) and 2+ (mode tag).
#[inline]
pub(super) fn encode_projection_reduction_context_bits(
    context: crate::semantic_query::ProjectionReductionContext,
) -> u32 {
    let mode_tag: u32 = match context.mode {
        ProjectionMode::Identity => 0,
        ProjectionMode::Navigate => 1,
        ProjectionMode::Shallow => 2,
        ProjectionMode::Expanded => 3,
        ProjectionMode::Skeleton => 4,
    };
    // 2-bit demand tag (three demands: Published / StructuralTransit /
    // MacroObjectSurface). Mode shifts by 2 so the demand axis
    // stays disjoint in the packed identity.
    let demand_tag: u32 = match context.demand {
        ReductionDemand::Published => 0,
        ReductionDemand::StructuralTransit => 1,
        ReductionDemand::MacroObjectSurface => 2,
    };
    (mode_tag << 2) | demand_tag
}

// Per-call counter (test-only). Incremented every time
// `find_longest_warm_prefix` returns `Some(_)` during a
// `ProjectSemanticDispatch::build_project_path` invocation. Used by
// `project_path_prefix_peek_short_circuits_sibling_walk` to discriminate
// pre-fix (counter never increments — peek helper not yet wired) vs
// post-fix (counter delta is exactly 1 across a sibling-prefix replay).
//
// Diagnostic-only — never read on the hot path. Tests using this
// counter MUST reset it before measuring (`with(|c| *c.borrow_mut() = 0)`)
// because the thread-local persists across tests in the same process.
#[cfg(test)]
thread_local! {
    pub(super) static PREFIX_PEEK_HITS: std::cell::RefCell<u32> = const { std::cell::RefCell::new(0) };
}

impl<'a> ProjectSemanticDispatch<'a> {
    /// Collect the observed self-roots of a set of input `SemanticNodeId`s.
    ///
    /// A node kind keyed by already-interned input nodes (`ProjectPath` /
    /// `ProjectMember` / `IndexedAccess` rooted at `base`; `KeyOf` /
    /// `MappedType` / `Conditional` / `NormalizeUnion` /
    /// `NormalizeIntersection` over their input nodes) produces a result
    /// whose identity transitively depends on the file content each
    /// file-derived input was lowered from. The input node's origin scope
    /// — recorded in the arena sidecar at intern time — names that file
    /// and the content version (`whole_hash`) it was observed at.
    ///
    /// This helper reads [`crate::semantic_query_memo::SemanticGraphStore::node_scope`]
    /// for each input id and yields one `(canonical, observed_whole_hash)`
    /// self-root per [`crate::semantic_query::NodeScopeId::File`]-scoped
    /// input. `Global`-scoped inputs (primitives, structural helper
    /// intermediates) and sidecar-exempt inputs contribute nothing — a
    /// fully structural result has no file self-root.
    ///
    /// The `whole_hash` is the version the input node already carries in
    /// its scope sidecar — captured when the node was interned. This
    /// helper never re-reads current content; it only projects the
    /// already-observed identity each input node carries.
    pub(super) fn observed_self_roots_from_nodes(
        &self,
        nodes: impl IntoIterator<Item = SemanticNodeId>,
    ) -> Vec<crate::semantic_query_memo::ObservedGraphSelfRoot> {
        let graph = self.graph();
        let mut roots: Vec<crate::semantic_query_memo::ObservedGraphSelfRoot> = Vec::new();
        for node in nodes {
            if let Some(NodeScopeId::File {
                canonical_id,
                whole_hash,
                ..
            }) = graph.node_scope(node)
            {
                if !roots
                    .iter()
                    .any(|(c, h)| *c == canonical_id && *h == whole_hash)
                {
                    roots.push((canonical_id, whole_hash));
                }
            }
        }
        roots
    }

    /// Resolve a top-level declaration lookup via the host's shallow state.
    ///
    /// Declaration identity is carried by the `Instantiate` key's
    /// `DeclIdentity` field directly — no separate `DeclAnchor` node is
    /// interned. This builder validates that the name exists in the
    /// shallow state, records the file scope in the sidecar, and returns
    /// an `Opaque(Miss)` placeholder node; the actual identity is carried
    /// by the caller via the key.
    pub(super) fn build_resolve_decl(
        &self,
        key: &ResolveDeclKey,
    ) -> crate::project_semantic_dispatch::walk::QueryBuildOutput {
        // Self-version rooting: observe the scope canonical.s
        // `IndexedReady` ONCE through `ensure_indexed_ready_serve` — the
        // overlay-aware host accessor (a `SessionResolverContext`
        // observes the overlay artifact, not the base file). The
        // single observed artifact roots both the node.s
        // `NodeScopeId::File` and the memo entry.s self-root
        // `FileWholeHash` on `indexed.whole_hash`, and its
        // `shallow_state` is the shallow inventory this builder reads
        // — value basis and self-root descend from one observation, so
        // a concurrent edit cannot tear them. `shallow_file_state` is
        // deliberately not used: it reads the base file hash under an
        // overlay and re-reads content outside the single observation.
        let indexed = match self
            .ctx
            .ensure_indexed_ready_serve(key.scope.canonical_id.as_ref())
            .map(|serve| serve.indexed)
        {
            Some(indexed) => indexed,
            None => return (QueryResult::Error(QueryError::Miss), empty_signature()).into(),
        };
        let shallow = &indexed.shallow_state;
        let observed_hash = indexed.whole_hash;

        // Local PRESENCE through the CENTRALIZED effective header lookup — a
        // user/synthesized declaration first, then (rune-module-gated) the
        // ambient rune inventory, so a Svelte rune module's `$state`/`$derived`/
        // `$effect`/`$inspect` is treated as locally declared and resolves at
        // this dispatch surface. A plain `.ts` is unaffected (the effective
        // lookup is rune-module-gated, so it reduces to the header-index probe).
        // No per-site rune branch — the single authority lives on
        // `ShallowFileState`.
        let has_type_symbol = shallow.effective_type_header_present(key.name.as_ref());
        let has_value_symbol = shallow.effective_value_header_present(key.name.as_ref());
        let has_export = shallow.exports.contains_key(key.name.as_ref());
        let has_import_local = shallow.import_targets.contains_key(key.name.as_ref());
        // A `declare global { ... }` declaration is not on the file surface but
        // IS resolvable as the merged global declaration (the prepared-decl
        // builder falls back to the global augmentation inventory).
        let has_global_augmentation = shallow.has_global_augmentation(key.name.as_ref());

        // A name DECLARED in this file (type / value symbol, or a
        // `declare global` contribution) resolves here. A name that is
        // only re-exported / imported through this file resolves at its
        // DEFINING file instead — see the fall-through below.
        let has_local_declaration = has_type_symbol || has_value_symbol || has_global_augmentation;

        if !has_local_declaration {
            // Re-export fall-through. A barrel reaches the declaration via
            // `export * from './base'` (no direct symbol / named export),
            // `export { X } from './base'`, or `import { X } ...; export
            // { X }`. In every shape the declaration is authored elsewhere:
            // follow the export graph (the shared route resolver already
            // implements direct > aliased > wildcard precedence) to the
            // defining `(canonical, name)` and resolve THAT declaration, so
            // the declaration's scope — and every member's
            // `declaration_origin`, which anchors typeinfo JSDoc enrichment
            // — labels the file the author wrote it in, not the barrel hop.
            if has_export || has_import_local || shallow.has_wildcard_reexports() {
                if let Some((target_canonical, target_name)) =
                    self.ctx.resolve_named_type_export_target(
                        key.scope.canonical_id.as_ref(),
                        key.name.as_ref(),
                    )
                {
                    if target_canonical.as_str() != key.scope.canonical_id.as_ref()
                        || target_name.as_str() != key.name.as_ref()
                    {
                        let resolved_key = ResolveDeclKey {
                            scope: crate::semantic_query::ScopeId {
                                canonical_id: Arc::from(target_canonical.as_str()),
                                local_scope: None,
                            },
                            name: Arc::from(target_name.as_str()),
                        };
                        // Re-root the barrel-resolved entry on BOTH the
                        // barrel file (whose export surface selected the
                        // target) AND the resolved declaration's own
                        // self-roots, so a content edit to EITHER the
                        // barrel's re-export clause or the defining file
                        // misses the warm read.
                        let inner = self.build_resolve_decl(&resolved_key);
                        let barrel_root = (Arc::clone(&key.scope.canonical_id), observed_hash);
                        let mut roots = inner.observed_self_roots.clone();
                        if !roots
                            .iter()
                            .any(|(c, h)| *c == barrel_root.0 && *h == barrel_root.1)
                        {
                            roots.push(barrel_root);
                        }
                        return inner.with_observed_self_roots(roots);
                    }
                }
            }
            if !(has_export || has_import_local) {
                return (QueryResult::Error(QueryError::Miss), empty_signature()).into();
            }
            // The export graph could not resolve the defining site (e.g. an
            // unresolvable specifier): fall through to the legacy
            // placeholder scoped to this file, whose prepared-declaration
            // builder follows the import lazily.
        }

        // Record the declaration's origin scope in the sidecar so
        // dispatch builders reached from this placeholder can route
        // per-base-scope lookups through the scope's declaration-scope
        // payload.
        let scope = NodeScopeId::File {
            canonical_id: Arc::clone(&key.scope.canonical_id),
            whole_hash: observed_hash,
            local_scope: key.scope.local_scope,
        };
        let signature = self.dep_signature_for(&key.scope.canonical_id, observed_hash);
        // Return a DeclPlaceholder that carries enough identity for
        // callers to construct Instantiate keys.
        let node_id = self.graph().intern_node_with_scope(
            SemanticNodeData::Opaque(QueryError::DeclPlaceholder {
                canonical_id: Arc::clone(&key.scope.canonical_id),
                name: Arc::clone(&key.name),
                whole_hash: observed_hash,
            }),
            scope,
        );
        crate::project_semantic_dispatch::walk::QueryBuildOutput::from((
            QueryResult::Value(node_id),
            signature,
        ))
        .with_observed_self_roots([(Arc::clone(&key.scope.canonical_id), observed_hash)])
    }

    /// `typeof`-rooted declaration lookup. Shape mirrors [`Self::build_resolve_decl`]
    /// but routes through the shallow value-symbol space so the result is
    /// keyed by the value binding's identity.
    ///
    /// `context` is the caller's projection-reduction demand: the value's
    /// annotation / object shape / signature surface / enum surface lowers
    /// AT that demand, so a `Skeleton` / `Navigate` / `Shallow` caller gets
    /// carrier-preserving lowering (member-value type references intern as
    /// `DeclRef` / `InstantiationRef` carriers) while a genuine `Expanded`
    /// caller keeps eager lowering. The overload-visibility projection rule
    /// below is mode-independent semantics and applies in every mode.
    pub(super) fn build_typeof(
        &self,
        value_root: &ValueRootKey,
        context: crate::semantic_query::ProjectionReductionContext,
    ) -> crate::project_semantic_dispatch::walk::QueryBuildOutput {
        // Telemetry for the typeof traversal site.
        if let Some(observer) = verter_audit::current_observer() {
            observer.record_event(verter_audit::AuditEvent::BuildTypeofCall);
        }
        // Self-version rooting: observe the value-root scope
        // canonical.s `IndexedReady` ONCE through the overlay-aware
        // `ensure_indexed_ready_serve` accessor. The single observed
        // artifact roots both the node.s `NodeScopeId::File` and the
        // memo entry.s self-root `FileWholeHash` on
        // `indexed.whole_hash`; its `shallow_state` is the shallow
        // inventory this builder reads — value basis and self-root
        // descend from one observation.
        let indexed = match self
            .ctx
            .ensure_indexed_ready_serve(value_root.scope.canonical_id.as_ref())
            .map(|serve| serve.indexed)
        {
            Some(indexed) => indexed,
            None => {
                // Telemetry: prepared-value miss site.
                if let Some(observer) = verter_audit::current_observer() {
                    observer.record_event(verter_audit::AuditEvent::BuildTypeofPreparedValueMiss);
                }
                return (QueryResult::Error(QueryError::Miss), empty_signature()).into();
            }
        };
        let shallow = &indexed.shallow_state;
        let observed_hash = indexed.whole_hash;

        // Local PRESENCE through the CENTRALIZED effective header lookup so a
        // rune module's ambient `$state`/`$derived`/… value (and the rune
        // namespace types) is seen as locally declared at the `typeof`-rooted
        // dispatch surface. Plain `.ts` is unaffected (rune-module-gated).
        let has_value = shallow.effective_value_header_present(value_root.name.as_ref());
        let has_import_local = shallow
            .import_targets
            .contains_key(value_root.name.as_ref());
        let has_type_symbol = shallow.effective_type_header_present(value_root.name.as_ref());
        // Namespace-qualified root: `Ns.Member` where `Ns` is an import
        // alias (`import * as Ns from './m'`). The shallow state indexes
        // only the top-level alias; the dotted name itself never appears
        // as a literal symbol. Defer resolution to `resolve_bare_name_in_scope`,
        // which handles the namespace-member case via
        // `resolve_namespace_member_from_facts`.
        let has_namespace_prefix = value_root
            .name
            .split_once('.')
            .is_some_and(|(prefix, _)| shallow.import_targets.contains_key(prefix));

        if !(has_value || has_import_local || has_type_symbol || has_namespace_prefix) {
            return (QueryResult::Error(QueryError::Miss), empty_signature()).into();
        }

        // Same scope-recording rule as `build_resolve_decl` — the value
        // binding's origin scope is the owning canonical so dispatch
        // builders downstream can reach the correct declaration file.
        let scope = NodeScopeId::File {
            canonical_id: Arc::clone(&value_root.scope.canonical_id),
            whole_hash: observed_hash,
            local_scope: value_root.scope.local_scope,
        };
        let scope_payload = self
            .ctx
            .prepared_decl_bundle(value_root.scope.canonical_id.as_ref())
            .map(|bundle| {
                crate::resolver_core::bare_name_resolve::DeclarationScopePayload::from_bundle(
                    &bundle,
                )
            });
        // R15/F11 — capture the scope-shadowing context
        // once for the whole `build_typeof` body so every recursive
        // lowering observes the same shadow set.
        let shadowing = crate::resolver_core::scope_shadowing::ScopeShadowing::from_scope_payload(
            scope_payload.as_ref(),
        );
        let root_identity =
            match crate::resolver_core::bare_name_resolve::resolve_bare_name_in_scope(
                self.ctx,
                value_root.scope.canonical_id.as_ref(),
                scope_payload.as_ref(),
                value_root.name.as_ref(),
            ) {
                Some(identity) => identity,
                None => {
                    // `typeof name` where `name` is an IMPORT whose specifier
                    // does not (yet) resolve — `import theme from './theme'`
                    // before `/theme.ts` exists. The miss is a MissingDependency
                    // result: it MUST invalidate the moment the dependency
                    // appears. The build-layer fence (`WholeHash` /
                    // `RouteGeneration` / `ProjectGeneration`) carries NO
                    // `DerivedFactKind::ImportRoute` rail — the only rail whose
                    // hash shifts when a known-miss specifier resolves (a
                    // synthetic project-generation dep is the wrong correctness
                    // rail, per the architecture ruling). So when the name is
                    // import-backed we OBSERVE the owner's `ImportRoute` fact
                    // into the active tracer (`generation_current_import_route_hash`
                    // re-resolves the owner's known-miss specifiers against the
                    // live workspace, so the recorded hash shifts the moment the
                    // dependency appears) — bubbling it into the outer
                    // component-meta result signature so the warm read misses
                    // after the dependency is added. When the route fact cannot
                    // be produced (no import-route surface to root on), the
                    // miss is unrootable and MUST be cache-suppressed
                    // (`ReturnOnly`): the value still flows, but no warm entry
                    // publishes, so the next request recomputes cold and
                    // recovers. A non-import miss (a genuinely absent LOCAL
                    // symbol) stays the ordinary unrooted Miss.
                    let is_import_backed = has_import_local
                        || has_namespace_prefix
                        || scope_payload.as_ref().is_some_and(|p| {
                            p.import_bindings.contains_key(value_root.name.as_ref())
                        });
                    let mut output: crate::project_semantic_dispatch::walk::QueryBuildOutput =
                        (QueryResult::Error(QueryError::Miss), empty_signature()).into();
                    if is_import_backed {
                        let owner_canonical = value_root.scope.canonical_id.as_ref();
                        // Best-effort: observe the owner's `ImportRoute` derived
                        // fact into the active tracer so any consumer cache whose
                        // validity rail consults import-route facts re-validates
                        // when the specifier resolves.
                        if let Some(route_hash) = self
                            .ctx
                            .host_for_fact_tracer_install()
                            .generation_current_import_route_hash(owner_canonical)
                        {
                            crate::fact_signature_helpers::observe_fact_signature(&[
                                crate::resolver_core::FactVersionRef::DerivedFactHash {
                                    canonical_id: owner_canonical.to_string(),
                                    kind: crate::resolver_core::DerivedFactKind::ImportRoute,
                                    hash: route_hash,
                                },
                            ]);
                        }
                        // The build-layer fence cannot carry the `ImportRoute`
                        // rail (no `DepVersion` variant expresses a derived-fact
                        // hash) and a value-import known-miss may not even surface
                        // in the owner's type-import route table — so the route
                        // fact above is NOT a guaranteed invalidation rail for
                        // EVERY consuming memo (the `TypeOf` memo, the
                        // `evaluate_deferred` memo, the field-materialize memo, the
                        // resolved-meta result cache). A `typeof <unresolved
                        // import>` is a genuine `MissingDependency` PARTIAL: the
                        // resolution is structurally incomplete because a dependency
                        // is absent. Marking it `result_is_partial` makes EVERY
                        // consuming cache refuse warm admission (the no-poison
                        // invariant — `result_is_partial` folds through every
                        // nested read and the finalisation boundary forces
                        // `cache_suppress`), so the next request after the
                        // dependency appears recomputes cold and recovers. Also
                        // mark the request-scoped materialization suppress sticky
                        // (which OR-folds into the resolved-meta result's
                        // `synthesis_should_suppress` gate) for the no-`RequestContext`
                        // belt-and-braces. (Per the architecture ruling: when the
                        // route fact cannot guarantee invalidation, the degraded
                        // MissingDependency result is `ReturnOnly`.)
                        output.result_is_partial = true;
                        output.cache_suppress = true;
                        crate::request_context::mark_request_materialization_cache_suppress();
                    }
                    return output;
                }
            };
        // Effective post-fallback identity: when the resolved root names a
        // re-exporting canonical with no local prepared VALUE decl, the
        // export-target walk yields the DECLARING decl — every downstream
        // consumer (the class-surface slot in particular) keys and lowers
        // under THAT identity, never the stale re-export root.
        let Some((effective_canonical, effective_symbol, prepared)) = self
            .effective_prepared_value_decl(&root_identity.canonical_id, &root_identity.symbol_name)
        else {
            return (QueryResult::Error(QueryError::Miss), empty_signature()).into();
        };
        let empty_env = FxHashMap::default();
        let mut substitutions = Vec::new();
        // CONVERGENCE (scope-bounded to a synthesized `.vue`/typeinfo-scratch
        // `default` ONLY): when the resolved value root is EXACTLY the
        // synthesized `.vue` public-instance `default` — `root_identity`'s symbol
        // is `default` AND the resolved canonical's shallow `default` value
        // symbol carries the `is_synthesised_component_default` provenance flag — the
        // construct-signature RETURN must be produced by the keyed
        // `Instantiate(.vue default)` query, not by re-lowering the synthesized
        // default's first signature `return_type` here. This keeps `typeof
        // Foo` / `InstanceType<typeof Foo>` (Foo a `.vue`) on the SAME semantic
        // identity + recursion guard the bare-`Ref` `.vue` route uses. NOTHING
        // else changes: non-`.vue` `typeof`, userland `.vue` defaults, ordinary
        // `.ts`/`.tsx` constructors/classes/functions/enums/object-literals, and
        // generic `InstanceType<T>` all fall through to the unchanged chain
        // below.
        let synthesised_default: Option<crate::semantic_query::HashValue> =
            if root_identity.symbol_name == "default" {
                self.ctx
                    .ensure_indexed_ready_serve(root_identity.canonical_id.as_str())
                    .map(|serve| serve.indexed)
                    .and_then(|indexed| {
                        indexed
                            .shallow_state
                            .value_symbol("default")
                            .filter(|sym| sym.is_synthesised_component_default)
                            .map(|_| indexed.whole_hash)
                    })
            } else {
                None
            };
        let mut composed_partial = false;
        let node_id = if let Some(_resolved_default_whole_hash) = synthesised_default {
            let resolved_default_canonical: Arc<str> =
                Arc::from(root_identity.canonical_id.as_str());
            self.build_synthesized_vue_default_construct_object(&resolved_default_canonical, &scope)
        } else if prepared.kind == verter_semantic::analysis::type_eval::ValueDeclKind::Class {
            // Class value root — `typeof C` IS the class's STATIC surface,
            // whose owning composer is `ResolveClassSurface::Static` (own
            // statics + ctor PLUS heritage statics). Delegate through the
            // keyed query so the composed surface is addressable, memoised
            // per class identity, and fact-rooted on every heritage
            // contributor. The composer lowers the prepared value decl
            // directly (it never dispatches `TypeOf` back), so this
            // delegation cannot recurse onto itself. The slot carries the
            // EFFECTIVE (post-fallback) identity: a re-export root would
            // key, lower, and invalidate the composed surface against the
            // wrong canonical.
            let slot = self.type_slot_for(
                Arc::clone(&effective_canonical),
                Arc::clone(&effective_symbol),
            );
            let class_context =
                self.class_surface_context_for(effective_canonical.as_ref(), context.mode);
            let read = self.execute_read(SemanticQueryKey::ResolveClassSurface {
                decl_slot: slot,
                type_args: Arc::from(Vec::<SemanticNodeId>::new().into_boxed_slice()),
                side: crate::semantic_query::ClassSurfaceSide::Static,
                context: class_context,
            });
            composed_partial |= read.result_is_partial;
            match read.value {
                QueryResult::Value(id) => id,
                _ => return (QueryResult::Error(QueryError::Miss), empty_signature()).into(),
            }
        } else if let Some(ty_ann) = prepared.type_annotation.as_ref() {
            self.shallow_lower_type_expr_with_context(
                ty_ann,
                &empty_env,
                &scope,
                &prepared.name_resolution,
                scope_payload.as_ref(),
                &shadowing,
                &mut substitutions,
                context,
            )
        } else if let Some(shape) = prepared.object_shape.as_ref() {
            self.shallow_lower_type_expr_with_context(
                &TypeExpr::Object(Arc::new(shape.clone())),
                &empty_env,
                &scope,
                &prepared.name_resolution,
                scope_payload.as_ref(),
                &shadowing,
                &mut substitutions,
                context,
            )
        } else if !prepared.signatures.is_empty() {
            // Overload visibility (projection-time rule): a lone signature is
            // always visible (even if bodied); a multi-signature overload group
            // surfaces every bodiless overload in source order and HIDES the
            // trailing implementation signature. `FunctionSignature` carries
            // per-parameter spans (preserved by the clone) but no
            // whole-signature span, so the signature span stays `None` here.
            let is_class =
                prepared.kind == verter_semantic::analysis::type_eval::ValueDeclKind::Class;
            let visible: Vec<&verter_semantic::analysis::type_eval::FunctionSignature> =
                if prepared.signatures.len() == 1 {
                    prepared.signatures.iter().collect()
                } else {
                    let bodiless: Vec<_> = prepared
                        .signatures
                        .iter()
                        .filter(|sig| !sig.has_implementation_body)
                        .collect();
                    // Defensive: an overload set with no bodiless members is
                    // ill-formed TS; surface every signature rather than none.
                    if bodiless.is_empty() {
                        prepared.signatures.iter().collect()
                    } else {
                        bodiless
                    }
                };
            let properties = visible
                .into_iter()
                .map(|sig| {
                    let function_expr = FunctionExpr::synthetic(
                        sig.parameters.clone(),
                        sig.return_type.clone().map(Arc::new),
                        sig.type_parameters.clone(),
                    );
                    if is_class {
                        ObjectMember::ConstructSignature(function_expr)
                    } else {
                        ObjectMember::CallSignature(function_expr)
                    }
                })
                .collect();
            let object_expr = ObjectExpr { properties };
            self.shallow_lower_type_expr_with_context(
                &TypeExpr::Object(Arc::new(object_expr)),
                &empty_env,
                &scope,
                &prepared.name_resolution,
                scope_payload.as_ref(),
                &shadowing,
                &mut substitutions,
                context,
            )
        } else if let Some(members) = prepared.enum_members.as_ref() {
            let object_expr = ObjectExpr {
                properties: members
                    .iter()
                    .map(|(name, value)| {
                        // One synthetic property per member — EVERY member, not
                        // just the foldable subset. A foldable member carries its
                        // literal; a deferred member its degraded sound primitive
                        // (`EnumMemberValue::projected_type`), so `keyof typeof
                        // Enum` surfaces every declared name. The prepared enum
                        // member inventory carries no per-member source span.
                        ObjectMember::Property(ObjectProperty::synthetic_public(
                            name.clone(),
                            value.projected_type().clone(),
                            false,
                            true,
                        ))
                    })
                    .collect(),
            };
            self.shallow_lower_type_expr_with_context(
                &TypeExpr::Object(Arc::new(object_expr)),
                &empty_env,
                &scope,
                &prepared.name_resolution,
                scope_payload.as_ref(),
                &shadowing,
                &mut substitutions,
                context,
            )
        } else {
            return (QueryResult::Error(QueryError::Miss), empty_signature()).into();
        };
        let signature = self.dep_signature_for(&value_root.scope.canonical_id, observed_hash);
        let mut output = crate::project_semantic_dispatch::walk::QueryBuildOutput::from((
            QueryResult::Value(node_id),
            signature,
        ))
        .with_observed_self_roots([(Arc::clone(&value_root.scope.canonical_id), observed_hash)]);
        // Two-signal fold: a partial composed class-surface read surfaces as
        // a partial `TypeOf` result (it would otherwise pass through with
        // `result_is_partial = false`).
        output.result_is_partial |= composed_partial;
        output
    }

    /// The local value name a dependency module's CommonJS `export = X`
    /// assigns the whole module to, read from the dependency's shallow export
    /// inventory. `None` for an ordinary ESM module (or an unloadable dep).
    fn import_export_assignment_target(&self, dep_canonical: &str) -> Option<Arc<str>> {
        self.ctx
            .shallow_file_state(dep_canonical)?
            .export_assignment_target()
            .map(Arc::<str>::from)
    }

    /// Build the VALUE-export namespace object for `typeof import("./m")`.
    ///
    /// The produced [`SemanticNodeData::Object`] surface carries one member
    /// per VALUE export of the dependency module — each member's type
    /// resolved through the SHARED `TypeOf` dispatch (`build_typeof`) in the
    /// dependency's own scope, the SAME `effective_prepared_value_decl →
    /// resolve_value_export_target` rail every other value-root consumer uses
    /// (no forked resolver). A TYPE-only export (`interface`, `type`) resolves
    /// to no value decl, so its per-export `TypeOf` misses and the name is
    /// naturally EXCLUDED from the value namespace — matching TS, where
    /// `typeof import("…")` surfaces only the runtime value bindings.
    ///
    /// An ambient `export = X` module reduces instead to the type of the
    /// export-assignment target `X` (the CommonJS value-namespace identity):
    /// the whole module IS that single value, not an object wrapping it.
    ///
    /// Member order is the lexicographically-sorted export-name order so the
    /// interned surface is deterministic across the non-deterministic
    /// `exports` map iteration order.
    ///
    /// Read-set NOTE (lead-architect ruled, shared with the cross-file
    /// value-export rail — see the Enums block's identical carry-forward): the
    /// per-export `TypeOf` chase resolves each visible leaf correctly but does
    /// not bubble every resolved leaf decl into the consuming query's
    /// read-set / reverse index. That is a PRE-EXISTING property of the shared
    /// cross-file value-export rail (identical to bare `typeof E`), NOT an
    /// import-type-local defect; the clean fix is system-wide read-set / fact
    /// bubbling, deferred with the rest of that work.
    pub(super) fn build_import_value_namespace(
        &self,
        dep_canonical: &str,
        context: crate::semantic_query::ProjectionReductionContext,
    ) -> SemanticNodeId {
        let Some(indexed) = self
            .ctx
            .ensure_indexed_ready_serve(dep_canonical)
            .map(|serve| serve.indexed)
        else {
            return self.opaque(QueryError::Miss);
        };
        let dep_scope = NodeScopeId::File {
            canonical_id: Arc::from(dep_canonical),
            whole_hash: indexed.whole_hash,
            local_scope: None,
        };

        // Ambient `export = X`: the value namespace IS `typeof X` (the
        // CommonJS whole-module value). Resolved through the SAME `TypeOf`
        // rail as a named export.
        if let Some(assign_target) = self.import_export_assignment_target(dep_canonical) {
            return match self
                .execute_read(self.typeof_key_for(
                    ValueRootKey {
                        scope: crate::semantic_query::ScopeId {
                            canonical_id: Arc::from(dep_canonical),
                            local_scope: None,
                        },
                        name: assign_target,
                    },
                    context,
                ))
                .value
            {
                QueryResult::Value(id) => id,
                _ => self.opaque(QueryError::Miss),
            };
        }

        // ESM module: object of value exports, lex-sorted for determinism.
        let mut export_names: Vec<Arc<str>> = indexed
            .shallow_state
            .exports
            .keys()
            .map(|name| Arc::<str>::from(name.as_str()))
            .collect();
        export_names.sort();

        let mut members: Vec<SurfaceMember> = Vec::new();
        for name in &export_names {
            let node = match self
                .execute_read(self.typeof_key_for(
                    ValueRootKey {
                        scope: crate::semantic_query::ScopeId {
                            canonical_id: Arc::from(dep_canonical),
                            local_scope: None,
                        },
                        name: Arc::clone(name),
                    },
                    context,
                ))
                .value
            {
                QueryResult::Value(id) => id,
                // A TYPE-only export (no value decl) misses the value rail —
                // it is genuinely absent from the value namespace.
                _ => continue,
            };
            // Defensive: an opaque sentinel is not a real value-export type.
            if matches!(
                self.graph().node_data(node).as_deref(),
                Some(SemanticNodeData::Opaque(_)) | None
            ) {
                continue;
            }
            members.push(SurfaceMember {
                name: Arc::clone(name),
                value: node,
                optional: false,
                readonly: false,
                is_method: false,
                visibility: verter_type_expr::MemberVisibility::Public,
                // Synthesised namespace member — no single source decl site;
                // its declaration lives in the dependency module.
                spans: verter_type_expr::MemberSpans::default(),
                declaration_origin: Some(Arc::from(dep_canonical)),
                declared_in_macro_type_arg: crate::semantic_query::MacroOwnBodyStamp::NEUTRAL,
                merge_role: crate::semantic_query::MergeRoleStamp::NEUTRAL,
            });
        }

        self.graph().intern_node_with_scope(
            SemanticNodeData::Object(SurfaceView {
                members: Arc::from(members.into_boxed_slice()),
                call_signatures: Arc::from(Vec::<SemanticNodeId>::new().into_boxed_slice()),
                construct_signatures: Arc::from(Vec::<SemanticNodeId>::new().into_boxed_slice()),
                index_signatures: Arc::from(Vec::<IndexSignature>::new().into_boxed_slice()),
                keyspace: None,
                has_index_signature: false,
            }),
            dep_scope,
        )
    }

    /// Build a `.vue` SFC's synthesized `default` PUBLIC INSTANCE surface for
    /// `Instantiate{ .vue, "default", [] }`.
    ///
    /// The `.vue`'s `IndexedReady` carries a synthesized `default` VALUE symbol
    /// (`resolver_core::vue_default_synth`) whose construct-signature return type
    /// IS the instance object `{ $props, $emit, $slots }`. This lowers that
    /// instance object through the SHARED lowering pipeline (no second resolver)
    /// in the `.vue`'s scope, returning a normal
    /// [`SemanticNodeData::Object`]`(SurfaceView)` so `ProjectPath` / walkers
    /// navigate `$props` / `$emit` / `$slots`.
    ///
    /// Returns `None` when the canonical carries no synthesized `default` with an
    /// instance shape (a `.vue` with no type-based macros), letting
    /// `build_instantiate` fall through to its ordinary `resolve_prepared_type_decl`
    /// miss handling.
    ///
    /// Termination is by QUERY IDENTITY, not a depth bound. A circular `.vue`
    /// import is bounded by one of THREE outcomes depending on HOW the cycle
    /// re-enters this identity — do not over-claim that every cycle reaches the
    /// active-instantiation guard:
    ///
    /// - **lazy bare-`Ref` / mutual cross-file cycle** (the COMMON shape — e.g.
    ///   `defineProps<{ peer: Other }>()` with a reciprocal `E ↔ F`): the inner
    ///   cyclic reference lowers in `Navigate` to a shallow `DeclRef` carrier
    ///   (`Ref { name: "default" }`) instead of re-dispatching `Instantiate`, and
    ///   each `Instantiate(.vue default)` side completes and pops before the next
    ///   is demanded — so the SAME `(decl_canonical, "default")` frame is NEVER
    ///   active at the back-edge. The result is a bounded SHALLOW `Object`, NOT a
    ///   `RecursiveRef`. This branch's `push_instantiate_active` is paired
    ///   correctly but is not the bound for this shape.
    /// - **memo same-key `Instantiate` sentinel** (`semantic_query_memo`): when a
    ///   re-entry dispatches the SAME `Instantiate{ .vue default, [], context }`
    ///   KEY (identical context) that is already in flight, the memo's same-key
    ///   recursion sentinel returns `Opaque(RecursiveRef)`.
    /// - **`push_instantiate_active` / `is_instantiate_active` guard** (below):
    ///   when the instance shape re-references this same `(decl_canonical,
    ///   "default")` identity EAGERLY while this frame is still on the stack — the
    ///   `InstanceType<typeof Self>` same-file self-cycle projected
    ///   `Published(Expanded)`, where `typeof Self` routes through
    ///   `build_synthesized_vue_default_construct_object` back into
    ///   `Instantiate(.vue default)` under a DIFFERENT context key (so the memo
    ///   sentinel does not fire) — the active-instantiation guard short-circuits
    ///   to `Opaque(RecursiveRef)` before recursing.
    ///
    /// None of the three is a depth bound.
    fn build_vue_default_instance(
        &self,
        decl_canonical: &Arc<str>,
        decl_whole_hash: crate::semantic_query::HashValue,
        scope: &NodeScopeId,
        context: crate::semantic_query::ProjectionReductionContext,
    ) -> Option<crate::project_semantic_dispatch::walk::QueryBuildOutput> {
        // The synthesized `default` value symbol's construct-signature return
        // type is the instance object. Read it ONCE from the observed
        // `IndexedReady` (the same artifact that roots the memo entry below).
        let indexed = self
            .ctx
            .ensure_indexed_ready_serve(decl_canonical.as_ref())?
            .indexed;
        let default_symbol = indexed.shallow_state.value_symbol("default")?;
        // PROVENANCE gate (prefer-direct-structural-facts-over-heuristics): only
        // the SYNTHESIZED `.vue` public-instance `default` symbol drives this
        // branch. A USERLAND `export default` in a `.vue`'s `<script>` (synthesis
        // skipped, userland default present) carries `is_synthesised_component_default
        // == false` — even when its value type superficially looks like an
        // instance (`(): { $props: ... } => ...`) — so it falls through to the
        // ordinary prepared-decl path instead of being mistreated as the public
        // instance.
        if !default_symbol.is_synthesised_component_default {
            return None;
        }
        let default_body = indexed.shallow_state.value_decl("default")?;
        let instance_shape: TypeExpr = default_body
            .signatures
            .first()?
            .return_type
            .as_ref()?
            .clone();
        let observed_hash = indexed.whole_hash;

        // Body-lowering context for the `.vue` scope — mirrors the prepared-decl
        // path's scope-payload + shadowing capture so a synthesized instance
        // member that REFERENCES an imported `.vue` (`InstanceType<typeof Foo>`)
        // resolves through the SAME bare-name / import resolution every decl body
        // uses.
        let scope_payload = self
            .ctx
            .prepared_decl_bundle(decl_canonical.as_ref())
            .map(|bundle| {
                crate::resolver_core::bare_name_resolve::DeclarationScopePayload::from_bundle(
                    &bundle,
                )
            });
        let shadowing = crate::resolver_core::scope_shadowing::ScopeShadowing::from_scope_payload(
            scope_payload.as_ref(),
        );
        let env: FxHashMap<String, SemanticNodeId> = FxHashMap::default();
        let name_resolution: FxHashMap<String, ResolvedRootIdentity> = FxHashMap::default();
        let mut substitutions: Vec<(Arc<str>, SemanticNodeId)> = Vec::new();

        // Query-identity recursion control: push `(decl_canonical, "default")`
        // before lowering. A circular `.vue` import whose instance shape
        // transitively re-references this same identity sees the active entry in
        // `shallow_lower_type_expr` and emits `Opaque(RecursiveRef)` at the
        // back-edge — bounded, no hang.
        let active_identity: super::InstantiateIdentity =
            (Arc::clone(decl_canonical), Arc::from("default"));
        let pushed = self.push_instantiate_active(active_identity);
        if !pushed {
            return Some(
                (
                    QueryResult::Value(self.opaque(QueryError::RecursiveRef {
                        name: Arc::from("default"),
                    })),
                    empty_signature(),
                )
                    .into(),
            );
        }

        // Lower the instance object through the shared pipeline. An inline
        // `ObjectExpr` lowers directly to `SemanticNodeData::Object(SurfaceView)`,
        // so the result is a normal object the walkers navigate.
        let result = self.shallow_lower_type_expr_with_context(
            &instance_shape,
            &env,
            scope,
            &name_resolution,
            scope_payload.as_ref(),
            &shadowing,
            &mut substitutions,
            context,
        );
        self.pop_instantiate_active();

        // Origin edge + dep signature, mirroring `build_instantiate`'s shell. The
        // instance object is synthesized from the `.vue`'s macro type arguments,
        // so the result depends on the `.vue`'s content version — root the memo
        // entry on `(decl_canonical, observed_hash)`.
        self.graph().record_instantiate();
        let fence = self.dep_signature_for(decl_canonical, observed_hash);
        let base = self
            .graph()
            .intern_node_with_scope(SemanticNodeData::Opaque(QueryError::Miss), scope.clone());
        self.graph().record_origin_edge(
            result,
            OriginEdgeKind::Instantiate,
            Arc::from(vec![base].into_boxed_slice()),
            OriginMeta::None,
            Arc::clone(&fence),
        );
        for (param_name, arg_id) in substitutions {
            self.graph().record_origin_edge(
                result,
                OriginEdgeKind::SubstituteTypeParam,
                Arc::from(vec![arg_id].into_boxed_slice()),
                OriginMeta::SubstitutedParam(param_name),
                Arc::clone(&fence),
            );
        }

        // `decl_whole_hash` (re-sourced live at value-compute from
        // `ensure_indexed_ready_serve(base.defining_canonical)`'s serve
        // carrier `indexed.whole_hash` — the
        // content-free `Instantiate` key carries no version) and
        // `observed_hash` (the artifact just read) agree under a stable
        // content generation; root on the observed hash, which is the basis the
        // instance shape was actually read from.
        let _ = decl_whole_hash;
        Some(
            crate::project_semantic_dispatch::walk::QueryBuildOutput::from((
                QueryResult::Value(result),
                fence,
            ))
            .with_observed_self_roots([(Arc::clone(decl_canonical), observed_hash)]),
        )
    }

    /// `build_typeof` convergence for a synthesized `.vue`/typeinfo-scratch
    /// `default`: build the constructor-like value type (an Object carrying a
    /// single construct signature) whose construct-signature RETURN node is
    /// produced by the keyed `Instantiate{ .vue, "default", [] }` query — NOT by
    /// directly re-lowering the synthesized default's first signature
    /// `return_type`.
    ///
    /// `typeof Foo` (Foo a synthesized `.vue` default) and `InstanceType<typeof
    /// Foo>` therefore share ONE semantic identity for the instance shape: the
    /// `.vue default` instance object. `InstanceType` stays generic — it extracts
    /// this construct signature's return as usual — so there is no second
    /// `.vue`-aware production site to diverge in cache identity or recursion.
    /// Cyclic `InstanceType<typeof …>` re-entry on this shared identity is
    /// bounded by the `Instantiate(.vue default)` recursion machinery: a same-key
    /// in-flight re-entry hits the memo's same-key sentinel, and an EAGER
    /// re-entry of the SAME `(canonical, "default")` identity while the outer
    /// frame is still active (the `InstanceType<typeof Self>` self-cycle under
    /// `Published(Expanded)`) hits `push_instantiate_active` — both yielding
    /// `Opaque(RecursiveRef)`. (The lazy bare-`Ref` MUTUAL route is bounded
    /// differently — a shallow `DeclRef` carrier, not `RecursiveRef`; see
    /// [`Self::build_vue_default_instance`].)
    ///
    /// The synthesized default's construct signature takes no parameters and no
    /// type parameters (`vue_default_synth`), so the `Function` node is composed
    /// directly with an empty parameter list and the instance node as its return
    /// — faithful to what the ordinary lowering of that empty-param construct
    /// signature would intern, but with the return rooted on the shared query.
    fn build_synthesized_vue_default_construct_object(
        &self,
        resolved_default_canonical: &Arc<str>,
        scope: &NodeScopeId,
    ) -> SemanticNodeId {
        // The instance shape is the keyed `Instantiate(.vue default)` result —
        // the SOLE semantic identity for the `.vue`'s public instance. Navigate /
        // structural-transit keeps the instance members shallow (the consumer
        // drives any deeper projection), matching `resolve_vue_public_type`'s
        // own intermediate-hop demand.
        let instance_read = self.execute_read(SemanticQueryKey::Instantiate(
            crate::semantic_query::InstantiateKey::new(
                self.type_slot_for(Arc::clone(resolved_default_canonical), Arc::from("default")),
                Arc::from(Vec::new().into_boxed_slice()),
                self.instantiate_context_for(
                    resolved_default_canonical,
                    crate::semantic_query::ProjectionReductionContext::structural_transit_with_mode(
                        ProjectionMode::Navigate,
                    ),
                ),
            ),
        ));
        // Two-signal fold: the `.vue` instance synthesis helper returns a
        // bare node, so a genuinely-incomplete nested `Instantiate` (budget /
        // recursion / walker-fatal) folds onto the request's sticky partial
        // flag — the component-meta / materialize warm gates consult it and
        // refuse the partial-tainted result. (`.vue` import is the hardest
        // macro-traversal case; this is exactly where a leaked partial
        // would warm a poisoned result.)
        crate::request_context::observe_component_meta_read_suppress(&instance_read);
        let instance_return = match instance_read.value {
            QueryResult::Value(node) => node,
            QueryResult::Recursive(node) => node,
            // A `.vue` whose synthesized instance shape could not be produced
            // (e.g. mid-flight recursion supersession) yields the opaque miss as
            // the construct return — the value type stays a well-formed
            // constructor object, the instance is just unresolved.
            QueryResult::Error(err) => self.opaque(err),
        };

        // Construct signature `new (): <instance>`: empty params + empty type
        // params (the synthesized default takes neither), no source spans.
        let ctor_fn = self.graph().intern_node_with_scope(
            SemanticNodeData::Function {
                params: Arc::from(Vec::new().into_boxed_slice()),
                return_type: instance_return,
                type_parameters: Arc::from(Vec::new().into_boxed_slice()),
                signature_span: None,
                return_type_span: None,
            },
            scope.clone(),
        );

        // Constructor-like value type: an Object carrying exactly that one
        // construct signature — the same shape `typeof <class>` produces, so
        // `InstanceType<typeof Foo>` extracts the construct return generically.
        let view = SurfaceView {
            members: Arc::from(Vec::new().into_boxed_slice()),
            call_signatures: Arc::from(Vec::new().into_boxed_slice()),
            construct_signatures: Arc::from(vec![ctor_fn].into_boxed_slice()),
            index_signatures: Arc::from(Vec::new().into_boxed_slice()),
            keyspace: None,
            has_index_signature: false,
        };
        self.graph()
            .intern_node_with_scope(SemanticNodeData::Object(view), scope.clone())
    }

    /// Class instance / static surface (the dual-space model).
    ///
    /// Realises the class dual-space algorithm through the ONE shared
    /// typed-IR dispatch — there is NO query-time OXC, NO per-surface
    /// walker, NO re-parse:
    ///
    /// - `Instance` → instantiate the TYPE-space half under `Shallow`
    ///   (`execute(Instantiate { base: type_slot, args, published(Shallow) })`).
    /// - `Static` → `TypeOf` the VALUE-space half's constructor value
    ///   (`execute(TypeOf { value_root: value_root_of(value_slot) })`).
    ///
    /// The `side` selects the half; the slot's `symbol_space` does not feed
    /// the composed sub-query, because both the env-bearing
    /// [`ResolvedDeclSlotIdentity`](crate::semantic_query::ResolvedDeclSlotIdentity)
    /// base and
    /// [`value_root_of`](crate::semantic_query::value_root_of) are
    /// symbol-space-agnostic (they read only `(defining_canonical,
    /// merged_symbol_name)`).
    ///
    /// The post-query projection is THIN / non-owning: the build returns
    /// the composed sub-query node directly (identity projection) and does
    /// NOT walk heritage and does NOT eagerly materialise members. A deep
    /// instance/static surface projection (heritage descent, member-demand
    /// materialisation) is not part of this producer.
    ///
    /// Self-rooting: the composed surface roots on the class
    /// declaration's own file-content version (re-sourced live from the
    /// indexed view, R6/R20), so a same-canonical edit misses the warm
    /// read. The cross-file dependency facts of the composed sub-query
    /// fan out to this build's fact tracer automatically. When the decl's
    /// live content version cannot be observed (the file is unknown to
    /// the live view) the build is marked non-cacheable (`cache_suppress`)
    /// — the value still flows to the caller, the memo refuses admission.
    pub(super) fn build_class_surface(
        &self,
        decl_slot: &crate::semantic_query::ResolvedDeclSlotIdentity,
        type_args: &Arc<[SemanticNodeId]>,
        side: crate::semantic_query::ClassSurfaceSide,
        context: crate::semantic_query::ClassSurfaceContext,
    ) -> crate::project_semantic_dispatch::walk::QueryBuildOutput {
        use crate::semantic_query::{ClassSurfaceSide, ProjectionReductionContext};

        // Self-root the composed surface on the class declaration's own
        // live content version (R6/R20 — the key is content-free).
        let defining_canonical = &decl_slot.defining_canonical;
        let observed = self
            .ctx
            .ensure_indexed_ready_serve(defining_canonical.as_ref())
            .map(|serve| serve.indexed);
        let observed_self_roots: Vec<crate::semantic_query_memo::ObservedGraphSelfRoot> =
            match &observed {
                Some(indexed) => vec![(Arc::clone(defining_canonical), indexed.whole_hash)],
                None => Vec::new(),
            };

        // Dual-space — both sides route through `execute` (the ONE shared
        // engine). No query-time OXC. The env-bearing `ResolvedDeclSlotIdentity`
        // base / `value_root_of` read only
        // `(defining_canonical, merged_symbol_name)`, so the slot's
        // `symbol_space` is not re-tagged here — `side` is the selector.
        let composed = match side {
            ClassSurfaceSide::Instance => {
                // Instance side = the TYPE-space half: instantiate under
                // Shallow. The class slot is already the env-bearing
                // type-space slot; canonicalize its symbol space to `Type`
                // for the `Instantiate` base (the dual-space selector is
                // `side`, not the slot's space).
                let base =
                    decl_slot.with_symbol_space(crate::semantic_query::SemanticSymbolSpace::Type);
                let inst_ctx = self.instantiate_context_for(
                    &decl_slot.defining_canonical,
                    ProjectionReductionContext::published(ProjectionMode::Shallow),
                );
                self.execute_read(SemanticQueryKey::Instantiate(
                    crate::semantic_query::InstantiateKey::new(
                        base,
                        Arc::clone(type_args),
                        inst_ctx,
                    ),
                ))
            }
            ClassSurfaceSide::Static => {
                // Static side — the OWNING composer. Own statics + own
                // ctor come from the prepared VALUE decl's constructor
                // shape (lowered directly — never a `TypeOf` re-dispatch,
                // which would recurse: `build_typeof` delegates class
                // values HERE). Base statics compose recursively through
                // the SAME keyed query via the type-side heritage refs on
                // the sibling slot's `PreparedTypeDecl.body` (Intersection
                // fold, base-first), with shadow precedence: own members
                // shadow base members; an own DECLARED ctor replaces the
                // base's; a ctor-LESS subclass inherits the base ctor's
                // parameters with the DERIVED instance return. Heritage
                // type-arguments (`extends Base<string>`) lower in THIS
                // class's defining scope and ride the recursive base key's
                // `type_args` — instantiated surfaces are semantic meaning,
                // so the substitution is part of the base query identity.
                // The key's own `type_args` specialize THIS class's type
                // parameters across the composed surface: statics cannot
                // legally reference class type parameters (TS2302), so the
                // substitution reaches only the constructor signatures the
                // shells survive into (a static method's OWN same-name
                // generic is protected by the shadow-aware collection).
                // `prototype` is synthesized at projection time (the
                // walker's member hop), never stored here.
                //
                // Effective post-fallback identity (the ONE shared
                // export-target fallback rail — `build_typeof` keys NEW
                // slots with it already): a slot that still names a
                // re-exporting canonical rebases HERE, so the constructor
                // shape lowers in the DECLARING file's scope, heritage
                // reads the declaring type-side sibling decl, and the
                // entry self-roots on the declaring content version.
                let mut observed_self_roots = observed_self_roots;
                let effective = self.effective_prepared_value_decl(
                    defining_canonical.as_ref(),
                    decl_slot.merged_symbol_name.as_ref(),
                );
                let Some((own_canonical, own_symbol, prepared)) = effective else {
                    let mut output: crate::project_semantic_dispatch::walk::QueryBuildOutput =
                        (QueryResult::Error(QueryError::Miss), empty_signature()).into();
                    if observed.is_none() {
                        output.cache_suppress = true;
                    }
                    return output.with_observed_self_roots(observed_self_roots);
                };
                let mut effective_root_missing = false;
                if own_canonical.as_ref() != defining_canonical.as_ref() {
                    // Self-root derivation observes the declaring file's
                    // whole hash; fenced-ness flows via the chokepoint flag
                    // into the dispatch executor's admission gate.
                    match self
                        .ctx
                        .ensure_indexed_ready_serve(own_canonical.as_ref())
                        .map(|serve| serve.indexed)
                    {
                        Some(indexed) => observed_self_roots
                            .push((Arc::clone(&own_canonical), indexed.whole_hash)),
                        // The declaring file's content version could not be
                        // observed — refuse warm admission (the value would
                        // otherwise survive a declaring-file edit).
                        None => effective_root_missing = true,
                    }
                }
                let own = self.lower_class_constructor_object(
                    own_canonical.as_ref(),
                    own_symbol.as_ref(),
                    &prepared,
                    ProjectionReductionContext::published(context.mode),
                );
                let Some(own_node) = own else {
                    let mut output: crate::project_semantic_dispatch::walk::QueryBuildOutput =
                        (QueryResult::Error(QueryError::Miss), empty_signature()).into();
                    if observed.is_none() || effective_root_missing {
                        output.cache_suppress = true;
                    }
                    return output.with_observed_self_roots(observed_self_roots);
                };
                let mut composed_node = own_node;
                let mut composed_partial = false;
                for (base_canonical, base_name, base_args) in
                    self.class_heritage_bases(own_canonical.as_ref(), own_symbol.as_ref())
                {
                    let lowered_args: Vec<SemanticNodeId> = if base_args.is_empty() {
                        Vec::new()
                    } else {
                        self.lower_class_heritage_args(
                            own_canonical.as_ref(),
                            own_symbol.as_ref(),
                            base_args.as_ref(),
                            context.mode,
                        )
                    };
                    let base_slot =
                        self.type_slot_for(Arc::clone(&base_canonical), Arc::clone(&base_name));
                    let base_context =
                        self.class_surface_context_for(base_canonical.as_ref(), context.mode);
                    let read = self.execute_read(SemanticQueryKey::ResolveClassSurface {
                        decl_slot: base_slot,
                        type_args: Arc::from(lowered_args.into_boxed_slice()),
                        side: ClassSurfaceSide::Static,
                        context: base_context,
                    });
                    composed_partial |= read.result_is_partial;
                    if let QueryResult::Value(base_node) = read.value {
                        composed_node = self.merge_static_surfaces(composed_node, base_node);
                    }
                }
                // Specialize THIS class's own type parameters with the key's
                // `type_args` (produced by a derived class's heritage hop
                // above, or any caller instantiating the static surface).
                if !type_args.is_empty() {
                    composed_node = self.apply_class_surface_type_args(
                        own_canonical.as_ref(),
                        own_symbol.as_ref(),
                        composed_node,
                        type_args,
                    );
                }
                let mut output: crate::project_semantic_dispatch::walk::QueryBuildOutput = (
                    QueryResult::Value(composed_node),
                    self.project_generation_signature(),
                )
                    .into();
                output.result_is_partial = composed_partial;
                if observed.is_none() || effective_root_missing {
                    output.cache_suppress = true;
                }
                return output.with_observed_self_roots(observed_self_roots);
            }
        };

        // Two-signal fold: fold the composed sub-query read's partiality so
        // a budget/recursion/walker-fatal nested side surfaces as a partial
        // (it would otherwise return through the identity projection with
        // `result_is_partial=false`).
        let composed_is_partial = composed.result_is_partial;
        // Identity projection: return the composed sub-query node directly.
        let result = composed.value;
        let mut output: crate::project_semantic_dispatch::walk::QueryBuildOutput =
            (result, self.project_generation_signature()).into();
        output.result_is_partial = composed_is_partial;
        if observed.is_none() {
            // Could not self-root on the decl's live content version —
            // refuse warm admission; the value still flows to the caller.
            output.cache_suppress = true;
        }
        output.with_observed_self_roots(observed_self_roots)
    }

    /// The EFFECTIVE prepared VALUE-decl identity for a `(canonical,
    /// symbol)` root: the root itself when its prepared value decl exists
    /// locally; otherwise the value-export-target walk's declaring identity
    /// (the post-fallback canonical) — the ONE shared fallback rail for
    /// re-exported value roots (`build_typeof`, the class-surface Static
    /// composer, and the `Enum.Member` projection hook all consume it, so the
    /// rails cannot drift). Returns `None` when neither side has a prepared
    /// value decl.
    pub(super) fn effective_prepared_value_decl(
        &self,
        canonical: &str,
        symbol: &str,
    ) -> Option<(
        Arc<str>,
        Arc<str>,
        Arc<verter_semantic::analysis::type_solver::PreparedValueDecl>,
    )> {
        if let Some(prepared) = self.ctx.prepared_value_decl(canonical, symbol) {
            return Some((Arc::from(canonical), Arc::from(symbol), prepared));
        }
        if canonical.is_empty() {
            return None;
        }
        let target = self.ctx.resolve_value_export_target(canonical, symbol)?;
        if target.canonical_id == canonical && target.name == symbol {
            return None;
        }
        let prepared = self
            .ctx
            .prepared_value_decl(&target.canonical_id, &target.name)?;
        Some((
            Arc::from(target.canonical_id.as_str()),
            Arc::from(target.name.as_str()),
            prepared,
        ))
    }

    /// `ResolveOverloadSet` — the live signature-group reducer.
    ///
    /// Resolves the already-resolved `callee` node to its ordered VISIBLE
    /// signature group and returns the group-bearing node (the public
    /// `execute` boundary converts it into the
    /// `OverloadSet(Arc<[SignatureRef]>)` value domain — call bucket
    /// first, then construct). Visibility is build_typeof's projection
    /// rule, applied UPSTREAM where the callee node was produced: a lone
    /// signature is visible even if bodied; a multi-signature group
    /// carries every bodiless overload in source order with the trailing
    /// implementation already hidden — this reducer never re-derives it.
    /// The LAST element is therefore the last visible overload (the
    /// signature-utility selection rule; U6's call resolution reads the
    /// same order first-applicable).
    ///
    /// - The callee settles through the ONE shared signature-source rail
    ///   ([`Self::resolve_signature_source_carrier`]) — the same demand
    ///   point the signature utilities (`ReturnType` / `Parameters` / …)
    ///   use. The rails share the FUNCTION, not the CONTEXT: the
    ///   utilities pass their caller's context, this reducer always
    ///   passes the non-published structural transit (the key is
    ///   mode-erased). A carrier-shaped callee (`DeclRef` to an
    ///   annotation-typed overloaded interface, `InstantiationRef` to a
    ///   generic one) still settles to the same signature group on both
    ///   rails because `Instantiate` of a signature-bearing decl is
    ///   mode-stable.
    /// - `Alias` chains unwrap (cycle-guarded, mirroring
    ///   `select_signature_function`).
    /// - A callee with no signature group (no `Function`, no signature-
    ///   bearing `Object`) is an honest `Miss` — never a fabricated empty
    ///   set.
    /// - Non-empty `type_args` instantiate each candidate positionally
    ///   through the shared `apply_typeof_instantiation_args`; a candidate
    ///   that cannot accept the argument list (non-generic, unsatisfied
    ///   arity) DROPS from the set (TS overload resolution under explicit
    ///   type arguments); all-dropped is an honest `Miss`.
    ///
    /// Self-version rooting: on the file-derived origins of EVERY input
    /// node — the callee AND each explicit type argument (the node-keyed
    /// rooting rule `NormalizeUnion` uses; the produced value semantically
    /// depends on the arg nodes, so they root too).
    pub(super) fn build_resolve_overload_set(
        &self,
        callee: SemanticNodeId,
        type_args: &Arc<[SemanticNodeId]>,
    ) -> crate::project_semantic_dispatch::walk::QueryBuildOutput {
        let miss = || -> crate::project_semantic_dispatch::walk::QueryBuildOutput {
            (QueryResult::Error(QueryError::Miss), empty_signature()).into()
        };
        // Settle the callee through the ONE shared signature-source rail —
        // the same demand point the signature utilities use. The deferred
        // evaluator alone deliberately leaves `DeclRef` / `InstantiationRef`
        // carriers symbolic (the intermediate indexed-access preservation
        // carve-out), but an overload-set read IS a demand point: an
        // annotation-typed callee (`declare const x: Overloaded`) arrives as
        // a carrier and must resolve to its signature-bearing surface here,
        // exactly as it does for `ReturnType` / `Parameters` — one rail, no
        // divergence. The key is mode-erased (no projection context), so the
        // settlement runs under the non-published `StructuralTransit` context
        // (`Shallow`) — the shallow-by-default-consistent choice: the reducer
        // needs the structural signature group, never a published member
        // surface. An unsettleable carrier stays non-signature and misses
        // honestly below.
        let settled = self.resolve_signature_source_carrier(
            callee,
            crate::semantic_query::ProjectionReductionContext::structural_transit(),
        );
        // Unwrap alias chains to the signature-group-bearing node.
        let mut group_node = settled;
        let mut visited: FxHashSet<SemanticNodeId> = FxHashSet::default();
        let (call_sigs, construct_sigs): (Vec<SemanticNodeId>, Vec<SemanticNodeId>) = loop {
            if !visited.insert(group_node) {
                return miss();
            }
            let Some(data) = self.graph().node_data(group_node) else {
                return miss();
            };
            match &*data {
                SemanticNodeData::Alias(target) => {
                    let target = *target;
                    drop(data);
                    group_node = target;
                }
                SemanticNodeData::Function { .. } => break (vec![group_node], Vec::new()),
                SemanticNodeData::Object(surface) => {
                    break (
                        surface.call_signatures.to_vec(),
                        surface.construct_signatures.to_vec(),
                    )
                }
                _ => return miss(),
            }
        };
        if call_sigs.is_empty() && construct_sigs.is_empty() {
            return miss();
        }

        let result_node = if type_args.is_empty() {
            group_node
        } else {
            // Explicit type arguments: instantiate per candidate; drop the
            // candidates that cannot accept the argument list.
            let instantiate = |sigs: &[SemanticNodeId]| -> Vec<SemanticNodeId> {
                sigs.iter()
                    .filter_map(|sig| {
                        let instantiated = self.apply_typeof_instantiation_args(*sig, type_args);
                        let dropped = matches!(
                            self.graph().node_data(instantiated).as_deref(),
                            Some(SemanticNodeData::Opaque(_))
                        );
                        (!dropped).then_some(instantiated)
                    })
                    .collect()
            };
            let instantiated_calls = instantiate(&call_sigs);
            let instantiated_constructs = instantiate(&construct_sigs);
            match (
                instantiated_calls.as_slice(),
                instantiated_constructs.as_slice(),
            ) {
                ([], []) => return miss(),
                // A lone instantiated signature IS the group node — no
                // synthetic surface wrapper for the single-candidate case.
                ([lone], []) | ([], [lone]) => *lone,
                _ => self
                    .graph()
                    .intern_node(SemanticNodeData::Object(SurfaceView {
                        members: Arc::from(Vec::new().into_boxed_slice()),
                        call_signatures: Arc::from(instantiated_calls.into_boxed_slice()),
                        construct_signatures: Arc::from(instantiated_constructs.into_boxed_slice()),
                        index_signatures: Arc::from(Vec::new().into_boxed_slice()),
                        keyspace: None,
                        has_index_signature: false,
                    })),
            }
        };

        let observed_self_roots = self.observed_self_roots_from_nodes(
            std::iter::once(callee).chain(type_args.iter().copied()),
        );
        crate::project_semantic_dispatch::walk::QueryBuildOutput::from((
            QueryResult::Value(result_node),
            self.project_generation_signature(),
        ))
        .with_observed_self_roots(observed_self_roots)
    }

    /// Lower the class's OWN constructor-object surface — the prepared
    /// VALUE decl's `object_shape` (construct signature + own statics with
    /// visibility) — in the DECLARING file's scope. The own half of the
    /// `ResolveClassSurface::Static` composer; heritage is composed by the
    /// caller, which also resolves the effective post-fallback identity
    /// (`canonical`/`symbol`/`prepared` arrive already rebased — this
    /// helper performs NO export-target walk of its own). Returns `None`
    /// when the declaring file cannot be indexed or the prepared decl
    /// carries no constructor shape.
    fn lower_class_constructor_object(
        &self,
        canonical: &str,
        symbol: &str,
        prepared: &verter_semantic::analysis::type_solver::PreparedValueDecl,
        context: crate::semantic_query::ProjectionReductionContext,
    ) -> Option<SemanticNodeId> {
        let indexed = self.ctx.ensure_indexed_ready_serve(canonical)?.indexed;
        let scope = NodeScopeId::File {
            canonical_id: Arc::from(canonical),
            whole_hash: indexed.whole_hash,
            local_scope: None,
        };
        let scope_payload = self.ctx.prepared_decl_bundle(canonical).map(|bundle| {
            crate::resolver_core::bare_name_resolve::DeclarationScopePayload::from_bundle(&bundle)
        });
        let shadowing = crate::resolver_core::scope_shadowing::ScopeShadowing::from_scope_payload(
            scope_payload.as_ref(),
        );
        let shape = prepared.object_shape.as_ref()?;
        // Bind the class's OWN type parameters as `TypeParam` shells so a
        // declared `constructor(x: T)` lowers `T` to the substitutable
        // binder node (the heritage hop specializes it through the key's
        // `type_args`). Statics cannot legally reference class type
        // parameters, so the shells surface only through the ctor.
        let env = self.class_type_param_shell_env(canonical, symbol, indexed.whole_hash, &scope);
        let mut substitutions = Vec::new();
        Some(self.shallow_lower_type_expr_with_context(
            &TypeExpr::Object(Arc::new(shape.clone())),
            &env,
            &scope,
            &prepared.name_resolution,
            scope_payload.as_ref(),
            &shadowing,
            &mut substitutions,
            context,
        ))
    }

    /// Interned `TypeParam` shell nodes for a class declaration's own type
    /// parameters, env-keyed by parameter name. Bound during value-side
    /// constructor-shape lowering and heritage-argument lowering so the
    /// class-level binders stay substitutable (mirrors `build_instantiate`'s
    /// Skeleton shell binding). A class with no type-side sibling decl (or
    /// no parameters) yields an empty env.
    fn class_type_param_shell_env(
        &self,
        canonical: &str,
        symbol: &str,
        whole_hash: crate::semantic_query::HashValue,
        scope: &NodeScopeId,
    ) -> FxHashMap<String, SemanticNodeId> {
        let mut env: FxHashMap<String, SemanticNodeId> = FxHashMap::default();
        let Some(type_decl) = self.ctx.prepared_type_decl(canonical, symbol) else {
            return env;
        };
        for (index, param) in type_decl.type_parameters.iter().enumerate() {
            let shell = self.graph().intern_node_with_scope(
                SemanticNodeData::TypeParam {
                    decl: crate::semantic_query::DeclIdentity {
                        canonical_id: Arc::from(canonical),
                        whole_hash,
                        decl_name: Arc::from(symbol),
                    },
                    param_index: index as u16,
                    constraint: None,
                    default: None,
                    display_name: Arc::from(param.name.as_str()),
                },
                scope.clone(),
            );
            env.insert(param.name.clone(), shell);
        }
        env
    }

    /// Lower a heritage clause's type-arguments (`extends Base<...>`) in the
    /// DERIVED class's defining scope — the arguments are authored there, so
    /// imports resolve through the derived decl's `name_resolution` and the
    /// derived class's own type parameters bind as shells (`class Mid<U>
    /// extends Base<U>` lowers `U` to Mid's binder shell, which Mid's own
    /// `type_args` substitution later specializes).
    fn lower_class_heritage_args(
        &self,
        canonical: &str,
        symbol: &str,
        args: &[TypeExpr],
        mode: crate::semantic_query::ProjectionMode,
    ) -> Vec<SemanticNodeId> {
        let Some(indexed) = self
            .ctx
            .ensure_indexed_ready_serve(canonical)
            .map(|serve| serve.indexed)
        else {
            return Vec::new();
        };
        let Some(type_decl) = self.ctx.prepared_type_decl(canonical, symbol) else {
            return Vec::new();
        };
        let scope = NodeScopeId::File {
            canonical_id: Arc::from(canonical),
            whole_hash: indexed.whole_hash,
            local_scope: None,
        };
        let scope_payload = self.ctx.prepared_decl_bundle(canonical).map(|bundle| {
            crate::resolver_core::bare_name_resolve::DeclarationScopePayload::from_bundle(&bundle)
        });
        let shadowing = crate::resolver_core::scope_shadowing::ScopeShadowing::from_scope_payload(
            scope_payload.as_ref(),
        );
        let env = self.class_type_param_shell_env(canonical, symbol, indexed.whole_hash, &scope);
        let mut substitutions = Vec::new();
        args.iter()
            .map(|arg| {
                self.shallow_lower_type_expr_with_context(
                    arg,
                    &env,
                    &scope,
                    &type_decl.name_resolution,
                    scope_payload.as_ref(),
                    &shadowing,
                    &mut substitutions,
                    crate::semantic_query::ProjectionReductionContext::published(mode),
                )
            })
            .collect()
    }

    /// Substitute a class's OWN type parameters positionally with the
    /// `ResolveClassSurface` key's `type_args` across the composed static
    /// surface. Unfilled trailing parameters keep their open shells (the
    /// instance rail's `Instantiate` owns default settlement). Collection is
    /// shadow-aware, so a static method re-declaring a same-name generic
    /// keeps its own binder.
    fn apply_class_surface_type_args(
        &self,
        canonical: &str,
        symbol: &str,
        surface: SemanticNodeId,
        args: &Arc<[SemanticNodeId]>,
    ) -> SemanticNodeId {
        let Some(type_decl) = self.ctx.prepared_type_decl(canonical, symbol) else {
            return surface;
        };
        let mut result = surface;
        for (index, param) in type_decl.type_parameters.iter().enumerate() {
            let Some(arg) = args.get(index).copied() else {
                continue;
            };
            for binder in self.collect_type_param_nodes_by_name(
                result,
                param.name.as_str(),
                /* root_is_own_signature */ false,
            ) {
                result = self.substitute_semantic_type_param(result, binder, arg);
            }
        }
        result
    }

    /// The class's heritage bases, base-first, read from the type-side
    /// sibling slot's `PreparedTypeDecl.body` (the producer's Intersection
    /// fold puts heritage `Ref` arms before the own `Object` arm). Each
    /// base ref resolves through the prepared decl's own `name_resolution`
    /// (import-aware); an unresolved bare name falls back to the same file.
    /// The third element carries the heritage clause's type-arguments
    /// (`extends Base<string>` — preserved by the producer as
    /// `named_with_args`), still as authored `TypeExpr`s; the Static
    /// composer lowers them in the derived class's scope. Non-class decls
    /// and heritage-free classes return an empty list.
    fn class_heritage_bases(&self, canonical: &str, symbol: &str) -> Vec<HeritageBase> {
        let Some(prepared) = self.ctx.prepared_type_decl(canonical, symbol) else {
            return Vec::new();
        };
        if prepared.kind != verter_semantic::analysis::type_eval::TypeDeclKind::Class {
            return Vec::new();
        }
        let TypeExpr::Intersection(parts) = &prepared.body else {
            return Vec::new();
        };
        parts
            .iter()
            .filter_map(|part| match part {
                TypeExpr::Ref {
                    name,
                    type_arguments,
                } => match prepared.name_resolution.get(name.as_ref()) {
                    Some(root) => Some((
                        Arc::<str>::from(root.canonical_id.as_str()),
                        Arc::<str>::from(root.symbol_name.as_str()),
                        Arc::clone(type_arguments),
                    )),
                    // Same-file base not in the name-resolution map —
                    // resolve locally.
                    None => Some((
                        Arc::<str>::from(canonical),
                        Arc::clone(name),
                        Arc::clone(type_arguments),
                    )),
                },
                _ => None,
            })
            .collect()
    }

    /// Merge a class's own static surface with ONE base class's composed
    /// static surface (shadow precedence):
    ///
    /// - own members shadow base members by name (visibility carried);
    /// - an own DECLARED constructor (signature span present) replaces the
    ///   base's; a ctor-LESS class (its construct signatures are all
    ///   synthesized — no signature span) inherits the base constructor's
    ///   parameters with the DERIVED instance return type;
    /// - call/index signatures union own-first.
    ///
    /// Non-object inputs return `own` unchanged (a miss on either side
    /// never fabricates a surface).
    fn merge_static_surfaces(&self, own: SemanticNodeId, base: SemanticNodeId) -> SemanticNodeId {
        let Some(own_data) = self.graph().node_data(own) else {
            return own;
        };
        let SemanticNodeData::Object(own_view) = &*own_data else {
            return own;
        };
        let own_view = own_view.clone();
        drop(own_data);
        let Some(base_data) = self.graph().node_data(base) else {
            return own;
        };
        let SemanticNodeData::Object(base_view) = &*base_data else {
            return own;
        };
        let base_view = base_view.clone();
        drop(base_data);

        let mut members: Vec<SurfaceMember> = own_view.members.to_vec();
        for base_member in base_view.members.iter() {
            if !members
                .iter()
                .any(|own_member| own_member.name == base_member.name)
            {
                members.push(base_member.clone());
            }
        }

        let function_signature_span = |node: SemanticNodeId| -> Option<verter_span::Span> {
            match self.graph().node_data(node).as_deref() {
                Some(SemanticNodeData::Function { signature_span, .. }) => *signature_span,
                _ => None,
            }
        };
        let own_ctor_declared = own_view
            .construct_signatures
            .iter()
            .any(|sig| function_signature_span(*sig).is_some());
        let construct_signatures: Vec<SemanticNodeId> =
            if own_ctor_declared || base_view.construct_signatures.is_empty() {
                own_view.construct_signatures.to_vec()
            } else {
                // Ctor-less class: inherit the base constructor's parameters,
                // keep the DERIVED instance return (the own synthesized
                // construct signature's return IS the derived instance ref).
                let own_instance_return = own_view.construct_signatures.first().and_then(|sig| {
                    match self.graph().node_data(*sig).as_deref() {
                        Some(SemanticNodeData::Function { return_type, .. }) => Some(*return_type),
                        _ => None,
                    }
                });
                match own_instance_return {
                    Some(derived_return) => base_view
                        .construct_signatures
                        .iter()
                        .map(
                            |base_sig| match self.graph().node_data(*base_sig).as_deref() {
                                Some(SemanticNodeData::Function {
                                    params,
                                    type_parameters,
                                    ..
                                }) => self.graph().intern_node(SemanticNodeData::Function {
                                    params: Arc::clone(params),
                                    return_type: derived_return,
                                    type_parameters: Arc::clone(type_parameters),
                                    // Composed signature — no single source site.
                                    signature_span: None,
                                    return_type_span: None,
                                }),
                                _ => *base_sig,
                            },
                        )
                        .collect(),
                    None => own_view.construct_signatures.to_vec(),
                }
            };

        // Union dedup is STRUCTURAL, not id-only: `intern_node_with_scope`
        // can fork two ids for byte-identical signature data lowered under
        // different scopes, and an id-only `contains` would double-list the
        // same declaration. Distinct declarations stay distinct — their
        // source spans participate in node data, so two same-shape
        // signatures from different sites never compare equal.
        let same_node_data = |a: SemanticNodeId, b: SemanticNodeId| -> bool {
            a == b || self.graph().node_data(a) == self.graph().node_data(b)
        };
        let mut call_signatures: Vec<SemanticNodeId> = own_view.call_signatures.to_vec();
        for sig in base_view.call_signatures.iter() {
            if !call_signatures
                .iter()
                .any(|existing| same_node_data(*existing, *sig))
            {
                call_signatures.push(*sig);
            }
        }
        let mut index_signatures = own_view.index_signatures.to_vec();
        for sig in base_view.index_signatures.iter() {
            let duplicate = index_signatures.iter().any(|existing| {
                existing == sig
                    || (existing.readonly == sig.readonly
                        && existing.spans == sig.spans
                        && same_node_data(existing.key_type, sig.key_type)
                        && same_node_data(existing.value_type, sig.value_type))
            });
            if !duplicate {
                index_signatures.push(sig.clone());
            }
        }
        let has_index_signature = !index_signatures.is_empty();
        self.graph()
            .intern_node(SemanticNodeData::Object(SurfaceView {
                members: Arc::from(members.into_boxed_slice()),
                call_signatures: Arc::from(call_signatures.into_boxed_slice()),
                construct_signatures: Arc::from(construct_signatures.into_boxed_slice()),
                index_signatures: Arc::from(index_signatures.into_boxed_slice()),
                keyspace: None,
                has_index_signature,
            }))
    }

    /// Generic instantiation.
    ///
    /// Receives the content-free `ResolvedDeclSlotIdentity` slot directly
    /// from the `Instantiate` key — declaration identity is part of the
    /// key rather than a separate `DeclAnchor` node. The slot carries the
    /// env dims only; the live content version (`decl_whole_hash`) is
    /// re-sourced at value-compute from
    /// `ensure_indexed_ready_serve(base.defining_canonical)`'s serve
    /// carrier `indexed.whole_hash`, never
    /// from the key. Fetches the [`PreparedTypeDecl`] via
    /// [`DispatchHost`] and produces **one shell level** of the
    /// declaration's structural shape with `args` bound to the decl's
    /// type parameters.
    ///
    /// The `InstantiateContext`'s embedded `projection_reduction` mode
    /// controls how the decl body and its argument expressions are
    /// lowered after substitution. Memo entries split per the
    /// `InstantiateContext` (its `projection_reduction` mode plus the
    /// `resolve_env_hash` env dim; see `family_and_slot` in
    /// [`semantic_query_memo`](crate::semantic_query_memo)) so a Navigate
    /// caller and an Expanded caller never collide on the same shell
    /// result. Member bodies are not recursively lowered — nested
    /// references emit `Opaque(Miss)` placeholders per the
    /// lazy-materialisation rule; deeper lowering is driven by
    /// `ProjectPath` sub-queries through the family memo.
    ///
    /// Origin edges emitted:
    /// - One [`OriginEdgeKind::Instantiate`] edge on the result, sourced
    ///   from `[base_placeholder, args...]`.
    /// - One [`OriginEdgeKind::SubstituteTypeParam`] edge per type-parameter
    ///   reference visited at the shell level, sourced from the bound arg.
    pub(super) fn build_instantiate(
        &self,
        base: &crate::semantic_query::ResolvedDeclSlotIdentity,
        args: &Arc<[SemanticNodeId]>,
        instantiate_context: crate::semantic_query::InstantiateContext,
    ) -> crate::project_semantic_dispatch::walk::QueryBuildOutput {
        // The key carries an `InstantiateContext` (embedded
        // projection-reduction identity + the `resolve_env_hash` env dim).
        // Destructure into the embedded `ProjectionReductionContext` so the
        // rest of the lowering pipeline keeps consulting `context`
        // unchanged; `resolve_env_hash` is threaded onto any nested
        // `Instantiate` sub-key this build re-emits via
        // `instantiate_context_for`.
        let context = instantiate_context.projection_reduction();
        // demand-driven reducer spec: the call-site provides
        // the publication / structural-transit context. `body_mode` is
        // shorthand for `context.mode` everywhere the existing lowering
        // pipeline consults it; the demand axis flows through to
        // builtin-utility dispatch and nested operator builders so a
        // `StructuralTransit` instantiation never reifies `keyof` /
        // `Mapped` operators along its decl body.
        let body_mode = context.mode;
        // Count Instantiate dispatches that ask for the Expanded body
        // mode. Used by the slot-binding regression `enrich_does_not_eagerly_instantiate_carrier`
        // to enforce that synthesis stays in Navigate mode and never
        // re-enters the giant-tree pathology through the carrier walk.
        //
        // Two counters are bumped: the process-global
        // `SLOT_BINDING_EXPANDED_INSTANTIATE_CALLS` (preserves existing
        // semantics for warm-pass tests that read it without an active
        // RequestContext) AND the active request's per-request mirror
        // when a context is installed in TLS. The per-request mirror
        // surfaces on the audit payload so attribution tests can assert
        // "no synthesis-attributable Instantiate{Expanded} fired during
        // this request" without false positives from peer dispatches in
        // workspace-parallel runs.
        if matches!(body_mode, crate::semantic_query::ProjectionMode::Expanded) {
            crate::loop5_instrumentation::SLOT_BINDING_EXPANDED_INSTANTIATE_CALLS
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            if let Some(ctx) = crate::request_context::current_request_context() {
                ctx.expanded_instantiate_calls
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            }
            // Synthesis-scoped attribution: bump the
            // synthesis-attributable counter ONLY when the slot-binding
            // synthesis phase is active. The request-wide counter above
            // also counts the canonical macro-surface PRODUCER's Expanded
            // expansions (legitimate); the synthesis-scoped counter isolates
            // "an Expanded Instantiate fired INSIDE slot-binding synthesis"
            // — the eagerness defect `enrich_does_not_eagerly_instantiate_carrier`
            // gates on (must stay zero).
            crate::request_context::note_expanded_instantiate_for_synthesis_scope();
        }

        let decl_canonical = &base.defining_canonical;
        let decl_name = &base.merged_symbol_name;
        // R6 / R20: the key is content-free. Re-source the base file's
        // content version from the live indexed view at value-build
        // time, so the cached `MemoEntry` self-roots on the current
        // generation's whole_hash. Three classes of non-file base
        // exist and must NOT invent a synthetic `FileWholeHash`:
        //  - the global / structural sentinel (`canonical_id == ""`);
        //  - built-in utility carriers (`canonical_id == "__builtin__"`);
        //  - synthetic test identities (`canonical_id == "<synthetic>"`).
        // These bases root self-version through their `args` nodes
        // only (no file fact). A real-file base whose `ensure_indexed_ready_serve`
        // returns `None` (the file is unknown to the live view) is a
        // stale key — the build cannot publish a cacheable result and
        // returns `cache_suppress` below.
        let decl_canonical_str = decl_canonical.as_ref();
        let is_non_file_base = crate::semantic_query::is_non_file_base(decl_canonical_str);
        let live_indexed: Option<Arc<crate::project_type_store::IndexedReady>> = if is_non_file_base
        {
            None
        } else {
            self.ctx
                .ensure_indexed_ready_serve(decl_canonical_str)
                .map(|serve| serve.indexed)
        };
        let decl_whole_hash: crate::semantic_query::HashValue = match &live_indexed {
            Some(indexed) => indexed.whole_hash,
            None => crate::semantic_query::HashValue::default(),
        };

        // Intern a scope-carrying placeholder so DispatchHost methods
        // (utility_source, resolve_prepared_type_decl, etc.) can look
        // up the declaration scope via node_scope(base).
        let scope = NodeScopeId::File {
            canonical_id: Arc::clone(decl_canonical),
            whole_hash: decl_whole_hash,
            local_scope: None,
        };
        let base = self
            .graph()
            .intern_node_with_scope(SemanticNodeData::Opaque(QueryError::Miss), scope.clone());

        let adapter = SessionDispatchHost::new(self.ctx);

        // 1b. `.vue` synthesized `default` public-instance dispatch.
        //
        // A `.vue` SFC has no userland `default` TYPE declaration — its public
        // component type is the SYNTHESIZED instance object
        // `{ $props, $emit, $slots }` carried as the construct-signature return
        // type of a synthesized `default` VALUE symbol
        // (`resolver_core::vue_default_synth`). `Instantiate{ .vue, "default", [] }`
        // is the SOLE semantic identity for that public instance — the public
        // API (`resolve_vue_public_type`) and a `.vue`-importing-`.vue` reference
        // (`Ref("Foo")` → `DeclRef{Foo.vue, "default"}` → `Instantiate`) BOTH
        // route here, so there is exactly one query identity and one resolver.
        //
        // Without this branch the query would fall through to
        // `resolve_prepared_type_decl` below (a `.vue` has no prepared `default`
        // TYPE decl) and miss. The branch is gated on the STRUCTURAL PROVENANCE
        // flag `is_synthesised_component_default` of the resolved `default` value
        // symbol (NOT the file-classifier `is_synthesis_candidate`), so a
        // userland `export default` — in a `.ts` file OR in a `.vue`'s
        // `<script>` block — is never hijacked even when its value type looks
        // instance-shaped. It requires `args.is_empty()` (the synthesized
        // default takes no type arguments).
        //
        // Termination is by query identity (NO depth bound): the memo's
        // same-key `Instantiate` recursion sentinel (`mod.rs`) returns
        // `Opaque(RecursiveRef)` for a circular `A.vue ↔ B.vue` import, and the
        // `push_instantiate_active`/`pop` discipline below catches same-identity
        // re-entry while the instance shape is lowering.
        if decl_name.as_ref() == "default"
            && args.is_empty()
            && self
                .ctx
                .ensure_indexed_ready_serve(decl_canonical.as_ref())
                .map(|serve| serve.indexed)
                .and_then(|indexed| {
                    indexed
                        .shallow_state
                        .value_symbol("default")
                        .map(|sym| sym.is_synthesised_component_default)
                })
                .unwrap_or(false)
        {
            if let Some(output) =
                self.build_vue_default_instance(decl_canonical, decl_whole_hash, &scope, context)
            {
                return output;
            }
        }

        // 2. Built-in utility dispatch.
        // A utility name (Partial, Pick, ReturnType, etc.) that the user
        // has NOT shadowed routes through the utility-specific dispatch
        // path — producing the same shell structure + origin edges a
        // userland-equivalent alias would produce (userland-equivalence
        // rule). Shadowed names fall through to the ordinary
        // `resolve_prepared_type_decl` path.
        if matches!(
            adapter.utility_source(base, decl_name.as_ref()),
            UtilitySource::Builtin
        ) {
            // Route/mode-INDEPENDENT L1 carrier-stop (Instantiate-EXECUTION
            // entrance). An object-filter builtin (`Pick`/`Omit`) whose
            // enumeration domain (argument 0) is OPEN must NOT route into
            // `build_builtin_utility` — that helper materialises argument 0's
            // source surface, which degenerates into full cross-file generic
            // expansion of an open source (the Table.vue
            // `Omit<CoreOptions<T>, …>` structural decl-body-lowering
            // memo-cycle). Instead return the COMPLETE `InstantiationRef`
            // carrier verbatim. The shared open-domain predicate decides
            // openness (no second walker); a CLOSED domain falls through to
            // `build_builtin_utility` and materialises path-precisely.
            let builtin_identity = crate::semantic_query::DeclIdentity {
                canonical_id: Arc::clone(decl_canonical),
                whole_hash: decl_whole_hash,
                decl_name: Arc::clone(decl_name),
            };
            if crate::project_semantic_dispatch::raise::utility_enumeration_domain_is_open_or_unknown(
                self,
                &builtin_identity,
                args.as_ref(),
            ) {
                let carrier = self.graph().intern_node_with_scope(
                    SemanticNodeData::InstantiationRef {
                        base: builtin_identity,
                        args: Arc::clone(args),
                    },
                    scope.clone(),
                );
                // Mirror `build_builtin_utility`'s `Instantiate` origin edge so
                // origin-walks resolve through the carrier: sources are the
                // builtin base node + each arg.
                let fence = self.project_generation_signature();
                let mut inst_sources: Vec<SemanticNodeId> = Vec::with_capacity(args.len() + 1);
                inst_sources.push(base);
                inst_sources.extend(args.iter().copied());
                self.graph().record_origin_edge(
                    carrier,
                    OriginEdgeKind::Instantiate,
                    Arc::from(inst_sources.into_boxed_slice()),
                    OriginMeta::None,
                    Arc::clone(&fence),
                );
                // Root the carrier value on the same file-derived arg set the
                // materialised builtin would (an edit to a file-derived arg
                // rejects this entry on the strict warm-read validator).
                let observed_self_roots = self.observed_self_roots_from_nodes(args.iter().copied());
                let output: crate::project_semantic_dispatch::walk::QueryBuildOutput =
                    (QueryResult::Value(carrier), fence).into();
                return output.with_observed_self_roots(observed_self_roots);
            }

            // A built-in utility instantiation (`Pick<X, K>`, `Omit<X, K>`,
            // `ReturnType<F>`, …) inspects its argument nodes to form the
            // result — `build_builtin_utility` reads `X`'s Object surface,
            // enumerates `K`'s key space, walks `F`'s call signature, etc.
            // The result therefore transitively depends on the file
            // content each file-derived argument was lowered from. Derive
            // the self-roots from the `args` node set (the same
            // file-derived-input rooting `KeyOf` / `MappedType` /
            // `IndexedAccess` use): each `NodeScopeId::File`-scoped arg
            // contributes one `(canonical, observed_hash)` self-root so an
            // edit to that file rejects this utility memo entry on the
            // strict warm-read validator. Structural args (`Global`-scoped
            // primitives, literal-union key sets) contribute nothing.
            let observed_self_roots = self.observed_self_roots_from_nodes(args.iter().copied());
            // Builtin-utility results are NEVER macro-T own-body (by
            // design): `defineProps<Omit<Vendor, K>>()` surfaces
            // members that came from `Vendor` via the utility, none of
            // which the author wrote in the macro T body. Downgrade the
            // provenance to structural so the utility's produced members
            // report `declared_in_macro_type_arg = false`. (A carrier
            // `CarrierProps extends Omit<Vendor, K>` is the SEPARATE
            // direct-decl case: CarrierProps's own body is stamped below
            // and only its `extends`-reached members go through this
            // utility path as structural.)
            let (utility_result, utility_fence, utility_is_partial) = self.build_builtin_utility(
                base,
                decl_name.as_ref(),
                args,
                context.into_structural_provenance(),
            );
            let mut output: crate::project_semantic_dispatch::walk::QueryBuildOutput =
                (utility_result, utility_fence).into();
            // Two-signal fold: a mapper-utility surface whose
            // nested KeyOf/MappedType subquery was genuinely incomplete
            // (budget / recursion / walker-fatal) surfaces here as a
            // complete-looking `Value` shell — carry its partiality
            // through the published value so the component-meta + shape /
            // materialize warm gates refuse it. Benign non-cacheability of
            // the nested read does NOT taint this (it is `cache_suppress`
            // on the inner memo only).
            output.result_is_partial = utility_is_partial;
            return output.with_observed_self_roots(observed_self_roots);
        }

        // 3. Resolve prepared type decl via `DispatchHost` — the adapter
        // routes through the sidecar-recorded scope for `base`.
        //
        // The prepared decl is recovered from the declaration artifact
        // pinned to `(decl_canonical, decl_whole_hash)` — `decl_whole_hash`
        // was re-sourced above from the live indexed view. When the
        // prepared decl cannot be recovered the result is left
        // non-cacheable (`cache_suppress`): the value still flows to
        // the caller but the memo refuses admission rather than rooting
        // an entry on a decl identity whose artifact is gone.
        //
        // R6 non-file fork: when the base names a real file but the
        // live indexed view yields no `IndexedReady`, the key is stale
        // (file dropped or never loaded) — refuse admission so the
        // next caller cold-recomputes against the current state.
        if !is_non_file_base && live_indexed.is_none() {
            let mut out = crate::project_semantic_dispatch::walk::QueryBuildOutput::from((
                QueryResult::Value(self.opaque(QueryError::Miss)),
                empty_signature(),
            ));
            out.cache_suppress = true;
            return out;
        }
        let ri = ResolvedRootIdentity::new(decl_canonical.as_ref(), decl_name.as_ref());
        let prepared = match adapter.resolve_prepared_type_decl(base, &ri) {
            Some(p) => p,
            None => {
                let mut out = crate::project_semantic_dispatch::walk::QueryBuildOutput::from((
                    QueryResult::Value(self.opaque(QueryError::Miss)),
                    empty_signature(),
                ));
                out.cache_suppress = true;
                return out;
            }
        };

        // 4. Bind type parameters to args (positional). When a
        // parameter has no explicit arg but carries a default
        // expression, lower the default in the decl's scope and bind
        // it — mirrors the solver's `resolve_type_parameters_in_body`
        // behaviour at solve.rs:2580.
        // Instrumentation: callsite attribution for
        // `prepared_decl_bundle_warm` reads from `build_instantiate`.
        if let Some(obs) = verter_audit::current_observer() {
            obs.record_event(verter_audit::AuditEvent::PreparedDeclBundleCallsiteBuildInstantiate);
        }
        let scope_payload = self
            .ctx
            .prepared_decl_bundle(decl_canonical.as_ref())
            .map(|bundle| {
                crate::resolver_core::bare_name_resolve::DeclarationScopePayload::from_bundle(
                    &bundle,
                )
            });
        // R15/F11 — capture the scope-shadowing context
        // once for the whole `build_instantiate` body so every
        // recursive lowering observes the same shadow set.
        let shadowing = crate::resolver_core::scope_shadowing::ScopeShadowing::from_scope_payload(
            scope_payload.as_ref(),
        );
        let mut env: FxHashMap<String, SemanticNodeId> = FxHashMap::default();
        let mut substitutions: Vec<(Arc<str>, SemanticNodeId)> = Vec::new();
        for (index, param) in prepared.type_parameters.iter().enumerate() {
            let arg_id = if let Some(explicit) = args.get(index).copied() {
                explicit
            } else if let Some(default) = param.default.as_deref() {
                // Propagate the full
                // `ProjectionReductionContext` through default-param
                // lowering so a `StructuralTransit` instantiation
                // survives. The legacy `body_mode`-only wrapper at
                // [`Self::shallow_lower_type_expr`] rebuilds
                // `Published(mode)` and would clobber the demand axis.
                // Provenance downgrades to structural: a type-parameter default is a substituted value, not
                // the macro-T own body.
                self.shallow_lower_type_expr_with_context(
                    default,
                    &env,
                    &scope,
                    &prepared.name_resolution,
                    scope_payload.as_ref(),
                    &shadowing,
                    &mut substitutions,
                    context.into_structural_provenance(),
                )
            } else if body_mode == crate::semantic_query::ProjectionMode::Skeleton {
                // Skeleton mode preserves open generics.
                // Bind unbound param to a TypeParam shell so body lowering
                // produces TypeParam graph nodes (instead of resolving
                // T-refs to Opaque(Miss)). The relation engine treats
                // TypeParam as deferred → Conditional branches stay live
                // → collect_ref_identities_node walks both → recursive refs
                // through nested mapped/template-literal/conditional
                // bodies become visible to the cycle BFS.
                let display_name: Arc<str> = Arc::from(param.name.as_str());
                let decl_identity = crate::semantic_query::DeclIdentity {
                    canonical_id: Arc::clone(decl_canonical),
                    whole_hash: decl_whole_hash,
                    decl_name: Arc::clone(decl_name),
                };
                self.graph().intern_node_with_scope(
                    SemanticNodeData::TypeParam {
                        decl: decl_identity,
                        param_index: index as u16,
                        constraint: None,
                        default: None,
                        display_name,
                    },
                    scope.clone(),
                )
            } else {
                // Existing Navigate/Expanded behavior preserved: unbound
                // param means `Opaque(Miss)` propagates through the body.
                // Callers that genuinely need open-generic access must
                // explicitly request Skeleton mode.
                continue;
            };
            env.insert(param.name.clone(), arg_id);
        }

        // 5. Shallow-lower the body. Collects substitution facts for
        // origin-edge emission. `name_resolution` is the prepared
        // decl's map from bare names used inside its body to the
        // resolved declaration identities — the walker consults this
        // when it encounters `TypeExpr::Ref { name, args }` so member
        // bodies that reference other declarations produce proper
        // sub-Instantiate shells instead of opaque placeholders.
        //
        // Recursive-ref guard: push `(decl_canonical, decl_name)`
        // onto the dispatcher's `instantiate_active` stack before body
        // lowering. A nested `TypeExpr::Ref` resolving back to the same
        // identity — e.g. `type TreeNode = { children: TreeNode[] }` —
        // sees the active entry in `shallow_lower_type_expr` and emits
        // `Opaque(RecursiveRef)` at the back-edge instead of recursing.
        // When the identity is already active (should never happen for
        // top-level `build_instantiate` calls, but safely handled),
        // short-circuit to `RecursiveRef` here too.
        let active_identity: super::InstantiateIdentity =
            (Arc::clone(decl_canonical), Arc::clone(decl_name));
        let pushed = self.push_instantiate_active(active_identity);
        if !pushed {
            return (
                QueryResult::Value(self.opaque(QueryError::RecursiveRef {
                    name: Arc::clone(decl_name),
                })),
                empty_signature(),
            )
                .into();
        }
        // Propagate the full
        // `ProjectionReductionContext` through body lowering so a
        // `StructuralTransit` instantiation lowers its body in transit
        // demand. The legacy `body_mode`-only wrapper at
        // [`Self::shallow_lower_type_expr`] rebuilds `Published(mode)`
        // at lower.rs:80 and would clobber the demand axis —
        // intermediate-hop `keyof T` / `{ [K in S]: V }` operators
        // along the decl body would then reach the publication-edge
        // loops and emit the spurious member edges that the
        // ChatMessages `outputSchema|execute` leak captured.
        // Surface-provenance handling for the declaration body (by
        // design — own-body vs reference discrimination). The bit
        // is stamped only for members lowered from an INLINE object
        // literal that is the macro-T own body; members reached through a
        // REFERENCE arm decay to structural. See
        // `lower_decl_body_with_provenance` for the per-arm rule (inline
        // `Object` arms keep the caller's provenance; `Ref` arms — an
        // author intersection `A & B`'s named refs, or an interface's
        // `extends Base` heritage `Ref` — go structural).
        let mut result = self.lower_decl_body_with_provenance(
            &prepared,
            &env,
            &scope,
            scope_payload.as_ref(),
            &shadowing,
            &mut substitutions,
            context,
        );
        // Member-index overlay (carries the caller's provenance):
        // `member_index` holds the declaration's OWN-body direct members.
        // It APPENDS own members not yet on the surface (the heritage /
        // member-index split) and RE-STAMPS any surface member that
        // matches an own-body index entry. With per-arm body lowering the
        // own `Object` arm members already carry the correct bit, so the
        // re-stamp is a no-op for those; the overlay remains the authority
        // for own members appended from `member_index` and is the safety
        // net for surfaces where the own members were lowered structurally.
        result = self.backfill_member_index_surface(
            result,
            &prepared,
            &env,
            &scope,
            scope_payload.as_ref(),
            &shadowing,
            &mut substitutions,
            context,
        );

        // Cross-file declaration augmentation (`declare module "X"` /
        // `declare global` interface merging from sibling files). Fold every
        // augmenter file's contributed body into the base body through the ONE
        // `MergedDecl` peer-merge carrier — there is no second merge engine.
        // Performed while `(decl_canonical, decl_name)` is still on the
        // instantiate-active stack so a self-referential augmenter body
        // terminates at the recursive-ref back-edge instead of recursing.
        let mut augmenter_contributor_roots: Vec<AugmentationContributorRoot> = Vec::new();
        let mut augmentation_source_env_unobservable = false;
        if !is_non_file_base {
            if let Some(stitch) =
                self.stitch_module_augmentations(decl_canonical, decl_name, result, &scope, context)
            {
                result = stitch.merged;
                augmenter_contributor_roots = stitch.contributor_roots;
                augmentation_source_env_unobservable = stitch.source_env_unobservable;
            }
        }
        self.pop_instantiate_active();

        // 6. Emit origin edges + build dep signature.
        self.graph().record_instantiate();
        let fence = self.dep_signature_for(decl_canonical, decl_whole_hash);

        // Instantiate edge: result <- [base, args...].
        let mut inst_sources: Vec<SemanticNodeId> = Vec::with_capacity(args.len() + 1);
        inst_sources.push(base);
        inst_sources.extend(args.iter().copied());
        self.graph().record_origin_edge(
            result,
            OriginEdgeKind::Instantiate,
            Arc::from(inst_sources.into_boxed_slice()),
            OriginMeta::None,
            Arc::clone(&fence),
        );

        // SubstituteTypeParam edges on the shell result: one per visited
        // substituted occurrence — edges emitted at substitution
        // position; at shell level this aggregates on the result node
        // per lazy block.
        for (param_name, arg_id) in substitutions {
            self.graph().record_origin_edge(
                result,
                OriginEdgeKind::SubstituteTypeParam,
                Arc::from(vec![arg_id].into_boxed_slice()),
                OriginMeta::SubstitutedParam(param_name),
                Arc::clone(&fence),
            );
        }

        // Self-version rooting: the instantiated shell's body and member
        // structure were lowered from the prepared decl declared in
        // `decl_canonical` at the observed `decl_whole_hash` (re-sourced
        // from the live indexed view above) AND from the generic
        // `args` substituted into the decl body. The result therefore
        // transitively depends on the file content each file-derived
        // argument was lowered from — the same file-derived-input rooting
        // the built-in utility path applies. Root the memo entry on the
        // declaring file's observed content version AND on each
        // `NodeScopeId::File`-scoped arg's `(canonical, observed_hash)`
        // self-root so a content edit to the declaring file OR to any
        // argument's originating file misses the warm read. Structural
        // args (`Global`-scoped primitives) contribute nothing.
        //
        // R6 non-file fork: when the base names no file (the global /
        // structural sentinel `canonical_id == ""`, the built-in
        // utility carrier `"__builtin__"`, or the synthetic test
        // sentinel `"<synthetic>"`), there is no `FileWholeHash` to
        // root on the file — `decl_whole_hash` is the default sentinel
        // `0`. Recording `(non_file_canonical, 0)` as a self-root
        // would either be a no-op (the sentinel canonical is never
        // tracked by the live view, so strict validation would
        // optimistically accept) or — worse — would pretend the
        // builtin/global has a content version. Skip the file-side
        // self-root in those cases and rely entirely on the args
        // self-roots (the same rule the builtin-utility path applies).
        let mut observed_self_roots: Vec<(Arc<str>, crate::semantic_query::HashValue)> =
            if is_non_file_base {
                Vec::new()
            } else {
                vec![(Arc::clone(decl_canonical), decl_whole_hash)]
            };
        for arg_root in self.observed_self_roots_from_nodes(args.iter().copied()) {
            if !observed_self_roots
                .iter()
                .any(|(c, h)| *c == arg_root.0 && *h == arg_root.1)
            {
                observed_self_roots.push(arg_root);
            }
        }
        // Cross-file augmentation self-roots: each contributor root's
        // `(canonical, whole_hash)` (`self_root_canonicals = {base} ∪
        // {augmenters}`), so a content edit to ANY augmenter misses the warm
        // read. Every root entering here already carried its artifact key
        // through the fold, so its `FileSourceEnv` observation is on this
        // build's read-set by construction.
        for aug_root in augmenter_contributor_roots {
            if !observed_self_roots
                .iter()
                .any(|(c, h)| *c == aug_root.canonical && *h == aug_root.whole_hash)
            {
                observed_self_roots.push((aug_root.canonical, aug_root.whole_hash));
            }
        }
        let mut output = crate::project_semantic_dispatch::walk::QueryBuildOutput::from((
            QueryResult::Value(result),
            fence,
        ))
        .with_observed_self_roots(observed_self_roots);
        // A contributor whose source-env identity could not be observed
        // coherently routes the parent result through no-warm-admission
        // semantics: the value is served, never published warm.
        if augmentation_source_env_unobservable {
            output.cache_suppress = true;
        }
        output
    }

    /// Cross-file declaration-augmentation stitch (`declare module "./local"`
    /// relative interface merging from sibling files).
    ///
    /// Given a base declaration `(decl_canonical, decl_name)` and its
    /// already-lowered base body `base_result`, this finds every augmenter file
    /// whose `declare module "<rel>"` block resolves to `decl_canonical` and
    /// augments `decl_name`, lowers each augmenter's RETAINED inner body in the
    /// augmenter's own file context, and folds all contributions into a single
    /// [`SemanticNodeData::MergedDecl`] peer-merge carrier — the ONE
    /// declaration-merge path (no second merge engine; no source slicing —
    /// bodies come from the typed `augmentation_scopes` inventory).
    ///
    /// Returns `None` when there are no contributing augmenters (the caller
    /// keeps `base_result` unchanged). On a hit it returns the merged node and
    /// the per-augmenter `FileWholeHash` self-roots, and observes the
    /// `ModuleAugmentationIndexShape` (augmenter-set) fact plus each augmenter's
    /// `FileWholeHash` onto the active fact tracer so the cached value
    /// invalidates on an augmenter add/remove OR an augmenter content edit.
    /// It reuses the same fact rail as
    /// [`crate::resolver_core::route_db::RouteDb::get_or_compute_effective_export_set`].
    fn stitch_module_augmentations(
        &self,
        decl_canonical: &Arc<str>,
        decl_name: &Arc<str>,
        base_result: SemanticNodeId,
        base_scope: &NodeScopeId,
        context: crate::semantic_query::ProjectionReductionContext,
    ) -> Option<AugmentationStitch> {
        use crate::file_artifact_store::AugmentationTargetKind;

        // Candidate-augmenter discovery (program completeness for relative
        // augmentation): an augmenter `declare module "./base"` block lives in a
        // file that depends on the base — so the base's reverse-dependency set
        // is exactly the candidate-augmenter set. Ensure each is indexed BEFORE
        // the augmentation-index scan (which only sees loaded artifacts), so a
        // sibling augmenter pulled in via a side-effect `import "./base"` is
        // discovered rather than silently dropped. Loads are idempotent /
        // content-hash cached; the augmentation index is then built once.
        let host = self.ctx.host_for_fact_tracer_install();
        for rdep in host.workspace().reverse_deps_for(decl_canonical.as_ref()) {
            let _ = self.ctx.ensure_indexed_ready_serve(&rdep);
        }

        let target = AugmentationTargetKind::ResolvedRelativeCanonical(Arc::clone(decl_canonical));
        let AugmentationContributions {
            contributor_nodes,
            contributor_roots,
            source_env_unobservable,
        } = self.collect_augmentation_contributions(target, decl_name.as_ref(), context)?;

        // Tainted-EMPTY collection: augmenters targeted this decl but every
        // contribution was unobservable. Keep the base body UNCHANGED (no false
        // single-contributor `MergedDecl` wrapper) but propagate the no-warm
        // taint so the enclosing `instantiate_shell` sets
        // `output.cache_suppress`. `source_env_unobservable` is `true` here (the
        // collector returns `None` for a genuine no-augmentation empty).
        if contributor_nodes.is_empty() {
            return Some(AugmentationStitch {
                merged: base_result,
                contributor_roots: Vec::new(),
                source_env_unobservable,
            });
        }

        // Build the single peer-merge carrier: base contributors ∪ augmenter
        // contributions. If the base body is itself a `MergedDecl` (same-file
        // merge), flatten its contributors so the augmenter peers merge AT THE
        // SAME LEVEL — a nested `MergedDecl` contributor would be dropped by the
        // contributor splitter (`collect_merged_contributor_arms` only reads
        // Object / Intersection / Alias bodies).
        let mut all_contributors: Vec<SemanticNodeId> = match self.graph().node_data(base_result) {
            Some(data) => match data.as_ref() {
                SemanticNodeData::MergedDecl { contributors } => contributors.to_vec(),
                _ => vec![base_result],
            },
            None => vec![base_result],
        };
        all_contributors.extend(contributor_nodes);
        let merged = self.graph().intern_node_with_scope(
            SemanticNodeData::MergedDecl {
                contributors: Arc::from(all_contributors.into_boxed_slice()),
            },
            base_scope.clone(),
        );
        Some(AugmentationStitch {
            merged,
            contributor_roots,
            source_env_unobservable,
        })
    }

    /// Shared augmenter-contribution folder — the ONE cross-file
    /// declaration-merge augmenter path, used by BOTH the relative-canonical
    /// stitch ([`Self::stitch_module_augmentations`]) and the external
    /// string-literal resolution ([`Self::resolve_external_module_augmentation`]).
    ///
    /// Scans the overlay-aware augmentation index for `target`, lowers each
    /// contributing augmenter's RETAINED `decl_name` body in the augmenter's own
    /// file context (typed-IR `augmentation_scopes` inventory — never a source
    /// scan), and returns the ordered contributor nodes plus their per-augmenter
    /// `FileWholeHash` self-roots. Augmenter order is the stable
    /// `(canonical, parse_stable_hash)` key (`AugmenterSet.entries` is pre-sorted
    /// that way — deterministic, discovery-order-independent).
    ///
    /// Observes the augmenter-set fingerprint (`ModuleAugmentationIndexShape`)
    /// plus each augmenter's `FileWholeHash` onto the active fact tracer
    /// so the cached value invalidates on an augmenter
    /// add/remove/reorder OR an augmenter content edit — the same fact rail as
    /// [`crate::resolver_core::route_db::RouteDb::get_or_compute_effective_export_set`].
    /// These two facts are the sole cache-validity rail for the merged value:
    /// a member-body edit that leaves the augmenter skeleton (hence the
    /// set fingerprint) intact still moves the augmenter's `FileWholeHash`, so
    /// the per-augmenter whole-hash fact is what catches it.
    ///
    /// Returns `None` when no augmenter contributes.
    fn collect_augmentation_contributions(
        &self,
        target: crate::file_artifact_store::AugmentationTargetKind,
        decl_name: &str,
        context: crate::semantic_query::ProjectionReductionContext,
    ) -> Option<AugmentationContributions> {
        use crate::file_artifact_store::{AugmentationTargetKey, AugmentationTargetKind};
        use verter_semantic::analysis::type_eval::AugmentationScopeKind;

        let host = self.ctx.host_for_fact_tracer_install();
        let env_hashes = host.host_view_env_hashes();
        let project_identity = host.host_view_project_identity();

        // Population identity (overlay-aware augmentation index): under an
        // active session view the augmenter set is keyed under
        // `Session(overlay-set fingerprint)` and the cold scan unions the
        // session's overlay artifacts (matched by the session overlay
        // discriminator) with base; otherwise base-only. A session overlay's
        // `declare module` augmenters stay isolated from the base index. The
        // population + discriminator are derived through the shared
        // `augmentation_population_for_view` so the names stitch
        // (`RouteDb::get_or_compute_effective_export_set`) agrees on the
        // `Session(u64)` semantics.
        let (population, overlay_discriminator) =
            crate::session_view::augmentation_population_for_view(self.ctx.active_session_view());

        let key = AugmentationTargetKey {
            project_identity,
            resolve_env_hash: env_hashes.resolve_env_hash,
            lib_env_hash: env_hashes.lib_env_hash,
            population,
            target: target.clone(),
        };

        let artifact_store = self.ctx.project_type_store().indexed();
        let resolve_rel = |augmenter: &str, spec: &str| -> Option<Arc<str>> {
            self.ctx
                .resolve_type_dependency_canonical(augmenter, spec)
                .map(Arc::from)
        };
        let augmenter_set = artifact_store.ensure_augmentation_index_populated(
            &key,
            resolve_rel,
            overlay_discriminator,
        );
        if augmenter_set.entries.is_empty() {
            return None;
        }

        let mut contributor_nodes: Vec<SemanticNodeId> = Vec::new();
        // One root per contributing augmenter: the version self-root AND the
        // EXACT artifact key the contributor `LowerLocator` served from,
        // inseparable in one carrier — recorded below as paired
        // `FileWholeHash` + `FileSourceEnv` observations on the parent
        // read-set.
        let mut contributor_roots: Vec<AugmentationContributorRoot> = Vec::new();
        let mut source_env_unobservable = false;
        // Stale-key self-heals discovered below are written back into the
        // cached `AugmenterSet` after the loop so the NEXT stitch hits the
        // fast exact-key path instead of re-healing every call — the SAME
        // write-back the names stitch
        // (`RouteDb::get_or_compute_effective_export_set`) performs.
        let mut refreshed_keys: Vec<(usize, crate::file_artifact_store::FileArtifactKey)> =
            Vec::new();
        for (augmenter_idx, augmenter) in augmenter_set.entries.iter().enumerate() {
            let augmenter_canonical = augmenter.canonical();
            let Some(indexed) = self
                .ctx
                .ensure_indexed_ready_serve(augmenter_canonical.as_ref())
                .map(|serve| serve.indexed)
            else {
                // A set member the live view cannot serve is a torn state:
                // its contribution (and source-env identity) cannot be
                // observed coherently — the parent result must not warm.
                source_env_unobservable = true;
                continue;
            };
            let state = &indexed.shallow_state;

            // The addressable contribution pointers: each augmenter
            // `ModuleAugmentationFact` that targets THIS decl gives the raw
            // `declare module "<spec>"` specifier under which the typed inner
            // body is retained in `augmentation_scopes`.
            //
            // Self-heal a STALE captured `artifact_key`: a cosmetic /
            // member-body re-key of the augmenter advances its content hash
            // (draining the captured key) without moving its decl skeleton,
            // so the cached `AugmenterSet` keeps the pre-edit key. Skipping
            // the augmenter on that miss would silently drop a real
            // augmentation. `ensure_indexed_ready_serve` above already materialised
            // the augmenter's CURRENT version, so `indexed.whole_hash` is the
            // scheduler-authoritative current content hash — the SAME healing
            // path the names stitch
            // (`RouteDb::get_or_compute_effective_export_set`) uses.
            let Some((art, refreshed_key)) = artifact_store
                .augmenter_artifacts_self_healing(&augmenter.artifact_key, indexed.whole_hash)
            else {
                // Unhealable captured key: the augmenter's exact artifact
                // identity is unobservable — refuse warm admission.
                source_env_unobservable = true;
                continue;
            };
            // The EXACT key the contributor read serves from (the healed
            // current key when the captured one was stale).
            let effective_artifact_key = refreshed_key
                .clone()
                .unwrap_or_else(|| augmenter.artifact_key.clone());
            if let Some(refreshed_key) = refreshed_key {
                refreshed_keys.push((augmenter_idx, refreshed_key));
            }
            let mut matched_specs: Vec<String> = Vec::new();
            for fact in art.augmentations.iter() {
                if fact.augmented_name.as_ref() != decl_name {
                    continue;
                }
                if !crate::file_artifact_store::augmenter_matches_target(
                    fact,
                    &key,
                    augmenter_canonical.as_ref(),
                    resolve_rel,
                ) {
                    continue;
                }
                let spec = fact.specifier.as_ref().to_string();
                if !matched_specs.contains(&spec) {
                    matched_specs.push(spec);
                }
            }
            if matched_specs.is_empty() {
                continue;
            }

            let bundle = self.ctx.prepared_decl_bundle(augmenter_canonical.as_ref());
            let dep_edges = bundle.as_ref().map(|b| Arc::clone(&b.dep_edges));
            let aug_scope = NodeScopeId::File {
                canonical_id: Arc::clone(augmenter_canonical),
                whole_hash: indexed.whole_hash,
                local_scope: None,
            };
            let aug_scope_payload = bundle.as_ref().map(|bundle| {
                crate::resolver_core::bare_name_resolve::DeclarationScopePayload::from_bundle(
                    bundle,
                )
            });
            let aug_shadowing =
                crate::resolver_core::scope_shadowing::ScopeShadowing::from_scope_payload(
                    aug_scope_payload.as_ref(),
                );
            let aug_env: FxHashMap<String, SemanticNodeId> = FxHashMap::default();

            let mut any_contribution = false;
            for spec in &matched_specs {
                let aug_prepared = match
                    crate::resolver_core::prepared_decl::prepare_augmentation_type_decl_outcome(
                        augmenter_canonical.as_ref(),
                        state,
                        &AugmentationScopeKind::Module(spec.clone()),
                        decl_name,
                        dep_edges.as_deref(),
                    )
                {
                    crate::resolver_core::prepared_decl::PreparedDeclOutcome::Ready(Some(
                        prepared,
                    )) => prepared,
                    // Genuine absence: this augmenter has no contributor for the spec.
                    crate::resolver_core::prepared_decl::PreparedDeclOutcome::Ready(None) => {
                        continue
                    }
                    // A broken decl-body lease pin (the augmenter body demand
                    // ReturnOnly'd): the augmenter's source-env is UNOBSERVABLE —
                    // fold into the fold's no-warm rail so the enclosing query's
                    // `cache_suppress` is set, rather than silently dropping the
                    // contributor and warm-admitting an under-merged surface. A
                    // later demand under a live lease recovers.
                    crate::resolver_core::prepared_decl::PreparedDeclOutcome::LeaseMiss => {
                        source_env_unobservable = true;
                        continue;
                    }
                };
                let mut aug_subs: Vec<(Arc<str>, SemanticNodeId)> = Vec::new();
                // Demand the augmenter's RETAINED contribution body through
                // the augmenter's OWN `LowerLocator` (the augmentation-scoped
                // locator leaf) — never an inline lowering of another file's
                // prepared body. An augmenter contribution is never the
                // macro-T own body — downgrade provenance to structural
                // (same rule the builtin / heritage paths apply).
                let locator = verter_type_expr::locators::AuthoredBodyLocator::AugmentationBody(
                    verter_type_expr::locators::AugmentationBodyLocator {
                        anchor: verter_type_expr::locators::AuthoredAnchor {
                            canonical_id: Arc::clone(augmenter_canonical),
                            symbol: Arc::from(decl_name),
                            space: verter_type_expr::locators::LocatorSymbolSpace::Type,
                        },
                        scope: verter_type_expr::locators::AuthoredAugmentationScope::Module {
                            specifier: Arc::from(spec.as_str()),
                        },
                        // The whole augmentation contribution body (no sub-slot).
                        path: Arc::from(
                            Vec::<verter_type_expr::locators::TypeBodyPathStep>::new()
                                .into_boxed_slice(),
                        ),
                    },
                );
                let node = self.lower_located_body_with_provenance(
                    locator,
                    aug_prepared.kind,
                    &aug_prepared.type_parameters,
                    &aug_prepared.name_resolution,
                    &aug_env,
                    &aug_scope,
                    aug_scope_payload.as_ref(),
                    &aug_shadowing,
                    &mut aug_subs,
                    context.into_structural_provenance(),
                );
                contributor_nodes.push(node);
                any_contribution = true;
            }
            if any_contribution {
                // Root on the OVERLAY-AWARE content version the body was
                // actually lowered from (`indexed.whole_hash` == `aug_scope`'s
                // hash), NOT `get_whole_hash` (which can report the BASE hash
                // under a session view). A session-overlay augmenter rooted on
                // the base hash would tear: the value reflects overlay content
                // but the fact pins base content, so a BASE re-query validates
                // the session candidate and is poisoned. Rooting on the
                // overlay hash makes the base re-query miss and recompute.
                // The version root and the source-env artifact key travel as
                // ONE carrier: a contributor cannot be version-rooted without
                // its source-env identity.
                contributor_roots.push(AugmentationContributorRoot {
                    canonical: Arc::clone(augmenter_canonical),
                    whole_hash: indexed.whole_hash,
                    artifact_key: effective_artifact_key,
                });
            }
        }

        // Test-injection: taint the (successfully-collected) contribution set as
        // if one contributor's source-env identity were unobservable — the exact
        // no-warm state a torn/unhealable/unservable augmenter organically
        // produces, but with a deterministic trigger and WITHOUT emptying the
        // contribution set (so the resolved surface stays a real merged type,
        // isolating the `source_env_unobservable` fold from the unrelated
        // import-miss suppress rail). Gated
        // `#[cfg(any(test, feature = "test-support"))]` so a production build
        // has NO field and NO load on this augmentation-collection hot path.
        #[cfg(any(test, feature = "test-support"))]
        if host
            .augmentation_force_source_env_unobservable
            .load(std::sync::atomic::Ordering::Relaxed)
        {
            source_env_unobservable = true;
        }

        // Persist any healed exact keys back into the cached `AugmenterSet`.
        // The augmenter-set fingerprint folds `parse_stable_hash` (NOT
        // `content_hash`), and a stale key is healed only when the augmenter's
        // `parse_stable_hash` is unchanged — so the rebuilt set carries the
        // SAME fingerprint and the same per-entry `parse_stable_hash`; only the
        // `artifact_key` content-hash dimension advances. Re-publishing under
        // the identical fingerprint keeps every recorded
        // `ModuleAugmentationIndexShape` signature valid while making the next
        // stitch hit the fast exact-key path (mirrors the names stitch).
        if !refreshed_keys.is_empty() {
            use crate::file_artifact_store::{AugmenterEntry, AugmenterSet};
            let mut entries = augmenter_set.entries.clone();
            for (idx, current_key) in refreshed_keys {
                entries[idx] = AugmenterEntry {
                    artifact_key: current_key,
                    parse_stable_hash: entries[idx].parse_stable_hash,
                };
            }
            artifact_store.populate_augmenter_set(
                key.clone(),
                Arc::new(AugmenterSet {
                    entries,
                    fingerprint: augmenter_set.fingerprint,
                }),
            );
        }

        if contributor_nodes.is_empty() {
            // DISTINGUISH the two empty outcomes (they are NOT the same):
            //   - `source_env_unobservable == true`: augmenters targeted this
            //     decl but EVERY contribution was unobservable (torn / unhealable
            //     / unservable). This is NOT "no augmentation" — the merged
            //     surface would be incomplete, so the caller must fold a no-warm
            //     `cache_suppress` signal. Return a tainted-empty outcome (no
            //     nodes, no roots, `source_env_unobservable = true`) so the
            //     relative and external callers route it to the semantic
            //     cache-suppress rail instead of publishing a base-only / miss
            //     result warm.
            //   - `source_env_unobservable == false`: genuinely no augmenter
            //     contributes this decl — a real, cacheable no-augmentation
            //     result. Return `None`; the caller keeps the base body unchanged
            //     and warms normally.
            if source_env_unobservable {
                return Some(AugmentationContributions {
                    contributor_nodes: Vec::new(),
                    contributor_roots: Vec::new(),
                    source_env_unobservable: true,
                });
            }
            return None;
        }

        // Fact rail: the augmenter-set shape fact (invalidates
        // on augmenter add/remove/reorder) + each augmenter's whole-hash
        // (invalidates on augmenter content edit). Observed onto the active
        // tracer so they enter this build's `ReadSetSignature.facts`. The shape
        // fact's `canonical_id` is attribution only — `ModuleAugmentationIndexShape`
        // validation keys entirely on the typed `FactKey` (target kind +
        // specifier/canonical) and `expected_hash`, never on this field.
        let shape_attribution = match &target {
            AugmentationTargetKind::ResolvedRelativeCanonical(canon) => canon.as_ref().to_owned(),
            AugmentationTargetKind::ExternalSpecifier(spec) => spec.as_ref().to_owned(),
            AugmentationTargetKind::WildcardAmbient(pat) => pat.as_ref().to_owned(),
            AugmentationTargetKind::GlobalAugmentation => String::new(),
        };
        crate::resolver_core::resolver_context::observe_fan_out(
            crate::resolver_core::FactVersionRef::RouteSurface(
                crate::resolver_core::RouteSurfaceFactRef {
                    canonical_id: shape_attribution,
                    key: crate::resolver_core::route_db::build_module_augmentation_index_shape_fact_key(
                        &target,
                    ),
                    lane: verter_semantic::facts::FactLane::Semantic,
                    expected_hash: augmenter_set.fingerprint,
                },
            ),
        );
        // Parent transitive contributor rule, emitted from the ONE carrier in
        // ONE step per contributor: the content-version fact
        // (`FileWholeHash`) AND the typed SOURCE-ENV observation
        // (`FileSourceEnv`) — its parser-version/file-language identity taken
        // from the EXACT artifact key the contributor `LowerLocator` served
        // from (never re-derived from canonical/path at this site, never a
        // stale index entry), its parse-env dimension the contributor
        // canonical's LIVE per-canonical parse env (the SAME dimension the
        // contributor `LowerLocator` key folds; the base key's own slot is
        // the zero sentinel, not an env identity). A warm parent hit
        // revalidates the source-env identity against the live view — a
        // contributor parse-env / parser-version / file-language move with
        // UNCHANGED content misses the warm read and recomputes through the
        // contributor's new `LowerLocator`. Because both facts come from the
        // same element, a contributor that is version-rooted on the parent is
        // source-env-observed by construction.
        for root in &contributor_roots {
            crate::resolver_core::resolver_context::observe_fan_out(
                crate::resolver_core::FactVersionRef::FileWholeHash {
                    canonical_id: root.canonical.as_ref().to_owned(),
                    hash: root.whole_hash,
                },
            );
            if crate::fact_signature_helpers::observe_file_source_env_from_artifact_key(
                self.ctx,
                Some(&root.artifact_key),
            )
            .is_none()
            {
                source_env_unobservable = true;
            }
        }

        Some(AugmentationContributions {
            contributor_nodes,
            contributor_roots,
            source_env_unobservable,
        })
    }

    /// Resolve a bare type name imported from a NON-FILE (external / ambient)
    /// module specifier through `declare module "<spec>" { ... }` augmentation.
    ///
    /// This is the canonical Vue/Vite `vite/client` pattern: a virtual module
    /// `"<spec>"` is declared and/or augmented across one or more files, none of
    /// which is a workspace FILE the specifier resolves to. The imported name
    /// therefore has no file-scope declaration and `resolve_bare_name_in_scope`
    /// returns `None`. Every contributing `declare module "<spec>"` block is a
    /// PEER (TS merges them with no base-vs-augmenter distinction), so the merged
    /// surface is built PURELY from the `ExternalSpecifier(spec)` augmentation
    /// index — there is NO base body. Folding routes through the SAME
    /// [`Self::collect_augmentation_contributions`] augmenter path + `MergedDecl`
    /// peer-merge carrier the relative stitch uses (one merge engine, no source
    /// scan).
    ///
    /// Returns `None` when `name` is not imported from a non-file specifier or
    /// no `declare module` block contributes it (caller falls through to the
    /// `Opaque(Miss)` sentinel). A relative / wildcard / global specifier is
    /// excluded by `augmenter_matches_target`'s `ExternalSpecifier` arm, so the
    /// index scan returns empty and this yields `None`.
    pub(super) fn resolve_external_module_augmentation(
        &self,
        scope_canonical: &str,
        name: &str,
        scope: &NodeScopeId,
        context: crate::semantic_query::ProjectionReductionContext,
    ) -> Option<SemanticNodeId> {
        use crate::file_artifact_store::{AugmentationTargetKind, InternedSpecifier};

        // The imported specifier for `name` in the importing file. Only a
        // NON-FILE specifier reaches here: a specifier resolving to a workspace
        // file would have resolved `name` through the normal import path before
        // the miss.
        let indexed = self
            .ctx
            .ensure_indexed_ready_serve(scope_canonical)?
            .indexed;
        let specifier = indexed
            .shallow_state
            .import_target(name)
            .map(|t| t.source_specifier.clone())?;

        // Program-completeness discovery for ambient external modules. Unlike a
        // relative `declare module "./base"` augmenter (discovered via the
        // base's reverse-dependency set), an ambient `declare module "<bare>"`
        // DECLARER may be a program-root `.d.ts` that NOTHING imports (the
        // canonical `vite/client` shape — referenced through tsconfig `types`/
        // `include`, not the import graph). It is reachable only via program
        // membership, so ensure every known program member is indexed BEFORE
        // the `ExternalSpecifier` index scan — the augmentation index only sees
        // loaded artifacts (R29). Loads are idempotent / content-hash cached;
        // this mirrors the relative stitch's "index the candidate set, then scan
        // once" shape, widened to program membership because an ambient module
        // has no base-file anchor.
        let host = self.ctx.host_for_fact_tracer_install();
        for canonical in host.workspace().known_canonicals() {
            let _ = self.ctx.ensure_indexed_ready_serve(&canonical);
        }

        let target =
            AugmentationTargetKind::ExternalSpecifier(InternedSpecifier::from(specifier.as_str()));
        let AugmentationContributions {
            contributor_nodes,
            // The external path DISCARDS the returned `contributor_roots`
            // (the relative stitch folds them into the enclosing
            // `instantiate_shell` output's `observed_self_roots`; there is no
            // such per-base output here — the carrier is interned mid-reference
            // resolution and returned as a bare node). This is COMPENSATED, not
            // a hole: `collect_augmentation_contributions` ALREADY observed each
            // folded contributor's `FileWholeHash` + `FileSourceEnv` onto the
            // active fact tracer (the enclosing `Instantiate` read-set), so a
            // contributor content edit OR source-env move rejects the warm
            // parent through the strict per-contributor reject rail — proven
            // end-to-end by
            // `cross_file_augmentation_merge_equivalence_tests::external_module_augmentation_warm_parent_rejects_contributor_content_edit_end_to_end`.
            contributor_roots: _,
            source_env_unobservable,
        } = self.collect_augmentation_contributions(target, name, context)?;
        // A torn contributor (unobservable source-env identity — a
        // torn/unhealable/unservable augmenter) is SERVED but must NEVER be
        // warm-admitted. This carrier is interned mid-reference-resolution and
        // returned as a bare node, so it owns no `QueryBuildOutput` of its own;
        // fold the no-warm signal into the ENCLOSING cold build's local taint
        // frame — the SAME semantic `QueryBuildOutput.cache_suppress` rail the
        // relative stitch uses (`instantiate_shell` sets `output.cache_suppress`
        // from the collector's `source_env_unobservable`). That is the rail memo
        // admission actually consults; the earlier request-materialisation
        // sticky alone did NOT gate the enclosing `Instantiate` memo. The
        // completion fence separately covers a mid-flight torn contributor
        // (revalidate-before-publish). Also mark the request-materialisation
        // sticky as a fail-closed backstop for the (unreachable-by-construction)
        // case where no enclosing cold-build frame is active.
        if source_env_unobservable {
            self.fold_into_top_build_local_taint(false, true);
            crate::request_context::mark_request_materialization_cache_suppress();
        }
        // A tainted-EMPTY collection (augmenters targeted the specifier but were
        // all unobservable) produces no body: the taint is already folded, so
        // fall through to the caller's `Opaque(Miss)` sentinel WITHOUT
        // synthesising a false zero-contributor `MergedDecl`.
        if contributor_nodes.is_empty() {
            return None;
        }

        // Peer-merge carrier from the augmenter contributions ONLY (no base
        // body): every `declare module "<spec>"` block is a peer. The carrier is
        // interned in the importing file's scope so its surface projects in the
        // caller's context; the per-augmenter `FileWholeHash` facts the folder
        // observed onto the active tracer are the cache-validity rail (each
        // contributor's `MergedDecl` reduction validates on those facts).
        let merged = self.graph().intern_node_with_scope(
            SemanticNodeData::MergedDecl {
                contributors: Arc::from(contributor_nodes.into_boxed_slice()),
            },
            scope.clone(),
        );
        Some(merged)
    }

    /// Resolve the typed declaration kind of a prepared declaration rooted at
    /// `identity` (Alias vs Interface vs Class).
    ///
    /// Used by the empty-path Shallow walker to decide whether a declaration
    /// body's reference arms are REAL interface/class heritage (which shadows)
    /// or an authored intersection (which intersects) — the P2-1
    /// distinguishing fact. Returns `None` when the prepared decl cannot be
    /// recovered, in which case the caller treats the body as non-heritage.
    pub(super) fn prepared_decl_kind(
        &self,
        identity: &crate::semantic_query::DeclIdentity,
    ) -> Option<verter_semantic::analysis::type_eval::TypeDeclKind> {
        let scope = NodeScopeId::File {
            canonical_id: Arc::clone(&identity.canonical_id),
            whole_hash: identity.whole_hash,
            local_scope: None,
        };
        let base = self
            .graph()
            .intern_node_with_scope(SemanticNodeData::Opaque(QueryError::Miss), scope);
        let adapter = SessionDispatchHost::new(self.ctx);
        let ri =
            ResolvedRootIdentity::new(identity.canonical_id.as_ref(), identity.decl_name.as_ref());
        adapter
            .resolve_prepared_type_decl(base, &ri)
            .map(|prepared| prepared.kind)
    }

    /// Lower a prepared declaration's `body` carrying the macro-surface
    /// provenance with own-body-vs-heritage discrimination (by
    /// design).
    ///
    /// - **Alias**: the body is the author-written macro type argument
    ///   (`defineProps<A & B>()` → `A & B`). Every member is own-body, so
    ///   the whole body is lowered with the caller's `context` (an
    ///   author intersection's arms keep `MacroTypeArgOwnBody`).
    /// - **Interface / Class**: the body folds `extends` heritage into an
    ///   `Intersection` whose heritage arms are `Ref` / `DeclRef` nodes
    ///   and whose own-body arms are `Object` nodes. Each arm is lowered
    ///   individually: own-body `Object` (and `Parenthesized(Object)`)
    ///   arms keep the caller's provenance; heritage `Ref`-shaped arms
    ///   downgrade to structural so inherited members surface with
    ///   `declared_in_macro_type_arg = false`. A plain interface body
    ///   (single `Object`, no `extends`) keeps the caller's provenance.
    ///
    /// This is per-arm SHAPE discrimination gated on `kind`, not arm
    /// order: declaration-merged interfaces (every own slice an `Object`
    /// arm) and `extends` heritage (always a `Ref` arm) are both handled
    /// correctly.
    #[allow(clippy::too_many_arguments)]
    fn lower_decl_body_with_provenance(
        &self,
        prepared: &PreparedTypeDecl,
        env: &FxHashMap<String, SemanticNodeId>,
        scope: &NodeScopeId,
        scope_payload: Option<&crate::resolver_core::bare_name_resolve::DeclarationScopePayload>,
        shadowing: &crate::resolver_core::scope_shadowing::ScopeShadowing,
        substitutions: &mut Vec<(Arc<str>, SemanticNodeId)>,
        context: crate::semantic_query::ProjectionReductionContext,
    ) -> SemanticNodeId {
        // Dual-leg parity seam (test builds only): while the oracle's
        // legacy-leg RAII guard is active on this thread, the retained
        // prepared-body implementation serves the body so the parity
        // harness can compare both body sources over live published
        // surfaces. Never compiled into production or plain debug builds.
        #[cfg(test)]
        if super::stage10_parity_oracle::legacy_prepared_body_leg_active() {
            return self.legacy_lower_decl_body_from_prepared(
                prepared,
                env,
                scope,
                scope_payload,
                shadowing,
                substitutions,
                context,
            );
        }

        // The declaration's OWN decl-body locator (whole body).
        let canonical: Arc<str> = match scope {
            NodeScopeId::File { canonical_id, .. } => Arc::clone(canonical_id),
            NodeScopeId::Global => Arc::from(prepared.root_identity.canonical_id.as_str()),
        };
        let locator = verter_type_expr::locators::AuthoredBodyLocator::DeclBody(
            verter_type_expr::locators::TypeBodySlot {
                anchor: verter_type_expr::locators::AuthoredAnchor {
                    canonical_id: canonical,
                    symbol: Arc::from(prepared.root_identity.symbol_name.as_str()),
                    space: verter_type_expr::locators::LocatorSymbolSpace::Type,
                },
                path: Arc::from(Vec::new().into_boxed_slice()),
            },
        );
        self.lower_located_body_with_provenance(
            locator,
            prepared.kind,
            &prepared.type_parameters,
            &prepared.name_resolution,
            env,
            scope,
            scope_payload,
            shadowing,
            substitutions,
            context,
        )
    }

    /// Lower an authored body named by `locator` under the caller's demand:
    /// fetch the ROLE-FREE unsubstituted shape through the memoized
    /// `LowerLocator` query, apply the caller's `env` bindings via semantic
    /// type-param substitution over the declaration's binder shells, and
    /// ONLY THEN project the demand-specific view (`ProjectionStamp`
    /// application + deferred-carrier evaluation) under `context`.
    ///
    /// This is the SOLE production body source — a locator miss surfaces as
    /// the query's miss semantics (`Opaque(Miss)`), never a prepared-body
    /// read. An `env` name with no binding substitutes the shared miss
    /// sentinel (open generics are preserved only when the caller pre-bound
    /// `TypeParam` shells, e.g. Skeleton-mode instantiation).
    #[allow(clippy::too_many_arguments)]
    pub(super) fn lower_located_body_with_provenance(
        &self,
        locator: verter_type_expr::locators::AuthoredBodyLocator,
        decl_kind: verter_semantic::analysis::type_eval::TypeDeclKind,
        type_parameters: &[verter_type_expr::TypeParam],
        name_resolution: &FxHashMap<String, ResolvedRootIdentity>,
        env: &FxHashMap<String, SemanticNodeId>,
        scope: &NodeScopeId,
        scope_payload: Option<&crate::resolver_core::bare_name_resolve::DeclarationScopePayload>,
        shadowing: &crate::resolver_core::scope_shadowing::ScopeShadowing,
        substitutions: &mut Vec<(Arc<str>, SemanticNodeId)>,
        context: crate::semantic_query::ProjectionReductionContext,
    ) -> SemanticNodeId {
        let owner_symbol = match &locator {
            verter_type_expr::locators::AuthoredBodyLocator::DeclBody(slot) => {
                Arc::clone(&slot.anchor.symbol)
            }
            verter_type_expr::locators::AuthoredBodyLocator::AugmentationBody(aug) => {
                Arc::clone(&aug.anchor.symbol)
            }
            verter_type_expr::locators::AuthoredBodyLocator::MacroPayload(payload) => {
                Arc::clone(&payload.anchor.symbol)
            }
        };

        // 1. Fetch the fixed authored shape (one reusable body-shape family
        //    per locator/source-env).
        let shape = match self.lower_locator(locator) {
            QueryResult::Value(node) => node,
            QueryResult::Recursive(_) | QueryResult::Error(_) => {
                return self.opaque(QueryError::Miss)
            }
        };

        // 2. Apply the caller's bindings via semantic type-param
        //    substitution. Binder identity is re-derived deterministically
        //    from the declaration's parameter list (content-addressed
        //    interning — the same ids the shape build bound). An unbound
        //    parameter substitutes the shared miss sentinel, mirroring the
        //    "unbound param propagates a miss through the body" rule.
        let (_, bindings) = self.locator_shape_binder_frame(
            scope,
            &owner_symbol,
            type_parameters,
            Some(name_resolution),
            scope_payload,
        );
        let mut substituted = shape;
        for (name, binder) in bindings {
            let bound = env.get(name.as_ref()).copied();
            let replacement = bound.unwrap_or_else(|| self.opaque(QueryError::Miss));
            let next = self.substitute_semantic_type_param(substituted, binder, replacement);
            if next != substituted {
                if let Some(arg_id) = bound {
                    substitutions.push((name, arg_id));
                }
            }
            substituted = next;
        }

        // 3. Project the demanded view: per-arm ProjectionStamp application
        //    + deferred-carrier evaluation under the caller's context.
        let inputs = crate::project_semantic_dispatch::locator_view::LocatorViewInputs {
            env,
            scope,
            name_resolution,
            scope_payload,
            shadowing,
        };
        self.project_located_decl_body(substituted, decl_kind, &inputs, substitutions, context)
    }

    pub(super) fn backfill_member_index_surface(
        &self,
        result: SemanticNodeId,
        prepared: &PreparedTypeDecl,
        env: &FxHashMap<String, SemanticNodeId>,
        scope: &NodeScopeId,
        scope_payload: Option<&crate::resolver_core::bare_name_resolve::DeclarationScopePayload>,
        shadowing: &crate::resolver_core::scope_shadowing::ScopeShadowing,
        substitutions: &mut Vec<(Arc<str>, SemanticNodeId)>,
        context: crate::semantic_query::ProjectionReductionContext,
    ) -> SemanticNodeId {
        let Some(data) = self.graph().node_data(result) else {
            return result;
        };
        let SemanticNodeData::Object(surface) = data.as_ref() else {
            return result;
        };
        if prepared.member_index.is_empty() {
            return result;
        }

        // `member_index` is the declaration's OWN-body direct-member
        // index (by design — authoritative own-member
        // overlay). `PreparedTypeDecl::build_member_index` populates it
        // from direct Object members only, skipping heritage `extends`
        // `Ref` arms. So a member present in `member_index` is
        // author-declared in this declaration's own body; under a
        // macro-type-argument own-body instantiation it carries
        // `declared_in_macro_type_arg = true`.
        let own_body_bit = context.is_macro_type_arg_own_body();
        let existing: FxHashSet<Arc<str>> = surface
            .members
            .iter()
            .map(|member| Arc::clone(&member.name))
            .collect();

        // (1) RE-STAMP existing surface members that are own-body index
        // entries. The interface body was lowered STRUCTURALLY (heritage
        // arms must stay `false`), so own members already on the surface
        // carry `false`; the overlay marks exactly the own-body ones
        // `own_body_bit`. Heritage members (not in `member_index`) are
        // left untouched. This is the "replace/mark, not only append"
        // requirement — without it a plain "append missing" overlay
        // would leave own members `false` after structural body lowering.
        let mut restamped_any = false;
        let mut members: Vec<SurfaceMember> = surface
            .members
            .iter()
            .map(|member| {
                if own_body_bit
                    && !member.declared_in_macro_type_arg.get()
                    && prepared.member_index.contains_key(member.name.as_ref())
                {
                    restamped_any = true;
                    SurfaceMember {
                        declared_in_macro_type_arg: context.own_body_stamp(),
                        ..member.clone()
                    }
                } else {
                    member.clone()
                }
            })
            .collect();

        // (2) APPEND own-body members not yet on the surface. Member
        // VALUE lowering downgrades to structural provenance: a nested
        // object inside the member's type is NOT the macro-T own body —
        // only the member's PRESENCE on this declaration is.
        let mut added: Vec<SurfaceMember> = prepared
            .member_index
            .iter()
            .filter(|(name, _)| !existing.contains(name.as_str()))
            .map(|(name, member)| {
                let value = self.shallow_lower_type_expr_with_context(
                    &member.ty,
                    env,
                    scope,
                    &prepared.name_resolution,
                    scope_payload,
                    shadowing,
                    substitutions,
                    context.into_structural_provenance(),
                );
                SurfaceMember {
                    name: Arc::from(name.as_str()),
                    value,
                    optional: member.optional,
                    readonly: member.readonly,
                    is_method: member.is_method,
                    // The `PreparedMember` carries the IR member's declared
                    // accessibility verbatim, so the overlay append preserves
                    // it (Public for every non-class origin).
                    visibility: member.visibility,
                    // The `PreparedMember` (the `member_index` entry) now
                    // carries the IR member's OXC declaration-site spans + the
                    // declaration's defining file, so the append is span-rich.
                    spans: member.spans,
                    declaration_origin: (!member.declaration_origin.is_empty())
                        .then(|| Arc::from(member.declaration_origin.as_str())),
                    declared_in_macro_type_arg: context.own_body_stamp(),
                    // `member_index` is the declaration's OWN-body direct
                    // member index (heritage `extends` arms are excluded), so
                    // an appended member is own-body — it SHADOWS an inherited
                    // heritage member of the same name.
                    merge_role: context.stamp_role(crate::semantic_query::MemberMergeRole::OwnBody),
                }
            })
            .collect::<Vec<_>>();

        if added.is_empty() && !restamped_any {
            return result;
        }

        added.sort_unstable_by(|left, right| left.name.as_ref().cmp(right.name.as_ref()));
        members.extend(added);
        self.graph().intern_node_with_scope(
            SemanticNodeData::Object(SurfaceView {
                members: Arc::from(members.into_boxed_slice()),
                call_signatures: Arc::clone(&surface.call_signatures),
                construct_signatures: Arc::clone(&surface.construct_signatures),
                index_signatures: Arc::clone(&surface.index_signatures),
                keyspace: surface.keyspace,
                has_index_signature: surface.has_index_signature,
            }),
            scope.clone(),
        )
    }

    /// Read the source surface of an object-filter utility (`Pick` /
    /// `Omit`), carrier-complete.
    ///
    /// The source of `Pick<X, K>` / `Omit<X, K>` is usually a resolved
    /// `Object` after `evaluate_deferred_semantic_node_with_context` — that
    /// fast path preserves every signature kind (call / construct / index)
    /// verbatim. But a CROSS-FILE source (`Omit<ImportedBase, K>` reached
    /// from a heritage arm) lowers to a `DeclRef` / `InstantiationRef`
    /// carrier in `Navigate` / `Skeleton`, which the deferred evaluator
    /// deliberately does NOT unwrap (it would over-evaluate symbolic
    /// IndexedAccess hops — see `evaluate.rs`). For those carrier sources we
    /// read the one-level surface through the shared empty-path `Shallow`
    /// reader [`Self::resolve_typeinfo_surface_view`], which routes through the
    /// SOLE query-time resolver and returns the core [`SurfaceView`]
    /// PRESERVING call / construct / index signatures + keyspace. The `Omit`
    /// arm then carries those signatures through (TS semantics: `Omit<T, K>`
    /// filters property names only), where the old `MacroSurfaceView` reader
    /// silently dropped construct / index signatures for a carrier-sourced
    /// `Omit`.
    fn object_filter_source_surface(&self, source_resolved: SemanticNodeId) -> Option<SurfaceView> {
        match self.graph().node_data(source_resolved).as_deref() {
            Some(SemanticNodeData::Object(view)) => Some(view.clone()),
            Some(
                SemanticNodeData::DeclRef { .. }
                | SemanticNodeData::InstantiationRef { .. }
                // Compound carriers — an `Intersection` / `Union` source arises
                // when the Pick/Omit source is itself a HERITAGE-bearing
                // declaration: `Omit<SelectMenuProps<T>, K>` where
                // `interface SelectMenuProps<T> extends Pick<RootProps<T>, …> { … }`
                // resolves its instantiated body to
                // `Intersection([<extends-Pick arm>, <own body>])`, NOT a flat
                // `Object`. Reading that compound surface through the SAME
                // shared empty-path Shallow reader merges the heritage arm(s)
                // with the own body into one `SurfaceView`, so the inherited
                // members survive the outer `Omit`/`Pick`. Without this arm the
                // utility collapses the heritage-bearing source to
                // `Opaque(Miss)` and every inherited member is lost (the
                // generic-Omit-of-Pick-of-generic heritage collapse).
                | SemanticNodeData::Intersection(_)
                | SemanticNodeData::Union(_),
            ) => {
                // A Pick/Omit source is never the macro-T own body — read it
                // under the structural `published(Shallow)` context.
                self.resolve_typeinfo_surface_view(
                    source_resolved,
                    crate::semantic_query::ProjectionReductionContext::published(
                        ProjectionMode::Shallow,
                    ),
                )
            }
            _ => None,
        }
    }

    /// Drop the call/construct signatures whose Vue emit event NAME (the first
    /// parameter's string-literal type — or each literal of a union first
    /// parameter) is in `omit_set`. A signature whose first parameter is not a
    /// string literal, or whose event name is not omitted, is kept verbatim.
    ///
    /// This is the call-signature analogue of property-name `Omit`: a Vue emit
    /// interface declares each event as a call signature `(e: 'name', …): void`,
    /// so omitting `'name'` must remove that signature. The event-name read uses
    /// the SAME first-parameter-literal rule the emit normalizer
    /// (`emits_from_typeinfo_surface`) uses, so the two agree on which signatures
    /// represent which events.
    fn filter_omitted_event_signatures(
        &self,
        signatures: &[SemanticNodeId],
        omit_set: &FxHashSet<&str>,
    ) -> Arc<[SemanticNodeId]> {
        let kept: Vec<SemanticNodeId> = signatures
            .iter()
            .filter(|sig| {
                // Keep the signature unless EVERY event name it declares is
                // omitted (a single signature can declare a union of event
                // names; only drop it when all are omitted, mirroring how the
                // normalizer would surface the surviving names).
                match self.call_signature_event_names(**sig) {
                    Some(names) if !names.is_empty() => {
                        !names.iter().all(|name| omit_set.contains(name.as_ref()))
                    }
                    // No string-literal event name → not an omittable emit
                    // signature; keep it (general TS `Omit` leaves it intact).
                    _ => true,
                }
            })
            .copied()
            .collect();
        Arc::from(kept.into_boxed_slice())
    }

    /// The Vue emit event name(s) a call/construct signature declares — its
    /// first parameter's string-literal type, or each literal of a union first
    /// parameter. `None` when the node is not a `Function`, has no parameters,
    /// or the first parameter is not a string-literal (union). Mirrors the
    /// first-parameter event-name rule in `emits_from_typeinfo_surface`.
    fn call_signature_event_names(&self, sig: SemanticNodeId) -> Option<Vec<Arc<str>>> {
        let data = self.graph().node_data(sig)?;
        let SemanticNodeData::Function { params, .. } = data.as_ref() else {
            return None;
        };
        let first = params.first()?;
        let mut names = Vec::new();
        self.collect_string_literal_names(first.ty, &mut names);
        if names.is_empty() {
            None
        } else {
            Some(names)
        }
    }

    /// Push the string-literal value(s) carried by `node` into `out` — the node
    /// itself when it is a `Literal(String)`, or each `Literal(String)` arm when
    /// it is a `Union`. A `Union` arm that is not a string literal is skipped
    /// (the surrounding event-name rule only recognises string literals).
    fn collect_string_literal_names(&self, node: SemanticNodeId, out: &mut Vec<Arc<str>>) {
        let resolved = self.evaluate_deferred_semantic_node(node);
        let Some(data) = self.graph().node_data(resolved) else {
            return;
        };
        match data.as_ref() {
            SemanticNodeData::Literal(LiteralValue::String(name)) => {
                out.push(Arc::from(name.as_str()))
            }
            SemanticNodeData::Union(members) => {
                for member in members.iter() {
                    self.collect_string_literal_names(*member, out);
                }
            }
            _ => {}
        }
    }

    /// Built-in utility dispatch.
    ///
    /// Routes recognised utility names (`Partial`, `Required`, `Readonly`,
    /// `Record`, `NoInfer`, string intrinsics, etc.) through the same
    /// `SemanticQueryKey::{MappedType, ProjectMember, ProjectPath, Normalize}`
    /// dispatch as userland aliases. Userland equivalence rule: a userland
    /// alias `type MyPartial<T> = { [K in keyof T]?: T[K] }` and the
    /// built-in `Partial<T>` produce the same `SemanticNodeId` and the
    /// same origin-edge structure when they route through the same
    /// `MappedType` dispatch key.
    ///
    /// Utilities are classified into three groups by implementation shape:
    ///
    /// - **Mapper-based** (`Partial`, `Required`, `Readonly`, `Record`):
    ///   synthesise a `MapperKey` whose modifiers encode the utility
    ///   transformation and dispatch through `SemanticQueryKey::MappedType`.
    ///   The resulting node is shared with any userland mapped type that
    ///   happens to produce an equivalent `MapperKey` because the memo
    ///   dedups on the full key.
    /// - **Identity** (`NoInfer`): returns the first argument as an `Alias`
    ///   node, emitting `Instantiate` + `SubstituteTypeParam` +
    ///   `AliasResolve` edges.
    /// - **Object-filter** (`Pick`, `Omit`): produce an Object surface
    ///   filtered by enumerable key set; preserve modifier flags +
    ///   (for `Omit`) source signatures.
    /// - **Union-filter** (`Extract`, `Exclude`): per-member
    ///   assignability via `relate_nodes`; survivors reconstituted via
    ///   `intern_normalized_union_or_intersection` so empty and
    ///   singleton surviving sets canonicalise to `Never` / the lone
    ///   member respectively. This closes the literal-type reduction;
    ///   non-literal arms still fall through to the deferred shell.
    /// - **Opaque** (`NonNullable`, `Awaited`, function-signature
    ///   utilities when the argument shape does not match, string
    ///   intrinsics with a broad/open carrier): return a shell anchored
    ///   to the utility + arg identity with `Instantiate` +
    ///   `SubstituteTypeParam` edges. The shell's body is lazy —
    ///   callers projecting into it follow the normal `ProjectPath`
    ///   route which terminates with `Miss` until a later track
    ///   implements the full shape. String intrinsics reduce
    ///   literal/union inputs to the transformed literal/union
    ///   (`Uppercase<"a">`→`"A"`, distributing over unions); only a
    ///   broad/open carrier (`string`/non-literal) widens to `String`,
    ///   and a bare `TypeParam` is preserved as an `InstantiationRef`
    ///   carrier.
    ///
    /// Every utility path emits the `Instantiate` edge with sources
    /// `[base, args...]` and per-arg `SubstituteTypeParam` edges so the
    /// origin graph is walkable end-to-end.
    pub(super) fn build_builtin_utility(
        &self,
        base: SemanticNodeId,
        name: &str,
        args: &Arc<[SemanticNodeId]>,
        context: crate::semantic_query::ProjectionReductionContext,
    ) -> (QueryResult<SemanticNodeId>, DepSignature, bool) {
        use crate::semantic_query::{MapperKey, OptionalityMod, ReadonlyMod};

        let graph = self.graph();
        let fence = self.project_generation_signature();
        self.graph().record_instantiate();

        // Two-signal taxonomy fold. The mapper-based
        // utilities (`Partial` / `Required` / `Readonly` / `Record` and
        // the shared `keyof source` reification) surface their result
        // through nested `KeyOf` / `MappedType` subqueries. When such a
        // nested subquery is GENUINELY INCOMPLETE — budget exceeded,
        // same-path recursion, walker fatal/pathological — the utility's
        // produced surface is itself partial even though it surfaces as a
        // complete-looking `QueryResult::Value` (an `Opaque(Miss)` shell).
        // `result_is_partial` accumulates that partiality across every
        // nested read so the cold-build helper can mark the published
        // value partial and the component-meta / shape / materialize warm
        // gates refuse it. A benign non-cacheable nested read (signature
        // overflow, unrootable, ReturnOnly) is COMPLETE — it does NOT set
        // this flag (it only blocks the inner memo via `cache_suppress`).
        let result_is_partial = std::cell::Cell::new(false);

        // Look up the utility's real TS type-parameter names so
        // `SubstituteTypeParam` edges carry names identical to those
        // the userland-equivalent alias would emit. `Partial<T>` and
        // `type MyPartial<T> = ...` both produce
        // `SubstituteTypeParam("T", arg)` — a synthesised `"T0"`-style
        // name would break origin-walk equivalence.
        let param_names = utility_param_names(name);

        // Helper: emit the common `Instantiate` + per-arg
        // `SubstituteTypeParam` edges on a utility result node.
        let record_utility_edges = |result_id: SemanticNodeId| {
            let mut inst_sources: Vec<SemanticNodeId> = Vec::with_capacity(args.len() + 1);
            inst_sources.push(base);
            inst_sources.extend(args.iter().copied());
            graph.record_origin_edge(
                result_id,
                OriginEdgeKind::Instantiate,
                Arc::from(inst_sources.into_boxed_slice()),
                OriginMeta::None,
                Arc::clone(&fence),
            );
            for (idx, arg_id) in args.iter().enumerate() {
                // Use the utility's declared type-parameter name when
                // known; fall back to a positional label only for
                // unknown utilities (which return Opaque anyway).
                let param_name: Arc<str> = param_names
                    .get(idx)
                    .map(|n| Arc::<str>::from(*n))
                    .unwrap_or_else(|| Arc::<str>::from(format!("T{idx}")));
                graph.record_origin_edge(
                    result_id,
                    OriginEdgeKind::SubstituteTypeParam,
                    Arc::from(vec![*arg_id].into_boxed_slice()),
                    OriginMeta::SubstitutedParam(param_name),
                    Arc::clone(&fence),
                );
            }
        };

        // Mapper-based utilities route through `SemanticQueryKey::MappedType`.
        // The mapper's `value_expr` is an `Opaque(Miss)` shell marker, never
        // a substitution target: these mappers classify as
        // `MapperKind::Identity`, and every Identity per-key producer reads
        // the matching source member's value directly or dispatches
        // `source[K]` through the shared `IndexedAccess` query.
        let mapper_for = |opt: OptionalityMod, ro: ReadonlyMod, source: SemanticNodeId| {
            // Thread the outer instantiation's context so a builtin
            // utility called under `StructuralTransit` (e.g.
            // `Partial<Record<keyof T, undefined>>` reached through a
            // relation-engine `infer`-binding pass) does NOT reify
            // `keyof source` into a literal-anchor union.
            let key_space_read = self.execute_read(SemanticQueryKey::KeyOf {
                base: source,
                context,
            });
            // FOLD: a genuinely-incomplete nested KeyOf (budget /
            // recursion / walker-fatal) taints the utility surface.
            if key_space_read.result_is_partial {
                result_is_partial.set(true);
            }
            let key_space = match key_space_read.value {
                QueryResult::Value(node) => node,
                _ => self.opaque(QueryError::Miss),
            };
            // Shell marker only — no per-key producer reads an Identity
            // mapper's `value_expr`. `build_mapped_type` reuses a matching
            // source member's value directly; a key without a projectable
            // source member dispatches `source[K]` through the shared
            // `IndexedAccess` query, publishing the addressable deferred
            // `IndexedAccess` carrier when the access cannot close.
            let value_expr = self.opaque(QueryError::Miss);
            // Synthesise a TypeParam binder node for the utility
            // mapper's `K`: the param_index is the
            // source SemanticNodeId itself (truncated to u16) so
            // two utility invocations on the SAME source share
            // the SAME binder identity → same `MapperKey` → same
            // `SemanticQueryKey::MappedType` cache key. Distinct
            // sources naturally get distinct ordinals via the
            // SemanticNodeId.
            //
            // For sources whose SemanticNodeId exceeds u16 the
            // truncated ordinals can theoretically collide, but
            // the arena dedup also includes the `display_name`,
            // `decl`, and `constraint` fields — two utility
            // mappers with the same display_name + decl that
            // happen to alias at u16 are extremely unlikely AND
            // the failure mode is "two mappers share a binder
            // SemanticNodeId" which causes a substitute walk to
            // bind the same K for both — semantically benign
            // because the consumer's source-keyed dispatch
            // remains source-specific via the outer
            // `SemanticQueryKey::MappedType.source`.
            let param_index = (source.0 & 0xFFFF) as u16;
            let parameter_node = self.graph().intern_node(SemanticNodeData::TypeParam {
                decl: crate::semantic_query::DeclIdentity {
                    canonical_id: Arc::from("<utility>"),
                    whole_hash: crate::semantic_query::HashValue::default(),
                    decl_name: Arc::from("<utility-mapper>"),
                },
                param_index,
                constraint: None,
                default: None,
                display_name: Arc::from("K"),
            });
            MapperKey {
                parameter_node,
                key_space,
                value_expr,
                optionality: opt,
                readonly: ro,
                name_remap: None,
                // Partial / Required / Readonly are the canonical
                // `{ [K in keyof T]: T[K] }` mappers — classify them
                // as `Identity` explicitly. The placeholder
                // `value_expr = Miss` is a shell marker (the build
                // path never reads it for Identity mappers; it reads
                // source member values directly), not a
                // runtime-discoverable `T[K]` shape.
                kind: crate::semantic_query::MapperKind::Identity,
            }
        };

        // Degenerate-operand short-circuit (shared §22-style table in
        // `absorb.rs`). A DIRECT lattice-extreme source operand resolves the
        // utility before any signature walk / keyspace enumeration / mapped
        // dispatch / per-arm relation runs — `ReturnType<any>` is `any`
        // without consulting call signatures, `Exclude<any, U>` is `any`
        // without entering the relation loop. Non-degenerate operands fall
        // through to the structural arms below unchanged.
        if let Some(absorbed) = self.absorb_builtin_utility_degenerate(name, args.as_ref()) {
            record_utility_edges(absorbed);
            return (QueryResult::Value(absorbed), fence, false);
        }

        match name {
            // ---- Mapper-based utilities ----
            "Partial" if args.len() == 1 => {
                let source = args[0];
                let mapper = mapper_for(OptionalityMod::Add, ReadonlyMod::Keep, source);
                let mapped_read = self.execute_read(SemanticQueryKey::MappedType {
                    source,
                    mapper,
                    context,
                });
                if mapped_read.result_is_partial {
                    result_is_partial.set(true);
                }
                let result = match mapped_read.value {
                    QueryResult::Value(node) => node,
                    _ => self.opaque(QueryError::Miss),
                };
                record_utility_edges(result);
                (QueryResult::Value(result), fence, result_is_partial.get())
            }
            "Required" if args.len() == 1 => {
                let source = args[0];
                let mapper = mapper_for(OptionalityMod::Remove, ReadonlyMod::Keep, source);
                let mapped_read = self.execute_read(SemanticQueryKey::MappedType {
                    source,
                    mapper,
                    context,
                });
                if mapped_read.result_is_partial {
                    result_is_partial.set(true);
                }
                let result = match mapped_read.value {
                    QueryResult::Value(node) => node,
                    _ => self.opaque(QueryError::Miss),
                };
                record_utility_edges(result);
                (QueryResult::Value(result), fence, result_is_partial.get())
            }
            "Readonly" if args.len() == 1 => {
                let source = args[0];
                let mapper = mapper_for(OptionalityMod::Keep, ReadonlyMod::Add, source);
                let mapped_read = self.execute_read(SemanticQueryKey::MappedType {
                    source,
                    mapper,
                    context,
                });
                if mapped_read.result_is_partial {
                    result_is_partial.set(true);
                }
                let result = match mapped_read.value {
                    QueryResult::Value(node) => node,
                    _ => self.opaque(QueryError::Miss),
                };
                record_utility_edges(result);
                (QueryResult::Value(result), fence, result_is_partial.get())
            }
            "Record" if args.len() == 2 => {
                // Record<K, V>: map K's key space to V.
                let key_arg = args[0];
                let value_arg = args[1];
                // Key space is K itself (usually a union of literals).
                // For equivalence with userland `{ [P in K]: V }`, both
                // paths set `key_space = K` and `value_expr = V`.
                // Synthesise a TypeParam binder node id for `P`.
                //
                // The param_index is derived from
                // (key_arg, value_arg) so two `Record<K, V>`
                // invocations on the same K, V share the SAME
                // binder identity → same `MapperKey` → same
                // `SemanticQueryKey::MappedType` cache key.
                let param_index = ((key_arg.0 ^ value_arg.0.rotate_left(8)) & 0xFFFF) as u16;
                let parameter_node = self.graph().intern_node(SemanticNodeData::TypeParam {
                    decl: crate::semantic_query::DeclIdentity {
                        canonical_id: Arc::from("<utility>"),
                        whole_hash: crate::semantic_query::HashValue::default(),
                        decl_name: Arc::from("<utility-mapper>"),
                    },
                    param_index,
                    constraint: None,
                    default: None,
                    display_name: Arc::from("P"),
                });
                let mapper = MapperKey {
                    parameter_node,
                    key_space: key_arg,
                    value_expr: value_arg,
                    optionality: OptionalityMod::Keep,
                    readonly: ReadonlyMod::Keep,
                    name_remap: None,
                    // `Record<K, V>` maps every key to the same `V`
                    // expression — a computed projection, not the
                    // identity `T[K]`. Tag as `Computed` so
                    // `build_mapped_type` takes the substitute +
                    // evaluate path.
                    kind: crate::semantic_query::MapperKind::Computed,
                };
                // Source is K; `build_mapped_type` reads names from K's
                // keyspace branch when the source isn't an Object.
                let mapped_read = self.execute_read(SemanticQueryKey::MappedType {
                    source: key_arg,
                    mapper,
                    context,
                });
                if mapped_read.result_is_partial {
                    result_is_partial.set(true);
                }
                let result = match mapped_read.value {
                    QueryResult::Value(node) => node,
                    _ => self.opaque(QueryError::Miss),
                };
                record_utility_edges(result);
                (QueryResult::Value(result), fence, result_is_partial.get())
            }

            // ---- Identity utility ----
            "NoInfer" if args.len() == 1 => {
                let source = args[0];
                let result = graph.intern_node(SemanticNodeData::Alias(source));
                graph.record_origin_edge(
                    result,
                    OriginEdgeKind::AliasResolve,
                    Arc::from(vec![source].into_boxed_slice()),
                    OriginMeta::None,
                    Arc::clone(&fence),
                );
                record_utility_edges(result);
                (QueryResult::Value(result), fence, false)
            }

            // ---- String intrinsics ----
            // `Uppercase` / `Lowercase` / `Capitalize` / `Uncapitalize` are
            // literal-preserving: a string literal maps to its case-transformed
            // literal, a union distributes per-arm then renormalises, `never`
            // stays `never`, and a broad `string` (or any unresolved/non-string
            // shape) fails closed to the `string` primitive. The template-literal
            // reducer consumes this result so `` `on${Capitalize<"submit"|"cancel">}` ``
            // distributes correctly. Typed-IR only — the case transform applies
            // to the interned literal value, never to source/display text.
            "Uppercase" | "Lowercase" | "Capitalize" | "Uncapitalize" if args.len() == 1 => {
                let resolved_arg =
                    self.evaluate_deferred_semantic_node_with_context(args[0], context);
                let result = if matches!(
                    graph.node_data(resolved_arg).as_deref(),
                    Some(SemanticNodeData::TypeParam { .. })
                ) {
                    // Binder-preservation catches ONLY a BARE, unsubstituted
                    // `TypeParam` (e.g. the mapper binder `K` in
                    // `as `on${Capitalize<K>}``): preserve the intrinsic as a
                    // deferred `InstantiationRef` carrier so a later per-key
                    // substitution can bind the param and reduce. Reducing now
                    // would erase the binder (collapse to the broad `string`)
                    // before the key is known. A COMPOUND / nested open arg
                    // (an open conditional, a `TypeParam` buried inside a union
                    // or object) is NOT caught here — it falls through to
                    // `apply_string_intrinsic`, which fails closed to the
                    // `string` primitive for any non-finite-literal shape.
                    graph.intern_node(SemanticNodeData::InstantiationRef {
                        base: crate::semantic_query::DeclIdentity {
                            canonical_id: Arc::from("__builtin__"),
                            whole_hash: crate::semantic_query::HashValue::default(),
                            decl_name: Arc::from(name),
                        },
                        args: Arc::from(vec![args[0]].into_boxed_slice()),
                    })
                } else {
                    // Reuse the node we already resolved above — do NOT re-evaluate
                    // `args[0]` from scratch inside `apply_string_intrinsic`.
                    self.apply_string_intrinsic(name, resolved_arg, context)
                };
                record_utility_edges(result);
                (QueryResult::Value(result), fence, false)
            }

            // ---- Function-signature utilities ----
            // `ReturnType<F>` / `Parameters<F>` inspect call signatures;
            // `ConstructorParameters<C>` / `InstanceType<C>` inspect
            // construct signatures. Resolves when the argument is a
            // canonical `Function` node directly, or an `Object` surface
            // carrying exactly one call / construct signature and no
            // user-level members. Typical entry: `ReturnType<typeof fn>`
            // where `build_typeof` produced an Object with a single
            // lowered call signature, or `ReturnType<() => T>` where
            // lowering produced a `Function` node straight away.
            //
            // When the argument does not match either shape the branch
            // falls through to the opaque shell so downstream consumers
            // still see an `Instantiate` edge anchored to the utility
            // identity.
            "ReturnType" | "InstanceType" if args.len() == 1 => {
                // Demand-point source resolution: a decl-placeholder /
                // alias-shell / `DeclRef` carrier argument
                // (`ReturnType<Handler>` where `Handler` is a
                // function-type alias) settles to its canonical Function
                // carrier before the signature walk. Bucket selection:
                // `ReturnType` reads the CALL bucket, `InstanceType` the
                // CONSTRUCT bucket — hybrids and member-bearing
                // constructor objects select per kind, never per shape.
                let bucket = if name == "ReturnType" {
                    SignatureBucket::Call
                } else {
                    SignatureBucket::Construct
                };
                let source_resolved = self.resolve_signature_source_carrier(args[0], context);
                if let Some(function_node) = self.select_signature_function(source_resolved, bucket)
                {
                    if let Some(SemanticNodeData::Function { return_type, .. }) =
                        self.graph().node_data(function_node).as_deref()
                    {
                        // Free signature generics instantiate at `unknown`
                        // (the sb15 rule: `ReturnType<typeof id>` over
                        // `id<T>(x: T): T` is `unknown`).
                        let id = self.instantiate_free_signature_params_at_unknown(
                            function_node,
                            *return_type,
                        );
                        record_utility_edges(id);
                        return (QueryResult::Value(id), fence, false);
                    }
                }
                let result = self.opaque(QueryError::Miss);
                record_utility_edges(result);
                (QueryResult::Value(result), fence, false)
            }
            "Parameters" | "ConstructorParameters" if args.len() == 1 => {
                // Demand-point source resolution + bucket selection — see
                // ReturnType/InstanceType above.
                let bucket = if name == "Parameters" {
                    SignatureBucket::Call
                } else {
                    SignatureBucket::Construct
                };
                let source_resolved = self.resolve_signature_source_carrier(args[0], context);
                if let Some(function_node) = self.select_signature_function(source_resolved, bucket)
                {
                    if let Some(tuple_id) = self.intern_function_params_tuple(function_node) {
                        let tuple_id = self
                            .instantiate_free_signature_params_at_unknown(function_node, tuple_id);
                        record_utility_edges(tuple_id);
                        return (QueryResult::Value(tuple_id), fence, false);
                    }
                }
                let result = self.opaque(QueryError::Miss);
                record_utility_edges(result);
                (QueryResult::Value(result), fence, false)
            }

            // ---- Object-filter utilities ----
            // `Pick<X, K>` produces an Object surface containing the
            // subset of `X`'s members whose names appear in `K`'s
            // enumerable key space. `Omit<X, K>` is the inverse —
            // members of `X` whose names are NOT in `K`. Both
            // implementations preserve the source's per-member
            // optional / readonly / is_method flags so downstream
            // path-walking lands on the same value SemanticNodeIds
            // a userland-equivalent definition would emit.
            //
            // When `K` cannot be enumerated (e.g. still a TypeParam
            // or deferred shell) OR `X` does not resolve to an
            // Object surface, the utility falls through to the
            // deferred shell so callers re-dispatch once the inputs
            // become enumerable.
            "Pick" if args.len() == 2 => {
                let source = args[0];
                let keys_arg = args[1];
                let pick_names = match self.key_names_from_keyspace_node(keys_arg) {
                    Some(names) => names,
                    None => {
                        let result = self.opaque(QueryError::Miss);
                        record_utility_edges(result);
                        return (QueryResult::Value(result), fence, false);
                    }
                };
                // Context-propagating deferred
                // resolution. Under `StructuralTransit(_)` the source's
                // nested operators carrier-stop (`may_reduce_operator
                // (StructuralTransit(_)) == false`); the Pick falls through
                // to its deferred shell (Opaque(Miss)) so callers re-
                // dispatch once admission lifts to publication demand.
                // Closes the V chain leak empirically traced to this
                // call (`evaluate_deferred_semantic_node` defaulted to
                // `Published(Expanded)`, breaking context propagation
                // when the outer caller dispatched with
                // `StructuralTransit(Shallow)`).
                let source_resolved =
                    self.evaluate_deferred_semantic_node_with_context(source, context);
                // `Pick<any, K>` (pinned tsgo): a CLOSED surface holding
                // exactly the enumerated keys, each `any` and required —
                // `Pick<any, "x">` = `{ x: any }`, `Pick<any, never>` = `{}`.
                // Materialised AFTER key enumeration because the result
                // depends on K (a non-enumerable K already deferred above);
                // the source-side `any` check sits here, not in the absorb
                // table, for the same reason.
                if matches!(
                    self.peek_special(source_resolved),
                    Some((super::absorb::SpecialKind::Any, _))
                ) {
                    let any_node = graph.intern_node(SemanticNodeData::Primitive(
                        crate::semantic_query::PrimitiveKind::Any,
                    ));
                    let members: Vec<SurfaceMember> = pick_names
                        .iter()
                        .map(|name| SurfaceMember {
                            name: Arc::clone(name),
                            value: any_node,
                            optional: false,
                            readonly: false,
                            is_method: false,
                            visibility: verter_type_expr::MemberVisibility::Public,
                            // Synthetic mapped-produced members: no single
                            // source declaration site.
                            spans: verter_type_expr::MemberSpans::default(),
                            declaration_origin: None,
                            declared_in_macro_type_arg:
                                crate::semantic_query::MacroOwnBodyStamp::NEUTRAL,
                            merge_role: crate::semantic_query::MergeRoleStamp::NEUTRAL,
                        })
                        .collect();
                    let result_surface = SurfaceView {
                        members: Arc::from(members.into_boxed_slice()),
                        call_signatures: Arc::from(Vec::<SemanticNodeId>::new().into_boxed_slice()),
                        construct_signatures: Arc::from(
                            Vec::<SemanticNodeId>::new().into_boxed_slice(),
                        ),
                        index_signatures: Arc::from(
                            Vec::<IndexSignature>::new().into_boxed_slice(),
                        ),
                        keyspace: None,
                        has_index_signature: false,
                    };
                    let result = graph.intern_node(SemanticNodeData::Object(result_surface));
                    record_utility_edges(result);
                    return (QueryResult::Value(result), fence, false);
                }
                let surface = match self.object_filter_source_surface(source_resolved) {
                    Some(view) => view,
                    None => {
                        let result = self.opaque(QueryError::Miss);
                        record_utility_edges(result);
                        return (QueryResult::Value(result), fence, false);
                    }
                };
                let pick_set: FxHashSet<&str> = pick_names.iter().map(|s| s.as_ref()).collect();
                // `Pick<C, K>` is a PUBLIC-keyspace projection (TS:
                // `Pick<T, K extends keyof T>`, and `keyof ClassType` excludes
                // protected/private members). Filter non-public source members
                // BEFORE the name predicate so a `Pick` whose key names a
                // non-public class member yields an empty surface rather than
                // re-minting the non-public member — the same public-only
                // keyspace `source_members_for_published_projection` /
                // `build_key_of` apply. The full member set (incl. non-public)
                // stays recorded on the source surface for the keep-all
                // `native_props` carrier; only this DERIVATION is gated.
                let picked: Vec<SurfaceMember> = surface
                    .members
                    .iter()
                    .filter(|m| m.visibility.is_public())
                    .filter(|m| pick_set.contains(m.name.as_ref()))
                    .cloned()
                    .collect();
                let result_surface = SurfaceView {
                    members: Arc::from(picked.into_boxed_slice()),
                    call_signatures: Arc::from(Vec::<SemanticNodeId>::new().into_boxed_slice()),
                    construct_signatures: Arc::from(
                        Vec::<SemanticNodeId>::new().into_boxed_slice(),
                    ),
                    index_signatures: Arc::from(Vec::<IndexSignature>::new().into_boxed_slice()),
                    keyspace: None,
                    has_index_signature: false,
                };
                let result = graph.intern_node(SemanticNodeData::Object(result_surface));
                record_utility_edges(result);
                (QueryResult::Value(result), fence, false)
            }
            "Omit" if args.len() == 2 => {
                let source = args[0];
                let keys_arg = args[1];
                let omit_names = match self.key_names_from_keyspace_node(keys_arg) {
                    Some(names) => names,
                    None => {
                        let result = self.opaque(QueryError::Miss);
                        record_utility_edges(result);
                        return (QueryResult::Value(result), fence, false);
                    }
                };
                // Context-propagating deferred
                // resolution (see Pick comment above for chain).
                let source_resolved =
                    self.evaluate_deferred_semantic_node_with_context(source, context);
                // `Omit<any, K-literals>` (pinned tsgo): the index-signature
                // surface `{ [x: string]: any; [x: number]: any;
                // [x: symbol]: any }`, independent of WHICH literal keys are
                // omitted (excluding finite literals from the broad key
                // domain removes nothing). A non-enumerable K (e.g.
                // `Omit<any, string>`, whose tsgo result drops the string
                // signature) already deferred above.
                if matches!(
                    self.peek_special(source_resolved),
                    Some((super::absorb::SpecialKind::Any, _))
                ) {
                    let result = self.any_index_signature_object(&[
                        crate::semantic_query::PrimitiveKind::String,
                        crate::semantic_query::PrimitiveKind::Number,
                        crate::semantic_query::PrimitiveKind::Symbol,
                    ]);
                    record_utility_edges(result);
                    return (QueryResult::Value(result), fence, false);
                }
                let surface = match self.object_filter_source_surface(source_resolved) {
                    Some(view) => view,
                    None => {
                        let result = self.opaque(QueryError::Miss);
                        record_utility_edges(result);
                        return (QueryResult::Value(result), fence, false);
                    }
                };
                let omit_set: FxHashSet<&str> = omit_names.iter().map(|s| s.as_ref()).collect();
                // `Omit<C, K>` = `Pick<C, Exclude<keyof C, K>>` — a PUBLIC-keyspace
                // projection. Filter non-public source members BEFORE the name
                // predicate so an `Omit` over a class never LEAVES a non-public
                // member published (the keyspace `Omit` derives from is
                // public-only). The non-public members stay recorded on the
                // source surface for the keep-all `native_props` carrier; only
                // this derivation is gated.
                let kept: Vec<SurfaceMember> = surface
                    .members
                    .iter()
                    .filter(|m| m.visibility.is_public())
                    .filter(|m| !omit_set.contains(m.name.as_ref()))
                    .cloned()
                    .collect();
                // `Omit<T, K>` over a property surface leaves call/construct
                // signatures intact (TS mapped-type semantics touch only named
                // properties). For a Vue EMIT interface the events are call
                // signatures whose first parameter is a string-literal event
                // NAME, so omitting an event name must drop the matching call
                // signature(s) — the call-sig event name is the conceptual key.
                // A signature whose first parameter is NOT a string literal in
                // `omit_set` (any non-emit call signature) is unaffected.
                let kept_call_signatures =
                    self.filter_omitted_event_signatures(&surface.call_signatures, &omit_set);
                let kept_construct_signatures =
                    self.filter_omitted_event_signatures(&surface.construct_signatures, &omit_set);
                let result_surface = SurfaceView {
                    members: Arc::from(kept.into_boxed_slice()),
                    call_signatures: kept_call_signatures,
                    construct_signatures: kept_construct_signatures,
                    index_signatures: Arc::clone(&surface.index_signatures),
                    keyspace: surface.keyspace,
                    has_index_signature: surface.has_index_signature,
                };
                let result = graph.intern_node(SemanticNodeData::Object(result_surface));
                record_utility_edges(result);
                (QueryResult::Value(result), fence, false)
            }

            // ---- Union-filter utilities ----
            // `Extract<T, U>` keeps each member of `T`'s union that is
            // assignable to `U`; `Exclude<T, U>` keeps each member that
            // is NOT assignable to `U`. Both delegate per-member
            // assignability to the relation engine (`relate_nodes`),
            // which already decides literal-vs-literal equality
            // (`literals_equal`) plus the broader assignability
            // lattice. The reduction reconstitutes the survivors as a
            // canonical Union via
            // `intern_normalized_union_or_intersection`, which yields
            // `Primitive(Never)` for an empty survivor set and the
            // single member directly for a one-element survivor.
            //
            // When the source `T` does not resolve to a Union /
            // Literal / Primitive after `evaluate_deferred_semantic_node`
            // OR any per-member relation returns `Unknown`, the
            // utility falls through to the deferred shell so callers
            // re-dispatch once the inputs become decidable. This
            // preserves the prior `Opaque(Miss)` semantics for
            // genuinely-undecidable shapes (TypeParam, Conditional
            // shells, opaque carriers) while closing the literal-type
            // case the `mapped_types` seed exercises.
            //
            // `Exclude<U, F>` / `Extract<U, F>` concrete-literal
            // reduction. The relation engine's union-distribution path
            // already handles each per-member judgement; this arm
            // composes those judgements into the utility's result.
            "Extract" | "Exclude" if args.len() == 2 => {
                let source_arg = args[0];
                let filter_arg = args[1];
                // Context-propagating deferred
                // resolution (see Pick comment above for chain).
                let source_resolved =
                    self.evaluate_deferred_semantic_node_with_context(source_arg, context);
                let source_data = graph.node_data(source_resolved);
                let arms: Vec<SemanticNodeId> = match source_data.as_deref() {
                    Some(SemanticNodeData::Union(members)) => members.iter().copied().collect(),
                    Some(SemanticNodeData::Literal(_) | SemanticNodeData::Primitive(_)) => {
                        vec![source_resolved]
                    }
                    _ => {
                        // Source did not resolve to a decidable shape;
                        // fall through to the deferred shell.
                        let result = self.opaque(QueryError::Miss);
                        record_utility_edges(result);
                        return (QueryResult::Value(result), fence, false);
                    }
                };
                drop(source_data);
                let keep_assignable = name == "Extract";
                let mut survivors: Vec<SemanticNodeId> = Vec::with_capacity(arms.len());
                for arm in arms.iter().copied() {
                    let (relation, _arm_fence) = self.relate_nodes(arm, filter_arg);
                    match relation {
                        crate::semantic_query::RelationResult::Assignable { .. } => {
                            if keep_assignable {
                                survivors.push(arm);
                            }
                        }
                        crate::semantic_query::RelationResult::NotAssignable => {
                            if !keep_assignable {
                                survivors.push(arm);
                            }
                        }
                        crate::semantic_query::RelationResult::Unknown => {
                            // Any undecidable arm forces the whole
                            // utility result to defer — partial
                            // reduction would silently drop
                            // information.
                            let result = self.opaque(QueryError::Miss);
                            record_utility_edges(result);
                            return (QueryResult::Value(result), fence, false);
                        }
                    }
                }
                let result = self.intern_normalized_union_or_intersection(&survivors, true);
                record_utility_edges(result);
                (QueryResult::Value(result), fence, false)
            }

            // `NonNullable<T>` strips `null` / `undefined` from a
            // SETTLED operand: a settled union filters its nullish
            // arms (empty result ⇒ `never`); a settled non-nullable
            // shape (function / object / literal / intersection /
            // template literal / non-nullish primitive) passes
            // through; nullish primitives reduce to `never`. An
            // UNSETTLED operand (a carrier, an unresolved shape, a
            // union with an unsettled arm) keeps the deferred
            // `Opaque(Miss)` shell — the demand points re-dispatch
            // once the operand settles (the conditional oracle's
            // carrier-check materialisation relies on this arm to
            // close `NonNullable<ChatSlots["header"]>` to the
            // member's function shape).
            "NonNullable" if args.len() == 1 => {
                use crate::semantic_query::PrimitiveKind;
                // Context-propagating deferred resolution (see Pick comment
                // above for the chain): an alias / indexed-access shell
                // operand (`NonNullable<T["items"]>`) settles to its
                // canonical shape before the nullish filter; a genuinely
                // unsettled operand returns its carrier and keeps the
                // deferred shell below.
                let arg = self.evaluate_deferred_semantic_node_with_context(args[0], context);
                // `NonNullable<T>` is `T & {}`: `unknown & {}` collapses to
                // the empty-object base (the trap row — NOT `unknown`, NOT
                // `never`). `any`/`never` pass through the settled arms
                // below (`any & {}` = `any`; distribution over `never`
                // collapses).
                if matches!(
                    self.peek_special(arg),
                    Some((
                        crate::project_semantic_dispatch::absorb::SpecialKind::Unknown,
                        _
                    ))
                ) {
                    let result = graph.intern_node(SemanticNodeData::Object(
                        crate::project_semantic_dispatch::walk::empty_surface_view(),
                    ));
                    record_utility_edges(result);
                    return (QueryResult::Value(result), fence, false);
                }
                let nullish = |id: SemanticNodeId| {
                    matches!(
                        graph.node_data(id).as_deref(),
                        Some(SemanticNodeData::Primitive(
                            PrimitiveKind::Null | PrimitiveKind::Undefined
                        ))
                    )
                };
                let settled_non_nullable = |id: SemanticNodeId| {
                    matches!(
                        graph.node_data(id).as_deref(),
                        Some(
                            SemanticNodeData::Function { .. }
                                | SemanticNodeData::Object(_)
                                | SemanticNodeData::Literal(_)
                                | SemanticNodeData::Intersection(_)
                                | SemanticNodeData::TemplateLiteral { .. }
                                | SemanticNodeData::Primitive(_)
                                | SemanticNodeData::Array { .. }
                                | SemanticNodeData::Tuple { .. }
                        )
                    ) && !nullish(id)
                };
                let reduced: Option<SemanticNodeId> = match graph.node_data(arg).as_deref() {
                    Some(SemanticNodeData::Union(arms))
                        if arms.iter().all(|a| nullish(*a) || settled_non_nullable(*a)) =>
                    {
                        let survivors: Vec<SemanticNodeId> =
                            arms.iter().copied().filter(|a| !nullish(*a)).collect();
                        Some(match survivors.len() {
                            0 => {
                                graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Never))
                            }
                            1 => survivors[0],
                            _ => graph.intern_node(SemanticNodeData::Union(Arc::from(
                                survivors.into_boxed_slice(),
                            ))),
                        })
                    }
                    _ if nullish(arg) => {
                        Some(graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Never)))
                    }
                    _ if settled_non_nullable(arg) => Some(arg),
                    _ => None,
                };
                let result = reduced.unwrap_or_else(|| self.opaque(QueryError::Miss));
                record_utility_edges(result);
                (QueryResult::Value(result), fence, false)
            }

            // ---- Promise utility ----
            // `Awaited<T>` recursively unwraps `Promise<...>` carriers
            // (recognised by registry lookup on the carrier's declaration
            // identity), preserves nullish inputs (the first conditional
            // clause `T extends null | undefined ? T : ...`), distributes
            // over unions, and passes settled non-thenables through. See
            // [`Self::reduce_awaited`] for the full per-shape contract and
            // the structural-thenable scope boundary.
            "Awaited" if args.len() == 1 => {
                let result = self.reduce_awaited(args[0], context, 0);
                record_utility_edges(result);
                (QueryResult::Value(result), fence, false)
            }

            // ---- Deferred utilities ----
            // Unknown / not-yet-implemented utilities emit an
            // `Opaque(Miss)` shell anchored to the instantiate identity so
            // the origin walk remains coherent.
            _ => {
                let result = self.opaque(QueryError::Miss);
                record_utility_edges(result);
                (QueryResult::Value(result), fence, false)
            }
        }
    }

    /// Demand-point carrier resolution for a function-signature utility's
    /// SOURCE argument (`ReturnType<F>` / `Parameters<F>` /
    /// `ConstructorParameters<C>` / `InstanceType<C>`) and for the
    /// `ResolveOverloadSet` reducer's callee — every signature-demanding
    /// settlement runs through this ONE rail so the rails never diverge on
    /// the same carrier node.
    ///
    /// The deferred-shell evaluator deliberately leaves `DeclRef` /
    /// `InstantiationRef` carriers symbolic (intermediate indexed-access
    /// hops must stay carrier-shaped), but a signature utility IS a
    /// demand point: its argument's call/construct surface must settle
    /// before the signature walk can read it. Mirrors the keyspace
    /// enumerator's carrier demand point
    /// (`key_names_from_keyspace_node`): resolve the carrier through the
    /// shared `Instantiate` dispatch under the CALLER's context (a
    /// structural-transit caller carrier-stops as usual), then re-settle
    /// deferred shells. Bounded carrier-chain unwrap; an unresolvable
    /// carrier returns itself so the utility falls through to its
    /// deferred `Opaque(Miss)` shell.
    fn resolve_signature_source_carrier(
        &self,
        node: SemanticNodeId,
        context: crate::semantic_query::ProjectionReductionContext,
    ) -> SemanticNodeId {
        let mut current = self.evaluate_deferred_semantic_node_with_context(node, context);
        // Carrier chains are short (alias → DeclRef → instantiated body);
        // 8 hops mirrors the alias-peek budget.
        // bounded-loop: at most 8 carrier-resolution hops.
        for _ in 0..8 {
            let (slot, inst_args, owner_canonical) = match self
                .graph()
                .node_data(current)
                .as_deref()
            {
                Some(SemanticNodeData::DeclRef { identity }) => (
                    self.type_slot_for(
                        Arc::clone(&identity.canonical_id),
                        Arc::clone(&identity.decl_name),
                    ),
                    Arc::from(Vec::<SemanticNodeId>::new().into_boxed_slice()),
                    Arc::clone(&identity.canonical_id),
                ),
                Some(SemanticNodeData::InstantiationRef { base, args }) => (
                    self.type_slot_for(Arc::clone(&base.canonical_id), Arc::clone(&base.decl_name)),
                    Arc::clone(args),
                    Arc::clone(&base.canonical_id),
                ),
                _ => return current,
            };
            let read = self.execute_read(SemanticQueryKey::Instantiate(
                crate::semantic_query::InstantiateKey::new(
                    slot,
                    inst_args,
                    self.instantiate_context_for(&owner_canonical, context),
                ),
            ));
            // A2 signal-split: fold a genuinely-incomplete carrier resolve.
            crate::request_context::observe_component_meta_read_suppress(&read);
            let next = match read.value {
                QueryResult::Value(id) => id,
                _ => return current,
            };
            if next == current {
                return current;
            }
            current = self.evaluate_deferred_semantic_node_with_context(next, context);
        }
        current
    }

    /// Registry classification: `name` is the global lib `Promise` type
    /// (`IntrinsicRegistry` → [`IntrinsicImpl::PromiseGlobal`](crate::intrinsic_registry::IntrinsicImpl::PromiseGlobal)).
    /// The registry lookup is the lib-decl-identity rail — resolver code
    /// never matches the name string directly.
    pub(super) fn is_promise_global_name(&self, name: &str) -> bool {
        matches!(
            self.ctx
                .project_type_store()
                .intrinsic_registry()
                .lookup(name),
            crate::intrinsic_registry::IntrinsicLookup::Found(
                crate::intrinsic_registry::IntrinsicImpl::PromiseGlobal
            )
        )
    }

    /// Whether `identity` is the builtin-sentinel `Promise` carrier
    /// identity the lowering fast path interns for an unshadowed global
    /// `Promise<...>` reference.
    fn is_promise_global_identity(&self, identity: &crate::semantic_query::DeclIdentity) -> bool {
        identity.canonical_id.as_ref() == "__builtin__"
            && self.is_promise_global_name(identity.decl_name.as_ref())
    }

    /// `Awaited<T>` reduction over a SETTLED operand.
    ///
    /// Mirrors the lib conditional chain
    /// `T extends null | undefined ? T :
    ///  T extends object & { then(...): any } ? ... Awaited<V> ... : T`:
    ///
    /// - lattice extremes: `any` ⇒ `any`, `never` ⇒ `never`, `unknown` ⇒
    ///   `unknown` (no thenable branch matches; the final fallthrough
    ///   returns `T`); an `error` carrier dominates and passes through.
    /// - `null` / `undefined` pass through (the first conditional clause).
    /// - a union distributes per arm and renormalises; any undecidable arm
    ///   defers the whole reduction (partial distribution would silently
    ///   drop information).
    /// - a `Promise<V>` carrier — the builtin-sentinel `InstantiationRef`
    ///   whose declaration identity the registry classifies as
    ///   `PromiseGlobal` — recursively unwraps `V`, bounded by
    ///   [`AWAITED_UNWRAP_BUDGET`](Self::reduce_awaited) (budget exhaustion
    ///   defers to the `Opaque(Miss)` shell, never a wrong answer).
    /// - settled non-thenables (primitives, literals, template literals,
    ///   functions, tuples, arrays, objects WITHOUT a `then` member) pass
    ///   through unchanged.
    ///
    /// **Structural thenables are out of scope.** TS unwraps any object
    /// whose callable `then` member matches the awaited protocol; no
    /// corpus row requires that, so an Object surface that carries a
    /// `then` member (and every other unsettled shape — type params,
    /// conditionals, intersections, opaque carriers) keeps the deferred
    /// `Opaque(Miss)` shell rather than risking a wrong passthrough.
    fn reduce_awaited(
        &self,
        node: SemanticNodeId,
        context: crate::semantic_query::ProjectionReductionContext,
        depth: u32,
    ) -> SemanticNodeId {
        use crate::project_semantic_dispatch::absorb::SpecialKind;
        use crate::semantic_query::PrimitiveKind;

        /// Nested `Promise<Promise<...>>` unwrap ceiling. Real-world
        /// nesting is shallow (2–3 levels); the bound is a runaway fuse
        /// for adversarial self-referential carriers. Exhaustion defers.
        const AWAITED_UNWRAP_BUDGET: u32 = 32;

        if depth >= AWAITED_UNWRAP_BUDGET {
            return self.opaque(QueryError::Miss);
        }
        let resolved = self.evaluate_deferred_semantic_node_with_context(node, context);
        if let Some((kind, special)) = self.peek_special(resolved) {
            return match kind {
                // `any` / `never` / `unknown` and the dominating error
                // carrier all return the resolved operand verbatim.
                SpecialKind::Any
                | SpecialKind::Never
                | SpecialKind::Unknown
                | SpecialKind::Error => special,
            };
        }
        let Some(data) = self.graph().node_data(resolved) else {
            return self.opaque(QueryError::Miss);
        };
        match data.as_ref() {
            // Nullish passthrough — the first conditional clause.
            SemanticNodeData::Primitive(PrimitiveKind::Null | PrimitiveKind::Undefined) => resolved,
            // Settled non-thenable passthrough.
            SemanticNodeData::Primitive(_)
            | SemanticNodeData::Literal(_)
            | SemanticNodeData::TemplateLiteral { .. }
            | SemanticNodeData::Function { .. }
            | SemanticNodeData::Tuple { .. }
            | SemanticNodeData::Array { .. } => resolved,
            // An object surface unwraps only when it provably carries NO
            // `then` member — a `then`-bearing surface may be a structural
            // thenable (out of scope), so it defers instead of passing
            // through a wrong answer.
            SemanticNodeData::Object(surface) => {
                if surface
                    .members
                    .iter()
                    .any(|member| member.name.as_ref() == "then")
                {
                    self.opaque(QueryError::Miss)
                } else {
                    resolved
                }
            }
            // Union distribution: every arm must reduce; renormalise the
            // results through the shared union intern.
            SemanticNodeData::Union(members) => {
                let members = members.clone();
                drop(data);
                let mut reduced: Vec<SemanticNodeId> = Vec::with_capacity(members.len());
                for member in members.iter() {
                    let arm = self.reduce_awaited(*member, context, depth + 1);
                    if matches!(
                        self.graph().node_data(arm).as_deref(),
                        Some(SemanticNodeData::Opaque(QueryError::Miss))
                    ) {
                        return self.opaque(QueryError::Miss);
                    }
                    reduced.push(arm);
                }
                self.intern_normalized_union_or_intersection(&reduced, true)
            }
            // `Promise<V>` carrier — registry-recognised declaration
            // identity — recursively unwraps its payload.
            SemanticNodeData::InstantiationRef { base, args }
                if args.len() == 1 && self.is_promise_global_identity(base) =>
            {
                let payload = args[0];
                drop(data);
                self.reduce_awaited(payload, context, depth + 1)
            }
            // Everything else (type params, infer shells, conditionals,
            // intersections, mapped carriers, opaque shells, decl refs the
            // evaluator could not settle) keeps the deferred shell.
            _ => self.opaque(QueryError::Miss),
        }
    }

    /// Resolve `node` to the SELECTED `SemanticNodeData::Function` node via
    /// signature-kind BUCKET selection — the one shared rule for the
    /// function-signature utilities. `Parameters` / `ReturnType` read the
    /// CALL bucket; `ConstructorParameters` / `InstanceType` read the
    /// CONSTRUCT bucket.
    ///
    /// - A canonical `Function` node serves BOTH buckets (a bare
    ///   `new (...) => R` constructor type lowers through the same
    ///   `Function` carrier — the constructor-vs-function distinction is
    ///   consumed before query-time dispatch; see the `ConstructorType`
    ///   lowering arm).
    /// - An `Object` surface selects from the REQUESTED bucket only. The
    ///   surface MAY carry user-level members (a class's static surface)
    ///   and MAY carry both buckets (a call+construct hybrid) — selection
    ///   never requires the other bucket or the member list to be empty.
    /// - A multi-signature bucket is a visibility-filtered overload group
    ///   (build_typeof already hides trailing implementation signatures);
    ///   per TS, the signature utilities read the LAST visible overload.
    /// - `Alias` chains unwrap (cycle guarded).
    ///
    /// Returns `None` when the shape carries no signature in the requested
    /// bucket — callers fall through to the utility's `Opaque(Miss)` shell.
    fn select_signature_function(
        &self,
        node: SemanticNodeId,
        bucket: SignatureBucket,
    ) -> Option<SemanticNodeId> {
        let mut visited: FxHashSet<SemanticNodeId> = FxHashSet::default();
        self.select_signature_function_inner(node, bucket, &mut visited)
    }

    fn select_signature_function_inner(
        &self,
        node: SemanticNodeId,
        bucket: SignatureBucket,
        visited: &mut FxHashSet<SemanticNodeId>,
    ) -> Option<SemanticNodeId> {
        if !visited.insert(node) {
            return None;
        }
        let data = self.graph().node_data(node)?;
        match &*data {
            SemanticNodeData::Function { .. } => Some(node),
            SemanticNodeData::Alias(target) => {
                self.select_signature_function_inner(*target, bucket, visited)
            }
            SemanticNodeData::Object(surface) => {
                let group = match bucket {
                    SignatureBucket::Call => &surface.call_signatures,
                    SignatureBucket::Construct => &surface.construct_signatures,
                };
                let selected = *group.last()?;
                drop(data);
                self.select_signature_function_inner(selected, bucket, visited)
            }
            _ => None,
        }
    }

    /// Instantiate a signature-utility extraction at `unknown`: every type
    /// parameter OWNED by `function_node` that survives into `extracted`
    /// substitutes to the `unknown` primitive (TS instantiates free
    /// signature generics at `unknown` when a signature utility reads the
    /// bare generic — `ReturnType<typeof id>` for `id<T>(x: T): T` is
    /// `unknown`). A non-generic signature returns `extracted` unchanged.
    ///
    /// The `Function` node's `type_parameters` carry [`TypeParamDecl`]s
    /// (name + constraint/default), not binder node ids; the binder NODES
    /// the body's references interned are discovered by walking
    /// `extracted` for `TypeParam` nodes whose `display_name` matches a
    /// declared parameter name — consistent with the file-scoped
    /// name-keyed `TypeParam` identity the lowering uses. The walk is
    /// SHADOWING-AWARE: a function node inside `extracted` (its root
    /// included — `extracted` is not the owning signature) that
    /// re-declares the name owns every same-name binder in its subtree,
    /// so those occurrences stay generic instead of collapsing to
    /// `unknown`. Each discovered binder node substitutes through the
    /// shared binder-identity substitution (never a name-rewrite of the
    /// subtree).
    fn instantiate_free_signature_params_at_unknown(
        &self,
        function_node: SemanticNodeId,
        extracted: SemanticNodeId,
    ) -> SemanticNodeId {
        let Some(data) = self.graph().node_data(function_node) else {
            return extracted;
        };
        let SemanticNodeData::Function {
            type_parameters, ..
        } = &*data
        else {
            return extracted;
        };
        let type_parameters = Arc::clone(type_parameters);
        drop(data);
        if type_parameters.is_empty() {
            return extracted;
        }
        let unknown = self.graph().intern_node(SemanticNodeData::Primitive(
            crate::semantic_query::PrimitiveKind::Unknown,
        ));
        let mut result = extracted;
        for decl in type_parameters.iter() {
            // Shadowing-aware per-name collection: `extracted` is NOT the
            // owning signature, so a function node ANYWHERE in it (its root
            // included) that re-declares `decl.name` owns every same-name
            // binder in its subtree — those occurrences are the nested
            // binder's, never the free outer parameter being instantiated.
            for binder in self.collect_type_param_nodes_by_name(
                result,
                decl.name.as_ref(),
                /* root_is_own_signature */ false,
            ) {
                result = self.substitute_semantic_type_param(result, binder, unknown);
            }
        }
        result
    }

    /// Apply instantiation-expression type arguments
    /// (`typeof C.make<string>`) to a resolved generic signature node:
    /// each of the function's OWN type parameters substitutes positionally
    /// to the matching argument (an unfilled trailing parameter takes its
    /// declared default when present), and the instantiated signature is
    /// re-interned WITHOUT the consumed type parameters — an instantiation
    /// expression yields a non-generic signature. Returns the deferred
    /// `Opaque(Miss)` shell when the node is not a generic function or the
    /// arguments cannot satisfy the parameter list (an honest miss, never
    /// a partially-substituted signature).
    pub(super) fn apply_typeof_instantiation_args(
        &self,
        node: SemanticNodeId,
        args: &[SemanticNodeId],
    ) -> SemanticNodeId {
        let Some(data) = self.graph().node_data(node) else {
            return self.opaque(QueryError::Miss);
        };
        let SemanticNodeData::Function {
            type_parameters, ..
        } = &*data
        else {
            return self.opaque(QueryError::Miss);
        };
        let type_parameters = Arc::clone(type_parameters);
        drop(data);
        if type_parameters.is_empty() || args.len() > type_parameters.len() {
            return self.opaque(QueryError::Miss);
        }
        let mut result = node;
        for (index, decl) in type_parameters.iter().enumerate() {
            let arg = match args.get(index).copied().or(decl.default) {
                Some(arg) => arg,
                // Unsatisfied non-defaulted parameter — honest miss.
                None => return self.opaque(QueryError::Miss),
            };
            // The walked root IS the signature whose own parameters are being
            // instantiated — its own `type_parameters` entry for `decl.name`
            // must not shadow; NESTED functions re-declaring the name do.
            for binder in self.collect_type_param_nodes_by_name(
                result,
                decl.name.as_ref(),
                /* root_is_own_signature */ true,
            ) {
                result = self.substitute_semantic_type_param(result, binder, arg);
            }
        }
        // Strip the consumed type parameters — the instantiated signature
        // is non-generic.
        match self.graph().node_data(result).as_deref() {
            Some(SemanticNodeData::Function {
                params,
                return_type,
                signature_span,
                return_type_span,
                ..
            }) => self.graph().intern_node(SemanticNodeData::Function {
                params: Arc::clone(params),
                return_type: *return_type,
                type_parameters: Arc::from(
                    Vec::<crate::semantic_query::TypeParamDecl>::new().into_boxed_slice(),
                ),
                signature_span: *signature_span,
                return_type_span: *return_type_span,
            }),
            _ => result,
        }
    }

    /// Bounded subtree walk collecting the distinct `TypeParam` binder
    /// NODES under `root` whose `display_name` is `name`. Cycle-guarded
    /// via the visited set; descends the same structural child edges the
    /// substitute engine rewrites through
    /// ([`Self::substitute_semantic_type_param`]) — including
    /// `InstantiationRef` type-argument vectors, `Conditional` operands,
    /// `Mapped` sub-trees, and `MergedDecl` contributors — so every
    /// position substitution can reach is also a position collection
    /// discovers. Two deliberate divergences from that mirror: the
    /// `Function` and `Mapped` descents are NAME-shadowing-aware (below;
    /// substitution shadows by binder node identity instead, which the
    /// per-name collection decides up front), and `Infer { name }` nodes
    /// are never collected — an `infer X` DECLARES a fresh
    /// conditional-scoped binder, never an occurrence of a function's
    /// declared type parameter. The substitute engine's cross-variant
    /// Infer name-bridge is Infer-BINDER-gated in both directions, so
    /// collection-driven substitutions — whose binders come from
    /// `Function::type_parameters` and always lower as `TypeParam`
    /// shells — never rewrite `infer` declarations; the bridge fires
    /// only for its dedicated Infer-binder consumer (the Conditional
    /// reducer's infer-arm substitution, which passes an `Infer` node
    /// as the binder directly).
    ///
    /// SHADOWING-AWARE: a `Function` node whose own `type_parameters`
    /// re-declare `name` is NOT descended into — its subtree's same-name
    /// occurrences belong to the nested binder (TS lexical shadowing), so
    /// rewriting them for an OUTER parameter would corrupt the inner
    /// signature (`outer<T>(): <T>(x: T) => T` — the inner `T` survives an
    /// outer instantiation). A NON-shadowing nested function is walked in
    /// full, including its own `type_parameters[*].constraint` / `.default`
    /// nodes — those positions can reference the searched outer binder
    /// (`<U extends T>` / `<U = T>`) and the substitute engine descends
    /// into them, so collection mirrors that coverage.
    /// `root_is_own_signature` exempts the ROOT node
    /// from the shadow check: when the walked root is exactly the signature
    /// whose own parameters are being instantiated, its own declaration of
    /// `name` is the parameter being substituted, not a shadow. The
    /// file-scoped name-keyed `TypeParam` identity itself is unchanged —
    /// this stops CROSS-BINDER rewrites within one extraction only.
    fn collect_type_param_nodes_by_name(
        &self,
        root: SemanticNodeId,
        name: &str,
        root_is_own_signature: bool,
    ) -> Vec<SemanticNodeId> {
        use crate::semantic_query::IndexKey;
        let mut visited: FxHashSet<SemanticNodeId> = FxHashSet::default();
        let mut stack: Vec<SemanticNodeId> = vec![root];
        let mut found: Vec<SemanticNodeId> = Vec::new();
        while let Some(node) = stack.pop() {
            if !visited.insert(node) {
                continue;
            }
            let Some(data) = self.graph().node_data(node) else {
                continue;
            };
            match data.as_ref() {
                SemanticNodeData::TypeParam { display_name, .. } => {
                    if display_name.as_ref() == name {
                        found.push(node);
                    }
                }
                SemanticNodeData::Alias(target) => stack.push(*target),
                SemanticNodeData::Union(members) | SemanticNodeData::Intersection(members) => {
                    stack.extend(members.iter().copied());
                }
                SemanticNodeData::Array { element, .. } => stack.push(*element),
                SemanticNodeData::Tuple { elements, .. } => {
                    stack.extend(elements.iter().map(|element| element.value));
                }
                SemanticNodeData::Object(surface) => {
                    stack.extend(surface.members.iter().map(|member| member.value));
                    stack.extend(surface.call_signatures.iter().copied());
                    stack.extend(surface.construct_signatures.iter().copied());
                    for signature in surface.index_signatures.iter() {
                        stack.push(signature.key_type);
                        stack.push(signature.value_type);
                    }
                    if let Some(keyspace) = surface.keyspace {
                        stack.push(keyspace);
                    }
                }
                SemanticNodeData::Function {
                    params,
                    return_type,
                    type_parameters,
                    ..
                } => {
                    // A nested signature re-declaring `name` shadows: its
                    // subtree's `name` binders are its own — skip descent
                    // (including its own type-parameter constraint/default
                    // nodes: the re-declaring function owns its WHOLE
                    // subtree).
                    let shadows = type_parameters
                        .iter()
                        .any(|decl| decl.name.as_ref() == name)
                        && !(node == root && root_is_own_signature);
                    if shadows {
                        continue;
                    }
                    stack.extend(params.iter().map(|param| param.ty));
                    stack.push(*return_type);
                    // A non-shadowing nested signature's own type-parameter
                    // declarations can reference the searched OUTER binder in
                    // constraint/default position (`<U extends T>` /
                    // `<U = T>`). The substitute engine descends into these
                    // nodes, so collection must walk them too — otherwise
                    // `outer<T>(): <U = T>() => U` leaves `T` unspecialized
                    // when its only occurrence is the nested default.
                    for decl in type_parameters.iter() {
                        if let Some(constraint) = decl.constraint {
                            stack.push(constraint);
                        }
                        if let Some(default) = decl.default {
                            stack.push(default);
                        }
                    }
                }
                SemanticNodeData::TemplateLiteral { expressions, .. } => {
                    stack.extend(expressions.iter().copied());
                }
                SemanticNodeData::KeyOf { base } => stack.push(*base),
                SemanticNodeData::IndexedAccess { object, index } => {
                    stack.push(*object);
                    if let IndexKey::TypeNode(idx_node) = index {
                        stack.push(*idx_node);
                    }
                }
                // A generic-application carrier's type-argument vector is a
                // substitutable position (`Boxed<T>` — the carrier-preserving
                // lowering of a generic type reference); `base` is a
                // declaration identity with no binder occurrences.
                SemanticNodeData::InstantiationRef { args, .. } => {
                    stack.extend(args.iter().copied());
                }
                SemanticNodeData::Conditional {
                    check,
                    extends,
                    true_branch_ref,
                    false_branch_ref,
                    ..
                } => {
                    stack.push(*check);
                    stack.push(*extends);
                    stack.push(*true_branch_ref);
                    stack.push(*false_branch_ref);
                }
                SemanticNodeData::Mapped { source, mapper } => {
                    // The mapped SOURCE and key space evaluate in the outer
                    // scope; the VALUE / name-remap positions are inside the
                    // mapper binder's scope, so a mapped type whose OWN
                    // binder re-declares `name` owns every same-name
                    // occurrence there (TS lexical shadowing) — the same
                    // shadow stop the substitute engine applies to a
                    // shadowing mapper, decided per-name here because the
                    // binder node is what this walk is discovering.
                    stack.push(*source);
                    stack.push(mapper.key_space);
                    let mapper_shadows = self
                        .graph()
                        .node_data(mapper.parameter_node)
                        .as_deref()
                        .is_some_and(|binder| {
                            matches!(
                                binder,
                                SemanticNodeData::TypeParam { display_name, .. }
                                    if display_name.as_ref() == name
                            )
                        });
                    if !mapper_shadows {
                        stack.push(mapper.value_expr);
                        if let Some(remap) = mapper.name_remap {
                            stack.push(remap);
                        }
                    }
                }
                SemanticNodeData::MergedDecl { contributors } => {
                    stack.extend(contributors.iter().copied());
                }
                // A `BareRef` / `TypeOf` / `ImportType` carrier applies its
                // arguments at the reference site — a substitutable position the
                // substitute engine reaches, mirroring the `InstantiationRef`
                // arm above. Descend the carrier args via the shared accessor
                // (args-only; the carrier head is not resolved). The depth /
                // visited guards bound the walk; carrier args are finite interned
                // nodes (no infinite recursion via `apply_typeof_instantiation_args`,
                // which calls this fn).
                SemanticNodeData::BareRef(_)
                | SemanticNodeData::TypeOf(_)
                | SemanticNodeData::ImportType(_) => {
                    stack.extend(data.carrier_type_args().iter().copied());
                }
                _ => {}
            }
        }
        found
    }

    // (See `NormalizedTupleShape` at module scope below the impl block.)

    /// Spread-normalise a tuple's element list (the shared
    /// normalize-on-intern rule for every tuple intern site that can
    /// receive substituted / signature-derived elements):
    ///
    /// 1. **Variadic splice** — a `rest: true` element whose value is a
    ///    settled `Tuple` (through transparent aliases) splices that
    ///    tuple's elements in place, preserving the inner labels /
    ///    optional / rest markers. This is what makes a userland
    ///    `Concat<A, B> = [...A, ...B]` concatenate once `A` / `B`
    ///    substitute to concrete tuples — no utility name special-casing.
    ///    Splicing recurses into the spliced elements under a small depth
    ///    budget (nested `[...[...T]]` shapes); budget exhaustion keeps
    ///    the element verbatim.
    /// 2. **Non-trailing optional reconciliation** — an `optional` marker
    ///    followed anywhere later by a REQUIRED (non-optional, non-rest)
    ///    element converts to a REQUIRED `T | undefined` slot.
    ///    Optional-before-required is unrepresentable TS (TS1257); the
    ///    pinned tsgo materialises `[...[a?: number], string]` as
    ///    `[number | undefined, string]` (length 2). A trailing optional
    ///    run — including one followed only by a rest tail, which IS
    ///    legal (`[a?: number, ...boolean[]]`) — keeps its `?`.
    /// 3. **Sole-rest collapse** — a tuple consisting SOLELY of one
    ///    rest-of-array element IS that array (`[...E[]]` ≡ `E[]`,
    ///    the `(...args: E[])` parameters surface). The collapsed
    ///    array's `readonly` mirrors the OUTER tuple (`readonly`
    ///    parameter — pinned tsgo: `readonly [...number[]]` ≡
    ///    `readonly number[]`; `[...(readonly number[])]` ≡ MUTABLE
    ///    `number[]`), never the inner array's flag.
    ///
    /// A rest element whose value is open / unresolved (a generic, a
    /// carrier, anything not a settled tuple/array) is preserved
    /// verbatim — normalization never forces materialisation.
    pub(super) fn normalize_tuple_spread(
        &self,
        elements: &[crate::semantic_query::TupleElement],
        readonly: bool,
    ) -> NormalizedTupleShape {
        let mut out: Vec<crate::semantic_query::TupleElement> = Vec::with_capacity(elements.len());
        self.splice_tuple_elements(elements, &mut out, 0);
        // Non-trailing optional reconciliation (reverse scan: a slot's
        // conversion depends only on LATER elements; a converted slot is
        // itself required and forces conversion of earlier optionals).
        let mut required_follows = false;
        for index in (0..out.len()).rev() {
            if out[index].optional && required_follows {
                let undefined_node = self.graph().intern_node(SemanticNodeData::Primitive(
                    crate::semantic_query::PrimitiveKind::Undefined,
                ));
                let widened = self.intern_normalized_union_or_intersection(
                    &[out[index].value, undefined_node],
                    /* is_union */ true,
                );
                out[index].value = widened;
                out[index].optional = false;
            }
            if !out[index].optional && !out[index].rest {
                required_follows = true;
            }
        }
        if out.len() == 1 && out[0].rest {
            if let Some(array_node) = self.settled_node_through_aliases(out[0].value, |data| {
                matches!(data, SemanticNodeData::Array { .. })
            }) {
                if let Some(SemanticNodeData::Array {
                    element,
                    readonly: inner_readonly,
                }) = self.graph().node_data(array_node).as_deref()
                {
                    // The collapsed array's readonly mirrors the OUTER
                    // tuple; reuse the inner node only when the flags
                    // already agree. The replacement intern preserves the
                    // inner node's origin scope so `ProjectPath`
                    // self-rooting over the collapsed base still records
                    // the origin file's `(canonical, whole_hash)` root.
                    if *inner_readonly == readonly {
                        return NormalizedTupleShape::Array(array_node);
                    }
                    let element = *element;
                    return NormalizedTupleShape::Array(self.graph().intern_preserving_scope(
                        array_node,
                        SemanticNodeData::Array { element, readonly },
                    ));
                }
            }
        }
        NormalizedTupleShape::Tuple(out)
    }

    /// Recursive splice body for [`Self::normalize_tuple_spread`].
    fn splice_tuple_elements(
        &self,
        elements: &[crate::semantic_query::TupleElement],
        out: &mut Vec<crate::semantic_query::TupleElement>,
        depth: u32,
    ) {
        /// Nested `[...[...T]]` splice ceiling — real-world nesting is
        /// 1–2 levels; the bound is a runaway fuse.
        const SPLICE_DEPTH_BUDGET: u32 = 8;
        for element in elements {
            if element.rest && depth < SPLICE_DEPTH_BUDGET {
                let inner_tuple = self.settled_node_through_aliases(element.value, |data| {
                    matches!(data, SemanticNodeData::Tuple { .. })
                });
                if let Some(tuple_node) = inner_tuple {
                    if let Some(SemanticNodeData::Tuple {
                        elements: inner, ..
                    }) = self.graph().node_data(tuple_node).as_deref()
                    {
                        let inner = Arc::clone(inner);
                        self.splice_tuple_elements(&inner, out, depth + 1);
                        continue;
                    }
                }
            }
            out.push(element.clone());
        }
    }

    /// Unwrap transparent `Alias` hops (bounded) and return the settled
    /// node iff its data matches `predicate`. `None` for open /
    /// unresolved carriers — callers preserve those verbatim.
    fn settled_node_through_aliases(
        &self,
        node: SemanticNodeId,
        predicate: impl Fn(&SemanticNodeData) -> bool,
    ) -> Option<SemanticNodeId> {
        let mut current = node;
        // bounded-loop: at most 8 transparent Alias hops.
        for _ in 0..8 {
            let data = self.graph().node_data(current)?;
            match &*data {
                SemanticNodeData::Alias(target) => {
                    let next = *target;
                    drop(data);
                    current = next;
                }
                other => return predicate(other).then_some(current),
            }
        }
        None
    }

    /// Build a tuple node whose elements are the function's parameter
    /// types — the surface shape of `Parameters<F>` /
    /// `ConstructorParameters<F>`. Labels carry over from the parameter
    /// names (TS reflects them in hover); optional / rest flags track
    /// the original signature. `function_node` must be a
    /// `SemanticNodeData::Function`; returns `None` otherwise.
    fn intern_function_params_tuple(
        &self,
        function_node: SemanticNodeId,
    ) -> Option<SemanticNodeId> {
        use crate::semantic_query::TupleElement;

        let data = self.graph().node_data(function_node)?;
        let SemanticNodeData::Function { params, .. } = &*data else {
            return None;
        };
        let params = Arc::clone(params);
        drop(data);
        let elements: Vec<TupleElement> = params
            .iter()
            .map(|param| TupleElement {
                label: param.name.as_ref().map(Arc::clone),
                // An optional parameter's tuple SLOT type is
                // `T | undefined` (TS widens the optional slot), while the
                // `optional` marker below keeps the `?` surface flag —
                // both are part of the published `Parameters<F>` shape.
                value: if param.optional {
                    let undefined = self.graph().intern_node(SemanticNodeData::Primitive(
                        crate::semantic_query::PrimitiveKind::Undefined,
                    ));
                    self.intern_normalized_union_or_intersection(&[param.ty, undefined], true)
                } else {
                    param.ty
                },
                optional: param.optional,
                rest: param.rest,
            })
            .collect();
        // Normalize-on-intern: a `(...args: E[])` signature's sole rest
        // slot collapses to `E[]`; concrete rest-of-tuple slots splice.
        // The Parameters tuple is mutable.
        Some(
            match self.normalize_tuple_spread(&elements, /* readonly */ false) {
                NormalizedTupleShape::Array(array_node) => array_node,
                NormalizedTupleShape::Tuple(elements) => {
                    self.graph().intern_node(SemanticNodeData::Tuple {
                        elements: elements.into(),
                        readonly: false,
                    })
                }
            },
        )
    }

    // Declaration identity is carried directly by
    // `SemanticQueryKey::Instantiate.base` (the env-bearing content-free
    // `ResolvedDeclSlotIdentity` slot), so there is no arena node to unwrap.

    /// Single-hop union-index distribution — the `IndexedAccessUnionDistribution`
    /// reduction for `Obj[A | B]`. Resolves the index node; when it is a FINITE
    /// union whose every arm normalises to a literal key (`string` / `number`),
    /// projects `Obj[arm]` per arm through the shared `IndexedAccess` query and
    /// renormalises the results through `NormalizeUnion`. Returns `None` (fall
    /// through to the path walker) for a non-`TypeNode` index, a non-union
    /// resolved index, an open/generic union arm, or any per-arm projection
    /// miss — so symbolic / partial cases keep their carrier.
    fn distribute_union_index(
        &self,
        base: SemanticNodeId,
        index: &crate::semantic_query::IndexKey,
        context: crate::semantic_query::ProjectionReductionContext,
    ) -> Option<crate::project_semantic_dispatch::walk::QueryBuildOutput> {
        use crate::semantic_query::IndexKey;
        let IndexKey::TypeNode(index_node) = index else {
            return None;
        };
        let resolved = self.evaluate_deferred_semantic_node_with_context(*index_node, context);
        let members = match self.graph().node_data(resolved).as_deref() {
            Some(SemanticNodeData::Union(members)) => Arc::clone(members),
            _ => return None,
        };
        if members.is_empty() {
            return None;
        }
        let mut projected: Vec<SemanticNodeId> = Vec::with_capacity(members.len());
        let mut any_partial = false;
        for &member in members.iter() {
            // Each arm MUST be a concrete literal key; a non-literal arm
            // aborts the distribution (the union is not a finite key set).
            // A NUMERIC-literal arm outside the bounded integer
            // convention (`Obj[1.5 | 1e21]`, big integers with divergent
            // shortest-round-trip spellings) stays `TypeNode` by the
            // producer predicate yet IS a concrete key — keep it as the
            // `TypeNode` index so the per-arm `IndexedAccess` dispatch
            // recovers its canonical `js_number_to_string` needle
            // through the same G4.5 path as a single-key access.
            let member_index = match self.normalized_index_key_node(member) {
                key @ (IndexKey::String(_) | IndexKey::Number(_)) => key,
                IndexKey::TypeNode(resolved) => match self.graph().node_data(resolved).as_deref() {
                    Some(SemanticNodeData::Literal(LiteralValue::Number(_))) => {
                        IndexKey::TypeNode(resolved)
                    }
                    _ => return None,
                },
            };
            let read = self.execute_read(SemanticQueryKey::IndexedAccess {
                base,
                index: member_index,
                mode: context.mode,
            });
            if read.result_is_partial {
                any_partial = true;
            }
            match read.value {
                // An arm whose KEY is absent aborts the distribution —
                // the walker reports an absent member as the
                // `Opaque(Miss)` sentinel, and tsgo ERRORS on
                // `Obj[1.5 | 2.5]` when an arm has no member, so
                // fabricating a partial union of the arms that DID
                // resolve would be unsound. Falling through to the path
                // walker keeps the honest single-needle miss. An arm
                // whose key EXISTS but whose VALUE is an opaque carrier
                // (a deferred `DeclPlaceholder` shell, an unresolved
                // declaration) is NOT a miss: the per-arm read returns
                // the member's stored value verbatim — exactly what the
                // single-key `Obj['b']` access publishes — so the
                // carrier contributes to the union (per-arm single-key
                // consistency; carrier-preserving shallow-by-default).
                //
                // This classification is sound only under the sentinel
                // discipline that `Opaque(Miss)` never surfaces as an
                // EXISTING member's per-arm terminal: existing members
                // project as real value nodes or addressable carriers
                // (`DeclPlaceholder`, `DeclRef`, `InstantiationRef`,
                // `Mapped`, deferred `IndexedAccess`). Every per-key
                // producer that could forge `Opaque(Miss)` for an
                // existing key by substituting into a builtin Identity
                // utility's lazy `value_expr` placeholder dispatches
                // `source[K]` through the shared `IndexedAccess` query
                // instead — the PathWalker's Mapped narrowing (guard:
                // `identity_utility_mapped_carrier_projects_existing_members_not_miss`),
                // the Shallow walker's `synthesise_mapped_surface` (guard:
                // `identity_utility_shallow_empty_path_surface_publishes_source_member_values_not_miss`),
                // and `build_mapped_type`'s per-key value selection (guard:
                // `identity_mapped_build_without_projectable_source_publishes_addressable_carrier_not_miss`).
                QueryResult::Value(id)
                    if !matches!(
                        self.graph().node_data(id).as_deref(),
                        Some(SemanticNodeData::Opaque(
                            crate::semantic_query::QueryError::Miss
                        ))
                    ) =>
                {
                    projected.push(id)
                }
                _ => return None,
            }
        }
        let norm = self.execute_read(SemanticQueryKey::NormalizeUnion {
            members: Arc::from(projected.into_boxed_slice()),
        });
        match norm.value {
            QueryResult::Value(id) => {
                let observed_self_roots = self.observed_self_roots_from_nodes([base]);
                let mut out = crate::project_semantic_dispatch::walk::QueryBuildOutput::from((
                    QueryResult::Value(id),
                    self.project_generation_signature(),
                ))
                .with_observed_self_roots(observed_self_roots);
                out.result_is_partial = any_partial || norm.result_is_partial;
                Some(out)
            }
            _ => None,
        }
    }

    /// Path-precise projection. Walks each [`PathSegment`]
    /// from `base` via a fresh [`PathWalker`] that dispatches per-hop on
    /// every shell variant (`Object`, `Union`, `Intersection`,
    /// `Conditional`, `Alias`) and emits per-segment origin edges
    /// (`ProjectMember` / `ProjectIndex` / `AliasResolve` /
    /// `ConditionalSelect`). An empty path returns `base` directly —
    /// that is the canonical form of "expand the whole surface" (the
    /// retired `Expand` variant).
    ///
    /// Alias-cycle detection terminates with
    /// `Opaque(QueryError::AliasCycle)`; stack depth is additionally
    /// bounded by [`PathWalker::max_depth`]. Open conditionals
    /// distribute the remaining path into both branches via
    /// `SemanticQueryApi::execute` re-entry so each branch-projection
    /// is a separately memoised sub-query.
    ///
    /// Emits a whole-path `ProjectPath` edge on the result (when the
    /// result differs from the base) so consumers can recover the
    /// entry path without rebuilding it from per-segment edges.
    pub(super) fn build_project_path(
        &self,
        base: SemanticNodeId,
        path: &Arc<[PathSegment]>,
        context: crate::semantic_query::ProjectionReductionContext,
    ) -> crate::project_semantic_dispatch::walk::QueryBuildOutput {
        // §22 fast-reject for the `?[K]` indexed-access shape: `any[K]=any`,
        // `never[K]=never`, `unknown[K]`=UNCONDITIONAL error, `error[K]=error`.
        // Member projection (`.foo`) is a distinct surface left to the walker.
        if Self::project_path_is_indexed_access(path) {
            if let Some(absorbed) = self.absorb_indexed_access(base) {
                return absorbed;
            }
        }
        // Union-index distribution (`Obj[A | B]` = `Obj[A] | Obj[B]`). A
        // single-hop indexed access whose index resolves to a FINITE union of
        // literal keys distributes per-arm through the shared `IndexedAccess`
        // query and renormalises. Multi-segment paths, non-union indices, and
        // open/generic union arms fall through to the path walker unchanged
        // (one engine — this re-dispatches the shared key, never hand-reduces).
        if let [PathSegment::Index(index)] = path.as_ref() {
            if let Some(distributed) = self.distribute_union_index(base, index, context) {
                return distributed;
            }
        }
        let fence = self.project_generation_signature();
        self.graph().record_path_length(path.len() as u32);
        // Longest-prefix-first peek. Skip when path.len() < 2.
        // Prefix entries are cached as Navigate regardless
        // of the caller's mode (path-precise rule — intermediate hops are
        // Navigate, terminal hop is the caller's mode).
        let (start_base, start_index) = if path.len() < 2 {
            (base, 0usize)
        } else {
            find_longest_warm_prefix(self.graph(), self.ctx, base, path).unwrap_or((base, 0))
        };
        let walker_path: Arc<[PathSegment]> = if start_index == 0 {
            Arc::clone(path)
        } else {
            Arc::from(path[start_index..].to_vec().into_boxed_slice())
        };
        // The walker carries the full `ProjectionReductionContext`
        // (not just `mode`) so the empty-path Shallow synthesiser can
        // gate per-key Mapped surface materialisation on the demand
        // axis. Published(Shallow) walks enumerate & substitute mapped
        // members; StructuralTransit walks carrier-stop without
        // enumeration (per the boundary constraint that transit is the
        // non-publication rail).
        let mut walker = PathWalker::new(self, context, &fence);
        let result = walker.walk(start_base, walker_path.as_ref());
        // Drain the walker's diagnostics + cache_suppress flag so the
        // memo no-poison contract sees them at admission time.
        let walker_diagnostics: Vec<crate::project_semantic_dispatch::walk::ShallowDiagnostic> =
            std::mem::take(&mut walker.walker_diagnostics);
        let cache_suppress = walker.cache_suppress;
        let result_is_partial = walker.result_is_partial;
        // Supplement §5.D.0 r17 — surface a budget-exceeded
        // sentinel as `QueryResult::Recursive` so §5.D.4
        // `no_cache_promotion_for_budget_exceeded_*` callers can
        // discriminate via a type-level `matches!(_, Recursive(_))`
        // check. The walker itself emits an `Opaque(RecursiveRef)`
        // node id; we surface it through the QueryResult variant so
        // the result is NOT cached as a warm `Value` (per CLAUDE.md
        // "cancelled / superseded / interrupted / budget-exceeded
        // ... must not be promoted as warm shared cache entries").
        if let Some(crate::semantic_query::SemanticNodeData::Opaque(
            crate::semantic_query::QueryError::RecursiveRef { .. },
        )) = self.graph().node_data(result).as_deref()
        {
            return crate::project_semantic_dispatch::walk::QueryBuildOutput {
                result: QueryResult::Recursive(result),
                dep_signature: fence,
                walker_diagnostics,
                cache_suppress,
                result_is_partial,
                taint: crate::semantic_query::ResultTaint::Clean,
                observed_self_roots: Vec::new(),
                graph_carrier: None,
                self_root_canonicals: Arc::from([]),
                pending_prefix_backfills: Vec::new(),
                // Recursive / budget-exceeded sentinel — never warm-published,
                // so it records nothing the §3.4 gate could reuse.
                satisfied_projection: crate::semantic_query::demand::MaterializedSet::empty(),
            };
        }
        // Emit a whole-path `ProjectPath` edge on the result so consumers
        // can recover the entry path without rebuilding it from per-hop
        // edges.
        if result != base {
            self.graph().record_origin_edge(
                result,
                OriginEdgeKind::ProjectPath,
                Arc::from(vec![base].into_boxed_slice()),
                OriginMeta::Path(Arc::clone(path)),
                Arc::clone(&fence),
            );
        }
        // Collect intermediate path-prefix backfill records so a
        // sibling dispatch sharing the same prefix can short-circuit
        // through `find_longest_warm_prefix`. Backfill always targets
        // Navigate (path-precise rule — intermediate hops are
        // Navigate-mode entries). The terminal full-path key keeps the
        // caller's mode and is published by `execute_cooperative`'s
        // admission flow, not by this helper.
        //
        // Carrier-aware publication: the records accumulate onto
        // `QueryBuildOutput.pending_prefix_backfills` and the shared
        // cold-build helper publishes them AFTER `install_fact_tracer`
        // returns Ok so each backfilled memo entry's carrier holds the
        // parent's authoritative path-precise fact signature. Publishing
        // here before the tracer finalises would attach a legacy-only
        // signature derived from the fence (the pre-carrier behaviour
        // flagged in `publish_warm_if_absent`).
        let pending_prefix_backfills =
            collect_prefix_backfills(start_base, &walker_path, &walker.intermediate_nodes);
        // §3.4 materialised-record set for the TERMINAL entry: the
        // terminal point at the FULL path (the caller's terminal mode)
        // PLUS one `Demand::navigate(prefix)` per CONTIGUOUS LINEAR walked
        // intermediate (§3.5). This is what the compute ACTUALLY
        // materialised — a deep terminal that only `Navigate`-walked its
        // intermediates records a `Navigate` point there, NEVER the
        // terminal mode it never expanded at the prefix. The navigate-hop
        // points are inert for THIS family's warm-hit gate (every request
        // to family `(base, full_path)` is at `full_path`, so only the
        // terminal point can match), but they record the honest
        // materialisation per §3.4 and never inflate a prefix to the
        // terminal mode.
        let satisfied_projection =
            path_walk_materialized_set(path, context.mode, start_index, &walker.intermediate_nodes);
        // Self-version rooting: the projection result depends on the
        // file content the projection `base` was lowered from. The
        // base node's origin scope (recorded in the arena sidecar)
        // names that file and the version observed at intern time —
        // root the memo entry on it so a content edit to the base's
        // file misses the warm read.
        let observed_self_roots = self.observed_self_roots_from_nodes([base]);
        crate::project_semantic_dispatch::walk::QueryBuildOutput {
            result: QueryResult::Value(result),
            dep_signature: fence,
            walker_diagnostics,
            cache_suppress,
            result_is_partial,
            taint: crate::semantic_query::ResultTaint::Clean,
            observed_self_roots,
            graph_carrier: None,
            self_root_canonicals: Arc::from([]),
            pending_prefix_backfills,
            satisfied_projection,
        }
    }

    /// `keyof` projection. For an `Object` surface, materializes a union of
    /// the member names as `Primitive(String)` anchors — this matches the
    /// TS semantics that `keyof T` yields a union of string literals.
    /// For non-objects, returns `Opaque(Miss)`.
    ///
    /// Emits one `ProjectMember` edge per keyspace literal back to the
    /// source object base, carrying the member name in
    /// `OriginMeta::ProjectedMember` with provenance
    /// `MemberEdgeProvenance::KeyOfEnumerated`. The edge lets walkers
    /// reconstruct which source member each keyspace literal derives from.
    pub(super) fn build_key_of(
        &self,
        base: SemanticNodeId,
        context: crate::semantic_query::ProjectionReductionContext,
    ) -> crate::project_semantic_dispatch::walk::QueryBuildOutput {
        // §22 fast-reject: `keyof any`/`keyof never` = `string|number|symbol`,
        // `keyof unknown` = `never`, `keyof error` = `error`. A lattice-extreme
        // base resolves to a fully-determined keyspace regardless of mode, so
        // this runs before the carrier-stop and the structural keyspace walk.
        if let Some(absorbed) = self.absorb_key_of(base) {
            return absorbed;
        }
        let data = self.graph().node_data(base);
        let fence = self.project_generation_signature();
        // demand-driven reducer carrier-stop: when the
        // caller's context is not a publication-mode-Expanded demand,
        // return a deferred `KeyOf { base }` carrier instead of
        // reifying the keyspace as a literal-anchor union with one
        // `ProjectMember` edge per literal. Member-anchor reification
        // is publication-only work — relation-engine binding, generic
        // substitution, and other structural-transit callers consume
        // the carrier without paying for it. The check is purely on
        // `context`; no operand-name inspection.
        if !crate::semantic_query::may_reduce_operator(context) {
            let node = self.graph().intern_node(SemanticNodeData::KeyOf { base });
            let observed_self_roots = self.observed_self_roots_from_nodes([base]);
            return crate::project_semantic_dispatch::walk::QueryBuildOutput::from((
                QueryResult::Value(node),
                fence,
            ))
            .with_observed_self_roots(observed_self_roots);
        }
        // Two-signal fold: a `keyof` over an Intersection/Union enumerates
        // its keyspace through `member_names_for_published_projection`,
        // whose underlying ProjectPath read can be genuinely incomplete
        // (budget / recursion / walker-fatal). Carry that partiality onto
        // the published keyof surface.
        let mut keyof_is_partial = false;
        let node = match data.as_deref() {
            // `keyof ClassType` yields only public keys (TS semantics): a
            // private/protected member is not part of the keyspace. Filter
            // non-public members out here, the direct-Object keyof chokepoint.
            Some(SemanticNodeData::Object(surface)) => self.intern_keyspace_names(
                base,
                surface
                    .members
                    .iter()
                    .filter(|member| member.visibility.is_public())
                    .map(|member| Arc::clone(&member.name)),
                &fence,
            ),
            Some(SemanticNodeData::Intersection(_) | SemanticNodeData::Union(_)) => self
                .member_names_for_published_projection(base)
                .map(|(names, is_partial)| {
                    keyof_is_partial |= is_partial;
                    names
                })
                .or_else(|| self.key_names_from_base_node(base))
                .map(|names| self.intern_keyspace_names(base, names, &fence))
                .unwrap_or_else(|| self.graph().intern_node(SemanticNodeData::KeyOf { base })),
            // Declaration Merging (CRITICAL): `keyof <merged decl>` routes
            // through the single peer-merge reducer to the merged
            // `Object`/`Intersection` surface, then re-runs THIS keyspace
            // reducer on it — `keyof` is a `MergedDecl` consumer of
            // `reduce_merged_decl` (a bare `Intersection` is forbidden as the
            // merged-decl representation, so the Intersection arm must not own
            // the merge). The reduced surface is freshly interned with no file
            // scope of its own, and the `MergedDecl` carrier is scoped only to
            // its base/importing file — but an AUGMENTED `MergedDecl`
            // (cross-file `declare module` / `declare global`) carries
            // contributor nodes lowered in AUGMENTER file scopes (Declaration
            // Augmentation CRITICAL). Root the entry on `base` PLUS every
            // contributor node, deduped — the same self-root collection the
            // peer-merge surface roots on. A SAME-FILE merge's contributors are
            // all base-scoped, so this folds back to `[base]`; for a cross-file
            // augmented merge it additionally records each augmenter's
            // `FileWholeHash`, so an edit to ANY contributor file (base OR
            // augmenter) misses the warm keyof entry. Nested-read partiality
            // (an incomplete heritage projection) folds through the
            // re-dispatched output verbatim.
            Some(SemanticNodeData::MergedDecl { contributors }) => {
                let merged = self.reduce_merged_decl(contributors);
                let observed_self_roots = self.observed_self_roots_from_nodes(
                    std::iter::once(base).chain(contributors.iter().copied()),
                );
                return self
                    .build_key_of(merged, context)
                    .with_observed_self_roots(observed_self_roots);
            }
            Some(
                SemanticNodeData::TypeParam { .. }
                | SemanticNodeData::IndexedAccess { .. }
                | SemanticNodeData::Mapped { .. }
                | SemanticNodeData::TypeOf(_)
                | SemanticNodeData::Conditional { .. }
                | SemanticNodeData::Alias(_)
                // Un-resolved reference carriers. Navigate / Skeleton body
                // lowering deliberately preserves a no-args named reference
                // as a `DeclRef` / `InstantiationRef` shell (cycle-BFS
                // visibility — the carrier-preservation branch in
                // `lower.rs`), so a COLD reduce-demanded keyof can receive
                // one as its operand. Per this builder's contract
                // (documented at the `materialize_through_aliases` KeyOf
                // bridge arm), an un-resolved reference operand returns the
                // DEFERRED carrier — the bridge surfaces the operand through
                // the shared empty-path projection and re-dispatches —
                // rather than degrading the whole reduction to a
                // `semanticMiss` terminal.
                | SemanticNodeData::DeclRef { .. }
                | SemanticNodeData::InstantiationRef { .. }
                | SemanticNodeData::Opaque(QueryError::DeclPlaceholder { .. }),
            ) => self.graph().intern_node(SemanticNodeData::KeyOf { base }),
            _ => self.opaque(QueryError::Miss),
        };
        // Self-version rooting: `keyof base` depends on `base`'s member
        // surface, which was lowered from the file `base` originates in.
        // Root the memo entry on that file's observed content version.
        let observed_self_roots = self.observed_self_roots_from_nodes([base]);
        let mut keyof_output = crate::project_semantic_dispatch::walk::QueryBuildOutput::from((
            QueryResult::Value(node),
            fence,
        ))
        .with_observed_self_roots(observed_self_roots);
        keyof_output.result_is_partial = keyof_is_partial;
        keyof_output
    }

    pub(super) fn intern_keyspace_names<I>(
        &self,
        base: SemanticNodeId,
        names: I,
        fence: &DepSignature,
    ) -> SemanticNodeId
    where
        I: IntoIterator<Item = Arc<str>>,
    {
        let mut seen = FxHashSet::default();
        let member_literals: Vec<(SemanticNodeId, Arc<str>)> = names
            .into_iter()
            .filter(|name| seen.insert(Arc::clone(name)))
            .map(|name| {
                let lit =
                    self.graph()
                        .intern_node(SemanticNodeData::Literal(LiteralValue::String(
                            name.as_ref().to_string(),
                        )));
                (lit, name)
            })
            .collect();
        for (lit_id, name) in &member_literals {
            self.graph().record_origin_edge(
                *lit_id,
                OriginEdgeKind::ProjectMember,
                Arc::from(vec![base].into_boxed_slice()),
                OriginMeta::ProjectedMember {
                    name: Arc::clone(name),
                    provenance: verter_audit::MemberEdgeProvenance::KeyOfEnumerated,
                },
                Arc::clone(fence),
            );
        }
        let ids: Vec<SemanticNodeId> = member_literals.into_iter().map(|(id, _)| id).collect();
        if ids.is_empty() {
            self.graph()
                .intern_node(SemanticNodeData::Primitive(PrimitiveKind::Never))
        } else if ids.len() == 1 {
            ids[0]
        } else {
            self.graph()
                .intern_node(SemanticNodeData::Union(Arc::from(ids.into_boxed_slice())))
        }
    }

    pub(super) fn uses_synthetic_mapped_key_names(&self, members: &[SurfaceMember]) -> bool {
        !members.is_empty()
            && members.iter().all(|member| {
                member.name.strip_prefix("key_").is_some_and(|suffix| {
                    !suffix.is_empty() && suffix.chars().all(|ch| ch.is_ascii_digit())
                })
            })
    }

    /// Resolve a source carrier to the one-level published member surface.
    ///
    /// Mapped-type publication needs full [`SurfaceMember`] records, not
    /// names alone: identity mappers (`Partial<T>` / `Required<T>` /
    /// `Readonly<T>`) must reuse each source member's `value`, while
    /// modifier-preserving mappers inherit `optional` and `readonly`.
    /// The global key-name enumerators intentionally return names only
    /// and do not unwrap `DeclRef` / `InstantiationRef` carriers, so
    /// publication code uses this local surface helper when a source is
    /// not already an `Object`.
    ///
    /// Non-public members are EXCLUDED from the result. This is the keyof /
    /// mapped / Pick keyspace chokepoint: TypeScript's `keyof ClassType`
    /// yields only public keys, so `Partial<ClassWithPrivate>` /
    /// `Pick<ClassWithPrivate, K>` / `{ [K in keyof ClassWithPrivate]: V }`
    /// must not carry the non-public members at all. The keep-all native
    /// surface (`native_props`) reads the member surface DIRECTLY (it does
    /// not route through this published-projection helper), so it is
    /// unaffected by this filter.
    /// Returns the source's public members for a published projection,
    /// PLUS the `result_is_partial` flag of the underlying `ProjectPath`
    /// read (two-signal fold). A genuinely-incomplete projection (budget /
    /// recursion / walker-fatal) surfaces a complete-looking member list
    /// here; the bool carries that partiality so `build_mapped_type` can
    /// taint its published surface. The fast path over an already-resolved
    /// `Object` node is always complete (`false`).
    pub(super) fn source_members_for_published_projection(
        &self,
        source: SemanticNodeId,
    ) -> Option<(Vec<SurfaceMember>, bool)> {
        fn public_members(members: &[SurfaceMember]) -> Vec<SurfaceMember> {
            members
                .iter()
                .filter(|member| member.visibility.is_public())
                .cloned()
                .collect()
        }

        if let Some(SemanticNodeData::Object(view)) = self.graph().node_data(source).as_deref() {
            return Some((public_members(&view.members), false));
        }

        let read = self.execute_read(SemanticQueryKey::ProjectPath {
            base: source,
            path: Arc::from(Vec::<PathSegment>::new().into_boxed_slice()),
            context: crate::semantic_query::ProjectionReductionContext::published(
                ProjectionMode::Shallow,
            ),
        });
        crate::meta_resolve::emit_dispatch_dep_signature_facts(self.ctx, &read.dep_signature);
        let read_is_partial = read.result_is_partial;
        let node = match read.value {
            QueryResult::Value(id) => id,
            QueryResult::Recursive(_) | QueryResult::Error(_) => return None,
        };
        match self.graph().node_data(node).as_deref() {
            Some(SemanticNodeData::Object(view)) => {
                Some((public_members(&view.members), read_is_partial))
            }
            _ => None,
        }
    }

    fn member_names_for_published_projection(
        &self,
        source: SemanticNodeId,
    ) -> Option<(Vec<Arc<str>>, bool)> {
        self.source_members_for_published_projection(source)
            .map(|(members, is_partial)| {
                (
                    members.into_iter().map(|member| member.name).collect(),
                    is_partial,
                )
            })
    }

    /// Mapped-type rewrite.
    ///
    /// For a mapped type `{ [K in key_space]: value_expr }` with
    /// optional / readonly modifiers (stored on the `MapperKey` and
    /// participating in the cache key):
    ///
    /// 1. Carrier-stops run first: outside a publication demand
    ///    (`may_reduce_operator(context)` is false), or when the
    ///    produced surface still depends on an unbound OUTER generic
    ///    (`mapped_type_is_open_or_unknown` — the route/mode-independent
    ///    L1 Shallow-By-Default rule), return the deferred
    ///    `SemanticNodeData::Mapped { source, mapper }` carrier with one
    ///    `Normalize` edge over the contribution set.
    /// 2. Enumerate the key domain: the source's projected member names
    ///    when the source surfaces an Object (synthetic-name keyspaces
    ///    reroute through the keyspace node's literals), else the
    ///    keyspace node's literal union. Neither enumerable → the
    ///    deferred `Mapped` carrier again.
    /// 3. For each key, reserve a member slot. Member optionality /
    ///    readonly derive from the mapper's modifiers (`Add` → always
    ///    on, `Remove` → always off, `Keep` → inherit from the source
    ///    if available, else default off).
    /// 4. Member values dispatch on `mapper.kind`. An `Identity` mapper
    ///    (the canonical `{ [K in keyof T]: T[K] }` behind `Partial` /
    ///    `Required` / `Readonly`) reuses the matching source member's
    ///    value directly; a key without a projectable source member
    ///    dispatches `source[K]` through the shared `IndexedAccess`
    ///    query, publishing the ADDRESSABLE deferred `IndexedAccess`
    ///    carrier when the access cannot close — never the builtin
    ///    mapper's `Opaque(Miss)` `value_expr` shell marker. A
    ///    `Computed` mapper substitutes the binder and evaluates via
    ///    [`Self::materialize_mapped_member_value_for_key`]
    ///    (K-independent value bodies hoist one shared evaluation above
    ///    the loop), falling back to the substituted carrier on `Opaque`
    ///    so the free binder never leaks onto the published surface.
    /// 5. Apply the `as`-clause `name_remap` per key via
    ///    [`Self::mapped_member_name_remap_outcome`]: `Drop` filters the
    ///    key; duplicate produced names union their per-key values;
    ///    `DeferCarrier` fails the whole mapped type closed back to the
    ///    deferred `Mapped` carrier.
    /// 6. Emit one `Normalize` edge from the mapped result over the full
    ///    contribution set (`[source, key_space, value_expr,
    ///    name_remap?]`) and one `ProjectMember` edge per produced
    ///    member value sourcing `[source, key_space]` with
    ///    `OriginMeta::ProjectedMember` (provenance
    ///    `MemberEdgeProvenance::MappedKeyEnumerated`) carrying the
    ///    produced name (post-remap if `name_remap` is set).
    ///
    /// The `mapper: MapperKey` participates in the `SemanticQueryKey`
    /// hash so different modifier / value-expression combinations
    /// intern distinct entries — enforced by
    /// `mapped_type_optionality_and_readonly_modifiers_in_cache_key`.
    ///
    /// Mapper-value classification is done at lowering time, not at
    /// build time. Every [`MapperKey`](crate::semantic_query::MapperKey)
    /// carries a stable [`MapperKind`](crate::semantic_query::MapperKind)
    /// tag (see
    /// [`crate::semantic_query::MapperKind::classify_value_expr`]);
    /// `build_mapped_type` matches on `mapper.kind` directly rather
    /// than re-classifying the runtime AST shape.
    pub(super) fn build_mapped_type(
        &self,
        source: SemanticNodeId,
        mapper: &crate::semantic_query::MapperKey,
        context: crate::semantic_query::ProjectionReductionContext,
    ) -> crate::project_semantic_dispatch::walk::QueryBuildOutput {
        // §22 fast-reject on the mapped SOURCE: over `any` ⇒ `any`; over
        // `never` ⇒ `{}`; over `error` ⇒ `error`; a direct mapping over
        // `unknown` is illegal ⇒ error. Runs before key-space enumeration.
        if let Some(absorbed) = self.absorb_mapped(source) {
            return absorbed;
        }
        let graph = self.graph();
        let fence = self.project_generation_signature();
        // Mapped contribution set: the `source` surface, the `key_space`,
        // the `value_expr` — plus the `as`-clause `name_remap` when
        // present. The remap is a first-class decision input (the
        // open-mapped carrier-stop consults it; the per-key remap
        // evaluation consumes it), so it participates in BOTH the memo
        // entry's observed self-roots (a remap-only edit must reject the
        // warm entry on the read-side validator — the R6/R21 invalidation
        // rail) AND the structural `Normalize` origin edges below.
        let contribution_nodes: Vec<SemanticNodeId> = [source, mapper.key_space, mapper.value_expr]
            .into_iter()
            .chain(mapper.name_remap)
            .collect();
        // Self-version rooting: all contribution nodes are
        // already-interned. Root the memo entry on the file content
        // version each file-derived input was lowered from.
        let observed_self_roots =
            self.observed_self_roots_from_nodes(contribution_nodes.iter().copied());

        // demand-driven reducer carrier-stop: outside a
        // publication-mode-Expanded demand the build returns a
        // deferred `Mapped { source, mapper }` carrier without
        // enumerating the source's key space or emitting per-member
        // `ProjectMember` edges. Member materialisation is publication
        // work; structural-transit callers (relation engine, deferred-
        // shell evaluation, generic substitution) inspect the carrier
        // directly. The decision is purely on `context` — no
        // operand-name inspection.
        if !crate::semantic_query::may_reduce_operator(context) {
            let node = graph.intern_node(SemanticNodeData::Mapped {
                source,
                mapper: mapper.clone(),
            });
            // Capture the contribution set on a single `Normalize`
            // edge so origin-graph consumers still see the structural
            // dependency; per-member edges only emit on the
            // publication path below.
            graph.record_origin_edge(
                node,
                OriginEdgeKind::Normalize,
                Arc::from(contribution_nodes.clone().into_boxed_slice()),
                OriginMeta::None,
                Arc::clone(&fence),
            );
            return crate::project_semantic_dispatch::walk::QueryBuildOutput::from((
                QueryResult::Value(node),
                fence,
            ))
            .with_observed_self_roots(observed_self_roots);
        }

        // Route/mode-INDEPENDENT L1 (Shallow-By-Default), MAPPED-TYPE
        // family. Under a publication demand (`Published` /
        // `MacroObjectSurface`, where `may_reduce_operator` is true) a
        // mapped type whose produced surface still depends on an unbound
        // OUTER generic — an open source / key space, or a value body /
        // name remap reaching the outer generic (NOT the bound mapper
        // binder `K`) — must NOT enumerate its keys and materialise the
        // per-key value. That per-key value loop over an open value body
        // is the `ChatMessagesSlots<T>` / `TableSlots<T>` storm across
        // `node_modules`. Instead return the COMPLETE deferred `Mapped`
        // carrier verbatim (a shallow shell preserving source / key space
        // / value / name-remap); consumers re-resolve on demand. A CLOSED
        // mapped type (`Partial`/`Required`/`Readonly`, `{ [K in keyof
        // Closed]: Closed[K] }`, a K-only transform, a finite keyspace)
        // falls through to enumerate path-precisely.
        if crate::project_semantic_dispatch::raise::mapped_type_is_open_or_unknown(
            self, source, mapper,
        ) {
            let node = graph.intern_node(SemanticNodeData::Mapped {
                source,
                mapper: mapper.clone(),
            });
            graph.record_origin_edge(
                node,
                OriginEdgeKind::Normalize,
                Arc::from(contribution_nodes.clone().into_boxed_slice()),
                OriginMeta::None,
                Arc::clone(&fence),
            );
            return crate::project_semantic_dispatch::walk::QueryBuildOutput::from((
                QueryResult::Value(node),
                fence,
            ))
            .with_observed_self_roots(observed_self_roots);
        }

        // 1. Resolve the key space.
        //
        // TS semantics: `{ [K in keyof T]: V }` walks T's member names.
        // When `source` is an Object, its member names ARE the correct
        // keys — even if `mapper.key_space` was pre-computed as a union
        // of string-literal primitives (the current graph model has no
        // literal-type PrimitiveKind, so we recover names from the
        // source directly). If `source` is not an Object we can read
        // member names from, fall back to the keyspace shape — but
        // opaque keyspaces terminate the mapped dispatch cleanly.
        // Two-signal fold partiality accumulator. Folds the partiality of
        // every nested subquery this mapped-type build surfaces (the
        // source-member ProjectPath, the K-independent value hoist's
        // nested Instantiate) into the published surface so a
        // genuinely-incomplete (budget / recursion / walker-fatal) input
        // taints the mapped result even though it surfaces as a complete
        // `Value`.
        let mut mapped_is_partial = false;
        let (source_members, source_members_partial): (Vec<SurfaceMember>, bool) = self
            .source_members_for_published_projection(source)
            .unwrap_or_default();
        mapped_is_partial |= source_members_partial;
        let source_member_keys = |members: &[SurfaceMember]| {
            super::enumerate::KeyDomainKey::from_names(
                members.iter().map(|m| Arc::clone(&m.name)).collect(),
            )
        };
        let keys: Vec<super::enumerate::KeyDomainKey> = if !source_members.is_empty() {
            if self.uses_synthetic_mapped_key_names(&source_members) {
                match self.key_literals_from_keyspace_node(mapper.key_space) {
                    Some(keys) => keys,
                    None => source_member_keys(&source_members),
                }
            } else {
                source_member_keys(&source_members)
            }
        } else if let Some(keys) = self.key_literals_from_keyspace_node(mapper.key_space) {
            keys
        } else {
            // Change M: `KeyEnumeration::Unresolvable`. Neither the
            // source surface nor the key space enumerate to concrete names.
            // The canonical form is a deferred
            // `SemanticNodeData::Mapped { source, mapper }` shell — callers
            // can re-dispatch through `MappedType` once one of the inputs
            // becomes enumerable. One `Normalize` edge captures the
            // contribution set (`[source, key_space, value_expr]`); the
            // `mapper.name_remap` field is preserved verbatim via the
            // interned `mapper` key.
            //
            // This replaces the retired `Alias(KeyOf(source))` surrogate.
            // The surrogate reinterpreted the mapped result AS its keyspace
            // (a relation confusion), which no downstream consumer could
            // safely navigate. `SemanticNodeData::Mapped` is the
            // dispatch-native deferred form.
            let node = graph.intern_node(SemanticNodeData::Mapped {
                source,
                mapper: mapper.clone(),
            });
            graph.record_origin_edge(
                node,
                OriginEdgeKind::Normalize,
                Arc::from(contribution_nodes.clone().into_boxed_slice()),
                OriginMeta::None,
                Arc::clone(&fence),
            );
            let mut deferred_output =
                crate::project_semantic_dispatch::walk::QueryBuildOutput::from((
                    QueryResult::Value(node),
                    fence,
                ))
                .with_observed_self_roots(observed_self_roots.clone());
            deferred_output.result_is_partial = mapped_is_partial;
            return deferred_output;
        };

        // 2. Build member slots.
        //
        // For each key K, the mapped member's VALUE type comes from
        // the value expression. The fast path — reading
        // `source_member.value` directly from the enumerated source
        // object — is ONLY valid when `mapper.kind` is
        // `MapperKind::Identity` (the canonical
        // `{ [K in keyof T]: T[K] }` pattern behind `Partial<T>` /
        // `Required<T>` / `Readonly<T>`). For any other value shape
        // (e.g. `keyof T['variants'][K]`, `ExtendSlotWithPlan<TPlan, K>`,
        // `infer`-bearing conditional bodies), classification yields
        // `MapperKind::Computed` and the value goes through the
        // substitute-and-evaluate path: intern `Literal(name)`,
        // substitute the mapper parameter in `mapper.value_expr`,
        // then evaluate the substituted node. Evaluation yielding
        // `Opaque(_)` publishes the un-evaluated substituted node so
        // the value stays addressable by path re-dispatch.
        //
        // Identity vs Computed is decided once at lowering time and
        // carried on `mapper.kind`; the build path reads the tag
        // directly rather than re-inspecting the value-expression AST
        // at runtime.
        let value_is_identity = matches!(mapper.kind, crate::semantic_query::MapperKind::Identity);
        // Key-space-independent value hoist. When the mapper's binder
        // (`mapper.parameter_node`) is not structurally reachable
        // inside `mapper.value_expr`, the per-K substitution collapses
        // to the identity (`substitute_with_change_tracking` returns
        // `(node, false)` for the whole subtree), so every K's
        // substituted carrier IS `value_expr` itself. Reduce
        // `value_expr` ONCE under the caller's context and reuse the
        // result for every enumerated key. Skipping this hoist is
        // correct but wasteful: each K otherwise re-walks the same
        // `evaluate_deferred_*` chain (Conditional / IndexedAccess /
        // Mapped re-dispatch + any nested cross-package import
        // resolution embedded in the body) even though the underlying
        // dispatch sees identical inputs across K.
        //
        // The hoist is gated on `!value_is_identity` because the
        // identity fast-path reads `source_member.value` directly and
        // never enters the materialiser. The check declines on any
        // structural reference to the binder so the per-K materialiser
        // remains the authority for K-dependent value expressions
        // (`T[K]`-style, `K`-keyed `IndexedAccess`, conditional checks
        // on `K`, etc.). Cross-variant `infer`-name matches are also
        // counted as references (see `subtree_references_node`'s
        // contract), preventing over-aggressive hoisting on
        // `infer`-bearing value expressions.
        //
        // Correctness mirror: when the hoist applies the substitution
        // for the K-independent case IS the identity, so the per-K
        // materialiser would compute exactly the same evaluation we
        // perform here. The `InstantiationRef` redispatch and
        // `Opaque` fallback below mirror
        // `materialize_mapped_member_value_for_key` exactly so both
        // paths produce the same surface member value.
        let value_expr_is_k_independent = !value_is_identity
            && !self.subtree_references_node(mapper.value_expr, mapper.parameter_node);
        let shared_value: Option<SemanticNodeId> = if value_expr_is_k_independent {
            let evaluated =
                self.evaluate_deferred_semantic_node_with_context(mapper.value_expr, context);
            let resolved = match graph.node_data(evaluated).as_deref() {
                Some(SemanticNodeData::InstantiationRef { base, args }) => {
                    let slot = self
                        .type_slot_for(Arc::clone(&base.canonical_id), Arc::clone(&base.decl_name));
                    let inst_ctx = self.instantiate_context_for(&base.canonical_id, context);
                    let args = Arc::clone(args);
                    let inst_read = self.execute_read(SemanticQueryKey::Instantiate(
                        crate::semantic_query::InstantiateKey::new(slot, args, inst_ctx),
                    ));
                    if inst_read.result_is_partial {
                        mapped_is_partial = true;
                    }
                    match inst_read.value {
                        QueryResult::Value(id) => id,
                        _ => evaluated,
                    }
                }
                _ => evaluated,
            };
            let final_value = if matches!(
                graph.node_data(resolved).as_deref(),
                Some(SemanticNodeData::Opaque(_))
            ) {
                // Mirror the materialiser's `Opaque` fallback. For the
                // K-independent case the substituted carrier IS
                // `value_expr` itself (substitution is the identity),
                // so reusing `value_expr` here matches the per-K
                // materialiser exactly: the free binder cannot leak
                // because there is no reference to leak in the first
                // place.
                mapper.value_expr
            } else {
                resolved
            };
            Some(final_value)
        } else {
            None
        };
        let mut produced: Vec<SurfaceMember> = Vec::with_capacity(keys.len());
        let mut project_member_edges: Vec<(SemanticNodeId, Arc<str>)> = Vec::new();
        // A key whose `as` remap fails closed (`DeferCarrier`) taints the whole
        // mapped type: it returns the deferred `Mapped` carrier rather than a
        // torn surface (set inside the loop, checked after).
        let mut remap_defers = false;
        for key in &keys {
            let name = &key.name;
            let source_member = source_members.iter().find(|m| &m.name == name);
            let optional = match mapper.optionality {
                crate::semantic_query::OptionalityMod::Add => true,
                crate::semantic_query::OptionalityMod::Remove => false,
                crate::semantic_query::OptionalityMod::Keep => {
                    source_member.map(|m| m.optional).unwrap_or(false)
                }
            };
            let readonly = match mapper.readonly {
                crate::semantic_query::ReadonlyMod::Add => true,
                crate::semantic_query::ReadonlyMod::Remove => false,
                crate::semantic_query::ReadonlyMod::Keep => {
                    source_member.map(|m| m.readonly).unwrap_or(false)
                }
            };
            // Value selection branches:
            //
            // - `source_members` matches this key AND `mapper.kind` is
            //   `Identity` → use the member value directly
            //   (Partial/Required/Readonly-style mapped types).
            // - `mapper.kind` is `Identity` but the source surface did
            //   NOT project a matching member (non-Object source whose
            //   key space still enumerated, synthetic-name keyspace
            //   reroute) → the per-key value is STILL `source[K]` by
            //   definition: dispatch the shared `IndexedAccess` query,
            //   never a substitution into `mapper.value_expr` — the
            //   builtin Identity mapper carries the lazy `Opaque(Miss)`
            //   placeholder there, and substituting into it forges
            //   `Opaque(Miss)` as an EXISTING member's published value.
            //   An access the query cannot close publishes the
            //   ADDRESSABLE deferred `IndexedAccess` carrier instead.
            // - `value_expr` does NOT reference `mapper.parameter_node`
            //   → reuse `shared_value` (the once-per-mapped-type
            //   evaluation hoisted above the loop). Both correctness
            //   and the substituted-carrier fallback collapse to the
            //   identical surface member value the per-K materialiser
            //   would produce.
            // - Otherwise (`value_expr` is not `T[K]` and IS K-dependent,
            //   or a Computed mapper's source has no matching member) →
            //   defer to the shared per-key materialiser
            //   [`Self::materialize_mapped_member_value_for_key`].
            //   That helper substitutes the binder, evaluates under the
            //   caller's `context` (publication callers reify; transit
            //   callers carrier-stop), and falls back to the substituted
            //   carrier on `Opaque` so the free mapper binder never leaks
            //   onto the published surface. The Shallow walker's
            //   `synthesise_mapped_surface` calls the same helper at the
            //   Published(Shallow) macro publication boundary so both
            //   paths converge on identical per-key semantics.
            let value = if let (Some(source_member), true) = (source_member, value_is_identity) {
                source_member.value
            } else if value_is_identity {
                let key_node = graph.intern_node(SemanticNodeData::Literal(key.literal.clone()));
                let read = self.execute_read(SemanticQueryKey::IndexedAccess {
                    base: source,
                    index: IndexKey::TypeNode(key_node),
                    mode: ProjectionMode::Navigate,
                });
                if read.result_is_partial {
                    mapped_is_partial = true;
                }
                match read.value {
                    QueryResult::Value(id)
                        if !matches!(
                            graph.node_data(id).as_deref(),
                            Some(SemanticNodeData::Opaque(_))
                        ) =>
                    {
                        id
                    }
                    _ => graph.intern_node(SemanticNodeData::IndexedAccess {
                        object: source,
                        index: IndexKey::TypeNode(key_node),
                    }),
                }
            } else if let Some(shared) = shared_value {
                shared
            } else {
                self.materialize_mapped_member_value_for_key(mapper, &key.literal, context)
            };
            // Apply `name_remap` (the `as <expr>` clause) via the shared
            // [`Self::mapped_member_name_remap_outcome`] classifier — same
            // substitution + context-aware evaluation the Shallow walker uses.
            // `Drop` filters the key, `Keys` emits one member per produced
            // name, `DeferCarrier` fails the whole mapped type closed.
            let produced_names = match self.mapped_member_name_remap_outcome(mapper, key, context) {
                MappedKeyRemapOutcome::Keep(n) => vec![n],
                MappedKeyRemapOutcome::Keys(ns) => ns,
                MappedKeyRemapOutcome::Drop => continue,
                MappedKeyRemapOutcome::DeferCarrier => {
                    remap_defers = true;
                    break;
                }
            };
            for produced_name in produced_names {
                // Duplicate produced names UNION their per-K values: the
                // numeric key `1` and the string key `"1"` address the
                // SAME property, each contributing its own kind (pinned
                // tsgo, probe12: `{ [K in 1 | "1"]: K }` = `{ 1: 1 | "1" }`
                // — both first-wins and last-wins were falsified). The
                // first production keeps the member slot (position,
                // modifiers, and declaration site); later same-name
                // productions fold their value into a Union arm.
                if let Some(existing) = produced.iter_mut().find(|m| m.name == produced_name) {
                    if existing.value != value {
                        existing.value = graph.intern_node(SemanticNodeData::Union(Arc::from(
                            vec![existing.value, value].into_boxed_slice(),
                        )));
                    }
                    project_member_edges.push((value, produced_name));
                    continue;
                }
                // Rationale on [`mapped_produced_name_inherits_declaration_site`]
                // — the one shared predicate both rails judge inheritance with.
                let identity_source = source_member.filter(|_| {
                    mapped_produced_name_inherits_declaration_site(
                        produced_name.as_ref(),
                        name.as_ref(),
                    )
                });
                // SAFETY: mapped-type member synthesis (e.g.,
                // `Partial<T>` / `Required<T>` / `{ [K in S]: V }`).
                // Members reach the surface via the mapped construction,
                // NOT via own-body declaration in any consuming macro's
                // T body. The construction layer is structurally
                // heritage-equivalent — `false` is the truth.
                produced.push(SurfaceMember {
                    name: Arc::clone(&produced_name),
                    value,
                    optional,
                    readonly,
                    is_method: false,
                    // Mapped-type produced member. The key domain is already
                    // public-only (non-public class members are filtered out of
                    // the keyspace at `source_members_for_published_projection` /
                    // `key_names_step`), so every produced member is public. For
                    // the homomorphic case (`{ [K in keyof T]: T[K] }` / Partial /
                    // Required / Readonly) thread the matched source member's
                    // (public) visibility verbatim so the invariant is preserved
                    // even if the keyspace gate is ever bypassed; otherwise the
                    // synthesized member is `Public`.
                    visibility: source_member
                        .map_or(verter_type_expr::MemberVisibility::Public, |m| m.visibility),
                    declared_in_macro_type_arg: crate::semantic_query::MacroOwnBodyStamp::NEUTRAL,
                    // Mapped-type produced members are synthesized by the mapped
                    // construction, never an interface/class heritage overlay —
                    // `Authored` (they do not participate in own-body shadowing).
                    merge_role: crate::semantic_query::MergeRoleStamp::NEUTRAL,
                    spans: identity_source.map(|m| m.spans).unwrap_or_default(),
                    declaration_origin: identity_source.and_then(|m| m.declaration_origin.clone()),
                });
                project_member_edges.push((value, produced_name));
            }
        }

        // Fail closed: a key whose remap could not resolve to a finite key set
        // returns the deferred `Mapped` carrier (callers re-dispatch once the
        // remap becomes decidable) — never a torn partial surface.
        if remap_defers {
            let node = graph.intern_node(SemanticNodeData::Mapped {
                source,
                mapper: mapper.clone(),
            });
            graph.record_origin_edge(
                node,
                OriginEdgeKind::Normalize,
                Arc::from(contribution_nodes.clone().into_boxed_slice()),
                OriginMeta::None,
                Arc::clone(&fence),
            );
            let mut deferred_output =
                crate::project_semantic_dispatch::walk::QueryBuildOutput::from((
                    QueryResult::Value(node),
                    fence,
                ))
                .with_observed_self_roots(observed_self_roots);
            deferred_output.result_is_partial = mapped_is_partial;
            return deferred_output;
        }

        let view = SurfaceView {
            members: Arc::from(produced.into_boxed_slice()),
            call_signatures: Arc::from(Vec::<SemanticNodeId>::new().into_boxed_slice()),
            construct_signatures: Arc::from(Vec::<SemanticNodeId>::new().into_boxed_slice()),
            index_signatures: Arc::from(Vec::<IndexSignature>::new().into_boxed_slice()),
            keyspace: Some(mapper.key_space),
            has_index_signature: false,
        };
        let node = graph.intern_node(SemanticNodeData::Object(view));

        // 3. Emit origin edges.
        //    - Normalize: result ← [source, key_space, value_expr,
        //      name_remap?] (the full contribution set).
        //    - ProjectMember per produced member.
        graph.record_origin_edge(
            node,
            OriginEdgeKind::Normalize,
            Arc::from(contribution_nodes.clone().into_boxed_slice()),
            OriginMeta::None,
            Arc::clone(&fence),
        );
        for (value_id, name) in project_member_edges {
            graph.record_origin_edge(
                value_id,
                OriginEdgeKind::ProjectMember,
                Arc::from(vec![source, mapper.key_space].into_boxed_slice()),
                OriginMeta::ProjectedMember {
                    name,
                    provenance: verter_audit::MemberEdgeProvenance::MappedKeyEnumerated,
                },
                Arc::clone(&fence),
            );
        }

        let mut mapped_output = crate::project_semantic_dispatch::walk::QueryBuildOutput::from((
            QueryResult::Value(node),
            fence,
        ))
        .with_observed_self_roots(observed_self_roots);
        mapped_output.result_is_partial = mapped_is_partial;
        mapped_output
    }

    /// Per-key Mapped member value materialiser — the SHARED
    /// substrate used by both [`Self::build_mapped_type`]
    /// (Published(Expanded) publication path) and the Shallow walker's
    /// `synthesise_mapped_surface` (Published(Shallow) surface
    /// synthesis at the macro publication boundary).
    ///
    /// Substitutes the mapper binder (`mapper.parameter_node`) with the
    /// enumerated key's ORIGINAL literal (kind-preserving: a numeric
    /// keyspace member substitutes `Literal::Number`, never its
    /// stringified member name — pinned tsgo, probe12:
    /// `{ [K in 1]: K }` = `{ 1: 1 }`) inside `mapper.value_expr`, then
    /// materialises the substituted node only enough to close the
    /// selected key under the caller's `context`:
    ///
    /// 1. [`Self::evaluate_deferred_semantic_node_with_context`] walks
    ///    Alias / KeyOf / IndexedAccess / Mapped / Conditional / TypeOf
    ///    / TemplateLiteral / DeclPlaceholder hops. Under
    ///    [`crate::semantic_query::ReductionDemand::StructuralTransit`]
    ///    every `KeyOf` / `MappedType` re-dispatch carrier-stops via
    ///    [`crate::semantic_query::may_reduce_operator`]; `Conditional`
    ///    reduction is gated separately by the relation engine and
    ///    still fires when the post-substitution check is concrete.
    /// 2. When the evaluator leaves an `InstantiationRef` shell (the
    ///    deferred-shell evaluator deliberately stops at carrier
    ///    boundaries — the relation-engine identity-carrier rule),
    ///    dispatch `SemanticQueryKey::Instantiate` so the substituted
    ///    args bind into the body. Under the caller's transit demand
    ///    the body lowering inherits
    ///    `may_reduce_operator(context) == false`, so nested `KeyOf`/
    ///    `Mapped` operators stay carrier-shaped; only `Conditional`
    ///    reduces. This is what turns `ExtendSlotWithPlan<TPlan,
    ///    "badge">` into the `Function` surface the slot-binding
    ///    extractor reads.
    /// 3. When materialisation returns `Opaque`, fall back to the
    ///    SUBSTITUTED carrier (the InstantiationRef with the binder
    ///    replaced) — NEVER the original `mapper.value_expr`, which
    ///    still contains the free mapper binder. The substituted
    ///    carrier remains addressable by path re-dispatch and never
    ///    leaks the free binder onto the consumer surface.
    pub(super) fn materialize_mapped_member_value_for_key(
        &self,
        mapper: &crate::semantic_query::MapperKey,
        key_literal: &LiteralValue,
        context: crate::semantic_query::ProjectionReductionContext,
    ) -> SemanticNodeId {
        self.graph().record_mapped_per_k_materialization();
        let key_arg = self
            .graph()
            .intern_node(SemanticNodeData::Literal(key_literal.clone()));
        // Instrumentation — classify this per-K call as
        // unique or repeated based on the identity tuple a typed
        // mapped-member materialization cache would key on. The
        // classifier records the tuple in the per-request observed
        // set and bumps the matching unique/repeated counter pair.
        if let Some(ctx) = crate::request_context::current_request_context() {
            ctx.classify_mapped_member_materialization(
                crate::request_context::MappedMemberIdentity {
                    parameter_node: mapper.parameter_node.0,
                    value_expr: mapper.value_expr.0,
                    key_node: key_arg.0,
                    context_bits: encode_projection_reduction_context_bits(context),
                    variant: 0,
                },
            );
        }
        let substituted =
            self.substitute_semantic_type_param(mapper.value_expr, mapper.parameter_node, key_arg);
        let evaluated = self.evaluate_deferred_semantic_node_with_context(substituted, context);
        let resolved = match self.graph().node_data(evaluated).as_deref() {
            Some(SemanticNodeData::InstantiationRef { base, args }) => {
                let slot =
                    self.type_slot_for(Arc::clone(&base.canonical_id), Arc::clone(&base.decl_name));
                let inst_ctx = self.instantiate_context_for(&base.canonical_id, context);
                let args = Arc::clone(args);
                let read = self.execute_read(SemanticQueryKey::Instantiate(
                    crate::semantic_query::InstantiateKey::new(slot, args, inst_ctx),
                ));
                // Two-signal fold: per-K materialiser returns a bare node, so
                // fold a genuinely-incomplete nested Instantiate onto the
                // request's sticky partial flag.
                crate::request_context::observe_component_meta_read_suppress(&read);
                match read.value {
                    QueryResult::Value(id) => id,
                    _ => evaluated,
                }
            }
            _ => evaluated,
        };
        if matches!(
            self.graph().node_data(resolved).as_deref(),
            Some(SemanticNodeData::Opaque(_))
        ) {
            substituted
        } else {
            resolved
        }
    }

    /// Per-key Mapped member value materialiser with explicit
    /// **Selected-Key Transit Realization**: drives one extra
    /// evaluate dispatch after the per-K substitution so a generic
    /// helper body that lowers to a `Conditional` reduces to its
    /// realised arm before reaching the publication boundary.
    ///
    /// Extends [`Self::materialize_mapped_member_value_for_key`] by
    /// dispatching one more
    /// [`Self::evaluate_deferred_semantic_node_with_context`] on the
    /// post-`Instantiate` body. The Instantiate of a Conditional-bodied
    /// generic helper (e.g.
    /// `ExtendSlotWithPlan<TPlan, K> = PricingPlanSlots[K] extends
    /// (props: infer P) => unknown ? ... : ...`) returns the body
    /// `Conditional` carrier WITHOUT triggering Conditional reduction;
    /// the per-key materialiser's caller would see a shell where
    /// `Function` was expected. The trailing evaluate dispatches the
    /// evaluator's `Conditional` arm
    /// ([`crate::project_semantic_dispatch::evaluate`]: 142) →
    /// `SemanticQueryKey::Conditional` →
    /// [`Self::build_conditional`]'s **infer-binding path**
    /// (build.rs: 2266) → `evaluate_deferred_semantic_node(check)`
    /// at default `Published(Expanded)` → IndexedAccess evaluator arm
    /// (evaluate.rs: 95) which hard-codes `ProjectionMode::Navigate`
    /// for the index re-dispatch, so the `PricingPlanSlots["badge"]`
    /// **selected-index / path projection** reduces independently of
    /// the caller's `StructuralTransit(Navigate)` demand. The infer
    /// binding then closes the conditional to a `Function`.
    ///
    /// dispatch chain (slot fixture):
    /// ```text
    /// DefineSlots payload lowered StructuralTransit(Navigate)
    ///   → ProjectPath([], Published(Shallow))
    ///     → InstantiationRef body
    ///       → Mapped shallow synthesis
    ///         → selected key "badge"
    ///           → substitute K = "badge"
    ///             → instantiate ExtendSlotWithPlan<PricingPlan, "badge">
    ///                                      under StructuralTransit(Navigate)
    ///               → conditional check resolves PricingPlanSlots["badge"]
    ///                            through selected-index/path projection
    ///                 → infer binds P
    ///                   → true branch becomes a Function.
    /// ```
    ///
    /// `Opaque` fallback policy: when the trailing evaluate returns
    /// `Opaque`, the result is whatever the inner per-key materialiser
    /// produced (which already falls back to the substituted carrier on
    /// its own Opaque-handling — the free mapper binder is never
    /// leaked).
    ///
    /// Call sites (publication-only):
    /// - [`crate::project_semantic_dispatch::walk::ProjectPathContext::synthesise_mapped_surface`]
    /// - [`crate::project_semantic_dispatch::walk::PathWalker`]'s
    ///   mapped-type literal-key narrowing arm (walk.rs ≈ 980).
    pub(super) fn materialize_selected_key_mapped_value(
        &self,
        mapper: &crate::semantic_query::MapperKey,
        key_literal: &LiteralValue,
        context: crate::semantic_query::ProjectionReductionContext,
    ) -> SemanticNodeId {
        let key_arg = self
            .graph()
            .intern_node(SemanticNodeData::Literal(key_literal.clone()));
        self.materialize_selected_key_mapped_value_with_node(mapper, key_arg, context)
    }

    /// Node-keyed variant of
    /// [`Self::materialize_selected_key_mapped_value`] — the PathWalker
    /// mapped-type literal-key narrowing arm passes a pre-interned key
    /// literal node so the String / Number kind survives substitution
    /// (G4-series soundness — `M[1]` substitutes
    /// `K = Literal::Number(1)`, NOT `Literal::String("1")`).
    ///
    /// Factors the common substrate path with
    /// [`Self::materialize_mapped_member_value_for_key`] so both
    /// publication-side callers (synthesise + PathWalker narrowing)
    /// route through the same substitute → evaluate → Instantiate →
    /// **trailing Conditional reduction** chain. The trailing
    /// evaluate is what distinguishes the selected-key helper from
    /// the plain mapped-value materialiser: the plain helper stops at
    /// the Instantiate boundary, leaving the body's Conditional shell
    /// addressable by re-dispatch but unrealised; this selected-key
    /// helper drives Conditional reduction inline so consumers see
    /// the closed `Function` (or projected member).
    pub(super) fn materialize_selected_key_mapped_value_with_node(
        &self,
        mapper: &crate::semantic_query::MapperKey,
        key_arg: SemanticNodeId,
        context: crate::semantic_query::ProjectionReductionContext,
    ) -> SemanticNodeId {
        self.graph().record_mapped_per_k_materialization();
        // Instrumentation — selected-key variant.
        if let Some(ctx) = crate::request_context::current_request_context() {
            ctx.classify_mapped_member_materialization(
                crate::request_context::MappedMemberIdentity {
                    parameter_node: mapper.parameter_node.0,
                    value_expr: mapper.value_expr.0,
                    key_node: key_arg.0,
                    context_bits: encode_projection_reduction_context_bits(context),
                    variant: 1,
                },
            );
        }
        let substituted =
            self.substitute_semantic_type_param(mapper.value_expr, mapper.parameter_node, key_arg);
        let evaluated = self.evaluate_deferred_semantic_node_with_context(substituted, context);
        let resolved = match self.graph().node_data(evaluated).as_deref() {
            Some(SemanticNodeData::InstantiationRef { base, args }) => {
                let slot =
                    self.type_slot_for(Arc::clone(&base.canonical_id), Arc::clone(&base.decl_name));
                let inst_ctx = self.instantiate_context_for(&base.canonical_id, context);
                let args = Arc::clone(args);
                let read = self.execute_read(SemanticQueryKey::Instantiate(
                    crate::semantic_query::InstantiateKey::new(slot, args, inst_ctx),
                ));
                // Two-signal fold: fold a genuinely-incomplete nested
                // Instantiate onto the request's sticky partial flag.
                crate::request_context::observe_component_meta_read_suppress(&read);
                match read.value {
                    QueryResult::Value(id) => id,
                    _ => evaluated,
                }
            }
            _ => evaluated,
        };
        // Selected-Key Transit Realization: drive Conditional
        // reduction on the post-Instantiate body. The evaluator's
        // Conditional arm dispatches `SemanticQueryKey::Conditional`,
        // which routes through `build_conditional`'s nested-infer
        // binding path — the check operand evaluates via
        // `evaluate_deferred_semantic_node` (default
        // `Published(Expanded)`) which dispatches `IndexedAccess` in
        // `Navigate` mode. The check operand (e.g.
        // `PricingPlanSlots["badge"]`) reduces to a `Function` even
        // though the caller's demand is `StructuralTransit`, and the
        // nested-infer arm binds the `infer P` then substitutes into
        // the true branch — closing the conditional to a `Function`
        // so consumers see the realised body rather than the shell.
        let realized = self.evaluate_deferred_semantic_node_with_context(resolved, context);
        if matches!(
            self.graph().node_data(realized).as_deref(),
            Some(SemanticNodeData::Opaque(_))
        ) {
            // Trailing Conditional reduction stalled (e.g. the
            // relation engine could not bind / the check operand was
            // not yet enumerable). Preserve the per-key
            // `materialize_mapped_member_value_for_key` substrate's
            // Opaque-fallback contract: hand the substituted carrier
            // back to the caller so the free mapper binder is never
            // leaked but the value stays addressable by re-dispatch.
            if matches!(
                self.graph().node_data(resolved).as_deref(),
                Some(SemanticNodeData::Opaque(_))
            ) {
                substituted
            } else {
                resolved
            }
        } else {
            realized
        }
    }

    /// K-independent variant of
    /// [`Self::materialize_selected_key_mapped_value`] — used by the
    /// Shallow walker's `synthesise_mapped_surface` hoist path when
    /// `mapper.value_expr` contains no structural reference to the
    /// binder. Substitution would be the identity, so the helper skips
    /// substitution entirely and dispatches the same evaluate →
    /// Instantiate → trailing Conditional reduction chain on
    /// `mapper.value_expr` directly. The Opaque fallback returns the
    /// original `mapper.value_expr` (the per-K materialiser's
    /// "substituted carrier" reduces to `value_expr` for the
    /// K-independent case), keeping the surface value addressable by
    /// re-dispatch without leaking the free binder — which cannot
    /// leak because there is no reference to leak in the first place.
    pub(super) fn materialize_selected_key_mapped_value_k_independent(
        &self,
        mapper: &crate::semantic_query::MapperKey,
        context: crate::semantic_query::ProjectionReductionContext,
    ) -> SemanticNodeId {
        let evaluated =
            self.evaluate_deferred_semantic_node_with_context(mapper.value_expr, context);
        let resolved = match self.graph().node_data(evaluated).as_deref() {
            Some(SemanticNodeData::InstantiationRef { base, args }) => {
                let slot =
                    self.type_slot_for(Arc::clone(&base.canonical_id), Arc::clone(&base.decl_name));
                let inst_ctx = self.instantiate_context_for(&base.canonical_id, context);
                let args = Arc::clone(args);
                let read = self.execute_read(SemanticQueryKey::Instantiate(
                    crate::semantic_query::InstantiateKey::new(slot, args, inst_ctx),
                ));
                // Two-signal fold: fold a genuinely-incomplete nested
                // Instantiate onto the request's sticky partial flag.
                crate::request_context::observe_component_meta_read_suppress(&read);
                match read.value {
                    QueryResult::Value(id) => id,
                    _ => evaluated,
                }
            }
            _ => evaluated,
        };
        let realized = self.evaluate_deferred_semantic_node_with_context(resolved, context);
        if matches!(
            self.graph().node_data(realized).as_deref(),
            Some(SemanticNodeData::Opaque(_))
        ) {
            if matches!(
                self.graph().node_data(resolved).as_deref(),
                Some(SemanticNodeData::Opaque(_))
            ) {
                mapper.value_expr
            } else {
                resolved
            }
        } else {
            realized
        }
    }

    /// Source-surface enumeration for the Shallow walker's
    /// `synthesise_mapped_surface` ONLY.
    ///
    /// Transit-shallow publication (leak-fix-3b) — closes the
    /// architectural gap that prevented the macro-publication boundary
    /// from migrating from `Published(Expanded)` to
    /// `StructuralTransit(Navigate)`.
    ///
    /// **Why this helper, not an extension of `key_names_step`**: the
    /// global key-name enumerators
    /// (`key_names_from_base_node`, `key_names_from_keyspace_node`,
    /// `key_names_step`) are deliberately scoped to
    /// `build_mapped_type`'s Identity fast path (`Readonly<T>` /
    /// `Partial<T>` / `Required<T>`) where the source surface is
    /// already an `Object` lowered through the Expanded path.
    /// Teaching them to unwrap `DeclRef` / `InstantiationRef`
    /// interferes with that Identity-mapper case — observable as a
    /// `mapped_readonly.correctness.snap.json` regression — so the
    /// `Navigate`-lowered carrier-unwrap path is factored into this
    /// separate helper instead.
    ///
    /// This helper is the local fallback `synthesise_mapped_surface`
    /// calls when `key_names_from_base_node(source)` returns `None`
    /// because the source is a `DeclRef` / `InstantiationRef` carrier
    /// (the canonical form imported interfaces take under
    /// `structural_transit_with_mode(Navigate)` lowering). It dispatches
    /// `ProjectPath { source, [], Published(Shallow) }` so the walker
    /// unwraps the carrier through the existing `Instantiate(Published(
    /// Shallow))` path and returns the source's
    /// [`SurfaceMember`]s directly.
    ///
    /// Returning the full `SurfaceMember` list (not just names) lets the
    /// Shallow walker's `synthesise_mapped_surface`:
    ///   - reuse `source_member.value` directly for the Identity-mapper
    ///     case (preserving `Readonly<T>` / `Partial<T>` semantics even
    ///     when the source is a DeclRef);
    ///   - inherit `source_member.optional` / `source_member.readonly`
    ///     bits for the `OptionalityMod::Keep` / `ReadonlyMod::Keep`
    ///     cases;
    ///   - fall back to the per-key
    ///     [`Self::materialize_mapped_member_value_for_key`] substrate
    ///     for the `MapperKind::Computed` case.
    ///
    /// **Diagnostic propagation**: the inner dispatch's
    /// `walker_diagnostics` / `dep_signature` / `cache_suppress` flow
    /// into the active fact tracer via the dispatch's own
    /// `execute` cache pipeline; no diagnostics are silently dropped.
    ///
    /// **Recursion safety**: the helper dispatches one
    /// `SemanticQueryKey::ProjectPath` and reads its terminal node. The
    /// dispatch's same-path recursion sentinel guards against
    /// `Mapped`-shaped sources whose synthesise call re-enters this
    /// helper (the recursion is caught by the dispatch layer's
    /// reentrance check and returns `Recursive`, which surfaces as
    /// `None` here).
    ///
    /// **`context` parameter**: carries the caller's
    /// `ProjectionReductionContext` for telemetry / cache scoping
    /// alignment — the internal dispatch is fixed at
    /// `Published(Shallow)` per the contract. Callers
    /// passing a non-Published context to this helper are calling
    /// out-of-contract; only `synthesise_mapped_surface` (running
    /// under publication demand) is meant to drive it.
    pub(super) fn mapped_surface_source_members_for_projection(
        &self,
        source: SemanticNodeId,
        _context: crate::semantic_query::ProjectionReductionContext,
    ) -> Option<(Vec<SurfaceMember>, bool)> {
        // Returns the source surface PLUS the A2 partiality flag of the
        // underlying ProjectPath read so the Shallow walker's
        // `synthesise_mapped_surface` caller can taint its surface when
        // the source projection was genuinely incomplete.
        self.source_members_for_published_projection(source)
    }

    /// Per-key Mapped key-remap OUTCOME — shared by
    /// [`Self::build_mapped_type`] and the Shallow walker's
    /// `synthesise_mapped_surface`.
    ///
    /// Applies the mapper's `name_remap` (the `as <expr>` clause) by
    /// substituting the binder with the key literal and evaluating the remap
    /// under the caller's `context`, then classifies the result per the TS
    /// key-remap contract:
    ///
    /// - no remap clause ⇒ [`MappedKeyRemapOutcome::Keep`] (the iteration key);
    /// - `never` ⇒ [`MappedKeyRemapOutcome::Drop`] (the key is filtered out);
    /// - a finite string OR numeric `Literal` ⇒ [`MappedKeyRemapOutcome::Keys`]
    ///   with that one key (a numeric literal publishes under its canonical
    ///   `js_number_to_string` name — pinned tsgo, probe16: `{ [K in 1 as K]:
    ///   K }` = `{ 1: 1 }`, `{ [K in 1e21 as K]: K }` = `{ "1e+21": 1e21 }`);
    ///   a finite union of such literals ⇒ those keys;
    /// - an unresolved / non-finite / non-key-capable remap ⇒
    ///   [`MappedKeyRemapOutcome::DeferCarrier`] — the mapped type FAILS CLOSED
    ///   to its deferred carrier (it NEVER falls back to the original key,
    ///   which would publish a wrong surface).
    ///
    /// The template-literal evaluator folds `` `prefix-${K}` `` shapes through
    /// the shared `TemplateLiteralReduce` producer before this classification.
    pub(super) fn mapped_member_name_remap_outcome(
        &self,
        mapper: &crate::semantic_query::MapperKey,
        key: &super::enumerate::KeyDomainKey,
        context: crate::semantic_query::ProjectionReductionContext,
    ) -> MappedKeyRemapOutcome {
        let Some(remap_node) = mapper.name_remap else {
            return MappedKeyRemapOutcome::Keep(Arc::clone(&key.name));
        };
        // Kind-preserving substitution: the `as` clause sees the key's
        // ORIGINAL literal (probe12: `{ [K in 1 as K extends number ?
        // "n" : "s"]: K }` = `{ n: 1 }` — a stringified K would select
        // the wrong branch).
        let key_literal = self
            .graph()
            .intern_node(SemanticNodeData::Literal(key.literal.clone()));
        let substituted_remap =
            self.substitute_semantic_type_param(remap_node, mapper.parameter_node, key_literal);
        let evaluated_remap =
            self.evaluate_deferred_semantic_node_with_context(substituted_remap, context);
        self.classify_remap_outcome(evaluated_remap, context)
    }

    /// Classify an evaluated key-remap node into a [`MappedKeyRemapOutcome`].
    /// `never` ⇒ Drop, a string / numeric literal or a finite union of them ⇒
    /// Keys (numeric literals under their canonical `js_number_to_string`
    /// names), anything else (broad `string`, an unresolved shell, a
    /// non-key-capable shape — boolean / bigint literals are not property
    /// keys) ⇒ DeferCarrier (fail closed). Recurses through union arms,
    /// evaluating each arm through the shared deferred-node evaluator first:
    /// the root evaluation folds a bare template remap, but a one-to-many
    /// union remap (`as K | `x-${K}``) reaches here with its substituted arm
    /// shells unfolded — each arm gets the same evaluation demand the root
    /// already received.
    fn classify_remap_outcome(
        &self,
        node: SemanticNodeId,
        context: crate::semantic_query::ProjectionReductionContext,
    ) -> MappedKeyRemapOutcome {
        match self.graph().node_data(node).as_deref() {
            Some(SemanticNodeData::Literal(LiteralValue::String(text))) => {
                MappedKeyRemapOutcome::Keys(vec![Arc::from(text.as_str())])
            }
            Some(SemanticNodeData::Literal(LiteralValue::Number(number))) => {
                MappedKeyRemapOutcome::Keys(vec![Arc::from(js_number_to_string(*number).as_str())])
            }
            Some(SemanticNodeData::Primitive(PrimitiveKind::Never)) => MappedKeyRemapOutcome::Drop,
            Some(SemanticNodeData::Union(members)) => {
                let members = Arc::clone(members);
                let mut keys: Vec<Arc<str>> = Vec::new();
                for member in members.iter() {
                    let evaluated =
                        self.evaluate_deferred_semantic_node_with_context(*member, context);
                    match self.classify_remap_outcome(evaluated, context) {
                        MappedKeyRemapOutcome::Keys(ks) => keys.extend(ks),
                        // A `never` arm contributes no key (filtered);
                        // anything non-finite taints the whole remap.
                        MappedKeyRemapOutcome::Drop => {}
                        MappedKeyRemapOutcome::Keep(_) | MappedKeyRemapOutcome::DeferCarrier => {
                            return MappedKeyRemapOutcome::DeferCarrier;
                        }
                    }
                }
                if keys.is_empty() {
                    MappedKeyRemapOutcome::Drop
                } else {
                    MappedKeyRemapOutcome::Keys(keys)
                }
            }
            _ => MappedKeyRemapOutcome::DeferCarrier,
        }
    }

    /// Conditional type (lazy-block evaluation +
    /// distributive-conditional authority).
    ///
    /// Evaluates `check extends extends ? true_branch : false_branch`
    /// using the shared relation engine and returns one of:
    ///
    /// - **Distributive union check** — when `distributive == true` AND
    ///   `check` resolves to a [`SemanticNodeData::Union`], the builder
    ///   distributes per-member by re-entering the dispatcher with
    ///   `SemanticQueryApi::execute(SemanticQueryKey::Conditional {
    ///   check: member, extends, true_branch, false_branch,
    ///   distributive: false })` for every member, then combines the
    ///   per-member results through
    ///   `SemanticQueryApi::execute(SemanticQueryKey::NormalizeUnion {
    ///   members: per_member_results })`. Termination is guaranteed by
    ///   the `distributive: false` flag on each sub-query (no re-
    ///   distribution), the family memo's per-member dedup, and the
    ///   dispatch layer's same-path recursion sentinel. Dispatch owns
    ///   distributive distribution.
    /// - **Closed/decidable check** — one of the branch shell references
    ///   directly (no `Conditional` node interned). Emits a
    ///   [`OriginEdgeKind::ConditionalSelect`] edge with
    ///   [`BranchSelection::True`] or [`BranchSelection::False`]. The
    ///   unselected branch is NOT materialised beyond its shell
    ///   reference (it already has one via the key's
    ///   `true_branch` / `false_branch` fields).
    /// - **Open/undecidable check** — a
    ///   [`SemanticNodeData::Conditional`] shell with both branch
    ///   references intact. Emits
    ///   [`OriginEdgeKind::ConditionalSelect`] with
    ///   [`BranchSelection::Deferred`]. Neither branch is recursively
    ///   materialised; path projection into the result drives
    ///   per-subexpression lazy expansion.
    ///
    /// The relation evaluator handles the decidable shapes the shallow
    /// walker reaches directly: primitive identity, primitive-to-top/any,
    /// `never` bottom, exact node identity, and the obvious
    /// non-assignability cases. Object / union / intersection / generic
    /// relations stay deferred — the full solver routing lands via the
    /// `resolve_conditional` dispatch handoff in . Bare-infer
    /// bindings (`T extends infer X`) are handled by the shortcut below;
    /// nested-infer in complex patterns defers to the relation engine.
    pub(super) fn build_conditional(
        &self,
        check: SemanticNodeId,
        extends: SemanticNodeId,
        true_branch: SemanticNodeId,
        false_branch: SemanticNodeId,
        distributive: bool,
    ) -> crate::project_semantic_dispatch::walk::QueryBuildOutput {
        // Fast-reject (runs BEFORE the distributive-`Union` distribution,
        // the `infer`-binding paths, and `shallow_relation_check`): `error
        // extends T` ⇒ `error` (carrier dominates), `any extends T ? X : Y` ⇒
        // `X | Y` (union of both branches, mode-independent), and DISTRIBUTIVE
        // naked-`never` ⇒ `never` (empty distribution). Non-distributive
        // `never` and every other check fall through to the branch logic below
        // — see `absorb_conditional`.
        if let Some(absorbed) =
            self.absorb_conditional(check, extends, true_branch, false_branch, distributive)
        {
            return absorbed;
        }
        let graph = self.graph();
        let fence = self.project_generation_signature();
        // Self-version rooting: the conditional's resolution depends on
        // the `check`, `extends`, and both branch shells — all
        // already-interned nodes. Root the memo entry on the file
        // content version each file-derived input was lowered from.
        let observed_self_roots =
            self.observed_self_roots_from_nodes([check, extends, true_branch, false_branch]);

        // Distributive distribution is the dispatch layer's
        // responsibility. When `distributive == true`
        // and `check` is a union, re-enter `execute` per-member with
        // `distributive: false`, then normalise the per-member results
        // through `NormalizeUnion`. Each sub-dispatch lands in a
        // different family entry (check differs per member) and the
        // `distributive: false` flag guarantees no re-distribution, so
        // the cooperative-wait mechanism terminates and the same-path
        // sentinel catches any accidental self-recursion.
        //
        // Robustness: if any sub-query returns `Recursive` or `Error`
        // (cycle or miss), fall through to the ordinary deferred-shell
        // path below so the caller sees a well-formed conditional node
        // rather than a partial distribution.
        if distributive {
            if let Some(members) = graph.node_data(check).and_then(|data| match &*data {
                SemanticNodeData::Union(members) => Some(Arc::clone(members)),
                _ => None,
            }) {
                let mut per_member: Vec<SemanticNodeId> = Vec::with_capacity(members.len());
                let mut distribution_ok = true;
                // Two-signal fold: accumulate the partiality of every
                // per-member sub-conditional so an incomplete distribution
                // (a member whose nested resolution tripped the budget /
                // recurred / hit a walker fatal) taints the conditional
                // result, whether it early-returns the normalised union or
                // falls through to the deferred shell.
                let mut distribution_is_partial = false;
                for &member in members.iter() {
                    let member_read = self.execute_read(SemanticQueryKey::Conditional {
                        check: member,
                        extends,
                        true_branch,
                        false_branch,
                        distributive: false,
                    });
                    distribution_is_partial |= member_read.result_is_partial;
                    match member_read.value {
                        QueryResult::Value(id) => per_member.push(id),
                        _ => {
                            distribution_ok = false;
                            break;
                        }
                    }
                }
                if distribution_ok {
                    let normalize_read = self.execute_read(SemanticQueryKey::NormalizeUnion {
                        members: Arc::from(per_member.into_boxed_slice()),
                    });
                    distribution_is_partial |= normalize_read.result_is_partial;
                    if let QueryResult::Value(normalised) = normalize_read.value {
                        let mut output =
                            crate::project_semantic_dispatch::walk::QueryBuildOutput::from((
                                QueryResult::Value(normalised),
                                fence,
                            ))
                            .with_observed_self_roots(observed_self_roots.clone());
                        output.result_is_partial = distribution_is_partial;
                        return output;
                    }
                }
                // Distribution failed or normalisation did not produce a
                // value: a partial member taints the request so the
                // fall-through deferred-shell result does not warm.
                if distribution_is_partial {
                    crate::request_context::mark_request_materialization_cache_suppress();
                }
                // Fall through to the deferred-shell path below.
            }
        }

        // Conditionals route through the shared relation authority via
        // the tri-state branch-selection oracle
        // ([`Self::conditional_branch_selection`]) — the SAME
        // helper the key-domain closedness classifiers in `raise.rs`
        // consult, so build-time reduction and predicate-time
        // classification cannot diverge on which branch a conditional
        // takes. The oracle owns the FULL selection path, INCLUDING the
        // pre-relation infer-pattern cases
        // ([`Self::pre_relation_infer_selection`]); this build path owns
        // only the selection's SUBSTITUTION side-effects (binding the
        // selected infer names into the true branch + `InferBind` origin
        // edges).
        let (selection, infer) = self.conditional_branch_selection(check, extends);
        if let Some(selected) = infer {
            // An infer-pattern selection is always TRUE: substitute the
            // payload's bindings into the true branch. (The `infer X`
            // binding occupies a separate name-slot mechanism from
            // regular type parameters; the substitute helper's Infer arm
            // matches by display_name to bridge that boundary.)
            // Multi-infer (`T extends [infer A, infer B] ? ...`) and
            // template-literal-infer patterns are NOT selected by the
            // oracle and stay deferred; they require the full
            // relation-engine bindings integration still pending per the
            // TODO in `conditional_branch_selection`.
            let result = match selected {
                InferPatternSelection::BareInfer { name } => {
                    let infer_node = graph.intern_node(SemanticNodeData::Infer {
                        name: Arc::clone(&name),
                    });
                    let result =
                        self.substitute_semantic_type_param(true_branch, infer_node, check);
                    graph.record_origin_edge(
                        result,
                        OriginEdgeKind::InferBind,
                        Arc::from(vec![check, extends].into_boxed_slice()),
                        OriginMeta::SubstitutedParam(name),
                        Arc::clone(&fence),
                    );
                    result
                }
                InferPatternSelection::FunctionInfer { bindings } => {
                    let mut result = true_branch;
                    for (name, bound) in bindings {
                        let infer_node = graph.intern_node(SemanticNodeData::Infer {
                            name: Arc::clone(&name),
                        });
                        result = self.substitute_semantic_type_param(result, infer_node, bound);
                        graph.record_origin_edge(
                            result,
                            OriginEdgeKind::InferBind,
                            Arc::from(vec![check, extends].into_boxed_slice()),
                            OriginMeta::SubstitutedParam(name),
                            Arc::clone(&fence),
                        );
                    }
                    result
                }
            };
            graph.record_conditional_decided();
            graph.record_branch_selection_true();
            return crate::project_semantic_dispatch::walk::QueryBuildOutput::from((
                QueryResult::Value(result),
                fence,
            ))
            .with_observed_self_roots(observed_self_roots.clone());
        }
        let (result, branch, is_deferred) = match selection {
            ConditionalBranchSelection::True => (true_branch, BranchSelection::True, false),
            ConditionalBranchSelection::False => (false_branch, BranchSelection::False, false),
            ConditionalBranchSelection::Deferred => {
                let node = graph.intern_node(SemanticNodeData::Conditional {
                    check,
                    extends,
                    true_branch_ref: true_branch,
                    false_branch_ref: false_branch,
                    distributive,
                });
                (node, BranchSelection::Deferred, true)
            }
        };
        graph.record_origin_edge(
            result,
            OriginEdgeKind::ConditionalSelect,
            Arc::from(vec![check, extends].into_boxed_slice()),
            OriginMeta::Branch(branch),
            Arc::clone(&fence),
        );
        if is_deferred {
            graph.record_conditional_deferred();
        } else {
            graph.record_conditional_decided();
            match branch {
                BranchSelection::True => graph.record_branch_selection_true(),
                BranchSelection::False => graph.record_branch_selection_false(),
                BranchSelection::Deferred => {}
            }
        }
        crate::project_semantic_dispatch::walk::QueryBuildOutput::from((
            QueryResult::Value(result),
            fence,
        ))
        .with_observed_self_roots(observed_self_roots)
    }

    /// Shallow hot-path relation check used by [`Self::build_conditional`].
    /// Decides the trivial primitive/identity/top/bottom cases inline
    /// without descending into the full relation engine. Non-trivial
    /// pairs return `Unknown`, in which case `build_conditional` falls
    /// through to [`Self::relate_nodes`] for the full structural
    /// decision.
    pub(super) fn shallow_relation_check(
        &self,
        source: SemanticNodeId,
        target: SemanticNodeId,
    ) -> ShallowRelation {
        if source == target {
            return ShallowRelation::Assignable;
        }
        let graph = self.graph();
        let Some(source_data) = graph.node_data(source) else {
            return ShallowRelation::Unknown;
        };
        let Some(target_data) = graph.node_data(target) else {
            return ShallowRelation::Unknown;
        };
        match (&*source_data, &*target_data) {
            (SemanticNodeData::Primitive(PrimitiveKind::Never), _) => ShallowRelation::Assignable,
            (_, SemanticNodeData::Primitive(PrimitiveKind::Unknown)) => ShallowRelation::Assignable,
            (_, SemanticNodeData::Primitive(PrimitiveKind::Any)) => ShallowRelation::Assignable,
            (SemanticNodeData::Primitive(PrimitiveKind::Any), _) => ShallowRelation::Assignable,
            (_, SemanticNodeData::Primitive(PrimitiveKind::Never)) => {
                ShallowRelation::NotAssignable
            }
            (SemanticNodeData::Primitive(a), SemanticNodeData::Primitive(b)) => {
                if a == b {
                    ShallowRelation::Assignable
                } else {
                    ShallowRelation::NotAssignable
                }
            }
            _ => ShallowRelation::Unknown,
        }
    }

    /// Pre-relation INFER-PATTERN selection — the structural cases that
    /// select a branch BEFORE any relation query, factored out of
    /// `build_conditional`'s former inline arms so the key-domain
    /// closedness classifiers see the SAME selection (the oracle owns
    /// the FULL selection path, not just its relation tail):
    ///
    /// - **Bare `Infer` extends** (`T extends infer X`) — an infer
    ///   pattern matches anything ⇒ TRUE selected with `X := check`,
    ///   for ANY check (`check` is not consulted; `None` is accepted, so
    ///   the TypeExpr classifier can select even when its check operand
    ///   does not resolve to a node).
    /// - **Function-typed extends with `infer` positions**
    ///   (`T extends (x: infer U, y: infer V) => infer R`) — the check
    ///   is materialised via [`Self::evaluate_deferred_semantic_node`]
    ///   (so `PricingPlanSlots["badge"]` / mapped-type references
    ///   resolve to their underlying Function before position-wise
    ///   binding); if it resolves to a Function and at least one infer
    ///   position binds the corresponding check position, TRUE is
    ///   selected with those `name := node` bindings. The relation
    ///   engine's Function arm short-circuits to `Unknown` in the
    ///   presence of Infer positions (it does not currently emit infer
    ///   bindings), so without this case those conditionals would
    ///   defer. The evaluator does not propagate the caller's
    ///   publication demand — sound because the recursive resolution it
    ///   triggers dispatches `Conditional` / `Instantiate`, both
    ///   counted toward the aggregate request work budget.
    ///
    /// Everything else — multi-infer tuples, template-literal-infer —
    /// returns `None` and the relation ladder decides (those patterns
    /// stay deferred today).
    pub(super) fn pre_relation_infer_selection(
        &self,
        check: Option<SemanticNodeId>,
        extends: SemanticNodeId,
    ) -> Option<InferPatternSelection> {
        let graph = self.graph();
        match graph.node_data(extends).as_deref() {
            Some(SemanticNodeData::Infer { name }) => Some(InferPatternSelection::BareInfer {
                name: Arc::clone(name),
            }),
            Some(SemanticNodeData::Function {
                params: extends_params,
                return_type: extends_return,
                ..
            }) => {
                let extends_params = Arc::clone(extends_params);
                let extends_return = *extends_return;
                let has_infer_position = extends_params.iter().any(|p| {
                    matches!(
                        graph.node_data(p.ty).as_deref(),
                        Some(SemanticNodeData::Infer { .. })
                    )
                }) || matches!(
                    graph.node_data(extends_return).as_deref(),
                    Some(SemanticNodeData::Infer { .. })
                );
                if !has_infer_position {
                    return None;
                }
                let mut check_resolved = self.evaluate_deferred_semantic_node(check?);
                // Demand point (the relation/conditional oracle): a check
                // riding an `InstantiationRef` carrier — the
                // carrier-preserving lowering's shape for a builtin /
                // generic over open-then-substituted arguments
                // (`NonNullable<ChatSlots["header"]>` after per-key
                // substitution) — is materialised HERE, where the
                // function-infer pattern genuinely demands the check's
                // shape for positional binding. The deferred-shell
                // evaluator deliberately has no `InstantiationRef` arm
                // (intermediate hops must stay carrier-shaped), so the
                // oracle executes the one demanded instantiation under
                // the structural-transit operand context and re-evaluates.
                if let Some(SemanticNodeData::InstantiationRef { base, args }) =
                    graph.node_data(check_resolved).as_deref()
                {
                    let owner_canonical = Arc::clone(&base.canonical_id);
                    let slot = self
                        .type_slot_for(Arc::clone(&base.canonical_id), Arc::clone(&base.decl_name));
                    let operand_context =
                        crate::semantic_query::ProjectionReductionContext::structural_transit_with_mode(
                            crate::semantic_query::ProjectionMode::Navigate,
                        );
                    // The carrier's operator-shaped arguments (the
                    // post-substitution `ChatSlots["header"]` indexed
                    // access) resolve through the deferred-shell
                    // evaluator first — path-precise single hops — so
                    // the instantiation consumes settled operands.
                    let args: Arc<[SemanticNodeId]> = Arc::from(
                        args.iter()
                            .map(|arg| {
                                self.evaluate_deferred_semantic_node_with_context(
                                    *arg,
                                    operand_context,
                                )
                            })
                            .collect::<Vec<_>>()
                            .into_boxed_slice(),
                    );
                    let read = self.execute_read(SemanticQueryKey::Instantiate(
                        crate::semantic_query::InstantiateKey::new(
                            slot,
                            args,
                            self.instantiate_context_for(&owner_canonical, operand_context),
                        ),
                    ));
                    crate::request_context::observe_component_meta_read_suppress(&read);
                    if let QueryResult::Value(id) = read.value {
                        check_resolved = self.evaluate_deferred_semantic_node(id);
                    }
                }
                let (check_params, check_return) = match graph.node_data(check_resolved).as_deref()
                {
                    Some(SemanticNodeData::Function {
                        params,
                        return_type,
                        ..
                    }) => (Arc::clone(params), *return_type),
                    _ => return None,
                };
                let mut bindings: Vec<(Arc<str>, SemanticNodeId)> = Vec::new();
                for (e_param, c_param) in extends_params.iter().zip(check_params.iter()) {
                    if let Some(SemanticNodeData::Infer { name }) =
                        graph.node_data(e_param.ty).as_deref()
                    {
                        bindings.push((Arc::clone(name), c_param.ty));
                    }
                }
                if let Some(SemanticNodeData::Infer { name }) =
                    graph.node_data(extends_return).as_deref()
                {
                    bindings.push((Arc::clone(name), check_return));
                }
                if bindings.is_empty() {
                    return None;
                }
                Some(InferPatternSelection::FunctionInfer { bindings })
            }
            _ => None,
        }
    }

    /// Tri-state conditional branch selection — THE shared oracle for
    /// every consumer that must decide which branch a conditional
    /// semantically takes: [`Self::build_conditional`]'s reduction path
    /// AND the key-domain closedness classifiers in `raise.rs` (the
    /// TypeExpr-layer `Conditional` arm and the node-level `OpenWalk`
    /// arm), which invoke it for selected-branch-only classification
    /// instead of reimplementing assignability or privately
    /// materialising branches. Returns the selection PLUS the
    /// infer-pattern payload when the selection came from a
    /// pre-relation infer case, so `build_conditional` can perform the
    /// binding substitution and the classifiers can bind branch infer
    /// names to the same check-derived identities.
    ///
    /// Decision order mirrors `build_conditional`'s historical ladder
    /// exactly:
    ///
    /// 1. an `error` check DOMINATES the whole conditional — no branch
    ///    is selected ⇒ `Deferred` (in `build_conditional` this row is
    ///    pre-absorbed by `absorb_conditional`; the guard makes the
    ///    oracle safe for classifier callers, which have no absorber in
    ///    front of them);
    /// 2. the pre-relation infer-pattern cases
    ///    ([`Self::pre_relation_infer_selection`]) — an infer pattern
    ///    matches anything, so even an `any` check selects TRUE with
    ///    `X := any`;
    /// 3. an `any` check semantically uses BOTH branches
    ///    (`any extends T ? X : Y` ⇒ `X | Y`) ⇒ `Deferred` (likewise
    ///    pre-absorbed in build for non-infer extends);
    /// 4. [`Self::shallow_relation_check`] (the hot-path
    ///    primitive/identity table), then the full memoised
    ///    [`Self::relate_nodes`] relation engine; `Unknown` ⇒
    ///    `Deferred`. `relate_nodes` internally guards cyclic re-entry
    ///    and bounds structural descent with an iterative heap-backed
    ///    worklist plus a graph-size work budget
    ///    (`10 × graph.node_count()`, 4096 floor) that yields `Unknown`
    ///    on runaway — there is no per-frame recursion cap.
    pub(super) fn conditional_branch_selection(
        &self,
        check: SemanticNodeId,
        extends: SemanticNodeId,
    ) -> (ConditionalBranchSelection, Option<InferPatternSelection>) {
        if matches!(
            self.peek_special(check),
            Some((super::absorb::SpecialKind::Error, _))
        ) {
            return (ConditionalBranchSelection::Deferred, None);
        }
        if let Some(selected) = self.pre_relation_infer_selection(Some(check), extends) {
            return (ConditionalBranchSelection::True, Some(selected));
        }
        if matches!(
            self.peek_special(check),
            Some((super::absorb::SpecialKind::Any, _))
        ) {
            return (ConditionalBranchSelection::Deferred, None);
        }
        let selection = match self.shallow_relation_check(check, extends) {
            ShallowRelation::Assignable => ConditionalBranchSelection::True,
            ShallowRelation::NotAssignable => ConditionalBranchSelection::False,
            ShallowRelation::Unknown => {
                // Full relation authority. `relate_nodes` memoises all
                // three outcomes with dep-signature fencing.
                match self.relate_nodes(check, extends).0 {
                    // TODO: in `build_conditional`, substitute infer
                    // bindings carried by the RELATION result into
                    // `true_branch` via `substitute_semantic_type_param`
                    // and emit `InferBind` origin edges for non-empty
                    // bindings (the pre-relation cases above cover the
                    // bare-infer and function-infer patterns only).
                    // Infer-bearing conditionals beyond those lower to
                    // the deferred shell today; see §6.3 test
                    // `relate_result_assignable_carries_infer_bindings_into_conditional`.
                    crate::semantic_query::RelationResult::Assignable { bindings: _ } => {
                        ConditionalBranchSelection::True
                    }
                    crate::semantic_query::RelationResult::NotAssignable => {
                        ConditionalBranchSelection::False
                    }
                    crate::semantic_query::RelationResult::Unknown => {
                        ConditionalBranchSelection::Deferred
                    }
                }
            }
        };
        (selection, None)
    }

    /// Union normalization. Structurally sorts + dedups the supplied members
    /// and publishes the canonical union node. Singleton unions fold to
    /// their only member; empty unions fold to `Primitive(Never)`.
    ///
    /// Emits one `Normalize` origin edge from the result to each
    /// contributing source member. The edge lets walkers
    /// recover the pre-canonical input set even after dedup / sorting.
    /// Single-member / empty folds emit no edge — the result IS one of
    /// the inputs (or a fresh Never node) and there's no canonicalisation
    /// fact to record.
    pub(super) fn build_normalize_union(
        &self,
        members: &Arc<[SemanticNodeId]>,
    ) -> crate::project_semantic_dispatch::walk::QueryBuildOutput {
        // §22 fast-reject: `X|any=any`, `X|never=X`, `X|unknown=unknown`,
        // `X|error=error`. Runs BEFORE structural normalization.
        if let Some(absorbed) = self.absorb_union(members) {
            return absorbed;
        }
        let node = self.intern_normalized_union_or_intersection(members, /* is_union */ true);
        let fence = self.project_generation_signature();
        if members.len() > 1 {
            self.graph().record_origin_edge(
                node,
                OriginEdgeKind::Normalize,
                Arc::clone(members),
                OriginMeta::None,
                Arc::clone(&fence),
            );
        }
        // Self-version rooting: the normalised union depends on every
        // contributing member node. Root the memo entry on the file
        // content version each file-derived member was lowered from.
        let observed_self_roots = self.observed_self_roots_from_nodes(members.iter().copied());
        crate::project_semantic_dispatch::walk::QueryBuildOutput::from((
            QueryResult::Value(node),
            fence,
        ))
        .with_observed_self_roots(observed_self_roots)
    }

    /// Intersection normalization. Structurally sorts + dedups; singleton
    /// folds to the only member; empty folds to `Primitive(Never)`.
    ///
    /// Emits one `Normalize` origin edge from the result to each
    /// contributing source member.
    pub(super) fn build_normalize_intersection(
        &self,
        members: &Arc<[SemanticNodeId]>,
    ) -> crate::project_semantic_dispatch::walk::QueryBuildOutput {
        // §22 fast-reject: `X&never=never`, `X&any=any`, `X&unknown=X`,
        // `X&error=error`. Runs BEFORE structural normalization.
        if let Some(absorbed) = self.absorb_intersection(members) {
            return absorbed;
        }
        let node = self.intern_normalized_union_or_intersection(members, /* is_union */ false);
        let fence = self.project_generation_signature();
        if members.len() > 1 {
            self.graph().record_origin_edge(
                node,
                OriginEdgeKind::Normalize,
                Arc::clone(members),
                OriginMeta::None,
                Arc::clone(&fence),
            );
        }
        // Self-version rooting: the normalised intersection depends on
        // every contributing member node. Root the memo entry on the
        // file content version each file-derived member was lowered
        // from.
        let observed_self_roots = self.observed_self_roots_from_nodes(members.iter().copied());
        crate::project_semantic_dispatch::walk::QueryBuildOutput::from((
            QueryResult::Value(node),
            fence,
        ))
        .with_observed_self_roots(observed_self_roots)
    }

    /// Literal-preserving string-intrinsic transform shared by the
    /// `Uppercase` / `Lowercase` / `Capitalize` / `Uncapitalize` arms of
    /// [`Self::build_instantiate`]. Takes an ALREADY-RESOLVED argument node
    /// (the caller evaluates it ONCE through the shared deferred evaluator), then:
    ///
    /// - a string literal ⇒ the case-transformed literal;
    /// - a union ⇒ per-arm transform + `NormalizeUnion` (each arm is resolved
    ///   before transforming);
    /// - `never` ⇒ `never` (empty domain);
    /// - a broad `string` / unresolved / non-string shape ⇒ fail closed to the
    ///   `string` primitive.
    ///
    /// Typed-IR only: the transform applies to the interned `LiteralValue`,
    /// never to source/display text.
    pub(super) fn apply_string_intrinsic(
        &self,
        intrinsic: &str,
        resolved: SemanticNodeId,
        context: crate::semantic_query::ProjectionReductionContext,
    ) -> SemanticNodeId {
        let graph = self.graph();
        match graph.node_data(resolved).as_deref() {
            Some(SemanticNodeData::Literal(LiteralValue::String(text))) => {
                let transformed = transform_string_intrinsic(intrinsic, text);
                graph.intern_node(SemanticNodeData::Literal(LiteralValue::String(transformed)))
            }
            Some(SemanticNodeData::Union(members)) => {
                let members = Arc::clone(members);
                let mapped: Vec<SemanticNodeId> = members
                    .iter()
                    .map(|m| {
                        // Union arms are raw member nodes — resolve each once
                        // before transforming (the entry node was resolved by
                        // the caller, but its members were not).
                        let resolved_member =
                            self.evaluate_deferred_semantic_node_with_context(*m, context);
                        self.apply_string_intrinsic(intrinsic, resolved_member, context)
                    })
                    .collect();
                let read = self.execute_read(SemanticQueryKey::NormalizeUnion {
                    members: Arc::from(mapped.into_boxed_slice()),
                });
                match read.value {
                    QueryResult::Value(id) => id,
                    _ => graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String)),
                }
            }
            Some(SemanticNodeData::Primitive(PrimitiveKind::Never)) => {
                graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Never))
            }
            // Broad `string`, an unresolved deferred shell, or any non-string
            // shape: fail closed to the `string` primitive (the intrinsic's
            // declared result domain).
            _ => graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String)),
        }
    }

    /// Template-literal reduction — the LIVE producer for
    /// [`SemanticQueryKey::TemplateLiteralReduce`].
    ///
    /// ONE-ENGINE shape: this build is the SINGLE shared template-literal
    /// reducer. It resolves every interpolated expression of the key's
    /// `pattern` (quasis) + `args` (expressions) to its finite set of
    /// string-literal choices through the shared deferred evaluator, then
    /// forms the CARTESIAN PRODUCT of those choices into the folded surface
    /// ([`Self::reduce_template_literal_nodes`]). An all-single-literal
    /// template folds to one [`SemanticNodeData::Literal`] string; a finite
    /// union of choices renormalises through
    /// [`SemanticQueryKey::NormalizeUnion`]; any non-finite expression
    /// carrier-stops to the `TemplateLiteral` shell. There is no second
    /// walker — the deferred evaluator's `TemplateLiteral` arm and the
    /// mapped key-remap path both reach this reducer THROUGH this query.
    ///
    /// Keyspace budget: a finite product whose running width exceeds
    /// [`TEMPLATE_LITERAL_KEYSPACE_CAP`] carrier-stops to the deferred shell
    /// and the result is marked NON-CACHEABLE / budget-tainted
    /// (`cache_suppress` + `result_is_partial`) — a truncated / over-budget
    /// product is never warm-admitted (mirrors the evaluator's
    /// budget-exhaustion `ReturnOnly` discipline).
    ///
    /// Self-version rooting: the reduction depends on every interpolated
    /// arg node, so the memo entry roots on the file content version each
    /// file-derived arg was lowered from (mirrors `build_normalize_union`).
    /// `args` is consumed in ORDER — concatenation order is semantic and is
    /// never reordered.
    pub(super) fn build_template_literal_reduce(
        &self,
        pattern: &Arc<[Arc<str>]>,
        args: &Arc<[SemanticNodeId]>,
        _context: crate::semantic_query::TemplateLiteralReduceContext,
    ) -> crate::project_semantic_dispatch::walk::QueryBuildOutput {
        let outcome = self.reduce_template_literal_nodes(
            pattern,
            args,
            crate::semantic_query::ProjectionReductionContext::published(ProjectionMode::Expanded),
        );
        let observed_self_roots = self.observed_self_roots_from_nodes(args.iter().copied());
        let mut output = crate::project_semantic_dispatch::walk::QueryBuildOutput::from((
            QueryResult::Value(outcome.node),
            self.project_generation_signature(),
        ))
        .with_observed_self_roots(observed_self_roots);
        if outcome.keyspace_budget_exceeded {
            // The product width tripped the keyspace cap: the value is a
            // deferred carrier-stop shell, not the fully-enumerated surface.
            // Mark it a non-cacheable budget-tainted partial so it is never
            // warm-admitted and the taint folds into the enclosing request.
            output.cache_suppress = true;
            output.result_is_partial = true;
        }
        output
    }

    /// Production constructor for the env-bearing
    /// [`TemplateLiteralReduceContext`](crate::semantic_query::TemplateLiteralReduceContext).
    /// The key has no decl slot; the already-lowered arg nodes carry the
    /// content roots, so the context carries ONLY the R/T/L/J environment
    /// (R21). No content/version hash enters the query-identity key (R6).
    pub(crate) fn template_literal_reduce_context(
        &self,
    ) -> crate::semantic_query::TemplateLiteralReduceContext {
        let host = self.ctx.host_for_fact_tracer_install();
        let env = host.host_view_env_hashes();
        crate::semantic_query::TemplateLiteralReduceContext {
            resolve_env_hash: env.resolve_env_hash,
            type_env_hash: env.type_env_hash,
            lib_env_hash: env.lib_env_hash,
            project_identity: host.host_view_project_identity().fold_u32(),
        }
    }

    /// The ONE shared template-literal reduction helper (typed-IR only).
    /// Produces the CARTESIAN PRODUCT over the finite string-literal choices
    /// of every interpolated expression:
    ///
    /// - `` `cell:${"name" | "count"}` `` ⇒ `"cell:name" | "cell:count"`;
    /// - an all-single-literal template ⇒ a single `Literal` string;
    /// - an empty product (some expression resolved to `never`) ⇒ `never`;
    /// - any non-finite / non-string expression ⇒ carrier-stop to the
    ///   `TemplateLiteral` shell (the caller re-dispatches once it resolves).
    ///
    /// Keyspace budget: the running product width `∏ |choice_set_i|` is
    /// bounded by [`TEMPLATE_LITERAL_KEYSPACE_CAP`]. A product whose width
    /// exceeds the cap carrier-stops to the `TemplateLiteral` shell and the
    /// returned [`TemplateReduceOutcome::keyspace_budget_exceeded`] flag is
    /// set so the live producer refuses to warm-admit the over-budget result
    /// (the per-arg recursion ceiling on the deferred evaluator bounds depth,
    /// NOT product width — this cap is the width bound). The check runs on the
    /// per-arg choice-set cardinalities BEFORE any string is allocated.
    ///
    /// The multi-result case renormalises through
    /// [`SemanticQueryKey::NormalizeUnion`] so the union is canonical. Used by
    /// [`Self::build_template_literal_reduce`] (the live query producer); the
    /// deferred evaluator's `TemplateLiteral` arm and the mapped key-remap path
    /// both reach this helper THROUGH that query, never a second walker.
    pub(super) fn reduce_template_literal_nodes(
        &self,
        quasis: &[Arc<str>],
        args: &[SemanticNodeId],
        eval_context: crate::semantic_query::ProjectionReductionContext,
    ) -> TemplateReduceOutcome {
        let graph = self.graph();
        let carrier_stop = || TemplateReduceOutcome {
            node: graph.intern_node(SemanticNodeData::TemplateLiteral {
                quasis: Arc::from(quasis.to_vec().into_boxed_slice()),
                expressions: Arc::from(args.to_vec().into_boxed_slice()),
            }),
            keyspace_budget_exceeded: false,
        };
        // Resolve every interpolated expression to its finite set of
        // string-literal choices. A `None` carrier-stops the whole template.
        let mut choice_sets: Vec<Vec<Arc<str>>> = Vec::with_capacity(args.len());
        for &arg in args {
            match self.template_arg_literal_choices(arg, eval_context) {
                Some(choices) => choice_sets.push(choices),
                None => return carrier_stop(),
            }
        }
        // Keyspace budget gate: bound the cartesian product width BEFORE
        // allocating any string. A finite-but-huge keyspace (e.g. a template
        // over several wide finite unions) would otherwise explode allocation
        // and could warm-publish a truncated surface. An over-cap product
        // carrier-stops to the deferred shell, tainted budget-exceeded so the
        // producer never warm-admits it.
        let mut product_width: usize = 1;
        for choices in &choice_sets {
            product_width = product_width.saturating_mul(choices.len());
        }
        if product_width > TEMPLATE_LITERAL_KEYSPACE_CAP {
            return TemplateReduceOutcome {
                keyspace_budget_exceeded: true,
                ..carrier_stop()
            };
        }
        // Cartesian product: result = quasis[0] expr[0] quasis[1] … quasis[n].
        // An empty choice set (a `never` expression) collapses the product to
        // the empty set, which becomes `never` below.
        let mut results: Vec<String> = vec![String::new()];
        for (idx, choices) in choice_sets.iter().enumerate() {
            let quasi = quasis.get(idx).map(|q| q.as_ref()).unwrap_or("");
            let mut next: Vec<String> = Vec::with_capacity(results.len() * choices.len());
            for prefix in &results {
                for choice in choices {
                    let mut combined =
                        String::with_capacity(prefix.len() + quasi.len() + choice.len());
                    combined.push_str(prefix);
                    combined.push_str(quasi);
                    combined.push_str(choice);
                    next.push(combined);
                }
            }
            results = next;
        }
        let tail = quasis.get(args.len()).map(|q| q.as_ref()).unwrap_or("");
        for combined in &mut results {
            combined.push_str(tail);
        }

        let node = match results.len() {
            0 => graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Never)),
            1 => graph.intern_node(SemanticNodeData::Literal(LiteralValue::String(
                results.into_iter().next().expect("len == 1"),
            ))),
            _ => {
                let members: Vec<SemanticNodeId> = results
                    .into_iter()
                    .map(|s| graph.intern_node(SemanticNodeData::Literal(LiteralValue::String(s))))
                    .collect();
                let read = self.execute_read(SemanticQueryKey::NormalizeUnion {
                    members: Arc::from(members.into_boxed_slice()),
                });
                match read.value {
                    QueryResult::Value(id) => id,
                    _ => graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Never)),
                }
            }
        };
        TemplateReduceOutcome {
            node,
            keyspace_budget_exceeded: false,
        }
    }

    /// Resolve ONE template-literal interpolated expression to its finite set
    /// of string-literal choices, or `None` when the expression is non-finite /
    /// non-string (broad `string`, an unresolved deferred shell, an open
    /// generic). `never` ⇒ `Some(empty)` (an empty product factor). The arg is
    /// resolved through the shared deferred evaluator; a residual
    /// `InstantiationRef` (e.g. `Capitalize<…>`) is dispatched through
    /// `Instantiate` so string intrinsics fold before enumeration.
    fn template_arg_literal_choices(
        &self,
        arg: SemanticNodeId,
        eval_context: crate::semantic_query::ProjectionReductionContext,
    ) -> Option<Vec<Arc<str>>> {
        let graph = self.graph();
        let mut resolved = self.evaluate_deferred_semantic_node_with_context(arg, eval_context);
        if let Some(SemanticNodeData::InstantiationRef { base, args }) =
            graph.node_data(resolved).as_deref()
        {
            let slot =
                self.type_slot_for(Arc::clone(&base.canonical_id), Arc::clone(&base.decl_name));
            let inst_ctx = self.instantiate_context_for(&base.canonical_id, eval_context);
            let args = Arc::clone(args);
            let read = self.execute_read(SemanticQueryKey::Instantiate(
                crate::semantic_query::InstantiateKey::new(slot, args, inst_ctx),
            ));
            if let QueryResult::Value(id) = read.value {
                resolved = id;
            }
        }
        match graph.node_data(resolved).as_deref() {
            // A finite literal interpolant — string OR numeric / boolean /
            // bigint — contributes ONE TS-stringified choice. TS interpolates
            // `` `${1 | 2}` `` ⇒ `"1" | "2"`, `` `${true}` `` ⇒ `"true"`, and a
            // bigint literal as its base-10 digits. Stringification is
            // canonical-TS and typed-IR only (it reads the interned
            // `LiteralValue`, never source text). See
            // [`literal_value_template_text`].
            Some(SemanticNodeData::Literal(value)) => {
                Some(vec![Arc::from(literal_value_template_text(value).as_str())])
            }
            Some(SemanticNodeData::Primitive(PrimitiveKind::Never)) => Some(Vec::new()),
            Some(SemanticNodeData::Union(members)) => {
                let members = Arc::clone(members);
                let mut out: Vec<Arc<str>> = Vec::new();
                for member in members.iter() {
                    let choices = self.template_arg_literal_choices(*member, eval_context)?;
                    out.extend(choices);
                }
                Some(out)
            }
            _ => None,
        }
    }

    /// Vue macro resolution lookup.
    ///
    /// Hot-path reads go through
    /// [`SemanticGraphStore::get_resolved_named_type`](crate::semantic_query_memo::SemanticGraphStore::get_resolved_named_type)
    /// directly from the parser's
    /// [`NamedTypeCache`](verter_compiler::utils::oxc::vue::named_type_keys::NamedTypeCache)
    /// adapter — the formal `execute` path stays available as an entry
    /// point for callers that want to check presence through the shared
    /// query API but must not be relied on in the refcount-only hot
    /// path. Writes enter from the adapter side via
    /// [`SemanticGraphStore::insert_resolved_named_type`](crate::semantic_query_memo::SemanticGraphStore::insert_resolved_named_type).
    ///
    /// Returns a warm node id when the identity map has an entry, or
    /// [`QueryError::Miss`] when the entry has not been written yet.
    /// Carries a dispatch fence fragment capturing
    /// `(canonical_id, whole_hash, project_generation)` so warm-read
    /// validation against the live `StoreView` catches stale hits if
    /// any downstream layer memoizes this dispatch path.
    pub(super) fn build_resolved_named_type(
        &self,
        key: &HostResolvedNamedTypeKey,
    ) -> (QueryResult<SemanticNodeId>, DepSignature) {
        let graph = self.graph();
        match graph.resolved_named_type_node_id(key) {
            Some(node_id) => (
                QueryResult::Value(node_id),
                self.dep_signature_for(&key.canonical_id, key.whole_hash),
            ),
            None => (QueryResult::Error(QueryError::Miss), empty_signature()),
        }
    }

    /// `ResolveMacroPayload` body.
    ///
    /// Resolve a Vue compiler macro's payload to a single
    /// `SemanticNodeId` representing the macro's effective TypeExpr.
    /// The `DefineEmits` / `DefineSlots` / `DefineModel` arms read the
    /// owner SFC's `AnalyzedMacro` sidecar (`macros.get(macro_index)`
    /// off the owner artifact's `script_analysis`) rather than
    /// re-walking the AST. The `owner` slot is the env-bearing,
    /// content-free `ResolvedDeclSlotIdentity` (R6) — its whole_hash is
    /// NOT in the key. At value-build time the owner's current content
    /// version is re-sourced live via
    /// `ensure_indexed_ready_serve(owner.defining_canonical)`; a real-file
    /// owner unknown to the live view is a stale key and the build is
    /// marked non-cacheable (`cache_suppress`) (see the arm body for
    /// the stale-key guard).
    ///
    /// Per-arm logic mirrors §3.2 body sketch:
    /// - `DefineProps` / `WithDefaults`: 0 args → `Opaque(Miss)`;
    ///   1 arg → arg unchanged; ≥2 args → `NormalizeIntersection`.
    /// - `DefineEmits`: dispatch `type_args[0]` through `ProjectPath`
    ///   in the caller's mode. Returns the projected surface; the
    ///   consumer (`extract_component_meta` at
    ///   `verter_semantic/src/analysis/component_meta.rs:2449+`) walks
    ///   `properties` + `call_signatures` from the resulting
    ///   `Object`. Sidecar lookup confirms the macro exists and
    ///   anchors the dep_signature.
    /// - `DefineSlots`: dispatch `type_args[0]` through `ProjectPath`
    ///   in the caller's mode; consumer walks slot members from the
    ///   projected `Object`.
    /// - `DefineModel`: dispatch `type_args[0]` through `ProjectPath`
    ///   in the caller's mode; consumer constructs the
    ///   `{ <model_name>: T, "update:<model_name>": (val: T) -> void }`
    ///   shape from `analyzed.model_name` + the resolved T payload.
    /// - `DefineExpose` / `DefineOptions`: 0 args → `Opaque(Miss)`;
    ///   else `type_args[0]` unchanged.
    ///
    /// A real-file owner records `(owner.defining_canonical,
    /// WholeHash(<re-sourced live whole_hash>))` in the local fence so
    /// warm-hit revalidation against the live `StoreView` observes the
    /// macro's owning file generation. A non-file owner (the global /
    /// structural sentinel, a `__builtin__` carrier, or a `<synthetic>`
    /// test identity) records NO file whole_hash and roots its version
    /// entirely through its `type_args` nodes. Every arm additionally
    /// pins the project generation in the fence.
    ///
    /// **Recursion safety:** A self-reference like
    /// `type R = { next: R }; defineEmits<{ recurse: [R] }>()` reaches
    /// the same `Instantiate` key during projection of `R` and the
    /// dispatch sentinel emits `Opaque(QueryError::RecursiveRef)` —
    /// the projection path observes a `Recursive` `QueryResult`,
    /// which propagates here as `QueryResult::Recursive(node)`.
    pub(super) fn build_resolve_macro_payload(
        &self,
        owner: &crate::semantic_query::ResolvedDeclSlotIdentity,
        macro_index: usize,
        macro_kind: verter_semantic::analysis::AnalyzedMacroKind,
        type_args: &Arc<[SemanticNodeId]>,
        mode: ProjectionMode,
    ) -> crate::project_semantic_dispatch::walk::QueryBuildOutput {
        use verter_semantic::analysis::AnalyzedMacroKind;

        // R6 / R20: the key is content-free. Re-source the owning
        // SFC's content version from the live indexed view at
        // value-build time so the cached `MemoEntry` self-roots on
        // the current generation's whole_hash. Three classes of
        // non-file owner exist and must NOT invent a synthetic
        // `FileWholeHash` (mirrors the `build_instantiate` non-file
        // base rule):
        //  - the global / structural sentinel (`canonical_id == ""`);
        //  - built-in utility carriers (`canonical_id == "__builtin__"`);
        //  - synthetic test identities (`canonical_id == "<synthetic>"`).
        // These owners root self-version through their `type_args`
        // nodes only (no file fact in the fence or the self-root set).
        // A real-file owner whose `ensure_indexed_ready_serve` returns `None`
        // (the file is unknown to the live view) is a STALE KEY: the
        // build still hands the value to the caller but marks the
        // output non-cacheable (`cache_suppress`) rather than publishing
        // a result self-rooted on the sentinel hash `0`, which could
        // later serve stale.
        let owner_canonical_str = owner.defining_canonical.as_ref();
        let is_non_file_owner = crate::semantic_query::is_non_file_base(owner_canonical_str);
        let owner_indexed: Option<Arc<crate::project_type_store::IndexedReady>> =
            if is_non_file_owner {
                None
            } else {
                self.ctx
                    .ensure_indexed_ready_serve(owner_canonical_str)
                    .map(|serve| serve.indexed)
            };
        // A real-file owner unknown to the live view is a stale key —
        // suppress admission so the next caller cold-recomputes.
        let stale_real_file_owner = !is_non_file_owner && owner_indexed.is_none();
        let owner_whole_hash: crate::semantic_query::HashValue = owner_indexed
            .as_ref()
            .map(|indexed| indexed.whole_hash)
            .unwrap_or_default();

        // Seed the local fence with the macro's owning canonical and
        // the re-sourced content version — but ONLY for a real-file
        // owner. A non-file owner has no `FileWholeHash` to root on;
        // recording `(non_file_canonical, 0)` would fabricate a content
        // version for a builtin/structural carrier. The non-file owner
        // roots entirely through its `type_args` (the same rule
        // `build_instantiate` applies to a non-file base).
        let mut local_fence: Vec<(Arc<str>, crate::semantic_query::DepVersion)> = Vec::new();
        if !is_non_file_owner {
            local_fence.push((
                Arc::clone(&owner.defining_canonical),
                crate::semantic_query::DepVersion::WholeHash(owner_whole_hash),
            ));
        }
        // Always pin the project-generation so the fence catches
        // workspace-wide changes that could invalidate the lowering
        // basis (mirrors `dep_signature_for` semantics).
        let project_gen = self.ctx.project_type_store().project_generation();
        local_fence.push((
            Arc::clone(&owner.defining_canonical),
            crate::semantic_query::DepVersion::ProjectGeneration(project_gen),
        ));

        // Fold BOTH metadata signals from the nested macro-payload reads:
        // `cache_suppress` (inner-memo non-cacheability) and
        // `result_is_partial` (budget/walker partial). A bare
        // `read.value`-only extraction here would drop both via
        // `QueryBuildOutput::from`'s default-false, letting a budget-tripped
        // nested Instantiate's partial warm the component-meta result.
        let mut nested_cache_suppress = false;
        let mut nested_result_is_partial = false;
        let result = match macro_kind {
            AnalyzedMacroKind::DefineProps | AnalyzedMacroKind::WithDefaults => {
                // §3.2 sketch: 0 args → Miss; 1 arg → arg directly;
                // ≥2 args → intersection-normalised result.
                if type_args.is_empty() {
                    QueryResult::Value(self.opaque(QueryError::Miss))
                } else if type_args.len() == 1 {
                    QueryResult::Value(type_args[0])
                } else {
                    let read = self.execute_read(SemanticQueryKey::NormalizeIntersection {
                        members: Arc::clone(type_args),
                    });
                    crate::component_meta_audit::merge_dep_signature_into_local_fence(
                        &mut local_fence,
                        &read.dep_signature,
                    );
                    nested_cache_suppress |= read.cache_suppress;
                    nested_result_is_partial |= read.result_is_partial;
                    read.value
                }
            }
            AnalyzedMacroKind::DefineEmits
            | AnalyzedMacroKind::DefineSlots
            | AnalyzedMacroKind::DefineModel => {
                // Read the macro snapshot directly from the live
                // indexed view obtained at function entry.
                // Source-and-consistency: `owner_indexed` came from
                // `ensure_indexed_ready_serve` (the live view), and its
                // `whole_hash` already discriminates content versions
                // through the cached `MemoEntry`'s self-root rail. A
                // missing `IndexedReady` for a snapshot-consuming arm
                // is a stale key — refuse admission so the next
                // caller cold-recomputes. A missing `script_analysis`
                // on a present `IndexedReady` means the SFC carries no
                // macros — return Miss.
                let snapshot = match owner_indexed
                    .as_ref()
                    .and_then(|indexed| indexed.script_analysis.clone())
                {
                    Some(s) => s,
                    None => {
                        let mut out =
                            crate::project_semantic_dispatch::walk::QueryBuildOutput::from((
                                QueryResult::Error(QueryError::Miss),
                                fence_to_dep_signature(local_fence),
                            ));
                        // Only suppress when the owner artifact is
                        // genuinely missing (stale key). A present
                        // `IndexedReady` with no macros yields the
                        // non-suppressing Miss above.
                        if owner_indexed.is_none() {
                            out.cache_suppress = true;
                        }
                        return out;
                    }
                };
                if snapshot.macros.get(macro_index).is_none() {
                    return (
                        QueryResult::Error(QueryError::Miss),
                        fence_to_dep_signature(local_fence),
                    )
                        .into();
                }
                if type_args.is_empty() {
                    QueryResult::Value(self.opaque(QueryError::Miss))
                } else {
                    // Project the macro's first type argument through the
                    // shared `ProjectPath` dispatcher in the caller's
                    // mode. The consumer (component_meta extractor at
                    // `verter_semantic/src/analysis/component_meta.rs:2449+`
                    // for emits, the slot-bindings walker for slots, and
                    // the model-name composer) walks the resulting
                    // surface. Mode-aware so Navigate callers don't pay
                    // for full member expansion.
                    let read = self.execute_read(SemanticQueryKey::ProjectPath {
                        base: type_args[0],
                        path: Arc::from(Vec::<PathSegment>::new().into_boxed_slice()),
                        context: crate::semantic_query::ProjectionReductionContext::published(mode),
                    });
                    local_fence.extend(read.dep_signature.iter().cloned());
                    nested_cache_suppress |= read.cache_suppress;
                    nested_result_is_partial |= read.result_is_partial;
                    read.value
                }
            }
            AnalyzedMacroKind::DefineExpose | AnalyzedMacroKind::DefineOptions => {
                if type_args.is_empty() {
                    QueryResult::Value(self.opaque(QueryError::Miss))
                } else {
                    QueryResult::Value(type_args[0])
                }
            }
        };

        // Self-version rooting: the macro payload was resolved from the
        // owning SFC's macro analysis at the re-sourced `owner_whole_hash`
        // AND from the `type_args` nodes — every arm derives its value
        // from `type_args` (returned directly for `DefineProps` /
        // `WithDefaults` 1-arg and `DefineExpose` / `DefineOptions`;
        // `NormalizeIntersection`-normalised for the ≥2-arg props arms;
        // `ProjectPath`-projected for `DefineEmits` / `DefineSlots` /
        // `DefineModel`). When a type argument is file-derived from
        // another canonical the result transitively depends on that
        // file's content, so the carrier must self-root on it too.
        // Root the memo entry on the owning canonical AND on each
        // `NodeScopeId::File`-scoped `type_args` node's
        // `(canonical, observed_hash)` self-root so a content edit to the
        // SFC OR to any type argument's originating file misses the warm
        // read. Structural type args (`Global`-scoped primitives) and an
        // empty `type_args` set contribute nothing.
        //
        // Non-file owner fork: when the owner names no file (the
        // structural sentinel `""`, the builtin carrier `"__builtin__"`,
        // or the synthetic test sentinel `"<synthetic>"`), there is no
        // `FileWholeHash` to root on — `owner_whole_hash` is the default
        // sentinel `0`. Skip the file-side self-root in those cases and
        // rely entirely on the `type_args` self-roots (the same rule the
        // `build_instantiate` non-file base path applies).
        let mut observed_self_roots: Vec<(Arc<str>, crate::semantic_query::HashValue)> =
            if is_non_file_owner {
                Vec::new()
            } else {
                vec![(Arc::clone(&owner.defining_canonical), owner_whole_hash)]
            };
        for arg_root in self.observed_self_roots_from_nodes(type_args.iter().copied()) {
            if !observed_self_roots
                .iter()
                .any(|(c, h)| *c == arg_root.0 && *h == arg_root.1)
            {
                observed_self_roots.push(arg_root);
            }
        }
        let mut output = crate::project_semantic_dispatch::walk::QueryBuildOutput::from((
            result,
            fence_to_dep_signature(local_fence),
        ))
        .with_observed_self_roots(observed_self_roots);
        // Fold the nested macro-payload reads' metadata onto the build
        // output so a budget/walker partial in a nested
        // `NormalizeIntersection` / `ProjectPath` read taints this macro
        // payload result (and suppresses the component-meta warm gate),
        // while a benign non-cacheable nested read still refuses inner-memo
        // admission without falsely marking this result partial.
        output.cache_suppress |= nested_cache_suppress;
        output.result_is_partial |= nested_result_is_partial;
        // Stale real-file owner: the key names a file unknown to the
        // live view (`ensure_indexed_ready_serve == None`). The value flows to
        // the caller, but the entry is non-cacheable — never publish a
        // result self-rooted on the sentinel hash `0`.
        if stale_real_file_owner {
            output.cache_suppress = true;
        }
        output
    }

    pub(super) fn intern_normalized_union_or_intersection(
        &self,
        members: &[SemanticNodeId],
        is_union: bool,
    ) -> SemanticNodeId {
        let mut sorted: Vec<SemanticNodeId> = members.to_vec();
        sorted.sort_by_key(|id| id.0);
        sorted.dedup();
        if sorted.is_empty() {
            return self
                .graph()
                .intern_node(SemanticNodeData::Primitive(PrimitiveKind::Never));
        }
        if sorted.len() == 1 {
            return sorted[0];
        }
        let boxed: Arc<[SemanticNodeId]> = Arc::from(sorted.into_boxed_slice());
        if is_union {
            self.graph().intern_node(SemanticNodeData::Union(boxed))
        } else {
            self.graph()
                .intern_node(SemanticNodeData::Intersection(boxed))
        }
    }
}

/// Path-prefix peek. Walks `path` from longest to
/// shortest non-empty prefix, returning the warm `(base, path[..k],
/// Navigate)` entry's resolved node and `k` if any such prefix is
/// memoized. Returns `None` when no prefix is warm — caller falls back
/// to walking the full path from `base`.
///
/// The lookup forces `mode: Navigate` regardless of
/// the caller's mode because intermediate path hops MUST be cached as
/// Navigate per the path-precise rule (CLAUDE.md "Macro Type Traversal
/// Rule"). The terminal hop keeps the caller's mode and is published by
/// `execute_cooperative` directly; the prefix peek only inspects
/// intermediate hops.
///
/// Increments the test-only `PREFIX_PEEK_HITS` thread-local on success
/// so `project_path_prefix_peek_short_circuits_sibling_walk` can
/// discriminate pre-fix (no helper / always None) vs post-fix (peek hits
/// the warm prefix).
fn find_longest_warm_prefix(
    graph: &crate::semantic_query_memo::SemanticGraphStore,
    ctx: &dyn crate::resolver_core::ResolverContext,
    base: SemanticNodeId,
    path: &Arc<[PathSegment]>,
) -> Option<(SemanticNodeId, usize)> {
    for k in (1..path.len()).rev() {
        let prefix_path: Arc<[PathSegment]> = Arc::from(path[..k].to_vec().into_boxed_slice());
        let prefix_key = SemanticQueryKey::ProjectPath {
            base,
            path: prefix_path,
            context: crate::semantic_query::ProjectionReductionContext::published(
                ProjectionMode::Navigate,
            ),
        };
        // Validate-before-bubble: a stale prefix entry must neither
        // surface as a hit nor pollute the active fact tracer.
        if let Some(hit) = graph.get_validated(&prefix_key, ctx) {
            if let QueryResult::Value(prefix_node) = hit.value {
                #[cfg(test)]
                PREFIX_PEEK_HITS.with(|c| *c.borrow_mut() += 1);
                return Some((prefix_node, k));
            }
        }
    }
    None
}

/// Collect per-linear-prefix backfill records for the
/// `(base, path[..i+1], Navigate)` keys.
///
/// **Deferred publication.** Returns a `Vec<PrefixBackfill>` the
/// caller threads onto `QueryBuildOutput.pending_prefix_backfills`.
/// The shared cold-build helper publishes those records AFTER
/// `install_fact_tracer` finalises so each backfilled memo entry's
/// carrier holds the parent's authoritative path-precise fact
/// signature (not a fence-derived legacy-only signature that loses
/// `Parse(...)` / `ResolveImports(...)` / `RouteSurface(...)`
/// facts).
///
/// Skips the last index — the full key is owned by
/// `execute_cooperative` (it carries the caller's mode, not Navigate;
/// the path-precise rule places terminal hops at the caller's mode).
///
/// Skips `None` entries (arm-splits at Union / Intersection /
/// open-Conditional positions); those positions have no single
/// canonical answer for `(base, path[..k], Navigate)`.
/// The §3.4 materialised-record set for a path-projection terminal entry:
/// the terminal point at the FULL `path` (the caller's `terminal_mode`)
/// PLUS one `Demand::navigate(path[..k])` per CONTIGUOUS LINEAR walked
/// intermediate. The linear run spans the warm prefix already established
/// by [`find_longest_warm_prefix`] (`start_index` positions, all linear by
/// construction — a prior walk produced a single canonical node at
/// `path[..start_index]`) plus the leading `Some` run of the current
/// walk's `intermediates` (excluding the terminal slot), stopping at the
/// first arm-split `None` exactly as [`collect_prefix_backfills`] does.
///
/// The navigate-hop points are inert for the terminal family's own
/// warm-hit gate (requests there are always at the full path); they record
/// the honest materialisation per §3.4 and NEVER inflate a prefix hop to
/// the terminal mode it never expanded.
pub(super) fn path_walk_materialized_set(
    path: &Arc<[PathSegment]>,
    terminal_mode: ProjectionMode,
    start_index: usize,
    intermediates: &[Option<SemanticNodeId>],
) -> MaterializedSet {
    let n = path.len();
    let mut terminal = Demand::from(terminal_mode);
    terminal.projection.path = ProjectionPath::from(Arc::clone(path));
    let mut points = vec![MaterializedPoint::new(terminal)];
    if n >= 2 {
        // Number of full-path intermediate positions (1..=n-1) that are on
        // the contiguous linear run. `start_index` warm-prefix positions
        // are linear; the current walk extends the run by its leading
        // `Some` count (excluding the terminal slot at index
        // `walker_path.len() - 1`).
        let walker_path_len = n - start_index;
        let walked_linear = intermediates
            .iter()
            .take(walker_path_len.saturating_sub(1))
            .take_while(|node| node.is_some())
            .count();
        let linear_hops = (start_index + walked_linear).min(n - 1);
        for k in 1..=linear_hops {
            let prefix: Arc<[PathSegment]> = Arc::from(path[..k].to_vec().into_boxed_slice());
            points.push(MaterializedPoint::new(Demand::navigate(
                ProjectionPath::from(prefix),
            )));
        }
    }
    MaterializedSet::from_points(points)
}

fn collect_prefix_backfills(
    base: SemanticNodeId,
    path: &Arc<[PathSegment]>,
    intermediates: &[Option<SemanticNodeId>],
) -> Vec<crate::project_semantic_dispatch::walk::PrefixBackfill> {
    // Backfill is only meaningful for the contiguous LINEAR prefix of
    // the walk — the leading run of `Some(node)` entries before any
    // arm-split. Once the walker hits a Union / Intersection /
    // Conditional, subsequent intermediates may belong to per-arm
    // sub-walks (which the iterative worklist runs as their own
    // advance_step calls) and no longer line up with the trunk's
    // path index.
    //
    // Bound `i` so that:
    //   - `i < intermediates.len() - 1` — skip the last intermediate;
    //     the terminal full-path key is owned by `execute_cooperative`
    //     (it carries the caller's mode, not Navigate).
    //   - `i < path.len() - 1` — keep `path[..i + 1]` strictly shorter
    //     than the full path (sibling-sharable prefixes only) and avoid
    //     out-of-range slicing when arm-split sub-walks pushed extra
    //     entries past `path.len()`.
    //   - Break at the first `None` — after an arm-split the index no
    //     longer lines up with `path[..i + 1]` so subsequent entries
    //     are not canonical answers for that key.
    let max_i = intermediates.len().min(path.len()).saturating_sub(1);
    let mut out = Vec::new();
    for i in 0..max_i {
        let Some(node) = intermediates[i] else { break };
        let prefix_path: Arc<[PathSegment]> = Arc::from(path[..i + 1].to_vec().into_boxed_slice());
        let prefix_key = SemanticQueryKey::ProjectPath {
            base,
            path: Arc::clone(&prefix_path),
            context: crate::semantic_query::ProjectionReductionContext::published(
                ProjectionMode::Navigate,
            ),
        };
        // §3.4: the prefix family's entry records exactly its own
        // `Navigate` hop at the prefix path — NOT a nominal/meet point. A
        // `Navigate` request at this prefix self-satisfies; a `Shallow` /
        // `Expanded` request at this prefix misses (the walk never
        // expanded the prefix — it only navigated through it).
        let satisfied_projection = MaterializedSet::single(MaterializedPoint::new(
            Demand::navigate(ProjectionPath::from(prefix_path)),
        ));
        out.push(crate::project_semantic_dispatch::walk::PrefixBackfill {
            key: prefix_key,
            node,
            satisfied_projection,
        });
    }
    out
}

/// Helper — convert a per-call local fence (one
/// `(canonical, version)` entry per dep fact accumulated during the
/// build path) into the canonical `Arc<[(Arc<str>, DepVersion)]>`
/// shape returned alongside `QueryResult`.
fn fence_to_dep_signature(
    fence: Vec<(Arc<str>, crate::semantic_query::DepVersion)>,
) -> DepSignature {
    Arc::from(fence.into_boxed_slice())
}

/// Apply a TS string-intrinsic case transform to a single string literal value.
/// `Capitalize` / `Uncapitalize` toggle the case of the FIRST character only;
/// `Uppercase` / `Lowercase` transform the whole string.
fn transform_string_intrinsic(intrinsic: &str, text: &str) -> String {
    match intrinsic {
        "Uppercase" => text.to_uppercase(),
        "Lowercase" => text.to_lowercase(),
        "Capitalize" => {
            let mut chars = text.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                None => String::new(),
            }
        }
        "Uncapitalize" => {
            let mut chars = text.chars();
            match chars.next() {
                Some(first) => first.to_lowercase().collect::<String>() + chars.as_str(),
                None => String::new(),
            }
        }
        // Unreachable in practice (the caller matches the four names), but a
        // total function is safer than a panic on a future caller.
        _ => text.to_string(),
    }
}

/// The outcome of applying a mapped-type key remap (`[K in … as <expr>]`) to a
/// single iteration key. The shared classifier
/// [`ProjectSemanticDispatch::mapped_member_name_remap_outcome`] produces this;
/// both the Expanded build ([`ProjectSemanticDispatch::build_mapped_type`]) and
/// the Shallow walker's `synthesise_mapped_surface` consume it identically.
#[derive(Debug, Clone)]
pub(super) enum MappedKeyRemapOutcome {
    /// No `as` clause — keep the iteration key verbatim.
    Keep(std::sync::Arc<str>),
    /// The remap resolved to `never` — DROP this key from the surface.
    Drop,
    /// The remap resolved to a finite string literal / union of literals —
    /// these are the produced key(s) (usually one).
    Keys(Vec<std::sync::Arc<str>>),
    /// The remap is unresolved / non-finite / non-string — the mapped type
    /// FAILS CLOSED to its deferred carrier (never the original key).
    DeferCarrier,
}

/// The result of [`ProjectSemanticDispatch::normalize_tuple_spread`]: either a
/// (possibly spliced) element list that interns as a `Tuple`, or the array
/// node a sole rest-of-array tuple collapses to (`[...E[]]` ≡ `E[]`).
#[derive(Debug, Clone)]
pub(super) enum NormalizedTupleShape {
    Tuple(Vec<crate::semantic_query::TupleElement>),
    Array(SemanticNodeId),
}

/// Whether a mapped-type produced member inherits the matched source member's
/// declaration site (spans + `declaration_origin`).
///
/// Declaration-site inheritance is judged PER PRODUCED NAME: a produced member
/// inherits the matched source member's spans + `declaration_origin` ONLY when
/// its produced name is identical to the source key — the homomorphic /
/// name-preserving image (no-`as` Partial / Required / Readonly, `as K`
/// identity, the verbatim arm of a one-to-many remap). TypeScript preserves
/// JSDoc through that image, and the typeinfo JSDoc enrichment anchors on
/// these spans. A key-remapped arm (a true `as` rename, including renamed arms
/// of a one-to-many remap) publishes a name no source declaration declares —
/// inheriting the source's spans/origin would be a false declaration-site
/// claim, so both sever (default spans, no origin). Optional / readonly /
/// visibility / value selection stay sourced from the matched source member
/// regardless: the rename severs the declaration-site claim, not the member
/// semantics.
///
/// Both rails — the Expanded build ([`ProjectSemanticDispatch::build_mapped_type`])
/// and the Shallow walker's `synthesise_mapped_surface` — judge inheritance
/// through this one predicate so they can never drift.
pub(super) fn mapped_produced_name_inherits_declaration_site(
    produced_name: &str,
    source_key: &str,
) -> bool {
    produced_name == source_key
}

/// The signature-kind bucket a function-signature utility selects from —
/// `Parameters` / `ReturnType` read [`Call`](Self::Call);
/// `ConstructorParameters` / `InstanceType` read
/// [`Construct`](Self::Construct). See
/// [`ProjectSemanticDispatch::select_signature_function`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum SignatureBucket {
    Call,
    Construct,
}

#[cfg(test)]
mod carrier_type_param_descent_tests {
    //! Carrier-arg descent for the type-parameter binder collector.
    //!
    //! `collect_type_param_nodes_by_name` walks a signature's structural
    //! children to find every `TypeParam` binder node named `name` that the
    //! substitute engine would rewrite. A `BareRef` / `TypeOf` / `ImportType`
    //! carrier applies its `type_args` at the reference site; a binder occurrence
    //! inside those args is a position the substitute engine reaches, so the
    //! collector must descend `SemanticNodeData::carrier_type_args` (args-only,
    //! no head resolution) — otherwise a `<T>`-bearing carrier arg leaves `T`
    //! unspecialised at instantiation time.

    use std::sync::Arc;

    use crate::project_semantic_dispatch::ProjectSemanticDispatch;
    use crate::semantic_query::{
        DeclIdentity, NodeScopeId, ScopeId, SemanticNodeData, SemanticNodeId, ValueRootKey,
    };
    use crate::types::HostConfig;
    use crate::VerterHost;

    fn carrier_wrapping(
        graph: &crate::semantic_query_memo::SemanticGraphStore,
        arg: SemanticNodeId,
        kind: u8,
    ) -> SemanticNodeId {
        let args: Arc<[SemanticNodeId]> = Arc::from(vec![arg].into_boxed_slice());
        match kind {
            0 => graph.intern_node(SemanticNodeData::new_bare_ref(
                Arc::from("Foo"),
                NodeScopeId::Global,
                args,
            )),
            1 => graph.intern_node(SemanticNodeData::new_typeof(
                ValueRootKey {
                    scope: ScopeId {
                        canonical_id: Arc::from("/v.ts"),
                        local_scope: None,
                    },
                    name: Arc::from("factory"),
                },
                Arc::from(Vec::new().into_boxed_slice()),
                args,
            )),
            _ => graph.intern_node(SemanticNodeData::new_import_type(
                Arc::from("./m"),
                Arc::from(vec![Arc::<str>::from("G")].into_boxed_slice()),
                args,
                false,
            )),
        }
    }

    // ── D4 — collect_type_param_nodes_by_name descends carrier args ─────────
    //
    // A `TypeParam` named `X` inside a carrier's `type_args` is a binder
    // occurrence the substitute engine reaches; the collector must return it.
    // NEGATIVE: with the unchanged `_ => {}` arm the carrier is a leaf and the
    // binder node is missed (the returned set would not contain it).
    #[test]
    fn collect_type_param_nodes_descends_carrier_args() {
        let host = VerterHost::new_standalone(HostConfig::default());
        let dispatch = ProjectSemanticDispatch::new(&host);
        let graph = Arc::clone(host.project_type_store().semantic_graph());

        let binder = graph.intern_node(SemanticNodeData::TypeParam {
            decl: DeclIdentity::synthetic("X"),
            param_index: 0,
            constraint: None,
            default: None,
            display_name: Arc::from("X"),
        });

        for kind in 0u8..3 {
            let carrier = carrier_wrapping(&graph, binder, kind);
            let found = dispatch.collect_type_param_nodes_by_name(carrier, "X", false);
            assert!(
                found.contains(&binder),
                "a `TypeParam` named X inside a carrier's type_args (kind {kind}) must be \
                 collected; got {found:?} for carrier {:?}",
                graph.node_data(carrier).as_deref()
            );
        }

        // NEGATIVE control: searching for a DIFFERENT name does NOT collect the
        // X binder (proving the descent honours the name, not a blanket collect).
        for kind in 0u8..3 {
            let carrier = carrier_wrapping(&graph, binder, kind);
            let found = dispatch.collect_type_param_nodes_by_name(carrier, "Y", false);
            assert!(
                !found.contains(&binder),
                "searching name Y must NOT collect the X binder reached through a carrier arg \
                 (kind {kind})"
            );
        }
    }
}
