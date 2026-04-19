//! Project-global [`SemanticQueryApi`] dispatcher (Phase 2.2).
//!
//! Binds [`SemanticQueryKey`] variants onto the shared
//! [`SemanticGraphStore`](crate::semantic_query_memo::SemanticGraphStore) memo
//! and routes them to the existing resolver/solver entry points. This is the
//! single dispatch site every reusable type-resolution operation flows
//! through, regardless of which higher-level request initiated it.
//!
//! ## Scope of this landing
//!
//! Every [`SemanticQueryKey`] variant dispatches through this module:
//!
//! - `ResolveDecl` — returns a structural node sourced from the shallow state
//!   (an `Object` surface when the declaration carries member signatures,
//!   otherwise a `Primitive(Never)` anchor that memoizes the declaration's
//!   scope/name identity).
//! - `Instantiate` — identity-preserving alias anchored to the base node.
//!   Memoizing the key is the dedup guarantee; the richer instantiation
//!   shape is produced on demand by the solver once the caller walks into
//!   it. Mode-free per the lazy-materialisation rule (plan §7.14).
//! - `ProjectPath` — path-precise projection rooted at `base` walking each
//!   [`PathSegment`]. The empty-path form is the canonical shape of
//!   "expand the whole surface" and supersedes the retired `Expand`
//!   variant. `ProjectMember { base, member, mode }` and
//!   `IndexedAccess { base, index, mode }` admission-canonicalise to the
//!   length-1 `ProjectPath` form **before** memo hashing so sugar and
//!   canonical share one warm entry and one in-flight wait graph (plan
//!   B1a).
//! - `NormalizeUnion` / `NormalizeIntersection` — structural dedup over the
//!   supplied members with stable ordering.
//! - `KeyOf` / `MappedType` / `Conditional` — navigation operations that
//!   walk the base node's shared-graph payload. Paths that do not reach a
//!   concrete node fall through to a recorded `Opaque(Miss)` entry — this
//!   is distinct from a dispatch miss because the warm entry observes the
//!   base node identity and therefore dedups repeated asks.
//! - `TypeOf` — mirrors `ResolveDecl` but routes through the shallow
//!   value-symbol space.
//!
//! Every variant observes a dep-signature fragment (at minimum the project
//! generation; additional file hashes flow in from the base nodes' origin
//! scopes where the dispatcher can observe them). Dep-signature propagation
//! is the substrate [`CompletionFence`](crate::completion_fence::CompletionFence)
//! relies on for transitive final-result validation.
//!
//! ## Design rules
//!
//! - Navigators stay non-owning: new semantic nodes must enter through
//!   [`SemanticQueryApi::execute`], not through ad-hoc helpers on the
//!   dispatcher.
//! - Errors, partial results, and recursion sentinels never promote to warm
//!   memo entries — the underlying [`SemanticGraphStore`] enforces this
//!   invariant at publish time.
//! - Dep-signatures returned from warm hits must merge into the caller's
//!   active [`CompletionFence`](crate::completion_fence::CompletionFence)
//!   so final-result validation stays transitive.

use std::sync::Arc;

use verter_semantic::analysis::type_solver::host::{
    BareRefOrigin, ResolvedRootIdentity, TypeSolverHost, UtilitySource,
};
use verter_semantic::analysis::type_solver::PreparedTypeDecl;

use rustc_hash::FxHashMap;
use verter_semantic::analysis::type_expr::{LiteralValue, ObjectMember, PrimitiveName, TypeExpr};

use crate::resolver_core::solver_host::SessionSolverHost;
use crate::semantic_query::{
    BranchSelection, CacheRead, DepSignature, DepVersion, HostResolvedNamedTypeKey, IndexKey,
    IndexSignature, NodeScopeId, OriginEdgeKind, OriginMeta, PathSegment, PrimitiveKind,
    QueryError, QueryResult, ResolveDeclKey, ScopeId, SemanticNodeData, SemanticNodeId,
    SemanticQueryApi, SemanticQueryKey, SurfaceMember, SurfaceView, ValueRootKey,
};
use crate::semantic_query_memo::SemanticGraphStore;
use crate::VerterHost;

/// Host-bound dispatcher for [`SemanticQueryApi`].
///
/// The dispatcher borrows the host for the duration of a query — every
/// `execute()` call threads through the host's
/// [`ProjectTypeStore`](crate::project_type_store::ProjectTypeStore) and its
/// owned [`SemanticGraphStore`].
pub struct ProjectSemanticDispatch<'a> {
    host: &'a VerterHost,
}

impl<'a> ProjectSemanticDispatch<'a> {
    /// Create a dispatcher bound to `host`.
    #[must_use]
    pub fn new(host: &'a VerterHost) -> Self {
        Self { host }
    }

    fn graph(&self) -> &Arc<SemanticGraphStore> {
        self.host.project_type_store().semantic_graph()
    }

    fn recursion_sentinel(&self) -> SemanticNodeId {
        self.graph()
            .intern_node(SemanticNodeData::Opaque(QueryError::Miss))
    }

    /// Intern an opaque node carrying the supplied query error. Used as the
    /// fallback when a semantic subquery cannot be satisfied but the caller
    /// wants a node id rather than a top-level error.
    fn opaque(&self, err: QueryError) -> SemanticNodeId {
        self.graph().intern_node(SemanticNodeData::Opaque(err))
    }

    /// Build the dep-signature fragment for a canonical file at a given
    /// content hash. Carries both the file-version fact and the current
    /// project generation so the completion fence picks up both.
    fn dep_signature_for(&self, canonical_id: &Arc<str>, hash: [u8; 16]) -> DepSignature {
        let project_gen = self.host.project_type_store().project_generation();
        Arc::from(
            vec![
                (canonical_id.clone(), DepVersion::WholeHash(hash)),
                (
                    canonical_id.clone(),
                    DepVersion::ProjectGeneration(project_gen),
                ),
            ]
            .into_boxed_slice(),
        )
    }

    /// Build a dep-signature fragment that records only the project
    /// generation. Used by derived semantic operations (e.g. `Instantiate`,
    /// `NormalizeUnion`) where no single canonical scope owns the result —
    /// dep signatures flow in through the warm memo hits of the bases the
    /// caller already supplied.
    fn project_generation_signature(&self) -> DepSignature {
        let project_gen = self.host.project_type_store().project_generation();
        Arc::from(
            vec![(
                Arc::<str>::from("<project>"),
                DepVersion::ProjectGeneration(project_gen),
            )]
            .into_boxed_slice(),
        )
    }

    /// Resolve a top-level declaration lookup via the host's shallow state.
    ///
    /// Returns a [`SemanticNodeData::DeclAnchor`] carrying the declaration's
    /// `(canonical_id, name, whole_hash)` identity (plan §2 Lazy block + C1).
    /// `build_instantiate` inspects this variant directly to recover the
    /// identity for a `DispatchHost::resolve_prepared_type_decl` lookup — no
    /// side-table or reverse-lookup is needed.
    ///
    /// No origin edges are emitted: `DeclAnchor` is a terminal identity, not
    /// a derived result. `AliasResolve` for declaration-level alias unwraps
    /// lands in C5; path-walk `AliasResolve` lands in C3.
    fn build_resolve_decl(
        &self,
        key: &ResolveDeclKey,
    ) -> (QueryResult<SemanticNodeId>, DepSignature) {
        let shallow = match self
            .host
            .shallow_file_state(key.scope.canonical_id.as_ref())
        {
            Some(state) => state,
            None => return (QueryResult::Error(QueryError::Miss), empty_signature()),
        };

        let has_type_symbol = shallow.symbol(key.name.as_ref()).is_some();
        let has_value_symbol = shallow.value_symbol(key.name.as_ref()).is_some();
        let has_export = shallow.exports.contains_key(key.name.as_ref());
        let has_import_local = shallow.import_targets.contains_key(key.name.as_ref());

        if !(has_type_symbol || has_value_symbol || has_export || has_import_local) {
            return (QueryResult::Error(QueryError::Miss), empty_signature());
        }

        // Record the declaration's origin scope in the sidecar (plan §7.10
        // + C1) so dispatch builders reached from this anchor can route
        // per-base-scope lookups through the correct `SessionSolverHost`.
        let scope = NodeScopeId::File {
            canonical_id: Arc::clone(&key.scope.canonical_id),
            whole_hash: shallow.whole_hash,
            local_scope: key.scope.local_scope,
        };
        let node_id = self.graph().intern_node_with_scope(
            SemanticNodeData::DeclAnchor {
                canonical_id: Arc::clone(&key.scope.canonical_id),
                name: Arc::clone(&key.name),
                whole_hash: shallow.whole_hash,
            },
            scope,
        );
        let signature = self.dep_signature_for(&key.scope.canonical_id, shallow.whole_hash);
        (QueryResult::Value(node_id), signature)
    }

    /// `typeof`-rooted declaration lookup. Shape mirrors [`Self::build_resolve_decl`]
    /// but routes through the shallow value-symbol space so the result is
    /// keyed by the value binding's identity.
    fn build_typeof(
        &self,
        value_root: &ValueRootKey,
    ) -> (QueryResult<SemanticNodeId>, DepSignature) {
        let shallow = match self
            .host
            .shallow_file_state(value_root.scope.canonical_id.as_ref())
        {
            Some(state) => state,
            None => return (QueryResult::Error(QueryError::Miss), empty_signature()),
        };

        let has_value = shallow.value_symbol(value_root.name.as_ref()).is_some();
        let has_import_local = shallow
            .import_targets
            .contains_key(value_root.name.as_ref());
        let has_type_symbol = shallow.symbol(value_root.name.as_ref()).is_some();

        if !(has_value || has_import_local || has_type_symbol) {
            return (QueryResult::Error(QueryError::Miss), empty_signature());
        }

        // Same scope-recording rule as `build_resolve_decl` — the value
        // binding's origin scope is the owning canonical so dispatch
        // builders downstream can reach the correct declaration file.
        let scope = NodeScopeId::File {
            canonical_id: Arc::clone(&value_root.scope.canonical_id),
            whole_hash: shallow.whole_hash,
            local_scope: value_root.scope.local_scope,
        };
        let node_id = self
            .graph()
            .intern_node_with_scope(SemanticNodeData::Primitive(PrimitiveKind::Never), scope);
        let signature = self.dep_signature_for(&value_root.scope.canonical_id, shallow.whole_hash);
        (QueryResult::Value(node_id), signature)
    }

    /// Generic instantiation (plan §3 C1 + §2 lazy materialisation + §7.14).
    ///
    /// Resolves `base` to a declaration identity by inspecting the
    /// [`SemanticNodeData::DeclAnchor`] variant (unwrapping any `Alias`
    /// chain), fetches the [`PreparedTypeDecl`] via [`DispatchHost`], and
    /// produces **one shell level** of the declaration's structural shape
    /// with `args` bound to the decl's type parameters.
    ///
    /// The result is mode-free: the caller issues a follow-up
    /// [`SemanticQueryKey::ProjectPath`] if a specific mode / depth is
    /// needed (§7.14). Member bodies are not recursively lowered —
    /// nested references emit `DeclAnchor` / `Opaque(Miss)` placeholders
    /// per the lazy-materialisation rule; deeper lowering is driven by
    /// `ProjectPath` sub-queries through the family memo.
    ///
    /// Origin edges emitted:
    /// - One [`OriginEdgeKind::Instantiate`] edge on the result, sourced
    ///   from `[base, args...]`.
    /// - One [`OriginEdgeKind::SubstituteTypeParam`] edge per type-parameter
    ///   reference visited at the shell level, sourced from the bound arg.
    ///
    /// If `base` is not a `DeclAnchor` (and not an `Alias` chain ending in
    /// one), or if the prepared decl cannot be fetched, returns
    /// `Opaque(QueryError::Miss)` — a caller handing a non-anchor base to
    /// `build_instantiate` has violated the dispatch contract.
    fn build_instantiate(
        &self,
        base: SemanticNodeId,
        args: &Arc<[SemanticNodeId]>,
    ) -> (QueryResult<SemanticNodeId>, DepSignature) {
        // 1. Unwrap Alias chains to find the DeclAnchor identity.
        let (decl_canonical, decl_name, decl_whole_hash) = match self.resolve_decl_anchor(base) {
            Some(triple) => triple,
            None => {
                return (
                    QueryResult::Value(self.opaque(QueryError::Miss)),
                    empty_signature(),
                )
            }
        };

        // 2. Resolve prepared type decl via `DispatchHost` — the adapter
        // routes through the sidecar-recorded scope for `base`.
        let adapter = SessionDispatchHost::new(self.host);
        let ri = ResolvedRootIdentity::new(decl_canonical.as_ref(), decl_name.as_ref());
        let prepared = match adapter.resolve_prepared_type_decl(base, &ri) {
            Some(p) => p,
            None => {
                return (
                    QueryResult::Value(self.opaque(QueryError::Miss)),
                    empty_signature(),
                )
            }
        };

        // 3. Bind type parameters to args (positional).
        let mut env: FxHashMap<String, SemanticNodeId> = FxHashMap::default();
        for (param, arg_id) in prepared.type_parameters.iter().zip(args.iter()) {
            env.insert(param.name.clone(), *arg_id);
        }

        // 4. Shallow-lower the body. Collects substitution facts for
        // origin-edge emission.
        let scope = NodeScopeId::File {
            canonical_id: Arc::clone(&decl_canonical),
            whole_hash: decl_whole_hash,
            local_scope: None,
        };
        let mut substitutions: Vec<(Arc<str>, SemanticNodeId)> = Vec::new();
        let result = self.shallow_lower_type_expr(&prepared.body, &env, &scope, &mut substitutions);

        // 5. Emit origin edges.
        self.graph().record_instantiate();
        let fence = self.project_generation_signature();

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
        // substituted occurrence (plan §3 C1 — edges emitted at
        // substitution position; at shell level this aggregates on the
        // result node per plan §2 lazy block).
        for (param_name, arg_id) in substitutions {
            self.graph().record_origin_edge(
                result,
                OriginEdgeKind::SubstituteTypeParam,
                Arc::from(vec![arg_id].into_boxed_slice()),
                OriginMeta::SubstitutedParam(param_name),
                Arc::clone(&fence),
            );
        }

        (QueryResult::Value(result), fence)
    }

