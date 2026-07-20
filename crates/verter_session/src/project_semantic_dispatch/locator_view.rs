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

use super::locator_view_worklist::ProjectedViewOutcome;
use super::ProjectSemanticDispatch;
use crate::resolver_core::bare_name_resolve::DeclarationScopePayload;
use crate::resolver_core::scope_shadowing::ScopeShadowing;
use crate::semantic_query::{
    MemberMergeRole, NodeScopeId, PrimitiveKind, ProjectionMode, ProjectionReductionContext,
    ResultCompleteness, SemanticNodeData, SemanticNodeId,
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
    pub(super) name_resolution: &'a FxHashMap<std::sync::Arc<str>, ResolvedRootIdentity>,
    pub(super) scope_payload: Option<&'a DeclarationScopePayload>,
    pub(super) shadowing: &'a ScopeShadowing,
    pub(super) authored_resolution_debt: Option<&'a super::carrier::AuthoredResolutionDebtFrame>,
}

/// Per-projection memo so shared sub-graphs project once per context.
pub(super) type ViewMemo = FxHashMap<(SemanticNodeId, ProjectionReductionContext), SemanticNodeId>;

/// Prepared input for the projection-view Criterion benchmark. This lives
/// behind `test-support`; ordinary production builds cannot construct or see
/// the otherwise-private locator projection inputs.
#[cfg(any(test, feature = "test-support"))]
pub struct ProjectionBenchCase {
    root: SemanticNodeId,
    scope: NodeScopeId,
    name_resolution: FxHashMap<std::sync::Arc<str>, ResolvedRootIdentity>,
    scope_payload: Option<DeclarationScopePayload>,
    shadowing: ScopeShadowing,
}

/// Test-support-only driver for benchmarking the real projection primitive
/// with a persistent per-demand memo. It does not provide a second semantic
/// implementation: every operation delegates to
/// [`ProjectSemanticDispatch::project_view_node_worklist`].
#[cfg(any(test, feature = "test-support"))]
pub struct ProjectionBenchHarness<'a> {
    dispatch: ProjectSemanticDispatch<'a>,
    env: FxHashMap<String, SemanticNodeId>,
    substitutions: Vec<(Arc<str>, SemanticNodeId)>,
    memo: ViewMemo,
}

#[cfg(any(test, feature = "test-support"))]
impl<'a> ProjectionBenchHarness<'a> {
    #[must_use]
    pub fn new(host: &'a crate::VerterHost) -> Self {
        Self {
            dispatch: ProjectSemanticDispatch::new(host),
            env: FxHashMap::default(),
            substitutions: Vec::new(),
            memo: ViewMemo::default(),
        }
    }

    /// Lower one authored alias body once and retain the production scope and
    /// reference-resolution inputs its view projection consumes.
    #[must_use]
    pub fn prepare_decl(&self, canonical_id: &str, symbol: &str) -> Option<ProjectionBenchCase> {
        self.prepare_decl_with_resolved_names(canonical_id, symbol, &[])
    }

