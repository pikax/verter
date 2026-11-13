//! `substitute_semantic_type_param` — generic type-parameter
//! substitution into the semantic graph.
//!
//! Substitution is one owned heap walk ([`SubstitutionWalk`]): the levels
//! whose memo probe missed, root first, each holding the node payload it
//! read once, the children it visits in order and the values they
//! substituted to. A level opens its children, hands them on one at a time,
//! and rebuilds once they are done. A level none of whose children changed
//! returns its own node, skipping the rebuild, `intern_preserving_scope` and
//! the per-arm allocations; the output is identical to an all-rebuild walk
//! (the shard dedup collapses identical rebuilds back to the same id).
//!
//! One walk, two drivers. The public entry drives it in place, answering
//! the one read a level names — its template literal's reduction — with
//! [`ProjectSemanticDispatch::execute_read`] where the walk stands, exactly
//! as the walk always read it. A continuation holding the walk (an explicit
//! instantiation inside an overload set) returns that read to its own
//! driver and resumes with its delivery
//! ([`ProjectSemanticDispatch::resume_substitution`]); nothing the walk
//! computed before the read runs again. Every other read a rebuild makes —
//! the intersection funnel, an index key's evaluation, `NoInfer`'s
//! genericness — is made in place by both drivers.
//!
//! The walk's levels record their cost on the connected demand's recording
//! stack with an open scope each, sealed or abandoned as the level
//! finishes; a walk waiting on its read keeps them open above its holder's
//! recording, and a walk released while it waits abandons them top first
//! ([`ProjectSemanticDispatch::release_substitution_walk`]).
//!
//! Both helpers operate on immutable `SemanticNodeData`. Non-composite
//! shells (objects, arrays, tuples, aliases, `MergedDecl`, ...) publish
//! rebuilt identity via [`SemanticGraphStore::intern_preserving_scope`]
//! so the rebuilt shell's scope is preserved from the origin shell.
//! Composite rebuilds SPLIT by carrier semantics: a changed union and a
//! provably order-safe changed intersection are DERIVED composites and
//! route through the canonical authority, which interns a multi-arm
//! result under `Global` BY DESIGN (a derived composite has no lexical
//! scope; its file dependence rides the canonical evidence deposit and
//! observed self-roots, and the evidence-blind-replay fence below keeps
//! the cross-request memo from replaying an under-rooted entry) — while
//! a possibly-callable changed intersection is an overload-ordered
//! carrier and keeps `intern_preserving_scope`. The caller's completion
//! fence observes the new dep-signature through the shared memo once
//! the substituted result enters a build flow.
//!
//! A class's polymorphic `this` rides a [`SemanticNodeData::TypeParam`]
//! at [`THIS_BINDER_INDEX`] (see `this_binder`).
//!
//! **Binder identity contract.** Binder matching is done by
//! `SemanticNodeId` equality (the binder's interned `TypeParam`
//! node id) rather than by `display_name` string equality. This
//! makes substitution correct even when two binders in the same
//! file share a display name (`K`) but are otherwise distinct
//! identities — the substitute only touches the binder whose node
//! id matches the caller's `parameter_node` argument.

/// The clause position a class's polymorphic `this` binder carries: no
/// authored clause has this many parameters, so it names no authored one.
pub(crate) const THIS_BINDER_INDEX: u16 = u16::MAX;

/// The declaration name of the `this` binder an object literal's methods
/// read the literal through: the literal's start offset identifies it.
pub(crate) fn object_literal_this_name(offset: u32) -> String {
    format!("(object literal)@{offset}")
}

use std::sync::Arc;
use verter_type_expr::OwnedStr;

use super::ProjectSemanticDispatch;
use crate::request_context::RecursiveSubstituteIdentity;
use crate::semantic_query::{
    CacheRead, ConditionalPendingSubstitution, FunctionParam, IndexKey, IndexSignature, MapperKey,
    PendingSubstitutionFrame, PrimitiveKind, QueryError, QueryResult, SemanticNodeData,
    SemanticNodeId, SemanticQueryKey, SurfaceMember, SurfaceView, TypeParamDecl,
};

/// The identity of one substitution walk, `(node, parameter, arg)`: the
/// memo entry its receipt prices. A public substitution and the walk at its
/// root are one computation, so they share one receipt and one entry.
#[derive(Clone, Copy, Hash, PartialEq, Eq)]
struct SubstitutionCost(SemanticNodeId, SemanticNodeId, SemanticNodeId);

impl<'a, C: crate::resolver_core::ResolverCapabilities> ProjectSemanticDispatch<'a, C> {
    pub(super) fn type_parameter_origin(
        &self,
        parameter: SemanticNodeId,
    ) -> Option<verter_session_query::retention::owned::Owned<crate::semantic_query::TypeParamOrigin>>
    {
        let data = self.graph().node_data(parameter)?;
        let SemanticNodeData::TypeParam {
            decl, param_index, ..
        } = data.as_ref()
        else {
            return None;
        };
        Some(verter_session_query::retention::owned::Owned::new(
            crate::semantic_query::TypeParamOrigin {
                declaration: decl.clone(),
                index: *param_index,
            },
        ))
    }

    /// A deterministic alpha identity outside the surrounding binder slots.
    pub(super) fn capture_avoiding_parameter_decl(
        &self,
        origin: &crate::semantic_query::DeclIdentity,
        index: u16,
        forbidden: &[SemanticNodeId],
    ) -> crate::semantic_query::DeclIdentity {
        for ordinal in 1..=forbidden.len() + 1 {
            let mut candidate = origin.clone();
            candidate.decl_name =
                OwnedStr::from(format!("{}\u{1}capture{ordinal}", origin.decl_name));
            if !forbidden.iter().any(|node| {
                matches!(self.graph().node_data(*node).as_deref(),
                Some(SemanticNodeData::TypeParam { decl, param_index, .. })
                    if decl == &candidate && *param_index == index)
            }) {
                return candidate;
            }
        }
        unreachable!("more fresh slots than forbidden slots")
    }

    /// The read-free start of instantiating a signature's clause with
    /// distinct local parameters before an outer argument is inserted: the
    /// fresh parameter shells and the clause's variant occurrences, or
    /// `None` when the argument cannot be captured by the clause (a later
    /// substitution preserves those parameters unless its argument would be
    /// captured by them) or a clause parameter is no binder.
    fn begin_capture_avoidance(
        &self,
        signature: SemanticNodeId,
        parameters: &Arc<[TypeParamDecl]>,
        argument: SemanticNodeId,
    ) -> Option<CaptureAvoidance> {
        if parameters.is_empty() {
            return None;
        }
        let free = self.type_params_within(&[argument]);
        if parameters
            .iter()
            .all(|parameter| parameter.original.is_some())
            && !parameters
                .iter()
                .any(|parameter| free.contains(&parameter.param))
        {
            return None;
        }
        let graph = self.graph();
        let mut forbidden = self.type_params_within(&[signature, argument]);
        forbidden.extend(parameters.iter().map(|parameter| parameter.param));
        let families: Vec<_> = parameters
            .iter()
            .map(
                |parameter| match graph.node_data(parameter.param).as_deref() {
                    Some(SemanticNodeData::TypeParam {
                        decl, param_index, ..
                    }) => Some((decl.clone(), *param_index)),
                    _ => None,
                },
            )
            .collect::<Option<_>>()?;
        let mut shells = Vec::with_capacity(parameters.len());
        for (parameter, (origin, index)) in parameters.iter().zip(&families) {
            let decl = self.capture_avoiding_parameter_decl(origin, *index, &forbidden);
            let shell = graph.intern_preserving_scope(
                parameter.param,
                SemanticNodeData::TypeParam {
                    decl,
                    param_index: *index,
                    constraint: None,
                    default: None,
                    display_name: parameter.name.clone(),
                },
            );
            forbidden.push(shell);
            shells.push(shell);
        }
        // Bounded and bound-free representations name the same clause slot.
        let mut variants = Vec::new();
        let mut pending = vec![signature];
        let mut seen = rustc_hash::FxHashSet::default();
        while let Some(node) = pending.pop() {
            if !seen.insert(node) {
                continue;
            }
            let Some(data) = graph.node_data(node) else {
                continue;
            };
            if let SemanticNodeData::TypeParam {
                decl, param_index, ..
            } = data.as_ref()
            {
                if let Some(position) = families
                    .iter()
                    .position(|(origin, index)| origin == decl && index == param_index)
                {
                    variants.push((node, position));
                }
            }
            let _ = data.for_each_child(|child| pending.push(child));
        }
        Some(CaptureAvoidance {
            parameters: Arc::clone(parameters),
            shells,
            variants,
            fresh: Vec::with_capacity(parameters.len()),
            ordinal: 0,
            slot: CaptureSlot::Constraint,
            bindings: None,
            constraint: None,
            closure: None,
        })
    }

    /// Carry a signature's capture avoidance on to the next public
    /// substitution it makes: per clause parameter, the closure of its
    /// constraint and then of its default under the bindings fixed so far,
    /// the parameter re-interned with both; then the closure of the
    /// clause-free body under every fresh parameter, which is the signature
    /// the level substitutes instead. A shell the graph no longer reads
    /// ends the avoidance, and the level substitutes its own children.
    fn advance_capture(
        &self,
        signature: SemanticNodeId,
        capture: &mut CaptureAvoidance,
    ) -> CaptureStep {
        loop {
            if let Some(closure) = capture.closure.as_mut() {
                let bindings = capture
                    .bindings
                    .as_ref()
                    .expect("a capture closure runs under its bindings");
                if let Some((parameter, bound)) = closure.next(bindings.bindings()) {
                    return CaptureStep::Arrive(closure.node(), parameter, bound);
                }
                let value = closure.node();
                capture.closure = None;
                match capture.slot {
                    CaptureSlot::Constraint => {
                        capture.constraint = Some(value);
                        capture.slot = CaptureSlot::Default;
                    }
                    CaptureSlot::Default => {
                        if !self.capture_parameter(capture, Some(value)) {
                            return CaptureStep::Abort;
                        }
                    }
                    CaptureSlot::Body => {
                        let fresh = std::mem::take(&mut capture.fresh);
                        return CaptureStep::Fresh(
                            self.signature_with_type_parameters(value, fresh),
                        );
                    }
                }
                continue;
            }
            match capture.slot {
                CaptureSlot::Constraint => {
                    let Some(parameter) = capture.parameters.get(capture.ordinal) else {
                        capture.slot = CaptureSlot::Body;
                        continue;
                    };
                    let constraint = parameter.constraint;
                    let bindings = crate::semantic_query::CanonicalTypeSubstitution::new(
                        self.graph(),
                        capture
                            .variants
                            .iter()
                            .map(|&(node, position)| {
                                (
                                    node,
                                    capture
                                        .fresh
                                        .get(position)
                                        .map_or(capture.shells[position], |decl| decl.param),
                                )
                            })
                            .collect(),
                    );
                    capture.bindings = Some(bindings);
                    match constraint {
                        Some(node) => capture.closure = Some(CanonicalClosure::new(node)),
                        None => {
                            capture.constraint = None;
                            capture.slot = CaptureSlot::Default;
                        }
                    }
                }
                CaptureSlot::Default => match capture.parameters[capture.ordinal].default {
                    Some(node) => capture.closure = Some(CanonicalClosure::new(node)),
                    None => {
                        if !self.capture_parameter(capture, None) {
                            return CaptureStep::Abort;
                        }
                    }
                },
                CaptureSlot::Body => {
                    let bindings = crate::semantic_query::CanonicalTypeSubstitution::new(
                        self.graph(),
                        capture
                            .variants
                            .iter()
                            .map(|&(node, position)| (node, capture.fresh[position].param))
                            .collect(),
                    );
                    capture.bindings = Some(bindings);
                    capture.closure = Some(CanonicalClosure::new(
                        self.signature_without_type_parameters(signature),
                    ));
                }
            }
        }
    }

    /// Re-intern the clause parameter the capture stands at with its
    /// substituted bounds, as a fresh parameter: `false` when its shell is
    /// no longer a binder the graph reads.
    fn capture_parameter(
        &self,
        capture: &mut CaptureAvoidance,
        default: Option<SemanticNodeId>,
    ) -> bool {
        let graph = self.graph();
        let Some(data) = graph.node_data(capture.shells[capture.ordinal]) else {
            return false;
        };
        let SemanticNodeData::TypeParam {
            decl, param_index, ..
        } = data.as_ref()
        else {
            return false;
        };
        let parameter = &capture.parameters[capture.ordinal];
        let constraint = capture.constraint.take();
        let param = graph.intern_preserving_scope(
            parameter.param,
            SemanticNodeData::TypeParam {
                decl: decl.clone(),
                param_index: *param_index,
                constraint,
                default,
                display_name: parameter.name.clone(),
            },
        );
        capture.fresh.push(TypeParamDecl {
            name: parameter.name.clone(),
            param,
            constraint,
            default,
            original: self.type_parameter_origin(parameter.param),
            is_const: parameter.is_const,
        });
        capture.bindings = None;
        capture.ordinal += 1;
        capture.slot = CaptureSlot::Constraint;
        true
    }

    fn priced_substitution_read(
        &self,
        node: SemanticNodeId,
        parameter: SemanticNodeId,
        arg: SemanticNodeId,
    ) -> Option<SemanticNodeId> {
        let (value, receipt) = self
            .graph()
            .substitute_memo_get_priced(node, parameter, arg)?;
        matches!(
            self.admit_served(&receipt, super::cost_receipt::Nesting::InPlace),
            super::ServedAdmission::Admitted
        )
        .then_some(value)
    }

