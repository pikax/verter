//! `shallow_lower_type_expr` — TypeExpr → SemanticNodeId shallow lowering
//!
//! Produces the first structural layer of the semantic graph from a
//! parsed [`TypeExpr`] tree. Deeper expansion is the caller's
//! responsibility via [`SemanticQueryKey::ProjectPath`] sub-queries —
//! this pass stays one member / arm / sub-expression deep so the
//! published shell identity is stable across entry paths.
//!
//! **Authority contract:** this is the *only* EAGER (resolving) TypeExpr
//! lowering path in the workspace. The §6.5 invariant test
//! `type_expr_lowering_has_exactly_two_single_definition_producers` asserts
//! exactly one `fn shallow_lower_type_expr_with_context` exists in `crates/`
//! (and no bare-`mode` wrapper beside it — every caller states its
//! full [`ProjectionReductionContext`] demand explicitly), alongside the one
//! query-free structural producer `fn lower_type_expr_structural`.

use std::sync::Arc;

use rustc_hash::FxHashMap;
use verter_semantic::analysis::type_solver::host::ResolvedRootIdentity;
use verter_type_expr::facts::{EnumPrimitiveDomain, EnumScalar, LeafTypeFact};
use verter_type_expr::{FunctionExpr, ObjectMember, PrimitiveName, TypeExpr};

use super::{map_primitive_name, ProjectSemanticDispatch};
use crate::resolver_core::bare_name_resolve::{
    resolve_bare_name_in_scope, DeclarationScopePayload,
};
use crate::resolver_core::scope_shadowing::ScopeShadowing;
use crate::semantic_query::{
    DeclIdentity, HashValue, IndexSignature, NodeScopeId, PathSegment, PrimitiveKind,
    ProjectionMode, ProjectionReductionContext, QueryError, QueryResult, ScopeId, SemanticNodeData,
    SemanticNodeId, SemanticQueryApi, SemanticQueryKey, SemanticQueryOutput, SurfaceEntry,
    SurfaceMember, SurfaceView, TupleElement, ValueRootKey,
};

fn infer_declaration_env_key(name: &str) -> String {
    format!("\0verter:infer-declaration:{name}")
}

fn register_eager_function_alias(
    infer_binders: &crate::semantic_query::InferBinderFactory,
    alias: &TypeExpr,
    original: &FunctionExpr,
) {
    let alias_function = match alias {
        TypeExpr::Function(function) | TypeExpr::ConstructorType(function) => function,
        _ => return,
    };
    for (alias_parameter, original_parameter) in alias_function
        .type_parameters
        .iter()
        .zip(&original.type_parameters)
    {
        if let (Some(alias), Some(original)) = (
            alias_parameter.constraint.as_deref(),
            original_parameter.constraint.as_deref(),
        ) {
            infer_binders.register_equivalent_subtree(alias, original);
        }
        if let (Some(alias), Some(original)) = (
            alias_parameter.default.as_deref(),
            original_parameter.default.as_deref(),
        ) {
            infer_binders.register_equivalent_subtree(alias, original);
        }
    }
    for (alias_parameter, original_parameter) in
        alias_function.parameters.iter().zip(&original.parameters)
    {
        infer_binders.register_equivalent_subtree(&alias_parameter.ty, &original_parameter.ty);
    }
    if let (Some(alias), Some(original)) = (
        alias_function.return_type.as_deref(),
        original.return_type.as_deref(),
    ) {
        infer_binders.register_equivalent_subtree(alias, original);
    }
}

/// The scalar → projected-`TypeExpr` mapping for a stored enum member fact —
/// the session-side reader of the closed [`EnumScalar`] vocabulary (a folded
/// numeric scalar stores the CANONICAL `f64` display string, so the parse-back
/// recovers the exact bits; a deferred member's domain maps to its degraded
/// sound arm). Mirrors the `verter_semantic` fingerprint producer's
/// `scalar_to_type_expr` mapping — the shared closed grammar, not a resolver.
pub(crate) fn enum_scalar_type_expr(scalar: &EnumScalar) -> TypeExpr {
    match scalar {
        EnumScalar::String(value) => TypeExpr::string_literal(value.as_str()),
        EnumScalar::Number(value) => TypeExpr::number_literal(
            value
                .parse::<f64>()
                .expect("EnumScalar::Number stores the canonical f64 display string"),
        ),
        EnumScalar::Primitive(domain) => match domain {
            EnumPrimitiveDomain::Number => TypeExpr::Primitive(PrimitiveName::Number),
            EnumPrimitiveDomain::String => TypeExpr::Primitive(PrimitiveName::String),
            EnumPrimitiveDomain::NumberOrString => TypeExpr::union(vec![
                TypeExpr::Primitive(PrimitiveName::Number),
                TypeExpr::Primitive(PrimitiveName::String),
            ]),
            EnumPrimitiveDomain::Unknown => TypeExpr::Primitive(PrimitiveName::Unknown),
        },
    }
}

/// The leaf-fact → `TypeExpr` projection for a directly-closed
/// [`LeafTypeFact`] source (the trivially-closed inferred-annotation carrier)
/// — a closed-grammar data projection, not a resolver: a bare `Ref` leaf
/// stays a shallow reference the shared dispatch resolves on demand.
pub(crate) fn leaf_type_fact_expr(leaf: &LeafTypeFact) -> TypeExpr {
    match leaf {
        LeafTypeFact::Primitive(name) => TypeExpr::Primitive(*name),
        LeafTypeFact::StringLiteral(value) => TypeExpr::string_literal(value.as_str()),
        LeafTypeFact::NumberLiteral(value) => TypeExpr::number_literal(
            value
                .parse::<f64>()
                .expect("LeafTypeFact::NumberLiteral stores the canonical f64 display string"),
        ),
        LeafTypeFact::BooleanLiteral(value) => {
            TypeExpr::Literal(verter_type_expr::LiteralValue::Boolean(*value))
        }
        LeafTypeFact::Ref(name) => TypeExpr::Ref {
            name: Arc::from(name.as_str()),
            type_arguments: Arc::from(Vec::new().into_boxed_slice()),
        },
    }
}

