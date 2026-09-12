//! The CLOSED inventory of every carrier that transports a completion
//! fact from the slice lowering to a published flow-return value.
//!
//! # Why a closed inventory exists
//!
//! A completion fact decides whether a body's end point contributes an
//! arm to the return join. Getting one wrong is the worst defect class
//! this substrate has: the answer is WRONG and it WARMS, because a
//! completion fact carries no degradation of its own — nothing downstream
//! can tell a dropped fall-through edge from a body that genuinely never
//! completes.
//!
//! The failure this module makes unrepresentable is a carrier that is
//! simply MISSED. A prose list of carriers, amended one entry at a time
//! whenever a new one is discovered, is not a specification: it asserts a
//! completeness it cannot check, and the entry it omits is exactly the one
//! that silently drops a fact. The inventory here is CODE-FIRST instead —
//! every row exists because real code cites it, and the compiler plus the
//! inventory suite together decide whether that citation still holds.
//!
//! # What makes the inventory closed
//!
//! Four properties hold together, and each covers a direction the others
//! cannot:
//!
//! 1. **The fact cannot be produced or read without naming a row.** A
//!    normal-completion fact is an opaque type whose inner boolean is
//!    private to this module. [`NormalCompletion::minted`] is its ONLY
//!    constructor and takes a [`CompletionConstruction`];
//!    [`NormalCompletion::reaches_end`] is its ONLY reader and takes a
//!    [`CompletionDischarge`]. A new site that wants to derive anything
//!    from a completion fact therefore cannot compile until it names a
//!    row. This is the direction an amended prose list could never cover.
//! 2. **The two site vocabularies cannot be confused.** They are distinct
//!    closed enums, so a construction row is unrepresentable at a
//!    discharge and the reverse — a mismatch is a type error, not a
//!    runtime assertion that compiles out of a shipped build.
//! 3. **Every stored carrier binds to a real type.** [`TransportsCompletion`]
//!    is SEALED and implemented once per carrier type, so a
//!    [`CompletionTransport`] row always names a type that exists and
//!    really holds the fact.
//! 4. **The listed inventory and the cited inventory are the same set.**
//!    The suite records which rows real evaluations visit and requires
//!    that set to EQUAL the listed one. The equality is what closes both
//!    residual directions at once: a row nobody cites is prose that drifted
//!    in, and a site citing a row the list does not carry is precisely the
//!    missed carrier this module exists to catch.
//!
//! # Roles
//!
//! [`FlowCompletionRole`] is the lifecycle column: which part of the
//! journey a row stands for, from the lowering that mints a fact to the
//! admission boundary that decides whether the value carrying it may warm.
//! It is what makes the inventory readable as a whole rather than a flat
//! list of names.

/// One completion FACT — the closed set of things this substrate knows
/// about how a region or a body completes.
///
/// The fact / role / transport COLUMNS and the coverage recording below
/// are the inventory's description of itself, and only the inventory
/// suite reads them, so they are built in the test configuration alone.
/// The ENFORCEMENT half — the opaque [`NormalCompletion`] and the typed
/// construction / discharge rows every production site must name — is
/// unconditional, so a shipped build carries the confinement and none of
/// the bookkeeping.
#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum FlowCompletionFact {
    /// Whether control reaches past a region or body without returning.
    NormalCompletion,
    /// A `return;` with no argument contributed to the join.
    BareReturn,
    /// An inference-only implicit `undefined` contributed to the join.
    ImplicitUndefined,
    /// What a body models as when it contributes no return arm and never
    /// completes normally — a property of the function's authored form.
    AuthoredForm,
    /// A `switch` clause path exits the switch through `break`.
    SwitchCaseBreak,
}

#[cfg(test)]
/// Where in a completion fact's life one inventory row stands.
///
/// The vocabulary is the one the completion debt was scoped against:
/// facts are PRODUCED, held by TRANSIENT carriers, CONSTRUCTED,
/// TRANSFERRED across an evaluation boundary, DISCHARGED into a decision,
/// ASSEMBLED into a result, PUBLISHED on that result, and finally gate
/// ADMISSION.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum FlowCompletionRole {
    /// The lowering that derives a fact from authored syntax.
    Producer,
    /// A value that holds a fact between production and discharge.
    TransientCarrier,
    /// A point one fact's value is minted.
    Construction,
    /// A fact crossing an evaluation boundary — a nested body, a
    /// recursive component member, a resolver-side refinement.
    Transfer,
    /// A read that turns a fact into a decision.
    Discharge,
    /// The join that folds the facts into the return type.
    ResultAssembly,
    /// A fact surfaced on the published result.
    Publication,
    /// The boundary that decides whether the carrying value may warm.
    AdmissionExit,
}

