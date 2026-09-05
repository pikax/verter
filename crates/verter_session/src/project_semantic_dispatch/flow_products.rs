//! The product lattice of the shared flow authority: the per-binding,
//! per-domain dataflow PRODUCTS a flow-bearing operation computes over one
//! store-bound [`FunctionFlowGraph`], plus the ONE transfer route, the ONE
//! join route, and the ONE deterministic worklist that drive them to a
//! fixed point.
//!
//! The layer owns the live value path: the flow evaluator holds its whole
//! semantic state in a [`FlowProductStore`] over the frame subject
//! vocabulary minted here, and every merge point folds through the
//! per-domain joins. Nothing here resolves a type, opens a file, reaches a
//! store view, or dispatches a query — the substrate is PURE over its
//! inputs, and the semantic content of a product is supplied by its
//! producer.
//!
//! Ownership boundaries, all load-bearing:
//!
//! - **Product-state algebra is owned here; semantic type algebra is
//!   not.** A join that must produce a semantic composite aggregates its
//!   flow-domain contributors and asks the canonical algebra
//!   ([`FlowSemanticAlgebra`], whose sole production implementor forwards
//!   to the dispatch's canonical union authority) to construct the
//!   result. There is no flow-private union or intersection reducer.
//! - **One domain registry.** The domains are the closed
//!   [`FlowDomain`] registry; there is no second domain enum.
//!   [`flow_product_kind`] is the total, wildcard-free projection of that
//!   registry onto the domains this substrate carries a product for — a
//!   domain with no product is a typed `None`, never a fallthrough.
//! - **One store, no public product query.** [`FlowProductStore`] is the
//!   only product storage — the graph solve reaches a populated one only
//!   through a converged solve, and the evaluator reaches its own through
//!   the frame subject mints. There is no second store and no standalone
//!   product query API.
//! - **A degraded outcome retains nothing.** [`FlowTransferOutcome`]'s
//!   `Gap` and `BudgetExceeded` arms carry NO [`FlowProductValue`], so a
//!   gapped or budget-exhausted step has nothing a store could admit, and
//!   the solve returns the degraded arm WITHOUT its partially-populated
//!   store. Warmability is structurally unreachable, not policed.
//! - **Binding subjects carry stable cross-frame identity.** A binding
//!   node's key mints ONLY with the frame's resolved
//!   [`FlowBindingIdentity`], resolved through the demand planner's own
//!   single slot-numbering authority; a binding the frame's inventory
//!   cannot name is a typed key error, never a fabricated slot.
//!
//! Determinism is structural rather than incidental: the worklist is an
//! ORDERED ready set keyed by `(domain rank, node index)`, so equivalent
//! insertion orders produce one visitation order; every product's carrier
//! is a canonical (sorted, deduplicated) set; and the requested domain
//! list is canonicalized before the solve starts. The solution encodes to
//! canonical bytes, so "same answer" is byte-checkable rather than
//! field-by-field.

// The graph-side worklist and the canonical solution encoding are the
// substrate's own surface, exercised by the flow product suites; the frame
// side is the evaluator's live state.
#![allow(dead_code)]

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use rustc_hash::FxHashMap;
use verter_identity::encoding::{CanonicalEncode, CanonicalEncoder};
use verter_semantic::analysis::flow::flow_graph::{
    FlowEdgeClass, FlowNodeId, FlowNodeKind, FunctionFlowGraph,
};
use verter_semantic::analysis::function_program::{FlowBindingIdentity, FunctionBindingKind};

use super::flow_solve::{FlowBindingInventory, FlowDomain};
use crate::semantic_query::{FlowGap, SemanticNodeId};

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
}

impl FlowSemanticAlgebra for super::ProjectSemanticDispatch<'_> {
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
    fn union(&self, members: &[SemanticNodeId]) -> FlowAlgebraComposite {
        let composite = super::canonical_algebra::canonical_union(self.0, members);
        FlowAlgebraComposite {
            node: composite.node,
            incomplete: composite.evidence.incomplete,
        }
    }
}

// ── The product vocabulary ─────────────────────────────────────────────

/// The product kind one flow domain carries in this substrate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum FlowProductKind {
    /// Reaching definitions: which graph sites provide the subject's value.
    ReachingValue,
    /// Reaching types: the semantic contributors reaching the subject.
    ReachingType,
    /// The subject's declared (annotation) type.
    DeclaredType,
    /// The guard facts that hold at the subject.
    Narrowing,
    /// The subject's definite-assignment state.
    DefiniteAssignment,
}

/// The product kind of `domain`, or `None` when the closed domain registry
/// declares a domain this substrate carries no product for. TOTAL over the
/// registry by construction — a wildcard-free match, so a new registry
/// domain must decide its product route deliberately.
#[rustfmt::skip]
pub const fn flow_product_kind(domain: FlowDomain) -> Option<FlowProductKind> {
    match domain {
        FlowDomain::ReachingValue => Some(FlowProductKind::ReachingValue),
        FlowDomain::ReachingType => Some(FlowProductKind::ReachingType),
        FlowDomain::DeclaredType => Some(FlowProductKind::DeclaredType),
        FlowDomain::Narrowing => Some(FlowProductKind::Narrowing),
        FlowDomain::DefiniteAssignment => Some(FlowProductKind::DefiniteAssignment),
        // Declared by the registry, carried by no product lattice: these
        // domains discharge on evidence, not on a lattice value.
        FlowDomain::Completion | FlowDomain::ClosureCapture | FlowDomain::Freshness
        | FlowDomain::Effects | FlowDomain::CallResolution | FlowDomain::Relation
        | FlowDomain::ContextualTyping | FlowDomain::Coverage => None,
    }
}

/// The edge classes a product kind propagates along. Value products follow
/// the value-provider families; narrowing follows control-region
/// membership, because a guard fact is established by the region a site
/// belongs to. TOTAL over the product vocabulary.
#[rustfmt::skip]
const fn product_edge_classes(kind: FlowProductKind) -> &'static [FlowEdgeClass] {
    match kind {
        FlowProductKind::ReachingValue => &[FlowEdgeClass::ValueDef, FlowEdgeClass::PathWrite],
        FlowProductKind::ReachingType => &[FlowEdgeClass::ValueDef],
        FlowProductKind::DeclaredType => &[FlowEdgeClass::ValueDef],
        FlowProductKind::Narrowing => &[FlowEdgeClass::ControlRegion],
        FlowProductKind::DefiniteAssignment => &[FlowEdgeClass::ValueDef, FlowEdgeClass::PathWrite],
    }
}

/// Reaching definitions: the canonical SET of graph sites that provide the
/// subject's value. The carrier is sorted by node index and deduplicated
/// at construction, so two equal definition sets are one value and a join
/// cannot depend on contributor arrival order.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ReachingValueProduct {
    definitions: Arc<[FlowNodeId]>,
}

