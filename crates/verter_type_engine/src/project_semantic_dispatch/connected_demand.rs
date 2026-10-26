//! The connected-demand operational ledger.
//!
//! One connected semantic demand is the whole tree of structural and query
//! work rooted at an outermost dispatch or direct projector entry. Bounding
//! that tree is an OPERATIONAL responsibility — work units, query-boundary
//! depth, and the request's cancellation signal — with no semantic content:
//! the ledger decides only *whether more work may run*, never *what a type
//! means*.
//!
//! That responsibility lives here rather than on
//! [`ProjectSemanticDispatch`](super::ProjectSemanticDispatch) so a budget
//! consumer can be handed the ledger alone. The accounting cells are private
//! to this module, so no consumer — inside the dispatch tree or outside it —
//! can read or move the counters except through the operations below; and a
//! consumer typed on [`ConnectedDemandLedger`] cannot reach a semantic
//! capability at all, because the ledger holds none. The sole outside signal
//! it observes is [`DemandCancellation`], a one-question seam over the
//! request's cancellation authority.
//!
//! The caps are single-sourced: [`ConnectedDemandLedger::new`] installs the
//! production limits and only the test-only setter replaces them. Beginning a
//! demand resets the COUNTERS, never the caps, so there is exactly one place a
//! limit can come from.

use std::cell::{Cell, RefCell};
use std::sync::Arc;
use verter_execution::sync_cell::{SyncBool, SyncRefCell, SyncU16, SyncU32, SyncU64, SyncUsize};

use rustc_hash::FxHashSet;

use super::cost_receipt::{
    BudgetProfileSpec, CostDependency, CostIdentity, CostIdentitySet, CostScope, DemandCostReceipt,
    LogicalUsage, Nesting, ReplayRefusal, COST_MODEL_REVISION,
};

use crate::semantic_query::PartialReasonSet;

/// The work units one structured comparison the checker records costs the
/// relation at most, on the shape its relation-complexity limit is reached
/// by: a union arm scanning a union target of object types charges the
/// alternative and its pair's worklist step (measured: 22,329 units for the
/// 20,100 comparisons of 200 reversed arms).
pub const RELATION_UNITS_PER_COMPARISON: usize = 2;

/// The construction bytes one structured comparison reserves: what a pair
/// the relation relates holds live at its peak — its queued member and the
/// index its answer is read back through, its relation flight and prepared
/// identity, its interned key — with the frames, bindings, receipts and
/// connected-demand state the relation holds beside them. No proof or
/// explanation is held per pair. The allocator canary
/// `a_relation_holds_no_more_than_it_charges` measures that working state
/// independently of this figure and fails when the charge understates it.
/// The allowance it sizes is calibrated with the other operation and
/// retention limits.
pub const RELATION_PAIR_BYTES: usize = 2_048;

/// The construction bytes one arm of a constructed union reserves while the
/// union is canonicalized: its place in the member lists the passes build,
/// its ordering key and its entries in the dedup and literal tables
/// (measured: about 175 bytes an arm canonicalizing the 99,999 string
/// literals a two-span template forms). Provisional: calibrated with the
/// other operation and retention limits.
pub const UNION_ARM_BYTES: usize = 256;

/// The construction bytes interning one derived node reserves: its payload
/// record, held twice (the arena slot and the dedup index's key), a fixed
/// index entry, the construction root and arena ownership slots, and the
/// `payload_bytes` of text and children it owns.
pub(crate) fn derived_node_bytes(payload_bytes: usize) -> usize {
    2 * std::mem::size_of::<crate::semantic_query::SemanticNodeData>()
        + 32
        + crate::semantic_query_memo::node_roots::construction_root_bytes()
        + std::mem::size_of::<usize>()
        + std::mem::size_of::<bool>()
        + payload_bytes
}

/// The work units one instantiation the checker counts costs a connected
/// demand at most, on the shapes whose size only the checker's
/// instantiation allowance bounds: a distributive conditional repeating an
/// `infer` charges 60 units per union member where the checker instantiates
/// 7 types, a tuple inferring through distinct aliases of one generic 30
/// against 5, and a recursive conditional over an `as const` route tree 132
/// per parent against 219 (measured with `tsc --extendedDiagnostics` on
/// TypeScript 7.0.2).
pub const WORK_UNITS_PER_INSTANTIATION: usize = 9;

/// The construction bytes one instantiation the checker counts costs a
/// connected demand at most, on the same shapes: the repeated `infer`
/// reserves 8,192 bytes per union member against the checker's 7
/// instantiations, the route tree 41 KB per parent against 219.
pub const CONSTRUCTION_BYTES_PER_INSTANTIATION: usize = 1_280;

/// Work-unit cap for one connected semantic demand: the operational
/// backstop, sized to cover both of the checker's own allowances at once,
/// so it never refuses an input the checker answers. The checker counts
/// relation comparisons and instantiations against separate limits, so one
/// demand may spend both in full: a conditional distributing over a union
/// relates every member it instantiates. The cap is their sum. The relation
/// envelope is
/// [`checker_policy::RELATION_COMPARISONS`] structured comparisons at
/// [`RELATION_UNITS_PER_COMPARISON`] reducer units plus one cold query-entry
/// unit each (the reducer mapping alone excludes the enclosing dispatch
/// entries and would stop the reversed-object conditional construction
/// before its comparison allowance). The instantiation envelope is
/// [`checker_policy::INSTANTIATION_COUNT`] instantiations at
/// [`WORK_UNITS_PER_INSTANTIATION`] units each: a distribution over a union
/// of hundreds of thousands of members, which the checker answers, runs no
/// relation the first envelope would size. Work units do not bound
/// memory; the construction-byte rail
/// ([`MAX_CONNECTED_CONSTRUCTION_BYTES`]) does. Both operational rails
/// include both checker envelopes, so neither substitutes an unevaluated
/// conditional for the checker's answer.
///
/// [`checker_policy::RELATION_COMPARISONS`]: crate::semantic_query::checker_policy::RELATION_COMPARISONS
/// [`checker_policy::INSTANTIATION_COUNT`]: crate::semantic_query::checker_policy::INSTANTIATION_COUNT
pub const MAX_CONNECTED_PROJECTION_WORK: usize =
    crate::semantic_query::checker_policy::RELATION_COMPARISONS as usize
        * (RELATION_UNITS_PER_COMPARISON + 1)
        + crate::semantic_query::checker_policy::INSTANTIATION_COUNT as usize
            * WORK_UNITS_PER_INSTANTIATION;
/// Nested query-boundary cap for one connected semantic demand.
pub const MAX_CONNECTED_QUERY_DEPTH: u16 = 24;
/// Construction-byte allowance for one connected semantic demand: the bytes
/// it reserves before it builds, never refunded within the demand, so the
/// allowance bounds what the demand holds at its peak. The sum of the
/// relation mapping (the full checker relation envelope at
/// [`RELATION_PAIR_BYTES`] a pair) and the instantiation mapping (the
/// checker's instantiation allowance at
/// [`CONSTRUCTION_BYTES_PER_INSTANTIATION`]), for the same reason the work
/// cap sums them, with the non-relation allowance left for operand
/// construction and the consuming projection. This is logical construction
/// accounting, not an allocation or a retained-memory quota, so it is
/// counted in `u64` on every target: the allowance exceeds a 32-bit
/// `usize`, and a target's pointer width must not change which inputs a
/// demand answers.
pub(super) const MAX_CONNECTED_CONSTRUCTION_BYTES: u64 =
    crate::semantic_query::checker_policy::RELATION_COMPARISONS as u64 * RELATION_PAIR_BYTES as u64
        + crate::semantic_query::checker_policy::INSTANTIATION_COUNT as u64
            * CONSTRUCTION_BYTES_PER_INSTANTIATION as u64
        + (512 << 20);

/// The cost of an inline part of a computation, measured apart from the
/// rest of it ([`ConnectedDemandLedger::open_inline_cost`]): the usage it
/// charged itself, its longest tail run, and the results it consumed. A
/// consumer that skips performing the part again charges it with
/// [`ConnectedDemandLedger::replay_inline_cost`].
#[derive(Clone)]
pub(crate) struct InlineCost {
    exclusive: LogicalUsage,
    tail_steps: u32,
    prerequisites: Vec<CostDependency>,
}

/// An inline measurement being taken
/// ([`ConnectedDemandLedger::open_inline_cost`]). Dropped unclosed — an
/// unwind — it ends unmeasured, with the recordings opened above it, each
/// abandoned into the one below.
pub(crate) struct InlineCostRecording<'l> {
    ledger: &'l dyn InlineCostLedger,
    place: usize,
    open: bool,
}

impl InlineCostRecording<'_> {
    /// Close it: what it measured, kept by the computation below. `None`
    /// when it is not the recording on top.
    pub(crate) fn close(mut self) -> Option<InlineCost> {
        self.open = false;
        self.ledger.close_at(self.place)
    }
}

impl Drop for InlineCostRecording<'_> {
    fn drop(&mut self) {
        if self.open {
            self.ledger.abandon_at(self.place);
        }
    }
}

/// The ledger operations an [`InlineCostRecording`] ends itself with.
trait InlineCostLedger {
    fn close_at(&self, place: usize) -> Option<InlineCost>;
    fn abandon_at(&self, place: usize);
}

impl InlineCostLedger for ConnectedDemandLedger<'_> {
    fn close_at(&self, place: usize) -> Option<InlineCost> {
        self.close_inline_cost(place)
    }
    fn abandon_at(&self, place: usize) {
        self.abandon_inline_cost(place);
    }
}

/// The identity of an inline measurement, which is never sealed.
#[derive(Hash, PartialEq, Eq)]
struct InlineCostKey;

/// What one connected demand has charged so far: its work units and its
/// construction bytes.
#[cfg(any(test, feature = "test-support"))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct DemandUsage {
    pub work: usize,
    pub bytes: u64,
}

/// Verter's own instantiation budget for one connected semantic demand:
/// how deep instantiations evaluated on the continuation runtime may nest
/// (each one's frame is held on the heap while the one it needs builds).
/// Far past the checker's own depth limit, so Verter answers where the
/// checker gives up; reaching it is the checker's TS2589, reported at
/// Verter's limit ([`checker_policy::instantiation_budget`]). It bounds
/// the frames one chain holds, so an instantiation that never terminates
/// stops in bounded memory; the work ledger bounds its time.
///
/// [`checker_policy::instantiation_budget`]: crate::semantic_query::checker_policy::instantiation_budget
pub(super) const MAX_CONNECTED_INSTANTIATION_DEPTH: u32 = 10_000;

/// Verter's own budget for one tail run of a conditional type: how many
/// tail steps (each the conditional over its next arguments, evaluated in
/// place) one run may take. Far past the checker's own tail limit, so
/// Verter answers where the checker gives up; reaching it is the checker's
/// TS2589 ([`ConditionalTail`]). A run holds one step's arguments at a
/// time, and the arguments it has seen for its recurrence check.
///
/// [`ConditionalTail`]: crate::semantic_query::checker_policy::ConditionalTail
pub(super) const MAX_CONNECTED_TAIL_STEPS: u32 = 10_000;

#[cfg(any(test, feature = "test-support"))]
thread_local! {
    /// A lower instantiation budget for the ledgers this thread installs,
    /// set by [`InstantiationBudgetForTests`].
    static INSTANTIATION_DEPTH_FOR_TESTS: Cell<Option<u32>> = const { Cell::new(None) };
    /// A lower tail budget for the ledgers this thread installs, set by
    /// [`TailBudgetForTests`].
    static TAIL_STEPS_FOR_TESTS: Cell<Option<u32>> = const { Cell::new(None) };
    /// A lower work budget for the ledgers this thread installs, set by
    /// [`WorkBudgetForTests`].
    static WORK_FOR_TESTS: Cell<Option<usize>> = const { Cell::new(None) };
}