    /// Open the cost recording of one substitution level, rooted at its
    /// operands: `false` when an operand is not live in this graph (released
    /// or foreign), where the level still runs on its `node_data` fallback
    /// but seals no receipt and so publishes nothing to the memo.
    fn open_substitution_cost(
        &self,
        node: SemanticNodeId,
        parameter: SemanticNodeId,
        arg: SemanticNodeId,
    ) -> bool {
        let Ok(roots) = self
            .graph()
            .root_nodes([node, parameter, arg].into_iter().filter(|node| {
                // Signature-clause tokens are virtual semantic leaves, not
                // graph allocations. Their identity stays in the cost key;
                // only actual graph operands acquire counted root holds.
                !crate::signature_kernel::SignatureStore::is_binder_token(*node)
            }))
        else {
            return false;
        };
        self.connected_demand.open_cost_scope(
            super::cost_receipt::CostIdentity::of_key(SubstitutionCost(node, parameter, arg))
                .with_roots(roots),
        );
        true
    }

    /// End a substitution level `(node, parameter, arg)` with its `value`:
    /// its recording sealed when the level completed — no canonical
    /// evidence deposited since `epoch`, no trip, no cancellation — and the
    /// receipt recorded as a prerequisite of the level below and published
    /// with the value to the memo; otherwise abandoned.
    ///
    /// Evidence-blind-replay fence: the store-owned memo is cross-request,
    /// so a walk whose canonical composite routing deposited NON-TRIVIAL
    /// evidence (file self-roots, or an incomplete comparison's warm
    /// suppression) must not be replayed to a later request that would
    /// then skip the deposit — a stale warm read served on an under-rooted
    /// entry. The epoch advancing across the level suppresses the publish,
    /// for this level and every enclosing one (ancestors observe the same
    /// advance); the result itself still flows to the caller.
    fn settle_substitution_level(
        &self,
        (node, parameter, arg): (SemanticNodeId, SemanticNodeId, SemanticNodeId),
        epoch: u64,
        recording: bool,
        value: (SemanticNodeId, bool),
    ) -> (SemanticNodeId, bool) {
        if !recording {
            return value;
        }
        let request = crate::request_context::current_request_budget();
        let complete = self.canonical_evidence_epoch.get() == epoch
            && self.connected_demand_trip().is_none()
            && !self.cancellation.is_cancelled();
        let receipt = if complete {
            self.connected_demand.seal_cost_scope(request.as_deref())
        } else {
            self.connected_demand.abandon_cost_scope();
            None
        };
        if let Some(receipt) = receipt {
            self.connected_demand
                .record_prerequisite(&receipt, super::cost_receipt::Nesting::InPlace);
            self.graph()
                .substitute_memo_publish_priced(node, parameter, arg, value.0, receipt);
        }
        value
    }

    construction_ordinal_seam! {
        /// Substitute `arg` for the binder `parameter_node` in `node`: the
        /// walk driven in place, its template read made where the walk
        /// stands. The walk's buffers are the dispatcher's spare walk, so a
        /// cold substitution allocates nothing once warmed up; a nested
        /// substitution finds none spare and makes its own.
        fn substitute_semantic_type_param(
            &self,
            node: SemanticNodeId,
            parameter_node: SemanticNodeId,
            arg: SemanticNodeId,
        ) -> SemanticNodeId {
            let _demand = self.enter_connected_demand(false).0;
            let mut held = HeldSubstitutionWalk {
                dispatch: self,
                walk: self.substitution_spare.borrow_mut().take().unwrap_or_default(),
            };
            let mut step = self.begin_substitution(&mut held.walk, node, parameter_node, arg);
            loop {
                match step {
                    SubstitutionStep::Done(value) => return value,
                    SubstitutionStep::Read(key) => {
                        let mut key = Some(key);
                        step = self.resume_substitution(&mut held.walk, &mut || {
                            self.execute_read(key.take().expect("a walk's read is made once"))
                        });
                    }
                }
            }
        }
    }

    /// The closure of `node` under the canonical substitution: bounded
    /// passes of one public substitution per binding, until a pass changes
    /// nothing.
    pub(crate) fn substitute_canonical(
        &self,
        node: SemanticNodeId,
        substitution: &crate::semantic_query::CanonicalTypeSubstitution,
    ) -> SemanticNodeId {
        let mut closure = CanonicalClosure::new(node);
        while let Some((parameter, bound)) = closure.next(substitution.bindings()) {
            let value = self.substitute_semantic_type_param(closure.node(), parameter, bound);
            closure.accept(value);
        }
        closure.node()
    }

    /// Begin substituting `arg` for `parameter` in `node` on `walk`, an
    /// empty walk: the public arrival — top-level telemetry, the binder
    /// shortcuts and the priced memo probe — and, on a miss, the walk up to
    /// its answer or the read it stands at. Its caller holds the connected
    /// demand the walk's recordings belong to. A served substitution
    /// allocates nothing and opens no level.
    pub(super) fn begin_substitution(
        &self,
        walk: &mut SubstitutionWalk,
        node: SemanticNodeId,
        parameter: SemanticNodeId,
        arg: SemanticNodeId,
    ) -> SubstitutionStep {
        verter_debug_assert!(
            walk.frames.is_empty() && walk.answer.is_none(),
            "a substitution begins on an empty walk"
        );
        match self.arrive_at_substitution(walk, node, parameter, arg, Arrival::Public) {
            Some((value, _)) => SubstitutionStep::Done(value),
            None => self.run_substitution(walk, None),
        }
    }

    /// Resume `walk`, standing at its template read, with `deliver`, that
    /// read: delivered inside the level that named it, over its open
    /// recording, and consumed there; the walk goes on from what it
    /// retained to its answer or its next read.
    pub(super) fn resume_substitution(
        &self,
        walk: &mut SubstitutionWalk,
        deliver: &mut dyn FnMut() -> CacheRead<QueryResult<SemanticNodeId>>,
    ) -> SubstitutionStep {
        self.run_substitution(walk, Some(deliver))
    }

    /// Release what `walk` holds — its levels' open recordings, top first,
    /// so what they charged stays their consumer's — leaving it empty. A
    /// walk that finished holds nothing.
    pub(super) fn release_substitution_walk(&self, walk: &mut SubstitutionWalk) {
        while let Some(frame) = walk.frames.pop() {
            if frame.recording {
                self.connected_demand.abandon_cost_scope();
            }
        }
        walk.children.clear();
        walk.values.clear();
        walk.answer = None;
    }

    /// Advance `walk` until it answers or a level names its template read:
    /// the only place levels advance. With `delivery`, the level on top
    /// stands at that read and settles with it first.
    #[inline(never)]
    fn run_substitution(
        &self,
        walk: &mut SubstitutionWalk,
        mut delivery: Option<&mut dyn FnMut() -> CacheRead<QueryResult<SemanticNodeId>>>,
    ) -> SubstitutionStep {
        loop {
            if let Some(deliver) = delivery.take() {
                let value = self.settle_template(walk, deliver());
                self.finish_substitution_level(walk, value);
                continue;
            }
            let Some(top) = walk.frames.last_mut() else {
                let (value, _) = walk
                    .answer
                    .take()
                    .expect("a walk with no level left holds its root's answer");
                return SubstitutionStep::Done(value);
            };
            if let Some((value, changed)) = walk.answer.take() {
                let fresh = match &mut top.stage {
                    FrameStage::Children => {
                        walk.values.push(value);
                        top.any_changed |= changed;
                        top.last_changed = changed;
                        false
                    }
                    FrameStage::Capture(capture) => {
                        capture.accept(value);
                        false
                    }
                    FrameStage::Rebind(rebind) => {
                        rebind.accept(value);
                        false
                    }
                    FrameStage::Fresh => true,
                    FrameStage::AwaitTemplate => {
                        unreachable!(
                            "a level standing at its template read settles with its delivery"
                        )
                    }
                };
                // The capture-avoided signature's substitution is the
                // level's own, and changed.
                if fresh {
                    self.finish_substitution_level(walk, (value, true));
                    continue;
                }
            }
            if let Some(read) = self.advance_substitution_level(walk) {
                return SubstitutionStep::Read(read);
            }
        }
    }

    /// Take the next step of the level on top of `walk`: hand on its next
    /// child, or rebuild it once its children are done; `Some` when the
    /// rebuild names its template read.
    fn advance_substitution_level(&self, walk: &mut SubstitutionWalk) -> Option<SemanticQueryKey> {
        let level = walk.frames.len() - 1;
        let frame = &mut walk.frames[level];
        match std::mem::replace(&mut frame.stage, FrameStage::Children) {
            FrameStage::Children => {
                let position = frame.children_from + (walk.values.len() - frame.values_from);
                if let Some(&child) = walk.children.get(position) {
                    let (parameter, arg) = (frame.parameter, frame.arg);
                    self.visit_substitution(walk, child, parameter, arg, Arrival::Child);
                    return None;
                }
                match self.rebuild_substituted(frame, &walk.values[frame.values_from..]) {
                    Rebuilt::Node(value, changed) => {
                        self.finish_substitution_level(walk, (value, changed));
                    }
                    Rebuilt::Template(key) => {
                        walk.frames[level].stage = FrameStage::AwaitTemplate;
                        return Some(key);
                    }
                    Rebuilt::Rebind(rebind) => {
                        walk.frames[level].stage = FrameStage::Rebind(rebind);
                    }
                }
            }
            FrameStage::Capture(mut capture) => {
                match self.advance_capture(frame.node, &mut capture) {
                    CaptureStep::Arrive(node, parameter, bound) => {
                        frame.stage = FrameStage::Capture(capture);
                        // A capture's substitution is a public one, entering
                        // the connected demand as the public entry does.
                        let _demand = self.enter_connected_demand(false).0;
                        self.visit_substitution(walk, node, parameter, bound, Arrival::Public);
                    }
                    CaptureStep::Fresh(fresh) => {
                        frame.stage = FrameStage::Fresh;
                        let (parameter, arg) = (frame.parameter, frame.arg);
                        self.visit_substitution(walk, fresh, parameter, arg, Arrival::Child);
                    }
                    CaptureStep::Abort => {
                        Self::signature_children(frame.data.as_ref(), &mut walk.children);
                    }
                }
            }
            FrameStage::Rebind(mut rebind) => match self
                .rebind_step(frame.data.as_ref(), &mut rebind)
            {
                Some((node, old, new)) => {
                    frame.stage = FrameStage::Rebind(rebind);
                    self.visit_substitution(walk, node, old, new, Arrival::Child);
                }
                None => {
                    let value =
                        self.intern_substituted_signature(frame.node, frame.data.as_ref(), *rebind);
                    self.finish_substitution_level(walk, (value, true));
                }
            },
            FrameStage::Fresh | FrameStage::AwaitTemplate => {
                unreachable!("a level waiting on an answer is not advanced")
            }
        }
        None
    }

    /// Arrive at `(node, parameter, arg)`: its answer, when a shortcut or
    /// the memo serves it or its level finishes as it opens, else `None`
    /// with its level opened on top of `walk`.
    fn visit_substitution(
        &self,
        walk: &mut SubstitutionWalk,
        node: SemanticNodeId,
        parameter: SemanticNodeId,
        arg: SemanticNodeId,
        arrival: Arrival,
    ) {
        if let Some(answer) = self.arrive_at_substitution(walk, node, parameter, arg, arrival) {
            walk.answer = Some(answer);
        }
    }

    /// The probes an arrival makes before its level opens, in the order
    /// each kind always made them.
    ///
    /// A public arrival — the entry every caller makes, a capture's
    /// closure included — records the top-level telemetry, classifies its
    /// binder afresh, and answers the binder itself, a node the binder's
    /// dependencies prove it absent from, or a priced memo hit. The
    /// hash-cons memo collapses identical `(node, parameter, arg)` triples
    /// reaching the walk from different call paths to one result: node ids
    /// inside one arena are content-addressed, so the triple is a complete
    /// identity for the substitution's immutable result. It stores only
    /// complete, priced results; a budget- or site-dependent refusal never
    /// publishes.
    ///
    /// A child arrival — a level handing on one of its children — is
    /// counted before any shortcut; it answers a node the binder is proven
    /// absent from, classifies the triple in the request's audit footprint
    /// (the identity branch included: visiting the binder twice in
    /// `Foo<K, K>` is the memo's hit case at the boundary), answers the
    /// binder itself, and probes the same memo, recovering `changed` from
    /// `result != node`: substitution is a pure function of its three
    /// inputs and the shard dedup maps structurally equal rebuilds to one
    /// id, so a stored result equal to `node` is the unchanged outcome.
    fn arrive_at_substitution(
        &self,
        walk: &mut SubstitutionWalk,
        node: SemanticNodeId,
        parameter: SemanticNodeId,
        arg: SemanticNodeId,
        arrival: Arrival,
    ) -> Option<(SemanticNodeId, bool)> {
        match arrival {
            Arrival::Public => {
                verter_audit::observe!(C::Observe => on self.observation.observer.as_deref() => |observer| {
                    observer.record_event(verter_audit::AuditEvent::SubstituteTopLevelCall);
                });
                self.enter_substitution_walk();
                if node == parameter {
                    self.classify_recursive_substitution(node, parameter, arg);
                    return Some((arg, true));
                }
                if self.binder_dependency_proves_absent(node, parameter) {
                    return Some((node, false));
                }
                if let Some(cached) = self.priced_substitution_read(node, parameter, arg) {
                    verter_audit::observe!(C::Observe => on self.observation.observer.as_deref() => |observer| {
                        observer.record_event(verter_audit::AuditEvent::SubstituteMemoHit);
                    });
                    return Some((cached, cached != node));
                }
                self.classify_recursive_substitution(node, parameter, arg);
            }
            Arrival::Child => {
                self.graph().record_substitution_child_visit();
                if node != parameter && self.binder_dependency_proves_absent(node, parameter) {
                    return Some((node, false));
                }
                self.classify_recursive_substitution(node, parameter, arg);
                if node == parameter {
                    return Some((arg, true));
                }
                if let Some(cached) = self.priced_substitution_read(node, parameter, arg) {
                    verter_audit::observe!(C::Observe => on self.observation.observer.as_deref() => |observer| {
                        observer.record_event(verter_audit::AuditEvent::RecursiveSubstituteMemoHit);
                    });
                    return Some((cached, cached != node));
                }
            }
        }
        self.open_substitution_level(walk, node, parameter, arg)
    }

