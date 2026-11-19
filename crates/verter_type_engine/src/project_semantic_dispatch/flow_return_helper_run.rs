//! Retained positions in reading helper chains, shared by both drivers.

use super::*;

/// A carrier's graph-only answer, or the standing walk and its first read.
pub(in crate::project_semantic_dispatch) enum CarrierStep {
    Settled(IdentityCarrierUnwrap),
    Wait(Box<IdentityCarrierRead>, ReadWait),
}

impl CarrierStep {
    pub(super) fn begin<C: crate::resolver_core::ResolverCapabilities>(
        dispatch: &ProjectSemanticDispatch<'_, C>,
        node: SemanticNodeId,
    ) -> Self {
        let mut walk =
            IdentityCarrierRead::at(super::super::relation::CarrierWalkPoint { node, depth: 0 });
        match dispatch.advance_identity_carrier_read(&mut walk) {
            IdentityCarrierStep::Done(done) => Self::Settled(done),
            IdentityCarrierStep::Read(key) => {
                let wait = carrier_wait(dispatch, &walk, key);
                Self::Wait(Box::new(walk), wait)
            }
        }
    }
}

/// Preserve the carrier consumer's dispatch intent before requesting its read.
/// The read is staged: a deferred evaluation the walk stands in makes it
/// under its own tracer, so its proof names what the read observed.
pub(super) fn carrier_wait<C: crate::resolver_core::ResolverCapabilities>(
    dispatch: &ProjectSemanticDispatch<'_, C>,
    walk: &IdentityCarrierRead,
    key: SemanticQueryKey,
) -> ReadWait {
    if walk.reads_for_carrier() && !matches!(key, SemanticQueryKey::TypeOf { .. }) {
        dispatch.record_dispatch_intent_counters(&key);
    }
    dispatch.request(key, ReadShape::Staged, ReadOrigin::Carrier)
}

static_assertions::const_assert!(std::mem::size_of::<CarrierStep>() <= 32);

/// Settle an identity carrier where its caller cannot suspend. The walk is
/// requested as every carrier read is ([`CarrierStep::begin`]): a concrete
/// carrier settles without a read, and each read the walk stands at is
/// answered at once, made inside the walk's stage and attributed to the
/// carrier as an in-place answer.
pub(super) fn carrier_in_place(
    dispatch: &impl FlowDemandDriver,
    canonical: &str,
    node: SemanticNodeId,
) -> IdentityCarrierUnwrap {
    match dispatch.request_carrier(node) {
        CarrierStep::Settled(done) => done,
        CarrierStep::Wait(mut walk, wait) => {
            let done = settle_now(
                ReadStep::Wait(wait),
                |wait| dispatch.read_in_place(wait, canonical),
                |delivery| dispatch.deliver_carrier(&mut walk, delivery, canonical),
            );
            dispatch.release_carrier(walk);
            done
        }
    }
}

/// A reduction operand's view read through its identity carrier where its
/// caller cannot suspend ([`carrier_in_place`]): the structure an alias or
/// declaration carrier names, the node itself when it names none or its
/// carrier does not resolve, with an object-spread program regularized as
/// [`ProjectSemanticDispatch::resolved_reduction_view`] regularizes it.
pub(super) fn reduction_view_in_place(
    dispatch: &impl FlowDemandDriver,
    canonical: &str,
    view: SemanticNodeId,
) -> SemanticNodeId {
    match dispatch.arena().node_data(view).as_deref() {
        Some(SemanticNodeData::ObjectSpreadProgram(_)) => {
            return dispatch.regularized_empty_object_program(view);
        }
        Some(
            SemanticNodeData::Alias(_)
            | SemanticNodeData::DeclRef { .. }
            | SemanticNodeData::InstantiationRef { .. },
        ) => {}
        _ => return view,
    }
    match carrier_in_place(dispatch, canonical, view) {
        IdentityCarrierUnwrap::Concrete(resolved) => {
            dispatch.regularized_empty_object_program(resolved)
        }
        IdentityCarrierUnwrap::Unresolvable => view,
    }
}

/// A type lowering's node, or the lowering and the first read it waits on.
/// A lowering always yields a node.
pub(in crate::project_semantic_dispatch) enum LowerStart<'e> {
    Done(SemanticNodeId),
    Wait(Box<FlowLowering<'e>>, ReadWait),
}

