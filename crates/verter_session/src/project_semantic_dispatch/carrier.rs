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
    name_resolution: &'a FxHashMap<std::sync::Arc<str>, ResolvedRootIdentity>,
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
        name_resolution: &'a FxHashMap<std::sync::Arc<str>, ResolvedRootIdentity>,
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
    pub(crate) fn name_resolution(&self) -> &FxHashMap<std::sync::Arc<str>, ResolvedRootIdentity> {
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

/// Head-resolution outcome fed to the ONE shared reference-head carrier
/// interner ([`ProjectSemanticDispatch::intern_ref_head_carrier`]).
pub(super) enum RefHeadResolution {
    /// The head resolved to a real declaration slot identity.
    Resolved(DeclIdentity),
    /// The head is an unshadowed global lib type (`Promise` / a builtin
    /// utility) — always the nominal `__builtin__` `InstantiationRef`
    /// carrier, even with zero applied arguments.
    Builtin(DeclIdentity),
    /// The head is CURRENTLY unresolvable in the raise scope.
    Unresolved,
}

/// A reference head can be resolved without touching its type arguments.
/// Argument projection is requested only after the head proves that the
/// arguments are live, allowing structural callers to suspend the resolution
/// on an explicit worklist instead of recursively projecting from a closure.
pub(super) enum CarrierResolutionPlan {
    Ready(SemanticNodeId),
    NeedsArgs(CarrierArgsContinuation),
}

/// Owned second half of a reference resolution whose head consumes arguments.
/// Keeping this continuation independent of [`CarrierResolverContext`] is what
/// lets the projection worklist resume it after all argument nodes complete.
pub(super) enum CarrierArgsContinuation {
    Intern {
        head: RefHeadResolution,
        name: Arc<str>,
        scope: NodeScopeId,
    },
    Builtin {
        identity: DeclIdentity,
        name: Arc<str>,
        scope: NodeScopeId,
        context: ProjectionReductionContext,
    },
    Instantiate {
        identity: DeclIdentity,
        context: ProjectionReductionContext,
    },
    ApplyTypeof {
        base: SemanticNodeId,
    },
}

impl<'a> ProjectSemanticDispatch<'a> {
    pub(super) fn finish_carrier_resolution(
        &self,
        continuation: CarrierArgsContinuation,
        type_args: Arc<[SemanticNodeId]>,
    ) -> SemanticNodeId {
        match continuation {
            CarrierArgsContinuation::Intern { head, name, scope } => {
                self.intern_ref_head_carrier(head, &name, &scope, type_args)
            }
            CarrierArgsContinuation::Builtin {
                identity,
                name,
                scope,
                context,
            } => {
                let build_carrier = context.mode == ProjectionMode::Shallow
                    || (crate::project_semantic_dispatch::raise::is_l1_object_filter_utility(
                        name.as_ref(),
                    ) && (context.mode == ProjectionMode::Navigate
                        || crate::project_semantic_dispatch::raise::
                            utility_enumeration_domain_is_open_or_unknown(
                                self,
                                &identity,
                                &type_args,
                            )))
                    || (matches!(
                        context.mode,
                        ProjectionMode::Navigate | ProjectionMode::Skeleton
                    ) && type_args.iter().any(|arg| {
                        crate::project_semantic_dispatch::raise::
                            builtin_lowering_argument_is_open(self, *arg)
                    }));
                if build_carrier {
                    return self.intern_ref_head_carrier(
                        RefHeadResolution::Builtin(identity),
                        &name,
                        &scope,
                        type_args,
                    );
                }
                match self.execute_type_node(SemanticQueryKey::Instantiate(
                    crate::semantic_query::InstantiateKey::new(
                        self.type_slot_for(
                            Arc::clone(&identity.canonical_id),
                            Arc::clone(&identity.decl_name),
                        ),
                        type_args,
                        self.instantiate_context_for(&identity.canonical_id, context),
                    ),
                )) {
                    QueryResult::Value(SemanticQueryOutput { value, .. }) => value,
                    _ => self.opaque(QueryError::Miss),
                }
            }
            CarrierArgsContinuation::Instantiate { identity, context } => {
                match self.execute_type_node(SemanticQueryKey::Instantiate(
                    crate::semantic_query::InstantiateKey::new(
                        self.type_slot_for(
                            Arc::clone(&identity.canonical_id),
                            Arc::clone(&identity.decl_name),
                        ),
                        type_args,
                        self.instantiate_context_for(&identity.canonical_id, context),
                    ),
                )) {
                    QueryResult::Value(SemanticQueryOutput { value, .. }) => value,
                    _ => self.opaque(QueryError::Miss),
                }
            }
            CarrierArgsContinuation::ApplyTypeof { base } => {
                self.apply_typeof_instantiation_args(base, &type_args)
            }
        }
    }

    /// The ONE shared reference-head carrier interner. Both locator-shape
    /// lowering ([`Self::resolve_locator_ref_head`]) and the shared
    /// bare-ref head resolver ([`Self::resolve_bare_ref_head`] — reached
    /// from the eager `Ref` lowering arm, carrier-subject normalization,
    /// and closed-leaf raising) intern their head-resolution results
    /// through this single helper:
    ///
    /// - a RESOLVED user declaration interns the transparent `DeclRef`
    ///   (0-arg) / terminal `InstantiationRef` (n-arg) identity carrier;
    /// - a BUILTIN head interns the nominal `__builtin__`
    ///   `InstantiationRef` carrier;
    /// - a currently-UNRESOLVED authored reference stays a scoped
    ///   `BareRef` carrier — name / already-lowered args / scope are
    ///   PRESERVED, never collapsed to an `Opaque(Miss)` (which destroys
    ///   the authored head). The demand points own its resolution: a
    ///   later `Navigate` demand retries identity, a later `Expanded`
    ///   demand resolves and executes the carrier.
    pub(super) fn intern_ref_head_carrier(
        &self,
        head: RefHeadResolution,
        name: &Arc<str>,
        scope: &NodeScopeId,
        args: Arc<[SemanticNodeId]>,
    ) -> SemanticNodeId {
        let graph = self.graph();
        match head {
            RefHeadResolution::Resolved(identity) if args.is_empty() => {
                graph.intern_node_with_scope(SemanticNodeData::DeclRef { identity }, scope.clone())
            }
            RefHeadResolution::Resolved(identity) => graph.intern_node_with_scope(
                SemanticNodeData::InstantiationRef {
                    base: identity,
                    args,
                },
                scope.clone(),
            ),
            RefHeadResolution::Builtin(identity) => graph.intern_node_with_scope(
                SemanticNodeData::InstantiationRef {
                    base: identity,
                    args,
                },
                scope.clone(),
            ),
            RefHeadResolution::Unresolved => graph.intern_node_with_scope(
                SemanticNodeData::new_bare_ref(Arc::clone(name), scope.clone(), args),
                scope.clone(),
            ),
        }
    }

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
        match self.plan_bare_ref_head(ctx, name, arg_count) {
            CarrierResolutionPlan::Ready(value) => value,
            CarrierResolutionPlan::NeedsArgs(continuation) => {
                self.finish_carrier_resolution(continuation, lower_args())
            }
        }
    }

    pub(super) fn plan_bare_ref_head(
        &self,
        ctx: &CarrierResolverContext<'_>,
        name: &Arc<str>,
        arg_count: usize,
    ) -> CarrierResolutionPlan {
        let scope = ctx.scope();
        let name_resolution = ctx.name_resolution();
        let scope_payload = ctx.scope_payload();
        let shadowing = ctx.shadowing();
        let reduction_context = ctx.reduction_context();
        let mode = ctx.mode();

        // Global lib-type fast path: an unshadowed runtime nominal interns a
        // `DeclRef` / `InstantiationRef` carrier in EVERY mode, preserving its
        // declaration identity for semantic classifiers and reducers.
        // Userland shadowing wins via the same `name_resolution` /
        // `ScopeShadowing` gates the builtin utilities use.
        if !name_resolution.contains_key(name.as_ref())
            && !shadowing.is_shadowing_lib(name.as_ref())
            && self.runtime_nominal_global_name(name.as_ref()).is_some()
        {
            return CarrierResolutionPlan::NeedsArgs(CarrierArgsContinuation::Intern {
                head: RefHeadResolution::Builtin(DeclIdentity {
                    canonical_id: Arc::from("__builtin__"),
                    whole_hash: HashValue::default(),
                    decl_name: Arc::clone(name),
                }),
                name: Arc::clone(name),
                scope: scope.clone(),
            });
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
            return CarrierResolutionPlan::NeedsArgs(CarrierArgsContinuation::Builtin {
                identity: builtin_identity,
                name: Arc::clone(name),
                scope: scope.clone(),
                context: reduction_context,
            });
        }

        // Bare-name resolution: the prepared-decl `name_resolution` map is the
        // fast path; the `resolve_bare_name_in_scope` fallback recovers any name
        // the map does not carry (the carrier-subject entry rehydrates an EMPTY
        // map + the scope payload, so the fallback is the whole resolver there).
        let resolved_root = if let Some(direct) = name_resolution.get(name.as_ref()) {
            Some((
                Arc::clone(&direct.canonical_id),
                Arc::clone(&direct.symbol_name),
            ))
        } else if let NodeScopeId::File { canonical_id, .. } = scope {
            resolve_bare_name_in_scope(
                self.ctx,
                canonical_id.as_ref(),
                scope_payload,
                name.as_ref(),
            )
            .map(|ri| (ri.canonical_id, ri.symbol_name))
        } else {
            None
        };

        // A resolution "reaches a real declaration" only when its canonical
        // names a LOADABLE workspace file. An unresolved bare specifier resolves
        // to the empty non-file canonical OR the specifier string — neither
        // loadable — which is the external ambient-module case the augmentation
        // hook below handles.
        let resolves_to_file = match resolved_root.as_ref() {
            Some((canonical, _)) if !canonical.is_empty() => {
                // The DIRECT carrier serve (the LB3 poison shape): the head
                // probes `IndexedReady` availability, then Navigate/Skeleton/
                // Shallow interns a `DeclRef`/`InstantiationRef` and returns
                // with NO nested `execute_read` — so a FENCED serve here marks
                // ONLY the fan-out tracer, never the `CacheRead` funnel. The
                // `IndexedReadyServe` is retained (not immediately `.is_some()`)
                // so the test seam can consult it before presence collapses.
                let serve = self.ctx.ensure_indexed_ready_serve(canonical.as_ref());
                // Test-only: an armed fence treats a present serve as
                // `store_published == false` and fans a non-cacheable read onto
                // every active tracer — the deterministic in-process equivalent
                // of a mid-flight-supersession fenced serve consumed AT this
                // direct probe. Presence still governs `resolves_to_file`, so
                // the production resolution shape is byte-identical; placing the
                // injection AT the probe proves the direct serve lies inside the
                // evaluator's nested-tracer scope.
                #[cfg(test)]
                if serve.is_some()
                    && self
                        .ctx
                        .host_for_fact_tracer_install()
                        .test_force
                        .force_carrier_direct_serve_fence_for_tests
                        .load(std::sync::atomic::Ordering::Relaxed)
                {
                    crate::resolver_core::resolver_context::note_non_cacheable_read_fan_out(
                        crate::resolver_core::resolver_context::NonCacheableReadReason::FencedServe,
                    );
                }
                serve.is_some()
            }
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
                    return CarrierResolutionPlan::Ready(merged);
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
                    return CarrierResolutionPlan::Ready(
                        self.shallow_lower_type_expr_with_context(
                            &member_value,
                            env,
                            scope,
                            name_resolution,
                            scope_payload,
                            shadowing,
                            &mut substitutions,
                            reduction_context,
                        ),
                    );
                }
            }
            // Currently-unresolved authored reference. Under the carrier
            // modes the head STAYS a scoped `BareRef` carrier — the authored
            // name / args / scope are semantic content the demand points
            // retry (`Navigate` retries identity, `Expanded` resolves +
            // executes); collapsing to `Opaque(Miss)` here would destroy
            // them at raise time. The eager modes (`Expanded` / `Identity`)
            // ARE the demand point: a head that still does not resolve there
            // is an honest miss.
            if matches!(
                mode,
                ProjectionMode::Navigate | ProjectionMode::Skeleton | ProjectionMode::Shallow
            ) {
                return CarrierResolutionPlan::NeedsArgs(CarrierArgsContinuation::Intern {
                    head: RefHeadResolution::Unresolved,
                    name: Arc::clone(name),
                    scope: scope.clone(),
                });
            }
            return CarrierResolutionPlan::Ready(self.opaque(QueryError::Miss));
        };

        // Recursive-ref back-edge: a 0-arg head resolving to an identity already
        // being materialised by an enclosing `build_instantiate` frame mints
        // `Opaque(RecursiveRef)` — the dispatcher-local `instantiate_active`
        // stack is the single source of truth (never copied into the context).
        if arg_count == 0
            && self.is_instantiate_active(resolved_canonical.as_ref(), resolved_name.as_ref())
        {
            return CarrierResolutionPlan::Ready(self.opaque(QueryError::RecursiveRef {
                name: Arc::clone(&resolved_name),
            }));
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
                return CarrierResolutionPlan::Ready(self.intern_ref_head_carrier(
                    RefHeadResolution::Resolved(decl_identity),
                    name,
                    scope,
                    Arc::from(Vec::new().into_boxed_slice()),
                ));
            }
            return CarrierResolutionPlan::NeedsArgs(CarrierArgsContinuation::Intern {
                head: RefHeadResolution::Resolved(decl_identity),
                name: Arc::clone(name),
                scope: scope.clone(),
            });
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
            _ => return CarrierResolutionPlan::Ready(self.opaque(QueryError::Miss)),
        };
        let decl_routes_through_instantiate = self
            .ctx
            .prepared_type_decl(resolved_canonical.as_ref(), resolved_name.as_ref())
            .is_some_and(|prepared| !prepared.type_parameters.is_empty());
        if arg_count == 0 && !decl_routes_through_instantiate {
            CarrierResolutionPlan::Ready(anchor)
        } else {
            CarrierResolutionPlan::NeedsArgs(CarrierArgsContinuation::Instantiate {
                identity: decl_identity,
                context: reduction_context,
            })
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
        match self.plan_import_type_head(
            ctx,
            owner_canonical,
            specifier,
            qualifier,
            typeof_query,
            arg_count,
        ) {
            CarrierResolutionPlan::Ready(value) => value,
            CarrierResolutionPlan::NeedsArgs(continuation) => {
                self.finish_carrier_resolution(continuation, lower_args())
            }
        }
    }

    pub(super) fn plan_import_type_head(
        &self,
        ctx: &CarrierResolverContext<'_>,
        owner_canonical: &str,
        specifier: &Arc<str>,
        qualifier: &[Arc<str>],
        typeof_query: bool,
        arg_count: usize,
    ) -> CarrierResolutionPlan {
        let reduction_context = ctx.reduction_context();
        let Some(dep_canonical) = self
            .ctx
            .resolve_type_dependency_canonical(owner_canonical, specifier.as_ref())
        else {
            // Unresolvable specifier — miss WITHOUT lowering the dead args.
            return CarrierResolutionPlan::Ready(self.opaque(QueryError::Miss));
        };

        if typeof_query {
            // `typeof import("./m")` — the module's VALUE-export namespace. A
            // trailing qualifier projects a member path; trailing type args
            // apply the SAME positional binder substitution as
            // `typeof C.make<string>` (the shared `apply_typeof_instantiation_args`).
            let namespace = self.build_import_value_namespace(&dep_canonical, reduction_context);
            let result = if qualifier.is_empty() {
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
                return CarrierResolutionPlan::NeedsArgs(CarrierArgsContinuation::ApplyTypeof {
                    base: result,
                });
            }
            return CarrierResolutionPlan::Ready(result);
        }

        // `import("./m").Member` in TYPE position — resolve the qualifier as a
        // TYPE export of the module.
        let Some((first, rest)) = qualifier.split_first() else {
            // A bare `import("./m")` with no qualifier is the whole
            // module-namespace TYPE — not a single addressable declaration.
            return CarrierResolutionPlan::Ready(self.opaque(QueryError::Miss));
        };
        // A MULTI-SEGMENT qualifier carrying generic args binds them to the
        // TERMINAL segment, which the multi-hop tail (`ProjectPath`, plain
        // member projection) cannot carry — emit an HONEST error carrier rather
        // than silently dropping the args.
        if !rest.is_empty() && arg_count > 0 {
            return CarrierResolutionPlan::Ready(self.opaque(QueryError::Other(Arc::from(
                "import-type generic args on a multi-segment qualifier are not yet instantiated",
            ))));
        }

        // Bind the head segment into the imported module's TYPE-export space
        // over a CLONE of the caller's `name_resolution`, then route the head
        // through the SHARED `resolve_bare_ref_head` (its fast-path targets the
        // injected entry directly). The terminal segment carries the args (lowered
        // lazily by the head resolver); a multi-hop resolves the head bare and
        // projects the tail.
        let mut injected = ctx.name_resolution().clone();
        let head_name: Arc<str> = Arc::from(first.as_ref());
        injected.insert(
            Arc::clone(&head_name),
            ResolvedRootIdentity::new(dep_canonical.as_str(), head_name),
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
        let head_plan = self.plan_bare_ref_head(&head_ctx, first, head_arg_count);
        if rest.is_empty() {
            return head_plan;
        }
        let head_node = match head_plan {
            CarrierResolutionPlan::Ready(value) => value,
            CarrierResolutionPlan::NeedsArgs(continuation) => self
                .finish_carrier_resolution(continuation, Arc::from(Vec::new().into_boxed_slice())),
        };
        let path: Arc<[PathSegment]> = Arc::from(
            rest.iter()
                .map(|seg| PathSegment::Member(Arc::clone(seg)))
                .collect::<Vec<_>>()
                .into_boxed_slice(),
        );
        CarrierResolutionPlan::Ready(
            match self.execute_type_node(SemanticQueryKey::ProjectPath {
                base: head_node,
                path,
                context: ProjectionReductionContext::published(ProjectionMode::Navigate),
            }) {
                QueryResult::Value(SemanticQueryOutput { value: id, .. }) => id,
                _ => self.opaque(QueryError::Miss),
            },
        )
    }

    /// Resolve a carrier SUBJECT node to its real semantic node when `node` is a
    /// `BareRef` / `ImportType` / `TypeOf` carrier, returning the rewritten node
    /// id; returns `node` UNCHANGED for any non-carrier (or a re-entrant carrier
    /// already being normalized — the pre-memo cycle guard).
    ///
    /// This is the carrier-subject side of the ONE shared resolver. For a
    /// `BareRef` / `ImportType` carrier it rehydrates a value-side
    /// [`CarrierResolverContext`] from the carrier's own node-level scope (the
    /// body file) — an EMPTY `name_resolution` plus the scope's
    /// `DeclarationScopePayload` + `ScopeShadowing` derived from
    /// `prepared_decl_bundle`, exactly the shape
    /// [`Self::lower_type_expr_in_scope_with_context`] uses in production — then
    /// calls [`Self::resolve_bare_ref_head`] / [`Self::resolve_import_type_head`]
    /// in the query's `context`. A carrier node carries no body-file
    /// `name_resolution` map (that map is a lowering-time local), so this path
    /// recovers the resolution through the `resolve_bare_name_in_scope` fallback;
    /// the import-type head re-injects its own head binding.
    ///
    /// For a `TypeOf` carrier (`typeof value.path<args>`) it re-enters the shared
    /// dispatch through [`Self::typeof_key_for`] (-> `build_typeof`), projects the
    /// carrier's internal dotted path in `Navigate`, then applies the carrier's
    /// instantiation `type_args` — resolve -> project -> apply, the same chain
    /// the PathWalker's mid-walk `TypeOf` arm runs. This is what resolves an
    /// empty-path `typeof` macro payload, whose carrier never reaches the
    /// walker's per-segment arm; the `build_typeof` subquery records the
    /// value-root `FileWholeHash` / `ImportRoute` facts into the active tracer so
    /// they bubble into the outer component-meta result signature.
    pub(crate) fn resolve_carrier_subject_node(
        &self,
        node: SemanticNodeId,
        context: ProjectionReductionContext,
    ) -> SemanticNodeId {
        // ORDINARY (non-evaluator) callers (~19 walk / raise / enumerate /
        // graph-predicate / slot-binding / output-sink sites): a local
        // `BuildLocalTaint` observation frame + re-fold into the enclosing
        // frame, and NO nested non-cacheability tracer. Every nested read here
        // funnels through the shared read boundary into the local frame; these
        // callers always run inside an enclosing cold build whose OWN tracer a
        // direct fenced / lease-miss serve marks through the fan-out, so they
        // need no dedicated tracer (the ~19 keep their current cost).
        let observation =
            crate::project_semantic_dispatch::BuildLocalTaintGuard::push(&self.build_local_taint);
        let resolved = self.resolve_carrier_subject_node_inner(node, context);
        let observed = observation.finish();
        self.fold_into_top_build_local_taint(observed.result_is_partial, observed.cache_suppress);
        resolved
    }

    /// Suppress/partial-CAPTURING variant of
    /// [`Self::resolve_carrier_subject_node`] — the EVALUATOR-only entry.
    ///
    /// The head resolution performs NESTED reads — the `TypeOf` value-root +
    /// dotted-path `execute_read`s, the `BareRef` builtin `Instantiate`, and the
    /// `ImportType` qualified-path `ProjectPath` (`carrier.rs`
    /// `resolve_import_type_head`) — whose `CacheRead.cache_suppress` /
    /// `result_is_partial` the bare `resolve_carrier_subject_node` return type
    /// (a single node) cannot carry. Every one of those FUNNEL reads folds its
    /// rails into the TOP `build_local_taint` frame this variant pushes, making
    /// the "OR of every nested read" aggregation STRUCTURALLY true.
    ///
    /// But the head ALSO performs DIRECT serves (the `ensure_indexed_ready_serve`
    /// `resolves_to_file` probe, the bare-name resolver's indexed/export/prepared
    /// serves, frontier route reads) that mark the fan-out non-cacheability
    /// tracer, NOT the `CacheRead` funnel — and in Navigate/Skeleton/Shallow the
    /// head interns a `DeclRef` / `InstantiationRef` and returns with NO nested
    /// `execute_read`, so the local `build_local_taint` frame is EMPTY. Because
    /// the evaluator calls this DIRECTLY (not through the query-entry key
    /// normalization, which installs its own tracer), such a direct fenced /
    /// lease-miss serve would be invisible to the returned suppress the
    /// `evaluate_deferred_memo` publish gate consults — the cache-poison hole.
    /// This variant therefore installs a NESTED fact / non-cacheability tracer
    /// around the whole inner resolution and folds its bit (and any tracer
    /// overflow) into the returned suppress.
    ///
    /// It re-folds the merged observation into the ENCLOSING frame (idempotent
    /// OR) and RETURNS the aggregated
    /// [`BuildLocalTaint`](crate::project_semantic_dispatch::BuildLocalTaint) so
    /// the deferred-shell evaluator's `BareRef` / `ImportType` arm can OR the
    /// suppress / partial into its OWN [`EvaluateDeferredOutcome`] accumulators —
    /// the publish gate is a SEPARATE admission authority from any enclosing
    /// build frame. `cache_suppress` stays ORTHOGONAL to completeness: a fenced /
    /// non-cacheable but VALID head resolution keeps `result_is_partial = false`
    /// (Complete) and only refuses memo admission.
    pub(super) fn resolve_carrier_subject_node_capturing_suppress(
        &self,
        node: SemanticNodeId,
        context: ProjectionReductionContext,
    ) -> (
        SemanticNodeId,
        crate::project_semantic_dispatch::BuildLocalTaint,
    ) {
        let host = self.ctx.host_for_fact_tracer_install();
        let observation =
            crate::project_semantic_dispatch::BuildLocalTaintGuard::push(&self.build_local_taint);
        let (resolved, finalise) = crate::fact_signature_helpers::install_fact_tracer(host, || {
            self.resolve_carrier_subject_node_inner(node, context)
        });
        // Order is LOAD-BEARING: finish the local frame, OR the nested tracer's
        // non-cacheability AND its overflow into `observed.cache_suppress`, and
        // ONLY THEN re-fold the merged observation into the enclosing frame.
        // Re-folding the Rail-1-only frame before adding the tracer bits would
        // recreate the gap the nested tracer exists to close.
        let mut observed = observation.finish();
        if matches!(
            &finalise,
            crate::resolver_core::FactReadSetFinalise::NonCacheable(_)
        ) {
            observed.cache_suppress = true;
        }
        if matches!(
            finalise,
            crate::resolver_core::FactReadSetFinalise::Overflow
        ) {
            observed.cache_suppress = true;
        }
        self.fold_into_top_build_local_taint(observed.result_is_partial, observed.cache_suppress);
        (resolved, observed)
    }

    /// Carrier-subject head-resolution body (see
    /// [`Self::resolve_carrier_subject_node`] for the contract). Wrapped by
    /// [`Self::resolve_carrier_subject_node_capturing_suppress`] under an
    /// observation taint frame; never called directly except through those two
    /// entry points.
    fn resolve_carrier_subject_node_inner(
        &self,
        node: SemanticNodeId,
        context: ProjectionReductionContext,
    ) -> SemanticNodeId {
        let graph = self.graph();
        let Some(data) = graph.node_data(node) else {
            return node;
        };
        // Fast reject: only the three unresolved-reference carriers are
        // head-resolvable as a query subject.
        let is_bare = data.bare_ref_head().is_some();
        let is_import = data.import_type_head().is_some();
        let is_typeof = data.typeof_head().is_some();
        if !is_bare && !is_import && !is_typeof {
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

        // TypeOf carrier subject: `typeof value.path<args>`. The empty-path
        // entry points (a macro payload `defineProps<typeof config>()`, an
        // imported `typeof imported.value`) reach the PathWalker with an EMPTY
        // path, so the walker's per-segment `TypeOf` arm never fires — the
        // carrier must resolve HERE, at the query subject, exactly as the
        // walker's mid-walk arm does. This re-enters the ONE shared dispatch
        // (`typeof_key_for` -> `build_typeof`), which roots the value name
        // through the shared bare-name / import path and records its
        // `FileWholeHash` (resolved file) / `ImportRoute` (unresolved import)
        // facts into the active tracer so they bubble into the outer
        // component-meta result signature. NO side-band dep preflight: the
        // mirror stays query-free; resolution + dep recording happen at THIS
        // resolving demand. The carrier's internal dotted path projects in
        // `Navigate` (an intermediate hop) and the carrier's instantiation
        // `type_args` apply AFTER the projection — resolve -> project -> apply,
        // mirroring the walker's `TypeOf` arm.
        if is_typeof {
            let (value_root, typeof_path) = {
                let data = graph.node_data(node).expect("TypeOf carrier data");
                let (value_root, typeof_path) = data.typeof_head().expect("TypeOf head");
                (value_root.clone(), typeof_path.clone())
            };
            let type_args: Vec<SemanticNodeId> = {
                let data = graph.node_data(node).expect("TypeOf carrier data");
                data.carrier_type_args().to_vec()
            };
            // The typeof value-root resolves under the caller's reduction
            // context (the demand point); the carrier-internal dotted path is
            // an INTERMEDIATE hop and projects in `Navigate`. Use `execute_read`
            // (NOT `execute_type_node`) so a budget-exhausted / partial subquery
            // folds its `result_is_partial` + `cache_suppress` into the request
            // (`observe_component_meta_read_suppress`) — the no-poison rail: a
            // partial carrier resolution must not warm any enclosing cache.
            let typeof_key = self.typeof_key_for(value_root, context);
            let root_read = self.execute_read(typeof_key);
            crate::request_context::observe_component_meta_read_suppress(&root_read);
            let mut resolved = match root_read.value {
                QueryResult::Value(id) => id,
                _ => self.opaque(QueryError::Miss),
            };
            if resolved != node && !typeof_path.is_empty() {
                let projection_path: Arc<[PathSegment]> = Arc::from(
                    typeof_path
                        .iter()
                        .map(|seg| PathSegment::Member(Arc::clone(seg)))
                        .collect::<Vec<_>>()
                        .into_boxed_slice(),
                );
                let path_read = self.execute_read(SemanticQueryKey::ProjectPath {
                    base: resolved,
                    path: projection_path,
                    context: ProjectionReductionContext::published(ProjectionMode::Navigate),
                });
                crate::request_context::observe_component_meta_read_suppress(&path_read);
                resolved = match path_read.value {
                    QueryResult::Value(id) => id,
                    _ => self.opaque(QueryError::Miss),
                };
            }
            if resolved != node && !type_args.is_empty() {
                resolved = self.apply_typeof_instantiation_args(resolved, &type_args);
            }
            self.carrier_normalizing.borrow_mut().pop();
            return resolved;
        }

        // The carrier's already-lowered args (read through the SOLE sanctioned
        // descent accessor) and node-level scope (the owner/body file).
        let type_args: Arc<[SemanticNodeId]> = {
            let data = graph.node_data(node).expect("carrier node data");
            Arc::from(data.carrier_type_args().to_vec().into_boxed_slice())
        };
        let owner_canonical = graph.node_scope(node).and_then(|s| s.canonical_file());

        // Rehydrate the value-side resolution inputs from the carrier's scope.
        let env: FxHashMap<String, SemanticNodeId> = FxHashMap::default();
        let name_resolution: FxHashMap<std::sync::Arc<str>, ResolvedRootIdentity> =
            FxHashMap::default();
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
                // A `TypeOf` carrier subject with a NON-EMPTY path is left for the
                // PathWalker's per-segment `TypeOf` arm, which runs the path-precise
                // resolve -> project (Navigate intermediate hop) -> apply chain.
                // Pre-resolving it here would over-resolve the internal carrier path
                // under the caller's outer mode and bypass the walker's
                // intermediate-hop demotion. The EMPTY-path case (a `typeof config`
                // macro payload) has no walker segment to fire the arm, so it MUST
                // resolve here. `BareRef` / `ImportType` subjects normalize in both
                // cases (the walker only re-enters this same helper for them).
                let base = if !path.is_empty()
                    && self
                        .graph()
                        .node_data(base)
                        .is_some_and(|d| d.typeof_head().is_some())
                {
                    base
                } else {
                    self.resolve_carrier_subject_node(base, context)
                };
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
            SemanticQueryKey::ClassifyBroadRuntime { subject, context } => {
                let subject = self.resolve_carrier_subject_node(
                    subject,
                    ProjectionReductionContext::structural_transit_with_mode(
                        ProjectionMode::Navigate,
                    ),
                );
                SemanticQueryKey::ClassifyBroadRuntime { subject, context }
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
            SemanticQueryKey::ClassifyBroadRuntime { subject, .. } => *subject,
            _ => return false,
        };
        let graph = self.graph();
        let Some(data) = graph.node_data(subject) else {
            return false;
        };
        data.bare_ref_head().is_some()
            || data.import_type_head().is_some()
            || data.typeof_head().is_some()
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
        let mut name_resolution: FxHashMap<std::sync::Arc<str>, ResolvedRootIdentity> =
            FxHashMap::default();
        name_resolution.insert(
            Arc::from("Foo"),
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
                .map(|r| r.symbol_name.as_ref()),
            Some("Foo")
        );
        assert!(ctx.scope_payload().is_none());
        // The shadow set is the empty set here (no userland shadow).
        let _ = ctx.shadowing();
        assert_eq!(ctx.mode(), ProjectionMode::Navigate);
        assert_eq!(ctx.reduction_context().mode, ProjectionMode::Navigate);
    }
}
