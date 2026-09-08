//! Selected, scoped flow products and their exhaustive transfer/join algebra.
//!
//! The control interpreter supplies executed transfers and actual continuation
//! snapshots. The function graph and sealed demand own selection; dependence
//! edges never stand in for control-flow predecessors. All mutable product
//! state lives here. Semantic composites use the canonical type algebra.
//! Execution capabilities and snapshots are thread-local: a control interpreter
//! cannot share partially published source authority with another worker.
//! Immutable graph/selection artifacts remain shared across independent executions.

#![cfg_attr(not(test), allow(dead_code))]

use super::flow_solve::{FlowDemandBasis, FlowDemandPlan, FlowDomain, FlowExecutionSelection};
use crate::cache_runtime::flow_slice_node::{BoundFlowGraph, FlowSliceFunctionKey};
use crate::semantic_query::{FlowGap, SemanticNodeId};
use rustc_hash::FxHashMap;
use std::cell::{Cell, OnceCell, RefCell};
use std::hash::{Hash, Hasher};
use std::rc::Rc;
use std::sync::Arc;
use verter_semantic::analysis::flow::flow_graph::{FlowNodeId, FlowNodeKind, FunctionFlowGraph};
use verter_semantic::analysis::flow::flow_ir::ReturnSlicePlan;
use verter_semantic::analysis::flow::{FlowBindingMap, FlowBindingRef};
use verter_semantic::analysis::function_program::{FlowBindingIdentity, FunctionProgramKey};

// Compact offsets are private runtime addresses, never persisted identities.
const PRODUCT_DOMAINS: [FlowDomain; 5] = [
    FlowDomain::ReachingValue,
    FlowDomain::ReachingType,
    FlowDomain::Narrowing,
    FlowDomain::DeclaredType,
    FlowDomain::DefiniteAssignment,
];

fn product_offset(domain: FlowDomain) -> Option<usize> {
    match domain {
        FlowDomain::ReachingValue => Some(0),
        FlowDomain::ReachingType => Some(1),
        FlowDomain::Narrowing => Some(2),
        FlowDomain::DeclaredType => Some(3),
        FlowDomain::DefiniteAssignment => Some(4),
        FlowDomain::Completion
        | FlowDomain::ClosureCapture
        | FlowDomain::Freshness
        | FlowDomain::Effects
        | FlowDomain::CallResolution
        | FlowDomain::Relation
        | FlowDomain::ContextualTyping
        | FlowDomain::Coverage => None,
    }
}

/// Why an execution or selected key cannot be constructed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlowProductKeyError {
    GraphMismatch,
    UnselectedNode,
    UnmodeledBinding,
    DomainCarriesNoProduct,
    InvalidCapture,
    SelectionBudget,
}

/// Immutable structural and binding inputs supplied by shared owners.
#[derive(Debug, Clone)]
pub struct FlowProductInputs {
    graph: Arc<FunctionFlowGraph>,
    scope: FlowSliceFunctionKey,
    bindings: Arc<FlowBindingMap>,
}

impl FlowProductInputs {
    pub(crate) fn for_bound_graph(bound: &BoundFlowGraph) -> Self {
        Self {
            graph: Arc::clone(&bound.bundle().graph),
            scope: bound.key().clone(),
            bindings: Arc::clone(&bound.bundle().bindings),
        }
    }

    /// Graph-owned captured subjects in stable selected-node order. This can
    /// encode selected inputs before the execution basis is sealed, without
    /// rebuilding capture inventory or minting caller-supplied reference sites.
    /// The flag distinguishes value demand from effect-only capture dependencies.
    pub(crate) fn selected_captures<'a>(
        &'a self,
        selection: &'a ReturnSlicePlan,
    ) -> impl Iterator<Item = (&'a FlowBindingIdentity, bool)> + 'a {
        selected_nodes(selection).filter_map(|(node, value_demanded)| {
            match self.graph.node_kind(node) {
                FlowNodeKind::CapturedBinding(binding) => {
                    Some((self.graph.captured_binding(binding), value_demanded))
                }
                FlowNodeKind::Binding(_)
                | FlowNodeKind::ExprSite(_)
                | FlowNodeKind::ReturnSite(_)
                | FlowNodeKind::Region(_) => None,
            }
        })
    }

    #[must_use]
    pub fn graph(&self) -> &FunctionFlowGraph {
        &self.graph
    }
}

/// Merge the owner's sorted disjoint selected arrays without allocation.
fn selected_nodes(selection: &ReturnSlicePlan) -> impl Iterator<Item = (FlowNodeId, bool)> + '_ {
    let mut values = selection.value_nodes().iter().peekable();
    let mut effects = selection.effect_only_nodes().iter().peekable();
    std::iter::from_fn(move || {
        let values_first = match (values.peek(), effects.peek()) {
            (Some(a), Some(b)) => a.index() < b.index(),
            (Some(_), None) => true,
            (None, _) => false,
        };
        (if values_first {
            values.next()
        } else {
            effects.next()
        })
        .copied()
        .map(|node| (node, values_first))
    })
}

#[derive(Debug)]
struct ProductSubject {
    node: FlowNodeId,
    binding: Option<FlowBindingIdentity>,
    binding_ref: Option<FlowBindingRef>,
    storage: usize,
}

/// A layout is also an unforgeable execution capability. Pointer equality
/// permits constant-time scope checks; graph/content and full demand basis
/// are validated once, before it can be minted.
#[derive(Debug)]
struct ProductLayout {
    inputs: FlowProductInputs,
    selection: Arc<FlowExecutionSelection>,
    subjects: Box<[ProductSubject]>,
    representatives: Box<[usize]>,
    captures: FxHashMap<FlowBindingIdentity, usize>,
    // Authored facts have execution lifetime, independent of branch snapshots.
    declarations: Box<[OnceCell<Box<FlowProductValue>>]>,
    declared_active: RefCell<imbl::OrdSet<usize>>,
    declared_by_runtime: Box<[Cell<Option<usize>>]>,
}

impl ProductLayout {
    fn subject_at(&self, node: FlowNodeId) -> Result<usize, FlowProductKeyError> {
        self.subjects
            .binary_search_by_key(&node.index(), |subject| subject.node.index())
            .map_err(|_| FlowProductKeyError::UnselectedNode)
    }
    fn key(
        self: &Rc<Self>,
        domain: FlowDomain,
        subject: usize,
    ) -> Result<FlowProductKey, FlowProductKeyError> {
        let offset = product_offset(domain).ok_or(FlowProductKeyError::DomainCarriesNoProduct)?;
        Ok(FlowProductKey {
            scope: Rc::clone(self),
            subject,
            offset,
        })
    }
}

/// A selected domain/subject address, bound to one execution.
#[derive(Debug, Clone)]
pub struct FlowProductKey {
    scope: Rc<ProductLayout>,
    subject: usize,
    offset: usize,
}

impl PartialEq for FlowProductKey {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.scope, &other.scope)
            && self.subject == other.subject
            && self.offset == other.offset
    }
}
impl Eq for FlowProductKey {}
impl Hash for FlowProductKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        Rc::as_ptr(&self.scope).hash(state);
        self.subject.hash(state);
        self.offset.hash(state);
    }
}

impl FlowProductKey {
    #[must_use]
    pub fn domain(&self) -> FlowDomain {
        PRODUCT_DOMAINS[self.offset]
    }
    #[must_use]
    pub fn node(&self) -> FlowNodeId {
        self.scope.subjects[self.subject].node
    }
    #[must_use]
    pub fn binding(&self) -> Option<&FlowBindingIdentity> {
        self.scope.subjects[self.subject].binding.as_ref()
    }
    #[must_use]
    pub fn binding_ref(&self) -> Option<&FlowBindingRef> {
        self.scope.subjects[self.subject].binding_ref.as_ref()
    }
    /// Canonical runtime variable identity; declaration evidence still uses binding().
    #[must_use]
    pub fn runtime_binding(&self) -> Option<&FlowBindingIdentity> {
        match self.binding_ref()? {
            FlowBindingRef::Local(binding) => self.scope.inputs.bindings.runtime_identity(*binding),
            FlowBindingRef::Captured(identity) => Some(identity),
        }
    }
    fn storage(&self) -> (usize, usize) {
        (
            self.offset,
            if self.domain() == FlowDomain::DeclaredType {
                self.subject
            } else {
                self.scope.subjects[self.subject].storage
            },
        )
    }
    fn evidence(&self) -> usize {
        self.subject * PRODUCT_DOMAINS.len() + self.offset
    }
}

