//! Relation engine — the SOLE relation authority, riding
//! `execute(SemanticQueryKey::Relate)` on the cold-compute frame of the ONE
//! resolver (design `docs/arch/u2-relation-infer-design.md`).
//!
//! Every relation judgement — top-level consumer asks, conditional branch
//! selection, `Extract`/`Exclude` per-arm filtering, the oracle adapter,
//! AND every recursive sub-relation — re-enters the SAME full-key authority
//! [`ProjectSemanticDispatch::execute_relate`]. There is no second engine:
//! the deleted process-global TLS in-flight guard and the bare-pair
//! entry point are replaced by the per-transaction
//! [`super::dispatch_txn::CheckerDispatchTransaction`] reentry/assumption substrate,
//! and the family `execute` path's degenerate `Miss` arm plus the
//! execute-invisibility fence are gone — decided binary judgements admit
//! into the `Relate` family slot and warm-serve through the standard family
//! read.
//!
//! Admission (design §2.3 / Decision 4): a pure non-binding SCC closes at
//! SCC-close (positive ⇒ `Assignable` + `CoinductiveCycle`; a negative
//! non-assumptive obligation ⇒ publishable `NotAssignable`); any
//! `Unknown` / budget edge routes the WHOLE component through `ReturnOnly`
//! — `Unknown` is NEVER admitted anywhere (memo / fact / reverse index),
//! and a public `BudgetExceeded` payload is returned to the caller with
//! admission suppressed (three-layer non-admission). The
//! `shallow_relation_check` prefilter survives ONLY as the O(tag) fast
//! reject INSIDE this authority (RI-5), never a parallel truth source.
//!
//! Reverse-mapped recovery's input preflight, precision boundary, opaque-state
//! polarity, and fixture ledger live in `/type-resolution` under
//! "Reverse-homomorphic mapped recovery".

use std::sync::Arc;

use rustc_hash::FxHashSet;

use super::dispatch_txn::{
    provisional_relate_step, redischarge_is_stable, select_inference_candidates,
    CompletedResolveCallMember, CompletedSccMember, FlowReturnPendingOutcome, InferenceInfoSetup,
    InferenceOccurrence, InferenceSession, InferenceSessionSetup, InferenceSessionState,
    ObligationFrameDomain, ObligationIdentity, PendingObligation, PendingObligationDomain,
    PendingVerdict, ProvisionalSubstitution, ProvisionalVerdict, RelationFrameState,
    RelationPendingState, RelationStep, ResolveCallPendingState, ReverseProjectionState,
    ReverseRecoveredEntry, SessionCheckpoint, StrictFamilyConfig,
};
use super::relation_predicates::*;
use super::ProjectSemanticDispatch;
use crate::semantic_query::{
    ConstParamPolicy, ContextualInferenceMode, DeclIdentity, IndexKey, InferBinding,
    InferenceCandidatePriority, InferencePassKind, LiteralValue, NoInferMask, OptionalityMod,
    PrimitiveKind, ProjectionReductionContext, QueryError, QueryResult, ReadonlyMod,
    RecursionOrBudgetCap, RelateKeyId, RelateMemoKey, RelationContext, RelationFailureCode,
    RelationKind, RelationOutcome, RelationPayload, RelationPolicy, RelationProof, RelationResult,
    SemanticNodeData, SemanticNodeId, SemanticQueryApi, SemanticQueryKey, SemanticQueryOutput,
    SemanticQueryValue, SubRelationPosition, SubRelationRef, SurfaceView, VariancePhase,
};
use crate::semantic_query_memo::InlineRelationFlight;

#[cfg(test)]
std::thread_local! {
    static REDISCHARGE_EXECUTE_VISITS: std::cell::Cell<usize> =
        const { std::cell::Cell::new(0) };
}

#[cfg(test)]
fn redischarge_execute_visits_for_tests() -> usize {
    REDISCHARGE_EXECUTE_VISITS.get()
}

#[cfg(test)]
pub(super) fn record_redischarge_execute_visit_for_tests() {
    REDISCHARGE_EXECUTE_VISITS.set(REDISCHARGE_EXECUTE_VISITS.get() + 1);
}

/// The O(tag) fast-reject prefilter verdict (RI-5) — the retired
/// `shallow_relation_check`, surviving ONLY as a tag-only prefilter inside
/// the relation authority. `Unknown` falls through to the full structural
/// reducer; the decided arms short-circuit BEFORE any recursive
/// structural work.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ShallowRelation {
    Assignable,
    NotAssignable,
    Unknown,
}

/// Which in-scope inference position a pattern-side `Infer` occupies —
/// drives the candidate's priority rung and combination variance.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum InferPosition {
    /// Covariant pattern position (object property, tuple/array element,
    /// bare) — the `Argument` rung.
    Covariant,
    /// Function parameter — the `Argument` rung, contravariant
    /// combination.
    ContravariantParam,
    /// Function return — the `ReturnType` rung, covariant combination.
    Return,
}

fn inference_occurrence_for_position(
    ambient: InferenceOccurrence,
    position: InferPosition,
) -> InferenceOccurrence {
    match position {
        // Structural object/array/tuple/index descent preserves the complete
        // occurrence selected by its enclosing relation position.
        InferPosition::Covariant => ambient,
        // Entering a function parameter flips orientation and starts the
        // ordinary argument-priority rung.
        InferPosition::ContravariantParam => InferenceOccurrence {
            priority: InferenceCandidatePriority::Argument,
            variance: match ambient.variance {
                VariancePhase::Covariant => VariancePhase::Contravariant,
                VariancePhase::Contravariant => VariancePhase::Covariant,
                VariancePhase::Invariant => VariancePhase::Invariant,
            },
        },
        // A return changes the priority rung while preserving the enclosing
        // orientation (including a return nested inside a parameter).
        InferPosition::Return => InferenceOccurrence {
            priority: InferenceCandidatePriority::ReturnType,
            variance: ambient.variance,
        },
    }
}

/// The shape of an in-scope conditional-`infer` pattern.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum InferPatternShape {
    /// `T extends infer X`.
    Bare,
    /// `T extends { a: infer U, .. }` — direct `Infer` member values.
    ObjectProps,
    /// `T extends [infer H, .., ...infer Rest]` — direct `Infer` elements.
    TupleHeadTail,
    /// `T extends (infer U)[]` — a direct `Infer` array element (the
    /// `Flatten` class).
    ArrayElement,
    /// `T extends (p: infer U, ..) => infer R` — direct `Infer`
    /// parameter / return positions.
    Function,
    /// `{ [P in keyof infer T]: X }` with no key remap.
    ReverseHomomorphicMapped,
}

/// Mapped modifiers whose inverse metadata effect is applied while the
/// source shape is reconstructed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ReverseMappedModifiers {
    pub(crate) optionality: OptionalityMod,
    pub(crate) readonly: ReadonlyMod,
}

/// Exact descriptor for a reverse-homomorphic mapped target.
#[derive(Debug, Clone)]
pub(crate) struct ReverseHomomorphicSpec {
    pub(crate) mapped_node: SemanticNodeId,
    pub(crate) base_infer: SemanticNodeId,
    pub(crate) mapper_parameter: SemanticNodeId,
    pub(crate) template: SemanticNodeId,
    pub(crate) modifiers: ReverseMappedModifiers,
}

enum ReverseSourceShape {
    Object,
    Array { readonly: bool },
    Tuple { readonly: bool },
}

fn reverse_optional(observed: bool, modifier: OptionalityMod) -> Option<bool> {
    match modifier {
        OptionalityMod::Add => Some(false),
        OptionalityMod::Keep => Some(observed),
        OptionalityMod::Remove => (!observed).then_some(false),
    }
}

fn reverse_readonly(observed: bool, modifier: ReadonlyMod) -> Option<bool> {
    match modifier {
        ReadonlyMod::Add => Some(false),
        ReadonlyMod::Keep => Some(observed),
        ReadonlyMod::Remove => Some(observed),
    }
}

/// One inferable parameter discovered in a pattern.
#[derive(Debug, Clone)]
pub(super) struct InferParamSite {
    /// The `Infer` node (content-free parameter identity).
    node: SemanticNodeId,
    /// The parameter display name.
    name: Arc<str>,
    /// The highest rung this site's position admits.
    priority: InferenceCandidatePriority,
}

/// The detected pattern payload: shape plus the one frozen session setup
/// shared by key construction and session opening.
#[derive(Debug, Clone)]
pub(crate) struct InferPatternInfo {
    pub(crate) shape: InferPatternShape,
    setup: InferenceSessionSetup,
    reverse_homomorphic: Option<ReverseHomomorphicSpec>,
}

impl InferPatternInfo {
    fn new(
        shape: InferPatternShape,
        sites: Vec<InferParamSite>,
        reverse_homomorphic: Option<ReverseHomomorphicSpec>,
    ) -> Self {
        let pass_kind = if reverse_homomorphic.is_some() {
            InferencePassKind::ReverseHomomorphicMapped
        } else {
            InferencePassKind::Ordinary
        };
        let candidate_priority = sites
            .iter()
            .map(|site| site.priority)
            .max_by_key(|priority| crate::semantic_query::inference_candidate_precedence(*priority))
            .unwrap_or(InferenceCandidatePriority::Argument);
        let infos = Arc::from(
            sites
                .into_iter()
                .map(|site| InferenceInfoSetup::new(site.node, site.name))
                .collect::<Vec<_>>()
                .into_boxed_slice(),
        );
        Self {
            shape,
            setup: InferenceSessionSetup::new(
                infos,
                VariancePhase::Covariant,
                pass_kind,
                candidate_priority,
                NoInferMask::empty(),
                ConstParamPolicy::NonConst,
                ContextualInferenceMode::None,
            ),
            reverse_homomorphic,
        }
    }
}

/// The relation-payload bindings a binding-producing judgement fixed at
/// session close, plus the pattern shape that produced them (the
/// closedness classifiers widen non-`Bare` shapes to `Deferred`).
#[derive(Debug, Clone)]
pub(crate) struct RelationInferBindings {
    pub(crate) shape: InferPatternShape,
    pub(crate) bindings: Arc<[InferBinding]>,
}

/// The outcome of closing the machinery ROOT frame of a relation.
enum RootClose {
    /// A decided binary judgement — publish the payload.
    Decided(RelationPayload),
    /// Budget exhaustion — PUBLIC payload, never admitted (three-layer
    /// non-admission).
    BudgetExceeded(RelationPayload),
    /// No public value-domain form (Unknown / poisoned SCC) — `Miss`.
    Undecided,
}

#[derive(Clone)]
struct ProjectedRelationMember {
    key: crate::semantic_query::PropertyKey,
    presence: crate::semantic_query::PositiveKeyPresence,
    value: crate::semantic_query::ProjectionEvidence<SemanticNodeId>,
}

#[derive(Clone)]
struct ProjectedRelationIndex {
    key_type: SemanticNodeId,
    value: crate::semantic_query::ProjectionEvidence<SemanticNodeId>,
}

#[derive(Clone)]
struct ProjectedRelationBranch {
    members: Vec<ProjectedRelationMember>,
    indices: Vec<ProjectedRelationIndex>,
    call_signatures: Vec<SemanticNodeId>,
    construct_signatures: Vec<SemanticNodeId>,
    open: bool,
}

pub(super) type DischargedMember = (
    RelateMemoKey,
    InferenceOccurrence,
    PendingVerdict,
    bool,
    bool,
    Option<super::dispatch_txn::SessionId>,
    Option<InlineRelationFlight>,
);

/// A relation-domain view over a drained tagged pending member: the SCC
/// close's verdict algebra operates on this shape; the tagged
/// `PendingObligation` storage lives in the generic ledger.
pub(super) struct DrainedRelationMember {
    pub(super) key: RelateMemoKey,
    pub(super) occurrence: InferenceOccurrence,
    pub(super) verdict: PendingVerdict,
    pub(super) session_delta: bool,
    pub(super) opened_session: Option<super::dispatch_txn::SessionId>,
    pub(super) inline_flight: Option<InlineRelationFlight>,
}

/// A flow-return-domain view over a drained tagged pending member. The
/// outcome is final at pop (a same-slot recursive backedge is a
/// coinductive hold decided by the seed check); the close admits
/// `Complete` outcomes or poisons the whole tagged component on a
/// `Degraded` one.
pub(super) struct DrainedFlowReturnMember {
    pub(super) key: crate::semantic_query::FlowReturnKey,
    pub(super) outcome: super::dispatch_txn::FlowReturnPendingOutcome,
    pub(super) inline_flight: Option<crate::semantic_query_memo::InlineFlowReturnFlight>,
    /// The coinductive hold targets the member's evaluation met — the SCC
    /// close discharges an empty-cycle member on its targets' admitted
    /// returns.
    pub(super) holds: Vec<super::flow_return_callee::HeldCallee>,
    /// The member's own file roots (unioned into the published component's
    /// self-roots).
    pub(super) self_roots: Vec<crate::semantic_query_memo::ObservedGraphSelfRoot>,
    /// The materialised point set the member's compute ACTUALLY produced
    /// (§3.4) — carried to the fenced member publish.
    pub(super) materialized: crate::semantic_query::demand::MaterializedSet,
    /// Whether the member's own contributors were all FRESH literals —
    /// the post-convergence literal-widening input.
    pub(super) fresh_seed: bool,
}

type DrainedCallResult = (
    crate::semantic_query::ResolveCallKey,
    ResolveCallPendingState,
    crate::semantic_query::ResolvedCallResult,
);

type MixedDischargeResult = Result<
    (Vec<FlowReturnPendingOutcome>, Vec<DrainedCallResult>),
    crate::semantic_query::ResolveCallFailure,
>;

/// The relation-root outcome of [`ProjectSemanticDispatch::relation_discharge_and_route`].
pub(super) struct RelationDischargeOutcome {
    /// The machinery relation root's family payload (its build output).
    pub(super) self_publish: Option<RelationPayload>,
    /// The caller-return step of an inline relation SCC root (or of a
    /// session-delta root, which never publishes).
    pub(super) self_step: Option<RelationStep>,
}

impl<'a> ProjectSemanticDispatch<'a> {
    // ──────────────────────────────────────────────────────────────────
    // The sole relation authority
    // ──────────────────────────────────────────────────────────────────

    /// Ergonomic pair constructor (design Decision 5 — a pure-delegation
    /// helper, owning ZERO memoization / cycle / assumption / admission
    /// logic): constructs the full default assignability key for
    /// `(source, target)` and delegates to [`Self::execute_relate`].
    pub(crate) fn execute_relate_pair(
        &self,
        source: SemanticNodeId,
        target: SemanticNodeId,
    ) -> RelationStep {
        self.execute_relate(self.relate_key_for(source, target))
    }

    /// Test-support adapter mapping the authority's step onto the
    /// reducer's tri-state lattice so legacy verdict assertions keep
    /// their shape. `Assumed` / `BudgetExceeded` collapse onto `Unknown`
    /// (both are non-decided from a consumer's perspective).
    #[cfg(any(test, feature = "test-support"))]
    #[doc(hidden)]
    pub fn execute_relate_pair_as_result_for_tests(
        &self,
        source: SemanticNodeId,
        target: SemanticNodeId,
    ) -> RelationResult {
        match self.execute_relate_pair(source, target) {
            RelationStep::Assignable { bindings } => RelationResult::Assignable { bindings },
            RelationStep::NotAssignable => RelationResult::NotAssignable,
            RelationStep::Unknown | RelationStep::BudgetExceeded(_) | RelationStep::Assumed(_) => {
                RelationResult::Unknown
            }
        }
    }

    #[cfg(test)]
    pub fn redischarge_execute_visits_for_tests(
        &self,
        source: SemanticNodeId,
        target: SemanticNodeId,
    ) -> usize {
        let before = redischarge_execute_visits_for_tests();
        let _ = self.relation_redischarge(
            &self.relate_key_for(source, target),
            InferenceOccurrence::ARGUMENT_COVARIANT,
            &ProvisionalSubstitution::default(),
        );
        redischarge_execute_visits_for_tests() - before
    }

    /// Exercise a one-member cyclic binding judgement through the real
    /// frame-close/fixation/re-discharge path. `negative` changes only a
    /// fixed tuple obligation, so both polarities still collect and fix
    /// the same direct-infer candidate before SCC close.
    #[cfg(test)]
    pub fn binding_scc_discharge_for_tests(
        &self,
        negative: bool,
    ) -> (RelationOutcome, Arc<[InferBinding]>, usize) {
        let graph = self.graph();
        let string = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
        let number = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
        let infer = graph.intern_node(SemanticNodeData::Infer {
            name: Arc::from("CyclicBinding"),
            binder: graph.alloc_infer_binder_id(),
        });
        let tuple = |first, second| {
            graph.intern_node(SemanticNodeData::Tuple {
                elements: Arc::from(
                    vec![
                        crate::semantic_query::TupleElement {
                            label: None,
                            value: first,
                            optional: false,
                            rest: false,
                        },
                        crate::semantic_query::TupleElement {
                            label: None,
                            value: second,
                            optional: false,
                            rest: false,
                        },
                    ]
                    .into_boxed_slice(),
                ),
                readonly: true,
            })
        };
        let source = tuple(string, if negative { string } else { number });
        let target = tuple(infer, number);
        let key = self.relation_key_with_inference(self.relate_key_for(source, target));
        let occurrence = InferenceOccurrence::ARGUMENT_COVARIANT;
        let idx = self.relation_frame_open(&key, occurrence);
        {
            // A self edge is enough to select the cyclic discharge branch;
            // the reducer below remains the real positive/negative binding
            // judgement.
            let mut txn = self.dispatch_txn.borrow_mut();
            txn.obligations.record_assumption(idx);
        }
        let mut bindings = Vec::new();
        let verdict = self.reduce_relation(&key, &mut bindings);
        let before = redischarge_execute_visits_for_tests();
        let payload = match self.relation_frame_close_root(idx, verdict, bindings) {
            RootClose::Decided(payload) => payload,
            other => panic!(
                "cyclic binding fixture must decide, got {}",
                match other {
                    RootClose::BudgetExceeded(_) => "BudgetExceeded",
                    RootClose::Undecided => "Undecided",
                    RootClose::Decided(_) => unreachable!(),
                }
            ),
        };
        (
            payload.outcome,
            Arc::clone(&payload.bindings),
            redischarge_execute_visits_for_tests() - before,
        )
    }

    /// Exercise a mixed SCC whose root fixes one binding while a nested
    /// non-binding member closes negative against an assumption edge.
    #[cfg(test)]
    pub fn mixed_binding_scc_discharge_for_tests(
        &self,
    ) -> (RelationOutcome, Arc<[InferBinding]>, usize) {
        let graph = self.graph();
        let string = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
        let number = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
        let infer = graph.intern_node(SemanticNodeData::Infer {
            name: Arc::from("MixedCyclicBinding"),
            binder: graph.alloc_infer_binder_id(),
        });
        let root_key = self.relation_key_with_inference(self.relate_key_for(string, infer));
        let member_key = self.relate_key_for(string, number);
        let occurrence = InferenceOccurrence::ARGUMENT_COVARIANT;
        let root_idx = self.relation_frame_open(&root_key, occurrence);
        let member_idx = self.relation_frame_open(&member_key, occurrence);
        {
            let mut txn = self.dispatch_txn.borrow_mut();
            txn.obligations.record_assumption(root_idx);
        }
        let mut member_bindings = Vec::new();
        let member_verdict = self.reduce_relation(&member_key, &mut member_bindings);
        let member_step = self.relation_frame_close(member_idx, member_verdict, member_bindings);
        assert!(matches!(member_step, RelationStep::NotAssignable));

        let mut root_bindings = Vec::new();
        let root_verdict = self.reduce_relation(&root_key, &mut root_bindings);
        let before = redischarge_execute_visits_for_tests();
        let payload = match self.relation_frame_close_root(root_idx, root_verdict, root_bindings) {
            RootClose::Decided(payload) => payload,
            other => panic!(
                "mixed cyclic binding fixture must decide, got {}",
                match other {
                    RootClose::BudgetExceeded(_) => "BudgetExceeded",
                    RootClose::Undecided => "Undecided",
                    RootClose::Decided(_) => unreachable!(),
                }
            ),
        };
        let result = (
            payload.outcome,
            Arc::clone(&payload.bindings),
            redischarge_execute_visits_for_tests() - before,
        );
        self.relation_abort_completed_members();
        result
    }

