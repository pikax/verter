//! Selected, scoped flow products and their exhaustive transfer/join algebra.
//!
//! The control interpreter supplies executed transfers and actual continuation
//! snapshots. The function graph and sealed demand own selection; dependence
//! edges never stand in for control-flow predecessors. All mutable product
//! state lives here. Semantic composites use the canonical type algebra.

#![cfg_attr(not(test), allow(dead_code))]

use super::flow_solve::{FlowBindingInventory, FlowDemandBasis, FlowDemandPlan, FlowDomain};
use crate::cache_runtime::flow_slice_node::{BoundFlowGraph, FlowSliceFunctionKey};
use crate::semantic_query::{FlowGap, SemanticNodeId};
use rustc_hash::FxHashMap;
use std::hash::{Hash, Hasher};
use std::sync::Arc;
use verter_semantic::analysis::flow::flow_graph::{FlowNodeId, FlowNodeKind, FunctionFlowGraph};
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
    bindings: Option<FlowBindingMap>,
    captures: Vec<(FlowBindingIdentity, FlowNodeId)>,
}

impl FlowProductInputs {
    pub(crate) fn for_bound_graph(
        bound: &BoundFlowGraph,
        inventory: &FlowBindingInventory,
    ) -> Self {
        Self {
            graph: Arc::clone(&bound.bundle().graph),
            scope: bound.key().clone(),
            bindings: FlowBindingMap::build(
                &bound.bundle().skeleton,
                &inventory.bindings,
                &bound.key().function,
                inventory.anchor,
            )
            .ok(),
            captures: Vec::new(),
        }
    }

    /// Resolved captured references from the child's content lowering, each
    /// anchored to its real read site. Selection is checked at attachment.
    pub(crate) fn with_captures(
        mut self,
        captures: impl IntoIterator<Item = (FlowBindingIdentity, FlowNodeId)>,
    ) -> Self {
        self.captures.extend(captures);
        self
    }

    #[must_use]
    pub fn graph(&self) -> &FunctionFlowGraph {
        &self.graph
    }
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
    basis: FlowDemandBasis,
    subjects: Box<[ProductSubject]>,
    node_subjects: usize,
    representatives: Box<[usize]>,
    captures: FxHashMap<FlowBindingIdentity, usize>,
}

impl ProductLayout {
    fn subject_at(&self, node: FlowNodeId) -> Result<usize, FlowProductKeyError> {
        self.subjects[..self.node_subjects]
            .binary_search_by_key(&node.index(), |subject| subject.node.index())
            .map_err(|_| FlowProductKeyError::UnselectedNode)
    }
    fn key(
        self: &Arc<Self>,
        domain: FlowDomain,
        subject: usize,
    ) -> Result<FlowProductKey, FlowProductKeyError> {
        let offset = product_offset(domain).ok_or(FlowProductKeyError::DomainCarriesNoProduct)?;
        Ok(FlowProductKey {
            scope: Arc::clone(self),
            subject,
            offset,
        })
    }
}

/// A selected domain/subject address, bound to one execution.
#[derive(Debug, Clone)]
pub struct FlowProductKey {
    scope: Arc<ProductLayout>,
    subject: usize,
    offset: usize,
}

impl PartialEq for FlowProductKey {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.scope, &other.scope)
            && self.subject == other.subject
            && self.offset == other.offset
    }
}
impl Eq for FlowProductKey {}
impl Hash for FlowProductKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        Arc::as_ptr(&self.scope).hash(state);
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
            FlowBindingRef::Local(binding) => self
                .scope
                .inputs
                .bindings
                .as_ref()?
                .runtime_identity(*binding),
            FlowBindingRef::Captured(identity) => Some(identity),
        }
    }
    fn storage(&self) -> usize {
        self.scope.subjects[self.subject].storage * PRODUCT_DOMAINS.len() + self.offset
    }
    fn evidence(&self) -> usize {
        self.subject * PRODUCT_DOMAINS.len() + self.offset
    }
}

