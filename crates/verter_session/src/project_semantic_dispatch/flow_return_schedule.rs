//! The flow-return callee schedule: nested callee evaluation on an
//! explicit work stack instead of the native one.
//!
//! A frame's body demands a callee's return where the call sits, and the
//! callee's body is evaluated there, inside the caller's evaluation — for
//! a generic call, beneath the `TypeOf` and `LowerLocator` queries that
//! lower `typeof callee`, and again under the call's instantiation. Left to
//! that alone, a linear call chain nests one native evaluation (and, for a
//! generic call, two connected queries) per level, so its length is bounded
//! by the native stack and the connected-query depth guard rather than by
//! the work it does.
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
//!   the frame's lowered slice selects, which the per-file function index
//!   resolves to a same-file function exactly as the content lowering does
//!   (a free, unshadowed, bare-identifier callee), and whose return the
//!   direct-call rail takes from the body: the key is minted by the one key
//!   construction that rail and the signature lowering both use. An
//!   instantiated frame demands what its uninstantiated frame's call
//!   resolution demanded, under the instantiation's arguments: the
//!   resolution's own answers are recorded as it makes them, and carried
//!   over by binder name — the mapping the instantiated frame's binder
//!   environment applies. A call nested inside another call's arguments, a
//!   call the index cannot resolve, an instantiation whose arguments are
//!   anything but forwarded binders, and every demand made through a type
//!   position are left to the body, which evaluates them recursively
//!   exactly as before.
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
//! The schedule keeps one transaction-local session while any frame is
//! evaluating: the callee returns it has already settled — evaluated, left
//! to the body, or abandoned — so a nested frame's schedule never
//! re-evaluates them, and the instantiated demands each uninstantiated
//! frame made. The session is cleared when the outermost
//! frame finishes.

use std::cell::RefCell;
use std::sync::Arc;

use rustc_hash::{FxHashMap, FxHashSet};
use verter_semantic::analysis::flow::flow_ir::{FlowCallee, FlowEffect, FlowExprRole, FlowSliceIR};
use verter_semantic::analysis::flow::FrameSpan;
use verter_semantic::analysis::function_program::{
    FunctionEffectCallee, FunctionProgramEntry, FunctionProgramIndex, FunctionProgramKey,
    FunctionReferenceBinding,
};
use verter_type_expr::facts::FunctionPartIdentity;

