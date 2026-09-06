//! The product lattice of the shared flow authority: the per-binding,
//! per-domain dataflow PRODUCTS a flow-bearing operation computes over the
//! frame it evaluates, plus the ONE join route that drives every merge
//! point to a fixed point.
//!
//! The layer owns the live value path: the flow evaluator holds its whole
//! semantic state in a [`FlowProductStore`] over the frame subject
//! vocabulary minted here, and every merge point it models folds through
//! the per-domain joins ([`join_frame_products`] over [`join_product`]).
//! Nothing here resolves a type, opens a file, reaches a store view, or
//! dispatches a query — the substrate is PURE over its inputs, and the
//! semantic content of a product is supplied by its producer.
//!
//! Ownership boundaries, all load-bearing:
//!
//! - **Product-state algebra is owned here; semantic type algebra is
//!   not.** A join that must produce a semantic composite aggregates its
//!   flow-domain contributors and asks the canonical algebra
//!   ([`FlowSemanticAlgebra`], whose sole production implementor forwards
//!   to the dispatch's canonical union authority) to construct the
//!   result. There is no flow-private union or intersection reducer.
//! - **One domain registry.** The domains ARE the closed [`FlowDomain`]
//!   registry — this substrate declares no product-kind enum of its own,
//!   so a product slot, a product value and a join arm all name the same
//!   registry variant. [`domain_carries_product`] is the total,
//!   wildcard-free projection of that registry onto the domains this
//!   substrate carries a product for — a domain with no product is a
//!   typed refusal, never a fallthrough.
//! - **One store, no public product query.** [`FlowProductStore`] is the
//!   only product storage, and the evaluator reaches its own through the
//!   frame subject mints. There is no second store and no standalone
//!   product query API.
//! - **A degraded outcome retains nothing.** [`FlowTransferOutcome`]'s
//!   `Gap` and `BudgetExceeded` arms carry NO [`FlowProductValue`], so a
//!   gapped or budget-exhausted step has nothing a store could admit, and
//!   the frame join returns the degraded arm WITHOUT its partially-joined
//!   store. Warmability is structurally unreachable, not policed.
//! - **Binding subjects carry stable identity.** A frame subject is a
//!   resolved slot of [`FlowFrameBindings`] or a parameter ordinal, never
//!   an authored name: two same-named bindings of different scope layers
//!   are different slots and can never alias.
//!
//! Determinism is structural rather than incidental: the frame join folds
//! the canonical (tie-break ordered) union of both states' subjects in
//! [`FLOW_FRAME_DOMAINS`] order, so equivalent insertion orders produce
//! one answer; every product's carrier is a canonical set; and a rule that
//! keeps moving exhausts the demand plan's own convergence policy instead
//! of spinning.

use std::sync::Arc;

use rustc_hash::{FxHashMap, FxHashSet};

// The domain rank is the registry's OWN stable discriminant
// (`flow_solve::domain_discriminant`), imported rather than restated: a
// second copy could number a future variant differently and silently
// desynchronise this store's canonical key order from the result-contract
// identity that ranks over the same registry.
use super::flow_solve::{domain_discriminant, FlowDomain};
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