    /// Unwrap an `Alias` chain to recover the `DeclAnchor` identity at the
    /// tail. Returns `None` if the chain does not end in a `DeclAnchor`.
    /// Bounded chain walk (16 hops) guards against pathological cycles.
    fn resolve_decl_anchor(
        &self,
        base: SemanticNodeId,
    ) -> Option<(Arc<str>, Arc<str>, crate::semantic_query::HashValue)> {
        let mut current = base;
        let graph = self.graph();
        for _ in 0..16 {
            let data = graph.node_data(current)?;
            match &*data {
                SemanticNodeData::DeclAnchor {
                    canonical_id,
                    name,
                    whole_hash,
                } => {
                    return Some((Arc::clone(canonical_id), Arc::clone(name), *whole_hash));
                }
                SemanticNodeData::Alias(next) => {
                    current = *next;
                }
                _ => return None,
            }
        }
        None
    }

    /// Shallow-lower a [`TypeExpr`] under `env` (type-parameter bindings)
    /// into a [`SemanticNodeId`]. "Shallow" means one structural level:
    /// object members, union/intersection arms, and function / conditional
    /// sub-expressions are interned as references rather than recursively
    /// expanded. Deeper lowering is the caller's responsibility via
    /// [`SemanticQueryKey::ProjectPath`] sub-queries.
    ///
    /// `substitutions` accumulates `(param_name, arg_id)` facts for
    /// `SubstituteTypeParam` origin-edge emission at the shell level.
    fn shallow_lower_type_expr(
        &self,
        expr: &TypeExpr,
        env: &FxHashMap<String, SemanticNodeId>,
        scope: &NodeScopeId,
        substitutions: &mut Vec<(Arc<str>, SemanticNodeId)>,
    ) -> SemanticNodeId {
        let graph = self.graph();
        match expr {
            TypeExpr::Primitive(name) => graph.intern_node_with_scope(
                SemanticNodeData::Primitive(map_primitive_name(*name)),
                scope.clone(),
            ),
            TypeExpr::Literal(value) => graph.intern_node_with_scope(
                SemanticNodeData::Primitive(primitive_for_literal(value)),
                scope.clone(),
            ),
            TypeExpr::TypeParameter(param) => {
                if let Some(arg_id) = env.get(&param.name) {
                    substitutions.push((Arc::from(param.name.as_str()), *arg_id));
                    *arg_id
                } else {
                    // Unbound parameter — opaque placeholder (bound
                    // handling lives in the solver hand-off in D1).
                    self.opaque(QueryError::Miss)
                }
            }
            // `type Foo<T> = { x: T }` — the parser keeps bare `T` as
            // `TypeExpr::Ref { name: "T", type_arguments: [] }` at top-level
            // alias bodies (only function-type parameters are normalised
            // via `normalize_type_parameter_refs`). Check the
            // substitution env first; a named match means this is a
            // parameter reference that should substitute.
            TypeExpr::Ref {
                name,
                type_arguments,
            } if type_arguments.is_empty() && env.contains_key(name.as_ref()) => {
                let arg_id = env.get(name.as_ref()).copied().unwrap();
                substitutions.push((Arc::clone(name), arg_id));
                arg_id
            }
            TypeExpr::Union(arms) => {
                let mut arm_ids: Vec<SemanticNodeId> = Vec::with_capacity(arms.len());
                for arm in arms.iter() {
                    arm_ids.push(self.shallow_lower_type_expr(arm, env, scope, substitutions));
                }
                if arm_ids.is_empty() {
                    graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Never))
                } else if arm_ids.len() == 1 {
                    arm_ids[0]
                } else {
                    graph.intern_node_with_scope(
                        SemanticNodeData::Union(Arc::from(arm_ids.into_boxed_slice())),
                        scope.clone(),
                    )
                }
            }
            TypeExpr::Intersection(arms) => {
                let mut arm_ids: Vec<SemanticNodeId> = Vec::with_capacity(arms.len());
                for arm in arms.iter() {
                    arm_ids.push(self.shallow_lower_type_expr(arm, env, scope, substitutions));
                }
                if arm_ids.is_empty() {
                    graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Never))
                } else if arm_ids.len() == 1 {
                    arm_ids[0]
                } else {
                    graph.intern_node_with_scope(
                        SemanticNodeData::Intersection(Arc::from(arm_ids.into_boxed_slice())),
                        scope.clone(),
                    )
                }
            }
            TypeExpr::Object(obj) => {
                let mut members: Vec<SurfaceMember> = Vec::new();
                let mut call_signatures: Vec<SemanticNodeId> = Vec::new();
                let mut construct_signatures: Vec<SemanticNodeId> = Vec::new();
                let mut index_signatures: Vec<IndexSignature> = Vec::new();
                for member in &obj.properties {
                    match member {
                        ObjectMember::Property(prop) => {
                            let value =
                                self.shallow_lower_type_expr(&prop.ty, env, scope, substitutions);
                            members.push(SurfaceMember {
                                name: Arc::from(prop.name.as_str()),
                                value,
                                optional: prop.optional,
                                readonly: prop.readonly,
                                is_method: false,
                            });
                        }
                        ObjectMember::Method(method) => {
                            // Methods are not fully lowered at shell
                            // level; record the member slot with an
                            // opaque value. Real function-signature
                            // lowering lands when the function-semantic
                            // variant is added post-C1.
                            let value = self.opaque(QueryError::Miss);
                            members.push(SurfaceMember {
                                name: Arc::from(method.name.as_str()),
                                value,
                                optional: method.optional,
                                readonly: false,
                                is_method: true,
                            });
                        }
                        ObjectMember::CallSignature(_) => {
                            call_signatures.push(self.opaque(QueryError::Miss));
                        }
                        ObjectMember::ConstructSignature(_) => {
                            construct_signatures.push(self.opaque(QueryError::Miss));
                        }
                        ObjectMember::IndexSignature(sig) => {
                            let key_type = self.shallow_lower_type_expr(
                                &sig.key_type,
                                env,
                                scope,
                                substitutions,
                            );
                            let value_type = self.shallow_lower_type_expr(
                                &sig.value_type,
                                env,
                                scope,
                                substitutions,
                            );
                            index_signatures.push(IndexSignature {
                                key_type,
                                value_type,
                                readonly: sig.readonly,
                            });
                        }
                    }
                }
                let has_index_signature = !index_signatures.is_empty();
                let view = SurfaceView {
                    members: Arc::from(members.into_boxed_slice()),
                    call_signatures: Arc::from(call_signatures.into_boxed_slice()),
                    construct_signatures: Arc::from(construct_signatures.into_boxed_slice()),
                    index_signatures: Arc::from(index_signatures.into_boxed_slice()),
                    keyspace: None,
                    has_index_signature,
                };
                graph.intern_node_with_scope(SemanticNodeData::Object(view), scope.clone())
            }
            // References, conditionals, mapped types, indexed access,
            // keyof, infer, template literals, and other non-shell-level
            // constructs are deferred to later phases (C2-C7). For C1b
            // we emit an opaque placeholder — the lazy-materialisation
            // rule says member bodies expand on-demand via ProjectPath
            // sub-queries (C3), which the caller drives.
            _ => self.opaque(QueryError::Miss),
        }
    }

    /// Path-precise projection. Walks each [`PathSegment`] from `base`,
    /// hopping through [`SurfaceMember`] / index entries on object surfaces.
    /// An empty path returns `base` directly — that is the canonical form of
    /// "expand the whole surface" (the retired `Expand` variant).
    ///
    /// This implementation is the B1a baseline: it walks structural members
    /// only and returns `Opaque(Miss)` on anything else (intersections,
    /// unions, conditionals, instantiations). The lazy / path-distributing /
    /// origin-emitting upgrade lands in C3 (`build_project_path` real
    /// implementation per plan §3 Phase C).
    fn build_project_path(
        &self,
        base: SemanticNodeId,
        path: &Arc<[PathSegment]>,
    ) -> (QueryResult<SemanticNodeId>, DepSignature) {
        let mut current = base;
        for segment in path.iter() {
            let data = self.graph().node_data(current);
            current = match (data.as_deref(), segment) {
                (Some(SemanticNodeData::Object(surface)), PathSegment::Member(name)) => surface
                    .members
                    .iter()
                    .find(|m| m.name.as_ref() == name.as_ref())
                    .map(|m| m.value)
                    .unwrap_or_else(|| self.opaque(QueryError::Miss)),
                (
                    Some(SemanticNodeData::Object(surface)),
                    PathSegment::Index(IndexKey::String(s)),
                ) => surface
                    .members
                    .iter()
                    .find(|m| m.name.as_ref() == s.as_ref())
                    .map(|m| m.value)
                    .unwrap_or_else(|| self.opaque(QueryError::Miss)),
                (
                    Some(SemanticNodeData::Object(surface)),
                    PathSegment::Index(IndexKey::Number(n)),
                ) => {
                    let needle = n.to_string();
                    surface
                        .members
                        .iter()
                        .find(|m| m.name.as_ref() == needle.as_str())
                        .map(|m| m.value)
                        .unwrap_or_else(|| self.opaque(QueryError::Miss))
                }
                _ => self.opaque(QueryError::Miss),
            };
            if let Some(SemanticNodeData::Opaque(_)) = self.graph().node_data(current).as_deref() {
                break;
            }
        }
        (
            QueryResult::Value(current),
            self.project_generation_signature(),
        )
    }

    /// Member projection. Inspects the base node's shared-graph payload and
    /// navigates into the matching member when the base is a concrete
    /// surface. Every other shape (primitive, opaque, alias-without-body)
    /// resolves to `Opaque(Miss)` — but the memoized key captures the
    /// identity so repeated asks share the same warm entry.
    fn build_project_member(
        &self,
        base: SemanticNodeId,
        member: &Arc<str>,
    ) -> (QueryResult<SemanticNodeId>, DepSignature) {
        let data = self.graph().node_data(base);
        let node = match data.as_deref() {
            Some(SemanticNodeData::Object(surface)) => surface
                .members
                .iter()
                .find(|m| m.name.as_ref() == member.as_ref())
                .map(|m| m.value)
                .unwrap_or_else(|| self.opaque(QueryError::Miss)),
            _ => self.opaque(QueryError::Miss),
        };
        let signature = self.project_generation_signature();
        (QueryResult::Value(node), signature)
    }

    /// Indexed access. For `Number` / `String` keys on an `Object` surface,
    /// looks up the member name directly. For `TypeNode` keys or any
    /// non-object base, returns `Opaque(Miss)` under the memoized identity.
    fn build_indexed_access(
        &self,
        base: SemanticNodeId,
        index: &IndexKey,
    ) -> (QueryResult<SemanticNodeId>, DepSignature) {
        let data = self.graph().node_data(base);
        let node = match (data.as_deref(), index) {
            (Some(SemanticNodeData::Object(surface)), IndexKey::String(s)) => surface
                .members
                .iter()
                .find(|m| m.name.as_ref() == s.as_ref())
                .map(|m| m.value)
                .unwrap_or_else(|| self.opaque(QueryError::Miss)),
            (Some(SemanticNodeData::Object(surface)), IndexKey::Number(n)) => {
                let needle = n.to_string();
                surface
                    .members
                    .iter()
                    .find(|m| m.name.as_ref() == needle.as_str())
                    .map(|m| m.value)
                    .unwrap_or_else(|| self.opaque(QueryError::Miss))
            }
            _ => self.opaque(QueryError::Miss),
        };
        (
            QueryResult::Value(node),
            self.project_generation_signature(),
        )
    }

    /// `keyof` projection. For an `Object` surface, materializes a union of
    /// the member names as `Primitive(String)` anchors — this matches the
    /// TS semantics that `keyof T` yields a union of string literals.
    /// For non-objects, returns `Opaque(Miss)`.
    fn build_key_of(&self, base: SemanticNodeId) -> (QueryResult<SemanticNodeId>, DepSignature) {
        let data = self.graph().node_data(base);
        let node = match data.as_deref() {
            Some(SemanticNodeData::Object(surface)) => {
                let member_ids: Vec<SemanticNodeId> = surface
                    .members
                    .iter()
                    .map(|_member| {
                        self.graph()
                            .intern_node(SemanticNodeData::Primitive(PrimitiveKind::String))
                    })
                    .collect();
                if member_ids.is_empty() {
                    self.graph()
                        .intern_node(SemanticNodeData::Primitive(PrimitiveKind::Never))
                } else if member_ids.len() == 1 {
                    member_ids[0]
                } else {
                    self.graph().intern_node(SemanticNodeData::Union(Arc::from(
                        member_ids.into_boxed_slice(),
                    )))
                }
            }
            _ => self.opaque(QueryError::Miss),
        };
        (
            QueryResult::Value(node),
            self.project_generation_signature(),
        )
    }

    /// Mapped-type rewrite. The shared-graph payload does not yet retain
    /// enough structure to produce a fully rewritten surface — the memoized
    /// key captures the source + mapper identity so repeated asks dedup,
    /// but the concrete body is deferred to later phases that wire the
    /// mapped-type solver. Returns an `Alias(source)` anchor for now.
    fn build_mapped_type(
        &self,
        source: SemanticNodeId,
    ) -> (QueryResult<SemanticNodeId>, DepSignature) {
        let node = self.graph().intern_node(SemanticNodeData::Alias(source));
        (
            QueryResult::Value(node),
            self.project_generation_signature(),
        )
    }

    /// Conditional type (plan §3 C2 + §2 lazy block).
    ///
    /// Evaluates `check extends extends ? true_branch : false_branch`
    /// using the shared relation engine and returns either:
    ///
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
    /// relations stay deferred — the full solver routing lands in D2,
    /// where the solver hands conditionals over to dispatch via
    /// `SemanticQueryApi::execute`. `infer` bindings require the full
    /// relation engine integration and remain symbolic (no `InferBind`
    /// edges) until D2 — the two `infer_*` C2 tests are
    /// `#[ignore = "pending D2"]` per plan §3 C2.
    fn build_conditional(
        &self,
        check: SemanticNodeId,
        extends: SemanticNodeId,
        true_branch: SemanticNodeId,
        false_branch: SemanticNodeId,
        distributive: bool,
    ) -> (QueryResult<SemanticNodeId>, DepSignature) {
        let graph = self.graph();
        let fence = self.project_generation_signature();
        let relation = self.shallow_relation_check(check, extends);
        let (result, branch, is_deferred) = match relation {
            ShallowRelation::Assignable => (true_branch, BranchSelection::True, false),
            ShallowRelation::NotAssignable => (false_branch, BranchSelection::False, false),
            ShallowRelation::Unknown => {
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
        (QueryResult::Value(result), fence)
    }

    /// Minimum-correct relation check over shallow semantic nodes.
    ///
    /// C2 handles the decidable shapes that show up directly at the
    /// dispatch boundary without reaching into the full relation engine:
    ///
    /// - Identity: same `SemanticNodeId` → `Assignable`.
    /// - `never` on the source → `Assignable` (bottom assigns anywhere).
    /// - `any` / `unknown` on the target → `Assignable` (top accepts).
    /// - `any` on the source → `Assignable` (TS-semantics: any flows).
    /// - `never` on the target, non-`never` source → `NotAssignable`.
    /// - Primitive pair equality → `Assignable`; pair inequality →
    ///   `NotAssignable`.
    ///
    /// Every other shape (objects, unions, intersections, aliases, decl
    /// anchors, conditionals themselves, opaque, vue-macro elements)
    /// returns `Unknown` so the dispatch leaves the conditional
    /// deferred. Real relation-engine handling lands in D2.
    fn shallow_relation_check(
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
    fn build_normalize_union(
        &self,
        members: &Arc<[SemanticNodeId]>,
    ) -> (QueryResult<SemanticNodeId>, DepSignature) {
        let node = self.intern_normalized_union_or_intersection(members, /* is_union */ true);
        (
            QueryResult::Value(node),
            self.project_generation_signature(),
        )
    }

    /// Intersection normalization. Structurally sorts + dedups; singleton
    /// folds to the only member; empty folds to `Primitive(Never)`.
    fn build_normalize_intersection(
        &self,
        members: &Arc<[SemanticNodeId]>,
    ) -> (QueryResult<SemanticNodeId>, DepSignature) {
        let node = self.intern_normalized_union_or_intersection(members, /* is_union */ false);
        (
            QueryResult::Value(node),
            self.project_generation_signature(),
        )
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
    /// Carries a dep-signature fragment capturing
    /// `(canonical_id, whole_hash, project_generation)` so
    /// [`HostFenceValidator`](crate::host_manage::HostFenceValidator)
    /// catches stale warm hits if any downstream layer memoizes this
    /// dispatch path.
    fn build_resolved_named_type(
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

    fn intern_normalized_union_or_intersection(
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

fn empty_signature() -> DepSignature {
    Arc::from(Vec::new().into_boxed_slice())
}

/// Result of C2's minimum-correct shallow relation check used by
/// [`build_conditional`]. Real relation-engine results land in D2
/// via the solver hand-off — C2 only decides the obvious cases.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ShallowRelation {
    Assignable,
    NotAssignable,
    Unknown,
}

/// Map a [`PrimitiveName`] from the parser's IR onto the semantic-graph
/// [`PrimitiveKind`]. Kept colocated with `build_instantiate` because no
/// other dispatch builder produces primitive nodes from `TypeExpr`.
fn map_primitive_name(name: PrimitiveName) -> PrimitiveKind {
    match name {
        PrimitiveName::String => PrimitiveKind::String,
        PrimitiveName::Number => PrimitiveKind::Number,
        PrimitiveName::Boolean => PrimitiveKind::Boolean,
        PrimitiveName::Symbol => PrimitiveKind::Symbol,
        PrimitiveName::BigInt => PrimitiveKind::BigInt,
        PrimitiveName::Any => PrimitiveKind::Any,
        PrimitiveName::Unknown => PrimitiveKind::Unknown,
        PrimitiveName::Void => PrimitiveKind::Void,
        PrimitiveName::Never => PrimitiveKind::Never,
        PrimitiveName::Null => PrimitiveKind::Null,
        PrimitiveName::Undefined => PrimitiveKind::Undefined,
        PrimitiveName::Object => PrimitiveKind::Object,
    }
}

/// Approximate a literal type as its underlying primitive. The semantic
/// graph's `PrimitiveKind` does not yet carry literal-typed variants;
/// literal-aware projection lands post-C1 alongside expanded primitive
/// coverage.
fn primitive_for_literal(value: &LiteralValue) -> PrimitiveKind {
    match value {
        LiteralValue::String(_) => PrimitiveKind::String,
        LiteralValue::Number(_) => PrimitiveKind::Number,
        LiteralValue::Boolean(_) => PrimitiveKind::Boolean,
        LiteralValue::BigInt(_) => PrimitiveKind::BigInt,
    }
}

/// Stable-sort + dedup a list of semantic node ids. Used to canonicalize
/// the key surface for union / intersection dispatches so
/// `NormalizeUnion({A,B})` and `NormalizeUnion({B,A})` converge on one
/// warm memo entry.
fn canonicalize_node_list(members: &[SemanticNodeId]) -> Arc<[SemanticNodeId]> {
    let mut sorted: Vec<SemanticNodeId> = members.to_vec();
    sorted.sort_by_key(|id| id.0);
    sorted.dedup();
    Arc::from(sorted.into_boxed_slice())
}

impl<'a> SemanticQueryApi for ProjectSemanticDispatch<'a> {
    fn execute(&self, key: SemanticQueryKey) -> QueryResult<SemanticNodeId> {
        // Admission-time canonicalisation per plan B1a:
        //   - `ProjectMember { base, member, mode }` rewrites to
        //     `ProjectPath { base, path: [Member(member)], mode }` BEFORE the
        //     key is hashed into the memo, so sugar and canonical share one
        //     entry and one in-flight wait graph.
        //   - `IndexedAccess { base, index, mode }` rewrites the same way to
        //     `ProjectPath { base, path: [Index(index)], mode }`.
        //   - `NormalizeUnion` / `NormalizeIntersection` get structural
        //     member-list canonicalisation so `{A, B}` and `{B, A}` converge.
        // Other variants key off [`SemanticNodeId`]s that are already hashed
        // verbatim.
        let key = match key {
            SemanticQueryKey::ProjectMember { base, member, mode } => {
                SemanticQueryKey::ProjectPath {
                    base,
                    path: Arc::from(vec![PathSegment::Member(member)].into_boxed_slice()),
                    mode,
                }
            }
            SemanticQueryKey::IndexedAccess { base, index, mode } => {
                SemanticQueryKey::ProjectPath {
                    base,
                    path: Arc::from(vec![PathSegment::Index(index)].into_boxed_slice()),
                    mode,
                }
            }
            SemanticQueryKey::NormalizeUnion { members } => SemanticQueryKey::NormalizeUnion {
                members: canonicalize_node_list(&members),
            },
            SemanticQueryKey::NormalizeIntersection { members } => {
                SemanticQueryKey::NormalizeIntersection {
                    members: canonicalize_node_list(&members),
                }
            }
            other => other,
        };

        let graph = Arc::clone(self.graph());
        let sentinel = || self.recursion_sentinel();
        let key_for_build = key.clone();
        let build = move || match &key_for_build {
            SemanticQueryKey::ResolveDecl(decl_key) => self.build_resolve_decl(decl_key),
            SemanticQueryKey::TypeOf { value_root } => self.build_typeof(value_root),
            SemanticQueryKey::Instantiate { base, args } => self.build_instantiate(*base, args),
            // ProjectMember / IndexedAccess never reach the build closure once
            // admission canonicalisation rewrites them to ProjectPath above —
            // the arms remain to satisfy exhaustiveness; they retire in C4
            // when the variants themselves are rewritten as thin wrappers.
            SemanticQueryKey::ProjectMember { base, member, .. } => {
                self.build_project_member(*base, member)
            }
            SemanticQueryKey::IndexedAccess { base, index, .. } => {
                self.build_indexed_access(*base, index)
            }
            SemanticQueryKey::ProjectPath { base, path, .. } => {
                self.build_project_path(*base, path)
            }
            SemanticQueryKey::KeyOf { base } => self.build_key_of(*base),
            SemanticQueryKey::MappedType { source, .. } => self.build_mapped_type(*source),
            SemanticQueryKey::Conditional {
                check,
                extends,
                true_branch,
                false_branch,
                distributive,
            } => {
                self.build_conditional(*check, *extends, *true_branch, *false_branch, *distributive)
            }
            SemanticQueryKey::NormalizeUnion { members } => self.build_normalize_union(members),
            SemanticQueryKey::NormalizeIntersection { members } => {
                self.build_normalize_intersection(members)
            }
            SemanticQueryKey::ResolvedNamedType { key } => self.build_resolved_named_type(key),
        };
        let CacheRead { value, .. } = graph.execute_cooperative(key, sentinel, build);
        value
    }
}

// ──────────────────────────────────────────────────────────────────────────
// Utilities exposed for higher-level callers
// ──────────────────────────────────────────────────────────────────────────

/// Convenience: construct a [`ResolveDeclKey`] for a top-level symbol in
/// `canonical_id`. Wrapping the arc-conversion here keeps call sites tidy
/// and avoids having each caller re-invent the scope construction.
#[must_use]
pub fn resolve_decl_key(canonical_id: &str, name: &str) -> ResolveDeclKey {
    ResolveDeclKey {
        scope: ScopeId {
            canonical_id: Arc::from(canonical_id),
            local_scope: None,
        },
        name: Arc::from(name),
    }
}

/// Convenience: fetch the resolved semantic-node payload for a previously
/// executed key. Returns `None` if the memo has not warmed the key yet.
#[must_use]
pub fn node_data_for(host: &VerterHost, node: SemanticNodeId) -> Option<Arc<SemanticNodeData>> {
    host.project_type_store().semantic_graph().node_data(node)
}

// Small helper to let the dispatcher express "this node has no member of
// name N on a concrete surface" without the call-site pattern-matching
// boilerplate leaking into the build_* functions. Not part of the public
// API; `SurfaceView` itself owns the traversal structure.
#[allow(dead_code)]
fn find_member(surface: &SurfaceView, needle: &str) -> Option<SemanticNodeId> {
    surface
        .members
        .iter()
        .find(|m| m.name.as_ref() == needle)
        .map(|m| m.value)
}

// ──────────────────────────────────────────────────────────────────────────
// DispatchHost trait + session-owned adapter (plan §7.9 + §7.10 + C1)
// ──────────────────────────────────────────────────────────────────────────
//
// `DispatchHost` is the scope-free minimum-surface host seam dispatch
// builders use to reach host-owned prepared declarations, root identities,
// utility classification, and bare-reference origin classification. It is
// **not** the solver's `TypeSolverHost` — `SessionSolverHost` stays the
// solver-internal host for `TypeQueryEngine`.
//
// The session-owned adapter [`SessionDispatchHost`] implements the trait
// by consulting [`SemanticGraphStore::node_scope`] for each base node to
// reconstruct the originating scope and construct the correct
// [`SessionSolverHost`] per-base. Dispatch builders take `&dyn DispatchHost`
// rather than `&VerterHost`, so they never know about `SessionSolverHost`
// directly (plan §7.9).
//
// C1 lands the scaffolding; the consuming builders (real `build_instantiate`
// etc.) pull `&dyn DispatchHost` in a later structural increment within the
// C1 commit band. Until then, the adapter is complete but only directly
// tested via the routing assertion.

/// Scope-free, minimum-surface host seam for dispatch builders (plan §7.9 +
/// §7.10 + C1).
///
/// Builders that need to reach host-owned prepared declarations, classify
/// utility names, resolve root identities, or classify bare references take
/// `&dyn DispatchHost`. The adapter internally queries
/// [`SemanticGraphStore::node_scope`](crate::semantic_query_memo::SemanticGraphStore::node_scope)
/// on the `base` id to route each lookup through the correct per-base
/// scope — builders are scope-free and never construct a
/// [`SessionSolverHost`] themselves.
///
/// **Contract:** every method takes the base [`SemanticNodeId`] whose scope
/// informs the lookup. Implementations must return the same answer the
/// scope-specific [`SessionSolverHost`] would for that base's origin scope.
///
/// Implementations: [`SessionDispatchHost`] routes per-base via the
/// node-scope sidecar.
pub trait DispatchHost {
    /// Look up a prepared type declaration by its canonical root identity,
    /// using the scope recorded in `base`'s sidecar to select the correct
    /// per-scope [`SessionSolverHost`].
    fn resolve_prepared_type_decl(
        &self,
        base: SemanticNodeId,
        root_identity: &ResolvedRootIdentity,
    ) -> Option<Arc<PreparedTypeDecl>>;

    /// Resolve a `(canonical_id, symbol_name)` pair into a stable root
    /// declaration identity, following re-exports and barrel hops through
    /// `base`'s scope.
    fn root_identity(
        &self,
        base: SemanticNodeId,
        canonical_id: &str,
        symbol_name: &str,
    ) -> Option<ResolvedRootIdentity>;

    /// Classify whether `name` is a built-in TS utility, user-shadowed, or
    /// unknown in `base`'s scope. Scope matters because a local binding in
    /// scope A can shadow a built-in that is not shadowed in scope B.
    fn utility_source(&self, base: SemanticNodeId, name: &str) -> UtilitySource;

    /// Classify whether `name` resolves locally or through an import in
    /// `base`'s scope. Used by lazy field expansion to keep imported
    /// object-like refs symbolic until a deeper route is requested.
    fn bare_ref_origin(&self, base: SemanticNodeId, name: &str) -> BareRefOrigin;
}

/// Session-owned [`DispatchHost`] implementation (plan §7.10).
///
/// Given a base [`SemanticNodeId`], consults
/// [`SemanticGraphStore::node_scope`] to reconstruct the originating
/// [`NodeScopeId`]. Routes each method through a freshly-constructed
/// [`SessionSolverHost`]:
///
/// - [`NodeScopeId::File { canonical_id, .. }`] → `SessionSolverHost::with_declaration_scope`
/// - [`NodeScopeId::Global`] / exempt / missing → `SessionSolverHost::new` (unscoped)
///
/// The adapter holds only a host reference; scope is resolved fresh per
/// call so the adapter stays `Send + Sync` and cheap to construct.
pub struct SessionDispatchHost<'a> {
    host: &'a VerterHost,
}

impl<'a> SessionDispatchHost<'a> {
    /// Construct an adapter bound to `host`. The adapter does not retain a
    /// base node or a scope payload — it re-resolves per-base scope on
    /// every call via [`Self::base_scope`].
    #[must_use]
    pub fn new(host: &'a VerterHost) -> Self {
        Self { host }
    }

    /// Public accessor for `base`'s recorded origin scope. Returns
    /// [`NodeScopeId::Global`] for exempt or missing nodes so every base
    /// has a well-defined routing decision.
    ///
    /// Exposed so tests can observe routing behaviour without setting up
    /// a full `SessionSolverHost` fixture.
    #[must_use]
    pub fn base_scope(&self, base: SemanticNodeId) -> NodeScopeId {
        self.host
            .project_type_store()
            .semantic_graph()
            .node_scope(base)
            .unwrap_or(NodeScopeId::Global)
    }

    /// Construct a [`SessionSolverHost`] scoped to `base`'s origin scope.
    /// Called internally by every trait method.
    fn solver_host_for_base(&self, base: SemanticNodeId) -> SessionSolverHost<'a> {
        match self.base_scope(base) {
            NodeScopeId::File { canonical_id, .. } => {
                SessionSolverHost::with_declaration_scope(self.host, canonical_id.as_ref())
            }
            NodeScopeId::Global => SessionSolverHost::new(self.host),
        }
    }
}

impl<'a> DispatchHost for SessionDispatchHost<'a> {
    fn resolve_prepared_type_decl(
        &self,
        base: SemanticNodeId,
        root_identity: &ResolvedRootIdentity,
    ) -> Option<Arc<PreparedTypeDecl>> {
        self.solver_host_for_base(base)
            .resolve_prepared_type_decl(root_identity)
    }

    fn root_identity(
        &self,
        base: SemanticNodeId,
        canonical_id: &str,
        symbol_name: &str,
    ) -> Option<ResolvedRootIdentity> {
        self.solver_host_for_base(base)
            .root_identity(canonical_id, symbol_name)
    }

    fn utility_source(&self, base: SemanticNodeId, name: &str) -> UtilitySource {
        self.solver_host_for_base(base).utility_source(name)
    }

    fn bare_ref_origin(&self, base: SemanticNodeId, name: &str) -> BareRefOrigin {
        self.solver_host_for_base(base).bare_ref_origin(name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::semantic_query::{
        IndexSignature, NodeScopeId, OriginEdgeKind, PathSegment, ProjectionMode, ScopeId,
        SemanticNodeData, SurfaceMember, SurfaceView,
    };
    use crate::{CompileErrorPolicy, FileKind, HostConfig, UpsertRequest, VerterHost};

    fn host() -> VerterHost {
        VerterHost::new_standalone(HostConfig {
            dev_mode: false,
            compile_error_policy: CompileErrorPolicy::StrictError,
            ..HostConfig::default()
        })
    }

    fn upsert_ts(host: &VerterHost, id: &str, source: &str) {
        let _ = host
            .upsert(UpsertRequest {
                canonical_id: None,
                input_id: id.to_string(),
                source: Arc::from(source),
                file_kind: FileKind::NonSfc,
                aliases: Vec::new(),
            })
            .unwrap();
    }

    /// `ResolveDecl` for a known top-level type returns a value node. The
    /// memo is keyed by the semantic identity, so a second query for the
    /// same key returns the same [`SemanticNodeId`].
    #[test]
    fn resolve_decl_dedups_across_repeated_queries() {
        let host = host();
        upsert_ts(&host, "/w/types.ts", "export type Foo = { x: number }");
        let dispatch = ProjectSemanticDispatch::new(&host);

        let key = resolve_decl_key("/w/types.ts", "Foo");
        let first = dispatch.execute(SemanticQueryKey::ResolveDecl(key.clone()));
        let second = dispatch.execute(SemanticQueryKey::ResolveDecl(key.clone()));

        let (a, b) = match (first, second) {
            (QueryResult::Value(a), QueryResult::Value(b)) => (a, b),
            other => panic!("expected two values, got {other:?}"),
        };
        assert_eq!(a, b, "repeated queries must dedup onto the same node id");
    }

    /// Missing bindings return a structured miss instead of a warm node.
    #[test]
    fn resolve_decl_misses_for_unknown_name() {
        let host = host();
        upsert_ts(&host, "/w/types.ts", "export type Foo = { x: number }");
        let dispatch = ProjectSemanticDispatch::new(&host);
        let key = resolve_decl_key("/w/types.ts", "Missing");
        match dispatch.execute(SemanticQueryKey::ResolveDecl(key)) {
            QueryResult::Error(QueryError::Miss) => {}
            other => panic!("expected Miss, got {other:?}"),
        }
    }

    /// The shared memo survives across distinct higher-level requests — a
    /// second `VerterHost` call against the same key observes the warm id.
    #[test]
    fn resolve_decl_warm_node_survives_between_execute_calls() {
        let host = host();
        upsert_ts(&host, "/w/a.ts", "export type A = { a: number }");
        let dispatch = ProjectSemanticDispatch::new(&host);
        let key = resolve_decl_key("/w/a.ts", "A");

        let first = dispatch.execute(SemanticQueryKey::ResolveDecl(key.clone()));
        let QueryResult::Value(first_id) = first else {
            panic!("expected value");
        };

        let warm = host
            .project_type_store()
            .semantic_graph()
            .get(&SemanticQueryKey::ResolveDecl(key.clone()))
            .expect("warm memo entry must exist after first query");
        match warm.value {
            QueryResult::Value(id) => assert_eq!(id, first_id),
            other => panic!("expected warm value, got {other:?}"),
        }
    }

    /// Different canonical ids for the same name produce different semantic
    /// node ids — scope-aware identity prevents cross-file aliasing.
    #[test]
    fn resolve_decl_disambiguates_by_scope() {
        let host = host();
        upsert_ts(&host, "/w/a.ts", "export type Foo = { a: number }");
        upsert_ts(&host, "/w/b.ts", "export type Foo = { b: number }");
        let dispatch = ProjectSemanticDispatch::new(&host);
        let a_key = resolve_decl_key("/w/a.ts", "Foo");
        let b_key = resolve_decl_key("/w/b.ts", "Foo");

        let (a_id, b_id) = match (
            dispatch.execute(SemanticQueryKey::ResolveDecl(a_key)),
            dispatch.execute(SemanticQueryKey::ResolveDecl(b_key)),
        ) {
            (QueryResult::Value(a), QueryResult::Value(b)) => (a, b),
            other => panic!("expected two values, got {other:?}"),
        };
        assert_ne!(a_id, b_id);
    }

    /// `ResolveDecl` dep-signatures include the file whole-hash and the
    /// project generation so the completion fence picks up both file-level
    /// and project-level invalidation facts.
    #[test]
    fn resolve_decl_dep_signature_captures_file_hash_and_project_gen() {
        let host = host();
        upsert_ts(&host, "/w/a.ts", "export type A = { a: number }");
        let dispatch = ProjectSemanticDispatch::new(&host);
        let key = resolve_decl_key("/w/a.ts", "A");
        let _ = dispatch.execute(SemanticQueryKey::ResolveDecl(key.clone()));

        let warm = host
            .project_type_store()
            .semantic_graph()
            .get(&SemanticQueryKey::ResolveDecl(key))
            .expect("warm entry must exist");
        let mut has_whole_hash = false;
        let mut has_project_gen = false;
        for (_, dv) in warm.dep_signature.iter() {
            match dv {
                DepVersion::WholeHash(_) => has_whole_hash = true,
                DepVersion::ProjectGeneration(_) => has_project_gen = true,
                DepVersion::RouteGeneration(_) => {}
            }
        }
        assert!(has_whole_hash, "dep signature must carry file whole hash");
        assert!(
            has_project_gen,
            "dep signature must carry project generation"
        );
    }

    /// `ResolveDecl` can also reach import-local symbols — the shallow
    /// state surfaces them through `import_targets`. This ensures the
    /// dispatch covers the common "owner imports a type" path in addition
    /// to top-level declarations.
    #[test]
    fn resolve_decl_recognises_import_local_bindings() {
        let host = host();
        upsert_ts(&host, "/w/types.ts", "export type Foo = { x: number }");
        upsert_ts(
            &host,
            "/w/owner.ts",
            "import type { Foo } from './types'\nexport type Owner = Foo",
        );
        let dispatch = ProjectSemanticDispatch::new(&host);

        // `Foo` is not a top-level declaration in owner.ts — it is only an
        // import-local binding. The dispatch must still return a value.
        let key = resolve_decl_key("/w/owner.ts", "Foo");
        match dispatch.execute(SemanticQueryKey::ResolveDecl(key)) {
            QueryResult::Value(_) => {}
            other => panic!("expected value for import-local binding, got {other:?}"),
        }
    }

    /// `Instantiate(base, args)` dedups: two repeated calls share one warm
    /// entry and one node id. `Instantiate` with different args is a
    /// distinct key and must not alias.
    #[test]
    fn instantiate_dedups_by_args() {
        let host = host();
        let dispatch = ProjectSemanticDispatch::new(&host);
        let store = host.project_type_store();
        let graph = store.semantic_graph();
        let base = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
        let arg = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
        let args_a: Arc<[SemanticNodeId]> = Arc::from(vec![arg].into_boxed_slice());
        let args_b: Arc<[SemanticNodeId]> =
            Arc::from(Vec::<SemanticNodeId>::new().into_boxed_slice());

        let k_a = SemanticQueryKey::Instantiate {
            base,
            args: args_a.clone(),
        };
        let k_b = SemanticQueryKey::Instantiate {
            base,
            args: args_b.clone(),
        };

        let a1 = dispatch.execute(k_a.clone());
        let a2 = dispatch.execute(k_a.clone());
        let b = dispatch.execute(k_b);

        let (ida1, ida2, idb) = match (a1, a2, b) {
            (QueryResult::Value(a), QueryResult::Value(c), QueryResult::Value(d)) => (a, c, d),
            other => panic!("expected three values, got {other:?}"),
        };
        assert_eq!(ida1, ida2, "same args must memoize to one node id");
        assert_ne!(ida1, idb, "distinct args must not alias");
    }

    /// `NormalizeUnion` is structural: `[A, B]` and `[B, A]` normalize to the
    /// same canonical node. Duplicate members dedup; a singleton folds to
    /// the only member.
    #[test]
    fn normalize_union_is_structurally_canonical() {
        let host = host();
        let dispatch = ProjectSemanticDispatch::new(&host);
        let graph = host.project_type_store().semantic_graph();
        let a = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
        let b = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));

        let ab = dispatch.execute(SemanticQueryKey::NormalizeUnion {
            members: Arc::from(vec![a, b].into_boxed_slice()),
        });
        let ba = dispatch.execute(SemanticQueryKey::NormalizeUnion {
            members: Arc::from(vec![b, a].into_boxed_slice()),
        });

        let (id_ab, id_ba) = match (ab, ba) {
            (QueryResult::Value(x), QueryResult::Value(y)) => (x, y),
            other => panic!("expected two values, got {other:?}"),
        };
        assert_eq!(
            id_ab, id_ba,
            "union of {{A, B}} and {{B, A}} must canonicalize"
        );

        // Singleton folds to the only member.
        let single = dispatch.execute(SemanticQueryKey::NormalizeUnion {
            members: Arc::from(vec![a].into_boxed_slice()),
        });
        match single {
            QueryResult::Value(id) => assert_eq!(id, a, "singleton union folds to its member"),
            other => panic!("expected singleton fold, got {other:?}"),
        }
    }

    /// `ProjectMember` on a known surface returns the member's node id; on
    /// a primitive (no members) it returns an opaque sentinel. Both cases
    /// memoize under distinct keys.
    #[test]
    fn project_member_reads_object_surface() {
        let host = host();
        let dispatch = ProjectSemanticDispatch::new(&host);
        let graph = host.project_type_store().semantic_graph();

        let string_id = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
        let surface = SurfaceView {
            members: Arc::from(
                vec![SurfaceMember {
                    name: Arc::from("foo"),
                    value: string_id,
                    optional: false,
                    readonly: false,
                    is_method: false,
                }]
                .into_boxed_slice(),
            ),
            call_signatures: Arc::from(Vec::<SemanticNodeId>::new().into_boxed_slice()),
            construct_signatures: Arc::from(Vec::<SemanticNodeId>::new().into_boxed_slice()),
            index_signatures: Arc::from(Vec::<IndexSignature>::new().into_boxed_slice()),
            keyspace: None,
            has_index_signature: false,
        };
        let obj = graph.intern_node(SemanticNodeData::Object(surface));

        let hit = dispatch.execute(SemanticQueryKey::ProjectMember {
            base: obj,
            member: Arc::from("foo"),
            mode: ProjectionMode::Identity,
        });
        let id = match hit {
            QueryResult::Value(id) => id,
            other => panic!("expected value, got {other:?}"),
        };
        assert_eq!(
            id, string_id,
            "project_member must hand back the surface's member node id"
        );

        let miss = dispatch.execute(SemanticQueryKey::ProjectMember {
            base: obj,
            member: Arc::from("absent"),
            mode: ProjectionMode::Identity,
        });
        let opaque_id = match miss {
            QueryResult::Value(id) => id,
            other => panic!("expected value (opaque node), got {other:?}"),
        };
        // Sanity: the opaque value's node data is Opaque.
        let data = graph.node_data(opaque_id).unwrap();
        assert!(
            matches!(*data, SemanticNodeData::Opaque(_)),
            "absent member resolves to an opaque node"
        );
    }

    /// `KeyOf` on an `Object` surface folds to a union of
    /// `Primitive(String)` anchors (one per member). On a primitive base
    /// it returns an `Opaque` node.
    #[test]
    fn key_of_object_yields_string_union() {
        let host = host();
        let dispatch = ProjectSemanticDispatch::new(&host);
        let graph = host.project_type_store().semantic_graph();
        let string_id = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
        let num_id = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
        let surface = SurfaceView {
            members: Arc::from(
                vec![
                    SurfaceMember {
                        name: Arc::from("a"),
                        value: string_id,
                        optional: false,
                        readonly: false,
                        is_method: false,
                    },
                    SurfaceMember {
                        name: Arc::from("b"),
                        value: num_id,
                        optional: false,
                        readonly: false,
                        is_method: false,
                    },
                ]
                .into_boxed_slice(),
            ),
            call_signatures: Arc::from(Vec::<SemanticNodeId>::new().into_boxed_slice()),
            construct_signatures: Arc::from(Vec::<SemanticNodeId>::new().into_boxed_slice()),
            index_signatures: Arc::from(Vec::<IndexSignature>::new().into_boxed_slice()),
            keyspace: None,
            has_index_signature: false,
        };
        let obj = graph.intern_node(SemanticNodeData::Object(surface));

        let keyof = dispatch.execute(SemanticQueryKey::KeyOf { base: obj });
        let id = match keyof {
            QueryResult::Value(id) => id,
            other => panic!("expected value, got {other:?}"),
        };
        let data = graph.node_data(id).unwrap();
        match &*data {
            SemanticNodeData::Union(members) => assert_eq!(members.len(), 2),
            other => panic!("keyof must be a union, got {other:?}"),
        }
    }

    /// B1a: `ProjectMember { base, member, mode }` and the equivalent
    /// `ProjectPath { base, path: [Member(member)], mode }` admission-rewrite
    /// to the same canonical key, so two repeated calls — sugar then
    /// canonical — share one warm memo entry.
    #[test]
    fn project_path_of_length_one_dedups_with_project_member_at_memo() {
        let host = host();
        let dispatch = ProjectSemanticDispatch::new(&host);
        let graph = host.project_type_store().semantic_graph();
        let string_id = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
        let surface = SurfaceView {
            members: Arc::from(
                vec![SurfaceMember {
                    name: Arc::from("foo"),
                    value: string_id,
                    optional: false,
                    readonly: false,
                    is_method: false,
                }]
                .into_boxed_slice(),
            ),
            call_signatures: Arc::from(Vec::<SemanticNodeId>::new().into_boxed_slice()),
            construct_signatures: Arc::from(Vec::<SemanticNodeId>::new().into_boxed_slice()),
            index_signatures: Arc::from(Vec::<IndexSignature>::new().into_boxed_slice()),
            keyspace: None,
            has_index_signature: false,
        };
        let obj = graph.intern_node(SemanticNodeData::Object(surface));

        let via_sugar = dispatch.execute(SemanticQueryKey::ProjectMember {
            base: obj,
            member: Arc::from("foo"),
            mode: ProjectionMode::Identity,
        });
        let via_canonical = dispatch.execute(SemanticQueryKey::ProjectPath {
            base: obj,
            path: Arc::from(vec![PathSegment::Member(Arc::from("foo"))].into_boxed_slice()),
            mode: ProjectionMode::Identity,
        });
        let (sugar_id, canonical_id) = match (via_sugar, via_canonical) {
            (QueryResult::Value(a), QueryResult::Value(b)) => (a, b),
            other => panic!("expected two values, got {other:?}"),
        };
        assert_eq!(sugar_id, canonical_id, "sugar must dedup to canonical");
        assert_eq!(sugar_id, string_id);

        // The warm memo entry is the canonical ProjectPath form, not the
        // sugar variant — admission canonicalisation rewrote both before
        // hashing.
        let canonical_key = SemanticQueryKey::ProjectPath {
            base: obj,
            path: Arc::from(vec![PathSegment::Member(Arc::from("foo"))].into_boxed_slice()),
            mode: ProjectionMode::Identity,
        };
        let warm = graph
            .get(&canonical_key)
            .expect("canonical ProjectPath must be warm");
        match &warm.value {
            QueryResult::Value(id) => assert_eq!(*id, sugar_id),
            other => panic!("warm entry value mismatch: {other:?}"),
        }

        // The sugar key admission-canonicalises to the same entry — there is
        // no separate `ProjectMember` warm entry.
        let sugar_key = SemanticQueryKey::ProjectMember {
            base: obj,
            member: Arc::from("foo"),
            mode: ProjectionMode::Identity,
        };
        assert!(
            graph.get(&sugar_key).is_none(),
            "raw ProjectMember key should not appear in the memo — admission rewrite folds it into ProjectPath"
        );
    }

    /// B1a: `IndexedAccess { base, index, mode }` admission-canonicalises to
    /// `ProjectPath { base, path: [Index(index)], mode }` BEFORE hashing.
    #[test]
    fn indexed_access_canonicalises_to_project_path_before_admission() {
        let host = host();
        let dispatch = ProjectSemanticDispatch::new(&host);
        let graph = host.project_type_store().semantic_graph();
        let num_id = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
        let surface = SurfaceView {
            members: Arc::from(
                vec![SurfaceMember {
                    name: Arc::from("k"),
                    value: num_id,
                    optional: false,
                    readonly: false,
                    is_method: false,
                }]
                .into_boxed_slice(),
            ),
            call_signatures: Arc::from(Vec::<SemanticNodeId>::new().into_boxed_slice()),
            construct_signatures: Arc::from(Vec::<SemanticNodeId>::new().into_boxed_slice()),
            index_signatures: Arc::from(Vec::<IndexSignature>::new().into_boxed_slice()),
            keyspace: None,
            has_index_signature: false,
        };
        let obj = graph.intern_node(SemanticNodeData::Object(surface));

        let via_sugar = dispatch.execute(SemanticQueryKey::IndexedAccess {
            base: obj,
            index: IndexKey::String(Arc::from("k")),
            mode: ProjectionMode::Identity,
        });
        let via_canonical = dispatch.execute(SemanticQueryKey::ProjectPath {
            base: obj,
            path: Arc::from(
                vec![PathSegment::Index(IndexKey::String(Arc::from("k")))].into_boxed_slice(),
            ),
            mode: ProjectionMode::Identity,
        });
        let (sugar_id, canonical_id) = match (via_sugar, via_canonical) {
            (QueryResult::Value(a), QueryResult::Value(b)) => (a, b),
            other => panic!("expected two values, got {other:?}"),
        };
        assert_eq!(sugar_id, canonical_id);
        assert_eq!(sugar_id, num_id);

        let raw_sugar_key = SemanticQueryKey::IndexedAccess {
            base: obj,
            index: IndexKey::String(Arc::from("k")),
            mode: ProjectionMode::Identity,
        };
        assert!(
            graph.get(&raw_sugar_key).is_none(),
            "raw IndexedAccess key must not appear in the memo — admission rewrite folds it into ProjectPath"
        );
    }

    /// B1a: `SurfaceView::members` carries the full TypeScript member
    /// metadata via [`SurfaceMember`]. The struct's `optional`, `readonly`,
    /// and `is_method` fields round-trip through interning unchanged so
    /// downstream consumers (component-meta, LSP hover) can read them
    /// without touching the deprecated `ProjectedMember` types.
    #[test]
    fn surface_view_carries_surface_member_optional_readonly_is_method() {
        let host = host();
        let graph = host.project_type_store().semantic_graph();
        let string_id = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));

        let surface = SurfaceView {
            members: Arc::from(
                vec![
                    SurfaceMember {
                        name: Arc::from("optional_readonly_method"),
                        value: string_id,
                        optional: true,
                        readonly: true,
                        is_method: true,
                    },
                    SurfaceMember {
                        name: Arc::from("plain"),
                        value: string_id,
                        optional: false,
                        readonly: false,
                        is_method: false,
                    },
                ]
                .into_boxed_slice(),
            ),
            call_signatures: Arc::from(Vec::<SemanticNodeId>::new().into_boxed_slice()),
            construct_signatures: Arc::from(Vec::<SemanticNodeId>::new().into_boxed_slice()),
            index_signatures: Arc::from(Vec::<IndexSignature>::new().into_boxed_slice()),
            keyspace: None,
            has_index_signature: false,
        };
        let obj = graph.intern_node(SemanticNodeData::Object(surface));
        let data = graph.node_data(obj).expect("interned");
        match &*data {
            SemanticNodeData::Object(s) => {
                let m0 = &s.members[0];
                assert_eq!(m0.name.as_ref(), "optional_readonly_method");
                assert!(m0.optional, "optional bit must persist");
                assert!(m0.readonly, "readonly bit must persist");
                assert!(m0.is_method, "is_method bit must persist");
                let m1 = &s.members[1];
                assert!(!m1.optional);
                assert!(!m1.readonly);
                assert!(!m1.is_method);
            }
            other => panic!("expected Object, got {other:?}"),
        }
    }

    /// B1a: `SurfaceView` carries `call_signatures` and `construct_signatures`
    /// arrays alongside `members` so callable / newable types' signatures
    /// flow through interning.
    #[test]
    fn surface_view_carries_call_signatures_and_construct_signatures() {
        let host = host();
        let graph = host.project_type_store().semantic_graph();
        let string_id = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
        let num_id = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));

        let surface = SurfaceView {
            members: Arc::from(Vec::<SurfaceMember>::new().into_boxed_slice()),
            call_signatures: Arc::from(vec![string_id].into_boxed_slice()),
            construct_signatures: Arc::from(vec![num_id].into_boxed_slice()),
            index_signatures: Arc::from(Vec::<IndexSignature>::new().into_boxed_slice()),
            keyspace: None,
            has_index_signature: false,
        };
        let obj = graph.intern_node(SemanticNodeData::Object(surface));
        let data = graph.node_data(obj).expect("interned");
        match &*data {
            SemanticNodeData::Object(s) => {
                assert_eq!(s.call_signatures.as_ref(), &[string_id]);
                assert_eq!(s.construct_signatures.as_ref(), &[num_id]);
            }
            other => panic!("expected Object, got {other:?}"),
        }
    }

    /// B1a: `SemanticQueryKey::Expand`, `ExpandMode`, `SemanticQueryApi::expand`,
    /// `build_expand`, and `ExpandMode::` are absent across the workspace's
    /// Rust crate sources and TypeScript packages. The B1a commit retires
    /// these identifiers; this test fails loudly if any survive.
    ///
    /// The terminology script (`tools/check-four-mode-terminology.sh`) also
    /// catches this at CI time, but the in-repo test surfaces the failure
    /// inside `cargo test` on the same change that introduces a regression.
    #[test]
    fn expand_variant_and_expand_mode_absent_from_workspace() {
        use std::path::{Path, PathBuf};
        let workspace_root: PathBuf = Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .find(|p| p.join("Cargo.toml").exists() && p.join("crates").is_dir())
            .expect("workspace root with crates/ dir")
            .to_path_buf();

        // Each needle is followed by a punctuation character so it cannot
        // prefix-match an unrelated identifier like `build_expanded_type_text`
        // or `SemanticQueryKey::Expanded` (a hypothetical future variant
        // outside this track). `ExpandMode` is bare because Rust requires the
        // `ExpandMode::Foo` prefix anywhere it surfaces — there is no
        // identifier whose first characters are `ExpandMode` followed by
        // anything other than `::` in this workspace.
        let needles = [
            "SemanticQueryKey::Expand ",
            "SemanticQueryKey::Expand{",
            "SemanticQueryKey::Expand,",
            "ExpandMode::",
            "SemanticQueryApi::expand(",
            "fn expand(",
            "build_expand(",
            "fn build_expand(",
        ];

        let exclude_files = [
            // The test itself contains the needle strings.
            "project_semantic_dispatch.rs",
            // The plan + the audit feedback file describe the retirement.
            "generic-navigation-prep-plan.md",
            "feedback-2026-04-19-gennav.md",
            "tmp-plan.md",
        ];

        let mut violations: Vec<String> = Vec::new();
        let mut visit = |path: &Path| {
            let lossy = path.to_string_lossy();
            if exclude_files.iter().any(|n| lossy.ends_with(n)) {
                return;
            }
            // build_expanded_type_text / build_expanded_type_expr are
            // unrelated text-construction helpers in
            // verter_semantic::analysis::macros — the script's needles are
            // tightened above (`build_expand(` and `fn build_expand`) to
            // avoid colliding with them.
            let Ok(content) = std::fs::read_to_string(path) else {
                return;
            };
            for needle in &needles {
                if content.contains(needle) {
                    violations.push(format!("{}: contains `{}`", path.display(), needle));
                }
            }
        };

        fn walk(dir: &std::path::Path, exts: &[&str], visit: &mut dyn FnMut(&std::path::Path)) {
            let Ok(entries) = std::fs::read_dir(dir) else {
                return;
            };
            for entry in entries.flatten() {
                let p = entry.path();
                let name = entry.file_name();
                if p.is_dir() {
                    if matches!(
                        name.to_string_lossy().as_ref(),
                        "target" | "node_modules" | ".git" | "dist" | "build" | "out"
                    ) {
                        continue;
                    }
                    walk(&p, exts, visit);
                } else if exts.iter().any(|e| p.extension().is_some_and(|x| x == *e)) {
                    visit(&p);
                }
            }
        }

        walk(&workspace_root.join("crates"), &["rs"], &mut visit);
        walk(
            &workspace_root.join("packages"),
            &["ts", "tsx", "js", "mjs", "cjs"],
            &mut visit,
        );
        assert!(
            violations.is_empty(),
            "Found Expand/ExpandMode/build_expand references after B1a retirement:\n{}",
            violations.join("\n")
        );
    }

    /// `TypeOf { value_root }` looks up through the shallow value-symbol
    /// space. A declared value binding returns a value node; a missing
    /// name returns a structured miss.
    #[test]
    fn type_of_resolves_value_binding() {
        let host = host();
        upsert_ts(
            &host,
            "/w/v.ts",
            "export const foo = { x: 1 as const }\nexport type Helper = typeof foo",
        );
        let dispatch = ProjectSemanticDispatch::new(&host);
        let value_key = ValueRootKey {
            scope: ScopeId {
                canonical_id: Arc::from("/w/v.ts"),
                local_scope: None,
            },
            name: Arc::from("foo"),
        };
        let hit = dispatch.execute(SemanticQueryKey::TypeOf {
            value_root: value_key,
        });
        assert!(matches!(hit, QueryResult::Value(_)));

        let miss_key = ValueRootKey {
            scope: ScopeId {
                canonical_id: Arc::from("/w/v.ts"),
                local_scope: None,
            },
            name: Arc::from("notThere"),
        };
        let miss = dispatch.execute(SemanticQueryKey::TypeOf {
            value_root: miss_key,
        });
        assert!(matches!(miss, QueryResult::Error(QueryError::Miss)));
    }

    /// Identical [`SemanticQueryKey::ResolveDecl`] keys share exactly one
    /// warm memo entry — the memo counter does not grow for repeated asks.
    #[test]
    fn repeated_asks_do_not_grow_memo() {
        let host = host();
        upsert_ts(&host, "/w/a.ts", "export type A = { a: number }");
        let dispatch = ProjectSemanticDispatch::new(&host);
        let key = resolve_decl_key("/w/a.ts", "A");
        let before = host
            .project_type_store()
            .semantic_graph()
            .memo_entry_count();
        for _ in 0..5 {
            let _ = dispatch.execute(SemanticQueryKey::ResolveDecl(key.clone()));
        }
        let after = host
            .project_type_store()
            .semantic_graph()
            .memo_entry_count();
        assert_eq!(
            after - before,
            1,
            "five identical asks must produce one warm memo entry"
        );
    }

    /// `ResolvedNamedType` dispatches through `execute` after the adapter
    /// has written the entry: reads come back as `QueryResult::Value` and
    /// carry the file's whole-hash + project generation in the dep
    /// signature. The hot path still goes direct through
    /// `get_resolved_named_type` (refcount-only) — this test exercises
    /// the formal entry point so ad-hoc callers of the shared query API
    /// see the warm entry too.
    #[test]
    fn resolved_named_type_dispatch_returns_value_after_insert() {
        use crate::semantic_query::HostResolvedNamedTypeKey;
        use verter_compiler::utils::oxc::vue::resolve_type::cache_keys::ResolvedNamedTypeCacheKey;
        use verter_compiler::utils::oxc::vue::resolve_type::ResolvedElements;

        let host = host();
        let dispatch = ProjectSemanticDispatch::new(&host);
        let graph = host.project_type_store().semantic_graph();

        let key = HostResolvedNamedTypeKey {
            canonical_id: Arc::from("/w/a.ts"),
            whole_hash: [7u8; 16],
            inner: ResolvedNamedTypeCacheKey {
                name: b"Foo".to_vec().into_boxed_slice(),
                surface: None,
                base_offset: 0,
                companion_cache_key: Arc::from(Vec::<Box<[u8]>>::new().into_boxed_slice()),
                type_param_bindings: Arc::from(Vec::new().into_boxed_slice()),
            },
        };
        let payload = Arc::new(ResolvedElements::default());

        // Miss before insert: formal entry point returns `Error(Miss)`.
        let miss = dispatch.execute(SemanticQueryKey::ResolvedNamedType {
            key: Arc::new(key.clone()),
        });
        assert!(matches!(miss, QueryResult::Error(QueryError::Miss)));

        // Write via the semantic graph (adapter-side path).
        let expected_id = graph.insert_resolved_named_type(key.clone(), Arc::clone(&payload));

        // Hit after insert: the formal entry point hands back the same
        // interned node id.
        let hit = dispatch.execute(SemanticQueryKey::ResolvedNamedType { key: Arc::new(key) });
        match hit {
            QueryResult::Value(id) => assert_eq!(id, expected_id),
            other => panic!("expected value after insert, got {other:?}"),
        }
    }

    /// B1a: two concurrent threads — one calling the `ProjectMember` sugar
    /// form and one calling the canonical `ProjectPath` form for the
    /// equivalent member — admission-rewrite to the same canonical key and
    /// share one in-flight wait graph. Only one cold build runs, both
    /// threads see the same node id, and the warm memo entry lives under
    /// the canonical `ProjectPath` shape.
    #[test]
    fn concurrent_sugar_and_canonical_requests_share_in_flight_entry() {
        let host = host();
        let graph = host.project_type_store().semantic_graph();
        let string_id = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
        let surface = SurfaceView {
            members: Arc::from(
                vec![SurfaceMember {
                    name: Arc::from("foo"),
                    value: string_id,
                    optional: false,
                    readonly: false,
                    is_method: false,
                }]
                .into_boxed_slice(),
            ),
            call_signatures: Arc::from(Vec::<SemanticNodeId>::new().into_boxed_slice()),
            construct_signatures: Arc::from(Vec::<SemanticNodeId>::new().into_boxed_slice()),
            index_signatures: Arc::from(Vec::<IndexSignature>::new().into_boxed_slice()),
            keyspace: None,
            has_index_signature: false,
        };
        let obj = graph.intern_node(SemanticNodeData::Object(surface));

        let (r1, r2) = std::thread::scope(|s| {
            let h = &host;
            let t1 = s.spawn(move || {
                let dispatch = ProjectSemanticDispatch::new(h);
                dispatch.execute(SemanticQueryKey::ProjectMember {
                    base: obj,
                    member: Arc::from("foo"),
                    mode: ProjectionMode::Identity,
                })
            });
            let t2 = s.spawn(move || {
                let dispatch = ProjectSemanticDispatch::new(h);
                dispatch.execute(SemanticQueryKey::ProjectPath {
                    base: obj,
                    path: Arc::from(vec![PathSegment::Member(Arc::from("foo"))].into_boxed_slice()),
                    mode: ProjectionMode::Identity,
                })
            });
            (t1.join().unwrap(), t2.join().unwrap())
        });

        let (id1, id2) = match (r1, r2) {
            (QueryResult::Value(a), QueryResult::Value(b)) => (a, b),
            other => panic!("expected two values, got {other:?}"),
        };
        assert_eq!(id1, id2, "concurrent sugar + canonical must dedup");
        assert_eq!(id1, string_id);

        // The warm memo entry is on the canonical ProjectPath key only —
        // both threads' admission canonicalisations folded onto the same
        // entry.
        let canonical_key = SemanticQueryKey::ProjectPath {
            base: obj,
            path: Arc::from(vec![PathSegment::Member(Arc::from("foo"))].into_boxed_slice()),
            mode: ProjectionMode::Identity,
        };
        let warm = graph
            .get(&canonical_key)
            .expect("canonical ProjectPath warm after concurrent dispatch");
        match &warm.value {
            QueryResult::Value(id) => assert_eq!(*id, id1),
            other => panic!("warm entry value mismatch: {other:?}"),
        }
        let raw_sugar = SemanticQueryKey::ProjectMember {
            base: obj,
            member: Arc::from("foo"),
            mode: ProjectionMode::Identity,
        };
        assert!(
            graph.get(&raw_sugar).is_none(),
            "raw ProjectMember key should not appear in the memo"
        );
    }

    // ──────────────────────────────────────────────────────────────────
    // DispatchHost adapter routing (plan §7.10 + C1)
    // ──────────────────────────────────────────────────────────────────

    /// The session-owned [`SessionDispatchHost`] adapter consults the
    /// [`SemanticGraphStore`] sidecar to route each base node to its
    /// origin scope. Two nodes interned under different scopes route to
    /// different scopes; an exempt node routes to `Global`.
    #[test]
    fn dispatch_host_adapter_routes_per_base_scope() {
        let host = host();
        let graph = Arc::clone(host.project_type_store().semantic_graph());

        let scope_a = NodeScopeId::File {
            canonical_id: Arc::from("/w/scope_a.ts"),
            whole_hash: [1u8; 16],
            local_scope: None,
        };
        let scope_b = NodeScopeId::File {
            canonical_id: Arc::from("/w/scope_b.ts"),
            whole_hash: [2u8; 16],
            local_scope: Some(5),
        };

        let anchor_a = graph.intern_node_with_scope(
            SemanticNodeData::Primitive(PrimitiveKind::Never),
            scope_a.clone(),
        );
        let anchor_b = graph.intern_node_with_scope(
            SemanticNodeData::Primitive(PrimitiveKind::String),
            scope_b.clone(),
        );
        // Global-origin helper intermediate.
        let global_anchor = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));

        let adapter = SessionDispatchHost::new(&host);

        // Per-base routing: each base's scope comes back from the sidecar.
        assert_eq!(adapter.base_scope(anchor_a), scope_a);
        assert_eq!(adapter.base_scope(anchor_b), scope_b);
        // Scope-less base routes to `Global`.
        assert_eq!(adapter.base_scope(global_anchor), NodeScopeId::Global);

        // The adapter reports the origin scope, not the caller's scope —
        // two reads from two different "perspectives" always see the
        // origin. We simulate this by making two calls in sequence; the
        // sidecar is write-once, so the recorded scope stays stable
        // regardless of the caller.
        assert_eq!(adapter.base_scope(anchor_a), scope_a);
        assert_eq!(adapter.base_scope(anchor_a), scope_a);

        // Exempt nodes (VueMacroElements) route to `Global` because
        // the sidecar has no entry for them — the fallback is `Global`
        // so every base has a well-defined routing decision.
        use verter_compiler::utils::oxc::vue::resolve_type::ResolvedElements;
        let vue_id = graph.intern_node(SemanticNodeData::VueMacroElements(Arc::new(
            ResolvedElements::default(),
        )));
        assert_eq!(adapter.base_scope(vue_id), NodeScopeId::Global);

        // Trait methods route through `solver_host_for_base`. Without
        // prepared decls set up, `resolve_prepared_type_decl` returns
        // `None` but the call succeeds for all scopes (no panic, no
        // stale state between calls).
        let ri = ResolvedRootIdentity::new("/w/scope_a.ts", "Missing");
        assert!(adapter.resolve_prepared_type_decl(anchor_a, &ri).is_none());
        assert!(adapter.resolve_prepared_type_decl(anchor_b, &ri).is_none());
        assert!(adapter
            .resolve_prepared_type_decl(global_anchor, &ri)
            .is_none());

        // `utility_source` and `bare_ref_origin` behave per-scope; without
        // user shadowings these return `Builtin` / `Unknown` respectively.
        let _ = adapter.utility_source(anchor_a, "Partial");
        let _ = adapter.bare_ref_origin(anchor_a, "Foo");
    }

    /// `build_resolve_decl` records the declaration's origin scope in
    /// the [`SemanticGraphStore`] sidecar at intern time. Verified
    /// end-to-end through the dispatch API so we exercise the full
    /// integration path (plan §3 C1 + §7.10).
    #[test]
    fn resolve_decl_records_file_scope_in_sidecar() {
        let host = host();
        upsert_ts(&host, "/w/types.ts", "export type Foo = { x: number }");
        let dispatch = ProjectSemanticDispatch::new(&host);
        let graph = Arc::clone(host.project_type_store().semantic_graph());

        let key = resolve_decl_key("/w/types.ts", "Foo");
        let node = match dispatch.execute(SemanticQueryKey::ResolveDecl(key)) {
            QueryResult::Value(id) => id,
            other => panic!("expected Value, got {other:?}"),
        };

        // The anchor carries a File-scoped sidecar entry pointing at the
        // defining canonical. Future builders reach this via
        // `SessionDispatchHost::base_scope(node)`.
        let scope = graph
            .node_scope(node)
            .expect("build_resolve_decl must populate the sidecar");
        match scope {
            NodeScopeId::File {
                canonical_id,
                local_scope,
                ..
            } => {
                assert_eq!(canonical_id.as_ref(), "/w/types.ts");
                assert_eq!(local_scope, None);
            }
            NodeScopeId::Global => panic!("expected File-scoped sidecar, got Global"),
        }

        // Round-trip through the adapter confirms routing for this base.
        let adapter = SessionDispatchHost::new(&host);
        match adapter.base_scope(node) {
            NodeScopeId::File { canonical_id, .. } => {
                assert_eq!(canonical_id.as_ref(), "/w/types.ts");
            }
            NodeScopeId::Global => panic!("adapter routed to Global instead of File scope"),
        }
    }

    // ──────────────────────────────────────────────────────────────────
    // C1-Commit-B — real `build_instantiate` (plan §3 C1 + §2 lazy block)
    // ──────────────────────────────────────────────────────────────────
    //
    // The tests below exercise the shallow + lazy + mode-free
    // `build_instantiate` behaviour. They depend on:
    //  - `SemanticNodeData::DeclAnchor` variant (added in C1-Commit-B).
    //  - `build_resolve_decl` producing `DeclAnchor` instead of
    //    `Primitive(Never)`.
    //  - `build_instantiate` resolving the base via `DispatchHost` and
    //    interning the shell-level object with member refs.
    //  - `Instantiate` + `SubstituteTypeParam` origin edges.

    fn resolve_decl_anchor(
        dispatch: &ProjectSemanticDispatch<'_>,
        canonical_id: &str,
        name: &str,
    ) -> SemanticNodeId {
        match dispatch.execute(SemanticQueryKey::ResolveDecl(resolve_decl_key(
            canonical_id,
            name,
        ))) {
            QueryResult::Value(id) => id,
            other => {
                panic!("expected Value from ResolveDecl({canonical_id}::{name}), got {other:?}")
            }
        }
    }

    /// `build_resolve_decl` now produces a [`SemanticNodeData::DeclAnchor`]
    /// carrying the declaration's `(canonical_id, name, whole_hash)`. This
    /// is the identity-carrier `build_instantiate` inspects.
    #[test]
    fn resolve_decl_produces_decl_anchor_variant() {
        let host = host();
        upsert_ts(&host, "/w/types.ts", "export type Foo<T> = { x: T }");
        let dispatch = ProjectSemanticDispatch::new(&host);
        let graph = Arc::clone(host.project_type_store().semantic_graph());
        let node = resolve_decl_anchor(&dispatch, "/w/types.ts", "Foo");
        let data = graph.node_data(node).expect("node interned");
        match &*data {
            SemanticNodeData::DeclAnchor {
                canonical_id, name, ..
            } => {
                assert_eq!(canonical_id.as_ref(), "/w/types.ts");
                assert_eq!(name.as_ref(), "Foo");
            }
            other => panic!("expected DeclAnchor variant, got {other:?}"),
        }
    }

    /// `Instantiate(base, args)` is mode-free per plan §7.14. Executing
    /// the key produces exactly **one** entry in the family memo,
    /// regardless of how many follow-up `ProjectPath(result, [...], mode)`
    /// queries are issued at different modes.
    #[test]
    fn instantiate_is_mode_free_one_entry_across_depth_requests() {
        let host = host();
        upsert_ts(&host, "/w/types.ts", "export type Foo<T> = { x: T }");
        let dispatch = ProjectSemanticDispatch::new(&host);
        let graph = Arc::clone(host.project_type_store().semantic_graph());

        let base = resolve_decl_anchor(&dispatch, "/w/types.ts", "Foo");
        let string_arg = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
        let args: Arc<[SemanticNodeId]> = Arc::from(vec![string_arg].into_boxed_slice());

        let key = SemanticQueryKey::Instantiate {
            base,
            args: args.clone(),
        };
        let _ = dispatch.execute(key.clone());

        // Follow-up path projections at two different modes.
        let empty_path: Arc<[PathSegment]> =
            Arc::from(Vec::<PathSegment>::new().into_boxed_slice());
        let result = match dispatch.execute(key.clone()) {
            QueryResult::Value(id) => id,
            other => panic!("expected Value, got {other:?}"),
        };
        let _ = dispatch.execute(SemanticQueryKey::ProjectPath {
            base: result,
            path: empty_path.clone(),
            mode: ProjectionMode::Identity,
        });
        let _ = dispatch.execute(SemanticQueryKey::ProjectPath {
            base: result,
            path: empty_path,
            mode: ProjectionMode::Expanded,
        });

        // The `Instantiate` family has exactly one warm entry regardless
        // of the two different-mode ProjectPath queries on the result.
        let warm = graph
            .get(&key)
            .expect("Instantiate entry must be warm after execute");
        match warm.value {
            QueryResult::Value(_) => {}
            other => panic!("expected warm Value, got {other:?}"),
        }
        // A second Instantiate call with the same (base, args) returns
        // the same node id — dedup through the memo (mode-free).
        let again = match dispatch.execute(key) {
            QueryResult::Value(id) => id,
            other => panic!("expected Value, got {other:?}"),
        };
        assert_eq!(result, again, "Instantiate must dedup across calls");
    }

    /// `Instantiate(base, args)` emits one `Instantiate` origin edge
    /// (result ← [decl_anchor, args...]) and one `SubstituteTypeParam`
    /// edge per substituted type-parameter occurrence visited at the
    /// shell level.
    #[test]
    fn instantiate_with_concrete_args_emits_substitute_edges() {
        let host = host();
        upsert_ts(&host, "/w/types.ts", "export type Foo<T> = { x: T }");
        let dispatch = ProjectSemanticDispatch::new(&host);
        let graph = Arc::clone(host.project_type_store().semantic_graph());

        let base = resolve_decl_anchor(&dispatch, "/w/types.ts", "Foo");
        let string_arg = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
        let args: Arc<[SemanticNodeId]> = Arc::from(vec![string_arg].into_boxed_slice());

        let result = match dispatch.execute(SemanticQueryKey::Instantiate {
            base,
            args: args.clone(),
        }) {
            QueryResult::Value(id) => id,
            other => panic!("expected Value, got {other:?}"),
        };

        // `Instantiate` edge on the result.
        let inst_edges = graph.origins_of_kind(result, OriginEdgeKind::Instantiate);
        assert_eq!(
            inst_edges.len(),
            1,
            "expected exactly one Instantiate edge, got {:?}",
            inst_edges
        );
        let edge = &inst_edges[0];
        // sources = [decl_anchor, args...].
        assert!(
            edge.sources.iter().any(|id| *id == base),
            "Instantiate edge sources must include the decl anchor"
        );
        assert!(
            edge.sources.iter().any(|id| *id == string_arg),
            "Instantiate edge sources must include each arg"
        );

        // At least one `SubstituteTypeParam` edge for `T -> string` on the
        // shell (the x: T substitution at member position).
        // Walk all nodes the shell emits — the substitution may be
        // recorded on a member's node or on the result itself.
        let mut found_substitute = false;
        for candidate in [result, string_arg].iter().copied() {
            let subs = graph.origins_of_kind(candidate, OriginEdgeKind::SubstituteTypeParam);
            if !subs.is_empty() {
                found_substitute = true;
                break;
            }
        }
        // Also walk the result's object members (if any) for substitutes.
        if let Some(data) = graph.node_data(result) {
            if let SemanticNodeData::Object(view) = &*data {
                for m in view.members.iter() {
                    let subs = graph.origins_of_kind(m.value, OriginEdgeKind::SubstituteTypeParam);
                    if !subs.is_empty() {
                        found_substitute = true;
                        break;
                    }
                }
            }
        }
        assert!(
            found_substitute,
            "expected at least one SubstituteTypeParam edge on the shell or a member/arg"
        );
    }

    /// The shell's members point at reference nodes, NOT at recursively-
    /// lowered concrete object shapes. A member whose body references
    /// another declaration (e.g. `y: Other<T>`) yields a member `.value`
    /// that is not itself an `Object` surface — it is a reference /
    /// placeholder / sub-instantiation shell.
    #[test]
    fn shallow_instantiate_does_not_materialise_member_bodies() {
        let host = host();
        upsert_ts(
            &host,
            "/w/types.ts",
            "export type Other<T> = { inner: T }\nexport type Foo<T> = { x: T; y: Other<T> }",
        );
        let dispatch = ProjectSemanticDispatch::new(&host);
        let graph = Arc::clone(host.project_type_store().semantic_graph());

        let base = resolve_decl_anchor(&dispatch, "/w/types.ts", "Foo");
        let string_arg = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
        let args: Arc<[SemanticNodeId]> = Arc::from(vec![string_arg].into_boxed_slice());

        let result = match dispatch.execute(SemanticQueryKey::Instantiate { base, args }) {
            QueryResult::Value(id) => id,
            other => panic!("expected Value, got {other:?}"),
        };

        let data = graph.node_data(result).expect("result node");
        let view = match &*data {
            SemanticNodeData::Object(v) => v.clone(),
            other => panic!("expected Object shell after Instantiate, got {other:?}"),
        };
        // Member `y` must not be recursively lowered into an `Object`
        // that already carries `Other<T>`'s `inner` member. It must be a
        // reference: an `Alias`, an `Opaque`, a `DeclAnchor`, another
        // shell-level `Object` for the sub-instantiation, etc. — but
        // crucially not pre-expanded past one level.
        let y_member = view
            .members
            .iter()
            .find(|m| m.name.as_ref() == "y")
            .expect("y member must exist");
        let y_data = graph.node_data(y_member.value).expect("y value node");
        // If it's an Object (the sub-Instantiate shell), it must not
        // carry `inner`'s own members recursively expanded — i.e., its
        // members are themselves refs, not a full-body expansion.
        match &*y_data {
            SemanticNodeData::Object(_sub) => {
                // Sub-instantiation shell is fine as long as it was
                // reached via Instantiate (origin edge present).
                let sub_inst = graph.origins_of_kind(y_member.value, OriginEdgeKind::Instantiate);
                assert!(
                    !sub_inst.is_empty(),
                    "sub-object at `y` must have an Instantiate edge recording how it was produced"
                );
            }
            SemanticNodeData::Alias(_)
            | SemanticNodeData::Opaque(_)
            | SemanticNodeData::DeclAnchor { .. }
            | SemanticNodeData::Primitive(_) => {
                // All acceptable — reference-level member, not a
                // recursive full-body expansion.
            }
            other => panic!("unexpected y member body: {other:?}"),
        }
    }

    /// Two separate `Instantiate(base, [string])` calls dedup to the same
    /// `SemanticNodeId`. `stats.hits` increments on the second call —
    /// evidence that the memo short-circuited the second build.
    #[test]
    fn same_args_different_callers_dedup_to_one_entry() {
        let host = host();
        upsert_ts(&host, "/w/types.ts", "export type Foo<T> = { x: T }");
        let dispatch = ProjectSemanticDispatch::new(&host);
        let graph = Arc::clone(host.project_type_store().semantic_graph());

        let base = resolve_decl_anchor(&dispatch, "/w/types.ts", "Foo");
        let string_arg = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
        let args: Arc<[SemanticNodeId]> = Arc::from(vec![string_arg].into_boxed_slice());

        let stats_before = graph.stats_snapshot();
        let first = match dispatch.execute(SemanticQueryKey::Instantiate {
            base,
            args: args.clone(),
        }) {
            QueryResult::Value(id) => id,
            other => panic!("expected Value, got {other:?}"),
        };
        let second = match dispatch.execute(SemanticQueryKey::Instantiate {
            base,
            args: args.clone(),
        }) {
            QueryResult::Value(id) => id,
            other => panic!("expected Value, got {other:?}"),
        };
        assert_eq!(
            first, second,
            "two calls to Instantiate(base, same-args) must share one node id"
        );
        let stats_after = graph.stats_snapshot();
        assert!(
            stats_after.hits > stats_before.hits,
            "second Instantiate call must hit the warm memo (hits before={}, after={})",
            stats_before.hits,
            stats_after.hits
        );
    }

    /// Whole-surface expansion through `ProjectPath(result, [], Expanded)`
    /// drives deeper lowering via [`SemanticQueryApi::execute`] re-entry
    /// rather than a private walker. The real enforcement (per plan §3 C3)
    /// lands alongside `build_project_path`'s lazy path-projection
    /// rewrite; the test stays ignored until C3 so we do not assert on
    /// behaviour that only becomes visible after the path walker
    /// materialises sub-queries through dispatch.
    #[test]
    #[ignore = "pending C3 (build_project_path lazy path-projection rewrite)"]
    fn expanded_instantiate_materialises_through_dispatcher_not_private_walker() {
        let host = host();
        upsert_ts(
            &host,
            "/w/types.ts",
            "export type Other<T> = { inner: T }\nexport type Foo<T> = { x: T; y: Other<T> }",
        );
        let dispatch = ProjectSemanticDispatch::new(&host);
        let graph = Arc::clone(host.project_type_store().semantic_graph());

        let base = resolve_decl_anchor(&dispatch, "/w/types.ts", "Foo");
        let string_arg = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
        let args: Arc<[SemanticNodeId]> = Arc::from(vec![string_arg].into_boxed_slice());
        let result = match dispatch.execute(SemanticQueryKey::Instantiate { base, args }) {
            QueryResult::Value(id) => id,
            other => panic!("expected Value, got {other:?}"),
        };
        let empty_path: Arc<[PathSegment]> =
            Arc::from(Vec::<PathSegment>::new().into_boxed_slice());
        let _ = dispatch.execute(SemanticQueryKey::ProjectPath {
            base: result,
            path: empty_path,
            mode: ProjectionMode::Expanded,
        });

        // Each deeply-materialised member body must have been reached
        // via an Instantiate edge (dispatch path). Private-walker
        // production would leave the sub-body without such an edge.
        let data = graph.node_data(result).expect("result node");
        if let SemanticNodeData::Object(view) = &*data {
            for m in view.members.iter() {
                if let Some(member_data) = graph.node_data(m.value) {
                    if matches!(&*member_data, SemanticNodeData::Object(_)) {
                        let inst = graph.origins_of_kind(m.value, OriginEdgeKind::Instantiate);
                        assert!(
                            !inst.is_empty(),
                            "Expanded-mode material member `{}` must have an Instantiate origin edge (dispatch path)",
                            m.name
                        );
                    }
                }
            }
        }
    }

    /// Two instantiations that walk the same shared sub-expression at a
    /// common path share one family-memo entry at that sub-query. Full
    /// enforcement requires C3's lazy path-projection rewrite.
    #[test]
    #[ignore = "pending C3 (ProjectPath lazy sub-query reuse across distinct instantiations)"]
    fn distinct_instantiations_share_visited_subpath_lowering_not_full_body() {
        let host = host();
        upsert_ts(
            &host,
            "/w/types.ts",
            "export type Other<T> = { inner: T }\nexport type Foo<T> = { a: Other<T>; b: Other<T> }",
        );
        let dispatch = ProjectSemanticDispatch::new(&host);
        let graph = Arc::clone(host.project_type_store().semantic_graph());

        let base = resolve_decl_anchor(&dispatch, "/w/types.ts", "Foo");
        let string_arg = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
        let number_arg = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
        let args_s: Arc<[SemanticNodeId]> = Arc::from(vec![string_arg].into_boxed_slice());
        let args_n: Arc<[SemanticNodeId]> = Arc::from(vec![number_arg].into_boxed_slice());

        let inst_s = match dispatch.execute(SemanticQueryKey::Instantiate { base, args: args_s }) {
            QueryResult::Value(id) => id,
            other => panic!("expected Value, got {other:?}"),
        };
        let inst_n = match dispatch.execute(SemanticQueryKey::Instantiate { base, args: args_n }) {
            QueryResult::Value(id) => id,
            other => panic!("expected Value, got {other:?}"),
        };

        // Path projections at [a] for each instantiation.
        let a_path: Arc<[PathSegment]> =
            Arc::from(vec![PathSegment::Member(Arc::from("a"))].into_boxed_slice());
        let _ = dispatch.execute(SemanticQueryKey::ProjectPath {
            base: inst_s,
            path: a_path.clone(),
            mode: ProjectionMode::Identity,
        });
        let before = graph.stats_snapshot().memo_entry_count;
        let _ = dispatch.execute(SemanticQueryKey::ProjectPath {
            base: inst_n,
            path: a_path,
            mode: ProjectionMode::Identity,
        });
        let after = graph.stats_snapshot().memo_entry_count;
        // Structurally-identical sub-queries across distinct
        // instantiations should reach the same family entry after C3 —
        // the second execute must not add entries beyond the path
        // itself.
        assert!(
            after <= before + 2,
            "distinct instantiations must share visited-subpath lowering (before={before}, after={after})"
        );
    }

    // ──────────────────────────────────────────────────────────────────
    // C2 — real `build_conditional` (plan §3 C2 + §2 lazy block)
    // ──────────────────────────────────────────────────────────────────

    use crate::semantic_query::BranchSelection;

    fn primitive(
        graph: &Arc<crate::semantic_query_memo::SemanticGraphStore>,
        kind: PrimitiveKind,
    ) -> SemanticNodeId {
        graph.intern_node(SemanticNodeData::Primitive(kind))
    }

    /// A closed/decidable conditional (`string extends string ? A : B`)
    /// returns the selected branch directly and emits a
    /// `ConditionalSelect` edge with `BranchSelection::True`. The losing
    /// branch is NOT materialised beyond its shell reference.
    #[test]
    fn closed_conditional_selects_and_emits_edges() {
        let host = host();
        let dispatch = ProjectSemanticDispatch::new(&host);
        let graph = Arc::clone(host.project_type_store().semantic_graph());

        let string_node = primitive(&graph, PrimitiveKind::String);
        let true_branch = primitive(&graph, PrimitiveKind::Boolean);
        let false_branch = primitive(&graph, PrimitiveKind::Number);

        let result = match dispatch.execute(SemanticQueryKey::Conditional {
            check: string_node,
            extends: string_node,
            true_branch,
            false_branch,
            distributive: false,
        }) {
            QueryResult::Value(id) => id,
            other => panic!("expected Value, got {other:?}"),
        };
        // Closed selection returns the true branch directly (no
        // `Conditional` shell interned).
        assert_eq!(
            result, true_branch,
            "closed conditional must return the true branch node id"
        );
        let edges = graph.origins_of_kind(result, OriginEdgeKind::ConditionalSelect);
        assert!(
            !edges.is_empty(),
            "expected at least one ConditionalSelect edge on the result"
        );
        let has_true = edges.iter().any(|e| {
            matches!(&e.meta, OriginMeta::Branch(BranchSelection::True))
                && e.sources.iter().any(|id| *id == string_node)
        });
        assert!(
            has_true,
            "ConditionalSelect edge must carry Branch::True and source the check/extends"
        );
    }

    /// The losing branch in a closed conditional is not interned as a
    /// sub-object or sub-instantiation. Because we hand back the winning
    /// branch id directly, the only materialised shape is the branch the
    /// test prepared — no extra shell for the loser.
    #[test]
    fn closed_conditional_does_not_materialise_losing_branch_body() {
        let host = host();
        let dispatch = ProjectSemanticDispatch::new(&host);
        let graph = Arc::clone(host.project_type_store().semantic_graph());

        let string_node = primitive(&graph, PrimitiveKind::String);
        let number_node = primitive(&graph, PrimitiveKind::Number);
        let true_branch = primitive(&graph, PrimitiveKind::Boolean);
        let false_branch = primitive(&graph, PrimitiveKind::Symbol);

        let node_count_before = graph.node_count();
        let _ = dispatch.execute(SemanticQueryKey::Conditional {
            check: string_node,
            extends: number_node,
            true_branch,
            false_branch,
            distributive: false,
        });
        let node_count_after = graph.node_count();
        // No new structural nodes should have been interned — both
        // branches already exist; the loser is not re-materialised.
        assert!(
            node_count_after - node_count_before <= 1,
            "closed conditional must not intern extra nodes beyond existing branches (before={node_count_before}, after={node_count_after})"
        );
    }

    /// An open/undecidable conditional keeps both branch references
    /// intact in a `Conditional` shell node. Neither branch is recursively
    /// expanded; the shell's fields point at the as-supplied branch ids.
    #[test]
    fn open_conditional_stays_deferred_with_shell_branch_refs_not_expanded_bodies() {
        let host = host();
        upsert_ts(
            &host,
            "/w/types.ts",
            "export type Foo = { x: number }\nexport type Bar = { y: string }",
        );
        let dispatch = ProjectSemanticDispatch::new(&host);
        let graph = Arc::clone(host.project_type_store().semantic_graph());

        // Check / extends are DeclAnchors — the shallow relation check
        // returns `Unknown` so the conditional stays deferred.
        let foo = resolve_decl_anchor(&dispatch, "/w/types.ts", "Foo");
        let bar = resolve_decl_anchor(&dispatch, "/w/types.ts", "Bar");
        let true_branch = primitive(&graph, PrimitiveKind::Boolean);
        let false_branch = primitive(&graph, PrimitiveKind::Number);

        let result = match dispatch.execute(SemanticQueryKey::Conditional {
            check: foo,
            extends: bar,
            true_branch,
            false_branch,
            distributive: false,
        }) {
            QueryResult::Value(id) => id,
            other => panic!("expected Value, got {other:?}"),
        };
        let data = graph.node_data(result).expect("result node");
        match &*data {
            SemanticNodeData::Conditional {
                check,
                extends,
                true_branch_ref,
                false_branch_ref,
                distributive,
            } => {
                assert_eq!(*check, foo);
                assert_eq!(*extends, bar);
                assert_eq!(*true_branch_ref, true_branch);
                assert_eq!(*false_branch_ref, false_branch);
                assert!(!distributive);
            }
            other => panic!("expected Conditional shell, got {other:?}"),
        }
        let edges = graph.origins_of_kind(result, OriginEdgeKind::ConditionalSelect);
        let has_deferred = edges
            .iter()
            .any(|e| matches!(&e.meta, OriginMeta::Branch(BranchSelection::Deferred)));
        assert!(
            has_deferred,
            "deferred conditional must emit ConditionalSelect(Branch::Deferred)"
        );
    }

    /// `infer` inside a closed conditional binds via the shared relation
    /// engine and emits `InferBind` edges. C2 does not wire the full
    /// relation engine yet — the solver hand-off lands in D2. Stay
    /// ignored until then; F1 un-ignores at track end.
    #[test]
    #[ignore = "pending D2 (solver conditional hand-off through dispatch)"]
    fn infer_in_closed_conditional_binds_via_relation() {}

    /// `infer` inside an open conditional must stay symbolic: no
    /// `InferBind` edge is emitted because the check could not decide.
    /// Requires the D2 solver integration to prove the negative
    /// (no edge) through a real infer-path fixture.
    #[test]
    #[ignore = "pending D2 (solver conditional hand-off through dispatch)"]
    fn infer_in_open_conditional_stays_symbolic_without_private_bind() {}

    /// Distinct projections into the same open conditional materialise
    /// only the visited subexpressions. Requires C3's path walker to
    /// distribute `ProjectPath` into each branch via dispatch re-entry.
    #[test]
    #[ignore = "pending C3 (build_project_path conditional distribution)"]
    fn distinct_projections_into_same_open_conditional_materialise_only_visited_subexpressions() {}
}
