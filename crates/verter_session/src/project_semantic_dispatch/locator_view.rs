//! Post-substitution VIEW projection of a fetched locator shape — the
//! demand-side half of the shape/view split.
//!
//! The `LowerLocator` query produces one ROLE-FREE, strictly-unsubstituted
//! body shape per locator/source-env (see `locator_shape.rs`). `Instantiate`
//! fetches that shape, applies its `args`/defaults via semantic type-param
//! substitution, and ONLY THEN projects the demand-specific VIEW through
//! this module: the caller-relative [`ProjectionStamp`] (surface provenance
//! plus inbound merge role) is applied to the substituted shape, and the
//! deferred carriers (conditionals, mapped types, `keyof`, indexed
//! accesses, instantiation refs, `typeof`, unresolved bare/import heads)
//! are evaluated under the caller's `ProjectionReductionContext` — the same
//! per-position dispatch decisions the reducing lowering entry
//! (`shallow_lower_type_expr_with_context`) applies while lowering authored
//! IR, mirrored onto graph nodes. Stamps are re-interned VIEW nodes; the
//! cached neutral shape nodes are never republished restamped.
//!
//! Reducing a type-parameter-mentioning body before substitution is a
//! defect (later substitution cannot repair the result), which is why this
//! projection runs strictly AFTER the substitution step.

use std::sync::Arc;

use rustc_hash::FxHashMap;
use verter_semantic::analysis::type_eval::TypeDeclKind;
use verter_semantic::analysis::type_solver::host::ResolvedRootIdentity;

use super::carrier::CarrierResolverContext;
use super::ProjectSemanticDispatch;
use crate::resolver_core::bare_name_resolve::DeclarationScopePayload;
use crate::resolver_core::scope_shadowing::ScopeShadowing;
use crate::semantic_query::{
    may_reduce_operator, DeclIdentity, FunctionParam, IndexKey, IndexSignature, MapperKey,
    MemberMergeRole, NodeScopeId, PathSegment, PrimitiveKind, ProjectionMode,
    ProjectionReductionContext, QueryError, QueryResult, ReductionDemand, ResolveDeclKey, ScopeId,
    SemanticNodeData, SemanticNodeId, SemanticQueryApi, SemanticQueryKey, SemanticQueryOutput,
    SurfaceMember, SurfaceView, TupleElement, TypeParamDecl, ValueRootKey,
};

/// The demand-specific stamp `Instantiate`/`ProjectPath` applies to a
/// fetched shape AFTER substitution: the caller's surface provenance, the
/// inbound merge role, and the authored arm kind the per-arm rule derived
/// from the shape's own topology. Never part of shape-node identity.
#[derive(Debug, Clone, Copy)]
pub(super) struct ProjectionStamp {
    provenance: crate::semantic_query::SurfaceProvenanceContext,
    inbound_merge_role: MemberMergeRole,
    authored_arm_kind: AuthoredArmKind,
}

/// Authored arm kind, read from locator-shape TOPOLOGY (an inline object
/// arm vs a reference/other arm vs the whole body) — never stored on shape
/// nodes.
#[derive(Debug, Clone, Copy)]
pub(super) enum AuthoredArmKind {
    /// An inline object-literal own-body arm (or a whole object body).
    OwnBodyObject,
    /// A reference / non-object arm of an authored intersection or a
    /// declaration's heritage clause.
    ReferenceArm,
    /// The whole body, verbatim (no per-arm discrimination applies).
    WholeBody,
}

/// How a declaration-body REFERENCE arm (an `extends` heritage carrier / a
/// non-object authored-intersection arm) evaluates during view projection.
///
/// The two consumers of a projected body have different arm contracts: a
/// SINGLE declaration's `Intersection` body flows to the role-driven
/// intersection surface merge (stamped merge roles classify members, so a
/// reference arm may evaluate eagerly under the caller's demand), while a
/// `MergedDecl` contributor flows to the TOPOLOGY-driven peer-merge reducer
/// (`Intersection([heritage refs…, own Object])`, heritage arms preserved
/// and resolved lazily under the heritage-overlay role) — an eagerly
/// materialised heritage reference there is indistinguishable from an own
/// `Object` arm and silently loses own-body-shadows-heritage precedence.
#[derive(Debug, Clone, Copy)]
enum ReferenceArmDemand {
    /// Evaluate the reference arm under the caller's projection mode.
    CallerMode,
    /// Keep the reference arm a DEFERRED carrier (eager modes demote to
    /// `Navigate`), preserving the arm topology for the peer-merge reducer.
    Deferred,
}

impl ProjectionStamp {
    fn new(
        context: ProjectionReductionContext,
        inbound_merge_role: MemberMergeRole,
        authored_arm_kind: AuthoredArmKind,
    ) -> Self {
        Self {
            provenance: context.provenance,
            inbound_merge_role,
            authored_arm_kind,
        }
    }

    /// The projection context this stamp applies to its arm: an own-body
    /// object arm keeps the caller's provenance and carries the inbound
    /// merge role; a reference arm decays to structural provenance; the
    /// whole-body kind leaves the caller's context untouched.
    fn stamped_context(&self, base: ProjectionReductionContext) -> ProjectionReductionContext {
        match self.authored_arm_kind {
            AuthoredArmKind::OwnBodyObject => base
                .with_provenance(self.provenance)
                .with_merge_role(self.inbound_merge_role),
            AuthoredArmKind::ReferenceArm => base
                .into_structural_provenance()
                .with_merge_role(self.inbound_merge_role),
            AuthoredArmKind::WholeBody => base.with_provenance(self.provenance),
        }
    }
}