/// A real selected site. The control interpreter decides when it executes.
#[derive(Debug, Clone)]
pub struct SelectedFlowSite {
    scope: Rc<ProductLayout>,
    subject: usize,
}

/// Persistent continuation state. Cloning shares the ordered tree; writes
/// copy only a tree path. Empty selected capacity never enters snapshots.
/// Source-declaration facts live in the shared append-only authority bank.
#[derive(Debug, Clone)]
pub struct FlowProductStore {
    scope: Rc<ProductLayout>,
    values: imbl::OrdMap<(usize, usize), FlowProductValue>,
}

// Immutable structural artifacts can cross workers; mutable execution and its
// publication-observing handles cannot. This is an architectural contract in
// every profile, not a convention left to the current caller.
static_assertions::assert_impl_all!(FlowProductInputs: Send, Sync);
static_assertions::assert_not_impl_any!(FlowProductStore: Send, Sync);
static_assertions::assert_not_impl_any!(FlowProductExecution: Send, Sync);
static_assertions::assert_not_impl_any!(FlowProductKey: Send, Sync);
static_assertions::assert_not_impl_any!(SelectedFlowSite: Send, Sync);

impl FlowProductStore {
    #[must_use]
    pub fn get(&self, key: &FlowProductKey) -> Option<&FlowProductValue> {
        if !Rc::ptr_eq(&self.scope, &key.scope) {
            return None;
        }
        self.get_at(key.storage())
    }
    /// Exact authored authority wins. Otherwise use the first source-ordered
    /// installed declaration in the same runtime group, without copying its fact.
    #[must_use]
    pub fn declared_type(&self, key: &FlowProductKey) -> Option<SemanticNodeId> {
        if !Rc::ptr_eq(&self.scope, &key.scope) || key.domain() != FlowDomain::DeclaredType {
            return None;
        }
        let exact = self.scope.declarations[key.subject].get();
        let value = exact.or_else(|| {
            let storage = self.scope.subjects[key.subject].storage;
            let authority = self.scope.declared_by_runtime[storage].get()?;
            self.scope.declarations[authority].get()
        })?;
        let FlowProductValue::DeclaredType(product) = value.as_ref() else {
            return None;
        };
        product.declared()
    }
    fn get_at(&self, address: (usize, usize)) -> Option<&FlowProductValue> {
        if address.0 == product_offset(FlowDomain::DeclaredType).unwrap() {
            self.scope.declarations[address.1].get().map(Box::as_ref)
        } else {
            self.values.get(&address)
        }
    }
    #[must_use]
    pub fn len(&self) -> usize {
        self.values.len() + self.scope.declared_active.borrow().len()
    }
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.values.is_empty() && self.scope.declared_active.borrow().is_empty()
    }
    /// Visits only materialized cells, in domain then selected-subject order.
    pub fn ordered_entries(&self) -> impl Iterator<Item = (FlowProductKey, &FlowProductValue)> {
        let declarations = self.scope.declared_active.borrow().clone();
        let runtime = move |(&(offset, storage), value)| {
            (
                self.scope
                    .key(PRODUCT_DOMAINS[offset], self.scope.representatives[storage])
                    .expect("a product domain"),
                value,
            )
        };
        self.values
            .range(..(3, 0))
            .map(runtime)
            .chain(declarations.into_iter().map(move |subject| {
                (
                    self.scope
                        .key(FlowDomain::DeclaredType, subject)
                        .expect("a product domain"),
                    self.scope.declarations[subject]
                        .get()
                        .expect("published authored fact")
                        .as_ref(),
                )
            }))
            .chain(self.values.range((4, 0)..).map(runtime))
    }
    /// Structural sharing is observable only to the hermetic performance proof.
    #[cfg(any(test, feature = "test-support"))]
    pub fn shares_continuation_storage(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.scope, &other.scope) && self.values.ptr_eq(&other.values)
    }
}

/// Scoped attachment held only by the content interpreter after matching its
/// retained structural owner. Raw local node/binding ids never cross the public
/// product API; returned keys and definition sites retain this execution scope.
#[derive(Debug, Clone)]
pub(crate) struct FlowProductContent {
    scope: Rc<ProductLayout>,
}

impl FlowProductContent {
    pub(crate) fn site(&self, node: FlowNodeId) -> Result<SelectedFlowSite, FlowProductKeyError> {
        Ok(SelectedFlowSite {
            scope: Rc::clone(&self.scope),
            subject: self.scope.subject_at(node)?,
        })
    }
    pub(crate) fn key(
        &self,
        domain: FlowDomain,
        node: FlowNodeId,
    ) -> Result<FlowProductKey, FlowProductKeyError> {
        self.scope.key(domain, self.scope.subject_at(node)?)
    }
    pub(crate) fn key_for_binding(
        &self,
        domain: FlowDomain,
        binding: &FlowBindingRef,
    ) -> Result<FlowProductKey, FlowProductKeyError> {
        let subject = match binding {
            FlowBindingRef::Local(local) => {
                if self.scope.inputs.bindings.identity(*local).is_none() {
                    return Err(FlowProductKeyError::UnmodeledBinding);
                }
                self.scope
                    .subject_at(self.scope.inputs.graph.binding_node(*local))?
            }
            FlowBindingRef::Captured(identity) => *self
                .scope
                .captures
                .get(identity)
                .ok_or(FlowProductKeyError::UnmodeledBinding)?,
        };
        self.scope.key(domain, subject)
    }
}

impl FlowProductKey {
    pub fn site(&self) -> SelectedFlowSite {
        SelectedFlowSite {
            scope: Rc::clone(&self.scope),
            subject: self.subject,
        }
    }
    pub fn for_domain(&self, domain: FlowDomain) -> Result<Self, FlowProductKeyError> {
        self.scope.key(domain, self.subject)
    }
}
impl SelectedFlowSite {
    pub fn node(&self) -> FlowNodeId {
        self.scope.subjects[self.subject].node
    }
    pub fn key(&self, domain: FlowDomain) -> Result<FlowProductKey, FlowProductKeyError> {
        self.scope.key(domain, self.subject)
    }
}

/// Successful value execution, with no obligation-discharge capability.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FlowProductStatus {
    iterations: u32,
}
impl FlowProductStatus {
    pub fn iterations(&self) -> u32 {
        self.iterations
    }
}

/// Explicit execution intent. Clear and replacement both pass through the
/// same domain validation. A write submits its value and narrowing kill in
/// one bundle; guards submit their own facts without masquerading as writes.
#[derive(Debug, Clone)]
pub struct FlowProductTransfer {
    pub key: FlowProductKey,
    pub value: Option<FlowProductValue>,
}

/// A failed execution can expose neither completion evidence nor a final store.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FlowProductFailure {
    ScopeMismatch,
    Sealed,
    Gap(FlowGap),
    BudgetExceeded(FlowProductBudgetExceeded),
}

impl From<FlowProductKeyError> for FlowProductFailure {
    fn from(error: FlowProductKeyError) -> Self {
        match error {
            FlowProductKeyError::GraphMismatch => Self::ScopeMismatch,
            FlowProductKeyError::UnselectedNode
            | FlowProductKeyError::UnmodeledBinding
            | FlowProductKeyError::DomainCarriesNoProduct
            | FlowProductKeyError::InvalidCapture
            | FlowProductKeyError::SelectionBudget => Self::Gap(FlowGap::UnmodeledExpression),
        }
    }
}

/// Successfully executed domains/subjects, including unchanged operations.
#[derive(Debug, Clone)]
pub struct FlowProductEvidence {
    scope: Rc<ProductLayout>,
    executed: Arc<[bool]>,
    iterations: u32,
}

impl FlowProductEvidence {
    #[must_use]
    pub fn executed(&self, key: &FlowProductKey) -> bool {
        Rc::ptr_eq(&self.scope, &key.scope) && self.executed[key.evidence()]
    }
    #[must_use]
    pub fn iterations(&self) -> u32 {
        self.iterations
    }
    #[must_use]
    pub fn basis(&self) -> &FlowDemandBasis {
        self.scope.selection.basis()
    }
}

/// Shared execution controller. Snapshots share its immutable layout; only
/// actual transfers and predecessor joins can change stored products.
#[derive(Debug)]
pub struct FlowProductExecution {
    scope: Rc<ProductLayout>,
    budget: FlowProductBudget,
    executed: Vec<bool>,
    iterations: u32,
    failure: Option<FlowProductFailure>,
    sealed: bool,
    #[cfg(any(test, feature = "test-support"))]
    join_product_visits: usize,
}