    /// Open the level of `(node, parameter, arg)` after its memo missed:
    /// recorded as one computation, its payload read once and its children
    /// listed. A level that has no child to visit finishes at once.
    fn open_substitution_level(
        &self,
        walk: &mut SubstitutionWalk,
        node: SemanticNodeId,
        parameter: SemanticNodeId,
        arg: SemanticNodeId,
    ) -> Option<(SemanticNodeId, bool)> {
        verter_debug_assert!(node != parameter, "an arrival answers the binder itself");
        let epoch_before = self.canonical_evidence_epoch.get();
        let recording = self.open_substitution_cost(node, parameter, arg);
        let Some(data) = self.graph().node_data(node) else {
            let miss = self.opaque(QueryError::Miss);
            return Some(self.settle_substitution_level(
                (node, parameter, arg),
                epoch_before,
                recording,
                (miss, true),
            ));
        };
        let children_from = walk.children.len();
        let (stage, flag) = match self.open_substitution_children(
            node,
            parameter,
            arg,
            data.as_ref(),
            &mut walk.children,
        ) {
            Opening::Leaf(value) => {
                verter_debug_assert!(
                    walk.children.len() == children_from,
                    "a level that finishes as it opens lists no child"
                );
                return Some(self.settle_substitution_level(
                    (node, parameter, arg),
                    epoch_before,
                    recording,
                    value,
                ));
            }
            Opening::Children { flag } => (FrameStage::Children, flag),
            Opening::Capture(capture) => (FrameStage::Capture(capture), false),
        };
        walk.frames.push(SubstitutionFrame {
            node,
            parameter,
            arg,
            data,
            children_from,
            values_from: walk.values.len(),
            any_changed: false,
            last_changed: false,
            flag,
            epoch_before,
            recording,
            stage,
        });
        None
    }

    /// End the level on top of `walk` with `value`, which becomes the
    /// answer of the level below or of the walk.
    fn finish_substitution_level(
        &self,
        walk: &mut SubstitutionWalk,
        value: (SemanticNodeId, bool),
    ) {
        let frame = walk
            .frames
            .pop()
            .expect("the level finishing is the one on top");
        walk.children.truncate(frame.children_from);
        walk.values.truncate(frame.values_from);
        walk.answer = Some(self.settle_substitution_level(
            (frame.node, frame.parameter, frame.arg),
            frame.epoch_before,
            frame.recording,
            value,
        ));
    }

    /// The template level on top of `walk` settled with `read`, its
    /// reduction: what the reduction built, unless the read is partial or
    /// built nothing, where the template keeps its open holes and folds its
    /// literal ones into its text. The read is consumed here and never read
    /// from the memo again.
    fn settle_template(
        &self,
        walk: &SubstitutionWalk,
        read: CacheRead<QueryResult<SemanticNodeId>>,
    ) -> (SemanticNodeId, bool) {
        let frame = walk
            .frames
            .last()
            .expect("a delivery answers the level that named its read");
        verter_debug_assert!(
            matches!(frame.stage, FrameStage::AwaitTemplate),
            "only a level standing at its template read is delivered one"
        );
        if let (QueryResult::Value(built), false) = (&read.value, read.result_is_partial()) {
            return (*built, true);
        }
        let SemanticNodeData::TemplateLiteral { quasis, .. } = frame.data.as_ref() else {
            unreachable!("a template read is named by a template level")
        };
        (
            self.substituted_template(frame.node, quasis, &walk.values[frame.values_from..]),
            true,
        )
    }

    /// A substituted template that keeps a hole open: its literal holes
    /// fold into its text all the same.
    fn substituted_template(
        &self,
        node: SemanticNodeId,
        quasis: &verter_type_expr::OwnedSlice<OwnedStr>,
        holes: &[SemanticNodeId],
    ) -> SemanticNodeId {
        let (quasis, holes) = match self.folded_literal_holes(quasis, holes) {
            Some((folded, holes)) => (verter_type_expr::OwnedSlice::from(folded.as_slice()), holes),
            None => ((*quasis).clone(), holes.to_vec()),
        };
        self.graph().intern_preserving_scope(
            node,
            SemanticNodeData::TemplateLiteral {
                quasis,
                expressions: crate::semantic_query::PayloadArray::from(holes.into_boxed_slice()),
            },
        )
    }

    /// The first half of a level: its children pushed onto `out` in the
    /// order the level visits them, with the one shadow decision its arm
    /// makes — or the level's value when it has no child to visit, or the
    /// capture avoidance a signature takes before its children.
    fn open_substitution_children(
        &self,
        node: SemanticNodeId,
        parameter: SemanticNodeId,
        arg: SemanticNodeId,
        data: &SemanticNodeData,
        out: &mut Vec<SemanticNodeId>,
    ) -> Opening {
        // Infer substitution is exact-binder-only. A legitimate reference is
        // an `InferRef` carrying the same opaque binder identity; a same-name
        // `TypeParam` is an unrelated declaration and is never rewritten.
        let parameter_infer_binder = match self.substitution_binder(parameter) {
            BinderKind::Infer(binder) => Some(binder),
            _ => None,
        };
        let mut flag = false;
        match data {
            // Exact infer declaration/reference occurrence.
            SemanticNodeData::Infer { binder, .. } | SemanticNodeData::InferRef { binder, .. }
                if parameter_infer_binder.as_ref() == Some(binder) =>
            {
                return Opening::Leaf((arg, true));
            }
            SemanticNodeData::Alias(target) => out.push(*target),
            // Instantiating a type parameter a class expression can see
            // substitutes into the reference's type arguments and its
            // instance surface; the class identity is unchanged.
            SemanticNodeData::ClassExpressionInstance {
                identity,
                type_arguments,
                surface,
            } => {
                out.extend(type_arguments.iter().copied());
                // An object literal's surface mentions no `this` but its
                // own, which its identity binds.
                flag = identity.object_literal
                    && self.is_object_literal_this_binder(identity, parameter);
                if !flag {
                    out.push(*surface);
                }
            }
            SemanticNodeData::Union(members) => out.extend(members.iter().copied()),
            SemanticNodeData::Intersection(members) => out.extend(members.iter().copied()),
            SemanticNodeData::MergedDecl { contributors } => {
                out.extend(contributors.iter().copied());
            }
            SemanticNodeData::Array { element, .. } => out.push(*element),
            SemanticNodeData::Tuple { elements, .. } => {
                out.extend(elements.iter().map(|element| element.value));
            }
            SemanticNodeData::Object(surface) => {
                for member in surface.positive_members().iter() {
                    out.extend(crate::semantic_query::authored_property_key_child(
                        &member.key,
                    ));
                    out.push(member.value);
                }
                out.extend(surface.call_signatures.iter().copied());
                out.extend(surface.construct_signatures.iter().copied());
                for signature in surface.index_signatures.iter() {
                    out.push(signature.key_type);
                    out.push(signature.value_type);
                }
                out.extend(surface.keyspace);
            }
            SemanticNodeData::ObjectSpreadProgram(program) => {
                program.for_each_child_node(|child| out.push(child));
            }
            SemanticNodeData::TemplateLiteral { expressions, .. } => {
                out.extend(expressions.iter().copied());
            }
            SemanticNodeData::KeyOf { base } => out.push(*base),
            SemanticNodeData::IndexedAccess { object, index, .. } => {
                out.push(*object);
                if let IndexKey::Computed(key) = index {
                    out.push(*key);
                }
            }
            SemanticNodeData::Mapped { source, mapper } => {
                // "mapped descents" counter.
                verter_audit::observe!(C::Observe => on self.observation.observer.as_deref() => |observer| {
                    observer.record_event(verter_audit::AuditEvent::SubstituteMappedTypeDescend);
                });
                // Exact node identity is the sole shadowing axis. A mapped
                // TypeParam that merely shares an infer display name is a
                // distinct declaration and cannot match an exact InferRef.
                flag = mapper.parameter_node == parameter;
                out.push(*source);
                out.push(mapper.key_space);
                if !flag {
                    out.push(mapper.value_expr);
                    out.extend(mapper.name_remap);
                }
                out.extend(mapper.modifiers_type);
            }
            // The value-root + path of a `typeof` carrier are opaque to
            // substitution (they are not node ids). Structural
            // child-integrity requires descending into its instantiation
            // `type_args` so a `T` inside `typeof f<T>` is rewritten —
            // STRUCTURAL recursion only, NOT semantic instantiation
            // application (that is a demand-time carrier-resolution
            // reduction). The same holds for an unresolved bare-name
            // carrier `Foo<arg…>` and an import-type carrier
            // `import("m").Q<arg…>`, whose name, scope, specifier and
            // qualifier are preserved verbatim. The nominal terminal
            // carries no args.
            SemanticNodeData::TypeOf(_)
            | SemanticNodeData::TypeOfNominal(_)
            | SemanticNodeData::BareRef(_)
            | SemanticNodeData::ImportType(_) => {
                out.extend(data.carrier_type_args().iter().copied());
            }
            SemanticNodeData::Conditional { check, extends, .. } => {
                // Substitution rewrites check/extends only. Branch
                // substitution is deferred onto the sealed pending frame
                // so an undemanded loser is never traversed. Selection
                // does not run here.
                verter_audit::observe!(C::Observe => on self.observation.observer.as_deref() => |observer| {
                    observer.record_event(verter_audit::AuditEvent::SubstituteConditionalDescend);
                });
                // Capture-avoidance for an `Infer` binder — the same shadow
                // rule the Mapped arm applies, here on the conditional-scope
                // axis: a nested conditional whose OWN `extends` pattern
                // DECLARES the same infer name RE-BINDS it. The re-binding
                // scopes the extends pattern and the TRUE branch; the check
                // and the FALSE branch stay in the OUTER binder's scope and
                // still substitute. The detection is DECLARATION-scoped
                // ([`Self::extends_pattern_declares_infer`]).
                flag = parameter_infer_binder.is_some()
                    && self.extends_pattern_declares_infer(*extends, parameter);
                out.push(*check);
                if !flag {
                    out.push(*extends);
                }
            }
            // An intrinsic application defers only because its operand is
            // binder-dependent: supplying the binder re-enters the owning
            // query family and the operation reduces (`Awaited<T>` with
            // `T := Promise<string>` becomes `string`).
            SemanticNodeData::IntrinsicApplication { args, .. } => {
                out.extend(args.iter().copied());
            }
            // A lazy carrier of `Helper<arg1, arg2>`: substitution descends
            // into each arg so type-parameter references inside an
            // unrealised instantiation are rewritten when the outer binder
            // fires per-key realisation. `base` carries no type-parameter
            // references and is preserved verbatim.
            SemanticNodeData::InstantiationRef { args, .. } => {
                out.extend(args.iter().copied());
            }
            // Function arm: every parameter type and the return type, so
            // `T` / `infer X` references inside `(x: T, y: infer X) => R`
            // are rewritten when the outer binder fires.
            SemanticNodeData::Signature {
                type_parameters, ..
            } => {
                // Binding the clause's own exact node is an intentional
                // instantiation. An outer substitution instead keeps its
                // argument free of the signature's local clause.
                if !type_parameters.iter().any(|decl| decl.param == parameter)
                    && !self.binder_dependency_proves_absent(node, parameter)
                {
                    if let Some(capture) =
                        self.begin_capture_avoidance(node, type_parameters.as_arc(), arg)
                    {
                        return Opening::Capture(Box::new(capture));
                    }
                }
                Self::signature_children(data, out);
            }
            // The sealed index-composed carrier substitutes its OWN
            // parameters and binder constraints/defaults; the occurrence
            // and the deferred return carrier are preserved.
            SemanticNodeData::DeferredCallable(callable) => {
                let parts = callable.parts(&crate::semantic_query::ResolveCallConsumer::witness());
                callable.for_each_source_value(|value| out.push(value));
                out.extend(parts.params.iter().map(|param| param.ty));
                for tp in parts.type_parameters.iter() {
                    out.extend(tp.constraint);
                    out.extend(tp.default);
                }
            }
            _ => return Opening::Leaf((node, false)),
        }
        Opening::Children { flag }
    }

    /// A signature's children in the order its level visits them: its
    /// return carrier's source images, its parameters, its return, its
    /// predicate's target, then each clause parameter's constraint and
    /// default.
    fn signature_children(data: &SemanticNodeData, out: &mut Vec<SemanticNodeId>) {
        let SemanticNodeData::Signature {
            params,
            return_type,
            type_parameters,
            return_carrier,
            predicate,
            ..
        } = data
        else {
            unreachable!("signature children are a signature's")
        };
        return_carrier.for_each_source_image(|image| out.push(image));
        out.extend(params.iter().map(|param| param.ty));
        out.push(*return_type);
        out.extend(predicate.and_then(|predicate| predicate.ty));
        for tp in type_parameters.iter() {
            out.extend(tp.constraint);
            out.extend(tp.default);
        }
    }

