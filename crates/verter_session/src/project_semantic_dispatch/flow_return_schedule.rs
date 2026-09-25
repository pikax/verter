//! The flow-return callee schedule: nested callee evaluation on an
//! explicit work stack instead of the native one.
//!
//! A frame's body demands a callee's return where the call sits, and the
//! callee's body is evaluated there, inside the caller's evaluation — for
//! a generic or imported call, beneath the `TypeOf` and `LowerLocator`
//! queries that lower `typeof callee`, and again under the call's
//! instantiation. Left to that alone, a linear call chain nests one native
//! evaluation (and, through a `typeof` lowering, two connected queries) per
//! level, so its length is bounded by the native stack and the
//! connected-query depth guard rather than by the work it does.
//!
//! Before a frame evaluates its body, the schedule discovers the callee
//! returns that body will demand and evaluates them bottom-up from an
//! explicit stack, each through the ordinary inline path
//! ([`ProjectSemanticDispatch::execute_flow_return`]). A callee that closes
//! as its own proven SCC root is recorded as a reusable completed member
//! exactly as it is when evaluated at its call, so the body's demand — and
//! every scheduled caller's — reuses it instead of recursing. A chain then
//! costs the connected work it always did, and the native stack and
//! connected-query depth of a single level. A callee with nothing left to
//! evaluate beneath it is not scheduled at all: its own demand evaluates it
//! one level deep, in the body's order.
//!
//! What the schedule may change is the ORDER of evaluation, never its
//! meaning:
//!
//! - **Discovery is the evaluator's own.** A callee is scheduled for a call
//!   the frame's lowered slice selects whose free, bare-identifier callee
//!   the call rails resolve to a function with a body-derived return: the
//!   direct-call rail's same-file target, or the function the file's owner
//!   scope resolves the name to through its imports, as the rail lowering
//!   `typeof callee` resolves it. A call inside another call's argument is
//!   one the body evaluates when every call around it resolves through the
//!   call executor, which evaluates its arguments in the frame. A bare
//!   `typeof name` in a type position the evaluation reads — the frame's
//!   parameter list, the annotation of a binding whose value the slice
//!   selects, a type a selected expression carries — demands `name`'s
//!   return exactly as a call does. Each key is minted by the one key
//!   construction the rails and the signature lowering share. An
//!   instantiated frame demands what its uninstantiated frame's call
//!   resolution demanded, under the instantiation's arguments. When that
//!   frame evaluated in this session, its resolution's own answers were
//!   recorded as it made them and are carried over by binder name — the
//!   mapping the instantiated frame's binder environment applies. When its
//!   answer was already warm, the instantiation is read from the two
//!   signatures the call executor reads, for a call — by the frame or by a
//!   function value it composes — that forwards the frame's own binders.
//!   A call no rail resolves to a named function, an argument of any other
//!   call, and an instantiation of any other shape are not predicted.
//! - **What discovery does not predict is found where it is demanded.**
//!   Under an open schedule, an inline evaluation of a callee return no
//!   schedule has settled does not nest. Beneath a scheduled evaluation —
//!   a probe — it is recorded and refused, typed and inside the probe's
//!   private rails; the recorded returns are walked and evaluated first,
//!   and the probed entry is evaluated again. Anywhere else it is
//!   scheduled where it is made, its own evaluation probed in turn. An
//!   instantiation read from an argument of any form, or a call nested
//!   where discovery does not look, then costs one refused probe per level
//!   rather than one native level.
//! - **Cycles stay with the SCC machinery.** A callee whose discovered
//!   closure reaches a frame in flight, an open member of a pending
//!   component, or an entry below it on the explicit stack is never
//!   evaluated out of order: the component's first-discovered member is
//!   evaluated through the ordinary path, where the re-entry intercept
//!   holds each back-edge and the component's close discharges it.
//! - **A speculative evaluation leaves no rail unless it is reused.** Each
//!   scheduled evaluation runs under a private build-local frame, a fresh
//!   cold-compute completeness scope and a deferred request sticky. A
//!   reusable member is clean by definition, and its reuse replays its
//!   reads into whichever scope demands it; any other outcome is dropped,
//!   the rest of that branch is left to the recursive path, and the body's
//!   own demand re-derives the outcome with its rails.
//! - **Budget and cancellation stay the ledger's.** A scheduled evaluation
//!   charges the connected demand exactly as a nested one does, and a trip
//!   ends the schedule; the body then meets the same sticky trip.
//!
//! What the schedule leaves to the recursive path stays bounded: an inline
//! flow evaluation that would open more nested frames than the connected
//! demand's depth cap is refused with the depth rail's typed
//! incompleteness ([`ProjectSemanticDispatch::refuse_nested_flow_evaluation`])
//! before the native stack can run out — the backstop beside the
//! connected-query depth guard, which bounds nesting through queries.
//!
//! The schedule keeps one transaction-local session while any frame is
//! evaluating: the callee returns it has already settled — evaluated, left
//! to the body, or abandoned — so a nested frame's schedule never
//! re-evaluates them, the instantiated demands each uninstantiated frame
//! made, and the probes in progress. The session is cleared when the
//! outermost frame finishes.

use std::cell::RefCell;
use std::sync::Arc;

use rustc_hash::{FxHashMap, FxHashSet};
use verter_semantic::analysis::flow::flow_ir::{FlowCallee, FlowEffect, FlowExprRole, FlowSliceIR};
use verter_semantic::analysis::flow::{FrameSpan, NameMeaning};
use verter_semantic::analysis::function_program::{
    FunctionBindingKind, FunctionEffectCallee, FunctionProgramEntry, FunctionProgramIndex,
    FunctionProgramKey, FunctionReferenceBinding, FunctionTypeQueryPosition,
};
use verter_type_expr::facts::{FlowFunctionReturnIdentity, FunctionPartIdentity};

use super::super::dispatch_txn::{CheckerDispatchTransaction, ObligationIdentity};
use super::super::{BuildLocalTaintGuard, ProjectSemanticDispatch};
use crate::resolver_core::bare_name_resolve::DeclarationScopePayload;
use crate::semantic_query::{FlowReturnKey, FlowReturnStep, SemanticNodeData, SemanticNodeId};

/// The schedule's transaction-local session state (see the module
/// documentation).
#[derive(Debug, Default)]
pub(crate) struct FlowScheduleSession {
    /// Frames evaluating under an open schedule scope.
    open: u32,
    /// Callee returns an earlier schedule of this session already settled.
    settled: FxHashSet<FlowReturnKey>,
    /// The instantiated callee returns each uninstantiated frame demanded
    /// while it evaluated, in demand order.
    demands: FxHashMap<FlowReturnKey, Vec<FlowReturnKey>>,
    /// The scheduled evaluations in progress, innermost last: `Some` for a
    /// probe collecting the unsettled callee returns its evaluation
    /// demands, `None` for one whose demands are scheduled where they are
    /// made.
    probes: Vec<Option<Vec<FlowReturnKey>>>,
}