    /// Re-discharge a binding SCC consumer whose structural child edge is
    /// already fixed in the SCC substitution table. The returned tuple
    /// exposes the consumed binding snapshot and the real stability gate.
    #[cfg(test)]
    pub fn binding_scc_substitution_edge_for_tests(&self) -> (Arc<[InferBinding]>, bool) {
        let graph = self.graph();
        let string = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
        let unknown = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Unknown));
        let infer = graph.intern_node(SemanticNodeData::Infer {
            name: Arc::from("SubstitutionEdgeBinding"),
            binder: graph.alloc_infer_binder_id(),
        });
        let member = |value, readonly| crate::semantic_query::SurfaceMember {
            excess_origin: verter_type_expr::ExcessPropertyOrigin::NonLiteral,
            visibility: verter_type_expr::MemberVisibility::Public,
            key: crate::semantic_query::AuthoredPropertyKey::string("value"),
            value,
            optional: false,
            readonly,
            method_kind: None,
            has_implementation_body: false,
            declared_in_macro_type_arg: crate::semantic_query::MacroOwnBodyStamp::NEUTRAL,
            merge_role: crate::semantic_query::MergeRoleStamp::NEUTRAL,
            spans: Default::default(),
            declaration_origin: None,
        };
        let source = graph.intern_node(SemanticNodeData::Object(
            crate::semantic_query::surface_view! {
                members: Arc::from(vec![member(string, false)].into_boxed_slice()),
                call_signatures: Arc::from(Vec::<SemanticNodeId>::new().into_boxed_slice()),
                construct_signatures: Arc::from(Vec::<SemanticNodeId>::new().into_boxed_slice()),
                index_signatures: Arc::from(Vec::<crate::semantic_query::IndexSignature>::new().into_boxed_slice()),
                keyspace: None,
                has_index_signature: false,
            },
        ));
        let target = graph.intern_node(SemanticNodeData::Object(
            crate::semantic_query::surface_view! {
                members: Arc::from(vec![member(unknown, true)].into_boxed_slice()),
                call_signatures: Arc::from(Vec::<SemanticNodeId>::new().into_boxed_slice()),
                construct_signatures: Arc::from(Vec::<SemanticNodeId>::new().into_boxed_slice()),
                index_signatures: Arc::from(Vec::<crate::semantic_query::IndexSignature>::new().into_boxed_slice()),
                keyspace: None,
                has_index_signature: false,
            },
        ));
        let occurrence = InferenceOccurrence::ARGUMENT_COVARIANT;
        let binding = InferBinding {
            param: infer,
            name: Arc::from("SubstitutionEdgeBinding"),
            bound: string,
        };
        let fixed = Arc::from(vec![binding].into_boxed_slice());
        let substitution = ProvisionalSubstitution::from_iter([(
            ObligationIdentity::Relate {
                key: self.relate_key_for(string, unknown),
                occurrence,
            },
            ProvisionalVerdict::Relate(RelationStep::Assignable {
                bindings: Arc::clone(&fixed),
            }),
        )]);
        let rerun = self.relation_redischarge(
            &self.relate_key_for(source, target),
            occurrence,
            &substitution,
        );
        let bindings = match &rerun {
            PendingVerdict::Assignable { bindings } => Arc::clone(bindings),
            other => panic!("substitution-edge redischarge must stay assignable, got {other:?}"),
        };
        let provisional = PendingVerdict::Assignable { bindings: fixed };
        let stable = redischarge_is_stable(&provisional, &rerun);
        (bindings, stable)
    }

    /// Exercise the production nested-frame registration path for a
    /// non-binding relation member.
    #[cfg(test)]
    pub fn nested_nonbinding_frame_registers_inline_flight_for_tests(&self) -> bool {
        let graph = self.graph();
        let string = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
        let number = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
        let occurrence = InferenceOccurrence::ARGUMENT_COVARIANT;
        let root_key = self.relate_key_for(string, string);
        let member_key = self.relate_key_for(string, number);
        let root_idx = self.relation_frame_open(&root_key, occurrence);
        let member_idx = self.relation_frame_open(&member_key, occurrence);
        let member_step =
            self.relation_frame_close(member_idx, RelationResult::NotAssignable, Vec::new());
        assert!(matches!(member_step, RelationStep::NotAssignable));
        let registered = self
            .dispatch_txn
            .borrow()
            .relation
            .completed_members
            .last()
            .is_some_and(|member| member.inline_flight.is_some());
        let root_close =
            self.relation_frame_close_root(root_idx, assignable(&Vec::new()), Vec::new());
        assert!(matches!(root_close, RootClose::Decided(_)));
        self.relation_abort_completed_members();
        registered
    }

    /// The full default relation identity for `(source, target)` under the
    /// live `R T L J` env (workspace-global host view — the established
    /// one-engine convention), the EMPTY canonical substitution, and the
    /// structural-transit reduction context. The strict-family
    /// configuration in force folds into BOTH the policy (the variance
    /// regime) and the `type_env_hash` (RI-10 key isolation).
    pub(crate) fn relate_key_for(
        &self,
        source: SemanticNodeId,
        target: SemanticNodeId,
    ) -> RelateMemoKey {
        let host = self.ctx.host_for_fact_tracer_install();
        let env = host.host_view_env_hashes();
        let strict = self.relation_strict_config();
        let context = RelationContext {
            resolve_env_hash: env.resolve_env_hash,
            type_env_hash: strict.mix_into_type_env_hash(env.type_env_hash),
            lib_env_hash: env.lib_env_hash,
            project_identity: host.host_view_project_identity().0,
            substitution: crate::semantic_query::SubstitutionCanonicalHash::empty(),
            projection_reduction: ProjectionReductionContext::structural_transit(),
        };
        let mut key = RelateMemoKey::assignable(source, target, context);
        key.policy = RelationPolicy {
            variance: strict.variance_policy(),
            ..RelationPolicy::default()
        };
        key
    }

    /// The normalized strict-family configuration in force on this host
    /// (RI-10). Production always runs the TS-strict default (matching the
    /// pre-activation engine); the per-host test knob flips individual
    /// flags for the paired strict-on/off fixture.
    pub(crate) fn relation_strict_config(&self) -> StrictFamilyConfig {
        let host = self.ctx.host_for_fact_tracer_install();
        let bits = host
            .relation_knobs
            .strict_family_relax_bits
            .load(std::sync::atomic::Ordering::Relaxed);
        StrictFamilyConfig {
            strict_null_checks: bits & 0b01 == 0,
            strict_function_types: bits & 0b10 == 0,
            exact_optional_property_types: bits & 0b100 != 0,
        }
    }

    /// THE relation authority (design §2.1–§2.3). Every relation judgement
    /// enters here with a full §2.7 identity:
    ///
    /// 1. **Reentry intercept** — the identity is already in flight on
    ///    this transaction ⇒ record the scoped assumption edge and return
    ///    the `Assumed` sentinel (no recompute, no self-await, no warm
    ///    consult — the coinductive "assume it holds" step).
    /// 2. **Warm read** — a validated published payload (decided binary
    ///    outcomes only; `BudgetExceeded` / `Unknown` are never warm).
    /// 3. **Cold compute** — the machinery ROOT goes through the family
    ///    singleflight (`execute(Relate)` → `build_relate`); a nested
    ///    sub-relation computes INLINE on the transaction (its publish is
    ///    batched at its SCC's close and drained by the root).
    pub(crate) fn execute_relate(&self, key: RelateMemoKey) -> RelationStep {
        self.execute_relate_with_occurrence(key, InferenceOccurrence::ARGUMENT_COVARIANT)
    }

    /// Execute one relation under a transient inference occurrence. The
    /// occurrence is deliberately excluded from the persistent memo key:
    /// it changes only session-local candidate deposits. It is included in
    /// reentry identity so opposite-orientation visits cannot intercept one
    /// another while a reverse-inference session is active.
    fn execute_relate_with_occurrence(
        &self,
        key: RelateMemoKey,
        occurrence: InferenceOccurrence,
    ) -> RelationStep {
        let graph = self.graph();
        graph.record_relation_check();
        // Binding-producing upgrade: an in-scope `infer` pattern on the
        // target opens this judgement under the pattern's immutable
        // session-setup fingerprint. Reverse-projection sub-relations retain
        // their outer session context even though their immediate target is
        // no longer the mapped root.
        let key = self.relation_key_with_inference(key);
        // (1) Reentry intercept.
        {
            let identity = ObligationIdentity::Relate {
                key: key.clone(),
                occurrence,
            };
            let mut txn = self.dispatch_txn.borrow_mut();
            if let Some(idx) = txn.reentry().find(&identity) {
                let evidence = txn.reentry().assumption_evidence(idx);
                txn.obligations.record_assumption(idx);
                return RelationStep::Assumed(evidence);
            }
        }
        // (2) Warm read (generation-gated, carrier-validated). An active
        // inference session must execute the relation so its transient
        // projection/direct-infer deposits occur; a persistent binary warm
        // verdict cannot stand in for those session-local effects.
        if self.dispatch_txn.borrow().active_session().is_none() {
            if let Some(payload) = graph.get_relation_payload(self.ctx, &key) {
                return relation_step_from_payload(&payload);
            }
        }
        // (3) Cold compute. Root versus inline is decided by the generic
        // obligation transaction: any open frame — of any domain — makes
        // this judgement inline.
        if self.dispatch_txn.borrow().obligations.decides_root() {
            self.execute_relate_root(key)
        } else {
            self.execute_relate_inline(key, occurrence)
        }
    }

    pub(super) fn relation_redischarge_active(&self) -> bool {
        self.dispatch_txn
            .borrow()
            .relation
            .redischarge_occurrence
            .is_some()
    }

    /// Producer body used only by the `SemanticQueryApi::execute(Relate)`
    /// redischarge branch. It deliberately runs inline and every frame opened
    /// under the transient redischarge context is ReturnOnly.
    pub(super) fn execute_relate_redischarge_from_api(
        &self,
        key: RelateMemoKey,
    ) -> QueryResult<SemanticQueryValue> {
        if self.relation_key_with_inference(key.clone()) != key {
            return QueryResult::Error(QueryError::Miss);
        }
        let occurrence = self
            .dispatch_txn
            .borrow()
            .relation
            .redischarge_occurrence
            .map(|(_, occurrence)| occurrence)
            .unwrap_or(InferenceOccurrence::ARGUMENT_COVARIANT);
        let step = self.execute_relate_inline(key.clone(), occurrence);
        let payload = match step {
            RelationStep::Assignable { bindings } => self.relation_payload(
                RelationOutcome::Assignable,
                bindings,
                RelationProof::Assignable {
                    witness: crate::semantic_query::DerivationTree {
                        sub_derivations: Arc::from(Vec::new().into_boxed_slice()),
                    },
                },
            ),
            RelationStep::NotAssignable => self.relation_payload(
                RelationOutcome::NotAssignable,
                Arc::from(Vec::<InferBinding>::new().into_boxed_slice()),
                RelationProof::NotAssignable {
                    reason: RelationFailureCode::Structural,
                    failing_sub: SubRelationRef {
                        source: key.source,
                        target: key.target,
                        position: SubRelationPosition::Root,
                    },
                },
            ),
            RelationStep::BudgetExceeded(cap) => self.relation_payload(
                RelationOutcome::BudgetExceeded(cap.kind),
                Arc::from(Vec::<InferBinding>::new().into_boxed_slice()),
                RelationProof::BudgetExceeded { cap },
            ),
            RelationStep::Unknown | RelationStep::Assumed(_) => {
                return QueryResult::Error(QueryError::Miss);
            }
        };
        QueryResult::Value(SemanticQueryValue::Relation(payload))
    }

    /// The machinery ROOT path: the full family singleflight
    /// (`execute(Relate)` → warm fast path / cross-thread join / traced
    /// cold build / publish). After a published cold build, drain the
    /// SCC-closed member batch onto the root's SCC-union carrier (design
    /// §2.3 step 4 R-a batched admission).
    fn execute_relate_root(&self, key: RelateMemoKey) -> RelationStep {
        let mut publication = None;
        let read = self.execute_via_cold_build_helper_capturing_publication(
            key.to_query_key(),
            &mut publication,
        );
        let step = match read.value {
            QueryResult::Value(SemanticQueryValue::Relation(payload)) => {
                relation_step_from_payload(&payload)
            }
            // An undecided judgement surfaces `Error(Miss)` — loud, never a
            // fallback, never admitted.
            _ => RelationStep::Unknown,
        };
        if let Some(publication) = publication {
            #[cfg(any(test, feature = "test-support"))]
            self.graph().wait_relation_root_pre_member_drain_gate();
            self.relation_drain_completed_members(&key, &publication);
        } else {
            // ReturnOnly exit (poisoned SCC / budget / undecided): the
            // deferred batch releases WITHOUT publish — no entry, no fact
            // signature, no backfill, no reverse-index metadata.
            self.relation_abort_completed_members();
        }
        step
    }

    /// Binding roots carry transient candidate deposits and therefore cannot
    /// join another transaction's in-flight inference session. Completed
    /// payloads are still eligible for the explicit warm read in
    /// `execute_relate_with_occurrence`; this policy controls only the cold
    /// build after that read misses.
    #[cfg(test)]
    pub(super) fn relate_root_uses_family_singleflight(key: &RelateMemoKey) -> bool {
        key.inference_context.is_none()
    }

    /// A nested sub-relation's INLINE cold compute: push a frame, run the
    /// reducer, close the frame through the SCC discharge. The publish is
    /// NEVER direct — it is batched at this frame's SCC close and drained
    /// by the machinery root onto the SCC-union carrier.
    fn execute_relate_inline(
        &self,
        key: RelateMemoKey,
        occurrence: InferenceOccurrence,
    ) -> RelationStep {
        let idx = self.relation_frame_open(&key, occurrence);
        let mut bindings: Vec<InferBinding> = Vec::new();
        let verdict = self.reduce_relation(&key, &mut bindings);
        self.relation_frame_close(idx, verdict, bindings)
    }

    /// The family cold-build arm (the `execute(Relate)` reducer). Runs the
    /// root frame and maps the close onto the admission boundary: decided
    /// binary ⇒ publish; `BudgetExceeded` ⇒ public value, suppressed
    /// admission; undecided ⇒ `Error(Miss)`, never admitted.
    pub(super) fn build_relate(
        &self,
        key: &RelateMemoKey,
    ) -> crate::project_semantic_dispatch::walk::QueryBuildOutput<SemanticQueryValue> {
        let fence = self.project_generation_signature();
        // A raw `SemanticQueryKey::Relate` can enter the family dispatcher
        // without passing through `execute_relate`. Refuse any such key whose
        // supplied inference context does not equal the target pattern's
        // immutable setup projection. Otherwise release builds could execute
        // one session setup while admitting under another fingerprint.
        if self.relation_key_with_inference(key.clone()) != *key {
            let mut output: crate::project_semantic_dispatch::walk::QueryBuildOutput<
                SemanticQueryValue,
            > = (QueryResult::Error(QueryError::Miss), fence).into();
            output.cache_suppress = true;
            return output;
        }
        // Test-only fact-injection hook (ported from the retired
        // `relate_nodes` cold path): when the host's per-host
        // `relation_knobs.force_overflow_observations` knob is non-zero, emit
        // that many synthetic `FileWholeHash` observations onto the
        // active tracer so finalise reports `Overflow` once the
        // per-signature cap is exceeded — exercising the overflow
        // non-admission path without a pathological multi-file fixture.
        let host = self.ctx.host_for_fact_tracer_install();
        let force_n = host
            .relation_knobs
            .force_overflow_observations
            .load(std::sync::atomic::Ordering::Relaxed);
        if force_n > 0 {
            for n in 0..force_n {
                crate::resolver_core::resolver_context::observe_fan_out(
                    crate::resolver_core::FactVersionRef::FileWholeHash {
                        canonical_id: format!("__relation_force_overflow_{n}.ts"),
                        hash: [(n & 0xff) as u8; 16],
                    },
                );
            }
        }
        let idx = self.relation_frame_open(key, InferenceOccurrence::ARGUMENT_COVARIANT);
        let mut bindings: Vec<InferBinding> = Vec::new();
        let verdict = self.reduce_relation(key, &mut bindings);
        match self.relation_frame_close_root(idx, verdict, bindings) {
            RootClose::Decided(payload) => {
                let observed_self_roots = self.relation_completed_publication_roots(key);
                crate::project_semantic_dispatch::walk::QueryBuildOutput::from((
                    QueryResult::Value(SemanticQueryValue::Relation(payload)),
                    fence,
                ))
                .with_observed_self_roots(observed_self_roots)
            }
            RootClose::BudgetExceeded(payload) => {
                let observed_self_roots =
                    self.observed_self_roots_from_nodes([key.source, key.target]);
                let mut output: crate::project_semantic_dispatch::walk::QueryBuildOutput<
                    SemanticQueryValue,
                > = (
                    QueryResult::Value(SemanticQueryValue::Relation(payload)),
                    fence,
                )
                    .into();
                // ReturnOnly-but-public: the value flows to the caller, the
                // memo refuses admission (no warm entry, no fact signature,
                // no reverse-index metadata).
                output.cache_suppress = true;
                output.observed_self_roots = observed_self_roots;
                output
            }
            RootClose::Undecided => (QueryResult::Error(QueryError::Miss), fence).into(),
        }
    }

    // ──────────────────────────────────────────────────────────────────
    // Frames, sessions, and the SCC discharge
    // ──────────────────────────────────────────────────────────────────

    /// Push a reentry frame for `key`, opening an inference session when
    /// the key carries a fingerprint and no session is active (a binding
    /// root). Also snapshots the strict configuration at the root push.
    fn relation_frame_open(&self, key: &RelateMemoKey, occurrence: InferenceOccurrence) -> usize {
        // Snapshot the strict config + pattern BEFORE taking the borrow —
        // `relation_pattern_info` re-borrows the transaction (its
        // per-target cache lives there).
        let strict = self.relation_strict_config();
        let redischarge = self.relation_redischarge_active();
        let wants_inline_flight = !redischarge
            && key.inference_context.is_none()
            && !self.dispatch_txn.borrow().obligations.decides_root();
        let inline_flight = wants_inline_flight
            .then(|| self.graph().begin_inline_relation_flight(key))
            .flatten();
        let wants_session = key.inference_context.is_some()
            && self.dispatch_txn.borrow().active_session().is_none();
        let pattern = if wants_session {
            self.relation_pattern_info(key.target)
        } else {
            None
        };
        let mut txn = self.dispatch_txn.borrow_mut();
        if txn.reentry().nearest_relate().is_none() {
            // Re-snapshot at every relation ROOT so the behavioral branch
            // and the key's strict fold can never diverge (the key reads
            // the live config; the reducer reads this snapshot).
            txn.relation.strict = Some(strict);
        }
        let watermark = txn.obligations.pending().pending_len();
        let idx = txn
            .reentry_mut()
            .push_relate(key.clone(), occurrence, watermark);
        txn.note_inline_flight(idx, inline_flight);
        if redischarge {
            txn.note_session_delta_range(idx, idx + 1);
        }
        if wants_session {
            if let Some(pattern) = pattern {
                let session_id = txn.push_collecting_session(
                    pattern.setup,
                    pattern.reverse_homomorphic.map(ReverseProjectionState::new),
                );
                // Both identities come from the same immutable setup value;
                // candidate collection cannot make them diverge.
                debug_assert_eq!(
                    txn.relation
                        .sessions
                        .last()
                        .map(InferenceSession::context_key),
                    key.inference_context.as_ref(),
                    "the opened session must retain the relation key's frozen inference setup"
                );
                txn.note_opened_session(idx, session_id);
            }
        }
        idx
    }

    /// Close an INLINE frame: stage and commit an owned relation session,
    /// classify the pop (SCC-root vs provisional member), and run the SCC
    /// discharge at the root. Returns the caller-return step (PROVISIONAL
    /// for an unpublished member — never itself the published payload).
    fn relation_frame_close(
        &self,
        idx: usize,
        verdict: RelationResult,
        bindings: Vec<InferBinding>,
    ) -> RelationStep {
        match self.relation_frame_pop(idx, verdict, bindings, false) {
            FramePop::Provisional(step) => step,
            FramePop::RootClose(close) => {
                // Poison surfaces (budget / undecided) reach the inline
                // caller as steps; a decided inline SCC root returned
                // through the provisional path above.
                match close {
                    RootClose::Decided(payload) => relation_step_from_payload(&payload),
                    RootClose::BudgetExceeded(payload) => relation_step_from_payload(&payload),
                    RootClose::Undecided => RelationStep::Unknown,
                }
            }
        }
    }

    /// Close the machinery ROOT frame (same pop machinery, plus the
    /// public-outcome mapping).
    fn relation_frame_close_root(
        &self,
        idx: usize,
        verdict: RelationResult,
        bindings: Vec<InferBinding>,
    ) -> RootClose {
        match self.relation_frame_pop(idx, verdict, bindings, true) {
            FramePop::RootClose(close) => close,
            FramePop::Provisional(_) => unreachable!(
                "the machinery root frame is always its SCC's root: the stack is \
                 empty below it, so no open assumption can target a deeper frame"
            ),
        }
    }

    /// The shared frame-pop + SCC-discharge engine. On a non-root pop the
    /// member defers PROVISIONALLY to the ledger and returns its
    /// caller-return step; on an SCC-root pop the whole component
    /// discharges (design §2.3 steps 3–4). `machinery_root` distinguishes
    /// the family singleflight's root frame (its payload returns as the
    /// build output) from an inline SCC root (its payload batch-publishes
    /// with the SCC drain).
    fn relation_frame_pop(
        &self,
        idx: usize,
        verdict: RelationResult,
        bindings: Vec<InferBinding>,
        machinery_root: bool,
    ) -> FramePop {
        // Session fixation: a binding root's session closes at the frame's
        // pop — after EVERY member's candidates have deposited (the
        // session's opener is the outermost frame of its deposits). A
        // budget edge inside the session ABANDONS it (design admission
        // row 8 — budget-exceeded abandon ⇒ ReturnOnly).
        let (popped, self_cycle) = {
            let mut txn = self.dispatch_txn.borrow_mut();
            let popped = txn.reentry_mut().pop();
            let self_cycle = popped.assumption_targets.contains(&idx);
            (popped, self_cycle)
        };
        // The popped frame is always a RELATION frame on this code path:
        // unpack its tagged identity and domain state into the relation
        // close's local shape.
        let (frame_key, frame_occurrence) = {
            let (key, occurrence) = popped.identity.expect_relate();
            (key.clone(), occurrence)
        };
        let budget_cap = popped.budget_cap;
        let min_open_target = popped.min_open_target;
        let pending_watermark = popped.pending_watermark;
        let self_assumptive = !popped.assumption_targets.is_empty();
        let RelationFrameState {
            session_delta,
            opened_session,
            inline_flight,
        } = match popped.domain {
            ObligationFrameDomain::Relate(state) => state,
            ObligationFrameDomain::FlowReturn(_) | ObligationFrameDomain::ResolveCall(_) => {
                unreachable!("a relation code path pops a relation frame")
            }
        };
        let mut session_bindings: Option<Arc<[InferBinding]>> = None;
        let mut session_abandoned = false;
        if let Some(sid) = opened_session {
            let mut txn = self.dispatch_txn.borrow_mut();
            if let Some(position) = txn.relation.sessions.iter().position(|s| s.id == sid) {
                if budget_cap.is_some() {
                    txn.relation.sessions[position].abandon();
                    session_abandoned = true;
                } else {
                    let combine = |nodes: &[SemanticNodeId], variance: VariancePhase| {
                        self.relation_combine_candidates(nodes, variance)
                    };
                    let mut session = txn.relation.sessions.remove(position);
                    let fixed = session.stage_fixation(combine);
                    let committed = session.commit_completed();
                    let state = session.state;
                    txn.relation.sessions.insert(position, session);
                    match state {
                        InferenceSessionState::CommittedDeterministic => {
                            debug_assert!(committed, "relation fixation commits at its safe pop");
                            session_bindings = fixed;
                        }
                        InferenceSessionState::Abandoned => session_abandoned = true,
                        InferenceSessionState::Collecting
                        | InferenceSessionState::StagedDeterministic => unreachable!(
                            "relation fixation stages and commits before the frame closes"
                        ),
                    }
                }
            }
        }
        let pending = pending_verdict_of(&verdict, &budget_cap, &mut session_bindings, bindings);
        let is_scc_root = match min_open_target {
            None => true,
            Some(target) => target >= idx,
        };
        if !is_scc_root {
            // PROVISIONAL member: defer to the ledger, propagate the still-
            // open lowlink to the parent, and return the caller-return
            // step. NEVER publishes here. A binding member additionally
            // records into its session's `SessionAdmissionLedger` (design
            // §2.3 step 4 — it admits only at its session's close, drained
            // at the SCC's batched-publish instant below).
            let step = relation_step_from_pending(&pending);
            let mut txn = self.dispatch_txn.borrow_mut();
            txn.obligations.propagate_lowlink(min_open_target);
            if let Some(sid) = opened_session {
                txn.relation.session_admission.defer(sid, frame_key.clone());
            }
            txn.obligations.pending_mut().deposit(PendingObligation {
                identity: ObligationIdentity::Relate {
                    key: frame_key.clone(),
                    occurrence: frame_occurrence,
                },
                domain: PendingObligationDomain::Relate(RelationPendingState {
                    verdict: pending,
                    session_delta,
                    opened_session,
                    inline_flight,
                }),
            });
            return FramePop::Provisional(step);
        }

        // ── SCC close at this root (design §2.3 step 3) ──────────────
        // Drain by the frame's push-time watermark, NEVER by stack index —
        // indices recycle after pops, and a recycled index would let this
        // close steal a pending member of a still-open outer SCC (which
        // would then publish a stale provisional verdict).
        let mut flow_members: Vec<DrainedFlowReturnMember> = Vec::new();
        let mut members: Vec<DrainedRelationMember> = Vec::new();
        let mut call_members: Vec<(
            crate::semantic_query::ResolveCallKey,
            ResolveCallPendingState,
        )> = Vec::new();
        for member in self
            .dispatch_txn
            .borrow_mut()
            .obligations
            .pending_mut()
            .drain_scc(pending_watermark)
        {
            match member.domain {
                PendingObligationDomain::Relate(state) => {
                    let (key, occurrence) = member.identity.expect_relate();
                    members.push(DrainedRelationMember {
                        key: key.clone(),
                        occurrence,
                        verdict: state.verdict,
                        session_delta: state.session_delta,
                        opened_session: state.opened_session,
                        inline_flight: state.inline_flight,
                    });
                }
                PendingObligationDomain::FlowReturn(state) => {
                    let key = member
                        .identity
                        .as_flow_return()
                        .expect("flow-return pending member carries a flow identity")
                        .clone();
                    flow_members.push(DrainedFlowReturnMember {
                        key,
                        outcome: state.outcome,
                        inline_flight: state.inline_flight,
                        holds: state.holds,
                        self_roots: state.self_roots,
                        materialized: state.materialized,
                        fresh_seed: state.fresh_seed,
                    });
                }
                PendingObligationDomain::ResolveCall(state) => {
                    let state = *state;
                    let key = member
                        .identity
                        .as_resolve_call()
                        .expect("call pending member carries a call identity")
                        .clone();
                    call_members.push((key, state));
                }
            }
        }
        let cyclic = !members.is_empty()
            || !flow_members.is_empty()
            || !call_members.is_empty()
            || self_cycle;

        // Row 3 batched poison: ANY Unknown / budget / abandoned-session
        // edge anywhere in the component routes the WHOLE SCC through
        // ReturnOnly — nothing publishes.
        let budget_cap = budget_cap.or_else(|| {
            members.iter().find_map(|m| match &m.verdict {
                PendingVerdict::BudgetExceeded(cap) => Some(*cap),
                _ => None,
            })
        });
        // The mixed component discharges to ONE joint fixed point: the
        // flow members' callee-clause transfer / empty-cycle resurrection
        // and the call members' replay + return equation iterate against
        // each other's current values until neither moves — either
        // refusing poisons the whole component exactly like a degraded
        // flow member.
        let initial_substitution: ProvisionalSubstitution = std::iter::once((
            ObligationIdentity::Relate {
                key: frame_key.clone(),
                occurrence: frame_occurrence,
            },
            ProvisionalVerdict::Relate(relation_step_from_pending(&pending)),
        ))
        .chain(members.iter().map(|member| {
            (
                ObligationIdentity::Relate {
                    key: member.key.clone(),
                    occurrence: member.occurrence,
                },
                ProvisionalVerdict::Relate(relation_step_from_pending(&member.verdict)),
            )
        }))
        .collect();
        // A degraded flow member AFTER the joint discharge poisons the
        // WHOLE tagged component (atomic admission: nothing publishes,
        // every flight aborts). The check runs post-discharge because the
        // fixed point resurrects hold-only empty cycles — poisoning on
        // the pre-discharge outcome condemns members the close recovers.
        let (_prefix_outcomes, call_results) = match self.discharge_mixed_component_to_fixed_point(
            Vec::new(),
            &mut flow_members,
            &mut call_members,
            &initial_substitution,
        ) {
            Ok(ok) => ok,
            Err(failure) => {
                self.relation_abort_inline_flight(inline_flight.as_ref());
                for member in &members {
                    self.relation_abort_inline_flight(member.inline_flight.as_ref());
                }
                self.flow_return_abort_drained_flights(&flow_members);
                for (_, member) in &call_members {
                    self.resolve_call_abort_inline_flight(member.inline_flight.as_ref());
                    if let Some(session) = member.staged_session {
                        self.abandon_session(session);
                    }
                }
                return FramePop::RootClose(match failure {
                    crate::semantic_query::ResolveCallFailure::Budget => {
                        RootClose::BudgetExceeded(self.relation_payload(
                            RelationOutcome::BudgetExceeded(
                                crate::semantic_query::BudgetExceededKind::CallResolutionBudget,
                            ),
                            Arc::from([]),
                            RelationProof::BudgetExceeded {
                                cap: RecursionOrBudgetCap {
                                    kind: crate::semantic_query::BudgetExceededKind::CallResolutionBudget,
                                    limit: super::call_resolve::MAX_CANDIDATES_STARTED as u32,
                                },
                            },
                        ))
                    }
                    _ => RootClose::Undecided,
                });
            }
        };
        // Any poison edge (a post-discharge flow no-value, an abandoned
        // session, a budget verdict anywhere, an Unknown) routes the
        // WHOLE SCC through ReturnOnly — nothing publishes, every flight
        // aborts. The flow check runs POST-discharge: the joint fixed
        // point resurrects hold-only empty cycles, so poisoning on the
        // pre-discharge outcome would condemn members the close recovers.
        let poisoned = flow_members
            .iter()
            .any(|member| matches!(member.outcome, FlowReturnPendingOutcome::NoValue { .. }))
            || session_abandoned
            || budget_cap.is_some()
            || matches!(
                pending,
                PendingVerdict::Unknown | PendingVerdict::BudgetExceeded(_)
            )
            || members.iter().any(|m| {
                matches!(
                    m.verdict,
                    PendingVerdict::Unknown | PendingVerdict::BudgetExceeded(_)
                )
            });
        if poisoned {
            self.relation_abort_inline_flight(inline_flight.as_ref());
            for member in &members {
                self.relation_abort_inline_flight(member.inline_flight.as_ref());
            }
            self.flow_return_abort_drained_flights(&flow_members);
            self.resolve_call_abort_drained_flights(&call_results);
            // Release WITHOUT publish (no entry / fact signature /
            // backfill / reverse-index metadata). The machinery root
            // surfaces the public `BudgetExceeded` payload when a budget
            // edge drove the poison.
            if let Some(cap) = budget_cap {
                let payload = self.relation_payload(
                    RelationOutcome::BudgetExceeded(cap.kind),
                    Arc::from(Vec::<InferBinding>::new().into_boxed_slice()),
                    RelationProof::BudgetExceeded { cap },
                );
                return FramePop::RootClose(RootClose::BudgetExceeded(payload));
            }
            return FramePop::RootClose(RootClose::Undecided);
        }

        match self.relation_discharge_and_route(
            machinery_root,
            Some((
                frame_key.clone(),
                frame_occurrence,
                pending,
                self_assumptive,
                session_delta,
                opened_session,
                inline_flight,
            )),
            members,
            flow_members,
            None,
            call_results,
            cyclic,
        ) {
            Ok(outcome) => {
                if let Some(step) = outcome.self_step {
                    return FramePop::Provisional(step);
                }
                FramePop::RootClose(RootClose::Decided(
                    outcome
                        .self_publish
                        .expect("the machinery root always produces its own payload"),
                ))
            }
            Err(cap) => {
                // The component released WITHOUT publish (aborted session,
                // lost ledger member, non-stable redischarge, or a poisoned
                // relation member). The machinery root surfaces the public
                // `BudgetExceeded` payload when a budget edge drove it.
                if let Some(cap) = cap {
                    let payload = self.relation_payload(
                        RelationOutcome::BudgetExceeded(cap.kind),
                        Arc::from(Vec::<InferBinding>::new().into_boxed_slice()),
                        RelationProof::BudgetExceeded { cap },
                    );
                    return FramePop::RootClose(RootClose::BudgetExceeded(payload));
                }
                FramePop::RootClose(RootClose::Undecided)
            }
        }
    }

    /// The relation-member half of a tagged SCC close (the generic
    /// coordinator's discharge step, design §2.3 steps 3–4): build the
    /// discharged set from the optional relation root plus the drained
    /// relation members, gate binding members on their session's drain,
    /// redischarge deepest-first/root-last against the ONE tagged
    /// provisional substitution table, and route every decided member —
    /// and every completed flow member — into the batched publish queue.
    ///
    /// `root_relation = None` is the FLOW-rooted shape: the close's root
    /// is a flow frame, so every relation member routes to the completed
    /// batch (no self publish, no self step) and the flow root publishes
    /// through its own family.
    ///
    /// Returns `Err(cap)` when the component releases WITHOUT publish
    /// (abandoned session, lost ledger member, non-stable redischarge, or
    /// a poisoned relation member); `cap` carries the budget edge when
    /// one drove the abort. The helper aborts every flight it owns; the
    /// caller aborts its own root flight.
    #[allow(clippy::type_complexity)]
    pub(super) fn relation_discharge_and_route(
        &self,
        machinery_root: bool,
        root_relation: Option<(
            RelateMemoKey,
            InferenceOccurrence,
            PendingVerdict,
            bool,
            bool,
            Option<super::dispatch_txn::SessionId>,
            Option<InlineRelationFlight>,
        )>,
        members: Vec<DrainedRelationMember>,
        flow_members: Vec<DrainedFlowReturnMember>,
        provisional_call_root: Option<(
            crate::semantic_query::ResolveCallKey,
            crate::semantic_query::ResolvedCallResult,
            Option<super::dispatch_txn::SessionId>,
        )>,
        call_members: Vec<(
            crate::semantic_query::ResolveCallKey,
            ResolveCallPendingState,
            crate::semantic_query::ResolvedCallResult,
        )>,
        cyclic: bool,
    ) -> Result<RelationDischargeOutcome, Option<RecursionOrBudgetCap>> {
        // Discharge verdicts — a member recorded POSITIVE that consumed
        // assumptions re-discharges against the converged state when ANY
        // member closed NEGATIVE (the collapsed-back-edge case, design
        // §2.3 step 3); a non-stable re-discharge (a flip to Unknown)
        // releases the whole batch without publish. Each record carries
        // the member's session-delta flag (row 7: a session-local delta
        // never publishes) and its opened-session token (a binding
        // member admits only through its session's
        // `SessionAdmissionLedger` drain below).
        let any_negative = root_relation
            .as_ref()
            .is_some_and(|(_, _, pending, _, _, _, _)| {
                matches!(pending, PendingVerdict::NotAssignable)
            })
            || members
                .iter()
                .any(|m| matches!(m.verdict, PendingVerdict::NotAssignable));
        let mut discharged: Vec<DischargedMember> = Vec::new();
        if let Some((
            key,
            occurrence,
            pending,
            self_assumptive,
            session_delta,
            opened_session,
            flight,
        )) = root_relation
        {
            if let Some(sid) = opened_session {
                self.dispatch_txn
                    .borrow_mut()
                    .relation
                    .session_admission
                    .defer(sid, key.clone());
            }
            discharged.push((
                key,
                occurrence,
                pending,
                self_assumptive,
                session_delta,
                opened_session,
                flight,
            ));
        }
        let has_relation_root = !discharged.is_empty();
        for member in members {
            discharged.push((
                member.key,
                member.occurrence,
                member.verdict,
                true,
                member.session_delta,
                member.opened_session,
                member.inline_flight,
            ));
        }
        let has_binding_member = discharged
            .iter()
            .any(|(key, _, _, _, _, opened_session, _)| {
                opened_session.is_some() || key.inference_context.is_some()
            });
        let has_return_member =
            !flow_members.is_empty() || provisional_call_root.is_some() || !call_members.is_empty();
        // Re-discharge is an SCC-close operation. A redischarge itself
        // opens an ordinary acyclic frame; allowing a merely-negative
        // binding result to enter this branch again would recursively
        // redischarge forever.
        if cyclic && (any_negative || has_binding_member || has_return_member) {
            let mut substitution: ProvisionalSubstitution = discharged
                .iter()
                .map(|(key, occurrence, verdict, _, _, _, _)| {
                    (
                        ObligationIdentity::Relate {
                            key: key.clone(),
                            occurrence: *occurrence,
                        },
                        ProvisionalVerdict::Relate(relation_step_from_pending(verdict)),
                    )
                })
                .collect();
            substitution.extend(call_members.iter().map(|(key, _, result)| {
                (
                    ObligationIdentity::ResolveCall(key.clone()),
                    ProvisionalVerdict::ResolveCall(result.clone()),
                )
            }));
            substitution.extend(provisional_call_root.iter().map(|(key, result, _)| {
                (
                    ObligationIdentity::ResolveCall(key.clone()),
                    ProvisionalVerdict::ResolveCall(result.clone()),
                )
            }));
            // Bottom-up over the condensation: re-discharge the POSITIVE
            // assumption-consuming members DEEPEST-FIRST so a shallower
            // member re-runs against the FINAL deeper verdicts. Layout:
            // `discharged[0]` is the SCC root (shallowest) when there is
            // a relation root; `discharged[1..]` are the drained members
            // in POP order — deepest-popped first — so deepest-first is
            // positions `1..len` in order, with the root LAST. With no
            // relation root the drained members themselves already run
            // deepest-first and none is privileged. (The reversed scan
            // froze a shallow member against a stale provisional deep
            // `Assignable` before the deep member flipped on its
            // collapsed back-edge.)
            let order: Vec<usize> = if has_relation_root {
                (1..discharged.len()).chain(std::iter::once(0)).collect()
            } else {
                (0..discharged.len()).collect()
            };
            for position in order {
                let (key, occurrence, verdict, assumptive, _, opened_session, _) =
                    &discharged[position];
                let binding_member = opened_session.is_some() || key.inference_context.is_some();
                let must_redischarge = if has_binding_member || has_return_member {
                    true
                } else {
                    *assumptive && matches!(verdict, PendingVerdict::Assignable { .. })
                };
                if !binding_member && !must_redischarge {
                    continue;
                }
                let key = key.clone();
                let occurrence = *occurrence;
                let rerun = self.relation_redischarge(&key, occurrence, &substitution);
                match rerun {
                    PendingVerdict::Unknown => {
                        // Non-stable re-discharge ⇒ release the whole batch
                        // WITHOUT publish (joiners recompute).
                        self.relation_abort_discharged_flights(&discharged);
                        self.flow_return_abort_drained_flights(&flow_members);
                        self.resolve_call_abort_drained_flights(&call_members);
                        return Err(None);
                    }
                    PendingVerdict::BudgetExceeded(cap) => {
                        self.relation_abort_discharged_flights(&discharged);
                        self.flow_return_abort_drained_flights(&flow_members);
                        self.resolve_call_abort_drained_flights(&call_members);
                        return Err(Some(cap));
                    }
                    stable => {
                        if (has_binding_member || has_return_member)
                            && !redischarge_is_stable(verdict, &stable)
                        {
                            // A binding SCC may publish only when every
                            // member retains its provisional polarity and the
                            // binding members retain their complete fixed
                            // binding snapshot. Pure non-binding SCCs instead
                            // converge bottom-up: a provisional positive may
                            // legitimately collapse to the final negative
                            // verdict carried by its dependency.
                            self.relation_abort_discharged_flights(&discharged);
                            self.flow_return_abort_drained_flights(&flow_members);
                            self.resolve_call_abort_drained_flights(&call_members);
                            return Err(None);
                        }
                        substitution.insert(
                            ObligationIdentity::Relate {
                                key: key.clone(),
                                occurrence,
                            },
                            ProvisionalVerdict::Relate(relation_step_from_pending(&stable)),
                        );
                        discharged[position].2 = stable;
                    }
                }
            }
        }

        let deferred_relation_sessions = discharged
            .iter()
            .filter_map(|(key, _, _, _, _, opened_session, _)| {
                opened_session.map(|session| (session, key.clone()))
            })
            .collect::<Vec<_>>();
        // Validate without consuming. From call-session commit through the
        // ledger drain and completed-member enqueue, no semantic work runs.
        let ledgers_ready = {
            let txn = self.dispatch_txn.borrow();
            deferred_relation_sessions.iter().all(|(session, key)| {
                let session_ok = txn
                    .relation
                    .sessions
                    .iter()
                    .find(|candidate| candidate.id == *session)
                    .is_some_and(|candidate| {
                        candidate.state == InferenceSessionState::CommittedDeterministic
                    });
                session_ok && txn.relation.session_admission.contains(*session, key)
            })
        };
        if !ledgers_ready {
            self.relation_abort_discharged_flights(&discharged);
            self.flow_return_abort_drained_flights(&flow_members);
            self.resolve_call_abort_drained_flights(&call_members);
            return Err(None);
        }

        // Publish routing (design §2.3 step 4): decided members queue for
        // the root's batched publish onto the SCC-union carrier; a
        // session-local delta (row 7) never publishes.
        let scc_keys: Arc<[RelateKeyId]> = if cyclic {
            let keys: Vec<RelateKeyId> = discharged
                .iter()
                .map(|(key, _, _, _, _, _, _)| self.graph().intern_relate_key(key.clone()))
                .collect();
            Arc::from(keys.into_boxed_slice())
        } else {
            Arc::from(Vec::<RelateKeyId>::new().into_boxed_slice())
        };
        let mut self_publish: Option<RelationPayload> = None;
        let mut self_step: Option<RelationStep> = None;
        let mut completed: Vec<CompletedSccMember> = Vec::new();
        for (position, (key, _, verdict, _, session_delta, _, inline_flight)) in
            discharged.into_iter().enumerate()
        {
            let is_self = has_relation_root && position == 0;
            let payload = match &verdict {
                PendingVerdict::Assignable { bindings } => {
                    let proof = if cyclic {
                        RelationProof::CoinductiveCycle {
                            keys: Arc::clone(&scc_keys),
                        }
                    } else {
                        RelationProof::Assignable {
                            witness: crate::semantic_query::DerivationTree {
                                sub_derivations: Arc::from(
                                    vec![SubRelationRef {
                                        source: key.source,
                                        target: key.target,
                                        position: SubRelationPosition::Root,
                                    }]
                                    .into_boxed_slice(),
                                ),
                            },
                        }
                    };
                    self.relation_payload(RelationOutcome::Assignable, Arc::clone(bindings), proof)
                }
                PendingVerdict::NotAssignable => self.relation_payload(
                    RelationOutcome::NotAssignable,
                    Arc::from(Vec::<InferBinding>::new().into_boxed_slice()),
                    RelationProof::NotAssignable {
                        reason: RelationFailureCode::Structural,
                        failing_sub: SubRelationRef {
                            source: key.source,
                            target: key.target,
                            position: SubRelationPosition::Root,
                        },
                    },
                ),
                PendingVerdict::Unknown | PendingVerdict::BudgetExceeded(_) => {
                    unreachable!("poisoned SCCs return before the publish routing")
                }
            };
            if is_self {
                if session_delta {
                    // Admission row 7: a session-local delta never
                    // publishes — the caller gets the computed step.
                    self.relation_abort_inline_flight(inline_flight.as_ref());
                    self_step = Some(relation_step_from_payload(&payload));
                } else if machinery_root {
                    // The machinery root publishes through the family
                    // singleflight (its build output IS this payload).
                    self_publish = Some(payload);
                } else {
                    // An inline SCC root: its payload batch-publishes with
                    // the SCC (drained by the machinery root); the caller
                    // consumes the computed step.
                    self_step = Some(relation_step_from_payload(&payload));
                    completed.push(CompletedSccMember {
                        key,
                        payload,
                        inline_flight,
                    });
                }
            } else if !session_delta {
                completed.push(CompletedSccMember {
                    key,
                    payload,
                    inline_flight,
                });
            } else {
                self.relation_abort_inline_flight(inline_flight.as_ref());
            }
        }
        let staged_call_sessions = call_members
            .iter()
            .filter_map(|(_, state, _)| state.staged_session)
            .chain(
                provisional_call_root
                    .iter()
                    .filter_map(|(_, _, session)| *session),
            )
            .collect::<Vec<_>>();
        if !self.commit_call_sessions(&staged_call_sessions) {
            for member in &completed {
                self.relation_abort_inline_flight(member.inline_flight.as_ref());
            }
            self.flow_return_abort_drained_flights(&flow_members);
            self.resolve_call_abort_drained_flights(&call_members);
            return Err(None);
        }
        let mut rootless_flights = Vec::new();
        {
            let mut txn = self.dispatch_txn.borrow_mut();
            for (session, _) in deferred_relation_sessions {
                let _ = txn.relation.session_admission.drain(session);
            }
            txn.relation.completed_members.extend(completed);
            for member in flow_members {
                let FlowReturnPendingOutcome::Complete(result) = member.outcome else {
                    unreachable!(
                        "a degraded flow member poisons the whole tagged component at the close"
                    )
                };
                txn.flow
                    .completed_members
                    .push(super::dispatch_txn::CompletedFlowReturnMember {
                        key: member.key,
                        result,
                        inline_flight: member.inline_flight,
                        self_roots: member.self_roots,
                        materialized: member.materialized,
                    });
            }
            for (key, state, result) in call_members {
                // A rootless winner has no stable occurrence to key a
                // shared entry on: it stays transaction-local, so its
                // inline flight is released instead of queued.
                match crate::semantic_query::AdmissibleCallResult::new(result) {
                    Some(result) => txn.call.completed_members.push(CompletedResolveCallMember {
                        key,
                        result,
                        inline_flight: state.inline_flight,
                        self_roots: state.self_roots,
                    }),
                    None => rootless_flights.push(state.inline_flight),
                }
            }
        }
        for flight in rootless_flights {
            self.resolve_call_abort_inline_flight(flight.as_ref());
        }
        Ok(RelationDischargeOutcome {
            self_publish,
            self_step,
        })
    }

    /// Re-discharge ONE member of a negatively-closed SCC against the
    /// converged state (design §2.3 step 4): the member's cold compute
    /// re-runs through the same `execute(Relate)` dispatch with the SCC's
    /// discharged verdicts as the substitution table, so a stale
    /// SCC-close snapshot is impossible by construction.
    fn relation_redischarge(
        &self,
        key: &RelateMemoKey,
        occurrence: InferenceOccurrence,
        substitution: &ProvisionalSubstitution,
    ) -> PendingVerdict {
        let saved_context = {
            let mut txn = self.dispatch_txn.borrow_mut();
            let next_substitution = substitution
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect();
            txn.replace_redischarge_context(next_substitution, occurrence)
        };
        let verdict = self.execute(key.to_query_key());
        self.dispatch_txn
            .borrow_mut()
            .restore_redischarge_context(saved_context);
        match verdict {
            QueryResult::Value(SemanticQueryOutput {
                value: SemanticQueryValue::Relation(payload),
                ..
            }) => match payload.outcome {
                RelationOutcome::Assignable => PendingVerdict::Assignable {
                    bindings: Arc::clone(&payload.bindings),
                },
                RelationOutcome::NotAssignable => PendingVerdict::NotAssignable,
                RelationOutcome::BudgetExceeded(_) => PendingVerdict::Unknown,
            },
            _ => PendingVerdict::Unknown,
        }
    }

    /// Fixation combinator (design §4.2 candidate combination): covariant
    /// candidates union (canonicalized), contravariant candidates
    /// intersect, a single candidate binds directly, and an unfixed
    /// parameter deterministically defaults to `unknown`.
    pub(super) fn relation_combine_candidates(
        &self,
        nodes: &[SemanticNodeId],
        variance: VariancePhase,
    ) -> SemanticNodeId {
        let graph = self.graph();
        match nodes {
            [] => graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Unknown)),
            [single] => *single,
            many => {
                let mut dedup: Vec<SemanticNodeId> = many.to_vec();
                dedup.sort_by_key(|id| id.0);
                dedup.dedup();
                if dedup.len() == 1 {
                    return dedup[0];
                }
                if matches!(variance, VariancePhase::Contravariant) {
                    // Intersection combination: tag-level-disjoint
                    // candidate pairs (distinct primitives, distinct
                    // literals, literal against a mismatched base
                    // primitive) collapse the intersection to `never`;
                    // undecidable shapes conservatively keep the
                    // structural Intersection carrier.
                    let disjoint = dedup.iter().enumerate().any(|(i, &a)| {
                        dedup[i + 1..]
                            .iter()
                            .any(|&b| tag_level_disjoint(graph, a, b))
                    });
                    if disjoint {
                        return graph
                            .intern_node(SemanticNodeData::Primitive(PrimitiveKind::Never));
                    }
                    return self.intern_normalized_union_or_intersection(&dedup, false);
                }
                self.intern_normalized_union_or_intersection(&dedup, true)
            }
        }
    }

    pub(super) fn relation_abort_inline_flight(&self, flight: Option<&InlineRelationFlight>) {
        if let Some(flight) = flight {
            self.graph().abort_inline_relation_flight(flight);
        }
    }

    pub(super) fn relation_abort_discharged_flights(&self, discharged: &[DischargedMember]) {
        for (_, _, _, _, _, _, flight) in discharged {
            self.relation_abort_inline_flight(flight.as_ref());
        }
    }

    pub(super) fn flow_return_abort_inline_flight(
        &self,
        flight: Option<&crate::semantic_query_memo::InlineFlowReturnFlight>,
    ) {
        if let Some(flight) = flight {
            self.graph().abort_inline_flow_return_flight(flight);
        }
    }

    pub(super) fn flow_return_abort_drained_flights(&self, members: &[DrainedFlowReturnMember]) {
        for member in members {
            self.flow_return_abort_inline_flight(member.inline_flight.as_ref());
        }
    }

    /// Hand one closed component's deferred members to the store's
    /// batched SCC publish, fenced on the root's published candidate.
    ///
    /// THE single drain shape both roots use: a member without a claimed
    /// flight was never deferred and has nothing to publish; every other
    /// member rides the root's SCC-union carrier and the root witness, so
    /// a superseded root releases the whole component with zero member
    /// publication.
    pub(super) fn publish_scc_member_batch(
        &self,
        required_root: crate::semantic_query_memo::SccRootWitness,
        carrier: &crate::semantic_query_memo::PublishedMemoCandidate,
        relation_members: Vec<CompletedSccMember>,
        flow_members: Vec<super::dispatch_txn::CompletedFlowReturnMember>,
        call_members: Vec<super::dispatch_txn::CompletedResolveCallMember>,
    ) {
        let relation_members: Vec<_> = relation_members
            .into_iter()
            .filter_map(|member| {
                member.inline_flight.map(|flight| {
                    crate::semantic_query_memo::PendingRelationMember {
                        key: member.key,
                        payload: member.payload,
                        flight,
                    }
                })
            })
            .collect();
        let flow_members: Vec<_> = flow_members
            .into_iter()
            .filter_map(|member| {
                member.inline_flight.map(|flight| {
                    crate::semantic_query_memo::PendingFlowReturnMember {
                        key: member.key,
                        result: member.result,
                        materialized: member.materialized,
                        flight,
                    }
                })
            })
            .collect();
        let call_members: Vec<_> = call_members
            .into_iter()
            .filter_map(|member| {
                member.inline_flight.map(|flight| {
                    crate::semantic_query_memo::PendingResolveCallMember {
                        key: member.key,
                        result: member.result,
                        flight,
                    }
                })
            })
            .collect();
        self.graph().publish_scc_members_fenced(
            Some(self.ctx),
            &required_root,
            &carrier.read_set_signature,
            &carrier.self_root_canonicals,
            carrier.validated_at_generation,
            relation_members,
            flow_members,
            call_members,
        );
    }

    /// Discharge a mixed flow/call component to ONE joint fixed point.
    ///
    /// The flow side owns the callee-clause transfer, empty-cycle
    /// resurrection, and freshness widening; the call side owns
    /// applicability replay and the return equation. Neither closes
    /// first: a flow member can hold an in-flight call (its result joins
    /// raw, already in the caller's terms) exactly as a call can hold a
    /// body-derived callee return (read from the just-discharged
    /// override map, never the store), so the two iterate against each
    /// other's current values — both monotone joins on the same leaf
    /// lattice — until the call results stop moving, at which point the
    /// flow side has already discharged against that same map and is
    /// final too. The pass bound is one per member plus one: both sides
    /// reach their join in strictly fewer passes, so exhausting the
    /// bound means the component cannot be trusted to be at its fixed
    /// point and fails closed.
    ///
    /// `prefix_entries` carries any extra flow entries ahead of the
    /// drained members (the flow-root close's own root); their outcomes
    /// come back in order. The drained members' outcomes update in
    /// place.
    pub(super) fn discharge_mixed_component_to_fixed_point(
        &self,
        prefix_entries: Vec<super::dispatch_txn::FlowDischargeEntry>,
        flow_members: &mut [DrainedFlowReturnMember],
        call_members: &mut [(
            crate::semantic_query::ResolveCallKey,
            ResolveCallPendingState,
        )],
        replay_substitution: &ProvisionalSubstitution,
    ) -> MixedDischargeResult {
        let mut call_result_map: rustc_hash::FxHashMap<
            crate::semantic_query::ResolveCallKey,
            SemanticNodeId,
        > = rustc_hash::FxHashMap::default();
        let mut prefix_outcomes: Vec<FlowReturnPendingOutcome> = prefix_entries
            .iter()
            .map(|entry| entry.outcome.clone())
            .collect();
        let bound = prefix_entries.len() + flow_members.len() + call_members.len() + 1;
        for _pass in 0..bound {
            if !prefix_entries.is_empty() || !flow_members.is_empty() {
                let mut entries = prefix_entries.clone();
                for (entry, outcome) in entries.iter_mut().zip(prefix_outcomes.iter()) {
                    entry.outcome = outcome.clone();
                }
                for member in flow_members.iter() {
                    entries.push(super::dispatch_txn::FlowDischargeEntry {
                        key: member.key.clone(),
                        outcome: member.outcome.clone(),
                        holds: member.holds.clone(),
                        fresh_seed: member.fresh_seed,
                    });
                }
                self.discharge_flow_component_to_fixed_point(&mut entries, &call_result_map);
                let split = entries.len() - flow_members.len();
                prefix_outcomes = entries[..split]
                    .iter()
                    .map(|entry| entry.outcome.clone())
                    .collect();
                for (member, entry) in flow_members.iter_mut().zip(entries[split..].iter()) {
                    member.outcome = entry.outcome.clone();
                }
            }
            if call_members.is_empty() {
                return Ok((prefix_outcomes, Vec::new()));
            }
            // The overrides the call equation reads its flow hold targets
            // from: the JUST-discharged drained members AND any prefix
            // entries (a flow root is a hold target like any other) —
            // final at this pass but not yet published, so never the
            // store.
            let mut flow_overrides: rustc_hash::FxHashMap<
                crate::semantic_query::FlowReturnKey,
                SemanticNodeId,
            > = flow_members
                .iter()
                .filter_map(|member| match &member.outcome {
                    FlowReturnPendingOutcome::Complete(result) => {
                        Some((member.key.clone(), result.return_type()))
                    }
                    FlowReturnPendingOutcome::NoValue { .. } => None,
                })
                .collect();
            for (entry, outcome) in prefix_entries.iter().zip(prefix_outcomes.iter()) {
                if let FlowReturnPendingOutcome::Complete(result) = outcome {
                    flow_overrides.insert(entry.key.clone(), result.return_type());
                }
            }
            let new_results = self.solve_drained_call_members(
                call_members,
                &flow_overrides,
                replay_substitution,
            )?;
            let new_map: rustc_hash::FxHashMap<
                crate::semantic_query::ResolveCallKey,
                SemanticNodeId,
            > = new_results
                .iter()
                .map(|(key, _, result)| {
                    (
                        key.clone(),
                        super::return_equation::resolved_call_return_type(result),
                    )
                })
                .collect();
            if new_map == call_result_map {
                return Ok((prefix_outcomes, new_results));
            }
            call_result_map = new_map;
        }
        Err(crate::semantic_query::ResolveCallFailure::Undecidable)
    }

    /// Replay + solve the drained call members of one closing component.
    ///
    /// Relation-only applicability assumptions replay against the
    /// caller's converged provisional table; the survivors solve their
    /// return equation with the JUST-discharged in-component flow results
    /// as overrides — those targets are final at the close but not yet
    /// published, so they must never be read from the store.
    pub(super) fn solve_drained_call_members(
        &self,
        call_members: &mut [(
            crate::semantic_query::ResolveCallKey,
            ResolveCallPendingState,
        )],
        flow_overrides: &rustc_hash::FxHashMap<
            crate::semantic_query::FlowReturnKey,
            SemanticNodeId,
        >,
        replay_substitution: &ProvisionalSubstitution,
    ) -> Result<Vec<DrainedCallResult>, crate::semantic_query::ResolveCallFailure> {
        if call_members.is_empty() {
            return Ok(Vec::new());
        }
        for (key, state) in call_members.iter_mut() {
            if !state.replay_applicability {
                continue;
            }
            *state = self.replay_resolve_call_pending(key, state, replay_substitution)?;
        }
        let equation: Vec<super::dispatch_txn::ReturnEquationMember> = call_members
            .iter()
            .map(|(key, state)| super::dispatch_txn::ReturnEquationMember {
                fresh_literal_returns: state.selection.fresh_literal_returns().to_vec(),
                identity: super::dispatch_txn::ReturnObligationIdentity::ResolveCall(key.clone()),
                concrete_seeds: state.concrete_seeds.clone(),
                holds: state.holds.clone(),
                domain: super::dispatch_txn::ReturnDomainMetadata::ResolveCall,
            })
            .collect();
        let solved = self
            .solve_return_equation(&equation, flow_overrides)
            .map_err(|_| crate::semantic_query::ResolveCallFailure::Undecidable)?;
        Ok(call_members
            .iter()
            .zip(solved.iter().copied())
            .map(|((key, state), return_type)| {
                (
                    key.clone(),
                    state.clone(),
                    state.selection.with_return_type(self, return_type),
                )
            })
            .collect())
    }

    pub(super) fn resolve_call_abort_drained_flights(
        &self,
        members: &[(
            crate::semantic_query::ResolveCallKey,
            ResolveCallPendingState,
            crate::semantic_query::ResolvedCallResult,
        )],
    ) {
        for (_, state, _) in members {
            self.resolve_call_abort_inline_flight(state.inline_flight.as_ref());
            if let Some(session) = state.staged_session {
                self.abandon_session(session);
            }
        }
    }

    pub(super) fn relation_abort_completed_members(&self) {
        let (members, flow_members, call_members) = {
            let mut txn = self.dispatch_txn.borrow_mut();
            (
                std::mem::take(&mut txn.relation.completed_members),
                std::mem::take(&mut txn.flow.completed_members),
                std::mem::take(&mut txn.call.completed_members),
            )
        };
        for member in &members {
            self.relation_abort_inline_flight(member.inline_flight.as_ref());
        }
        for member in &flow_members {
            self.flow_return_abort_inline_flight(member.inline_flight.as_ref());
        }
        for member in &call_members {
            self.resolve_call_abort_inline_flight(member.inline_flight.as_ref());
        }
    }

    fn relation_publication_roots(
        &self,
        root_key: &RelateMemoKey,
        member_keys: impl IntoIterator<Item = RelateMemoKey>,
    ) -> Vec<crate::semantic_query_memo::ObservedGraphSelfRoot> {
        let mut nodes = vec![root_key.source, root_key.target];
        for member in member_keys {
            nodes.push(member.source);
            nodes.push(member.target);
        }
        self.observed_self_roots_from_nodes(nodes)
    }

    fn relation_completed_publication_roots(
        &self,
        root_key: &RelateMemoKey,
    ) -> Vec<crate::semantic_query_memo::ObservedGraphSelfRoot> {
        let (member_keys, flow_self_roots, call_self_roots) = {
            let txn = self.dispatch_txn.borrow();
            (
                txn.relation
                    .completed_members
                    .iter()
                    .map(|member| member.key.clone())
                    .collect::<Vec<_>>(),
                txn.flow
                    .completed_members
                    .iter()
                    .flat_map(|member| member.self_roots.iter().cloned())
                    .collect::<Vec<_>>(),
                txn.call
                    .completed_members
                    .iter()
                    .flat_map(|member| member.self_roots.iter().cloned())
                    .collect::<Vec<_>>(),
            )
        };
        // The published component's self-roots are the UNION of every
        // drained member's roots across BOTH domains: a flow member's file
        // roots ride the relation-rooted carrier, so a cross-file edit
        // invalidates the whole component.
        let mut roots = self.relation_publication_roots(root_key, member_keys);
        for root in flow_self_roots {
            if !roots.iter().any(|(canonical, _)| canonical == &root.0) {
                roots.push(root);
            }
        }
        for root in call_self_roots {
            if !roots.iter().any(|(canonical, _)| canonical == &root.0) {
                roots.push(root);
            }
        }
        roots
    }

    #[cfg(test)]
    pub(super) fn scc_publication_roots_for_tests(
        &self,
        root_key: &RelateMemoKey,
        member_keys: &[RelateMemoKey],
    ) -> Vec<crate::semantic_query_memo::ObservedGraphSelfRoot> {
        self.relation_publication_roots(root_key, member_keys.iter().cloned())
    }

    #[cfg(test)]
    pub(super) fn publish_staged_scc_member_for_tests(
        &self,
        root_key: RelateMemoKey,
        member_key: RelateMemoKey,
    ) -> RelationStep {
        let inline_flight = self
            .graph()
            .begin_inline_relation_flight(&member_key)
            .expect("the staged member must claim its relation flight");
        let payload = self.relation_payload(
            RelationOutcome::Assignable,
            Arc::from(Vec::<InferBinding>::new().into_boxed_slice()),
            RelationProof::Assignable {
                witness: crate::semantic_query::DerivationTree {
                    sub_derivations: Arc::from(Vec::new().into_boxed_slice()),
                },
            },
        );
        self.dispatch_txn
            .borrow_mut()
            .relation
            .completed_members
            .push(CompletedSccMember {
                key: member_key,
                payload,
                inline_flight: Some(inline_flight),
            });
        self.execute_relate_root(root_key)
    }

    /// Drain the SCC-closed member batch onto the root's published
    /// SCC-union carrier (design §2.3: the published fact set is the UNION
    /// of all SCC members' observed facts, never the bare per-member set).
    ///
    /// The root's admitted publish is the component's COMMIT BOUNDARY.
    /// Every member here is independently fenced backfill: it revalidates
    /// at its own publish, and one the fence refuses stays cold and
    /// recomputes on demand rather than weakening the committed root.
    fn relation_drain_completed_members(
        &self,
        root_key: &RelateMemoKey,
        carrier: &crate::semantic_query_memo::PublishedMemoCandidate,
    ) {
        let (members, flow_members, call_members) = {
            let mut txn = self.dispatch_txn.borrow_mut();
            (
                std::mem::take(&mut txn.relation.completed_members),
                std::mem::take(&mut txn.flow.completed_members),
                std::mem::take(&mut txn.call.completed_members),
            )
        };
        self.publish_scc_member_batch(
            crate::semantic_query_memo::SccRootWitness::relate(
                root_key.clone(),
                carrier.admission_seq,
            ),
            carrier,
            members,
            flow_members,
            call_members,
        );
    }

    // ──────────────────────────────────────────────────────────────────
    // Inference pattern detection + session plumbing
    // ──────────────────────────────────────────────────────────────────

    /// Upgrade a plain relation key with the target pattern's immutable
    /// session-setup fingerprint. Session opening consumes the same setup
    /// value, so there is no second projection to drift.
    pub(super) fn relation_key_with_inference(&self, mut key: RelateMemoKey) -> RelateMemoKey {
        if key.relation != RelationKind::Assignable
            || self.dispatch_txn.borrow().binding_is_disabled()
        {
            return key;
        }
        // Only upgrade when a binding could actually occur: the pattern
        // scan is cached per target node on the transaction.
        let Some(pattern) = self.relation_pattern_info(key.target) else {
            return key;
        };
        // Canonicalize even a caller-supplied context: behavior and memo
        // identity are one projection of the same frozen setup.
        key.inference_context = Some(pattern.setup.context_key().clone());
        key
    }

    /// Raw family dispatch accepts only the target pattern's exact frozen
    /// inference context. A target without an inferable pattern accepts no
    /// caller-supplied context. Reverse-projection sub-relations do not enter
    /// through raw dispatch; they retain the active session context through
    /// [`Self::relation_sub_key`].
    pub(super) fn relation_raw_key_has_exact_inference_context(&self, key: &RelateMemoKey) -> bool {
        if key.relation != RelationKind::Assignable {
            return key.inference_context.is_none();
        }
        let expected = self
            .relation_pattern_info(key.target)
            .map(|pattern| pattern.setup.context_key().clone());
        key.inference_context == expected
    }

    /// Detect an in-scope conditional-`infer` pattern on `target`. Direct
    /// infer occupants are supported in bare, object, tuple, array, and
    /// function positions. An exact unremapped homomorphic mapped target
    /// enables reverse projection; all other deeper nesting stays deferred.
    /// Results are cached per target node on the transaction.
    pub(super) fn relation_pattern_info(&self, target: SemanticNodeId) -> Option<InferPatternInfo> {
        if let Some(cached) = self
            .dispatch_txn
            .borrow()
            .relation
            .pattern_cache
            .get(&target)
        {
            return cached.clone();
        }
        let computed = self.relation_pattern_info_uncached(target);
        self.dispatch_txn
            .borrow_mut()
            .relation
            .pattern_cache
            .insert(target, computed.clone());
        computed
    }

    fn relation_pattern_info_uncached(&self, target: SemanticNodeId) -> Option<InferPatternInfo> {
        let graph = self.graph();
        if let Some(spec) = self.reverse_homomorphic_spec(target) {
            let name = match graph.node_data(spec.base_infer).as_deref() {
                Some(SemanticNodeData::Infer { name, .. }) => Arc::clone(name),
                _ => return None,
            };
            return Some(InferPatternInfo::new(
                InferPatternShape::ReverseHomomorphicMapped,
                vec![InferParamSite {
                    node: spec.base_infer,
                    name,
                    priority: InferenceCandidatePriority::HomomorphicMapped,
                }],
                Some(spec),
            ));
        }
        match graph.node_data(target).as_deref() {
            Some(SemanticNodeData::Infer { name, .. }) => Some(InferPatternInfo::new(
                InferPatternShape::Bare,
                vec![InferParamSite {
                    node: target,
                    name: Arc::clone(name),
                    priority: InferenceCandidatePriority::NakedTypeParameter,
                }],
                None,
            )),
            Some(SemanticNodeData::Object(view)) => {
                let mut sites = Vec::new();
                for member in view.positive_members().iter() {
                    if let Some(SemanticNodeData::Infer { name, .. }) =
                        graph.node_data(member.value).as_deref()
                    {
                        sites.push(InferParamSite {
                            node: member.value,
                            name: Arc::clone(name),
                            priority: InferenceCandidatePriority::Argument,
                        });
                    }
                }
                (!sites.is_empty())
                    .then(|| InferPatternInfo::new(InferPatternShape::ObjectProps, sites, None))
            }
            Some(SemanticNodeData::Tuple { elements, .. }) => {
                let mut sites = Vec::new();
                for element in elements.iter() {
                    if let Some(SemanticNodeData::Infer { name, .. }) =
                        graph.node_data(element.value).as_deref()
                    {
                        sites.push(InferParamSite {
                            node: element.value,
                            name: Arc::clone(name),
                            priority: InferenceCandidatePriority::Argument,
                        });
                    }
                }
                (!sites.is_empty())
                    .then(|| InferPatternInfo::new(InferPatternShape::TupleHeadTail, sites, None))
            }
            Some(SemanticNodeData::Array { element, .. }) => {
                if let Some(SemanticNodeData::Infer { name, .. }) =
                    graph.node_data(*element).as_deref()
                {
                    Some(InferPatternInfo::new(
                        InferPatternShape::ArrayElement,
                        vec![InferParamSite {
                            node: *element,
                            name: Arc::clone(name),
                            priority: InferenceCandidatePriority::Argument,
                        }],
                        None,
                    ))
                } else {
                    None
                }
            }
            Some(SemanticNodeData::Signature {
                params,
                return_type,
                ..
            }) => {
                let mut sites = Vec::new();
                for param in params.iter() {
                    if let Some(SemanticNodeData::Infer { name, .. }) =
                        graph.node_data(param.ty).as_deref()
                    {
                        sites.push(InferParamSite {
                            node: param.ty,
                            name: Arc::clone(name),
                            priority: InferenceCandidatePriority::Argument,
                        });
                    } else {
                        // A function parameter may itself be a variadic
                        // tuple/array inference pattern (`...args:
                        // [...infer R]`). The function relation flips the
                        // occurrence to contravariant; the nested
                        // container reducer deposits into these exact
                        // sites.
                        match graph.node_data(param.ty).as_deref() {
                            Some(SemanticNodeData::Tuple { elements, .. }) => {
                                for element in elements.iter() {
                                    if let Some(SemanticNodeData::Infer { name, .. }) =
                                        graph.node_data(element.value).as_deref()
                                    {
                                        sites.push(InferParamSite {
                                            node: element.value,
                                            name: Arc::clone(name),
                                            priority: InferenceCandidatePriority::Argument,
                                        });
                                    }
                                }
                            }
                            Some(SemanticNodeData::Array { element, .. }) => {
                                if let Some(SemanticNodeData::Infer { name, .. }) =
                                    graph.node_data(*element).as_deref()
                                {
                                    sites.push(InferParamSite {
                                        node: *element,
                                        name: Arc::clone(name),
                                        priority: InferenceCandidatePriority::Argument,
                                    });
                                }
                            }
                            _ => {}
                        }
                    }
                }
                if let Some(SemanticNodeData::Infer { name, .. }) =
                    graph.node_data(*return_type).as_deref()
                {
                    sites.push(InferParamSite {
                        node: *return_type,
                        name: Arc::clone(name),
                        priority: InferenceCandidatePriority::ReturnType,
                    });
                }
                (!sites.is_empty())
                    .then(|| InferPatternInfo::new(InferPatternShape::Function, sites, None))
            }
            _ => None,
        }
    }

    /// Recognize only the exact `{ [P in keyof infer T]: X }` descriptor.
    fn reverse_homomorphic_spec(
        &self,
        mapped_node: SemanticNodeId,
    ) -> Option<ReverseHomomorphicSpec> {
        let graph = self.graph();
        let mapped = graph.node_data(mapped_node)?;
        let (source, mapper) = match mapped.as_ref() {
            SemanticNodeData::Mapped { source, mapper } if mapper.name_remap.is_none() => {
                (*source, mapper.clone())
            }
            _ => return None,
        };
        drop(mapped);

        let source = self.peel_relation_alias(source)?;
        let key_space = self.peel_relation_alias(mapper.key_space)?;
        let key_base = match graph.node_data(key_space).as_deref() {
            Some(SemanticNodeData::KeyOf { base }) => self.peel_relation_alias(*base)?,
            _ => return None,
        };
        if source != key_base
            || !matches!(
                graph.node_data(key_base).as_deref(),
                Some(SemanticNodeData::Infer { .. })
            )
            || !matches!(
                graph.node_data(mapper.parameter_node).as_deref(),
                Some(SemanticNodeData::TypeParam { .. })
            )
        {
            return None;
        }
        Some(ReverseHomomorphicSpec {
            mapped_node,
            base_infer: key_base,
            mapper_parameter: mapper.parameter_node,
            template: mapper.value_expr,
            modifiers: ReverseMappedModifiers {
                optionality: mapper.optionality,
                readonly: mapper.readonly,
            },
        })
    }

    fn peel_relation_alias(&self, node: SemanticNodeId) -> Option<SemanticNodeId> {
        let mut current = node;
        let mut seen = FxHashSet::default();
        while seen.insert(current) {
            match self.graph().node_data(current).as_deref() {
                Some(SemanticNodeData::Alias(inner)) => current = *inner,
                Some(_) => return Some(current),
                None => return None,
            }
        }
        None
    }

    /// The transient inference occurrence of the current reducer. A popped
    /// SCC member re-discharges through a virtual root occurrence until it
    /// opens a nested real frame; ordinary structural frames read their
    /// occurrence from the nearest open RELATE ancestor on the shared
    /// reentry stack.
    fn relation_current_occurrence(&self) -> InferenceOccurrence {
        let txn = self.dispatch_txn.borrow();
        if let Some((virtual_depth, occurrence)) = txn.relation.redischarge_occurrence {
            if txn.reentry().depth() <= virtual_depth {
                return occurrence;
            }
        }
        txn.reentry()
            .nearest_relate()
            .map(|(_, occurrence)| occurrence)
            .unwrap_or(InferenceOccurrence::ARGUMENT_COVARIANT)
    }

    fn relation_occurrence(&self, position: InferPosition) -> InferenceOccurrence {
        inference_occurrence_for_position(self.relation_current_occurrence(), position)
    }

    /// Deposit an inference candidate into the active session (a
    /// session-local delta — the deposit itself is ReturnOnly, never
    /// published). The current top frame records the delta flag ONLY when
    /// the session belongs to an OUTER frame (admission row 7); the
    /// binding root's own deposits into its OWN session do not suppress
    /// its publish (its payload carries the session's fixed bindings).
    fn relation_deposit(
        &self,
        param_node: SemanticNodeId,
        mut bound: SemanticNodeId,
        occurrence: InferenceOccurrence,
    ) -> bool {
        let (call_policy, deposit_is_top_level) = {
            let txn = self.dispatch_txn.borrow();
            (
                txn.active_session()
                    .and_then(|session| session.call_const_policy(param_node))
                    .zip(txn.call_argument_literal_mode()),
                txn.call_argument_target_is_top_level(param_node),
            )
        };
        if let Some((policy, literal_mode)) = call_policy {
            // A bare literal argument's candidate widens under the
            // inferring parameter's own const policy; an argument whose
            // authored form already pins its type deposits as authored.
            // A NAKED top-level inference position preserves a primitive
            // literal (the constraint is an upper-bound check, not a
            // widening target: `cstr<T extends string>("a")` is `"a"`);
            // nested positions — an array element, an object member —
            // widen as before.
            if literal_mode == crate::semantic_query::ArgumentLiteralMode::Widened {
                let preserve_top_literal = deposit_is_top_level
                    && policy == crate::semantic_query::ConstParamPolicy::NonConst
                    && matches!(
                        self.graph().node_data(bound).as_deref(),
                        Some(SemanticNodeData::Literal(_))
                    );
                if !preserve_top_literal {
                    bound = self.call_inference_candidate(bound, policy);
                } else {
                    // A preserved bare literal at a naked position is FRESH
                    // provenance for an unconstrained parameter (the note
                    // is a no-op for a constrained one, whose preserved
                    // literal is regular).
                    if let Some(session) = self.dispatch_txn.borrow_mut().active_session_mut() {
                        session.note_fresh_literal_deposit(param_node, bound);
                    }
                }
            }
        }
        let mut txn = self.dispatch_txn.borrow_mut();
        let active_id = txn.active_session().map(|session| session.id);
        let accepted = txn.active_session_mut().is_some_and(|session| {
            session.deposit(param_node, bound, occurrence.priority, occurrence.variance)
        });
        if !accepted {
            return false;
        }
        txn.relation.accepted_inference_deposits += 1;
        txn.note_candidate_write(active_id);
        true
    }

    fn relation_projection_target(&self, node: SemanticNodeId) -> bool {
        self.dispatch_txn
            .borrow()
            .active_session()
            .is_some_and(|session| session.is_projection_target(node))
    }

    /// Whether the active inference session declares `node` as one of its
    /// frozen inference parameters (a deposit target).
    pub(super) fn relation_session_declares(&self, node: SemanticNodeId) -> bool {
        self.dispatch_txn
            .borrow()
            .active_session()
            .is_some_and(|session| session.declares(node))
    }

    /// Deposit the assembled reverse candidate through the same frame/session
    /// ownership gate as ordinary and projection candidates. A nested frame
    /// mutating an outer session is a session-local delta and therefore cannot
    /// publish an otherwise context-free relation payload.
    fn relation_reverse_aggregate_deposit(
        &self,
        param_node: SemanticNodeId,
        candidate: SemanticNodeId,
        priority: InferenceCandidatePriority,
    ) -> bool {
        let mut txn = self.dispatch_txn.borrow_mut();
        let active_id = txn.active_session().map(|session| session.id);
        let accepted = txn.active_session_mut().is_some_and(|session| {
            session.deposit_reverse_aggregate(param_node, candidate, priority)
        });
        if !accepted {
            return false;
        }
        txn.relation.accepted_inference_deposits += 1;
        txn.note_candidate_write(active_id);
        true
    }

    /// Deposit into a registered reverse projection. The indexed access is
    /// only a projection target; it never becomes an `Infer` declaration.
    fn relation_projection_deposit(
        &self,
        projection: SemanticNodeId,
        bound: SemanticNodeId,
        occurrence: InferenceOccurrence,
    ) -> bool {
        let bound = match self.unwrap_identity_carrier_for_relation(bound) {
            IdentityCarrierUnwrap::Concrete(bound) => bound,
            IdentityCarrierUnwrap::Unresolvable => return false,
        };
        if self.relation_subtree_contains_semantically_unresolved(bound)
            || super::raise::node_is_unknown_materializing_failure(self, bound)
            || super::raise::node_contains_semantic_miss_with_dispatch(self, bound) != Some(false)
        {
            return false;
        }
        let Some(bound_data) = self.graph().node_data(bound) else {
            return false;
        };
        if is_deferred(&bound_data)
            || matches!(
                bound_data.as_ref(),
                SemanticNodeData::TypeParam { .. }
                    | SemanticNodeData::Infer { .. }
                    | SemanticNodeData::InferRef { .. }
            )
        {
            return false;
        }
        drop(bound_data);
        let mut txn = self.dispatch_txn.borrow_mut();
        let active_id = txn.active_session().map(|session| session.id);
        let deposited = txn.active_session_mut().is_some_and(|session| {
            session.deposit_projection(projection, bound, occurrence.priority, occurrence.variance)
        });
        if !deposited {
            return false;
        }
        txn.relation.accepted_inference_deposits += 1;
        txn.note_candidate_write(active_id);
        true
    }

    fn relation_subtree_contains_semantically_unresolved(&self, root: SemanticNodeId) -> bool {
        self.relation_subtree_matches(root, |_, data| data.means_type_is_not_yet_known())
    }

    fn relation_reverse_input_is_semantically_resolved(&self, root: SemanticNodeId) -> bool {
        !self.relation_subtree_contains_semantically_unresolved(root)
            && !super::raise::node_is_unknown_materializing_failure(self, root)
            && super::raise::node_contains_semantic_miss_with_dispatch(self, root) == Some(false)
    }

    /// Enforces the assembled-input preflight documented in `/type-resolution`
    /// under "Reverse-homomorphic mapped recovery".
    fn relation_reverse_source_inputs_are_semantically_resolved(
        &self,
        source: SemanticNodeId,
    ) -> bool {
        let Some(data) = self.graph().node_data(source) else {
            return false;
        };
        match data.as_ref() {
            SemanticNodeData::Object(surface) => {
                surface.positive_members().iter().all(|member| {
                    self.relation_reverse_input_is_semantically_resolved(member.value)
                }) && surface.index_signatures.iter().all(|signature| {
                    self.relation_reverse_input_is_semantically_resolved(signature.key_type)
                        && self
                            .relation_reverse_input_is_semantically_resolved(signature.value_type)
                })
            }
            SemanticNodeData::Array { element, .. } => {
                self.relation_reverse_input_is_semantically_resolved(*element)
            }
            SemanticNodeData::Tuple { elements, .. } => elements
                .iter()
                .all(|element| self.relation_reverse_input_is_semantically_resolved(element.value)),
            _ => false,
        }
    }

    fn relation_subtree_contains_projection(&self, root: SemanticNodeId) -> bool {
        self.relation_subtree_matches(root, |node, _| self.relation_projection_target(node))
    }

    fn relation_subtree_matches(
        &self,
        root: SemanticNodeId,
        mut matches: impl FnMut(SemanticNodeId, &SemanticNodeData) -> bool,
    ) -> bool {
        let graph = self.graph();
        let mut visited = FxHashSet::default();
        let mut stack = vec![root];
        while let Some(node) = stack.pop() {
            if !visited.insert(node) {
                continue;
            }
            let Some(data) = graph.node_data(node) else {
                continue;
            };
            if matches(node, data.as_ref()) {
                return true;
            }
            match data.as_ref() {
                SemanticNodeData::Alias(inner) => stack.push(*inner),
                SemanticNodeData::Union(members) | SemanticNodeData::Intersection(members) => {
                    stack.extend(members.iter().copied());
                }
                SemanticNodeData::Array { element, .. } => stack.push(*element),
                SemanticNodeData::Tuple { elements, .. } => {
                    stack.extend(elements.iter().map(|element| element.value));
                }
                SemanticNodeData::Object(surface) => {
                    stack.extend(surface.positive_members().iter().map(|member| member.value));
                    stack.extend(surface.call_signatures.iter().copied());
                    stack.extend(surface.construct_signatures.iter().copied());
                    for signature in surface.index_signatures.iter() {
                        stack.push(signature.key_type);
                        stack.push(signature.value_type);
                    }
                    if let Some(keyspace) = surface.keyspace {
                        stack.push(keyspace);
                    }
                }
                SemanticNodeData::ObjectSpreadProgram(program) => {
                    stack.extend(program.child_nodes());
                }
                SemanticNodeData::Signature {
                    params,
                    return_type,
                    type_parameters,
                    ..
                } => {
                    stack.extend(params.iter().map(|parameter| parameter.ty));
                    stack.push(*return_type);
                    for parameter in type_parameters.iter() {
                        stack.extend(parameter.constraint);
                        stack.extend(parameter.default);
                    }
                }
                SemanticNodeData::TemplateLiteral { expressions, .. } => {
                    stack.extend(expressions.iter().copied());
                }
                SemanticNodeData::KeyOf { base } => stack.push(*base),
                SemanticNodeData::IndexedAccess { object, index } => {
                    stack.push(*object);
                    if let IndexKey::Computed(index) = index {
                        stack.push(*index);
                    }
                }
                SemanticNodeData::Mapped { source, mapper } => {
                    stack.push(*source);
                    stack.push(mapper.key_space);
                    stack.push(mapper.value_expr);
                    stack.extend(mapper.name_remap);
                }
                SemanticNodeData::Conditional {
                    check,
                    extends,
                    true_branch_ref,
                    false_branch_ref,
                    ..
                } => {
                    stack.extend([*check, *extends, *true_branch_ref, *false_branch_ref]);
                }
                SemanticNodeData::InstantiationRef { args, .. } => {
                    stack.extend(args.iter().copied());
                }
                SemanticNodeData::MergedDecl { contributors } => {
                    stack.extend(contributors.iter().copied());
                }
                SemanticNodeData::SyntheticBinding { value_node, .. } => {
                    stack.push(SemanticNodeId(*value_node));
                }
                SemanticNodeData::TypeParam {
                    constraint,
                    default,
                    ..
                } => {
                    stack.extend(constraint.iter().copied());
                    stack.extend(default.iter().copied());
                }
                SemanticNodeData::TypeOf(_)
                | SemanticNodeData::BareRef(_)
                | SemanticNodeData::ImportType(_) => {
                    stack.extend(data.carrier_type_args().iter().copied());
                }
                SemanticNodeData::Primitive(_)
                | SemanticNodeData::Literal(_)
                | SemanticNodeData::Opaque(_)
                | SemanticNodeData::Infer { .. }
                | SemanticNodeData::InferRef { .. }
                | SemanticNodeData::DeclRef { .. }
                // The sealed callable carrier never carries an `infer`
                // placeholder.
                | SemanticNodeData::DeferredCallable(_)
                | SemanticNodeData::RawFallback { .. } => {}
            }
        }
        false
    }

    fn try_relation_projection(
        &self,
        source: SemanticNodeId,
        target: SemanticNodeId,
        bindings: &mut [InferBinding],
        occurrence: InferenceOccurrence,
    ) -> Option<RelationResult> {
        let projection = match occurrence.variance {
            VariancePhase::Covariant => self
                .relation_projection_target(target)
                .then_some((target, source)),
            VariancePhase::Contravariant => self
                .relation_projection_target(source)
                .then_some((source, target)),
            VariancePhase::Invariant => {
                if self.relation_projection_target(target) {
                    Some((target, source))
                } else {
                    self.relation_projection_target(source)
                        .then_some((source, target))
                }
            }
        };
        let projection = projection?;
        Some(
            if self.relation_projection_deposit(projection.0, projection.1, occurrence) {
                assignable(bindings)
            } else {
                RelationResult::Unknown
            },
        )
    }

    /// Whether an inference session is currently active.
    fn relation_session_active(&self) -> bool {
        self.dispatch_txn.borrow().active_session().is_some()
    }

    /// Checkpoint the ACTIVE inference session's deposits (`None` when no
    /// session is active). The alternative-scoping half of the
    /// losing-alternative rule: a first-match loop over overload /
    /// signature-group alternatives brackets each alternative with a
    /// checkpoint and rolls back on failure, so a LOSING alternative's
    /// deposits never reach fixation (`{ (a: number, b: number): void;
    /// (a: string, b: string): void } extends (a: infer U, b: string) =>
    /// void` fixes `U := string`, never `number ∧ string`).
    fn relation_session_checkpoint(&self) -> Option<SessionCheckpoint> {
        self.dispatch_txn
            .borrow()
            .active_session()
            .map(InferenceSession::checkpoint)
    }

    /// Roll the ACTIVE session's deposits back to `checkpoint` (no-op when
    /// no session is active or no checkpoint was taken).
    fn relation_session_rollback(&self, checkpoint: &Option<SessionCheckpoint>) {
        if let Some(checkpoint) = checkpoint {
            if let Some(session) = self.dispatch_txn.borrow_mut().active_session_mut() {
                session.rollback_to(checkpoint);
            }
        }
    }

    fn relate_pair_alternatives(
        &self,
        alternatives: &[(SemanticNodeId, SemanticNodeId)],
        bindings: &mut Vec<InferBinding>,
        position: InferPosition,
    ) -> RelationResult {
        self.relate_pair_alternatives_with_freshness(alternatives, bindings, position, false)
    }

    fn relate_union_target_alternatives(
        &self,
        alternatives: &[(SemanticNodeId, SemanticNodeId)],
        bindings: &mut Vec<InferBinding>,
        position: InferPosition,
    ) -> RelationResult {
        self.relate_pair_alternatives_with_freshness(alternatives, bindings, position, true)
    }

    fn relate_pair_alternatives_with_freshness(
        &self,
        alternatives: &[(SemanticNodeId, SemanticNodeId)],
        bindings: &mut Vec<InferBinding>,
        position: InferPosition,
        excess_prepass_completed: bool,
    ) -> RelationResult {
        let mut any_unknown = false;
        for (source, target) in alternatives {
            let checkpoint = self.relation_session_checkpoint();
            let bindings_len = bindings.len();
            let result = if excess_prepass_completed {
                self.relate_union_arm_after_excess_prepass(*source, *target, bindings, position)
            } else {
                self.relate_member(*source, *target, bindings, position)
            };
            match result {
                result @ RelationResult::Assignable { .. } => return result,
                RelationResult::Unknown => {
                    self.relation_session_rollback(&checkpoint);
                    bindings.truncate(bindings_len);
                    any_unknown = true;
                }
                RelationResult::NotAssignable => {
                    self.relation_session_rollback(&checkpoint);
                    bindings.truncate(bindings_len);
                }
            }
        }
        if any_unknown {
            RelationResult::Unknown
        } else {
            RelationResult::NotAssignable
        }
    }

    fn relate_signature_alternatives(
        &self,
        source_signatures: &[SemanticNodeId],
        target_signature: SemanticNodeId,
        bindings: &mut Vec<InferBinding>,
    ) -> RelationResult {
        let alternatives: Vec<_> = source_signatures
            .iter()
            .map(|source| (*source, target_signature))
            .collect();
        self.relate_pair_alternatives(&alternatives, bindings, InferPosition::Covariant)
    }

    /// Recover the input of an exact homomorphic mapped target. The only
    /// externally visible output is the normal relation verdict; recovered
    /// values and the aggregate candidate stay in the active session.
    fn relate_reverse_homomorphic(
        &self,
        source: SemanticNodeId,
        spec: &ReverseHomomorphicSpec,
        bindings: &mut Vec<InferBinding>,
    ) -> RelationResult {
        let source = match self.unwrap_identity_carrier_for_relation(source) {
            IdentityCarrierUnwrap::Concrete(source) => source,
            IdentityCarrierUnwrap::Unresolvable => return RelationResult::Unknown,
        };
        if !self.relation_reverse_source_inputs_are_semantically_resolved(source) {
            return RelationResult::Unknown;
        }
        let overall_checkpoint = self.relation_session_checkpoint();
        let Some(checkpoint) = overall_checkpoint.as_ref() else {
            return RelationResult::Unknown;
        };
        let bindings_len = bindings.len();
        let graph = self.graph();
        let source_shape = match graph.node_data(source).as_deref() {
            Some(SemanticNodeData::Object(view)) => {
                if view.has_known_index_signature() && view.index_signatures.is_empty() {
                    self.relation_session_rollback(&overall_checkpoint);
                    return RelationResult::Unknown;
                }
                ReverseSourceShape::Object
            }
            Some(SemanticNodeData::Array { readonly, .. }) => ReverseSourceShape::Array {
                readonly: *readonly,
            },
            Some(SemanticNodeData::Tuple { readonly, .. }) => ReverseSourceShape::Tuple {
                readonly: *readonly,
            },
            _ => {
                self.relation_session_rollback(&overall_checkpoint);
                return RelationResult::Unknown;
            }
        };

        let relation = match graph.node_data(source).as_deref() {
            Some(SemanticNodeData::Object(view)) => {
                let members = view.positive_members().to_vec();
                let index_signatures = view.index_signatures.to_vec();
                let mut verdict = assignable(bindings);
                for member in members {
                    let Some(optional) =
                        reverse_optional(member.optional, spec.modifiers.optionality)
                    else {
                        verdict = RelationResult::NotAssignable;
                        break;
                    };
                    let Some(readonly) = reverse_readonly(member.readonly, spec.modifiers.readonly)
                    else {
                        verdict = RelationResult::NotAssignable;
                        break;
                    };
                    let Some(known_key) = member.key.cloned_known() else {
                        verdict = RelationResult::Unknown;
                        break;
                    };
                    let key = match known_key {
                        crate::semantic_query::PropertyKey::String(name) => graph.intern_node(
                            SemanticNodeData::Literal(LiteralValue::String(name.to_string())),
                        ),
                        crate::semantic_query::PropertyKey::Number(number) => graph.intern_node(
                            SemanticNodeData::Literal(LiteralValue::Number(number.get() as f64)),
                        ),
                        crate::semantic_query::PropertyKey::UniqueSymbol(_) => {
                            verdict = RelationResult::Unknown;
                            break;
                        }
                    };
                    verdict = self.recover_reverse_projection(
                        member.value,
                        key,
                        spec,
                        bindings,
                        move |value| {
                            let mut recovered = member;
                            recovered.value = value;
                            recovered.optional = optional;
                            recovered.readonly = readonly;
                            ReverseRecoveredEntry::ObjectMember { member: recovered }
                        },
                    );
                    if !matches!(verdict, RelationResult::Assignable { .. }) {
                        break;
                    }
                }
                if matches!(verdict, RelationResult::Assignable { .. }) {
                    for signature in index_signatures {
                        let Some(readonly) =
                            reverse_readonly(signature.readonly, spec.modifiers.readonly)
                        else {
                            verdict = RelationResult::NotAssignable;
                            break;
                        };
                        let key = signature.key_type;
                        verdict = self.recover_reverse_projection(
                            signature.value_type,
                            key,
                            spec,
                            bindings,
                            move |value| {
                                let mut recovered = signature;
                                recovered.value_type = value;
                                recovered.readonly = readonly;
                                ReverseRecoveredEntry::IndexSignature {
                                    signature: recovered,
                                }
                            },
                        );
                        if !matches!(verdict, RelationResult::Assignable { .. }) {
                            break;
                        }
                    }
                }
                verdict
            }
            Some(SemanticNodeData::Array { element, .. }) => {
                let number_key =
                    graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
                self.recover_reverse_projection(*element, number_key, spec, bindings, |value| {
                    ReverseRecoveredEntry::ArrayElement { value }
                })
            }
            Some(SemanticNodeData::Tuple { elements, .. }) => {
                let elements = elements.to_vec();
                let mut verdict = assignable(bindings);
                let mut variadic_key_domain = false;
                for (index, element) in elements.into_iter().enumerate() {
                    let Some(optional) =
                        reverse_optional(element.optional, spec.modifiers.optionality)
                    else {
                        verdict = RelationResult::NotAssignable;
                        break;
                    };
                    if element.rest {
                        variadic_key_domain = true;
                        let rest = match self.unwrap_identity_carrier_for_relation(element.value) {
                            IdentityCarrierUnwrap::Concrete(rest) => rest,
                            IdentityCarrierUnwrap::Unresolvable => {
                                verdict = RelationResult::Unknown;
                                break;
                            }
                        };
                        let Some(rest_data) = graph.node_data(rest) else {
                            verdict = RelationResult::Unknown;
                            break;
                        };
                        let (rest_element, rest_readonly) = match rest_data.as_ref() {
                            SemanticNodeData::Array { element, readonly } => (*element, *readonly),
                            _ => {
                                verdict = RelationResult::Unknown;
                                break;
                            }
                        };
                        drop(rest_data);
                        let key =
                            graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
                        verdict = self.recover_reverse_projection(
                            rest_element,
                            key,
                            spec,
                            bindings,
                            move |value| {
                                let mut recovered = element;
                                recovered.value = graph.intern_node(SemanticNodeData::Array {
                                    element: value,
                                    readonly: rest_readonly,
                                });
                                recovered.optional = optional;
                                ReverseRecoveredEntry::TupleElement { element: recovered }
                            },
                        );
                    } else {
                        let key = if variadic_key_domain {
                            graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number))
                        } else {
                            graph.intern_node(SemanticNodeData::Literal(LiteralValue::String(
                                index.to_string(),
                            )))
                        };
                        verdict = self.recover_reverse_projection(
                            element.value,
                            key,
                            spec,
                            bindings,
                            move |value| {
                                let mut recovered = element;
                                recovered.value = value;
                                recovered.optional = optional;
                                ReverseRecoveredEntry::TupleElement { element: recovered }
                            },
                        );
                    }
                    if !matches!(verdict, RelationResult::Assignable { .. }) {
                        break;
                    }
                }
                verdict
            }
            _ => RelationResult::Unknown,
        };
        if !matches!(relation, RelationResult::Assignable { .. }) {
            self.relation_session_rollback(&overall_checkpoint);
            bindings.truncate(bindings_len);
            return relation;
        }

        let (recovered, partial) = self
            .dispatch_txn
            .borrow()
            .active_session()
            .map(|session| {
                (
                    session.recovered_since(checkpoint),
                    session.reverse_is_partial(),
                )
            })
            .unwrap_or_default();
        let Some(aggregate) = self.assemble_reverse_candidate(source_shape, recovered) else {
            self.relation_session_rollback(&overall_checkpoint);
            bindings.truncate(bindings_len);
            return RelationResult::Unknown;
        };
        let priority = if partial {
            InferenceCandidatePriority::PartialHomomorphicMapped
        } else {
            InferenceCandidatePriority::HomomorphicMapped
        };
        if partial {
            if let Some(session) = self.dispatch_txn.borrow_mut().active_session_mut() {
                session.mark_reverse_partial();
            }
        }
        let deposited =
            self.relation_reverse_aggregate_deposit(spec.base_infer, aggregate, priority);
        if !deposited {
            self.relation_session_rollback(&overall_checkpoint);
            bindings.truncate(bindings_len);
            return RelationResult::Unknown;
        }
        assignable(bindings)
    }

    fn recover_reverse_projection<F>(
        &self,
        source: SemanticNodeId,
        key: SemanticNodeId,
        spec: &ReverseHomomorphicSpec,
        bindings: &mut Vec<InferBinding>,
        recover: F,
    ) -> RelationResult
    where
        F: FnOnce(SemanticNodeId) -> ReverseRecoveredEntry,
    {
        let property_checkpoint = self.relation_session_checkpoint();
        let Some(checkpoint) = property_checkpoint.as_ref() else {
            return RelationResult::Unknown;
        };
        let bindings_len = bindings.len();
        let template =
            self.substitute_semantic_type_param(spec.template, spec.mapper_parameter, key);
        let projection_probe = self.graph().intern_node(SemanticNodeData::IndexedAccess {
            object: spec.base_infer,
            index: IndexKey::Computed(spec.mapper_parameter),
        });
        let projection_probe =
            self.substitute_semantic_type_param(projection_probe, spec.mapper_parameter, key);
        let expected_index = match self.graph().node_data(projection_probe).as_deref() {
            Some(SemanticNodeData::IndexedAccess { index, .. }) => index.clone(),
            _ => {
                self.relation_session_rollback(&property_checkpoint);
                return RelationResult::Unknown;
            }
        };
        let projection_targets =
            self.discover_reverse_projection_targets(template, spec.base_infer, &expected_index);
        let registered = self
            .dispatch_txn
            .borrow_mut()
            .active_session_mut()
            .is_some_and(|session| session.register_projection_targets(&projection_targets));
        if !registered {
            self.relation_session_rollback(&property_checkpoint);
            return RelationResult::Unknown;
        }

        let relation = self.relate_member(source, template, bindings, InferPosition::Covariant);
        if !matches!(relation, RelationResult::Assignable { .. }) {
            self.relation_session_rollback(&property_checkpoint);
            bindings.truncate(bindings_len);
            return relation;
        }
        let candidates = self
            .dispatch_txn
            .borrow()
            .active_session()
            .map(|session| session.projection_candidates_since(checkpoint))
            .unwrap_or_default();
        let (candidate_nodes, variance) = select_inference_candidates(&candidates);
        let projection_recovered = !candidate_nodes.is_empty();
        let recovered = if projection_recovered {
            self.relation_combine_candidates(&candidate_nodes, variance)
        } else {
            self.graph()
                .intern_node(SemanticNodeData::Primitive(PrimitiveKind::Unknown))
        };
        let mut txn = self.dispatch_txn.borrow_mut();
        let Some(session) = txn.active_session_mut() else {
            drop(txn);
            self.relation_session_rollback(&property_checkpoint);
            bindings.truncate(bindings_len);
            return RelationResult::Unknown;
        };
        if !projection_recovered {
            session.mark_reverse_partial();
        }
        session.push_recovered(recover(recovered));
        assignable(bindings)
    }

    fn assemble_reverse_candidate(
        &self,
        source: ReverseSourceShape,
        recovered: Vec<ReverseRecoveredEntry>,
    ) -> Option<SemanticNodeId> {
        let graph = self.graph();
        match source {
            ReverseSourceShape::Object => {
                let mut members = Vec::new();
                let mut index_signatures = Vec::new();
                for entry in recovered {
                    match entry {
                        ReverseRecoveredEntry::ObjectMember { member, .. } => {
                            members.push(member);
                        }
                        ReverseRecoveredEntry::IndexSignature { signature, .. } => {
                            index_signatures.push(signature);
                        }
                        _ => return None,
                    }
                }
                let has_index_signature = !index_signatures.is_empty();
                Some(graph.intern_node(SemanticNodeData::Object(
                    crate::semantic_query::surface_view! {
                        members: Arc::from(members.into_boxed_slice()),
                        call_signatures: Arc::from(Vec::<SemanticNodeId>::new().into_boxed_slice()),
                        construct_signatures: Arc::from(Vec::<SemanticNodeId>::new().into_boxed_slice()),
                        index_signatures: Arc::from(index_signatures.into_boxed_slice()),
                        keyspace: None,
                        has_index_signature,
                    },
                )))
            }
            ReverseSourceShape::Array { readonly } => match recovered.as_slice() {
                [ReverseRecoveredEntry::ArrayElement { value, .. }] => {
                    Some(graph.intern_node(SemanticNodeData::Array {
                        element: *value,
                        readonly,
                    }))
                }
                _ => None,
            },
            ReverseSourceShape::Tuple { readonly } => {
                let mut elements = Vec::with_capacity(recovered.len());
                for entry in recovered {
                    match entry {
                        ReverseRecoveredEntry::TupleElement { element, .. } => {
                            elements.push(element);
                        }
                        _ => return None,
                    }
                }
                match self.normalize_tuple_spread(&elements, readonly) {
                    super::build::NormalizedTupleShape::Tuple(elements) => {
                        Some(graph.intern_node(SemanticNodeData::Tuple {
                            elements: Arc::from(elements.into_boxed_slice()),
                            readonly,
                        }))
                    }
                    super::build::NormalizedTupleShape::Array(array) => Some(array),
                }
            }
        }
    }

    fn discover_reverse_projection_targets(
        &self,
        root: SemanticNodeId,
        base_infer: SemanticNodeId,
        expected_index: &IndexKey,
    ) -> Vec<SemanticNodeId> {
        let graph = self.graph();
        let Some(SemanticNodeData::Infer {
            name: base_name,
            binder: base_binder,
        }) = graph.node_data(base_infer).as_deref().cloned()
        else {
            return Vec::new();
        };
        let mut targets = Vec::new();
        let mut visited: FxHashSet<(SemanticNodeId, bool)> = FxHashSet::default();
        let mut stack = vec![(root, false)];
        while let Some((node, shadowed)) = stack.pop() {
            if !visited.insert((node, shadowed)) {
                continue;
            }
            let Some(data) = graph.node_data(node) else {
                continue;
            };
            match data.as_ref() {
                SemanticNodeData::IndexedAccess { object, index } => {
                    if !shadowed
                        && index == expected_index
                        && self.reverse_projection_object_matches(
                            *object,
                            base_infer,
                            base_binder.clone(),
                        )
                    {
                        targets.push(node);
                    }
                    stack.push((*object, shadowed));
                    if let IndexKey::Computed(index) = index {
                        stack.push((*index, shadowed));
                    }
                }
                SemanticNodeData::Mapped { source, mapper } => {
                    stack.push((*source, shadowed));
                    stack.push((mapper.key_space, shadowed));
                    let mapper_shadows = matches!(
                        graph.node_data(mapper.parameter_node).as_deref(),
                        Some(SemanticNodeData::TypeParam { display_name, .. })
                            if display_name.as_ref() == base_name.as_ref()
                    );
                    stack.push((mapper.value_expr, shadowed || mapper_shadows));
                    if let Some(remap) = mapper.name_remap {
                        stack.push((remap, shadowed || mapper_shadows));
                    }
                }
                SemanticNodeData::Conditional {
                    check,
                    extends,
                    true_branch_ref,
                    false_branch_ref,
                    ..
                } => {
                    let conditional_shadows =
                        self.extends_pattern_declares_infer(*extends, base_infer);
                    stack.push((*check, shadowed));
                    stack.push((*false_branch_ref, shadowed));
                    stack.push((*extends, shadowed || conditional_shadows));
                    stack.push((*true_branch_ref, shadowed || conditional_shadows));
                }
                SemanticNodeData::Signature {
                    params,
                    return_type,
                    type_parameters,
                    ..
                } => {
                    if shadowed
                        || type_parameters
                            .iter()
                            .any(|parameter| parameter.name.as_ref() == base_name.as_ref())
                    {
                        continue;
                    }
                    for parameter in params.iter() {
                        stack.push((parameter.ty, false));
                    }
                    stack.push((*return_type, false));
                    for parameter in type_parameters.iter() {
                        if let Some(constraint) = parameter.constraint {
                            stack.push((constraint, false));
                        }
                        if let Some(default) = parameter.default {
                            stack.push((default, false));
                        }
                    }
                }
                SemanticNodeData::Alias(inner) => stack.push((*inner, shadowed)),
                SemanticNodeData::Union(members) | SemanticNodeData::Intersection(members) => {
                    stack.extend(members.iter().map(|member| (*member, shadowed)));
                }
                SemanticNodeData::Array { element, .. } => stack.push((*element, shadowed)),
                SemanticNodeData::Tuple { elements, .. } => {
                    stack.extend(elements.iter().map(|element| (element.value, shadowed)));
                }
                SemanticNodeData::Object(surface) => {
                    stack.extend(
                        surface
                            .positive_members()
                            .iter()
                            .map(|member| (member.value, shadowed)),
                    );
                    stack.extend(
                        surface
                            .call_signatures
                            .iter()
                            .chain(surface.construct_signatures.iter())
                            .map(|signature| (*signature, shadowed)),
                    );
                    for signature in surface.index_signatures.iter() {
                        stack.push((signature.key_type, shadowed));
                        stack.push((signature.value_type, shadowed));
                    }
                    if let Some(keyspace) = surface.keyspace {
                        stack.push((keyspace, shadowed));
                    }
                }
                SemanticNodeData::MergedDecl { contributors } => {
                    stack.extend(
                        contributors
                            .iter()
                            .map(|contributor| (*contributor, shadowed)),
                    );
                }
                SemanticNodeData::TemplateLiteral { expressions, .. } => {
                    stack.extend(expressions.iter().map(|expression| (*expression, shadowed)));
                }
                SemanticNodeData::KeyOf { base } => stack.push((*base, shadowed)),
                SemanticNodeData::InstantiationRef { args, .. } => {
                    stack.extend(args.iter().map(|argument| (*argument, shadowed)));
                }
                other => {
                    stack.extend(
                        other
                            .carrier_type_args()
                            .iter()
                            .map(|argument| (*argument, shadowed)),
                    );
                }
            }
        }
        targets.sort_by_key(|node| node.0);
        targets.dedup();
        targets
    }

    fn reverse_projection_object_matches(
        &self,
        object: SemanticNodeId,
        base_infer: SemanticNodeId,
        base_binder: crate::semantic_query::InferBinderId,
    ) -> bool {
        let Some(object) = self.peel_relation_alias(object) else {
            return false;
        };
        if object == base_infer {
            return true;
        }
        let Some(data) = self.graph().node_data(object) else {
            return false;
        };
        match data.as_ref() {
            SemanticNodeData::InferRef { binder, .. } => *binder == base_binder,
            _ => false,
        }
    }

    // ──────────────────────────────────────────────────────────────────
    // The lattice adapter: full-key sub-relations from the reducer
    // ──────────────────────────────────────────────────────────────────

    /// The full identity of a sub-relation inside the current frame:
    /// inherits the relation kind / policy / freshness / env context of the
    /// nearest open RELATE ancestor — never the untyped top of a mixed
    /// stack. Ordinary direct-infer member judgements remain
    /// session-independent. Reverse-projection judgements retain the frozen
    /// inference context because registered indexed-access targets alter
    /// their reduction and therefore their memo identity.
    fn relation_sub_key(&self, source: SemanticNodeId, target: SemanticNodeId) -> RelateMemoKey {
        let txn = self.dispatch_txn.borrow();
        match txn.reentry().nearest_relate() {
            Some((top, _)) => {
                let inference_context = top.inference_context.as_ref().and_then(|context| {
                    (context.pass_kind == InferencePassKind::ReverseHomomorphicMapped)
                        .then(|| context.clone())
                });
                RelateMemoKey {
                    source,
                    target,
                    relation: top.relation,
                    policy: top.policy,
                    source_freshness: top.source_freshness,
                    inference_context,
                    context: top.context,
                }
            }
            None => self.relate_key_for(source, target),
        }
    }

    /// The reducer's sub-relation step (the 8 recursion sites of the
    /// retired hidden path): an in-scope `Infer` occupant binds through
    /// the active session; every other sub-judgement re-enters the SAME
    /// full-key authority ([`Self::execute_relate`]) and folds onto the
    /// reducer's lattice.
    pub(super) fn relate_member(
        &self,
        source: SemanticNodeId,
        target: SemanticNodeId,
        bindings: &mut Vec<InferBinding>,
        position: InferPosition,
    ) -> RelationResult {
        self.relate_member_with_freshness(source, target, bindings, position, None)
    }

    /// Relate an ordinary union arm after the enclosing fresh-source frame
    /// has completed its one excess-property prepass. Freshness is consumed
    /// by that enclosing check; carrying it into each arm would rerun a
    /// branch-local excess check and reject names known by sibling arms.
    fn relate_union_arm_after_excess_prepass(
        &self,
        source: SemanticNodeId,
        target: SemanticNodeId,
        bindings: &mut Vec<InferBinding>,
        position: InferPosition,
    ) -> RelationResult {
        self.relate_member_with_freshness(
            source,
            target,
            bindings,
            position,
            Some(crate::semantic_query::FreshnessKey::Regular),
        )
    }

    fn relate_member_with_freshness(
        &self,
        source: SemanticNodeId,
        target: SemanticNodeId,
        bindings: &mut Vec<InferBinding>,
        position: InferPosition,
        source_freshness: Option<crate::semantic_query::FreshnessKey>,
    ) -> RelationResult {
        let occurrence = self.relation_occurrence(position);
        if let Some(result) = self.try_relation_projection(source, target, bindings, occurrence) {
            return result;
        }
        let graph = self.graph();
        if self.relation_session_active() {
            match occurrence.variance {
                VariancePhase::Covariant | VariancePhase::Invariant => {
                    if matches!(
                        graph.node_data(target).as_deref(),
                        Some(SemanticNodeData::Infer { .. } | SemanticNodeData::TypeParam { .. })
                    ) {
                        if !self.relation_deposit(target, source, occurrence) {
                            return RelationResult::Unknown;
                        }
                        return assignable(bindings);
                    }
                }
                VariancePhase::Contravariant => {
                    if matches!(
                        graph.node_data(source).as_deref(),
                        Some(SemanticNodeData::Infer { .. } | SemanticNodeData::TypeParam { .. })
                    ) {
                        if !self.relation_deposit(source, target, occurrence) {
                            return RelationResult::Unknown;
                        }
                        return assignable(bindings);
                    }
                }
            }
        }
        // The discharge substitution rail (re-discharge, design §2.3 step
        // 4): a member of a negatively-closed SCC re-runs against the
        // converged verdicts.
        let mut key = self.relation_sub_key(source, target);
        if let Some(source_freshness) = source_freshness {
            key.source_freshness = source_freshness;
        }
        {
            let txn = self.dispatch_txn.borrow();
            if !txn.obligations.substitution().is_empty() {
                if let Some(step) =
                    provisional_relate_step(txn.obligations.substitution(), &key, occurrence)
                {
                    return match step {
                        RelationStep::Assignable { bindings: sub } => {
                            for binding in sub.iter() {
                                if !bindings
                                    .iter()
                                    .any(|existing| existing.param == binding.param)
                                {
                                    bindings.push(binding.clone());
                                }
                            }
                            assignable(bindings)
                        }
                        RelationStep::NotAssignable => RelationResult::NotAssignable,
                        _ => RelationResult::Unknown,
                    };
                }
            }
        }
        match self.execute_relate_with_occurrence(key, occurrence) {
            RelationStep::Assumed(_) => {
                // The coinductive hypothesis: assumed to hold; the edge is
                // recorded on the frame.
                assignable(bindings)
            }
            RelationStep::Assignable { bindings: sub } => {
                for binding in sub.iter() {
                    if !bindings.iter().any(|b| b.param == binding.param) {
                        bindings.push(binding.clone());
                    }
                }
                assignable(bindings)
            }
            RelationStep::NotAssignable => RelationResult::NotAssignable,
            RelationStep::Unknown => RelationResult::Unknown,
            RelationStep::BudgetExceeded(cap) => {
                let mut txn = self.dispatch_txn.borrow_mut();
                if let Some(depth) = txn.reentry().depth().checked_sub(1) {
                    txn.reentry_mut().note_budget_edge(depth, cap);
                }
                RelationResult::Unknown
            }
        }
    }

    // ──────────────────────────────────────────────────────────────────
    // The reducer: prefilter → carrier unwrap → structural worklist
    // ──────────────────────────────────────────────────────────────────

    /// Run one frame's reduction: the O(tag) prefilter first (RI-5 — never
    /// a parallel truth source), then the dispatch-aware structural
    /// judgement. `bindings` accumulates sub-relation bindings onto the
    /// caller's lattice.
    fn reduce_relation(
        &self,
        key: &RelateMemoKey,
        bindings: &mut Vec<InferBinding>,
    ) -> RelationResult {
        // Axis refusal: the reducer implements the ASSIGNABILITY relation
        // (regular AND fresh sources; the excess-property policy runs the
        // fresh prepass below). A key on any not-yet-implemented axis
        // (`Identity` / `Subtype` / `StrictSubtype` / `Comparable`, a
        // non-default overload-selection policy) must REFUSE — undecided,
        // ReturnOnly, zero admission — never route the ask through the
        // assignability lattice (an `Identity` ask through the
        // `(_, unknown) => Assignable` arm would publish a false verdict).
        // Both strict variance regimes ARE implemented (RI-10).
        if key.relation != RelationKind::Assignable
            || key.policy.overload_selection != crate::semantic_query::OverloadSelectionPolicy::All
        {
            return RelationResult::Unknown;
        }
        // Test-only forced budget knob (D6): trips the work budget on the
        // first driver pass so the typed `BudgetExceeded` outcome and its
        // three-layer non-admission are exercised deterministically.
        let host = self.ctx.host_for_fact_tracer_install();
        if host
            .relation_knobs
            .force_budget_exhaustion
            .load(std::sync::atomic::Ordering::Relaxed)
        {
            let cap = RecursionOrBudgetCap {
                kind: crate::semantic_query::BudgetExceededKind::RelationBudget,
                limit: 0,
            };
            let mut txn = self.dispatch_txn.borrow_mut();
            if let Some(depth) = txn.reentry().depth().checked_sub(1) {
                txn.reentry_mut().note_budget_edge(depth, cap);
            }
            return RelationResult::Unknown;
        }
        let occurrence = self.relation_current_occurrence();
        if let Some(result) =
            self.try_relation_projection(key.source, key.target, bindings, occurrence)
        {
            return result;
        }
        let reverse_spec = self
            .dispatch_txn
            .borrow()
            .active_session()
            .and_then(InferenceSession::reverse_spec)
            .cloned()
            .filter(|spec| spec.mapped_node == key.target);
        if let Some(spec) = reverse_spec {
            return self.relate_reverse_homomorphic(key.source, &spec, bindings);
        }
        match self.reduce_relation_conditional(key.source) {
            Some(Some(reduced)) => {
                return self.relate_member(reduced, key.target, bindings, InferPosition::Covariant);
            }
            Some(None) => return RelationResult::Unknown,
            None => {}
        }
        match self.reduce_relation_conditional(key.target) {
            Some(Some(reduced)) => {
                return self.relate_member(key.source, reduced, bindings, InferPosition::Covariant);
            }
            Some(None) => return RelationResult::Unknown,
            None => {}
        }
        // The binding root's bare-`Infer` arm: `check extends infer X`
        // binds `X := check` for any check through the active session.
        if self.relation_session_active() {
            match occurrence.variance {
                VariancePhase::Covariant | VariancePhase::Invariant => {
                    if let Some(SemanticNodeData::Infer { .. }) =
                        self.graph().node_data(key.target).as_deref()
                    {
                        if !self.relation_deposit(key.target, key.source, occurrence) {
                            return RelationResult::Unknown;
                        }
                        return assignable(bindings);
                    }
                }
                VariancePhase::Contravariant => {
                    if let Some(SemanticNodeData::Infer { .. }) =
                        self.graph().node_data(key.source).as_deref()
                    {
                        if !self.relation_deposit(key.source, key.target, occurrence) {
                            return RelationResult::Unknown;
                        }
                        return assignable(bindings);
                    }
                }
            }
        }
        // The fresh excess-property prepass (once per frame, BEFORE ordinary
        // union-arm distribution): gate = Fresh source + excess policy; a
        // rejection decides the frame, an undecidable check stays Unknown
        // (never collapsed), a pass continues into the ordinary relation.
        if key.source_freshness == crate::semantic_query::FreshnessKey::Fresh
            && key.policy.excess_property_check
        {
            match self.excess_property_prepass(key, bindings) {
                super::relation_excess::ExcessPrepassOutcome::Reject => {
                    return RelationResult::NotAssignable;
                }
                super::relation_excess::ExcessPrepassOutcome::Undecided => {
                    return RelationResult::Unknown;
                }
                super::relation_excess::ExcessPrepassOutcome::Pass => {}
            }
        }
        // Object-spread programs are formulas, not ordinary graph-node
        // surfaces. Relate them before the identity fast path and before any
        // legacy Object handling so an unresolved program is never accepted
        // merely because both sides carry the same node id.
        if let Some(result) =
            self.try_object_spread_program_relation(key.source, key.target, bindings)
        {
            return result;
        }
        match self.shallow_relation_check(key.source, key.target) {
            ShallowRelation::Assignable => return assignable(bindings),
            ShallowRelation::NotAssignable => return RelationResult::NotAssignable,
            ShallowRelation::Unknown => {}
        }
        self.decide_relation_with_dispatch(key.source, key.target, bindings)
    }

    fn try_object_spread_program_relation(
        &self,
        source: SemanticNodeId,
        target: SemanticNodeId,
        bindings: &mut Vec<InferBinding>,
    ) -> Option<RelationResult> {
        // Transparent aliases are followed before program recognition so an
        // aliased program is still a program side.
        let source = self.follow_relation_aliases(source);
        let target = self.follow_relation_aliases(target);
        let source_is_program = matches!(
            self.graph().node_data(source).as_deref(),
            Some(SemanticNodeData::ObjectSpreadProgram(_))
        );
        let target_is_program = matches!(
            self.graph().node_data(target).as_deref(),
            Some(SemanticNodeData::ObjectSpreadProgram(_))
        );
        if !source_is_program && !target_is_program {
            return None;
        }

        // One side is a program: resolve identity carriers on both sides so a
        // `DeclRef` / `InstantiationRef` counterpart reaches its Object surface
        // instead of collapsing to Unknown.
        let source = match self.unwrap_identity_carrier_for_relation(source) {
            IdentityCarrierUnwrap::Concrete(id) => id,
            IdentityCarrierUnwrap::Unresolvable => return Some(RelationResult::Unknown),
        };
        let target = match self.unwrap_identity_carrier_for_relation(target) {
            IdentityCarrierUnwrap::Concrete(id) => id,
            IdentityCarrierUnwrap::Unresolvable => return Some(RelationResult::Unknown),
        };
        // Top/bottom rules apply before projection: `any` / `unknown` accept
        // from either side, `never` is bottom, an error type swallows, and the
        // `object` nonprimitive accepts every program (a construction program
        // always produces an object). These mirror `expand_pair`'s wildcard
        // and `object` arms so root and worklist agree.
        let source_data = self.graph().node_data(source);
        let target_data = self.graph().node_data(target);
        let error_swallows = |data: &SemanticNodeData| matches!(data, SemanticNodeData::Opaque(err) if err.is_error_type());
        match (source_data.as_deref(), target_data.as_deref()) {
            (Some(data), _) | (_, Some(data)) if error_swallows(data) => {
                return Some(assignable(bindings));
            }
            (Some(SemanticNodeData::Primitive(PrimitiveKind::Never)), _)
            | (_, Some(SemanticNodeData::Primitive(PrimitiveKind::Any | PrimitiveKind::Unknown)))
            | (Some(SemanticNodeData::Primitive(PrimitiveKind::Any)), _) => {
                return Some(assignable(bindings));
            }
            (_, Some(SemanticNodeData::Primitive(PrimitiveKind::Never))) => {
                return Some(RelationResult::NotAssignable);
            }
            (_, Some(SemanticNodeData::Primitive(PrimitiveKind::Object))) => {
                return Some(assignable(bindings));
            }
            _ => {}
        }
        let Some(source_branches) = self.projected_relation_branches(source) else {
            return Some(RelationResult::Unknown);
        };
        let Some(target_branches) = self.projected_relation_branches(target) else {
            return Some(RelationResult::Unknown);
        };
        let overall_checkpoint = self.relation_session_checkpoint();
        let overall_bindings_len = bindings.len();
        let mut universal_unknown = false;

        for source_branch in &source_branches {
            let source_checkpoint = self.relation_session_checkpoint();
            let source_bindings_len = bindings.len();
            let mut existential_unknown = false;
            let mut accepted = false;
            for target_branch in &target_branches {
                let alternative_checkpoint = self.relation_session_checkpoint();
                let alternative_bindings_len = bindings.len();
                match self.relate_projected_object_branch(source_branch, target_branch, bindings) {
                    RelationResult::Assignable { .. } => {
                        accepted = true;
                        break;
                    }
                    RelationResult::Unknown => {
                        self.relation_session_rollback(&alternative_checkpoint);
                        bindings.truncate(alternative_bindings_len);
                        existential_unknown = true;
                    }
                    RelationResult::NotAssignable => {
                        self.relation_session_rollback(&alternative_checkpoint);
                        bindings.truncate(alternative_bindings_len);
                    }
                }
            }
            if accepted {
                continue;
            }
            self.relation_session_rollback(&source_checkpoint);
            bindings.truncate(source_bindings_len);
            if existential_unknown {
                universal_unknown = true;
                continue;
            }
            self.relation_session_rollback(&overall_checkpoint);
            bindings.truncate(overall_bindings_len);
            return Some(RelationResult::NotAssignable);
        }
        if universal_unknown {
            self.relation_session_rollback(&overall_checkpoint);
            bindings.truncate(overall_bindings_len);
            Some(RelationResult::Unknown)
        } else {
            Some(assignable(bindings))
        }
    }

    /// Follow transparent `Alias` indirection (with a cycle guard) without
    /// touching declaration carriers — the cheap half of relation
    /// normalization.
    fn follow_relation_aliases(&self, node: SemanticNodeId) -> SemanticNodeId {
        let graph = self.graph();
        let mut current = node;
        let mut seen = FxHashSet::default();
        while seen.insert(current) {
            match graph.node_data(current).as_deref() {
                Some(SemanticNodeData::Alias(inner)) => current = *inner,
                _ => return current,
            }
        }
        current
    }

    /// When `node == node` unwraps (alias follow + identity-carrier unwrap) to
    /// an object-spread program, the identical pair answers through the
    /// program protocol: node identity is not a completeness proof, so an
    /// open program stays non-publishing `Unknown` while a closed one
    /// decides. `None` when the node is not (or does not unwrap to) a
    /// program — the ordinary identity shortcut applies.
    fn try_identical_open_program_result(
        &self,
        node: SemanticNodeId,
        bindings: &mut Vec<InferBinding>,
    ) -> Option<RelationResult> {
        let followed = self.follow_relation_aliases(node);
        let normalized = match self.unwrap_identity_carrier_for_relation(followed) {
            IdentityCarrierUnwrap::Concrete(id) => id,
            IdentityCarrierUnwrap::Unresolvable => return None,
        };
        let normalized = match self.graph().node_data(normalized).as_deref() {
            Some(SemanticNodeData::BareRef(_) | SemanticNodeData::ImportType(_)) => {
                let transit = ProjectionReductionContext::structural_transit();
                let (resolved, _, _) =
                    self.resolve_carrier_subject_node_capturing_suppress(normalized, transit);
                resolved
            }
            _ => normalized,
        };
        if !matches!(
            self.graph().node_data(normalized).as_deref(),
            Some(SemanticNodeData::ObjectSpreadProgram(_))
        ) {
            return None;
        }
        self.try_object_spread_program_relation(normalized, normalized, bindings)
    }

    fn projected_relation_branches(
        &self,
        node: SemanticNodeId,
    ) -> Option<Vec<ProjectedRelationBranch>> {
        let node = self.follow_relation_aliases(node);
        let data = self.graph().node_data(node)?;
        match data.as_ref() {
            SemanticNodeData::Union(arms) => {
                // Target disjunction: each arm is one accepting alternative.
                let arms = Arc::clone(arms);
                drop(data);
                let mut branches = Vec::with_capacity(arms.len());
                for arm in arms.iter() {
                    let arm = match self.unwrap_identity_carrier_for_relation(*arm) {
                        IdentityCarrierUnwrap::Concrete(id) => id,
                        IdentityCarrierUnwrap::Unresolvable => return None,
                    };
                    branches.extend(self.projected_relation_branches(arm)?);
                }
                Some(branches)
            }
            SemanticNodeData::ObjectSpreadProgram(_) => {
                drop(data);
                let formula = match self.project_object_spread_for_consumer(
                    node,
                    crate::semantic_query::ObjectProjectionSelector::Surface,
                    ProjectionReductionContext::structural_transit(),
                ) {
                    QueryResult::Value(formula) => formula,
                    QueryResult::Recursive(_) | QueryResult::Error(_) => return None,
                };
                Some(
                    formula
                        .alternatives()
                        .iter()
                        .map(|alternative| {
                            let mut members = Vec::new();
                            alternative.positive().visit(|fact| {
                                members.push(ProjectedRelationMember {
                                    key: fact.key().clone(),
                                    presence: fact.presence(),
                                    value: fact.value().clone(),
                                });
                            });
                            let mut call_signatures = Vec::new();
                            let mut construct_signatures = Vec::new();
                            for signature in alternative.signatures() {
                                match signature.kind() {
                                    crate::semantic_query::ObjectSignatureKind::Call => {
                                        call_signatures.push(signature.node());
                                    }
                                    crate::semantic_query::ObjectSignatureKind::Construct => {
                                        construct_signatures.push(signature.node());
                                    }
                                }
                            }
                            ProjectedRelationBranch {
                                members,
                                indices: alternative
                                    .indices()
                                    .iter()
                                    .map(|index| ProjectedRelationIndex {
                                        key_type: index.key_type(),
                                        value: index.value().clone(),
                                    })
                                    .collect(),
                                call_signatures,
                                construct_signatures,
                                open: alternative.closed().is_none(),
                            }
                        })
                        .collect(),
                )
            }
            SemanticNodeData::Object(surface) => {
                let branch = ProjectedRelationBranch {
                    members: surface
                        .positive_members()
                        .iter()
                        .filter_map(|member| {
                            Some(ProjectedRelationMember {
                                key: member.key.cloned_known()?,
                                presence: if member.optional {
                                    crate::semantic_query::PositiveKeyPresence::Optional
                                } else {
                                    crate::semantic_query::PositiveKeyPresence::Required
                                },
                                value: crate::semantic_query::ProjectionEvidence::Proven(
                                    member.value,
                                ),
                            })
                        })
                        .collect(),
                    indices: surface
                        .index_signatures
                        .iter()
                        .map(|index| ProjectedRelationIndex {
                            key_type: index.key_type,
                            value: crate::semantic_query::ProjectionEvidence::Proven(
                                index.value_type,
                            ),
                        })
                        .collect(),
                    call_signatures: surface.call_signatures.to_vec(),
                    construct_signatures: surface.construct_signatures.to_vec(),
                    open: false,
                };
                Some(vec![branch])
            }
            _ => None,
        }
    }

    fn relate_projected_object_branch(
        &self,
        source: &ProjectedRelationBranch,
        target: &ProjectedRelationBranch,
        bindings: &mut Vec<InferBinding>,
    ) -> RelationResult {
        let mut acc = assignable(bindings);
        for target_member in &target.members {
            // Element-access identity: a numeric key and its canonical
            // string spelling address the same property.
            let source_member = source
                .members
                .iter()
                .find(|member| member.key.element_access_collides(&target_member.key));
            let pair = match source_member {
                Some(source_member)
                    if target_member.presence
                        == crate::semantic_query::PositiveKeyPresence::Required
                        && source_member.presence
                            == crate::semantic_query::PositiveKeyPresence::Optional =>
                {
                    // Optional-to-required: under `strictNullChecks` the
                    // implied `undefined` cannot relate to the required value;
                    // with null checks relaxed the pair relates on the value
                    // types alone (mirrors `relate_property_pair`).
                    let strict = self
                        .dispatch_txn
                        .borrow()
                        .relation
                        .strict
                        .unwrap_or(StrictFamilyConfig::TS_STRICT);
                    if strict.strict_null_checks {
                        RelationResult::NotAssignable
                    } else {
                        self.relate_projected_values(
                            &source_member.value,
                            &target_member.value,
                            bindings,
                        )
                    }
                }
                Some(source_member) => self.relate_projected_values(
                    &source_member.value,
                    &target_member.value,
                    bindings,
                ),
                None => {
                    // An index signature constrains the value a key would
                    // carry; it never manufactures named presence. Required
                    // named target keys therefore need a named source fact.
                    if target_member.presence
                        == crate::semantic_query::PositiveKeyPresence::Required
                    {
                        if source.open {
                            RelationResult::Unknown
                        } else {
                            RelationResult::NotAssignable
                        }
                    } else {
                        // Optional target member satisfied by source index
                        // evidence: EVERY applicable source index relates
                        // (mirroring the legacy
                        // `relate_property_via_source_index` loop) — the
                        // first match alone would let an `any` index
                        // swallow a refuting narrower index.
                        let mut matched = false;
                        let mut pair = assignable(bindings);
                        for index in source.indices.iter().filter(|index| {
                            index_signature_applies_to_property(
                                self.graph(),
                                index.key_type,
                                &target_member.key,
                            )
                        }) {
                            matched = true;
                            pair = result_and(
                                pair,
                                self.relate_projected_values(
                                    &index.value,
                                    &target_member.value,
                                    bindings,
                                ),
                            );
                            if matches!(pair, RelationResult::NotAssignable) {
                                break;
                            }
                        }
                        if matched {
                            pair
                        } else if source.open {
                            RelationResult::Unknown
                        } else {
                            assignable(bindings)
                        }
                    }
                }
            };
            acc = result_and(acc, pair);
            if matches!(acc, RelationResult::NotAssignable) {
                return acc;
            }
        }

        for target_index in &target.indices {
            // Broad index obligations are universal conditional value
            // obligations: relate EVERY source index fact whose domain
            // overlaps the target's (a number index satisfies a string
            // obligation; a string index covers the number domain for
            // value relating — numeric keys are strings at runtime) and
            // every known named contribution the target's key type covers.
            // The legacy authority (`relate_target_index_signature`)
            // relates all overlaps; relating only the first would let an
            // `any` index swallow a refuting narrower index (order-
            // dependent false Accept). A closed branch with only
            // compatible known contributions satisfies the obligation
            // without owning an index signature itself. A source index
            // whose domain does not overlap is SKIPPED — domain
            // non-overlap alone never rejects (the legacy rule; tsc
            // agrees for string-to-number). The payload relation still
            // rejects value-mismatched overlapping indices.
            for source_index in source.indices.iter().filter(|index| {
                index_domains_overlap(self.graph(), index.key_type, target_index.key_type)
            }) {
                acc = result_and(
                    acc,
                    self.relate_projected_values(
                        &source_index.value,
                        &target_index.value,
                        bindings,
                    ),
                );
                if matches!(acc, RelationResult::NotAssignable) {
                    return acc;
                }
            }
            for source_member in source.members.iter().filter(|member| {
                index_signature_applies_to_property(
                    self.graph(),
                    target_index.key_type,
                    &member.key,
                )
            }) {
                acc = result_and(
                    acc,
                    self.relate_projected_values(
                        &source_member.value,
                        &target_index.value,
                        bindings,
                    ),
                );
            }
            if source.open {
                // A live residual needs an exact enumerable-value envelope;
                // without one the universal obligation cannot close.
                acc = result_and(acc, RelationResult::Unknown);
            }
            if matches!(acc, RelationResult::NotAssignable) {
                return acc;
            }
        }

        for target_signature in &target.call_signatures {
            let pair = if source.call_signatures.is_empty() && source.open {
                RelationResult::Unknown
            } else {
                self.relate_signature_alternatives(
                    &source.call_signatures,
                    *target_signature,
                    bindings,
                )
            };
            acc = result_and(acc, pair);
        }
        for target_signature in &target.construct_signatures {
            let pair = if source.construct_signatures.is_empty() && source.open {
                RelationResult::Unknown
            } else {
                self.relate_signature_alternatives(
                    &source.construct_signatures,
                    *target_signature,
                    bindings,
                )
            };
            acc = result_and(acc, pair);
        }

        if target.open {
            acc = result_and(acc, RelationResult::Unknown);
        }
        acc
    }

    fn relate_projected_values(
        &self,
        source: &crate::semantic_query::ProjectionEvidence<SemanticNodeId>,
        target: &crate::semantic_query::ProjectionEvidence<SemanticNodeId>,
        bindings: &mut Vec<InferBinding>,
    ) -> RelationResult {
        match (source, target) {
            (
                crate::semantic_query::ProjectionEvidence::Proven(source),
                crate::semantic_query::ProjectionEvidence::Proven(target),
            ) => self.relate_member(*source, *target, bindings, InferPosition::Covariant),
            _ => RelationResult::Unknown,
        }
    }

    /// Reduce one conditional shell through the canonical conditional query.
    /// The outer option distinguishes a non-conditional node; the inner
    /// option distinguishes a decided reduction from an undecided shell.
    fn reduce_relation_conditional(&self, node: SemanticNodeId) -> Option<Option<SemanticNodeId>> {
        let data = self.graph().node_data(node)?;
        let SemanticNodeData::Conditional {
            check,
            extends,
            true_branch_ref,
            false_branch_ref,
            distributive,
        } = data.as_ref()
        else {
            return None;
        };
        let key = SemanticQueryKey::Conditional {
            check: *check,
            extends: *extends,
            true_branch: *true_branch_ref,
            false_branch: *false_branch_ref,
            distributive: *distributive,
        };
        drop(data);
        Some(match self.execute_type_node(key) {
            QueryResult::Value(SemanticQueryOutput { value, .. }) if value != node => Some(value),
            _ => None,
        })
    }

    /// The O(tag) fast-reject prefilter (RI-5): decides the trivial
    /// primitive/identity/top/bottom cases inline BEFORE any recursive
    /// structural work. Non-trivial pairs return `Unknown` and fall
    /// through to the structural reducer.
    pub(super) fn shallow_relation_check(
        &self,
        source: SemanticNodeId,
        target: SemanticNodeId,
    ) -> ShallowRelation {
        if source == target {
            let mut no_bindings = Vec::new();
            if let Some(result) = self.try_identical_open_program_result(source, &mut no_bindings) {
                return match result {
                    RelationResult::Assignable { .. } => ShallowRelation::Assignable,
                    RelationResult::NotAssignable => ShallowRelation::NotAssignable,
                    RelationResult::Unknown => ShallowRelation::Unknown,
                };
            }
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
            // The error-type wildcard fires BEFORE the `(_, Never)` bottom
            // arm — `error` relates bidirectionally like `any` (the same
            // arm order the structural reducer applies).
            (SemanticNodeData::Opaque(err), _) if err.is_error_type() => {
                ShallowRelation::Assignable
            }
            (_, SemanticNodeData::Opaque(err)) if err.is_error_type() => {
                ShallowRelation::Assignable
            }
            (SemanticNodeData::Primitive(PrimitiveKind::Never), _) => ShallowRelation::Assignable,
            (_, SemanticNodeData::Primitive(PrimitiveKind::Unknown)) => ShallowRelation::Assignable,
            (_, SemanticNodeData::Primitive(PrimitiveKind::Any)) => ShallowRelation::Assignable,
            (SemanticNodeData::Primitive(PrimitiveKind::Any), _) => ShallowRelation::Assignable,
            (_, SemanticNodeData::Primitive(PrimitiveKind::Never)) => {
                ShallowRelation::NotAssignable
            }
            // Strict-family behavioral branch (RI-10), mirrored from the
            // structural reducer's arm order: with `strictNullChecks` OFF,
            // `null` / `undefined` are assignable to every remaining target
            // (`never` already rejected above).
            (SemanticNodeData::Primitive(PrimitiveKind::Null | PrimitiveKind::Undefined), _)
                if !self
                    .dispatch_txn
                    .borrow()
                    .relation
                    .strict
                    .unwrap_or(StrictFamilyConfig::TS_STRICT)
                    .strict_null_checks =>
            {
                ShallowRelation::Assignable
            }
            (SemanticNodeData::Primitive(a), SemanticNodeData::Primitive(b)) => {
                if a == b || (*a == PrimitiveKind::Undefined && *b == PrimitiveKind::Void) {
                    ShallowRelation::Assignable
                } else {
                    ShallowRelation::NotAssignable
                }
            }
            _ => ShallowRelation::Unknown,
        }
    }

    /// Construct a public payload and intern its proof into the store's
    /// payload-side proof table (Decision 4 — the proof rides the table
    /// BY ID, never embedded on the value / type-values surface).
    fn relation_payload(
        &self,
        outcome: RelationOutcome,
        bindings: Arc<[InferBinding]>,
        proof: RelationProof,
    ) -> RelationPayload {
        let relation_proof = self.graph().intern_relation_proof(proof);
        RelationPayload {
            outcome,
            bindings,
            relation_proof,
        }
    }

    /// Dispatch-aware relation judgement: the Object-vs-Record arm first,
    /// then the identity-carrier unwrap, then the structural worklist.
    fn decide_relation_with_dispatch(
        &self,
        source: SemanticNodeId,
        target: SemanticNodeId,
        bindings: &mut Vec<InferBinding>,
    ) -> RelationResult {
        if let Some(r) = self.try_object_vs_record_relation(source, target, bindings) {
            return r;
        }
        let source = match self.unwrap_identity_carrier_for_relation(source) {
            IdentityCarrierUnwrap::Concrete(id) => id,
            IdentityCarrierUnwrap::Unresolvable => return RelationResult::Unknown,
        };
        let target = match self.unwrap_identity_carrier_for_relation(target) {
            IdentityCarrierUnwrap::Concrete(id) => id,
            IdentityCarrierUnwrap::Unresolvable => return RelationResult::Unknown,
        };
        // Function-inference demand point (the pre-relation function-infer
        // case, now inside the authority): with an active session and a
        // Function pattern, a check still riding a deferred /
        // `InstantiationRef` shell materialises through the oracle demand
        // so positional binding can zip its signature.
        if self.relation_session_active() {
            if let Some(pattern) = self.relation_pattern_info(target) {
                if pattern.shape == InferPatternShape::Function {
                    let materialised = self.materialise_function_infer_check(source);
                    if materialised != source {
                        return self.decide_relation(materialised, target, bindings);
                    }
                }
            }
        }
        self.decide_relation(source, target, bindings)
    }

    /// Materialise a function-infer check through the oracle's transit
    /// demand (the retired pre-relation path): deferred-shell evaluation,
    /// then a one-level demanded `Instantiate` for an `InstantiationRef`
    /// carrier. Returns the input unchanged when no materialisation fires.
    fn materialise_function_infer_check(&self, check: SemanticNodeId) -> SemanticNodeId {
        let graph = self.graph();
        let oracle_demand = ProjectionReductionContext::structural_transit_with_mode(
            crate::semantic_query::ProjectionMode::Navigate,
        );
        let mut resolved = self
            .evaluate_deferred_semantic_node_with_context(check, oracle_demand)
            .into_active_query_build_node(self);
        if let Some(SemanticNodeData::InstantiationRef { base, args }) =
            graph.node_data(resolved).as_deref()
        {
            let owner_canonical = Arc::clone(&base.canonical_id);
            let slot = self.type_slot_for(
                Arc::clone(&base.canonical_id),
                base.owner,
                Arc::clone(&base.decl_name),
            );
            let args: Arc<[SemanticNodeId]> = Arc::from(
                args.iter()
                    .map(|arg| {
                        self.evaluate_deferred_semantic_node_with_context(*arg, oracle_demand)
                            .into_active_query_build_node(self)
                    })
                    .collect::<Vec<_>>()
                    .into_boxed_slice(),
            );
            let read = self.execute_read(SemanticQueryKey::Instantiate(
                crate::semantic_query::InstantiateKey::new(
                    slot,
                    args,
                    self.instantiate_context_for(&owner_canonical, oracle_demand),
                ),
            ));
            crate::request_context::observe_component_meta_read_suppress(&read);
            if let QueryResult::Value(id) = read.value {
                resolved = self.evaluate_deferred_semantic_node(id);
            }
        }
        resolved
    }

    /// The iterative structural worklist driver. Consumes a worklist of
    /// pairs and reducers, combining the final [`RelationResult`].
    ///
    /// **Termination budget.** The driver caps total work at
    /// `10 × graph.node_count()` with a minimum floor of 4096 entries.
    /// Exceeding the budget poisons the frame with the typed
    /// [`RecursionOrBudgetCap`] (the public `BudgetExceeded` outcome) and
    /// yields `Unknown` — the SCC gate routes the whole component through
    /// ReturnOnly.
    pub(super) fn decide_relation(
        &self,
        source: SemanticNodeId,
        target: SemanticNodeId,
        bindings: &mut Vec<InferBinding>,
    ) -> RelationResult {
        // Program recognition precedes the identity shortcut here exactly as
        // at the root and in `expand_pair`: an open program is never accepted
        // on node identity.
        if let Some(result) = self.try_object_spread_program_relation(source, target, bindings) {
            return result;
        }
        let occurrence = self.relation_current_occurrence();
        if let Some(result) = self.try_relation_projection(source, target, bindings, occurrence) {
            return result;
        }
        if source == target {
            if let Some(result) = self.try_identical_open_program_result(source, bindings) {
                return result;
            }
            return assignable(bindings);
        }
        let graph = self.graph();
        let budget_limit: u64 = (graph.node_count() as u64).saturating_mul(10).max(4096);
        let mut budget_used: u64 = 0;
        let mut work: Vec<RelateWork> = Vec::new();
        let mut results: Vec<RelationResult> = Vec::new();
        work.push(RelateWork::Expand(source, target));
        while let Some(item) = work.pop() {
            budget_used = budget_used.saturating_add(1);
            if budget_used > budget_limit {
                let cap = RecursionOrBudgetCap {
                    kind: crate::semantic_query::BudgetExceededKind::RelationBudget,
                    limit: budget_limit as u32,
                };
                let mut txn = self.dispatch_txn.borrow_mut();
                if let Some(depth) = txn.reentry().depth().checked_sub(1) {
                    txn.reentry_mut().note_budget_edge(depth, cap);
                }
                return RelationResult::Unknown;
            }
            match item {
                RelateWork::Expand(s, t) => {
                    self.expand_pair(s, t, bindings, &mut work, &mut results);
                }
                RelateWork::Eval(s, t) => {
                    if self.relation_eval_requires_canonical_frame(s, t) {
                        results.push(self.relate_member(s, t, bindings, InferPosition::Covariant));
                    } else {
                        self.expand_pair(s, t, bindings, &mut work, &mut results);
                    }
                }
                RelateWork::ReduceAnd(n) => {
                    let combined = reduce_and_from_results(&mut results, n);
                    results.push(combined);
                }
            }
        }
        results.pop().unwrap_or(RelationResult::Unknown)
    }

    fn relation_eval_requires_canonical_frame(
        &self,
        source: SemanticNodeId,
        target: SemanticNodeId,
    ) -> bool {
        let graph = self.graph();
        [source, target].into_iter().any(|node| {
            matches!(
                graph.node_data(node).as_deref(),
                Some(SemanticNodeData::Conditional { .. })
            )
        })
    }

    /// Expand a single relate pair into direct result(s) or sub-work
    /// items. Pushes exactly one net result onto `results` by the time all
    /// sub-work drains.
    fn expand_pair(
        &self,
        source: SemanticNodeId,
        target: SemanticNodeId,
        bindings: &mut Vec<InferBinding>,
        work: &mut Vec<RelateWork>,
        results: &mut Vec<RelationResult>,
    ) {
        // Program recognition precedes the identity shortcut, structural
        // shortcuts, inference deposits, and distribution — identical to the
        // root protocol. An open program is never accepted on node identity.
        let graph = self.graph();
        let source_data = match graph.node_data(source) {
            Some(d) => d,
            None => {
                results.push(RelationResult::Unknown);
                return;
            }
        };
        let target_data = match graph.node_data(target) {
            Some(d) => d,
            None => {
                results.push(RelationResult::Unknown);
                return;
            }
        };

        // ── Alias: unwrap transparently on either side ─────────────────
        if let SemanticNodeData::Alias(inner) = &*source_data {
            let inner = *inner;
            drop(source_data);
            drop(target_data);
            work.push(RelateWork::Eval(inner, target));
            return;
        }
        if let SemanticNodeData::Alias(inner) = &*target_data {
            let inner = *inner;
            drop(source_data);
            drop(target_data);
            work.push(RelateWork::Eval(source, inner));
            return;
        }

        // ── Object-spread programs are formulas: distributed and aliased
        //    program operands relate through the SAME protocol as the root
        //    (there is no second matcher). ───────────────────────────────
        if let Some(result) = self.try_object_spread_program_relation(source, target, bindings) {
            results.push(result);
            return;
        }

        let occurrence = self.relation_current_occurrence();
        if let Some(result) = self.try_relation_projection(source, target, bindings, occurrence) {
            results.push(result);
            return;
        }
        if source == target {
            if let Some(result) = self.try_identical_open_program_result(source, bindings) {
                results.push(result);
                return;
            }
            results.push(assignable(bindings));
            return;
        }

        // ── MergedDecl: reduce to its peer-merged object surface ───────
        if let SemanticNodeData::MergedDecl { contributors } = &*source_data {
            let contributors = contributors.clone();
            drop(source_data);
            drop(target_data);
            let merged = super::walk::reduce_merged_decl_with_graph(graph, &contributors);
            work.push(RelateWork::Eval(merged, target));
            return;
        }
        if let SemanticNodeData::MergedDecl { contributors } = &*target_data {
            let contributors = contributors.clone();
            drop(source_data);
            drop(target_data);
            let merged = super::walk::reduce_merged_decl_with_graph(graph, &contributors);
            work.push(RelateWork::Eval(source, merged));
            return;
        }

        // ── Top / bottom + error-type wildcard ─────────────────────────
        match (&*source_data, &*target_data) {
            (SemanticNodeData::Opaque(err), _) if err.is_error_type() => {
                results.push(assignable(bindings));
                return;
            }
            (_, SemanticNodeData::Opaque(err)) if err.is_error_type() => {
                results.push(assignable(bindings));
                return;
            }
            (SemanticNodeData::Primitive(PrimitiveKind::Never), _) => {
                results.push(assignable(bindings));
                return;
            }
            (_, SemanticNodeData::Primitive(PrimitiveKind::Unknown)) => {
                results.push(assignable(bindings));
                return;
            }
            (SemanticNodeData::Primitive(PrimitiveKind::Any), _) => {
                results.push(assignable(bindings));
                return;
            }
            (_, SemanticNodeData::Primitive(PrimitiveKind::Any)) => {
                results.push(assignable(bindings));
                return;
            }
            (_, SemanticNodeData::Primitive(PrimitiveKind::Never)) => {
                results.push(RelationResult::NotAssignable);
                return;
            }
            _ => {}
        }

        // ── Strict-family behavioral branch (RI-10): with
        //    `strictNullChecks` OFF, `null` / `undefined` are assignable
        //    to every remaining target (`never` already returned above). ──
        {
            let strict = self
                .dispatch_txn
                .borrow()
                .relation
                .strict
                .unwrap_or(StrictFamilyConfig::TS_STRICT);
            if !strict.strict_null_checks
                && matches!(
                    &*source_data,
                    SemanticNodeData::Primitive(PrimitiveKind::Null | PrimitiveKind::Undefined)
                )
            {
                results.push(assignable(bindings));
                return;
            }
        }

        // ── Type parameters: call-owned sessions bind their exact declared
        // parameter nodes; otherwise Unknown unless identical. Runs BEFORE
        // the deferred-shell arm: a deposit records the source node AS the
        // inference candidate, so a carrier source (`DeclRef` /
        // `InstantiationRef` — an interface-typed argument against a naked
        // binder) deposits verbatim and resolves at the bound's own demand
        // points, exactly like any other candidate. ──────────────────────
        if matches!(&*source_data, SemanticNodeData::TypeParam { .. })
            || matches!(&*target_data, SemanticNodeData::TypeParam { .. })
        {
            if self.relation_session_active() {
                let deposited = match occurrence.variance {
                    VariancePhase::Covariant | VariancePhase::Invariant => {
                        matches!(&*target_data, SemanticNodeData::TypeParam { .. })
                            && self.relation_deposit(target, source, occurrence)
                    }
                    VariancePhase::Contravariant => {
                        matches!(&*source_data, SemanticNodeData::TypeParam { .. })
                            && self.relation_deposit(source, target, occurrence)
                    }
                };
                if deposited {
                    results.push(assignable(bindings));
                    return;
                }
            }
            results.push(RelationResult::Unknown);
            return;
        }

        // ── Deferred shells on either side → Unknown ───────────────────
        if is_deferred(&source_data) || is_deferred(&target_data) {
            results.push(RelationResult::Unknown);
            return;
        }

        // ── Remaining opaque carriers → Unknown ────────────────────────
        if matches!(&*source_data, SemanticNodeData::Opaque(_))
            || matches!(&*target_data, SemanticNodeData::Opaque(_))
        {
            results.push(RelationResult::Unknown);
            return;
        }

        // ── Infer: bind through the active session (RI-6); without one,
        //    defensive Unknown. ──────────────────────────────────────────
        match occurrence.variance {
            VariancePhase::Covariant | VariancePhase::Invariant => {
                if let SemanticNodeData::Infer { .. } = &*target_data {
                    if self.relation_session_active()
                        && self.relation_deposit(target, source, occurrence)
                    {
                        results.push(assignable(bindings));
                    } else {
                        results.push(RelationResult::Unknown);
                    }
                    return;
                }
                if matches!(&*source_data, SemanticNodeData::Infer { .. }) {
                    results.push(RelationResult::Unknown);
                    return;
                }
            }
            VariancePhase::Contravariant => {
                if let SemanticNodeData::Infer { .. } = &*source_data {
                    if self.relation_session_active()
                        && self.relation_deposit(source, target, occurrence)
                    {
                        results.push(assignable(bindings));
                    } else {
                        results.push(RelationResult::Unknown);
                    }
                    return;
                }
                if matches!(&*target_data, SemanticNodeData::Infer { .. }) {
                    results.push(RelationResult::Unknown);
                    return;
                }
            }
        }

        // ── InferRef: an in-scope infer REFERENCE that reached the relate
        //    unsubstituted is undecidable (it is never a deposit target —
        //    only the declaration site binds). ─────────────────────────────
        if matches!(&*source_data, SemanticNodeData::InferRef { .. })
            || matches!(&*target_data, SemanticNodeData::InferRef { .. })
        {
            results.push(RelationResult::Unknown);
            return;
        }

        // ── Union/Intersection distribution ────────────────────────────
        if let SemanticNodeData::Union(members) = &*source_data {
            let members = Arc::clone(members);
            drop(source_data);
            drop(target_data);
            distribute_and(work, results, &members, |m| (*m, target));
            return;
        }
        if let SemanticNodeData::Union(members) = &*target_data {
            let members = Arc::clone(members);
            drop(source_data);
            drop(target_data);
            let alternatives: Vec<_> = members.iter().map(|member| (source, *member)).collect();
            results.push(self.relate_union_target_alternatives(
                &alternatives,
                bindings,
                InferPosition::Covariant,
            ));
            return;
        }
        if let SemanticNodeData::Intersection(members) = &*source_data {
            let members = Arc::clone(members);
            drop(source_data);
            drop(target_data);
            let alternatives: Vec<_> = members.iter().map(|member| (*member, target)).collect();
            results.push(self.relate_pair_alternatives(
                &alternatives,
                bindings,
                InferPosition::Covariant,
            ));
            return;
        }
        if let SemanticNodeData::Intersection(members) = &*target_data {
            let members = Arc::clone(members);
            drop(source_data);
            drop(target_data);
            distribute_and(work, results, &members, |m| (source, *m));
            return;
        }

        // ── `object` nonprimitive target: every object-like source
        //    (surface / array / tuple / bare signature) is assignable —
        //    the TS `object` semantics. Non-object sources fall through to
        //    the primitive/literal arms below (which reject them). ────────
        if matches!(
            &*target_data,
            SemanticNodeData::Primitive(PrimitiveKind::Object)
        ) && matches!(
            &*source_data,
            SemanticNodeData::Object(_)
                | SemanticNodeData::Array { .. }
                | SemanticNodeData::Tuple { .. }
                | SemanticNodeData::Signature { .. }
        ) {
            results.push(assignable(bindings));
            return;
        }

        // ── Primitives / literals ──────────────────────────────────────
        if let (SemanticNodeData::Primitive(s), SemanticNodeData::Primitive(t)) =
            (&*source_data, &*target_data)
        {
            results.push(relate_primitives(*s, *t, bindings));
            return;
        }
        if let (SemanticNodeData::Literal(lit), SemanticNodeData::Primitive(prim)) =
            (&*source_data, &*target_data)
        {
            results.push(relate_literal_to_primitive(lit, *prim, bindings));
            return;
        }
        if let (SemanticNodeData::Literal(s), SemanticNodeData::Literal(t)) =
            (&*source_data, &*target_data)
        {
            results.push(if literals_equal(s, t) {
                assignable(bindings)
            } else {
                RelationResult::NotAssignable
            });
            return;
        }
        if matches!(&*source_data, SemanticNodeData::Primitive(_))
            && matches!(&*target_data, SemanticNodeData::Literal(_))
        {
            results.push(RelationResult::NotAssignable);
            return;
        }

        // ── Array / Tuple ──────────────────────────────────────────────
        match (&*source_data, &*target_data) {
            (
                SemanticNodeData::Array {
                    element: s_el,
                    readonly: s_ro,
                },
                SemanticNodeData::Array {
                    element: t_el,
                    readonly: t_ro,
                },
            ) => {
                let (s_el, s_ro, t_el, t_ro) = (*s_el, *s_ro, *t_el, *t_ro);
                drop(source_data);
                drop(target_data);
                if !t_ro && s_ro {
                    results.push(RelationResult::NotAssignable);
                    return;
                }
                // An INFERENCE element position under an active session is
                // covariant-only: the forward arm's deposit IS the binding,
                // and an invariant reverse arm against the binding node
                // (`Infer ≤ element`, `T ≤ element`) is undecidable and
                // would defer the whole pattern. This covers `infer`
                // declarations AND the session's own declared type-parameter
                // binders (a call argument against a generic `T[]`
                // parameter). Other mutable arrays KEEP the invariant
                // bidirectional check.
                let inference_element = match occurrence.variance {
                    VariancePhase::Covariant | VariancePhase::Invariant => t_el,
                    VariancePhase::Contravariant => s_el,
                };
                let infer_element = self.relation_session_active()
                    && (matches!(
                        graph.node_data(inference_element).as_deref(),
                        Some(SemanticNodeData::Infer { .. })
                    ) || (matches!(
                        graph.node_data(inference_element).as_deref(),
                        Some(SemanticNodeData::TypeParam { .. })
                    ) && self.relation_session_declares(inference_element))
                        || self.relation_subtree_contains_projection(inference_element));
                if t_ro || s_ro || infer_element {
                    work.push(RelateWork::Eval(s_el, t_el));
                } else {
                    let forward = vec![
                        RelateWork::Eval(s_el, t_el),
                        RelateWork::Eval(t_el, s_el),
                        RelateWork::ReduceAnd(2),
                    ];
                    push_forward_work(work, forward);
                }
                return;
            }
            (
                SemanticNodeData::Tuple {
                    elements: s_els,
                    readonly: s_ro,
                },
                SemanticNodeData::Tuple {
                    elements: t_els,
                    readonly: t_ro,
                },
            ) => {
                let s_els = Arc::clone(s_els);
                let t_els = Arc::clone(t_els);
                let s_ro = *s_ro;
                let t_ro = *t_ro;
                drop(source_data);
                drop(target_data);
                if !t_ro && s_ro {
                    results.push(RelationResult::NotAssignable);
                    return;
                }
                // Tuple-inference rest tail (RI-6 in-scope): a trailing
                // `...infer Rest` element binds the remaining source
                // elements as a tuple through the active session.
                let rest_on_source = matches!(occurrence.variance, VariancePhase::Contravariant);
                let session_rest = if self.relation_session_active() {
                    let inference_elements = if rest_on_source { &s_els } else { &t_els };
                    inference_elements.iter().position(|e| {
                        e.rest
                            && matches!(
                                graph.node_data(e.value).as_deref(),
                                Some(SemanticNodeData::Infer { .. })
                            )
                    })
                } else {
                    None
                };
                let required_source_len = s_els.iter().filter(|e| !e.optional && !e.rest).count();
                let required_target_len = t_els.iter().filter(|e| !e.optional && !e.rest).count();
                let required_lengths_compatible = if session_rest.is_some() {
                    let (required_inference_len, required_remainder_len) = if rest_on_source {
                        (required_source_len, required_target_len)
                    } else {
                        (required_target_len, required_source_len)
                    };
                    required_remainder_len >= required_inference_len
                } else {
                    required_source_len >= required_target_len
                };
                if !required_lengths_compatible {
                    results.push(RelationResult::NotAssignable);
                    return;
                }
                let mut pairs: Vec<(SemanticNodeId, SemanticNodeId)> = Vec::new();
                if let Some(rest_index) = session_rest {
                    let (inference_elements, remainder_elements) = if rest_on_source {
                        (&s_els, &t_els)
                    } else {
                        (&t_els, &s_els)
                    };
                    let infer_element = inference_elements[rest_index].value;
                    let prefix = &inference_elements[..rest_index];
                    let suffix = &inference_elements[rest_index + 1..];
                    let required_prefix_len =
                        prefix.iter().filter(|element| !element.optional).count();
                    let required_suffix_len =
                        suffix.iter().filter(|element| !element.optional).count();
                    if remainder_elements.len() < required_prefix_len + required_suffix_len {
                        results.push(RelationResult::NotAssignable);
                        return;
                    }
                    // Reserve the full fixed suffix when present; when the
                    // concrete tuple is shorter, only optional trailing suffix
                    // slots may disappear. The same rule applies to the fixed
                    // prefix before the variadic capture.
                    let suffix_len = suffix
                        .len()
                        .min(remainder_elements.len().saturating_sub(required_prefix_len));
                    let prefix_len = prefix
                        .len()
                        .min(remainder_elements.len().saturating_sub(suffix_len));
                    if prefix[prefix_len..].iter().any(|element| !element.optional)
                        || suffix[suffix_len..].iter().any(|element| !element.optional)
                    {
                        results.push(RelationResult::NotAssignable);
                        return;
                    }
                    let remainder_end = remainder_elements.len() - suffix_len;
                    let remainder: Vec<crate::semantic_query::TupleElement> = remainder_elements
                        .iter()
                        .skip(prefix_len)
                        .take(remainder_end - prefix_len)
                        .cloned()
                        .collect();
                    let remainder_tuple = graph.intern_node(SemanticNodeData::Tuple {
                        elements: Arc::from(remainder.into_boxed_slice()),
                        readonly: if rest_on_source { t_ro } else { s_ro },
                    });
                    if !self.relation_deposit(infer_element, remainder_tuple, occurrence) {
                        results.push(RelationResult::Unknown);
                        return;
                    }
                    for position in 0..prefix_len {
                        pairs.push((s_els[position].value, t_els[position].value));
                    }
                    for offset in 0..suffix_len {
                        let inference_position = rest_index + 1 + offset;
                        let remainder_position = remainder_elements.len() - suffix_len + offset;
                        if rest_on_source {
                            pairs.push((
                                inference_elements[inference_position].value,
                                remainder_elements[remainder_position].value,
                            ));
                        } else {
                            pairs.push((
                                remainder_elements[remainder_position].value,
                                inference_elements[inference_position].value,
                            ));
                        }
                    }
                } else {
                    pairs.extend(
                        s_els
                            .iter()
                            .zip(t_els.iter())
                            .map(|(source, target)| (source.value, target.value)),
                    );
                }
                if pairs.is_empty() {
                    results.push(assignable(bindings));
                    return;
                }
                // Tuple assignability is covariant elementwise — mutable
                // tuples included — exactly as TypeScript relates tuples:
                // `[1, 1]` satisfies `[number, number]`, and a generic
                // element (`[T, T]`) binds through the forward deposit. A
                // reverse (`target-element ≤ source-element`) leg would
                // reject literal-element sources and defer inference
                // elements, so no element pair evaluates one.
                let mut forward: Vec<RelateWork> = Vec::with_capacity(pairs.len() + 1);
                for (source_element, target_element) in pairs.iter().copied() {
                    forward.push(RelateWork::Eval(source_element, target_element));
                }
                if pairs.len() > 1 {
                    forward.push(RelateWork::ReduceAnd(pairs.len() as u32));
                }
                push_forward_work(work, forward);
                return;
            }
            // Tuple ≤ Array (readonly): elementwise check.
            (
                SemanticNodeData::Tuple {
                    elements: s_els,
                    readonly: s_ro,
                },
                SemanticNodeData::Array {
                    element: t_el,
                    readonly: t_ro,
                },
            ) => {
                let s_els = Arc::clone(s_els);
                let s_ro = *s_ro;
                let t_el = *t_el;
                let t_ro = *t_ro;
                drop(source_data);
                drop(target_data);
                if !t_ro && s_ro {
                    results.push(RelationResult::NotAssignable);
                    return;
                }
                if s_els.is_empty() {
                    results.push(assignable(bindings));
                    return;
                }
                let mut forward: Vec<RelateWork> = Vec::with_capacity(s_els.len() + 1);
                for s in s_els.iter() {
                    forward.push(RelateWork::Eval(s.value, t_el));
                }
                if s_els.len() > 1 {
                    forward.push(RelateWork::ReduceAnd(s_els.len() as u32));
                }
                push_forward_work(work, forward);
                return;
            }
            _ => {}
        }

        // ── Direct signatures: relate through the SHARED kind-aware
        //    signature surface — same kind relates the parameter/return
        //    structure; a call signature NEVER satisfies a construct
        //    signature or vice versa. ───────────────────────────────────
        if let (
            SemanticNodeData::Signature {
                kind: s_kind,
                params: s_params,
                return_type: s_ret,
                ..
            },
            SemanticNodeData::Signature {
                kind: t_kind,
                params: t_params,
                return_type: t_ret,
                ..
            },
        ) = (&*source_data, &*target_data)
        {
            if s_kind != t_kind {
                drop(source_data);
                drop(target_data);
                results.push(RelationResult::NotAssignable);
                return;
            }
            let s_params = Arc::clone(s_params);
            let t_params = Arc::clone(t_params);
            let s_ret = *s_ret;
            let t_ret = *t_ret;
            drop(source_data);
            drop(target_data);
            results.push(self.relate_function(&s_params, s_ret, &t_params, t_ret, bindings));
            return;
        }

        // ── Object structural (with heritage via SurfaceView) ──────────
        if let (SemanticNodeData::Object(s_surf), SemanticNodeData::Object(t_surf)) =
            (&*source_data, &*target_data)
        {
            let s_surf = s_surf.clone();
            let t_surf = t_surf.clone();
            drop(source_data);
            drop(target_data);
            results.push(self.relate_objects(&s_surf, &t_surf, bindings));
            return;
        }

        // ── Direct signature source vs Object target: every target
        //    signature bucket must be satisfied by a MATCHING-KIND source
        //    signature (the shared kind-aware signature surface — a bare
        //    signature exposes exactly its one bucket). ─────────────────
        if let (SemanticNodeData::Signature { kind, .. }, SemanticNodeData::Object(t_surf)) =
            (&*source_data, &*target_data)
        {
            let s_kind = *kind;
            let t_surf = t_surf.clone();
            drop(source_data);
            drop(target_data);
            results.push(self.relate_signature_source_to_object(source, s_kind, &t_surf, bindings));
            return;
        }

        // ── Object source vs direct signature target: the source's
        //    MATCHING-KIND signature group must satisfy the target
        //    signature (the mirror direction of the surface rule). ──────
        if let (SemanticNodeData::Object(s_surf), SemanticNodeData::Signature { kind, .. }) =
            (&*source_data, &*target_data)
        {
            let t_kind = *kind;
            let s_surf = s_surf.clone();
            drop(source_data);
            drop(target_data);
            results.push(self.relate_object_to_signature(&s_surf, t_kind, target, bindings));
            return;
        }

        // Different concrete kinds → NotAssignable.
        results.push(RelationResult::NotAssignable);
    }

    // ──────────────────────────────────────────────────────────────────
    // Identity-carrier unwrap + the Object-vs-Record arm (unchanged
    // shapes; recursion re-enters the authority)
    // ──────────────────────────────────────────────────────────────────

    /// Instantiate a decl identity carrier into its concrete shape for
    /// relation dispatch through `execute(Instantiate{…})` — the shared
    /// dispatch, never a private instantiation path.
    pub(super) fn unwrap_identity_carrier_one_step(
        &self,
        id: SemanticNodeId,
    ) -> IdentityCarrierUnwrap {
        let graph = self.graph();
        let Some(data) = graph.node_data(id) else {
            return IdentityCarrierUnwrap::Unresolvable;
        };
        let (identity, args): (DeclIdentity, Arc<[SemanticNodeId]>) = match &*data {
            SemanticNodeData::Alias(inner) => return IdentityCarrierUnwrap::Concrete(*inner),
            SemanticNodeData::MergedDecl { contributors } => {
                return IdentityCarrierUnwrap::Concrete(
                    super::walk::reduce_merged_decl_with_graph(graph, contributors),
                );
            }
            SemanticNodeData::Opaque(QueryError::DeclPlaceholder {
                canonical_id,
                owner,
                name,
                whole_hash,
            }) => (
                DeclIdentity {
                    canonical_id: Arc::clone(canonical_id),
                    owner: *owner,
                    whole_hash: *whole_hash,
                    decl_name: Arc::clone(name),
                },
                Arc::from(Vec::<SemanticNodeId>::new().into_boxed_slice()),
            ),
            SemanticNodeData::DeclRef { identity } => (
                identity.clone(),
                Arc::from(Vec::<SemanticNodeId>::new().into_boxed_slice()),
            ),
            SemanticNodeData::InstantiationRef { base, args } => (base.clone(), Arc::clone(args)),
            _ => return IdentityCarrierUnwrap::Concrete(id),
        };
        drop(data);
        let transit = ProjectionReductionContext::structural_transit();
        let unwrapped = match self.execute_type_node(SemanticQueryKey::Instantiate(
            crate::semantic_query::InstantiateKey::new(
                self.type_slot_for(
                    Arc::clone(&identity.canonical_id),
                    identity.owner,
                    Arc::clone(&identity.decl_name),
                ),
                args,
                self.instantiate_context_for(&identity.canonical_id, transit),
            ),
        )) {
            QueryResult::Value(SemanticQueryOutput {
                value: unwrapped, ..
            }) => unwrapped,
            _ => return IdentityCarrierUnwrap::Unresolvable,
        };
        if unwrapped == id {
            IdentityCarrierUnwrap::Unresolvable
        } else {
            IdentityCarrierUnwrap::Concrete(unwrapped)
        }
    }

    /// Fully unwrap an identity carrier for ordinary structural relation.
    pub(super) fn unwrap_identity_carrier_for_relation(
        &self,
        id: SemanticNodeId,
    ) -> IdentityCarrierUnwrap {
        let graph = self.graph();
        let transit = ProjectionReductionContext::structural_transit();
        let mut current = id;
        let mut seen = FxHashSet::default();
        while seen.insert(current) {
            let Some(data) = graph.node_data(current) else {
                return IdentityCarrierUnwrap::Unresolvable;
            };
            let (identity, args): (DeclIdentity, Arc<[SemanticNodeId]>) = match &*data {
                SemanticNodeData::Alias(inner) => {
                    current = *inner;
                    continue;
                }
                SemanticNodeData::MergedDecl { contributors } => {
                    let contributors = Arc::clone(contributors);
                    drop(data);
                    current = super::walk::reduce_merged_decl_with_graph(graph, &contributors);
                    continue;
                }
                SemanticNodeData::Opaque(QueryError::DeclPlaceholder {
                    canonical_id,
                    owner,
                    name,
                    whole_hash,
                }) => (
                    DeclIdentity {
                        canonical_id: Arc::clone(canonical_id),
                        owner: *owner,
                        whole_hash: *whole_hash,
                        decl_name: Arc::clone(name),
                    },
                    Arc::from(Vec::<SemanticNodeId>::new().into_boxed_slice()),
                ),
                // A lowering-time self-reference sentinel (`interface Num {
                // compareTo(other: Num): number }` — the `Num` inside its
                // own body) names its declaration but not its file; the
                // node's origin scope, recorded at intern time, supplies
                // it. Resolving through the shared `Instantiate` dispatch
                // makes the sentinel relate exactly like the `DeclRef` the
                // same reference lowers to from any OTHER file position; a
                // genuinely in-flight cycle re-enters the relation whose
                // identity is already open and closes coinductively. A
                // scope-less sentinel stays concrete (fail-closed Unknown
                // downstream, never a fabricated verdict).
                SemanticNodeData::Opaque(QueryError::RecursiveRef { name }) => {
                    let Some(crate::semantic_query::NodeScopeId::File {
                        canonical_id,
                        owner,
                        whole_hash,
                        ..
                    }) = graph.node_scope(current)
                    else {
                        return IdentityCarrierUnwrap::Concrete(current);
                    };
                    (
                        DeclIdentity {
                            canonical_id,
                            owner,
                            whole_hash,
                            decl_name: Arc::clone(name),
                        },
                        Arc::from(Vec::<SemanticNodeId>::new().into_boxed_slice()),
                    )
                }
                SemanticNodeData::DeclRef { identity } => (
                    identity.clone(),
                    Arc::from(Vec::<SemanticNodeId>::new().into_boxed_slice()),
                ),
                SemanticNodeData::InstantiationRef { base, args } => {
                    (base.clone(), Arc::clone(args))
                }
                _ => return IdentityCarrierUnwrap::Concrete(current),
            };
            drop(data);
            let unwrapped = match self.execute_type_node(SemanticQueryKey::Instantiate(
                crate::semantic_query::InstantiateKey::new(
                    self.type_slot_for(
                        Arc::clone(&identity.canonical_id),
                        identity.owner,
                        Arc::clone(&identity.decl_name),
                    ),
                    args,
                    self.instantiate_context_for(&identity.canonical_id, transit),
                ),
            )) {
                QueryResult::Value(SemanticQueryOutput {
                    value: unwrapped, ..
                }) => self
                    .evaluate_deferred_semantic_node_with_context(unwrapped, transit)
                    .into_active_query_build_node(self),
                _ => return IdentityCarrierUnwrap::Unresolvable,
            };
            if unwrapped == current {
                return IdentityCarrierUnwrap::Unresolvable;
            }
            current = unwrapped;
        }
        IdentityCarrierUnwrap::Unresolvable
    }

    /// Source-side declaration identity carrier with Object body against
    /// a target-side Record-shaped Object.
    fn try_object_vs_record_relation(
        &self,
        source: SemanticNodeId,
        target: SemanticNodeId,
        bindings: &mut Vec<InferBinding>,
    ) -> Option<RelationResult> {
        let graph = self.graph();
        let source_data = graph.node_data(source)?;
        let identity = match &*source_data {
            SemanticNodeData::Opaque(QueryError::DeclPlaceholder {
                canonical_id,
                owner,
                name,
                whole_hash,
            }) => Some(DeclIdentity {
                canonical_id: Arc::clone(canonical_id),
                owner: *owner,
                whole_hash: *whole_hash,
                decl_name: Arc::clone(name),
            }),
            SemanticNodeData::DeclRef { identity } => Some(identity.clone()),
            SemanticNodeData::Object(_) => None,
            _ => return None,
        };
        drop(source_data);

        let target_record = self.record_target_shape(target)?;
        let transit = ProjectionReductionContext::structural_transit();
        let unwrapped = match identity {
            None => source,
            Some(identity) => match self.execute_type_node(SemanticQueryKey::Instantiate(
                crate::semantic_query::InstantiateKey::new(
                    self.type_slot_for(
                        Arc::clone(&identity.canonical_id),
                        identity.owner,
                        Arc::clone(&identity.decl_name),
                    ),
                    Arc::from(Vec::<SemanticNodeId>::new().into_boxed_slice()),
                    self.instantiate_context_for(&identity.canonical_id, transit),
                ),
            )) {
                QueryResult::Value(SemanticQueryOutput { value: id, .. }) => self
                    .evaluate_deferred_semantic_node_with_context(id, transit)
                    .into_active_query_build_node(self),
                _ => return Some(RelationResult::Unknown),
            },
        };
        let source_view = match graph.node_data(unwrapped).as_deref() {
            Some(SemanticNodeData::Object(view)) => view.clone(),
            _ => return None,
        };

        Some(match target_record {
            RecordTargetShape::LiteralKey(target_view) => {
                self.relate_objects(&source_view, &target_view, bindings)
            }
            RecordTargetShape::GenericKey {
                key_type,
                value_type,
            } => self.relate_object_as_record(&source_view, key_type, value_type, bindings),
        })
    }

    /// Returns `Some(RecordTargetShape)` when `target` normalises to a
    /// Record-shaped `Object(SurfaceView)`.
    fn record_target_shape(&self, target: SemanticNodeId) -> Option<RecordTargetShape> {
        let graph = self.graph();
        let oracle_demand = ProjectionReductionContext::structural_transit_with_mode(
            crate::semantic_query::ProjectionMode::Navigate,
        );
        let mut normalised = self
            .evaluate_deferred_semantic_node_with_context(target, oracle_demand)
            .into_active_query_build_node(self);
        if let Some(SemanticNodeData::InstantiationRef { base, args }) =
            graph.node_data(normalised).as_deref()
        {
            let owner_canonical = Arc::clone(&base.canonical_id);
            let slot = self.type_slot_for(
                Arc::clone(&base.canonical_id),
                base.owner,
                Arc::clone(&base.decl_name),
            );
            let args: Arc<[SemanticNodeId]> = Arc::from(
                args.iter()
                    .map(|arg| {
                        self.evaluate_deferred_semantic_node_with_context(*arg, oracle_demand)
                            .into_active_query_build_node(self)
                    })
                    .collect::<Vec<_>>()
                    .into_boxed_slice(),
            );
            if let QueryResult::Value(SemanticQueryOutput { value: id, .. }) = self
                .execute_type_node(SemanticQueryKey::Instantiate(
                    crate::semantic_query::InstantiateKey::new(
                        slot,
                        args,
                        self.instantiate_context_for(&owner_canonical, oracle_demand),
                    ),
                ))
            {
                normalised = self
                    .evaluate_deferred_semantic_node_with_context(id, oracle_demand)
                    .into_active_query_build_node(self);
            }
        }
        if let Some(SemanticNodeData::Mapped { mapper, .. }) =
            graph.node_data(normalised).as_deref()
        {
            if mapper.name_remap.is_none()
                && matches!(
                    mapper.optionality,
                    crate::semantic_query::OptionalityMod::Keep
                )
                && !self.subtree_references_node(mapper.value_expr, mapper.parameter_node)
            {
                let key_space = mapper.key_space;
                let value_type = mapper.value_expr;
                let key_type = self
                    .evaluate_deferred_semantic_node_with_context(key_space, oracle_demand)
                    .into_active_query_build_node(self);
                return Some(RecordTargetShape::GenericKey {
                    key_type,
                    value_type,
                });
            }
        }
        let data = graph.node_data(normalised)?;
        match &*data {
            SemanticNodeData::Object(view)
                if view.call_signatures.is_empty() && view.construct_signatures.is_empty() =>
            {
                let closed = view.closed();
                let members = closed.complete_members();
                if members.is_empty() && view.index_signatures.len() == 1 {
                    let ix = &view.index_signatures[0];
                    Some(RecordTargetShape::GenericKey {
                        key_type: ix.key_type,
                        value_type: ix.value_type,
                    })
                } else if !members.is_empty() && view.index_signatures.is_empty() {
                    Some(RecordTargetShape::LiteralKey(view.clone()))
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    /// Relate an Object surface against a Record<K, V> target.
    fn relate_object_as_record(
        &self,
        source_view: &SurfaceView,
        key_type: SemanticNodeId,
        value_type: SemanticNodeId,
        bindings: &mut Vec<InferBinding>,
    ) -> RelationResult {
        let graph = self.graph();
        let key_data = match graph.node_data(key_type) {
            Some(d) => d,
            None => return RelationResult::Unknown,
        };
        let required_keys: Vec<Arc<str>> = match &*key_data {
            SemanticNodeData::Literal(LiteralValue::String(s)) => vec![Arc::from(s.as_str())],
            SemanticNodeData::Literal(LiteralValue::Number(n)) => {
                vec![Arc::from(super::build::js_number_to_string(*n).as_str())]
            }
            SemanticNodeData::Union(members) => {
                let members = Arc::clone(members);
                drop(key_data);
                let mut keys: Vec<Arc<str>> = Vec::with_capacity(members.len());
                for member in members.iter() {
                    match graph.node_data(*member).as_deref() {
                        Some(SemanticNodeData::Literal(LiteralValue::String(s))) => {
                            keys.push(Arc::from(s.as_str()));
                        }
                        Some(SemanticNodeData::Literal(LiteralValue::Number(n))) => {
                            keys.push(Arc::from(super::build::js_number_to_string(*n).as_str()));
                        }
                        _ => return RelationResult::Unknown,
                    }
                }
                keys
            }
            SemanticNodeData::Primitive(PrimitiveKind::String | PrimitiveKind::Number) => {
                drop(key_data);
                let mut acc = RelationResult::Assignable {
                    bindings: Arc::from(Vec::new().into_boxed_slice()),
                };
                for member in source_view.positive_members().iter() {
                    let r = self.relate_member(
                        member.value,
                        value_type,
                        bindings,
                        InferPosition::Covariant,
                    );
                    acc = result_and(acc, r);
                    if matches!(acc, RelationResult::NotAssignable) {
                        return RelationResult::NotAssignable;
                    }
                }
                return acc;
            }
            _ => return RelationResult::Unknown,
        };

        let mut acc = RelationResult::Assignable {
            bindings: Arc::from(Vec::new().into_boxed_slice()),
        };
        for key in required_keys {
            let member = match source_view.project_string_key(key.as_ref()) {
                crate::semantic_query::SurfaceKeyProjection::Exact(member) => member,
                crate::semantic_query::SurfaceKeyProjection::AbsentProven => {
                    return RelationResult::NotAssignable;
                }
            };
            let r =
                self.relate_member(member.value, value_type, bindings, InferPosition::Covariant);
            acc = result_and(acc, r);
            if matches!(acc, RelationResult::NotAssignable) {
                return RelationResult::NotAssignable;
            }
        }
        acc
    }

    // ──────────────────────────────────────────────────────────────────
    // Object / function structural predicates (the retired
    // `relation_predicates` recursion sites — now methods re-entering the
    // full-key authority)
    // ──────────────────────────────────────────────────────────────────

    /// Relate two object `SurfaceView`s structurally. Every required
    /// target member must be satisfied by a matching source member (or an
    /// applicable source index signature).
    pub(super) fn relate_objects(
        &self,
        source: &SurfaceView,
        target: &SurfaceView,
        bindings: &mut Vec<InferBinding>,
    ) -> RelationResult {
        let closed_target = target.closed();

        let mut acc = RelationResult::Assignable {
            bindings: Arc::from(Vec::new().into_boxed_slice()),
        };
        for t_prop in closed_target.complete_members() {
            let Some(target_key) = t_prop.key.cloned_known() else {
                return RelationResult::Unknown;
            };
            let prop_result = match source.project_known_key(&target_key) {
                crate::semantic_query::SurfaceKeyProjection::Exact(source_member) => {
                    self.relate_property_pair(source_member, t_prop, bindings)
                }
                crate::semantic_query::SurfaceKeyProjection::AbsentProven => {
                    if let Some(index_result) =
                        self.relate_property_via_source_index(source, t_prop, bindings)
                    {
                        index_result
                    } else if t_prop.optional {
                        assignable(bindings)
                    } else {
                        RelationResult::NotAssignable
                    }
                }
            };
            acc = result_and(acc, prop_result);
            if matches!(acc, RelationResult::NotAssignable) {
                return RelationResult::NotAssignable;
            }
        }
        for t_index in target.index_signatures.iter() {
            let index_result = self.relate_target_index_signature(source, t_index, bindings);
            acc = result_and(acc, index_result);
            if matches!(acc, RelationResult::NotAssignable) {
                return RelationResult::NotAssignable;
            }
        }
        for t_sig in target.call_signatures.iter() {
            let signature_result =
                self.relate_signature_alternatives(&source.call_signatures, *t_sig, bindings);
            acc = result_and(acc, signature_result);
            if matches!(acc, RelationResult::NotAssignable) {
                return RelationResult::NotAssignable;
            }
        }
        for t_sig in target.construct_signatures.iter() {
            let signature_result =
                self.relate_signature_alternatives(&source.construct_signatures, *t_sig, bindings);
            acc = result_and(acc, signature_result);
            if matches!(acc, RelationResult::NotAssignable) {
                return RelationResult::NotAssignable;
            }
        }
        acc
    }

    /// Resolve a member value that is a `RecursiveRef` control sentinel to
    /// its declaration surface through the SHARED dispatch
    /// (`execute(Instantiate)` on the `(declaration_origin, name)` slot —
    /// never a private resolution path). The unfolding cannot spiral: the
    /// resolved referent re-enters [`Self::execute_relate`], whose full
    /// identity is already in flight for a genuine cycle, so the reentry
    /// intercept turns the unfold into the coinductive back-edge
    /// (`Assumed`). An unresolvable referent keeps the sentinel (which
    /// stays `Unknown` — fail-closed, never a fabricated verdict).
    fn resolve_recursive_member_value(
        &self,
        value: SemanticNodeId,
        origin: Option<&Arc<str>>,
    ) -> SemanticNodeId {
        let graph = self.graph();
        let name = match graph.node_data(value).as_deref() {
            Some(SemanticNodeData::Opaque(QueryError::RecursiveRef { name })) => Arc::clone(name),
            _ => return value,
        };
        let Some(origin) = origin else {
            return value;
        };
        let transit = ProjectionReductionContext::structural_transit();
        match self.execute_type_node(SemanticQueryKey::Instantiate(
            crate::semantic_query::InstantiateKey::new(
                self.type_slot_for(
                    Arc::clone(origin),
                    verter_type_expr::TopLevelOwnerId::ordinary_file(),
                    name,
                ),
                Arc::from(Vec::<SemanticNodeId>::new().into_boxed_slice()),
                self.instantiate_context_for(origin, transit),
            ),
        )) {
            QueryResult::Value(SemanticQueryOutput {
                value: resolved, ..
            }) => {
                let resolved = self
                    .evaluate_deferred_semantic_node_with_context(resolved, transit)
                    .into_active_query_build_node(self);
                match graph.node_data(resolved).as_deref() {
                    // A referent that failed to materialise keeps the
                    // sentinel (Unknown), never a half-resolved carrier.
                    Some(SemanticNodeData::Opaque(_)) | None => value,
                    _ => resolved,
                }
            }
            _ => value,
        }
    }

    pub(super) fn relate_property_pair(
        &self,
        source: &crate::semantic_query::SurfaceMember,
        target: &crate::semantic_query::SurfaceMember,
        bindings: &mut Vec<InferBinding>,
    ) -> RelationResult {
        // A property `readonly` modifier is not part of assignability in
        // either direction: `{ readonly a: T }` and `{ a: T }` relate both
        // ways. The `readonly` gates that DO apply live on the array and
        // tuple arms (a readonly array / tuple is not assignable to a
        // mutable one) and on index signatures — distinct rules over
        // distinct carriers, not this member pair.
        //
        // Optional-to-required: a source member that may be ABSENT cannot
        // satisfy a required target member under `strictNullChecks` (the
        // optional's implied `undefined` does not relate to the required
        // value type). With strict null checks relaxed the implied
        // `undefined` collapses and the pair relates on the value types
        // alone (RI-10 behavioral branch).
        if !target.optional && source.optional {
            let strict = self
                .dispatch_txn
                .borrow()
                .relation
                .strict
                .unwrap_or(StrictFamilyConfig::TS_STRICT);
            if strict.strict_null_checks {
                return RelationResult::NotAssignable;
            }
        }
        // A `RecursiveRef` member value rebinds to its declaration surface
        // through the shared dispatch so a genuinely recursive type
        // re-enters the authority (the coinductive back-edge) instead of
        // dead-ending on the sentinel.
        let source_value =
            self.resolve_recursive_member_value(source.value, source.declaration_origin.as_ref());
        let target_value =
            self.resolve_recursive_member_value(target.value, target.declaration_origin.as_ref());
        self.relate_member(
            source_value,
            target_value,
            bindings,
            InferPosition::Covariant,
        )
    }

    pub(super) fn relate_property_via_source_index(
        &self,
        source: &SurfaceView,
        target_prop: &crate::semantic_query::SurfaceMember,
        bindings: &mut Vec<InferBinding>,
    ) -> Option<RelationResult> {
        let graph = self.graph();
        let mut matched = false;
        let mut acc = RelationResult::Assignable {
            bindings: Arc::from(Vec::new().into_boxed_slice()),
        };
        for s_index in source.index_signatures.iter() {
            let target_key = target_prop.key.cloned_known()?;
            if !index_signature_applies_to_property(graph, s_index.key_type, &target_key) {
                continue;
            }
            matched = true;
            let r = self.relate_member(
                s_index.value_type,
                target_prop.value,
                bindings,
                InferPosition::Covariant,
            );
            acc = result_and(acc, r);
            if matches!(acc, RelationResult::NotAssignable) {
                return Some(RelationResult::NotAssignable);
            }
        }
        matched.then_some(acc)
    }

    pub(super) fn relate_target_index_signature(
        &self,
        source: &SurfaceView,
        target_index: &crate::semantic_query::IndexSignature,
        bindings: &mut Vec<InferBinding>,
    ) -> RelationResult {
        let graph = self.graph();
        let mut acc = RelationResult::Assignable {
            bindings: Arc::from(Vec::new().into_boxed_slice()),
        };
        for s_index in source.index_signatures.iter() {
            if !index_domains_overlap(graph, s_index.key_type, target_index.key_type) {
                continue;
            }
            let r = self.relate_member(
                s_index.value_type,
                target_index.value_type,
                bindings,
                InferPosition::Covariant,
            );
            acc = result_and(acc, r);
            if matches!(acc, RelationResult::NotAssignable) {
                return RelationResult::NotAssignable;
            }
        }
        for prop in source.positive_members().iter() {
            let Some(property_key) = prop.key.cloned_known() else {
                return RelationResult::Unknown;
            };
            if !index_signature_applies_to_property(graph, target_index.key_type, &property_key) {
                continue;
            }
            let r = self.relate_member(
                prop.value,
                target_index.value_type,
                bindings,
                InferPosition::Covariant,
            );
            acc = result_and(acc, r);
            if matches!(acc, RelationResult::NotAssignable) {
                return RelationResult::NotAssignable;
            }
        }
        if source.has_known_index_signature() && source.index_signatures.is_empty() {
            RelationResult::Unknown
        } else {
            acc
        }
    }

    /// Relate two [`SemanticNodeData::Signature`] shells. Parameter
    /// variance follows the key's policy (RI-10 behavioral branch):
    /// strictly contravariant under `strictFunctionTypes`, bivariant
    /// otherwise (either direction suffices per parameter pair); the
    /// return is covariant.
    pub(super) fn relate_function(
        &self,
        source_params: &[crate::semantic_query::FunctionParam],
        source_return: SemanticNodeId,
        target_params: &[crate::semantic_query::FunctionParam],
        target_return: SemanticNodeId,
        bindings: &mut Vec<InferBinding>,
    ) -> RelationResult {
        let source_required = source_params
            .iter()
            .filter(|p| !p.optional && !p.rest)
            .count();
        let target_required = target_params
            .iter()
            .filter(|p| !p.optional && !p.rest)
            .count();
        if target_required < source_required {
            return RelationResult::NotAssignable;
        }
        let bivariant = {
            let txn = self.dispatch_txn.borrow();
            let strict = txn.relation.strict.unwrap_or(StrictFamilyConfig::TS_STRICT);
            !strict.strict_function_types
        };
        let mut acc = RelationResult::Assignable {
            bindings: Arc::from(Vec::new().into_boxed_slice()),
        };
        for (s_param, t_param) in source_params.iter().zip(target_params.iter()) {
            // Contravariant: target param ≤ source param. Under the
            // bivariant regime either direction discharges the pair.
            let checkpoint = self.relation_session_checkpoint();
            let bindings_len = bindings.len();
            let contravariant = self.relate_member(
                t_param.ty,
                s_param.ty,
                bindings,
                InferPosition::ContravariantParam,
            );
            let pair = if bivariant && !matches!(contravariant, RelationResult::Assignable { .. }) {
                self.relation_session_rollback(&checkpoint);
                bindings.truncate(bindings_len);
                let fallback_checkpoint = self.relation_session_checkpoint();
                let fallback_bindings_len = bindings.len();
                let fallback =
                    self.relate_member(s_param.ty, t_param.ty, bindings, InferPosition::Covariant);
                if !matches!(fallback, RelationResult::Assignable { .. }) {
                    self.relation_session_rollback(&fallback_checkpoint);
                    bindings.truncate(fallback_bindings_len);
                }
                result_or(contravariant, fallback)
            } else {
                contravariant
            };
            acc = result_and(acc, pair);
            if matches!(acc, RelationResult::NotAssignable) {
                return RelationResult::NotAssignable;
            }
        }
        // Covariant return.
        let r = self.relate_member(
            source_return,
            target_return,
            bindings,
            InferPosition::Return,
        );
        result_and(acc, r)
    }

    /// Relate a function source against an object target carrying call
    /// signatures.
    /// A DIRECT signature source against an Object target: required target
    /// MEMBERS reject (a bare signature has none), and every target
    /// signature bucket must be satisfied by the source's single
    /// matching-kind signature — a bucket of the OTHER kind is unmet.
    fn relate_signature_source_to_object(
        &self,
        source_sig: SemanticNodeId,
        source_kind: crate::semantic_query::SignatureKind,
        target: &SurfaceView,
        bindings: &mut Vec<InferBinding>,
    ) -> RelationResult {
        for m in target.positive_members().iter() {
            if !m.optional {
                return RelationResult::NotAssignable;
            }
        }
        let mut acc = RelationResult::Assignable {
            bindings: Arc::from(Vec::new().into_boxed_slice()),
        };
        for (bucket_kind, bucket) in [
            (
                crate::semantic_query::SignatureKind::Call,
                &target.call_signatures,
            ),
            (
                crate::semantic_query::SignatureKind::Construct,
                &target.construct_signatures,
            ),
        ] {
            for t_sig in bucket.iter() {
                if bucket_kind != source_kind {
                    return RelationResult::NotAssignable;
                }
                let r = self.relate_member(source_sig, *t_sig, bindings, InferPosition::Covariant);
                acc = result_and(acc, r);
                if matches!(acc, RelationResult::NotAssignable) {
                    return RelationResult::NotAssignable;
                }
            }
        }
        acc
    }

    /// An Object source against a DIRECT signature target: some signature
    /// in the source's MATCHING-KIND group must satisfy the target
    /// signature.
    fn relate_object_to_signature(
        &self,
        source: &SurfaceView,
        target_kind: crate::semantic_query::SignatureKind,
        target_sig: SemanticNodeId,
        bindings: &mut Vec<InferBinding>,
    ) -> RelationResult {
        let group = match target_kind {
            crate::semantic_query::SignatureKind::Call => &source.call_signatures,
            crate::semantic_query::SignatureKind::Construct => &source.construct_signatures,
        };
        let alternatives: Vec<_> = group
            .iter()
            .map(|source_signature| (*source_signature, target_sig))
            .collect();
        self.relate_pair_alternatives(&alternatives, bindings, InferPosition::Covariant)
    }
}

/// The pop result of a relation frame.
enum FramePop {
    /// A provisional member's caller-return step (never the published
    /// payload).
    Provisional(RelationStep),
    /// The SCC root's public close outcome.
    RootClose(RootClose),
}

/// Outcome of the identity-carrier unwrap performed before relation
/// dispatch.
pub(super) enum IdentityCarrierUnwrap {
    Concrete(SemanticNodeId),
    Unresolvable,
}

/// Canonical Record shapes the `Record<K, V>`-against-Object arm handles.
enum RecordTargetShape {
    LiteralKey(SurfaceView),
    GenericKey {
        key_type: SemanticNodeId,
        value_type: SemanticNodeId,
    },
}

/// Map a reducer verdict + optional session fixation onto the pending
/// record a popped frame carries.
fn pending_verdict_of(
    verdict: &RelationResult,
    budget_cap: &Option<RecursionOrBudgetCap>,
    session_bindings: &mut Option<Arc<[InferBinding]>>,
    bindings: Vec<InferBinding>,
) -> PendingVerdict {
    if let Some(cap) = budget_cap {
        return PendingVerdict::BudgetExceeded(*cap);
    }
    match verdict {
        RelationResult::Assignable { .. } => PendingVerdict::Assignable {
            bindings: session_bindings
                .take()
                .unwrap_or_else(|| Arc::from(bindings.into_boxed_slice())),
        },
        RelationResult::NotAssignable => PendingVerdict::NotAssignable,
        RelationResult::Unknown => PendingVerdict::Unknown,
    }
}

/// The caller-return step of a provisional pending verdict.
pub(super) fn relation_step_from_pending(pending: &PendingVerdict) -> RelationStep {
    match pending {
        PendingVerdict::Assignable { bindings } => RelationStep::Assignable {
            bindings: Arc::clone(bindings),
        },
        PendingVerdict::NotAssignable => RelationStep::NotAssignable,
        PendingVerdict::Unknown => RelationStep::Unknown,
        PendingVerdict::BudgetExceeded(cap) => RelationStep::BudgetExceeded(*cap),
    }
}

/// The caller-return step of a published (or warm) payload.
fn relation_step_from_payload(payload: &RelationPayload) -> RelationStep {
    match &payload.outcome {
        RelationOutcome::Assignable => RelationStep::Assignable {
            bindings: Arc::clone(&payload.bindings),
        },
        RelationOutcome::NotAssignable => RelationStep::NotAssignable,
        RelationOutcome::BudgetExceeded(kind) => {
            RelationStep::BudgetExceeded(RecursionOrBudgetCap {
                kind: *kind,
                limit: 0,
            })
        }
    }
}

/// Iterative worklist item for [`ProjectSemanticDispatch::decide_relation`].
#[derive(Debug, Clone)]
enum RelateWork {
    /// Expand the current frame's root pair locally.
    Expand(SemanticNodeId, SemanticNodeId),
    /// Evaluate `(source, target)`.
    Eval(SemanticNodeId, SemanticNodeId),
    /// Pop `n` prior results, AND them, push one combined result.
    ReduceAnd(u32),
}

fn reduce_and_from_results(results: &mut Vec<RelationResult>, n: u32) -> RelationResult {
    let mut combined = RelationResult::Assignable {
        bindings: Arc::from(Vec::new().into_boxed_slice()),
    };
    // bounded-loop: drains `n` per-pair results owned by this reducer — fan-out of the originating distribution; total work bounded by `decide_relation` budget (graph-size × 10).
    for _ in 0..n {
        let r = results
            .pop()
            .expect("RelateWork::ReduceAnd: result-stack underflow");
        combined = result_and(combined, r);
    }
    combined
}

/// Build a forward-ordered sequence of `RelateWork` items such that after
/// `push_forward_work`, the first item pops first.
fn push_forward_work(work: &mut Vec<RelateWork>, forward: Vec<RelateWork>) {
    for item in forward.into_iter().rev() {
        work.push(item);
    }
}

/// Build and push the worklist fan-out for a distribution whose reducer
/// is AND-all.
fn distribute_and<F>(
    work: &mut Vec<RelateWork>,
    results: &mut Vec<RelationResult>,
    members: &[SemanticNodeId],
    mut pairer: F,
) where
    F: FnMut(&SemanticNodeId) -> (SemanticNodeId, SemanticNodeId),
{
    let n = members.len();
    if n == 0 {
        results.push(RelationResult::Assignable {
            bindings: Arc::from(Vec::new().into_boxed_slice()),
        });
        return;
    }
    let mut forward: Vec<RelateWork> = Vec::with_capacity(n + 1);
    for m in members.iter() {
        let (s, t) = pairer(m);
        forward.push(RelateWork::Eval(s, t));
    }
    if n > 1 {
        forward.push(RelateWork::ReduceAnd(n as u32));
    }
    push_forward_work(work, forward);
}

/// O(tag) disjointness for the contravariant-candidate intersection
/// collapse: `true` ONLY for pairs whose intersection is provably empty at
/// tag level — distinct concrete primitives (modulo the
/// `undefined`/`void` widening pair), distinct literals, or a literal
/// against a mismatched base primitive. Conservative `false` for every
/// other shape (the structural Intersection carrier is kept).
fn tag_level_disjoint(
    graph: &crate::semantic_query_memo::SemanticGraphStore,
    a: SemanticNodeId,
    b: SemanticNodeId,
) -> bool {
    let (Some(a_data), Some(b_data)) = (graph.node_data(a), graph.node_data(b)) else {
        return false;
    };
    fn literal_base(lit: &LiteralValue) -> PrimitiveKind {
        match lit {
            LiteralValue::String(_) => PrimitiveKind::String,
            LiteralValue::Number(_) => PrimitiveKind::Number,
            LiteralValue::Boolean(_) => PrimitiveKind::Boolean,
            LiteralValue::BigInt(_) => PrimitiveKind::BigInt,
        }
    }
    fn concrete(kind: PrimitiveKind) -> bool {
        !matches!(
            kind,
            PrimitiveKind::Any | PrimitiveKind::Unknown | PrimitiveKind::Never
        )
    }
    match (&*a_data, &*b_data) {
        (SemanticNodeData::Primitive(x), SemanticNodeData::Primitive(y)) => {
            let widening_pair = matches!(
                (*x, *y),
                (PrimitiveKind::Undefined, PrimitiveKind::Void)
                    | (PrimitiveKind::Void, PrimitiveKind::Undefined)
            );
            concrete(*x) && concrete(*y) && x != y && !widening_pair
        }
        (SemanticNodeData::Literal(x), SemanticNodeData::Literal(y)) => x != y,
        (SemanticNodeData::Literal(lit), SemanticNodeData::Primitive(prim))
        | (SemanticNodeData::Primitive(prim), SemanticNodeData::Literal(lit)) => {
            concrete(*prim) && literal_base(lit) != *prim
        }
        _ => false,
    }
}

#[cfg(test)]
pub(crate) mod reverse_ownership_tests {
    use super::super::dispatch_txn::SessionId;
    use super::*;

    fn reverse_setup(param: SemanticNodeId) -> InferenceSessionSetup {
        InferenceSessionSetup::new(
            Arc::from(vec![InferenceInfoSetup::new(param, Arc::from("T"))].into_boxed_slice()),
            VariancePhase::Covariant,
            InferencePassKind::ReverseHomomorphicMapped,
            InferenceCandidatePriority::HomomorphicMapped,
            NoInferMask::empty(),
            ConstParamPolicy::NonConst,
            ContextualInferenceMode::None,
        )
    }

    fn reverse_state(param: SemanticNodeId) -> ReverseProjectionState {
        ReverseProjectionState::new(ReverseHomomorphicSpec {
            mapped_node: SemanticNodeId(301),
            base_infer: param,
            mapper_parameter: SemanticNodeId(302),
            template: SemanticNodeId(303),
            modifiers: ReverseMappedModifiers {
                optionality: OptionalityMod::Keep,
                readonly: ReadonlyMod::Keep,
            },
        })
    }

    fn require_relation_result_signature<'dispatch>(
        _pass: fn(
            &ProjectSemanticDispatch<'dispatch>,
            SemanticNodeId,
            &ReverseHomomorphicSpec,
            &mut Vec<InferBinding>,
        ) -> RelationResult,
    ) {
    }

    fn classify_relation_result_exhaustively(result: RelationResult) {
        match result {
            RelationResult::Assignable { .. }
            | RelationResult::NotAssignable
            | RelationResult::Unknown => {}
        }
    }

    #[test]
    pub(crate) fn reverse_mapped_inference_is_relation_owned_in_session() {
        // This private function item is nameable only from the relation
        // authority's own module tree, and its sole output is the closed
        // reducer lattice rather than a standalone binding map.
        require_relation_result_signature(ProjectSemanticDispatch::relate_reverse_homomorphic);
        classify_relation_result_exhaustively(RelationResult::Unknown);

        let active_param = SemanticNodeId(304);
        let aggregate = SemanticNodeId(305);
        let fallback = SemanticNodeId(306);

        let mut inactive = InferenceSession::new(
            SessionId(1),
            reverse_setup(active_param),
            Some(reverse_state(active_param)),
        );
        assert!(
            !inactive.deposit_reverse_aggregate(
                SemanticNodeId(999),
                aggregate,
                InferenceCandidatePriority::HomomorphicMapped,
            ),
            "a reverse aggregate cannot bind outside the frozen session setup"
        );
        let inactive_bindings = inactive
            .stage_fixation(|nodes, _| nodes.first().copied().unwrap_or(fallback))
            .expect("collecting session stages");
        assert_eq!(
            inactive_bindings[0].bound, fallback,
            "a refused deposit must leave no independently publishable reverse result"
        );

        let mut active = InferenceSession::new(
            SessionId(2),
            reverse_setup(active_param),
            Some(reverse_state(active_param)),
        );
        assert!(active.deposit_reverse_aggregate(
            active_param,
            aggregate,
            InferenceCandidatePriority::HomomorphicMapped,
        ));
        let active_bindings = active
            .stage_fixation(|nodes, _| nodes.first().copied().unwrap_or(fallback))
            .expect("collecting session stages");
        assert_eq!(
            active_bindings[0].bound, aggregate,
            "the accepted aggregate reaches bindings only through session fixation"
        );
    }
}
