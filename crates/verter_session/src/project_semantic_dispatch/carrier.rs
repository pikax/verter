//! Demand-time carrier-resolution context.
//!
//! [`CarrierResolverContext`] bundles the read-only resolution inputs a
//! `BareRef` / `ImportType` carrier needs to resolve at demand time — the
//! same inputs the eager
//! [`shallow_lower_type_expr_with_context`](super::ProjectSemanticDispatch::shallow_lower_type_expr_with_context)
//! `Ref` path consumes. The carrier-resolution dispatch reads it instead of
//! threading six positional arguments through every hop.
//!
//! **It is a RUNTIME / VALUE-SIDE context, NEVER a query key.** None of its
//! fields — `name_resolution`, the `DeclarationScopePayload`, the
//! `ScopeShadowing` set, the substitution env, or the reduction-demand axis
//! — may be hashed into a [`SemanticQueryKey`](crate::semantic_query::SemanticQueryKey):
//! query keys stay the content-free slot/fact identities (R6), and the
//! materialised VALUE roots its version through the produced node's
//! [`NodeScopeId`] + read-set. To make that misuse structurally impossible
//! the context borrows its inputs and deliberately derives neither `Hash`
//! nor `Eq`, so it cannot be a map key nor embedded in a derived-`Hash`
//! cache key.
//!
//! Two pieces the eager `Ref` path also touches are threaded SEPARATELY at
//! resolution time rather than living on this read-only bundle:
//! - the mutable `substitutions: &mut Vec<(Arc<str>, SemanticNodeId)>`
//!   accumulator (a write sink, not a read input), and
//! - the dispatcher-local active-instantiate stack (`instantiate_active`),
//!   which is `ProjectSemanticDispatch` state, not per-resolution context.
//!
//! The ambient-augmentation scope is NOT a separate field: it is derived
//! from `scope` (the owning canonical) plus the resolver's augmentation
//! index, exactly as the eager `Ref` path derives it.

use std::sync::Arc;

use rustc_hash::FxHashMap;

use verter_semantic::analysis::type_solver::host::ResolvedRootIdentity;

use super::ProjectSemanticDispatch;
use crate::resolver_core::bare_name_resolve::{
    resolve_bare_name_in_scope, DeclarationScopePayload,
};
use crate::resolver_core::scope_shadowing::ScopeShadowing;
use crate::semantic_query::{
    DeclIdentity, HashValue, NodeScopeId, PathSegment, ProjectionMode, ProjectionReductionContext,
    QueryError, QueryResult, ResolveDeclKey, ScopeId, SemanticNodeData, SemanticNodeId,
    SemanticQueryApi, SemanticQueryKey, SemanticQueryOutput,
};

/// Read-only, value-side resolution context for resolving a graph carrier
/// (`BareRef` / `ImportType`) at demand time. See the module docs for the
/// no-query-key contract.
pub(crate) struct CarrierResolverContext<'a> {
    /// Type-parameter binder environment (`param_name → arg node`). A bare
    /// reference that names a bound parameter substitutes to its argument.
    env: &'a FxHashMap<String, SemanticNodeId>,
    /// The lexical scope the carrier was captured in — the declaration-origin
    /// file + content generation + optional inner scope. Drives bare-name
    /// resolution and ambient-augmentation lookup.
    scope: &'a NodeScopeId,
    /// The prepared-decl `name_resolution` fast-path map (already-resolved
    /// imports from the body-file scope). Consulted before the host-owned
    /// bare-name resolver fallback.
    name_resolution: &'a FxHashMap<String, ResolvedRootIdentity>,
    /// The owner declaration-scope payload (scope-local type names /
    /// bindings), consulted by the bare-name resolver fallback. `None` for a
    /// global scope or a pre-bundle fixture.
    scope_payload: Option<&'a DeclarationScopePayload>,
    /// The builtin-shadowing set: bare names whose userland declaration must
    /// win over a same-named ambient-lib builtin.
    shadowing: &'a ScopeShadowing,
    /// The reduction-demand axis (`Published` / `StructuralTransit`) plus the
    /// query mode — selects carrier-vs-execute at the demand point.
    reduction_context: ProjectionReductionContext,
}

