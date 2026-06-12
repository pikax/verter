//! `shallow_lower_type_expr` — TypeExpr → SemanticNodeId shallow lowering
//!
//! Produces the first structural layer of the semantic graph from a
//! parsed [`TypeExpr`] tree. Deeper expansion is the caller's
//! responsibility via [`SemanticQueryKey::ProjectPath`] sub-queries —
//! this pass stays one member / arm / sub-expression deep so the
//! published shell identity is stable across entry paths.
//!
//! **Authority contract:** this is the *only* TypeExpr
//! lowering path in the workspace. The §6.5 invariant test
//! `type_expr_lowering_has_exactly_one_path` asserts exactly one
//! `fn shallow_lower_type_expr_with_context` exists in `crates/`
//! (and no bare-`mode` wrapper beside it — every caller states its
//! full [`ProjectionReductionContext`] demand explicitly).

use std::sync::Arc;

use rustc_hash::FxHashMap;
use verter_semantic::analysis::type_solver::host::ResolvedRootIdentity;
use verter_type_expr::{ObjectMember, TypeExpr};

use super::{map_primitive_name, ProjectSemanticDispatch};
use crate::resolver_core::bare_name_resolve::{
    resolve_bare_name_in_scope, DeclarationScopePayload,
};
use crate::resolver_core::scope_shadowing::ScopeShadowing;
use crate::semantic_query::{
    DeclIdentity, HashValue, IndexSignature, NodeScopeId, PathSegment, PrimitiveKind,
    ProjectionMode, ProjectionReductionContext, QueryError, QueryResult, ResolveDeclKey, ScopeId,
    SemanticNodeData, SemanticNodeId, SemanticQueryApi, SemanticQueryKey, SemanticQueryOutput,
    SurfaceMember, SurfaceView, TupleElement, ValueRootKey,
};

