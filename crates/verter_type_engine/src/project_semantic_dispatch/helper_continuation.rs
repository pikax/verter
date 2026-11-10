//! Callable and relation helper computations as owned continuations.
//!
//! A family continuation (`family_continuation`) is a query's whole build.
//! The relation authority and the call executor are not only queries a
//! consumer needs: they are helpers a computation runs inline, under the
//! checker state it holds — an open obligation, an inference session, a
//! binding barrier, a contextual call's collector. A consumer that must not
//! block (a flow evaluation retained across its reads, a relation root on
//! the continuation runtime) runs such a helper as an owned computation it
//! advances:
//!
//! - it begins the helper ([`RelationRead::pair`], [`RelationRead::assignable`],
//!   [`RelationRead::member`], [`CallRead::begin`]), which reads nothing;
//! - it advances it with the delivery of the request it last stood at
//!   (`None` on the first advance), and the helper answers
//!   [`HelperStep::Done`] with its output or [`HelperStep::Request`] with
//!   the one dependency it reads next, holding everything it computed
//!   before that read;
//! - a frame of the continuation runtime turns a request into its own need
//!   (`ProjectSemanticDispatch::helper_request_poll`) and hands the delivery
//!   back (`ProjectSemanticDispatch::helper_delivery`); the synchronous
//!   driver answers in place, so the helper never stops for one;
//! - a consumer that stops while its helper waits releases it
//!   (`ProjectSemanticDispatch::release_relation_read`), which closes what
//!   the helper holds open.
//!
//! The protocol is the stable interface: the requests a helper makes grow
//! as its internal stages are converted, and its consumers keep driving it
//! the same way. A dependency is a typed transition into the helper's
//! retained state, never a re-run of the helper with an answer cache.
//!
//! What a helper still computes in place — its stages below the point it
//! suspends at — is inventoried in `docs/arch/root-continuations.md`
//! ("Boundaries that still block"). Today only a machinery relation root
//! driven by a frame suspends: at the first instantiations its structural
//! dispatch reads, unwrapping its operands. A pair or
//! member relation and a call compute in place on either driver.

use super::call_resolve::{ContextualCallState, DeclaredCallResult, ResolveCallStep};
use super::dispatch_txn::{InferenceOccurrence, RelationStep};
use super::query_frames::{OperandDemand, QueryDelivery};
use super::relation::RelateRootBuild;
use super::walk::QueryBuildOutput;
use super::ProjectSemanticDispatch;
use crate::semantic_query::{
    RelateMemoKey, RelationKind, ResolveCallKey, SemanticNodeId, SemanticQueryKey,
    SemanticQueryValue,
};
use crate::semantic_query_memo::ClaimAttempt;

/// Where a helper computation stands after it advanced.
pub(super) enum HelperStep<T> {
    /// The helper's output.
    Done(T),
    /// The helper reads this dependency next, holding everything it
    /// computed before the read.
    Request(HelperRequest),
}

/// The one dependency a helper computation reads next. Opaque: its
/// consumer's driver answers it.
pub(super) struct HelperRequest {
    kind: RequestKind,
}

enum RequestKind {
    /// A query, needed as a frame's need would be: the claim it already
    /// entered with continues when the runtime starts it.
    Query(SemanticQueryKey, Option<ClaimAttempt>),
}

impl HelperRequest {
    pub(super) fn query(key: SemanticQueryKey, claim: Option<ClaimAttempt>) -> Self {
        Self {
            kind: RequestKind::Query(key, claim),
        }
    }

    /// The query and the claim it entered with, for the driver.
    pub(super) fn into_query(self) -> (SemanticQueryKey, Option<ClaimAttempt>) {
        match self.kind {
            RequestKind::Query(key, claim) => (key, claim),
        }
    }
}

/// What a consumer's driver keeps while a helper it advances waits on a
/// request, to hand the request's delivery back to it.
pub(super) struct HelperAwait {
    kind: AwaitKind,
}