    /// The second half of a level: its node rebuilt from `values`, one per
    /// child it visited, in the order it visited them — or the template
    /// read a changed closed template names, or a signature's moved binders
    /// still to rebind.
    fn rebuild_substituted(&self, frame: &SubstitutionFrame, values: &[SemanticNodeId]) -> Rebuilt {
        let node = frame.node;
        let data = frame.data.as_ref();
        if !frame.any_changed {
            match data {
                // A conditional's pending frame may still change.
                SemanticNodeData::Conditional { .. } => {}
                SemanticNodeData::TypeOf(_) | SemanticNodeData::TypeOfNominal(_) => {
                    // "opaque TypeOf returns" counter.
                    verter_audit::observe!(C::Observe => on self.observation.observer.as_deref() => |observer| {
                        observer.record_event(verter_audit::AuditEvent::SubstituteTypeOfOpaque);
                    });
                    return Rebuilt::Node(node, false);
                }
                _ => return Rebuilt::Node(node, false),
            }
        }
        let graph = self.graph();
        let mut values = ValueCursor { values, at: 0 };
        let rebuilt = match data {
            SemanticNodeData::Alias(_) => {
                graph.intern_preserving_scope(node, SemanticNodeData::Alias(values.next()))
            }
            SemanticNodeData::ClassExpressionInstance {
                identity,
                type_arguments,
                surface,
            } => {
                let mut new_arguments = Vec::with_capacity(type_arguments.len());
                for _ in type_arguments.iter() {
                    new_arguments.push(values.next());
                }
                let surface = if frame.flag { *surface } else { values.next() };
                graph.intern_preserving_scope(
                    node,
                    SemanticNodeData::ClassExpressionInstance {
                        identity: Arc::clone(identity),
                        type_arguments: crate::semantic_query::PayloadArray::from(
                            new_arguments.into_boxed_slice(),
                        ),
                        surface,
                    },
                )
            }
            // Substitution is a composite CONSTRUCTION site: a CHANGED union
            // routes through the canonical authority — substituting
            // `T := string` into `T | string` yields two structurally equal
            // arms the raw rebuild would keep, and `T := never` leaves a
            // `never` arm the lattice absorbs. Union arm order carries no
            // overload precedence, so the commutative route is
            // unconditionally safe here.
            SemanticNodeData::Union(_) => {
                return Rebuilt::Node(
                    self.intern_normalized_union_or_intersection(values.rest(), true),
                    true,
                );
            }
            // A CHANGED intersection splits by CARRIER SEMANTICS, exactly as
            // the member-value merge does: a possibly-callable contributor
            // makes it an overload-ordered carrier (call resolution tries
            // arms in declaration order), so the rebuild PRESERVES the
            // authored order and scope — the classification follows
            // transparent carriers and fails CLOSED on anything undecidable
            // from the graph alone. Otherwise the derived instantiation
            // routes through the canonical authority.
            SemanticNodeData::Intersection(members) => {
                let new_members = values.rest();
                // A heritage body is a declaration, not an intersection type:
                // its substitution stays a heritage body whatever it carries.
                let category = members.origin_category();
                let rebuilt = if category
                    == crate::semantic_query::composite::CompositeOriginCategory::Heritage
                    || new_members.iter().any(|member| {
                        crate::project_semantic_dispatch::walk::value_may_contribute_call_signatures(
                            graph.store(),
                            *member,
                        )
                    }) {
                    graph.intern_preserving_scope(
                        node,
                        SemanticNodeData::Intersection(
                            crate::semantic_query::composite::CompositeList::rebuilt_from(
                                category,
                                Arc::from(new_members),
                            ),
                        ),
                    )
                } else {
                    self.intern_normalized_union_or_intersection(new_members, false)
                };
                return Rebuilt::Node(rebuilt, true);
            }
            // Each merged contributor substituted, preserving the distinct
            // carrier so the peer-merge reducer still applies to the
            // instantiated declaration.
            SemanticNodeData::MergedDecl { .. } => graph.intern_preserving_scope(
                node,
                SemanticNodeData::MergedDecl {
                    contributors: crate::semantic_query::PayloadArray::from(values.rest()),
                },
            ),
            SemanticNodeData::Array { readonly, .. } => graph.intern_preserving_scope(
                node,
                SemanticNodeData::Array {
                    element: values.next(),
                    readonly: *readonly,
                },
            ),
            SemanticNodeData::Tuple { elements, readonly } => {
                let mut new_elements = Vec::with_capacity(elements.len());
                for element in elements.iter() {
                    new_elements.push(crate::semantic_query::TupleElement {
                        label: element.label.clone(),
                        value: values.next(),
                        optional: element.optional,
                        rest: element.rest,
                    });
                }
                values.end();
                // Normalize-on-intern (the variadic-spread rule): a
                // substituted `rest` element whose value settled to a
                // concrete tuple splices in place and a sole rest-of-array
                // tuple collapses to the array. Open / unresolved rest
                // values keep their carrier verbatim.
                return Rebuilt::Node(
                    match self.normalize_tuple_spread(&new_elements, *readonly) {
                        crate::project_semantic_dispatch::build::NormalizedTupleShape::Array(
                            array_node,
                        ) => array_node,
                        crate::project_semantic_dispatch::build::NormalizedTupleShape::Tuple(
                            normalized,
                        ) => graph.intern_preserving_scope(
                            node,
                            SemanticNodeData::Tuple {
                                elements: crate::semantic_query::PayloadArray::from(
                                    normalized.into_boxed_slice(),
                                ),
                                readonly: *readonly,
                            },
                        ),
                    },
                    true,
                );
            }
            SemanticNodeData::Object(surface) => {
                let mut new_members = Vec::with_capacity(surface.positive_members().len());
                for member in surface.positive_members().iter() {
                    let key = member
                        .key
                        .clone()
                        .map(|_computed| values.next(), |identity| identity);
                    new_members.push(SurfaceMember {
                        key,
                        value: values.next(),
                        optional: member.optional,
                        readonly: member.readonly,
                        method_kind: member.method_kind,
                        has_implementation_body: member.has_implementation_body,
                        // Type-parameter substitution preserves the source
                        // member's declared accessibility, excess-property
                        // provenance, structural shape, macro type-argument
                        // fact, merge role, declaration-site spans and
                        // declaration file: only the value's type-param
                        // occurrences change.
                        visibility: member.visibility,
                        excess_origin: member.excess_origin,
                        spans: member.spans,
                        declaration_origin: member.declaration_origin.clone(),
                        declared_in_macro_type_arg: member.declared_in_macro_type_arg,
                        merge_role: member.merge_role,
                    });
                }
                let mut new_call_signatures = Vec::with_capacity(surface.call_signatures.len());
                for _ in surface.call_signatures.iter() {
                    new_call_signatures.push(values.next());
                }
                let mut new_construct_signatures =
                    Vec::with_capacity(surface.construct_signatures.len());
                for _ in surface.construct_signatures.iter() {
                    new_construct_signatures.push(values.next());
                }
                let mut new_index_signatures = Vec::with_capacity(surface.index_signatures.len());
                for signature in surface.index_signatures.iter() {
                    let key_type = values.next();
                    new_index_signatures.push(IndexSignature {
                        key_type,
                        value_type: values.next(),
                        readonly: signature.readonly,
                        key_name: signature.key_name.clone(),
                        // Preserve the index signature's OXC spans + declaration file.
                        spans: signature.spans,
                        declaration_origin: signature.declaration_origin.clone(),
                    });
                }
                let new_keyspace = surface.keyspace.map(|_| values.next());
                let mut members = new_members.into_iter();
                let mut calls = new_call_signatures.into_iter();
                let mut constructs = new_construct_signatures.into_iter();
                let mut indexes = new_index_signatures.into_iter();
                let entries = surface
                    .entries
                    .iter()
                    .map(|entry| match entry {
                        crate::semantic_query::SurfaceEntry::Member(_) => {
                            crate::semantic_query::SurfaceEntry::Member(
                                members.next().expect("derived member index matches stream"),
                            )
                        }
                        crate::semantic_query::SurfaceEntry::CallSignature(_) => {
                            crate::semantic_query::SurfaceEntry::CallSignature(
                                calls.next().expect("derived call index matches stream"),
                            )
                        }
                        crate::semantic_query::SurfaceEntry::ConstructSignature(_) => {
                            crate::semantic_query::SurfaceEntry::ConstructSignature(
                                constructs
                                    .next()
                                    .expect("derived construct index matches stream"),
                            )
                        }
                        crate::semantic_query::SurfaceEntry::IndexSignature(_) => {
                            crate::semantic_query::SurfaceEntry::IndexSignature(
                                indexes.next().expect("derived index matches stream"),
                            )
                        }
                    })
                    .collect();
                graph.intern_preserving_scope(
                    node,
                    SemanticNodeData::Object(SurfaceView::from_entries(
                        entries,
                        new_keyspace,
                        surface.has_known_index_signature(),
                    )),
                )
            }
            SemanticNodeData::ObjectSpreadProgram(program) => {
                let rebuilt = program.map_child_nodes(|_| values.next());
                graph.intern_preserving_scope(node, SemanticNodeData::ObjectSpreadProgram(rebuilt))
            }
            // Substitution is a template CONSTRUCTION site, as it is for a
            // union: the checker instantiates a template literal type
            // through `getTemplateLiteralType`, which folds literal, `null`,
            // `undefined` and `boolean` holes into its text and maps over a
            // union hole, so a template whose every hole is now one of those
            // is the strings it builds (`${P}${S}` with `P := "/a/"` and
            // `S := "x"` is `"/a/x"`; `x${N}` with `N := null` is
            // `"xnull"`). The template authority builds it, its cross
            // product and that product's limit included: the level names
            // that reduction as its read.
            SemanticNodeData::TemplateLiteral { quasis, .. } => {
                let holes = values.rest();
                let folds = |expression: SemanticNodeId| {
                    matches!(
                        graph.node_data(expression).as_deref(),
                        Some(
                            SemanticNodeData::Literal(_)
                                | SemanticNodeData::Primitive(
                                    PrimitiveKind::Null
                                        | PrimitiveKind::Undefined
                                        | PrimitiveKind::Boolean
                                        | PrimitiveKind::Never
                                )
                        )
                    )
                };
                let all_closed =
                    holes
                        .iter()
                        .all(|expression| match graph.node_data(*expression).as_deref() {
                            Some(SemanticNodeData::Union(members)) => {
                                members.iter().all(|member| folds(*member))
                            }
                            _ => folds(*expression),
                        });
                if all_closed {
                    return Rebuilt::Template(SemanticQueryKey::TemplateLiteralReduce {
                        pattern: (*quasis).clone(),
                        args: Arc::from(holes),
                        context: self.template_literal_reduce_context(),
                    });
                }
                // A template with a hole still open keeps it; its literal
                // holes fold into its text all the same.
                return Rebuilt::Node(self.substituted_template(node, quasis, holes), true);
            }
            SemanticNodeData::KeyOf { .. } => graph.intern_preserving_scope(
                node,
                SemanticNodeData::KeyOf {
                    base: values.next(),
                },
            ),
            SemanticNodeData::IndexedAccess {
                object,
                index,
                origin,
            } => {
                let sub_object = values.next();
                // A changed computed key normalizes as its evaluated key; an
                // unchanged or literal key is kept as written.
                let sub_index = match index {
                    IndexKey::Computed(_) => {
                        let key = values.next();
                        if frame.last_changed {
                            self.normalized_index_key_node(key)
                        } else {
                            index.clone()
                        }
                    }
                    other => other.clone(),
                };
                values.end();
                // An access of the permissive instantiation's wildcard, or
                // by it, is the wildcard (`getIndexedAccessType` returns
                // `wildcardType` for either operand).
                let wildcard = |id: SemanticNodeId| {
                    matches!(
                        graph.node_data(id).as_deref(),
                        Some(SemanticNodeData::Opaque(QueryError::PermissiveWildcard))
                    )
                };
                if wildcard(sub_object) {
                    return Rebuilt::Node(sub_object, true);
                }
                if let IndexKey::Computed(index) = &sub_index {
                    if wildcard(*index) {
                        return Rebuilt::Node(*index, true);
                    }
                }
                return Rebuilt::Node(
                    graph.intern_preserving_scope(
                        node,
                        SemanticNodeData::IndexedAccess {
                            object: sub_object,
                            index: sub_index,
                            origin: self.instantiated_access_origin(*origin, *object, index),
                        },
                    ),
                    true,
                );
            }
            SemanticNodeData::Mapped { mapper, .. } => {
                let sub_source = values.next();
                let sub_key_space = values.next();
                let sub_value_expr = if frame.flag {
                    mapper.value_expr
                } else {
                    values.next()
                };
                let sub_name_remap = match mapper.name_remap {
                    Some(_) if !frame.flag => Some(values.next()),
                    other => other,
                };
                let sub_modifiers_type = mapper.modifiers_type.map(|_| values.next());
                // "Mapped rebuilt" counter: distinct from
                // `SubstituteMappedTypeDescend` (every visit), it fires only
                // on the rebuild after a descendant changed.
                verter_audit::observe!(C::Observe => on self.observation.observer.as_deref() => |observer| {
                    observer.record_event(verter_audit::AuditEvent::SubstituteMappedRebuild);
                });
                graph.intern_preserving_scope(
                    node,
                    SemanticNodeData::Mapped {
                        source: sub_source,
                        mapper: MapperKey {
                            parameter_node: mapper.parameter_node,
                            key_space: sub_key_space,
                            value_expr: sub_value_expr,
                            optionality: mapper.optionality,
                            readonly: mapper.readonly,
                            name_remap: sub_name_remap,
                            // Substitution applies uniformly to `source` and
                            // `value_expr.object` for non-shadowed
                            // substitutions, so a mapper that lowered as
                            // identity `T[K]` remains identity after the
                            // type-parameter rewrite.
                            kind: mapper.kind,
                            // The mapping stays homomorphic over its
                            // instantiation.
                            over_type_variable: mapper.over_type_variable,
                            modifiers_type: sub_modifiers_type,
                        },
                    },
                )
            }
            SemanticNodeData::TypeOf(_) | SemanticNodeData::TypeOfNominal(_) => {
                let rebuilt = data
                    .map_carrier_type_args(Arc::from(values.rest()))
                    .expect("TypeOf is a carrier");
                graph.intern_preserving_scope(node, rebuilt)
            }
            SemanticNodeData::BareRef(_) => {
                let rebuilt = data
                    .map_carrier_type_args(Arc::from(values.rest()))
                    .expect("BareRef is a carrier");
                graph.intern_preserving_scope(node, rebuilt)
            }
            SemanticNodeData::ImportType(_) => {
                let rebuilt = data
                    .map_carrier_type_args(Arc::from(values.rest()))
                    .expect("ImportType is a carrier");
                graph.intern_preserving_scope(node, rebuilt)
            }
            SemanticNodeData::Conditional {
                check,
                extends,
                true_branch_ref,
                false_branch_ref,
                distributive,
                pending,
            } => {
                let (parameter, arg) = (frame.parameter, frame.arg);
                let sub_check = values.next();
                let sub_extends = if frame.flag { *extends } else { values.next() };
                values.end();
                let mut next_pending = pending
                    .as_ref()
                    .map(|substitution| substitution.as_ref().clone())
                    .unwrap_or_else(ConditionalPendingSubstitution::empty);
                if frame.flag {
                    next_pending = next_pending.append_false(parameter, arg);
                } else {
                    next_pending = next_pending.append_both(parameter, arg);
                    // A distributive conditional's check IS this parameter:
                    // its branches see each member it distributes over. An
                    // `infer` binder's check is a reference carrying its
                    // binder identity, never the declaration node itself.
                    let parameter_infer_binder = match self.substitution_binder(parameter) {
                        BinderKind::Infer(binder) => Some(binder),
                        _ => None,
                    };
                    let check_is_parameter = *check == parameter
                        || parameter_infer_binder.as_ref().is_some_and(|binder| {
                            matches!(
                                graph.node_data(*check).as_deref(),
                                Some(
                                    SemanticNodeData::Infer { binder: written, .. }
                                        | SemanticNodeData::InferRef { binder: written, .. }
                                ) if written == binder
                            )
                        });
                    if *distributive && check_is_parameter {
                        next_pending = next_pending.distributing(parameter);
                    }
                }
                let next_pending = if next_pending.is_empty() {
                    None
                } else {
                    Some(verter_session_query::retention::owned::Owned::new(
                        next_pending,
                    ))
                };
                if !frame.any_changed && next_pending == *pending {
                    return Rebuilt::Node(node, false);
                }
                verter_audit::observe!(C::Observe => on self.observation.observer.as_deref() => |observer| {
                    observer.record_event(verter_audit::AuditEvent::SubstituteConditionalRebuild);
                });
                return Rebuilt::Node(
                    graph.intern_preserving_scope(
                        node,
                        SemanticNodeData::Conditional {
                            check: sub_check,
                            extends: sub_extends,
                            true_branch_ref: *true_branch_ref,
                            false_branch_ref: *false_branch_ref,
                            distributive: *distributive,
                            pending: next_pending,
                        },
                    ),
                    true,
                );
            }
            SemanticNodeData::IntrinsicApplication { op, .. } => {
                let new_args = values.rest();
                // `NoInfer` over an operand that is no longer generic is
                // that operand.
                if matches!(op, crate::semantic_query::CompilerIntrinsicTypeOp::NoInfer)
                    && !self.type_is_generic(new_args[0])
                {
                    return Rebuilt::Node(new_args[0], true);
                }
                graph.intern_preserving_scope(
                    node,
                    SemanticNodeData::intrinsic_application(*op, Arc::from(new_args))
                        .expect("substituted one-for-one from a well-formed application"),
                )
            }
            SemanticNodeData::InstantiationRef { base, .. } => graph.intern_preserving_scope(
                node,
                SemanticNodeData::InstantiationRef {
                    base: base.clone(),
                    args: crate::semantic_query::PayloadArray::from(values.rest()),
                },
            ),
            SemanticNodeData::Signature {
                params,
                type_parameters,
                return_carrier,
                predicate,
                ..
            } => {
                let mapped_carrier = return_carrier.map_source_images(|_| values.next());
                let mut new_params = Vec::with_capacity(params.len());
                for param in params.iter() {
                    new_params.push(FunctionParam {
                        name: param.name.clone(),
                        ty: values.next(),
                        optional: param.optional,
                        rest: param.rest,
                        // Substitution preserves the parameter's OXC span
                        // and its declaration facts.
                        span: param.span,
                        declared_literal: param.declared_literal,
                    });
                }
                let sub_return = values.next();
                // A generic predicate instantiates with the signature:
                // `x is T` at `T := string` narrows to `string`.
                let sub_predicate =
                    predicate.map(|predicate| predicate.map_type(|_| values.next()));
                // Each clause parameter's bounds substituted; its binder is
                // decided once the bounds have followed every earlier
                // parameter's re-interned binder (`rebind_step`).
                let mut new_type_parameters = Vec::with_capacity(type_parameters.len());
                for tp in type_parameters.iter() {
                    new_type_parameters.push(TypeParamDecl {
                        name: tp.name.clone(),
                        param: tp.param,
                        constraint: tp.constraint.map(|_| values.next()),
                        default: tp.default.map(|_| values.next()),
                        original: tp.original.clone(),
                        is_const: tp.is_const,
                    });
                }
                values.end();
                let mut draft = SignatureRebind {
                    new_params,
                    sub_return,
                    sub_predicate,
                    new_type_parameters,
                    mapped_carrier,
                    moved: Vec::new(),
                    clause_at: 0,
                    bound: 0,
                    pair: 0,
                    slot: 0,
                };
                if self.rebind_step(data, &mut draft).is_some() {
                    return Rebuilt::Rebind(Box::new(draft));
                }
                self.intern_substituted_signature(node, data, draft)
            }
            SemanticNodeData::DeferredCallable(callable) => {
                let parts = callable.parts(&crate::semantic_query::ResolveCallConsumer::witness());
                let mapped_source = callable.map_source_values(|_| values.next());
                let mut new_params = Vec::with_capacity(parts.params.len());
                for param in parts.params.iter() {
                    new_params.push(FunctionParam {
                        name: param.name.clone(),
                        ty: values.next(),
                        optional: param.optional,
                        rest: param.rest,
                        span: param.span,
                        declared_literal: param.declared_literal,
                    });
                }
                let mut new_type_parameters = Vec::with_capacity(parts.type_parameters.len());
                for tp in parts.type_parameters.iter() {
                    let constraint = tp.constraint.map(|_| values.next());
                    let default = tp.default.map(|_| values.next());
                    new_type_parameters.push(TypeParamDecl {
                        name: tp.name.clone(),
                        param: tp.param,
                        constraint,
                        default,
                        original: tp.original.clone(),
                        is_const: tp.is_const,
                    });
                }
                values.end();
                let rebuilt = SemanticNodeData::DeferredCallable(mapped_source.with_substituted(
                    Arc::from(new_params.into_boxed_slice()),
                    Arc::from(new_type_parameters.into_boxed_slice()),
                ));
                return Rebuilt::Node(graph.intern_preserving_scope(node, rebuilt), true);
            }
            _ => unreachable!("a level that lists children is one of the arms that rebuild"),
        };
        values.end();
        Rebuilt::Node(rebuilt, true)
    }