impl ReachingValueProduct {
    /// The canonical product over `definitions` (sorted, deduplicated).
    #[must_use]
    pub fn new(definitions: impl IntoIterator<Item = FlowNodeId>) -> Self {
        let mut sites: Vec<FlowNodeId> = definitions.into_iter().collect();
        sites.sort_by_key(|node| node.index());
        sites.dedup();
        Self {
            definitions: Arc::from(sites.into_boxed_slice()),
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

/// Reaching types: the SET of semantic contributors reaching the subject
/// in first-contribution order, the composite the CANONICAL ALGEBRA
/// constructed from exactly that set, and the literal-widening membership
/// those values carry. The substrate owns the contributor set; it never
/// owns the composite's construction.
///
/// Contributor order is FIRST CONTRIBUTION, deduplicated — not a sort.
/// The order is semantic rather than cosmetic: it is the arm order of the
/// union the canonical algebra constructs from the set, and every
/// contribution site feeds the set in one deterministic order, so
/// equivalent inputs still produce one product.
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

    /// The same product carrying `widening` as its literal-widening
    /// membership.
    #[must_use]
    pub fn with_widening(mut self, widening: Option<WideningMembership>) -> Self {
        self.widening = widening;
        self
    }

    /// A product carrying ONLY a widening membership — no reaching value.
    /// The shape a captured frame inherits when the enclosing frame's
    /// membership survives a boundary its value does not.
    #[must_use]
    pub fn widening_only(widening: WideningMembership) -> Self {
        Self {
            contributors: Arc::from([]),
            united: None,
            widening: Some(widening),
        }
    }

    /// The contributor set, in first-contribution order.
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

    /// The literal-widening membership the reaching values carry.
    #[must_use]
    pub fn widening(&self) -> Option<&WideningMembership> {
        self.widening.as_ref()
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

/// One guard fact: a subject — or an authored member path under it —
/// narrowed to a semantic type. The PATH is what lets the one narrowing
/// product carry a guard on `x.a.b` as well as one on `x`; an empty path
/// is the subject itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlowNarrowingFact {
    /// The narrowed subject.
    pub subject: FlowProductSubject,
    /// The authored member path under the subject the guard narrowed.
    pub path: Arc<[Arc<str>]>,
    /// The type the guard narrows it to.
    pub narrowed_to: SemanticNodeId,
}

/// The canonical ordering key of one guard fact: the subject's canonical
/// order, then the authored path, then the narrowed node. Ordering is
/// explicit rather than derived because [`FlowBindingIdentity`] carries a
/// frame key whose ordering is not a product-state concern.
fn narrowing_order(fact: &FlowNarrowingFact) -> (u32, u32, u32, &str, Vec<&str>, u64) {
    let (space, ordinal, slot, name) = match &fact.subject {
        FlowProductSubject::GraphNode { node, binding } => (
            1u32,
            u32::try_from(node.index()).unwrap_or(u32::MAX),
            binding.as_ref().map_or(0, |b| b.binding_slot + 1),
            binding.as_ref().map_or("", |b| b.name.as_ref()),
        ),
        FlowProductSubject::FrameBinding(slot) => (2, slot.0, 0, ""),
        FlowProductSubject::FrameParam(ordinal) => (3, *ordinal, 0, ""),
    };
    (
        space,
        ordinal,
        slot,
        name,
        fact.path.iter().map(Arc::as_ref).collect(),
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

    /// The product's definite-assignment lattice point.
    #[must_use]
    pub fn state(self) -> DefiniteAssignment {
        self.state
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

/// One product value. Exactly one arm per [`FlowProductKind`]; the store
/// refuses a value whose arm does not match its key's domain.
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
    /// The value's product kind.
    #[must_use]
    pub fn kind(&self) -> FlowProductKind {
        match self {
            Self::ReachingValue(_) => FlowProductKind::ReachingValue,
            Self::ReachingType(_) => FlowProductKind::ReachingType,
            Self::DeclaredType(_) => FlowProductKind::DeclaredType,
            Self::Narrowing(_) => FlowProductKind::Narrowing,
            Self::DefiniteAssignment(_) => FlowProductKind::DefiniteAssignment,
        }
    }

    /// The kind's bottom element — the value at a subject no edge has
    /// reached yet. TOTAL over the product vocabulary.
    #[must_use]
    pub fn bottom(kind: FlowProductKind) -> Self {
        match kind {
            FlowProductKind::ReachingValue => Self::ReachingValue(ReachingValueProduct::default()),
            FlowProductKind::ReachingType => Self::ReachingType(ReachingTypeProduct::default()),
            FlowProductKind::DeclaredType => Self::DeclaredType(DeclaredTypeProduct::default()),
            FlowProductKind::Narrowing => Self::Narrowing(NarrowingProduct::default()),
            FlowProductKind::DefiniteAssignment => {
                Self::DefiniteAssignment(DefiniteAssignmentProduct::default())
            }
        }
    }
}

// ── Keys and subjects ──────────────────────────────────────────────────

/// Why a product key could not be minted: the node is outside the bound
/// graph's index space, the graph node is a binding the frame's inventory
/// cannot name (so no stable cross-frame identity exists), or the domain
/// carries no product in this substrate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlowProductKeyError {
    /// The node index is outside the bound graph.
    NodeOutOfRange,
    /// A binding node with no stable cross-frame identity.
    UnmodeledBinding,
    /// A registry domain this substrate carries no product for.
    DomainCarriesNoProduct,
}

/// The scope layer one frame binding lives in — the split the evaluator's
/// two former local layers expressed as two parallel name maps. `Var`
/// hoists to function scope and survives a block restore; `const` / `let`
/// are lexical.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum FlowBindingLayer {
    /// Block-scoped (`const` / `let`).
    Lexical,
    /// Function-scoped (`var`, and the hoisted seeds).
    Function,
}

/// One frame binding slot: the evaluator's own stable subject identity for
/// a binding of the frame it evaluates.
///
/// The field is private and the sole mint is
/// [`FlowFrameBindings::slot`], so a slot always names a binding that
/// authority resolved — a number cast into a subject is unrepresentable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct FlowFrameSlot(u32);

impl FlowFrameSlot {
    /// The slot's ordinal in its frame's binding table.
    #[must_use]
    pub fn index(self) -> u32 {
        self.0
    }
}

/// What one product is held FOR. Two disjoint subject spaces, each with
/// exactly one sealed mint, so a graph subject and a frame subject can
/// never alias:
///
/// - [`Self::GraphNode`] is a node of one bound [`FunctionFlowGraph`],
///   carrying the resolved stable cross-frame identity of a binding node.
///   Minted ONLY by [`FlowProductInputs::key`].
/// - [`Self::FrameBinding`] is a binding of the frame an evaluation is
///   walking, by its resolved slot. Minted ONLY by
///   [`FlowFrameBindings::slot`].
/// - [`Self::FrameParam`] is a formal parameter of that frame, by ordinal.
///   Minted ONLY by [`FlowFrameBindings::param`].
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum FlowProductSubject {
    /// A node of the bound graph, with a binding node's resolved identity.
    GraphNode {
        /// The graph node.
        node: FlowNodeId,
        /// The binding's stable cross-frame identity, for a binding node.
        binding: Option<FlowBindingIdentity>,
    },
    /// A frame binding, by its resolved slot.
    FrameBinding(FlowFrameSlot),
    /// A formal parameter of the frame, by ordinal.
    FrameParam(u32),
}

/// One product slot: a flow domain over one subject. A BINDING subject
/// carries stable identity (the graph's cross-frame identity, or the
/// frame's own resolved slot), so two same-named bindings of different
/// frames, different slots, or different scope layers are different slots
/// and can never alias.
///
/// Fields are private and the only constructors are
/// [`FlowProductInputs::key`] and [`FlowFrameBindings`]'s mints: a key
/// naming a binding node WITHOUT its resolved identity, a key over a node
/// outside the graph, a key over an unresolved frame slot, and a key on a
/// productless domain are all unrepresentable rather than rejected later.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FlowProductKey {
    domain: FlowDomain,
    subject: FlowProductSubject,
}

impl FlowProductKey {
    /// The key's flow domain.
    #[must_use]
    pub fn domain(&self) -> FlowDomain {
        self.domain
    }

    /// The key's product kind (total: a key mints only on a product domain).
    #[must_use]
    pub fn kind(&self) -> FlowProductKind {
        flow_product_kind(self.domain).expect("a product key mints only on a product domain")
    }

    /// The subject the slot is held for.
    #[must_use]
    pub fn subject(&self) -> &FlowProductSubject {
        &self.subject
    }

    /// The graph node the slot is anchored at, for a graph subject.
    #[must_use]
    pub fn node(&self) -> Option<FlowNodeId> {
        match &self.subject {
            FlowProductSubject::GraphNode { node, .. } => Some(*node),
            FlowProductSubject::FrameBinding(_) | FlowProductSubject::FrameParam(_) => None,
        }
    }

    /// The subject's stable cross-frame binding identity, for a binding node.
    #[must_use]
    pub fn binding(&self) -> Option<&FlowBindingIdentity> {
        match &self.subject {
            FlowProductSubject::GraphNode { binding, .. } => binding.as_ref(),
            FlowProductSubject::FrameBinding(_) | FlowProductSubject::FrameParam(_) => None,
        }
    }
}

/// The frame's binding-NAME resolution authority — the one place an
/// authored name of the frame under evaluation becomes a stable
/// [`FlowProductSubject`], and the sole mint of a frame product key.
///
/// This table holds NAMES, never semantic state: the frame's semantic
/// state lives in [`FlowProductStore`], keyed by the typed subjects minted
/// here. Resolution is per (layer, name) and stable for the whole frame,
/// so the two former parallel local layers become two disjoint slot
/// spaces and a lexical binding can never read a function-scoped
/// binding's product by sharing its map key.
#[derive(Debug, Clone, Default)]
pub struct FlowFrameBindings {
    records: Vec<(Arc<str>, FlowBindingLayer)>,
    index: FxHashMap<(FlowBindingLayer, Arc<str>), FlowFrameSlot>,
}

impl FlowFrameBindings {
    /// An empty table.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The slot of `name` in `layer`, allocating one on first resolution.
    pub fn slot(&mut self, layer: FlowBindingLayer, name: &str) -> FlowFrameSlot {
        if let Some(slot) = self.index.get(&(layer, Arc::from(name))) {
            return *slot;
        }
        let name: Arc<str> = Arc::from(name);
        let slot = FlowFrameSlot(u32::try_from(self.records.len()).unwrap_or(u32::MAX));
        self.records.push((Arc::clone(&name), layer));
        self.index.insert((layer, name), slot);
        slot
    }

    /// The slot of `name` in `layer` when one was already resolved. A
    /// name no declaration or read ever resolved has no slot and thus no
    /// product — the read-side counterpart of [`Self::slot`], so a pure
    /// read never grows the table.
    #[must_use]
    pub fn resolved(&self, layer: FlowBindingLayer, name: &str) -> Option<FlowFrameSlot> {
        self.index.get(&(layer, Arc::from(name))).copied()
    }

    /// The subject of `name` in `layer`, allocating its slot.
    pub fn subject(&mut self, layer: FlowBindingLayer, name: &str) -> FlowProductSubject {
        FlowProductSubject::FrameBinding(self.slot(layer, name))
    }

    /// The subject of one formal parameter, by ordinal.
    #[must_use]
    pub fn param(ordinal: u32) -> FlowProductSubject {
        FlowProductSubject::FrameParam(ordinal)
    }

    /// The authored name of `slot`.
    #[must_use]
    pub fn name(&self, slot: FlowFrameSlot) -> Option<&Arc<str>> {
        self.records.get(slot.0 as usize).map(|(name, _)| name)
    }

    /// The scope layer of `slot`.
    #[must_use]
    pub fn layer(&self, slot: FlowFrameSlot) -> Option<FlowBindingLayer> {
        self.records.get(slot.0 as usize).map(|(_, layer)| *layer)
    }

    /// The number of resolved slots.
    #[must_use]
    pub fn len(&self) -> usize {
        self.records.len()
    }

    /// Whether the frame resolved no binding name at all.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }
}

/// The product key of `domain` at `subject` — the mint frame-subject
/// callers use. A productless domain is a typed error, exactly as it is
/// on the graph mint.
pub fn frame_product_key(
    domain: FlowDomain,
    subject: FlowProductSubject,
) -> Result<FlowProductKey, FlowProductKeyError> {
    if flow_product_kind(domain).is_none() {
        return Err(FlowProductKeyError::DomainCarriesNoProduct);
    }
    match &subject {
        FlowProductSubject::FrameBinding(_) | FlowProductSubject::FrameParam(_) => {
            Ok(FlowProductKey { domain, subject })
        }
        // A graph subject mints only through the bound-graph inputs, which
        // are the authority for its identity resolution.
        FlowProductSubject::GraphNode { .. } => Err(FlowProductKeyError::UnmodeledBinding),
    }
}

/// The pure inputs one product solve runs over: the bound graph and the
/// frame's resolved binding identities, in skeleton binding order. Built
/// ONCE from a store-bound graph plus the frame's binding inventory
/// through the demand planner's own binding-identity resolution — never a
/// second slot-numbering authority.
#[derive(Debug, Clone)]
pub struct FlowProductInputs {
    graph: Arc<FunctionFlowGraph>,
    identities: Arc<[Option<FlowBindingIdentity>]>,
}

impl FlowProductInputs {
    /// The inputs over a store-bound graph.
    pub(crate) fn for_bound_graph(
        bound: &crate::cache_runtime::flow_slice_node::BoundFlowGraph,
        inventory: &FlowBindingInventory,
    ) -> Self {
        let bundle = bound.bundle();
        let identities = super::flow_solve::resolve_binding_identities(
            &bundle.skeleton,
            inventory,
            &bound.key().function,
        );
        Self {
            graph: Arc::clone(&bundle.graph),
            identities: Arc::from(identities.into_boxed_slice()),
        }
    }

    /// The bound graph.
    #[must_use]
    pub fn graph(&self) -> &FunctionFlowGraph {
        &self.graph
    }

    /// The frame's resolved binding identities, in skeleton binding order.
    #[must_use]
    pub fn identities(&self) -> &[Option<FlowBindingIdentity>] {
        &self.identities
    }

    /// Mint the product key of `domain` at `node` — the SOLE key
    /// construction. A binding node resolves its stable identity here; a
    /// binding the frame cannot name is a typed error, never a key with a
    /// fabricated slot.
    pub fn key(
        &self,
        domain: FlowDomain,
        node: FlowNodeId,
    ) -> Result<FlowProductKey, FlowProductKeyError> {
        if flow_product_kind(domain).is_none() {
            return Err(FlowProductKeyError::DomainCarriesNoProduct);
        }
        if node.index() >= self.graph.node_count() {
            return Err(FlowProductKeyError::NodeOutOfRange);
        }
        let binding = match self.graph.node_kind(node) {
            FlowNodeKind::Binding(binding) => match self.identities.get(binding.index()) {
                Some(Some(identity)) => Some(identity.clone()),
                Some(None) | None => return Err(FlowProductKeyError::UnmodeledBinding),
            },
            FlowNodeKind::ExprSite(_) | FlowNodeKind::ReturnSite(_) | FlowNodeKind::Region(_) => {
                None
            }
        };
        Ok(FlowProductKey {
            domain,
            subject: FlowProductSubject::GraphNode { node, binding },
        })
    }
}

// ── Store, seeds, budget ───────────────────────────────────────────────

/// Why a product write was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlowProductStoreError {
    /// The value's product kind is not the key domain's product kind.
    KindMismatch {
        /// The kind the key's domain declares.
        expected: FlowProductKind,
        /// The kind the value carries.
        observed: FlowProductKind,
    },
}

/// The ONE product store: computed products keyed by [`FlowProductKey`].
///
/// The store's whole write surface is [`Self::insert`], which takes a
/// [`FlowProductValue`]. Neither degraded [`FlowTransferOutcome`] arm
/// carries one, so admitting a gapped or budget-exhausted step is
/// unrepresentable rather than policed; and a populated store escapes a
/// solve only on the converged arm.
#[derive(Debug, Clone, Default)]
pub struct FlowProductStore {
    entries: FxHashMap<FlowProductKey, FlowProductValue>,
}

impl FlowProductStore {
    /// An empty store.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The product at `key`.
    #[must_use]
    pub fn get(&self, key: &FlowProductKey) -> Option<&FlowProductValue> {
        self.entries.get(key)
    }