/// The scope-resolution inputs of one view projection — the same value-side
/// inputs the reducing lowering entry receives, threaded to the shared
/// bare-name / import-head resolvers at the demand points.
pub(super) struct LocatorViewInputs<'a> {
    pub(super) env: &'a FxHashMap<String, SemanticNodeId>,
    pub(super) scope: &'a NodeScopeId,
    pub(super) name_resolution: &'a FxHashMap<String, ResolvedRootIdentity>,
    pub(super) scope_payload: Option<&'a DeclarationScopePayload>,
    pub(super) shadowing: &'a ScopeShadowing,
}

/// Per-projection memo so shared sub-graphs project once per context.
type ViewMemo = FxHashMap<(SemanticNodeId, ProjectionReductionContext), SemanticNodeId>;

impl<'a> ProjectSemanticDispatch<'a> {
    /// Project the substituted decl-body shape into the caller's demanded
    /// view, applying the per-arm [`ProjectionStamp`] rule:
    ///
    /// - a `MergedDecl` body projects each contributor as an OWN-body
    ///   surface (preserving the distinct peer-merge carrier);
    /// - an `Intersection` body stamps inline object arms as own-body
    ///   (caller provenance + `OwnBody` role) and reference arms as
    ///   structural with the declaration-kind role (`Heritage` for an
    ///   interface/class, `Authored` for an alias);
    /// - a whole `Object` body is its own own-body arm;
    /// - any other body projects under the caller's context verbatim.
    pub(super) fn project_located_decl_body(
        &self,
        shape: SemanticNodeId,
        decl_kind: TypeDeclKind,
        inputs: &LocatorViewInputs<'_>,
        substitutions: &mut Vec<(Arc<str>, SemanticNodeId)>,
        context: ProjectionReductionContext,
    ) -> SemanticNodeId {
        // The declaration-kind role stamped onto reference arms. Two
        // consumers: on the `CallerMode` path (a single declaration's
        // Intersection body) it drives the role-driven intersection surface
        // merge — `Heritage` shadows, `Authored` intersects (the
        // interface-vs-alias collision semantics the published-surface tests
        // lock). On the `Deferred` path it rides the transit context as
        // projection identity (distinct memo slots per role) and as the
        // member stamp for any arm the transit projection still reduces
        // (e.g. a closed conditional); the peer-merge walker re-derives the
        // heritage classification for carrier arms from topology.
        let reference_arm_role = match decl_kind {
            TypeDeclKind::Interface | TypeDeclKind::Class => MemberMergeRole::Heritage,
            TypeDeclKind::Alias => MemberMergeRole::Authored,
        };
        let mut memo: ViewMemo = ViewMemo::default();
        let data = self.graph().node_data(shape);
        match data.as_deref() {
            Some(SemanticNodeData::MergedDecl { contributors }) => {
                let contributors = contributors.clone();
                drop(data);
                // Per-arm heritage discrimination applies INSIDE each merged
                // contributor exactly as it does to a single declaration's
                // body: a contributor shaped `Intersection([extends Ref…,
                // own Object])` stamps its inline object arms as OWN-body
                // and its reference (heritage) arms as HERITAGE — never a
                // blanket own-body stamp over the whole contributor, which
                // would materialise the heritage reference into an `Object`
                // that the peer-merge reducer then mis-buckets as OWN
                // surface, losing own-body-shadows-heritage precedence.
                let ids: Vec<SemanticNodeId> = contributors
                    .iter()
                    .map(|contributor| {
                        self.project_decl_body_arms(
                            *contributor,
                            reference_arm_role,
                            // The peer-merge reducer consumes contributor arms
                            // by TOPOLOGY (`Intersection([heritage refs…, own
                            // Object])`, heritage arms preserved for lazy
                            // resolution under the heritage-overlay role) —
                            // so a heritage reference must reach it as a
                            // CARRIER, never eagerly materialised here.
                            ReferenceArmDemand::Deferred,
                            inputs,
                            substitutions,
                            context,
                            &mut memo,
                        )
                    })
                    .collect();
                self.graph().intern_preserving_scope(
                    shape,
                    SemanticNodeData::MergedDecl {
                        contributors: Arc::from(ids.into_boxed_slice()),
                    },
                )
            }
            Some(SemanticNodeData::Intersection(_)) => {
                drop(data);
                self.project_decl_body_arms(
                    shape,
                    reference_arm_role,
                    // A single declaration's body flows to the role-driven
                    // intersection surface merge, which classifies members by
                    // their stamped merge role — reference arms may evaluate
                    // under the caller's demand.
                    ReferenceArmDemand::CallerMode,
                    inputs,
                    substitutions,
                    context,
                    &mut memo,
                )
            }
            Some(SemanticNodeData::Object(_)) => {
                drop(data);
                let own = ProjectionStamp::new(
                    context,
                    MemberMergeRole::OwnBody,
                    AuthoredArmKind::OwnBodyObject,
                );
                self.project_view_node(
                    shape,
                    own.stamped_context(context),
                    inputs,
                    substitutions,
                    &mut memo,
                )
            }
            _ => {
                drop(data);
                let whole =
                    ProjectionStamp::new(context, context.merge_role(), AuthoredArmKind::WholeBody);
                self.project_view_node(
                    shape,
                    whole.stamped_context(context),
                    inputs,
                    substitutions,
                    &mut memo,
                )
            }
        }
    }