impl<'a> ProjectSemanticDispatch<'a> {
    fn unique_symbol_identity_for_resolved_root(
        &self,
        root: &ResolvedRootIdentity,
    ) -> Option<verter_type_expr::facts::ValueDeclIdentityPart> {
        let (canonical_id, owner, symbol, prepared) = self.effective_prepared_value_decl(
            root.canonical_id.as_ref(),
            root.owner,
            root.symbol_name.as_ref(),
        )?;
        prepared.type_annotation.is_unique_symbol.then(|| {
            verter_type_expr::facts::ValueDeclIdentityPart {
                canonical_id,
                owner,
                symbol,
                member_path: Arc::from([]),
            }
        })
    }

    pub(super) fn unique_symbol_identity_for_value_root(
        &self,
        value_root: &ValueRootKey,
    ) -> Option<verter_type_expr::facts::ValueDeclIdentityPart> {
        let payload = self
            .ctx
            .prepared_decl_bundle(value_root.scope.canonical_id.as_ref())
            .map(|bundle| DeclarationScopePayload::from_bundle(&bundle, value_root.scope.owner));
        let root = resolve_bare_name_in_scope(
            self.ctx,
            value_root.scope.canonical_id.as_ref(),
            value_root.scope.owner,
            payload.as_ref(),
            value_root.name.as_ref(),
        )?;
        self.unique_symbol_identity_for_resolved_root(&root)
    }

    pub(super) fn unique_symbol_identity_for_typeof_node(
        &self,
        node: SemanticNodeId,
    ) -> Option<verter_type_expr::facts::ValueDeclIdentityPart> {
        let data = self.graph().node_data(node)?;
        let (value_root, path) = data.typeof_head()?;
        if !path.is_empty() || !data.carrier_type_args().is_empty() {
            return None;
        }
        self.unique_symbol_identity_for_value_root(value_root)
    }

