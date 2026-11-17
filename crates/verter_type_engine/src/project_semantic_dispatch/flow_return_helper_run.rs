//! Retained positions in reading helper chains, shared by both drivers.

use super::*;

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
}

/// A helper's own stack, kept at the read it requests.
pub(super) struct HelperRun<'e> {
    frames: Vec<HelperFrame<'e>>,
}

pub(super) enum HelperFrame<'e> {
    MemberPath(MemberPathFrame<'e>),
    List(
        Box<super::super::continuation_stages::SignatureListRead>,
        Option<ReadWait>,
    ),
    Indexed(Box<IndexedArgumentFrame>),
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

pub(super) enum HelperValue {
    Node(Option<SemanticNodeId>),
    List(Option<Box<[SemanticNodeId]>>),
    Indexed(Option<IndexedValue>),
}

impl HelperValue {
    pub(super) fn node(self) -> Option<SemanticNodeId> {
        match self {
            Self::Node(node) => node,
            Self::List(_) | Self::Indexed(_) => unreachable!("a member path answers a node"),
        }
    }

    fn list(self) -> Option<Vec<SemanticNodeId>> {
        match self {
            Self::List(nodes) => nodes.map(|nodes| nodes.into_vec()),
            Self::Node(_) | Self::Indexed(_) => {
                unreachable!("a signature list answers signature nodes")
            }
        }
    }

    pub(super) fn indexed(self) -> Option<IndexedValue> {
        match self {
            Self::Indexed(value) => value,
            Self::Node(_) | Self::List(_) => {
                unreachable!("an indexed read answers an indexed value")
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
        let mut value = if matches!(
            run.frames.last(),
            Some(HelperFrame::List(..) | HelperFrame::Indexed(_))
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
            AfterHelper::MemberOf {
                object,
                member,
                span,
            } => ResumedCall::Value(self.finish_member_of(object, member, span, value.node())),
            AfterHelper::ClassBase(mut frame, base) => {
                let step = self.class_base_delivered(&mut frame, *base, value.list());
                self.class_step_on_run(run, *frame, step, suspend)
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