/// Install `limit` as the connected-work budget of every ledger this
/// thread creates until the guard drops — the production budget's code
/// path at an amount of work a test can reach quickly. Test-only.
#[cfg(any(test, feature = "test-support"))]
pub struct WorkBudgetForTests {
    previous: Option<usize>,
}

#[cfg(any(test, feature = "test-support"))]
impl WorkBudgetForTests {
    pub fn install(limit: usize) -> Self {
        Self {
            previous: WORK_FOR_TESTS.with(|slot| slot.replace(Some(limit))),
        }
    }
}

#[cfg(any(test, feature = "test-support"))]
impl Drop for WorkBudgetForTests {
    fn drop(&mut self) {
        WORK_FOR_TESTS.with(|slot| slot.set(self.previous));
    }
}

/// Install `limit` as the tail budget of every ledger this thread creates
/// until the guard drops — the production budget's code path at a length a
/// test can reach quickly. Test-only.
#[cfg(any(test, feature = "test-support"))]
pub struct TailBudgetForTests {
    previous: Option<u32>,
}

#[cfg(any(test, feature = "test-support"))]
impl TailBudgetForTests {
    pub fn install(limit: u32) -> Self {
        Self {
            previous: TAIL_STEPS_FOR_TESTS.with(|slot| slot.replace(Some(limit))),
        }
    }
}

#[cfg(any(test, feature = "test-support"))]
impl Drop for TailBudgetForTests {
    fn drop(&mut self) {
        TAIL_STEPS_FOR_TESTS.with(|slot| slot.set(self.previous));
    }
}

/// Install `limit` as the instantiation budget of every ledger this
/// thread creates until the guard drops — the production budget's code
/// path at a depth a test can reach quickly. Test-only.
#[cfg(any(test, feature = "test-support"))]
pub struct InstantiationBudgetForTests {
    previous: Option<u32>,
}

#[cfg(any(test, feature = "test-support"))]
impl InstantiationBudgetForTests {
    pub fn install(limit: u32) -> Self {
        Self {
            previous: INSTANTIATION_DEPTH_FOR_TESTS.with(|slot| slot.replace(Some(limit))),
        }
    }
}

#[cfg(any(test, feature = "test-support"))]
impl Drop for InstantiationBudgetForTests {
    fn drop(&mut self) {
        INSTANTIATION_DEPTH_FOR_TESTS.with(|slot| slot.set(self.previous));
    }
}

/// The one signal the ledger observes from outside its own accounting:
/// whether the owning request has been cancelled.
///
/// A newtype rather than the resolver context itself, and its field is
/// private to this module: the ledger — and therefore every consumer typed on
/// the ledger — can ask the single question below and reach nothing else. No
/// host caches, no store view, and above all no route back into semantic
/// dispatch.
#[derive(Clone, Copy)]
pub(crate) struct DemandCancellation<'a> {
    checkpoint: crate::resolver_core::request_ports::CancellationCheckpoint,
    _request: std::marker::PhantomData<&'a ()>,
}

impl<'a> DemandCancellation<'a> {
    /// Narrow the request snapshot's flag handles down to the cancellation
    /// signal. The dispatch passes the snapshot it borrowed once at request
    /// admission, so every charge-boundary checkpoint below is a plain
    /// field read.
    pub(super) fn from_flags(
        flags: &'a crate::resolver_core::resolver_context::RequestFlags,
    ) -> Self {
        Self {
            checkpoint: flags.cancellation_checkpoint(),
            _request: std::marker::PhantomData,
        }
    }

    /// Cheap cancellation checkpoint, consulted at every charge boundary.
    #[cfg_attr(feature = "test-support", track_caller)]
    fn is_cancelled(&self) -> bool {
        self.checkpoint.is_cancelled()
    }
}

/// The reason set's explicit bit representation, read and replaced whole.
struct PartialReasonCell(SyncU32);

impl PartialReasonCell {
    fn new(value: PartialReasonSet) -> Self {
        Self(SyncU32::new(value.to_bits()))
    }

    fn get(&self) -> PartialReasonSet {
        PartialReasonSet::from_bits(self.0.get())
    }

    fn set(&self, value: PartialReasonSet) {
        self.0.set(value.to_bits());
    }

    fn swap(&self, other: &Self) {
        self.0.swap(&other.0);
    }
}

/// Exclusive receipt charges since the last recording transition. Budget
/// counters advance immediately; only their attribution to the locked scope
/// stack is batched. Like the ledger, these cells have one driver at a time.
#[derive(Default)]
struct PendingUsage {
    work: SyncU64,
    bytes: SyncU64,
    advisory_bytes: SyncU64,
    operations: SyncU64,
    comparisons: SyncU64,
}

impl PendingUsage {
    fn take(&self) -> LogicalUsage {
        fn take(cell: &SyncU64) -> u64 {
            let value = cell.get();
            cell.set(0);
            value
        }
        LogicalUsage {
            work: take(&self.work),
            bytes: take(&self.bytes),
            advisory_bytes: take(&self.advisory_bytes),
            operations: take(&self.operations),
            comparisons: take(&self.comparisons),
        }
    }

    fn swap(&self, other: &Self) {
        self.work.swap(&other.work);
        self.bytes.swap(&other.bytes);
        self.advisory_bytes.swap(&other.advisory_bytes);
        self.operations.swap(&other.operations);
        self.comparisons.swap(&other.comparisons);
    }
}

#[derive(Default)]
struct RequestEffectCells {
    operations: SyncUsize,
    paid: SyncUsize,
}

impl RequestEffectCells {
    fn get(&self) -> RequestEffects {
        RequestEffects {
            operations: self.operations.get(),
            paid: self.paid.get(),
        }
    }

    fn reset(&self) {
        self.operations.set(0);
        self.paid.set(0);
    }

    fn swap(&self, other: &Self) {
        self.operations.swap(&other.operations);
        self.paid.swap(&other.paid);
    }
}

/// Operational budget ledger for one dispatcher's connected demands.
///
/// Holds the live counters plus the configured caps. Every field is private
/// to this module: the ledger's invariants (a counter advances only inside an
/// active demand; a trip is sticky for the rest of the demand; caps change
/// only outside a demand) are enforced by the operations, not by convention.
pub struct ConnectedDemandLedger<'a> {
    cancellation: DemandCancellation<'a>,
    active: SyncBool,
    work_used: SyncUsize,
    work_limit: SyncUsize,
    /// Every construction byte the demand charged: the bytes it reserved
    /// and the advisory footprint (`advisory_bytes_used`).
    bytes_used: SyncU64,
    /// The part of `bytes_used` charged while the advisory
    /// construction-byte rail is off: accounted, never enforced, so no
    /// later reservation is refused for it.
    advisory_bytes_used: SyncU64,
    bytes_limit: SyncU64,
    /// Whether a canonical construction's footprint
    /// ([`Self::charge_construction`]) may be refused at `bytes_limit`: the
    /// advisory construction-byte rail, off unless explicitly enabled. Off,
    /// the footprint is still charged to `bytes_used`, as advisory bytes.
    construction_byte_refusal: SyncBool,
    query_depth: SyncU16,
    query_depth_limit: SyncU16,
    instantiation_depth_limit: u32,
    tail_steps_limit: u32,
    tripped: PartialReasonCell,
    /// The cold computations recording their exclusive cost, innermost
    /// last; `scope_open` mirrors "the stack is not empty" so a charge
    /// taken outside any recording pays one flag read.
    scopes: SyncRefCell<Vec<CostScope>>,
    scope_open: SyncBool,
    pending_usage: PendingUsage,
    /// The computations whose receipt, and its whole prerequisite closure,
    /// this connected demand has paid.
    paid: SyncRefCell<CostIdentitySet>,
    /// What this connected demand itself did to its request: the request
    /// operations it spent and the computations it was first to pay.
    request_effects: RequestEffectCells,
    /// Whether one of this demand's reads was answered with a recursion
    /// carrier another task's schedule decided — a cross-task wait cycle,
    /// or a joined producer's own re-entry: the demand went on without work
    /// it alone would have done, so where it stopped is not its own.
    schedule_cut: SyncBool,
}

/// What one connected demand did to its request
/// ([`ConnectedDemandLedger::request_effects`]).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct RequestEffects {
    /// The request operations the demand spent.
    pub(crate) operations: usize,
    /// The computations the demand added to the request's paid set.
    pub(crate) paid: usize,
}

/// Connected charges of one component-meta compute. A root resets the
/// demand counters; it does not reset an open span, so the compute keeps
/// the sum across every root it entered. The copy admission stores is taken
/// when the span ends. This stack is not that copy.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct ComponentMetaSpan {
    pub(crate) work: usize,
    pub(crate) bytes: u64,
    /// The part of `bytes` that is advisory construction footprint.
    pub(crate) advisory_bytes: u64,
    pub(crate) trip: PartialReasonSet,
    pub(crate) schedule_cuts: u64,
}

thread_local! {
    static COMPONENT_META_SPANS: RefCell<Vec<ComponentMetaSpan>> = const { RefCell::new(Vec::new()) };
    static COMPONENT_META_SPAN_OPEN: Cell<bool> = const { Cell::new(false) };
    #[cfg(test)]
    static SCOPE_CHARGE_WRITES: Cell<usize> = const { Cell::new(0) };
}

/// The component-meta span of one compute, which the compute owns across
/// its suspensions. Each step of the compute opens it on the running
/// thread ([`Self::install`]); nested computes each keep their own frame,
/// and a charge is recorded on every frame open.
#[derive(Default)]
pub(crate) struct ComputeSpan {
    span: ComponentMetaSpan,
}

impl ComputeSpan {
    /// Open the span on this thread until the returned installation drops,
    /// unwinding included.
    pub(crate) fn install(&mut self) -> ComputeSpanInstall<'_> {
        COMPONENT_META_SPANS.with(|spans| spans.borrow_mut().push(self.span));
        COMPONENT_META_SPAN_OPEN.with(|open| open.set(true));
        ComputeSpanInstall { owner: self }
    }

    pub(crate) fn finish(self) -> ComponentMetaSpan {
        self.span
    }
}

/// One step's installation of a [`ComputeSpan`]; dropping it closes the
/// frame on the thread and keeps its charges in the compute.
pub(crate) struct ComputeSpanInstall<'c> {
    owner: &'c mut ComputeSpan,
}

impl Drop for ComputeSpanInstall<'_> {
    fn drop(&mut self) {
        self.owner.span = pop_component_meta_span();
    }
}

/// Capture all connected work across the roots of one test request without
/// installing or changing its request context. The ledger owns the charges;
/// capture uses its existing scoped telemetry and panic cleanup.
#[cfg(any(test, feature = "test-support"))]
pub fn with_connected_work_capture_for_tests<R>(body: impl FnOnce() -> R) -> (R, DemandUsage) {
    let mut span = ComputeSpan::default();
    let result = {
        let _installed = span.install();
        body()
    };
    let span = span.finish();
    (
        result,
        DemandUsage {
            work: span.work,
            bytes: span.bytes,
        },
    )
}

fn pop_component_meta_span() -> ComponentMetaSpan {
    let span = COMPONENT_META_SPANS.with(|spans| spans.borrow_mut().pop().unwrap_or_default());
    let empty = COMPONENT_META_SPANS.with(|spans| spans.borrow().is_empty());
    if empty {
        COMPONENT_META_SPAN_OPEN.with(|open| open.set(false));
    }
    span
}

fn span_open() -> bool {
    COMPONENT_META_SPAN_OPEN.with(Cell::get)
}