impl FlowProductExecution {
    pub fn new(
        inputs: &FlowProductInputs,
        plan: &FlowDemandPlan,
        budget: FlowProductBudget,
    ) -> Result<Self, FlowProductKeyError> {
        Self::new_for_selection(inputs, Arc::clone(plan.execution_selection()), budget)
    }

    pub fn new_for_selection(
        inputs: &FlowProductInputs,
        selection: Arc<FlowExecutionSelection>,
        mut budget: FlowProductBudget,
    ) -> Result<Self, FlowProductKeyError> {
        let plan = &selection;
        if inputs.scope != plan.basis().graph_body {
            return Err(FlowProductKeyError::GraphMismatch);
        }
        budget.max_iterations = budget.max_iterations.min(plan.convergence().max_iterations);
        let structural = plan.structural_selection();
        if structural
            .value_nodes()
            .len()
            .saturating_add(structural.effect_only_nodes().len())
            > plan.resources().slice_budget.max_selected_nodes as usize
        {
            return Err(FlowProductKeyError::SelectionBudget);
        }
        let mut subjects = Vec::with_capacity(
            structural.value_nodes().len() + structural.effect_only_nodes().len(),
        );
        let mut representatives = Vec::new();
        let mut local_storage = FxHashMap::default();
        let mut captures = FxHashMap::default();
        for (node, _) in selected_nodes(structural) {
            let subject = subjects.len();
            let (binding, binding_ref, existing_storage) = match inputs.graph.node_kind(node) {
                FlowNodeKind::Binding(binding) => {
                    let map = &inputs.bindings;
                    let identity = map
                        .identity(binding)
                        .ok_or(FlowProductKeyError::UnmodeledBinding)?
                        .clone();
                    let canonical = map.canonical_local(binding);
                    let storage = *local_storage.entry(canonical).or_insert_with(|| {
                        let storage = representatives.len();
                        representatives.push(subject);
                        storage
                    });
                    (
                        Some(identity),
                        Some(FlowBindingRef::Local(binding)),
                        Some(storage),
                    )
                }
                FlowNodeKind::CapturedBinding(binding) => {
                    let identity = inputs.graph.captured_binding(binding).clone();
                    if identity.defining_function == inputs.scope.function {
                        return Err(FlowProductKeyError::InvalidCapture);
                    }
                    captures.insert(identity.clone(), subject);
                    (
                        Some(identity.clone()),
                        Some(FlowBindingRef::Captured(identity)),
                        None,
                    )
                }
                FlowNodeKind::ExprSite(_)
                | FlowNodeKind::ReturnSite(_)
                | FlowNodeKind::Region(_) => (None, None, None),
            };
            let storage = existing_storage.unwrap_or_else(|| {
                let storage = representatives.len();
                representatives.push(subject);
                storage
            });
            subjects.push(ProductSubject {
                node,
                binding,
                binding_ref,
                storage,
            });
        }
        budget.max_products = budget.max_products.min(
            u32::try_from(representatives.len())
                .unwrap_or(u32::MAX)
                .saturating_mul((PRODUCT_DOMAINS.len() - 1) as u32),
        );
        budget.max_declared_products = budget
            .max_declared_products
            .min(u32::try_from(subjects.len()).unwrap_or(u32::MAX));
        let declarations = (0..subjects.len()).map(|_| OnceCell::new()).collect();
        let declared_by_runtime = (0..representatives.len())
            .map(|_| Cell::new(None))
            .collect();
        let executed = vec![false; subjects.len() * PRODUCT_DOMAINS.len()];
        Ok(Self {
            scope: Rc::new(ProductLayout {
                inputs: inputs.clone(),
                selection,
                subjects: subjects.into(),
                representatives: representatives.into(),
                captures,
                declarations,
                declared_active: RefCell::new(imbl::OrdSet::new()),
                declared_by_runtime,
            }),
            budget,
            executed,
            iterations: 0,
            failure: None,
            sealed: false,
            #[cfg(any(test, feature = "test-support"))]
            join_product_visits: 0,
        })
    }