    /// Store `value` at `key`, returning whether the stored product moved.
    /// A value whose arm does not match the key's domain is refused.
    pub fn insert(
        &mut self,
        key: FlowProductKey,
        value: FlowProductValue,
    ) -> Result<bool, FlowProductStoreError> {
        let expected = key.kind();
        let observed = value.kind();
        if expected != observed {
            return Err(FlowProductStoreError::KindMismatch { expected, observed });
        }
        let moved = self.entries.get(&key) != Some(&value);
        self.entries.insert(key, value);
        Ok(moved)
    }

    /// The number of stored products.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the store holds no product.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Every stored product, in canonical key order.
    #[must_use]
    pub fn ordered_entries(&self) -> Vec<(&FlowProductKey, &FlowProductValue)> {
        let mut entries: Vec<(&FlowProductKey, &FlowProductValue)> = self.entries.iter().collect();
        entries.sort_by(|a, b| key_order(a.0).cmp(&key_order(b.0)));
        entries
    }
}

/// The node-local contribution of one product slot: the fact the graph
/// site itself establishes. The substrate never RESOLVES a contribution —
/// resolution is the shared type-resolution engine's job — so the seeds
/// are supplied by the demand's producer.
#[derive(Debug, Clone, Default)]
pub struct FlowProductSeeds {
    entries: FxHashMap<FlowProductKey, FlowProductValue>,
}

impl FlowProductSeeds {
    /// An empty seed table.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Seed `key` with `value`, refusing a value whose arm does not match
    /// the key's domain.
    pub fn insert(
        &mut self,
        key: FlowProductKey,
        value: FlowProductValue,
    ) -> Result<(), FlowProductStoreError> {
        let expected = key.kind();
        let observed = value.kind();
        if expected != observed {
            return Err(FlowProductStoreError::KindMismatch { expected, observed });
        }
        self.entries.insert(key, value);
        Ok(())
    }