    /// The substitution `(node, old, new)` a signature's rebind makes next,
    /// or `None` once it is done: first each clause parameter's bounds
    /// following the binders moved before it, then the moved binders into
    /// the signature's positions. A clause parameter whose bounds are done
    /// gets its binder here: the parameter's own binder node embeds its
    /// declaration-local bounds, so it is kept verbatim when they are
    /// untouched and re-interned with the substituted bounds (same decl
    /// identity, index and name) when they move, so `param` stays the
    /// decl's own `TypeParam` node. A binder re-interned with its moved
    /// bounds is still the parameter its signature's positions name: every
    /// occurrence of the old binder node follows it, so inference and
    /// substitution, which bind the exact binder node, reach them
    /// (`pick<S extends T>(x: S): S` over `Bx<string | number>` infers `S`
    /// from `x`).
    fn rebind_step(
        &self,
        data: &SemanticNodeData,
        rebind: &mut SignatureRebind,
    ) -> Option<(SemanticNodeId, SemanticNodeId, SemanticNodeId)> {
        let SemanticNodeData::Signature {
            type_parameters, ..
        } = data
        else {
            unreachable!("a signature level rebinds a signature's clause")
        };
        while rebind.clause_at < rebind.new_type_parameters.len() {
            let tp = &rebind.new_type_parameters[rebind.clause_at];
            if rebind.bound < 2 {
                let bound = if rebind.bound == 0 {
                    tp.constraint
                } else {
                    tp.default
                };
                if let (Some(bound), Some(&(old, new))) = (bound, rebind.moved.get(rebind.pair)) {
                    return Some((bound, old, new));
                }
                rebind.bound += 1;
                rebind.pair = 0;
                continue;
            }
            let declared = &type_parameters[rebind.clause_at];
            if tp.constraint != declared.constraint || tp.default != declared.default {
                let graph = self.graph();
                let param = match graph.node_data(declared.param).as_deref() {
                    Some(SemanticNodeData::TypeParam {
                        decl, param_index, ..
                    }) => graph.intern_preserving_scope(
                        declared.param,
                        SemanticNodeData::TypeParam {
                            decl: decl.clone(),
                            param_index: *param_index,
                            constraint: tp.constraint,
                            default: tp.default,
                            display_name: tp.name.clone(),
                        },
                    ),
                    _ => declared.param,
                };
                if param != declared.param {
                    rebind.moved.push((declared.param, param));
                }
                rebind.new_type_parameters[rebind.clause_at].param = param;
            }
            rebind.clause_at += 1;
            rebind.bound = 0;
            rebind.pair = 0;
        }
        rebind.next_position()
    }

    /// The signature `node` substituted from `draft`: its parameters,
    /// return, predicate and clause rebuilt. Instantiation PRESERVES the
    /// occurrence — an instantiated candidate is the same occurrence; a
    /// declared carrier retargets the substituted return node, and a
    /// body-derived carrier keeps its own or its substituted images.
    fn intern_substituted_signature(
        &self,
        node: SemanticNodeId,
        data: &SemanticNodeData,
        draft: SignatureRebind,
    ) -> SemanticNodeId {
        let SemanticNodeData::Signature {
            kind,
            occurrence,
            return_carrier,
            signature_span,
            return_type_span,
            is_abstract,
            ..
        } = data
        else {
            unreachable!("a signature level rebuilds a signature")
        };
        let SignatureRebind {
            new_params,
            sub_return,
            sub_predicate,
            new_type_parameters,
            mapped_carrier,
            ..
        } = draft;
        self.graph().intern_preserving_scope(
            node,
            SemanticNodeData::Signature {
                kind: *kind,
                params: crate::semantic_query::PayloadArray::from(new_params.into_boxed_slice()),
                return_type: sub_return,
                type_parameters: crate::semantic_query::PayloadArray::from(
                    new_type_parameters.into_boxed_slice(),
                ),
                occurrence: occurrence.clone(),
                return_carrier: match return_carrier {
                    crate::semantic_query::SignatureReturnCarrier::Declared(_) => {
                        crate::semantic_query::SignatureReturnCarrier::Declared(sub_return)
                    }
                    crate::semantic_query::SignatureReturnCarrier::Function(source) => {
                        crate::semantic_query::SignatureReturnCarrier::Function(source.clone())
                    }
                    crate::semantic_query::SignatureReturnCarrier::DeferredFlow(_)
                    | crate::semantic_query::SignatureReturnCarrier::EvaluatedFlow(_) => {
                        mapped_carrier.unwrap_or_else(|| return_carrier.clone())
                    }
                },
                // Substitution preserves the signature's OXC spans.
                signature_span: *signature_span,
                return_type_span: *return_type_span,
                predicate: sub_predicate,
                is_abstract: *is_abstract,
            },
        )
    }

    /// A class's polymorphic `this` type as a class member body reads it:
    /// a binder whose constraint is the class instance `instance` (none for
    /// a class expression, whose instance is built from its members). A
    /// member read off a receiver binds it to the receiver
    /// ([`Self::bind_this_receiver`]), the checker's instantiation of a
    /// class's `this` type with the reference it is accessed through.
    pub(super) fn this_binder(
        &self,
        canonical: &str,
        owner: verter_type_expr::TopLevelOwnerId,
        class: &Arc<str>,
        instance: Option<SemanticNodeId>,
    ) -> SemanticNodeId {
        self.graph().intern_node(SemanticNodeData::TypeParam {
            decl: crate::semantic_query::DeclIdentity {
                canonical_id: verter_type_expr::OwnedStr::from(canonical),
                owner,
                whole_hash: crate::semantic_query::HashValue::default(),
                decl_name: verter_type_expr::OwnedStr::from(class),
            },
            param_index: THIS_BINDER_INDEX,
            constraint: instance,
            default: None,
            display_name: OwnedStr::from("this"),
        })
    }

    /// The class instance a polymorphic `this` binder stands for, when
    /// `node` is one.
    pub(super) fn this_binder_instance(&self, node: SemanticNodeId) -> Option<SemanticNodeId> {
        match self.graph().node_data(node)?.as_ref() {
            SemanticNodeData::TypeParam {
                param_index: THIS_BINDER_INDEX,
                constraint,
                ..
            } => *constraint,
            _ => None,
        }
    }

    /// Whether `node` mentions `target` anywhere below it.
    pub(super) fn mentions_node(&self, node: SemanticNodeId, target: SemanticNodeId) -> bool {
        let mut visited: rustc_hash::FxHashSet<SemanticNodeId> = rustc_hash::FxHashSet::default();
        let mut stack = vec![node];
        while let Some(current) = stack.pop() {
            if current == target {
                return true;
            }
            if !visited.insert(current) {
                continue;
            }
            if let Some(data) = self.graph().node_data(current) {
                let _ = data.for_each_child(|child| stack.push(child));
            }
        }
        false
    }