    #[must_use]
    pub fn empty_state(&self) -> FlowProductStore {
        FlowProductStore {
            scope: Rc::clone(&self.scope),
            values: imbl::OrdMap::new(),
        }
    }
    #[must_use]
    pub fn selected_subject_count(&self) -> usize {
        self.scope.subjects.len()
    }
    /// Numeric graph addresses enter only through the pinned content interpreter.
    /// Both the retained content key and graph allocation must match this execution.
    pub(crate) fn attach_content(
        &self,
        bound: &BoundFlowGraph,
    ) -> Result<FlowProductContent, FlowProductKeyError> {
        if bound.key() != &self.scope.inputs.scope
            || !Arc::ptr_eq(&bound.bundle().graph, &self.scope.inputs.graph)
        {
            return Err(FlowProductKeyError::GraphMismatch);
        }
        Ok(FlowProductContent {
            scope: Rc::clone(&self.scope),
        })
    }
    /// Enumerate already scoped selected sites; callers cannot supply a naked id
    /// and silently relabel a site belonging to another execution or graph.
    pub fn selected_sites(&self) -> impl ExactSizeIterator<Item = SelectedFlowSite> + '_ {
        (0..self.scope.subjects.len()).map(|subject| SelectedFlowSite {
            scope: Rc::clone(&self.scope),
            subject,
        })
    }
    pub fn key_at_site(
        &self,
        domain: FlowDomain,
        site: &SelectedFlowSite,
    ) -> Result<FlowProductKey, FlowProductKeyError> {
        if !Rc::ptr_eq(&self.scope, &site.scope) {
            return Err(FlowProductKeyError::GraphMismatch);
        }
        self.scope.key(domain, site.subject)
    }

    fn ready(&self) -> Result<(), FlowProductFailure> {
        if let Some(failure) = &self.failure {
            return Err(failure.clone());
        }
        if self.sealed {
            return Err(FlowProductFailure::Sealed);
        }
        Ok(())
    }
    fn reject<T>(&mut self, failure: FlowProductFailure) -> Result<T, FlowProductFailure> {
        self.failure = Some(failure.clone());
        Err(failure)
    }

    /// Validate the complete bundle, then publish it atomically. Neither
    /// state nor successful-work evidence changes if any member fails.
    pub fn apply_transfers(
        &mut self,
        state: &mut FlowProductStore,
        site: &SelectedFlowSite,
        transfers: &[FlowProductTransfer],
    ) -> Result<bool, FlowProductFailure> {
        self.ready()?;
        if !Rc::ptr_eq(&self.scope, &state.scope) || !Rc::ptr_eq(&self.scope, &site.scope) {
            return self.reject(FlowProductFailure::ScopeMismatch);
        }
        type StagedWrite = ((usize, usize), Option<FlowProductValue>);
        let mut staged: smallvec::SmallVec<[StagedWrite; 5]> = smallvec::SmallVec::new();
        let mut active = state.values.len();
        let mut declared = self.scope.declared_active.borrow().len();
        for transfer in transfers {
            if !Rc::ptr_eq(&self.scope, &transfer.key.scope) {
                return self.reject(FlowProductFailure::ScopeMismatch);
            }
            let slot = transfer.key.storage();
            let prior = staged
                .iter()
                .find(|(held, _)| *held == slot)
                .map(|(_, value)| value.as_ref())
                .unwrap_or_else(|| state.get_at(slot));
            let value =
                match transfer_product(&self.budget, &transfer.key, prior, transfer.value.as_ref())
                {
                    Ok(value) => value,
                    Err(failure) => return self.reject(failure),
                };
            let count = if transfer.key.domain() == FlowDomain::DeclaredType {
                &mut declared
            } else {
                &mut active
            };
            *count = *count - usize::from(prior.is_some()) + usize::from(value.is_some());
            if let Some((_, held)) = staged.iter_mut().find(|(held, _)| *held == slot) {
                *held = value;
            } else {
                staged.push((slot, value));
            }
        }
        if active > self.budget.max_products as usize {
            return self.reject(FlowProductFailure::BudgetExceeded(
                FlowProductBudgetExceeded {
                    axis: FlowProductBudgetAxis::Products,
                    limit: self.budget.max_products,
                    observed: u32::try_from(active).unwrap_or(u32::MAX),
                },
            ));
        }
        if declared > self.budget.max_declared_products as usize {
            return self.reject(FlowProductFailure::BudgetExceeded(
                FlowProductBudgetExceeded {
                    axis: FlowProductBudgetAxis::DeclaredProducts,
                    limit: self.budget.max_declared_products,
                    observed: u32::try_from(declared).unwrap_or(u32::MAX),
                },
            ));
        }
        // The execution capability is thread-local. Publication performs no
        // callbacks, awaits, or evaluator reentry, so no observer can see only
        // part of this fully validated bank/continuation transaction.
        let mut changed = false;
        for (slot, value) in staged {
            if state.get_at(slot) == value.as_ref() {
                continue;
            }
            changed = true;
            if slot.0 == product_offset(FlowDomain::DeclaredType).unwrap() {
                if let Some(value) = value {
                    if self.scope.declarations[slot.1].get().is_none() {
                        self.scope.declarations[slot.1]
                            .set(Box::new(value))
                            .expect("validated install-once authority");
                        self.scope.declared_active.borrow_mut().insert(slot.1);
                        let storage = self.scope.subjects[slot.1].storage;
                        let authority = &self.scope.declared_by_runtime[storage];
                        authority.set(Some(
                            authority.get().map_or(slot.1, |held| held.min(slot.1)),
                        ));
                    }
                }
            } else if let Some(value) = value {
                state.values.insert(slot, value);
            } else {
                state.values.remove(&slot);
            }
        }
        for transfer in transfers {
            self.executed[transfer.key.evidence()] = true;
        }
        Ok(changed)
    }

    /// Fold actual predecessors in the control interpreter's source/edge
    /// order. Recompute from those snapshots when they change; do not fold
    /// asynchronous arrivals into a previously joined result.
    pub fn join_products(
        &mut self,
        incoming: &[&FlowProductStore],
        algebra: &dyn FlowSemanticAlgebra,
    ) -> Result<FlowProductStore, FlowProductFailure> {
        self.ready()?;
        #[cfg(any(test, feature = "test-support"))]
        {
            self.join_product_visits = 0;
        }
        for state in incoming {
            if !Rc::ptr_eq(&self.scope, &state.scope) {
                return self.reject(FlowProductFailure::ScopeMismatch);
            }
        }
        let mut result = match incoming {
            [only] => (*only).clone(),
            _ => self.empty_state(),
        };
        if incoming.len() > 1 {
            // Merge the ordered materialized streams. The heap holds one head
            // per predecessor, so scratch is O(predecessors), and absent cells
            // never enter the work stream. Equal addresses retain source order.
            let mut streams: Vec<_> = incoming
                .iter()
                .map(|state| state.values.iter().peekable())
                .collect();
            let mut heads = std::collections::BinaryHeap::with_capacity(streams.len());
            for (index, stream) in streams.iter_mut().enumerate() {
                if let Some((address, _)) = stream.peek() {
                    heads.push(std::cmp::Reverse((**address, index)));
                }
            }
            while let Some(std::cmp::Reverse((address, first))) = heads.pop() {
                let mut products: smallvec::SmallVec<[&FlowProductValue; 4]> =
                    smallvec::SmallVec::new();
                let mut index = first;
                loop {
                    let (_, product) = streams[index].next().expect("queued product stream head");
                    #[cfg(any(test, feature = "test-support"))]
                    {
                        self.join_product_visits += 1;
                    }
                    products.push(product);
                    if let Some((next, _)) = streams[index].peek() {
                        heads.push(std::cmp::Reverse((**next, index)));
                    }
                    match heads.peek() {
                        Some(std::cmp::Reverse((next, _))) if *next == address => {
                            index = heads.pop().expect("matching queued head").0 .1;
                        }
                        _ => break,
                    }
                }
                let domain = PRODUCT_DOMAINS[address.0];
                let bottom = FlowProductValue::bottom(domain).expect("a product domain");
                let value = if domain == FlowDomain::ReachingType {
                    let mut reaching: smallvec::SmallVec<[&ReachingTypeProduct; 4]> =
                        smallvec::SmallVec::new();
                    for product in &products {
                        let FlowProductValue::ReachingType(product) = product else {
                            return self
                                .reject(FlowProductFailure::Gap(FlowGap::UnmodeledExpression));
                        };
                        reaching.push(product);
                    }
                    match join_reaching_types(algebra, &self.budget, &reaching) {
                        Ok(product) => FlowProductValue::ReachingType(product),
                        Err(failure) => return self.reject(failure),
                    }
                } else if domain == FlowDomain::ReachingValue {
                    match join_reaching_values(&self.budget, &products) {
                        Ok(product) => FlowProductValue::ReachingValue(product),
                        Err(failure) => return self.reject(failure),
                    }
                } else {
                    // Missing paths carry no reaching definition/type or source
                    // fact. They do remove narrowing and contribute Unassigned
                    // to assignment. One bottom accounts for any number of
                    // absent paths in these idempotent domain joins.
                    let missing_matters = match domain {
                        FlowDomain::Narrowing | FlowDomain::DefiniteAssignment => true,
                        FlowDomain::ReachingValue
                        | FlowDomain::ReachingType
                        | FlowDomain::DeclaredType => false,
                        FlowDomain::Completion
                        | FlowDomain::ClosureCapture
                        | FlowDomain::Freshness
                        | FlowDomain::Effects
                        | FlowDomain::CallResolution
                        | FlowDomain::Relation
                        | FlowDomain::ContextualTyping
                        | FlowDomain::Coverage => {
                            return self
                                .reject(FlowProductFailure::Gap(FlowGap::UnmodeledExpression));
                        }
                    };
                    let absent =
                        (missing_matters && products.len() < incoming.len()).then_some(&bottom);
                    let mut value = products[0].clone();
                    for next in products.iter().skip(1).copied().chain(absent) {
                        match join_product(algebra, &self.budget, &value, next) {
                            FlowTransferOutcome::Unchanged => {}
                            FlowTransferOutcome::Changed(joined) => value = joined,
                            FlowTransferOutcome::Gap(gap) => {
                                return self.reject(FlowProductFailure::Gap(gap))
                            }
                            FlowTransferOutcome::BudgetExceeded(exceeded) => {
                                return self.reject(FlowProductFailure::BudgetExceeded(exceeded))
                            }
                        }
                    }
                    value
                };
                if let Some(exceeded) = width_exceeded(&self.budget, value.width()) {
                    return self.reject(FlowProductFailure::BudgetExceeded(exceeded));
                }
                if value != bottom {
                    result.values.insert(address, value);
                }
            }
        }
        if result.values.len() > self.budget.max_products as usize {
            return self.reject(FlowProductFailure::BudgetExceeded(
                FlowProductBudgetExceeded {
                    axis: FlowProductBudgetAxis::Products,
                    limit: self.budget.max_products,
                    observed: u32::try_from(result.values.len()).unwrap_or(u32::MAX),
                },
            ));
        }
        Ok(result)
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn join_product_visits_for_tests(&self) -> usize {
        self.join_product_visits
    }

    /// Begin one actual fixed-point round, before running its transfers.
    /// Acyclic statements and ordinary continuation joins are not rounds.
    pub fn note_iteration(&mut self) -> Result<(), FlowProductFailure> {
        self.ready()?;
        if self.iterations >= self.budget.max_iterations {
            return self.reject(FlowProductFailure::BudgetExceeded(
                FlowProductBudgetExceeded {
                    axis: FlowProductBudgetAxis::Iterations,
                    limit: self.budget.max_iterations,
                    observed: self.iterations.saturating_add(1),
                },
            ));
        }
        self.iterations += 1;
        Ok(())
    }

    /// Seal successful execution. Later mutation is rejected, and a prior
    /// failure cannot be hidden by finishing an earlier valid snapshot.
    pub fn finish(&mut self) -> Result<FlowProductStatus, FlowProductFailure> {
        self.ready()?;
        self.sealed = true;
        Ok(FlowProductStatus {
            iterations: self.iterations,
        })
    }

    /// Only the exact proof plan expanded from this execution selection can
    /// obtain discharge evidence. An equal independently minted basis is insufficient.
    pub fn finish_with_plan(
        &mut self,
        plan: &FlowDemandPlan,
    ) -> Result<FlowProductEvidence, FlowProductFailure> {
        self.ready()?;
        if !plan.matches_execution(&self.scope.selection)
            || plan.basis() != self.scope.selection.basis()
        {
            return self.reject(FlowProductFailure::ScopeMismatch);
        }
        self.sealed = true;
        Ok(FlowProductEvidence {
            scope: Rc::clone(&self.scope),
            executed: std::mem::take(&mut self.executed).into(),
            iterations: self.iterations,
        })
    }
}

