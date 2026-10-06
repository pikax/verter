//! Typed accessors over the shared, selected flow product execution.
//!
//! Snapshots retain one canonical product store and share the same execution
//! capability. This adapter performs no lattice operations and owns no second
//! semantic state map.

use std::cell::RefCell;
use std::rc::Rc;
use verter_session_query::flow::binding::FlowBindingRef;
use verter_session_query::flow::flow_graph::FlowNodeId;

use super::flow_products::{
    DeclaredTypeProduct, DefiniteAssignmentProduct, FlowProductExecution, FlowProductFailure,
    FlowProductStore, FlowProductTransfer, FlowProductValue, FlowSemanticAlgebra, NarrowingProduct,
    ReachingTypeProduct, ReachingValueProduct, WideningMembership,
};
use super::flow_solve::FlowDomain;
use crate::semantic_query::SemanticNodeId;

/// Declaration lifetime metadata; it never participates in product identity.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FlowBindingLayer {
    Lexical,
    Function,
}

#[derive(Clone, PartialEq, Eq)]
pub struct FlowWriteSubject(
    verter_session_query::function_program::FunctionProgramKey,
    u32,
);

impl PartialOrd for FlowWriteSubject {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

// Counts subject/sequence key comparisons so the write-index probe can bound
// the work one write lookup performs.
#[cfg(any(test, feature = "test-support"))]
thread_local! {
    pub static WRITE_KEY_COMPARISONS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

impl Ord for FlowWriteSubject {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        #[cfg(any(test, feature = "test-support"))]
        WRITE_KEY_COMPARISONS.with(|count| count.set(count.get() + 1));
        (&self.0, self.1).cmp(&(&other.0, other.1))
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct FlowWriteSequence(u64);
impl PartialOrd for FlowWriteSequence {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for FlowWriteSequence {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        #[cfg(any(test, feature = "test-support"))]
        WRITE_KEY_COMPARISONS.with(|count| count.set(count.get() + 1));
        self.0.cmp(&other.0)
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct FlowWriteReceipt {
    pub sequence: u64,
    pub(crate) subject: FlowBindingRef,
}

pub struct FlowFrameExecution {
    pub execution: FlowProductExecution,
    pub content: super::flow_products::FlowProductContent,
    pub failure: Option<FlowProductFailure>,
    write_sequence: u64,
}

pub(super) type SharedFlowExecution = Rc<RefCell<FlowFrameExecution>>;

/// A scoped observation of successful transfers at a control entry.
#[derive(Clone)]
pub struct FlowWriteObservation {
    execution: SharedFlowExecution,
    sequence: u64,
}

impl FlowWriteObservation {
    /// Whether `other` observes the same execution at the same write.
    pub(super) fn same_as(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.execution, &other.execution) && self.sequence == other.sequence
    }
}

/// A snapshot of the sole product store, with typed binding accessors.
#[derive(Clone)]
pub struct FlowFrameProducts {
    pub(crate) state: FlowProductStore,
    pub execution: SharedFlowExecution,
    // Successful write receipts are control metadata, not semantic values.
    // Persistent snapshots retain exactly the writes on their own paths.
    pub writes: imbl::OrdMap<FlowWriteSubject, FlowWriteReceipt>,
    pub writes_by_sequence: imbl::OrdMap<FlowWriteSequence, FlowBindingRef>,
}

impl FlowFrameProducts {
    pub fn captured_input_bytes(
        declared: Option<SemanticNodeId>,
        reaching: Option<&ReachingTypeProduct>,
        assignment: DefiniteAssignmentProduct,
    ) -> Vec<u8> {
        use verter_identity::encoding::CanonicalEncoder;
        let mut e = CanonicalEncoder::new("verter.session.flow.captured_input.v1");
        let declared = declared.map(|node| node.0.to_le_bytes());
        e.field_option(1, declared.as_ref().map(|bytes| bytes.as_slice()));
        if let Some(reaching) = reaching {
            e.field_bool(2, true);
            for node in reaching.contributors() {
                e.field_u64(3, node.0);
            }
            let united = reaching.united().map(|node| node.0.to_le_bytes());
            e.field_option(4, united.as_ref().map(|bytes| bytes.as_slice()));
            match reaching.widening() {
                None => {
                    e.field_enum_discriminant(5, 0);
                }
                Some(WideningMembership::All) => {
                    e.field_enum_discriminant(5, 1);
                }
                Some(WideningMembership::Partial(values)) => {
                    e.field_enum_discriminant(5, 2);
                    for node in values.iter() {
                        e.field_u64(6, node.0);
                    }
                }
            }
        } else {
            e.field_bool(2, false);
        }
        e.field_bool(7, assignment.single_path());
        e.field_bool(8, assignment.failed_initializer());
        for (tag, state) in [
            super::flow_products::DefiniteAssignment::Unassigned,
            super::flow_products::DefiniteAssignment::Assigned,
            super::flow_products::DefiniteAssignment::MaybeAssigned,
        ]
        .into_iter()
        .enumerate()
        {
            if assignment == assignment.with_state(state) {
                e.field_u32(9, tag as u32);
            }
        }
        if reaching.is_some_and(ReachingTypeProduct::widening_nullish) {
            e.field_bool(10, true);
        }
        e.finish()
    }

    pub fn new(
        execution: FlowProductExecution,
        bound: &verter_session_query::flow::bundle::BoundFlowGraph,
    ) -> Result<Self, FlowProductFailure> {
        let content = execution.attach_content(bound)?;
        let state = execution.empty_state();
        Ok(Self {
            state,
            writes: imbl::OrdMap::new(),
            writes_by_sequence: imbl::OrdMap::new(),
            execution: Rc::new(RefCell::new(FlowFrameExecution {
                execution,
                content,
                failure: None,
                write_sequence: 0,
            })),
        })
    }

    pub fn contains_subject(&self, subject: &FlowBindingRef) -> bool {
        self.execution
            .borrow()
            .content
            .key_for_binding(FlowDomain::ReachingType, subject)
            .is_ok()
    }

    pub fn identity(
        &self,
        subject: &FlowBindingRef,
    ) -> Option<verter_session_query::function_program::FlowBindingIdentity> {
        self.execution
            .borrow()
            .content
            .key_for_binding(FlowDomain::ReachingType, subject)
            .ok()?
            .runtime_binding()
            .cloned()
    }

    pub fn key(
        &self,
        domain: FlowDomain,
        subject: &FlowBindingRef,
    ) -> Option<super::flow_products::FlowProductKey> {
        self.execution
            .borrow()
            .content
            .key_for_binding(domain, subject)
            .ok()
    }

    pub fn finish(
        &self,
        plan: Option<&super::flow_solve::FlowDemandPlan>,
    ) -> Result<Option<super::flow_products::FlowProductEvidence>, FlowProductFailure> {
        let mut frame = self.execution.borrow_mut();
        if let Some(failure) = &frame.failure {
            return Err(failure.clone());
        }
        match plan {
            Some(plan) => frame.execution.finish_with_plan(plan).map(Some),
            None => frame.execution.finish().map(|_| None),
        }
    }

    pub fn get(&self, domain: FlowDomain, subject: &FlowBindingRef) -> Option<&FlowProductValue> {
        let key = self
            .execution
            .borrow()
            .content
            .key_for_binding(domain, subject)
            .ok()?;
        self.state.get(&key)
    }

    fn set(
        &mut self,
        domain: FlowDomain,
        subject: &FlowBindingRef,
        value: Option<FlowProductValue>,
    ) {
        self.set_values(subject, [(domain, value)]);
    }

    fn set_values(
        &mut self,
        subject: &FlowBindingRef,
        values: impl IntoIterator<Item = (FlowDomain, Option<FlowProductValue>)>,
    ) -> bool {
        let mut frame = self.execution.borrow_mut();
        if frame.failure.is_some() {
            return false;
        }
        let result: Result<bool, FlowProductFailure> = (|| {
            let transfers = values.into_iter().map(|(domain, value)| {
                let key = frame.content.key_for_binding(domain, subject)?;
                Ok(FlowProductTransfer { key, value })
            }).collect::<Result<smallvec::SmallVec<[FlowProductTransfer; 4]>, FlowProductFailure>>()?;
            let Some(first) = transfers.first() else {
                return Ok(false);
            };
            let site = first.key.site();
            frame
                .execution
                .apply_transfers(&mut self.state, &site, &transfers)
        })();
        if let Err(failure) = result {
            frame.failure = Some(failure);
            return false;
        }
        true
    }

    /// Import the exact prepared child input. Its selected captured hub is the
    /// definition site; the enclosing initializer and assignment state are not
    /// fabricated by annotation hydration.
    pub fn import_capture_input(
        &mut self,
        subject: &FlowBindingRef,
        assignment: DefiniteAssignmentProduct,
        reaching: Option<ReachingTypeProduct>,
        declared: Option<SemanticNodeId>,
    ) {
        self.set_declared_type(subject, declared);
        if let Some(reaching) = reaching {
            self.bind(subject, assignment, reaching);
        } else {
            self.set_assignment(subject, assignment);
        }
    }

    /// Establish the assignment and reaching value and invalidate the replaced
    /// value's guard facts as one executed, transactional binding operation.
    pub fn bind(
        &mut self,
        subject: &FlowBindingRef,
        assignment: DefiniteAssignmentProduct,
        reaching: ReachingTypeProduct,
    ) {
        let definition = self
            .key(FlowDomain::ReachingValue, subject)
            .map(|key| key.node());
        self.bind_at(subject, assignment, reaching, definition);
    }

    pub fn bind_at(
        &mut self,
        subject: &FlowBindingRef,
        assignment: DefiniteAssignmentProduct,
        reaching: ReachingTypeProduct,
        definition: Option<FlowNodeId>,
    ) {
        let Some((write_subject, sequence)) = self.prepare_write(subject) else {
            return;
        };
        let reaching_value = {
            let mut frame = self.execution.borrow_mut();
            let site = definition
                .ok_or(super::flow_products::FlowProductKeyError::UnselectedNode)
                .and_then(|definition| frame.content.site(definition));
            match site {
                Ok(site) => ReachingValueProduct::at(&site),
                Err(failure) => {
                    frame.failure = Some(failure.into());
                    return;
                }
            }
        };
        if self.set_values(
            subject,
            [
                (
                    FlowDomain::DefiniteAssignment,
                    Some(FlowProductValue::DefiniteAssignment(assignment)),
                ),
                (
                    FlowDomain::ReachingType,
                    Some(FlowProductValue::ReachingType(reaching)),
                ),
                (FlowDomain::Narrowing, None),
                (
                    FlowDomain::ReachingValue,
                    Some(FlowProductValue::ReachingValue(reaching_value)),
                ),
            ],
        ) {
            self.execution.borrow_mut().write_sequence = sequence;
            self.install_write(
                write_subject,
                FlowWriteReceipt {
                    sequence,
                    subject: subject.clone(),
                },
            );
        }
    }

    pub fn write_subject(&self, subject: &FlowBindingRef) -> Option<FlowWriteSubject> {
        let identity = self.identity(subject)?;
        Some(FlowWriteSubject(
            identity.defining_function,
            identity.binding_slot,
        ))
    }

    fn prepare_write(&self, subject: &FlowBindingRef) -> Option<(FlowWriteSubject, u64)> {
        let mut frame = self.execution.borrow_mut();
        let identity = match frame
            .content
            .key_for_binding(FlowDomain::ReachingValue, subject)
        {
            Ok(key) => key.runtime_binding().cloned(),
            Err(failure) => {
                frame.failure = Some(failure.into());
                return None;
            }
        };
        let Some(identity) = identity else {
            frame.failure =
                Some(super::flow_products::FlowProductKeyError::UnmodeledBinding.into());
            return None;
        };
        let key = FlowWriteSubject(identity.defining_function, identity.binding_slot);
        let Some(sequence) = frame.write_sequence.checked_add(1) else {
            frame.failure = Some(FlowProductFailure::Gap(
                verter_session_query::flow::policy::FlowGap::UnmodeledExpression,
            ));
            return None;
        };
        Some((key, sequence))
    }

    fn install_write(&mut self, subject: FlowWriteSubject, receipt: FlowWriteReceipt) {
        if let Some(previous) = self.writes.insert(subject, receipt.clone()) {
            self.writes_by_sequence
                .remove(&FlowWriteSequence(previous.sequence));
        }
        self.writes_by_sequence
            .insert(FlowWriteSequence(receipt.sequence), receipt.subject);
    }

    fn remove_write(&mut self, subject: &FlowWriteSubject) {
        if let Some(previous) = self.writes.remove(subject) {
            self.writes_by_sequence
                .remove(&FlowWriteSequence(previous.sequence));
        }
    }

    /// Whether `other` is this very continuation: the same execution, the
    /// same product values and the same write receipts, so every later
    /// transfer and join reads it exactly as it reads this one.
    pub fn same_as(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.execution, &other.execution)
            && self.writes_by_sequence == other.writes_by_sequence
            && self.writes == other.writes
            && self.state.same_as(&other.state)
    }

    pub fn observe_writes(&self) -> FlowWriteObservation {
        FlowWriteObservation {
            execution: Rc::clone(&self.execution),
            sequence: self.execution.borrow().write_sequence,
        }
    }

    fn accepts_observation(&self, observation: &FlowWriteObservation) -> bool {
        if Rc::ptr_eq(&self.execution, &observation.execution) {
            return true;
        }
        self.execution.borrow_mut().failure = Some(FlowProductFailure::ScopeMismatch);
        false
    }

    pub fn writes_since(&self, observation: &FlowWriteObservation) -> Vec<FlowBindingRef> {
        if !self.accepts_observation(observation) {
            return Vec::new();
        }
        // Seek past the shared execution's explicit control-entry sequence.
        // Only the latest receipt per selected subject is retained.
        self.writes_by_sequence
            .range((
                std::ops::Bound::Excluded(FlowWriteSequence(observation.sequence)),
                std::ops::Bound::Unbounded,
            ))
            .map(|(_, subject)| subject.clone())
            .collect()
    }

    pub fn join(
        incoming: &[&Self],
        observation: &FlowWriteObservation,
        algebra: &dyn FlowSemanticAlgebra,
    ) -> Self {
        let first = incoming.first().expect("a continuation has a predecessor");
        let mut joined = (*first).clone();
        if !first.accepts_observation(observation) || incoming.len() == 1 {
            return joined;
        }
        let mut frame = first.execution.borrow_mut();
        if frame.failure.is_some() {
            return joined;
        }
        let states: smallvec::SmallVec<[&FlowProductStore; 4]> =
            incoming.iter().map(|products| &products.state).collect();
        match frame.execution.join_products(&states, algebra) {
            Ok(state) => {
                joined.state = state;
                for predecessor in &incoming[1..] {
                    for (sequence, binding) in predecessor.writes_by_sequence.range((
                        std::ops::Bound::Excluded(FlowWriteSequence(observation.sequence)),
                        std::ops::Bound::Unbounded,
                    )) {
                        let identity = frame
                            .content
                            .key_for_binding(FlowDomain::ReachingValue, binding)
                            .ok()
                            .and_then(|key| key.runtime_binding().cloned())
                            .expect("successful receipt retains its selected runtime identity");
                        let subject =
                            FlowWriteSubject(identity.defining_function, identity.binding_slot);
                        if joined
                            .writes
                            .get(&subject)
                            .is_none_or(|earlier| sequence.0 > earlier.sequence)
                        {
                            joined.install_write(
                                subject,
                                FlowWriteReceipt {
                                    sequence: sequence.0,
                                    subject: binding.clone(),
                                },
                            );
                        }
                    }
                }
            }
            Err(failure) => frame.failure = Some(failure),
        }
        joined
    }

    pub fn remove(&mut self, domain: FlowDomain, subject: &FlowBindingRef) {
        if self.set_values(subject, [(domain, None)]) && domain == FlowDomain::ReachingValue {
            if let Some(identity) = self.write_subject(subject) {
                self.remove_write(&identity);
            }
        }
    }

    /// Rewind a continuation's runtime products as one replacement. Authored
    /// declaration authority lives independently of these snapshots.
    pub fn restore_reaching_from(
        &mut self,
        subject: &FlowBindingRef,
        source: &Self,
        assignment: Option<DefiniteAssignmentProduct>,
        restore_narrowing: bool,
    ) {
        let mut values = smallvec::SmallVec::<[(FlowDomain, Option<FlowProductValue>); 4]>::new();
        for domain in [
            FlowDomain::ReachingValue,
            FlowDomain::ReachingType,
            FlowDomain::DefiniteAssignment,
        ] {
            let value = if domain == FlowDomain::DefiniteAssignment {
                assignment
                    .map(FlowProductValue::DefiniteAssignment)
                    .or_else(|| source.get(domain, subject).cloned())
            } else {
                source.get(domain, subject).cloned()
            };
            values.push((domain, value));
        }
        if restore_narrowing {
            values.push((
                FlowDomain::Narrowing,
                source.get(FlowDomain::Narrowing, subject).cloned(),
            ));
        }
        if self.set_values(subject, values) {
            if let Some(identity) = self.write_subject(subject) {
                if let Some(sequence) = source.writes.get(&identity) {
                    self.install_write(identity, sequence.clone());
                } else {
                    self.remove_write(&identity);
                }
            }
        }
    }

    /// [`Self::restore_reaching_from`] of a loop head's value: the
    /// reference's values only, never a write receipt. A loop head is a
    /// value the analysis settled, not a write on the path into the loop:
    /// the passes that settled it are discarded, and the pass run from the
    /// head records its own writes.
    pub fn restore_head_value_from(&mut self, subject: &FlowBindingRef, source: &Self) {
        let values: smallvec::SmallVec<[(FlowDomain, Option<FlowProductValue>); 4]> = [
            FlowDomain::ReachingValue,
            FlowDomain::ReachingType,
            FlowDomain::DefiniteAssignment,
            FlowDomain::Narrowing,
        ]
        .into_iter()
        .map(|domain| (domain, source.get(domain, subject).cloned()))
        .collect();
        self.set_values(subject, values);
    }

    /// Replay a successfully executed clause write onto its normal continuation.
    /// All runtime domains move together, and the old value's guards are killed.
    pub fn apply_executed_write_from(&mut self, subject: &FlowBindingRef, source: &Self) {
        let values: smallvec::SmallVec<[(FlowDomain, Option<FlowProductValue>); 4]> = [
            FlowDomain::ReachingValue,
            FlowDomain::ReachingType,
            FlowDomain::DefiniteAssignment,
        ]
        .into_iter()
        .map(|domain| (domain, source.get(domain, subject).cloned()))
        .chain(std::iter::once((FlowDomain::Narrowing, None)))
        .collect();
        if self.set_values(subject, values) {
            if let Some(identity) = self.write_subject(subject) {
                if let Some(sequence) = source.writes.get(&identity) {
                    self.install_write(identity, sequence.clone());
                }
            }
        }
    }

    pub fn reaching_type(&self, subject: &FlowBindingRef) -> Option<&ReachingTypeProduct> {
        match self.get(FlowDomain::ReachingType, subject)? {
            FlowProductValue::ReachingType(value) => Some(value),
            _ => None,
        }
    }

    pub fn reaching(&self, subject: &FlowBindingRef) -> Option<SemanticNodeId> {
        self.reaching_type(subject)?.united()
    }

    pub fn set_reaching_type(&mut self, subject: &FlowBindingRef, value: ReachingTypeProduct) {
        self.set(
            FlowDomain::ReachingType,
            subject,
            Some(FlowProductValue::ReachingType(value)),
        );
    }

    pub fn widening(&self, subject: &FlowBindingRef) -> Option<&WideningMembership> {
        self.reaching_type(subject)?.widening()
    }

    pub fn declared_type(&self, subject: &FlowBindingRef) -> Option<SemanticNodeId> {
        match self.get(FlowDomain::DeclaredType, subject)? {
            FlowProductValue::DeclaredType(value) => value.declared(),
            _ => None,
        }
    }

    /// Runtime reads use the kernel's installed source-order authority index.
    /// The exact-only accessor above remains the source hydration boundary.
    pub fn runtime_declared_type(&self, subject: &FlowBindingRef) -> Option<SemanticNodeId> {
        let key = self.key(FlowDomain::DeclaredType, subject)?;
        self.state.declared_type(&key)
    }

    pub fn set_declared_type(&mut self, subject: &FlowBindingRef, value: Option<SemanticNodeId>) {
        let Some(value) = value else {
            return;
        };
        self.set(
            FlowDomain::DeclaredType,
            subject,
            Some(FlowProductValue::DeclaredType(DeclaredTypeProduct::of(
                value,
            ))),
        );
    }

    pub fn assignment(&self, subject: &FlowBindingRef) -> DefiniteAssignmentProduct {
        match self.get(FlowDomain::DefiniteAssignment, subject) {
            Some(FlowProductValue::DefiniteAssignment(value)) => *value,
            _ => DefiniteAssignmentProduct::default(),
        }
    }

    pub fn set_assignment(&mut self, subject: &FlowBindingRef, value: DefiniteAssignmentProduct) {
        self.set(
            FlowDomain::DefiniteAssignment,
            subject,
            Some(FlowProductValue::DefiniteAssignment(value)),
        );
    }

    pub fn narrowing(&self, subject: &FlowBindingRef) -> Option<&NarrowingProduct> {
        match self.get(FlowDomain::Narrowing, subject)? {
            FlowProductValue::Narrowing(value) => Some(value),
            _ => None,
        }
    }

    pub fn set_narrowing(&mut self, subject: &FlowBindingRef, value: NarrowingProduct) {
        self.set(
            FlowDomain::Narrowing,
            subject,
            (!value.facts().is_empty()).then_some(FlowProductValue::Narrowing(value)),
        );
    }

    pub fn subjects_in(&self, domain: FlowDomain) -> Vec<FlowBindingRef> {
        self.state
            .ordered_entries()
            .filter_map(|(key, _)| {
                if key.domain() != domain {
                    return None;
                }
                key.binding_ref().cloned()
            })
            .collect()
    }

    pub fn subjects(&self) -> Vec<FlowBindingRef> {
        let mut seen = rustc_hash::FxHashSet::default();
        self.state
            .ordered_entries()
            .filter_map(|(key, _)| {
                let subject = key.binding_ref()?.clone();
                seen.insert(subject.clone()).then_some(subject)
            })
            .collect()
    }
}