    /// Project one declaration-body ROOT (a whole single body or one merged
    /// contributor) applying the per-arm [`ProjectionStamp`] rule:
    ///
    /// - an `Intersection` body stamps inline object arms as own-body
    ///   (caller provenance + `OwnBody` role) and reference arms as
    ///   structural with the declaration-kind `reference_arm_role`
    ///   (`Heritage` for an interface/class, `Authored` for an alias);
    ///   reference arms evaluate per the caller's [`ReferenceArmDemand`];
    /// - a whole `Object` body is its own own-body arm;
    /// - any other shape projects as an own-body contributor surface.
    #[allow(clippy::too_many_arguments)]
    fn project_decl_body_arms(
        &self,
        body: SemanticNodeId,
        reference_arm_role: MemberMergeRole,
        reference_arm_demand: ReferenceArmDemand,
        inputs: &LocatorViewInputs<'_>,
        substitutions: &mut Vec<(Arc<str>, SemanticNodeId)>,
        context: ProjectionReductionContext,
        memo: &mut ViewMemo,
    ) -> SemanticNodeId {
        let data = self.graph().node_data(body);
        match data.as_deref() {
            Some(SemanticNodeData::Intersection(arms)) => {
                let arms = arms.clone();
                drop(data);
                let arm_ids: Vec<SemanticNodeId> = arms
                    .iter()
                    .map(|arm| {
                        let arm_kind = match self.graph().node_data(*arm).as_deref() {
                            Some(SemanticNodeData::Object(_)) => AuthoredArmKind::OwnBodyObject,
                            _ => AuthoredArmKind::ReferenceArm,
                        };
                        let role = match arm_kind {
                            AuthoredArmKind::OwnBodyObject => MemberMergeRole::OwnBody,
                            _ => reference_arm_role,
                        };
                        let stamp = ProjectionStamp::new(context, role, arm_kind);
                        let mut arm_ctx = stamp.stamped_context(context);
                        if matches!(arm_kind, AuthoredArmKind::ReferenceArm)
                            && matches!(reference_arm_demand, ReferenceArmDemand::Deferred)
                        {
                            // A DEFERRED reference arm is a TRUE carrier-only
                            // projection: the arm projects under the
                            // NON-PUBLICATION `StructuralTransit` demand (with
                            // the eager modes demoted to `Navigate`) so every
                            // materialisation gate along the arm — the mapper
                            // builtins (`Partial`/`Required`/`Readonly`)
                            // included — carrier-stops. A `Published` demand
                            // here would let a closed-arg builtin heritage ref
                            // fall through to an executed `Instantiate`, and
                            // the resulting `Object` is mis-bucketed as OWN
                            // surface by the topology-driven peer-merge
                            // reducer — inverting own-body-shadows-heritage.
                            // The stamped merge role (`Heritage` for an
                            // interface/class) is PRESERVED on the transit
                            // context; substitution env and structural
                            // provenance carry through unchanged.
                            let mode = match arm_ctx.mode {
                                ProjectionMode::Expanded | ProjectionMode::Identity => {
                                    ProjectionMode::Navigate
                                }
                                other => other,
                            };
                            arm_ctx =
                                ProjectionReductionContext::structural_transit_with_mode(mode)
                                    .with_merge_role(arm_ctx.merge_role);
                        }
                        self.project_view_node(*arm, arm_ctx, inputs, substitutions, memo)
                    })
                    .collect();
                if arm_ids.is_empty() {
                    self.graph()
                        .intern_node(SemanticNodeData::Primitive(PrimitiveKind::Never))
                } else if arm_ids.len() == 1 {
                    arm_ids[0]
                } else {
                    self.graph().intern_preserving_scope(
                        body,
                        SemanticNodeData::Intersection(Arc::from(arm_ids.into_boxed_slice())),
                    )
                }
            }
            _ => {
                drop(data);
                let own = ProjectionStamp::new(
                    context,
                    MemberMergeRole::OwnBody,
                    AuthoredArmKind::OwnBodyObject,
                );
                self.project_view_node(
                    body,
                    own.stamped_context(context),
                    inputs,
                    substitutions,
                    memo,
                )
            }
        }
    }

    /// Project one substituted shape node into the demanded view — the
    /// graph-node mirror of the reducing lowering entry's per-position
    /// dispatch decisions.
    #[allow(clippy::too_many_lines)]
    fn project_view_node(
        &self,
        node: SemanticNodeId,
        ctx: ProjectionReductionContext,
        inputs: &LocatorViewInputs<'_>,
        substitutions: &mut Vec<(Arc<str>, SemanticNodeId)>,
        memo: &mut ViewMemo,
    ) -> SemanticNodeId {
        if let Some(&done) = memo.get(&(node, ctx)) {
            return done;
        }
        crate::loop5_instrumentation::watchdog_beat();
        crate::loop5_instrumentation::watchdog_check_and_dump("project_view_node");
        let result = self.project_view_node_uncached(node, ctx, inputs, substitutions, memo);
        memo.insert((node, ctx), result);
        result
    }

