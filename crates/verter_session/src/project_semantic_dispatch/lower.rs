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
//! `fn shallow_lower_type_expr` exists in `crates/`.

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
    ProjectionMode, QueryError, QueryResult, ResolveDeclKey, ScopeId, SemanticNodeData,
    SemanticNodeId, SemanticQueryApi, SemanticQueryKey, SurfaceMember, SurfaceView, TupleElement,
    ValueRootKey,
};

impl<'a> ProjectSemanticDispatch<'a> {
    /// Shallow-lower a [`TypeExpr`] under `env` (type-parameter bindings)
    /// into a [`SemanticNodeId`]. "Shallow" means one structural level:
    /// object members, union/intersection arms, and function / conditional
    /// sub-expressions are interned as references rather than recursively
    /// expanded. Deeper lowering is the caller's responsibility via
    /// [`SemanticQueryKey::ProjectPath`] sub-queries.
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
    pub(crate) fn shallow_lower_type_expr(
        &self,
        expr: &TypeExpr,
        env: &FxHashMap<String, SemanticNodeId>,
        scope: &NodeScopeId,
        name_resolution: &FxHashMap<String, ResolvedRootIdentity>,
        scope_payload: Option<&DeclarationScopePayload>,
        shadowing: &ScopeShadowing,
        substitutions: &mut Vec<(Arc<str>, SemanticNodeId)>,
        mode: ProjectionMode,
    ) -> SemanticNodeId {
        // Watchdog hooks for hang investigation. Both calls are inert
        // when the watchdog has not been spawned (single relaxed atomic
        // load + early return). When active, they advance a heartbeat
        // counter and respond to the watchdog's stall signal by
        // printing a self-backtrace from inside this recursion.
        // See `loop5_instrumentation.rs` watchdog module.
        crate::loop5_instrumentation::watchdog_beat();
        crate::loop5_instrumentation::watchdog_check_and_dump("shallow_lower_type_expr");
        // Step 0 spike #2 hook: marks that dispatch lowering has been
        // entered on this thread. Reads recorded *before* this call
        // are PRE_LOWER (MIGRATE candidates); reads after are POST_LOWER.
        // The hook is `#[cfg(test)]`-gated, no-op outside spike runs.
        #[cfg(test)]
        crate::spike_instrumentation::record_lower_called();
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
                    // Cluster A.
                    let constraint = param.constraint.as_ref().map(|c| {
                        self.shallow_lower_type_expr(
                            c,
                            env,
                            scope,
                            name_resolution,
                            scope_payload,
                            shadowing,
                            substitutions,
                            mode,
                        )
                    });
                    let default = param.default.as_ref().map(|d| {
                        self.shallow_lower_type_expr(
                            d,
                            env,
                            scope,
                            name_resolution,
                            scope_payload,
                            shadowing,
                            substitutions,
                            mode,
                        )
                    });
                    let display_name: Arc<str> = Arc::from(param.name.as_str());
                    // Path C C6a item 2: unresolved `TypeParameter`
                    // path uses **file-scoped name-keyed identity** —
                    // `decl_name = reference.name` (NOT the owning
                    // declaration's name, which is unavailable at
                    // this site since the parameter could not be
                    // resolved). Two unresolved `K` references
                    // anywhere in the same file alias to one
                    // SemanticNodeId; cross-file unresolved `K`
                    // references stay distinct via canonical_id.
                    // `param_index = 0` is the documented zero per
                    // item 2 ("file-scoped name-keyed
                    // identity"). Escalation path if too coarse:
                    // owner-scope-local `(name → ordinal)` map.
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
            // Cluster A + Path C C3: script-setup generic parameter.
            // When the bare name maps to a `script_setup_type_bindings`
            // entry, lower directly to a rich
            // `SemanticNodeData::TypeParam { name, constraint, default }`
            // — NOT via the ResolveDecl fallback. This
            // preserves declaration-site constraint/default so the
            // projection back to `TypeExpr::TypeParameter(TypeParam)`
            // is complete at meta-extraction time. Must match on
            // `scope_type_bindings` specifically (the script-setup
            // map), not `scope_type_names` which also contains
            // same-file type decls.
            //
            // Pre-C3 the binding lookup produced a wrapped
            // `PreparedTypeDecl` whose body was a `TypeExpr::TypeParameter`;
            // C3 retired the wrapper and stores
            // [`crate::resolver_core::prepared_decl::TypeParamBinding`]
            // entries that already carry the unlowered constraint /
            // default expressions. The arm reads them directly.
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
                    self.shallow_lower_type_expr(
                        c,
                        env,
                        scope,
                        name_resolution,
                        scope_payload,
                        shadowing,
                        substitutions,
                        mode,
                    )
                });
                let default = binding.default.as_ref().map(|d| {
                    self.shallow_lower_type_expr(
                        d,
                        env,
                        scope,
                        name_resolution,
                        scope_payload,
                        shadowing,
                        substitutions,
                        mode,
                    )
                });
                let display_name = Arc::clone(&binding.name);
                // Path C C6: script-setup type parameters get
                // `decl_name = "<script-setup>"` sentinel per
                // Pass C6, with the file's canonical_id + whole_hash
                // taken from the current lowering scope. Path C C6a
                // item 1: `param_index` is the binder's 0-based
                // position in the `<script setup generic="...">`
                // clause (carried on `TypeParamBinding.ordinal`),
                // disambiguating multiple script-setup parameters in
                // the same file.
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
            // dispatch per C1:
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
                            self.shallow_lower_type_expr(
                                arg,
                                env,
                                scope,
                                name_resolution,
                                scope_payload,
                                shadowing,
                                substitutions,
                                mode,
                            )
                        })
                        .collect();

                    // In `Navigate` mode, Pick/Omit
                    // preserve the carrier `InstantiationRef` shell so the
                    // materialiser's registry-route guard can apply
                    // cycle / package gates on the wrapped root identity
                    // BEFORE dispatch's `build_builtin_utility` projects.
                    // Other utilities (Extract, Exclude, NonNullable,
                    // Partial, Required, Readonly, Mutable, …) keep the
                    // existing eager-resolve path so they still reduce
                    // through dispatch as before. closes the
                    // literal-type reduction for `Extract` / `Exclude`
                    // through `build_builtin_utility`; the eager-resolve
                    // path here remains unchanged because the lowering
                    // contract (build the InstantiationRef carrier vs
                    // dispatch the Instantiate query) is independent of
                    // the utility's body evaluator.
                    if mode == ProjectionMode::Navigate && matches!(name.as_ref(), "Pick" | "Omit")
                    {
                        return graph.intern_node_with_scope(
                            SemanticNodeData::InstantiationRef {
                                base: builtin_identity,
                                args: Arc::from(arg_ids.into_boxed_slice()),
                            },
                            scope.clone(),
                        );
                    }
                    return match self.execute(SemanticQueryKey::Instantiate {
                        base: builtin_identity,
                        args: Arc::from(arg_ids.into_boxed_slice()),
                        body_mode: mode,
                    }) {
                        QueryResult::Value(id) => id,
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
                let Some((resolved_canonical, resolved_name)) = resolved_root else {
                    return self.opaque(QueryError::Miss);
                };
                // Phase D recursive-ref guard: if the resolved root is
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

                // D26+D27 lazy carriers: in `Navigate` mode, intern a
                // `DeclRef` / `InstantiationRef` carrier rather than
                // executing `ResolveDecl` / `Instantiate` eagerly. Cache
                // identity is the `DeclIdentity` (canonical_id +
                // whole_hash + decl_name) — same as the eager path
                // would have produced internally, so the lazy form's
                // hash collapses to the same memo entry. The walker
                // (D28) treats `DeclRef` as transparent through alias
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
                if matches!(mode, ProjectionMode::Navigate | ProjectionMode::Skeleton) {
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
                    if type_arguments.is_empty() {
                        return graph.intern_node_with_scope(
                            SemanticNodeData::DeclRef {
                                identity: decl_identity,
                            },
                            scope.clone(),
                        );
                    } else {
                        let arg_ids: Vec<SemanticNodeId> = type_arguments
                            .iter()
                            .map(|arg| {
                                self.shallow_lower_type_expr(
                                    arg,
                                    env,
                                    scope,
                                    name_resolution,
                                    scope_payload,
                                    shadowing,
                                    substitutions,
                                    mode,
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

                let anchor = match self.execute(SemanticQueryKey::ResolveDecl(ResolveDeclKey {
                    scope: ScopeId {
                        canonical_id: resolved_canonical,
                        local_scope: None,
                    },
                    name: resolved_name,
                })) {
                    QueryResult::Value(id) => id,
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
                    let arg_ids: Vec<SemanticNodeId> = type_arguments
                        .iter()
                        .map(|arg| {
                            self.shallow_lower_type_expr(
                                arg,
                                env,
                                scope,
                                name_resolution,
                                scope_payload,
                                shadowing,
                                substitutions,
                                mode,
                            )
                        })
                        .collect();
                    match self.execute(SemanticQueryKey::Instantiate {
                        base: decl_identity,
                        args: Arc::from(arg_ids.into_boxed_slice()),
                        body_mode: mode,
                    }) {
                        QueryResult::Value(id) => id,
                        _ => self.opaque(QueryError::Miss),
                    }
                }
            }
            TypeExpr::Union(arms) => {
                let mut arm_ids: Vec<SemanticNodeId> = Vec::with_capacity(arms.len());
                for arm in arms.iter() {
                    arm_ids.push(self.shallow_lower_type_expr(
                        arm,
                        env,
                        scope,
                        name_resolution,
                        scope_payload,
                        shadowing,
                        substitutions,
                        mode,
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
                    arm_ids.push(self.shallow_lower_type_expr(
                        arm,
                        env,
                        scope,
                        name_resolution,
                        scope_payload,
                        shadowing,
                        substitutions,
                        mode,
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
                            let value = self.shallow_lower_type_expr(
                                &prop.ty,
                                env,
                                scope,
                                name_resolution,
                                scope_payload,
                                shadowing,
                                substitutions,
                                mode,
                            );
                            members.push(SurfaceMember {
                                name: Arc::from(prop.name.as_str()),
                                value,
                                optional: prop.optional,
                                readonly: prop.readonly,
                                is_method: false,
                            });
                        }
                        ObjectMember::Method(method) => {
                            // Step 1.5 mapped+conditional infer closure:
                            // lower methods to canonical Function nodes
                            // (matching CallSignature handling below) so
                            // `PricingPlanSlots[K]` IndexedAccess can
                            // resolve to a real Function for C11a's
                            // Function-extends infer-binding arm. The
                            // pre-Step-1.5 `Opaque(Miss)` placeholder
                            // broke `IndexedAccess<I, "method-name">`
                            // projection: the path walker finds the
                            // member but its value is opaque, so
                            // downstream C11a's `let Some(Function...) =
                            // graph.node_data(check_resolved)` match
                            // fails and the conditional drops to a
                            // deferred shell.
                            let function_expr =
                                TypeExpr::Function(Arc::new(method.function.clone()));
                            let value = self.shallow_lower_type_expr(
                                &function_expr,
                                env,
                                scope,
                                name_resolution,
                                scope_payload,
                                shadowing,
                                substitutions,
                                mode,
                            );
                            members.push(SurfaceMember {
                                name: Arc::from(method.name.as_str()),
                                value,
                                optional: method.optional,
                                readonly: false,
                                is_method: true,
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
                            let fn_id = self.shallow_lower_type_expr(
                                &function_expr,
                                env,
                                scope,
                                name_resolution,
                                scope_payload,
                                shadowing,
                                substitutions,
                                mode,
                            );
                            call_signatures.push(fn_id);
                        }
                        ObjectMember::ConstructSignature(func) => {
                            let function_expr = TypeExpr::Function(Arc::new(func.clone()));
                            let fn_id = self.shallow_lower_type_expr(
                                &function_expr,
                                env,
                                scope,
                                name_resolution,
                                scope_payload,
                                shadowing,
                                substitutions,
                                mode,
                            );
                            construct_signatures.push(fn_id);
                        }
                        ObjectMember::IndexSignature(sig) => {
                            let key_type = self.shallow_lower_type_expr(
                                &sig.key_type,
                                env,
                                scope,
                                name_resolution,
                                scope_payload,
                                shadowing,
                                substitutions,
                                mode,
                            );
                            let value_type = self.shallow_lower_type_expr(
                                &sig.value_type,
                                env,
                                scope,
                                name_resolution,
                                scope_payload,
                                shadowing,
                                substitutions,
                                mode,
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
            // Arrays publish through the dedicated
            // `SemanticNodeData::Array { element, readonly }` variant per
            // B4 + §7.14: array indexed-access is hot and must not
            // pay generic `Array<T>` declaration-instantiation cost on
            // every access.
            TypeExpr::Array { element, readonly } => {
                let element_id = self.shallow_lower_type_expr(
                    element,
                    env,
                    scope,
                    name_resolution,
                    scope_payload,
                    shadowing,
                    substitutions,
                    mode,
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
                    let value = self.shallow_lower_type_expr(
                        &element.ty,
                        env,
                        scope,
                        name_resolution,
                        scope_payload,
                        shadowing,
                        substitutions,
                        mode,
                    );
                    lowered_elements.push(TupleElement {
                        label: element.label.as_deref().map(Arc::<str>::from),
                        value,
                        optional: element.optional,
                        rest: element.rest,
                    });
                }
                graph.intern_node_with_scope(
                    SemanticNodeData::Tuple {
                        elements: Arc::from(lowered_elements.into_boxed_slice()),
                        readonly: *readonly,
                    },
                    scope.clone(),
                )
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
                        self.shallow_lower_type_expr(
                            expr,
                            env,
                            scope,
                            name_resolution,
                            scope_payload,
                            shadowing,
                            substitutions,
                            mode,
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
            TypeExpr::Parenthesized(inner) => self.shallow_lower_type_expr(
                inner,
                env,
                scope,
                name_resolution,
                scope_payload,
                shadowing,
                substitutions,
                mode,
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
                // Path C C6: the mapper parameter K is introduced by
                // the enclosing `[K in S]` binding; treat its
                // declaration as the mapped-type shell itself. The
                // scope's canonical_id + whole_hash identifies the
                // file; `decl_name = "<mapper-param>"` is a sentinel
                // to distinguish from user-declared interface/type-
                // alias parameters.
                //
                // Path C C6a item 3: assign a per-dispatcher ordinal
                // as `param_index` so two distinct `[K in ...]`
                // binders in the same file (or same scope) hash to
                // distinct identity tuples. Documented fallback from
                // per-owning-scope per — see
                // `ProjectSemanticDispatch::mapped_binder_ordinal`
                // for the trade-off discussion.
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
                let mapper_ordinal = self.next_mapped_binder_ordinal();
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
                        let inner_id = self.shallow_lower_type_expr(
                            inner,
                            env,
                            scope,
                            name_resolution,
                            scope_payload,
                            shadowing,
                            substitutions,
                            mode,
                        );
                        let key_space =
                            match self.execute(SemanticQueryKey::KeyOf { base: inner_id }) {
                                QueryResult::Value(id) => id,
                                _ => self.opaque(QueryError::Miss),
                            };
                        (inner_id, key_space)
                    }
                    // Fallback: the source shape IS the key space.
                    _ => {
                        let lowered = self.shallow_lower_type_expr(
                            source,
                            env,
                            scope,
                            name_resolution,
                            scope_payload,
                            shadowing,
                            substitutions,
                            mode,
                        );
                        (lowered, lowered)
                    }
                };

                let value_sem = self.shallow_lower_type_expr(
                    value,
                    &mapper_env,
                    scope,
                    name_resolution,
                    scope_payload,
                    shadowing,
                    substitutions,
                    mode,
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
                    self.shallow_lower_type_expr(
                        nt,
                        &mapper_env,
                        scope,
                        name_resolution,
                        scope_payload,
                        shadowing,
                        substitutions,
                        mode,
                    )
                });

                // Path C C5 + C6a item 7: classify value_expr once
                // at lowering time so `build_mapped_type` can match
                // on `mapper.kind` instead of running the runtime
                // `mapper_value_is_identity_t_of_k` detection on every
                // call. Classification compares the indexed-access
                // index node id against the mapper's binder node id
                // directly, avoiding display-name conflation.
                let kind = crate::semantic_query::MapperKind::classify_value_expr(
                    graph,
                    value_sem,
                    source_sem,
                    parameter_id,
                );
                let mapper = MapperKey {
                    // Path C C6a item 6: the mapper carries the
                    // binder's interned `TypeParam` node id, not the
                    // display-name string.
                    parameter_node: parameter_id,
                    key_space: key_space_sem,
                    value_expr: value_sem,
                    optionality,
                    readonly: readonly_mod,
                    name_remap,
                    kind,
                };

                match self.execute(SemanticQueryKey::MappedType {
                    source: source_sem,
                    mapper,
                }) {
                    QueryResult::Value(id) => id,
                    _ => self.opaque(QueryError::Miss),
                }
            }
            // KeyOf at shell level routes through the KeyOf dispatch.
            TypeExpr::KeyOf(operand) => {
                let base_id = self.shallow_lower_type_expr(
                    operand,
                    env,
                    scope,
                    name_resolution,
                    scope_payload,
                    shadowing,
                    substitutions,
                    mode,
                );
                match graph.node_data(base_id).as_deref() {
                    Some(SemanticNodeData::Object(_)) => {
                        match self.execute(SemanticQueryKey::KeyOf { base: base_id }) {
                            QueryResult::Value(id) => id,
                            _ => self.opaque(QueryError::Miss),
                        }
                    }
                    Some(SemanticNodeData::Opaque(_)) | None => self.opaque(QueryError::Miss),
                    _ => graph.intern_node_with_scope(
                        SemanticNodeData::KeyOf { base: base_id },
                        scope.clone(),
                    ),
                }
            }
            // Indexed access at shell level routes through the IndexedAccess
            // dispatch. The path walker materialises `T[K]` via
            // `ProjectPath` semantics.
            TypeExpr::IndexedAccess { object, index } => {
                use crate::semantic_query::{IndexKey, ProjectionMode};
                let obj_id = self.shallow_lower_type_expr(
                    object,
                    env,
                    scope,
                    name_resolution,
                    scope_payload,
                    shadowing,
                    substitutions,
                    mode,
                );
                // Try to reduce literal-string / literal-number indices
                // to a `PathSegment::Index` — fall back to TypeNode for
                // general type-expression indices.
                let index_key = match index.as_ref() {
                    TypeExpr::Literal(lit) => match lit {
                        verter_type_expr::LiteralValue::String(s) => {
                            IndexKey::String(Arc::<str>::from(s.as_str()))
                        }
                        verter_type_expr::LiteralValue::Number(n) => {
                            IndexKey::Number(n.to_bits() as i64)
                        }
                        _ => {
                            let idx_id = self.shallow_lower_type_expr(
                                index,
                                env,
                                scope,
                                name_resolution,
                                scope_payload,
                                shadowing,
                                substitutions,
                                mode,
                            );
                            IndexKey::TypeNode(idx_id)
                        }
                    },
                    _ => {
                        let idx_id = self.shallow_lower_type_expr(
                            index,
                            env,
                            scope,
                            name_resolution,
                            scope_payload,
                            shadowing,
                            substitutions,
                            mode,
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
                    match self.execute(SemanticQueryKey::IndexedAccess {
                        base: obj_id,
                        index: index_key,
                        mode: ProjectionMode::Navigate,
                    }) {
                        QueryResult::Value(id) => id,
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
                let check_id = self.shallow_lower_type_expr(
                    check,
                    env,
                    scope,
                    name_resolution,
                    scope_payload,
                    shadowing,
                    substitutions,
                    mode,
                );
                let extends_id = self.shallow_lower_type_expr(
                    extends,
                    env,
                    scope,
                    name_resolution,
                    scope_payload,
                    shadowing,
                    substitutions,
                    mode,
                );
                // Cluster A + Step 1.5 mapped+conditional infer
                // closure: collect EVERY `SemanticNodeData::Infer { name }`
                // reachable from `extends` (bare position OR nested inside
                // Function / Tuple / Array / Union / Intersection / Object
                // shapes) and bind each name in the true-branch env so
                // `TypeExpr::Ref { name }` references in the true branch
                // resolve back to the same Infer node id. Without this
                // binding, the Ref routes through `ResolveDecl` and lowers
                // to `Opaque(Miss)`, at which point the name is lost and
                // `build_conditional`'s C11a Function-extends arm cannot
                // substitute the bound type into the true branch — leaving
                // a deferred shell with `Unknown { raw: "semanticMiss" }`
                // sitting in the position the user wrote `infer P`.
                //
                // The pre-Step-1.5 single-bare-Infer arm at
                // `extends` lowered as `SemanticNodeData::Infer { name }`
                // covered `T extends infer P ? P : T` only.
                // `T extends (props: infer P) => any ? P : T` and the
                // many compound-extends shapes need the recursive walk.
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
                let true_id = self.shallow_lower_type_expr(
                    true_type,
                    true_env,
                    scope,
                    name_resolution,
                    scope_payload,
                    shadowing,
                    substitutions,
                    mode,
                );
                let false_id = self.shallow_lower_type_expr(
                    false_type,
                    env,
                    scope,
                    name_resolution,
                    scope_payload,
                    shadowing,
                    substitutions,
                    mode,
                );
                match self.execute(SemanticQueryKey::Conditional {
                    check: check_id,
                    extends: extends_id,
                    true_branch: true_id,
                    false_branch: false_id,
                    distributive: matches!(check.as_ref(), TypeExpr::TypeParameter(_)),
                }) {
                    QueryResult::Value(id) => id,
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
                let single_root: Arc<str> = Arc::from(value_ref.path[0].as_str());
                let single_query = self.execute(SemanticQueryKey::TypeOf {
                    value_root: ValueRootKey {
                        scope: ScopeId {
                            canonical_id: Arc::clone(&scope_canonical_id),
                            local_scope: None,
                        },
                        name: Arc::clone(&single_root),
                    },
                });
                let (mut result, consumed_segments) = match single_query {
                    QueryResult::Value(id) => (id, 1usize),
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
                        match self.execute(SemanticQueryKey::TypeOf {
                            value_root: ValueRootKey {
                                scope: ScopeId {
                                    canonical_id: scope_canonical_id,
                                    local_scope: None,
                                },
                                name: joined,
                            },
                        }) {
                            QueryResult::Value(id) => (id, 2usize),
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
                    result = match self.execute(SemanticQueryKey::ProjectPath {
                        base: result,
                        path,
                        mode: ProjectionMode::Navigate,
                    }) {
                        QueryResult::Value(id) => id,
                        _ => return self.opaque(QueryError::Miss),
                    };
                }
                result
            }
            // Function-type lowering. Produces a
            // canonical `SemanticNodeData::Function` carrier with
            // lowered parameters and return type. Type parameters
            // lower to `TypeParamDecl` — constraints/defaults lower
            // recursively. `RecursiveRef`, `Infer`, `Rest`, and
            // `Unknown` remain scratch-only per §7.14.
            TypeExpr::Function(func) => {
                use crate::semantic_query::{FunctionParam, TypeParamDecl};
                let params: Vec<FunctionParam> = func
                    .parameters
                    .iter()
                    .map(|param| FunctionParam {
                        name: param.name.as_deref().map(Arc::<str>::from),
                        ty: self.shallow_lower_type_expr(
                            &param.ty,
                            env,
                            scope,
                            name_resolution,
                            scope_payload,
                            shadowing,
                            substitutions,
                            mode,
                        ),
                        optional: param.optional,
                        rest: param.rest,
                    })
                    .collect();
                let return_type = match func.return_type.as_deref() {
                    Some(ret) => self.shallow_lower_type_expr(
                        ret,
                        env,
                        scope,
                        name_resolution,
                        scope_payload,
                        shadowing,
                        substitutions,
                        mode,
                    ),
                    None => self.opaque(QueryError::Miss),
                };
                let type_parameters: Vec<TypeParamDecl> = func
                    .type_parameters
                    .iter()
                    .map(|tp| TypeParamDecl {
                        name: Arc::from(tp.name.as_str()),
                        constraint: tp.constraint.as_deref().map(|c| {
                            self.shallow_lower_type_expr(
                                c,
                                env,
                                scope,
                                name_resolution,
                                scope_payload,
                                shadowing,
                                substitutions,
                                mode,
                            )
                        }),
                        default: tp.default.as_deref().map(|d| {
                            self.shallow_lower_type_expr(
                                d,
                                env,
                                scope,
                                name_resolution,
                                scope_payload,
                                shadowing,
                                substitutions,
                                mode,
                            )
                        }),
                    })
                    .collect();
                graph.intern_node_with_scope(
                    SemanticNodeData::Function {
                        params: Arc::from(params.into_boxed_slice()),
                        return_type,
                        type_parameters: Arc::from(type_parameters.into_boxed_slice()),
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
            // through their own dispatch builders (C2/C7/...) or stay
            // solver-scratch-only per.
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