/// A type lowering held between its reads, with what each step rebuilds its
/// lowering context from.
pub(in crate::project_semantic_dispatch) enum FlowLowering<'e> {
    /// A body-position type under the evaluator's binder environment, shared
    /// with the activation that owns it.
    Body(super::super::lower::TypeLowerRead<'e>, Arc<FlowBinderEnv>),
    /// A type in a file owner scope, with the scope context the owner-scope
    /// entry builds.
    Owner(super::super::lower::OwnerTypeLowerRead<'e>),
}

static_assertions::assert_impl_all!(FlowLowering<'_>: Send);
static_assertions::assert_not_impl_any!(FlowLowering<'_>: Clone, Copy);
static_assertions::const_assert!(std::mem::size_of::<LowerStart<'static>>() <= 32);

/// Carry a lowering to its node where its caller cannot suspend: each read
/// it waits on is answered at once, attributed to the lowering.
pub(super) fn lowering_in_place(
    dispatch: &impl FlowDemandDriver,
    canonical: &str,
    start: LowerStart<'_>,
) -> SemanticNodeId {
    match start {
        LowerStart::Done(node) => node,
        LowerStart::Wait(mut lowering, wait) => settle_now(
            ReadStep::Wait(wait),
            |wait| dispatch.read_in_place(wait, canonical),
            |delivery| dispatch.deliver_lowering(&mut lowering, delivery),
        ),
    }
}

/// Answer every wait of an owned reading operation where its caller cannot suspend.
pub(super) fn settle_now<T, W, V>(
    mut step: ReadStep<T, W>,
    mut answer: impl FnMut(W) -> V,
    mut resume: impl FnMut(V) -> ReadStep<T, W>,
) -> T {
    loop {
        match step {
            ReadStep::Ready(value) => return value,
            ReadStep::Wait(wait) => step = resume(answer(wait)),
        }
    }
}

pub(super) fn signature_list_in_place(
    dispatch: &impl FlowDemandDriver,
    canonical: &str,
    subject: SemanticNodeId,
    kind: SignatureKind,
) -> Option<Vec<SemanticNodeId>> {
    let (mut list, wait) = dispatch.request_signature_list(subject, kind);
    settle_now(
        ReadStep::Wait(wait),
        |wait| dispatch.read_in_place(wait, canonical),
        |delivery| dispatch.deliver_signature_list(&mut list, delivery),
    )
}

pub(super) fn signature_buckets_in_place(
    dispatch: &impl FlowDemandDriver,
    canonical: &str,
    subject: SemanticNodeId,
) -> Option<(Vec<SemanticNodeId>, Vec<SemanticNodeId>)> {
    let calls = signature_list_in_place(dispatch, canonical, subject, SignatureKind::Call)?;
    let constructs =
        signature_list_in_place(dispatch, canonical, subject, SignatureKind::Construct)?;
    Some((calls, constructs))
}

#[cfg(any(test, feature = "test-support"))]
impl<C: crate::resolver_core::ResolverCapabilities> ProjectSemanticDispatch<'_, C> {
    /// Exercise the native bucket reader with a graph-only subject.
    pub fn flow_signature_buckets_for_tests(
        &self,
        canonical: &str,
        subject: SemanticNodeId,
    ) -> Option<(Vec<SemanticNodeId>, Vec<SemanticNodeId>)> {
        signature_buckets_in_place(self, canonical, subject)
    }

    /// Check the carrier driver's graph-only completion without answering a read.
    pub fn flow_carrier_without_read_for_tests(
        &self,
        node: SemanticNodeId,
    ) -> Option<SemanticNodeId> {
        match self.request_carrier(node) {
            CarrierStep::Settled(IdentityCarrierUnwrap::Concrete(node)) => Some(node),
            CarrierStep::Settled(IdentityCarrierUnwrap::Unresolvable) => None,
            CarrierStep::Wait(walk, _) => {
                self.release_carrier(walk);
                None
            }
        }
    }

    /// Act as a holder whose probe consumes a read it answered outside the
    /// probe: request the type node of `key` under the current checking
    /// site (marked observed when `observed`), answer it through
    /// [`FlowReadDriver::answer_read`] for the body in `canonical`, then
    /// open the answer inside a probe that `probe` consumes. Returns the
    /// node the holder consumed and the partial reasons the enclosing build
    /// holds afterwards (`None` when it is complete).
    pub fn flow_holder_probe_read_for_tests(
        &self,
        canonical: &str,
        key: SemanticQueryKey,
        observed: bool,
        probe: HolderProbeForTests,
    ) -> (
        Option<SemanticNodeId>,
        Option<crate::semantic_query::PartialReasonSet>,
    ) {
        let enclosing = ProjectSemanticDispatch::observe_read(self, || {
            let wait =
                FlowReadDriver::request(self, key, ReadShape::Node, ReadOrigin::IndexedValue);
            let wait = if observed { wait.observed() } else { wait };
            let delivery = FlowReadDriver::answer_read(self, wait, canonical);
            let read = ProjectSemanticDispatch::observe_read(self, || match delivery {
                ReadDelivery::Observed(read) => match probe {
                    HolderProbeForTests::DeclinedInFresh => {
                        FlowDemandDriver::discard_observed(self, read)
                    }
                    HolderProbeForTests::Stale | HolderProbeForTests::Fresh => {
                        FlowDemandDriver::adopt_observed(self, read)
                    }
                },
                delivery => Ok(delivery),
            });
            let opened = match probe {
                HolderProbeForTests::Stale => read.discard(),
                HolderProbeForTests::Fresh | HolderProbeForTests::DeclinedInFresh => {
                    FlowDemandDriver::adopt_read(self, read)
                }
            };
            opened
                .and_then(|delivery| delivery)
                .expect("the holder's read observes no abort")
                .node()
        });
        let completeness = enclosing.result_completeness();
        let node = enclosing
            .discard()
            .expect("the enclosing build observes no abort");
        (
            node,
            completeness.is_partial().then(|| completeness.reasons()),
        )
    }
}

/// A helper's own stack, kept at the read it requests.
pub(super) struct HelperRun<'e> {
    frames: Vec<HelperFrame<'e>>,
}

pub(super) enum HelperFrame<'e> {
    Carrier(Box<IdentityCarrierRead>),
    Lower(Box<FlowLowering<'e>>),
    TruthinessArms(Box<TruthinessArms>),
    MemberPath(MemberPathFrame<'e>),
    List(
        Box<super::super::continuation_stages::SignatureListRead>,
        Option<ReadWait>,
    ),
    Indexed(Box<IndexedArgumentFrame>),
    Provenance(Box<ProvenanceFrame>),
}

/// The lowering a provenance frame is parked at, borrowing the planned type
/// it lowers from the frame's own plan.
type ParkedLowering<'p> = Option<Box<FlowLowering<'p>>>;

self_cell::self_cell!(
    /// A call argument's provenance plan beside the lowering of its planned
    /// type that waits on a read, which borrows that type: one allocation
    /// the frame owns, never a borrow of another frame. Dropping it drops
    /// the lowering before the plan it borrows.
    pub(super) struct PlannedLowerings {
        owner: super::super::call_resolve::ProvenancePlan,

        #[not_covariant]
        dependent: ParkedLowering,
    }
);

// A parked lowering answers exactly one delivery: a copy would answer it
// twice.
static_assertions::assert_not_impl_any!(PlannedLowerings: Clone);

/// A call argument's literal provenance between the lowerings its plan
/// lists: each planned type is lowered in the plan's owner scope through
/// the stepping lowerer, in order, and the plan is finished once every one
/// has its node.
pub(super) struct ProvenanceFrame {
    plan: PlannedLowerings,
    /// The nodes of the planned types lowered so far: the next lowering is
    /// the plan's at this length.
    lowered: Vec<SemanticNodeId>,
    /// The read the frame's run requests first, before any delivery.
    wait: Option<ReadWait>,
}

impl ProvenanceFrame {
    /// Begin lowering `plan`'s types: the finished provenance when each
    /// lowers without a read, otherwise the frame and the read it waits on.
    pub(super) fn begin(
        dispatch: &impl FlowDemandDriver,
        plan: super::super::call_resolve::ProvenancePlan,
    ) -> ReadStep<Option<Arc<crate::semantic_query::ArgumentLiteralProvenance>>, Box<Self>> {
        let lowered = Vec::with_capacity(plan.lowerings().len());
        let mut frame = Box::new(Self {
            plan: PlannedLowerings::new(plan, |_| None),
            lowered,
            wait: None,
        });
        match frame.advance(dispatch, None) {
            ReadStep::Ready(()) => ReadStep::Ready((*frame).finish(dispatch)),
            ReadStep::Wait(wait) => {
                frame.wait = Some(wait);
                ReadStep::Wait(frame)
            }
        }
    }

    /// Hand the parked lowering its `delivery`, then request each planned
    /// type's lowering in turn: `Done` keeps its node, `Wait` parks the
    /// lowering at the read it requests.
    fn advance(
        &mut self,
        dispatch: &impl FlowDemandDriver,
        delivery: Option<ReadDelivery>,
    ) -> ReadStep<()> {
        let lowered = &mut self.lowered;
        self.plan.with_dependent_mut(|plan, parked| {
            if let Some(delivery) = delivery {
                let lowering = parked
                    .as_mut()
                    .expect("a delivered read answers the parked lowering");
                match dispatch.deliver_lowering(lowering, delivery) {
                    ReadStep::Wait(wait) => return ReadStep::Wait(provenance_wait(wait)),
                    ReadStep::Ready(node) => {
                        *parked = None;
                        lowered.push(node);
                    }
                }
            }
            while let Some(ty) = plan.lowerings().get(lowered.len()) {
                match dispatch.request_owner_lowering(plan.owner_scope(), ty, plan.context()) {
                    LowerStart::Done(node) => lowered.push(node),
                    LowerStart::Wait(lowering, wait) => {
                        *parked = Some(lowering);
                        return ReadStep::Wait(provenance_wait(wait));
                    }
                }
            }
            ReadStep::Ready(())
        })
    }

    /// The provenance the plan describes over its lowered nodes.
    fn finish(
        self,
        dispatch: &impl FlowDemandDriver,
    ) -> Option<Arc<crate::semantic_query::ArgumentLiteralProvenance>> {
        let Self { plan, lowered, .. } = self;
        dispatch.provenance_from(plan.into_owner(), &lowered)
    }
}

/// A provenance lowering's read, attributed in test builds to the frame
/// that requested it.
#[cfg(any(test, feature = "test-support"))]
fn provenance_wait(mut wait: ReadWait) -> ReadWait {
    wait.origin = ReadOrigin::ArgumentProvenance;
    wait
}

#[cfg(not(any(test, feature = "test-support")))]
fn provenance_wait(wait: ReadWait) -> ReadWait {
    wait
}

pub(super) struct IndexedArgumentFrame {
    indexed: Arc<verter_session_query::source::indexed_call::IndexedFlowCallExpression>,
    ordinal: usize,
    run: IndexedValueRun,
    wait: Option<ReadWait>,
}

impl IndexedArgumentFrame {
    pub(super) fn waiting(
        indexed: Arc<verter_session_query::source::indexed_call::IndexedFlowCallExpression>,
        ordinal: usize,
        run: IndexedValueRun,
        wait: ReadWait,
    ) -> Self {
        Self {
            indexed,
            ordinal,
            run,
            wait: Some(wait),
        }
    }
}

/// Only helpers that read, or call a reading helper, have frames. Pure
/// finishing work runs when its child's value is delivered.
pub(super) enum MemberPathFrame<'e> {
    Segments(SemanticNodeId, &'e [Arc<str>]),
    Parent(&'e Arc<str>),
    Optional(bool),
    Navigate(SemanticNodeId, &'e [Arc<str>]),
    Unbound(SemanticNodeId, &'e [Arc<str>]),
    Union(Box<MemberUnion<'e>>),
    Class(SemanticNodeId, &'e [Arc<str>]),
    Members(SemanticNodeId, &'e [Arc<str>]),
    Receiver(SemanticNodeId, &'e [Arc<str>]),
    Through(SemanticNodeId, &'e [Arc<str>]),
    Numeric(SemanticNodeId, &'e [Arc<str>]),
    NumericMember(Box<NumericMember<'e>>),
    Read(SemanticNodeId, &'e [Arc<str>]),
}

pub(super) struct MemberUnion<'e> {
    arms: Vec<SemanticNodeId>,
    next: usize,
    reads: Vec<SemanticNodeId>,
    path: &'e [Arc<str>],
}

pub(super) struct NumericMember<'e> {
    base: SemanticNodeId,
    segment: &'e Arc<str>,
    rest: &'e [Arc<str>],
}

/// The arm cursor survives each carrier read; classification follows that
/// walk's answer before the next arm begins.
pub(super) struct TruthinessArms {
    arms: Vec<SemanticNodeId>,
    next: usize,
    pub(super) survivors: Vec<SemanticNodeId>,
    pub(super) dropped: bool,
    pub(super) undecided: bool,
    negated: bool,
    split_boolean: bool,
}

pub(super) enum HelperValue {
    Node(Option<SemanticNodeId>),
    List(Option<Box<[SemanticNodeId]>>),
    Carrier(IdentityCarrierUnwrap),
    Truthiness(Box<TruthinessArms>),
    Indexed(Option<IndexedValue>),
    Provenance(Option<Arc<crate::semantic_query::ArgumentLiteralProvenance>>),
}

impl HelperValue {
    pub(super) fn node(self) -> Option<SemanticNodeId> {
        match self {
            Self::Node(node) => node,
            Self::List(_)
            | Self::Carrier(_)
            | Self::Truthiness(_)
            | Self::Indexed(_)
            | Self::Provenance(_) => {
                unreachable!("a member path answers a node")
            }
        }
    }

    fn list(self) -> Option<Vec<SemanticNodeId>> {
        match self {
            Self::List(nodes) => nodes.map(|nodes| nodes.into_vec()),
            Self::Node(_)
            | Self::Carrier(_)
            | Self::Truthiness(_)
            | Self::Indexed(_)
            | Self::Provenance(_) => {
                unreachable!("a signature list answers signature nodes")
            }
        }
    }

    fn carrier(self) -> IdentityCarrierUnwrap {
        match self {
            Self::Carrier(done) => done,
            Self::Node(_)
            | Self::List(_)
            | Self::Truthiness(_)
            | Self::Indexed(_)
            | Self::Provenance(_) => {
                unreachable!("a carrier walk answers its settled node")
            }
        }
    }

    fn truthiness(self) -> Box<TruthinessArms> {
        match self {
            Self::Truthiness(arms) => arms,
            Self::Node(_)
            | Self::List(_)
            | Self::Carrier(_)
            | Self::Indexed(_)
            | Self::Provenance(_) => {
                unreachable!("a truthiness loop answers its surviving arms")
            }
        }
    }

    pub(super) fn indexed(self) -> Option<IndexedValue> {
        match self {
            Self::Indexed(value) => value,
            Self::Node(_)
            | Self::List(_)
            | Self::Carrier(_)
            | Self::Truthiness(_)
            | Self::Provenance(_) => {
                unreachable!("an indexed read answers an indexed value")
            }
        }
    }

    pub(super) fn provenance(
        self,
    ) -> Option<Arc<crate::semantic_query::ArgumentLiteralProvenance>> {
        match self {
            Self::Provenance(provenance) => provenance,
            Self::Node(_)
            | Self::List(_)
            | Self::Carrier(_)
            | Self::Truthiness(_)
            | Self::Indexed(_) => {
                unreachable!("a provenance frame answers its argument's provenance")
            }
        }
    }
}

pub(super) enum AfterHelper<'e> {
    MemberOf {
        object: SemanticNodeId,
        member: &'e Arc<str>,
        span: verter_span::Span,
    },
    ClassBase(
        Box<class_expression::ClassEvalFrame<'e>>,
        Box<class_expression::ClassBaseRead>,
    ),
    IndexedArgument(
        Box<CallInFlight<'e>>,
        Box<call_stack::IndexedArgumentRead<'e>>,
    ),
    /// An argument whose literal provenance's planned lowerings request
    /// their reads, with its call's route retained.
    ArgumentProvenance(
        Box<CallInFlight<'e>>,
        Box<call_stack::ProvenanceArgumentRead<'e>>,
    ),
    /// A type leaf whose lowering waits: the lowered carrier then takes the
    /// leaf's closed-utility read.
    TypeLeaf(&'e verter_session_query::flow::slice::GatedLeaf),
}

pub(super) struct HelperWait<'e> {
    run: HelperRun<'e>,
    then: AfterHelper<'e>,
    wait: Option<ReadWait>,
}

/// A return statement retained at the helper its expression requested.
pub(super) struct PendingHelper<'e> {
    pub(super) helper: Box<HelperWait<'e>>,
    pub(super) then: Box<PendingReturn<'e>>,
}

impl<'e> Pending<'e> {
    pub(super) fn returning(mut then: Box<PendingReturn<'e>>) -> Self {
        if matches!(
            then.run.waiting.last(),
            Some(Waiting::Read(ReadSite::Helper(_)))
        ) {
            let Some(Waiting::Read(ReadSite::Helper(helper))) = then.run.waiting.pop() else {
                unreachable!("the return is suspended at its helper")
            };
            Self::Helper(Box::new(PendingHelper { helper, then }))
        } else {
            Self::Return(then)
        }
    }
}

impl<'e> HelperRun<'e> {
    pub(super) fn indexed_argument(frame: Box<IndexedArgumentFrame>) -> Self {
        Self {
            frames: vec![HelperFrame::Indexed(frame)],
        }
    }

    pub(super) fn argument_provenance(frame: Box<ProvenanceFrame>) -> Self {
        Self {
            frames: vec![HelperFrame::Provenance(frame)],
        }
    }

    fn at(frame: MemberPathFrame<'e>) -> Self {
        Self {
            frames: vec![HelperFrame::MemberPath(frame)],
        }
    }

    fn push(&mut self, frame: MemberPathFrame<'e>) {
        self.frames.push(HelperFrame::MemberPath(frame));
    }
}

impl<'e> HelperWait<'e> {
    pub(super) fn indexed_argument(
        flight: Box<CallInFlight<'e>>,
        mut argument: Box<call_stack::IndexedArgumentRead<'e>>,
    ) -> Self {
        let run = HelperRun::indexed_argument(argument.take_frame());
        Self {
            run,
            then: AfterHelper::IndexedArgument(flight, argument),
            wait: None,
        }
    }

    pub(super) fn argument_provenance(
        flight: Box<CallInFlight<'e>>,
        mut argument: Box<call_stack::ProvenanceArgumentRead<'e>>,
    ) -> Self {
        let run = HelperRun::argument_provenance(argument.take_frame());
        Self {
            run,
            then: AfterHelper::ArgumentProvenance(flight, argument),
            wait: None,
        }
    }

    pub(super) fn class_base(
        frame: Box<class_expression::ClassEvalFrame<'e>>,
        base: class_expression::ClassBaseWait,
    ) -> Self {
        let class_expression::ClassBaseWait { base, list, wait } = base;
        Self {
            run: HelperRun {
                frames: vec![HelperFrame::List(Box::new(list), Some(wait))],
            },
            then: AfterHelper::ClassBase(frame, Box::new(base)),
            wait: None,
        }
    }

    pub(super) fn type_leaf(
        lowering: Box<FlowLowering<'e>>,
        leaf: &'e verter_session_query::flow::slice::GatedLeaf,
    ) -> Self {
        Self {
            run: HelperRun {
                frames: vec![HelperFrame::Lower(lowering)],
            },
            then: AfterHelper::TypeLeaf(leaf),
            wait: None,
        }
    }

    pub(super) fn member_of(
        object: SemanticNodeId,
        member: &'e Arc<str>,
        span: verter_span::Span,
    ) -> Self {
        Self {
            run: HelperRun::at(MemberPathFrame::Segments(
                object,
                std::slice::from_ref(member),
            )),
            then: AfterHelper::MemberOf {
                object,
                member,
                span,
            },
            wait: None,
        }
    }
}

static_assertions::const_assert!(std::mem::size_of::<HelperValue>() <= 24);
static_assertions::const_assert!(std::mem::size_of::<HelperFrame<'static>>() <= 32);
static_assertions::const_assert!(std::mem::size_of::<IndexedValueDelivery<'static>>() <= 24);

impl<D: FlowDemandDriver> FlowEvaluator<'_, '_, '_, D> {
    /// Advance the standing chain exactly once, to its next read or value.
    pub(super) fn run_helper<'e>(
        &mut self,
        run: &mut HelperRun<'e>,
        delivered: Option<ReadDelivery>,
    ) -> ReadStep<HelperValue> {
        use MemberPathFrame as Frame;
        let mut delivered = delivered;
        let mut carrier = None;
        let mut value = if matches!(
            run.frames.last(),
            Some(
                HelperFrame::List(..)
                    | HelperFrame::Carrier(_)
                    | HelperFrame::Indexed(_)
                    | HelperFrame::Lower(_)
                    | HelperFrame::Provenance(_)
            )
        ) {
            None
        } else {
            delivered.take().map(ReadDelivery::node)
        };
        loop {
            let Some(frame) = run.frames.pop() else {
                return ReadStep::Ready(HelperValue::Node(
                    value.expect("a finished helper has a value"),
                ));
            };
            let frame = match frame {
                HelperFrame::Carrier(mut walk) => {
                    match self.dispatch.deliver_carrier(
                        &mut walk,
                        delivered
                            .take()
                            .expect("the carrier receives its requested read"),
                        self.st.canonical,
                    ) {
                        ReadStep::Wait(wait) => {
                            run.frames.push(HelperFrame::Carrier(walk));
                            return ReadStep::Wait(wait);
                        }
                        ReadStep::Ready(done) => {
                            self.dispatch.release_carrier(walk);
                            if run.frames.is_empty() {
                                return ReadStep::Ready(HelperValue::Carrier(done));
                            }
                            carrier = Some(done);
                            continue;
                        }
                    }
                }
                HelperFrame::TruthinessArms(mut arms) => {
                    if let Some(done) = carrier.take() {
                        self.truthiness_arm_delivered(&mut arms, done);
                    }
                    while let Some(arm) = arms.arms.get(arms.next).copied() {
                        arms.next += 1;
                        if arms.split_boolean
                            && matches!(
                                self.dispatch.arena().node_data(arm).as_deref(),
                                Some(SemanticNodeData::Primitive(PrimitiveKind::Boolean))
                            )
                        {
                            if arms.negated && !self.st.nullability.is_strict() {
                                arms.survivors.push(arm);
                            } else {
                                arms.survivors.push(self.dispatch.graph().intern_node(
                                    SemanticNodeData::Literal(
                                        crate::semantic_query::LiteralValue::Boolean(!arms.negated),
                                    ),
                                ));
                                arms.dropped = true;
                            }
                            continue;
                        }
                        match self.dispatch.request_carrier(arm) {
                            CarrierStep::Settled(done) => {
                                self.truthiness_arm_delivered(&mut arms, done)
                            }
                            CarrierStep::Wait(walk, wait) => {
                                run.frames.push(HelperFrame::TruthinessArms(arms));
                                run.frames.push(HelperFrame::Carrier(walk));
                                return ReadStep::Wait(wait);
                            }
                        }
                    }
                    return ReadStep::Ready(HelperValue::Truthiness(arms));
                }
                HelperFrame::Lower(mut lowering) => {
                    let delivery = delivered
                        .take()
                        .expect("the lowering receives its requested read");
                    return match self.dispatch.deliver_lowering(&mut lowering, delivery) {
                        ReadStep::Wait(wait) => {
                            run.frames.push(HelperFrame::Lower(lowering));
                            ReadStep::Wait(wait)
                        }
                        ReadStep::Ready(node) => ReadStep::Ready(HelperValue::Node(Some(node))),
                    };
                }
                HelperFrame::Indexed(mut frame) => {
                    let step = match frame.wait.take() {
                        Some(wait) => ReadStep::Wait(wait),
                        None => self.dispatch.advance_indexed_value(
                            self.st.canonical,
                            self.st.owner,
                            &frame.indexed.call.args[frame.ordinal].expression,
                            &mut frame.run,
                            delivered.take(),
                        ),
                    };
                    return match step {
                        ReadStep::Ready(value) => ReadStep::Ready(HelperValue::Indexed(value)),
                        ReadStep::Wait(wait) => {
                            run.frames.push(HelperFrame::Indexed(frame));
                            ReadStep::Wait(wait)
                        }
                    };
                }
                HelperFrame::Provenance(mut frame) => {
                    let step = match frame.wait.take() {
                        Some(wait) => ReadStep::Wait(wait),
                        None => frame.advance(self.dispatch, delivered.take()),
                    };
                    return match step {
                        ReadStep::Ready(()) => {
                            ReadStep::Ready(HelperValue::Provenance((*frame).finish(self.dispatch)))
                        }
                        ReadStep::Wait(wait) => {
                            run.frames.push(HelperFrame::Provenance(frame));
                            ReadStep::Wait(wait)
                        }
                    };
                }
                HelperFrame::List(mut list, first) => {
                    let step = match first {
                        Some(wait) => ReadStep::Wait(wait),
                        None => self.dispatch.deliver_signature_list(
                            &mut list,
                            delivered
                                .take()
                                .expect("the list receives its requested read"),
                        ),
                    };
                    return match step {
                        ReadStep::Wait(wait) => {
                            run.frames.push(HelperFrame::List(list, None));
                            ReadStep::Wait(wait)
                        }
                        ReadStep::Ready(nodes) => {
                            ReadStep::Ready(HelperValue::List(nodes.map(Vec::into_boxed_slice)))
                        }
                    };
                }
                HelperFrame::MemberPath(frame) => frame,
            };
            let bind_this = !matches!(&frame, Frame::Unbound(..));
            match frame {
                Frame::Segments(base, path) => {
                    let Some((terminal, prefix)) = path.split_last() else {
                        value = Some(Some(base));
                        continue;
                    };
                    run.push(Frame::Parent(terminal));
                    if prefix.is_empty() {
                        value = Some(Some(base));
                    } else {
                        run.push(Frame::Navigate(base, prefix));
                    }
                }
                Frame::Parent(terminal) => {
                    let Some(parent) = value.take().expect("the path's parent answered") else {
                        value = Some(None);
                        continue;
                    };
                    let optional = self.member_read_optionality(
                        self.member_declaring_surface(parent, terminal),
                        terminal.as_ref(),
                    ) == Some(true);
                    run.push(Frame::Optional(optional));
                    run.push(Frame::Navigate(parent, std::slice::from_ref(terminal)));
                }
                Frame::Optional(optional) => {
                    value = Some(
                        value
                            .take()
                            .expect("the terminal member answered")
                            .map(|node| {
                                if optional {
                                    self.fold_optional_read_undefined(node)
                                } else {
                                    node
                                }
                            }),
                    );
                }
                Frame::Navigate(base, path) | Frame::Unbound(base, path) => {
                    if let Some((first, rest)) = path.split_first() {
                        let base = self
                            .dispatch
                            .self_reference_declaration(base)
                            .unwrap_or(base);
                        let receiver = self.dispatch.type_parameter_apparent_receiver(base);
                        if receiver != base {
                            run.push(Frame::Navigate(receiver, path));
                            continue;
                        }
                        if let Some(arms) = self.class_reference_union_arms(base, first) {
                            let reads = Vec::with_capacity(arms.len());
                            run.push(Frame::Union(Box::new(MemberUnion {
                                arms,
                                next: 0,
                                reads,
                                path,
                            })));
                            continue;
                        }
                        if let Some(source) = self.class_reference_member_source(base, first) {
                            run.push(Frame::Class(base, rest));
                            run.push(Frame::Through(source, std::slice::from_ref(first)));
                            continue;
                        }
                    }
                    run.push(if bind_this {
                        Frame::Members(base, path)
                    } else {
                        Frame::Through(base, path)
                    });
                }
                Frame::Union(mut union) => {
                    if union.next != 0 {
                        let Some(node) = value.take().expect("the union arm answered") else {
                            value = Some(None);
                            continue;
                        };
                        union.reads.push(node);
                    }
                    if let Some(arm) = union.arms.get(union.next).copied() {
                        union.next += 1;
                        let path = union.path;
                        run.push(Frame::Union(union));
                        run.push(Frame::Navigate(arm, path));
                    } else {
                        value = Some(Some(
                            self.dispatch
                                .intern_normalized_union_or_intersection(&union.reads, true),
                        ));
                    }
                }
                Frame::Class(base, rest) => {
                    let Some(node) = value.take().expect("the class member answered") else {
                        value = Some(None);
                        continue;
                    };
                    let node = self.dispatch.bind_this_receiver(node, base);
                    if rest.is_empty() {
                        value = Some(Some(node));
                    } else {
                        run.push(Frame::Navigate(node, rest));
                    }
                }
                Frame::Members(base, path) => {
                    if let Some((first, rest)) = path.split_first() {
                        run.push(Frame::Receiver(base, rest));
                        run.push(Frame::Through(base, std::slice::from_ref(first)));
                    } else {
                        value = Some(Some(base));
                    }
                }
                Frame::Receiver(receiver, rest) => {
                    let Some(node) = value.take().expect("the receiver's member answered") else {
                        value = Some(None);
                        continue;
                    };
                    let node = if self.dispatch.receiver_this_types(node).is_empty() {
                        node
                    } else {
                        self.dispatch.bind_callee_receiver(node, receiver)
                    };
                    run.push(Frame::Members(node, rest));
                }
                Frame::Through(base, path) => {
                    run.push(
                        if path
                            .iter()
                            .any(|segment| numeric_member_name(segment).is_some())
                        {
                            Frame::Numeric(base, path)
                        } else {
                            Frame::Read(base, path)
                        },
                    );
                }
                Frame::Numeric(base, path) => {
                    if let Some((segment, rest)) = path.split_first() {
                        run.push(Frame::NumericMember(Box::new(NumericMember {
                            base,
                            segment,
                            rest,
                        })));
                        run.push(Frame::Read(base, std::slice::from_ref(segment)));
                    } else {
                        value = Some(Some(base));
                    }
                }
                Frame::NumericMember(member) => {
                    let Some(node) = value.take().expect("the numeric member answered") else {
                        value = Some(None);
                        continue;
                    };
                    let node = match numeric_member_name(member.segment) {
                        Some(number)
                            if matches!(
                                self.dispatch.arena().node_data(node).as_deref(),
                                Some(SemanticNodeData::Opaque(_))
                            ) =>
                        {
                            let index =
                                self.dispatch.graph().intern_node(SemanticNodeData::Literal(
                                    crate::semantic_query::LiteralValue::Number(number),
                                ));
                            let Some(node) = self.indexed_access(member.base, index) else {
                                value = Some(None);
                                continue;
                            };
                            node
                        }
                        _ => node,
                    };
                    run.push(Frame::Numeric(node, member.rest));
                }
                Frame::Read(base, path) => {
                    let mut wait = self.dispatch.request(
                        Self::member_path_key(base, path),
                        ReadShape::Node,
                        ReadOrigin::MemberPath,
                    );
                    wait.lexical = true;
                    return ReadStep::Wait(wait);
                }
            }
        }
    }

    fn truthiness_arm_delivered(&mut self, arms: &mut TruthinessArms, done: IdentityCarrierUnwrap) {
        use crate::semantic_query::TruthinessInhabitance;
        let arm = arms.arms[arms.next - 1];
        match self.classify_truthiness_edge(done, arms.negated) {
            TruthinessInhabitance::No => arms.dropped = true,
            TruthinessInhabitance::Yes => arms.survivors.push(arm),
            TruthinessInhabitance::Undecided => {
                arms.undecided = true;
                if arms.split_boolean {
                    self.record_degradation(FlowReturnDegradation::FlowGap(
                        verter_session_query::flow::policy::FlowGap::GuardNarrowing,
                    ));
                }
                arms.survivors.push(arm);
            }
        }
    }

    pub(super) fn truthiness_arms_in_place(
        &mut self,
        arms: Vec<SemanticNodeId>,
        negated: bool,
        split_boolean: bool,
    ) -> Box<TruthinessArms> {
        #[cfg(any(test, feature = "test-support"))]
        let _stage = body_reads::enter_call_stage("truthiness_arms");
        let survivors = Vec::with_capacity(arms.len() + usize::from(split_boolean));
        let mut run = HelperRun {
            frames: vec![HelperFrame::TruthinessArms(Box::new(TruthinessArms {
                arms,
                next: 0,
                survivors,
                dropped: false,
                undecided: false,
                negated,
                split_boolean,
            }))],
        };
        let first = self.run_helper(&mut run, None);
        self.settle_now(first, |this, delivery| {
            this.run_helper(&mut run, Some(delivery))
        })
        .truthiness()
    }

    pub(super) fn truthiness_carrier_in_place(
        &mut self,
        arm: SemanticNodeId,
    ) -> IdentityCarrierUnwrap {
        match self.dispatch.request_carrier(arm) {
            CarrierStep::Settled(done) => done,
            CarrierStep::Wait(walk, wait) => {
                let mut run = HelperRun {
                    frames: vec![HelperFrame::Carrier(walk)],
                };
                self.settle_now(ReadStep::Wait(wait), |this, delivery| {
                    this.run_helper(&mut run, Some(delivery))
                })
                .carrier()
            }
        }
    }

    pub(super) fn member_path_in_place(
        &mut self,
        frame: MemberPathFrame<'_>,
    ) -> Option<SemanticNodeId> {
        let mut run = HelperRun::at(frame);
        let first = self.run_helper(&mut run, None);
        self.settle_now(first, |this, delivery| {
            this.run_helper(&mut run, Some(delivery))
        })
        .node()
    }

    pub(super) fn project_member_path(
        &mut self,
        base: SemanticNodeId,
        path: &[Arc<str>],
    ) -> Option<SemanticNodeId> {
        self.member_path_in_place(MemberPathFrame::Read(base, path))
    }

    fn after_helper_on_run<'e>(
        &mut self,
        run: &mut ExprRun<'e>,
        then: AfterHelper<'e>,
        value: HelperValue,
        suspend: bool,
    ) -> ResumedCall {
        match then {
            AfterHelper::IndexedArgument(flight, argument) => {
                let progress = self.deliver_indexed_call_argument(argument, value.indexed());
                let step = self.continue_call_route(flight, progress);
                self.call_step_on_run(run, step, suspend)
            }
            AfterHelper::ArgumentProvenance(flight, argument) => {
                let progress = self.deliver_argument_provenance(argument, value.provenance());
                let step = self.continue_call_route(flight, progress);
                self.call_step_on_run(run, step, suspend)
            }
            AfterHelper::MemberOf {
                object,
                member,
                span,
            } => ResumedCall::Value(self.finish_member_of(object, member, span, value.node())),
            AfterHelper::ClassBase(mut frame, base) => {
                let step = self.class_base_delivered(&mut frame, *base, value.list());
                self.class_step_on_run(run, *frame, step, suspend)
            }
            AfterHelper::TypeLeaf(leaf) => {
                let carrier = value.node().expect("a lowering yields a node");
                let node = if suspend && body_requests_reads() {
                    match self.type_leaf_lowered(run, leaf, carrier) {
                        ReadStep::Ready(node) => node,
                        ReadStep::Wait(wait) => return ResumedCall::Wait(BodyWait::Read(wait)),
                    }
                } else {
                    self.settle_now(self.body_type_read(carrier), |_, delivery| {
                        ReadStep::Ready(delivery.node().unwrap_or(carrier))
                    })
                };
                ResumedCall::Value(self.finish_type_leaf(leaf, node))
            }
        }
    }

    pub(super) fn member_of_in_place(
        &mut self,
        object: SemanticNodeId,
        member: &Arc<str>,
        span: verter_span::Span,
    ) -> Positional<SemanticNodeId> {
        let mut helper = HelperWait::member_of(object, member, span);
        let first = self.run_helper(&mut helper.run, None);
        let value = self.settle_now(first, |this, delivery| {
            this.run_helper(&mut helper.run, Some(delivery))
        });
        self.finish_member_of(object, member, span, value.node())
    }

    pub(super) fn helper_step_on_run<'e>(
        &mut self,
        run: &mut ExprRun<'e>,
        mut helper: Box<HelperWait<'e>>,
        delivered: Option<ReadDelivery>,
        suspend: bool,
    ) -> ResumedCall {
        let first = self.run_helper(&mut helper.run, delivered);
        if suspend && body_requests_reads() {
            match first {
                ReadStep::Wait(wait) => {
                    helper.wait = Some(wait);
                    let wait = helper
                        .wait
                        .take()
                        .expect("a helper requests its waiting read once");
                    run.waiting.push(Waiting::Read(ReadSite::Helper(helper)));
                    ResumedCall::Wait(BodyWait::Read(wait))
                }
                ReadStep::Ready(value) => {
                    self.after_helper_on_run(run, helper.then, value, suspend)
                }
            }
        } else {
            let value = self.settle_now(first, |this, delivery| {
                this.run_helper(&mut helper.run, Some(delivery))
            });
            self.after_helper_on_run(run, helper.then, value, suspend)
        }
    }
}