/// What [`ProjectSemanticDispatch::intercept_unscheduled_flow_demand`]
/// did with one inline flow demand.
pub(super) enum UnscheduledDemand {
    /// The demand was already settled, or no schedule is open: it runs.
    Runs,
    /// A probe recorded it and refused it, typed and partial.
    Refused,
    /// It was scheduled where it was made; its answer is reusable when the
    /// schedule reached one.
    Scheduled,
}

/// Keeps a frame's schedule session open for the frame's whole evaluation;
/// the outermost frame's release clears the session.
pub(super) struct FlowScheduleScope<'g> {
    txn: &'g RefCell<CheckerDispatchTransaction>,
}

impl Drop for FlowScheduleScope<'_> {
    fn drop(&mut self) {
        // An unwinding evaluation may still hold the transaction; its
        // session dies with the transaction, so nothing is lost by skipping.
        if let Ok(mut txn) = self.txn.try_borrow_mut() {
            let session = &mut txn.flow.schedule;
            session.open = session.open.saturating_sub(1);
            if session.open == 0 {
                session.settled.clear();
                session.demands.clear();
                session.probes.clear();
            }
        }
    }
}

/// Where one discovered callee return stands when the schedule meets it.
enum CalleeStanding {
    /// Discovered by this schedule and not yet in a closed component, at
    /// this discovery index: a static cycle edge.
    OnStack(usize),
    /// In flight on the transaction, or an open member of a pending
    /// component: an evaluation reaching it joins that component.
    Open,
    /// Answered: a warm candidate, or a reusable completed member.
    Answered,
    /// Settled by an earlier schedule of this session and left to its own
    /// demand, which evaluates it one level deeper.
    Left,
    /// None of the above: its evaluation would run where it is demanded.
    Unsettled,
}

/// The low-link of a closure that reaches an open component: below every
/// discovery index, which start at one.
const OPEN_COMPONENT: usize = 0;

/// One callee return being walked on the explicit stack.
struct ScheduledCallee {
    key: FlowReturnKey,
    callees: Vec<FlowReturnKey>,
    next: usize,
    /// Discovery index.
    index: usize,
    /// The lowest discovery index the closure reaches (Tarjan's low-link),
    /// or [`OPEN_COMPONENT`].
    low: usize,
    /// Whether evaluating this entry at its demand would nest a further
    /// evaluation — some callee was neither answered nor open.
    nests: bool,
    /// How many of `callees` discovery predicted; the rest a probe of
    /// this entry recorded.
    predicted: usize,
    /// Whether this entry is evaluated here even with nothing left beneath
    /// it: a demand scheduled where it was made, or one a probe recorded,
    /// whose own demands only its evaluation can show.
    forced: bool,
}

/// The explicit stacks of one schedule run: the depth-first walk, and
/// Tarjan's component stack of discovered entries not yet in a closed
/// component.
#[derive(Default)]
struct ScheduleRun {
    walking: Vec<ScheduledCallee>,
    component: Vec<FlowReturnKey>,
    discovered: FxHashMap<FlowReturnKey, usize>,
    next_index: usize,
}