enum AwaitKind {
    Query(SemanticQueryKey),
}

impl HelperAwait {
    pub(super) fn query(key: SemanticQueryKey) -> Self {
        Self {
            kind: AwaitKind::Query(key),
        }
    }

    pub(super) fn into_key(self) -> SemanticQueryKey {
        match self.kind {
            AwaitKind::Query(key) => key,
        }
    }
}

/// The answer to the request a helper stood at: the complete delivery, not
/// a node narrowed out of it — a helper adopting or discarding what the
/// read observed decides that itself.
pub(super) struct HelperDelivery {
    kind: DeliveryKind,
}

enum DeliveryKind {
    Query(SemanticQueryKey, QueryDelivery),
}

impl HelperDelivery {
    pub(super) fn query(key: SemanticQueryKey, delivery: QueryDelivery) -> Self {
        Self {
            kind: DeliveryKind::Query(key, delivery),
        }
    }

    /// The query this delivery answers, and its delivery.
    pub(super) fn into_query(self) -> (SemanticQueryKey, QueryDelivery) {
        match self.kind {
            DeliveryKind::Query(key, delivery) => (key, delivery),
        }
    }
}

/// How the consumer advancing a helper drives the requests it makes.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum HelperDrive {
    /// The synchronous driver: each read is made in place, so the helper
    /// never stops for one.
    InPlace,
    /// A frame of the continuation runtime whose needs run `depth` deep: a
    /// read the frame must produce first is a request, and the helper waits.
    Frame { depth: u32 },
}

/// A relation judgement as an owned computation: the relation authority's
/// pair entry (pure assignability behind a binding barrier), its member
/// entry (a sub-relation under the inference occurrence it stands at), or
/// a machinery root's build.
pub(super) struct RelationRead {
    state: RelationState,
}

enum RelationState {
    /// Not begun: nothing is read yet.
    Begin(RelationEntry),
    /// No earlier step answered: the relation computes. On a frame its
    /// operands' carrier walks are demanded first; each instantiation or
    /// `typeof` a walk reads that must be computed is a request.
    Compute(Prepared, Option<Box<OperandDemand>>),
    /// The output was delivered, or the relation was released.
    Finished,
}

/// Which entry of the relation authority a [`RelationRead`] runs.
enum RelationEntry {
    /// `isTypeRelatedTo` over a pair, no inference session depositing;
    /// `union_membership` takes the assignability fast path of a literal's
    /// membership in a literal union.
    Pair {
        source: SemanticNodeId,
        target: SemanticNodeId,
        kind: RelationKind,
        union_membership: bool,
    },
    /// A relation that may deposit into the inference session open around
    /// it, at its occurrence.
    Member {
        key: RelateMemoKey,
        occurrence: InferenceOccurrence,
    },
    /// The `Relate` family's build of `key` as its machinery root.
    Root(RelateMemoKey),
}

/// A relation no answer-before-compute step answered.
enum Prepared {
    /// The authority's cold compute of `key` (upgraded with its inference
    /// context), at `occurrence`, behind a binding barrier for a pair.
    Relation {
        key: RelateMemoKey,
        occurrence: InferenceOccurrence,
        barrier: bool,
    },
    /// A machinery root whose root obligation frame is open, owned here
    /// until it reduces or is released.
    Root(Box<RelateRootBuild>),
}

impl Prepared {
    fn operands(&self) -> [SemanticNodeId; 2] {
        let key = match self {
            Self::Relation { key, .. } => key,
            Self::Root(root) => root.key(),
        };
        [key.source, key.target]
    }
}

/// Where a relation stands after its first stage.
enum EntryStage {
    /// The relation is answered without being computed.
    Answered(RelationOutput),
    /// The relation is left to compute.
    Compute(Prepared),
}

/// What a relation computes: the authority's step for a pair or member
/// entry, the family build's output for a machinery root.
enum RelationOutput {
    Step(RelationStep),
    Root(Box<QueryBuildOutput<SemanticQueryValue>>),
}