    /// Whether `node` holds a type parameter or an `infer` placeholder
    /// anywhere below it: the checker's generic type, whose operators stay
    /// deferred.
    pub(super) fn mentions_binder(&self, node: SemanticNodeId) -> bool {
        self.graph()
            .binder_dependencies_for(node)
            .may_mention_binder()
    }

    fn binder_dependency_proves_absent(
        &self,
        node: SemanticNodeId,
        target: SemanticNodeId,
    ) -> bool {
        match self.substitution_binder(target) {
            BinderKind::TypeParam => self
                .graph()
                .binder_dependencies_for(node)
                .proves_type_param_absent(target),
            BinderKind::Infer(binder) => self
                .graph()
                .binder_dependencies_for(node)
                .proves_infer_absent(&binder),
            BinderKind::Other => false,
        }
    }

    /// What `binder` is, as a substitution walk reads it at every node it
    /// visits: read from the graph once per walk.
    pub(super) fn substitution_binder(&self, binder: SemanticNodeId) -> BinderKind {
        if let Some((held, kind)) = &*self.substitution_binder.borrow() {
            if *held == binder {
                return kind.clone();
            }
        }
        let kind = match self.graph().node_data(binder).as_deref() {
            Some(SemanticNodeData::TypeParam { .. }) => BinderKind::TypeParam,
            Some(SemanticNodeData::Infer { binder, .. }) => BinderKind::Infer(binder.clone()),
            _ => BinderKind::Other,
        };
        *self.substitution_binder.borrow_mut() = Some((binder, kind.clone()));
        kind
    }

    /// Begin a substitution walk: its binder is classified afresh.
    fn enter_substitution_walk(&self) {
        *self.substitution_binder.borrow_mut() = None;
    }

    /// Whether a member read off `node` binds a polymorphic `this`: a
    /// reference to a class (or class expression) instance.
    pub(super) fn is_this_receiver(&self, node: SemanticNodeId) -> bool {
        matches!(
            self.graph().node_data(node).as_deref(),
            Some(
                SemanticNodeData::DeclRef { .. }
                    | SemanticNodeData::InstantiationRef { .. }
                    | SemanticNodeData::ClassExpressionInstance { .. }
            )
        )
    }

    /// The surface a structural read of the class-expression (or object
    /// literal) instance `instance` reads through. An object literal's
    /// `this` is the literal itself, never the reference it is read
    /// through, so its surface reads with that `this` bound to the
    /// instance.
    pub(super) fn class_expression_read_surface(
        &self,
        instance: SemanticNodeId,
    ) -> Option<SemanticNodeId> {
        let (surface, object_literal) = match self.graph().node_data(instance)?.as_ref() {
            SemanticNodeData::ClassExpressionInstance {
                identity, surface, ..
            } => (*surface, identity.object_literal),
            _ => return None,
        };
        Some(if object_literal {
            self.bind_this_receiver(surface, instance)
        } else {
            surface
        })
    }

    /// Whether `binder` is the `this` of the object literal `identity`
    /// names — the binder its methods read the literal through.
    pub(super) fn is_object_literal_this_binder(
        &self,
        identity: &crate::semantic_query::ClassExpressionIdentity,
        binder: SemanticNodeId,
    ) -> bool {
        matches!(
            self.graph().node_data(binder).as_deref(),
            Some(SemanticNodeData::TypeParam { decl, param_index: THIS_BINDER_INDEX, .. })
                if decl.canonical_id == identity.canonical_id
                    && decl.owner == identity.owner
                    && decl.decl_name.as_ref() == object_literal_this_name(identity.offset)
        )
    }

    /// Bind every polymorphic `this` binder `node` mentions to
    /// `receiver`, the reference its member was read through.
    pub(super) fn bind_this_receiver(
        &self,
        node: SemanticNodeId,
        receiver: SemanticNodeId,
    ) -> SemanticNodeId {
        let mut binders: Vec<SemanticNodeId> = Vec::new();
        let mut deferred = Vec::new();
        let mut visited: rustc_hash::FxHashSet<SemanticNodeId> = rustc_hash::FxHashSet::default();
        let mut stack = vec![node];
        while let Some(current) = stack.pop() {
            if !visited.insert(current) {
                continue;
            }
            let Some(data) = self.graph().node_data(current) else {
                continue;
            };
            if let SemanticNodeData::Signature {
                return_carrier: crate::semantic_query::SignatureReturnCarrier::DeferredFlow(source),
                ..
            } = data.as_ref()
            {
                if source.receiver() != Some(receiver) {
                    let mut bound = data.as_ref().clone();
                    if let SemanticNodeData::Signature { return_carrier, .. } = &mut bound {
                        *return_carrier =
                            crate::semantic_query::SignatureReturnCarrier::DeferredFlow(
                                source.clone().with_receiver(receiver),
                            );
                    }
                    deferred.push((
                        current,
                        self.graph().intern_preserving_scope(current, bound),
                    ));
                }
            }
            if let SemanticNodeData::DeferredCallable(callable) = data.as_ref() {
                let bound = callable.with_receiver(receiver);
                if &bound != callable {
                    deferred.push((
                        current,
                        self.graph().intern_preserving_scope(
                            current,
                            SemanticNodeData::DeferredCallable(bound),
                        ),
                    ));
                }
            }
            if let SemanticNodeData::TypeParam { param_index, .. } = data.as_ref() {
                if *param_index == THIS_BINDER_INDEX && current != receiver {
                    binders.push(current);
                }
                continue;
            }
            // An object literal's own `this` is bound by the literal's
            // identity, never by a reference it is read through.
            if let SemanticNodeData::ClassExpressionInstance { identity, .. } = data.as_ref() {
                if identity.object_literal {
                    continue;
                }
            }
            let _ = data.for_each_child(|child| stack.push(child));
        }
        let node = deferred.into_iter().fold(node, |result, (source, bound)| {
            self.substitute_semantic_type_param(result, source, bound)
        });
        binders.into_iter().fold(node, |result, binder| {
            self.substitute_semantic_type_param(result, binder, receiver)
        })
    }

    /// The bare `this` types a callee reads in its own signature positions —
    /// a declared member's polymorphic `this` (`self(): this`,
    /// `wrap(): Promise<this>`), which the reference the member is read
    /// through binds. The walk stops at object surfaces and declaration
    /// contributors: a `this` inside one belongs to that type.
    pub(super) fn receiver_this_types(&self, node: SemanticNodeId) -> Vec<SemanticNodeId> {
        let mut found: Vec<SemanticNodeId> = Vec::new();
        let mut visited: rustc_hash::FxHashSet<SemanticNodeId> = rustc_hash::FxHashSet::default();
        let mut stack = vec![node];
        while let Some(current) = stack.pop() {
            if !visited.insert(current) {
                continue;
            }
            let Some(data) = self.graph().node_data(current) else {
                continue;
            };
            if data.bare_ref_head().is_some_and(|(name, _)| name == "this") {
                found.push(current);
                continue;
            }
            if matches!(
                data.as_ref(),
                SemanticNodeData::Object(_)
                    | SemanticNodeData::MergedDecl { .. }
                    | SemanticNodeData::ClassExpressionInstance { .. }
            ) {
                continue;
            }
            let _ = data.for_each_child(|child| stack.push(child));
        }
        found
    }

    construction_ordinal_seam! {
        /// `callee` read through `receiver`: its polymorphic `this` — a class
        /// member's `this` binder and a declared member's bare `this` — bound to
        /// the receiver, the checker's instantiation of a member's `this` type
        /// with the reference it is accessed through.
        fn bind_callee_receiver(
            &self,
            callee: SemanticNodeId,
            receiver: SemanticNodeId,
        ) -> SemanticNodeId {
            let bound = self.bind_this_receiver(callee, receiver);
            self.receiver_this_types(bound)
                .into_iter()
                .fold(bound, |result, this| {
                    self.substitute_semantic_type_param(result, this, receiver)
                })
        }
    }

    /// Rebind the ENCLOSING type parameters a body-derived return mentions
    /// to what the declaration lowering reading it binds them to.
    ///
    /// The flow lane interns an enclosing clause's parameter (a class's
    /// `H` in a method body) as the signature-scoped binder of the
    /// defining file — `DeclIdentity::from_scope(scope, name)` at ordinal
    /// 0 — while the lowering that reads the return binds the same name to
    /// its own binder or to an instantiation argument (`GH<number>` binds
    /// `H` to `number`). Every such binder of `canonical` whose name
    /// `bind` answers is replaced by the answer. A signature's OWN
    /// parameters are the caller's to refuse: the call resolver
    /// instantiates those through the body-derived carrier itself.
    pub(super) fn rebind_flow_return_binders(
        &self,
        node: SemanticNodeId,
        canonical: &str,
        bind: impl Fn(&str) -> Option<SemanticNodeId>,
    ) -> SemanticNodeId {
        let mut binders: Vec<(SemanticNodeId, Arc<str>)> = Vec::new();
        let mut visited: rustc_hash::FxHashSet<SemanticNodeId> = rustc_hash::FxHashSet::default();
        let mut stack = vec![node];
        while let Some(current) = stack.pop() {
            if !visited.insert(current) {
                continue;
            }
            let Some(data) = self.graph().node_data(current) else {
                continue;
            };
            if let SemanticNodeData::TypeParam {
                decl,
                param_index: 0,
                display_name,
                ..
            } = data.as_ref()
            {
                if decl.canonical_id.as_ref() == canonical && decl.decl_name == *display_name {
                    binders.push((current, Arc::<str>::from(display_name)));
                }
                continue;
            }
            let _ = data.for_each_child(|child| stack.push(child));
        }
        let bindings: Vec<_> = binders
            .into_iter()
            .filter_map(|(binder, name)| {
                bind(&name)
                    .filter(|target| *target != binder)
                    .map(|target| (binder, target))
            })
            .collect();
        self.substitute_flow_bindings_simultaneously(
            node,
            canonical,
            verter_type_expr::TopLevelOwnerId::ordinary_file(),
            &bindings,
        )
    }

    /// Bind a function's OWN clause in its body-derived return by
    /// declaration order, exactly as an instantiated frame's binder
    /// environment binds it: every binder the flow lane interned for
    /// `names[i]` in `canonical`'s `owner` scope becomes `args[i]`, all
    /// at once.
    ///
    /// A nested function value's clause interns its own binder identities
    /// (see the flow lane's binder environment), so a returned generic
    /// function value re-declaring one of `names` keeps its own parameter,
    /// and so does a function TYPE written with its own clause, whose
    /// binders are named by the clause's content. A signature that declares
    /// one of the binders being replaced — the same node, as a same-file
    /// generic function with a body-derived return holds when it is
    /// returned by name (`return { x, h }` from `l0<T>` over `h<T>`) —
    /// answers `None`, and so does an ordinal `args` does not cover.
    ///
    /// The binding is simultaneous: `g<A, B>` instantiated at `[B, A]`
    /// from a same-file `f<A, B>` (whose root clause shares `g`'s
    /// name-keyed binders) swaps them, as the instantiated frame's
    /// environment does, rather than collapsing both onto one.
    pub(super) fn bind_flow_return_own_clause(
        &self,
        node: SemanticNodeId,
        canonical: &str,
        owner: verter_type_expr::TopLevelOwnerId,
        names: &[Arc<str>],
        args: &[SemanticNodeId],
    ) -> Option<SemanticNodeId> {
        let mut binders: Vec<(SemanticNodeId, SemanticNodeId)> = Vec::new();
        let mut declared: rustc_hash::FxHashSet<SemanticNodeId> = rustc_hash::FxHashSet::default();
        let mut visited: rustc_hash::FxHashSet<SemanticNodeId> = rustc_hash::FxHashSet::default();
        let mut stack = vec![node];
        while let Some(current) = stack.pop() {
            if !visited.insert(current) {
                continue;
            }
            let data = self.graph().node_data(current)?;
            match data.as_ref() {
                SemanticNodeData::TypeParam {
                    decl,
                    param_index: 0,
                    display_name,
                    ..
                } => {
                    if decl.canonical_id.as_ref() == canonical
                        && decl.owner == owner
                        && decl.decl_name == *display_name
                    {
                        if let Some(ordinal) = names.iter().position(|name| name == display_name) {
                            binders.push((current, *args.get(ordinal)?));
                        }
                    }
                    continue;
                }
                SemanticNodeData::Signature {
                    type_parameters, ..
                } => declared.extend(type_parameters.iter().map(|param| param.param)),
                _ => {}
            }
            let _ = data.for_each_child(|child| stack.push(child));
        }
        binders.retain(|(binder, argument)| binder != argument);
        if binders.iter().any(|(binder, _)| declared.contains(binder)) {
            return None;
        }
        Some(self.substitute_flow_bindings_simultaneously(node, canonical, owner, &binders))
    }

    fn substitute_flow_bindings_simultaneously(
        &self,
        node: SemanticNodeId,
        canonical: &str,
        owner: verter_type_expr::TopLevelOwnerId,
        binders: &[(SemanticNodeId, SemanticNodeId)],
    ) -> SemanticNodeId {
        let replaced: Vec<SemanticNodeId> = binders.iter().map(|(binder, _)| *binder).collect();
        // One binder at a time is already simultaneous unless an argument
        // mentions a binder being replaced; then every binder first moves
        // to a placeholder no argument can mention.
        if !binders
            .iter()
            .any(|(_, argument)| self.mentions_any(*argument, &replaced))
        {
            return binders.iter().fold(node, |result, (binder, argument)| {
                self.substitute_semantic_type_param(result, *binder, *argument)
            });
        }
        let placeholders: Vec<SemanticNodeId> = (0..binders.len())
            .map(|ordinal| self.simultaneous_binding_placeholder(canonical, owner, ordinal))
            .collect();
        let parked =
            binders
                .iter()
                .zip(&placeholders)
                .fold(node, |result, ((binder, _), placeholder)| {
                    self.substitute_semantic_type_param(result, *binder, *placeholder)
                });
        binders
            .iter()
            .zip(&placeholders)
            .fold(parked, |result, ((_, argument), placeholder)| {
                self.substitute_semantic_type_param(result, *placeholder, *argument)
            })
    }