impl<'a> ProjectSemanticDispatch<'a> {
    /// Evaluate the callee returns `frame`'s body will demand, bottom-up,
    /// before the body runs — see the module documentation. `index`,
    /// `entry` and `lowered` are the frame's own served position and
    /// lowered slice, exactly as its evaluation reads them. The returned
    /// scope keeps the schedule session open while the frame evaluates.
    pub(super) fn schedule_flow_return_callees(
        &self,
        frame: &FlowReturnKey,
        index: &FunctionProgramIndex,
        entry: &FunctionProgramEntry,
        lowered: &FlowSliceIR,
    ) -> FlowScheduleScope<'_> {
        self.dispatch_txn.borrow_mut().flow.schedule.open += 1;
        let scope = FlowScheduleScope {
            txn: &self.dispatch_txn,
        };
        // Every frame evaluates beneath a fact tracer: an obligation frame
        // is pushed only inside a flow, relation or call-resolution
        // evaluation, the root of each is a cold build through the shared
        // dispatch choke point, and that build installs the tracer around
        // everything it computes.
        verter_debug_assert!(
            crate::resolver_core::resolver_context::fact_tracer_installed(),
            "a flow frame evaluates beneath its root's fact tracer"
        );
        // A body that makes no call and reads no `typeof` in a type
        // position demands no callee return. A completed
        // member is reusable only when its evaluation's reads were
        // recorded, which takes a live tracer: without one, every scheduled
        // evaluation would be thrown away.
        if demands_callees(entry)
            && flow_return_schedule_enabled()
            && crate::resolver_core::resolver_context::fact_tracer_installed()
        {
            let roots = self.callees_of(frame, index, entry, lowered);
            self.run_flow_return_schedule(roots, false);
        }
        scope
    }

    /// The backstop for native recursion the schedule does not predict:
    /// whether one more inline flow evaluation would nest past the
    /// connected demand's depth bound. Refused, it records the typed depth
    /// refusal on the ledger and on the demanding build, before the native
    /// stack can run out — the evaluation then fails with the budget
    /// failure every tripped demand takes.
    pub(super) fn refuse_nested_flow_evaluation(&self) -> bool {
        let open = {
            let txn = self.dispatch_txn.borrow();
            let reentry = txn.reentry();
            (0..reentry.depth())
                .filter_map(|index| reentry.frame(index))
                .filter(|frame| frame.identity.as_flow_return().is_some())
                .count()
        };
        match self.connected_demand().nesting_trip(open) {
            Some(reasons) => {
                self.fold_local_partial_completeness(reasons);
                true
            }
            None => false,
        }
    }

    /// The demand-time half of discovery, for an inline flow evaluation of
    /// `key` about to open beneath a frame evaluating under an open
    /// schedule. A demand the schedule already settled, answered or holds
    /// runs as it is. An unsettled one is a callee return no discovery
    /// predicted — an instantiation read from an argument of any form, a
    /// call nested where discovery does not look. Beneath a probe it is
    /// recorded and refused, so the probe's entry is evaluated again once
    /// the recorded return is; elsewhere it is scheduled where it is made,
    /// its own unpredicted demands probed in turn, so it is evaluated from
    /// the explicit stack rather than one native level deeper.
    pub(super) fn intercept_unscheduled_flow_demand(
        &self,
        key: &FlowReturnKey,
    ) -> UnscheduledDemand {
        let probing = {
            let txn = self.dispatch_txn.borrow();
            let session = &txn.flow.schedule;
            if session.open == 0 {
                return UnscheduledDemand::Runs;
            }
            matches!(session.probes.last(), Some(Some(_)))
        };
        if !flow_return_schedule_enabled()
            || !crate::resolver_core::resolver_context::fact_tracer_installed()
            || !matches!(
                self.callee_standing(key, &ScheduleRun::default()),
                CalleeStanding::Unsettled
            )
        {
            return UnscheduledDemand::Runs;
        }
        if probing {
            if let Some(Some(recorded)) = self
                .dispatch_txn
                .borrow_mut()
                .flow
                .schedule
                .probes
                .last_mut()
            {
                push_unique(recorded, key.clone());
            }
            // The refusal stays inside the probe's private rails: it marks
            // every build that read it partial, and never trips the
            // connected demand.
            self.fold_local_partial_completeness(
                crate::semantic_query::PartialReasonSet::CONNECTED_QUERY_DEPTH_LIMIT,
            );
            return UnscheduledDemand::Refused;
        }
        self.run_flow_return_schedule(vec![key.clone()], true);
        UnscheduledDemand::Scheduled
    }

    /// Run the schedule over `roots`; `forced` evaluates each root even
    /// with nothing discovered beneath it.
    fn run_flow_return_schedule(&self, roots: Vec<FlowReturnKey>, forced: bool) {
        let mut run = ScheduleRun {
            next_index: OPEN_COMPONENT + 1,
            ..ScheduleRun::default()
        };
        for root in roots {
            if !matches!(self.callee_standing(&root, &run), CalleeStanding::Unsettled) {
                continue;
            }
            self.push_discovered(&mut run, root, forced);
            if !self.drain_flow_return_schedule(&mut run) {
                return;
            }
        }
    }

    /// Discover `key`'s callees and push it on both stacks.
    fn push_discovered(&self, run: &mut ScheduleRun, key: FlowReturnKey, forced: bool) {
        let index = run.next_index;
        run.next_index += 1;
        let callees = self.discover_flow_return_callees(&key);
        run.discovered.insert(key.clone(), index);
        run.component.push(key.clone());
        run.walking.push(ScheduledCallee {
            key,
            predicted: callees.len(),
            callees,
            next: 0,
            index,
            low: index,
            nests: false,
            forced,
        });
    }

    /// Run the explicit stacks to empty. `false` when the connected demand
    /// tripped: the schedule then stops and the body meets the trip.
    fn drain_flow_return_schedule(&self, run: &mut ScheduleRun) -> bool {
        while let Some(top) = run.walking.last_mut() {
            if let Some(callee) = top.callees.get(top.next).cloned() {
                top.next += 1;
                let standing = self.callee_standing(&callee, run);
                let top = run.walking.last_mut().expect("the entry being walked");
                match standing {
                    CalleeStanding::Answered => {}
                    CalleeStanding::Open => top.low = OPEN_COMPONENT,
                    CalleeStanding::Left => top.nests = true,
                    CalleeStanding::OnStack(index) => {
                        top.low = top.low.min(index);
                        top.nests = true;
                    }
                    CalleeStanding::Unsettled => {
                        top.nests = true;
                        // A callee a probe recorded is forced in turn.
                        let forced = top.next > top.predicted;
                        self.push_discovered(run, callee, forced);
                    }
                }
                continue;
            }
            let done = run.walking.pop().expect("the finished entry");
            if done.low != done.index {
                // A member of a component closing lower on the stack, or of
                // an open one: the component's root evaluates it through
                // the ordinary path, where the re-entry intercept holds.
                if let Some(parent) = run.walking.last_mut() {
                    parent.low = parent.low.min(done.low);
                }
                continue;
            }
            let position = run
                .component
                .iter()
                .rposition(|key| key == &done.key)
                .expect("a discovered entry is on the component stack");
            let members = run.component.split_off(position);
            self.settle_all(members.iter());
            // A cycle's root, an entry with something left to evaluate
            // beneath it, and a forced entry are evaluated now; anything
            // else is left to its demand, one level deep, in the body's own
            // order.
            if members.len() == 1 && !done.nests && !done.forced {
                continue;
            }
            if self.connected_demand().work_available().is_err() {
                self.abandon(run);
                return false;
            }
            // A lone entry is probed: a callee return its evaluation
            // demands that no discovery predicted is recorded instead of
            // nesting. A cycle's root runs through the ordinary path.
            let evaluated = self.evaluate_scheduled_callee(&done.key, members.len() == 1);
            if evaluated.reusable {
                continue;
            }
            let recorded: Vec<FlowReturnKey> = evaluated
                .recorded
                .into_iter()
                .filter(|key| !done.callees.contains(key))
                .collect();
            if !recorded.is_empty() {
                // The probe met callee returns beneath it: they are walked,
                // and evaluated, before the entry is evaluated again.
                let mut entry = done;
                entry.callees.extend(recorded);
                run.component.push(entry.key.clone());
                run.walking.push(entry);
                continue;
            }
            // Not reusable: every entry still being walked would
            // re-evaluate it at its own demand, one level deeper each.
            // They are left to the recursive path, as without a schedule.
            self.abandon(run);
        }
        // Whatever reached an open component never closed here: it joins
        // that component at its demand.
        let rest = std::mem::take(&mut run.component);
        self.settle_all(rest.iter());
        true
    }

    fn callee_standing(&self, key: &FlowReturnKey, run: &ScheduleRun) -> CalleeStanding {
        if let Some(&index) = run.discovered.get(key) {
            if run.component.contains(key) {
                return CalleeStanding::OnStack(index);
            }
        }
        {
            let txn = self.dispatch_txn.borrow();
            let identity = ObligationIdentity::FlowReturn(key.clone());
            if txn.reentry().find(&identity).is_some()
                || txn.obligations.pending().contains(&identity)
            {
                return CalleeStanding::Open;
            }
            if txn
                .flow
                .completed_members
                .iter()
                .any(|member| &member.key == key && member.reuse.is_some())
            {
                return CalleeStanding::Answered;
            }
            if txn.flow.schedule.settled.contains(key) {
                return CalleeStanding::Left;
            }
        }
        if self
            .graph()
            .has_serving_flow_return_candidate(self.ctx, key)
        {
            return CalleeStanding::Answered;
        }
        CalleeStanding::Unsettled
    }

    fn settle_all<'k>(&self, keys: impl Iterator<Item = &'k FlowReturnKey>) {
        let mut txn = self.dispatch_txn.borrow_mut();
        for key in keys {
            txn.flow.schedule.settled.insert(key.clone());
        }
    }

    /// Leave every entry of `run` to its own demand.
    fn abandon(&self, run: &mut ScheduleRun) {
        self.settle_all(run.component.iter());
        run.component.clear();
        run.walking.clear();
    }

    /// Evaluate one scheduled callee through the ordinary inline path,
    /// under private rails (see the module documentation); `probe`
    /// records the unsettled callee returns it demands instead of nesting
    /// them.
    fn evaluate_scheduled_callee(&self, key: &FlowReturnKey, probe: bool) -> ScheduledEvaluation {
        let deferred_sticky = crate::request_context::DeferredPartialStickyScope::enter();
        let completeness = crate::request_context::ColdComputeCompletenessScope::enter();
        let frame = BuildLocalTaintGuard::push(&self.build_local_taint);
        self.dispatch_txn
            .borrow_mut()
            .flow
            .schedule
            .probes
            .push(probe.then(Vec::new));
        let step = self.execute_flow_return(key.clone());
        let recorded = self
            .dispatch_txn
            .borrow_mut()
            .flow
            .schedule
            .probes
            .pop()
            .flatten()
            .unwrap_or_default();
        let _ = frame.finish();
        completeness.discard();
        drop(deferred_sticky);
        let reusable = matches!(step, FlowReturnStep::Complete(_))
            && self
                .dispatch_txn
                .borrow()
                .flow
                .completed_members
                .iter()
                .any(|member| &member.key == key && member.reuse.is_some());
        ScheduledEvaluation { reusable, recorded }
    }

    /// The callee returns a discovered callee's body demands. It has not
    /// evaluated yet, so its served position and lowered slice are read
    /// here, through the same demand site its evaluation derives them from.
    fn discover_flow_return_callees(&self, key: &FlowReturnKey) -> Vec<FlowReturnKey> {
        let Ok(site) = self.flow_slice_demand_site(key) else {
            return Vec::new();
        };
        let index = site
            .indexed
            .shallow_state
            .decl_bodies()
            .function_program_index();
        let Some(entry) = frame_entry(&index, key) else {
            return Vec::new();
        };
        if !demands_callees(entry) {
            return Vec::new();
        }
        let flow_slice = self.ctx.project_type_store().flow_slice();
        let Some(crate::cache_runtime::flow_slice_node::FlowSliceHashOutcome::Planned(planned)) =
            crate::cache_runtime::lookup(flow_slice.hash_node(), site.slice_key.clone(), self.ctx)
        else {
            return Vec::new();
        };
        let lowered_key = crate::cache_runtime::flow_slice_node::FlowSliceLoweredKey {
            hash_key: site.slice_key,
            slice_hash: planned.hash(),
        };
        let Some(lowered) =
            crate::cache_runtime::lookup(flow_slice.lowered_node(), lowered_key, self.ctx)
        else {
            return Vec::new();
        };
        self.callees_of(key, &index, entry, &lowered)
    }

    /// The callee returns `frame`'s body demands — through its own calls,
    /// through the calls of the function values it composes, and, for an
    /// instantiated frame, under the instantiation — in source order,
    /// deduplicated.
    fn callees_of(
        &self,
        frame: &FlowReturnKey,
        index: &FunctionProgramIndex,
        entry: &FunctionProgramEntry,
        lowered: &FlowSliceIR,
    ) -> Vec<FlowReturnKey> {
        let mut out: Vec<FlowReturnKey> = Vec::new();
        // A nested position's bare names can bind in the frames around it,
        // which its own index entry does not record as free.
        if entry.lexical_parent.is_some() {
            return out;
        }
        let calls = self.frame_calls(frame, entry, lowered);
        for call in &calls {
            push_unique(&mut out, self.flow_return_key_for(&call.identity));
        }
        self.push_type_query_callees(frame, entry, lowered, &mut out);
        // A function value the slice selects is composed where it sits: its
        // body's return is evaluated inside this frame, calls and all.
        let canonical = frame.function.declaration_slot.defining_canonical.as_ref();
        let anchor = entry.span.start;
        let mut composed: Vec<(&FunctionProgramEntry, Vec<NamedCall>)> = Vec::new();
        for nested in entry.nested_captures.iter() {
            let relative = FrameSpan::rebase(anchor, nested.span);
            let selected = lowered.exprs.iter().any(|expression| {
                expression.role == FlowExprRole::Value && expression.span.contains(relative)
            });
            let Some(value) = index.get(&nested.function).map(|matched| matched.entry()) else {
                continue;
            };
            if selected {
                let calls = self.composed_value_calls(canonical, entry, value);
                for call in &calls {
                    push_unique(&mut out, self.flow_return_key_for(&call.identity));
                }
                composed.push((value, calls));
            }
        }
        if !is_uninstantiated(frame) && !self.push_logged_instantiations(frame, entry, &mut out) {
            self.push_forwarded_instantiations(frame, entry, &calls, &composed, &mut out);
        }
        out
    }

    /// The calls `frame`'s body evaluates for their value whose callee's
    /// body-derived return the evaluator's call rails demand: a free
    /// bare-identifier callee that is the direct-call rail's same-file
    /// target, or that the file's owner scope resolves — through its
    /// imports — to a function whose return is body-derived, exactly as
    /// the rail lowering `typeof callee` resolves it. A call inside
    /// another call's argument counts when the body evaluates it (see
    /// [`Self::call_is_evaluated`]).
    fn frame_calls(
        &self,
        frame: &FlowReturnKey,
        entry: &FunctionProgramEntry,
        lowered: &FlowSliceIR,
    ) -> Vec<NamedCall> {
        let slot = &frame.function.declaration_slot;
        let canonical = slot.defining_canonical.as_ref();
        let anchor = entry.span.start;
        let every_call: Vec<verter_span::Span> = lowered
            .effects
            .iter()
            .filter_map(|effect| match effect {
                FlowEffect::Call { span, .. } => Some(span.to_absolute(anchor)),
                FlowEffect::Write { .. } => None,
            })
            .collect();
        let mut scope_payload = None;
        let mut out = Vec::new();
        // Only a call whose value the slice evaluates: an effect-only
        // expression's calls are never demanded for their return.
        for effect in lowered.effects.iter() {
            let FlowEffect::Call {
                site,
                callee: FlowCallee::Named(name),
                new_construct: false,
                span,
            } = effect
            else {
                continue;
            };
            let span = span.to_absolute(anchor);
            if lowered.expr(*site).role != FlowExprRole::Value
                || !callee_is_free(entry, name, span)
                || !self.call_is_evaluated(
                    canonical,
                    slot.owner,
                    &mut scope_payload,
                    entry,
                    span,
                    &every_call,
                )
            {
                continue;
            }
            let identity = match entry.direct_calls.iter().find(|direct| direct.span == span) {
                // A call to the function itself is its own recursion hold.
                Some(direct) if direct.target == entry.key => continue,
                Some(direct) => self.direct_callee_identity(canonical, &direct.target),
                None => {
                    let payload = self.scope_payload(canonical, slot.owner, &mut scope_payload);
                    self.resolved_callee_identity(canonical, slot.owner, payload, name)
                }
            };
            if let Some(identity) = identity {
                out.push(NamedCall {
                    span,
                    name: Arc::clone(name),
                    identity,
                });
            }
        }
        out
    }

    /// The callee returns `frame`'s body demands through TYPE positions:
    /// each bare `typeof name` its evaluation lowers — in the frame's
    /// parameter list, in the annotation of a binding whose value the
    /// slice selects, or in a type a selected expression carries — whose
    /// name is free in the frame and which the owner scope resolves, as the
    /// type lowering resolves `typeof name`, to a function whose return is
    /// body-derived. `ReturnType<typeof f>` reads `f`'s return exactly as
    /// a call of `f` does.
    fn push_type_query_callees(
        &self,
        frame: &FlowReturnKey,
        entry: &FunctionProgramEntry,
        lowered: &FlowSliceIR,
        out: &mut Vec<FlowReturnKey>,
    ) {
        let slot = &frame.function.declaration_slot;
        let canonical = slot.defining_canonical.as_ref();
        let anchor = entry.span.start;
        let mut scope_payload = None;
        for query in entry.type_queries.iter() {
            if !matches!(query.binding, FunctionReferenceBinding::Free) {
                continue;
            }
            let read = match query.position {
                FunctionTypeQueryPosition::Parameter => true,
                FunctionTypeQueryPosition::Declarator(binding) => {
                    let binding = FrameSpan::rebase(anchor, binding);
                    lowered
                        .slots
                        .iter()
                        .any(|slot| slot.value_selected && slot.span == binding)
                }
                FunctionTypeQueryPosition::Expression => {
                    let position = FrameSpan::rebase(anchor, query.span);
                    lowered.exprs.iter().any(|expression| {
                        expression.role == FlowExprRole::Value && expression.span.contains(position)
                    })
                }
            };
            if !read {
                continue;
            }
            let payload = self.scope_payload(canonical, slot.owner, &mut scope_payload);
            if let Some(identity) =
                self.resolved_callee_identity(canonical, slot.owner, payload, &query.name)
            {
                push_unique(out, self.flow_return_key_for(&identity));
            }
        }
    }

    /// The owner scope's declaration payload, built on first use.
    fn scope_payload<'p>(
        &self,
        canonical: &str,
        owner: verter_type_expr::TopLevelOwnerId,
        payload: &'p mut Option<Option<DeclarationScopePayload>>,
    ) -> Option<&'p DeclarationScopePayload> {
        payload
            .get_or_insert_with(|| {
                self.ctx
                    .prepared_decl_bundle(canonical)
                    .map(|bundle| DeclarationScopePayload::from_bundle(&bundle, owner))
            })
            .as_ref()
    }

    /// Whether the body evaluates the call at `span` for its value, given
    /// that its own position is a value position: a call no other call
    /// encloses, or a direct argument of an enclosing call that is itself
    /// evaluated and whose call sink evaluates its arguments — a free,
    /// bare-identifier callee whose unannotated declaration is generic or
    /// overloaded, which the call rails resolve through the call executor.
    /// A call in a callee expression, and an argument of any other call,
    /// is left to the body.
    fn call_is_evaluated(
        &self,
        canonical: &str,
        owner: verter_type_expr::TopLevelOwnerId,
        scope_payload: &mut Option<Option<DeclarationScopePayload>>,
        entry: &FunctionProgramEntry,
        span: verter_span::Span,
        every_call: &[verter_span::Span],
    ) -> bool {
        let mut span = span;
        loop {
            let Some(enclosing) = every_call
                .iter()
                .filter(|outer| **outer != span && contains(**outer, span))
                .min_by_key(|outer| outer.end - outer.start)
                .copied()
            else {
                return true;
            };
            let Some(site) = entry.call_sites.iter().find(|site| site.span == enclosing) else {
                return false;
            };
            let direct_argument = site
                .args
                .iter()
                .any(|argument| !argument.spread && argument.point == span.start);
            let FunctionEffectCallee::Identifier(name) = &site.callee else {
                return false;
            };
            if !direct_argument || !callee_is_free(entry, name, enclosing) {
                return false;
            }
            let prepared = match &site.target {
                Some(target) => self.ctx.prepared_value_decl_return_only(
                    canonical,
                    target.declaration.owner,
                    target.declaration.name.as_ref(),
                ),
                None => {
                    let payload = self.scope_payload(canonical, owner, scope_payload);
                    crate::resolver_core::bare_name_resolve::resolve_bare_name_in_scope(
                        self.ctx, canonical, owner, payload, name,
                    )
                    .and_then(|root| {
                        self.ctx.prepared_value_decl_return_only(
                            root.canonical_id.as_ref(),
                            root.owner,
                            root.symbol_name.as_ref(),
                        )
                    })
                }
            };
            let evaluates_arguments = prepared.is_some_and(|prepared| {
                !annotated(&prepared)
                    && (prepared.signatures.len() > 1
                        || prepared
                            .signatures
                            .iter()
                            .any(|signature| !signature.type_parameters.is_empty()))
            });
            if !evaluates_arguments {
                return false;
            }
            span = enclosing;
        }
    }

    /// Record that the innermost evaluating flow frame demanded the
    /// instantiated callee return `key` — the call resolution's own
    /// answer, which an instantiation of that frame demands again under
    /// its own arguments (see [`Self::push_logged_instantiations`]).
    pub(super) fn note_flow_return_demand(&self, key: &FlowReturnKey) {
        if key.context.type_substitution.bindings().is_empty() {
            return;
        }
        let mut txn = self.dispatch_txn.borrow_mut();
        if txn.flow.schedule.open == 0 {
            return;
        }
        let reentry = txn.reentry();
        let frame = (0..reentry.depth())
            .rev()
            .filter_map(|index| reentry.frame(index))
            .find_map(|frame| frame.identity.as_flow_return())
            .filter(|frame| is_uninstantiated(frame))
            .cloned();
        let Some(frame) = frame else {
            return;
        };
        let demands = txn.flow.schedule.demands.entry(frame).or_default();
        if !demands.contains(key) {
            demands.push(key.clone());
        }
    }

    /// The instantiated callee returns an instantiation of `frame`'s
    /// function demands, when its uninstantiated frame evaluated in this
    /// session: the ones that frame's call resolution demanded, with the
    /// frame's own binders replaced by the instantiation's arguments — by
    /// binder NAME, the mapping the instantiated frame's binder environment
    /// applies. A demand whose arguments are anything but the frame's own
    /// binders or closed primitives and literals is not transported: its
    /// resolution under the arguments is the body's to make. `false` when
    /// the uninstantiated frame left no record.
    fn push_logged_instantiations(
        &self,
        frame: &FlowReturnKey,
        entry: &FunctionProgramEntry,
        out: &mut Vec<FlowReturnKey>,
    ) -> bool {
        let Some(demanded) = self
            .dispatch_txn
            .borrow()
            .flow
            .schedule
            .demands
            .get(&uninstantiated(frame))
            .cloned()
        else {
            return false;
        };
        if entry.type_parameters.len() != frame.normalized_type_args.len() {
            return true;
        }
        let slot = &frame.function.declaration_slot;
        let graph = self.graph();
        let transport = |node: SemanticNodeId| -> Option<SemanticNodeId> {
            match graph.node_data(node).as_deref()? {
                SemanticNodeData::TypeParam { decl, .. }
                    if decl.canonical_id == slot.defining_canonical && decl.owner == slot.owner =>
                {
                    let ordinal = entry
                        .type_parameters
                        .iter()
                        .position(|param| param.name == decl.decl_name)?;
                    frame.normalized_type_args.get(ordinal).copied()
                }
                SemanticNodeData::Primitive(_) | SemanticNodeData::Literal(_) => Some(node),
                _ => None,
            }
        };
        for demand in demanded {
            let Some(arguments) = demand
                .normalized_type_args
                .iter()
                .map(|argument| transport(*argument))
                .collect::<Option<Vec<_>>>()
            else {
                continue;
            };
            let Some(bindings) = demand
                .context
                .type_substitution
                .bindings()
                .iter()
                .map(|(param, image)| Some((*param, transport(*image)?)))
                .collect::<Option<Vec<_>>>()
            else {
                continue;
            };
            let mut instantiated = demand;
            instantiated.normalized_type_args = Arc::from(arguments.into_boxed_slice());
            instantiated.context.type_substitution =
                crate::semantic_query::CanonicalTypeSubstitution::new(bindings);
            push_unique(out, instantiated);
        }
        true
    }

    /// The instantiated callee returns an instantiation of `frame`'s
    /// function demands, when its uninstantiated frame did NOT evaluate in
    /// this session (its answer was already warm): read from the
    /// signatures both functions already have — the same ones the call
    /// executor reads — for a call that FORWARDS the frame's binders.
    ///
    /// The call is made by the frame's body or by a function value it
    /// composes (`const f = (y: T) => g(y)`), and its every argument is a
    /// bare read of a parameter declared as one of the frame's own type
    /// parameters, which the instantiated frame binds to that parameter's
    /// argument: a parameter of the frame, as its signature declares it,
    /// or a parameter of the composed value annotated with the binder's
    /// name where no clause of the value and no type declared in the frame
    /// around it takes that name. The callee's every parameter is declared
    /// as one of its own unconstrained type parameters, and every type
    /// parameter is declared by a parameter whose arguments all agree.
    /// Inference then fixes each callee binder to exactly the forwarded
    /// argument — the instantiation the executor demands. Every other
    /// shape, a parameter written anywhere, and a callee or frame whose
    /// uninstantiated answer is not already in hand (reading its signature
    /// would evaluate it) are left to the body.
    fn push_forwarded_instantiations(
        &self,
        frame: &FlowReturnKey,
        entry: &FunctionProgramEntry,
        calls: &[NamedCall],
        composed: &[(&FunctionProgramEntry, Vec<NamedCall>)],
        out: &mut Vec<FlowReturnKey>,
    ) {
        let slot = &frame.function.declaration_slot;
        let canonical = slot.defining_canonical.as_ref();
        let own = uninstantiated(frame);
        let readers = calls
            .iter()
            .map(|call| (entry, call))
            .chain(
                composed
                    .iter()
                    .flat_map(|(value, calls)| calls.iter().map(move |call| (*value, call))),
            )
            .collect::<Vec<_>>();
        if readers.is_empty() || !self.is_answered(&own) {
            return;
        }
        let Some(own_signature) =
            self.callable_signature(canonical, slot.owner, &slot.merged_symbol_name, &own)
        else {
            return;
        };
        if own_signature.type_parameters.len() != frame.normalized_type_args.len() {
            return;
        }
        let Some(serve) = self.ctx.ensure_indexed_ready_serve(canonical) else {
            return;
        };
        let decl_bodies = serve.indexed.shallow_state.decl_bodies();
        // The frame's own lexical structure, read only when a composed
        // value's annotation names a binder: a type the frame declares
        // around the value takes the name before the frame's clause does.
        let mut skeleton = None;
        let mut frame_declares_type = |name: &str, value: &FunctionProgramEntry| -> bool {
            let skeleton = skeleton.get_or_insert_with(|| {
                self.flow_slice_demand_site(frame).ok().and_then(|site| {
                    self.ctx
                        .project_type_store()
                        .flow_slice()
                        .skeleton_for(&site.slice_key_function, self.ctx)
                })
            });
            let Some(skeleton) = skeleton.as_ref() else {
                return true;
            };
            skeleton.name_id(name).is_some_and(|name| {
                let region = skeleton
                    .innermost_region_containing(FrameSpan::rebase(entry.span.start, value.span));
                skeleton.declares_meaning_in_scope(name, region, NameMeaning::Type)
            })
        };
        // The image of the parameter a bare argument read in `reader`
        // names: its declared binder's argument in this instantiation.
        let mut forwarded =
            |reader: &FunctionProgramEntry,
             argument: &verter_type_expr::IndexedValueCallArg,
             root: &verter_semantic::analysis::type_eval_build::IndexedValueReadRoot|
             -> Option<SemanticNodeId> {
                let verter_type_expr::IndexedValueExpression::Value(
                    verter_type_expr::TypeExpr::TypeOf(value),
                ) = &argument.expression
                else {
                    return None;
                };
                let ([name], true) = (value.path.as_slice(), value.type_args.is_empty()) else {
                    return None;
                };
                let verter_semantic::analysis::type_eval_build::IndexedValueReadRoot::Identifier(
                    root,
                ) = root
                else {
                    return None;
                };
                let binding =
                    reader
                        .references
                        .iter()
                        .find_map(|reference| match &reference.binding {
                            FunctionReferenceBinding::Resolved(binding)
                                if reference.span == *root
                                    && reference.name.as_ref() == name
                                    && reference.path.is_empty() =>
                            {
                                Some(binding)
                            }
                            _ => None,
                        })?;
                if binding.kind != FunctionBindingKind::Param
                    || entry.descendant_writes.contains(binding)
                {
                    return None;
                }
                let binder = if binding.defining_function == entry.key {
                    if parameter_written(entry, binding) {
                        return None;
                    }
                    let ordinal = entry.params.iter().position(|param| {
                        param.name.as_deref() == Some(name.as_str()) && !param.rest
                    })?;
                    let declared = own_signature.params.get(ordinal)?;
                    own_signature
                        .type_parameters
                        .iter()
                        .position(|param| param.param == declared.ty)?
                } else if binding.defining_function == reader.key {
                    if parameter_written(reader, binding)
                        || reader.descendant_writes.contains(binding)
                    {
                        return None;
                    }
                    let annotated = reader
                        .params
                        .iter()
                        .find(|param| param.name.as_deref() == Some(name.as_str()) && !param.rest)?
                        .annotation_reference
                        .as_deref()?;
                    if reader
                        .type_parameters
                        .iter()
                        .any(|param| param.name.as_ref() == annotated)
                        || frame_declares_type(annotated, reader)
                    {
                        return None;
                    }
                    entry
                        .type_parameters
                        .iter()
                        .position(|param| param.name.as_ref() == annotated)?
                } else {
                    return None;
                };
                frame.normalized_type_args.get(binder).copied()
            };
        for (reader, call) in readers {
            let generic = self.flow_return_key_for(&call.identity);
            if !self.is_answered(&generic) {
                continue;
            }
            let Some(callee) = self.callable_signature(canonical, slot.owner, &call.name, &generic)
            else {
                continue;
            };
            if callee.type_parameters.is_empty() {
                continue;
            }
            let Some(indexed) = decl_bodies.indexed_call_expression_at(call.span) else {
                continue;
            };
            let site = &indexed.call;
            if site.kind != verter_type_expr::IndexedValueCallKind::Call
                || site.receiver.is_some()
                || !site.explicit_type_args.is_empty()
                || site.args.len() != callee.params.len()
                || indexed.argument_roots.len() != site.args.len()
            {
                continue;
            }
            let mut images: Vec<Option<SemanticNodeId>> = vec![None; callee.type_parameters.len()];
            let mut forwards = true;
            for ((param, argument), root) in callee
                .params
                .iter()
                .zip(site.args.iter())
                .zip(indexed.argument_roots.iter())
            {
                let binder = callee
                    .type_parameters
                    .iter()
                    .position(|binder| binder.param == param.ty && binder.constraint.is_none());
                let image = if param.rest || argument.spread {
                    None
                } else {
                    forwarded(reader, argument, root)
                };
                match (binder, image) {
                    (Some(binder), Some(image))
                        if images[binder].is_none_or(|seen| seen == image) =>
                    {
                        images[binder] = Some(image);
                    }
                    _ => {
                        forwards = false;
                        break;
                    }
                }
            }
            let Some(arguments) = forwards
                .then(|| images.into_iter().collect::<Option<Vec<_>>>())
                .flatten()
            else {
                continue;
            };
            let substitution = crate::semantic_query::CanonicalTypeSubstitution::new(
                callee
                    .type_parameters
                    .iter()
                    .map(|binder| binder.param)
                    .zip(arguments.iter().copied())
                    .collect(),
            );
            push_unique(
                out,
                self.flow_return_key_for_instantiation(
                    &callee.identity,
                    Arc::from(arguments.into_boxed_slice()),
                    substitution,
                ),
            );
        }
    }

    /// The lone call signature `typeof name` has in `canonical`'s owner
    /// scope — the lowering the call rails perform — when its return is the
    /// body-derived return `expected` addresses.
    fn callable_signature(
        &self,
        canonical: &str,
        owner: verter_type_expr::TopLevelOwnerId,
        name: &str,
        expected: &FlowReturnKey,
    ) -> Option<CallableSignature> {
        let callee = self.lower_type_expr_in_owner_scope_with_context(
            canonical,
            owner,
            &verter_type_expr::TypeExpr::TypeOf(verter_type_expr::ValueRef {
                path: name.split('.').map(str::to_string).collect(),
                type_args: Vec::new(),
            }),
            crate::semantic_query::ProjectionReductionContext::structural_transit(),
        )?;
        let (call, construct) = self.shared_signature_buckets(callee).ok()?;
        let ([signature], []) = (call.as_slice(), construct.as_slice()) else {
            return None;
        };
        let data = self.graph().node_data(*signature)?;
        let SemanticNodeData::Signature {
            params,
            type_parameters,
            return_carrier:
                crate::semantic_query::SignatureReturnCarrier::Function(
                    verter_type_expr::facts::FunctionReturnSource::Flow(identity),
                ),
            ..
        } = data.as_ref()
        else {
            return None;
        };
        (self.flow_return_key_for(identity) == *expected).then(|| CallableSignature {
            params: Arc::clone(params),
            type_parameters: Arc::clone(type_parameters),
            identity: identity.clone(),
        })
    }

    /// The names of the type parameters `key`'s function declares itself,
    /// in declaration order — the clause an instantiated frame binds by
    /// ordinal. `None` when its served position is not read.
    pub(super) fn own_type_parameter_names(&self, key: &FlowReturnKey) -> Option<Vec<Arc<str>>> {
        let site = self.flow_slice_demand_site(key).ok()?;
        let index = site
            .indexed
            .shallow_state
            .decl_bodies()
            .function_program_index();
        let entry = frame_entry(&index, key)?;
        Some(
            entry
                .type_parameters
                .iter()
                .map(|param| Arc::clone(&param.name))
                .collect(),
        )
    }

    /// Whether `key` is answered without evaluating it here: a warm
    /// candidate, or a reusable completed member.
    fn is_answered(&self, key: &FlowReturnKey) -> bool {
        self.dispatch_txn
            .borrow()
            .flow
            .completed_members
            .iter()
            .any(|member| &member.key == key && member.reuse.is_some())
            || self
                .graph()
                .has_serving_flow_return_candidate(self.ctx, key)
    }

    /// The direct calls of one composed function value's return sites
    /// whose callee's return is body-derived.
    fn composed_value_calls(
        &self,
        canonical: &str,
        frame: &FunctionProgramEntry,
        value: &FunctionProgramEntry,
    ) -> Vec<NamedCall> {
        let mut out = Vec::new();
        let every_call: Vec<verter_span::Span> =
            value.effects.iter().map(|effect| effect.span).collect();
        for effect in value.effects.iter() {
            let FunctionEffectCallee::Identifier(name) = &effect.callee else {
                continue;
            };
            // An expression-bodied arrow records no return site: its body IS
            // the returned expression.
            let returned = value.body_span == effect.span
                || value
                    .return_sites
                    .iter()
                    .any(|site| contains(site.span, effect.span));
            if !returned || nested_in_another_call(effect.span, &every_call) {
                continue;
            }
            // A name the frame around the value binds is a captured binding
            // there, never the file's function.
            if frame.bindings.iter().any(|binding| &binding.name == name) {
                continue;
            }
            let Some(direct) = value
                .direct_calls
                .iter()
                .find(|direct| direct.span == effect.span)
            else {
                continue;
            };
            if direct.target == value.key || !callee_is_free(value, name, effect.span) {
                continue;
            }
            if let Some(identity) = self.direct_callee_identity(canonical, &direct.target) {
                out.push(NamedCall {
                    span: effect.span,
                    name: Arc::clone(name),
                    identity,
                });
            }
        }
        out
    }

    /// The return position the direct-call rail demands for `target` when
    /// it takes the callee's return from its body — mirroring that rail's
    /// own choice: an authored annotation types the callee instead, an
    /// overload group is resolved by the call executor over its declared
    /// signatures, and a declared return carries no body demand.
    fn direct_callee_identity(
        &self,
        canonical: &str,
        target: &FunctionProgramKey,
    ) -> Option<FlowFunctionReturnIdentity> {
        let prepared = self.ctx.prepared_value_decl_return_only(
            canonical,
            target.declaration.owner,
            target.declaration.name.as_ref(),
        );
        if prepared
            .as_ref()
            .is_some_and(|prepared| annotated(prepared) || prepared.signatures.len() > 1)
        {
            return None;
        }
        let ordinal = match &target.part {
            FunctionPartIdentity::DeclarationBody => target.overload_ordinal as usize,
            _ => 0,
        };
        let source = prepared
            .as_ref()
            .and_then(|prepared| {
                prepared
                    .signatures
                    .get(ordinal)
                    .map(|signature| signature.return_source.clone())
            })
            .unwrap_or_else(|| {
                verter_type_expr::facts::FunctionReturnSource::Flow(FlowFunctionReturnIdentity {
                    anchor: verter_type_expr::locators::AuthoredAnchor {
                        canonical_id: Arc::from(canonical),
                        owner: target.declaration.owner,
                        symbol: Arc::clone(&target.declaration.name),
                        space: verter_type_expr::locators::LocatorSymbolSpace::Value,
                    },
                    function_part: target.part.clone(),
                    overload_ordinal: target.overload_ordinal,
                })
            });
        match source {
            verter_type_expr::facts::FunctionReturnSource::Flow(identity) => Some(identity),
            verter_type_expr::facts::FunctionReturnSource::Declared(_)
            | verter_type_expr::facts::FunctionReturnSource::Absent => None,
        }
    }

    /// The return position a free callee `name` of `canonical`'s owner
    /// scope takes from its body, resolved as the call rail's `typeof name`
    /// lowering resolves it — through the scope's imports to the declaring
    /// file, whose lone, unannotated function signature carries the
    /// body-derived return.
    fn resolved_callee_identity(
        &self,
        canonical: &str,
        owner: verter_type_expr::TopLevelOwnerId,
        scope_payload: Option<&DeclarationScopePayload>,
        name: &str,
    ) -> Option<FlowFunctionReturnIdentity> {
        let root = crate::resolver_core::bare_name_resolve::resolve_bare_name_in_scope(
            self.ctx,
            canonical,
            owner,
            scope_payload,
            name,
        )?;
        let prepared = self.ctx.prepared_value_decl_return_only(
            root.canonical_id.as_ref(),
            root.owner,
            root.symbol_name.as_ref(),
        )?;
        if annotated(&prepared) {
            return None;
        }
        let [signature] = prepared.signatures.as_slice() else {
            return None;
        };
        let verter_type_expr::facts::FunctionReturnSource::Flow(identity) =
            &signature.return_source
        else {
            return None;
        };
        // The extractor stamps the declaration name; canonical and owner
        // come from the serving scope, as every signature composition fills
        // them.
        let mut identity = identity.clone();
        identity.anchor.canonical_id = root.canonical_id;
        identity.anchor.owner = root.owner;
        Some(identity)
    }
}