/// Whether `domain` carries a product in this substrate.
///
/// This is the ONE projection of the domain registry onto the product
/// lattice — there is no second product-kind enum to keep in step with it,
/// so a product slot, a product value and a join arm all name the same
/// registry variant. TOTAL over the registry by construction (a
/// wildcard-free match), so a new registry domain must decide its product
/// route deliberately.
#[rustfmt::skip]
pub const fn domain_carries_product(domain: FlowDomain) -> bool {
    match domain {
        FlowDomain::ReachingType | FlowDomain::DeclaredType
        | FlowDomain::Narrowing | FlowDomain::DefiniteAssignment => true,
        // Declared by the registry, carried by no product lattice: these
        // domains discharge on evidence, not on a lattice value.
        FlowDomain::ReachingValue | FlowDomain::Completion | FlowDomain::ClosureCapture
        | FlowDomain::Freshness | FlowDomain::Effects | FlowDomain::CallResolution
        | FlowDomain::Relation | FlowDomain::ContextualTyping | FlowDomain::Coverage => false,
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
/// explicit rather than derived so the subject spaces stay a stated total
/// order rather than a field order.
fn narrowing_order(fact: &FlowNarrowingFact) -> (u32, u32, Vec<&str>, u64) {
    let (space, ordinal) = subject_order(&fact.subject);
    (
        space,
        ordinal,
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
            FlowDomain::ReachingType => Some(Self::ReachingType(ReachingTypeProduct::default())),
            FlowDomain::DeclaredType => Some(Self::DeclaredType(DeclaredTypeProduct::default())),
            FlowDomain::Narrowing => Some(Self::Narrowing(NarrowingProduct::default())),
            FlowDomain::DefiniteAssignment => {
                Some(Self::DefiniteAssignment(DefiniteAssignmentProduct::default()))
            }
            FlowDomain::ReachingValue | FlowDomain::Completion | FlowDomain::ClosureCapture
            | FlowDomain::Freshness | FlowDomain::Effects | FlowDomain::CallResolution
            | FlowDomain::Relation | FlowDomain::ContextualTyping | FlowDomain::Coverage => None,
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

/// What one product is held FOR. Two disjoint frame subject spaces,
/// each with exactly one sealed mint, so a binding subject and a
/// parameter subject can never alias:
///
/// - [`Self::FrameBinding`] is a binding of the frame an evaluation is
///   walking, by its resolved slot. Minted ONLY by
///   [`FlowFrameBindings::slot`].
/// - [`Self::FrameParam`] is a formal parameter of that frame, by ordinal.
///   Minted ONLY by [`FlowFrameBindings::param`].
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum FlowProductSubject {
    /// A frame binding, by its resolved slot.
    FrameBinding(FlowFrameSlot),
    /// A formal parameter of the frame, by ordinal.
    FrameParam(u32),
}

/// One product slot: a flow domain over one subject. A BINDING subject
/// carries the frame's own resolved slot identity, so two same-named
/// bindings of different scope layers are different slots and can never
/// alias.
///
/// Fields are private and the only constructors are
/// [`frame_product_key`] and [`FlowFrameBindings`]'s mints: a key over an
/// unresolved frame slot and a key on a productless domain are both
/// unrepresentable rather than rejected later.
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

    /// The subject the slot is held for.
    #[must_use]
    pub fn subject(&self) -> &FlowProductSubject {
        &self.subject
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
}

/// The product key of `domain` at `subject` — the mint frame-subject
/// callers use. A productless domain is a typed error, exactly as it is
/// on the graph mint.
pub fn frame_product_key(
    domain: FlowDomain,
    subject: FlowProductSubject,
) -> Result<FlowProductKey, FlowProductKeyError> {
    if !domain_carries_product(domain) {
        return Err(FlowProductKeyError::DomainCarriesNoProduct);
    }
    Ok(FlowProductKey { domain, subject })
}

// ── Store, seeds, budget ───────────────────────────────────────────────

/// The ONE product store: computed products keyed by [`FlowProductKey`].
///
/// The store's whole write surface is the typed frame accessors below,
/// each of which mints its own slot and takes the domain's own product
/// type — a value cannot be filed under another domain's slot. Neither
/// degraded [`FlowTransferOutcome`] arm carries a [`FlowProductValue`], so
/// admitting a gapped or budget-exhausted step is unrepresentable rather
/// than policed, and a joined store escapes the frame join only on its
/// converged arm.
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

// ── Join ───────────────────────────────────────────────────────────────

/// Join `a` and `b` at a merge point — the ONE join route, exhaustive over
/// the product vocabulary and domain-SPECIFIC by construction:
///
/// - **Reaching types** union their canonical contributor SET and then ask
///   the canonical algebra to construct the semantic result; an unproven
///   construction is a typed gap, never an unproven published product.
/// - **Declared types** agree or gap: a merge point cannot invent a
///   declaration neither edge declared.
/// - **Narrowing** INTERSECTS: a guard fact survives only when EVERY
///   incoming edge established it.
/// - **Definite assignment** uses its declared lattice.
///
/// Every route is idempotent (`join(x, x)` is `Unchanged`). The declared-
/// type, narrowing and definite-assignment routes are permutation-stable
/// outright: their carriers are canonical sets or a commutative lattice.
/// The reaching-TYPE route is stable in MEANING but deliberately NOT in
/// representation — its contributor list is FIRST-CONTRIBUTION order,
/// because that list IS the arm order the canonical algebra unions, and
/// the widening membership prefers the RIGHT operand. Operand order is
/// therefore a caller obligation on that one route: every caller supplies
/// a fixed order (the frame join folds the held state against the
/// incoming one), and swapping the operands hands the algebra the
/// mirrored arm order rather than a different answer.
pub fn join_product(
    algebra: &dyn FlowSemanticAlgebra,
    budget: &FlowProductBudget,
    a: &FlowProductValue,
    b: &FlowProductValue,
) -> FlowTransferOutcome {
    if a.domain() != b.domain() {
        return FlowTransferOutcome::Gap(FlowGap::UnmodeledExpression);
    }
    let joined = match (a, b) {
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

// ── Canonical ordering ─────────────────────────────────────────────────

/// The subject-space rank and ordinal: frame bindings first, then frame
/// parameters. A total order over the two disjoint spaces.
const fn subject_order(subject: &FlowProductSubject) -> (u32, u32) {
    match subject {
        FlowProductSubject::FrameBinding(slot) => (1, slot.0),
        FlowProductSubject::FrameParam(ordinal) => (2, *ordinal),
    }
}

/// The canonical ordering key of one product key: the domain rank, then
/// the subject space, then the subject ordinal.
const fn key_order(key: &FlowProductKey) -> (u32, u32, u32) {
    let (space, ordinal) = subject_order(&key.subject);
    (domain_discriminant(key.domain), space, ordinal)
}

// ── The frame product environment ──────────────────────────────────────

impl FlowProductBudget {
    /// The product budget of one demand plan — CONNECTED to the plan's own
    /// policies, never a private constant:
    ///
    /// - `max_iterations` IS the plan's [`FlowConvergencePolicy`];
    /// - `max_products` IS the frame's own subject universe, one slot per
    ///   frame domain, over BOTH the spaces a frame subject can come from:
    ///   the selected obligation frontier plus the slice sites that
    ///   frontier does not name (every BINDING subject is minted at a
    ///   slice-selected site — a binding obligation is not the only one,
    ///   a name resolved in both scope layers holds products no obligation
    ///   counts), and `signature_params`, because the PARAMETER space is
    ///   not selection-bound at all: completing a snapshot's parameter
    ///   layer mints one product per SIGNATURE parameter, read or unread,
    ///   so a wide signature under a narrow demand holds parameter slots
    ///   the selection never names. A frame that accumulates more slots
    ///   than the two spaces together has left the plan's work universe;
    /// - `max_product_width` IS the slice budget's selected-node ceiling
    ///   (a subject cannot accumulate more contributors than the slice
    ///   selected sites to contribute them).
    #[must_use]
    pub fn for_demand_plan(
        plan: &super::flow_solve::FlowDemandPlan,
        signature_params: usize,
    ) -> Self {
        Self {
            max_iterations: plan.convergence().max_iterations,
            max_products: u32::try_from(
                plan.work_order().len()
                    + plan.structural_selection().value_nodes.len()
                    + plan.structural_selection().effect_only_nodes.len()
                    + signature_params,
            )
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
        keys.sort_unstable_by_key(|key| key_order(key));
        keys.into_iter().map(|key| key.subject().clone()).collect()
    }

    /// Every subject holding a product in ANY domain, in canonical key
    /// order and deduplicated — the pointwise join's subject universe.
    ///
    /// The order is minted HERE rather than held by the container: the
    /// store is a hash map because every other access is a point lookup
    /// by slot, and only the two enumerations need a sequence. The sort
    /// is therefore the price of determinism at a merge point, and it is
    /// bounded by the same axis the merge itself is — a join whose store
    /// grows past [`FlowProductBudget::max_products`] fails at the end of
    /// the pass that grew it, so the universe this walks stays within one
    /// pass of the frame's capped subject space. It reads no cache,
    /// resolves nothing, and enters no dispatch: the only work is over
    /// keys the frame already holds.
    #[must_use]
    pub fn subjects(&self) -> Vec<FlowProductSubject> {
        let mut keys: Vec<&FlowProductKey> = self.entries.keys().collect();
        keys.sort_unstable_by_key(|key| key_order(key));
        // Membership is a hash lookup rather than a scan of what has been
        // emitted: the universe is walked once per merge point, so a
        // quadratic dedup here is paid on every join of every frame.
        let mut seen: FxHashSet<&FlowProductSubject> = FxHashSet::default();
        let mut out: Vec<FlowProductSubject> = Vec::with_capacity(keys.len());
        for key in keys {
            if seen.insert(key.subject()) {
                out.push(key.subject().clone());
            }
        }
        out
    }
}

/// The domains a frame's product state is held in, in the SAME domain
/// rank [`key_order`] sorts by — the module has one canonical order, so
/// a reader of the join's fold order and a reader of the store's own
/// subject enumeration see the same sequence. TOTAL over the product
/// vocabulary the evaluator populates: the join below iterates exactly
/// this list, so a domain cannot be silently skipped at a merge point.
pub const FLOW_FRAME_DOMAINS: [FlowDomain; 4] = [
    FlowDomain::ReachingType,
    FlowDomain::Narrowing,
    FlowDomain::DeclaredType,
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

/// The bottom element of one frame domain. Every [`FLOW_FRAME_DOMAINS`]
/// entry carries a product, so the registry projection is total here.
fn bottom_of(domain: FlowDomain) -> FlowProductValue {
    FlowProductValue::bottom(domain).expect("every frame domain carries a product")
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
    let mut seen: FxHashSet<FlowProductSubject> = subjects.iter().cloned().collect();
    for subject in b.subjects() {
        if seen.insert(subject.clone()) {
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
                let (left, right) = match (held, incoming) {
                    (None, None) => continue,
                    (Some(_), None) if !bottom_missing => continue,
                    (Some(left), None) => (left, bottom_of(domain)),
                    (None, Some(right)) if !bottom_missing => {
                        joined.entries.insert(key, right);
                        moved = true;
                        continue;
                    }
                    (None, Some(right)) => (bottom_of(domain), right),
                    (Some(left), Some(right)) => (left, right),
                };
                match join_product(algebra, budget, &left, &right) {
                    FlowTransferOutcome::Unchanged => {}
                    FlowTransferOutcome::Changed(value) => {
                        // One representation of "no fact": the narrowing
                        // write accessor clears an emptied slot, so a join
                        // that intersected every guard fact away clears it
                        // too rather than filing an empty product beside
                        // the absent one.
                        if matches!(&value, FlowProductValue::Narrowing(product) if product.facts().is_empty())
                        {
                            // Clearing an already-absent slot is not a
                            // move: reporting one would re-ready the same
                            // subject every pass and spin the fold to its
                            // iteration budget.
                            moved |= joined.entries.remove(&key).is_some();
                        } else {
                            joined.entries.insert(key, value);
                            moved = true;
                        }
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