    /// The seed at `key`.
    #[must_use]
    pub fn get(&self, key: &FlowProductKey) -> Option<&FlowProductValue> {
        self.entries.get(key)
    }
}

/// The axis a product solve exhausted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FlowProductBudgetAxis {
    /// Fixed-point iterations.
    Iterations,
    /// Total stored products.
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
    /// The observed value that exceeded it.
    pub observed: u32,
}

/// The resource policy one product solve runs under.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FlowProductBudget {
    /// The maximum number of fixed-point iterations. A solve that
    /// stabilizes WITHIN this many iterations completes; one that would
    /// need another iteration is budget-exhausted.
    pub max_iterations: u32,
    /// The maximum number of stored products.
    pub max_products: u32,
    /// The maximum element count of one product's carrier.
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

// ── Transfer ───────────────────────────────────────────────────────────

/// The pure context one transfer reads: the bound graph inputs plus the
/// producer-supplied seeds.
#[derive(Debug, Clone, Copy)]
pub struct FlowProductContext<'a> {
    inputs: &'a FlowProductInputs,
    seeds: &'a FlowProductSeeds,
}

impl<'a> FlowProductContext<'a> {
    /// The context over `inputs` and `seeds`.
    #[must_use]
    pub fn new(inputs: &'a FlowProductInputs, seeds: &'a FlowProductSeeds) -> Self {
        Self { inputs, seeds }
    }

    /// The bound graph inputs.
    #[must_use]
    pub fn inputs(&self) -> &'a FlowProductInputs {
        self.inputs
    }

    /// The producer-supplied seeds.
    #[must_use]
    pub fn seeds(&self) -> &'a FlowProductSeeds {
        self.seeds
    }
}

/// Apply the node-local effect of `key`'s graph site to `incoming` — the
/// ONE transfer route, exhaustive over the product vocabulary and
/// wildcard-free.
///
/// - **Reaching values / reaching types / definite assignment** are
///   gen-kill: a site that establishes the fact REPLACES what reached it;
///   a site that does not is transparent.
/// - **Declared types** are a declaration fact, not a path-dependent one:
///   the transfer merges the site's declaration into the incoming one and
///   a genuine conflict is a typed gap, never an invented merge.
/// - **Narrowing** accumulates the site's guard facts, and an ASSIGNMENT
///   to the key's own binding kills the facts that named it — a narrowing
///   does not survive a write to its subject.
pub fn transfer_product(
    ctx: &FlowProductContext<'_>,
    key: &FlowProductKey,
    incoming: &FlowProductValue,
) -> FlowTransferOutcome {
    let kind = key.kind();
    if incoming.kind() != kind {
        return FlowTransferOutcome::Gap(FlowGap::UnmodeledExpression);
    }
    let seed = ctx.seeds.get(key);
    if let Some(seed) = seed {
        if seed.kind() != kind {
            return FlowTransferOutcome::Gap(FlowGap::UnmodeledExpression);
        }
    }
    match kind {
        // Gen-kill: the site's own fact replaces what reached it.
        FlowProductKind::ReachingValue
        | FlowProductKind::ReachingType
        | FlowProductKind::DefiniteAssignment => match seed {
            None => FlowTransferOutcome::Unchanged,
            Some(seed) if seed == incoming => FlowTransferOutcome::Unchanged,
            Some(seed) => FlowTransferOutcome::Changed(seed.clone()),
        },
        // A declaration fact: merge, never overwrite; a conflict is typed.
        FlowProductKind::DeclaredType => {
            let (
                FlowProductValue::DeclaredType(incoming),
                Some(FlowProductValue::DeclaredType(seed)),
            ) = (incoming, seed)
            else {
                return FlowTransferOutcome::Unchanged;
            };
            match (incoming.declared, seed.declared) {
                (_, None) => FlowTransferOutcome::Unchanged,
                (None, Some(_)) => {
                    FlowTransferOutcome::Changed(FlowProductValue::DeclaredType(*seed))
                }
                (Some(held), Some(established)) if held == established => {
                    FlowTransferOutcome::Unchanged
                }
                (Some(_), Some(_)) => FlowTransferOutcome::Gap(FlowGap::UnmodeledExpression),
            }
        }
        // Guard facts accumulate; an assignment to the subject kills its own.
        FlowProductKind::Narrowing => {
            let FlowProductValue::Narrowing(incoming) = incoming else {
                return FlowTransferOutcome::Unchanged;
            };
            let subject = key.binding();
            let assigned_here = subject.is_some()
                && key.node().is_some_and(|node| {
                    ctx.inputs
                        .key(FlowDomain::ReachingValue, node)
                        .ok()
                        .is_some_and(|write| ctx.seeds.get(&write).is_some())
                });
            let mut facts: Vec<FlowNarrowingFact> = incoming
                .facts()
                .iter()
                .filter(|fact| {
                    !(assigned_here
                        && matches!(
                            &fact.subject,
                            FlowProductSubject::GraphNode { binding, .. }
                                if binding.as_ref() == subject
                        ))
                })
                .cloned()
                .collect();
            if let Some(FlowProductValue::Narrowing(seed)) = seed {
                facts.extend(seed.facts().iter().cloned());
            }
            let produced = NarrowingProduct::new(facts);
            if &produced == incoming {
                FlowTransferOutcome::Unchanged
            } else {
                FlowTransferOutcome::Changed(FlowProductValue::Narrowing(produced))
            }
        }
    }
}

// ── Join ───────────────────────────────────────────────────────────────