    /// Resolve an authored `typeof value` property-key expression to the
    /// declaration's nominal `unique symbol` identity.
    ///
    /// Resolution follows the same bare-name and effective value-declaration
    /// chase as [`Self::build_typeof`], so imports and re-exports mint the
    /// declaring identity rather than the consumer's local alias.
    fn unique_symbol_identity_for_typeof(
        &self,
        value_ref: &verter_type_expr::ValueRef,
        scope: &NodeScopeId,
        name_resolution: &FxHashMap<Arc<str>, ResolvedRootIdentity>,
        scope_payload: Option<&DeclarationScopePayload>,
    ) -> Option<verter_type_expr::facts::ValueDeclIdentityPart> {
        if value_ref.path.len() != 1 || !value_ref.type_args.is_empty() {
            return None;
        }
        let (scope_canonical, scope_owner) = match scope {
            NodeScopeId::File {
                canonical_id,
                owner,
                ..
            } => (canonical_id.as_ref(), *owner),
            NodeScopeId::Global => return None,
        };
        let root_name = value_ref.path[0].as_str();
        let root = name_resolution.get(root_name).cloned().or_else(|| {
            resolve_bare_name_in_scope(
                self.ctx,
                scope_canonical,
                scope_owner,
                scope_payload,
                root_name,
            )
        })?;
        self.unique_symbol_identity_for_resolved_root(&root)
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn lower_authored_property_key(
        &self,
        key: &verter_type_expr::TypeAuthoredPropertyKey,
        infer_binders: &crate::semantic_query::InferBinderFactory,
        env: &FxHashMap<String, SemanticNodeId>,
        scope: &NodeScopeId,
        name_resolution: &FxHashMap<Arc<str>, ResolvedRootIdentity>,
        scope_payload: Option<&DeclarationScopePayload>,
        shadowing: &ScopeShadowing,
        substitutions: &mut Vec<(Arc<str>, SemanticNodeId)>,
        reduction_context: ProjectionReductionContext,
    ) -> crate::semantic_query::AuthoredPropertyKey {
        match key {
            verter_type_expr::AuthoredPropertyKey::String(value) => {
                verter_type_expr::AuthoredPropertyKey::String(Arc::clone(value))
            }
            verter_type_expr::AuthoredPropertyKey::Number(value) => {
                verter_type_expr::AuthoredPropertyKey::Number(*value)
            }
            verter_type_expr::AuthoredPropertyKey::UniqueSymbol(identity) => {
                verter_type_expr::AuthoredPropertyKey::UniqueSymbol(identity.clone())
            }
            verter_type_expr::AuthoredPropertyKey::Computed(
                computed @ TypeExpr::TypeOf(value_ref),
            ) => self
                .unique_symbol_identity_for_typeof(value_ref, scope, name_resolution, scope_payload)
                .map(verter_type_expr::AuthoredPropertyKey::UniqueSymbol)
                .unwrap_or_else(|| {
                    verter_type_expr::AuthoredPropertyKey::Computed(
                        self.lower_type_expr_with_infer_factory(
                            infer_binders,
                            computed,
                            env,
                            scope,
                            name_resolution,
                            scope_payload,
                            shadowing,
                            substitutions,
                            reduction_context,
                        ),
                    )
                }),
            verter_type_expr::AuthoredPropertyKey::Computed(computed) => {
                verter_type_expr::AuthoredPropertyKey::Computed(
                    self.lower_type_expr_with_infer_factory(
                        infer_binders,
                        computed,
                        env,
                        scope,
                        name_resolution,
                        scope_payload,
                        shadowing,
                        substitutions,
                        reduction_context,
                    ),
                )
            }
        }
    }

    /// Project a dotted type-position reference `Enum.Member` to the named
    /// member's projected type (a folded literal for a foldable member, the
    /// degraded sound primitive for a deferred one), GATED strictly on the typed
    /// [`ValueDeclKind::Enum`](verter_semantic::analysis::type_eval::ValueDeclKind::Enum)
    /// fact. Returns `None` for any prefix that is not a proven enum value
    /// declaration (so a non-enum `Ns.Member` reference is never
    /// mis-projected) or an unknown member name. The prefix is resolved to
    /// its declaring `(canonical, name)` through the prepared decl's
    /// pre-resolved map first, then the shared bare-name resolver — so a
    /// locally-declared OR an imported enum binding both resolve, with no
    /// private drill-down path.
    ///
    /// LIMITATION: only a TOP-LEVEL `Enum.Member` is projected. The
    /// `split_once('.')` below takes the FIRST dotted segment as the prefix, so
    /// a namespace-nested enum member (`Ns.Enum.Member`) resolves the prefix
    /// `Ns` — a namespace, not an enum — and returns `None` (a miss). Projecting
    /// `Ns.Enum.Member` would require first walking the namespace to its inner
    /// `Enum` binding.
    pub(super) fn resolve_enum_member_value(
        &self,
        scope_canonical: &str,
        scope_owner: verter_type_expr::TopLevelOwnerId,
        name_resolution: &FxHashMap<std::sync::Arc<str>, ResolvedRootIdentity>,
        scope_payload: Option<&DeclarationScopePayload>,
        dotted_name: &str,
    ) -> Option<TypeExpr> {
        let (prefix, member) = dotted_name.split_once('.')?;
        let identity = if let Some(direct) = name_resolution.get(prefix) {
            direct.clone()
        } else {
            resolve_bare_name_in_scope(
                self.ctx,
                scope_canonical,
                scope_owner,
                scope_payload,
                prefix,
            )?
        };
        // Resolve the prefix's enum VALUE decl through the SAME export-target
        // chase `typeof Enum` uses ([`Self::effective_prepared_value_decl`]): a
        // locally-declared enum resolves directly; a barrel re-export
        // (`export { E } from "./leaf"`) chases to the declaring leaf's decl. So
        // a re-exported enum projects its members exactly like a local one,
        // matching `typeof E`'s cross-file behaviour — one shared chase, no
        // forked resolution path.
        let (_, _, _, prepared) = self.effective_prepared_value_decl(
            identity.canonical_id.as_ref(),
            identity.owner,
            identity.symbol_name.as_ref(),
        )?;
        if prepared.kind != verter_semantic::analysis::type_eval::ValueDeclKind::Enum {
            return None;
        }
        // A DECLARED member projects to its type — the folded literal for a
        // foldable member, the degraded sound primitive for a deferred one
        // (the stored scalar via [`enum_scalar_type_expr`]), never a miss. An
        // UNDECLARED name is genuinely absent (`find` yields `None`) and
        // stays a miss, so the member-existence gate is preserved.
        prepared
            .enum_members
            .as_ref()?
            .members
            .iter()
            .find(|entry| entry.name == member)
            .map(|entry| enum_scalar_type_expr(&entry.value))
    }

    /// The ONE script-setup generic `TypeParam` node construction. BOTH
    /// query-time content readers route here — the eager shallow-lower
    /// `TypeExpr::Ref` arm below and the locator-view worklist projection
    /// (`locator_view_worklist/finish.rs`) — so the binder's identity and
    /// bound lowering can never diverge.
    ///
    /// The identity tuple is EXACTLY the historical one: the lowering
    /// scope's canonical / owner / whole hash, the `"<script-setup>"`
    /// sentinel, the stored clause ordinal, and the display name.
    ///
    /// Bound CONTENT is never stored: the full
    /// `<script setup generic="…">` clause is re-borrowed lease-only from
    /// the pinned `IndexedReady` through the ONE artifact-local transient
    /// producer
    /// ([`crate::host_resolve::indexed_script_setup_type_params`]), and the
    /// binding's stored `(ordinal, name)` selects + validates the transient
    /// parameter. Canonical / owner / whole-hash coherence with the
    /// lowering scope is required: a missing serve (a `Global` scope has no
    /// file to re-borrow from), a superseded artifact (hash drift), or a
    /// stale clause (ordinal / name mismatch) is a TYPED MISS with cache
    /// suppression — NEVER a bound-free fabricated binder.
    #[allow(clippy::too_many_arguments)]
    pub(in crate::project_semantic_dispatch) fn lower_script_setup_type_param_binding(
        &self,
        binding: &crate::resolver_core::prepared_decl::TypeParamBinding,
        env: &FxHashMap<String, SemanticNodeId>,
        scope: &NodeScopeId,
        name_resolution: &FxHashMap<std::sync::Arc<str>, ResolvedRootIdentity>,
        scope_payload: Option<&DeclarationScopePayload>,
        shadowing: &ScopeShadowing,
        substitutions: &mut Vec<(Arc<str>, SemanticNodeId)>,
        reduction_context: ProjectionReductionContext,
    ) -> SemanticNodeId {
        let graph = self.graph();
        let decl = match scope {
            NodeScopeId::Global => DeclIdentity {
                canonical_id: Arc::from(""),
                owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
                whole_hash: HashValue::default(),
                decl_name: Arc::from("<script-setup>"),
            },
            NodeScopeId::File {
                canonical_id,
                owner,
                whole_hash,
                ..
            } => DeclIdentity {
                canonical_id: Arc::clone(canonical_id),
                owner: *owner,
                whole_hash: *whole_hash,
                decl_name: Arc::from("<script-setup>"),
            },
        };
        let transient_param = match scope {
            NodeScopeId::Global => None,
            NodeScopeId::File {
                canonical_id,
                whole_hash,
                ..
            } => self
                .ctx
                .ensure_indexed_ready_serve(canonical_id.as_ref())
                .filter(|serve| serve.indexed.whole_hash == *whole_hash)
                .and_then(|serve| {
                    crate::host_resolve::indexed_script_setup_type_params(&serve.indexed)
                        .into_iter()
                        .nth(binding.ordinal as usize)
                        .filter(|param| param.name == binding.name.as_ref())
                }),
        };
        let Some(param) = transient_param else {
            crate::resolver_core::resolver_context::note_non_cacheable_read_fan_out(
                crate::resolver_core::resolver_context::NonCacheableReadReason::UnobservableSource,
            );
            return self.opaque(QueryError::Miss);
        };
        let bound_locator = |position| {
            let NodeScopeId::File {
                canonical_id,
                owner,
                ..
            } = scope
            else {
                unreachable!("script-setup bounds require their authored file scope");
            };
            verter_type_expr::locators::AuthoredBodyLocator::DeclBody(
                verter_type_expr::locators::TypeBodySlot {
                    anchor: verter_type_expr::locators::AuthoredAnchor {
                        canonical_id: Arc::clone(canonical_id),
                        owner: *owner,
                        symbol: Arc::from("<script-setup>"),
                        space: verter_type_expr::locators::LocatorSymbolSpace::Type,
                    },
                    path: Arc::from(
                        vec![
                            verter_type_expr::locators::TypeBodyPathStep::TypeParamBound {
                                ordinal: u32::from(binding.ordinal),
                                position,
                            },
                        ]
                        .into_boxed_slice(),
                    ),
                },
            )
        };
        let constraint = param.constraint.as_ref().map(|constraint| {
            self.shallow_lower_type_expr_with_context_at_locator(
                constraint,
                &bound_locator(verter_type_expr::locators::TypeParamBoundPosition::Constraint),
                env,
                scope,
                name_resolution,
                scope_payload,
                shadowing,
                substitutions,
                reduction_context,
            )
        });
        let default = param.default.as_ref().map(|default| {
            self.shallow_lower_type_expr_with_context_at_locator(
                default,
                &bound_locator(verter_type_expr::locators::TypeParamBoundPosition::Default),
                env,
                scope,
                name_resolution,
                scope_payload,
                shadowing,
                substitutions,
                reduction_context,
            )
        });
        graph.intern_node_with_scope(
            SemanticNodeData::TypeParam {
                decl,
                param_index: binding.ordinal,
                constraint,
                default,
                display_name: Arc::clone(&binding.name),
            },
            scope.clone(),
        )
    }

    #[cfg(test)]
    pub(super) fn lower_script_setup_type_params_for_tests(
        &self,
        canonical: &str,
    ) -> Vec<SemanticNodeId> {
        let indexed = self
            .ctx
            .ensure_indexed_ready_serve(canonical)
            .expect("script-setup test file must be indexed")
            .indexed;
        let bundle = self
            .ctx
            .prepared_decl_bundle(canonical)
            .expect("script-setup test file must have a prepared bundle");
        let (owner, owner_scope) = bundle
            .owner_scopes
            .iter()
            .find(|(_, scope)| !scope.script_setup_type_bindings.is_empty())
            .expect("script-setup owner scope");
        let owner = *owner;
        let scope = NodeScopeId::File {
            canonical_id: Arc::from(canonical),
            owner,
            whole_hash: indexed.whole_hash,
            local_scope: None,
        };
        let scope_payload =
            crate::resolver_core::bare_name_resolve::DeclarationScopePayload::from_bundle(
                &bundle, owner,
            );
        let shadowing = crate::resolver_core::scope_shadowing::ScopeShadowing::from_scope_payload(
            Some(&scope_payload),
        );
        let env = FxHashMap::default();
        let name_resolution = FxHashMap::default();
        let mut substitutions = Vec::new();
        let mut bindings: Vec<_> = owner_scope.script_setup_type_bindings.values().collect();
        bindings.sort_by_key(|binding| binding.ordinal);
        bindings
            .into_iter()
            .map(|binding| {
                self.lower_script_setup_type_param_binding(
                    binding,
                    &env,
                    &scope,
                    &name_resolution,
                    Some(&scope_payload),
                    &shadowing,
                    &mut substitutions,
                    ProjectionReductionContext::structural_transit(),
                )
            })
            .collect()
    }

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
        name_resolution: &FxHashMap<std::sync::Arc<str>, ResolvedRootIdentity>,
        scope_payload: Option<&DeclarationScopePayload>,
        shadowing: &ScopeShadowing,
        substitutions: &mut Vec<(Arc<str>, SemanticNodeId)>,
        reduction_context: ProjectionReductionContext,
    ) -> SemanticNodeId {
        let infer_binders = crate::semantic_query::InferBinderFactory::new(scope, expr);
        self.lower_type_expr_with_infer_factory(
            &infer_binders,
            expr,
            env,
            scope,
            name_resolution,
            scope_payload,
            shadowing,
            substitutions,
            reduction_context,
        )
    }