/// The component-meta charges of a semantic task, which may be polled on a
/// thread other than the one that lent it its request: one span standing
/// for every span open where it was lent (none when none was). Each poll
/// installs it as the polling thread's only span, so a charge lands in the
/// task — never in a span the polling thread has open for other work — and
/// the task's return folds it into the spans it was lent under.
#[derive(Default)]
pub(crate) struct TaskSpanCarry {
    open: bool,
    /// The task's span while it is not polled: one entry when `open`.
    stack: Vec<ComponentMetaSpan>,
}

impl TaskSpanCarry {
    /// The carry of a task lent here: open exactly when a span is.
    pub(crate) fn lend() -> Self {
        let open = span_open();
        Self {
            open,
            stack: if open {
                vec![ComponentMetaSpan::default()]
            } else {
                Vec::new()
            },
        }
    }

    /// Install the task's span on this thread for one poll; the returned
    /// guard restores the thread's own spans and takes the task's back.
    pub(crate) fn install(&mut self) -> TaskSpanInstall<'_> {
        let thread =
            COMPONENT_META_SPANS.with(|spans| spans.replace(std::mem::take(&mut self.stack)));
        let thread_open = COMPONENT_META_SPAN_OPEN.with(|open| open.replace(self.open));
        TaskSpanInstall {
            carry: self,
            thread,
            thread_open,
        }
    }

    /// Fold what the task charged into every span open on this thread, the
    /// one that lent it, as if each charge had been made here.
    pub(crate) fn return_to_lender(self) {
        let Some(span) = self.stack.into_iter().next() else {
            return;
        };
        if !span_open() {
            return;
        }
        COMPONENT_META_SPANS.with(|spans| {
            for open in spans.borrow_mut().iter_mut() {
                open.work = open.work.saturating_add(span.work);
                open.bytes = open.bytes.saturating_add(span.bytes);
                open.advisory_bytes = open.advisory_bytes.saturating_add(span.advisory_bytes);
                open.trip = open.trip.union(span.trip);
                open.schedule_cuts = open.schedule_cuts.saturating_add(span.schedule_cuts);
            }
        });
    }
}

/// A task's span installed for one poll ([`TaskSpanCarry::install`]).
pub(crate) struct TaskSpanInstall<'c> {
    carry: &'c mut TaskSpanCarry,
    thread: Vec<ComponentMetaSpan>,
    thread_open: bool,
}

impl Drop for TaskSpanInstall<'_> {
    fn drop(&mut self) {
        self.carry.stack =
            COMPONENT_META_SPANS.with(|spans| spans.replace(std::mem::take(&mut self.thread)));
        COMPONENT_META_SPAN_OPEN.with(|open| open.set(self.thread_open));
    }
}

/// Record one charge on every open span, including its advisory footprint.
fn remember_usage(work: usize, bytes: u64, advisory: u64) {
    if (work == 0 && bytes == 0) || !span_open() {
        return;
    }
    COMPONENT_META_SPANS.with(|spans| {
        for span in spans.borrow_mut().iter_mut() {
            if work != 0 {
                span.work = span.work.saturating_add(work);
            }
            if bytes != 0 {
                span.bytes = span.bytes.saturating_add(bytes);
                span.advisory_bytes = span.advisory_bytes.saturating_add(advisory);
            }
        }
    });
}

fn remember_trip(reason: PartialReasonSet) {
    if reason.is_empty() || !span_open() {
        return;
    }
    COMPONENT_META_SPANS.with(|spans| {
        for span in spans.borrow_mut().iter_mut() {
            span.trip = span.trip.union(reason);
        }
    });
}

fn remember_schedule_cut() {
    if !span_open() {
        return;
    }
    COMPONENT_META_SPANS.with(|spans| {
        for span in spans.borrow_mut().iter_mut() {
            span.schedule_cuts = span.schedule_cuts.saturating_add(1);
        }
    });
}

impl std::fmt::Debug for ConnectedDemandLedger<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ConnectedDemandLedger")
            .field("active", &self.active.get())
            .field("work_used", &self.work_used.get())
            .field("work_limit", &self.work_limit.get())
            .field("bytes_used", &self.bytes_used.get())
            .field("advisory_bytes_used", &self.advisory_bytes_used.get())
            .field("bytes_limit", &self.bytes_limit.get())
            .field(
                "construction_byte_refusal",
                &self.construction_byte_refusal.get(),
            )
            .field("query_depth", &self.query_depth.get())
            .field("query_depth_limit", &self.query_depth_limit.get())
            .field("instantiation_depth_limit", &self.instantiation_depth_limit)
            .field("tail_steps_limit", &self.tail_steps_limit)
            .field("tripped", &self.tripped.get())
            .finish()
    }
}

impl<'a> ConnectedDemandLedger<'a> {
    /// A ledger at this one's caps, observing its cancellation, with no
    /// demand of its own: the ledger a task's dispatcher holds while it
    /// carries this one's state ([`Self::exchange_state`]).
    pub(super) fn sibling(&self) -> Self {
        Self {
            instantiation_depth_limit: self.instantiation_depth_limit,
            tail_steps_limit: self.tail_steps_limit,
            ..Self::new(self.cancellation)
        }
    }

    /// Exchange every counter, cap, trip, cost recording and paid
    /// identity with `other`, a ledger at the same fixed caps: the
    /// connected demand moves between the dispatchers whole.
    pub(super) fn exchange_state(&self, other: &Self) {
        let Self {
            cancellation: _,
            active,
            work_used,
            work_limit,
            bytes_used,
            advisory_bytes_used,
            bytes_limit,
            construction_byte_refusal,
            query_depth,
            query_depth_limit,
            instantiation_depth_limit,
            tail_steps_limit,
            tripped,
            scopes,
            scope_open,
            pending_usage,
            paid,
            request_effects,
            schedule_cut,
        } = self;
        verter_debug_assert!(
            *instantiation_depth_limit == other.instantiation_depth_limit
                && *tail_steps_limit == other.tail_steps_limit,
            "a ledger exchanges its state with a sibling at its own fixed caps"
        );
        active.swap(&other.active);
        work_used.swap(&other.work_used);
        work_limit.swap(&other.work_limit);
        bytes_used.swap(&other.bytes_used);
        advisory_bytes_used.swap(&other.advisory_bytes_used);
        bytes_limit.swap(&other.bytes_limit);
        construction_byte_refusal.swap(&other.construction_byte_refusal);
        query_depth.swap(&other.query_depth);
        query_depth_limit.swap(&other.query_depth_limit);
        tripped.swap(&other.tripped);
        scopes.swap(&other.scopes);
        scope_open.swap(&other.scope_open);
        pending_usage.swap(&other.pending_usage);
        paid.swap(&other.paid);
        request_effects.swap(&other.request_effects);
        schedule_cut.swap(&other.schedule_cut);
    }

    /// How many cost recordings are open.
    #[cfg(any(test, feature = "test-support"))]
    pub fn open_cost_scopes_for_tests(&self) -> usize {
        self.scopes.borrow().len()
    }

    /// Install a ledger at the production caps, observing `cancellation`.
    pub(super) fn new(cancellation: DemandCancellation<'a>) -> Self {
        Self {
            cancellation,
            active: SyncBool::new(false),
            work_used: SyncUsize::new(0),
            #[cfg(not(any(test, feature = "test-support")))]
            work_limit: SyncUsize::new(MAX_CONNECTED_PROJECTION_WORK),
            #[cfg(any(test, feature = "test-support"))]
            work_limit: SyncUsize::new(
                WORK_FOR_TESTS
                    .with(Cell::get)
                    .unwrap_or(MAX_CONNECTED_PROJECTION_WORK),
            ),
            bytes_used: SyncU64::new(0),
            advisory_bytes_used: SyncU64::new(0),
            bytes_limit: SyncU64::new(MAX_CONNECTED_CONSTRUCTION_BYTES),
            construction_byte_refusal: SyncBool::new(false),
            query_depth: SyncU16::new(0),
            query_depth_limit: SyncU16::new(MAX_CONNECTED_QUERY_DEPTH),
            #[cfg(not(any(test, feature = "test-support")))]
            instantiation_depth_limit: MAX_CONNECTED_INSTANTIATION_DEPTH,
            #[cfg(any(test, feature = "test-support"))]
            instantiation_depth_limit: INSTANTIATION_DEPTH_FOR_TESTS
                .with(Cell::get)
                .unwrap_or(MAX_CONNECTED_INSTANTIATION_DEPTH),
            #[cfg(not(any(test, feature = "test-support")))]
            tail_steps_limit: MAX_CONNECTED_TAIL_STEPS,
            #[cfg(any(test, feature = "test-support"))]
            tail_steps_limit: TAIL_STEPS_FOR_TESTS
                .with(Cell::get)
                .unwrap_or(MAX_CONNECTED_TAIL_STEPS),
            tripped: PartialReasonCell::new(PartialReasonSet::empty()),
            scopes: SyncRefCell::new(Vec::new()),
            scope_open: SyncBool::new(false),
            pending_usage: PendingUsage::default(),
            paid: SyncRefCell::new(CostIdentitySet::default()),
            request_effects: RequestEffectCells::default(),
            schedule_cut: SyncBool::new(false),
        }
    }

    /// Whether an instantiation nested `depth` deep on the continuation
    /// runtime is within Verter's instantiation budget
    /// ([`MAX_CONNECTED_INSTANTIATION_DEPTH`]).
    pub(crate) fn instantiation_within_budget(&self, depth: u32) -> bool {
        depth <= self.instantiation_depth_limit
    }

    /// Take one more step of the tail run `tail` under this demand's tail
    /// budget, recording the steps the run has taken on the open recording:
    /// its receipt then names the tail allowance its cold run needed.
    pub(crate) fn tail_step(
        &self,
        tail: &mut crate::semantic_query::checker_policy::ConditionalTail,
        operation: crate::semantic_query::CheckerDiagnosticOperation,
    ) -> Result<(), crate::semantic_query::checker_policy::OperationRefusal> {
        tail.step(self.tail_steps_limit, operation)?;
        if self.scope_open.get() {
            if let Some(scope) = self.scopes.borrow_mut().last_mut() {
                scope.tail_steps = scope.tail_steps.max(tail.steps());
            }
        }
        Ok(())
    }

    /// Join the active connected demand, or install a fresh one when this is
    /// the outermost entry. Returns the panic-safe guard plus the trip that
    /// forbids the caller's work — `Some` when the ledger had already tripped,
    /// when the request is cancelled, or when `query_boundary` would exceed
    /// the nested-query depth cap.
    pub(super) fn enter(
        &self,
        query_boundary: bool,
    ) -> (ConnectedDemandGuard<'_>, Option<PartialReasonSet>) {
        self.enter_observed(query_boundary, self.cancellation.is_cancelled())
    }

    /// [`Self::enter`] with the request's cancellation already read as
    /// `cancelled`, for a caller that reads it once for the entry and the
    /// charge that follows it.
    pub(super) fn enter_observed(
        &self,
        query_boundary: bool,
        cancelled: bool,
    ) -> (ConnectedDemandGuard<'_>, Option<PartialReasonSet>) {
        let root = !self.active.get();
        if root {
            self.work_used.set(0);
            self.bytes_used.set(0);
            self.advisory_bytes_used.set(0);
            self.paid.borrow_mut().clear();
            self.request_effects.reset();
            self.schedule_cut.set(false);
            self.scopes.borrow_mut().clear();
            self.scope_open.set(false);
            self.pending_usage.take();
            self.query_depth.set(0);
            self.tripped.set(PartialReasonSet::empty());
            self.active.set(true);
        }
        if cancelled {
            self.record_trip(PartialReasonSet::CANCELLED);
        }
        let mut entered_query_depth = false;
        let tripped = self.tripped.get();
        let trip = if !tripped.is_empty() {
            Some(tripped)
        } else if query_boundary && self.query_depth.get() >= self.query_depth_limit.get() {
            Some(self.record_trip(PartialReasonSet::CONNECTED_QUERY_DEPTH_LIMIT))
        } else {
            if query_boundary {
                self.query_depth.set(self.query_depth.get() + 1);
                entered_query_depth = true;
            }
            None
        };
        (
            ConnectedDemandGuard {
                ledger: self,
                root,
                entered_query_depth,
                _operation: None,
            },
            trip,
        )
    }

