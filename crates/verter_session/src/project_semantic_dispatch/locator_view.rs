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
use verter_session_query::declarations::TypeDeclKind;
use verter_session_query::type_solver::host::ResolvedRootIdentity;

use super::locator_view_worklist::{
    ProjectedViewOutcome, ProjectionPoll, ProjectionRun, ProjectionSeam,
};
use super::ProjectSemanticDispatch;
use crate::resolver_core::bare_name_resolve::DeclarationScopePayload;
use crate::resolver_core::scope_shadowing::ScopeShadowing;
use crate::semantic_query::{
    MemberMergeRole, NodeScopeId, PrimitiveKind, ProjectionMode, ProjectionReductionContext,
    ResultCompleteness, SemanticNodeData, SemanticNodeId,
};

/// A decl-body projection the caller can resume: its roots, the one memo
/// they project over, and the run of the root in progress.
pub(super) struct DeclBodyProjection {
    shape: SemanticNodeId,
    context: ProjectionReductionContext,
    reference_arm_role: MemberMergeRole,
    class_body: bool,
    substitution_checkpoint: usize,
    memo: ViewMemo,
    completeness: ResultCompleteness,
    plan: BodyPlan,
    run: Option<ProjectionRun>,
    seam: ProjectionSeam,
}

impl DeclBodyProjection {
    /// Record the projected node of the root just finished.
    fn feed(&mut self, node: SemanticNodeId) {
        let group = match &mut self.plan {
            BodyPlan::Group(group) => group,
            BodyPlan::Merged { current, .. } => current
                .as_mut()
                .expect("a merged root belongs to an open group"),
        };
        match group {
            BodyGroup::Whole { projected, .. } => *projected = Some(node),
            BodyGroup::Arms { ids, .. } => ids.push(node),
        }
    }
}

/// How a decl body's projected roots combine.
enum BodyPlan {
    /// One group: the whole body, or a single declaration's arms.
    Group(BodyGroup),
    /// A merged declaration: one group per contributor, in order.
    Merged {
        contributors: Arc<[SemanticNodeId]>,
        next: usize,
        ids: Vec<SemanticNodeId>,
        current: Option<BodyGroup>,
    },
}

/// One group of roots that combine into one node.
enum BodyGroup {
    /// One root, whose projection is the group's node.
    Whole {
        root: (SemanticNodeId, ProjectionReductionContext),
        started: bool,
        projected: Option<SemanticNodeId>,
    },
    /// An intersection body's arms, projected one by one and rebuilt.
    Arms {
        body: SemanticNodeId,
        arms: Arc<[SemanticNodeId]>,
        next: usize,
        ids: Vec<SemanticNodeId>,
        demand: ReferenceArmDemand,
    },
}