/// The outcome of one scheduled evaluation.
struct ScheduledEvaluation {
    /// It closed as a reusable completed member.
    reusable: bool,
    /// The unsettled callee returns its probe recorded, in demand order.
    recorded: Vec<FlowReturnKey>,
}

/// One call a frame's body evaluates for its value, whose callee's
/// body-derived return the schedule can name.
struct NamedCall {
    /// The call expression's absolute span.
    span: verter_span::Span,
    /// The name the body calls the callee by, in the frame's owner scope.
    name: Arc<str>,
    /// The callee's body-derived return position.
    identity: FlowFunctionReturnIdentity,
}

/// The parts of a lone call signature the forwarded-binder reading needs.
struct CallableSignature {
    params: Arc<[crate::semantic_query::FunctionParam]>,
    type_parameters: Arc<[crate::semantic_query::TypeParamDecl]>,
    identity: FlowFunctionReturnIdentity,
}

/// Whether a value declaration carries an AUTHORED annotation, which types
/// it instead of its initializer's body.
fn annotated(
    prepared: &verter_semantic::analysis::type_solver::prepared::PreparedValueDecl,
) -> bool {
    matches!(
        prepared.type_annotation.classification,
        verter_type_expr::facts::ValueAnnotationClass::Direct
            | verter_type_expr::facts::ValueAnnotationClass::TypeOfAlias
    ) && matches!(
        prepared.type_annotation.annotation,
        Some(verter_type_expr::facts::SemanticTypeSource::Authored(_))
    )
}