/// Every point that MINTS a completion fact.
///
/// Passing one of these to [`NormalCompletion::minted`] is what puts a
/// construction on the inventory: there is no other way to build the
/// fact, so a new minting site does not compile until it has a row.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum CompletionConstruction {
    /// The statement-list lowering's running reachability accumulator —
    /// the producer of every region's normal-completion fact.
    RegionAccumulator,
    /// A region built without lowering a statement list: an unselected
    /// body, a parameter-lowering budget edge, an expression-bodied
    /// arrow's synthesized `return`, a body that hit an invoked-closure
    /// effect.
    SynthesizedRegion,
    /// The function body's own fact, taken from its root region.
    BodyFromRootRegion,
    /// The evaluator's refinement of the lowering's body fact — a `switch`
    /// whose case tests exhaust the discriminant has no no-matching-case
    /// path, which only the resolver can see. Narrows downward only.
    EvaluatorRefinement,
    /// The same refinement for a NESTED function body, whose fact
    /// crosses into the nested evaluation and back out on the callable
    /// value the outer body returns.
    NestedBodyRefinement,
    /// A `switch` clause's exit-through-`break` fact.
    SwitchCaseBreak,
    /// A hermetically minted result — the crate's `for_tests` fixtures and
    /// the unproven-member injection the seal and admission suites publish
    /// through. They construct a real completion fact for a body they never
    /// lowered, so they are carriers like any other and are listed rather
    /// than exempted; each stands in for a value, never for a reachable end
    /// point.
    ///
    /// `allow(dead_code)`: the citing helpers are themselves unreachable in
    /// a plain library build, so the row constructs only once a test binary
    /// links them.
    #[allow(dead_code)]
    HermeticFixture,
    /// The fact carried onto a rebuilt result that keeps its origin's
    /// completion — a projection or substitution over an already-decided
    /// value, which re-decides nothing.
    ResultRebuild,
}

/// Every point that READS a completion fact into a decision.
///
/// [`NormalCompletion::reaches_end`] is the fact's only reader, so a new
/// decision site does not compile until it has a row here.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum CompletionDischarge {
    /// The lowering's read of a child region's fact while composing its
    /// parent's.
    RegionComposition,
    /// The lowering's read of the body region that seeds the content's
    /// own fact.
    BodyComposition,
    /// The evaluator's region walk, deciding whether a statement path
    /// stays alive.
    EvaluatorRegionWalk,
    /// The return join's seed decision: whether to add the fall-through
    /// arm, and which primitive an arm-less body models as.
    ReturnJoin,
    /// The freshness decision — a fall-through arm is a join contributor,
    /// so a single fresh literal return no longer widens alone.
    FreshLiteralWidening,
    /// A member projection over a result whose body can still fall
    /// through.
    MemberProjection,
    /// The published fact, read off a finished result.
    PublishedResult,
}

/// Every TYPE that stores a completion fact between production and
/// discharge.
///
/// [`TransportsCompletion`] binds each row to the real type, so a row can
/// never name a carrier that does not exist.
#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum CompletionTransport {
    /// One lowered statement list.
    SliceRegion,
    /// One lowered function body.
    SliceContent,
    /// One lowered `switch` clause.
    SliceSwitchCase,
    /// The evaluator's per-body observation pair.
    BodyCompletionObservations,
    /// The published flow-return value.
    FlowReturnResult,
}

/// THE closed inventory: every carrier of a completion fact, in every
/// role it plays.
#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum FlowCompletionCarrier {
    /// A minting site.
    Construction(CompletionConstruction),
    /// A reading site.
    Discharge(CompletionDischarge),
    /// A storing type.
    Transport(CompletionTransport),
}