/// Exhaustive executed transfer validation. Selection and execution scope
/// are checked by the controller before this pure domain operation.
pub fn transfer_product(
    budget: &FlowProductBudget,
    key: &FlowProductKey,
    incoming: Option<&FlowProductValue>,
    value: Option<&FlowProductValue>,
) -> Result<Option<FlowProductValue>, FlowProductFailure> {
    let Some(value) = value else {
        if key.domain() == FlowDomain::DeclaredType && incoming.is_some() {
            return Err(FlowProductFailure::Gap(FlowGap::UnmodeledExpression));
        }
        return Ok(None);
    };
    if key.domain() != value.domain() {
        return Err(FlowProductFailure::Gap(FlowGap::UnmodeledExpression));
    }
    match key.domain() {
        FlowDomain::ReachingValue => {
            let FlowProductValue::ReachingValue(product) = value else {
                unreachable!()
            };
            if product
                .scope
                .as_ref()
                .is_some_and(|scope| !Rc::ptr_eq(scope, &key.scope))
            {
                return Err(FlowProductFailure::ScopeMismatch);
            }
            for node in product.definitions() {
                key.scope
                    .subject_at(*node)
                    .map_err(|_| FlowProductFailure::Gap(FlowGap::UnmodeledExpression))?;
            }
        }
        FlowDomain::ReachingType | FlowDomain::DefiniteAssignment => {}
        FlowDomain::DeclaredType => {
            if let (
                Some(FlowProductValue::DeclaredType(held)),
                FlowProductValue::DeclaredType(new),
            ) = (incoming, value)
            {
                if held.declared().is_some() && held != new {
                    return Err(FlowProductFailure::Gap(FlowGap::UnmodeledExpression));
                }
            }
        }
        FlowDomain::Narrowing => {
            let FlowProductValue::Narrowing(product) = value else {
                unreachable!()
            };
            let runtime = key.runtime_binding();
            if product.facts().iter().any(|fact| {
                let map = &key.scope.inputs.bindings;
                let identity = map
                    .local(&fact.binding)
                    .and_then(|local| map.runtime_identity(local))
                    .unwrap_or(&fact.binding);
                Some(identity) != runtime
            }) {
                return Err(FlowProductFailure::Gap(FlowGap::UnmodeledExpression));
            }
            let normalized = FlowProductValue::Narrowing(NarrowingProduct::new(
                product.facts().iter().cloned().map(|mut fact| {
                    fact.binding = runtime.expect("a fact has a binding").clone();
                    fact
                }),
            ));
            if let Some(exceeded) = width_exceeded(budget, normalized.width()) {
                return Err(FlowProductFailure::BudgetExceeded(exceeded));
            }
            return Ok((normalized.width() != 0).then_some(normalized));
        }
        FlowDomain::Completion
        | FlowDomain::ClosureCapture
        | FlowDomain::Freshness
        | FlowDomain::Effects
        | FlowDomain::CallResolution
        | FlowDomain::Relation
        | FlowDomain::ContextualTyping
        | FlowDomain::Coverage => {
            return Err(FlowProductFailure::Gap(FlowGap::UnmodeledExpression))
        }
    }
    if let Some(exceeded) = width_exceeded(budget, value.width()) {
        return Err(FlowProductFailure::BudgetExceeded(exceeded));
    }
    Ok((FlowProductValue::bottom(value.domain()).as_ref() != Some(value)).then(|| value.clone()))
}

// ── The canonical semantic-type algebra seam ───────────────────────────

/// One canonical composite construction: the constructed node plus whether
/// the construction was proven canonical. `incomplete` is not cosmetic —
/// an unproven composite is REFUSED by the join, which returns a typed gap
/// instead of publishing an unproven product.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FlowAlgebraComposite {
    /// The constructed semantic node.
    pub node: SemanticNodeId,
    /// Whether the canonical construction could not be proven.
    pub incomplete: bool,
}

pub use super::canonical_algebra::{LiteralFreshness, LiteralProvenance, LiteralProvenanceResult};

/// The canonical semantic-type algebra a product join constructs every
/// semantic composite through. This substrate never assembles a union or
/// an intersection itself; the sole production implementor forwards to the
/// dispatch's canonical union authority, which deposits the construction's
/// evidence on the dispatch's own rails.
pub trait FlowSemanticAlgebra {
    /// The canonical union of `members`.
    fn union(&self, members: &[SemanticNodeId]) -> FlowAlgebraComposite;
    /// One bounded canonical-owner inspection across both inputs and result.
    fn literal_provenance(
        &self,
        inputs: &[LiteralProvenance<'_>],
        result: SemanticNodeId,
    ) -> Result<LiteralProvenanceResult, FlowGap>;
}

impl FlowSemanticAlgebra for super::ProjectSemanticDispatch<'_> {
    fn literal_provenance(
        &self,
        inputs: &[LiteralProvenance<'_>],
        result: SemanticNodeId,
    ) -> Result<LiteralProvenanceResult, FlowGap> {
        let (membership, evidence) =
            super::canonical_algebra::inspect_literal_provenance(self.graph(), inputs, result);
        let incomplete = evidence.incomplete;
        self.deposit_canonical_evidence(evidence);
        if incomplete {
            Err(FlowGap::UnmodeledExpression)
        } else {
            Ok(membership)
        }
    }
    fn union(&self, members: &[SemanticNodeId]) -> FlowAlgebraComposite {
        let composite = super::canonical_algebra::canonical_union(self.graph(), members);
        let incomplete = composite.evidence.incomplete;
        self.deposit_canonical_evidence(composite.evidence);
        FlowAlgebraComposite {
            node: composite.node,
            incomplete,
        }
    }
}

/// The same canonical authority over a bare graph store — the seam the
/// product suites drive. Compiled only under the crate's explicit
/// test-support gate, so an ordinary production build has exactly one
/// implementor and both routes call the one canonical construction.
#[cfg(any(test, feature = "test-support"))]
pub struct GraphSemanticAlgebra<'g>(pub &'g crate::semantic_query_memo::SemanticGraphStore);

#[cfg(any(test, feature = "test-support"))]
impl FlowSemanticAlgebra for GraphSemanticAlgebra<'_> {
    fn literal_provenance(
        &self,
        inputs: &[LiteralProvenance<'_>],
        result: SemanticNodeId,
    ) -> Result<LiteralProvenanceResult, FlowGap> {
        let (membership, evidence) =
            super::canonical_algebra::inspect_literal_provenance(self.0, inputs, result);
        if evidence.incomplete {
            Err(FlowGap::UnmodeledExpression)
        } else {
            Ok(membership)
        }
    }
    fn union(&self, members: &[SemanticNodeId]) -> FlowAlgebraComposite {
        let composite = super::canonical_algebra::canonical_union(self.0, members);
        FlowAlgebraComposite {
            node: composite.node,
            incomplete: composite.evidence.incomplete,
        }
    }
}

// ── The product vocabulary ─────────────────────────────────────────────

/// Exhaustive projection of the shared domain registry onto stored products.
#[must_use]
pub fn domain_carries_product(domain: FlowDomain) -> bool {
    product_offset(domain).is_some()
}

/// Reaching definitions: the canonical SET of graph sites that provide the
/// subject's value. The carrier is sorted by node index and deduplicated
/// at construction, so two equal definition sets are one value and a join
/// cannot depend on contributor arrival order.
#[derive(Debug, Clone, Default)]
pub struct ReachingValueProduct {
    definitions: Arc<[FlowNodeId]>,
    scope: Option<Rc<ProductLayout>>,
}

impl PartialEq for ReachingValueProduct {
    fn eq(&self, other: &Self) -> bool {
        self.definitions == other.definitions
            && match (&self.scope, &other.scope) {
                (None, None) => true,
                (Some(a), Some(b)) => Rc::ptr_eq(a, b),
                _ => false,
            }
    }
}
impl Eq for ReachingValueProduct {}

impl ReachingValueProduct {
    /// One executed definition, carrying its selected execution scope.
    #[must_use]
    pub fn at(site: &SelectedFlowSite) -> Self {
        Self {
            definitions: Arc::from([site.scope.subjects[site.subject].node]),
            scope: Some(Rc::clone(&site.scope)),
        }
    }

    /// The reaching definition sites, in canonical order.
    #[must_use]
    pub fn definitions(&self) -> &[FlowNodeId] {
        &self.definitions
    }
}