/// A real selected site. The control interpreter decides when it executes.
#[derive(Debug, Clone)]
pub struct SelectedFlowSite {
    scope: Arc<ProductLayout>,
    subject: usize,
}

/// One snapshot of the only product store. The backing vector covers selected
/// runtime subjects; source declaration aliases share storage, not evidence.
#[derive(Debug, Clone)]
pub struct FlowProductStore {
    scope: Arc<ProductLayout>,
    values: Vec<Option<FlowProductValue>>,
    active: usize,
}

impl FlowProductStore {
    #[must_use]
    pub fn get(&self, key: &FlowProductKey) -> Option<&FlowProductValue> {
        if !Arc::ptr_eq(&self.scope, &key.scope) {
            return None;
        }
        self.values[key.storage()].as_ref()
    }
    #[must_use]
    pub fn len(&self) -> usize {
        self.active
    }
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.active == 0
    }
    /// Deterministic domain/selected-subject order, without sorting or cloning values.
    pub fn ordered_entries(&self) -> impl Iterator<Item = (FlowProductKey, &FlowProductValue)> {
        PRODUCT_DOMAINS
            .into_iter()
            .enumerate()
            .flat_map(move |(offset, domain)| {
                self.scope.representatives.iter().enumerate().filter_map(
                    move |(storage, subject)| {
                        let value =
                            self.values[storage * PRODUCT_DOMAINS.len() + offset].as_ref()?;
                        Some((
                            self.scope.key(domain, *subject).expect("a product domain"),
                            value,
                        ))
                    },
                )
            })
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
    scope: Arc<ProductLayout>,
    executed: Arc<[bool]>,
    iterations: u32,
}

impl FlowProductEvidence {
    #[must_use]
    pub fn executed(&self, key: &FlowProductKey) -> bool {
        Arc::ptr_eq(&self.scope, &key.scope) && self.executed[key.evidence()]
    }
    #[must_use]
    pub fn iterations(&self) -> u32 {
        self.iterations
    }
    #[must_use]
    pub fn basis(&self) -> &FlowDemandBasis {
        &self.scope.basis
    }
}

/// Shared execution controller. Snapshots share its immutable layout; only
/// actual transfers and predecessor joins can change stored products.
#[derive(Debug)]
pub struct FlowProductExecution {
    scope: Arc<ProductLayout>,
    budget: FlowProductBudget,
    executed: Vec<bool>,
    iterations: u32,
    failure: Option<FlowProductFailure>,
    sealed: bool,
}