    /// The `ordinal`-th placeholder binder of a simultaneous binding in
    /// `canonical`'s `owner` scope: a `TypeParam` whose identity no
    /// authored or flow-minted clause produces (its name begins with the
    /// `\u{1}` separator no identifier can hold), interned once per
    /// ordinal and never left in a bound value.
    fn simultaneous_binding_placeholder(
        &self,
        canonical: &str,
        owner: verter_type_expr::TopLevelOwnerId,
        ordinal: usize,
    ) -> SemanticNodeId {
        let name = verter_type_expr::OwnedStr::from(format!("\u{1}{ordinal}"));
        self.graph().intern_node(SemanticNodeData::TypeParam {
            decl: crate::semantic_query::DeclIdentity {
                canonical_id: verter_type_expr::OwnedStr::from(canonical),
                owner,
                whole_hash: crate::semantic_query::HashValue::default(),
                decl_name: name.clone(),
            },
            param_index: 0,
            constraint: None,
            default: None,
            display_name: name,
        })
    }

    /// Whether `node` is, or reaches through its children, any of `targets`.
    fn mentions_any(&self, node: SemanticNodeId, targets: &[SemanticNodeId]) -> bool {
        if targets.is_empty() {
            return false;
        }
        let mut visited: rustc_hash::FxHashSet<SemanticNodeId> = rustc_hash::FxHashSet::default();
        let mut stack = vec![node];
        while let Some(current) = stack.pop() {
            if targets.contains(&current) {
                return true;
            }
            if !visited.insert(current) {
                continue;
            }
            if let Some(data) = self.graph().node_data(current) {
                let _ = data.for_each_child(|child| stack.push(child));
            }
        }
        false
    }

    /// Apply a stored positional substitution frame to a demanded winner
    /// (or to an open branch that a consumer has already selected). Empty
    /// frames are identity.
    pub(super) fn apply_pending_substitution_frame(
        &self,
        node: SemanticNodeId,
        frame: &PendingSubstitutionFrame,
    ) -> SemanticNodeId {
        let mut evidence = super::canonical_algebra::CanonicalEvidence::default();
        evidence.inspected_file_roots = self
            .observed_self_roots_from_nodes(frame.pairs().iter().map(|&(_, argument)| argument));
        self.deposit_canonical_evidence(evidence);
        let mut result = node;
        for &(param, arg) in frame.pairs() {
            if self.cancellation.is_cancelled() {
                return self.opaque(QueryError::Miss);
            }
            if !self.subtree_references_node(result, param) {
                continue;
            }
            result = self.substitute_semantic_type_param(result, param, arg);
        }
        result
    }

    construction_ordinal_seam! {
        fn apply_conditional_branch_pending(
            &self,
            node: SemanticNodeId,
            pending: Option<&ConditionalPendingSubstitution>,
            true_branch: bool,
        ) -> SemanticNodeId {
            let Some(pending) = pending else {
                return node;
            };
            let frame = if true_branch {
                pending.true_branch()
            } else {
                pending.false_branch()
            };
            self.apply_pending_substitution_frame(node, frame)
        }
    }

    /// Count one arrival at `(node, parameter, arg)` in the request's
    /// audit footprint: unique or repeated within the request.
    fn classify_recursive_substitution(
        &self,
        node: SemanticNodeId,
        parameter_node: SemanticNodeId,
        arg: SemanticNodeId,
    ) {
        crate::observe_request!(C::Observe => on self.observation.request.as_deref() => |ctx| {
            ctx.classify_recursive_substitute(RecursiveSubstituteIdentity {
                node: node.0,
                parameter_node: parameter_node.0,
                arg: arg.0,
            })
        });
    }

    /// Declaration-scoped shadow predicate for the Conditional
    /// substitution arm: `true` iff `pattern` — a conditional's own
    /// `extends` pattern — DECLARES the infer binder `binder` at THIS
    /// pattern's level. Only a reachable `Infer` node with the binder's
    /// name counts (name-identity interning makes the inner declaration
    /// the binder node itself); explicitly NOT counted:
    ///
    /// - a bare REFERENCE to the name (a same-name `TypeParam` shell —
    ///   references never re-bind);
    /// - an `Infer` beneath a nested `Conditional` or `Mapped` inside the
    ///   pattern — TS scopes `infer` to the NEAREST enclosing conditional
    ///   (and a mapped type introduces its own binder scope), so such a
    ///   declaration binds at that inner level, never at this one.
    ///
    /// This is deliberately a separate predicate from
    /// [`Self::subtree_references_node`], whose unrestricted
    /// reference-reachability semantics its other callers depend on.
    pub fn extends_pattern_declares_infer(
        &self,
        pattern: SemanticNodeId,
        binder: SemanticNodeId,
    ) -> bool {
        let Some(SemanticNodeData::Infer {
            binder: target_binder,
            ..
        }) = self
            .graph()
            .node_data(binder)
            .as_deref()
            .map(crate::semantic_query::SemanticNodeData::clone)
        else {
            return false;
        };
        let graph = self.graph();
        let mut visited: rustc_hash::FxHashSet<SemanticNodeId> = rustc_hash::FxHashSet::default();
        let mut stack: Vec<SemanticNodeId> = vec![pattern];
        while let Some(node) = stack.pop() {
            if !visited.insert(node) {
                continue;
            }
            let Some(data) = graph.node_data(node) else {
                continue;
            };
            match data.as_ref() {
                SemanticNodeData::Infer { binder, .. } => {
                    if *binder == target_binder {
                        return true;
                    }
                }
                // A nested CONDITIONAL is the only infer-binding
                // boundary: an `infer` beneath it declares at THAT
                // conditional's level — do not descend. A `Mapped` is NOT
                // a boundary — an `infer` in its source / value /
                // `as`-remap declares for the ENCLOSING conditional (the
                // `conditional_binds_mapped_as_remap_infer_in_true_branch`
                // producer contract).
                SemanticNodeData::Conditional { .. } => {}
                SemanticNodeData::Mapped { source, mapper } => {
                    stack.push(*source);
                    stack.push(mapper.key_space);
                    stack.push(mapper.value_expr);
                    if let Some(remap) = mapper.name_remap {
                        stack.push(remap);
                    }
                    stack.extend(mapper.modifiers_type);
                }
                // A construction program is not an infer-binding boundary:
                // an `infer` inside a program effect declares for the
                // ENCLOSING conditional (same rule as an `infer` inside an
                // `Object` member value above). Mirrors the substitute
                // engine's `map_child_nodes` descent and `absorb`'s
                // `child_nodes` stack push.
                SemanticNodeData::ObjectSpreadProgram(program) => {
                    stack.extend(program.child_nodes());
                }
                SemanticNodeData::Alias(inner) => stack.push(*inner),
                composite @ (SemanticNodeData::Union(_) | SemanticNodeData::Intersection(_)) => {
                    let members = composite.composite_members().expect("composite arm");
                    for member in members.iter() {
                        stack.push(*member);
                    }
                }
                SemanticNodeData::Array { element, .. } => stack.push(*element),
                SemanticNodeData::Tuple { elements, .. } => {
                    for element in elements.iter() {
                        stack.push(element.value);
                    }
                }
                SemanticNodeData::Object(surface) => {
                    for member in surface.positive_members().iter() {
                        stack.push(member.value);
                    }
                    for signature in surface.call_signatures.iter() {
                        stack.push(*signature);
                    }
                    for signature in surface.construct_signatures.iter() {
                        stack.push(*signature);
                    }
                    for signature in surface.index_signatures.iter() {
                        stack.push(signature.key_type);
                        stack.push(signature.value_type);
                    }
                    if let Some(k) = surface.keyspace {
                        stack.push(k);
                    }
                }
                SemanticNodeData::Signature {
                    params,
                    return_type,
                    type_parameters,
                    predicate,
                    ..
                } => {
                    for param in params.iter() {
                        stack.push(param.ty);
                    }
                    stack.push(*return_type);
                    for tp in type_parameters.iter() {
                        if let Some(c) = tp.constraint {
                            stack.push(c);
                        }
                        if let Some(d) = tp.default {
                            stack.push(d);
                        }
                    }
                    stack.extend(predicate.and_then(|predicate| predicate.ty));
                }
                SemanticNodeData::InstantiationRef { args, .. } => {
                    for arg in args.iter() {
                        stack.push(*arg);
                    }
                }
                SemanticNodeData::TemplateLiteral { expressions, .. } => {
                    for expr in expressions.iter() {
                        stack.push(*expr);
                    }
                }
                SemanticNodeData::KeyOf { base } => stack.push(*base),
                SemanticNodeData::IndexedAccess { object, index, .. } => {
                    stack.push(*object);
                    if let crate::semantic_query::IndexKey::Computed(idx_node) = index {
                        stack.push(*idx_node);
                    }
                }
                _ => {}
            }
        }
        false
    }

    /// Read-only walker that returns `true` iff `target` is structurally
    /// reachable from `root` through the same recursion edges the substitution walk descends
    /// into ([`Self::open_substitution_children`]). Mirrors
    /// the substitute helper's structural recursion so the two stay in
    /// lock-step: a `false` return here means an identical
    /// substitution of `target` in `root` would answer `root`
    /// unchanged — i.e. no descendant references `target` so
    /// substitution is the identity on the entire subtree.
    ///
    /// Used by [`Self::build_mapped_type`] to hoist key-independent
    /// `value_expr` reduction out of the per-K materialisation loop:
    /// when the mapper's binder is not reachable inside `value_expr`,
    /// the per-K substituted carrier collapses to `value_expr` itself
    /// for every K, so the downstream evaluation is shared across the
    /// entire key space and the materialiser runs ONCE per mapped type
    /// rather than ONCE per enumerated key.
    ///
    /// **Recursion mirrors substitute exactly.** Every arm of
    /// the substitution walk that descends into child
    /// `SemanticNodeId`s descends here too — including the three
    /// unresolved carriers (`BareRef` / `TypeOf` / `ImportType`), whose
    /// `type_args` slices ARE descended. As a read-only reachability SCAN
    /// it reaches them through the shared
    /// [`SemanticNodeData::carrier_type_args`] accessor in the catch-all
    /// arm (so a future carrier added to that exhaustive accessor is
    /// descended automatically, never silently dropped) — the mirror
    /// contract. Arms that return `(node, false)` without recursion (leaf
    /// TypeParam / Primitive / Literal / Opaque / DeclRef / Never /
    /// Unknown / Any) terminate here too — the accessor
    /// returns an empty slice for them. Cyclic graphs are guarded by a
    /// `visited` set.
    ///
    /// **`Mapped` shadowing** is honoured: when a nested mapped binder
    /// shadows the same `target` (`nested_mapper.parameter_node ==
    /// target`), the walker does NOT recurse into the shadowed
    /// `value_expr` / `name_remap` arms, matching substitute's
    /// `shadowed` short-circuit.
    ///
    /// Infer reachability mirrors substitution exactly: only `Infer` /
    /// `InferRef` nodes carrying the target declaration's opaque binder count.
    /// Display names never participate.
    pub fn subtree_references_node(&self, root: SemanticNodeId, target: SemanticNodeId) -> bool {
        self.enter_substitution_walk();
        if root != target && self.binder_dependency_proves_absent(root, target) {
            return false;
        }
        let target_infer_binder = match self.substitution_binder(target) {
            BinderKind::Infer(binder) => Some(binder),
            _ => None,
        };

        let mut visited = crate::semantic_query::small_node_set::SmallNodeSet::default();
        let mut stack: Vec<SemanticNodeId> = Vec::new();
        stack.push(root);
        while let Some(node) = stack.pop() {
            if node == target {
                return true;
            }
            if !visited.insert(node) {
                continue;
            }
            let Some(data) = self.graph().node_data(node) else {
                continue;
            };
            match data.as_ref() {
                // Exact declaration/reference binder match.
                SemanticNodeData::Infer { binder, .. }
                | SemanticNodeData::InferRef { binder, .. } => {
                    if target_infer_binder == Some(binder.clone()) {
                        return true;
                    }
                }
                SemanticNodeData::TypeParam { .. } => {}
                SemanticNodeData::Alias(t) => {
                    stack.push(*t);
                }
                SemanticNodeData::ClassExpressionInstance {
                    type_arguments,
                    surface,
                    ..
                } => {
                    stack.extend(type_arguments.iter().copied());
                    stack.push(*surface);
                }
                composite @ (SemanticNodeData::Union(_) | SemanticNodeData::Intersection(_)) => {
                    let members = composite.composite_members().expect("composite arm");
                    for member in members.iter() {
                        stack.push(*member);
                    }
                }
                SemanticNodeData::Array { element, .. } => {
                    stack.push(*element);
                }
                SemanticNodeData::Tuple { elements, .. } => {
                    for element in elements.iter() {
                        stack.push(element.value);
                    }
                }
                SemanticNodeData::Object(surface) => {
                    for member in surface.positive_members().iter() {
                        stack.push(member.value);
                    }
                    for signature in surface.call_signatures.iter() {
                        stack.push(*signature);
                    }
                    for signature in surface.construct_signatures.iter() {
                        stack.push(*signature);
                    }
                    for signature in surface.index_signatures.iter() {
                        stack.push(signature.key_type);
                        stack.push(signature.value_type);
                    }
                    if let Some(k) = surface.keyspace {
                        stack.push(k);
                    }
                }
                SemanticNodeData::TemplateLiteral { expressions, .. } => {
                    for expr in expressions.iter() {
                        stack.push(*expr);
                    }
                }
                SemanticNodeData::KeyOf { base } => {
                    stack.push(*base);
                }
                SemanticNodeData::IndexedAccess { object, index, .. } => {
                    stack.push(*object);
                    if let IndexKey::Computed(idx_node) = index {
                        stack.push(*idx_node);
                    }
                }
                SemanticNodeData::Mapped { source, mapper } => {
                    let shadowed = mapper.parameter_node == target;
                    stack.push(*source);
                    stack.push(mapper.key_space);
                    stack.extend(mapper.modifiers_type);
                    if !shadowed {
                        stack.push(mapper.value_expr);
                        if let Some(remap) = mapper.name_remap {
                            stack.push(remap);
                        }
                    }
                }
                SemanticNodeData::Conditional {
                    check,
                    extends,
                    true_branch_ref,
                    false_branch_ref,
                    pending,
                    ..
                } => {
                    if let Some(pending) = pending {
                        stack.extend(pending.argument_nodes());
                    }
                    stack.push(*check);
                    stack.push(*extends);
                    stack.push(*true_branch_ref);
                    stack.push(*false_branch_ref);
                }
                SemanticNodeData::InstantiationRef { args, .. } => {
                    for arg in args.iter() {
                        stack.push(*arg);
                    }
                }
                // Substitute descends program effects via `map_child_nodes`;
                // the scanner must descend the same children or it reports a
                // program value holding the binder as K-independent (the
                // `build_mapped_type` hoist) / binder-free (the
                // `record_target_shape` generic-key gate).
                SemanticNodeData::ObjectSpreadProgram(program) => {
                    stack.extend(program.child_nodes());
                }
                SemanticNodeData::MergedDecl { contributors } => {
                    for contributor in contributors.iter() {
                        stack.push(*contributor);
                    }
                }
                SemanticNodeData::Signature {
                    params,
                    return_type,
                    type_parameters,
                    predicate,
                    ..
                } => {
                    for param in params.iter() {
                        stack.push(param.ty);
                    }
                    stack.push(*return_type);
                    for tp in type_parameters.iter() {
                        if let Some(c) = tp.constraint {
                            stack.push(c);
                        }
                        if let Some(d) = tp.default {
                            stack.push(d);
                        }
                    }
                    stack.extend(predicate.and_then(|predicate| predicate.ty));
                }
                // Unresolved carriers (`BareRef` / `TypeOf` / `ImportType`)
                // descend into their structural `type_args` exactly as
                // substitute's carrier arms do — the mirror contract. As a
                // SCAN this routes through the shared `carrier_type_args`
                // accessor in the catch-all below rather than hand-binding
                // each carrier, so a future carrier added to the exhaustive
                // accessor is descended here automatically and can never be
                // silently dropped by the catch-all.
                //
                // The catch-all also covers substitute's `_ => (node, false)`
                // leaf variants (Primitive, Literal, Opaque, DeclRef, Never,
                // Unknown, Any): none have child
                // semantic-node references, and the accessor returns an empty
                // slice for them, so the walker pushes nothing.
                other => {
                    for arg in other.carrier_type_args() {
                        stack.push(*arg);
                    }
                }
            }
        }
        false
    }
}