/// The widening membership of one subject — WHICH of its literal values
/// widen at a widening read. `All` is the classic widening-literal
/// `const`; `Partial` records exactly the fresh values of a
/// mixed-freshness conditional initializer or a union-carried fresh call
/// deposit, so an authored pinned arm alongside them stays pinned.
///
/// Literal-widening provenance is a property of the values REACHING a
/// subject, so it rides the reaching-type product rather than a second
/// state layer: a path that cannot carry a subject's value cannot carry
/// its widening membership either.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WideningMembership {
    /// Every literal (arm) widens at a widening read.
    All,
    /// Exactly these literal values widen; sibling arms stay pinned.
    Partial(Arc<[SemanticNodeId]>),
}

/// Ordered, unique semantic contributors and their canonical composite.
/// Predecessors arrive in the control interpreter's source/edge order;
/// canonical semantic algebra owns structural deduplication and the final
/// member representation. Literal widening provenance follows these values.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ReachingTypeProduct {
    contributors: Arc<[SemanticNodeId]>,
    united: Option<SemanticNodeId>,
    widening: Option<WideningMembership>,
}

impl ReachingTypeProduct {
    /// A single-contributor product: the contributor IS the reaching type,
    /// so no composite construction is needed.
    #[must_use]
    pub fn of(contributor: SemanticNodeId) -> Self {
        Self {
            contributors: Arc::from(vec![contributor].into_boxed_slice()),
            united: Some(contributor),
            widening: None,
        }
    }

    /// Literal membership retained by this reaching value.
    #[must_use]
    pub fn with_widening(mut self, widening: Option<WideningMembership>) -> Self {
        self.widening = widening.map(|membership| match membership {
            WideningMembership::All => WideningMembership::All,
            WideningMembership::Partial(members) => {
                let mut members = members.to_vec();
                members.sort_unstable();
                members.dedup();
                WideningMembership::Partial(members.into())
            }
        });
        self
    }

    /// Literal values which widen when this value is read.
    #[must_use]
    pub fn widening(&self) -> Option<&WideningMembership> {
        self.widening.as_ref()
    }

    /// The contributors in stable source/edge order.
    #[must_use]
    pub fn contributors(&self) -> &[SemanticNodeId] {
        &self.contributors
    }

    /// The algebra-constructed reaching type, when the product carries any
    /// contributor.
    #[must_use]
    pub fn united(&self) -> Option<SemanticNodeId> {
        self.united
    }
}

/// The subject's declared (annotation) type. A declaration fact, not a
/// path-dependent one: joining two DIFFERENT declared types is a typed gap
/// rather than an invented merge.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct DeclaredTypeProduct {
    declared: Option<SemanticNodeId>,
}

impl DeclaredTypeProduct {
    /// The product declaring `declared`.
    #[must_use]
    pub fn of(declared: SemanticNodeId) -> Self {
        Self {
            declared: Some(declared),
        }
    }

    /// The declared type, when one is established.
    #[must_use]
    pub fn declared(&self) -> Option<SemanticNodeId> {
        self.declared
    }
}

/// One guard fact: a binding narrowed to a semantic type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlowNarrowingFact {
    /// The narrowed binding's stable cross-frame identity.
    pub binding: FlowBindingIdentity,
    /// The authored member path; empty means the binding itself.
    pub path: Arc<[Arc<str>]>,
    /// The type the guard narrows it to.
    pub narrowed_to: SemanticNodeId,
}

/// Identity-only ordering: display names and binding-kind metadata cannot
/// move a fact's identity or prevent deduplication of equal subjects.
fn narrowing_order(fact: &FlowNarrowingFact) -> (&FunctionProgramKey, u32, &[Arc<str>], u64) {
    (
        &fact.binding.defining_function,
        fact.binding.binding_slot,
        fact.path.as_ref(),
        fact.narrowed_to.0,
    )
}

/// The guard facts that hold at the subject. The join INTERSECTS: a fact
/// survives a merge point only when every incoming edge established it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct NarrowingProduct {
    facts: Arc<[FlowNarrowingFact]>,
}

impl NarrowingProduct {
    /// The canonical product over `facts` (sorted, deduplicated).
    #[must_use]
    pub fn new(facts: impl IntoIterator<Item = FlowNarrowingFact>) -> Self {
        let mut facts: Vec<FlowNarrowingFact> = facts.into_iter().collect();
        facts.sort_by(|a, b| narrowing_order(a).cmp(&narrowing_order(b)));
        facts.dedup();
        Self {
            facts: Arc::from(facts.into_boxed_slice()),
        }
    }

    /// The guard facts, in canonical order.
    #[must_use]
    pub fn facts(&self) -> &[FlowNarrowingFact] {
        &self.facts
    }
}

/// The definite-assignment lattice. `MaybeAssigned` is the join of the two
/// definite states — the honest answer at a merge point where one incoming
/// edge assigned and another did not.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum DefiniteAssignment {
    /// No incoming path assigned the subject.
    #[default]
    Unassigned,
    /// Every incoming path assigned the subject.
    Assigned,
    /// Some incoming path assigned the subject and some did not.
    MaybeAssigned,
}

impl DefiniteAssignment {
    /// The lattice join: idempotent, commutative, associative, with
    /// `MaybeAssigned` as the top element.
    #[must_use]
    pub fn join(self, other: Self) -> Self {
        match (self, other) {
            (Self::Unassigned, Self::Unassigned) => Self::Unassigned,
            (Self::Assigned, Self::Assigned) => Self::Assigned,
            _ => Self::MaybeAssigned,
        }
    }
}

/// The definite-assignment product of one subject: its lattice point plus
/// the two READ-observable facts about the surviving definition.
///
/// The two flags are deliberately NOT the lattice: they union across a
/// merge (a subject whose value came from one path on EITHER incoming
/// edge is still one path's after the merge), while the lattice point
/// joins by its own rule. Folding them into the lattice would make a
/// subject defined on only one incoming edge indistinguishable from one
/// whose surviving definition is one path's — two different reasons a
/// read must fail closed, and only one of them is a `var` conditional
/// definition.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct DefiniteAssignmentProduct {
    state: DefiniteAssignment,
    single_path: bool,
    failed_initializer: bool,
}

impl DefiniteAssignmentProduct {
    /// The product of a subject definitely assigned on this path.
    #[must_use]
    pub fn assigned() -> Self {
        Self {
            state: DefiniteAssignment::Assigned,
            ..Self::default()
        }
    }

    /// Whether the surviving reaching definition is ONE control-flow
    /// path's rather than the join of every path that reaches the read.
    #[must_use]
    pub fn single_path(self) -> bool {
        self.single_path
    }

    /// Whether the subject's initializer FAILED with a typed flow failure
    /// (its value is a modeled `any`, not the initializer's real type).
    #[must_use]
    pub fn failed_initializer(self) -> bool {
        self.failed_initializer
    }

    /// The same product with `single_path` set to `value`.
    #[must_use]
    pub fn with_single_path(mut self, value: bool) -> Self {
        self.single_path = value;
        self
    }

    /// The same product with `failed_initializer` set to `value`.
    #[must_use]
    pub fn with_failed_initializer(mut self, value: bool) -> Self {
        self.failed_initializer = value;
        self
    }

    /// The same product at `state`.
    #[must_use]
    pub fn with_state(mut self, state: DefiniteAssignment) -> Self {
        self.state = state;
        self
    }

    /// The join: the lattice point joins by its own rule; both
    /// read-observable flags union.
    #[must_use]
    pub fn join(self, other: Self) -> Self {
        Self {
            state: self.state.join(other.state),
            single_path: self.single_path || other.single_path,
            failed_initializer: self.failed_initializer || other.failed_initializer,
        }
    }
}

/// One product value. Exactly one arm per product-bearing [`FlowDomain`];
/// the store refuses a value whose arm does not match its key's domain.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FlowProductValue {
    /// Reaching definitions.
    ReachingValue(ReachingValueProduct),
    /// Reaching types.
    ReachingType(ReachingTypeProduct),
    /// The declared type.
    DeclaredType(DeclaredTypeProduct),
    /// The guard facts.
    Narrowing(NarrowingProduct),
    /// The definite-assignment state.
    DefiniteAssignment(DefiniteAssignmentProduct),
}

impl FlowProductValue {
    /// The registry domain this value is the product of.
    #[must_use]
    pub fn domain(&self) -> FlowDomain {
        match self {
            Self::ReachingValue(_) => FlowDomain::ReachingValue,
            Self::ReachingType(_) => FlowDomain::ReachingType,
            Self::DeclaredType(_) => FlowDomain::DeclaredType,
            Self::Narrowing(_) => FlowDomain::Narrowing,
            Self::DefiniteAssignment(_) => FlowDomain::DefiniteAssignment,
        }
    }