impl FlowProductExecution {
    pub fn new(
        inputs: &FlowProductInputs,
        plan: &FlowDemandPlan,
        mut budget: FlowProductBudget,
    ) -> Result<Self, FlowProductKeyError> {
        if inputs.scope != plan.basis().graph_body {
            return Err(FlowProductKeyError::GraphMismatch);
        }
        budget.max_iterations = budget.max_iterations.min(plan.convergence().max_iterations);
        let selection = plan.structural_selection();
        if selection
            .value_nodes
            .len()
            .saturating_add(selection.effect_only_nodes.len())
            > plan.resources().slice_budget.max_selected_nodes as usize
        {
            return Err(FlowProductKeyError::SelectionBudget);
        }
        // Both owner arrays are sorted and disjoint. Merge them once; do not
        // rebuild graph adjacency or a whole-graph node/domain cross product.
        let mut values = selection.value_nodes.iter().peekable();
        let mut effects = selection.effect_only_nodes.iter().peekable();
        let mut subjects = Vec::with_capacity(values.len() + effects.len());
        let mut representatives = Vec::new();
        let mut local_storage = FxHashMap::default();
        while values.peek().is_some() || effects.peek().is_some() {
            let from_values = match (values.peek(), effects.peek()) {
                (Some(a), Some(b)) => a.index() < b.index(),
                (Some(_), None) => true,
                (None, _) => false,
            };
            let node = *if from_values {
                values.next()
            } else {
                effects.next()
            }
            .expect("a selected node");
            let subject = subjects.len();
            let (binding, binding_ref, existing_storage) = match inputs.graph.node_kind(node) {
                FlowNodeKind::Binding(binding) => {
                    let map = inputs
                        .bindings
                        .as_ref()
                        .ok_or(FlowProductKeyError::UnmodeledBinding)?;
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
        let node_subjects = subjects.len();
        let mut capture_sites: FxHashMap<&FlowBindingIdentity, FlowNodeId> = FxHashMap::default();
        for (identity, node) in &inputs.captures {
            if !selection.is_selected(*node) {
                continue;
            }
            if let Some(held) = capture_sites.get_mut(identity) {
                if node.index() < held.index() {
                    *held = *node;
                }
            } else {
                if capture_sites.len() >= plan.resources().max_obligations as usize {
                    return Err(FlowProductKeyError::SelectionBudget);
                }
                capture_sites.insert(identity, *node);
            }
        }
        let mut selected_captures: Vec<_> = capture_sites.into_iter().collect();
        selected_captures.sort_by(|(a, an), (b, bn)| {
            (&a.defining_function, a.binding_slot, an.index()).cmp(&(
                &b.defining_function,
                b.binding_slot,
                bn.index(),
            ))
        });
        let mut captures = FxHashMap::default();
        for (identity, node) in selected_captures {
            if identity.defining_function == inputs.scope.function {
                return Err(FlowProductKeyError::InvalidCapture);
            }
            if captures.contains_key(identity) {
                continue;
            }
            let subject = subjects.len();
            let storage = representatives.len();
            representatives.push(subject);
            captures.insert(identity.clone(), subject);
            subjects.push(ProductSubject {
                node,
                binding: Some(identity.clone()),
                binding_ref: Some(FlowBindingRef::Captured(identity.clone())),
                storage,
            });
        }
        let executed = vec![false; subjects.len() * PRODUCT_DOMAINS.len()];
        Ok(Self {
            scope: Arc::new(ProductLayout {
                inputs: inputs.clone(),
                basis: plan.basis().clone(),
                subjects: subjects.into(),
                node_subjects,
                representatives: representatives.into(),
                captures,
            }),
            budget,
            executed,
            iterations: 0,
            failure: None,
            sealed: false,
        })
    }

    #[must_use]
    pub fn empty_state(&self) -> FlowProductStore {
        FlowProductStore {
            scope: Arc::clone(&self.scope),
            values: vec![None; self.scope.representatives.len() * PRODUCT_DOMAINS.len()],
            active: 0,
        }
    }
    #[must_use]
    pub fn selected_subject_count(&self) -> usize {
        self.scope.subjects.len()
    }
    pub fn site(&self, node: FlowNodeId) -> Result<SelectedFlowSite, FlowProductKeyError> {
        Ok(SelectedFlowSite {
            scope: Arc::clone(&self.scope),
            subject: self.scope.subject_at(node)?,
        })
    }
    pub fn key(
        &self,
        domain: FlowDomain,
        node: FlowNodeId,
    ) -> Result<FlowProductKey, FlowProductKeyError> {
        self.scope.key(domain, self.scope.subject_at(node)?)
    }
    pub fn key_for_binding(
        &self,
        domain: FlowDomain,
        binding: &FlowBindingRef,
    ) -> Result<FlowProductKey, FlowProductKeyError> {
        match binding {
            FlowBindingRef::Local(local) => {
                let node = self.scope.inputs.graph.binding_node(*local);
                self.key(domain, node)
            }
            FlowBindingRef::Captured(identity) => {
                let subject = self
                    .scope
                    .captures
                    .get(identity)
                    .ok_or(FlowProductKeyError::UnmodeledBinding)?;
                self.scope.key(domain, *subject)
            }
        }
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
        if !Arc::ptr_eq(&self.scope, &state.scope) || !Arc::ptr_eq(&self.scope, &site.scope) {
            return self.reject(FlowProductFailure::ScopeMismatch);
        }
        let mut staged: smallvec::SmallVec<[(usize, Option<FlowProductValue>); 5]> =
            smallvec::SmallVec::new();
        let mut active = state.active;
        for transfer in transfers {
            if !Arc::ptr_eq(&self.scope, &transfer.key.scope) {
                return self.reject(FlowProductFailure::ScopeMismatch);
            }
            let slot = transfer.key.storage();
            let prior = staged
                .iter()
                .find(|(held, _)| *held == slot)
                .map(|(_, value)| value)
                .unwrap_or(&state.values[slot]);
            let value = match transfer_product(
                &self.budget,
                &transfer.key,
                prior.as_ref(),
                transfer.value.as_ref(),
            ) {
                Ok(value) => value,
                Err(failure) => return self.reject(failure),
            };
            active = active - usize::from(prior.is_some()) + usize::from(value.is_some());
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
        let mut changed = false;
        for (slot, value) in staged {
            changed |= state.values[slot] != value;
            state.values[slot] = value;
        }
        state.active = active;
        for transfer in transfers {
            self.executed[transfer.key.evidence()] = true;
            self.executed[site.subject * PRODUCT_DOMAINS.len() + transfer.key.offset] = true;
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
        for state in incoming {
            if !Arc::ptr_eq(&self.scope, &state.scope) {
                return self.reject(FlowProductFailure::ScopeMismatch);
            }
        }
        let mut result = self.empty_state();
        for (slot, held) in result.values.iter_mut().enumerate() {
            let domain = PRODUCT_DOMAINS[slot % PRODUCT_DOMAINS.len()];
            if incoming.iter().all(|state| state.values[slot].is_none()) {
                continue;
            }
            let mut predecessors = incoming.iter();
            let bottom = FlowProductValue::bottom(domain).expect("a product domain");
            let mut value = predecessors
                .next()
                .and_then(|state| state.values[slot].as_ref())
                .unwrap_or(&bottom)
                .clone();
            for state in predecessors {
                let next = state.values[slot].as_ref().unwrap_or(&bottom);
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
            if let Some(exceeded) = width_exceeded(&self.budget, value.width()) {
                return self.reject(FlowProductFailure::BudgetExceeded(exceeded));
            }
            *held = Some(value);
            result.active += 1;
        }
        if result.active > self.budget.max_products as usize {
            return self.reject(FlowProductFailure::BudgetExceeded(
                FlowProductBudgetExceeded {
                    axis: FlowProductBudgetAxis::Products,
                    limit: self.budget.max_products,
                    observed: u32::try_from(result.active).unwrap_or(u32::MAX),
                },
            ));
        }
        Ok(result)
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
    pub fn finish(&mut self) -> Result<FlowProductEvidence, FlowProductFailure> {
        self.ready()?;
        self.sealed = true;
        Ok(FlowProductEvidence {
            scope: Arc::clone(&self.scope),
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
                .is_some_and(|scope| !Arc::ptr_eq(scope, &key.scope))
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
                if held.declared().is_some() && new.declared().is_some() && held != new {
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
                let identity = key
                    .scope
                    .inputs
                    .bindings
                    .as_ref()
                    .and_then(|map| {
                        map.local(&fact.binding)
                            .and_then(|local| map.runtime_identity(local))
                    })
                    .unwrap_or(&fact.binding);
                Some(identity) != runtime
            }) {
                return Err(FlowProductFailure::Gap(FlowGap::UnmodeledExpression));
            }
            if let Some(exceeded) = width_exceeded(budget, value.width()) {
                return Err(FlowProductFailure::BudgetExceeded(exceeded));
            }
            return Ok(Some(FlowProductValue::Narrowing(NarrowingProduct::new(
                product.facts().iter().cloned().map(|mut fact| {
                    fact.binding = runtime.expect("a fact has a binding").clone();
                    fact
                }),
            ))));
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
    Ok(Some(value.clone()))
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

/// The canonical semantic-type algebra a product join constructs every
/// semantic composite through. This substrate never assembles a union or
/// an intersection itself; the sole production implementor forwards to the
/// dispatch's canonical union authority, which deposits the construction's
/// evidence on the dispatch's own rails.
pub trait FlowSemanticAlgebra {
    /// The canonical union of `members`.
    fn union(&self, members: &[SemanticNodeId]) -> FlowAlgebraComposite;
    /// Top-level literal constituents, with canonical-owner inspection evidence.
    fn literal_arms(
        &self,
        node: SemanticNodeId,
    ) -> Result<Vec<(SemanticNodeId, crate::semantic_query::LiteralValue)>, FlowGap>;
}

impl FlowSemanticAlgebra for super::ProjectSemanticDispatch<'_> {
    fn literal_arms(
        &self,
        node: SemanticNodeId,
    ) -> Result<Vec<(SemanticNodeId, crate::semantic_query::LiteralValue)>, FlowGap> {
        let (arms, evidence) = super::canonical_algebra::inspect_literal_arms(self.graph(), node);
        let incomplete = evidence.incomplete;
        self.deposit_canonical_evidence(evidence);
        if incomplete {
            Err(FlowGap::UnmodeledExpression)
        } else {
            Ok(arms)
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
    fn literal_arms(
        &self,
        node: SemanticNodeId,
    ) -> Result<Vec<(SemanticNodeId, crate::semantic_query::LiteralValue)>, FlowGap> {
        let (arms, evidence) = super::canonical_algebra::inspect_literal_arms(self.0, node);
        if evidence.incomplete {
            Err(FlowGap::UnmodeledExpression)
        } else {
            Ok(arms)
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
    scope: Option<Arc<ProductLayout>>,
}

impl PartialEq for ReachingValueProduct {
    fn eq(&self, other: &Self) -> bool {
        self.definitions == other.definitions
            && match (&self.scope, &other.scope) {
                (None, None) => true,
                (Some(a), Some(b)) => Arc::ptr_eq(a, b),
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
            scope: Some(Arc::clone(&site.scope)),
        }
    }

    fn merged(
        definitions: impl IntoIterator<Item = FlowNodeId>,
        scope: Option<Arc<ProductLayout>>,
    ) -> Self {
        let mut sites: Vec<FlowNodeId> = definitions.into_iter().collect();
        sites.sort_by_key(|node| node.index());
        sites.dedup();
        Self {
            definitions: Arc::from(sites.into_boxed_slice()),
            scope,
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
    /// Maximum materialized products in one snapshot. Empty selected
    /// capacity is separately bounded by the sealed plan's resource policy.
    pub max_products: u32,
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
            max_product_width: 64,
        }
    }
}

impl FlowProductBudget {
    /// Product capacity is connected to the selected work and obligation
    /// frontier. Unread signature parameters add neither work nor slots.
    #[must_use]
    pub fn for_demand_plan(plan: &FlowDemandPlan) -> Self {
        let selection = plan.structural_selection();
        let subjects = selection
            .value_nodes
            .len()
            .saturating_add(selection.effect_only_nodes.len())
            .saturating_add(plan.work_order().len());
        Self {
            max_iterations: plan.convergence().max_iterations,
            max_products: u32::try_from(subjects)
                .unwrap_or(u32::MAX)
                .saturating_mul(PRODUCT_DOMAINS.len() as u32),
            max_product_width: plan.resources().slice_budget.max_selected_nodes,
        }
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
        FlowDomain::ReachingValue => {
            let (FlowProductValue::ReachingValue(left), FlowProductValue::ReachingValue(right)) =
                (a, b)
            else {
                return FlowTransferOutcome::Gap(FlowGap::UnmodeledExpression);
            };
            if let (Some(a), Some(b)) = (&left.scope, &right.scope) {
                if !Arc::ptr_eq(a, b) {
                    return FlowTransferOutcome::Gap(FlowGap::UnmodeledExpression);
                }
            }
            let product = ReachingValueProduct::merged(
                left.definitions()
                    .iter()
                    .chain(right.definitions().iter())
                    .copied(),
                left.scope.as_ref().or(right.scope.as_ref()).cloned(),
            );
            if let Some(exceeded) = width_exceeded(budget, product.definitions().len()) {
                return FlowTransferOutcome::BudgetExceeded(exceeded);
            }
            FlowProductValue::ReachingValue(product)
        }
        FlowDomain::ReachingType => {
            let (FlowProductValue::ReachingType(left), FlowProductValue::ReachingType(right)) =
                (a, b)
            else {
                return FlowTransferOutcome::Gap(FlowGap::UnmodeledExpression);
            };
            let mut contributors: Vec<SemanticNodeId> = left
                .contributors()
                .iter()
                .chain(right.contributors().iter())
                .copied()
                .collect();
            let mut seen = rustc_hash::FxHashSet::default();
            contributors.retain(|node| seen.insert(*node));
            if let Some(exceeded) = width_exceeded(budget, contributors.len()) {
                return FlowTransferOutcome::BudgetExceeded(exceeded);
            }
            // The product-state algebra ends here: the SEMANTIC composite
            // over the aggregated contributors is constructed by the
            // canonical algebra, never assembled in this substrate.
            let united = match contributors.as_slice() {
                [] => None,
                [single] => Some(*single),
                members => {
                    let composite = algebra.union(members);
                    if composite.incomplete {
                        return FlowTransferOutcome::Gap(FlowGap::UnmodeledExpression);
                    }
                    Some(composite.node)
                }
            };
            let widening = match join_widening(algebra, left, right, united) {
                Ok(widening) => widening,
                Err(gap) => return FlowTransferOutcome::Gap(gap),
            };
            FlowProductValue::ReachingType(ReachingTypeProduct {
                contributors: Arc::from(contributors.into_boxed_slice()),
                united,
                widening,
            })
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

fn same_literal(
    a: &crate::semantic_query::LiteralValue,
    b: &crate::semantic_query::LiteralValue,
) -> bool {
    match (a, b) {
        (
            crate::semantic_query::LiteralValue::Number(a),
            crate::semantic_query::LiteralValue::Number(b),
        ) => !super::canonical_algebra::numeric_literal_values_disjoint(*a, *b),
        _ => a == b,
    }
}

/// Freshness joins by literal value with pinned occurrences winning. Result
/// membership names only surviving canonical literal arms, never an absorbed
/// input arm or a discarded scoped representative.
fn join_widening(
    algebra: &dyn FlowSemanticAlgebra,
    a: &ReachingTypeProduct,
    b: &ReachingTypeProduct,
    result: Option<SemanticNodeId>,
) -> Result<Option<WideningMembership>, FlowGap> {
    if a.widening.is_none() && b.widening.is_none() {
        return Ok(None);
    }
    let Some(result) = result else {
        return Ok(None);
    };
    let mut fresh = Vec::new();
    let mut pinned = Vec::new();
    for product in [a, b] {
        let Some(node) = product.united else {
            continue;
        };
        let arms = algebra.literal_arms(node)?;
        let partial = match product.widening() {
            Some(WideningMembership::Partial(members)) => {
                let mut values = Vec::new();
                for member in members.iter() {
                    values.extend(
                        algebra
                            .literal_arms(*member)?
                            .into_iter()
                            .map(|(_, value)| value),
                    );
                }
                values
            }
            _ => Vec::new(),
        };
        for (_, value) in arms {
            let is_fresh = matches!(product.widening(), Some(WideningMembership::All))
                || partial.iter().any(|member| same_literal(&value, member));
            if is_fresh {
                fresh.push(value);
            } else {
                pinned.push(value);
            }
        }
    }
    let surviving: Vec<_> = algebra
        .literal_arms(result)?
        .into_iter()
        .filter(|(_, value)| {
            fresh.iter().any(|candidate| same_literal(value, candidate))
                && !pinned
                    .iter()
                    .any(|candidate| same_literal(value, candidate))
        })
        .map(|(node, _)| node)
        .collect();
    if surviving.is_empty() {
        Ok(None)
    } else {
        Ok(Some(WideningMembership::Partial(surviving.into())))
    }
}