impl RelationRead {
    /// Whether `source` relates to `target` under `kind`, as a pair: the
    /// relation never deposits into an inference session open around it
    /// (`execute_relate_pair_kind`).
    pub(super) fn pair(source: SemanticNodeId, target: SemanticNodeId, kind: RelationKind) -> Self {
        Self::entry(RelationEntry::Pair {
            source,
            target,
            kind,
            union_membership: false,
        })
    }

    /// Whether `source` is assignable to `target`, as a pair, with the
    /// literal-union membership fast path (`execute_relate_pair`).
    pub(super) fn assignable(source: SemanticNodeId, target: SemanticNodeId) -> Self {
        Self::entry(RelationEntry::Pair {
            source,
            target,
            kind: RelationKind::Assignable,
            union_membership: true,
        })
    }

    /// The relation `key` at `occurrence`, depositing into the inference
    /// session open around it (`execute_relate_with_occurrence`).
    pub(super) fn member(key: RelateMemoKey, occurrence: InferenceOccurrence) -> Self {
        Self::entry(RelationEntry::Member { key, occurrence })
    }

    /// The `Relate` family's build of `key` as its machinery root.
    pub(super) fn root(key: RelateMemoKey) -> Self {
        Self::entry(RelationEntry::Root(key))
    }

    fn entry(entry: RelationEntry) -> Self {
        Self {
            state: RelationState::Begin(entry),
        }
    }
}

/// A call resolution as an owned computation: the call executor's
/// contextual entry over `key`, whose contextual continuation holds the
/// candidate's collector across the consumer's callback typing.
pub(super) struct CallRead {
    key: Option<ResolveCallKey>,
    contextual: ContextualCallState,
}

/// What a call resolution answers: the executor's step, and the declared
/// result a completed recipe supplied.
pub(super) type CallAnswer = (ResolveCallStep, Option<DeclaredCallResult>);

impl CallRead {
    /// The resolution of the call `key`, continuing `contextual`.
    pub(super) fn begin(key: ResolveCallKey, contextual: ContextualCallState) -> Self {
        Self {
            key: Some(key),
            contextual,
        }
    }

    /// The contextual continuation the resolution left, for the consumer's
    /// next round of callback typing.
    pub(super) fn into_contextual(self) -> ContextualCallState {
        self.contextual
    }
}

// A suspended helper is owned state its consumer carries to whichever
// thread resumes it, advanced through an exclusive borrow, never shared.
static_assertions::assert_impl_all!(RelationRead: Send);
static_assertions::assert_impl_all!(CallRead: Send);
static_assertions::assert_impl_all!(HelperRequest: Send);
static_assertions::assert_impl_all!(HelperAwait: Send);
static_assertions::assert_impl_all!(HelperDelivery: Send);
static_assertions::assert_not_impl_any!(RelationRead: Clone, Copy);
static_assertions::assert_not_impl_any!(CallRead: Clone, Copy);
static_assertions::assert_not_impl_any!(HelperRequest: Clone, Copy);

impl<'a, C: crate::resolver_core::ResolverCapabilities> ProjectSemanticDispatch<'a, C> {
    /// Advance a pair or member relation with the delivery of the request
    /// it stood at ([`Self::advance_relation`]).
    pub(super) fn advance_relation_read(
        &self,
        read: &mut RelationRead,
        delivery: Option<HelperDelivery>,
        drive: HelperDrive,
    ) -> HelperStep<RelationStep> {
        match self.advance_relation(read, delivery, drive) {
            HelperStep::Done(RelationOutput::Step(step)) => HelperStep::Done(step),
            HelperStep::Done(RelationOutput::Root(_)) => {
                unreachable!("a pair or member relation computes a step")
            }
            HelperStep::Request(request) => HelperStep::Request(request),
        }
    }