/// Whether the body can demand a callee return: it makes a call, in its
/// own frame or a composed one, or reads a `typeof` in a type position.
fn demands_callees(entry: &FunctionProgramEntry) -> bool {
    !(entry.effects.is_empty() && entry.nested_captures.is_empty() && entry.type_queries.is_empty())
}

/// Whether the callee identifier of the call at `span` is FREE in `entry`:
/// a parameter, local or nested declaration of that name shadows the
/// file's binding, and the content lowering calls the binding instead.
fn callee_is_free(entry: &FunctionProgramEntry, name: &str, span: verter_span::Span) -> bool {
    entry.references.iter().any(|reference| {
        reference.name.as_ref() == name
            && reference.span.start == span.start
            && matches!(reference.binding, FunctionReferenceBinding::Free)
    })
}

/// Whether the body writes `binding`, so a read of it need not see its
/// declared type.
fn parameter_written(
    entry: &FunctionProgramEntry,
    binding: &verter_semantic::analysis::function_program::FlowBindingIdentity,
) -> bool {
    entry.writes.iter().any(|write| {
        write.targets.iter().any(|target| match target {
            verter_semantic::analysis::function_program::FunctionWriteTarget::Binding {
                reference,
                ..
            } => match &reference.binding {
                FunctionReferenceBinding::Resolved(written) => written == binding,
                _ => false,
            },
            verter_semantic::analysis::function_program::FunctionWriteTarget::Unsupported {
                ..
            } => true,
        })
    })
}

