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
    empty_signature, utility_param_names, DispatchHost, ProjectSemanticDispatch,
    SessionDispatchHost, ShallowRelation,
};
use crate::semantic_query::{
    BranchSelection, DepSignature, HostResolvedNamedTypeKey, IndexSignature, LiteralValue,
    NodeScopeId, OriginEdgeKind, OriginMeta, PathSegment, PrimitiveKind, ProjectionMode,
    QueryError, QueryResult, ReductionDemand, ResolveDeclKey, SemanticNodeData, SemanticNodeId,
    SemanticQueryApi, SemanticQueryKey, SurfaceMember, SurfaceView, ValueRootKey,
};

/// Encode a [`ProjectionReductionContext`] as a compact u32 bit
/// pattern used in the Phase G mapped-member-materialization
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
        // `IndexedReady` ONCE through `ensure_indexed_ready` — the
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
            .ensure_indexed_ready(key.scope.canonical_id.as_ref())
        {
            Some(indexed) => indexed,
            None => return (QueryResult::Error(QueryError::Miss), empty_signature()).into(),
        };
        let shallow = &indexed.shallow_state;
        let observed_hash = indexed.whole_hash;

        let has_type_symbol = shallow.symbol(key.name.as_ref()).is_some();
        let has_value_symbol = shallow.value_symbol(key.name.as_ref()).is_some();
        let has_export = shallow.exports.contains_key(key.name.as_ref());
        let has_import_local = shallow.import_targets.contains_key(key.name.as_ref());

        if !(has_type_symbol || has_value_symbol || has_export || has_import_local) {
            // Wildcard re-export fall-through. A `export * from './base'`
            // barrel carries no direct symbol / named export for `BaseW` —
            // it is reachable only THROUGH the wildcard chain. When the
            // requested name is absent from every direct map BUT the file
            // declares `export *` sources, follow the export graph (the
            // shared route resolver already implements
            // direct > aliased > wildcard precedence) to the defining
            // `(canonical, name)` and resolve THAT declaration. This makes
            // `ResolveDecl` carrier-complete for wildcard-barrel heritage
            // (`interface Props extends BaseW` where `BaseW` is reached via
            // `export *`), parity with the eager OXC rail which already
            // follows the wildcard through the route frontier. Named
            // re-exports (`export { X } from`) populate `exports` directly
            // and never reach this fall-through.
            if shallow.has_wildcard_reexports() {
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
                        // barrel file (whose wildcard source list selected
                        // the target) AND the resolved declaration's own
                        // self-roots, so a content edit to EITHER the
                        // barrel's `export *` clause or the defining file
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
            return (QueryResult::Error(QueryError::Miss), empty_signature()).into();
        }

        // Record the declaration's origin scope in the sidecar (
        // + C1) so dispatch builders reached from this anchor can route
        // per-base-scope lookups through the scope's declaration-scope
        // payload.
        let scope = NodeScopeId::File {
            canonical_id: Arc::clone(&key.scope.canonical_id),
            whole_hash: observed_hash,
            local_scope: key.scope.local_scope,
        };
        let signature = self.dep_signature_for(&key.scope.canonical_id, observed_hash);
        // C16: DeclAnchor retired. Return a DeclPlaceholder that carries
        // enough identity for callers to construct Instantiate keys.
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
    pub(super) fn build_typeof(
        &self,
        value_root: &ValueRootKey,
    ) -> crate::project_semantic_dispatch::walk::QueryBuildOutput {
        // Phase C: telemetry for the typeof traversal site (gemini's
        // HIGH-confidence direction; brief site `build.rs:162`).
        if let Some(observer) = verter_audit::current_observer() {
            observer.record_event(verter_audit::AuditEvent::BuildTypeofCall);
        }
        // Self-version rooting: observe the value-root scope
        // canonical.s `IndexedReady` ONCE through the overlay-aware
        // `ensure_indexed_ready` accessor. The single observed
        // artifact roots both the node.s `NodeScopeId::File` and the
        // memo entry.s self-root `FileWholeHash` on
        // `indexed.whole_hash`; its `shallow_state` is the shallow
        // inventory this builder reads — value basis and self-root
        // descend from one observation.
        let indexed = match self
            .ctx
            .ensure_indexed_ready(value_root.scope.canonical_id.as_ref())
        {
            Some(indexed) => indexed,
            None => {
                // Phase C: prepared-value miss site.
                if let Some(observer) = verter_audit::current_observer() {
                    observer.record_event(verter_audit::AuditEvent::BuildTypeofPreparedValueMiss);
                }
                return (QueryResult::Error(QueryError::Miss), empty_signature()).into();
            }
        };
        let shallow = &indexed.shallow_state;
        let observed_hash = indexed.whole_hash;

        let has_value = shallow.value_symbol(value_root.name.as_ref()).is_some();
        let has_import_local = shallow
            .import_targets
            .contains_key(value_root.name.as_ref());
        let has_type_symbol = shallow.symbol(value_root.name.as_ref()).is_some();
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
                None => return (QueryResult::Error(QueryError::Miss), empty_signature()).into(),
            };
        let prepared = match self
            .ctx
            .prepared_value_decl(&root_identity.canonical_id, &root_identity.symbol_name)
            .or_else(|| {
                // Fallback to export-target walk when prepared cache misses
                // on the resolved root — matches the legacy
                // `SessionSolverHost::resolve_prepared_value_decl`.
                if root_identity.canonical_id.is_empty() {
                    return None;
                }
                let target = self.ctx.resolve_value_export_target(
                    &root_identity.canonical_id,
                    &root_identity.symbol_name,
                )?;
                if target.canonical_id == root_identity.canonical_id
                    && target.name == root_identity.symbol_name
                {
                    return None;
                }
                self.ctx
                    .prepared_value_decl(&target.canonical_id, &target.name)
            }) {
            Some(prepared) => prepared,
            None => return (QueryResult::Error(QueryError::Miss), empty_signature()).into(),
        };
        let empty_env = FxHashMap::default();
        let mut substitutions = Vec::new();
        // CONVERGENCE (scope-bounded to a synthesized `.vue`/typeinfo-scratch
        // `default` ONLY): when the resolved value root is EXACTLY the
        // synthesized `.vue` public-instance `default` — `root_identity`'s symbol
        // is `default` AND the resolved canonical's shallow `default` value
        // symbol carries the `is_synthesised_vue_default` provenance flag — the
        // construct-signature RETURN must be produced by the keyed
        // `Instantiate(.vue default)` query, not by re-lowering the synthesized
        // default's `function_signature.return_type` here. This keeps `typeof
        // Foo` / `InstanceType<typeof Foo>` (Foo a `.vue`) on the SAME semantic
        // identity + recursion guard the bare-`Ref` `.vue` route uses. NOTHING
        // else changes: non-`.vue` `typeof`, userland `.vue` defaults, ordinary
        // `.ts`/`.tsx` constructors/classes/functions/enums/object-literals, and
        // generic `InstanceType<T>` all fall through to the unchanged chain
        // below.
        let synthesised_default: Option<crate::semantic_query::HashValue> =
            if root_identity.symbol_name == "default" {
                self.ctx
                    .ensure_indexed_ready(root_identity.canonical_id.as_str())
                    .and_then(|indexed| {
                        indexed
                            .shallow_state
                            .value_symbol("default")
                            .filter(|sym| sym.is_synthesised_vue_default)
                            .map(|_| indexed.whole_hash)
                    })
            } else {
                None
            };
        let node_id = if let Some(_resolved_default_whole_hash) = synthesised_default {
            let resolved_default_canonical: Arc<str> =
                Arc::from(root_identity.canonical_id.as_str());
            self.build_synthesized_vue_default_construct_object(&resolved_default_canonical, &scope)
        } else if let Some(ty_ann) = prepared.type_annotation.as_ref() {
            self.shallow_lower_type_expr(
                ty_ann,
                &empty_env,
                &scope,
                &prepared.name_resolution,
                scope_payload.as_ref(),
                &shadowing,
                &mut substitutions,
                crate::semantic_query::ProjectionMode::Expanded,
            )
        } else if let Some(shape) = prepared.object_shape.as_ref() {
            self.shallow_lower_type_expr(
                &TypeExpr::Object(Arc::new(shape.clone())),
                &empty_env,
                &scope,
                &prepared.name_resolution,
                scope_payload.as_ref(),
                &shadowing,
                &mut substitutions,
                crate::semantic_query::ProjectionMode::Expanded,
            )
        } else if let Some(sig) = prepared.function_signature.as_ref() {
            // `FunctionSignature` carries per-parameter spans (preserved by the
            // clone) but no whole-signature span, so the signature span stays
            // `None` here.
            let function_expr = FunctionExpr::synthetic(
                sig.parameters.clone(),
                sig.return_type.clone().map(Arc::new),
                sig.type_parameters.clone(),
            );
            let object_expr = ObjectExpr {
                properties: vec![if prepared.kind
                    == verter_semantic::analysis::type_eval::ValueDeclKind::Class
                {
                    ObjectMember::ConstructSignature(function_expr)
                } else {
                    ObjectMember::CallSignature(function_expr)
                }],
            };
            self.shallow_lower_type_expr(
                &TypeExpr::Object(Arc::new(object_expr)),
                &empty_env,
                &scope,
                &prepared.name_resolution,
                scope_payload.as_ref(),
                &shadowing,
                &mut substitutions,
                crate::semantic_query::ProjectionMode::Expanded,
            )
        } else if let Some(members) = prepared.enum_members.as_ref() {
            let object_expr = ObjectExpr {
                properties: members
                    .iter()
                    .map(|(name, ty)| {
                        // Enum-member synthetic property — the prepared enum
                        // member map carries no per-member source span.
                        ObjectMember::Property(ObjectProperty::synthetic(
                            name.clone(),
                            ty.clone(),
                            false,
                            true,
                        ))
                    })
                    .collect(),
            };
            self.shallow_lower_type_expr(
                &TypeExpr::Object(Arc::new(object_expr)),
                &empty_env,
                &scope,
                &prepared.name_resolution,
                scope_payload.as_ref(),
                &shadowing,
                &mut substitutions,
                crate::semantic_query::ProjectionMode::Expanded,
            )
        } else {
            return (QueryResult::Error(QueryError::Miss), empty_signature()).into();
        };
        let signature = self.dep_signature_for(&value_root.scope.canonical_id, observed_hash);
        crate::project_semantic_dispatch::walk::QueryBuildOutput::from((
            QueryResult::Value(node_id),
            signature,
        ))
        .with_observed_self_roots([(Arc::clone(&value_root.scope.canonical_id), observed_hash)])
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
        let indexed = self.ctx.ensure_indexed_ready(decl_canonical.as_ref())?;
        let default_symbol = indexed.shallow_state.value_symbol("default")?;
        // PROVENANCE gate (prefer-direct-structural-facts-over-heuristics): only
        // the SYNTHESIZED `.vue` public-instance `default` symbol drives this
        // branch. A USERLAND `export default` in a `.vue`'s `<script>` (synthesis
        // skipped, userland default present) carries `is_synthesised_vue_default
        // == false` — even when its value type superficially looks like an
        // instance (`(): { $props: ... } => ...`) — so it falls through to the
        // ordinary prepared-decl path instead of being mistreated as the public
        // instance.
        if !default_symbol.is_synthesised_vue_default {
            return None;
        }
        let instance_shape: TypeExpr = default_symbol
            .function_signature
            .as_ref()?
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

        // `decl_whole_hash` (carried by the `Instantiate` key's `DeclIdentity`)
        // and `observed_hash` (the artifact just read) agree under a stable
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
    /// directly re-lowering the synthesized default's
    /// `function_signature.return_type`.
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
        let instance_return = match self.execute(SemanticQueryKey::Instantiate {
            base: crate::semantic_query::DeclKey {
                canonical_id: Arc::clone(resolved_default_canonical),
                decl_name: Arc::from("default"),
            },
            args: Arc::from(Vec::new().into_boxed_slice()),
            context:
                crate::semantic_query::ProjectionReductionContext::structural_transit_with_mode(
                    ProjectionMode::Navigate,
                ),
        }) {
            QueryResult::Value(node) | QueryResult::Recursive(node) => node,
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

    /// Generic instantiation.
    ///
    /// Receives `DeclIdentity` directly from the `Instantiate` key —
    /// declaration identity is part of the key rather than a separate
    /// `DeclAnchor` node. Fetches the [`PreparedTypeDecl`] via
    /// [`DispatchHost`] and produces **one shell level** of the
    /// declaration's structural shape with `args` bound to the decl's
    /// type parameters.
    ///
    /// `body_mode` controls how the decl body and its argument
    /// expressions are lowered after substitution. Memo entries split
    /// per body_mode (see `family_and_slot` in
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
        identity: &crate::semantic_query::DeclKey,
        args: &Arc<[SemanticNodeId]>,
        context: crate::semantic_query::ProjectionReductionContext,
    ) -> crate::project_semantic_dispatch::walk::QueryBuildOutput {
        // Codex-hybrid spec: the call-site provides
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

        let decl_canonical = &identity.canonical_id;
        let decl_name = &identity.decl_name;
        // R6 / R20: the key is content-free. Re-source the base file's
        // content version from the live indexed view at value-build
        // time, so the cached `MemoEntry` self-roots on the current
        // generation's whole_hash. Three classes of non-file base
        // exist and must NOT invent a synthetic `FileWholeHash`:
        //  - the global / structural sentinel (`canonical_id == ""`);
        //  - built-in utility carriers (`canonical_id == "__builtin__"`);
        //  - synthetic test identities (`canonical_id == "<synthetic>"`).
        // These bases root self-version through their `args` nodes
        // only (no file fact). A real-file base whose `ensure_indexed_ready`
        // returns `None` (the file is unknown to the live view) is a
        // stale key — the build cannot publish a cacheable result and
        // returns `cache_suppress` below.
        let decl_canonical_str = decl_canonical.as_ref();
        let is_non_file_base = decl_canonical_str.is_empty()
            || decl_canonical_str == "__builtin__"
            || decl_canonical_str == "<synthetic>";
        let live_indexed: Option<Arc<crate::project_type_store::IndexedReady>> = if is_non_file_base
        {
            None
        } else {
            self.ctx.ensure_indexed_ready(decl_canonical_str)
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
        // flag `is_synthesised_vue_default` of the resolved `default` value
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
                .ensure_indexed_ready(decl_canonical.as_ref())
                .and_then(|indexed| {
                    indexed
                        .shallow_state
                        .value_symbol("default")
                        .map(|sym| sym.is_synthesised_vue_default)
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
            // Builtin-utility results are NEVER macro-T own-body (codex
            // BINDING design): `defineProps<Omit<Vendor, K>>()` surfaces
            // members that came from `Vendor` via the utility, none of
            // which the author wrote in the macro T body. Downgrade the
            // provenance to structural so the utility's produced members
            // report `declared_in_macro_type_arg = false`. (A carrier
            // `CarrierProps extends Omit<Vendor, K>` is the SEPARATE
            // direct-decl case: CarrierProps's own body is stamped below
            // and only its `extends`-reached members go through this
            // utility path as structural.)
            let output: crate::project_semantic_dispatch::walk::QueryBuildOutput = self
                .build_builtin_utility(
                    base,
                    decl_name.as_ref(),
                    args,
                    context.into_structural_provenance(),
                )
                .into();
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
        // Phase G instrumentation: callsite attribution for
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
                // Provenance downgrades to structural (codex BINDING Stage
                // 1): a type-parameter default is a substituted value, not
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
        // Phase D recursive-ref guard: push `(decl_canonical, decl_name)`
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
        // Surface-provenance handling for the declaration body (codex
        // BINDING design — own-body vs reference discrimination). The bit
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
        // substituted occurrence ( C1 — edges emitted at
        // substitution position; at shell level this aggregates on the
        // result node per lazy block).
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
        crate::project_semantic_dispatch::walk::QueryBuildOutput::from((
            QueryResult::Value(result),
            fence,
        ))
        .with_observed_self_roots(observed_self_roots)
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
    /// provenance with own-body-vs-heritage discrimination (codex
    /// BINDING design).
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
        use verter_type_expr::TypeExpr;

        // Whether an intersection arm is an own-body member-bearing arm
        // (an inline `Object` literal, or a `Parenthesized` wrapper of
        // one) vs a reference carrier (`Ref` / anything else).
        //
        // This is the precise eager `from_root_body` rule: a member is
        // `declared_in_macro_type_arg = true` only when it was lowered
        // from an inline object LITERAL at the macro-T own body. Members
        // reached through a named REFERENCE arm inside an intersection
        // (`defineProps<A & B>()`, or an interface's `extends Base`
        // heritage `Ref`) are NOT literally written in the macro T body,
        // so their provenance decays to structural (`false`). A single
        // direct reference at the macro-T root (`defineProps<Props>()`)
        // is the other `true` case — its OWN-body members carry the bit
        // because `Instantiate(Props)` lowers Props's own object body
        // under the caller's provenance (the non-intersection arm below).
        fn arm_is_own_body(arm: &TypeExpr) -> bool {
            match arm {
                TypeExpr::Object(_) => true,
                TypeExpr::Parenthesized(inner) => arm_is_own_body(inner),
                _ => false,
            }
        }

        // Member-merge role for this declaration's REFERENCE arms (codex
        // BINDING design for the type-resolution unification). A reference arm
        // of an interface/class body is REAL `extends`/`implements` heritage —
        // its inherited members are `Heritage` and SHADOWED by the own-body
        // members. A reference arm of a type-alias body is an AUTHORED
        // intersection arm (`type Props = Base & { dup }`) — its members are
        // `Authored` and must NOT shadow (they intersect). This is the single
        // distinguishing fact P2-1 turns on, read from the typed
        // `prepared.kind`.
        use crate::semantic_query::MemberMergeRole;
        use verter_semantic::analysis::type_eval::TypeDeclKind;
        let reference_arm_role = match prepared.kind {
            TypeDeclKind::Interface | TypeDeclKind::Class => MemberMergeRole::Heritage,
            TypeDeclKind::Alias => MemberMergeRole::Authored,
        };

        match &prepared.body {
            TypeExpr::Intersection(arms) => {
                // Per-arm provenance + merge role: inline-object own-body arms
                // keep the caller's provenance and carry `OwnBody`; reference
                // arms (heritage `extends` Refs for interface/class, or named
                // refs in an author intersection `A & B` for an alias) decay to
                // structural provenance and carry `reference_arm_role`. The
                // own-body arm's members thus SHADOW heritage members of the
                // same name in the surface merge, while authored-intersection
                // arms intersect. Re-intern the merged Intersection so the
                // consumer-visible shape is unchanged.
                let arm_ids: Vec<SemanticNodeId> = arms
                    .iter()
                    .map(|arm| {
                        let arm_context = if arm_is_own_body(arm) {
                            context.with_merge_role(MemberMergeRole::OwnBody)
                        } else {
                            context
                                .into_structural_provenance()
                                .with_merge_role(reference_arm_role)
                        };
                        self.shallow_lower_type_expr_with_context(
                            arm,
                            env,
                            scope,
                            &prepared.name_resolution,
                            scope_payload,
                            shadowing,
                            substitutions,
                            arm_context,
                        )
                    })
                    .collect();
                if arm_ids.is_empty() {
                    self.graph()
                        .intern_node(SemanticNodeData::Primitive(PrimitiveKind::Never))
                } else if arm_ids.len() == 1 {
                    arm_ids[0]
                } else {
                    self.graph().intern_node_with_scope(
                        SemanticNodeData::Intersection(Arc::from(arm_ids.into_boxed_slice())),
                        scope.clone(),
                    )
                }
            }
            // Non-intersection body: a plain inline `Object` (own-body
            // members → caller provenance + `OwnBody`), or a single reference /
            // other shape. For a single direct reference at the macro-T root
            // the caller's provenance flows into the referenced decl's own
            // object body (which surfaces own members `true`, heritage
            // `false` via this same per-arm split one level down). A plain
            // inline object body carries `OwnBody` so a later intersection
            // (e.g. via declaration merging) shadows heritage correctly.
            TypeExpr::Object(_) | TypeExpr::Parenthesized(_) => self
                .shallow_lower_type_expr_with_context(
                    &prepared.body,
                    env,
                    scope,
                    &prepared.name_resolution,
                    scope_payload,
                    shadowing,
                    substitutions,
                    context.with_merge_role(MemberMergeRole::OwnBody),
                ),
            _ => self.shallow_lower_type_expr_with_context(
                &prepared.body,
                env,
                scope,
                &prepared.name_resolution,
                scope_payload,
                shadowing,
                substitutions,
                context,
            ),
        }
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
        // index (codex BINDING design — authoritative own-member
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
                    && !member.declared_in_macro_type_arg
                    && prepared.member_index.contains_key(member.name.as_ref())
                {
                    restamped_any = true;
                    SurfaceMember {
                        declared_in_macro_type_arg: true,
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
                    // The `PreparedMember` (the `member_index` entry) now
                    // carries the IR member's OXC declaration-site spans + the
                    // declaration's defining file, so the append is span-rich.
                    spans: member.spans,
                    declaration_origin: (!member.declaration_origin.is_empty())
                        .then(|| Arc::from(member.declaration_origin.as_str())),
                    declared_in_macro_type_arg: own_body_bit,
                    // `member_index` is the declaration's OWN-body direct
                    // member index (heritage `extends` arms are excluded), so
                    // an appended member is own-body — it SHADOWS an inherited
                    // heritage member of the same name.
                    merge_role: crate::semantic_query::MemberMergeRole::OwnBody,
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
    ///   intrinsics with non-literal inputs): return a shell anchored
    ///   to the utility + arg identity with `Instantiate` +
    ///   `SubstituteTypeParam` edges. The shell's body is lazy —
    ///   callers projecting into it follow the normal `ProjectPath`
    ///   route which terminates with `Miss` until a later track
    ///   implements the full shape. String intrinsics return the
    ///   `String` primitive directly.
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
    ) -> (QueryResult<SemanticNodeId>, DepSignature) {
        use crate::semantic_query::{MapperKey, OptionalityMod, ReadonlyMod};

        let graph = self.graph();
        let fence = self.project_generation_signature();
        self.graph().record_instantiate();

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
        // The mapper's `value_expr` is a placeholder `T[K]` identity — the
        // real evaluator would substitute K across T's member types, but
        // the shell-level result (with per-member modifiers applied)
        // matches the userland mapped type's shell, which is what
        // equivalence tests assert.
        let mapper_for = |opt: OptionalityMod, ro: ReadonlyMod, source: SemanticNodeId| {
            // Thread the outer instantiation's context so a builtin
            // utility called under `StructuralTransit` (e.g.
            // `Partial<Record<keyof T, undefined>>` reached through a
            // relation-engine `infer`-binding pass) does NOT reify
            // `keyof source` into a literal-anchor union.
            let key_space = match self.execute(SemanticQueryKey::KeyOf {
                base: source,
                context,
            }) {
                QueryResult::Value(id) => id,
                _ => self.opaque(QueryError::Miss),
            };
            // Value placeholder: the shell does not eagerly lower per-key
            // substitutions into the value expression. A caller that
            // projects `Partial<T>['x']` walks the ProjectPath into the
            // produced member (which has `value = Miss` under the lazy
            // rule) and the follow-up hop dispatches back through the
            // path walker.
            let value_expr = self.opaque(QueryError::Miss);
            // Synthesise a TypeParam binder node for the utility
            // mapper's `K`. Phase G fix: the param_index is the
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

        match name {
            // ---- Mapper-based utilities ----
            "Partial" if args.len() == 1 => {
                let source = args[0];
                let mapper = mapper_for(OptionalityMod::Add, ReadonlyMod::Keep, source);
                let result = match self.execute(SemanticQueryKey::MappedType {
                    source,
                    mapper,
                    context,
                }) {
                    QueryResult::Value(id) => id,
                    _ => self.opaque(QueryError::Miss),
                };
                record_utility_edges(result);
                (QueryResult::Value(result), fence)
            }
            "Required" if args.len() == 1 => {
                let source = args[0];
                let mapper = mapper_for(OptionalityMod::Remove, ReadonlyMod::Keep, source);
                let result = match self.execute(SemanticQueryKey::MappedType {
                    source,
                    mapper,
                    context,
                }) {
                    QueryResult::Value(id) => id,
                    _ => self.opaque(QueryError::Miss),
                };
                record_utility_edges(result);
                (QueryResult::Value(result), fence)
            }
            "Readonly" if args.len() == 1 => {
                let source = args[0];
                let mapper = mapper_for(OptionalityMod::Keep, ReadonlyMod::Add, source);
                let result = match self.execute(SemanticQueryKey::MappedType {
                    source,
                    mapper,
                    context,
                }) {
                    QueryResult::Value(id) => id,
                    _ => self.opaque(QueryError::Miss),
                };
                record_utility_edges(result);
                (QueryResult::Value(result), fence)
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
                // Phase G fix: the param_index is derived from
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
                let result = match self.execute(SemanticQueryKey::MappedType {
                    source: key_arg,
                    mapper,
                    context,
                }) {
                    QueryResult::Value(id) => id,
                    _ => self.opaque(QueryError::Miss),
                };
                record_utility_edges(result);
                (QueryResult::Value(result), fence)
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
                (QueryResult::Value(result), fence)
            }

            // ---- String intrinsics ----
            // These always produce a string primitive. The actual
            // transformation (uppercase, lowercase, etc.) applies at the
            // literal-string level; with no literal-type support in the
            // semantic graph today the result is the `String` primitive.
            "Uppercase" | "Lowercase" | "Capitalize" | "Uncapitalize" if args.len() == 1 => {
                let result = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
                record_utility_edges(result);
                (QueryResult::Value(result), fence)
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
            "ReturnType" if args.len() == 1 => {
                if let Some(function_node) = self.resolve_call_signature_function(args[0]) {
                    if let Some(SemanticNodeData::Function { return_type, .. }) =
                        self.graph().node_data(function_node).as_deref()
                    {
                        let id = *return_type;
                        record_utility_edges(id);
                        return (QueryResult::Value(id), fence);
                    }
                }
                let result = self.opaque(QueryError::Miss);
                record_utility_edges(result);
                (QueryResult::Value(result), fence)
            }
            "Parameters" if args.len() == 1 => {
                if let Some(function_node) = self.resolve_call_signature_function(args[0]) {
                    if let Some(tuple_id) = self.intern_function_params_tuple(function_node) {
                        record_utility_edges(tuple_id);
                        return (QueryResult::Value(tuple_id), fence);
                    }
                }
                let result = self.opaque(QueryError::Miss);
                record_utility_edges(result);
                (QueryResult::Value(result), fence)
            }
            "ConstructorParameters" if args.len() == 1 => {
                if let Some(function_node) = self.resolve_construct_signature_function(args[0]) {
                    if let Some(tuple_id) = self.intern_function_params_tuple(function_node) {
                        record_utility_edges(tuple_id);
                        return (QueryResult::Value(tuple_id), fence);
                    }
                }
                let result = self.opaque(QueryError::Miss);
                record_utility_edges(result);
                (QueryResult::Value(result), fence)
            }
            "InstanceType" if args.len() == 1 => {
                if let Some(function_node) = self.resolve_construct_signature_function(args[0]) {
                    if let Some(SemanticNodeData::Function { return_type, .. }) =
                        self.graph().node_data(function_node).as_deref()
                    {
                        let id = *return_type;
                        record_utility_edges(id);
                        return (QueryResult::Value(id), fence);
                    }
                }
                let result = self.opaque(QueryError::Miss);
                record_utility_edges(result);
                (QueryResult::Value(result), fence)
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
                        return (QueryResult::Value(result), fence);
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
                let surface = match self.object_filter_source_surface(source_resolved) {
                    Some(view) => view,
                    None => {
                        let result = self.opaque(QueryError::Miss);
                        record_utility_edges(result);
                        return (QueryResult::Value(result), fence);
                    }
                };
                let pick_set: FxHashSet<&str> = pick_names.iter().map(|s| s.as_ref()).collect();
                let picked: Vec<SurfaceMember> = surface
                    .members
                    .iter()
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
                (QueryResult::Value(result), fence)
            }
            "Omit" if args.len() == 2 => {
                let source = args[0];
                let keys_arg = args[1];
                let omit_names = match self.key_names_from_keyspace_node(keys_arg) {
                    Some(names) => names,
                    None => {
                        let result = self.opaque(QueryError::Miss);
                        record_utility_edges(result);
                        return (QueryResult::Value(result), fence);
                    }
                };
                // Context-propagating deferred
                // resolution (see Pick comment above for chain).
                let source_resolved =
                    self.evaluate_deferred_semantic_node_with_context(source, context);
                let surface = match self.object_filter_source_surface(source_resolved) {
                    Some(view) => view,
                    None => {
                        let result = self.opaque(QueryError::Miss);
                        record_utility_edges(result);
                        return (QueryResult::Value(result), fence);
                    }
                };
                let omit_set: FxHashSet<&str> = omit_names.iter().map(|s| s.as_ref()).collect();
                let kept: Vec<SurfaceMember> = surface
                    .members
                    .iter()
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
                (QueryResult::Value(result), fence)
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
                        return (QueryResult::Value(result), fence);
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
                            return (QueryResult::Value(result), fence);
                        }
                    }
                }
                let result = self.intern_normalized_union_or_intersection(&survivors, true);
                record_utility_edges(result);
                (QueryResult::Value(result), fence)
            }

            // ---- Deferred utilities ----
            // `NonNullable` requires recursive null/undefined filtering;
            // `Awaited` requires recursive promise unwrapping. Each
            // emits an `Opaque(Miss)` shell anchored to the instantiate
            // identity so the origin walk remains coherent; full
            // implementation requires path-precise projection upgrades.
            _ => {
                let result = self.opaque(QueryError::Miss);
                record_utility_edges(result);
                (QueryResult::Value(result), fence)
            }
        }
    }

    /// Resolve `node` to a `SemanticNodeData::Function` node via call
    /// signatures. Unwraps a canonical `Function` node directly, an
    /// `Object` surface with exactly one call signature and no user
    /// members / construct signatures, or an `Alias` chain (cycle
    /// guarded).
    ///
    /// Returns `None` when the shape is a plain object, primitive,
    /// opaque, or carries multiple overloads — callers fall through to
    /// the utility's `Opaque(Miss)` shell.
    fn resolve_call_signature_function(&self, node: SemanticNodeId) -> Option<SemanticNodeId> {
        let mut visited: FxHashSet<SemanticNodeId> = FxHashSet::default();
        self.resolve_call_signature_function_inner(node, &mut visited)
    }

    fn resolve_call_signature_function_inner(
        &self,
        node: SemanticNodeId,
        visited: &mut FxHashSet<SemanticNodeId>,
    ) -> Option<SemanticNodeId> {
        if !visited.insert(node) {
            return None;
        }
        let data = self.graph().node_data(node)?;
        match &*data {
            SemanticNodeData::Function { .. } => Some(node),
            SemanticNodeData::Alias(target) => {
                self.resolve_call_signature_function_inner(*target, visited)
            }
            SemanticNodeData::Object(surface)
                if surface.members.is_empty()
                    && surface.construct_signatures.is_empty()
                    && surface.call_signatures.len() == 1 =>
            {
                self.resolve_call_signature_function_inner(surface.call_signatures[0], visited)
            }
            _ => None,
        }
    }

    /// Mirror of [`Self::resolve_call_signature_function`] for construct
    /// signatures (`ConstructorParameters`, `InstanceType`). Construct
    /// signatures are lowered as `Function` nodes too — distinguished
    /// only by the surface bucket they live in.
    fn resolve_construct_signature_function(&self, node: SemanticNodeId) -> Option<SemanticNodeId> {
        let mut visited: FxHashSet<SemanticNodeId> = FxHashSet::default();
        self.resolve_construct_signature_function_inner(node, &mut visited)
    }

    fn resolve_construct_signature_function_inner(
        &self,
        node: SemanticNodeId,
        visited: &mut FxHashSet<SemanticNodeId>,
    ) -> Option<SemanticNodeId> {
        if !visited.insert(node) {
            return None;
        }
        let data = self.graph().node_data(node)?;
        match &*data {
            SemanticNodeData::Alias(target) => {
                self.resolve_construct_signature_function_inner(*target, visited)
            }
            SemanticNodeData::Object(surface)
                if surface.members.is_empty()
                    && surface.call_signatures.is_empty()
                    && surface.construct_signatures.len() == 1 =>
            {
                self.resolve_call_signature_function_inner(surface.construct_signatures[0], visited)
            }
            _ => None,
        }
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
        let elements: Arc<[TupleElement]> = params
            .iter()
            .map(|param| TupleElement {
                label: param.name.as_ref().map(Arc::clone),
                value: param.ty,
                optional: param.optional,
                rest: param.rest,
            })
            .collect::<Vec<_>>()
            .into();
        Some(self.graph().intern_node(SemanticNodeData::Tuple {
            elements,
            readonly: false,
        }))
    }

    // Declaration identity is carried directly by
    // `SemanticQueryKey::Instantiate.base` (`DeclKey`), so there is
    // no arena node to unwrap.

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
        let fence = self.project_generation_signature();
        self.graph().record_path_length(path.len() as u32);
        // Longest-prefix-first peek. Skip when path.len() < 2.
        // Codex-2 r3 fix: prefix entries are cached as Navigate regardless
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
                observed_self_roots: Vec::new(),
                graph_carrier: None,
                self_root_canonicals: Arc::from([]),
                pending_prefix_backfills: Vec::new(),
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
        // codex flagged in `publish_warm_if_absent`).
        let pending_prefix_backfills =
            collect_prefix_backfills(start_base, &walker_path, &walker.intermediate_nodes);
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
            observed_self_roots,
            graph_carrier: None,
            self_root_canonicals: Arc::from([]),
            pending_prefix_backfills,
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
        let data = self.graph().node_data(base);
        let fence = self.project_generation_signature();
        // Codex-hybrid carrier-stop: when the
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
        let node = match data.as_deref() {
            Some(SemanticNodeData::Object(surface)) => self.intern_keyspace_names(
                base,
                surface
                    .members
                    .iter()
                    .map(|member| Arc::clone(&member.name)),
                &fence,
            ),
            Some(SemanticNodeData::Intersection(_) | SemanticNodeData::Union(_)) => self
                .member_names_for_published_projection(base)
                .or_else(|| self.key_names_from_base_node(base))
                .map(|names| self.intern_keyspace_names(base, names, &fence))
                .unwrap_or_else(|| self.graph().intern_node(SemanticNodeData::KeyOf { base })),
            Some(
                SemanticNodeData::TypeParam { .. }
                | SemanticNodeData::IndexedAccess { .. }
                | SemanticNodeData::Mapped { .. }
                | SemanticNodeData::TypeOf { .. }
                | SemanticNodeData::Conditional { .. }
                | SemanticNodeData::Alias(_)
                | SemanticNodeData::Opaque(QueryError::DeclPlaceholder { .. }),
            ) => self.graph().intern_node(SemanticNodeData::KeyOf { base }),
            _ => self.opaque(QueryError::Miss),
        };
        // Self-version rooting: `keyof base` depends on `base`'s member
        // surface, which was lowered from the file `base` originates in.
        // Root the memo entry on that file's observed content version.
        let observed_self_roots = self.observed_self_roots_from_nodes([base]);
        crate::project_semantic_dispatch::walk::QueryBuildOutput::from((
            QueryResult::Value(node),
            fence,
        ))
        .with_observed_self_roots(observed_self_roots)
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
    pub(super) fn source_members_for_published_projection(
        &self,
        source: SemanticNodeId,
    ) -> Option<Vec<SurfaceMember>> {
        if let Some(SemanticNodeData::Object(view)) = self.graph().node_data(source).as_deref() {
            return Some(view.members.to_vec());
        }

        let read = self.execute_read(SemanticQueryKey::ProjectPath {
            base: source,
            path: Arc::from(Vec::<PathSegment>::new().into_boxed_slice()),
            context: crate::semantic_query::ProjectionReductionContext::published(
                ProjectionMode::Shallow,
            ),
        });
        crate::meta_resolve::emit_dispatch_dep_signature_facts(self.ctx, &read.dep_signature);
        let node = match read.value {
            QueryResult::Value(id) => id,
            QueryResult::Recursive(_) | QueryResult::Error(_) => return None,
        };
        match self.graph().node_data(node).as_deref() {
            Some(SemanticNodeData::Object(view)) => Some(view.members.to_vec()),
            _ => None,
        }
    }

    fn member_names_for_published_projection(
        &self,
        source: SemanticNodeId,
    ) -> Option<Vec<Arc<str>>> {
        self.source_members_for_published_projection(source)
            .map(|members| members.into_iter().map(|member| member.name).collect())
    }

    /// Mapped-type rewrite.
    ///
    /// For a mapped type `{ [K in key_space]: value_expr }` with
    /// optional / readonly modifiers (stored on the `MapperKey` and
    /// participating in the cache key), apply lazily:
    ///
    /// 1. Resolve the key space through `SemanticQueryKey::KeyOf` (or
    ///    use the explicit `MapperKey::key_space` if a caller passes a
    ///    pre-computed key union).
    /// 2. For each discovered key, reserve a member slot in the result
    ///    shell. Member optionality / readonly derive from the mapper's
    ///    modifiers (`Add` → always on, `Remove` → always off, `Keep` →
    ///    inherit from the source if available, else default off).
    /// 3. Member values are lazy: C6 interns them as opaque placeholders
    ///    because the full `K → key` substitution over `value_expr`
    ///    requires solver-scale `TypeExpr` lowering that lands in C7's
    ///    userland-equivalence pass. Callers projecting into a produced
    ///    member follow the ProjectPath sub-query route into the
    ///    keyspace + value expression.
    /// 4. Emit `Normalize` edges from the mapped result to each
    ///    contributing key. Emit one `ProjectMember` edge per produced
    ///    member sourcing `[source, key]` with `OriginMeta::ProjectedMember`
    ///    (provenance `MemberEdgeProvenance::MappedKeyEnumerated`)
    ///    carrying the produced name (post-remap if `name_remap` is
    ///    set).
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
        let graph = self.graph();
        let fence = self.project_generation_signature();
        // Self-version rooting: the mapped result depends on the
        // `source` surface, the `key_space`, and the `value_expr` — all
        // already-interned nodes. Root the memo entry on the file
        // content version each file-derived input was lowered from.
        let observed_self_roots =
            self.observed_self_roots_from_nodes([source, mapper.key_space, mapper.value_expr]);

        // Codex-hybrid carrier-stop: outside a
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
                Arc::from(vec![source, mapper.key_space, mapper.value_expr].into_boxed_slice()),
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
        let source_members: Vec<SurfaceMember> = self
            .source_members_for_published_projection(source)
            .unwrap_or_default();
        let key_names: Vec<Arc<str>> = if !source_members.is_empty() {
            if self.uses_synthetic_mapped_key_names(&source_members) {
                match self.key_names_from_keyspace_node(mapper.key_space) {
                    Some(names) => names,
                    None => source_members.iter().map(|m| Arc::clone(&m.name)).collect(),
                }
            } else {
                source_members.iter().map(|m| Arc::clone(&m.name)).collect()
            }
        } else if let Some(names) = self.key_names_from_keyspace_node(mapper.key_space) {
            names
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
                Arc::from(vec![source, mapper.key_space, mapper.value_expr].into_boxed_slice()),
                OriginMeta::None,
                Arc::clone(&fence),
            );
            return crate::project_semantic_dispatch::walk::QueryBuildOutput::from((
                QueryResult::Value(node),
                fence,
            ))
            .with_observed_self_roots(observed_self_roots.clone());
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
                    let base = base.to_decl_key();
                    let args = Arc::clone(args);
                    match self.execute(SemanticQueryKey::Instantiate {
                        base,
                        args,
                        context,
                    }) {
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
        let mut produced: Vec<SurfaceMember> = Vec::with_capacity(key_names.len());
        let mut project_member_edges: Vec<(SemanticNodeId, Arc<str>)> = Vec::new();
        for name in &key_names {
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
            // - `source_members` matches this key AND `value_expr` IS
            //   structurally `T[K]` → use the member value directly
            //   (Partial/Required/Readonly-style mapped types).
            // - `value_expr` does NOT reference `mapper.parameter_node`
            //   → reuse `shared_value` (the once-per-mapped-type
            //   evaluation hoisted above the loop). Both correctness
            //   and the substituted-carrier fallback collapse to the
            //   identical surface member value the per-K materialiser
            //   would produce.
            // - Otherwise (`value_expr` is not `T[K]` and IS K-dependent,
            //   or the source has no matching member) → defer to the
            //   shared per-key materialiser
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
            } else if let Some(shared) = shared_value {
                shared
            } else {
                self.materialize_mapped_member_value_for_key(mapper, name.as_ref(), context)
            };
            // Apply `name_remap` (the `as <expr>` clause) via the
            // shared [`Self::materialize_mapped_member_name_for_key`]
            // helper — same substitution + context-aware evaluation
            // the Shallow walker uses, so the two paths produce
            // identical post-remap names.
            let produced_name = self.materialize_mapped_member_name_for_key(mapper, name, context);
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
                declared_in_macro_type_arg: false,
                // Mapped-type produced members are synthesized by the mapped
                // construction, never an interface/class heritage overlay —
                // `Authored` (they do not participate in own-body shadowing).
                merge_role: crate::semantic_query::MemberMergeRole::Authored,
                // Mapped-produced member: synthesized from a key domain, no
                // single source declaration site — no spans, no declaration file.
                spans: verter_type_expr::MemberSpans::default(),
                declaration_origin: None,
            });
            project_member_edges.push((value, produced_name));
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
        //    - Normalize: result ← [source, key_space, value_expr].
        //    - ProjectMember per produced member.
        graph.record_origin_edge(
            node,
            OriginEdgeKind::Normalize,
            Arc::from(vec![source, mapper.key_space, mapper.value_expr].into_boxed_slice()),
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

        crate::project_semantic_dispatch::walk::QueryBuildOutput::from((
            QueryResult::Value(node),
            fence,
        ))
        .with_observed_self_roots(observed_self_roots)
    }

    /// Per-key Mapped member value materialiser — the SHARED
    /// substrate used by both [`Self::build_mapped_type`]
    /// (Published(Expanded) publication path) and the Shallow walker's
    /// `synthesise_mapped_surface` (Published(Shallow) surface
    /// synthesis at the macro publication boundary).
    ///
    /// Substitutes the mapper binder (`mapper.parameter_node`) with the
    /// enumerated `key_name` literal inside `mapper.value_expr`, then
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
        key_name: &str,
        context: crate::semantic_query::ProjectionReductionContext,
    ) -> SemanticNodeId {
        self.graph().record_mapped_per_k_materialization();
        let key_arg = self
            .graph()
            .intern_node(SemanticNodeData::Literal(LiteralValue::String(
                key_name.to_string(),
            )));
        // Phase G instrumentation — classify this per-K call as
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
                let base = base.to_decl_key();
                let args = Arc::clone(args);
                match self.execute(SemanticQueryKey::Instantiate {
                    base,
                    args,
                    context,
                }) {
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
    /// [`Self::build_conditional`]'s **C11a infer-binding path**
    /// (build.rs: 2266) → `evaluate_deferred_semantic_node(check)`
    /// at default `Published(Expanded)` → IndexedAccess evaluator arm
    /// (evaluate.rs: 95) which hard-codes `ProjectionMode::Navigate`
    /// for the index re-dispatch, so the `PricingPlanSlots["badge"]`
    /// **selected-index / path projection** reduces independently of
    /// the caller's `StructuralTransit(Navigate)` demand. The C11a
    /// binding then closes the conditional to a `Function`.
    ///
    /// Codex Q1 dispatch chain (slot fixture):
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
    ///                 → C11a infer binds P
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
        key_name: &str,
        context: crate::semantic_query::ProjectionReductionContext,
    ) -> SemanticNodeId {
        let key_arg = self
            .graph()
            .intern_node(SemanticNodeData::Literal(LiteralValue::String(
                key_name.to_string(),
            )));
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
        // Phase G instrumentation — selected-key variant.
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
                let base = base.to_decl_key();
                let args = Arc::clone(args);
                match self.execute(SemanticQueryKey::Instantiate {
                    base,
                    args,
                    context,
                }) {
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
                let base = base.to_decl_key();
                let args = Arc::clone(args);
                match self.execute(SemanticQueryKey::Instantiate {
                    base,
                    args,
                    context,
                }) {
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
    /// `Published(Shallow)` per the codex contract (Q3). Callers
    /// passing a non-Published context to this helper are calling
    /// out-of-contract; only `synthesise_mapped_surface` (running
    /// under publication demand) is meant to drive it.
    pub(super) fn mapped_surface_source_members_for_projection(
        &self,
        source: SemanticNodeId,
        _context: crate::semantic_query::ProjectionReductionContext,
    ) -> Option<Vec<SurfaceMember>> {
        self.source_members_for_published_projection(source)
    }

    /// Per-key Mapped member name materialiser — shared by
    /// [`Self::build_mapped_type`] and the Shallow walker's
    /// `synthesise_mapped_surface`.
    ///
    /// Applies the mapper's `name_remap` (the `as <expr>` clause) by
    /// substituting the binder with the key literal, evaluating the
    /// remap under the caller's `context`, and folding the result to a
    /// string-literal name (the template-literal evaluator handles
    /// `` `prefix-${K}` `` shapes). Falls back to the iteration key on
    /// remap resolution failure so the surface remains addressable.
    pub(super) fn materialize_mapped_member_name_for_key(
        &self,
        mapper: &crate::semantic_query::MapperKey,
        key_name: &Arc<str>,
        context: crate::semantic_query::ProjectionReductionContext,
    ) -> Arc<str> {
        let Some(remap_node) = mapper.name_remap else {
            return Arc::clone(key_name);
        };
        let key_literal =
            self.graph()
                .intern_node(SemanticNodeData::Literal(LiteralValue::String(
                    key_name.as_ref().to_string(),
                )));
        let substituted_remap =
            self.substitute_semantic_type_param(remap_node, mapper.parameter_node, key_literal);
        let evaluated_remap =
            self.evaluate_deferred_semantic_node_with_context(substituted_remap, context);
        match self.graph().node_data(evaluated_remap).as_deref() {
            Some(SemanticNodeData::Literal(LiteralValue::String(text))) => Arc::from(text.as_str()),
            _ => Arc::clone(key_name),
        }
    }

    /// Conditional type ( C2 + §2 lazy block + §3
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
    ///   materialised; path projection into the result (C3) drives
    ///   per-subexpression lazy expansion.
    ///
    /// C2's relation evaluator handles the decidable shapes the shallow
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
        let graph = self.graph();
        let fence = self.project_generation_signature();
        // Self-version rooting: the conditional's resolution depends on
        // the `check`, `extends`, and both branch shells — all
        // already-interned nodes. Root the memo entry on the file
        // content version each file-derived input was lowered from.
        let observed_self_roots =
            self.observed_self_roots_from_nodes([check, extends, true_branch, false_branch]);

        // C2 + §3: distributive distribution is the
        // dispatch layer's responsibility. When `distributive == true`
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
                for &member in members.iter() {
                    match self.execute(SemanticQueryKey::Conditional {
                        check: member,
                        extends,
                        true_branch,
                        false_branch,
                        distributive: false,
                    }) {
                        QueryResult::Value(id) => per_member.push(id),
                        _ => {
                            distribution_ok = false;
                            break;
                        }
                    }
                }
                if distribution_ok {
                    if let QueryResult::Value(normalised) =
                        self.execute(SemanticQueryKey::NormalizeUnion {
                            members: Arc::from(per_member.into_boxed_slice()),
                        })
                    {
                        return crate::project_semantic_dispatch::walk::QueryBuildOutput::from((
                            QueryResult::Value(normalised),
                            fence,
                        ))
                        .with_observed_self_roots(observed_self_roots.clone());
                    }
                }
                // Fall through to the deferred-shell path below.
            }
        }

        // Single-infer conditional `T extends infer X ? X : ...`.
        // When `extends` is a bare [`SemanticNodeData::Infer`], bind
        // the infer name to `check` on the true branch and emit an
        // `InferBind` origin edge carrying the binding name. The
        // true-branch substitution picks the Infer up symmetrically
        // with TypeParam via `substitute_semantic_type_param`.
        //
        // Multi-infer (`T extends [infer A, infer B] ? ...`) and
        // template-literal-infer patterns stay deferred; they require
        // the full relation-engine bindings integration still pending
        // per TODO below.
        if let Some(SemanticNodeData::Infer { name }) = graph.node_data(extends).as_deref() {
            let infer_name: Arc<str> = Arc::clone(name);
            // Intern the Infer node so it has a stable
            // `SemanticNodeId`, then substitute by node id. (The
            // `infer X` binding occupies a separate name-slot
            // mechanism from regular type parameters; the substitute
            // helper's Infer arm matches by display_name to bridge
            // that boundary.)
            let infer_node = graph.intern_node(SemanticNodeData::Infer {
                name: Arc::clone(&infer_name),
            });
            let result = self.substitute_semantic_type_param(true_branch, infer_node, check);
            graph.record_origin_edge(
                result,
                OriginEdgeKind::InferBind,
                Arc::from(vec![check, extends].into_boxed_slice()),
                OriginMeta::SubstitutedParam(infer_name),
                Arc::clone(&fence),
            );
            graph.record_conditional_decided();
            graph.record_branch_selection_true();
            return crate::project_semantic_dispatch::walk::QueryBuildOutput::from((
                QueryResult::Value(result),
                fence,
            ))
            .with_observed_self_roots(observed_self_roots.clone());
        }

        // Nested-infer in Function types. TypeScript patterns of the
        // form
        // `T extends (x: infer U, y: infer V) => infer R ? true_branch : false_branch`
        // bind each `infer` binding to the corresponding position in
        // `check`'s signature, then substitute those bindings into the
        // true branch. The relation engine's Function arm
        // short-circuits to `Unknown` in the presence of Infer
        // positions (it does not currently emit infer bindings — see
        // the TODO on the Assignable branch below), so the bindings
        // are extracted directly from the lowered shapes here and the
        // true-branch substitution surfaces the concrete result.
        //
        // `check` is first materialised via `evaluate_deferred_semantic_node`
        // so `PricingPlanSlots["badge"]` / mapped-type references resolve
        // to their underlying Function before position-wise binding.
        if let Some(SemanticNodeData::Function {
            params: extends_params,
            return_type: extends_return,
            ..
        }) = graph.node_data(extends).as_deref()
        {
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
            if has_infer_position {
                let check_resolved = self.evaluate_deferred_semantic_node(check);
                if let Some(SemanticNodeData::Function {
                    params: check_params,
                    return_type: check_return,
                    ..
                }) = graph.node_data(check_resolved).as_deref()
                {
                    let check_params = Arc::clone(check_params);
                    let check_return = *check_return;
                    let mut result = true_branch;
                    let mut any_bound = false;
                    for (e_param, c_param) in extends_params.iter().zip(check_params.iter()) {
                        if let Some(SemanticNodeData::Infer { name }) =
                            graph.node_data(e_param.ty).as_deref()
                        {
                            let infer_name: Arc<str> = Arc::clone(name);
                            let infer_node = graph.intern_node(SemanticNodeData::Infer {
                                name: Arc::clone(&infer_name),
                            });
                            result =
                                self.substitute_semantic_type_param(result, infer_node, c_param.ty);
                            graph.record_origin_edge(
                                result,
                                OriginEdgeKind::InferBind,
                                Arc::from(vec![check, extends].into_boxed_slice()),
                                OriginMeta::SubstitutedParam(infer_name),
                                Arc::clone(&fence),
                            );
                            any_bound = true;
                        }
                    }
                    if let Some(SemanticNodeData::Infer { name }) =
                        graph.node_data(extends_return).as_deref()
                    {
                        let infer_name: Arc<str> = Arc::clone(name);
                        let infer_node = graph.intern_node(SemanticNodeData::Infer {
                            name: Arc::clone(&infer_name),
                        });
                        result =
                            self.substitute_semantic_type_param(result, infer_node, check_return);
                        graph.record_origin_edge(
                            result,
                            OriginEdgeKind::InferBind,
                            Arc::from(vec![check, extends].into_boxed_slice()),
                            OriginMeta::SubstitutedParam(infer_name),
                            Arc::clone(&fence),
                        );
                        any_bound = true;
                    }
                    if any_bound {
                        graph.record_conditional_decided();
                        graph.record_branch_selection_true();
                        return crate::project_semantic_dispatch::walk::QueryBuildOutput::from((
                            QueryResult::Value(result),
                            fence,
                        ))
                        .with_observed_self_roots(observed_self_roots.clone());
                    }
                }
            }
        }

        // Phase D §3 Change S: conditionals route through the shared
        // relation authority. The shallow path handles the trivial
        // primitive/identity cases inline for hot-path efficiency; any
        // non-trivial pair falls through to the full relation engine
        // for memoised assignability. `relate_nodes` internally guards
        // cyclic re-entry and caps structural descent at
        // `RELATION_MAX_DEPTH` as a stack-safety rail.
        let relation = self.shallow_relation_check(check, extends);
        let (result, branch, is_deferred) = match relation {
            ShallowRelation::Assignable => (true_branch, BranchSelection::True, false),
            ShallowRelation::NotAssignable => (false_branch, BranchSelection::False, false),
            ShallowRelation::Unknown => {
                // Full relation authority. `bindings` from a successful
                // Assignable result become infer bindings on the true
                // branch; `relate_nodes` memoises all three outcomes
                // with dep-signature fencing.
                match self.relate_nodes(check, extends).0 {
                    crate::semantic_query::RelationResult::Assignable { bindings: _ } => {
                        // TODO: substitute infer
                        // bindings into `true_branch` via
                        // `substitute_semantic_type_param` and emit
                        // `InferBind` origin edges for non-empty
                        // bindings. Infer-bearing conditionals lower
                        // to the deferred shell today; see §6.3 test
                        // `relate_result_assignable_carries_infer_bindings_into_conditional`.
                        (true_branch, BranchSelection::True, false)
                    }
                    crate::semantic_query::RelationResult::NotAssignable => {
                        (false_branch, BranchSelection::False, false)
                    }
                    crate::semantic_query::RelationResult::Unknown => {
                        let node = graph.intern_node(SemanticNodeData::Conditional {
                            check,
                            extends,
                            true_branch_ref: true_branch,
                            false_branch_ref: false_branch,
                            distributive,
                        });
                        (node, BranchSelection::Deferred, true)
                    }
                }
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

    /// Vue macro resolution lookup.
    ///
    /// Hot-path reads go through
    /// [`SemanticGraphStore::get_resolved_named_type`](crate::semantic_query_memo::SemanticGraphStore::get_resolved_named_type)
    /// directly from the parser's
    /// [`NamedTypeCache`](verter_compiler::utils::oxc::vue::resolve_type::cache_keys::NamedTypeCache)
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
    /// re-walking the AST: the content-pinned `IndexedReady` lookup is
    /// tried first, and on a cache miss the artifact is rematerialized
    /// via `ensure_indexed_ready` only when `owner.whole_hash` is still
    /// the owner's current content (see the arm body for the stale-key
    /// guard).
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
    /// All arms record `(owner.canonical_id, WholeHash(owner.whole_hash))`
    /// in the local fence so warm-hit revalidation against the live
    /// `StoreView` observes the macro's owning file generation.
    ///
    /// **Recursion safety:** A self-reference like
    /// `type R = { next: R }; defineEmits<{ recurse: [R] }>()` reaches
    /// the same `Instantiate` key during projection of `R` and the
    /// dispatch sentinel emits `Opaque(QueryError::RecursiveRef)` —
    /// the projection path observes a `Recursive` `QueryResult`,
    /// which propagates here as `QueryResult::Recursive(node)`.
    pub(super) fn build_resolve_macro_payload(
        &self,
        owner: &crate::semantic_query::DeclKey,
        macro_index: usize,
        macro_kind: verter_semantic::analysis::AnalyzedMacroKind,
        type_args: &Arc<[SemanticNodeId]>,
        mode: ProjectionMode,
    ) -> crate::project_semantic_dispatch::walk::QueryBuildOutput {
        use verter_semantic::analysis::AnalyzedMacroKind;

        // R6 / R20: the key is content-free. Re-source the owning
        // SFC's content version from the live indexed view at
        // value-build time so the cached `MemoEntry` self-roots on
        // the current generation's whole_hash. When the owner has no
        // live `IndexedReady` (e.g. a synthetic test-only owner whose
        // canonical was never upserted), fall back to a sentinel hash
        // `0` for the simple arms (DefineProps / WithDefaults 0-1
        // args, DefineExpose, DefineOptions) which read no
        // owner-side macro data — those arms operate purely on
        // `type_args`. Snapshot-consuming arms (DefineEmits /
        // DefineSlots / DefineModel) re-check below and refuse
        // admission with `cache_suppress = true` when the snapshot
        // is missing.
        let owner_indexed = self.ctx.ensure_indexed_ready(owner.canonical_id.as_ref());
        let owner_whole_hash: crate::semantic_query::HashValue = owner_indexed
            .as_ref()
            .map(|indexed| indexed.whole_hash)
            .unwrap_or_default();

        // Seed local fence with the macro's owning canonical and the
        // re-sourced content version. This is also the memo entry's
        // self-root: a content edit to the owning SFC shifts the
        // whole-hash, the strict warm-read validator rejects the
        // stored signature, and the cold build re-runs.
        let mut local_fence: Vec<(Arc<str>, crate::semantic_query::DepVersion)> = Vec::new();
        local_fence.push((
            Arc::clone(&owner.canonical_id),
            crate::semantic_query::DepVersion::WholeHash(owner_whole_hash),
        ));
        // Also pin the project-generation so the fence catches
        // workspace-wide changes that could invalidate the lowering
        // basis (mirrors `dep_signature_for` semantics).
        let project_gen = self.ctx.project_type_store().project_generation();
        local_fence.push((
            Arc::clone(&owner.canonical_id),
            crate::semantic_query::DepVersion::ProjectGeneration(project_gen),
        ));

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
                    read.value
                }
            }
            AnalyzedMacroKind::DefineEmits
            | AnalyzedMacroKind::DefineSlots
            | AnalyzedMacroKind::DefineModel => {
                // Read the macro snapshot directly from the live
                // indexed view obtained at function entry.
                // Source-and-consistency: `owner_indexed` came from
                // `ensure_indexed_ready` (the live view), and its
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
        let mut observed_self_roots = vec![(Arc::clone(&owner.canonical_id), owner_whole_hash)];
        for arg_root in self.observed_self_roots_from_nodes(type_args.iter().copied()) {
            if !observed_self_roots
                .iter()
                .any(|(c, h)| *c == arg_root.0 && *h == arg_root.1)
            {
                observed_self_roots.push(arg_root);
            }
        }
        crate::project_semantic_dispatch::walk::QueryBuildOutput::from((
            result,
            fence_to_dep_signature(local_fence),
        ))
        .with_observed_self_roots(observed_self_roots)
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
/// **Codex-2 r3 fix.** The lookup forces `mode: Navigate` regardless of
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
            path: prefix_path,
            context: crate::semantic_query::ProjectionReductionContext::published(
                ProjectionMode::Navigate,
            ),
        };
        out.push(crate::project_semantic_dispatch::walk::PrefixBackfill {
            key: prefix_key,
            node,
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