    /// Advance a machinery relation root with the delivery of the request
    /// it stood at ([`Self::advance_relation`]): the family build's output
    /// once it reduces.
    pub(super) fn advance_relation_root(
        &self,
        read: &mut RelationRead,
        delivery: Option<HelperDelivery>,
        drive: HelperDrive,
    ) -> HelperStep<Box<QueryBuildOutput<SemanticQueryValue>>> {
        match self.advance_relation(read, delivery, drive) {
            HelperStep::Done(RelationOutput::Root(output)) => HelperStep::Done(output),
            HelperStep::Done(RelationOutput::Step(_)) => {
                unreachable!("a machinery root computes its build's output")
            }
            HelperStep::Request(request) => HelperStep::Request(request),
        }
    }

    /// Advance a relation with the delivery of the request it stood at.
    ///
    /// The authority's answer-before-compute steps (a literal pair, an
    /// overflowed chain, a reentered frame, a settled or warm answer) run
    /// once, as it begins; a machinery root instead validates its key and
    /// opens its root obligation frame. Driven by a frame, a machinery root
    /// then reduces up to its structural dispatch; when that leaves the pair
    /// undecided, the first instantiation each operand's carrier walk reads
    /// is demanded: one that must be computed is the request. With each
    /// answered, the dispatch runs in place, those reads answered by the
    /// deliveries.
    #[inline(never)]
    fn advance_relation(
        &self,
        read: &mut RelationRead,
        delivery: Option<HelperDelivery>,
        drive: HelperDrive,
    ) -> HelperStep<RelationOutput> {
        let mut delivery = delivery;
        loop {
            match std::mem::replace(&mut read.state, RelationState::Finished) {
                RelationState::Begin(entry) => {
                    verter_debug_assert!(delivery.is_none(), "a relation begins unasked");
                    let mut prepared = match self.relation_answer_at_entry(entry) {
                        EntryStage::Answered(output) => return HelperStep::Done(output),
                        EntryStage::Compute(prepared) => prepared,
                    };
                    // On a frame, a machinery root reduces up to its
                    // structural dispatch, whose first reads unwrap both
                    // operands: those reads are demanded before it goes on.
                    // Nothing is demanded that the reduction would not read
                    // next, and a pair or member relation computes in place.
                    let waits_at_dispatch = match (drive, &mut prepared) {
                        (HelperDrive::Frame { .. }, Prepared::Root(root)) => {
                            self.reduce_relate_build_before_dispatch(root)
                        }
                        _ => false,
                    };
                    let demand = waits_at_dispatch
                        .then(|| Box::new(OperandDemand::new(prepared.operands())));
                    read.state = RelationState::Compute(prepared, demand);
                }
                RelationState::Compute(prepared, None) => {
                    return HelperStep::Done(self.relation_compute_in_place(prepared));
                }
                RelationState::Compute(prepared, Some(mut demand)) => {
                    let HelperDrive::Frame { depth } = drive else {
                        unreachable!("only a frame suspends a relation at its operands")
                    };
                    let delivered = delivery.take().map(HelperDelivery::into_query);
                    if let Some(request) =
                        self.advance_operand_demand(&mut demand, delivered, depth)
                    {
                        read.state = RelationState::Compute(prepared, Some(demand));
                        return HelperStep::Request(request);
                    }
                    let output = self.with_operand_answers(&mut demand, depth, || {
                        self.relation_compute_in_place(prepared)
                    });
                    return HelperStep::Done(output);
                }
                RelationState::Finished => unreachable!("a relation delivers its output once"),
            }
        }
    }

    /// Release a relation its consumer stopped before it delivered: a
    /// machinery root waiting on its operands closes its root obligation
    /// frame undecided and aborts the members no publication will drain.
    pub(super) fn release_relation_read(&self, read: RelationRead) {
        if let RelationState::Compute(Prepared::Root(root), _) = read.state {
            self.abandon_relate_build(*root);
        }
    }