impl<'a> CarrierResolverContext<'a> {
    /// Bundle the read-only resolution inputs the eager `Ref` lowering path
    /// consumes. The argument order mirrors
    /// [`shallow_lower_type_expr_with_context`](super::ProjectSemanticDispatch::shallow_lower_type_expr_with_context)
    /// (minus the lowered `expr` and the mutable `substitutions` sink).
    pub(crate) fn new(
        env: &'a FxHashMap<String, SemanticNodeId>,
        scope: &'a NodeScopeId,
        name_resolution: &'a FxHashMap<String, ResolvedRootIdentity>,
        scope_payload: Option<&'a DeclarationScopePayload>,
        shadowing: &'a ScopeShadowing,
        reduction_context: ProjectionReductionContext,
    ) -> Self {
        Self {
            env,
            scope,
            name_resolution,
            scope_payload,
            shadowing,
            reduction_context,
        }
    }

    /// The type-parameter binder environment.
    pub(crate) fn env(&self) -> &FxHashMap<String, SemanticNodeId> {
        self.env
    }

    /// The captured lexical scope.
    pub(crate) fn scope(&self) -> &NodeScopeId {
        self.scope
    }

    /// The prepared-decl `name_resolution` fast-path map.
    pub(crate) fn name_resolution(&self) -> &FxHashMap<String, ResolvedRootIdentity> {
        self.name_resolution
    }

    /// The owner declaration-scope payload, if any.
    pub(crate) fn scope_payload(&self) -> Option<&DeclarationScopePayload> {
        self.scope_payload
    }

    /// The builtin-shadowing set.
    pub(crate) fn shadowing(&self) -> &ScopeShadowing {
        self.shadowing
    }

    /// The reduction-demand context (axis + mode).
    pub(crate) fn reduction_context(&self) -> ProjectionReductionContext {
        self.reduction_context
    }

    /// The query mode, projected out of the reduction context.
    pub(crate) fn mode(&self) -> ProjectionMode {
        self.reduction_context.mode
    }
}