    /// The trip that refuses one more natively nested inline evaluation
    /// when `nesting` evaluations are already open on the native stack —
    /// `None` when it may run. Nesting takes the same cap as nested query
    /// boundaries: an evaluation that re-enters inline, without a query
    /// boundary, would otherwise grow the native stack unbounded. The
    /// refusal is the depth rail's typed incompleteness, sticky like every
    /// trip.
    pub(super) fn nesting_trip(&self, nesting: usize) -> Option<PartialReasonSet> {
        if nesting < usize::from(self.query_depth_limit.get()) {
            return None;
        }
        Some(self.record_trip(PartialReasonSet::CONNECTED_QUERY_DEPTH_LIMIT))
    }

    /// Whether this demand has ALREADY tripped. A sub-query suppressed under a
    /// tripped ledger is never evidence about the queried surface — the caller
    /// must report the budget, not a semantic verdict.
    pub(crate) fn has_tripped(&self) -> bool {
        !self.tripped.get().is_empty()
    }

    /// The trip of the ACTIVE demand, if any. `None` outside a demand, and
    /// `None` inside an untripped one.
    pub(crate) fn active_trip(&self) -> Option<PartialReasonSet> {
        self.active
            .get()
            .then(|| self.tripped.get())
            .filter(|tripped| !tripped.is_empty())
    }

    /// Charge one work unit. `Err` carries the sticky trip set that now
    /// forbids further work.
    pub fn charge(&self) -> Result<(), PartialReasonSet> {
        self.charge_observed(self.cancellation.is_cancelled())
    }

    /// [`Self::charge`] with the request's cancellation already read as
    /// `cancelled`.
    pub(super) fn charge_observed(&self, cancelled: bool) -> Result<(), PartialReasonSet> {
        verter_debug_assert!(
            self.active.get(),
            "connected work must be charged inside a connected-demand guard"
        );
        if cancelled {
            return Err(self.record_trip(PartialReasonSet::CANCELLED));
        }
        let tripped = self.tripped.get();
        if !tripped.is_empty() {
            return Err(tripped);
        }
        let work_used = self.work_used.get();
        if work_used >= self.work_limit.get() {
            return Err(self.record_trip(PartialReasonSet::PROJECTION_WORK_LIMIT));
        }
        self.work_used.set(work_used + 1);
        remember_usage(1, 0, 0);
        self.accrue(1, 0, 0);
        Ok(())
    }

    /// Charge `units` work units at once — the work a planned stage costs,
    /// paid before it runs. `Err` carries the sticky trip set when the
    /// demand cannot pay for all of it.
    pub fn charge_units(&self, units: usize) -> Result<(), PartialReasonSet> {
        let available = self.work_available()?;
        if units > available {
            return Err(self.record_trip(PartialReasonSet::PROJECTION_WORK_LIMIT));
        }
        self.commit(units);
        Ok(())
    }

    /// Add one structured relation comparison to the open recording, if
    /// any: a comparison the computation on top recorded itself.
    #[inline(always)]
    pub(crate) fn accrue_comparison(&self) {
        self.accrue_comparisons(1);
    }

    /// Record a batch of proven structured comparisons without opening a
    /// query or allocating a receipt for each rejected alternative.
    pub(crate) fn accrue_comparisons(&self, comparisons: u32) {
        if !self.scope_open.get() {
            return;
        }
        let cell = &self.pending_usage.comparisons;
        cell.set(cell.get().saturating_add(u64::from(comparisons)));
    }

    /// Construction room for a batch, before committing any reservation.
    pub(crate) fn construction_bytes_available(&self) -> usize {
        usize::try_from(self.bytes_limit.get().saturating_sub(self.enforced_bytes()))
            .unwrap_or(usize::MAX)
    }

    /// The construction bytes the byte allowance holds the demand to: every
    /// byte it charged but its advisory footprint.
    fn enforced_bytes(&self) -> u64 {
        self.bytes_used
            .get()
            .saturating_sub(self.advisory_bytes_used.get())
    }

    /// Add `work` units and `bytes` construction bytes, `advisory` of them
    /// advisory footprint, to the open recording, if any: the cost the
    /// computation on top of the scope stack performs itself.
    #[inline(always)]
    fn accrue(&self, work: usize, bytes: u64, advisory: u64) {
        if !self.scope_open.get() {
            return;
        }
        let usage = &self.pending_usage;
        if work != 0 {
            usage.work.set(usage.work.get().saturating_add(work as u64));
        }
        if bytes != 0 {
            usage.bytes.set(usage.bytes.get().saturating_add(bytes));
        }
        if advisory != 0 {
            usage
                .advisory_bytes
                .set(usage.advisory_bytes.get().saturating_add(advisory));
        }
    }

    /// Snapshot the remaining work available to a query-free terminal run. The
    /// caller commits exactly the units it consumes before any nested semantic
    /// dispatch.
    #[inline(always)]
    pub(crate) fn work_available(&self) -> Result<usize, PartialReasonSet> {
        verter_debug_assert!(
            self.active.get(),
            "connected work must be observed inside a connected-demand guard"
        );
        if self.cancellation.is_cancelled() {
            return Err(self.record_trip(PartialReasonSet::CANCELLED));
        }
        let tripped = self.tripped.get();
        if !tripped.is_empty() {
            return Err(tripped);
        }
        Ok(self.work_limit.get().saturating_sub(self.work_used.get()))
    }

    /// Commit work consumed against a snapshot taken by
    /// [`Self::work_available`].
    #[inline(always)]
    pub(crate) fn commit(&self, consumed: usize) {
        self.commit_usage(consumed, 0, 0);
    }

    /// Commit an admitted charge once to the ledger, span and open receipt.
    fn commit_usage(&self, work: usize, bytes: u64, advisory: u64) {
        if work == 0 && bytes == 0 {
            return;
        }
        verter_debug_assert!(self.active.get());
        if work != 0 {
            let work_used = self.work_used.get();
            verter_debug_assert!(work_used.saturating_add(work) <= self.work_limit.get());
            self.work_used.set(work_used + work);
        }
        if bytes != 0 {
            self.charge_bytes(bytes, advisory);
        }
        remember_usage(work, bytes, advisory);
        self.accrue(work, bytes, advisory);
    }

    /// Reserve `bytes` of construction before building what they hold,
    /// against the byte allowance less what the demand already reserved —
    /// never less its advisory footprint. `Err` carries the sticky trip set when the demand cannot pay for them
    /// ([`PartialReasonSet::CONNECTED_MEMORY_LIMIT`] when this reservation
    /// is the one refused).
    pub fn reserve_bytes(&self, bytes: usize) -> Result<(), PartialReasonSet> {
        verter_debug_assert!(
            self.active.get(),
            "construction bytes must be reserved inside a connected-demand guard"
        );
        if self.cancellation.is_cancelled() {
            return Err(self.record_trip(PartialReasonSet::CANCELLED));
        }
        let tripped = self.tripped.get();
        if !tripped.is_empty() {
            return Err(tripped);
        }
        let bytes = bytes as u64;
        match self.enforced_bytes().checked_add(bytes) {
            Some(enforced) if enforced <= self.bytes_limit.get() => {
                self.charge_bytes(bytes, 0);
                remember_usage(0, bytes, 0);
                self.accrue(0, bytes, 0);
                Ok(())
            }
            _ => Err(self.record_trip(PartialReasonSet::CONNECTED_MEMORY_LIMIT)),
        }
    }

    /// Pay for a canonical construction before it builds: `units` work
    /// units, refused past the work allowance, and `bytes` of construction
    /// footprint. The footprint is always charged; it is refused at the
    /// byte allowance only when the advisory construction-byte rail is
    /// enabled ([`Self::set_construction_byte_refusal_for_tests`]).
    /// Otherwise it is advisory: it never stops the construction, and no
    /// later reservation of the demand is refused for it. `Err` carries the
    /// sticky trip set, and nothing is charged for the refused part.
    pub(crate) fn charge_construction(
        &self,
        units: usize,
        bytes: usize,
    ) -> Result<(), PartialReasonSet> {
        let available = self.work_available()?;
        if units > available {
            return Err(self.record_trip(PartialReasonSet::PROJECTION_WORK_LIMIT));
        }
        if self.construction_byte_refusal.get() {
            // Work is paid before an enforced byte reservation, including
            // when the reservation refuses or observes cancellation.
            self.commit(units);
            return self.reserve_bytes(bytes);
        }
        let bytes = bytes as u64;
        self.commit_usage(units, bytes, bytes);
        Ok(())
    }

    /// Charge `bytes` construction bytes, `advisory` of them advisory
    /// footprint, without weighing them against the allowance.
    fn charge_bytes(&self, bytes: u64, advisory: u64) {
        verter_debug_assert!(advisory <= bytes);
        self.bytes_used
            .set(self.bytes_used.get().saturating_add(bytes));
        self.advisory_bytes_used
            .set(self.advisory_bytes_used.get().saturating_add(advisory));
    }

    /// What the active demand has charged so far.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn usage(&self) -> DemandUsage {
        DemandUsage {
            work: self.work_used.get(),
            bytes: self.bytes_used.get(),
        }
    }

    /// Whether the construction-byte allowance has tripped the demand.
    #[cfg(any(test, feature = "test-support"))]
    pub fn memory_limit_tripped(&self) -> bool {
        self.tripped
            .get()
            .contains(PartialReasonSet::CONNECTED_MEMORY_LIMIT)
    }

    /// The `(limit, actual, rail)` triple describing why the demand tripped,
    /// for the budget-exceeded carrier the dispatcher reports. Purely
    /// operational reporting — the ledger owns the numbers, so no consumer
    /// reads the counters directly.
    pub fn limit_report(&self, reasons: PartialReasonSet) -> (usize, u64, &'static str) {
        if !self.active.get() {
            return (0, 0, "unknown");
        }
        if reasons.contains(PartialReasonSet::CONNECTED_QUERY_DEPTH_LIMIT) {
            (
                usize::from(self.query_depth_limit.get()),
                u64::from(self.query_depth.get()),
                "connected-query-depth",
            )
        } else if reasons.contains(PartialReasonSet::CONNECTED_MEMORY_LIMIT) {
            (
                usize::try_from(self.bytes_limit.get()).unwrap_or(usize::MAX),
                self.enforced_bytes(),
                "construction-bytes",
            )
        } else {
            (
                self.work_limit.get(),
                self.work_used.get() as u64,
                "projection-work",
            )
        }
    }

    /// Replace the caps. Test-only, and refused inside an active demand so one
    /// run can never observe two different caps.
    #[cfg(any(test, feature = "test-support"))]
    pub fn set_limits_for_tests(&self, work: usize, depth: u16) {
        assert!(
            !self.active.get(),
            "test limits must be set before entering a connected demand"
        );
        self.work_limit.set(work);
        self.query_depth_limit.set(depth);
    }

    /// Replace the construction-byte allowance. Test-only, and refused inside
    /// an active demand.
    #[cfg(any(test, feature = "test-support"))]
    pub fn set_byte_limit_for_tests(&self, bytes: u64) {
        assert!(
            !self.active.get(),
            "test limits must be set before entering a connected demand"
        );
        self.bytes_limit.set(bytes);
    }

    /// Enable or disable the advisory construction-byte rail for the next
    /// connected demand: enabled, a canonical construction whose footprint
    /// passes the byte allowance is refused. Test-only, and refused inside
    /// an active demand.
    #[cfg(any(test, feature = "test-support"))]
    pub fn set_construction_byte_refusal_for_tests(&self, enabled: bool) {
        assert!(
            !self.active.get(),
            "test limits must be set before entering a connected demand"
        );
        self.construction_byte_refusal.set(enabled);
    }

    /// The construction bytes the last connected demand reserved. Test-only:
    /// kept after the demand ends, and reset when the next one enters.
    #[cfg(any(test, feature = "test-support"))]
    pub fn bytes_used_for_tests(&self) -> u64 {
        self.bytes_used.get()
    }

    /// The work units the last connected demand charged. Test-only: kept
    /// after the demand ends, and reset when the next one enters.
    #[cfg(any(test, feature = "test-support"))]
    pub fn work_used_for_tests(&self) -> usize {
        self.work_used.get()
    }

    /// Fold `reason` into the sticky trip set and return the widened set.
    pub(super) fn record_trip(&self, reason: PartialReasonSet) -> PartialReasonSet {
        verter_debug_assert!(
            self.active.get(),
            "an operational limit can trip only inside a connected demand"
        );
        let tripped = self.tripped.get().union(reason);
        self.tripped.set(tripped);
        remember_trip(reason);
        tripped
    }
}