    /// The domain's bottom element — the value at a subject no edge has
    /// reached yet. `None` for a registry domain carrying no product.
    #[rustfmt::skip]
    #[must_use]
    pub fn bottom(domain: FlowDomain) -> Option<Self> {
        match domain {
            FlowDomain::ReachingValue => {
                Some(Self::ReachingValue(ReachingValueProduct::default()))
            }
            FlowDomain::ReachingType => Some(Self::ReachingType(ReachingTypeProduct::default())),
            FlowDomain::DeclaredType => Some(Self::DeclaredType(DeclaredTypeProduct::default())),
            FlowDomain::Narrowing => Some(Self::Narrowing(NarrowingProduct::default())),
            FlowDomain::DefiniteAssignment => {
                Some(Self::DefiniteAssignment(DefiniteAssignmentProduct::default()))
            }
            FlowDomain::Completion | FlowDomain::ClosureCapture | FlowDomain::Freshness
            | FlowDomain::Effects | FlowDomain::CallResolution | FlowDomain::Relation
            | FlowDomain::ContextualTyping | FlowDomain::Coverage => None,
        }
    }

    /// The element count of the value's carrier — the measurement
    /// [`FlowProductBudget::max_product_width`] bounds. A scalar product
    /// (a declared type, a definite-assignment state) is width 1; an
    /// unestablished declared type is width 0.
    #[must_use]
    pub fn width(&self) -> usize {
        match self {
            Self::ReachingValue(product) => product.definitions().len(),
            Self::ReachingType(product) => {
                product.contributors().len().max(match product.widening() {
                    Some(WideningMembership::Partial(members)) => members.len(),
                    _ => 0,
                })
            }
            Self::DeclaredType(product) => usize::from(product.declared().is_some()),
            Self::Narrowing(product) => product.facts().len(),
            Self::DefiniteAssignment(_) => 1,
        }
    }
}

/// The axis a product solve exhausted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FlowProductBudgetAxis {
    /// Fixed-point iterations.
    Iterations,
    /// The size of the product universe the solve would store.
    Products,
    /// Materialized source-declaration facts over the execution lifetime.
    DeclaredProducts,
    /// The element count of one product's carrier.
    Width,
}

/// A typed product-budget exhaustion.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FlowProductBudgetExceeded {
    /// The exhausted axis.
    pub axis: FlowProductBudgetAxis,
    /// The axis limit.
    pub limit: u32,
    /// The measurement that forced refusal: materialized product count,
    /// carrier width, or the attempted fixed-point round.
    pub observed: u32,
}

/// The resource policy one product solve runs under.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FlowProductBudget {
    /// The maximum number of fixed-point iterations. A solve that
    /// stabilizes WITHIN this many iterations completes; one that would
    /// need another iteration is budget-exhausted.
    pub max_iterations: u32,
    /// Maximum materialized runtime products in one continuation snapshot.
    /// Source authority has its own execution-global cap below; the sum of
    /// the caps bounds all values visible through any snapshot.
    pub max_products: u32,
    /// Maximum source-declaration facts in the execution's append-only bank.
    /// These facts survive every continuation and do not consume its runtime cap.
    pub max_declared_products: u32,
    /// The maximum element count of one product's carrier. A store
    /// INVARIANT, not a join-local check: every value a transfer or a join
    /// produces is measured against it before it can be stored.
    pub max_product_width: u32,
}

impl Default for FlowProductBudget {
    fn default() -> Self {
        Self {
            max_iterations: 16,
            max_products: 4096,
            max_declared_products: 4096,
            max_product_width: 64,
        }
    }
}

impl FlowProductBudget {
    /// Capacity follows selected execution, independently of proof obligations.
    #[must_use]
    pub fn for_execution_selection(plan: &FlowExecutionSelection) -> Self {
        let selection = plan.structural_selection();
        // Captured bindings are real selected graph nodes and therefore use
        // the same structural limit as every other product subject.
        let subjects = selection
            .value_nodes()
            .len()
            .saturating_add(selection.effect_only_nodes().len());
        let subjects = u32::try_from(subjects).unwrap_or(u32::MAX);
        Self {
            max_iterations: plan.convergence().max_iterations,
            max_products: subjects.saturating_mul((PRODUCT_DOMAINS.len() - 1) as u32),
            max_declared_products: subjects,
            max_product_width: plan.resources().slice_budget.max_selected_nodes,
        }
    }
    #[must_use]
    pub fn for_demand_plan(plan: &FlowDemandPlan) -> Self {
        Self::for_execution_selection(plan.execution_selection())
    }
}

/// The outcome of one transfer or join step. `Gap` and `BudgetExceeded`
/// carry NO product: a degraded step has nothing a store could admit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FlowTransferOutcome {
    /// The step's output equals its input.
    Unchanged,
    /// The step produced a different product.
    Changed(FlowProductValue),
    /// The step cannot be modelled; the typed gap says why.
    Gap(FlowGap),
    /// The step exhausted a budget axis.
    BudgetExceeded(FlowProductBudgetExceeded),
}

// ── Join ───────────────────────────────────────────────────────────────

/// Join `a` and `b` at a merge point — the ONE join route, exhaustive over
/// the domain registry and domain-SPECIFIC by construction:
///
/// - **Reaching values** union as a canonical SET of definition sites.
/// - **Reaching types** union their canonical contributor SET and then ask
///   the canonical algebra to construct the semantic result; an unproven
///   construction is a typed gap, never an unproven published product.
/// - **Declared types** agree or gap: a merge point cannot invent a
///   declaration neither edge declared.
/// - **Narrowing** INTERSECTS: a guard fact survives only when EVERY
///   incoming edge established it.
/// - **Definite assignment** uses its declared lattice.
///
/// Joins are idempotent. Reaching definitions and narrowing use canonical
/// sets; assignment is a semilattice. Reaching-type contributor order follows
/// actual predecessors so semantic representative selection is deterministic;
/// freshness combines commutatively with pinned literal occurrences winning.
/// Recompute a changed join from its ordered predecessors, never arrival order.
///
/// The match is on the shared DOMAIN, not on the value pair, so there is
/// no `_` arm: a new product-bearing registry domain fails to compile here
/// until it declares its join.
pub fn join_product(
    algebra: &dyn FlowSemanticAlgebra,
    budget: &FlowProductBudget,
    a: &FlowProductValue,
    b: &FlowProductValue,
) -> FlowTransferOutcome {
    let domain = a.domain();
    if domain != b.domain() {
        return FlowTransferOutcome::Gap(FlowGap::UnmodeledExpression);
    }
    if let Some(exceeded) = width_exceeded(budget, a.width().max(b.width())) {
        return FlowTransferOutcome::BudgetExceeded(exceeded);
    }
    if a == b {
        return FlowTransferOutcome::Unchanged;
    }
    let joined = match domain {
        FlowDomain::ReachingValue => match join_reaching_values(budget, &[a, b]) {
            Ok(product) => FlowProductValue::ReachingValue(product),
            Err(FlowProductFailure::BudgetExceeded(exceeded)) => {
                return FlowTransferOutcome::BudgetExceeded(exceeded)
            }
            Err(FlowProductFailure::Gap(gap)) => return FlowTransferOutcome::Gap(gap),
            Err(FlowProductFailure::ScopeMismatch | FlowProductFailure::Sealed) => {
                return FlowTransferOutcome::Gap(FlowGap::UnmodeledExpression)
            }
        },
        FlowDomain::ReachingType => {
            let (FlowProductValue::ReachingType(left), FlowProductValue::ReachingType(right)) =
                (a, b)
            else {
                return FlowTransferOutcome::Gap(FlowGap::UnmodeledExpression);
            };
            match join_reaching_types(algebra, budget, &[left, right]) {
                Ok(product) => FlowProductValue::ReachingType(product),
                Err(FlowProductFailure::BudgetExceeded(exceeded)) => {
                    return FlowTransferOutcome::BudgetExceeded(exceeded)
                }
                Err(FlowProductFailure::Gap(gap)) => return FlowTransferOutcome::Gap(gap),
                Err(FlowProductFailure::ScopeMismatch | FlowProductFailure::Sealed) => {
                    return FlowTransferOutcome::Gap(FlowGap::UnmodeledExpression)
                }
            }
        }
        FlowDomain::DeclaredType => {
            let (FlowProductValue::DeclaredType(left), FlowProductValue::DeclaredType(right)) =
                (a, b)
            else {
                return FlowTransferOutcome::Gap(FlowGap::UnmodeledExpression);
            };
            match (left.declared, right.declared) {
                (None, other) => {
                    FlowProductValue::DeclaredType(DeclaredTypeProduct { declared: other })
                }
                (held, None) => {
                    FlowProductValue::DeclaredType(DeclaredTypeProduct { declared: held })
                }
                (Some(left), Some(right)) if left == right => {
                    FlowProductValue::DeclaredType(DeclaredTypeProduct {
                        declared: Some(left),
                    })
                }
                (Some(_), Some(_)) => {
                    return FlowTransferOutcome::Gap(FlowGap::UnmodeledExpression)
                }
            }
        }
        FlowDomain::Narrowing => {
            let (FlowProductValue::Narrowing(left), FlowProductValue::Narrowing(right)) = (a, b)
            else {
                return FlowTransferOutcome::Gap(FlowGap::UnmodeledExpression);
            };
            let mut common = Vec::new();
            let (mut a, mut b) = (
                left.facts().iter().peekable(),
                right.facts().iter().peekable(),
            );
            while let (Some(left), Some(right)) = (a.peek(), b.peek()) {
                match narrowing_order(left).cmp(&narrowing_order(right)) {
                    std::cmp::Ordering::Less => {
                        a.next();
                    }
                    std::cmp::Ordering::Greater => {
                        b.next();
                    }
                    std::cmp::Ordering::Equal => {
                        common.push((*left).clone());
                        a.next();
                        b.next();
                    }
                }
            }
            FlowProductValue::Narrowing(NarrowingProduct {
                facts: common.into(),
            })
        }
        FlowDomain::DefiniteAssignment => {
            let (
                FlowProductValue::DefiniteAssignment(left),
                FlowProductValue::DefiniteAssignment(right),
            ) = (a, b)
            else {
                return FlowTransferOutcome::Gap(FlowGap::UnmodeledExpression);
            };
            FlowProductValue::DefiniteAssignment(left.join(*right))
        }
        // No value is a product of a productless domain, so this join is
        // unreachable — enumerated rather than a wildcard so a new
        // registry domain must classify its join deliberately.
        FlowDomain::Completion
        | FlowDomain::ClosureCapture
        | FlowDomain::Freshness
        | FlowDomain::Effects
        | FlowDomain::CallResolution
        | FlowDomain::Relation
        | FlowDomain::ContextualTyping
        | FlowDomain::Coverage => return FlowTransferOutcome::Gap(FlowGap::UnmodeledExpression),
    };
    if let Some(exceeded) = width_exceeded(budget, joined.width()) {
        return FlowTransferOutcome::BudgetExceeded(exceeded);
    }
    if &joined == a {
        FlowTransferOutcome::Unchanged
    } else {
        FlowTransferOutcome::Changed(joined)
    }
}