fn push_unique(out: &mut Vec<FlowReturnKey>, key: FlowReturnKey) {
    if !out.contains(&key) {
        out.push(key);
    }
}

/// Whether `key` addresses its function under no instantiation.
pub(super) fn is_uninstantiated(key: &FlowReturnKey) -> bool {
    key.normalized_type_args.is_empty() && key.context.type_substitution.bindings().is_empty()
}

/// `key` without its instantiation: the same function under the same
/// demand, its own binders unbound.
pub(super) fn uninstantiated(key: &FlowReturnKey) -> FlowReturnKey {
    let mut uninstantiated = key.clone();
    uninstantiated.normalized_type_args = Arc::from(Vec::new().into_boxed_slice());
    uninstantiated.context.type_substitution =
        crate::semantic_query::CanonicalTypeSubstitution::empty();
    uninstantiated
}
/// The served function position `frame` evaluates.
fn frame_entry<'i>(
    index: &'i FunctionProgramIndex,
    frame: &FlowReturnKey,
) -> Option<&'i FunctionProgramEntry> {
    index
        .value_function(
            frame.function.declaration_slot.owner,
            frame.function.declaration_slot.merged_symbol_name.as_ref(),
            &frame.function.function_part,
            frame.function.overload_ordinal,
        )
        .map(|matched| matched.entry())
}