impl<'a> ProjectSemanticDispatch<'a> {
    /// Resolve a `BareRef` head (`name` / `name<args>`) to its semantic node —
    /// the SOLE shared resolver for an unresolved bare-name reference, reached
    /// from BOTH the eager `TypeExpr::Ref` lowering arm AND carrier-subject
    /// normalization (when a `BareRef` carrier is a query subject). It recovers
    /// EXACTLY the resolution the eager `Ref` path performs: built-in
    /// shadowing-aware utility / Promise carriers, the bare-name fast-path
    /// (`name_resolution`) + `resolve_bare_name_in_scope` fallback,
    /// external/relative `declare module` augmentation, enum-member projection,
    /// the `is_instantiate_active` recursive-ref back-edge, and the routing
    /// through `DeclRef` / `InstantiationRef` (carrier modes) or `ResolveDecl` /
    /// `Instantiate` (eager modes).
    ///
    /// The argument NODES are produced LAZILY: `arg_count` is the (cheap) arity
    /// for the routing decisions (`is_empty` gates, the recursive-ref back-edge),
    /// and `lower_args` lowers the interned argument nodes ON DEMAND — invoked
    /// ONLY on the branch that actually consumes them (the Promise / builtin /
    /// carrier-mode / eager `Instantiate` branches). A head that MISSES
    /// (unresolvable name, no enum-member fallback) never calls `lower_args`, so
    /// it never lowers + dispatches dead args (which would load files, mark
    /// partials, and hit fuses for syntax the resolved head never consumes).
    ///
    /// The eager `Ref` arm passes a closure that lowers its raw `TypeExpr` args
    /// structurally; carrier resolution passes a closure that simply returns the
    /// already-lowered `carrier_type_args`. Either way the helper performs NO
    /// raw-`TypeExpr` arg lowering ITSELF — it keeps the typed-IR-only +
    /// single-resolver shape; the eager caller's closure routes through the same
    /// structural lowering.
    ///
    /// Returns a `SemanticNodeId` with `Opaque(Miss)` / `Opaque(RecursiveRef)`
    /// EXACTLY like the eager `Ref` arm — never a `QueryResult` (a bare node id
    /// keeps miss / recursive behaviour uniform across both callers). The active
    /// instantiate stack stays dispatcher state (`&self.is_instantiate_active`),
    /// never copied into the context.
    pub(super) fn resolve_bare_ref_head(
        &self,
        ctx: &CarrierResolverContext<'_>,
        name: &Arc<str>,
        arg_count: usize,
        lower_args: impl FnOnce() -> Arc<[SemanticNodeId]>,
    ) -> SemanticNodeId {
        let graph = self.graph();
        let scope = ctx.scope();
        let name_resolution = ctx.name_resolution();
        let scope_payload = ctx.scope_payload();
        let shadowing = ctx.shadowing();
        let reduction_context = ctx.reduction_context();
        let mode = ctx.mode();

        // Global lib-type fast path: an unshadowed `Promise<...>` interns a
        // nominal `InstantiationRef` carrier in EVERY mode — `Promise` has no
        // structural reducer arm, so the carrier preserves the declaration
        // identity + already-lowered type arguments for the demand points.
        // Userland shadowing wins via the same `name_resolution` /
        // `ScopeShadowing` gates the builtin utilities use.
        if !name_resolution.contains_key(name.as_ref())
            && !shadowing.is_shadowing_lib(name.as_ref())
            && self.is_promise_global_name(name.as_ref())
        {
            return graph.intern_node_with_scope(
                SemanticNodeData::InstantiationRef {
                    base: DeclIdentity {
                        canonical_id: Arc::from("__builtin__"),
                        whole_hash: HashValue::default(),
                        decl_name: Arc::clone(name),
                    },
                    args: lower_args(),
                },
                scope.clone(),
            );
        }

        // Built-in utility fast path: a recognised utility name NOT in
        // `name_resolution` (a global TS builtin, not imported / locally
        // declared) and NOT userland-shadowed routes through the builtin
        // carrier gate / eager `Instantiate`. Userland types that shadow a
        // builtin name live in `name_resolution` OR `ScopeShadowing`, so the
        // fast-path is suppressed and the bare-name walk below resolves the
        // userland alias — "user shadowing wins".
        if !name_resolution.contains_key(name.as_ref())
            && !shadowing.is_shadowing_lib(name.as_ref())
            && verter_semantic::analysis::type_solver::builtin::BuiltinUtility::from_name(
                name.as_ref(),
            )
            .is_some()
        {
            let builtin_identity = DeclIdentity {
                canonical_id: Arc::from("__builtin__"),
                whole_hash: HashValue::default(),
                decl_name: Arc::clone(name),
            };
            // A recognised builtin head DOES consume its args — lower them ONCE
            // here (the head resolved, so these are LIVE args), then use the
            // lowered nodes for the carrier gate AND the consume below.
            let type_args = lower_args();
            // Builtin carrier gate (LOWERING entrance) — the `InstantiationRef`
            // shell is preserved when: the mode is `Shallow` (all builtins);
            // the mode is `Navigate` and the name is an object-filter builtin
            // (`Pick`/`Omit`); the name is an object-filter builtin and the
            // enumeration domain (lowered argument 0) is OPEN (any mode); or the
            // mode is `Navigate`/`Skeleton` and any applied argument is OPEN. A
            // closed object-filter domain in `Expanded`/`Identity` executes the
            // `Instantiate` query path-precisely; non-object-filter builtins
            // keep the eager-resolve path in those modes. Family membership is
            // the ONE registry helper, never a local name match.
            let build_carrier = mode == ProjectionMode::Shallow
                || (crate::project_semantic_dispatch::raise::is_l1_object_filter_utility(
                    name.as_ref(),
                ) && (mode == ProjectionMode::Navigate
                    || crate::project_semantic_dispatch::raise::
                        utility_enumeration_domain_is_open_or_unknown(
                            self.ctx,
                            &builtin_identity,
                            &type_args,
                        )))
                || (matches!(mode, ProjectionMode::Navigate | ProjectionMode::Skeleton)
                    && type_args.iter().any(|arg| {
                        crate::project_semantic_dispatch::raise::builtin_lowering_argument_is_open(
                            self.ctx, *arg,
                        )
                    }));
            if build_carrier {
                return graph.intern_node_with_scope(
                    SemanticNodeData::InstantiationRef {
                        base: builtin_identity,
                        args: type_args,
                    },
                    scope.clone(),
                );
            }
            return match self.execute_type_node(SemanticQueryKey::Instantiate {
                base: self.type_slot_for(
                    Arc::clone(&builtin_identity.canonical_id),
                    Arc::clone(&builtin_identity.decl_name),
                ),
                args: type_args,
                context: self
                    .instantiate_context_for(&builtin_identity.canonical_id, reduction_context),
            }) {
                QueryResult::Value(SemanticQueryOutput { value: id, .. }) => id,
                _ => self.opaque(QueryError::Miss),
            };
        }

        // Bare-name resolution: the prepared-decl `name_resolution` map is the
        // fast path; the `resolve_bare_name_in_scope` fallback recovers any name
        // the map does not carry (the carrier-subject entry rehydrates an EMPTY
        // map + the scope payload, so the fallback is the whole resolver there).
        let resolved_root = if let Some(direct) = name_resolution.get(name.as_ref()) {
            Some((
                Arc::<str>::from(direct.canonical_id.as_str()),
                Arc::<str>::from(direct.symbol_name.as_str()),
            ))
        } else if let NodeScopeId::File { canonical_id, .. } = scope {
            resolve_bare_name_in_scope(
                self.ctx,
                canonical_id.as_ref(),
                scope_payload,
                name.as_ref(),
            )
            .map(|ri| {
                (
                    Arc::<str>::from(ri.canonical_id.as_str()),
                    Arc::<str>::from(ri.symbol_name.as_str()),
                )
            })
        } else {
            None
        };

        // A resolution "reaches a real declaration" only when its canonical
        // names a LOADABLE workspace file. An unresolved bare specifier resolves
        // to the empty non-file canonical OR the specifier string — neither
        // loadable — which is the external ambient-module case the augmentation
        // hook below handles.
        let resolves_to_file = match resolved_root.as_ref() {
            Some((canonical, _)) if !canonical.is_empty() => self
                .ctx
                .ensure_indexed_ready_serve(canonical.as_ref())
                .is_some(),
            _ => false,
        };
        if !resolves_to_file {
            if let NodeScopeId::File { canonical_id, .. } = scope {
                if let Some(merged) = self.resolve_external_module_augmentation(
                    canonical_id.as_ref(),
                    name.as_ref(),
                    scope,
                    reduction_context,
                ) {
                    return merged;
                }
            }
        }

        let Some((resolved_canonical, resolved_name)) = resolved_root else {
            // Enum-member projection (typed, GATED fallback): a dotted
            // `Enum.Member` whose prefix is a proven enum value declaration
            // projects the member's projected type. Gated on the typed
            // `ValueDeclKind::Enum` fact, not a dotted-name heuristic.
            if let NodeScopeId::File { canonical_id, .. } = scope {
                if let Some(member_value) = self.resolve_enum_member_value(
                    canonical_id.as_ref(),
                    name_resolution,
                    scope_payload,
                    name.as_ref(),
                ) {
                    // The projected member value is a raw `TypeExpr`; lower it
                    // through the shared eager lowering with the SAME value-side
                    // inputs (an enum member value carries no own type args).
                    let env = ctx.env();
                    let shadowing = ctx.shadowing();
                    let mut substitutions: Vec<(Arc<str>, SemanticNodeId)> = Vec::new();
                    return self.shallow_lower_type_expr_with_context(
                        &member_value,
                        env,
                        scope,
                        name_resolution,
                        scope_payload,
                        shadowing,
                        &mut substitutions,
                        reduction_context,
                    );
                }
            }
            return self.opaque(QueryError::Miss);
        };

        // Recursive-ref back-edge: a 0-arg head resolving to an identity already
        // being materialised by an enclosing `build_instantiate` frame mints
        // `Opaque(RecursiveRef)` — the dispatcher-local `instantiate_active`
        // stack is the single source of truth (never copied into the context).
        if arg_count == 0
            && self.is_instantiate_active(resolved_canonical.as_ref(), resolved_name.as_ref())
        {
            return self.opaque(QueryError::RecursiveRef {
                name: Arc::clone(&resolved_name),
            });
        }

        let whole_hash = self
            .ctx
            .shallow_file_state(resolved_canonical.as_ref())
            .map_or(HashValue::default(), |s| s.whole_hash);
        let decl_identity = DeclIdentity {
            canonical_id: Arc::clone(&resolved_canonical),
            whole_hash,
            decl_name: Arc::clone(&resolved_name),
        };

        // Carrier modes (Navigate / Skeleton / Shallow) intern a transparent
        // `DeclRef` (0-arg) / terminal `InstantiationRef` (n-arg) rather than
        // executing `ResolveDecl` / `Instantiate` eagerly — decl-body lowering
        // collects and indexes; the demand points materialise.
        if matches!(
            mode,
            ProjectionMode::Navigate | ProjectionMode::Skeleton | ProjectionMode::Shallow
        ) {
            if arg_count == 0 {
                return graph.intern_node_with_scope(
                    SemanticNodeData::DeclRef {
                        identity: decl_identity,
                    },
                    scope.clone(),
                );
            }
            return graph.intern_node_with_scope(
                SemanticNodeData::InstantiationRef {
                    base: decl_identity,
                    args: lower_args(),
                },
                scope.clone(),
            );
        }

        // Eager modes (Expanded / Identity): resolve the decl, then route
        // through `Instantiate` when the caller supplied args OR the decl has
        // type parameters (defaults must apply); a non-generic, no-arg decl
        // short-circuits to the bare `ResolveDecl` result.
        let anchor = match self.execute_type_node(SemanticQueryKey::ResolveDecl(ResolveDeclKey {
            scope: ScopeId {
                canonical_id: Arc::clone(&resolved_canonical),
                local_scope: None,
            },
            name: Arc::clone(&resolved_name),
        })) {
            QueryResult::Value(SemanticQueryOutput { value: id, .. }) => id,
            _ => return self.opaque(QueryError::Miss),
        };
        let decl_routes_through_instantiate = self
            .ctx
            .prepared_type_decl(resolved_canonical.as_ref(), resolved_name.as_ref())
            .is_some_and(|prepared| !prepared.type_parameters.is_empty());
        if arg_count == 0 && !decl_routes_through_instantiate {
            anchor
        } else {
            match self.execute_type_node(SemanticQueryKey::Instantiate {
                base: self.type_slot_for(
                    Arc::clone(&decl_identity.canonical_id),
                    Arc::clone(&decl_identity.decl_name),
                ),
                args: lower_args(),
                context: self
                    .instantiate_context_for(&decl_identity.canonical_id, reduction_context),
            }) {
                QueryResult::Value(SemanticQueryOutput { value: id, .. }) => id,
                _ => self.opaque(QueryError::Miss),
            }
        }
    }