/// The cost-receipt operations: recording each cold computation's
/// exclusive cost and admitting stored results by replaying their receipts.
impl ConnectedDemandLedger<'_> {
    /// Open the recording of a cold computation of `identity`: until it is
    /// sealed or abandoned, every charge the demand takes is its exclusive
    /// cost, except the charges of computations opened above it.
    pub fn open_cost_scope(&self, identity: CostIdentity) {
        self.resume_cost_scope(CostScope {
            identity,
            exclusive: LogicalUsage::default(),
            prerequisites: Vec::new(),
            tail_steps: 0,
        });
    }

    /// Refresh the mirror of "a recording is open".
    fn sync_top(&self, scopes: &[CostScope]) {
        self.scope_open.set(!scopes.is_empty());
    }

    /// Materialize the current recording's pending charges before it moves,
    /// is suspended, or lends its totals to a prefix recording. A transition
    /// already holds the stack, so charging itself never acquires that lock.
    fn flush_usage(&self, scopes: &mut [CostScope]) {
        if let Some(scope) = scopes.last_mut() {
            let usage = self.pending_usage.take();
            if usage != LogicalUsage::default() {
                #[cfg(test)]
                SCOPE_CHARGE_WRITES.with(|count| count.set(count.get() + 1));
                scope.exclusive = scope.exclusive.saturating_add(usage);
            }
        }
    }

    /// Take the open computation's recording off the stack without ending
    /// it: a suspended frame keeps its recording between its steps, so a
    /// sibling's charges never accrue to it. `None` when none is open.
    pub(crate) fn suspend_cost_scope(&self) -> Option<CostScope> {
        let mut scopes = self.scopes.borrow_mut();
        self.flush_usage(&mut scopes);
        let scope = scopes.pop();
        self.sync_top(&scopes);
        scope
    }

    /// Put a suspended recording back on top: its frame runs again.
    pub(crate) fn resume_cost_scope(&self, scope: CostScope) {
        let mut scopes = self.scopes.borrow_mut();
        self.flush_usage(&mut scopes);
        scopes.push(scope);
        self.sync_top(&scopes);
    }

    /// Spend one of `request`'s operations for the computation on top: the
    /// per-request projection fuse admits one more operator build. `true`
    /// when the request has now passed its cap.
    pub(crate) fn spend_request_operation(
        &self,
        request: &crate::request_budget::RequestBudget,
    ) -> bool {
        self.note_request_effects(1, 0);
        if self.scope_open.get() {
            let cell = &self.pending_usage.operations;
            cell.set(cell.get().saturating_add(1));
        }
        request.check_projection_op_count()
    }

    /// Record `operations` request operations this demand spent and `paid`
    /// computations it was first to pay in its request.
    pub(crate) fn note_request_effects(&self, operations: usize, paid: usize) {
        let effects = &self.request_effects;
        if operations != 0 {
            effects
                .operations
                .set(effects.operations.get().saturating_add(operations));
        }
        if paid != 0 {
            effects.paid.set(effects.paid.get().saturating_add(paid));
        }
    }

    /// What this connected demand itself has done to its request.
    pub(crate) fn request_effects(&self) -> RequestEffects {
        self.request_effects.get()
    }

    /// Record that one of this demand's reads was answered with a recursion
    /// carrier another task's schedule decided.
    pub(crate) fn note_schedule_cut(&self) {
        if self.active.get() {
            self.schedule_cut.set(true);
        }
        remember_schedule_cut();
    }

    /// Whether another task's schedule has cut this demand's evaluation:
    /// what it answered depends on that schedule.
    pub(crate) fn was_schedule_cut(&self) -> bool {
        self.schedule_cut.get()
    }

    /// Mark `identity` paid in `request`, counting it as this demand's
    /// effect when the demand is the first to pay it there.
    fn mark_request_paid(
        &self,
        request: &crate::request_budget::RequestBudget,
        identity: CostIdentity,
    ) {
        let added = request.mark_operations_paid(std::iter::once(identity));
        self.note_request_effects(0, added);
    }

    /// Record that the open computation consumed the result `receipt`
    /// costs, the way `nesting` names. Recorded however the result arrived
    /// — computed, served or already paid — so the receipt being built owes
    /// it in full.
    pub fn record_prerequisite(&self, receipt: &Arc<DemandCostReceipt>, nesting: Nesting) {
        if !self.scope_open.get() {
            return;
        }
        if let Some(scope) = self.scopes.borrow_mut().last_mut() {
            scope.prerequisites.push(CostDependency {
                receipt: Arc::clone(receipt),
                nesting,
            });
        }
    }

    /// Seal the open computation's recording: its receipt, now paid for
    /// this demand (and its request operations for `request`) together
    /// with its closure, which it consumed paid. `None` when no computation
    /// is recording.
    pub fn seal_cost_scope(
        &self,
        request: Option<&crate::request_budget::RequestBudget>,
    ) -> Option<Arc<DemandCostReceipt>> {
        let scope = self.suspend_cost_scope()?;
        self.paid.borrow_mut().insert(scope.identity.clone());
        if let Some(request) = request {
            self.mark_request_paid(request, scope.identity.clone());
        }
        Some(DemandCostReceipt::with_tail_steps(
            scope.identity,
            scope.exclusive,
            scope.prerequisites,
            scope.tail_steps,
        ))
    }

    /// How many computations are recording: the depth of the open
    /// recording on top.
    pub(crate) fn recording_depth(&self) -> usize {
        self.scopes.borrow().len()
    }

    /// Drop the open computation's recording without a receipt: it did not
    /// complete, so nothing about it may be served warm as paid. What it
    /// charged stays spent, and is its consumer's own cost: the consumer
    /// paid for a computation it then had to do without.
    pub fn abandon_cost_scope(&self) {
        if let Some(scope) = self.suspend_cost_scope() {
            self.fold_into_consumer(scope);
        }
    }

    /// Open the measurement of an inline part of the computation on top:
    /// until it closes ([`InlineCostRecording::close`]), the charges the
    /// demand takes and the results it consumes are measured apart, and
    /// stay the computation's own.
    pub(crate) fn open_inline_cost(&self) -> InlineCostRecording<'_> {
        let place = self.recording_depth();
        self.open_cost_scope(CostIdentity::of_key(InlineCostKey));
        InlineCostRecording {
            ledger: self,
            place,
            open: true,
        }
    }

    /// End the measurement at `place` unmeasured, with every recording
    /// opened above it: each is abandoned into the one below it.
    fn abandon_inline_cost(&self, place: usize) {
        // bounded-loop: at most the recordings open above `place`.
        while self.recording_depth() > place {
            self.abandon_cost_scope();
        }
    }

    /// Close the measurement [`Self::open_inline_cost`] opened at `place`:
    /// what it measured goes to the computation below exactly as if it had
    /// been charged there — its usage added, its tail runs joined, each
    /// result it consumed a prerequisite of its own — and is returned.
    /// `None`, closing nothing, when that measurement is not the recording
    /// on top.
    fn close_inline_cost(&self, place: usize) -> Option<InlineCost> {
        if self.recording_depth() != place + 1 {
            return None;
        }
        let scope = self.suspend_cost_scope()?;
        let cost = InlineCost {
            exclusive: scope.exclusive,
            tail_steps: scope.tail_steps,
            prerequisites: scope.prerequisites.clone(),
        };
        if let Some(consumer) = self.scopes.borrow_mut().last_mut() {
            consumer.exclusive = consumer.exclusive.saturating_add(scope.exclusive);
            consumer.tail_steps = consumer.tail_steps.max(scope.tail_steps);
            consumer.prerequisites.extend(scope.prerequisites);
        }
        Some(cost)
    }

    /// Perform `part`, a computation whose result the parts measured on
    /// top of it keep for one another, with the charges it takes going to
    /// the computation below those measurements: a measured part that
    /// performs it pays it once, and a part found by a measurement later
    /// performed again would find its result kept, so the measurement it
    /// is charged into never holds it.
    pub(crate) fn outside_inline_costs<T>(&self, part: impl FnOnce() -> T) -> T {
        if !self.scope_open.get() {
            return part();
        }
        let lifted = {
            let mut scopes = self.scopes.borrow_mut();
            let keep = scopes
                .iter()
                .rposition(|scope| !scope.identity.is_keyed_by::<InlineCostKey>())
                .map_or(0, |below| below + 1);
            if keep == scopes.len() {
                drop(scopes);
                return part();
            }
            self.flush_usage(&mut scopes);
            let lifted = scopes.split_off(keep);
            self.sync_top(&scopes);
            lifted
        };
        let value = part();
        let mut scopes = self.scopes.borrow_mut();
        self.flush_usage(&mut scopes);
        scopes.extend(lifted);
        self.sync_top(&scopes);
        value
    }

    /// Charge an inline part's measured cost again, as performing that part
    /// again would: its work units, construction bytes and request
    /// operations to the demand and its request, all of it, with its
    /// comparisons, tail runs and consumed results, to the computation on
    /// top. `false`, charging nothing, when the demand has tripped or been
    /// cancelled, lacks the room any of those charges needs, or a result the
    /// part consumed does not fit this reader's allowances
    /// ([`Self::footprint_refusal`]): performing the part again is then
    /// what meets the allowance.
    pub(crate) fn replay_inline_cost(
        &self,
        cost: &InlineCost,
        request: Option<&crate::request_budget::RequestBudget>,
    ) -> bool {
        if !self.tripped.get().is_empty() || self.cancellation.is_cancelled() {
            return false;
        }
        if cost.prerequisites.iter().any(|dependency| {
            self.footprint_refusal(&dependency.receipt, dependency.nesting)
                .is_some()
        }) {
            return false;
        }
        let usage = cost.exclusive;
        let work = usize::try_from(usage.work).unwrap_or(usize::MAX);
        if work > self.work_limit.get().saturating_sub(self.work_used.get()) {
            return false;
        }
        let enforced = usage.bytes.saturating_sub(usage.advisory_bytes);
        if self.enforced_bytes().saturating_add(enforced) > self.bytes_limit.get() {
            return false;
        }
        let operations = usize::try_from(usage.operations).unwrap_or(usize::MAX);
        if let Some(request) = request {
            let room = request
                .effective_projection_op_budget()
                .saturating_sub(request.projection_ops_executed_count());
            if operations > room {
                return false;
            }
        }
        self.commit_usage(work, usage.bytes, usage.advisory_bytes);
        if let Some(request) = request {
            // `operations` was checked against the request operations left.
            // bounded-loop: at most the request operations left in its allowance.
            for _ in 0..operations {
                self.spend_request_operation(request);
            }
        }
        if self.scope_open.get() {
            let comparisons = &self.pending_usage.comparisons;
            comparisons.set(comparisons.get().saturating_add(usage.comparisons));
            if let Some(scope) = self.scopes.borrow_mut().last_mut() {
                scope.tail_steps = scope.tail_steps.max(cost.tail_steps);
                scope
                    .prerequisites
                    .extend(cost.prerequisites.iter().cloned());
            }
        }
        true
    }

    /// Fold an ended recording into the recording below it, if any: the
    /// usage it charged, and the prerequisites it consumed — more than one
    /// as a single failed-prefix receipt
    /// ([`DemandCostReceipt::failed_prefix`]), so a chain of recordings
    /// that each fail moves a bounded list per level, never every
    /// prerequisite the levels above it already folded.
    fn fold_into_consumer(&self, scope: CostScope) {
        if let Some(consumer) = self.scopes.borrow_mut().last_mut() {
            consumer.exclusive = consumer.exclusive.saturating_add(scope.exclusive);
            consumer.tail_steps = consumer.tail_steps.max(scope.tail_steps);
            let mut prerequisites = scope.prerequisites;
            match prerequisites.len() {
                0 => {}
                1 => consumer.prerequisites.append(&mut prerequisites),
                _ => consumer.prerequisites.push(CostDependency {
                    receipt: DemandCostReceipt::failed_prefix(prerequisites),
                    nesting: Nesting::Folded,
                }),
            }
        }
    }

    /// Release the receipts an ended demand paid: each roots the operands
    /// its computation names, and the next root demand starts unpaid anyway.
    pub(super) fn release_paid(&self) {
        if !self.active.get() {
            let paid = std::mem::take(&mut *self.paid.borrow_mut());
            drop(paid);
        }
    }

    /// Whether this demand has paid `identity`'s receipt and its closure.
    pub fn is_paid(&self, identity: &CostIdentity) -> bool {
        self.paid.borrow().contains(identity)
    }

    /// The allowances this ledger answers to under `request`'s operation
    /// allowance: the spec its interned profile names.
    pub(crate) fn profile_spec(
        &self,
        request: Option<&crate::request_budget::RequestBudget>,
    ) -> BudgetProfileSpec {
        BudgetProfileSpec {
            work: self.work_limit.get(),
            bytes: self.bytes_limit.get(),
            construction_byte_refusal: self.construction_byte_refusal.get(),
            query_depth: self.query_depth_limit.get(),
            instantiation_depth: self.instantiation_depth_limit,
            tail_steps: self.tail_steps_limit,
            request_operations: request
                .map_or(0, |request| request.effective_projection_op_budget()),
            relation_comparisons: crate::semantic_query::checker_policy::relation_comparisons(),
            cost_model_revision: COST_MODEL_REVISION,
        }
    }

    /// Admit serving the result `receipt` costs without computing it, read
    /// the way `nesting` names: charge, in one admission, the exclusive
    /// usage of every computation in its closure this demand has not paid
    /// (and the request operations of every one `request` has not paid),
    /// and mark them paid. The receipt's footprint is checked against the
    /// reader's allowances even when its cost is paid: its nesting against
    /// the remaining query depth, its instantiation frames against the
    /// instantiation allowance at the reader's frame depth, its tail runs
    /// against the tail allowance. A refusal charges and marks nothing,
    /// and leaves the stored result untouched: this caller computes the
    /// result itself, and another demand with room may still serve it.
    /// Construction bytes are charged but never refuse: a complete result
    /// is not rejected at handoff for the bytes it took to build. Replayed
    /// charges belong to no open recording — the consumer records the
    /// receipt as a prerequisite instead. The unpaid closure's structured
    /// comparisons must fit a fresh relation check's whole allowance.
    pub fn replay_admit(
        &self,
        receipt: &Arc<DemandCostReceipt>,
        request: Option<&crate::request_budget::RequestBudget>,
        nesting: Nesting,
    ) -> Result<(), ReplayRefusal> {
        self.replay_admit_within(
            receipt,
            request,
            nesting,
            crate::semantic_query::checker_policy::relation_comparisons(),
        )
        .map(|_| ())
    }

    /// Why the result `receipt` costs, read the way `nesting` names, does
    /// not fit this reader's allowances whatever it has paid: its nesting
    /// past the remaining query depth, its instantiation frames past the
    /// instantiation allowance at the reader's frame depth, or its tail runs
    /// past the tail allowance. `None` when it fits.
    fn footprint_refusal(
        &self,
        receipt: &DemandCostReceipt,
        nesting: Nesting,
    ) -> Option<ReplayRefusal> {
        let footprint = receipt.footprint();
        // The reader's own nesting bounds where the result's drive, and so
        // each of its frames, would be entered from.
        if u32::from(self.query_depth.get())
            + u32::from(footprint.query_depth.max(footprint.frame_query_depth))
            > u32::from(self.query_depth_limit.get())
        {
            return Some(ReplayRefusal::Depth);
        }
        let first_frame = match nesting {
            Nesting::Frame { depth } => Some(depth),
            Nesting::Drive => Some(1),
            Nesting::InPlace | Nesting::Entered | Nesting::Folded => None,
        };
        if footprint.instantiation_required > self.instantiation_depth_limit
            || first_frame.is_some_and(|depth| {
                depth.saturating_add(u32::from(footprint.instantiation_height))
                    > self.instantiation_depth_limit
            })
        {
            return Some(ReplayRefusal::InstantiationDepth);
        }
        if footprint.tail_steps >= self.tail_steps_limit && footprint.tail_steps > 0 {
            return Some(ReplayRefusal::TailSteps);
        }
        None
    }

    /// [`Self::replay_admit`] for a reader whose relation check, if any,
    /// has `comparison_room` structured comparisons left: the unpaid
    /// closure's comparisons — what its cold run would have recorded into
    /// that check — must fit it. `Ok` carries those comparisons, which the
    /// reader's check records as its cold run would have.
    pub(crate) fn replay_admit_within(
        &self,
        receipt: &Arc<DemandCostReceipt>,
        request: Option<&crate::request_budget::RequestBudget>,
        nesting: Nesting,
        comparison_room: u32,
    ) -> Result<u32, ReplayRefusal> {
        let tripped = self.tripped.get();
        if !tripped.is_empty() || self.cancellation.is_cancelled() {
            return Err(ReplayRefusal::Tripped);
        }
        if let Some(refusal) = self.footprint_refusal(receipt, nesting) {
            return Err(refusal);
        }
        let operations_paid = request.map(|request| request.operations_paid());
        if self.paid.borrow().contains(receipt.identity())
            && operations_paid
                .as_ref()
                .is_none_or(|paid| paid.contains(receipt.identity()))
        {
            // The closure walk stops at a root paid in both ledgers.
            // Check its entry footprint above, then avoid allocating an
            // empty replay and repeating zero-charge bookkeeping.
            return Ok(0);
        }
        let work_room = self.work_limit.get().saturating_sub(self.work_used.get()) as u64;
        let operation_room = request.map_or(u64::MAX, |request| {
            request
                .effective_projection_op_budget()
                .saturating_sub(request.projection_ops_executed_count()) as u64
        });
        let (unpaid, unpaid_operations, total) = if receipt.prerequisites().is_empty() {
            // A leaf's closure is itself: what it owes is its own usage,
            // with no traversal to deduplicate.
            let identity = receipt.identity();
            let demand_paid = self.paid.borrow().contains(identity);
            let request_paid = operations_paid
                .as_ref()
                .is_none_or(|operations| operations.contains(identity));
            let exclusive = receipt.exclusive();
            let total = LogicalUsage {
                work: if demand_paid { 0 } else { exclusive.work },
                bytes: if demand_paid { 0 } else { exclusive.bytes },
                advisory_bytes: if demand_paid {
                    0
                } else {
                    exclusive.advisory_bytes
                },
                operations: if request_paid {
                    0
                } else {
                    exclusive.operations
                },
                comparisons: if demand_paid {
                    0
                } else {
                    exclusive.comparisons
                },
            };
            if total.work > work_room {
                return Err(ReplayRefusal::Work);
            }
            if total.operations > operation_room {
                return Err(ReplayRefusal::Operations);
            }
            if total.comparisons > u64::from(comparison_room) {
                return Err(ReplayRefusal::RelationComparisons);
            }
            (
                if demand_paid {
                    Vec::new()
                } else {
                    vec![identity.clone()]
                },
                if request_paid {
                    Vec::new()
                } else {
                    vec![identity.clone()]
                },
                total,
            )
        } else {
            let paid = self.paid.borrow();
            let mut unpaid: Vec<&CostIdentity> = Vec::new();
            let mut unpaid_operations: Vec<&CostIdentity> = Vec::new();
            let mut seen: FxHashSet<&CostIdentity> = FxHashSet::default();
            let mut total = LogicalUsage::default();
            let mut stack: Vec<&Arc<DemandCostReceipt>> = vec![receipt];
            while let Some(next) = stack.pop() {
                let identity = next.identity();
                let demand_paid = paid.contains(identity);
                let request_paid = operations_paid
                    .as_ref()
                    .is_none_or(|operations| operations.contains(identity));
                if (demand_paid && request_paid) || !seen.insert(identity) {
                    continue;
                }
                let exclusive = next.exclusive();
                let owed = LogicalUsage {
                    work: if demand_paid { 0 } else { exclusive.work },
                    bytes: if demand_paid { 0 } else { exclusive.bytes },
                    advisory_bytes: if demand_paid {
                        0
                    } else {
                        exclusive.advisory_bytes
                    },
                    operations: if request_paid {
                        0
                    } else {
                        exclusive.operations
                    },
                    comparisons: if demand_paid {
                        0
                    } else {
                        exclusive.comparisons
                    },
                };
                total = total.checked_add(owed).ok_or(ReplayRefusal::Work)?;
                // The rest of the closure can only add to what is owed.
                if total.work > work_room {
                    return Err(ReplayRefusal::Work);
                }
                if total.operations > operation_room {
                    return Err(ReplayRefusal::Operations);
                }
                if total.comparisons > u64::from(comparison_room) {
                    return Err(ReplayRefusal::RelationComparisons);
                }
                if !demand_paid {
                    unpaid.push(identity);
                }
                if !request_paid {
                    unpaid_operations.push(identity);
                }
                stack.extend(
                    next.prerequisites()
                        .iter()
                        .rev()
                        .map(|dependency| &dependency.receipt),
                );
            }
            (
                unpaid.into_iter().cloned().collect::<Vec<_>>(),
                unpaid_operations.into_iter().cloned().collect::<Vec<_>>(),
                total,
            )
        };
        if let (Some(request), Some(operations_paid)) = (request, operations_paid) {
            let newly_paid = unpaid_operations.len();
            if !request.admit_replayed_operations(
                total.operations,
                operations_paid,
                unpaid_operations,
            ) {
                return Err(ReplayRefusal::Operations);
            }
            self.note_request_effects(
                usize::try_from(total.operations).unwrap_or(usize::MAX),
                newly_paid,
            );
        }
        let replayed_work = total.work as usize;
        let replayed_bytes = total.bytes;
        self.work_used.set(self.work_used.get() + replayed_work);
        self.charge_bytes(replayed_bytes, total.advisory_bytes);
        remember_usage(replayed_work, replayed_bytes, total.advisory_bytes);
        self.paid.borrow_mut().extend(unpaid);
        // Within `comparison_room`, so within `u32`.
        Ok(u32::try_from(total.comparisons).unwrap_or(u32::MAX))
    }
}