#[cfg(test)]
impl CompletionConstruction {
    /// Every construction row.
    pub(crate) const ALL: &'static [Self] = &[
        Self::RegionAccumulator,
        Self::SynthesizedRegion,
        Self::BodyFromRootRegion,
        Self::EvaluatorRefinement,
        Self::NestedBodyRefinement,
        Self::SwitchCaseBreak,
        Self::HermeticFixture,
        Self::ResultRebuild,
    ];

    /// The fact this site mints. Exhaustive: a new variant must decide
    /// what it produces before it compiles.
    pub(crate) fn fact(self) -> FlowCompletionFact {
        match self {
            Self::RegionAccumulator
            | Self::SynthesizedRegion
            | Self::BodyFromRootRegion
            | Self::EvaluatorRefinement
            | Self::NestedBodyRefinement
            | Self::HermeticFixture
            | Self::ResultRebuild => FlowCompletionFact::NormalCompletion,
            Self::SwitchCaseBreak => FlowCompletionFact::SwitchCaseBreak,
        }
    }

    /// The lifecycle role this site plays.
    pub(crate) fn role(self) -> FlowCompletionRole {
        match self {
            Self::RegionAccumulator => FlowCompletionRole::Producer,
            Self::SynthesizedRegion
            | Self::BodyFromRootRegion
            | Self::SwitchCaseBreak
            | Self::HermeticFixture
            | Self::ResultRebuild => FlowCompletionRole::Construction,
            Self::EvaluatorRefinement | Self::NestedBodyRefinement => FlowCompletionRole::Transfer,
        }
    }
}

#[cfg(test)]
impl CompletionDischarge {
    /// Every discharge row.
    pub(crate) const ALL: &'static [Self] = &[
        Self::RegionComposition,
        Self::BodyComposition,
        Self::EvaluatorRegionWalk,
        Self::ReturnJoin,
        Self::FreshLiteralWidening,
        Self::MemberProjection,
        Self::PublishedResult,
    ];

    /// The fact this site reads.
    pub(crate) fn fact(self) -> FlowCompletionFact {
        match self {
            Self::RegionComposition
            | Self::BodyComposition
            | Self::EvaluatorRegionWalk
            | Self::ReturnJoin
            | Self::FreshLiteralWidening
            | Self::MemberProjection
            | Self::PublishedResult => FlowCompletionFact::NormalCompletion,
        }
    }

    /// The lifecycle role this site plays.
    pub(crate) fn role(self) -> FlowCompletionRole {
        match self {
            Self::RegionComposition | Self::BodyComposition | Self::EvaluatorRegionWalk => {
                FlowCompletionRole::Discharge
            }
            Self::ReturnJoin | Self::FreshLiteralWidening => FlowCompletionRole::ResultAssembly,
            Self::MemberProjection | Self::PublishedResult => FlowCompletionRole::Publication,
        }
    }
}

#[cfg(test)]
impl CompletionTransport {
    /// Every transport row.
    pub(crate) const ALL: &'static [Self] = &[
        Self::SliceRegion,
        Self::SliceContent,
        Self::SliceSwitchCase,
        Self::BodyCompletionObservations,
        Self::FlowReturnResult,
    ];

    /// The facts this carrier stores.
    pub(crate) fn facts(self) -> &'static [FlowCompletionFact] {
        match self {
            Self::SliceRegion => &[FlowCompletionFact::NormalCompletion],
            Self::SliceContent => &[
                FlowCompletionFact::NormalCompletion,
                FlowCompletionFact::AuthoredForm,
            ],
            Self::SliceSwitchCase => &[
                FlowCompletionFact::NormalCompletion,
                FlowCompletionFact::SwitchCaseBreak,
            ],
            Self::BodyCompletionObservations => &[
                FlowCompletionFact::BareReturn,
                FlowCompletionFact::ImplicitUndefined,
            ],
            Self::FlowReturnResult => &[FlowCompletionFact::NormalCompletion],
        }
    }

    /// The lifecycle role this carrier plays.
    pub(crate) fn role(self) -> FlowCompletionRole {
        match self {
            Self::SliceRegion
            | Self::SliceContent
            | Self::SliceSwitchCase
            | Self::BodyCompletionObservations => FlowCompletionRole::TransientCarrier,
            Self::FlowReturnResult => FlowCompletionRole::AdmissionExit,
        }
    }
}

#[cfg(test)]
impl FlowCompletionCarrier {
    /// The whole listed inventory, in a stable order.
    pub(crate) fn all() -> Vec<Self> {
        CompletionConstruction::ALL
            .iter()
            .copied()
            .map(Self::Construction)
            .chain(
                CompletionDischarge::ALL
                    .iter()
                    .copied()
                    .map(Self::Discharge),
            )
            .chain(
                CompletionTransport::ALL
                    .iter()
                    .copied()
                    .map(Self::Transport),
            )
            .collect()
    }