    /// Resolve an `ImportType` head (`import("specifier").qualifier<args>` /
    /// `typeof import("specifier")`) to its semantic node — the shared resolver
    /// for an unresolved dynamic-import reference, reached from BOTH the eager
    /// `TypeExpr::ImportType` arm AND carrier-subject normalization.
    ///
    /// `owner_canonical` is the file the import expression lives in (the
    /// node-level scope of an `ImportType` carrier); it resolves the relative
    /// `specifier`. The argument NODES are produced LAZILY (`arg_count` is the
    /// cheap arity; `lower_args` lowers them on demand) so an UNRESOLVABLE
    /// specifier / qualifier never lowers + dispatches dead args. Returns
    /// `Opaque(Miss)` on an unresolvable specifier / qualifier, EXACTLY like the
    /// eager arm.
    ///
    /// The TYPE-position qualifier head segment routes through the SAME
    /// `resolve_bare_ref_head` (over an injected `name_resolution` entry binding
    /// the head segment into the imported module's TYPE-export space), so there
    /// is ONE bare-name resolver — not a parallel import resolver.
    pub(super) fn resolve_import_type_head(
        &self,
        ctx: &CarrierResolverContext<'_>,
        owner_canonical: &str,
        specifier: &Arc<str>,
        qualifier: &[Arc<str>],
        typeof_query: bool,
        arg_count: usize,
        lower_args: impl FnOnce() -> Arc<[SemanticNodeId]>,
    ) -> SemanticNodeId {
        let reduction_context = ctx.reduction_context();
        let Some(dep_canonical) = self
            .ctx
            .resolve_type_dependency_canonical(owner_canonical, specifier.as_ref())
        else {
            // Unresolvable specifier — miss WITHOUT lowering the dead args.
            return self.opaque(QueryError::Miss);
        };

        if typeof_query {
            // `typeof import("./m")` — the module's VALUE-export namespace. A
            // trailing qualifier projects a member path; trailing type args
            // apply the SAME positional binder substitution as
            // `typeof C.make<string>` (the shared `apply_typeof_instantiation_args`).
            let namespace = self.build_import_value_namespace(&dep_canonical, reduction_context);
            let mut result = if qualifier.is_empty() {
                namespace
            } else {
                let path: Arc<[PathSegment]> = Arc::from(
                    qualifier
                        .iter()
                        .map(|seg| PathSegment::Member(Arc::clone(seg)))
                        .collect::<Vec<_>>()
                        .into_boxed_slice(),
                );
                match self.execute_type_node(SemanticQueryKey::ProjectPath {
                    base: namespace,
                    path,
                    context: ProjectionReductionContext::published(ProjectionMode::Navigate),
                }) {
                    QueryResult::Value(SemanticQueryOutput { value: id, .. }) => id,
                    _ => self.opaque(QueryError::Miss),
                }
            };
            if arg_count > 0 {
                let type_args = lower_args();
                result = self.apply_typeof_instantiation_args(result, &type_args);
            }
            return result;
        }

        // `import("./m").Member` in TYPE position — resolve the qualifier as a
        // TYPE export of the module.
        let Some((first, rest)) = qualifier.split_first() else {
            // A bare `import("./m")` with no qualifier is the whole
            // module-namespace TYPE — not a single addressable declaration.
            return self.opaque(QueryError::Miss);
        };
        // A MULTI-SEGMENT qualifier carrying generic args binds them to the
        // TERMINAL segment, which the multi-hop tail (`ProjectPath`, plain
        // member projection) cannot carry — emit an HONEST error carrier rather
        // than silently dropping the args.
        if !rest.is_empty() && arg_count > 0 {
            return self.opaque(QueryError::Other(Arc::from(
                "import-type generic args on a multi-segment qualifier are not yet instantiated",
            )));
        }

        // Bind the head segment into the imported module's TYPE-export space
        // over a CLONE of the caller's `name_resolution`, then route the head
        // through the SHARED `resolve_bare_ref_head` (its fast-path targets the
        // injected entry directly). The terminal segment carries the args (lowered
        // lazily by the head resolver); a multi-hop resolves the head bare and
        // projects the tail.
        let mut injected = ctx.name_resolution().clone();
        injected.insert(
            first.as_ref().to_string(),
            ResolvedRootIdentity::new(&dep_canonical, first.as_ref()),
        );
        // Single-segment terminal carries the args; a multi-hop head is bare (its
        // args, if any, were rejected by the error above).
        let head_arg_count = if rest.is_empty() { arg_count } else { 0 };
        let head_ctx = CarrierResolverContext::new(
            ctx.env(),
            ctx.scope(),
            &injected,
            ctx.scope_payload(),
            ctx.shadowing(),
            reduction_context,
        );
        let head_node = self.resolve_bare_ref_head(&head_ctx, first, head_arg_count, move || {
            if rest.is_empty() {
                lower_args()
            } else {
                Arc::from(Vec::new().into_boxed_slice())
            }
        });
        if rest.is_empty() {
            return head_node;
        }
        let path: Arc<[PathSegment]> = Arc::from(
            rest.iter()
                .map(|seg| PathSegment::Member(Arc::clone(seg)))
                .collect::<Vec<_>>()
                .into_boxed_slice(),
        );
        match self.execute_type_node(SemanticQueryKey::ProjectPath {
            base: head_node,
            path,
            context: ProjectionReductionContext::published(ProjectionMode::Navigate),
        }) {
            QueryResult::Value(SemanticQueryOutput { value: id, .. }) => id,
            _ => self.opaque(QueryError::Miss),
        }
    }