impl<'a> ConnectedDemandLedger<'a> {
    /// Begin recording the materialized prefixes of the computation
    /// recording at `depth`: what it charged before now stays its own, and
    /// each [`PrefixRecording::seal_prefix`] seals what it charged since
    /// the previous prefix as that prefix's receipt. `None` when no
    /// computation is recording at `depth`.
    pub(crate) fn record_prefixes(&self, depth: usize) -> Option<PrefixRecording<'_, 'a>> {
        let mut scopes = self.scopes.borrow_mut();
        if depth == 0 || scopes.len() != depth {
            return None;
        }
        self.flush_usage(&mut scopes);
        let scope = scopes.last_mut()?;
        let held = CostScope {
            identity: scope.identity.clone(),
            exclusive: std::mem::take(&mut scope.exclusive),
            prerequisites: std::mem::take(&mut scope.prerequisites),
            tail_steps: std::mem::take(&mut scope.tail_steps),
        };
        Some(PrefixRecording {
            ledger: self,
            depth,
            held: Some(held),
        })
    }
}

/// The prefixes one computation materializes on its way to its result,
/// each sealed as a receipt of its own ([`ConnectedDemandLedger::record_prefixes`]).
/// What the computation charged before the first prefix stays its own, and
/// is put back on its recording when this ends, however it ends.
pub(crate) struct PrefixRecording<'l, 'a> {
    ledger: &'l ConnectedDemandLedger<'a>,
    depth: usize,
    held: Option<CostScope>,
}