    /// The lifecycle role of this row.
    pub(crate) fn role(self) -> FlowCompletionRole {
        match self {
            Self::Construction(site) => site.role(),
            Self::Discharge(site) => site.role(),
            Self::Transport(site) => site.role(),
        }
    }
}

#[cfg(test)]
/// The sealing module: [`TransportsCompletion`] is implementable only
/// inside this crate's declared carrier set, so a transport row always
/// names a type this inventory vouched for.
pub(crate) mod sealed {
    /// Sealed: only this crate's declared completion carriers implement
    /// it, and it is the supertrait bound on [`super::TransportsCompletion`].
    pub(crate) trait Sealed {}
}

#[cfg(test)]
/// A type that stores a completion fact.
///
/// The impl is what makes [`CompletionTransport`] code-first: the row
/// exists because a real type claims it.
pub(crate) trait TransportsCompletion: sealed::Sealed {
    /// This carrier's inventory row.
    const TRANSPORT: CompletionTransport;
}

/// Declare one carrier type's inventory row.
macro_rules! transports_completion {
    ($ty:ty => $row:ident) => {
        #[cfg(test)]
        impl $crate::flow_completion_inventory::sealed::Sealed for $ty {}
        #[cfg(test)]
        impl $crate::flow_completion_inventory::TransportsCompletion for $ty {
            const TRANSPORT: $crate::flow_completion_inventory::CompletionTransport =
                $crate::flow_completion_inventory::CompletionTransport::$row;
        }
    };
}
pub(crate) use transports_completion;

/// Whether control can reach past a region or body without returning.
///
/// The inner fact is PRIVATE to this module. It is minted only through
/// [`Self::minted`] and read only through [`Self::reaches_end`], and both
/// take an inventory row — so every site that produces or consumes a
/// normal-completion fact is, by construction, on the inventory.
///
/// The type deliberately offers no `From<bool>`, no `Into<bool>`, and no
/// `Deref`: a conversion with no row would be exactly the silent carrier
/// this module exists to prevent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct NormalCompletion(bool);

impl NormalCompletion {
    /// Mint the fact at one inventory construction site.
    #[must_use]
    pub(crate) fn minted(reaches_end: bool, at: CompletionConstruction) -> Self {
        observe_construction(at);
        Self(reaches_end)
    }

    /// Read the fact at one inventory discharge site.
    #[must_use]
    pub(crate) fn reaches_end(self, at: CompletionDischarge) -> bool {
        observe_discharge(at);
        self.0
    }

    /// Mint the fact for a value a suite builds to assert ABOUT.
    ///
    /// The counterpart to [`Self::reaches_end_for_assertion`], and exempt
    /// for the same reason: a synthetic value a suite hands straight to an
    /// assertion is not a pipeline carrier, and giving it a row would put a
    /// stage on the inventory that production does not have. Built in the
    /// test configuration alone, so production has exactly one way to mint
    /// the fact: [`Self::minted`], with a row.
    #[cfg(test)]
    #[must_use]
    pub(crate) fn minted_for_fixture(reaches_end: bool) -> Self {
        Self(reaches_end)
    }

    /// Read the fact to ASSERT it, never to decide with it.
    ///
    /// A suite inspecting a lowered region is not a carrier: it takes no
    /// decision the pipeline depends on, so it neither needs a discharge
    /// row nor should add one — an inventory row that only the tests reach
    /// would describe a pipeline stage that does not exist. The reader is
    /// built in the test configuration alone, so production has exactly
    /// one way to read the fact: [`Self::reaches_end`], with a row.
    #[cfg(test)]
    #[must_use]
    pub(crate) fn reaches_end_for_assertion(self) -> bool {
        self.0
    }
}

/// The evaluator's per-body observation pair: the two contributions a
/// body makes to the join that are neither a value return nor the
/// fall-through arm.
///
/// They ride together because the join reads them together and because
/// they are the same KIND of fact — an arm contributed without an
/// authored value expression.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct BodyCompletionObservations {
    /// A `return;` with no argument was evaluated.
    bare_return: bool,
    /// An inference-only implicit `undefined` edge was retained.
    implicit_undefined: bool,
}

transports_completion!(BodyCompletionObservations => BodyCompletionObservations);