    /// Resolve a carrier SUBJECT node to its real semantic node when `node` is a
    /// `BareRef` / `ImportType` carrier, returning the rewritten node id; returns
    /// `node` UNCHANGED for any non-carrier (or a re-entrant carrier already
    /// being normalized — the pre-memo cycle guard).
    ///
    /// This is the carrier-subject side of the ONE shared resolver: it rehydrates
    /// a value-side [`CarrierResolverContext`] from the carrier's own
    /// node-level scope (the body file) — an EMPTY `name_resolution` plus the
    /// scope's `DeclarationScopePayload` + `ScopeShadowing` derived from
    /// `prepared_decl_bundle`, exactly the shape
    /// [`Self::lower_type_expr_in_scope_with_context`] uses in production — then
    /// calls [`Self::resolve_bare_ref_head`] / [`Self::resolve_import_type_head`]
    /// in the query's `context`. A carrier node carries no body-file
    /// `name_resolution` map (that map is a lowering-time local), so this path
    /// recovers the resolution through the `resolve_bare_name_in_scope` fallback;
    /// the import-type head re-injects its own head binding.
    pub(super) fn resolve_carrier_subject_node(
        &self,
        node: SemanticNodeId,
        context: ProjectionReductionContext,
    ) -> SemanticNodeId {
        let graph = self.graph();
        let Some(data) = graph.node_data(node) else {
            return node;
        };
        // Fast reject: only the two unresolved reference carriers are
        // head-resolvable as a query subject.
        let is_bare = data.bare_ref_head().is_some();
        let is_import = data.import_type_head().is_some();
        if !is_bare && !is_import {
            return node;
        }
        drop(data);

        // Pre-memo cycle guard: if this exact carrier node is already being
        // normalized on this dispatcher's call chain, return it unchanged. The
        // downstream `ResolveDecl` / `Instantiate` / `ProjectPath` memo
        // sentinels + the active-instantiation back-edge are the primary
        // termination mechanism; this only prevents a pre-memo carrier ->
        // carrier normalization loop.
        if self.carrier_normalizing.borrow().contains(&node) {
            return node;
        }
        self.carrier_normalizing.borrow_mut().push(node);

        // The carrier's already-lowered args (read through the SOLE sanctioned
        // descent accessor) and node-level scope (the owner/body file).
        let type_args: Arc<[SemanticNodeId]> = {
            let data = graph.node_data(node).expect("carrier node data");
            Arc::from(data.carrier_type_args().to_vec().into_boxed_slice())
        };
        let owner_canonical = graph.node_scope(node).and_then(|s| s.canonical_file());

        // Rehydrate the value-side resolution inputs from the carrier's scope.
        let env: FxHashMap<String, SemanticNodeId> = FxHashMap::default();
        let name_resolution: FxHashMap<String, ResolvedRootIdentity> = FxHashMap::default();
        let resolved = if is_bare {
            let (name, scope) = {
                let data = graph.node_data(node).expect("BareRef carrier data");
                let (name, scope) = data.bare_ref_head().expect("BareRef head");
                (Arc::clone(name), scope.clone())
            };
            // The scope payload is rehydrated from the carrier's PAYLOAD-scope
            // canonical — the exact scope handed to the resolver context — so the
            // `resolve_bare_name_in_scope` fallback consults the right file's
            // declaration scope (not a divergent node-level scope canonical).
            let scope_payload = scope.canonical_file().and_then(|c| {
                self.ctx
                    .prepared_decl_bundle(c.as_ref())
                    .map(|bundle| DeclarationScopePayload::from_bundle(&bundle))
            });
            let shadowing = ScopeShadowing::from_scope_payload(scope_payload.as_ref());
            let ctx = CarrierResolverContext::new(
                &env,
                &scope,
                &name_resolution,
                scope_payload.as_ref(),
                &shadowing,
                context,
            );
            // The carrier carries ALREADY-LOWERED args — the lazy `lower_args`
            // closure simply returns them (no lowering happens here).
            let arg_count = type_args.len();
            self.resolve_bare_ref_head(&ctx, &name, arg_count, move || type_args)
        } else {
            let (specifier, qualifier, typeof_query) = {
                let data = graph.node_data(node).expect("ImportType carrier data");
                let (specifier, qualifier, typeof_query) =
                    data.import_type_head().expect("ImportType head");
                (Arc::clone(specifier), Arc::clone(qualifier), typeof_query)
            };
            let scope = graph.node_scope(node).unwrap_or(NodeScopeId::Global);
            let scope_payload = owner_canonical.as_ref().and_then(|c| {
                self.ctx
                    .prepared_decl_bundle(c.as_ref())
                    .map(|bundle| DeclarationScopePayload::from_bundle(&bundle))
            });
            let shadowing = ScopeShadowing::from_scope_payload(scope_payload.as_ref());
            let ctx = CarrierResolverContext::new(
                &env,
                &scope,
                &name_resolution,
                scope_payload.as_ref(),
                &shadowing,
                context,
            );
            match owner_canonical {
                Some(owner) => {
                    // The carrier carries ALREADY-LOWERED args — the lazy closure
                    // returns them (no lowering here).
                    let arg_count = type_args.len();
                    self.resolve_import_type_head(
                        &ctx,
                        owner.as_ref(),
                        &specifier,
                        &qualifier,
                        typeof_query,
                        arg_count,
                        move || type_args,
                    )
                }
                // An `ImportType` carrier with no file scope cannot resolve its
                // relative specifier — an honest miss (mirrors the eager arm's
                // non-`File` scope guard).
                None => self.opaque(QueryError::Miss),
            }
        };

        self.carrier_normalizing.borrow_mut().pop();
        resolved
    }