use super::super::dispatch_txn::{CheckerDispatchTransaction, ObligationIdentity};
use super::super::{BuildLocalTaintGuard, ProjectSemanticDispatch};
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
        // A body with no direct call and no function value demands nothing
        // the schedule can discover, unless an instantiation carries its
        // uninstantiated frame's demands. A completed member is reusable
        // only when its evaluation's reads were recorded, which takes a
        // live tracer: without one, every scheduled evaluation would be
        // thrown away.
        let discoverable = !(entry.direct_calls.is_empty() && entry.nested_captures.is_empty())
            || !is_uninstantiated(frame);
        if discoverable
            && flow_return_schedule_enabled()
            && crate::resolver_core::resolver_context::fact_tracer_installed()
        {
            let roots = self.callees_of(frame, index, entry, lowered);
            self.run_flow_return_schedule(roots);
        }
        scope
    }

    fn run_flow_return_schedule(&self, roots: Vec<FlowReturnKey>) {
        let mut run = ScheduleRun {
            next_index: OPEN_COMPONENT + 1,
            ..ScheduleRun::default()
        };
        for root in roots {
            if !matches!(self.callee_standing(&root, &run), CalleeStanding::Unsettled) {
                continue;
            }
            self.push_discovered(&mut run, root);
            if !self.drain_flow_return_schedule(&mut run) {
                return;
            }
        }
    }

    /// Discover `key`'s callees and push it on both stacks.
    fn push_discovered(&self, run: &mut ScheduleRun, key: FlowReturnKey) {
        let index = run.next_index;
        run.next_index += 1;
        let callees = self.discover_flow_return_callees(&key);
        run.discovered.insert(key.clone(), index);
        run.component.push(key.clone());
        run.walking.push(ScheduledCallee {
            key,
            callees,
            next: 0,
            index,
            low: index,
            nests: false,
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
                        self.push_discovered(run, callee);
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
            // A cycle's root, or an entry with something left to evaluate
            // beneath it, is evaluated now; anything else is left to its
            // demand, one level deep, in the body's own order.
            if members.len() == 1 && !done.nests {
                continue;
            }
            if self.connected_demand().work_available().is_err() {
                self.abandon(run);
                return false;
            }
            if !self.evaluate_scheduled_callee(&done.key) {
                // Not reusable: every entry still being walked would
                // re-evaluate it at its own demand, one level deeper each.
                // They are left to the recursive path, as without a
                // schedule.
                self.abandon(run);
            }
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
        if self.graph().has_flow_return_candidate(key) {
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
    /// under private rails (see the module documentation). `true` when it
    /// closed as a reusable completed member.
    fn evaluate_scheduled_callee(&self, key: &FlowReturnKey) -> bool {
        let deferred_sticky = crate::request_context::DeferredPartialStickyScope::enter();
        let completeness = crate::request_context::ColdComputeCompletenessScope::enter();
        let frame = BuildLocalTaintGuard::push(&self.build_local_taint);
        let step = self.execute_flow_return(key.clone());
        let _ = frame.finish();
        completeness.discard();
        drop(deferred_sticky);
        matches!(step, FlowReturnStep::Complete(_))
            && self
                .dispatch_txn
                .borrow()
                .flow
                .completed_members
                .iter()
                .any(|member| &member.key == key && member.reuse.is_some())
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
        if entry.direct_calls.is_empty() && entry.nested_captures.is_empty() {
            let mut out = Vec::new();
            self.push_instantiated_callees(key, entry, &mut out);
            return out;
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

    /// The callee returns `frame`'s body demands — through its own direct
    /// calls, through the direct calls of the function values it composes,
    /// and, for an instantiated frame, under the instantiation — in source
    /// order, deduplicated.
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
        if entry.lexical_parent.is_none() {
            self.push_direct_callees(frame, index, entry, lowered, &mut out);
        }
        self.push_instantiated_callees(frame, entry, &mut out);
        out
    }

    /// The direct part of [`Self::callees_of`].
    fn push_direct_callees(
        &self,
        frame: &FlowReturnKey,
        index: &FunctionProgramIndex,
        entry: &FunctionProgramEntry,
        lowered: &FlowSliceIR,
        out: &mut Vec<FlowReturnKey>,
    ) {
        let canonical = frame.function.declaration_slot.defining_canonical.as_ref();
        let anchor = entry.span.start;
        // Only a call whose value the slice evaluates: an effect-only
        // expression's calls are never demanded for their return.
        let calls: Vec<(&str, verter_span::Span)> = lowered
            .effects
            .iter()
            .filter_map(|effect| match effect {
                FlowEffect::Call {
                    site,
                    callee: FlowCallee::Named(name),
                    new_construct: false,
                    span,
                } if lowered.expr(*site).role == FlowExprRole::Value => {
                    Some((name.as_ref(), span.to_absolute(anchor)))
                }
                _ => None,
            })
            .collect();
        let every_call: Vec<verter_span::Span> = lowered
            .effects
            .iter()
            .filter_map(|effect| match effect {
                FlowEffect::Call { span, .. } => Some(span.to_absolute(anchor)),
                FlowEffect::Write { .. } => None,
            })
            .collect();
        for (name, span) in calls {
            if nested_in_another_call(span, &every_call) {
                continue;
            }
            self.push_direct_callee(canonical, entry, name, span, out);
        }
        // A function value the slice selects is composed where it sits: its
        // body's return is evaluated inside this frame, calls and all.
        for nested in entry.nested_captures.iter() {
            let relative = FrameSpan::rebase(anchor, nested.span);
            let selected = lowered.exprs.iter().any(|expression| {
                expression.role == FlowExprRole::Value && expression.span.contains(relative)
            });
            if !selected {
                continue;
            }
            self.push_composed_value_callees(canonical, entry, index, &nested.function, out);
        }
    }

    /// Record that the innermost evaluating flow frame demanded the
    /// instantiated callee return `key` — the call resolution's own
    /// answer, which an instantiation of that frame demands again under
    /// its own arguments (see [`Self::push_instantiated_callees`]).
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
    /// function demands: the ones its uninstantiated frame's call
    /// resolution demanded this session, with the frame's own binders
    /// replaced by the instantiation's arguments — by binder NAME, the
    /// mapping the instantiated frame's binder environment applies. A
    /// demand whose arguments are anything but the frame's own binders or
    /// closed primitives and literals is not transported: its resolution
    /// under the arguments is the body's to make.
    fn push_instantiated_callees(
        &self,
        frame: &FlowReturnKey,
        entry: &FunctionProgramEntry,
        out: &mut Vec<FlowReturnKey>,
    ) {
        if is_uninstantiated(frame)
            || entry.type_parameters.len() != frame.normalized_type_args.len()
        {
            return;
        }
        let mut uninstantiated = frame.clone();
        uninstantiated.normalized_type_args = Arc::from(Vec::new().into_boxed_slice());
        uninstantiated.context.type_substitution =
            crate::semantic_query::CanonicalTypeSubstitution::empty();
        let Some(demanded) = self
            .dispatch_txn
            .borrow()
            .flow
            .schedule
            .demands
            .get(&uninstantiated)
            .cloned()
        else {
            return;
        };
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
            if !out.contains(&instantiated) {
                out.push(instantiated);
            }
        }
    }

    /// The direct callees of one composed function value's return sites.
    fn push_composed_value_callees(
        &self,
        canonical: &str,
        frame: &FunctionProgramEntry,
        index: &FunctionProgramIndex,
        value: &FunctionProgramKey,
        out: &mut Vec<FlowReturnKey>,
    ) {
        let Some(value) = index.get(value).map(|matched| matched.entry()) else {
            return;
        };
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
            self.push_direct_callee(canonical, value, name, effect.span, out);
        }
    }

    /// Push the flow-return key the direct-call rail demands for the call
    /// at `span` in `entry`, when the content lowering resolves that call to
    /// a same-file function and the rail takes its return from the body.
    fn push_direct_callee(
        &self,
        canonical: &str,
        entry: &FunctionProgramEntry,
        name: &str,
        span: verter_span::Span,
        out: &mut Vec<FlowReturnKey>,
    ) {
        let Some(direct) = entry.direct_calls.iter().find(|direct| direct.span == span) else {
            return;
        };
        // A call to the function itself is its own recursion hold.
        if direct.target == entry.key {
            return;
        }
        // The callee identifier must be FREE in this frame: a parameter,
        // local or nested declaration of that name shadows the file's
        // function, and the content lowering calls the binding instead.
        let free = entry.references.iter().any(|reference| {
            reference.name.as_ref() == name
                && reference.span.start == span.start
                && matches!(reference.binding, FunctionReferenceBinding::Free)
        });
        if !free {
            return;
        }
        let Some(key) = self.direct_callee_flow_return_key(canonical, &direct.target) else {
            return;
        };
        if !out.contains(&key) {
            out.push(key);
        }
    }

    /// The key the direct-call rail executes for `target` when it takes the
    /// callee's return from its body — mirroring that rail's own choice: an
    /// authored annotation types the callee instead, an overload group is
    /// resolved by the call executor over its declared signatures, and a
    /// declared return carries no body demand.
    fn direct_callee_flow_return_key(
        &self,
        canonical: &str,
        target: &FunctionProgramKey,
    ) -> Option<FlowReturnKey> {
        let prepared = self.ctx.prepared_value_decl_return_only(
            canonical,
            target.declaration.owner,
            target.declaration.name.as_ref(),
        );
        if let Some(prepared) = prepared.as_ref() {
            let annotated = matches!(
                prepared.type_annotation.classification,
                verter_type_expr::facts::ValueAnnotationClass::Direct
                    | verter_type_expr::facts::ValueAnnotationClass::TypeOfAlias
            ) && matches!(
                prepared.type_annotation.annotation,
                Some(verter_type_expr::facts::SemanticTypeSource::Authored(_))
            );
            if annotated || prepared.signatures.len() > 1 {
                return None;
            }
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
                verter_type_expr::facts::FunctionReturnSource::Flow(
                    verter_type_expr::facts::FlowFunctionReturnIdentity {
                        anchor: verter_type_expr::locators::AuthoredAnchor {
                            canonical_id: Arc::from(canonical),
                            owner: target.declaration.owner,
                            symbol: Arc::clone(&target.declaration.name),
                            space: verter_type_expr::locators::LocatorSymbolSpace::Value,
                        },
                        function_part: target.part.clone(),
                        overload_ordinal: target.overload_ordinal,
                    },
                )
            });
        match source {
            verter_type_expr::facts::FunctionReturnSource::Flow(identity) => {
                Some(self.flow_return_key_for(&identity))
            }
            verter_type_expr::facts::FunctionReturnSource::Declared(_)
            | verter_type_expr::facts::FunctionReturnSource::Absent => None,
        }
    }
}

/// Whether `key` addresses its function under no instantiation.
fn is_uninstantiated(key: &FlowReturnKey) -> bool {
    key.normalized_type_args.is_empty() && key.context.type_substitution.bindings().is_empty()
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