/// Whether `outer` contains `inner` (inclusive at both edges).
fn contains(outer: verter_span::Span, inner: verter_span::Span) -> bool {
    outer.start <= inner.start && inner.end <= outer.end
}

/// Whether the call at `span` sits inside another call — an argument, or a
/// callee expression — whose own evaluation decides whether it runs.
fn nested_in_another_call(span: verter_span::Span, calls: &[verter_span::Span]) -> bool {
    calls
        .iter()
        .any(|outer| *outer != span && contains(*outer, span))
}

#[cfg(test)]
std::thread_local! {
    static SCHEDULE_DISABLED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

#[cfg(test)]
fn flow_return_schedule_enabled() -> bool {
    !SCHEDULE_DISABLED.get()
}

#[cfg(not(test))]
fn flow_return_schedule_enabled() -> bool {
    true
}

/// Test-only: evaluate every callee where it is demanded, as the body does
/// without a schedule, while the returned guard lives on this thread.
#[cfg(test)]
pub(crate) fn disable_flow_return_schedule_for_tests() -> impl Drop {
    struct Restore(bool);
    impl Drop for Restore {
        fn drop(&mut self) {
            SCHEDULE_DISABLED.set(self.0);
        }
    }
    Restore(SCHEDULE_DISABLED.replace(true))
}