    /// Rewrite a base-bearing [`SemanticQueryKey`] whose SUBJECT node is a
    /// `BareRef` / `ImportType` carrier to the resolved subject — the canonical
    /// carrier-subject normalization hook, applied at the query entry AFTER
    /// sugar-canonicalisation and BEFORE cooperative-memo admission so dedup /
    /// singleflight keys on the REAL semantic subject.
    ///
    /// Only the carrier-bearing positions are rewritten; every other key is
    /// returned verbatim. `Instantiate` / `ResolveDecl` / `ResolveMacroPayload`
    /// carry content-free slot identities (not a `SemanticNodeId` subject) and
    /// are untouched. The reduction context handed to the head resolver is the
    /// key's own context (so an `Expanded` query resolves the carrier eagerly,
    /// a `Navigate` query carrier-preserves).
    pub(super) fn normalize_carrier_subject_key(&self, key: SemanticQueryKey) -> SemanticQueryKey {
        match key {
            SemanticQueryKey::ProjectPath {
                base,
                path,
                context,
            } => {
                let base = self.resolve_carrier_subject_node(base, context);
                SemanticQueryKey::ProjectPath {
                    base,
                    path,
                    context,
                }
            }
            SemanticQueryKey::ProjectMember { base, member, mode } => {
                let base = self.resolve_carrier_subject_node(
                    base,
                    ProjectionReductionContext::published(mode),
                );
                SemanticQueryKey::ProjectMember { base, member, mode }
            }
            SemanticQueryKey::IndexedAccess { base, index, mode } => {
                let base = self.resolve_carrier_subject_node(
                    base,
                    ProjectionReductionContext::published(mode),
                );
                SemanticQueryKey::IndexedAccess { base, index, mode }
            }
            SemanticQueryKey::KeyOf { base, context } => {
                let base = self.resolve_carrier_subject_node(base, context);
                SemanticQueryKey::KeyOf { base, context }
            }
            SemanticQueryKey::MappedType {
                source,
                mapper,
                context,
            } => {
                let source = self.resolve_carrier_subject_node(source, context);
                SemanticQueryKey::MappedType {
                    source,
                    mapper,
                    context,
                }
            }
            // Every other key carries no `SemanticNodeId` subject that could be a
            // carrier (or carries content-free slot identities) — return
            // verbatim. Nested carriers inside Intersection / Union / heritage
            // surfaces re-enter THIS normalization through the shallow-synthesis
            // worklist's re-dispatch (not resolved here).
            other => other,
        }
    }