impl PrefixRecording<'_, '_> {
    /// Seal what the computation charged since the previous prefix — with
    /// that prefix's receipt as its prerequisite — as the receipt of the
    /// prefix `identity` it just materialized, paid for this demand and
    /// `request` exactly as a computed result is; the computation goes on
    /// with that receipt as its prerequisite. `None` when the computation
    /// is no longer the recording on top.
    pub(crate) fn seal_prefix(
        &self,
        identity: CostIdentity,
        request: Option<&crate::request_budget::RequestBudget>,
    ) -> Option<Arc<DemandCostReceipt>> {
        let ledger = self.ledger;
        let receipt = {
            let mut scopes = ledger.scopes.borrow_mut();
            if scopes.len() != self.depth {
                return None;
            }
            ledger.flush_usage(&mut scopes);
            let scope = scopes.last_mut()?;
            let receipt = DemandCostReceipt::with_tail_steps(
                identity.clone(),
                std::mem::take(&mut scope.exclusive),
                std::mem::take(&mut scope.prerequisites),
                std::mem::take(&mut scope.tail_steps),
            );
            scope.prerequisites.push(CostDependency {
                receipt: Arc::clone(&receipt),
                nesting: Nesting::InPlace,
            });
            receipt
        };
        ledger.paid.borrow_mut().insert(identity.clone());
        if let Some(request) = request {
            ledger.mark_request_paid(request, identity);
        }
        Some(receipt)
    }
}

impl Drop for PrefixRecording<'_, '_> {
    fn drop(&mut self) {
        let Some(held) = self.held.take() else {
            return;
        };
        let mut scopes = self.ledger.scopes.borrow_mut();
        if let Some(scope) = scopes
            .get_mut(self.depth - 1)
            .filter(|scope| scope.identity == held.identity)
        {
            scope.exclusive = scope.exclusive.saturating_add(held.exclusive);
            scope.prerequisites.extend(held.prerequisites);
            scope.tail_steps = scope.tail_steps.max(held.tail_steps);
        }
    }
}

/// The recording of one cold computation, open until its build ends. A
/// recording dropped without [`Self::finish`] — a panic, an early return —
/// is abandoned, so its charges stay its consumer's.
pub(crate) struct CostRecording<'l, 'a> {
    ledger: &'l ConnectedDemandLedger<'a>,
    open: bool,
}

impl ConnectedDemandLedger<'_> {
    /// Open the recording of a cold computation of `identity`.
    pub(crate) fn record_cost(&self, identity: CostIdentity) -> CostRecording<'_, '_> {
        self.open_cost_scope(identity);
        CostRecording {
            ledger: self,
            open: true,
        }
    }

    /// Whether a connected demand is active.
    pub(crate) fn is_active(&self) -> bool {
        self.active.get()
    }

    /// The work units and construction bytes the active demand charged,
    /// and the part of those bytes that is advisory footprint.
    pub(crate) fn charged(&self) -> (usize, u64, u64) {
        (
            self.work_used.get(),
            self.bytes_used.get(),
            self.advisory_bytes_used.get(),
        )
    }

    /// Leave the active demand as a sealed refusal's evaluation left it:
    /// its work and bytes charged, `advisory` of those bytes advisory
    /// footprint, and `trip` tripped.
    pub(crate) fn apply_refusal(
        &self,
        work: usize,
        bytes: u64,
        advisory: u64,
        trip: PartialReasonSet,
    ) {
        self.work_used
            .set(self.work_used.get().saturating_add(work));
        self.charge_bytes(bytes, advisory);
        remember_usage(work, bytes, advisory);
        self.record_trip(trip);
    }

    /// Whether no computation is recording.
    pub(crate) fn is_unrecorded(&self) -> bool {
        !self.scope_open.get()
    }
}

impl CostRecording<'_, '_> {
    /// End the recording: a complete computation seals its receipt (paid
    /// for this demand, its operations for `request`); an incomplete one is
    /// abandoned and leaves none.
    pub(crate) fn finish(
        mut self,
        complete: bool,
        request: Option<&crate::request_budget::RequestBudget>,
    ) -> Option<Arc<DemandCostReceipt>> {
        self.open = false;
        if complete {
            self.ledger.seal_cost_scope(request)
        } else {
            self.ledger.abandon_cost_scope();
            None
        }
    }
}

impl Drop for CostRecording<'_, '_> {
    fn drop(&mut self) {
        if self.open {
            self.ledger.abandon_cost_scope();
        }
    }
}

/// Panic-safe lifetime of one connected semantic demand. The outermost
/// dispatch or direct projector installs the demand; nested query and worklist
/// entries join it without holding a `RefCell` borrow across semantic work.
pub struct ConnectedDemandGuard<'g> {
    ledger: &'g ConnectedDemandLedger<'g>,
    root: bool,
    entered_query_depth: bool,
    /// The dispatcher operation a root demand is: its construction owner
    /// and the receipts it paid are released when the demand ends, never
    /// kept by the dispatcher.
    _operation: Option<super::DispatchOperation<'g>>,
}

impl<'g> ConnectedDemandGuard<'g> {
    pub(super) fn is_root(&self) -> bool {
        self.root
    }

    /// Run this root demand as one dispatcher operation.
    pub(super) fn own_operation(&mut self, operation: super::DispatchOperation<'g>) {
        self._operation = Some(operation);
    }
}

impl Drop for ConnectedDemandGuard<'_> {
    fn drop(&mut self) {
        if self.entered_query_depth {
            self.ledger
                .query_depth
                .set(self.ledger.query_depth.get().saturating_sub(1));
        }
        if self.root {
            verter_debug_assert!(
                self.ledger.query_depth.get() == 0,
                "connected-demand root dropped while a nested query boundary remained active"
            );
            self.ledger.active.set(false);
        }
    }
}

/// An active connected-demand root for a component-meta test. Dropping the
/// token ends the root.
#[cfg(any(test, feature = "test-support"))]
pub struct ComponentMetaDemandRoot<'a> {
    ledger: &'a ConnectedDemandLedger<'a>,
    _guard: ConnectedDemandGuard<'a>,
}

#[cfg(any(test, feature = "test-support"))]
impl ComponentMetaDemandRoot<'_> {
    /// Charge this root. Panics when the demand refuses the work or the bytes.
    pub fn charge(&self, work: usize, bytes: usize, trip: PartialReasonSet) {
        if work > 0 {
            self.ledger
                .charge_units(work)
                .unwrap_or_else(|refused| panic!("test work charge refused: {refused:?}"));
        }
        if bytes > 0 {
            self.ledger
                .reserve_bytes(bytes)
                .unwrap_or_else(|refused| panic!("test byte reserve refused: {refused:?}"));
        }
        if !trip.is_empty() {
            self.ledger.record_trip(trip);
        }
    }

    /// One more work unit on this root.
    pub fn try_charge(&self) -> Result<(), PartialReasonSet> {
        self.ledger.charge()
    }

    /// Work, construction bytes, and the sticky trip charged on this root.
    pub fn charges(&self) -> (usize, u64, PartialReasonSet) {
        (
            self.ledger.work_used.get(),
            self.ledger.bytes_used.get(),
            self.ledger.tripped.get(),
        )
    }
}

#[cfg(any(test, feature = "test-support"))]
impl ConnectedDemandLedger<'_> {
    /// Enter a root and run `body` inside it.
    pub fn with_root_for_test<R>(
        &self,
        query_boundary: bool,
        body: impl FnOnce(&ComponentMetaDemandRoot<'_>) -> R,
    ) -> R {
        let (guard, trip) = self.enter(query_boundary);
        assert!(
            trip.is_none(),
            "component-meta test root tripped on entry: {trip:?}"
        );
        body(&ComponentMetaDemandRoot {
            ledger: self,
            _guard: guard,
        })
    }
}

/// Note a schedule cut on the open component-meta span.
#[cfg(any(test, feature = "test-support"))]
pub fn note_component_meta_schedule_cut_for_test() {
    remember_schedule_cut();
}

#[cfg(test)]
mod ownership_tests {
    use super::*;
    use crate::semantic_query::{LiteralValue, SemanticNodeData, SemanticQueryKey};
    use crate::semantic_query_memo::graph_fixture::GraphFixture;

    #[test]
    fn counted_failed_prefix_receipt_retains_its_materialized_operand() {
        let harness = GraphFixture::new();
        let receipt = harness.run(|graph| {
            let flags = crate::resolver_core::resolver_context::RequestFlags::new(
                crate::resolver_core::resolver_context::ProjectGenerationClock::new(Arc::new(
                    std::sync::atomic::AtomicU64::new(0),
                )),
            );
            let ledger = ConnectedDemandLedger::new(DemandCancellation::from_flags(&flags));
            ledger.set_limits_for_tests(1, 8);
            let (guard, trip) = ledger.enter(false);
            assert!(trip.is_none());
            ledger.open_cost_scope(CostIdentity::new(Arc::<[u8]>::from([1])));
            let prefixes = ledger.record_prefixes(1).unwrap();
            ledger.charge().unwrap();
            let node = graph.intern_node(SemanticNodeData::Literal(LiteralValue::String(
                "materialized prefix".into(),
            )));
            let identity = graph
                .prepare_rooted_key(SemanticQueryKey::ClassifyTruthinessDomain { subject: node });
            let receipt = prefixes
                .seal_prefix(CostIdentity::of_producer(identity.as_task_producer()), None)
                .unwrap();
            assert_eq!(receipt.exclusive().work, 1);
            assert!(ledger
                .charge()
                .unwrap_err()
                .contains(PartialReasonSet::PROJECTION_WORK_LIMIT));
            drop(prefixes);
            ledger.abandon_cost_scope();
            drop(guard);
            receipt
        });
        assert_eq!(
            harness.live_nodes(),
            1,
            "only the failed evaluation's returned prefix receipt remains"
        );
        drop(receipt);
        assert_eq!(harness.live_nodes(), 0);
    }
}

#[cfg(test)]
mod envelope_tests {
    use super::*;
    use crate::semantic_query::checker_policy::{INSTANTIATION_COUNT, RELATION_COMPARISONS};