    /// Prepare a case with an explicit bare-name identity map. Keeping this
    /// map explicit makes the resolved and unresolved benchmark rows
    /// deterministic without constructing a request-external prepared bundle.
    #[must_use]
    pub fn prepare_decl_with_resolved_names(
        &self,
        canonical_id: &str,
        symbol: &str,
        resolved_names: &[(&str, &str, &str)],
    ) -> Option<ProjectionBenchCase> {
        use verter_type_expr::locators::{
            AuthoredAnchor, AuthoredBodyLocator, LocatorSymbolSpace, TypeBodyPathStep, TypeBodySlot,
        };

        let root = match self
            .dispatch
            .lower_locator(AuthoredBodyLocator::DeclBody(TypeBodySlot {
                anchor: AuthoredAnchor {
                    canonical_id: Arc::from(canonical_id),
                    owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
                    symbol: Arc::from(symbol),
                    space: LocatorSymbolSpace::Type,
                },
                path: Arc::from(Vec::<TypeBodyPathStep>::new().into_boxed_slice()),
            })) {
            crate::semantic_query::QueryResult::Value(root) => root,
            crate::semantic_query::QueryResult::Recursive(root) => root,
            crate::semantic_query::QueryResult::Error(_) => return None,
        };
        let whole_hash = self
            .dispatch
            .ctx
            .shallow_file_state(canonical_id)
            .map(|state| state.whole_hash)
            .unwrap_or_default();
        let scope = NodeScopeId::File {
            canonical_id: Arc::from(canonical_id),
            owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
            whole_hash,
            local_scope: None,
        };
        let name_resolution = resolved_names
            .iter()
            .map(|(local_name, defining_canonical, defining_symbol)| {
                (
                    Arc::from(*local_name),
                    ResolvedRootIdentity::new_in_owner(
                        *defining_canonical,
                        verter_type_expr::TopLevelOwnerId::ordinary_file(),
                        *defining_symbol,
                    ),
                )
            })
            .collect();
        let scope_payload = None;
        let shadowing = ScopeShadowing::empty();
        Some(ProjectionBenchCase {
            root,
            scope,
            name_resolution,
            scope_payload,
            shadowing,
        })
    }

    /// Project after dropping both memo entries and retained memo capacity.
    /// Used only by the one-shot shallow allocation probe.
    pub fn project_fresh(
        &mut self,
        case: &ProjectionBenchCase,
        context: ProjectionReductionContext,
    ) -> (SemanticNodeId, ResultCompleteness) {
        self.memo = ViewMemo::default();
        self.project(case, context)
    }

    /// Project with an empty memo while retaining its allocation capacity.
    /// This is the steady-state cold path measured by Criterion.
    pub fn project_cold(
        &mut self,
        case: &ProjectionBenchCase,
        context: ProjectionReductionContext,
    ) -> (SemanticNodeId, ResultCompleteness) {
        self.memo.clear();
        self.project(case, context)
    }

    /// Project without clearing the memo, exercising the exact root memo-hit
    /// path and context-split reuse behavior.
    pub fn project_warm(
        &mut self,
        case: &ProjectionBenchCase,
        context: ProjectionReductionContext,
    ) -> (SemanticNodeId, ResultCompleteness) {
        self.project(case, context)
    }

    fn project(
        &mut self,
        case: &ProjectionBenchCase,
        context: ProjectionReductionContext,
    ) -> (SemanticNodeId, ResultCompleteness) {
        self.substitutions.clear();
        let inputs = LocatorViewInputs {
            env: &self.env,
            scope: &case.scope,
            name_resolution: &case.name_resolution,
            scope_payload: case.scope_payload.as_ref(),
            shadowing: &case.shadowing,
            authored_resolution_debt: None,
        };
        let outcome = self.dispatch.project_view_node_worklist(
            case.root,
            context,
            &inputs,
            &mut self.substitutions,
            &mut self.memo,
        );
        (outcome.node, outcome.completeness)
    }
}

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
    ) -> ProjectedViewOutcome {
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
        let mut completeness = ResultCompleteness::Complete;
        let substitution_checkpoint = substitutions.len();
        let data = self.graph().node_data(shape);
        let projected = match data.as_deref() {
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
                            &mut completeness,
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
                    &mut completeness,
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
                    &mut completeness,
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
                    &mut completeness,
                )
            }
        };
        if completeness.is_partial() {
            substitutions.truncate(substitution_checkpoint);
            ProjectedViewOutcome {
                node: shape,
                completeness,
            }
        } else {
            ProjectedViewOutcome {
                node: projected,
                completeness,
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
        completeness: &mut ResultCompleteness,
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
                            arm_ctx = arm_ctx.into_structural_transit_with_mode(mode);
                        }
                        self.project_view_node(
                            *arm,
                            arm_ctx,
                            inputs,
                            substitutions,
                            memo,
                            completeness,
                        )
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
                    completeness,
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
        completeness: &mut ResultCompleteness,
    ) -> SemanticNodeId {
        let outcome = self.project_view_node_worklist(node, ctx, inputs, substitutions, memo);
        *completeness = completeness.merge(outcome.completeness);
        outcome.node
    }
}