/// Join `a` and `b` at a merge point — the ONE join route, exhaustive over
/// the product vocabulary and domain-SPECIFIC by construction:
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
/// Every route is idempotent (`join(x, x)` is `Unchanged`) and
/// permutation-stable (the carriers are canonical sets, so joining the
/// same contributors in any order yields the same product).
pub fn join_product(
    algebra: &dyn FlowSemanticAlgebra,
    budget: &FlowProductBudget,
    a: &FlowProductValue,
    b: &FlowProductValue,
) -> FlowTransferOutcome {
    if a.kind() != b.kind() {
        return FlowTransferOutcome::Gap(FlowGap::UnmodeledExpression);
    }
    let joined = match (a, b) {
        (FlowProductValue::ReachingValue(left), FlowProductValue::ReachingValue(right)) => {
            let product = ReachingValueProduct::new(
                left.definitions()
                    .iter()
                    .chain(right.definitions().iter())
                    .copied(),
            );
            if let Some(exceeded) = width_exceeded(budget, product.definitions().len()) {
                return FlowTransferOutcome::BudgetExceeded(exceeded);
            }
            FlowProductValue::ReachingValue(product)
        }
        (FlowProductValue::ReachingType(left), FlowProductValue::ReachingType(right)) => {
            // First-contribution order, deduplicated: the contributor set
            // IS the arm list the canonical algebra unions, and arm order
            // is observable in the constructed composite.
            let mut contributors: Vec<SemanticNodeId> = Vec::new();
            for node in left
                .contributors()
                .iter()
                .chain(right.contributors().iter())
                .copied()
            {
                if !contributors.contains(&node) {
                    contributors.push(node);
                }
            }
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
            // Widening membership rides the values: the RIGHT contributor's
            // membership decides when both edges carry one, mirroring the
            // reaching-definition rule that the later contribution is the
            // one whose freshness the read observes.
            let widening = right.widening().or_else(|| left.widening()).cloned();
            FlowProductValue::ReachingType(ReachingTypeProduct {
                contributors: Arc::from(contributors.into_boxed_slice()),
                united,
                widening,
            })
        }
        (FlowProductValue::DeclaredType(left), FlowProductValue::DeclaredType(right)) => {
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
        (FlowProductValue::Narrowing(left), FlowProductValue::Narrowing(right)) => {
            FlowProductValue::Narrowing(NarrowingProduct::new(
                left.facts()
                    .iter()
                    .filter(|fact| right.facts().contains(fact))
                    .cloned(),
            ))
        }
        (
            FlowProductValue::DefiniteAssignment(left),
            FlowProductValue::DefiniteAssignment(right),
        ) => FlowProductValue::DefiniteAssignment(left.join(*right)),
        // Unreachable: the kinds were proven equal above. A typed gap
        // rather than a panic, so a future arm cannot fail loudly in a
        // shipped build.
        _ => return FlowTransferOutcome::Gap(FlowGap::UnmodeledExpression),
    };
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

// ── The deterministic worklist ─────────────────────────────────────────

/// The outcome of one product solve. Only [`Self::Converged`] carries a
/// store: a gapped, rejected, or budget-exhausted solve returns NOTHING a
/// caller could retain, warm, or publish.
#[derive(Debug, Clone)]
pub enum FlowProductSolveOutcome {
    /// The solve reached its fixed point within budget.
    Converged(FlowProductSolution),
    /// The solve could not be modelled; the typed gap says why. No store.
    Gap(FlowGap),
    /// The solve exhausted a budget axis. No store.
    BudgetExceeded(FlowProductBudgetExceeded),
    /// The solve's inputs could not mint a key. No store.
    Rejected(FlowProductKeyError),
}

impl FlowProductSolveOutcome {
    /// The converged solution, when the solve proved one.
    #[must_use]
    pub fn solution(&self) -> Option<&FlowProductSolution> {
        match self {
            Self::Converged(solution) => Some(solution),
            Self::Gap(_) | Self::BudgetExceeded(_) | Self::Rejected(_) => None,
        }
    }
}

/// A converged product solve: the products, the visitation order they were
/// computed in, and the iteration count the fixed point took.
#[derive(Debug, Clone)]
pub struct FlowProductSolution {
    store: FlowProductStore,
    visitation: Arc<[FlowProductKey]>,
    iterations: u32,
}

impl FlowProductSolution {
    /// The computed products.
    #[must_use]
    pub fn store(&self) -> &FlowProductStore {
        &self.store
    }

    /// The exact order the solve visited product slots in.
    #[must_use]
    pub fn visitation(&self) -> &[FlowProductKey] {
        &self.visitation
    }

    /// The number of fixed-point iterations the solve took.
    #[must_use]
    pub fn iterations(&self) -> u32 {
        self.iterations
    }
}

/// Drive `domains` to their fixed point over `ctx`'s bound graph — the ONE
/// product worklist.
///
/// Determinism is structural: `domains` is canonicalized (sorted by
/// registry rank, deduplicated) before the solve starts; the ready set is
/// an ORDERED set keyed by `(domain rank, node index)` rather than a
/// queue, so equivalent insertion orders drain in one order; and each
/// node's join folds its out-edge targets in ascending node order. A
/// caller therefore cannot influence the answer, the visitation order, or
/// the solution's canonical bytes by re-ordering its inputs.
///
/// The iteration budget is EXACT: a solve whose ready set empties within
/// `max_iterations` iterations converges; one that would need another
/// iteration returns [`FlowProductSolveOutcome::BudgetExceeded`] and its
/// partially-populated store is dropped unread.
pub fn solve_flow_products(
    ctx: &FlowProductContext<'_>,
    domains: &[FlowDomain],
    algebra: &dyn FlowSemanticAlgebra,
    budget: &FlowProductBudget,
) -> FlowProductSolveOutcome {
    let inputs = ctx.inputs;
    let graph = inputs.graph();

    // Canonical domain order: registry rank, deduplicated. A permuted
    // caller list is the same solve.
    let mut domains: Vec<FlowDomain> = domains.to_vec();
    domains.sort();
    domains.dedup();

    // The key universe, minted once. A binding the frame cannot name has
    // no stable identity, so the whole solve fails closed rather than
    // computing products over a fabricated slot.
    let node_count = graph.node_count();
    let mut keys: BTreeMap<(u32, u32), FlowProductKey> = BTreeMap::new();
    for (rank, domain) in domains.iter().enumerate() {
        let rank = u32::try_from(rank).unwrap_or(u32::MAX);
        for index in 0..node_count {
            let Some(node) = graph.node_at(index) else {
                return FlowProductSolveOutcome::Rejected(FlowProductKeyError::NodeOutOfRange);
            };
            match inputs.key(*domain, node) {
                Ok(key) => {
                    keys.insert((rank, u32::try_from(index).unwrap_or(u32::MAX)), key);
                }
                Err(FlowProductKeyError::UnmodeledBinding) => {
                    return FlowProductSolveOutcome::Gap(FlowGap::UnmodeledExpression)
                }
                Err(error) => return FlowProductSolveOutcome::Rejected(error),
            }
        }
    }

    // Predecessors: a product flows from a provider to the node that
    // depends on it, so a node whose product moved re-readies every node
    // holding an out-edge to it.
    let mut predecessors: Vec<Vec<u32>> = vec![Vec::new(); node_count];
    for edge in graph.edges() {
        let to = edge.to.index();
        if to < node_count {
            predecessors[to].push(u32::try_from(edge.from.index()).unwrap_or(u32::MAX));
        }
    }
    for list in &mut predecessors {
        list.sort_unstable();
        list.dedup();
    }

    let mut store = FlowProductStore::new();
    let mut visitation: Vec<FlowProductKey> = Vec::new();
    let mut ready: BTreeSet<(u32, u32)> = keys.keys().copied().collect();
    let mut iterations = 0u32;

    while !ready.is_empty() {
        if iterations == budget.max_iterations {
            return FlowProductSolveOutcome::BudgetExceeded(FlowProductBudgetExceeded {
                axis: FlowProductBudgetAxis::Iterations,
                limit: budget.max_iterations,
                observed: budget.max_iterations.saturating_add(1),
            });
        }
        iterations += 1;
        let round: Vec<(u32, u32)> = ready.iter().copied().collect();
        ready.clear();
        for slot in round {
            let key = keys
                .get(&slot)
                .expect("every ready slot was minted into the key universe")
                .clone();
            let kind = key.kind();
            visitation.push(key.clone());

            // Join the products of this node's out-edge targets, in
            // ascending target order.
            let classes = product_edge_classes(kind);
            let mut targets: Vec<u32> = graph
                .out_edges(
                    key.node()
                        .expect("the graph solve mints only graph-node keys"),
                )
                .iter()
                .filter(|edge| classes.contains(&edge.kind.class()))
                .map(|edge| u32::try_from(edge.to.index()).unwrap_or(u32::MAX))
                .collect();
            targets.sort_unstable();
            targets.dedup();
            let mut incoming: Option<FlowProductValue> = None;
            for target in targets {
                let Some(product) = keys.get(&(slot.0, target)).and_then(|key| store.get(key))
                else {
                    continue;
                };
                incoming = Some(match incoming {
                    None => product.clone(),
                    Some(held) => match join_product(algebra, budget, &held, product) {
                        FlowTransferOutcome::Unchanged => held,
                        FlowTransferOutcome::Changed(joined) => joined,
                        FlowTransferOutcome::Gap(gap) => return FlowProductSolveOutcome::Gap(gap),
                        FlowTransferOutcome::BudgetExceeded(exceeded) => {
                            return FlowProductSolveOutcome::BudgetExceeded(exceeded)
                        }
                    },
                });
            }
            let incoming = incoming.unwrap_or_else(|| FlowProductValue::bottom(kind));

            let outgoing = match transfer_product(ctx, &key, &incoming) {
                FlowTransferOutcome::Unchanged => incoming,
                FlowTransferOutcome::Changed(value) => value,
                FlowTransferOutcome::Gap(gap) => return FlowProductSolveOutcome::Gap(gap),
                FlowTransferOutcome::BudgetExceeded(exceeded) => {
                    return FlowProductSolveOutcome::BudgetExceeded(exceeded)
                }
            };
            let node = key
                .node()
                .expect("the graph solve mints only graph-node keys");
            let moved = match store.insert(key, outgoing) {
                Ok(moved) => moved,
                // A kind mismatch is unrepresentable here (the key's own
                // kind built every value), so a refusal is a typed gap
                // rather than a panic.
                Err(_) => return FlowProductSolveOutcome::Gap(FlowGap::UnmodeledExpression),
            };
            if store.len() > budget.max_products as usize {
                return FlowProductSolveOutcome::BudgetExceeded(FlowProductBudgetExceeded {
                    axis: FlowProductBudgetAxis::Products,
                    limit: budget.max_products,
                    observed: u32::try_from(store.len()).unwrap_or(u32::MAX),
                });
            }
            if moved {
                for predecessor in &predecessors[node.index()] {
                    if keys.contains_key(&(slot.0, *predecessor)) {
                        ready.insert((slot.0, *predecessor));
                    }
                }
            }
        }
    }

    FlowProductSolveOutcome::Converged(FlowProductSolution {
        store,
        visitation: Arc::from(visitation.into_boxed_slice()),
        iterations,
    })
}

// ── Canonical encoding ─────────────────────────────────────────────────

#[rustfmt::skip]
const fn domain_discriminant(domain: FlowDomain) -> u32 {
    match domain {
        FlowDomain::ReachingValue => 1, FlowDomain::ReachingType => 2, FlowDomain::Narrowing => 3,
        FlowDomain::Completion => 4, FlowDomain::ClosureCapture => 5, FlowDomain::Freshness => 6,
        FlowDomain::Effects => 7, FlowDomain::CallResolution => 8, FlowDomain::Relation => 9,
        FlowDomain::ContextualTyping => 10, FlowDomain::Coverage => 11,
        FlowDomain::DeclaredType => 12, FlowDomain::DefiniteAssignment => 13,
    }
}

#[rustfmt::skip]
const fn binding_kind_discriminant(kind: FunctionBindingKind) -> u32 {
    match kind {
        FunctionBindingKind::Param => 1, FunctionBindingKind::Const => 2,
        FunctionBindingKind::Let => 3, FunctionBindingKind::Var => 4,
        FunctionBindingKind::NestedFunction => 5,
    }
}

#[rustfmt::skip]
const fn assignment_discriminant(state: DefiniteAssignment) -> u32 {
    match state {
        DefiniteAssignment::Unassigned => 1, DefiniteAssignment::Assigned => 2,
        DefiniteAssignment::MaybeAssigned => 3,
    }
}

/// The subject-space rank: graph subjects first, then frame bindings,
/// then frame parameters. A total order over the disjoint spaces.
const fn subject_space(subject: &FlowProductSubject) -> u32 {
    match subject {
        FlowProductSubject::GraphNode { .. } => 1,
        FlowProductSubject::FrameBinding(_) => 2,
        FlowProductSubject::FrameParam(_) => 3,
    }
}

/// The canonical ordering key of one product key.
fn key_order(key: &FlowProductKey) -> (u32, u32, usize, u32, &str) {
    let space = subject_space(&key.subject);
    match &key.subject {
        FlowProductSubject::GraphNode { node, binding } => (
            domain_discriminant(key.domain),
            space,
            node.index(),
            binding.as_ref().map_or(0, |b| b.binding_slot + 1),
            binding.as_ref().map_or("", |b| b.name.as_ref()),
        ),
        FlowProductSubject::FrameBinding(slot) => (
            domain_discriminant(key.domain),
            space,
            slot.0 as usize,
            0,
            "",
        ),
        FlowProductSubject::FrameParam(ordinal) => (
            domain_discriminant(key.domain),
            space,
            *ordinal as usize,
            0,
            "",
        ),
    }
}

/// The canonical bytes of one product key.
fn key_bytes(key: &FlowProductKey) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(32);
    bytes.extend_from_slice(&domain_discriminant(key.domain).to_le_bytes());
    bytes.extend_from_slice(&subject_space(&key.subject).to_le_bytes());
    match &key.subject {
        FlowProductSubject::GraphNode { node, binding } => {
            bytes.extend_from_slice(&(node.index() as u64).to_le_bytes());
            match binding {
                None => bytes.push(0),
                Some(binding) => {
                    bytes.push(1);
                    bytes.extend_from_slice(&binding.binding_slot.to_le_bytes());
                    bytes.extend_from_slice(&binding_kind_discriminant(binding.kind).to_le_bytes());
                    bytes.extend_from_slice(&(binding.name.len() as u64).to_le_bytes());
                    bytes.extend_from_slice(binding.name.as_bytes());
                }
            }
        }
        FlowProductSubject::FrameBinding(slot) => {
            bytes.extend_from_slice(&u64::from(slot.0).to_le_bytes());
            bytes.push(0);
        }
        FlowProductSubject::FrameParam(ordinal) => {
            bytes.extend_from_slice(&u64::from(*ordinal).to_le_bytes());
            bytes.push(0);
        }
    }
    bytes
}

/// The canonical bytes of one product value.
fn value_bytes(value: &FlowProductValue) -> Vec<u8> {
    let mut bytes = Vec::new();
    match value {
        FlowProductValue::ReachingValue(product) => {
            bytes.push(1);
            bytes.extend_from_slice(&(product.definitions().len() as u64).to_le_bytes());
            for node in product.definitions() {
                bytes.extend_from_slice(&(node.index() as u64).to_le_bytes());
            }
        }
        FlowProductValue::ReachingType(product) => {
            bytes.push(2);
            bytes.extend_from_slice(&(product.contributors().len() as u64).to_le_bytes());
            for node in product.contributors() {
                bytes.extend_from_slice(&node.0.to_le_bytes());
            }
            match product.united() {
                None => bytes.push(0),
                Some(node) => {
                    bytes.push(1);
                    bytes.extend_from_slice(&node.0.to_le_bytes());
                }
            }
            match product.widening() {
                None => bytes.push(0),
                Some(WideningMembership::All) => bytes.push(1),
                Some(WideningMembership::Partial(values)) => {
                    bytes.push(2);
                    bytes.extend_from_slice(&(values.len() as u64).to_le_bytes());
                    for value in values.iter() {
                        bytes.extend_from_slice(&value.0.to_le_bytes());
                    }
                }
            }
        }
        FlowProductValue::DeclaredType(product) => {
            bytes.push(3);
            match product.declared() {
                None => bytes.push(0),
                Some(node) => {
                    bytes.push(1);
                    bytes.extend_from_slice(&node.0.to_le_bytes());
                }
            }
        }
        FlowProductValue::Narrowing(product) => {
            bytes.push(4);
            bytes.extend_from_slice(&(product.facts().len() as u64).to_le_bytes());
            for fact in product.facts() {
                let subject = FlowProductKey {
                    domain: FlowDomain::Narrowing,
                    subject: fact.subject.clone(),
                };
                let subject_bytes = key_bytes(&subject);
                bytes.extend_from_slice(&(subject_bytes.len() as u64).to_le_bytes());
                bytes.extend_from_slice(&subject_bytes);
                bytes.extend_from_slice(&(fact.path.len() as u64).to_le_bytes());
                for segment in fact.path.iter() {
                    bytes.extend_from_slice(&(segment.len() as u64).to_le_bytes());
                    bytes.extend_from_slice(segment.as_bytes());
                }
                bytes.extend_from_slice(&fact.narrowed_to.0.to_le_bytes());
            }
        }
        FlowProductValue::DefiniteAssignment(product) => {
            bytes.push(5);
            bytes.extend_from_slice(&assignment_discriminant(product.state()).to_le_bytes());
            bytes.push(u8::from(product.single_path()));
            bytes.push(u8::from(product.failed_initializer()));
        }
    }
    bytes
}

impl CanonicalEncode for FlowProductSolution {
    const DOMAIN_TAG: &'static str = "verter.session.flow.product_solution.v1";

    fn encode_fields(&self, e: &mut CanonicalEncoder) {
        e.field_u32(1, self.iterations);
        // The visitation ORDER is contract, so it encodes as an ordered
        // list rather than a set.
        let mut visitation = Vec::new();
        visitation.extend_from_slice(&(self.visitation.len() as u64).to_le_bytes());
        for key in self.visitation.iter() {
            let bytes = key_bytes(key);
            visitation.extend_from_slice(&(bytes.len() as u64).to_le_bytes());
            visitation.extend_from_slice(&bytes);
        }
        e.field_bytes(2, &visitation);
        let entries: Vec<(Vec<u8>, Vec<u8>)> = self
            .store
            .ordered_entries()
            .into_iter()
            .map(|(key, value)| (key_bytes(key), value_bytes(value)))
            .collect();
        let _ = e
            .field_sorted_map(3, entries)
            .expect("product keys are unique in one store");
    }
}

// ── The frame product environment ──────────────────────────────────────

impl FlowProductBudget {
    /// The product budget of one demand plan — CONNECTED to the plan's own
    /// policies, never a private constant:
    ///
    /// - `max_iterations` IS the plan's [`FlowConvergencePolicy`];
    /// - `max_products` IS the plan's selected obligation frontier, one
    ///   slot per frame domain (an obligation's subject carries at most
    ///   one product per domain, so a frame that accumulates more slots
    ///   than that has left the plan's own work universe);
    /// - `max_product_width` IS the slice budget's selected-node ceiling
    ///   (a subject cannot accumulate more contributors than the slice
    ///   selected sites to contribute them).
    #[must_use]
    pub fn for_demand_plan(plan: &super::flow_solve::FlowDemandPlan) -> Self {
        Self {
            max_iterations: plan.convergence().max_iterations,
            max_products: u32::try_from(plan.work_order().len())
                .unwrap_or(u32::MAX)
                .saturating_mul(FLOW_FRAME_DOMAINS.len() as u32),
            max_product_width: plan.resources().slice_budget.max_selected_nodes,
        }
    }
}

/// The frame-subject product accessors: the ONE typed read/write surface
/// the flow evaluator holds its semantic state through. Every accessor
/// mints its key through [`frame_product_key`], so a frame subject can
/// never be stored under a graph slot and vice versa.
impl FlowProductStore {
    fn frame_slot(domain: FlowDomain, subject: &FlowProductSubject) -> FlowProductKey {
        frame_product_key(domain, subject.clone())
            .expect("the frame domains all carry products and frame subjects are resolved")
    }

    /// Remove the product of `domain` at `subject`, if any.
    pub fn remove(&mut self, domain: FlowDomain, subject: &FlowProductSubject) {
        self.entries.remove(&Self::frame_slot(domain, subject));
    }

    /// The reaching-type product of `subject`.
    #[must_use]
    pub fn reaching_type(&self, subject: &FlowProductSubject) -> Option<&ReachingTypeProduct> {
        match self
            .entries
            .get(&Self::frame_slot(FlowDomain::ReachingType, subject))
        {
            Some(FlowProductValue::ReachingType(product)) => Some(product),
            _ => None,
        }
    }

    /// The subject's reaching type — the value a read observes.
    #[must_use]
    pub fn reaching(&self, subject: &FlowProductSubject) -> Option<SemanticNodeId> {
        self.reaching_type(subject)
            .and_then(ReachingTypeProduct::united)
    }

    /// Store `product` as `subject`'s reaching type.
    pub fn set_reaching_type(
        &mut self,
        subject: &FlowProductSubject,
        product: ReachingTypeProduct,
    ) {
        self.entries.insert(
            Self::frame_slot(FlowDomain::ReachingType, subject),
            FlowProductValue::ReachingType(product),
        );
    }

    /// The subject's literal-widening membership.
    #[must_use]
    pub fn widening(&self, subject: &FlowProductSubject) -> Option<&WideningMembership> {
        self.reaching_type(subject)
            .and_then(ReachingTypeProduct::widening)
    }

    /// The subject's declared (annotation) type.
    #[must_use]
    pub fn declared_type(&self, subject: &FlowProductSubject) -> Option<SemanticNodeId> {
        match self
            .entries
            .get(&Self::frame_slot(FlowDomain::DeclaredType, subject))
        {
            Some(FlowProductValue::DeclaredType(product)) => product.declared(),
            _ => None,
        }
    }

    /// Store `subject`'s declared type, or clear it when `declared` is `None`.
    pub fn set_declared_type(
        &mut self,
        subject: &FlowProductSubject,
        declared: Option<SemanticNodeId>,
    ) {
        match declared {
            Some(node) => {
                self.entries.insert(
                    Self::frame_slot(FlowDomain::DeclaredType, subject),
                    FlowProductValue::DeclaredType(DeclaredTypeProduct::of(node)),
                );
            }
            None => self.remove(FlowDomain::DeclaredType, subject),
        }
    }

    /// The subject's definite-assignment product (bottom when unrecorded).
    #[must_use]
    pub fn assignment(&self, subject: &FlowProductSubject) -> DefiniteAssignmentProduct {
        match self
            .entries
            .get(&Self::frame_slot(FlowDomain::DefiniteAssignment, subject))
        {
            Some(FlowProductValue::DefiniteAssignment(product)) => *product,
            _ => DefiniteAssignmentProduct::default(),
        }
    }

    /// Store `subject`'s definite-assignment product.
    pub fn set_assignment(
        &mut self,
        subject: &FlowProductSubject,
        product: DefiniteAssignmentProduct,
    ) {
        self.entries.insert(
            Self::frame_slot(FlowDomain::DefiniteAssignment, subject),
            FlowProductValue::DefiniteAssignment(product),
        );
    }

    /// The guard facts held for `subject`.
    #[must_use]
    pub fn narrowing(&self, subject: &FlowProductSubject) -> Option<&NarrowingProduct> {
        match self
            .entries
            .get(&Self::frame_slot(FlowDomain::Narrowing, subject))
        {
            Some(FlowProductValue::Narrowing(product)) => Some(product),
            _ => None,
        }
    }

    /// Store the guard facts of `subject`; an empty set clears the slot, so
    /// "no fact" is one state rather than two.
    pub fn set_narrowing(&mut self, subject: &FlowProductSubject, product: NarrowingProduct) {
        if product.facts().is_empty() {
            self.remove(FlowDomain::Narrowing, subject);
            return;
        }
        self.entries.insert(
            Self::frame_slot(FlowDomain::Narrowing, subject),
            FlowProductValue::Narrowing(product),
        );
    }

    /// Every subject holding a product in `domain`, in canonical key order.
    #[must_use]
    pub fn subjects_in(&self, domain: FlowDomain) -> Vec<FlowProductSubject> {
        let mut keys: Vec<&FlowProductKey> = self
            .entries
            .keys()
            .filter(|key| key.domain() == domain)
            .collect();
        keys.sort_by(|a, b| key_order(a).cmp(&key_order(b)));
        keys.into_iter().map(|key| key.subject().clone()).collect()
    }

    /// Every subject holding a product in ANY domain, in canonical key
    /// order and deduplicated — the pointwise join's subject universe.
    #[must_use]
    pub fn subjects(&self) -> Vec<FlowProductSubject> {
        let mut keys: Vec<&FlowProductKey> = self.entries.keys().collect();
        keys.sort_by(|a, b| key_order(a).cmp(&key_order(b)));
        let mut out: Vec<FlowProductSubject> = Vec::new();
        for key in keys {
            if !out.contains(key.subject()) {
                out.push(key.subject().clone());
            }
        }
        out
    }
}

/// The domains a frame's product state is held in, in the plan's own
/// tie-break rank order. TOTAL over the product vocabulary the evaluator
/// populates — the join below iterates exactly this list, so a domain
/// cannot be silently skipped at a merge point.
pub const FLOW_FRAME_DOMAINS: [FlowDomain; 4] = [
    FlowDomain::ReachingType,
    FlowDomain::DeclaredType,
    FlowDomain::Narrowing,
    FlowDomain::DefiniteAssignment,
];

/// What one incoming edge contributes when it holds NO product for a
/// subject the other edge does. TOTAL over the domain registry and
/// wildcard-free: a new frame domain must decide its own missing-edge
/// rule rather than inheriting one.
#[rustfmt::skip]
const fn missing_edge_is_bottom(domain: FlowDomain) -> bool {
    match domain {
        // A guard fact holds past a merge only when EVERY incoming edge
        // established it, so an edge that holds no fact contributes the
        // empty set and the intersection drops the fact.
        FlowDomain::Narrowing => true,
        // A value, a declaration, or an assignment state only one edge
        // knows about survives the merge as that edge's: the other edge
        // never contradicted it.
        FlowDomain::ReachingType | FlowDomain::DeclaredType
        | FlowDomain::DefiniteAssignment | FlowDomain::ReachingValue
        | FlowDomain::Completion | FlowDomain::ClosureCapture | FlowDomain::Freshness
        | FlowDomain::Effects | FlowDomain::CallResolution | FlowDomain::Relation
        | FlowDomain::ContextualTyping | FlowDomain::Coverage => false,
    }
}

/// The outcome of joining two frame product states.
#[derive(Debug, Clone)]
pub enum FlowFrameJoinOutcome {
    /// The merged state.
    Joined(FlowProductStore),
    /// A domain join could not be modelled; the typed gap says why.
    Gap(FlowGap),
    /// The merge exhausted a budget axis.
    BudgetExceeded(FlowProductBudgetExceeded),
}

/// Join two frame product states at a merge point — the ONE frame join
/// route, driven entirely by the per-domain [`join_product`] rules.
///
/// The subject universe is the canonical (tie-break ordered) union of both
/// states' subjects, and each subject's domains are folded in
/// [`FLOW_FRAME_DOMAINS`] order, so the merge is deterministic under any
/// insertion order of either side. A domain gap or a width overflow
/// returns the degraded arm WITHOUT a store: a merge that could not be
/// modelled has nothing a frame could keep evaluating against.
///
/// The fold repeats over the ordered subject universe until a whole pass
/// moves no product, bounded by `budget.max_iterations`: the second pass
/// is the idempotence proof of every domain rule that ran in the first,
/// and a rule that keeps moving exhausts the plan's own convergence
/// policy instead of spinning.
pub fn join_frame_products(
    algebra: &dyn FlowSemanticAlgebra,
    budget: &FlowProductBudget,
    a: &FlowProductStore,
    b: &FlowProductStore,
) -> FlowFrameJoinOutcome {
    let mut subjects: Vec<FlowProductSubject> = a.subjects();
    for subject in b.subjects() {
        if !subjects.contains(&subject) {
            subjects.push(subject);
        }
    }
    let mut joined = a.clone();
    let mut iterations = 0u32;
    loop {
        if iterations == budget.max_iterations {
            return FlowFrameJoinOutcome::BudgetExceeded(FlowProductBudgetExceeded {
                axis: FlowProductBudgetAxis::Iterations,
                limit: budget.max_iterations,
                observed: budget.max_iterations.saturating_add(1),
            });
        }
        iterations += 1;
        let mut moved = false;
        for subject in &subjects {
            for domain in FLOW_FRAME_DOMAINS {
                let key = FlowProductStore::frame_slot(domain, subject);
                let held = joined.entries.get(&key).cloned();
                let incoming = b.entries.get(&key).cloned();
                let bottom_missing = missing_edge_is_bottom(domain);
                let kind = key.kind();
                let (left, right) = match (held, incoming) {
                    (None, None) => continue,
                    (Some(_), None) if !bottom_missing => continue,
                    (Some(left), None) => (left, FlowProductValue::bottom(kind)),
                    (None, Some(right)) if !bottom_missing => {
                        joined.entries.insert(key, right);
                        moved = true;
                        continue;
                    }
                    (None, Some(right)) => (FlowProductValue::bottom(kind), right),
                    (Some(left), Some(right)) => (left, right),
                };
                match join_product(algebra, budget, &left, &right) {
                    FlowTransferOutcome::Unchanged => {}
                    FlowTransferOutcome::Changed(value) => {
                        joined.entries.insert(key, value);
                        moved = true;
                    }
                    FlowTransferOutcome::Gap(gap) => return FlowFrameJoinOutcome::Gap(gap),
                    FlowTransferOutcome::BudgetExceeded(exceeded) => {
                        return FlowFrameJoinOutcome::BudgetExceeded(exceeded)
                    }
                }
            }
        }
        if joined.entries.len() > budget.max_products as usize {
            return FlowFrameJoinOutcome::BudgetExceeded(FlowProductBudgetExceeded {
                axis: FlowProductBudgetAxis::Products,
                limit: budget.max_products,
                observed: u32::try_from(joined.entries.len()).unwrap_or(u32::MAX),
            });
        }
        if !moved {
            return FlowFrameJoinOutcome::Joined(joined);
        }
    }
}