    /// Locator-anchored eager lowering for an authored payload that has
    /// already been re-borrowed and validated by its producer.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn shallow_lower_type_expr_with_context_at_locator(
        &self,
        expr: &TypeExpr,
        locator: &verter_type_expr::locators::AuthoredBodyLocator,
        env: &FxHashMap<String, SemanticNodeId>,
        scope: &NodeScopeId,
        name_resolution: &FxHashMap<std::sync::Arc<str>, ResolvedRootIdentity>,
        scope_payload: Option<&DeclarationScopePayload>,
        shadowing: &ScopeShadowing,
        substitutions: &mut Vec<(Arc<str>, SemanticNodeId)>,
        reduction_context: ProjectionReductionContext,
    ) -> SemanticNodeId {
        let infer_binders =
            crate::semantic_query::InferBinderFactory::for_authored_locator(scope, expr, locator);
        self.lower_type_expr_with_infer_factory(
            &infer_binders,
            expr,
            env,
            scope,
            name_resolution,
            scope_payload,
            shadowing,
            substitutions,
            reduction_context,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn lower_type_expr_with_infer_factory(
        &self,
        infer_binders: &crate::semantic_query::InferBinderFactory,
        expr: &TypeExpr,
        env: &FxHashMap<String, SemanticNodeId>,
        scope: &NodeScopeId,
        name_resolution: &FxHashMap<std::sync::Arc<str>, ResolvedRootIdentity>,
        scope_payload: Option<&DeclarationScopePayload>,
        shadowing: &ScopeShadowing,
        substitutions: &mut Vec<(Arc<str>, SemanticNodeId)>,
        reduction_context: ProjectionReductionContext,
    ) -> SemanticNodeId {
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
                        self.lower_type_expr_with_infer_factory(
                            infer_binders,
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
                        self.lower_type_expr_with_infer_factory(
                            infer_binders,
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
            // [`crate::resolver_core::prepared_decl::TypeParamBinding`]
            // (the content-free name + ordinal fact pair); the ONE
            // shared helper re-borrows the clause lease-only from the
            // pinned artifact and constructs the node — never an
            // intermediate `PreparedTypeDecl` wrapper.
            TypeExpr::Ref {
                name,
                type_arguments,
            } if type_arguments.is_empty()
                && scope_payload
                    .map(|payload| payload.scope_type_bindings().contains_key(name.as_ref()))
                    .unwrap_or(false) =>
            {
                let binding = scope_payload
                    .and_then(|payload| payload.scope_type_bindings().get(name.as_ref()))
                    .expect("matched on scope_type_bindings.contains_key above");
                self.lower_script_setup_type_param_binding(
                    binding,
                    env,
                    scope,
                    name_resolution,
                    scope_payload,
                    shadowing,
                    substitutions,
                    reduction_context,
                )
            }
            // Named type reference (`type Foo<T> = { y: Other<T> }` ->
            // `Other<T>` at `y`'s position). The head resolves FIRST through the
            // shared `resolve_bare_ref_head` resolver -- the ONE bare-name
            // resolver, equally reached from carrier-subject normalization --
            // which performs builtin-shadowing-aware utility / Promise carriers,
            // the bare-name + augmentation + enum resolution, the recursive-ref
            // back-edge, and the route through `DeclRef` / `InstantiationRef`
            // (carrier modes) or `ResolveDecl` / `Instantiate` (eager modes).
            // The type-args lower LAZILY through the passed closure: an
            // unresolvable head never lowers dead args; the closure fires only on
            // the branches that consume the args (substituting INTO the resolved
            // decl body, never the macro-T own body), keeping the helper
            // typed-IR-only. Self-referential types are bounded by the memo's
            // same-path recursion sentinel.
            TypeExpr::Ref {
                name,
                type_arguments,
            } => {
                let ctx = crate::project_semantic_dispatch::carrier::CarrierResolverContext::new(
                    env,
                    scope,
                    name_resolution,
                    scope_payload,
                    shadowing,
                    reduction_context,
                );
                // Lower the type-args LAZILY — only on the branch the head
                // resolves to (the helper invokes this closure exactly on a
                // Promise / builtin / carrier-mode / eager `Instantiate`
                // branch). An UNRESOLVABLE head misses without lowering +
                // dispatching dead args. The closure routes through the SAME
                // structural lowering (typed-IR-only); the carrier-substituted
                // args carry `Structural` provenance.
                let arg_context = reduction_context.into_structural_provenance();
                self.resolve_bare_ref_head(&ctx, name, type_arguments.len(), || {
                    let arg_ids: Vec<SemanticNodeId> = type_arguments
                        .iter()
                        .map(|arg| {
                            self.lower_type_expr_with_infer_factory(
                                infer_binders,
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
                    Arc::from(arg_ids.into_boxed_slice())
                })
            }
            TypeExpr::Union(arms) => {
                let mut arm_ids: Vec<SemanticNodeId> = Vec::with_capacity(arms.len());
                for arm in arms.iter() {
                    arm_ids.push(self.lower_type_expr_with_infer_factory(
                        infer_binders,
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
                    arm_ids.push(self.lower_type_expr_with_infer_factory(
                        infer_binders,
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
                // A spread-bearing object literal folds through the shared
                // spread materializer (ordered left fold over direct members
                // and spread operands) instead of the plain member loop below.
                if obj
                    .properties
                    .iter()
                    .any(|m| matches!(m, ObjectMember::Spread(_)))
                {
                    return self.lower_spread_object_literal(
                        obj,
                        infer_binders,
                        env,
                        scope,
                        name_resolution,
                        scope_payload,
                        shadowing,
                        substitutions,
                        reduction_context,
                    );
                }
                let mut entries = Vec::with_capacity(obj.properties.len());
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
                            let value = self.lower_type_expr_with_infer_factory(
                                infer_binders,
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
                            // This is the canonical typed-IR producer of the
                            // macro-root provenance bit consumed by terminal
                            // projections.
                            entries.push(SurfaceEntry::Member(SurfaceMember {
                                key: self.lower_authored_property_key(
                                    &prop.key,
                                    infer_binders,
                                    env,
                                    scope,
                                    name_resolution,
                                    scope_payload,
                                    shadowing,
                                    substitutions,
                                    reduction_context.into_structural_provenance(),
                                ),
                                value,
                                optional: prop.optional,
                                readonly: prop.readonly,
                                method_kind: None,
                                has_implementation_body: false,
                                // Carry the IR member's declared accessibility
                                // verbatim onto the graph payload (Public for
                                // every non-class origin).
                                visibility: prop.visibility,
                                // Carry the IR member's excess-property
                                // provenance verbatim (`FreshOwn` only from
                                // direct object-literal materialization).
                                excess_origin: prop.excess_origin,
                                // Carry the IR member's OXC declaration-site
                                // spans verbatim onto the graph payload.
                                spans: prop.spans,
                                // The member's DECLARATION lives in THIS object's
                                // lowering file — independent of where its value
                                // type resolves (an unresolved `MissingType` value
                                // is scope-less but the member still declares here).
                                declaration_origin: scope.canonical_file(),
                                declared_in_macro_type_arg: reduction_context.own_body_stamp(),
                                // Leaf stamping of the surface-merge role from
                                // the threaded context (by design):
                                // an interface/class own `Object` arm flows
                                // `OwnBody`, a heritage reference arm flows
                                // `Heritage`, everything else stays `Authored`.
                                merge_role: reduction_context.role_stamp(),
                            }));
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
                            register_eager_function_alias(
                                infer_binders,
                                &function_expr,
                                &method.function,
                            );
                            // Method VALUE (its function shape) lowers
                            // structurally — see the `ObjectMember::Property`
                            // companion note. Only the method's presence on
                            // THIS object is macro-T own-body, not the
                            // function's nested parameter/return objects.
                            let value = self.lower_type_expr_with_infer_factory(
                                infer_binders,
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
                            entries.push(SurfaceEntry::Member(SurfaceMember {
                                key: self.lower_authored_property_key(
                                    &method.key,
                                    infer_binders,
                                    env,
                                    scope,
                                    name_resolution,
                                    scope_payload,
                                    shadowing,
                                    substitutions,
                                    reduction_context.into_structural_provenance(),
                                ),
                                value,
                                optional: method.optional,
                                readonly: false,
                                method_kind: Some(method.method_kind),
                                has_implementation_body: method.has_implementation_body,
                                // Carry the IR method's declared accessibility
                                // (Public for every non-class origin).
                                visibility: method.visibility,
                                // Carry the IR method's excess-property
                                // provenance verbatim.
                                excess_origin: method.excess_origin,
                                // Carry the IR method's OXC member spans.
                                spans: method.spans,
                                // Declaration file of THIS method (see the
                                // `Property` companion note).
                                declaration_origin: scope.canonical_file(),
                                declared_in_macro_type_arg: reduction_context.own_body_stamp(),
                                // Leaf stamping of the surface-merge role —
                                // mirrors the `Property` arm.
                                merge_role: reduction_context.role_stamp(),
                            }));
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
                            register_eager_function_alias(infer_binders, &function_expr, func);
                            let fn_id = self.lower_type_expr_with_infer_factory(
                                infer_binders,
                                &function_expr,
                                env,
                                scope,
                                name_resolution,
                                scope_payload,
                                shadowing,
                                substitutions,
                                reduction_context,
                            );
                            entries.push(SurfaceEntry::CallSignature(fn_id));
                        }
                        ObjectMember::ConstructSignature(func) => {
                            let function_expr = TypeExpr::ConstructorType(Arc::new(func.clone()));
                            register_eager_function_alias(infer_binders, &function_expr, func);
                            let fn_id = self.lower_type_expr_with_infer_factory(
                                infer_binders,
                                &function_expr,
                                env,
                                scope,
                                name_resolution,
                                scope_payload,
                                shadowing,
                                substitutions,
                                reduction_context,
                            );
                            entries.push(SurfaceEntry::ConstructSignature(fn_id));
                        }
                        // Unreachable by construction: the spread-bearing
                        // branch above routes the whole object through the
                        // spread lowering before this member loop runs.
                        ObjectMember::Spread(_) => {}
                        ObjectMember::IndexSignature(sig) => {
                            let key_type = self.lower_type_expr_with_infer_factory(
                                infer_binders,
                                &sig.key_type,
                                env,
                                scope,
                                name_resolution,
                                scope_payload,
                                shadowing,
                                substitutions,
                                reduction_context,
                            );
                            let value_type = self.lower_type_expr_with_infer_factory(
                                infer_binders,
                                &sig.value_type,
                                env,
                                scope,
                                name_resolution,
                                scope_payload,
                                shadowing,
                                substitutions,
                                reduction_context,
                            );
                            entries.push(SurfaceEntry::IndexSignature(IndexSignature {
                                key_type,
                                value_type,
                                readonly: sig.readonly,
                                // Carry the IR index signature's OXC spans.
                                spans: sig.spans,
                                // Declaration file of THIS index signature —
                                // from the object's lowering scope, not the
                                // (possibly scope-less) value-type node.
                                declaration_origin: scope.canonical_file(),
                            }));
                        }
                    }
                }
                let has_index_signature = entries
                    .iter()
                    .any(|entry| matches!(entry, SurfaceEntry::IndexSignature(_)));
                let view = SurfaceView::from_entries(entries, None, has_index_signature);
                graph.intern_node_with_scope(SemanticNodeData::Object(view), scope.clone())
            }
            // Arrays publish through the dedicated
            // `SemanticNodeData::Array { element, readonly }` variant per
            // B4 + §7.14: array indexed-access is hot and must not
            // pay generic `Array<T>` declaration-instantiation cost on
            // every access.
            TypeExpr::Array { element, readonly } => {
                let element_id = self.lower_type_expr_with_infer_factory(
                    infer_binders,
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
                    let value = self.lower_type_expr_with_infer_factory(
                        infer_binders,
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
                        self.lower_type_expr_with_infer_factory(
                            infer_binders,
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
            TypeExpr::Parenthesized(inner) => self.lower_type_expr_with_infer_factory(
                infer_binders,
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
                        owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
                        whole_hash: HashValue::default(),
                        decl_name: Arc::from("<mapper-param>"),
                    },
                    NodeScopeId::File {
                        canonical_id,
                        owner,
                        whole_hash,
                        ..
                    } => DeclIdentity {
                        canonical_id: Arc::clone(canonical_id),
                        owner: *owner,
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
                let (source_sem, key_space_sem, base_infer_name) = match source.as_ref() {
                    // `{ [K in keyof T]: ... }` — extract T.
                    TypeExpr::KeyOf(inner) => {
                        let inner_id = self.lower_type_expr_with_infer_factory(
                            infer_binders,
                            inner,
                            env,
                            scope,
                            name_resolution,
                            scope_payload,
                            shadowing,
                            substitutions,
                            reduction_context,
                        );
                        // `keyof infer T` is the descriptor of the exact
                        // reverse-homomorphic pattern. The selected `Infer`
                        // is intentionally open until the enclosing
                        // conditional relation fixes it, so eagerly asking
                        // the `KeyOf` reducer here can only return `Miss` and
                        // destroy the descriptor. Preserve that exact
                        // structurally selected operand as a `KeyOf` shell in
                        // every reduction mode; ordinary concrete operands
                        // retain the established eager-reduction path.
                        let selected_base_is_infer = matches!(
                            graph.node_data(inner_id).as_deref(),
                            Some(SemanticNodeData::Infer { .. })
                        );
                        let key_space = if selected_base_is_infer {
                            graph.intern_node_with_scope(
                                SemanticNodeData::KeyOf { base: inner_id },
                                scope.clone(),
                            )
                        } else if crate::semantic_query::may_reduce_operator(reduction_context) {
                            match self.execute_type_node(SemanticQueryKey::KeyOf {
                                base: inner_id,
                                context: reduction_context,
                            }) {
                                QueryResult::Value(SemanticQueryOutput { value: id, .. }) => id,
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
                        let base_infer = match graph.node_data(inner_id).as_deref() {
                            Some(SemanticNodeData::Infer { name, binder }) => {
                                Some((Arc::clone(name), binder.clone()))
                            }
                            _ => None,
                        };
                        (inner_id, key_space, base_infer)
                    }
                    // Fallback: the source shape IS the key space.
                    _ => {
                        let lowered = self.lower_type_expr_with_infer_factory(
                            infer_binders,
                            source,
                            env,
                            scope,
                            name_resolution,
                            scope_payload,
                            shadowing,
                            substitutions,
                            reduction_context,
                        );
                        (lowered, lowered, None)
                    }
                };

                // A reverse-homomorphic source introduces `infer T` from the
                // exact lowered `keyof infer T` operand. Seed only that selected
                // declaration as a scoped reference while lowering the mapped
                // body; an ambient/imported same-name reference never enters
                // this environment. Insert the mapped binder afterwards so it
                // remains the innermost binding when the names collide.
                let mut mapper_env = env.clone();
                if let Some((base_infer_name, binder)) = base_infer_name {
                    let reference = graph.intern_node_with_scope(
                        SemanticNodeData::InferRef {
                            name: Arc::clone(&base_infer_name),
                            binder,
                        },
                        scope.clone(),
                    );
                    mapper_env.insert(base_infer_name.to_string(), reference);
                }
                mapper_env.insert(parameter.clone(), parameter_id);

                let value_sem = self.lower_type_expr_with_infer_factory(
                    infer_binders,
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
                    self.lower_type_expr_with_infer_factory(
                        infer_binders,
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
                    self, source_sem, &mapper,
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
                let base_id = self.lower_type_expr_with_infer_factory(
                    infer_binders,
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
                let obj_id = self.lower_type_expr_with_infer_factory(
                    infer_binders,
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
                    TypeExpr::TypeOf(value_ref) => self
                        .unique_symbol_identity_for_typeof(
                            value_ref,
                            scope,
                            name_resolution,
                            scope_payload,
                        )
                        .map(IndexKey::UniqueSymbol),
                    _ => None,
                };
                let index_key = match folded_key {
                    Some(key) => key,
                    None => {
                        let idx_id = self.lower_type_expr_with_infer_factory(
                            infer_binders,
                            index,
                            env,
                            scope,
                            name_resolution,
                            scope_payload,
                            shadowing,
                            substitutions,
                            reduction_context,
                        );
                        IndexKey::Computed(idx_id)
                    }
                };
                let should_defer = matches!(index_key, IndexKey::Computed(_))
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
                    )
                    .with_orthogonal_axes_from(reduction_context);
                let check_id = self.lower_type_expr_with_infer_factory(
                    infer_binders,
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
                let extends_path = infer_binders.path_for_expr(expr).child(
                    crate::semantic_query::infer_binder_names::
                        InferSyntaxPathStep::ConditionalExtends,
                );
                let infer_sites =
                    crate::semantic_query::infer_binder_names::collect_extends_infer_declarations(
                        extends,
                        &extends_path,
                    );
                let mut extends_env_owned;
                let mut declarations = Vec::with_capacity(infer_sites.len());
                let extends_env = if infer_sites.is_empty() {
                    env
                } else {
                    extends_env_owned = env.clone();
                    for site in infer_sites {
                        let binder = infer_binders.binder_at(&site.path);
                        let declaration = graph.intern_node_with_scope(
                            SemanticNodeData::Infer {
                                name: Arc::clone(&site.name),
                                binder: binder.clone(),
                            },
                            scope.clone(),
                        );
                        extends_env_owned
                            .insert(infer_declaration_env_key(site.name.as_ref()), declaration);
                        declarations.push((site.name, binder));
                    }
                    &extends_env_owned
                };
                let extends_id = self.lower_type_expr_with_infer_factory(
                    infer_binders,
                    extends,
                    extends_env,
                    scope,
                    name_resolution,
                    scope_payload,
                    shadowing,
                    substitutions,
                    extends_context,
                );
                let true_env_owned;
                let true_env = if declarations.is_empty() {
                    env
                } else {
                    let mut extended = env.clone();
                    for (name, binder) in declarations {
                        let reference = graph.intern_node_with_scope(
                            SemanticNodeData::InferRef {
                                name: Arc::clone(&name),
                                binder,
                            },
                            scope.clone(),
                        );
                        extended.insert(name.to_string(), reference);
                    }
                    true_env_owned = extended;
                    &true_env_owned
                };
                let true_id = self.lower_type_expr_with_infer_factory(
                    infer_binders,
                    true_type,
                    true_env,
                    scope,
                    name_resolution,
                    scope_payload,
                    shadowing,
                    substitutions,
                    reduction_context,
                );
                let false_id = self.lower_type_expr_with_infer_factory(
                    infer_binders,
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
                let (scope_canonical_id, scope_owner) = match scope {
                    NodeScopeId::File {
                        canonical_id,
                        owner,
                        ..
                    } => (Arc::clone(canonical_id), *owner),
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
                            owner: scope_owner,
                            local_scope: None,
                            binder_scope_id: crate::semantic_query::BinderScopeId::file_scope(
                                scope_owner,
                            ),
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
                                    owner: scope_owner,
                                    local_scope: None,
                                    binder_scope_id:
                                        crate::semantic_query::BinderScopeId::file_scope(
                                            scope_owner,
                                        ),
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
                            .map(|segment| {
                                PathSegment::Member(crate::semantic_query::PropertyKey::identifier(
                                    Arc::from(segment.as_str()),
                                ))
                            })
                            .collect::<Vec<_>>()
                            .into_boxed_slice(),
                    );
                    result = match self.execute_type_node(SemanticQueryKey::ProjectPath {
                        base: result,
                        path,
                        context: crate::semantic_query::ProjectionReductionContext::published(
                            ProjectionMode::Navigate,
                        )
                        .with_orthogonal_axes_from(reduction_context),
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
                            self.lower_type_expr_with_infer_factory(
                                infer_binders,
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
            // canonical `SemanticNodeData::Signature` carrier with
            // lowered parameters and return type. Type parameters
            // lower to `TypeParamDecl` — constraints/defaults lower
            // recursively. `RecursiveRef`, `Infer`, `Rest`, and
            // `Unknown` remain scratch-only per §7.14.
            //
            // `ConstructorType` (a bare `new (...) => R`) lowers through
            // the SAME `SemanticNodeData::Signature` path with
            // `kind: Construct` — the call/construct distinction is
            // SEMANTIC and must survive lowering (`Construct` raises back
            // to `TypeExpr::ConstructorType`). Without this explicit arm
            // the wildcard `_ => opaque(QueryError::Miss)` below would
            // regress constructor-type props to `Unknown("semanticMiss")`.
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
                            self.lower_type_expr_with_infer_factory(
                                infer_binders,
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
                            self.lower_type_expr_with_infer_factory(
                                infer_binders,
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
                                // The shared signature-scoped binder
                                // convention (`BinderIdentityMode::Signature`):
                                // display-name-keyed at ordinal 0.
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
                        ty: self.lower_type_expr_with_infer_factory(
                            infer_binders,
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
                let (return_type, return_carrier) = match &func.flow_return {
                    // A body-derived return is demanded from the
                    // whole-function producer through the sealed helper:
                    // the extractor marked the served position with the
                    // declaration name; canonical / owner fill from THIS
                    // lowering scope (the defining file's). The carrier
                    // records the SAME served position so the call resolver
                    // holds the `FlowReturn` obligation instead of treating
                    // the evaluated node as a concrete seed.
                    Some(identity) => {
                        let mut identity = identity.as_ref().clone();
                        let scope_canonical = match scope {
                            NodeScopeId::File {
                                canonical_id,
                                owner,
                                ..
                            } => {
                                identity.anchor.canonical_id = Arc::clone(canonical_id);
                                identity.anchor.owner = *owner;
                                Some(Arc::clone(canonical_id))
                            }
                            _ => None,
                        };
                        match scope_canonical {
                            Some(scope_canonical) => {
                                let carrier =
                                    crate::semantic_query::SignatureReturnCarrier::Function(
                                        verter_type_expr::facts::FunctionReturnSource::Flow(
                                            identity.clone(),
                                        ),
                                    );
                                let return_type = match self.execute_function_return_source(
                                    &verter_type_expr::facts::FunctionReturnSource::Flow(identity),
                                    scope_canonical.as_ref(),
                                ) {
                                    super::flow_return::FunctionReturnNode::Flow(result) => {
                                        result.return_type()
                                    }
                                    _ => self.opaque(QueryError::Miss),
                                };
                                (return_type, carrier)
                            }
                            None => (
                                self.opaque(QueryError::Miss),
                                crate::semantic_query::SignatureReturnCarrier::Function(
                                    verter_type_expr::facts::FunctionReturnSource::Absent,
                                ),
                            ),
                        }
                    }
                    None => match func.return_type.as_deref() {
                        Some(ret) => {
                            let return_type = self.lower_type_expr_with_infer_factory(
                                infer_binders,
                                ret,
                                env,
                                scope,
                                name_resolution,
                                scope_payload,
                                shadowing,
                                substitutions,
                                reduction_context,
                            );
                            (
                                return_type,
                                crate::semantic_query::SignatureReturnCarrier::Declared(
                                    return_type,
                                ),
                            )
                        }
                        None => (
                            self.opaque(QueryError::Miss),
                            crate::semantic_query::SignatureReturnCarrier::Function(
                                verter_type_expr::facts::FunctionReturnSource::Absent,
                            ),
                        ),
                    },
                };
                let type_parameters: Vec<TypeParamDecl> = func
                    .type_parameters
                    .iter()
                    .map(|tp| TypeParamDecl {
                        name: Arc::from(tp.name.as_str()),
                        // The exact binder node interned above for this own
                        // parameter — the identity inference binds.
                        param: env
                            .get(tp.name.as_str())
                            .copied()
                            .expect("own type parameter binder interned in the scoped env"),
                        constraint: tp.constraint.as_deref().map(|c| {
                            self.lower_type_expr_with_infer_factory(
                                infer_binders,
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
                            self.lower_type_expr_with_infer_factory(
                                infer_binders,
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
                        is_const: tp.is_const,
                    })
                    .collect();
                let kind = match expr {
                    TypeExpr::ConstructorType(_) => crate::semantic_query::SignatureKind::Construct,
                    _ => crate::semantic_query::SignatureKind::Call,
                };
                graph.intern_node_with_scope(
                    SemanticNodeData::Signature {
                        kind,
                        params: Arc::from(params.into_boxed_slice()),
                        return_type,
                        type_parameters: Arc::from(type_parameters.into_boxed_slice()),
                        // The generic type-expression lowering carries no
                        // declaration occurrence: occurrence-aware provenance
                        // arrives through the locator-driven rails (the
                        // `LowerLocator` producer patches the ROOT signature).
                        occurrence: None,
                        return_carrier,
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
            TypeExpr::Infer { name } => {
                if let Some(declaration) = env.get(&infer_declaration_env_key(name)) {
                    *declaration
                } else {
                    graph.intern_node_with_scope(
                        SemanticNodeData::Infer {
                            name: Arc::from(name.as_str()),
                            binder: infer_binders.binder_for_expr(expr),
                        },
                        scope.clone(),
                    )
                }
            }
            // `import("./m")` / `import("./m").Member` / `typeof import("./m")`
            // — a dynamic-import type reference. Resolve the module specifier
            // to a canonical file id (TS-first, workspace-bounded) through the
            // SHARED module resolver, then route to the value-namespace
            // (`typeof_query`) or TYPE-export (bare import) rail. No raw-text
            // reparsing — the typed-IR carrier drives the whole resolution.
            // `import("specifier").qualifier<args>` / `typeof import("...")`.
            // Lower the args structurally HERE, then route through the shared
            // `resolve_import_type_head` resolver, whose TYPE-position qualifier
            // head segment delegates to the ONE `resolve_bare_ref_head` (over an
            // injected name-resolution entry) -- no parallel import resolver. The
            // owner canonical (which resolves the relative specifier) is the
            // file scope; a non-file scope is an honest miss.
            TypeExpr::ImportType {
                specifier,
                qualifier,
                typeof_query,
                type_arguments,
            } => {
                let NodeScopeId::File {
                    canonical_id: owner_canonical,
                    ..
                } = scope
                else {
                    return self.opaque(QueryError::Miss);
                };
                let ctx = crate::project_semantic_dispatch::carrier::CarrierResolverContext::new(
                    env,
                    scope,
                    name_resolution,
                    scope_payload,
                    shadowing,
                    reduction_context,
                );
                // Lower the type-args LAZILY — only when the import head actually
                // consumes them (a resolvable specifier + terminal qualifier).
                // An unresolvable specifier / multi-segment-with-args misses
                // without lowering + dispatching dead args. The import-type arm
                // lowers args under the caller's `reduction_context` (NOT
                // `into_structural_provenance`) — the import-type args stay tied
                // to the caller's evaluation mode/provenance.
                self.resolve_import_type_head(
                    &ctx,
                    owner_canonical.as_ref(),
                    specifier,
                    qualifier,
                    *typeof_query,
                    type_arguments.len(),
                    || {
                        let arg_ids: Vec<SemanticNodeId> = type_arguments
                            .iter()
                            .map(|arg| {
                                self.lower_type_expr_with_infer_factory(
                                    infer_binders,
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
                        Arc::from(arg_ids.into_boxed_slice())
                    },
                )
            }
            // Conditionals, rest, recursive-ref, and unknown
            // constructs remain out of this pass's scope — they route
            // through their own dispatch builders (conditional /
            // userland-equivalence) or stay solver-scratch-only.
            _ => self.opaque(QueryError::Miss),
        }
    }
}