    fn project_view_node_uncached(
        &self,
        node: SemanticNodeId,
        ctx: ProjectionReductionContext,
        inputs: &LocatorViewInputs<'_>,
        substitutions: &mut Vec<(Arc<str>, SemanticNodeId)>,
        memo: &mut ViewMemo,
    ) -> SemanticNodeId {
        let graph = self.graph();
        let Some(data) = graph.node_data(node) else {
            return node;
        };
        match data.as_ref() {
            // ── terminals ────────────────────────────────────────────────
            SemanticNodeData::Primitive(_)
            | SemanticNodeData::Literal(_)
            | SemanticNodeData::Opaque(_)
            | SemanticNodeData::Infer { .. }
            | SemanticNodeData::VueMacroElements(_)
            | SemanticNodeData::SyntheticBinding { .. } => node,

            // An authored raw fallback carries no lowerable structure; the
            // reducing entry resolves it to the shared miss sentinel.
            SemanticNodeData::RawFallback { .. } => self.opaque(QueryError::Miss),

            SemanticNodeData::Alias(target) => {
                let target = *target;
                drop(data);
                self.project_view_node(target, ctx, inputs, substitutions, memo)
            }

            // ── unbound type parameters keep their shells; the shell's
            //    constraint / default project under the caller's demand ──
            SemanticNodeData::TypeParam {
                decl,
                param_index,
                constraint,
                default,
                display_name,
            } => {
                let decl = decl.clone();
                let param_index = *param_index;
                let constraint = *constraint;
                let default = *default;
                let display_name = Arc::clone(display_name);
                drop(data);
                let new_constraint =
                    constraint.map(|c| self.project_view_node(c, ctx, inputs, substitutions, memo));
                let new_default =
                    default.map(|d| self.project_view_node(d, ctx, inputs, substitutions, memo));
                if new_constraint == constraint && new_default == default {
                    return node;
                }
                graph.intern_preserving_scope(
                    node,
                    SemanticNodeData::TypeParam {
                        decl,
                        param_index,
                        constraint: new_constraint,
                        default: new_default,
                        display_name,
                    },
                )
            }

            // ── composite shells ─────────────────────────────────────────
            SemanticNodeData::Union(arms) => {
                let arms = arms.clone();
                drop(data);
                let ids: Vec<SemanticNodeId> = arms
                    .iter()
                    .map(|arm| self.project_view_node(*arm, ctx, inputs, substitutions, memo))
                    .collect();
                if ids.is_empty() {
                    graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Never))
                } else if ids.len() == 1 {
                    ids[0]
                } else {
                    graph.intern_preserving_scope(
                        node,
                        SemanticNodeData::Union(Arc::from(ids.into_boxed_slice())),
                    )
                }
            }
            SemanticNodeData::Intersection(arms) => {
                let arms = arms.clone();
                drop(data);
                let ids: Vec<SemanticNodeId> = arms
                    .iter()
                    .map(|arm| self.project_view_node(*arm, ctx, inputs, substitutions, memo))
                    .collect();
                if ids.is_empty() {
                    graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Never))
                } else if ids.len() == 1 {
                    ids[0]
                } else {
                    graph.intern_preserving_scope(
                        node,
                        SemanticNodeData::Intersection(Arc::from(ids.into_boxed_slice())),
                    )
                }
            }
            SemanticNodeData::MergedDecl { contributors } => {
                let contributors = contributors.clone();
                drop(data);
                let ids: Vec<SemanticNodeId> = contributors
                    .iter()
                    .map(|c| self.project_view_node(*c, ctx, inputs, substitutions, memo))
                    .collect();
                graph.intern_preserving_scope(
                    node,
                    SemanticNodeData::MergedDecl {
                        contributors: Arc::from(ids.into_boxed_slice()),
                    },
                )
            }
            SemanticNodeData::Array { element, readonly } => {
                let element = *element;
                let readonly = *readonly;
                drop(data);
                let new_element = self.project_view_node(element, ctx, inputs, substitutions, memo);
                if new_element == element {
                    return node;
                }
                graph.intern_preserving_scope(
                    node,
                    SemanticNodeData::Array {
                        element: new_element,
                        readonly,
                    },
                )
            }
            SemanticNodeData::Tuple { elements, readonly } => {
                let elements = elements.clone();
                let readonly = *readonly;
                drop(data);
                let projected: Vec<TupleElement> = elements
                    .iter()
                    .map(|el| TupleElement {
                        label: el.label.clone(),
                        value: self.project_view_node(el.value, ctx, inputs, substitutions, memo),
                        optional: el.optional,
                        rest: el.rest,
                    })
                    .collect();
                // Normalize-on-intern (the variadic-spread rule) — identical
                // to the reducing entry's tuple arm.
                match self.normalize_tuple_spread(&projected, readonly) {
                    super::build::NormalizedTupleShape::Array(array_node) => array_node,
                    super::build::NormalizedTupleShape::Tuple(normalized) => graph
                        .intern_preserving_scope(
                            node,
                            SemanticNodeData::Tuple {
                                elements: Arc::from(normalized.into_boxed_slice()),
                                readonly,
                            },
                        ),
                }
            }
            SemanticNodeData::TemplateLiteral {
                quasis,
                expressions,
            } => {
                let quasis = quasis.clone();
                let expressions = expressions.clone();
                drop(data);
                let ids: Vec<SemanticNodeId> = expressions
                    .iter()
                    .map(|e| self.project_view_node(*e, ctx, inputs, substitutions, memo))
                    .collect();
                graph.intern_preserving_scope(
                    node,
                    SemanticNodeData::TemplateLiteral {
                        quasis,
                        expressions: Arc::from(ids.into_boxed_slice()),
                    },
                )
            }

            // ── the object surface: THE stamp application point ──────────
            SemanticNodeData::Object(view) => {
                let view = view.clone();
                drop(data);
                let member_ctx = ctx.into_structural_provenance();
                let members: Vec<SurfaceMember> = view
                    .members
                    .iter()
                    .map(|member| SurfaceMember {
                        name: Arc::clone(&member.name),
                        value: self.project_view_node(
                            member.value,
                            member_ctx,
                            inputs,
                            substitutions,
                            memo,
                        ),
                        optional: member.optional,
                        readonly: member.readonly,
                        is_method: member.is_method,
                        visibility: member.visibility,
                        spans: member.spans,
                        declaration_origin: member.declaration_origin.clone(),
                        // The caller-relative stamps — applied to the VIEW,
                        // never shape-node identity. The reduction context
                        // is the witness the producers require.
                        declared_in_macro_type_arg: ctx.own_body_stamp(),
                        merge_role: ctx.role_stamp(),
                    })
                    .collect();
                let call_signatures: Vec<SemanticNodeId> = view
                    .call_signatures
                    .iter()
                    .map(|s| self.project_view_node(*s, ctx, inputs, substitutions, memo))
                    .collect();
                let construct_signatures: Vec<SemanticNodeId> = view
                    .construct_signatures
                    .iter()
                    .map(|s| self.project_view_node(*s, ctx, inputs, substitutions, memo))
                    .collect();
                let index_signatures: Vec<IndexSignature> = view
                    .index_signatures
                    .iter()
                    .map(|sig| IndexSignature {
                        key_type: self.project_view_node(
                            sig.key_type,
                            ctx,
                            inputs,
                            substitutions,
                            memo,
                        ),
                        value_type: self.project_view_node(
                            sig.value_type,
                            ctx,
                            inputs,
                            substitutions,
                            memo,
                        ),
                        readonly: sig.readonly,
                        spans: sig.spans,
                        declaration_origin: sig.declaration_origin.clone(),
                    })
                    .collect();
                let keyspace = view
                    .keyspace
                    .map(|k| self.project_view_node(k, ctx, inputs, substitutions, memo));
                graph.intern_preserving_scope(
                    node,
                    SemanticNodeData::Object(SurfaceView {
                        members: Arc::from(members.into_boxed_slice()),
                        call_signatures: Arc::from(call_signatures.into_boxed_slice()),
                        construct_signatures: Arc::from(construct_signatures.into_boxed_slice()),
                        index_signatures: Arc::from(index_signatures.into_boxed_slice()),
                        keyspace,
                        has_index_signature: view.has_index_signature,
                    }),
                )
            }

            SemanticNodeData::Function {
                params,
                return_type,
                type_parameters,
                signature_span,
                return_type_span,
            } => {
                let params = params.clone();
                let return_type = *return_type;
                let type_parameters = type_parameters.clone();
                let signature_span = *signature_span;
                let return_type_span = *return_type_span;
                drop(data);
                let new_params: Vec<FunctionParam> = params
                    .iter()
                    .map(|p| FunctionParam {
                        name: p.name.clone(),
                        ty: self.project_view_node(p.ty, ctx, inputs, substitutions, memo),
                        optional: p.optional,
                        rest: p.rest,
                        span: p.span,
                    })
                    .collect();
                let new_return =
                    self.project_view_node(return_type, ctx, inputs, substitutions, memo);
                let new_tps: Vec<TypeParamDecl> = type_parameters
                    .iter()
                    .map(|tp| TypeParamDecl {
                        name: Arc::clone(&tp.name),
                        constraint: tp
                            .constraint
                            .map(|c| self.project_view_node(c, ctx, inputs, substitutions, memo)),
                        default: tp
                            .default
                            .map(|d| self.project_view_node(d, ctx, inputs, substitutions, memo)),
                    })
                    .collect();
                graph.intern_preserving_scope(
                    node,
                    SemanticNodeData::Function {
                        params: Arc::from(new_params.into_boxed_slice()),
                        return_type: new_return,
                        type_parameters: Arc::from(new_tps.into_boxed_slice()),
                        signature_span,
                        return_type_span,
                    },
                )
            }

            // A bare constructor type is consumed function-like at query
            // time — the reducing entry lowers `new (...) => R` through the
            // SAME canonical `Function` carrier; mirror that downgrade.
            SemanticNodeData::ConstructorType { signature } => {
                let signature = *signature;
                drop(data);
                self.project_view_node(signature, ctx, inputs, substitutions, memo)
            }

            // ── operator dispatches (per-position demand decisions) ──────
            SemanticNodeData::KeyOf { base } => {
                let base = *base;
                drop(data);
                let base_id = self.project_view_node(base, ctx, inputs, substitutions, memo);
                if may_reduce_operator(ctx) {
                    match self.execute_type_node(SemanticQueryKey::KeyOf {
                        base: base_id,
                        context: ctx,
                    }) {
                        QueryResult::Value(SemanticQueryOutput { value: id, .. }) => id,
                        _ => self.opaque(QueryError::Miss),
                    }
                } else {
                    match graph.node_data(base_id).as_deref() {
                        Some(SemanticNodeData::Opaque(_)) | None => self.opaque(QueryError::Miss),
                        _ => {
                            if base_id == base {
                                node
                            } else {
                                graph.intern_preserving_scope(
                                    node,
                                    SemanticNodeData::KeyOf { base: base_id },
                                )
                            }
                        }
                    }
                }
            }

            SemanticNodeData::IndexedAccess { object, index } => {
                let object = *object;
                let index = index.clone();
                drop(data);
                // Path-precision: an intermediate indexed-access object hop
                // demotes to Navigate; a non-indexed-access base keeps the
                // caller's mode (the single consumed terminal hop).
                let object_is_intermediate = matches!(
                    graph.node_data(object).as_deref(),
                    Some(SemanticNodeData::IndexedAccess { .. })
                );
                let object_ctx = if object_is_intermediate {
                    ctx.with_mode(ProjectionMode::Navigate)
                } else {
                    ctx
                };
                let obj_id =
                    self.project_view_node(object, object_ctx, inputs, substitutions, memo);
                let index_key = match index {
                    IndexKey::String(s) => IndexKey::String(s),
                    IndexKey::Number(n) => IndexKey::Number(n),
                    IndexKey::TypeNode(n) => {
                        let projected = self.project_view_node(n, ctx, inputs, substitutions, memo);
                        // A substituted index may have settled to a literal;
                        // fold it exactly like the substitution rail does.
                        self.normalized_index_key_node(projected)
                    }
                };
                let should_defer = matches!(index_key, IndexKey::TypeNode(_))
                    || !matches!(
                        graph.node_data(obj_id).as_deref(),
                        Some(SemanticNodeData::Object(_))
                    );
                if should_defer {
                    graph.intern_preserving_scope(
                        node,
                        SemanticNodeData::IndexedAccess {
                            object: obj_id,
                            index: index_key,
                        },
                    )
                } else {
                    match self.execute_type_node(SemanticQueryKey::IndexedAccess {
                        base: obj_id,
                        index: index_key,
                        mode: ctx.mode,
                    }) {
                        QueryResult::Value(SemanticQueryOutput { value: id, .. }) => id,
                        _ => self.opaque(QueryError::Miss),
                    }
                }
            }

            SemanticNodeData::Conditional {
                check,
                extends,
                true_branch_ref,
                false_branch_ref,
                distributive,
            } => {
                let check = *check;
                let extends = *extends;
                let true_branch = *true_branch_ref;
                let false_branch = *false_branch_ref;
                let distributive = *distributive;
                drop(data);
                let check_id = self.project_view_node(check, ctx, inputs, substitutions, memo);
                // Deferred / primitive checks cannot decide an
                // Object-vs-Record relation, so their `extends` arm
                // carrier-stops; object-like relation subjects keep the
                // caller's demand (mirrors the reducing entry).
                let check_is_object_relation_subject = matches!(
                    graph.node_data(check_id).as_deref(),
                    Some(
                        SemanticNodeData::Object(_)
                            | SemanticNodeData::Intersection(_)
                            | SemanticNodeData::Alias(_)
                            | SemanticNodeData::DeclRef { .. }
                            | SemanticNodeData::InstantiationRef { .. }
                            | SemanticNodeData::Opaque(QueryError::DeclPlaceholder { .. })
                    )
                );
                let extends_ctx = if check_is_object_relation_subject {
                    ctx
                } else {
                    ProjectionReductionContext::structural_transit_with_mode(ctx.mode)
                };
                let extends_id =
                    self.project_view_node(extends, extends_ctx, inputs, substitutions, memo);
                let true_id = self.project_view_node(true_branch, ctx, inputs, substitutions, memo);
                let false_id =
                    self.project_view_node(false_branch, ctx, inputs, substitutions, memo);
                match self.execute_type_node(SemanticQueryKey::Conditional {
                    check: check_id,
                    extends: extends_id,
                    true_branch: true_id,
                    false_branch: false_id,
                    distributive,
                }) {
                    QueryResult::Value(SemanticQueryOutput { value: id, .. }) => id,
                    _ => self.opaque(QueryError::Miss),
                }
            }

            SemanticNodeData::Mapped { source, mapper } => {
                let source = *source;
                let mapper = mapper.clone();
                drop(data);
                // The keyof-sourced shape carries `key_space = KeyOf(source)`;
                // the fallback shape carries `key_space == source`.
                let keyof_sourced = matches!(
                    graph.node_data(mapper.key_space).as_deref(),
                    Some(SemanticNodeData::KeyOf { base }) if *base == source
                );
                let source_id = self.project_view_node(source, ctx, inputs, substitutions, memo);
                let key_space = if keyof_sourced {
                    if may_reduce_operator(ctx) {
                        match self.execute_type_node(SemanticQueryKey::KeyOf {
                            base: source_id,
                            context: ctx,
                        }) {
                            QueryResult::Value(SemanticQueryOutput { value: id, .. }) => id,
                            _ => self.opaque(QueryError::Miss),
                        }
                    } else {
                        match graph.node_data(source_id).as_deref() {
                            Some(SemanticNodeData::Opaque(_)) | None => {
                                self.opaque(QueryError::Miss)
                            }
                            _ => graph.intern_preserving_scope(
                                mapper.key_space,
                                SemanticNodeData::KeyOf { base: source_id },
                            ),
                        }
                    }
                } else {
                    source_id
                };
                let value_expr =
                    self.project_view_node(mapper.value_expr, ctx, inputs, substitutions, memo);
                let name_remap = mapper
                    .name_remap
                    .map(|n| self.project_view_node(n, ctx, inputs, substitutions, memo));
                let kind = crate::semantic_query::MapperKind::classify_value_expr(
                    graph,
                    value_expr,
                    source_id,
                    mapper.parameter_node,
                );
                let projected_mapper = MapperKey {
                    parameter_node: mapper.parameter_node,
                    key_space,
                    value_expr,
                    optionality: mapper.optionality,
                    readonly: mapper.readonly,
                    name_remap,
                    kind,
                };
                // Route/mode-INDEPENDENT open carrier-stop: an open mapped
                // surface preserves the deferred shell in ANY mode.
                if super::raise::mapped_type_is_open_or_unknown(self, source_id, &projected_mapper)
                {
                    return graph.intern_preserving_scope(
                        node,
                        SemanticNodeData::Mapped {
                            source: source_id,
                            mapper: projected_mapper,
                        },
                    );
                }
                match self.execute_type_node(SemanticQueryKey::MappedType {
                    source: source_id,
                    mapper: projected_mapper,
                    context: ctx,
                }) {
                    QueryResult::Value(SemanticQueryOutput { value: id, .. }) => id,
                    _ => self.opaque(QueryError::Miss),
                }
            }

            // ── typeof: resolve → project → apply, the eager order ───────
            SemanticNodeData::TypeOf(_) => {
                let (value_root, path) = data.typeof_head().expect("TypeOf carrier head");
                let value_root = value_root.clone();
                let path = Arc::clone(path);
                let type_args: Vec<SemanticNodeId> = data.carrier_type_args().to_vec();
                drop(data);
                let single_query =
                    self.execute_type_node(self.typeof_key_for(value_root.clone(), ctx));
                let (mut result, consumed_rest) = match single_query {
                    QueryResult::Value(SemanticQueryOutput { value: id, .. }) => (id, 0usize),
                    _ if !path.is_empty() => {
                        // Namespace-member fallback: join the root and the
                        // first trailing segment (`Ns.Foo`).
                        let joined: Arc<str> =
                            Arc::from(format!("{}.{}", value_root.name, path[0]));
                        match self.execute_type_node(self.typeof_key_for(
                            ValueRootKey {
                                scope: value_root.scope.clone(),
                                name: joined,
                            },
                            ctx,
                        )) {
                            QueryResult::Value(SemanticQueryOutput { value: id, .. }) => {
                                (id, 1usize)
                            }
                            _ => return self.opaque(QueryError::Miss),
                        }
                    }
                    _ => return self.opaque(QueryError::Miss),
                };
                if path.len() > consumed_rest {
                    let segments: Arc<[PathSegment]> = Arc::from(
                        path[consumed_rest..]
                            .iter()
                            .map(|segment| PathSegment::Member(Arc::clone(segment)))
                            .collect::<Vec<_>>()
                            .into_boxed_slice(),
                    );
                    result = match self.execute_type_node(SemanticQueryKey::ProjectPath {
                        base: result,
                        path: segments,
                        context: ProjectionReductionContext::published(ProjectionMode::Navigate),
                    }) {
                        QueryResult::Value(SemanticQueryOutput { value: id, .. }) => id,
                        _ => return self.opaque(QueryError::Miss),
                    };
                }
                if !type_args.is_empty() {
                    let projected_args: Vec<SemanticNodeId> = type_args
                        .iter()
                        .map(|arg| self.project_view_node(*arg, ctx, inputs, substitutions, memo))
                        .collect();
                    result = self.apply_typeof_instantiation_args(result, &projected_args);
                }
                result
            }

            // ── reference carriers: the shared-resolver tail ─────────────
            SemanticNodeData::DeclRef { identity } => {
                let identity = identity.clone();
                drop(data);
                // Recursive-ref back-edge: a bare head resolving to an
                // identity already materialising in an enclosing frame.
                if self.is_instantiate_active(
                    identity.canonical_id.as_ref(),
                    identity.decl_name.as_ref(),
                ) {
                    return self.opaque(QueryError::RecursiveRef {
                        name: Arc::clone(&identity.decl_name),
                    });
                }
                if matches!(
                    ctx.mode,
                    ProjectionMode::Navigate | ProjectionMode::Skeleton | ProjectionMode::Shallow
                ) {
                    return node;
                }
                let anchor =
                    match self.execute_type_node(SemanticQueryKey::ResolveDecl(ResolveDeclKey {
                        scope: ScopeId {
                            canonical_id: Arc::clone(&identity.canonical_id),
                            local_scope: None,
                        },
                        name: Arc::clone(&identity.decl_name),
                    })) {
                        QueryResult::Value(SemanticQueryOutput { value: id, .. }) => id,
                        _ => return self.opaque(QueryError::Miss),
                    };
                let routes_through_instantiate = self
                    .ctx
                    .prepared_type_decl(identity.canonical_id.as_ref(), identity.decl_name.as_ref())
                    .is_some_and(|prepared| !prepared.type_parameters.is_empty());
                if !routes_through_instantiate {
                    return anchor;
                }
                match self.execute_type_node(SemanticQueryKey::Instantiate {
                    base: self.type_slot_for(
                        Arc::clone(&identity.canonical_id),
                        Arc::clone(&identity.decl_name),
                    ),
                    args: Arc::from(Vec::<SemanticNodeId>::new().into_boxed_slice()),
                    context: self.instantiate_context_for(&identity.canonical_id, ctx),
                }) {
                    QueryResult::Value(SemanticQueryOutput { value: id, .. }) => id,
                    _ => self.opaque(QueryError::Miss),
                }
            }

            SemanticNodeData::InstantiationRef { base, args } => {
                let base = base.clone();
                let args = args.clone();
                drop(data);
                // Reference-site type arguments carry structural provenance
                // (mirrors the reducing entry's lazy arg lowering).
                let arg_ctx = ctx.into_structural_provenance();
                let projected_args: Arc<[SemanticNodeId]> = Arc::from(
                    args.iter()
                        .map(|a| self.project_view_node(*a, arg_ctx, inputs, substitutions, memo))
                        .collect::<Vec<_>>()
                        .into_boxed_slice(),
                );
                let rebuild = |projected: Arc<[SemanticNodeId]>| -> SemanticNodeId {
                    if projected.as_ref() == args.as_ref() {
                        node
                    } else {
                        graph.intern_preserving_scope(
                            node,
                            SemanticNodeData::InstantiationRef {
                                base: base.clone(),
                                args: projected,
                            },
                        )
                    }
                };
                if base.canonical_id.as_ref() == "__builtin__" {
                    // Promise carrier stays nominal in EVERY mode.
                    if self.is_promise_global_name(base.decl_name.as_ref()) {
                        return rebuild(projected_args);
                    }
                    // Builtin carrier gate — identical decision table to the
                    // reducing entry's builtin fast path, PLUS the
                    // non-publication transit rail: a `StructuralTransit`
                    // demand carrier-stops the builtins (the deferred
                    // reference-arm projection above routes heritage refs
                    // here — a mapper builtin materialised mid-transit would
                    // reach the peer-merge reducer as an own-surface Object
                    // and invert own-body-shadows-heritage). `Skeleton` is
                    // exempt from the transit stop: the BFS / generic-helper
                    // traversal (the open/closed enumeration-domain oracle in
                    // particular) probes builtin bodies under
                    // `StructuralTransit(Skeleton)` and must keep executing
                    // them — carrier-stopping the probe makes the oracle
                    // judge every closed utility source "unknown" and
                    // over-broadens the L1 carrier-stop; Skeleton results
                    // never publish.
                    let build_carrier = (ctx.demand == ReductionDemand::StructuralTransit
                        && ctx.mode != ProjectionMode::Skeleton)
                        || ctx.mode == ProjectionMode::Shallow
                        || (super::raise::is_l1_object_filter_utility(base.decl_name.as_ref())
                            && (ctx.mode == ProjectionMode::Navigate
                                || super::raise::utility_enumeration_domain_is_open_or_unknown(
                                    self,
                                    &base,
                                    &projected_args,
                                )))
                        || (matches!(
                            ctx.mode,
                            ProjectionMode::Navigate | ProjectionMode::Skeleton
                        ) && projected_args.iter().any(|arg| {
                            super::raise::builtin_lowering_argument_is_open(self, *arg)
                        }));
                    if build_carrier {
                        return rebuild(projected_args);
                    }
                    return match self.execute_type_node(SemanticQueryKey::Instantiate {
                        base: self.type_slot_for(
                            Arc::clone(&base.canonical_id),
                            Arc::clone(&base.decl_name),
                        ),
                        args: projected_args,
                        context: self.instantiate_context_for(&base.canonical_id, ctx),
                    }) {
                        QueryResult::Value(SemanticQueryOutput { value: id, .. }) => id,
                        _ => self.opaque(QueryError::Miss),
                    };
                }
                if matches!(
                    ctx.mode,
                    ProjectionMode::Navigate | ProjectionMode::Skeleton | ProjectionMode::Shallow
                ) {
                    return rebuild(projected_args);
                }
                match self.execute_type_node(SemanticQueryKey::Instantiate {
                    base: self
                        .type_slot_for(Arc::clone(&base.canonical_id), Arc::clone(&base.decl_name)),
                    args: projected_args,
                    context: self.instantiate_context_for(&base.canonical_id, ctx),
                }) {
                    QueryResult::Value(SemanticQueryOutput { value: id, .. }) => id,
                    _ => self.opaque(QueryError::Miss),
                }
            }

            SemanticNodeData::BareRef(_) => {
                let (name, _bare_scope) = data.bare_ref_head().expect("BareRef head");
                let name = Arc::clone(name);
                let type_args: Vec<SemanticNodeId> = data.carrier_type_args().to_vec();
                drop(data);
                // Script-setup generic parameter: a bare name bound by the
                // scope payload's script-setup type bindings lowers to the
                // rich `TypeParam` shell (declaration-site constraint /
                // default preserved) — mirrors the reducing entry's
                // dedicated arm, which precedes the shared head resolver.
                if type_args.is_empty() {
                    if let Some(binding) = inputs
                        .scope_payload
                        .and_then(|payload| payload.scope_type_bindings.get(name.as_ref()))
                    {
                        let mut nested: Vec<(Arc<str>, SemanticNodeId)> = Vec::new();
                        let constraint = binding.constraint.as_ref().map(|c| {
                            self.shallow_lower_type_expr_with_context(
                                c,
                                inputs.env,
                                inputs.scope,
                                inputs.name_resolution,
                                inputs.scope_payload,
                                inputs.shadowing,
                                &mut nested,
                                ctx,
                            )
                        });
                        let default = binding.default.as_ref().map(|d| {
                            self.shallow_lower_type_expr_with_context(
                                d,
                                inputs.env,
                                inputs.scope,
                                inputs.name_resolution,
                                inputs.scope_payload,
                                inputs.shadowing,
                                &mut nested,
                                ctx,
                            )
                        });
                        substitutions.extend(nested);
                        let display_name = Arc::clone(&binding.name);
                        let decl = match inputs.scope {
                            NodeScopeId::Global => DeclIdentity {
                                canonical_id: Arc::from(""),
                                whole_hash: crate::semantic_query::HashValue::default(),
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
                        return graph.intern_node_with_scope(
                            SemanticNodeData::TypeParam {
                                decl,
                                param_index: binding.ordinal,
                                constraint,
                                default,
                                display_name,
                            },
                            inputs.scope.clone(),
                        );
                    }
                }
                let resolver_ctx = CarrierResolverContext::new(
                    inputs.env,
                    inputs.scope,
                    inputs.name_resolution,
                    inputs.scope_payload,
                    inputs.shadowing,
                    ctx,
                );
                let arg_ctx = ctx.into_structural_provenance();
                self.resolve_bare_ref_head(&resolver_ctx, &name, type_args.len(), || {
                    Arc::from(
                        type_args
                            .iter()
                            .map(|a| {
                                self.project_view_node(*a, arg_ctx, inputs, substitutions, memo)
                            })
                            .collect::<Vec<_>>()
                            .into_boxed_slice(),
                    )
                })
            }

            SemanticNodeData::ImportType(_) => {
                let (specifier, qualifier, typeof_query) =
                    data.import_type_head().expect("ImportType head");
                let specifier = Arc::clone(specifier);
                let qualifier: Vec<Arc<str>> = qualifier.iter().map(Arc::clone).collect();
                let type_args: Vec<SemanticNodeId> = data.carrier_type_args().to_vec();
                drop(data);
                let NodeScopeId::File {
                    canonical_id: owner_canonical,
                    ..
                } = inputs.scope
                else {
                    return self.opaque(QueryError::Miss);
                };
                let resolver_ctx = CarrierResolverContext::new(
                    inputs.env,
                    inputs.scope,
                    inputs.name_resolution,
                    inputs.scope_payload,
                    inputs.shadowing,
                    ctx,
                );
                self.resolve_import_type_head(
                    &resolver_ctx,
                    owner_canonical.as_ref(),
                    &specifier,
                    &qualifier,
                    typeof_query,
                    type_args.len(),
                    || {
                        Arc::from(
                            type_args
                                .iter()
                                .map(|a| {
                                    self.project_view_node(*a, ctx, inputs, substitutions, memo)
                                })
                                .collect::<Vec<_>>()
                                .into_boxed_slice(),
                        )
                    },
                )
            }
        }
    }
}