    /// Drive a relation to its step, making each read in place: the
    /// synchronous adapter.
    pub(super) fn drive_relation_in_place(&self, mut read: RelationRead) -> RelationStep {
        match self.advance_relation_read(&mut read, None, HelperDrive::InPlace) {
            HelperStep::Done(step) => step,
            HelperStep::Request(_) => unreachable!("an in-place relation makes no request"),
        }
    }

    /// The first stage of `entry`: for a pair or member, steps (0)–(2) of
    /// the relation authority — a pair's literal fast paths before any key
    /// is built, then a literal pair, an overflowed chain, a reentered
    /// frame, a settled or warm answer — a pair's behind its binding
    /// barrier; for a machinery root, its key validated and its root frame
    /// opened.
    fn relation_answer_at_entry(&self, entry: RelationEntry) -> EntryStage {
        match entry {
            RelationEntry::Pair {
                source,
                target,
                kind,
                union_membership,
            } => {
                if let Some(step) =
                    self.relation_pair_literal_answer(source, target, kind, union_membership)
                {
                    return EntryStage::Answered(RelationOutput::Step(step));
                }
                let mut key = self.relate_key_for_kind(source, target, kind);
                let occurrence = InferenceOccurrence::ARGUMENT_COVARIANT;
                match self.with_binding_disabled(|| {
                    self.relation_answer_before_compute(&mut key, occurrence)
                }) {
                    Some(step) => EntryStage::Answered(RelationOutput::Step(step)),
                    None => EntryStage::Compute(Prepared::Relation {
                        key,
                        occurrence,
                        barrier: true,
                    }),
                }
            }
            RelationEntry::Member {
                mut key,
                occurrence,
            } => match self.relation_answer_before_compute(&mut key, occurrence) {
                Some(step) => EntryStage::Answered(RelationOutput::Step(step)),
                None => EntryStage::Compute(Prepared::Relation {
                    key,
                    occurrence,
                    barrier: false,
                }),
            },
            RelationEntry::Root(key) => match self.begin_relate_build(&key) {
                Ok(root) => EntryStage::Compute(Prepared::Root(Box::new(root))),
                Err(refusal) => EntryStage::Answered(RelationOutput::Root(refusal)),
            },
        }
    }

    /// The compute stage of `prepared`, where the driver stands: every read
    /// the reduction makes is made in place.
    fn relation_compute_in_place(&self, prepared: Prepared) -> RelationOutput {
        match prepared {
            Prepared::Relation {
                key,
                occurrence,
                barrier: true,
            } => RelationOutput::Step(
                self.with_binding_disabled(|| self.relation_cold_compute(key, occurrence)),
            ),
            Prepared::Relation {
                key,
                occurrence,
                barrier: false,
            } => RelationOutput::Step(self.relation_cold_compute(key, occurrence)),
            Prepared::Root(root) => RelationOutput::Root(Box::new(self.finish_relate_build(*root))),
        }
    }

    /// Advance a call resolution with the delivery of the request it stood
    /// at. The executor resolves the call in place today: a call makes no
    /// request yet, so its consumer's driver answers none.
    #[inline(never)]
    pub(super) fn advance_call_read(
        &self,
        read: &mut CallRead,
        delivery: Option<HelperDelivery>,
        _drive: HelperDrive,
    ) -> HelperStep<CallAnswer> {
        verter_debug_assert!(delivery.is_none(), "a call makes no request yet");
        let key = read.key.take().expect("a call delivers its answer once");
        HelperStep::Done(self.execute_contextual_call(key, &mut read.contextual))
    }

    /// Drive a call resolution to its answer in place: the synchronous
    /// adapter.
    pub(super) fn drive_call_in_place(&self, read: &mut CallRead) -> CallAnswer {
        match self.advance_call_read(read, None, HelperDrive::InPlace) {
            HelperStep::Done(answer) => answer,
            HelperStep::Request(_) => unreachable!("an in-place call makes no request"),
        }
    }
}