impl<'a> ProjectSemanticDispatch<'a> {
    /// Shallow-lower a [`TypeExpr`] under `env` (type-parameter bindings)
    /// into a [`SemanticNodeId`]. "Shallow" means one structural level:
    /// object members, union/intersection arms, and function / conditional
    /// sub-expressions are interned as references rather than recursively
    /// expanded. Deeper lowering is the caller's responsibility via
    /// [`SemanticQueryKey::ProjectPath`] sub-queries.
    ///
    /// Accepts the full [`ProjectionReductionContext`] so callers thread
    /// their demand through nested lowering: a `StructuralTransit`
    /// instantiation lowers its body with the same demand and nested
    /// operator dispatches carrier-stop.
    ///
    /// `name_resolution` is the prepared decl's bare-name → canonical
    /// identity map; used by the walker to resolve `TypeExpr::Ref`
    /// hops through `ResolveDecl` or nested `Instantiate` sub-shells
    /// via `SemanticQueryApi::execute`.
    ///
    /// `scope_payload` carries the owning file's declaration-scope
    /// payload (script-setup type bindings, scope-local type/value
    /// names, import bindings). It is consulted when the bare `Ref`
    /// name is NOT in `name_resolution` — the walker falls through to
    /// [`resolve_bare_name_in_scope`] which looks at host-owned
    /// `shallow_file_state` + prepared-decl bundle + export-target
    /// resolvers ( — dispatch carries full
    /// name-resolution context without routing through
    /// `SessionSolverHost`).
    ///
    /// `shadowing` carries the scope-shadowing decision once per
    /// resolver context. The dispatch
    /// fast-path consults `shadowing.is_shadowing_lib(name)` before
    /// routing a bare `Ref` through the ambient-lib `__builtin__`
    /// path — when `true`, the userland declaration wins via the
    /// standard `ResolveDecl` route. The struct (rather than a bare
    /// `bool`) keeps the threading axis single-source-of-truth so the
    /// `ResolverContext` absorbs the field without inventing a
    /// parallel axis to undo. Constructible via
    /// [`ScopeShadowing::from_scope_payload`] (dispatch path) or
    /// [`ScopeShadowing::from_host_scope`] (materialise path).
    ///
    /// `substitutions` accumulates `(param_name, arg_id)` facts for
    /// `SubstituteTypeParam` origin-edge emission at the shell level.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn shallow_lower_type_expr_with_context(
        &self,
        expr: &TypeExpr,
        env: &FxHashMap<String, SemanticNodeId>,
        scope: &NodeScopeId,
        name_resolution: &FxHashMap<String, ResolvedRootIdentity>,
        scope_payload: Option<&DeclarationScopePayload>,
        shadowing: &ScopeShadowing,
        substitutions: &mut Vec<(Arc<str>, SemanticNodeId)>,
        reduction_context: ProjectionReductionContext,
    ) -> SemanticNodeId {
        let mode = reduction_context.mode;
        // Watchdog hooks for hang investigation. Both calls are inert
        // when the watchdog has not been spawned (single relaxed atomic
        // load + early return). When active, they advance a heartbeat
        // counter and respond to the watchdog's stall signal by
        // printing a self-backtrace from inside this recursion.
        // See `loop5_instrumentation.rs` watchdog module.
        crate::loop5_instrumentation::watchdog_beat();
        crate::loop5_instrumentation::watchdog_check_and_dump("shallow_lower_type_expr");
        let graph = self.graph();
        graph.record_decl_subexpression_lowering();
        match expr {
            TypeExpr::Primitive(name) => graph.intern_node_with_scope(
                SemanticNodeData::Primitive(map_primitive_name(*name)),
                scope.clone(),
            ),
            TypeExpr::Literal(value) => graph
                .intern_node_with_scope(SemanticNodeData::Literal(value.clone()), scope.clone()),
            TypeExpr::TypeParameter(param) => {
                if let Some(arg_id) = env.get(&param.name) {
                    substitutions.push((Arc::from(param.name.as_str()), *arg_id));
                    *arg_id
                } else {
                    // Unbound parameter — intern with lowered
                    // constraint / default so the projection back to
                    // `TypeExpr::TypeParameter(TypeParam { name,
                    // constraint, default })` is complete.
                    let constraint = param.constraint.as_ref().map(|c| {
                        self.shallow_lower_type_expr_with_context(
                            c,
                            env,
                            scope,
                            name_resolution,
                            scope_payload,
                            shadowing,
                            substitutions,
                            reduction_context,
                        )
                    });
                    let default = param.default.as_ref().map(|d| {
                        self.shallow_lower_type_expr_with_context(
                            d,
                            env,
                            scope,
                            name_resolution,
                            scope_payload,
                            shadowing,
                            substitutions,
                            reduction_context,
                        )
                    });
                    let display_name: Arc<str> = Arc::from(param.name.as_str());
                    // Unresolved `TypeParameter` path uses
                    // **file-scoped name-keyed identity**:
                    // `decl_name = reference.name` (NOT the owning
                    // declaration's name, which is unavailable at
                    // this site because the parameter could not be
                    // resolved). Two unresolved `K` references
                    // anywhere in the same file alias to one
                    // `SemanticNodeId`; cross-file unresolved `K`
                    // references stay distinct via `canonical_id`.
                    // `param_index = 0` is the file-scoped name-keyed
                    // identity slot; the escalation path if this
                    // proves too coarse is an owner-scope-local
                    // `(name → ordinal)` map.
                    let decl = crate::semantic_query::DeclIdentity::from_scope(
                        scope,
                        Arc::clone(&display_name),
                    );
                    graph.intern_node_with_scope(
                        SemanticNodeData::TypeParam {
                            decl,
                            param_index: 0,
                            constraint,
                            default,
                            display_name,
                        },
                        scope.clone(),
                    )
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
            // Script-setup generic parameter. When the bare name maps
            // to a `script_setup_type_bindings` entry, lower directly
            // to a rich
            // `SemanticNodeData::TypeParam { name, constraint, default }`
            // — NOT via the `ResolveDecl` fallback. This preserves
            // declaration-site constraint/default so the projection
            // back to `TypeExpr::TypeParameter(TypeParam)` is complete
            // at meta-extraction time. Must match on
            // `scope_type_bindings` specifically (the script-setup
            // map), not `scope_type_names` which also contains
            // same-file type decls.
            //
            // The binding store is
            // [`crate::resolver_core::prepared_decl::TypeParamBinding`],
            // which carries the unlowered constraint / default
            // expressions directly — this arm reads them without an
            // intermediate `PreparedTypeDecl` wrapper.
            TypeExpr::Ref {
                name,
                type_arguments,
            } if type_arguments.is_empty()
                && scope_payload
                    .map(|payload| payload.scope_type_bindings.contains_key(name.as_ref()))
                    .unwrap_or(false) =>
            {
                let binding = scope_payload
                    .and_then(|payload| payload.scope_type_bindings.get(name.as_ref()))
                    .expect("matched on scope_type_bindings.contains_key above");
                let constraint = binding.constraint.as_ref().map(|c| {
                    self.shallow_lower_type_expr_with_context(
                        c,
                        env,
                        scope,
                        name_resolution,
                        scope_payload,
                        shadowing,
                        substitutions,
                        reduction_context,
                    )
                });
                let default = binding.default.as_ref().map(|d| {
                    self.shallow_lower_type_expr_with_context(
                        d,
                        env,
                        scope,
                        name_resolution,
                        scope_payload,
                        shadowing,
                        substitutions,
                        reduction_context,
                    )
                });
                let display_name = Arc::clone(&binding.name);
                // Script-setup type parameters get a
                // `decl_name = "<script-setup>"` sentinel with the
                // file's `canonical_id` + `whole_hash` taken from the
                // current lowering scope. `param_index` is the
                // binder's 0-based position in the
                // `<script setup generic="...">` clause (carried on
                // `TypeParamBinding.ordinal`), disambiguating
                // multiple script-setup parameters in the same file.
                let decl = match scope {
                    NodeScopeId::Global => DeclIdentity {
                        canonical_id: Arc::from(""),
                        whole_hash: HashValue::default(),
                        decl_name: Arc::from("<script-setup>"),
                    },
                    NodeScopeId::File {
                        canonical_id,
                        whole_hash,
                        ..
                    } => DeclIdentity {
                        canonical_id: Arc::clone(canonical_id),
                        whole_hash: *whole_hash,
                        decl_name: Arc::from("<script-setup>"),
                    },
                };
                graph.intern_node_with_scope(
                    SemanticNodeData::TypeParam {
                        decl,
                        param_index: binding.ordinal,
                        constraint,
                        default,
                        display_name,
                    },
                    scope.clone(),
                )
            }
            // Named type reference (`type Foo<T> = { y: Other<T> }` →
            // `Other<T>` at `y`'s position). Resolve through
            // dispatch:
            //   - 0-arg refs → execute(ResolveDecl(...)).
            //   - n-arg refs → execute(ResolveDecl(...)) then
            //     execute(Instantiate(decl_identity, lowered_args)) →
            //     a sub-Instantiate shell. Self-referential types are
            //     bounded by the memo's same-path recursion sentinel.
            TypeExpr::Ref {
                name,
                type_arguments,
            } => {
                // Built-in utility fast path: recognised names (`Partial`,
                // `Pick`, `ReturnType`, string intrinsics, etc.) that are
                // NOT in `name_resolution` (they are global TS builtins,
                // not imported or locally declared) construct a
                // `DeclIdentity` with the sentinel canonical id
                // `"__builtin__"` and dispatch through
                // [`SemanticQueryKey::Instantiate`]. The builder
                // recognises the name via
                // [`DispatchHost::utility_source`] → `Builtin` and hands
                // off to [`Self::build_builtin_utility`].
                //
                // Userland types that shadow a builtin name (e.g.
                // `type Partial<T> = ...` in the user's scope) live in
                // `name_resolution` OR in the
                // [`ScopeShadowing`](ScopeShadowing) context built once
                // per resolver context from the owner scope's
                // `scope_type_names` + `scope_type_bindings` (
                // §5.10 r15/F11 — the second source covers callers
                // that lower with an empty `name_resolution` map but
                // still hand a populated `scope_payload`, e.g.
                // [`Self::lower_type_expr_in_scope_with_mode`]). In
                // both cases the builtin fast-path is suppressed and
                // the bare-name walk below resolves the userland
                // alias via the standard `ResolveDecl` path —
                // preserving the "user shadowing wins" rule across
                // BOTH lowering entry points (the materialise path's
                // `extract_route_root_identity_node` callers consume
                // the same `ScopeShadowing` value).
                // Global lib-type fast path: an unshadowed `Promise<...>`
                // reference interns a nominal `InstantiationRef` carrier in
                // EVERY mode. `Promise` has no structural reducer arm — the
                // carrier preserves the declaration identity + type
                // arguments so demand points (the `Awaited` reducer arm, the
                // raise layer's `Ref { name, args }` projection) can consume
                // them. Classification is the registry lookup on the
                // declaration name (`IntrinsicRegistry` → `PromiseGlobal`),
                // not a resolver-local name match; userland shadowing wins
                // via the same `name_resolution` / `ScopeShadowing` gates the
                // builtin utilities use. Without the carrier the bare-name
                // walk below misses (no lib file backs the global) and the
                // type arguments are erased into `Opaque(Miss)`.
                if !name_resolution.contains_key(name.as_ref())
                    && !shadowing.is_shadowing_lib(name.as_ref())
                    && self.is_promise_global_name(name.as_ref())
                {
                    let arg_ids: Vec<SemanticNodeId> = type_arguments
                        .iter()
                        .map(|arg| {
                            self.shallow_lower_type_expr_with_context(
                                arg,
                                env,
                                scope,
                                name_resolution,
                                scope_payload,
                                shadowing,
                                substitutions,
                                reduction_context,
                            )
                        })
                        .collect();
                    return graph.intern_node_with_scope(
                        SemanticNodeData::InstantiationRef {
                            base: DeclIdentity {
                                canonical_id: Arc::from("__builtin__"),
                                whole_hash: HashValue::default(),
                                decl_name: Arc::clone(name),
                            },
                            args: Arc::from(arg_ids.into_boxed_slice()),
                        },
                        scope.clone(),
                    );
                }

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
                    let arg_ids: Vec<SemanticNodeId> = type_arguments
                        .iter()
                        .map(|arg| {
                            self.shallow_lower_type_expr_with_context(
                                arg,
                                env,
                                scope,
                                name_resolution,
                                scope_payload,
                                shadowing,
                                substitutions,
                                reduction_context,
                            )
                        })
                        .collect();

                    // Builtin carrier gate (LOWERING entrance). The
                    // `InstantiationRef` shell is preserved WHEN any of:
                    //   - the lowering mode is `Shallow` — for ALL builtins.
                    //     Shallow decl-body lowering is carrier-preserving
                    //     exactly like Navigate / Skeleton: member-value
                    //     utilities (`Partial<Col<T>>`, `Omit<…>`, …) stay
                    //     carriers and materialise only at a demand point
                    //     (the shallow-surface synthesiser's carrier unwrap,
                    //     PathWalker hops, closed object-filter surface
                    //     reads). Eager execution here compounds
                    //     member-value instantiation recursion across large
                    //     transitive decl graphs (the expansion-storm
                    //     class).
                    //   - the mode is `Navigate` and the name is an
                    //     object-filter builtin (`Pick`/`Omit` — the
                    //     reducer's L1 decides closed→materialise
                    //     downstream, and the materialiser's registry-route
                    //     guard can apply cycle / package gates on the
                    //     wrapped root identity BEFORE dispatch's
                    //     `build_builtin_utility` projects), OR
                    //   - the name is an object-filter builtin and the
                    //     enumeration domain (lowered argument 0) is OPEN —
                    //     in ANY mode. An open `Pick`/`Omit` must never
                    //     build the `Instantiate` query that would
                    //     materialise the open source (the ChatMessages.vue
                    //     `Pick<PropsBase<T>, …>` storm class).
                    // A CLOSED object-filter domain in `Expanded`/`Identity`
                    // falls through to execute the `Instantiate` query and
                    // materialise path-precisely; non-object-filter builtins
                    // keep the eager-resolve path in those modes only.
                    // Family membership is decided by the ONE registry
                    // helper (`raise::is_l1_object_filter_utility`, backed
                    // by the `BuiltinUtility` registry — never a local name
                    // string match) and the shared open-domain predicate
                    // (`raise::utility_enumeration_domain_is_open_or_unknown`)
                    // is reused — no second walker.
                    // Carrier-mode open-argument rule: under `Navigate` /
                    // `Skeleton` a builtin over an OPEN argument (an
                    // unbound `TypeParam` — including a mapper binder
                    // substituted later at a demand point — or an open
                    // carrier over one) interns the carrier too. Eagerly
                    // executing `NonNullable<ChatSlots[K]>` with unbound
                    // `K` bakes `Opaque(Miss)` into the produced
                    // conditional check and destroys the structure the
                    // per-key realization demand point needs.
                    // Closed-argument builtins under Navigate / Skeleton
                    // keep the eager execute, byte-for-byte.
                    let build_carrier = mode == ProjectionMode::Shallow
                        || (crate::project_semantic_dispatch::raise::is_l1_object_filter_utility(
                            name.as_ref(),
                        ) && (mode == ProjectionMode::Navigate
                            || crate::project_semantic_dispatch::raise::
                                utility_enumeration_domain_is_open_or_unknown(
                                    self.ctx,
                                    &builtin_identity,
                                    &arg_ids,
                                )))
                        || (matches!(
                            mode,
                            ProjectionMode::Navigate | ProjectionMode::Skeleton
                        ) && arg_ids.iter().any(|arg| {
                            crate::project_semantic_dispatch::raise::
                                builtin_lowering_argument_is_open(self.ctx, *arg)
                        }));
                    if build_carrier {
                        return graph.intern_node_with_scope(
                            SemanticNodeData::InstantiationRef {
                                base: builtin_identity,
                                args: Arc::from(arg_ids.into_boxed_slice()),
                            },
                            scope.clone(),
                        );
                    }
                    return match self.execute_type_node(SemanticQueryKey::Instantiate {
                        base: self.type_slot_for(
                            Arc::clone(&builtin_identity.canonical_id),
                            Arc::clone(&builtin_identity.decl_name),
                        ),
                        args: Arc::from(arg_ids.into_boxed_slice()),
                        context: self.instantiate_context_for(
                            &builtin_identity.canonical_id,
                            reduction_context,
                        ),
                    }) {
                        QueryResult::Value(SemanticQueryOutput { value: id, .. }) => id,
                        _ => self.opaque(QueryError::Miss),
                    };
                }

                // Primary: the prepared decl's name_resolution map
                // carries already-resolved imports from the body-file
                // scope. This is the fast path for names that the
                // prepared-decl builder has already walked.
                //
                // Fallback: when the name is NOT in name_resolution
                // (common for bare types in re-exported declarations or
                // helpers that cross a package boundary but aren't
                // cached by the body-file's prepared-decl scope), fall
                // through to the host-owned bare-name resolver. It
                // consults the declaration-scope payload + the host's
                // `shallow_file_state` import_targets / exports maps +
                // `resolve_named_type_export_target`, the same substrate
                // `SessionSolverHost::root_identity` wraps (
                // step 3 — dispatch no longer routes through
                // `SessionSolverHost`).
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
                // Macro Type Traversal Rule (CRITICAL) — unresolved
                // imports short-circuit to `Opaque(QueryError::Miss)`.
                // When a bare type name cannot be resolved through
                // EITHER the prepared-decl `name_resolution` map NOR
                // the bare-name resolver fall-through chain in
                // `resolver_core::bare_name_resolve` (shallow facts →
                // scope payload → prepared-decl bundle), the lowering
                // emits an opaque sentinel. **No synthetic placeholder
                // root is invented for the unresolved specifier.**
                // Downstream projection observes the sentinel,
                // publishes a partial result for the field whose type
                // transitively depended on the unresolved name, and
                // other fields continue to resolve normally — the
                // component-meta payload stays well-formed.
                //
                // This is the explicit Macro Type Traversal contract
                // from `CLAUDE.md`: only follow the import graph
                // reachable from the requested type's declaration
                // graph; never treat plain imports as implicit exports
                // or synthesise external roots for absent specifiers.
                // Cross-file EXTERNAL string-literal module augmentation (the
                // canonical Vue/Vite `vite/client` pattern): `name` is imported
                // from a bare specifier that resolves to NO workspace file —
                // either `resolve_bare_name_in_scope` returned `None`, or the
                // pre-resolved `name_resolution` map / bare-name walk resolved it
                // to the empty non-file canonical. In both cases there is no
                // file-scope declaration, but `declare module "<spec>"` blocks
                // across files form an ambient module whose peers merge. Build
                // the peer-merged `MergedDecl` PURELY from the
                // `ExternalSpecifier(spec)` augmentation index (the SAME
                // augmenter-fold path + carrier as the relative stitch). Falls
                // through to the existing behaviour when no `declare module`
                // block contributes.
                // A resolution "reaches a real declaration" only when its
                // canonical names a LOADABLE workspace file. An unresolved bare
                // specifier resolves to the empty non-file canonical OR to the
                // specifier string itself (e.g. `"external-spec"`), neither of
                // which `ensure_indexed_ready_serve` can load — that is the external
                // ambient-module case the augmentation hook below handles.
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
                    return self.opaque(QueryError::Miss);
                };
                // Recursive-ref guard: if the resolved root is
                // currently being materialised by an enclosing
                // `build_instantiate` frame, emit `Opaque(RecursiveRef)`
                // as the back-edge. This handles `type T = { kids: T[] }`
                // — the inner `Ref{T}` short-circuits to a back-edge
                // instead of recursing into another Instantiate. The
                // dispatcher-local `instantiate_active` stack records the
                // (canonical, name) identity before body lowering; see
                // `build_instantiate` for the push / pop wiring.
                if type_arguments.is_empty()
                    && self
                        .is_instantiate_active(resolved_canonical.as_ref(), resolved_name.as_ref())
                {
                    return self.opaque(QueryError::RecursiveRef {
                        name: Arc::clone(&resolved_name),
                    });
                }
                let resolved_canonical_clone = Arc::clone(&resolved_canonical);
                let resolved_name_clone = Arc::clone(&resolved_name);

                // Lazy carriers: in `Navigate` mode, intern a
                // `DeclRef` / `InstantiationRef` carrier rather than
                // executing `ResolveDecl` / `Instantiate` eagerly. The
                // interned carrier node carries the versioned `DeclIdentity`
                // (canonical_id + whole_hash + decl_name) as its value-side
                // payload — this is the node identity, not a query-identity
                // key. (The eager `Instantiate` path keys instead on the
                // env-bearing, content-free `ResolvedDeclSlotIdentity` slot
                // derived via `type_slot_for` + `InstantiateContext`.) The
                // walker treats `DeclRef` as transparent through alias
                // chains and `InstantiationRef` as terminal.
                let whole_hash = self
                    .ctx
                    .shallow_file_state(resolved_canonical_clone.as_ref())
                    .map_or(HashValue::default(), |s| s.whole_hash);
                let decl_identity = DeclIdentity {
                    canonical_id: Arc::clone(&resolved_canonical_clone),
                    whole_hash,
                    decl_name: Arc::clone(&resolved_name_clone),
                };
                if matches!(
                    mode,
                    ProjectionMode::Navigate | ProjectionMode::Skeleton | ProjectionMode::Shallow
                ) {
                    // Skeleton mode preserves carriers
                    // (like Navigate) so the cycle-BFS can see recursive refs
                    // as DeclRef/InstantiationRef in the lowered graph rather
                    // than collapsed Opaque(RecursiveRef) sentinels. Without
                    // this, body lowering of DotPathKeys's recursive
                    // `DotPathKeys<NonNullable<T[K]>>` arm would go eager,
                    // hit the instantiate_active guard, and emit
                    // Opaque(RecursiveRef) — which collect_ref_identities_node
                    // doesn't walk as a DeclRef/InstantiationRef. The
                    // carrier-preservation makes the recursive identity
                    // visible to the graph walk.
                    //
                    // Shallow mode preserves carriers for the same reason
                    // Navigate does: decl-body lowering collects and
                    // indexes, it never eagerly evaluates. Member-value
                    // generic refs intern `DeclRef` / `InstantiationRef`
                    // shells; the shallow-surface synthesiser, PathWalker
                    // hops, and the relation/conditional oracle are the
                    // demand points that materialise them. Eager
                    // `ResolveDecl` / `Instantiate` at lowering time is
                    // `Expanded` / `Identity` only — an eager Shallow path
                    // is the member-value recursion storm class.
                    if type_arguments.is_empty() {
                        return graph.intern_node_with_scope(
                            SemanticNodeData::DeclRef {
                                identity: decl_identity,
                            },
                            scope.clone(),
                        );
                    } else {
                        // Type arguments are NOT the macro-T own body
                        // (by design): `defineProps<Foo<Bar>>()`
                        // has `Bar` as a generic argument substituted into
                        // `Foo`'s body — `Foo`'s own members are own-body,
                        // `Bar`'s members are not. Lower args structurally.
                        let arg_context = reduction_context.into_structural_provenance();
                        let arg_ids: Vec<SemanticNodeId> = type_arguments
                            .iter()
                            .map(|arg| {
                                self.shallow_lower_type_expr_with_context(
                                    arg,
                                    env,
                                    scope,
                                    name_resolution,
                                    scope_payload,
                                    shadowing,
                                    substitutions,
                                    arg_context,
                                )
                            })
                            .collect();
                        return graph.intern_node_with_scope(
                            SemanticNodeData::InstantiationRef {
                                base: decl_identity,
                                args: Arc::from(arg_ids.into_boxed_slice()),
                            },
                            scope.clone(),
                        );
                    }
                }

                let anchor =
                    match self.execute_type_node(SemanticQueryKey::ResolveDecl(ResolveDeclKey {
                        scope: ScopeId {
                            canonical_id: resolved_canonical,
                            local_scope: None,
                        },
                        name: resolved_name,
                    })) {
                        QueryResult::Value(SemanticQueryOutput { value: id, .. }) => id,
                        _ => return self.opaque(QueryError::Miss),
                    };
                // Route through Instantiate when the caller supplied
                // type arguments OR when the decl has type parameters
                // (defaults must apply). Non-generic declarations with
                // no args short-circuit to the bare ResolveDecl result.
                let decl_routes_through_instantiate = self
                    .ctx
                    .prepared_type_decl(
                        resolved_canonical_clone.as_ref(),
                        resolved_name_clone.as_ref(),
                    )
                    .is_some_and(|prepared| !prepared.type_parameters.is_empty());
                if type_arguments.is_empty() && !decl_routes_through_instantiate {
                    anchor
                } else {
                    // Type arguments lower structurally — they are
                    // substituted INTO the decl body, never the macro-T
                    // own body themselves (by design). The
                    // `Instantiate` itself keeps the caller's provenance:
                    // `build_instantiate` stamps the (non-utility) decl's
                    // OWN-body members with it and downgrades to
                    // structural for builtin-utility targets
                    // (`Omit`/`Pick` sources are never own-body).
                    let arg_context = reduction_context.into_structural_provenance();
                    let arg_ids: Vec<SemanticNodeId> = type_arguments
                        .iter()
                        .map(|arg| {
                            self.shallow_lower_type_expr_with_context(
                                arg,
                                env,
                                scope,
                                name_resolution,
                                scope_payload,
                                shadowing,
                                substitutions,
                                arg_context,
                            )
                        })
                        .collect();
                    match self.execute_type_node(SemanticQueryKey::Instantiate {
                        base: self.type_slot_for(
                            Arc::clone(&decl_identity.canonical_id),
                            Arc::clone(&decl_identity.decl_name),
                        ),
                        args: Arc::from(arg_ids.into_boxed_slice()),
                        context: self.instantiate_context_for(
                            &decl_identity.canonical_id,
                            reduction_context,
                        ),
                    }) {
                        QueryResult::Value(SemanticQueryOutput { value: id, .. }) => id,
                        _ => self.opaque(QueryError::Miss),
                    }
                }
            }
            TypeExpr::Union(arms) => {
                let mut arm_ids: Vec<SemanticNodeId> = Vec::with_capacity(arms.len());
                for arm in arms.iter() {
                    arm_ids.push(self.shallow_lower_type_expr_with_context(
                        arm,
                        env,
                        scope,
                        name_resolution,
                        scope_payload,
                        shadowing,
                        substitutions,
                        reduction_context,
                    ));
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
                    arm_ids.push(self.shallow_lower_type_expr_with_context(
                        arm,
                        env,
                        scope,
                        name_resolution,
                        scope_payload,
                        shadowing,
                        substitutions,
                        reduction_context,
                    ));
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
                            // Member VALUE lowering downgrades to
                            // structural provenance (Stage
                            // 1): a nested object inside this member's
                            // type (`{ outer: { inner: T } }`) is NOT the
                            // macro-T own body — only THIS object's
                            // direct members are. Stamping the value with
                            // macro provenance would mis-mark `inner`.
                            let value = self.shallow_lower_type_expr_with_context(
                                &prop.ty,
                                env,
                                scope,
                                name_resolution,
                                scope_payload,
                                shadowing,
                                substitutions,
                                reduction_context.into_structural_provenance(),
                            );
                            // `declared_in_macro_type_arg` reflects the
                            // surface-provenance context: when this object
                            // is lowered directly at the macro
                            // type-argument's own body (an inline
                            // `defineProps<{ a: string }>()` literal, the
                            // directly-referenced declaration's own body
                            // via `build_instantiate`, or an explicit
                            // Object arm of an intersection literal) the
                            // member is author-declared in the macro T.
                            // Otherwise (`Structural`) it is `false`.
                            // This is the canonical typed-IR producer of
                            // the bit; the parser-side `ResolvedProp`
                            // chain and the prepared-surface walker are
                            // the other producers the shared resolver
                            // consolidates onto this path.
                            members.push(SurfaceMember {
                                name: Arc::from(prop.name.as_str()),
                                value,
                                optional: prop.optional,
                                readonly: prop.readonly,
                                is_method: false,
                                // Carry the IR member's declared accessibility
                                // verbatim onto the graph payload (Public for
                                // every non-class origin).
                                visibility: prop.visibility,
                                // Carry the IR member's OXC declaration-site
                                // spans verbatim onto the graph payload.
                                spans: prop.spans,
                                // The member's DECLARATION lives in THIS object's
                                // lowering file — independent of where its value
                                // type resolves (an unresolved `MissingType` value
                                // is scope-less but the member still declares here).
                                declaration_origin: scope.canonical_file(),
                                declared_in_macro_type_arg: reduction_context
                                    .is_macro_type_arg_own_body(),
                                // Leaf stamping of the surface-merge role from
                                // the threaded context (by design):
                                // an interface/class own `Object` arm flows
                                // `OwnBody`, a heritage reference arm flows
                                // `Heritage`, everything else stays `Authored`.
                                merge_role: reduction_context.merge_role(),
                            });
                        }
                        ObjectMember::Method(method) => {
                            // Mapped+conditional infer closure: lower
                            // methods to canonical Function nodes (matching
                            // CallSignature handling below) so
                            // `PricingPlanSlots[K]` IndexedAccess can
                            // resolve to a real Function for the
                            // Function-extends infer-binding arm. An
                            // `Opaque(Miss)` placeholder here would break
                            // `IndexedAccess<I, "method-name">`
                            // projection: the path walker finds the
                            // member but its value is opaque, so the
                            // downstream `let Some(Function...) =
                            // graph.node_data(check_resolved)` match
                            // fails and the conditional drops to a
                            // deferred shell.
                            let function_expr =
                                TypeExpr::Function(Arc::new(method.function.clone()));
                            // Method VALUE (its function shape) lowers
                            // structurally — see the `ObjectMember::Property`
                            // companion note. Only the method's presence on
                            // THIS object is macro-T own-body, not the
                            // function's nested parameter/return objects.
                            let value = self.shallow_lower_type_expr_with_context(
                                &function_expr,
                                env,
                                scope,
                                name_resolution,
                                scope_payload,
                                shadowing,
                                substitutions,
                                reduction_context.into_structural_provenance(),
                            );
                            // `declared_in_macro_type_arg` mirrors the
                            // `ObjectMember::Property` arm: a method
                            // literally written in the macro type
                            // argument's own body is author-declared.
                            members.push(SurfaceMember {
                                name: Arc::from(method.name.as_str()),
                                value,
                                optional: method.optional,
                                readonly: false,
                                is_method: true,
                                // Carry the IR method's declared accessibility
                                // (Public for every non-class origin).
                                visibility: method.visibility,
                                // Carry the IR method's OXC member spans.
                                spans: method.spans,
                                // Declaration file of THIS method (see the
                                // `Property` companion note).
                                declaration_origin: scope.canonical_file(),
                                declared_in_macro_type_arg: reduction_context
                                    .is_macro_type_arg_own_body(),
                                // Leaf stamping of the surface-merge role —
                                // mirrors the `Property` arm.
                                merge_role: reduction_context.merge_role(),
                            });
                        }
                        ObjectMember::CallSignature(func) => {
                            // Lower call signatures as canonical `Function`
                            // nodes so utility dispatch (`ReturnType`,
                            // `Parameters`, `InstanceType`,
                            // `ConstructorParameters`, `Awaited`) can
                            // inspect parameter / return structure at the
                            // graph level instead of falling back to an
                            // opaque miss. The reverse mapping
                            // `raise_node_to_type_expr` reconstitutes
                            // `ObjectMember::CallSignature` entries from
                            // `SurfaceView.call_signatures` by matching
                            // `TypeExpr::Function(...)`.
                            let function_expr = TypeExpr::Function(Arc::new(func.clone()));
                            let fn_id = self.shallow_lower_type_expr_with_context(
                                &function_expr,
                                env,
                                scope,
                                name_resolution,
                                scope_payload,
                                shadowing,
                                substitutions,
                                reduction_context,
                            );
                            call_signatures.push(fn_id);
                        }
                        ObjectMember::ConstructSignature(func) => {
                            let function_expr = TypeExpr::Function(Arc::new(func.clone()));
                            let fn_id = self.shallow_lower_type_expr_with_context(
                                &function_expr,
                                env,
                                scope,
                                name_resolution,
                                scope_payload,
                                shadowing,
                                substitutions,
                                reduction_context,
                            );
                            construct_signatures.push(fn_id);
                        }
                        ObjectMember::IndexSignature(sig) => {
                            let key_type = self.shallow_lower_type_expr_with_context(
                                &sig.key_type,
                                env,
                                scope,
                                name_resolution,
                                scope_payload,
                                shadowing,
                                substitutions,
                                reduction_context,
                            );
                            let value_type = self.shallow_lower_type_expr_with_context(
                                &sig.value_type,
                                env,
                                scope,
                                name_resolution,
                                scope_payload,
                                shadowing,
                                substitutions,
                                reduction_context,
                            );
                            index_signatures.push(IndexSignature {
                                key_type,
                                value_type,
                                readonly: sig.readonly,
                                // Carry the IR index signature's OXC spans.
                                spans: sig.spans,
                                // Declaration file of THIS index signature —
                                // from the object's lowering scope, not the
                                // (possibly scope-less) value-type node.
                                declaration_origin: scope.canonical_file(),
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
            // Arrays publish through the dedicated
            // `SemanticNodeData::Array { element, readonly }` variant per
            // B4 + §7.14: array indexed-access is hot and must not
            // pay generic `Array<T>` declaration-instantiation cost on
            // every access.
            TypeExpr::Array { element, readonly } => {
                let element_id = self.shallow_lower_type_expr_with_context(
                    element,
                    env,
                    scope,
                    name_resolution,
                    scope_payload,
                    shadowing,
                    substitutions,
                    reduction_context,
                );
                graph.intern_node_with_scope(
                    SemanticNodeData::Array {
                        element: element_id,
                        readonly: *readonly,
                    },
                    scope.clone(),
                )
            }
            // Tuples publish via `SemanticNodeData::Tuple` preserving
            // label / optional / rest metadata for every element (plan
            // §3 B4 + §7.14). Element bodies are lazily interned at
            // shell level — deeper expansion happens through
            // `ProjectPath` sub-queries when a caller reaches into a
            // specific slot.
            TypeExpr::Tuple { elements, readonly } => {
                let mut lowered_elements: Vec<TupleElement> = Vec::with_capacity(elements.len());
                for element in elements.iter() {
                    let value = self.shallow_lower_type_expr_with_context(
                        &element.ty,
                        env,
                        scope,
                        name_resolution,
                        scope_payload,
                        shadowing,
                        substitutions,
                        reduction_context,
                    );
                    lowered_elements.push(TupleElement {
                        label: element.label.as_deref().map(Arc::<str>::from),
                        value,
                        optional: element.optional,
                        rest: element.rest,
                    });
                }
                // Normalize-on-intern (the variadic-spread rule): when an
                // instantiation env already substituted a rest element's
                // binder to a concrete tuple (`[...A, ...B]` lowered with
                // `A = [1, 2]`), the spread splices in place; a sole
                // rest-of-array tuple collapses to the array. Open rest
                // values (unbound generics, carriers) are preserved
                // verbatim — decl-body lowering stays carrier-shaped.
                match self.normalize_tuple_spread(&lowered_elements, *readonly) {
                    crate::project_semantic_dispatch::build::NormalizedTupleShape::Array(
                        array_node,
                    ) => array_node,
                    crate::project_semantic_dispatch::build::NormalizedTupleShape::Tuple(
                        normalized,
                    ) => graph.intern_node_with_scope(
                        SemanticNodeData::Tuple {
                            elements: Arc::from(normalized.into_boxed_slice()),
                            readonly: *readonly,
                        },
                        scope.clone(),
                    ),
                }
            }
            // Template-literal shells publish verbatim — the relation
            // engine's infer-pattern support for template matching is a
            // follow-up per, but the shell carrier itself is
            // not deferred.
            TypeExpr::TemplateLiteral {
                quasis,
                expressions,
            } => {
                let lowered_quasis: Vec<Arc<str>> = quasis
                    .iter()
                    .map(|q| Arc::<str>::from(q.as_str()))
                    .collect();
                let lowered_expressions: Vec<SemanticNodeId> = expressions
                    .iter()
                    .map(|expr| {
                        self.shallow_lower_type_expr_with_context(
                            expr,
                            env,
                            scope,
                            name_resolution,
                            scope_payload,
                            shadowing,
                            substitutions,
                            reduction_context,
                        )
                    })
                    .collect();
                graph.intern_node_with_scope(
                    SemanticNodeData::TemplateLiteral {
                        quasis: Arc::from(lowered_quasis.into_boxed_slice()),
                        expressions: Arc::from(lowered_expressions.into_boxed_slice()),
                    },
                    scope.clone(),
                )
            }
            // Parenthesised types are structurally transparent — `(A | B)`
            // is equivalent to `A | B`. Unwrap and recurse (plan B4
            // follow-up).
            TypeExpr::Parenthesized(inner) => self.shallow_lower_type_expr_with_context(
                inner,
                env,
                scope,
                name_resolution,
                scope_payload,
                shadowing,
                substitutions,
                reduction_context,
            ),
            // Mapped types (`{ [K in keyof T]: T[K] }` and friends)
            // route through `SemanticQueryKey::MappedType` so `build_mapped_type`
            // produces the correct shell + per-member
            // modifiers. The key insight for the common `keyof T`
            // pattern: `TypeExpr::Mapped.source` is the key space
            // expression (`keyof T`), not T itself. `build_mapped_type`'s
            // `source` parameter wants T (the underlying object being
            // mapped over) so it can project each key's value from T
            // directly (see `mapped_type_value_materialised_from_source_member_for_known_keys`).
            // We detect the `keyof T` shape and extract T; any other
            // mapped-source shape falls back to passing the lowered
            // source through for both slots.
            TypeExpr::Mapped {
                parameter,
                source,
                value,
                optional,
                readonly,
                name_type,
                ..
            } => {
                use crate::semantic_query::{MapperKey, OptionalityMod, ReadonlyMod};
                use verter_type_expr::MappedModifier;

                let mut mapper_env = env.clone();
                let mapper_display_name: Arc<str> = Arc::from(parameter.as_str());
                // The mapper parameter K is introduced by the
                // enclosing `[K in S]` binding; treat its declaration
                // as the mapped-type shell itself. The scope's
                // `canonical_id` + `whole_hash` identifies the file;
                // `decl_name = "<mapper-param>"` is a sentinel that
                // distinguishes mapper parameters from user-declared
                // interface / type-alias parameters.
                //
                // `param_index` is assigned from the host-owned
                // [`MapperBinderRegistry`](crate::mapper_binder_registry)
                // keyed by `(canonical, display_name,
                // structural-fingerprint(source_ptr, value_ptr,
                // name_type_ptr, optional, readonly))`. Two
                // lowerings of the SAME source mapper share the
                // same ordinal — and therefore the same
                // `TypeParam` SemanticNodeId, the same
                // `MapperKey`, and the same `MappedType` cache
                // key. Two distinct `[K in ...]` binders in the
                // same scope still get distinct ordinals via
                // distinct fingerprints. See
                // [`crate::mapper_binder_registry`].
                let mapper_decl = match scope {
                    NodeScopeId::Global => DeclIdentity {
                        canonical_id: Arc::from(""),
                        whole_hash: HashValue::default(),
                        decl_name: Arc::from("<mapper-param>"),
                    },
                    NodeScopeId::File {
                        canonical_id,
                        whole_hash,
                        ..
                    } => DeclIdentity {
                        canonical_id: Arc::clone(canonical_id),
                        whole_hash: *whole_hash,
                        decl_name: Arc::from("<mapper-param>"),
                    },
                };
                // Fix: resolve the `param_index` ordinal
                // through the host-owned
                // [`MapperBinderRegistry`](crate::mapper_binder_registry::MapperBinderRegistry)
                // keyed by `(canonical, display_name,
                // fingerprint(source_ptr, value_ptr,
                // name_type_ptr, optional, readonly))`. Two
                // lowerings of the SAME source mapper get the
                // SAME ordinal — and therefore the SAME
                // `TypeParam` SemanticNodeId, the SAME
                // `MapperKey`, and the SAME
                // `SemanticQueryKey::MappedType` cache key.
                //
                // This replaces the per-dispatcher counter
                // (`ProjectSemanticDispatch::next_mapped_binder_ordinal`)
                // which destabilised mapper identity across
                // dispatcher instances — the concern empirically
                // confirmed (258,546 ordinal
                // collisions ≈ 258,611 cold MappedType builds on
                // ChatMessages.vue).
                let fingerprint = crate::mapper_binder_registry::MapperFingerprint::from_components(
                    source,
                    value,
                    *optional,
                    *readonly,
                    name_type.as_ref(),
                );
                let mapper_ordinal = self
                    .ctx
                    .project_type_store()
                    .mapper_binder_registry()
                    .ordinal_for(&mapper_decl.canonical_id, &mapper_display_name, fingerprint);
                // Mapper-binder-ordinal classification. The counter
                // bumps whenever the SAME `(canonical, display_name)`
                // triple is observed with a DIFFERENT ordinal in the
                // same request — i.e. two `ordinal_for` calls for the
                // same display-name slot landed in different
                // [`MapperFingerprint`] entries.
                //
                // Dual meaning: a non-zero
                // count does NOT necessarily mean the host-owned
                // registry is "failing to stabilise mapper identity"
                // — the registry only deduplicates fingerprints that
                // share `(source_ptr, value_ptr, name_type_ptr,
                // optional, readonly)`. A non-zero count therefore
                // means at least one of:
                //   (a) genuine registry instability — the SAME
                //       logical mapper hashed to two pointers (e.g.
                //       prepared-body re-decoding handed out fresh
                //       Arcs across calls); OR
                //   (b) genuine substitution fanout — different
                //       instantiations of the same generic decl
                //       lower to structurally distinct Mapped
                //       subtrees with different lowered `source` /
                //       `value` SemanticNodeIds, which is
                //       semantically correct (each instantiation IS
                //       a distinct mapped type).
                //
                // To attribute the count between (a) and (b),
                // compare against `recursive_substitute_unique` /
                // `substitute_top_level_calls` on the audit
                // footprint: a substitution-driven fanout will show
                // up there too. Empirically on ChatMessages.vue the
                // 258K-collision count tracks 258K cold MappedType
                // dispatches, indicating (b) — the registry is
                // doing what it can.
                if let Some(ctx) = crate::request_context::current_request_context() {
                    let hb = mapper_decl.whole_hash;
                    let hash_u64 = u64::from_le_bytes([
                        hb[0], hb[1], hb[2], hb[3], hb[4], hb[5], hb[6], hb[7],
                    ]);
                    let identity = crate::request_context::MapperSourceIdentity {
                        canonical_id: Arc::clone(&mapper_decl.canonical_id),
                        whole_hash: hash_u64,
                        display_name: Arc::clone(&mapper_display_name),
                    };
                    ctx.classify_mapper_binder_ordinal(identity, mapper_ordinal);
                }
                let parameter_id = graph.intern_node_with_scope(
                    SemanticNodeData::TypeParam {
                        decl: mapper_decl,
                        param_index: mapper_ordinal,
                        // Mapper parameters carry no declaration-site
                        // constraint or default in TS mapped syntax —
                        // the keyspace is expressed via the outer
                        // `[K in S]` binding, not via `T extends` on K.
                        constraint: None,
                        default: None,
                        display_name: Arc::clone(&mapper_display_name),
                    },
                    scope.clone(),
                );
                mapper_env.insert(parameter.clone(), parameter_id);

                let (source_sem, key_space_sem) = match source.as_ref() {
                    // `{ [K in keyof T]: ... }` — extract T.
                    TypeExpr::KeyOf(inner) => {
                        let inner_id = self.shallow_lower_type_expr_with_context(
                            inner,
                            env,
                            scope,
                            name_resolution,
                            scope_payload,
                            shadowing,
                            substitutions,
                            reduction_context,
                        );
                        let key_space =
                            if crate::semantic_query::may_reduce_operator(reduction_context) {
                                match self.execute_type_node(SemanticQueryKey::KeyOf {
                                    base: inner_id,
                                    context: reduction_context,
                                }) {
                                    QueryResult::Value(SemanticQueryOutput {
                                        value: id, ..
                                    }) => id,
                                    _ => self.opaque(QueryError::Miss),
                                }
                            } else {
                                match graph.node_data(inner_id).as_deref() {
                                    Some(SemanticNodeData::Opaque(_)) | None => {
                                        self.opaque(QueryError::Miss)
                                    }
                                    _ => graph.intern_node_with_scope(
                                        SemanticNodeData::KeyOf { base: inner_id },
                                        scope.clone(),
                                    ),
                                }
                            };
                        (inner_id, key_space)
                    }
                    // Fallback: the source shape IS the key space.
                    _ => {
                        let lowered = self.shallow_lower_type_expr_with_context(
                            source,
                            env,
                            scope,
                            name_resolution,
                            scope_payload,
                            shadowing,
                            substitutions,
                            reduction_context,
                        );
                        (lowered, lowered)
                    }
                };

                let value_sem = self.shallow_lower_type_expr_with_context(
                    value,
                    &mapper_env,
                    scope,
                    name_resolution,
                    scope_payload,
                    shadowing,
                    substitutions,
                    reduction_context,
                );

                let optionality = match optional {
                    MappedModifier::Add => OptionalityMod::Add,
                    MappedModifier::Remove => OptionalityMod::Remove,
                    MappedModifier::None => OptionalityMod::Keep,
                };
                let readonly_mod = match readonly {
                    MappedModifier::Add => ReadonlyMod::Add,
                    MappedModifier::Remove => ReadonlyMod::Remove,
                    MappedModifier::None => ReadonlyMod::Keep,
                };

                let name_remap = name_type.as_ref().map(|nt| {
                    self.shallow_lower_type_expr_with_context(
                        nt,
                        &mapper_env,
                        scope,
                        name_resolution,
                        scope_payload,
                        shadowing,
                        substitutions,
                        reduction_context,
                    )
                });

                // Classify `value_expr` once at lowering time so
                // `build_mapped_type` matches on `mapper.kind`
                // directly instead of re-inspecting the runtime AST
                // shape on every call. Classification compares the
                // indexed-access index node id against the mapper's
                // binder node id directly, avoiding display-name
                // conflation.
                let kind = crate::semantic_query::MapperKind::classify_value_expr(
                    graph,
                    value_sem,
                    source_sem,
                    parameter_id,
                );
                let mapper = MapperKey {
                    // The mapper carries the binder's interned
                    // `TypeParam` node id, not the display-name
                    // string — binder identity in the semantic graph
                    // is by `SemanticNodeId`, not by display name.
                    parameter_node: parameter_id,
                    key_space: key_space_sem,
                    value_expr: value_sem,
                    optionality,
                    readonly: readonly_mod,
                    name_remap,
                    kind,
                };

                // Route/mode-INDEPENDENT L1 carrier-stop (LOWERING
                // entrance), MAPPED-TYPE family. A mapped type whose
                // produced surface still depends on an unbound OUTER
                // generic — an open source / key space, or a value body /
                // name remap reaching the outer generic (NOT the bound
                // mapper binder `K`) — preserves the deferred
                // `SemanticNodeData::Mapped` carrier shell in ANY mode
                // (Navigate / Expanded / Shallow / Skeleton /
                // StructuralTransit) WITHOUT dispatching the `MappedType`
                // query that would enumerate the keys and materialise the
                // per-key value (the `ChatMessagesSlots<T>` /
                // `TableSlots<T>` storm). The shells (`source_sem` /
                // `key_space_sem` / `value_sem` / `name_remap`) are
                // preserved verbatim. A CLOSED mapped type falls through
                // to the `MappedType` dispatch and materialises
                // path-precisely under a publication demand. The shared
                // open-mapped predicate decides openness (no second
                // walker).
                if crate::project_semantic_dispatch::raise::mapped_type_is_open_or_unknown(
                    self.ctx, source_sem, &mapper,
                ) {
                    return graph.intern_node_with_scope(
                        SemanticNodeData::Mapped {
                            source: source_sem,
                            mapper,
                        },
                        scope.clone(),
                    );
                }

                match self.execute_type_node(SemanticQueryKey::MappedType {
                    source: source_sem,
                    mapper,
                    context: reduction_context,
                }) {
                    QueryResult::Value(SemanticQueryOutput { value: id, .. }) => id,
                    _ => self.opaque(QueryError::Miss),
                }
            }
            // KeyOf at shell level routes through the KeyOf dispatch.
            TypeExpr::KeyOf(operand) => {
                let base_id = self.shallow_lower_type_expr_with_context(
                    operand,
                    env,
                    scope,
                    name_resolution,
                    scope_payload,
                    shadowing,
                    substitutions,
                    reduction_context,
                );
                if crate::semantic_query::may_reduce_operator(reduction_context) {
                    match self.execute_type_node(SemanticQueryKey::KeyOf {
                        base: base_id,
                        context: reduction_context,
                    }) {
                        QueryResult::Value(SemanticQueryOutput { value: id, .. }) => id,
                        _ => self.opaque(QueryError::Miss),
                    }
                } else {
                    match graph.node_data(base_id).as_deref() {
                        Some(SemanticNodeData::Opaque(_)) | None => self.opaque(QueryError::Miss),
                        _ => graph.intern_node_with_scope(
                            SemanticNodeData::KeyOf { base: base_id },
                            scope.clone(),
                        ),
                    }
                }
            }
            // Indexed access at shell level routes through the IndexedAccess
            // dispatch. The path walker materialises `T[K]` via
            // `ProjectPath` semantics.
            TypeExpr::IndexedAccess { object, index } => {
                use crate::semantic_query::IndexKey;
                // Path-precision rule (mirrors `evaluate.rs`): in a NESTED
                // `A['a']['b']`, the OUTER `['b']` access has an `object`
                // operand that is ITSELF a `TypeExpr::IndexedAccess`
                // (`A['a']`) — an INTERMEDIATE hop. That intermediate
                // operand reduction demotes to `ProjectionMode::Navigate`
                // so its sibling members are NOT eagerly expanded when the
                // caller demanded `Expanded`; only the consumed TERMINAL
                // segment (`['b']`) runs in the caller's mode (the
                // eager-projection arm below).
                //
                // When the `object` operand is NOT itself an indexed access
                // (a `Ref` / generic instantiation / inline object — e.g.
                // `ComponentSurface<T>['status']`), THIS access is the
                // single consumed terminal hop, so the object base keeps
                // the caller's mode. Demoting it unconditionally would lower
                // the base to a shallow carrier, flip the `should_defer`
                // shape gate below to a deferred shell, and leave a demanded
                // `Expanded` single-hop terminal unreduced.
                let object_is_intermediate_indexed_access =
                    matches!(object.as_ref(), TypeExpr::IndexedAccess { .. });
                let object_context = if object_is_intermediate_indexed_access {
                    reduction_context.with_mode(ProjectionMode::Navigate)
                } else {
                    reduction_context
                };
                let obj_id = self.shallow_lower_type_expr_with_context(
                    object,
                    env,
                    scope,
                    name_resolution,
                    scope_payload,
                    shadowing,
                    substitutions,
                    object_context,
                );
                // Try to reduce literal-string / literal-number indices
                // to a `PathSegment::Index` — fall back to TypeNode for
                // general type-expression indices.
                //
                // G4.4 (bounded): the numeric fold routes through the
                // single shared producer predicate
                // `build::integer_convention_index_key` — a literal
                // becomes `IndexKey::Number(i)` ONLY when `i`'s
                // `Display` IS its canonical `js_number_to_string`
                // spelling, so consumers rendering the needle with
                // `i64::to_string()` are correct by construction.
                // `evaluate::normalized_index_key_node` (and through it
                // `substitute::substitute_index_key_with_change_tracking`)
                // applies the same predicate; recovery is the symmetric
                // exact `as f64` raise (`raise::raise_index_key_to_type_expr`,
                // the walker's `Index(Number)` arm). Non-integer
                // literals (`Foo[1.5]`), exponent-regime literals
                // (`Foo[1e21]`), and integral literals whose shortest
                // round-trip diverges from their exact digits
                // (`Foo[4611686018427387904]`) stay `TypeNode`, where
                // the walker's G4.5 recovery re-derives the canonical
                // needle from the literal node.
                let folded_key = match index.as_ref() {
                    TypeExpr::Literal(verter_type_expr::LiteralValue::String(s)) => {
                        Some(IndexKey::String(Arc::<str>::from(s.as_str())))
                    }
                    TypeExpr::Literal(verter_type_expr::LiteralValue::Number(n)) => {
                        crate::project_semantic_dispatch::build::integer_convention_index_key(*n)
                            .map(IndexKey::Number)
                    }
                    _ => None,
                };
                let index_key = match folded_key {
                    Some(key) => key,
                    None => {
                        let idx_id = self.shallow_lower_type_expr_with_context(
                            index,
                            env,
                            scope,
                            name_resolution,
                            scope_payload,
                            shadowing,
                            substitutions,
                            reduction_context,
                        );
                        IndexKey::TypeNode(idx_id)
                    }
                };
                let should_defer = matches!(index_key, IndexKey::TypeNode(_))
                    || !matches!(
                        graph.node_data(obj_id).as_deref(),
                        Some(SemanticNodeData::Object(_))
                    );
                if should_defer {
                    graph.intern_node_with_scope(
                        SemanticNodeData::IndexedAccess {
                            object: obj_id,
                            index: index_key,
                        },
                        scope.clone(),
                    )
                } else {
                    // Path-precision rule: the literal `T[K]` single-hop
                    // is the TERMINAL projection of THIS indexed access,
                    // so it runs in the CALLER's mode (not a hardcoded
                    // `Navigate`). When `object` was itself an indexed
                    // access (an intermediate hop), it was lowered in
                    // `Navigate` above so its sibling members never expand;
                    // a non-indexed-access base kept the caller's mode so a
                    // demanded `Expanded` single-hop terminal still reduces.
                    // A structural-transit caller keeps transit/Navigate via
                    // its own `reduction_context.mode`.
                    match self.execute_type_node(SemanticQueryKey::IndexedAccess {
                        base: obj_id,
                        index: index_key,
                        mode: reduction_context.mode,
                    }) {
                        QueryResult::Value(SemanticQueryOutput { value: id, .. }) => id,
                        _ => self.opaque(QueryError::Miss),
                    }
                }
            }
            TypeExpr::Conditional {
                check,
                extends,
                true_type,
                false_type,
            } => {
                // Conditional relation targets are structural
                // consumers unless the check side is already an
                // object-like relation subject. Deferred / primitive
                // checks cannot decide an Object-vs-Record relation,
                // so their `extends` arm must carrier-stop and avoid
                // publishing nested `Partial<T>` / `keyof T` /
                // mapped-type keyspaces. Object-like checks such as
                // `A extends Record<U, Record<K, any>>` need the
                // target lowered under the outer demand so the
                // relation engine sees the concrete Record shape.
                //
                // The selected true/false branch keeps the outer
                // demand because that branch is the conditional's
                // published result.
                let relation_input_context =
                    crate::semantic_query::ProjectionReductionContext::structural_transit_with_mode(
                        reduction_context.mode,
                    );
                let check_id = self.shallow_lower_type_expr_with_context(
                    check,
                    env,
                    scope,
                    name_resolution,
                    scope_payload,
                    shadowing,
                    substitutions,
                    reduction_context,
                );
                let check_is_object_relation_subject = matches!(
                    graph.node_data(check_id).as_deref(),
                    Some(
                        SemanticNodeData::Object(_)
                            | SemanticNodeData::Intersection(_)
                            | SemanticNodeData::Alias(_)
                            | SemanticNodeData::DeclRef { .. }
                            | SemanticNodeData::InstantiationRef { .. }
                            | SemanticNodeData::Opaque(
                                crate::semantic_query::QueryError::DeclPlaceholder { .. }
                            )
                    )
                );
                let extends_context = if check_is_object_relation_subject {
                    reduction_context
                } else {
                    relation_input_context
                };
                let extends_id = self.shallow_lower_type_expr_with_context(
                    extends,
                    env,
                    scope,
                    name_resolution,
                    scope_payload,
                    shadowing,
                    substitutions,
                    extends_context,
                );
                // Mapped+conditional infer closure: collect EVERY
                // `SemanticNodeData::Infer { name }` reachable from
                // `extends` (bare position OR nested inside Function /
                // Tuple / Array / Union / Intersection / Object
                // shapes) and bind each name in the true-branch env
                // so `TypeExpr::Ref { name }` references in the true
                // branch resolve back to the same Infer node id.
                // Without this binding, the Ref routes through
                // `ResolveDecl` and lowers to `Opaque(Miss)`, at
                // which point the name is lost and
                // `build_conditional`'s nested-infer Function-extends
                // arm cannot substitute the bound type into the true
                // branch — leaving a deferred shell with
                // `Unknown { raw: "semanticMiss" }` sitting in the
                // position the user wrote `infer P`.
                //
                // The bare-Infer case `extends` lowered as
                // `SemanticNodeData::Infer { name }` covers
                // `T extends infer P ? P : T` directly;
                // `T extends (props: infer P) => any ? P : T` and the
                // many compound-extends shapes need this recursive
                // walk.
                let true_env_owned;
                let true_env = {
                    let mut extended = env.clone();
                    let mut visited = rustc_hash::FxHashSet::default();
                    self.collect_infer_bindings_into_env(extends_id, &mut extended, &mut visited);
                    if extended.len() != env.len() {
                        true_env_owned = extended;
                        &true_env_owned
                    } else {
                        env
                    }
                };
                let true_id = self.shallow_lower_type_expr_with_context(
                    true_type,
                    true_env,
                    scope,
                    name_resolution,
                    scope_payload,
                    shadowing,
                    substitutions,
                    reduction_context,
                );
                let false_id = self.shallow_lower_type_expr_with_context(
                    false_type,
                    env,
                    scope,
                    name_resolution,
                    scope_payload,
                    shadowing,
                    substitutions,
                    reduction_context,
                );
                match self.execute_type_node(SemanticQueryKey::Conditional {
                    check: check_id,
                    extends: extends_id,
                    true_branch: true_id,
                    false_branch: false_id,
                    distributive: matches!(check.as_ref(), TypeExpr::TypeParameter(_)),
                }) {
                    QueryResult::Value(SemanticQueryOutput { value: id, .. }) => id,
                    _ => self.opaque(QueryError::Miss),
                }
            }
            TypeExpr::TypeOf(value_ref) => {
                if value_ref.path.is_empty() {
                    return self.opaque(QueryError::Miss);
                }
                let scope_canonical_id = match scope {
                    NodeScopeId::File { canonical_id, .. } => Arc::clone(canonical_id),
                    NodeScopeId::Global => return self.opaque(QueryError::Miss),
                };
                // `typeof X.Y` semantic discrimination.
                //
                // The branch unconditionally joined the first
                // two path segments into `"X.Y"` whenever the path had
                // length > 1, turning EVERY dotted typeof into a
                // namespace-member lookup. That worked for
                // `import * as Ns from './m'; typeof Ns.Foo` (the
                // namespace-member case `build_typeof`'s
                // `has_namespace_prefix` branch handles via
                // `resolve_namespace_member_from_facts`) but broke
                // ordinary value-member projection like
                // `const sample: { id: string } = ...; typeof sample.id`,
                // because no value binding named `"sample.id"` exists.
                // The downstream Miss propagated up through `Instantiate`,
                // leaving the type argument as a free `T` placeholder when
                // the surface body referenced it through substitution
                // (`Instantiate { TypeOf { ... } }` chained substitution
                // gap — `-tier1-mismatches.md` row 4).
                //
                // The fix: attempt single-segment root resolution first
                // (the value-member projection case) and fall back to the
                // joined-2-segment lookup only when the single-segment
                // root misses AND a longer path exists. The fallback
                // preserves the namespace-member semantics for
                // `Ns.Foo[.Bar...]` shapes; the primary path closes the
                // value-member gap. Both branches reuse the same
                // `ProjectPath { mode: Navigate }` projection for the
                // tail segments — terminal-mode-only expansion is the
                // outer caller's responsibility (per CLAUDE.md "type
                // navigation must stay narrower than expansion").
                // The ambient lowering demand rides the `TypeOf` key: a
                // Skeleton / Navigate / Shallow body lowering crossing a
                // `typeof`-typed value lowers the value's declaration
                // graph carrier-preserving instead of detonating an
                // Expanded materialisation at build time.
                let single_root: Arc<str> = Arc::from(value_ref.path[0].as_str());
                let single_query = self.execute_type_node(self.typeof_key_for(
                    ValueRootKey {
                        scope: ScopeId {
                            canonical_id: Arc::clone(&scope_canonical_id),
                            local_scope: None,
                        },
                        name: Arc::clone(&single_root),
                    },
                    reduction_context,
                ));
                let (mut result, consumed_segments) = match single_query {
                    QueryResult::Value(SemanticQueryOutput { value: id, .. }) => (id, 1usize),
                    _ if value_ref.path.len() > 1 => {
                        // Namespace-member fallback: join the first two
                        // segments into `Ns.Foo` and let
                        // `resolve_namespace_member_from_facts` interpret
                        // the dotted prefix when the first segment is a
                        // namespace import alias.
                        let joined: Arc<str> = Arc::<str>::from(format!(
                            "{}.{}",
                            value_ref.path[0], value_ref.path[1]
                        ));
                        match self.execute_type_node(self.typeof_key_for(
                            ValueRootKey {
                                scope: ScopeId {
                                    canonical_id: scope_canonical_id,
                                    local_scope: None,
                                },
                                name: joined,
                            },
                            reduction_context,
                        )) {
                            QueryResult::Value(SemanticQueryOutput { value: id, .. }) => {
                                (id, 2usize)
                            }
                            _ => return self.opaque(QueryError::Miss),
                        }
                    }
                    _ => return self.opaque(QueryError::Miss),
                };
                if value_ref.path.len() > consumed_segments {
                    let path: Arc<[PathSegment]> = Arc::from(
                        value_ref.path[consumed_segments..]
                            .iter()
                            .map(|segment| PathSegment::Member(Arc::from(segment.as_str())))
                            .collect::<Vec<_>>()
                            .into_boxed_slice(),
                    );
                    result = match self.execute_type_node(SemanticQueryKey::ProjectPath {
                        base: result,
                        path,
                        context: crate::semantic_query::ProjectionReductionContext::published(
                            ProjectionMode::Navigate,
                        ),
                    }) {
                        QueryResult::Value(SemanticQueryOutput { value: id, .. }) => id,
                        _ => return self.opaque(QueryError::Miss),
                    };
                }
                // Instantiation expression: `typeof C.make<string>` applies
                // the lowered type arguments to the resolved generic
                // signature — positional binder substitution through the
                // shared substitute, yielding the non-generic instantiated
                // signature (the `ValueRef.type_args` axis from the
                // producer).
                if !value_ref.type_args.is_empty() {
                    let arg_nodes: Vec<SemanticNodeId> = value_ref
                        .type_args
                        .iter()
                        .map(|arg| {
                            self.shallow_lower_type_expr_with_context(
                                arg,
                                env,
                                scope,
                                name_resolution,
                                scope_payload,
                                shadowing,
                                substitutions,
                                reduction_context,
                            )
                        })
                        .collect();
                    result = self.apply_typeof_instantiation_args(result, &arg_nodes);
                }
                result
            }
            // Function-type lowering. Produces a
            // canonical `SemanticNodeData::Function` carrier with
            // lowered parameters and return type. Type parameters
            // lower to `TypeParamDecl` — constraints/defaults lower
            // recursively. `RecursiveRef`, `Infer`, `Rest`, and
            // `Unknown` remain scratch-only per §7.14.
            //
            // `ConstructorType` (a bare `new (...) => R`) lowers through the
            // SAME `SemanticNodeData::Function` path: it carries an identical
            // `FunctionExpr` payload, and the constructor-vs-function
            // distinction is consumed BEFORE this query-time dispatch (by the
            // Vue runtime-constructor reducer and the wire-graph builder). At
            // query time a bare constructor type is treated function-like, so
            // it shares the canonical Function carrier and raises back as
            // `TypeExpr::Function`. Without this explicit arm the wildcard
            // `_ => opaque(QueryError::Miss)` below would regress constructor-
            // type props to `Unknown("semanticMiss")`.
            TypeExpr::Function(func) | TypeExpr::ConstructorType(func) => {
                use crate::semantic_query::{FunctionParam, TypeParamDecl};
                // Function generic shadowing + binder binding: a function
                // type's OWN `<T>` shadows an identically-named outer
                // generic parameter, so the outer instantiation argument
                // must NOT substitute into this function's params /
                // return / type-param constraints+defaults. Each own
                // parameter binds to its interned `TypeParam` BINDER node
                // (the same file-scoped name-keyed identity the unbound
                // `TypeExpr::TypeParameter` arm interns), so a body
                // reference that reaches this lowering un-normalised — a
                // prepared declaration signature carries `Ref("T")`, not
                // `TypeParameter(T)` — lowers to the binder node instead
                // of a `ResolveDecl` miss. The storage binding lives for
                // the whole arm; `env` is re-bound to it only when the
                // function declares its own type parameters (functions
                // with none pay nothing — they keep the outer `env` by
                // reference).
                let scoped_env_storage;
                let env: &FxHashMap<String, SemanticNodeId> = if func.type_parameters.is_empty() {
                    env
                } else {
                    let mut scoped = env.clone();
                    for tp in &func.type_parameters {
                        let display_name: Arc<str> = Arc::from(tp.name.as_str());
                        // Constraint / default lower under the OUTER env
                        // (a constraint referencing an outer generic must
                        // still substitute; the own binder is not in
                        // scope for its own constraint head).
                        let constraint = tp.constraint.as_deref().map(|c| {
                            self.shallow_lower_type_expr_with_context(
                                c,
                                env,
                                scope,
                                name_resolution,
                                scope_payload,
                                shadowing,
                                substitutions,
                                reduction_context,
                            )
                        });
                        let default = tp.default.as_deref().map(|d| {
                            self.shallow_lower_type_expr_with_context(
                                d,
                                env,
                                scope,
                                name_resolution,
                                scope_payload,
                                shadowing,
                                substitutions,
                                reduction_context,
                            )
                        });
                        let decl = crate::semantic_query::DeclIdentity::from_scope(
                            scope,
                            Arc::clone(&display_name),
                        );
                        let binder = graph.intern_node_with_scope(
                            SemanticNodeData::TypeParam {
                                decl,
                                param_index: 0,
                                constraint,
                                default,
                                display_name,
                            },
                            scope.clone(),
                        );
                        scoped.insert(tp.name.clone(), binder);
                    }
                    scoped_env_storage = scoped;
                    &scoped_env_storage
                };
                let params: Vec<FunctionParam> = func
                    .parameters
                    .iter()
                    .map(|param| FunctionParam {
                        name: param.name.as_deref().map(Arc::<str>::from),
                        ty: self.shallow_lower_type_expr_with_context(
                            &param.ty,
                            env,
                            scope,
                            name_resolution,
                            scope_payload,
                            shadowing,
                            substitutions,
                            reduction_context,
                        ),
                        optional: param.optional,
                        rest: param.rest,
                        // Carry the IR parameter's OXC span verbatim.
                        span: param.span,
                    })
                    .collect();
                let return_type = match func.return_type.as_deref() {
                    Some(ret) => self.shallow_lower_type_expr_with_context(
                        ret,
                        env,
                        scope,
                        name_resolution,
                        scope_payload,
                        shadowing,
                        substitutions,
                        reduction_context,
                    ),
                    None => self.opaque(QueryError::Miss),
                };
                let type_parameters: Vec<TypeParamDecl> = func
                    .type_parameters
                    .iter()
                    .map(|tp| TypeParamDecl {
                        name: Arc::from(tp.name.as_str()),
                        constraint: tp.constraint.as_deref().map(|c| {
                            self.shallow_lower_type_expr_with_context(
                                c,
                                env,
                                scope,
                                name_resolution,
                                scope_payload,
                                shadowing,
                                substitutions,
                                reduction_context,
                            )
                        }),
                        default: tp.default.as_deref().map(|d| {
                            self.shallow_lower_type_expr_with_context(
                                d,
                                env,
                                scope,
                                name_resolution,
                                scope_payload,
                                shadowing,
                                substitutions,
                                reduction_context,
                            )
                        }),
                    })
                    .collect();
                graph.intern_node_with_scope(
                    SemanticNodeData::Function {
                        params: Arc::from(params.into_boxed_slice()),
                        return_type,
                        type_parameters: Arc::from(type_parameters.into_boxed_slice()),
                        // Stamp the whole-signature + return spans from the IR
                        // FunctionExpr (NOT recovered from child node ids).
                        signature_span: func.spans.signature,
                        return_type_span: func.spans.return_type,
                    },
                    scope.clone(),
                )
            }
            // `infer X` placeholder in a conditional's `extends` arm
            // Explicit semantic variant rather
            // than encoded via scope overloading. Substitution picks
            // the Infer arm up symmetrically with TypeParam in
            // `substitute_semantic_type_param`; `build_conditional`
            // recognises a bare Infer in `extends` and binds the
            // true-branch's placeholder to the check side.
            TypeExpr::Infer { name } => graph.intern_node_with_scope(
                SemanticNodeData::Infer {
                    name: Arc::from(name.as_str()),
                },
                scope.clone(),
            ),
            // Conditionals, rest, recursive-ref, and unknown
            // constructs remain out of this pass's scope — they route
            // through their own dispatch builders (conditional /
            // userland-equivalence) or stay solver-scratch-only.
            _ => self.opaque(QueryError::Miss),
        }
    }

    /// Walk `extends_id`'s graph subtree collecting every reachable
    /// `SemanticNodeData::Infer { name }` and binding `name → infer_node`
    /// in `env`. Used by the Conditional lowering arm to extend the
    /// true-branch lowering env so nested `infer P` positions
    /// (e.g. `T extends (props: infer P) => any` or
    /// `T extends [infer A, infer B]`) bind correctly. Cycles are guarded
    /// via `visited`.
    ///
    /// Walks Function / Tuple / Array / Union / Intersection / Object
    /// shapes — every composite a Conditional's `extends` clause may
    /// hold an `infer` position inside. Skips terminals and lazy
    /// carriers (DeclRef / InstantiationRef) because TypeScript only
    /// allows `infer` syntactically inside conditional `extends`
    /// positions, and the syntactic positions correspond to the
    /// composite shapes walked here.
    pub(super) fn collect_infer_bindings_into_env(
        &self,
        node: SemanticNodeId,
        env: &mut FxHashMap<String, SemanticNodeId>,
        visited: &mut rustc_hash::FxHashSet<SemanticNodeId>,
    ) {
        if !visited.insert(node) {
            return;
        }
        let Some(data) = self.graph().node_data(node) else {
            return;
        };
        match data.as_ref() {
            SemanticNodeData::Infer { name } => {
                env.insert(name.as_ref().to_string(), node);
            }
            SemanticNodeData::Function {
                params,
                return_type,
                ..
            } => {
                let params = params.clone();
                let return_type = *return_type;
                drop(data);
                for param in params.iter() {
                    self.collect_infer_bindings_into_env(param.ty, env, visited);
                }
                self.collect_infer_bindings_into_env(return_type, env, visited);
            }
            SemanticNodeData::Tuple { elements, .. } => {
                let elements = elements.clone();
                drop(data);
                for elem in elements.iter() {
                    self.collect_infer_bindings_into_env(elem.value, env, visited);
                }
            }
            SemanticNodeData::Array { element, .. } => {
                let element = *element;
                drop(data);
                self.collect_infer_bindings_into_env(element, env, visited);
            }
            SemanticNodeData::Union(members) | SemanticNodeData::Intersection(members) => {
                let members = members.clone();
                drop(data);
                for member in members.iter() {
                    self.collect_infer_bindings_into_env(*member, env, visited);
                }
            }
            SemanticNodeData::Object(surface) => {
                let surface = surface.clone();
                drop(data);
                for member in surface.members.iter() {
                    self.collect_infer_bindings_into_env(member.value, env, visited);
                }
                for sig in surface.call_signatures.iter() {
                    self.collect_infer_bindings_into_env(*sig, env, visited);
                }
                for sig in surface.construct_signatures.iter() {
                    self.collect_infer_bindings_into_env(*sig, env, visited);
                }
                for sig in surface.index_signatures.iter() {
                    self.collect_infer_bindings_into_env(sig.key_type, env, visited);
                    self.collect_infer_bindings_into_env(sig.value_type, env, visited);
                }
            }
            SemanticNodeData::IndexedAccess { object, index } => {
                let object = *object;
                let index = index.clone();
                drop(data);
                self.collect_infer_bindings_into_env(object, env, visited);
                if let crate::semantic_query::IndexKey::TypeNode(idx_node) = index {
                    self.collect_infer_bindings_into_env(idx_node, env, visited);
                }
            }
            SemanticNodeData::KeyOf { base } => {
                let base = *base;
                drop(data);
                self.collect_infer_bindings_into_env(base, env, visited);
            }
            // Terminals and lazy carriers — no nested infer positions
            // syntactically reachable. (TS rejects `infer` outside
            // conditional `extends`.)
            _ => {}
        }
    }
}