fn width_exceeded(budget: &FlowProductBudget, width: usize) -> Option<FlowProductBudgetExceeded> {
    if width > budget.max_product_width as usize {
        return Some(FlowProductBudgetExceeded {
            axis: FlowProductBudgetAxis::Width,
            limit: budget.max_product_width,
            observed: u32::try_from(width).unwrap_or(u32::MAX),
        });
    }
    None
}

/// Merge canonical definition streams once. Scratch follows actual inputs;
/// width is checked before admitting each new distinct output definition.
fn join_reaching_values(
    budget: &FlowProductBudget,
    products: &[&FlowProductValue],
) -> Result<ReachingValueProduct, FlowProductFailure> {
    let mut sources: smallvec::SmallVec<[&ReachingValueProduct; 4]> = smallvec::SmallVec::new();
    let mut scope: Option<Rc<ProductLayout>> = None;
    let mut total = 0usize;
    for product in products {
        let FlowProductValue::ReachingValue(value) = product else {
            return Err(FlowProductFailure::Gap(FlowGap::UnmodeledExpression));
        };
        if let Some(exceeded) = width_exceeded(budget, value.definitions.len()) {
            return Err(FlowProductFailure::BudgetExceeded(exceeded));
        }
        if let Some(incoming) = &value.scope {
            if let Some(held) = &scope {
                if !Rc::ptr_eq(held, incoming) {
                    return Err(FlowProductFailure::ScopeMismatch);
                }
            } else {
                scope = Some(Rc::clone(incoming));
            }
        }
        total = total.saturating_add(value.definitions.len());
        sources.push(value);
    }
    if let [only] = sources.as_slice() {
        return Ok((*only).clone());
    }
    let mut heads = std::collections::BinaryHeap::with_capacity(sources.len());
    for (source, product) in sources.iter().enumerate() {
        if let Some(node) = product.definitions.first() {
            heads.push(std::cmp::Reverse((node.index(), source, 0usize)));
        }
    }
    let mut definitions = Vec::with_capacity(total.min(budget.max_product_width as usize));
    while let Some(std::cmp::Reverse((_, source, offset))) = heads.pop() {
        let node = sources[source].definitions[offset];
        if definitions.last() != Some(&node) {
            if let Some(exceeded) = width_exceeded(budget, definitions.len().saturating_add(1)) {
                return Err(FlowProductFailure::BudgetExceeded(exceeded));
            }
            definitions.push(node);
        }
        if let Some(next) = sources[source].definitions.get(offset + 1) {
            heads.push(std::cmp::Reverse((next.index(), source, offset + 1)));
        }
    }
    Ok(ReachingValueProduct {
        definitions: Arc::from(definitions.into_boxed_slice()),
        scope,
    })
}

/// One canonical reaching-type operation over the actual incoming paths.
/// Contributors preserve source/edge order and are deduplicated before the
/// canonical owner constructs the final type. Temporary binary prefixes never
/// consume/reset independent provenance budgets or publish intermediate types.
fn join_reaching_types(
    algebra: &dyn FlowSemanticAlgebra,
    budget: &FlowProductBudget,
    products: &[&ReachingTypeProduct],
) -> Result<ReachingTypeProduct, FlowProductFailure> {
    for product in products {
        let width = product.contributors.len().max(match &product.widening {
            Some(WideningMembership::Partial(members)) => members.len(),
            _ => 0,
        });
        if let Some(exceeded) = width_exceeded(budget, width) {
            return Err(FlowProductFailure::BudgetExceeded(exceeded));
        }
    }
    let Some(first) = products.first() else {
        return Ok(ReachingTypeProduct::default());
    };
    if products.iter().all(|product| *product == *first) {
        return Ok((*first).clone());
    }
    let mut seen = rustc_hash::FxHashSet::default();
    let mut contributors = Vec::new();
    for product in products {
        for contributor in product.contributors.iter().copied() {
            if seen.insert(contributor) {
                contributors.push(contributor);
                if let Some(exceeded) = width_exceeded(budget, contributors.len()) {
                    return Err(FlowProductFailure::BudgetExceeded(exceeded));
                }
            }
        }
    }
    let united = match contributors.as_slice() {
        [] => None,
        [single] => Some(*single),
        members => {
            let composite = algebra.union(members);
            if composite.incomplete {
                return Err(FlowProductFailure::Gap(FlowGap::UnmodeledExpression));
            }
            Some(composite.node)
        }
    };
    let widening =
        match united.filter(|_| products.iter().any(|product| product.widening.is_some())) {
            None => None,
            Some(result) => {
                let inputs: smallvec::SmallVec<[LiteralProvenance<'_>; 4]> = products
                    .iter()
                    .map(|product| LiteralProvenance {
                        root: product.united,
                        fresh: match product.widening() {
                            None => LiteralFreshness::Pinned,
                            Some(WideningMembership::All) => LiteralFreshness::All,
                            Some(WideningMembership::Partial(members)) => {
                                LiteralFreshness::Partial(members)
                            }
                        },
                    })
                    .collect();
                match algebra
                    .literal_provenance(&inputs, result)
                    .map_err(FlowProductFailure::Gap)?
                {
                    LiteralProvenanceResult::None => None,
                    LiteralProvenanceResult::All => Some(WideningMembership::All),
                    LiteralProvenanceResult::Partial(members) => {
                        Some(WideningMembership::Partial(members.into()))
                    }
                }
            }
        };
    let product = ReachingTypeProduct {
        contributors: contributors.into(),
        united,
        widening,
    };
    if let Some(WideningMembership::Partial(members)) = &product.widening {
        if let Some(exceeded) = width_exceeded(budget, members.len()) {
            return Err(FlowProductFailure::BudgetExceeded(exceeded));
        }
    }
    Ok(product)
}