/// What a substitution walk's binder is: a declared type parameter, an
/// `infer` declaration with its opaque identity, or neither (a binder that
/// matches only itself).
#[derive(Clone)]
pub(super) enum BinderKind {
    TypeParam,
    Infer(crate::semantic_query::InferBinderId),
    Other,
}
/// Which probes an arrival at a substitution makes before its level opens:
/// a public substitution's — the entry every caller makes — or a child's,
/// a level handing one of its children on.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Arrival {
    Public,
    Child,
}

/// A substitution walk as an owned continuation: its open levels, root
/// first, with the children each visits and the values they substituted to
/// kept flat, a level's own at the end while it is on top. It stands inside
/// its walk or at the template read its top level names
/// ([`FrameStage::AwaitTemplate`]), and borrows nothing of the dispatcher
/// that stepped it, so a continuation embedding it carries it to whichever
/// thread resumes it. Empty between substitutions; an emptied walk keeps
/// its buffers for the next.
#[derive(Default)]
pub(super) struct SubstitutionWalk {
    frames: Vec<SubstitutionFrame>,
    children: Vec<SemanticNodeId>,
    values: Vec<SemanticNodeId>,
    /// The value of the level or arrival just finished, for the level on
    /// top, and whether it changed.
    answer: Option<(SemanticNodeId, bool)>,
}

// A suspended walk is owned state its holder's continuation carries to
// whichever thread resumes it, advanced by one driver at a time.
static_assertions::assert_impl_all!(SubstitutionWalk: Send);
static_assertions::assert_not_impl_any!(SubstitutionWalk: Clone, Copy);

/// Where a substitution walk stands after its latest step.
pub(super) enum SubstitutionStep {
    /// The substitution's result.
    Done(SemanticNodeId),
    /// The walk reads this template reduction next, holding everything it
    /// computed.
    Read(SemanticQueryKey),
}

/// One open level of a walk: `(node, parameter, arg)` after its memo
/// missed.
struct SubstitutionFrame {
    node: SemanticNodeId,
    parameter: SemanticNodeId,
    arg: SemanticNodeId,
    /// The node's payload, read once as the level opened.
    data: crate::semantic_query_memo::NodeData,
    /// Where the level's children and values begin in the walk's lists.
    children_from: usize,
    values_from: usize,
    /// Whether any child substituted to a change, and whether the last
    /// one did.
    any_changed: bool,
    last_changed: bool,
    /// The one shadow decision the level's arm makes as it opens: an
    /// object literal's own `this`, a mapped type's own binder, a nested
    /// conditional re-declaring an `infer` binder.
    flag: bool,
    /// The canonical evidence epoch as the level opened.
    epoch_before: u64,
    /// Whether the level's cost recording is open.
    recording: bool,
    stage: FrameStage,
}

/// What a level is doing.
enum FrameStage {
    /// Visiting its children.
    Children,
    /// Standing at its template literal's reduction, every hole substituted.
    AwaitTemplate,
    /// Renaming a signature's clause apart from the argument before the
    /// walk enters it.
    Capture(Box<CaptureAvoidance>),
    /// Substituting the capture-avoided signature, whose value is the
    /// level's.
    Fresh,
    /// Re-substituting the binders its rebuild moved into its parameters,
    /// return and predicate.
    Rebind(Box<SignatureRebind>),
}

/// How a level opens.
enum Opening {
    /// It has no child to visit: its value.
    Leaf((SemanticNodeId, bool)),
    /// It visits the children it listed.
    Children { flag: bool },
    /// It renames its signature's clause first.
    Capture(Box<CaptureAvoidance>),
}

/// What a level's rebuild gives.
enum Rebuilt {
    Node(SemanticNodeId, bool),
    /// A changed template whose every hole folds names its reduction.
    Template(SemanticQueryKey),
    /// A signature whose clause moved has binders to rebind first.
    Rebind(Box<SignatureRebind>),
}

/// The values a level's children substituted to, read back in the order
/// the level visited them.
struct ValueCursor<'v> {
    values: &'v [SemanticNodeId],
    at: usize,
}

impl<'v> ValueCursor<'v> {
    fn next(&mut self) -> SemanticNodeId {
        let value = self.values[self.at];
        self.at += 1;
        value
    }

    /// Every value not yet read.
    fn rest(&mut self) -> &'v [SemanticNodeId] {
        let rest = &self.values[self.at..];
        self.at = self.values.len();
        rest
    }

    fn end(&self) {
        verter_debug_assert!(
            self.at == self.values.len(),
            "a level's rebuild reads one value per child it visited"
        );
    }
}

/// The closure of one node under a canonical substitution: bounded passes
/// of one public substitution per binding, stopping at the first pass that
/// changes nothing. The bindings stay with the closure's holder.
pub(super) struct CanonicalClosure {
    node: SemanticNodeId,
    before: SemanticNodeId,
    pass: usize,
    at: usize,
}

impl CanonicalClosure {
    pub(super) fn new(node: SemanticNodeId) -> Self {
        Self {
            node,
            before: node,
            pass: 0,
            at: 0,
        }
    }

    /// The node closed so far.
    pub(super) fn node(&self) -> SemanticNodeId {
        self.node
    }

    /// The binding the closure substitutes into its node next, or `None`
    /// once it is closed.
    pub(super) fn next(
        &mut self,
        bindings: &[(SemanticNodeId, SemanticNodeId)],
    ) -> Option<(SemanticNodeId, SemanticNodeId)> {
        // bounded-loop: at most one substitution-closure pass per canonical
        // binding, after the first.
        loop {
            if let Some(binding) = bindings.get(self.at) {
                return Some(*binding);
            }
            if self.node == self.before || self.pass >= bindings.len() {
                return None;
            }
            self.pass += 1;
            self.at = 0;
            self.before = self.node;
        }
    }

    /// Take the substitution of the binding [`Self::next`] named.
    pub(super) fn accept(&mut self, value: SemanticNodeId) {
        self.node = value;
        self.at += 1;
    }
}

/// A signature's clause being renamed apart from an outer argument: the
/// fresh parameter shells, the clause's variant occurrences, the fresh
/// parameters made so far and the closure standing open.
struct CaptureAvoidance {
    parameters: Arc<[TypeParamDecl]>,
    shells: Vec<SemanticNodeId>,
    variants: Vec<(SemanticNodeId, usize)>,
    fresh: Vec<TypeParamDecl>,
    /// The clause parameter being renamed.
    ordinal: usize,
    slot: CaptureSlot,
    /// The bindings the open closure substitutes.
    bindings: Option<crate::semantic_query::CanonicalTypeSubstitution>,
    /// The renamed parameter's closed constraint.
    constraint: Option<SemanticNodeId>,
    closure: Option<CanonicalClosure>,
}

impl CaptureAvoidance {
    fn accept(&mut self, value: SemanticNodeId) {
        self.closure
            .as_mut()
            .expect("a capture is answered for its open closure")
            .accept(value);
    }
}

/// What a capture closes next.
#[derive(Clone, Copy, PartialEq, Eq)]
enum CaptureSlot {
    Constraint,
    Default,
    Body,
}

/// Where a capture avoidance stands after its latest step.
enum CaptureStep {
    /// It substitutes the binding `(parameter, bound)` into the node.
    Arrive(SemanticNodeId, SemanticNodeId, SemanticNodeId),
    /// The capture-avoided signature, substituted in the level's place.
    Fresh(SemanticNodeId),
    /// A shell the graph no longer reads: the level substitutes its own
    /// children.
    Abort,
}

/// A signature's rebuild once its children are done: its substituted
/// parameters, return, predicate and clause. The clause is finished
/// parameter by parameter: each one's bounds follow the binders of the
/// parameters before it that moved (`U = T` in `<T extends A, U = T>`
/// names `T`'s binder), one `(old, new)` pair at a time, and its binder is
/// re-interned when its bounds moved. Then every moved binder is rebound,
/// one pair at a time, into the parameters, return and predicate.
struct SignatureRebind {
    new_params: Vec<FunctionParam>,
    sub_return: SemanticNodeId,
    sub_predicate: Option<crate::semantic_query::SignaturePredicate>,
    new_type_parameters: Vec<TypeParamDecl>,
    mapped_carrier: Option<crate::semantic_query::SignatureReturnCarrier>,
    /// The binders the finished clause parameters moved, as `(old, new)`.
    moved: Vec<(SemanticNodeId, SemanticNodeId)>,
    /// The clause parameter being finished (the clause is finished once it
    /// reaches the clause's length), its bound (constraint, then default)
    /// following the moved pairs, and the pair it stands at.
    clause_at: usize,
    bound: usize,
    /// The pair being rebound (in the clause, into the bound), and the
    /// parameter (or the return after them, then the predicate) it stands
    /// at once the clause is finished.
    pair: usize,
    slot: usize,
}

impl SignatureRebind {
    /// The substitution `(node, old, new)` the positions' rebind makes
    /// next, or `None` once every pair is rebound.
    fn next_position(&mut self) -> Option<(SemanticNodeId, SemanticNodeId, SemanticNodeId)> {
        loop {
            let &(old, new) = self.moved.get(self.pair)?;
            let params = self.new_params.len();
            if self.slot < params {
                return Some((self.new_params[self.slot].ty, old, new));
            }
            if self.slot == params {
                return Some((self.sub_return, old, new));
            }
            if self.slot == params + 1 {
                if let Some(target) = self.sub_predicate.and_then(|predicate| predicate.ty) {
                    return Some((target, old, new));
                }
            }
            self.pair += 1;
            self.slot = 0;
        }
    }

    /// Take the value of the substitution the rebind named last.
    fn accept(&mut self, value: SemanticNodeId) {
        if self.clause_at < self.new_type_parameters.len() {
            let tp = &mut self.new_type_parameters[self.clause_at];
            let slot = if self.bound == 0 {
                &mut tp.constraint
            } else {
                &mut tp.default
            };
            *slot = Some(value);
            self.pair += 1;
            return;
        }
        let params = self.new_params.len();
        if self.slot < params {
            self.new_params[self.slot].ty = value;
        } else if self.slot == params {
            self.sub_return = value;
        } else {
            self.sub_predicate = self
                .sub_predicate
                .map(|predicate| predicate.map_type(|_| value));
        }
        self.slot += 1;
    }
}

/// The walk the public entry drives in place, released if the entry
/// unwinds — its levels' recordings abandoned so their charges stay the
/// consumer's — and handed back to the dispatcher as its spare walk.
struct HeldSubstitutionWalk<'d, 'a, C: crate::resolver_core::ResolverCapabilities> {
    dispatch: &'d ProjectSemanticDispatch<'a, C>,
    walk: SubstitutionWalk,
}

impl<C: crate::resolver_core::ResolverCapabilities> Drop for HeldSubstitutionWalk<'_, '_, C> {
    fn drop(&mut self) {
        self.dispatch.release_substitution_walk(&mut self.walk);
        *self.dispatch.substitution_spare.borrow_mut() = Some(std::mem::take(&mut self.walk));
    }
}