    /// CHEAP probe: does `key` carry a `BareRef` / `ImportType` carrier as its
    /// SUBJECT node? Returns `true` only for a base-bearing key whose subject
    /// node is one of the two unresolved-reference carriers — the exact set
    /// [`Self::normalize_carrier_subject_key`] rewrites. A non-carrier key (the
    /// common case) returns `false` with one node-data shape check and NO
    /// resolution / tracer cost. Used to gate the traced normalization prelude
    /// so the non-carrier fast path stays allocation-free.
    pub(super) fn key_subject_is_carrier(&self, key: &SemanticQueryKey) -> bool {
        let subject = match key {
            SemanticQueryKey::ProjectPath { base, .. }
            | SemanticQueryKey::ProjectMember { base, .. }
            | SemanticQueryKey::IndexedAccess { base, .. }
            | SemanticQueryKey::KeyOf { base, .. } => *base,
            SemanticQueryKey::MappedType { source, .. } => *source,
            _ => return false,
        };
        let graph = self.graph();
        let Some(data) = graph.node_data(subject) else {
            return false;
        };
        data.bare_ref_head().is_some() || data.import_type_head().is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn carrier_resolver_context_bundles_resolution_inputs() {
        // Construct from the same read-only inputs the eager `Ref` path uses,
        // and assert every accessor returns the wired value.
        let mut env: FxHashMap<String, SemanticNodeId> = FxHashMap::default();
        env.insert("T".to_string(), SemanticNodeId(11));
        let mut name_resolution: FxHashMap<String, ResolvedRootIdentity> = FxHashMap::default();
        name_resolution.insert(
            "Foo".to_string(),
            ResolvedRootIdentity::new("/foo.ts", "Foo"),
        );
        let scope = NodeScopeId::Global;
        let shadowing = ScopeShadowing::empty();
        let reduction = ProjectionReductionContext::published(ProjectionMode::Navigate);

        let ctx = CarrierResolverContext::new(
            &env,
            &scope,
            &name_resolution,
            None,
            &shadowing,
            reduction,
        );

        assert_eq!(ctx.env().get("T"), Some(&SemanticNodeId(11)));
        assert!(matches!(ctx.scope(), NodeScopeId::Global));
        assert_eq!(
            ctx.name_resolution()
                .get("Foo")
                .map(|r| r.symbol_name.as_str()),
            Some("Foo")
        );
        assert!(ctx.scope_payload().is_none());
        // The shadow set is the empty set here (no userland shadow).
        let _ = ctx.shadowing();
        assert_eq!(ctx.mode(), ProjectionMode::Navigate);
        assert_eq!(ctx.reduction_context().mode, ProjectionMode::Navigate);
    }
}