/// Where a decl-body projection stands after it ran as far as it could.
pub(super) enum BodyProjectionPoll {
    Done(ProjectedViewOutcome),
    Need(crate::semantic_query::SemanticQueryKey),
}

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
    /// The value whose declared body is being projected, when the body is
    /// a value's: a `typeof` naming that value inside it is the value's
    /// own type, read by reference.
    pub(super) self_value: Option<&'a verter_type_expr::locators::AuthoredAnchor>,
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
            self_value: None,
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
    /// Plan the projection of a substituted decl-body shape into the
    /// caller's demanded view, applying the per-arm [`ProjectionStamp`] rule:
    ///
    /// - a `MergedDecl` body projects each contributor as an OWN-body
    ///   surface (preserving the distinct peer-merge carrier);
    /// - an `Intersection` body stamps inline object arms as own-body
    ///   (caller provenance + `OwnBody` role) and reference arms as
    ///   structural with the declaration-kind role (`Heritage` for an
    ///   interface/class, `Authored` for an alias);
    /// - a whole `Object` body is its own own-body arm;
    /// - any other body projects under the caller's context verbatim.
    ///
    /// Its roots project one after
    /// another over one memo, and their nodes combine into the projected
    /// body. `substitution_checkpoint` is where the body's substitutions
    /// begin; a partial projection drops the ones it recorded.
    pub(super) fn begin_decl_body_projection(
        &self,
        shape: SemanticNodeId,
        decl_kind: TypeDeclKind,
        substitution_checkpoint: usize,
        context: ProjectionReductionContext,
        seam: ProjectionSeam,
    ) -> DeclBodyProjection {
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
        let class_body = decl_kind == TypeDeclKind::Class;
        let plan = match self.graph().node_data(shape).as_deref() {
            // Per-arm heritage discrimination applies INSIDE each merged
            // contributor exactly as it does to a single declaration's body:
            // a contributor shaped `Intersection([extends Ref…, own
            // Object])` stamps its inline object arms as OWN-body and its
            // reference (heritage) arms as HERITAGE — never a blanket
            // own-body stamp over the whole contributor, which would
            // materialise the heritage reference into an `Object` that the
            // peer-merge reducer then mis-buckets as OWN surface, losing
            // own-body-shadows-heritage precedence.
            Some(SemanticNodeData::MergedDecl { contributors }) => BodyPlan::Merged {
                contributors: Arc::clone(contributors),
                next: 0,
                ids: Vec::with_capacity(contributors.len()),
                current: None,
            },
            // A single declaration's body flows to the role-driven
            // intersection surface merge, which classifies members by their
            // stamped merge role — reference arms may evaluate under the
            // caller's demand.
            Some(SemanticNodeData::Intersection(arms)) => BodyPlan::Group(BodyGroup::Arms {
                body: shape,
                arms: arms.to_vec().into(),
                next: 0,
                ids: Vec::new(),
                demand: ReferenceArmDemand::CallerMode,
            }),
            Some(SemanticNodeData::Object(_)) => {
                let own = ProjectionStamp::new(
                    context,
                    MemberMergeRole::OwnBody,
                    AuthoredArmKind::OwnBodyObject,
                );
                BodyPlan::Group(BodyGroup::Whole {
                    root: (shape, own.stamped_context(context)),
                    started: false,
                    projected: None,
                })
            }
            _ => {
                let whole =
                    ProjectionStamp::new(context, context.merge_role(), AuthoredArmKind::WholeBody);
                BodyPlan::Group(BodyGroup::Whole {
                    root: (shape, whole.stamped_context(context)),
                    started: false,
                    projected: None,
                })
            }
        };
        DeclBodyProjection {
            shape,
            context,
            reference_arm_role,
            class_body,
            substitution_checkpoint,
            memo: ViewMemo::default(),
            completeness: ResultCompleteness::Complete,
            plan,
            run: None,
            seam,
        }
    }

    /// Carry a decl-body projection on as far as it can go. `delivery` is
    /// the node of the instantiation it last stopped at, when it stopped at
    /// one.
    pub(super) fn drain_decl_body_projection(
        &self,
        projection: &mut DeclBodyProjection,
        inputs: &LocatorViewInputs<'_>,
        substitutions: &mut Vec<(Arc<str>, SemanticNodeId)>,
        mut delivery: Option<SemanticNodeId>,
    ) -> BodyProjectionPoll {
        // Every root this call projects belongs to one connected demand.
        let (_connected_guard, _) = self.enter_connected_demand(false);
        loop {
            if let Some(run) = projection.run.as_mut() {
                let poll = self.drain_projection(
                    run,
                    inputs,
                    substitutions,
                    &mut projection.memo,
                    &mut projection.seam,
                    delivery.take(),
                );
                match poll {
                    ProjectionPoll::Need(key) => return BodyProjectionPoll::Need(key),
                    ProjectionPoll::Done(outcome) => {
                        projection.run = None;
                        projection.completeness =
                            projection.completeness.merge(outcome.completeness);
                        projection.feed(outcome.node);
                    }
                }
                continue;
            }
            match self.next_body_root(projection) {
                Some((root, root_context)) => {
                    let begun = self.begin_projection(
                        root,
                        root_context,
                        inputs,
                        substitutions,
                        &mut projection.memo,
                        &mut projection.seam,
                    );
                    match begun {
                        Ok(run) => projection.run = Some(run),
                        Err(outcome) => {
                            projection.completeness =
                                projection.completeness.merge(outcome.completeness);
                            projection.feed(outcome.node);
                        }
                    }
                }
                None => {
                    let projected = self.finish_body_plan(projection);
                    return BodyProjectionPoll::Done(if projection.completeness.is_partial() {
                        substitutions.truncate(projection.substitution_checkpoint);
                        ProjectedViewOutcome {
                            node: projection.shape,
                            completeness: projection.completeness,
                        }
                    } else {
                        ProjectedViewOutcome {
                            node: projected,
                            completeness: projection.completeness,
                        }
                    });
                }
            }
        }
    }

    /// The next root the projection projects, or `None` once every root is
    /// projected. Finishing a merged contributor's group records its node
    /// and opens the next contributor's group.
    fn next_body_root(
        &self,
        projection: &mut DeclBodyProjection,
    ) -> Option<(SemanticNodeId, ProjectionReductionContext)> {
        let context = projection.context;
        let reference_arm_role = projection.reference_arm_role;
        let class_body = projection.class_body;
        match &mut projection.plan {
            BodyPlan::Group(group) => {
                self.next_group_root(group, context, reference_arm_role, class_body)
            }
            BodyPlan::Merged {
                contributors,
                next,
                ids,
                current,
            } => loop {
                if let Some(group) = current.as_mut() {
                    if let Some(root) =
                        self.next_group_root(group, context, reference_arm_role, class_body)
                    {
                        return Some(root);
                    }
                    let finished = current.take().expect("an open contributor group");
                    ids.push(self.finish_body_group(finished, reference_arm_role));
                    continue;
                }
                let contributor = *contributors.get(*next)?;
                *next += 1;
                *current = Some(self.contributor_group(contributor, context));
            },
        }
    }

    /// The group one merged contributor projects as: its arms when it is an
    /// intersection (each reference arm a deferred carrier, preserving the
    /// topology the peer-merge reducer consumes), else the contributor as
    /// one own-body surface.
    fn contributor_group(
        &self,
        contributor: SemanticNodeId,
        context: ProjectionReductionContext,
    ) -> BodyGroup {
        match self.graph().node_data(contributor).as_deref() {
            // The peer-merge reducer consumes contributor arms by TOPOLOGY
            // (`Intersection([heritage refs…, own Object])`, heritage arms
            // preserved for lazy resolution under the heritage-overlay role)
            // — so a heritage reference must reach it as a CARRIER, never
            // eagerly materialised here.
            Some(SemanticNodeData::Intersection(arms)) => BodyGroup::Arms {
                body: contributor,
                arms: arms.to_vec().into(),
                next: 0,
                ids: Vec::new(),
                demand: ReferenceArmDemand::Deferred,
            },
            _ => {
                let own = ProjectionStamp::new(
                    context,
                    MemberMergeRole::OwnBody,
                    AuthoredArmKind::OwnBodyObject,
                );
                BodyGroup::Whole {
                    root: (contributor, own.stamped_context(context)),
                    started: false,
                    projected: None,
                }
            }
        }
    }

    /// The next root of one group, or `None` once the group's roots are all
    /// projected.
    fn next_group_root(
        &self,
        group: &mut BodyGroup,
        context: ProjectionReductionContext,
        reference_arm_role: MemberMergeRole,
        class_body: bool,
    ) -> Option<(SemanticNodeId, ProjectionReductionContext)> {
        match group {
            BodyGroup::Whole { root, started, .. } => {
                if *started {
                    return None;
                }
                *started = true;
                Some(*root)
            }
            BodyGroup::Arms {
                arms, next, demand, ..
            } => loop {
                let arm = *arms.get(*next)?;
                *next += 1;
                // A class's `extends` names a VALUE: one the type space does
                // not declare contributes the instance type of its construct
                // signature, or nothing.
                let arm = match class_body
                    .then(|| self.class_heritage_value_arm(arm, context))
                    .flatten()
                {
                    Some(Some(instance)) => instance,
                    Some(None) => continue,
                    None => arm,
                };
                let arm_kind = match self.graph().node_data(arm).as_deref() {
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
                    && matches!(demand, ReferenceArmDemand::Deferred)
                {
                    // A DEFERRED reference arm is a TRUE carrier-only
                    // projection: the arm projects under the NON-PUBLICATION
                    // `StructuralTransit` demand (with the eager modes demoted
                    // to `Navigate`) so every materialisation gate along the
                    // arm — the mapper builtins (`Partial`/`Required`/
                    // `Readonly`) included — carrier-stops. A `Published`
                    // demand here would let a closed-arg builtin heritage ref
                    // fall through to an executed `Instantiate`, and the
                    // resulting `Object` is mis-bucketed as OWN surface by the
                    // topology-driven peer-merge reducer — inverting
                    // own-body-shadows-heritage. The stamped merge role
                    // (`Heritage` for an interface/class) is PRESERVED on the
                    // transit context; substitution env and structural
                    // provenance carry through unchanged.
                    let mode = match arm_ctx.mode {
                        ProjectionMode::Expanded | ProjectionMode::Identity => {
                            ProjectionMode::Navigate
                        }
                        other => other,
                    };
                    arm_ctx = arm_ctx.into_structural_transit_with_mode(mode);
                }
                return Some((arm, arm_ctx));
            },
        }
    }

    /// The projected body once every root is projected.
    fn finish_body_plan(&self, projection: &mut DeclBodyProjection) -> SemanticNodeId {
        let reference_arm_role = projection.reference_arm_role;
        match std::mem::replace(
            &mut projection.plan,
            BodyPlan::Merged {
                contributors: Arc::from([]),
                next: 0,
                ids: Vec::new(),
                current: None,
            },
        ) {
            BodyPlan::Group(group) => self.finish_body_group(group, reference_arm_role),
            BodyPlan::Merged { ids, .. } => self.graph().intern_preserving_scope(
                projection.shape,
                SemanticNodeData::MergedDecl {
                    contributors: Arc::from(ids.into_boxed_slice()),
                },
            ),
        }
    }

    /// One group's node over its projected roots.
    fn finish_body_group(
        &self,
        group: BodyGroup,
        reference_arm_role: MemberMergeRole,
    ) -> SemanticNodeId {
        match group {
            BodyGroup::Whole { projected, .. } => {
                projected.expect("a whole-body group projects its root")
            }
            BodyGroup::Arms { body, ids, .. } => {
                if ids.is_empty() {
                    self.graph()
                        .intern_node(SemanticNodeData::Primitive(PrimitiveKind::Never))
                } else if ids.len() == 1 {
                    ids[0]
                } else {
                    // Order- and scope-preserving rebuild of the projected
                    // arms: own-body-last order is topology and display
                    // fidelity. An interface or class body (its reference
                    // arms are `extends` heritage) is minted a heritage body,
                    // which inherits signatures by concatenation; an alias's
                    // intersection stays an intersection.
                    let arms: Arc<[SemanticNodeId]> = Arc::from(ids.into_boxed_slice());
                    let list = match reference_arm_role {
                        MemberMergeRole::Heritage => {
                            crate::semantic_query::composite::CompositeList::heritage(arms)
                        }
                        MemberMergeRole::Authored | MemberMergeRole::OwnBody => {
                            crate::semantic_query::composite::CompositeList::preserving_rebuild(
                                arms,
                            )
                        }
                    };
                    self.graph()
                        .intern_preserving_scope(body, SemanticNodeData::Intersection(list))
                }
            }
        }
    }

    /// The base a class body's heritage `arm` contributes when it names a
    /// value the type space does not declare: `Some(Some(instance))` for
    /// the instance type of that value's construct signature, `Some(None)`
    /// when the value gives no base type, and `None` when the arm is not
    /// such a reference (it projects as it is).
    fn class_heritage_value_arm(
        &self,
        arm: SemanticNodeId,
        context: ProjectionReductionContext,
    ) -> Option<Option<SemanticNodeId>> {
        let (identity, args) = match self.graph().node_data(arm).as_deref() {
            Some(SemanticNodeData::DeclRef { identity }) => (
                identity.clone(),
                Arc::from(Vec::<SemanticNodeId>::new().into_boxed_slice()),
            ),
            Some(SemanticNodeData::InstantiationRef { base, args }) => {
                (base.clone(), Arc::clone(args))
            }
            _ => return None,
        };
        if !self.heritage_names_value_only(
            &identity.canonical_id,
            identity.owner,
            &identity.decl_name,
        ) {
            return None;
        }
        Some(
            self.class_value_base(
                &identity.canonical_id,
                identity.owner,
                &identity.decl_name,
                &args,
                context,
            )
            .and_then(|base| base.instance),
        )
    }
}