impl BodyCompletionObservations {
    /// Nothing observed yet.
    #[must_use]
    pub(crate) fn none() -> Self {
        Self::default()
    }

    /// Record an evaluated bare `return;`.
    pub(crate) fn observe_bare_return(&mut self) {
        self.bare_return = true;
    }

    /// Record a retained inference-only implicit `undefined` edge.
    pub(crate) fn observe_implicit_undefined(&mut self) {
        self.implicit_undefined = true;
    }

    /// Whether a bare `return;` contributed.
    #[must_use]
    pub(crate) fn bare_return(self) -> bool {
        self.bare_return
    }

    /// Whether an implicit `undefined` contributed.
    #[must_use]
    pub(crate) fn implicit_undefined(self) -> bool {
        self.implicit_undefined
    }

    /// Whether either observation contributes an arm to the join — the
    /// freshness rule's input, which does not care which one fired.
    #[must_use]
    pub(crate) fn contributes_arm(self) -> bool {
        self.bare_return || self.implicit_undefined
    }
}

/// Record that one inventory row was visited.
///
/// Compiled to nothing outside the crate's own test build, so the
/// inventory's coverage half costs a shipped evaluation nothing and
/// changes no behaviour in either build.
#[cfg(not(test))]
#[inline(always)]
fn observe_construction(_at: CompletionConstruction) {}

#[cfg(not(test))]
#[inline(always)]
fn observe_discharge(_at: CompletionDischarge) {}

#[cfg(test)]
fn observe_construction(at: CompletionConstruction) {
    coverage::visit(FlowCompletionCarrier::Construction(at));
}

#[cfg(test)]
fn observe_discharge(at: CompletionDischarge) {
    coverage::visit(FlowCompletionCarrier::Discharge(at));
}

#[cfg(test)]
pub(crate) mod coverage {
    use super::FlowCompletionCarrier;
    use std::collections::BTreeSet;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Mutex, MutexGuard, OnceLock};

    /// Whether a recording is open. A relaxed load is the whole cost of
    /// the instrumentation while no recording runs, which is every test
    /// but the coverage one.
    static RECORDING: AtomicBool = AtomicBool::new(false);

    fn rows() -> &'static Mutex<BTreeSet<FlowCompletionCarrier>> {
        static ROWS: OnceLock<Mutex<BTreeSet<FlowCompletionCarrier>>> = OnceLock::new();
        ROWS.get_or_init(|| Mutex::new(BTreeSet::new()))
    }

    /// Serializes recordings, so two of them can never interleave into one
    /// another's row set.
    fn recording_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    /// Note one visited row, when a recording is open.
    ///
    /// The set is PROCESS-wide rather than thread-local on purpose: a
    /// completion fact is minted on whichever worker lowers the body and
    /// discharged on whichever thread evaluates it, so a thread-local
    /// recording would silently miss exactly the producer rows that matter
    /// most.
    pub(super) fn visit(carrier: FlowCompletionCarrier) {
        if !RECORDING.load(Ordering::Acquire) {
            return;
        }
        if let Ok(mut set) = rows().lock() {
            set.insert(carrier);
        }
    }

    /// Record every inventory row visited while `body` runs.
    ///
    /// This is the coverage half of the inventory. The suite compares the
    /// recorded set against the listed one in BOTH directions: a listed
    /// row nobody cites is prose that drifted in, and a cited row the list
    /// does not carry is the missed carrier the inventory exists to catch.
    ///
    /// The window is process-wide, so an unrelated evaluation running
    /// concurrently in the same binary can contribute a row. That can only
    /// ADD citations, so the "cited but unlisted" direction stays exact,
    /// and the "listed but unreached" direction stays a sound lower bound
    /// on what the probes cover. The probe table's own correctness test is
    /// what pins each row to the program that exercises it.
    pub(crate) fn record<R>(body: impl FnOnce() -> R) -> (R, BTreeSet<FlowCompletionCarrier>) {
        let _serialized: MutexGuard<'_, ()> = recording_lock()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Ok(mut set) = rows().lock() {
            set.clear();
        }
        RECORDING.store(true, Ordering::Release);
        let result = body();
        RECORDING.store(false, Ordering::Release);
        let visited = rows()
            .lock()
            .map(|set| set.clone())
            .unwrap_or_else(|poisoned| poisoned.into_inner().clone());
        (result, visited)
    }
}