    /// The checker counts relation comparisons and instantiations against
    /// separate limits, so one demand that spends both allowances in full is
    /// still one the checker answers: neither operational rail refuses it.
    #[test]
    fn a_demand_spending_both_checker_allowances_is_not_refused() {
        let flags = crate::resolver_core::resolver_context::RequestFlags::new(
            crate::resolver_core::resolver_context::ProjectGenerationClock::new(Arc::new(
                std::sync::atomic::AtomicU64::new(0),
            )),
        );
        let ledger = ConnectedDemandLedger::new(DemandCancellation::from_flags(&flags));
        let (guard, trip) = ledger.enter(false);
        assert!(trip.is_none());

        ledger
            .charge_units(INSTANTIATION_COUNT as usize * WORK_UNITS_PER_INSTANTIATION)
            .expect("the instantiation allowance's work is granted");
        ledger
            .charge_units(RELATION_COMPARISONS as usize * (RELATION_UNITS_PER_COMPARISON + 1))
            .expect("the relation allowance's work is granted beside it");

        let reserve =
            |bytes: u64| ledger.reserve_bytes(usize::try_from(bytes).expect("a 64-bit test host"));
        reserve(u64::from(INSTANTIATION_COUNT) * CONSTRUCTION_BYTES_PER_INSTANTIATION as u64)
            .expect("the instantiation allowance's bytes are granted");
        reserve(u64::from(RELATION_COMPARISONS) * RELATION_PAIR_BYTES as u64)
            .expect("the relation allowance's bytes are granted beside them");
        drop(guard);
    }
}

#[cfg(test)]
mod advisory_bytes_tests {
    use super::*;

    /// Charging a run does not edit the locked recording for each operation.
    /// Suspending and resuming still attribute every charge to its producer.
    #[test]
    fn repeated_charges_commit_once_per_recording_interval() {
        let flags = crate::resolver_core::resolver_context::RequestFlags::new(
            crate::resolver_core::resolver_context::ProjectGenerationClock::new(Arc::new(
                std::sync::atomic::AtomicU64::new(0),
            )),
        );
        let ledger = ConnectedDemandLedger::new(DemandCancellation::from_flags(&flags));
        let (_guard, trip) = ledger.enter(false);
        assert!(trip.is_none());
        ledger.open_cost_scope(CostIdentity::new(Arc::<[u8]>::from([1])));
        let request = crate::request_budget::RequestBudget::new(352);
        SCOPE_CHARGE_WRITES.with(|count| count.set(0));
        // bounded-loop: fixed batch of parent charges before its child opens.
        for _ in 0..128 {
            ledger.charge_construction(1, 100).unwrap();
            ledger.accrue_comparisons(2);
            assert!(!ledger.spend_request_operation(&request));
        }
        ledger.open_cost_scope(CostIdentity::new(Arc::<[u8]>::from([2])));
        // bounded-loop: fixed batch of child charges before suspension.
        for _ in 0..128 {
            ledger.charge_construction(1, 100).unwrap();
            ledger.accrue_comparisons(2);
            assert!(!ledger.spend_request_operation(&request));
        }
        let child = ledger.suspend_cost_scope().unwrap();
        assert_eq!(child.exclusive.work, 128);
        assert_eq!(child.exclusive.bytes, 12_800);
        // bounded-loop: fixed batch charged to the resumed parent.
        for _ in 0..64 {
            ledger.charge_construction(1, 100).unwrap();
            ledger.accrue_comparisons(2);
            assert!(!ledger.spend_request_operation(&request));
        }
        ledger.resume_cost_scope(child);
        // bounded-loop: fixed batch charged to the resumed child.
        for _ in 0..32 {
            ledger.charge_construction(1, 100).unwrap();
            ledger.accrue_comparisons(2);
            assert!(!ledger.spend_request_operation(&request));
        }
        let child = ledger.seal_cost_scope(None).unwrap();
        assert_eq!(child.exclusive().work, 160);
        assert_eq!(child.exclusive().bytes, 16_000);
        assert_eq!(child.exclusive().advisory_bytes, 16_000);
        assert_eq!(child.exclusive().comparisons, 320);
        assert_eq!(child.exclusive().operations, 160);
        ledger.record_prerequisite(&child, Nesting::InPlace);
        let parent = ledger.seal_cost_scope(None).unwrap();
        assert_eq!(parent.exclusive().work, 192);
        assert_eq!(parent.exclusive().bytes, 19_200);
        assert_eq!(parent.exclusive().comparisons, 384);
        assert_eq!(parent.exclusive().operations, 192);
        assert_eq!(request.projection_ops_executed_count(), 352);
        assert_eq!(ledger.request_effects().operations, 352);
        assert_eq!(parent.closure_usage().work, 352);
        assert_eq!(parent.closure_usage().bytes, 35_200);
        let writes = SCOPE_CHARGE_WRITES.with(Cell::get);
        eprintln!("recording_charge_writes={writes}");
        assert!(
            writes <= 4,
            "charges must batch at scope transitions: {writes}"
        );

        ledger.open_cost_scope(CostIdentity::new(Arc::<[u8]>::from([3])));
        ledger.charge_units(3).unwrap();
        let prefixes = ledger.record_prefixes(1).unwrap();
        ledger.charge_units(5).unwrap();
        let prefix = prefixes
            .seal_prefix(CostIdentity::new(Arc::<[u8]>::from([4])), None)
            .unwrap();
        assert_eq!(prefix.exclusive().work, 5);
        ledger.charge_units(7).unwrap();
        drop(prefixes);
        let receipt = ledger.seal_cost_scope(None).unwrap();
        assert_eq!(receipt.exclusive().work, 10);
        assert_eq!(receipt.closure_usage().work, 15);
    }

    /// A canonical construction's footprint, charged while the advisory
    /// construction-byte rail is off, is accounted in the demand's bytes but
    /// never enforced: a reservation after it still has the whole byte
    /// allowance, whether the footprint was charged cold, replayed from the
    /// computation's receipt, or applied from a sealed refusal. Enabled, the
    /// rail refuses the same footprint.
    #[test]
    fn an_advisory_footprint_never_refuses_a_later_reservation() {
        let flags = crate::resolver_core::resolver_context::RequestFlags::new(
            crate::resolver_core::resolver_context::ProjectGenerationClock::new(Arc::new(
                std::sync::atomic::AtomicU64::new(0),
            )),
        );
        let ledger = ConnectedDemandLedger::new(DemandCancellation::from_flags(&flags));
        ledger.set_byte_limit_for_tests(100);

        let receipt = {
            let (_guard, trip) = ledger.enter(false);
            assert!(trip.is_none());
            ledger.open_cost_scope(CostIdentity::new(Arc::<[u8]>::from([7])));
            SCOPE_CHARGE_WRITES.with(|count| count.set(0));
            let (charged, usage) =
                with_connected_work_capture_for_tests(|| ledger.charge_construction(1, 1_000));
            charged.expect("the rail is off: the footprint never refuses");
            assert_eq!(
                usage,
                DemandUsage {
                    work: 1,
                    bytes: 1_000
                }
            );
            let writes = SCOPE_CHARGE_WRITES.with(Cell::get);
            eprintln!("construction_scope_writes={writes}");
            assert_eq!(
                writes, 0,
                "charging does not edit the locked scope until its transition"
            );
            ledger
                .reserve_bytes(100)
                .expect("the whole allowance remains after the advisory footprint");
            assert_eq!(ledger.bytes_used_for_tests(), 1_100, "both are accounted");
            assert!(ledger
                .reserve_bytes(1)
                .unwrap_err()
                .contains(PartialReasonSet::CONNECTED_MEMORY_LIMIT));
            let receipt = ledger
                .seal_cost_scope(None)
                .expect("a complete computation");
            assert_eq!(SCOPE_CHARGE_WRITES.with(Cell::get), 1);
            receipt
        };
        assert_eq!(receipt.exclusive().work, 1);
        assert_eq!(receipt.exclusive().bytes, 1_100);
        assert_eq!(receipt.exclusive().advisory_bytes, 1_000);

        {
            let (_guard, trip) = ledger.enter(false);
            assert!(trip.is_none());
            ledger
                .replay_admit(&receipt, None, Nesting::InPlace)
                .expect("a replay is never refused for its bytes");
            assert_eq!(ledger.bytes_used_for_tests(), 1_100);
            ledger
                .reserve_bytes(0)
                .expect("the replayed footprint is advisory, as it was cold");
            assert!(
                ledger.reserve_bytes(1).is_err(),
                "the replayed reservation is still enforced"
            );
        }

        {
            let (_guard, trip) = ledger.enter(false);
            assert!(trip.is_none());
            ledger.apply_refusal(0, 1_000, 1_000, PartialReasonSet::empty());
            ledger
                .reserve_bytes(100)
                .expect("a sealed refusal's advisory footprint is not enforced either");
        }

        ledger.set_construction_byte_refusal_for_tests(true);
        let (_guard, trip) = ledger.enter(false);
        assert!(trip.is_none());
        assert!(ledger
            .charge_construction(1, 1_000)
            .unwrap_err()
            .contains(PartialReasonSet::CONNECTED_MEMORY_LIMIT));
    }
}

#[cfg(test)]
mod inline_cost_tests {
    use super::*;

    fn with_ledger(run: impl FnOnce(&ConnectedDemandLedger<'_>)) {
        let flags = crate::resolver_core::resolver_context::RequestFlags::new(
            crate::resolver_core::resolver_context::ProjectGenerationClock::new(Arc::new(
                std::sync::atomic::AtomicU64::new(0),
            )),
        );
        let ledger = ConnectedDemandLedger::new(DemandCancellation::from_flags(&flags));
        let (_guard, trip) = ledger.enter(false);
        assert!(trip.is_none());
        ledger.open_cost_scope(CostIdentity::new(Arc::<[u8]>::from([1])));
        run(&ledger);
    }

    /// A measurement keeps what its part charged, the computation below it
    /// keeps that too, and a part every measured part shares is charged to
    /// the computation alone. Replaying the measurement charges the demand
    /// its work again, and refuses, charging nothing, past the allowance.
    #[test]
    fn a_measurement_replays_what_its_part_charged_and_no_shared_part() {
        with_ledger(|ledger| {
            let outer = ledger.open_inline_cost();
            ledger.charge().unwrap();
            let inner = ledger.open_inline_cost();
            ledger.charge().unwrap();
            ledger.outside_inline_costs(|| {
                ledger.charge_units(5).unwrap();
            });
            let inner = inner.close().expect("the inner measurement is on top");
            let outer = outer.close().expect("the outer measurement is on top");
            assert_eq!((inner.exclusive.work, outer.exclusive.work), (1, 2));
            let computation = ledger.suspend_cost_scope().unwrap();
            assert_eq!(computation.exclusive.work, 7);
            assert_eq!(ledger.recording_depth(), 0);

            let used = ledger.work_used.get();
            assert!(ledger.replay_inline_cost(&outer, None));
            assert_eq!(ledger.work_used.get(), used + 2);
            ledger.work_limit.set(ledger.work_used.get() + 1);
            assert!(!ledger.replay_inline_cost(&outer, None));
            assert_eq!(ledger.work_used.get(), used + 2);
        });
    }

    /// Measurements dropped unclosed, the outer first as an unwinding
    /// stack of them drops, end with every recording opened above them,
    /// each abandoned into the one below: the computation keeps every
    /// charge and is on top again.
    #[test]
    fn measurements_dropped_unclosed_leave_the_computation_on_top() {
        with_ledger(|ledger| {
            let mut measurements = vec![ledger.open_inline_cost()];
            ledger.charge().unwrap();
            measurements.push(ledger.open_inline_cost());
            ledger.charge().unwrap();
            drop(measurements);
            assert_eq!(ledger.recording_depth(), 1);
            let computation = ledger.suspend_cost_scope().unwrap();
            assert_eq!(computation.exclusive.work, 2);
        });
    }
}
