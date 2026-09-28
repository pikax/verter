//! Branch statements evaluated as frames of their region's run.
//!
//! An `if`'s arms, a `switch`'s clauses, a labeled statement's body, a
//! `try`'s clauses and a loop's init, test, body and update are regions of
//! their own. Evaluating each one inside the statement's own
//! evaluation took a native level per nested branch statement — and, where
//! a branch returns a nested function value, a native level per nested
//! function, since that function's body is evaluated from the stack the
//! branch's region was evaluated on. Instead a branch statement suspends
//! its region's frame ([`Entered`]): the region run
//! ([`FlowEvaluator::run_region`]) evaluates the branch's region as its own
//! frame, and the statement resumes with the outcome
//! ([`FlowEvaluator::resume_entered`]), entering its next region or
//! finishing.

use super::*;

/// A statement suspended at a region it entered, with what it resumes
/// with once that region is evaluated.
pub(super) enum Entered<'r> {
    /// A block statement: the scope bases its close replays from.
    Block((usize, usize, usize, usize)),
    If(Box<IfEval<'r>>),
    Switch(Box<SwitchEval<'r>>),
    Labeled(Box<LabeledEval<'r>>),
    Try(Box<TryEval<'r>>),
    Loop(Box<LoopEval<'r>>),
    /// A region no path reaches: the live walk it restores once evaluated.
    Unreachable(Box<DeadPath>),
}

/// A walk of statements no live path reaches, open: the live walk's state,
/// narrowings, edges and read and write modes it restores when it ends.
/// The checker still aggregates the returns and yields there; nothing the
/// walk does reaches the live path.
pub(super) struct DeadPath {
    state: FlowLayerState,
    narrowings: NarrowingSnapshot,
    bases: ScopeBases,
    declared_reads: bool,
    dead_writes: bool,
    /// The length of the dead-written log when the path opened.
    written: usize,
}

/// What a branch statement needs next.
pub(super) enum BranchStep<'r> {
    /// This region evaluated, the statement resuming as `Entered`.
    Enter(Entered<'r>, &'r crate::flow_slice_content::SliceRegion),
    /// Nothing: the statement's contributions and whether it completes.
    Done(Result<(Vec<FlowContribution>, bool), FlowReturnFailure>),
}

/// The scope bases a branch region's close replays from.
#[derive(Clone, Copy)]
struct ScopeBases {
    shadow: usize,
    break_exits: usize,
    return_edges: usize,
    throw_points: usize,
}

/// An `if` between its arms.
pub(super) struct IfEval<'r> {
    guard: &'r crate::flow_slice_content::SliceGuard,
    alternate: Option<&'r crate::flow_slice_content::SliceRegion>,
    contributors: Vec<FlowContribution>,
    entry_products: FlowProductStore,
    entry_writes: FlowWriteObservation,
    narrow_mark: NarrowingSnapshot,
    bases: ScopeBases,
    /// The consequent's end, once it is evaluated.
    consequent: Option<ArmEnd>,
}

/// One `if` arm's end: its products, whether it falls through, and its
/// narrowings.
type ArmEnd = (
    FlowProductStore,
    bool,
    Vec<(
        crate::flow_slice_content::SliceNarrowSubject,
        SemanticNodeId,
    )>,
);

/// A `switch` between its clauses.
pub(super) struct SwitchEval<'r> {
    discriminant: &'r Option<crate::flow_slice_content::SliceNarrowSubject>,
    cases: &'r [crate::flow_slice_content::SliceSwitchCase],
    has_default: bool,
    entry: FlowLayerState,
    bases: ScopeBases,
    tests: Vec<crate::flow_slice_content::SliceGuardLiteral>,
    /// The clauses whose relation is a guard (`switch (typeof x)`,
    /// `switch (true)`): the default edge and the no-matching-case path
    /// apply every one negated.
    guards: Vec<&'r crate::flow_slice_content::SliceGuard>,
    /// The guards of the clauses before the next one.
    guards_before: usize,
    covered: bool,
    chain_end: Option<FlowLayerState>,
    last_end: Option<FlowLayerState>,
    last_falls: bool,
    /// The next clause.
    next: usize,
    contributors: Vec<FlowContribution>,
}

/// A labeled statement whose body is being evaluated.
pub(super) struct LabeledEval<'r> {
    label: &'r Arc<str>,
    entry: FlowLayerState,
    bases: ScopeBases,
}

impl<'d, 'b> FlowEvaluator<'d, 'b> {
    fn scope_bases(&self) -> ScopeBases {
        ScopeBases {
            shadow: self.scope_shadows.len(),
            break_exits: self.break_exits.len(),
            return_edges: self.return_edges.len(),
            throw_points: self.throw_points.len(),
        }
    }

    /// Open a dead path at the current state. Its references read their
    /// declared type when `declared_reads` (the checker answers a
    /// reference on an unreachable flow node so) and their flow type
    /// otherwise; every write on it leaves its reference its declared
    /// type.
    pub(super) fn open_dead_path(&mut self, declared_reads: bool) -> DeadPath {
        let dead = DeadPath {
            state: self.layer_state(),
            narrowings: self.narrowing_snapshot(),
            bases: self.scope_bases(),
            declared_reads: self.declared_reads,
            dead_writes: self.dead_writes,
            written: self.dead_written_log.len(),
        };
        self.declared_reads |= declared_reads;
        self.dead_writes = true;
        dead
    }

    /// Close `dead`, restoring the live walk it saved.
    pub(super) fn close_dead_path(&mut self, dead: DeadPath) {
        self.declared_reads = dead.declared_reads;
        self.dead_writes = dead.dead_writes;
        for subject in self.dead_written_log.drain(dead.written..) {
            self.dead_written.remove(&subject);
        }
        self.break_exits.truncate(dead.bases.break_exits);
        self.return_edges.truncate(dead.bases.return_edges);
        self.throw_points.truncate(dead.bases.throw_points);
        self.scope_shadows.truncate(dead.bases.shadow);
        self.restore_narrowings(dead.narrowings);
        self.restore_layer_state(dead.state);
    }

    /// Record a write to `subject` on a dead path.
    pub(super) fn record_dead_write(&mut self, subject: &FlowProductSubject) {
        if self.dead_writes && self.dead_written.insert(subject.clone()) {
            self.dead_written_log.push(subject.clone());
        }
    }

    /// Whether a read of `subject` answers its declared type: under
    /// [`Self::declared_reads`], or once a dead path wrote it.
    pub(super) fn reads_declared(&self, subject: &FlowProductSubject) -> bool {
        self.declared_reads || self.dead_written.contains(subject)
    }

    fn close_scope_since(&mut self, bases: ScopeBases) -> Vec<ScopeShadow> {
        self.split_scope_shadows_close_exits(
            bases.shadow,
            bases.break_exits,
            bases.return_edges,
            bases.throw_points,
        )
    }

    /// Evaluate one `if` in place, each arm's region evaluated where it is
    /// entered: for an `if` whose arms hold only entered effects (a comma
    /// sequence's discarded conditional), which nest no statement.
    pub(super) fn eval_if(
        &mut self,
        guard: &crate::flow_slice_content::SliceGuard,
        consequent: &crate::flow_slice_content::SliceRegion,
        alternate: Option<&crate::flow_slice_content::SliceRegion>,
    ) -> Result<(Vec<FlowContribution>, bool), FlowReturnFailure> {
        let mut step = self.begin_if(guard, consequent, alternate);
        loop {
            match step {
                BranchStep::Enter(entered, region) => {
                    let outcome = self.eval_region(region);
                    step = self.resume_entered(entered, outcome);
                }
                BranchStep::Done(result) => return result,
            }
        }
    }

    /// Resume the statement `entered` with the outcome of the region it
    /// entered.
    pub(super) fn resume_entered<'r>(
        &mut self,
        entered: Entered<'r>,
        outcome: (Result<Vec<FlowContribution>, FlowReturnFailure>, bool),
    ) -> BranchStep<'r> {
        match entered {
            Entered::Block(_) | Entered::Unreachable(_) => {
                unreachable!("a block resumes in its region's walk")
            }
            Entered::If(eval) => self.if_arm_done(eval, outcome),
            Entered::Switch(eval) => self.switch_clause_done(eval, outcome),
            Entered::Labeled(eval) => self.labeled_body_done(*eval, outcome),
            Entered::Try(eval) => self.try_clause_done(eval, outcome),
            Entered::Loop(eval) => self.loop_region_done(eval, outcome),
        }
    }

    /// Begin one `if`: its contributions and whether it completes, once
    /// its arms are evaluated.
    ///
    /// Bindings are block-scoped: each arm evaluates under its own local
    /// scope, and the consequent reads the test's POSITIVE narrow, the
    /// alternate its NEGATED one. A WHOLE-BINDING WRITE inside an arm
    /// escapes through the branch JOIN, never the raw arm value: after the
    /// `if`, a rebound binding holds the union of its arm value and the
    /// value it had on the paths that never took that arm (tsc's own join
    /// of reaching definitions). An arm whose path TERMINATES (return /
    /// throw / break) does not reach the join at all: its writes leave
    /// with it, and the SURVIVING edge carries the other reading's guard
    /// facts — the negated reading when the consequent terminated, the
    /// positive one when the alternate did (the checker's own rule for
    /// `if (guard) exit; …`). The lexical layer restores; the
    /// function-scoped `var` layer (and parameter writes) join by the same
    /// rule. A reference both continuing arms narrowed reads the union of
    /// its per-arm narrows past the `if`.
    pub(super) fn begin_if<'r>(
        &mut self,
        guard: &'r crate::flow_slice_content::SliceGuard,
        consequent: &'r crate::flow_slice_content::SliceRegion,
        alternate: Option<&'r crate::flow_slice_content::SliceRegion>,
    ) -> BranchStep<'r> {
        let eval = IfEval {
            guard,
            alternate,
            contributors: Vec::new(),
            entry_products: self.products.clone(),
            entry_writes: self.products.observe_writes(),
            narrow_mark: self.narrowing_snapshot(),
            bases: self.scope_bases(),
            consequent: None,
        };
        self.apply_guard_scoped(guard, true);
        BranchStep::Enter(Entered::If(Box::new(eval)), consequent)
    }

    /// An `if` with one arm evaluated.
    fn if_arm_done<'r>(
        &mut self,
        mut eval: Box<IfEval<'r>>,
        (result, falls): (Result<Vec<FlowContribution>, FlowReturnFailure>, bool),
    ) -> BranchStep<'r> {
        // Close the arm's lexical scope BEFORE snapshotting its
        // contribution to the post-if join, and replay the same
        // close on every abrupt edge that crossed the arm.
        let shadows = self.close_scope_since(eval.bases);
        let mut state = self.layer_state();
        Self::close_lexical_scope(&mut state, &shadows);
        let products = state.products;
        let narrowings = self.narrowings_since(&eval.narrow_mark);
        self.restore_narrowings(eval.narrow_mark.clone());
        self.restore_arm_entry(&eval.entry_products);
        match result {
            Ok(contributors) => eval.contributors.extend(contributors),
            Err(failure) => return BranchStep::Done(Err(failure)),
        }
        let Some(consequent) = eval.consequent.take() else {
            eval.consequent = Some((products, falls, narrowings));
            if let Some(alternate) = eval.alternate {
                eval.bases = self.scope_bases();
                self.apply_guard_scoped(eval.guard, false);
                return BranchStep::Enter(Entered::If(eval), alternate);
            }
            // The implicit alternate is a real false-edge
            // predecessor, with no authored body or writes.
            self.apply_guard_scoped(eval.guard, false);
            let products = self.products.clone();
            let narrowings = self.narrowings_since(&eval.narrow_mark);
            self.restore_narrowings(eval.narrow_mark.clone());
            return self.finish_if(&mut eval, (products, true, narrowings));
        };
        eval.consequent = Some(consequent);
        self.finish_if(&mut eval, (products, falls, narrowings))
    }

    fn finish_if<'r>(
        &mut self,
        eval: &mut IfEval<'r>,
        (alternate_products, alternate_falls, alternate_narrowings): ArmEnd,
    ) -> BranchStep<'r> {
        let (consequent_products, consequent_falls, consequent_narrowings) = eval
            .consequent
            .take()
            .expect("the consequent is evaluated first");
        self.restore_arm_entry(&eval.entry_products);
        self.join_arm_writes(
            &consequent_products,
            consequent_falls,
            &alternate_products,
            alternate_falls,
            &eval.entry_products,
            &eval.entry_writes,
        );
        // The checker's join of the two arms: a reference both continuing
        // arms narrowed reads the union of its per-arm narrowed types.
        if consequent_falls && alternate_falls {
            self.join_arm_narrowings(vec![consequent_narrowings, alternate_narrowings]);
        }
        // A single continuing predecessor already carries its final
        // facts. Reapplying its original test here would revive a guard
        // invalidated by a later arm write.
        BranchStep::Done(Ok((
            std::mem::take(&mut eval.contributors),
            consequent_falls || alternate_falls,
        )))
    }

    /// Begin one `switch`.
    ///
    /// tsc's switch flow: a case clause is entered by the dispatch edge
    /// (the state at the switch) AND, for every clause after the first, by
    /// the previous clause's fall-through edge — so each clause starts from
    /// the JOIN of those two states. Each component carries its OWN reading
    /// of the discriminant, baked into the reaching-definition layer before
    /// the join: the dispatch edge into a clause tested positive for the
    /// clause's test (the default clause's edge is the discriminant minus
    /// every test), and the fall-through edge out of a clause carries that
    /// clause's narrow with it — so a fall-through-joined start unions the
    /// chain's tests, exactly the checker's flow. (The narrowing OVERLAY
    /// cannot carry this: the join intersects it, and the two edges' facts
    /// differ.) The state past the switch joins every path that leaves it
    /// normally: the state AT each `break` (never the end state of the
    /// clause the break sits in — a write after the break is not on the
    /// break's edge), falling off the last clause, and the no-matching-case
    /// path when no `default` exists AND the tests do not cover the
    /// discriminant's every arm. The clauses share ONE block scope, exactly
    /// as the authored switch body does.
    pub(super) fn begin_switch<'r>(
        &mut self,
        discriminant: &'r Option<crate::flow_slice_content::SliceNarrowSubject>,
        cases: &'r [crate::flow_slice_content::SliceSwitchCase],
        has_default: bool,
    ) -> BranchStep<'r> {
        let entry = self.layer_state();
        let bases = self.scope_bases();
        // The remainder the DEFAULT edge subtracts is built
        // from the CARRIED relations only. An unrecognized
        // clause contributes nothing to it, which leaves the
        // remainder a SUPERSET of the true default set — the
        // sound direction — and never lets that clause's own
        // values disappear from another clause's edge.
        let tests: Vec<crate::flow_slice_content::SliceGuardLiteral> = cases
            .iter()
            .filter_map(|case| match &case.test {
                crate::flow_slice_content::SliceSwitchTest::Literal(literal) => {
                    Some(literal.clone())
                }
                crate::flow_slice_content::SliceSwitchTest::Default
                | crate::flow_slice_content::SliceSwitchTest::Guard(_)
                | crate::flow_slice_content::SliceSwitchTest::Unmodeled => None,
            })
            .collect();
        let guards: Vec<&crate::flow_slice_content::SliceGuard> = cases
            .iter()
            .filter_map(|case| match &case.test {
                crate::flow_slice_content::SliceSwitchTest::Guard(guard) => Some(&**guard),
                _ => None,
            })
            .collect();
        // Exhaustiveness is a resolver question: the lowering
        // knows only `has_default`, so the no-matching-case
        // path dies here, where the discriminant's arms and
        // the tests can be related.
        // A DECLINED remainder probe (an unlowerable test, a
        // projection miss, an undecided relation) leaves the
        // no-matching-case path live over arms the checker may
        // prove covered — a superset, so it degrades: the
        // liveness verdict is then unproven, never clean.
        let covered = !has_default
            && discriminant.as_ref().is_some_and(|subject| {
                match self.switch_discriminant_remainder(subject, &tests) {
                    Some((remainder, _)) => remainder.is_empty(),
                    None => {
                        self.record_degradation(FlowReturnDegradation::FlowGap(
                            crate::semantic_query::FlowGap::GuardNarrowing,
                        ));
                        false
                    }
                }
            });
        // A destructured element aliasing a narrowing is carried
        // by guards only: its switch takes the typed gap.
        if let Some(subject) = discriminant {
            self.degrade_unaliased_test(subject);
        }
        self.switch_next_clause(Box::new(SwitchEval {
            discriminant,
            cases,
            has_default,
            entry,
            bases,
            tests,
            guards,
            guards_before: 0,
            covered,
            chain_end: None,
            last_end: None,
            last_falls: false,
            next: 0,
            contributors: Vec::new(),
        }))
    }

    /// Enter a `switch`'s next live clause, or finish the statement.
    fn switch_next_clause<'r>(&mut self, mut eval: Box<SwitchEval<'r>>) -> BranchStep<'r> {
        let discriminant = eval.discriminant;
        if let Some(case) = eval.cases.get(eval.next) {
            eval.next += 1;
            // The dispatch component of this clause's start.
            let mut dispatch = eval.entry.clone();
            match &case.test {
                // The clause's own guard, beneath every earlier
                // clause's guard negated (`narrowTypeBySwitchOnTrue`;
                // the `typeof` tests are disjoint, so the negations
                // change nothing there).
                crate::flow_slice_content::SliceSwitchTest::Guard(guard) => {
                    let applied: Vec<(&crate::flow_slice_content::SliceGuard, bool)> = eval.guards
                        [..eval.guards_before]
                        .iter()
                        .map(|earlier| (*earlier, false))
                        .chain(std::iter::once((&**guard, true)))
                        .collect();
                    dispatch = self.guarded_switch_state(&eval.entry, &applied).0;
                    eval.guards_before += 1;
                }
                crate::flow_slice_content::SliceSwitchTest::Default if !eval.guards.is_empty() => {
                    let applied: Vec<(&crate::flow_slice_content::SliceGuard, bool)> =
                        eval.guards.iter().map(|guard| (*guard, false)).collect();
                    // Reachable even when the guards leave the
                    // reference `never`: the clause is typed
                    // through it.
                    dispatch = self.guarded_switch_state(&eval.entry, &applied).0;
                }
                _ => {}
            }
            if let Some(subject) = discriminant {
                self.restore_layer_state(eval.entry.clone());
                match &case.test {
                    // An unrecognized relation: the clause is
                    // reachable for discriminant values this
                    // half cannot enumerate, so its dispatch
                    // edge carries NO narrow. It must never
                    // take the DEFAULT edge — the remainder is
                    // not this clause's reaching set, and
                    // baking it in would publish a type the
                    // clause was never proven to see.
                    crate::flow_slice_content::SliceSwitchTest::Unmodeled
                    | crate::flow_slice_content::SliceSwitchTest::Guard(_) => {}
                    // The dispatch edge: the discriminant IS
                    // this test.
                    // A test no discriminant arm matches bakes
                    // the subject's `never` narrow instead of
                    // killing the dispatch edge: the checker
                    // keeps the clause's contributors typed
                    // through the `never` subject (measured:
                    // `switch (x) { case "b": return 1 }` over
                    // `x: "a"` still contributes `1`).
                    crate::flow_slice_content::SliceSwitchTest::Literal(test) => {
                        match self.narrow_eq_literal(
                            subject,
                            test,
                            false,
                            LiteralComparison::SwitchCase,
                        ) {
                            GuardNarrowing::Narrowed(fact_subject, node) => {
                                self.bake_narrow_into_state(&mut dispatch, &fact_subject, node);
                            }
                            GuardNarrowing::Unchanged => {}
                        }
                    }
                    // The default clause's dispatch edge: the
                    // discriminant minus every carried test.
                    crate::flow_slice_content::SliceSwitchTest::Default => {
                        if let Some((remainder, total)) =
                            self.switch_discriminant_remainder(subject, &eval.tests)
                        {
                            if remainder.len() < total {
                                // Every arm covered leaves the
                                // clause reachable with the
                                // reference `never`: its
                                // contributors are typed through
                                // it, as the checker keeps them.
                                let node =
                                    if remainder.is_empty() {
                                        self.dispatch.graph().intern_node(
                                            SemanticNodeData::Primitive(PrimitiveKind::Never),
                                        )
                                    } else {
                                        self.union(&remainder)
                                    };
                                // The remainder's arms are the
                                // PARENT reference's, so the
                                // fact lands there — the root
                                // for a shallow discriminant,
                                // the enclosing reference for
                                // a nested one.
                                let parent_subject =
                                    crate::flow_slice_content::SliceNarrowSubject {
                                        root: subject.root.clone(),
                                        path: Arc::from(
                                            subject.path[..subject.path.len().saturating_sub(1)]
                                                .to_vec()
                                                .into_boxed_slice(),
                                        ),
                                    };
                                self.bake_narrow_into_state(&mut dispatch, &parent_subject, node);
                            }
                        } else {
                            // A DECLINED probe leaves this
                            // edge carrying the WHOLE
                            // discriminant where the checker
                            // subtracts the matched cases — a
                            // superset, so it degrades rather
                            // than publishing clean.
                            self.record_degradation(FlowReturnDegradation::FlowGap(
                                crate::semantic_query::FlowGap::GuardNarrowing,
                            ));
                        }
                    }
                }
            }
            let start = match &eval.chain_end {
                None => dispatch,
                Some(end) => {
                    let mut start =
                        self.join_states(&[&dispatch, end], &eval.entry.write_observation);
                    // A `var` the fall-through edge first
                    // defines has no reaching definition on the
                    // dispatch edge: flag it so a read fails
                    // closed instead of publishing the
                    // fall-through arm's value clean.
                    self.flag_fallthrough_only_bindings(&mut start, &eval.entry);
                    start
                }
            };
            self.restore_layer_state(start);
            return BranchStep::Enter(Entered::Switch(eval), &case.region);
        }
        self.finish_switch(eval)
    }

    /// A `switch` with its current clause evaluated.
    fn switch_clause_done<'r>(
        &mut self,
        mut eval: Box<SwitchEval<'r>>,
        (result, _): (Result<Vec<FlowContribution>, FlowReturnFailure>, bool),
    ) -> BranchStep<'r> {
        let case = &eval.cases[eval.next - 1];
        match result {
            Ok(contributors) => eval.contributors.extend(contributors),
            Err(failure) => return BranchStep::Done(Err(failure)),
        }
        let end = self.layer_state();
        // Only a clause whose path FALLS THROUGH passes its
        // end state to the next clause's start: a `break` /
        // `return` / `throw` exits the switch, and joining
        // that state into the next case would publish the
        // exited path's writes where the checker has the
        // dispatch edge's values.
        let falls = case
            .region
            .can_fall_through
            .reaches_end(CompletionDischarge::EvaluatorRegionWalk);
        eval.chain_end = falls.then_some(end.clone());
        eval.last_falls = falls;
        eval.last_end = Some(end);
        self.switch_next_clause(eval)
    }

    fn finish_switch<'r>(&mut self, mut eval: Box<SwitchEval<'r>>) -> BranchStep<'r> {
        let mut exit_states: Vec<FlowLayerState> = Vec::new();
        // The no-matching-case edge when only the evaluator proves it dead:
        // the checker still reads the code past the switch through it.
        let mut dead_no_match: Option<FlowLayerState> = None;
        if !eval.has_default && !eval.covered {
            if eval.guards.is_empty() {
                exit_states.push(eval.entry.clone());
            } else {
                // The no-matching-case path sees every guard
                // negated, and is dead when that leaves the
                // tested reference nothing (`isExhaustiveSwitchStatement`).
                let applied: Vec<(&crate::flow_slice_content::SliceGuard, bool)> =
                    eval.guards.iter().map(|guard| (*guard, false)).collect();
                let (state, dead) = self.guarded_switch_state(&eval.entry, &applied);
                if dead {
                    dead_no_match = Some(state);
                } else {
                    exit_states.push(state);
                }
            }
        } else if !eval.has_default {
            // The tests cover every arm of the discriminant: on the
            // no-matching-case edge its reference is `never`.
            if let Some(subject) = eval.discriminant {
                let mut state = eval.entry.clone();
                let never = self
                    .dispatch
                    .graph()
                    .intern_node(SemanticNodeData::Primitive(PrimitiveKind::Never));
                let parent_subject = crate::flow_slice_content::SliceNarrowSubject {
                    root: subject.root.clone(),
                    path: Arc::from(
                        subject.path[..subject.path.len().saturating_sub(1)]
                            .to_vec()
                            .into_boxed_slice(),
                    ),
                };
                self.bake_narrow_into_state(&mut state, &parent_subject, never);
                dead_no_match = Some(state);
            }
        }
        // Lexical bindings declared inside a clause are scoped
        // to the switch body: the close replays on every state
        // the clauses produced — each pending break exit
        // included — BEFORE the join, so a shadowed or
        // clause-declared binding cannot be unioned into the
        // post-switch state, and the clauses' writes to
        // bindings that PREDATE the switch survive.
        let shadows = self.close_scope_since(eval.bases);
        // The `break` exits, with the state each captured at
        // its own point (scope-closed above, like every state
        // that crossed the switch body's scope).
        exit_states.extend(self.drain_break_exits(eval.bases.break_exits, None));
        if eval.last_falls {
            if let Some(mut end) = eval.last_end.take() {
                Self::close_lexical_scope(&mut end, &shadows);
                exit_states.push(end);
            }
        }
        let reaches = !exit_states.is_empty();
        let mut joined = match exit_states.split_first() {
            Some(_) => {
                let incoming: smallvec::SmallVec<[&FlowLayerState; 4]> =
                    exit_states.iter().collect();
                self.join_states(&incoming, &eval.entry.write_observation)
            }
            // No path leaves the switch normally: the
            // post-switch state is unreachable. The code past it
            // reads the dead no-matching-case edge when there is
            // one (the discriminant `never`, every other reference
            // as it reached the switch), else the entry, which keeps
            // the layers sane.
            None => dead_no_match.unwrap_or_else(|| eval.entry.clone()),
        };
        if reaches {
            self.flag_conditionally_defined_bindings(&mut joined, &exit_states);
        }
        self.restore_layer_state(joined);
        BranchStep::Done(Ok((std::mem::take(&mut eval.contributors), reaches)))
    }

    /// Begin a labeled statement.
    ///
    /// The edge past the label joins every path that reaches it: the
    /// body's own fall-through end AND the state captured at each `break`
    /// naming the label — never the pre-statement layers (a write inside
    /// the body IS a reaching definition) and never the body's end state
    /// alone (a break before it carries a state of its own). The narrowing
    /// overlay rides each edge state, so the join's intersection is exactly
    /// "a fact holds past the label only when every path into it
    /// established it".
    pub(super) fn begin_labeled<'r>(
        &mut self,
        label: &'r Arc<str>,
        body: &'r crate::flow_slice_content::SliceRegion,
    ) -> BranchStep<'r> {
        let eval = LabeledEval {
            label,
            entry: self.layer_state(),
            bases: self.scope_bases(),
        };
        BranchStep::Enter(Entered::Labeled(Box::new(eval)), body)
    }

    fn labeled_body_done<'r>(
        &mut self,
        eval: LabeledEval<'r>,
        (result, body_falls): (Result<Vec<FlowContribution>, FlowReturnFailure>, bool),
    ) -> BranchStep<'r> {
        let body_contributors = match result {
            Ok(contributors) => contributors,
            Err(failure) => return BranchStep::Done(Err(failure)),
        };
        let mut end = self.layer_state();
        // The body's scope close replays on every state the
        // body produced — each pending break exit included —
        // BEFORE the join, so a shadowed or body-declared
        // binding cannot be unioned into the post-statement
        // state.
        let shadows = self.close_scope_since(eval.bases);
        let mut exits: Vec<FlowLayerState> =
            self.drain_break_exits(eval.bases.break_exits, Some(eval.label));
        Self::close_lexical_scope(&mut end, &shadows);
        if body_falls {
            exits.push(end);
        }
        let reaches = !exits.is_empty();
        let mut joined = match exits.split_first() {
            Some(_) => {
                let incoming: smallvec::SmallVec<[&FlowLayerState; 4]> = exits.iter().collect();
                self.join_states(&incoming, &eval.entry.write_observation)
            }
            // No path leaves the body: the post-statement
            // state is unreachable; restore the entry to keep
            // the layers sane.
            None => eval.entry.clone(),
        };
        if reaches {
            // A `var` the body first defines has no reaching
            // definition on an edge that skips the definition
            // (a break before it): flag it, exactly like the
            // switch's own exit join does.
            self.flag_conditionally_defined_bindings(&mut joined, &exits);
        }
        self.restore_layer_state(joined);
        BranchStep::Done(Ok((body_contributors, reaches)))
    }
}

/// A `try` clause being evaluated: what its evaluation is read against.
pub(super) struct TryClause {
    start: FlowLayerState,
    write_observation: FlowWriteObservation,
    shadow_base: usize,
    throw_base: usize,
    break_base: usize,
    return_base: usize,
    saved_collect: bool,
}

/// Which clause of a `try` is being evaluated.
#[derive(Clone, Copy)]
enum TryPhase {
    Block,
    Catch,
    Finally,
}

/// A `try` statement between its clauses.
pub(super) struct TryEval<'r> {
    block: &'r crate::flow_slice_content::SliceRegion,
    catch: Option<&'r crate::flow_slice_content::SliceCatchClause>,
    finally: Option<&'r crate::flow_slice_content::SliceRegion>,
    pending_break_contributes_undefined: bool,
    pending_break_following_return_targets: &'r Arc<[Arc<str>]>,
    entry: FlowLayerState,
    break_base: usize,
    return_base: usize,
    throw_base: usize,
    own: Vec<FlowContribution>,
    exit_states: Vec<FlowLayerState>,
    try_narrowings: Vec<FlowNarrowingFact>,
    try_writes: Option<FlowClauseWrites>,
    block_throws: Vec<FlowLayerState>,
    catch_writes: Option<FlowClauseWrites>,
    pre_finally: Option<FlowLayerState>,
    finally_break_base: usize,
    finally_return_base: usize,
    clause: Option<TryClause>,
    phase: TryPhase,
}

impl<'d, 'b> FlowEvaluator<'d, 'b> {
    /// Begin one `try` clause's evaluation from `start`: the region to
    /// evaluate is the caller's, and [`Self::finish_try_clause`] reads its
    /// outcome.
    fn begin_try_clause(
        &mut self,
        start: &FlowLayerState,
        catch_param: Option<(
            SkeletonBindingId,
            Option<&crate::flow_slice_content::GatedType>,
        )>,
        collect_throws: bool,
    ) -> TryClause {
        let write_observation = self.products.observe_writes();
        self.restore_layer_state(start.clone());
        let clause = TryClause {
            start: start.clone(),
            write_observation,
            shadow_base: self.scope_shadows.len(),
            throw_base: self.throw_points.len(),
            break_base: self.break_exits.len(),
            return_base: self.return_edges.len(),
            saved_collect: self.collect_throw_points,
        };
        self.collect_throw_points = collect_throws;
        if let Some((param, declared)) = catch_param.filter(|(param, _)| {
            self.products
                .contains_subject(&FlowProductSubject::Local(*param))
        }) {
            let subject = FlowProductSubject::Local(param);
            self.record_scope_shadow(&subject);
            // The catch variable's declared type is its annotation (`any`
            // or `unknown`), else `unknown` under the project's
            // `useUnknownInCatchVariables` and `any` without it; an
            // assignment to it narrows nothing (a declared type that is
            // not a union).
            let declared = match declared {
                Some(declared)
                    if !declared
                        .shadowed()
                        .iter()
                        .any(|name| self.owner_scope_answers_name(name)) =>
                {
                    self.lower_body_type(declared.ty())
                }
                Some(_) => {
                    super::super::flow_return_callee::unmodeled_position_marker(self.dispatch)
                }
                None => self
                    .dispatch
                    .graph()
                    .intern_node(SemanticNodeData::Primitive(
                        if self.use_unknown_in_catch_variables {
                            PrimitiveKind::Unknown
                        } else {
                            PrimitiveKind::Any
                        },
                    )),
            };
            self.set_declared_local(
                &subject,
                crate::flow_slice_content::SliceBindingKind::Let,
                Some(declared),
            );
            self.bind_local(
                &subject,
                crate::flow_slice_content::SliceBindingKind::Let,
                declared,
                None,
                false,
            );
        }
        clause
    }

    /// One `try` clause's contributions, its end state and its writes, from
    /// the outcome of its region.
    fn finish_try_clause(
        &mut self,
        clause: TryClause,
        (result, _): (Result<Vec<FlowContribution>, FlowReturnFailure>, bool),
    ) -> Result<(Vec<FlowContribution>, FlowLayerState, FlowClauseWrites), FlowReturnFailure> {
        let TryClause {
            start,
            write_observation,
            shadow_base,
            throw_base,
            break_base,
            return_base,
            saved_collect,
        } = clause;
        self.collect_throw_points = saved_collect;
        let contributions = result?;
        let mut end = self.layer_state();
        // The scope close replays on every state the clause's evaluation
        // produced: the end state, the throw points, and the pending
        // break exits that crossed the clause's scope.
        let shadows =
            self.split_scope_shadows_close_exits(shadow_base, break_base, return_base, throw_base);
        Self::close_lexical_scope(&mut end, &shadows);
        let executed = self.written_between(&write_observation, &end);
        // Preserve the clause-refusal contract without confusing it with
        // execution evidence. Only written subjects need a type comparison.
        let type_changes = FlowClauseTypeChanges(
            executed
                .0
                .iter()
                .filter(|subject| {
                    start.products.reaching(subject) != end.products.reaching(subject)
                })
                .cloned()
                .collect(),
        );
        self.restore_layer_state(end.clone());
        Ok((
            contributions,
            end,
            FlowClauseWrites {
                executed,
                type_changes,
            },
        ))
    }

    /// Begin one `try` statement.
    ///
    /// The catch / finally clauses are entered from ANY throw point of the
    /// try block, so they start from the JOIN of the try's ENTRY state with
    /// the state captured at each call / `throw` inside the block: the
    /// checker enters the catch from every one of those points, so a write
    /// between two throw points is exactly as visible to the clause as the
    /// checker has it. Every try-internal write is additionally flagged (an
    /// ELIDED call is a throw point this model never captures — the flag is
    /// the fail-closed net), and the overlay carries none of the try's
    /// narrow facts (tsgo: a `catch` / `finally` body reads the pre-try
    /// type, never the narrowed one). Past the statement, a
    /// clause-established narrow survives ONLY when no `catch` exists — the
    /// abrupt paths then leave the frame, so the normal-completion path's
    /// facts hold (tsgo narrows there) — minus any the finally clause's own
    /// writes killed. Return inference aggregates every authored return
    /// contribution, including a try return whose runtime completion is
    /// overridden by an abrupt finally.
    pub(super) fn begin_try<'r>(
        &mut self,
        block: &'r crate::flow_slice_content::SliceRegion,
        catch: Option<&'r crate::flow_slice_content::SliceCatchClause>,
        finally: Option<&'r crate::flow_slice_content::SliceRegion>,
        pending_break_contributes_undefined: bool,
        pending_break_following_return_targets: &'r Arc<[Arc<str>]>,
    ) -> BranchStep<'r> {
        let entry = self.layer_state();
        let break_base = self.break_exits.len();
        let return_base = self.return_edges.len();
        let throw_base = self.throw_points.len();
        let clause = self.begin_try_clause(&entry, None, true);
        BranchStep::Enter(
            Entered::Try(Box::new(TryEval {
                block,
                catch,
                finally,
                pending_break_contributes_undefined,
                pending_break_following_return_targets,
                entry,
                break_base,
                return_base,
                throw_base,
                own: Vec::new(),
                exit_states: Vec::new(),
                try_narrowings: Vec::new(),
                try_writes: None,
                block_throws: Vec::new(),
                catch_writes: None,
                pre_finally: None,
                finally_break_base: 0,
                finally_return_base: 0,
                clause: Some(clause),
                phase: TryPhase::Block,
            })),
            block,
        )
    }

    /// A `try` statement with its current clause evaluated.
    fn try_clause_done<'r>(
        &mut self,
        mut eval: Box<TryEval<'r>>,
        outcome: (Result<Vec<FlowContribution>, FlowReturnFailure>, bool),
    ) -> BranchStep<'r> {
        let clause = eval.clause.take().expect("the clause being evaluated");
        let (contributors, end, writes) = match self.finish_try_clause(clause, outcome) {
            Ok(clause) => clause,
            Err(failure) => return BranchStep::Done(Err(failure)),
        };
        eval.own.extend(contributors);
        match eval.phase {
            TryPhase::Block => {
                eval.try_narrowings = narrowing_facts_of(&end.products);
                if eval
                    .block
                    .can_fall_through
                    .reaches_end(CompletionDischarge::EvaluatorRegionWalk)
                {
                    eval.exit_states.push(end);
                }
                // The try block's throw points. A catch consumes them
                // into its entry join; with no catch the throw paths
                // leave the frame (through the finally), so they stay
                // on the stack for an OUTER try's catch to consume.
                eval.block_throws = if eval.catch.is_some() {
                    self.throw_points.split_off(eval.throw_base)
                } else {
                    self.throw_points[eval.throw_base..].to_vec()
                };
                if let Some(catch) = eval.catch {
                    let mut catch_start = {
                        let incoming: smallvec::SmallVec<[&FlowLayerState; 4]> =
                            std::iter::once(&eval.entry)
                                .chain(eval.block_throws.iter())
                                .collect();
                        self.join_states(&incoming, &eval.entry.write_observation)
                    };
                    self.flag_clause_type_changes(&mut catch_start, &writes.type_changes);
                    eval.try_writes = Some(writes);
                    let clause = self.begin_try_clause(
                        &catch_start,
                        catch
                            .binding
                            .map(|binding| (binding, catch.declared.as_ref())),
                        eval.finally.is_some(),
                    );
                    eval.clause = Some(clause);
                    eval.phase = TryPhase::Catch;
                    return BranchStep::Enter(Entered::Try(eval), &catch.region);
                }
                eval.try_writes = Some(writes);
                self.try_past_clauses(eval)
            }
            TryPhase::Catch => {
                let catch = eval.catch.expect("a catch clause is evaluated");
                eval.catch_writes = Some(writes);
                if catch
                    .region
                    .can_fall_through
                    .reaches_end(CompletionDischarge::EvaluatorRegionWalk)
                {
                    eval.exit_states.push(end);
                }
                self.try_past_clauses(eval)
            }
            TryPhase::Finally => self.finish_try(eval, end, writes),
        }
    }

    /// A `try` statement past its block and catch clause: its finally
    /// clause entered, or the statement finished.
    fn try_past_clauses<'r>(&mut self, mut eval: Box<TryEval<'r>>) -> BranchStep<'r> {
        let try_writes = eval.try_writes.take().expect("the try block is evaluated");
        // The pre-finally state joins every NORMAL completion
        // of the try/catch. With none, no state reaches past
        // the try — the entry stands in only to keep the
        // finally clause's own evaluation well-formed. The
        // clause writes stay flagged through it: the finally
        // (and the post-statement path) runs on the throw paths
        // too.
        let mut pre_finally = match eval.exit_states.split_first() {
            Some(_) => {
                let incoming: smallvec::SmallVec<[&FlowLayerState; 4]> =
                    eval.exit_states.iter().collect();
                self.join_states(&incoming, &eval.entry.write_observation)
            }
            None => eval.entry.clone(),
        };
        self.flag_clause_type_changes(&mut pre_finally, &try_writes.type_changes);
        if let Some(catch_writes) = &eval.catch_writes {
            self.flag_clause_type_changes(&mut pre_finally, &catch_writes.type_changes);
        }
        if !eval.exit_states.is_empty() {
            self.flag_conditionally_defined_bindings(&mut pre_finally, &eval.exit_states);
        }
        let Some(finally) = eval.finally else {
            // A catch clause exists (a bare `try` is
            // syntactically impossible without either
            // clause): its antecedent joins the flow past
            // the statement, so no clause-established
            // narrow survives — even when the catch itself
            // returns (tsgo, measured).
            self.restore_clause_entry_narrowings(&eval.entry, &mut pre_finally.products);
            self.restore_layer_state(pre_finally);
            let path_alive = !eval.exit_states.is_empty();
            return BranchStep::Done(Ok((std::mem::take(&mut eval.own), path_alive)));
        };
        // The finally BODY runs on every completion:
        // its start joins the normal completions with
        // the try's ENTRY (a throw can precede every
        // try-internal write — the checker reads the
        // pre-try value inside the finally too), every
        // throw point of the clauses, and every
        // pending abrupt edge's pre-state (`break` and
        // `return` both cross the finally before their
        // completion proceeds). Its overlay is
        // the ENTRY's, whatever the clauses
        // established (tsgo: a narrow from the try
        // does not apply inside the finally). The
        // dual does NOT hold: the finally's own
        // writes never merge into a pending abrupt
        // edge's continuation — the edge keeps the
        // value its point captured (tsgo, measured).
        // And the state PAST the statement is not the
        // finally body's wide start either: only the
        // normal completions reach it, plus the
        // finally's own writes.
        // Normal and abrupt completions are original inputs to
        // this merge, never binary union prefixes. Clause-local
        // narrows do not enter finally on normal completions.
        let mut normal_inputs = eval.exit_states.clone();
        normal_inputs.push(eval.entry.clone());
        for state in &mut normal_inputs {
            self.restore_clause_entry_narrowings(&eval.entry, &mut state.products);
            self.flag_clause_type_changes(state, &try_writes.type_changes);
            if let Some(written) = &eval.catch_writes {
                self.flag_clause_type_changes(state, &written.type_changes);
            }
        }
        let clause_throws = self.throw_points[eval.throw_base..].to_vec();
        let pending_exits: Vec<FlowLayerState> = self.break_exits[eval.break_base..]
            .iter()
            .map(|exit| exit.state.clone())
            .collect();
        let pending_returns = self.return_edges[eval.return_base..].to_vec();
        let finally_start = {
            let incoming: smallvec::SmallVec<[&FlowLayerState; 4]> = normal_inputs
                .iter()
                // Without a catch, block throws are already in
                // clause_throws; each actual predecessor enters once.
                .chain(eval.block_throws.iter().filter(|_| eval.catch.is_some()))
                .chain(clause_throws.iter())
                .chain(pending_exits.iter())
                .chain(pending_returns.iter())
                .collect();
            self.join_states(&incoming, &eval.entry.write_observation)
        };
        eval.finally_break_base = self.break_exits.len();
        eval.finally_return_base = self.return_edges.len();
        eval.try_writes = Some(try_writes);
        eval.pre_finally = Some(pre_finally);
        let clause = self.begin_try_clause(&finally_start, None, false);
        eval.clause = Some(clause);
        eval.phase = TryPhase::Finally;
        BranchStep::Enter(Entered::Try(eval), finally)
    }

    /// A `try` statement with its finally clause evaluated, ending at
    /// `finally_end` with `finally_writes`.
    fn finish_try<'r>(
        &mut self,
        mut eval: Box<TryEval<'r>>,
        finally_end: FlowLayerState,
        finally_writes: FlowClauseWrites,
    ) -> BranchStep<'r> {
        let finally = eval.finally.expect("a finally clause is evaluated");
        let try_writes = eval.try_writes.take().expect("the try block is evaluated");
        let pre_finally = eval.pre_finally.take().expect("the pre-finally state");
        let (break_base, return_base) = (eval.break_base, eval.return_base);
        let (finally_break_base, finally_return_base) =
            (eval.finally_break_base, eval.finally_return_base);
        // The post-statement state: the normal
        // completions (pre_finally, with its flags and
        // the entry's overlay) plus exactly the
        // finally's own writes.
        let mut post = pre_finally.clone();
        self.restore_clause_entry_narrowings(&eval.entry, &mut post.products);
        for subject in &finally_writes.executed.0 {
            post.products
                .apply_executed_write_from(subject, &finally_end.products);
            self.narrowing_writes.push(NarrowingLedgerEntry::Cleared {
                root: self.canonical_runtime_subject(subject),
            });
        }
        self.restore_layer_state(post);
        if eval.catch.is_none() {
            // No catch: the abrupt paths leave the
            // frame, so past the statement the
            // normal-completion path's narrow facts
            // hold again — re-establish the try's,
            // minus any the finally's own writes
            // killed — and the clause-write flags lose
            // their reason: no path past the statement
            // can have skipped those writes.
            let mut killed = rustc_hash::FxHashSet::default();
            for subject in &finally_writes.executed.0 {
                if let Some(root) = self.narrow_root_of(subject) {
                    if let Some(identity) = self.products.identity(&root) {
                        #[cfg(test)]
                        self.dispatch
                            .ctx
                            .host_for_fact_tracer_install()
                            .flow_fault_injection
                            .finally_identity_work
                            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        killed.insert(identity);
                    }
                }
            }
            // A fact EQUAL to one the entry carried is
            // re-established like any other. The try
            // PROVED it on the only path that reaches
            // here, and whether the entering overlay
            // happened to hold the same fact says
            // nothing about that: when the clause also
            // WROTE the binding, the restore dropped the
            // entering fact as stale and this proof is
            // the only thing standing. (The same reason
            // `standing_narrowings` counts a
            // re-establishment over the write ledger
            // instead of diffing the overlay: a state
            // diff cannot tell a proof from an
            // untouched position.) One product write per
            // root, and none for a root whose restored
            // product already holds the try's facts.
            let mut restored = eval
                .try_narrowings
                .iter()
                .filter(|fact| !killed.contains(&fact.binding))
                .peekable();
            let mut facts: Vec<FlowNarrowingFact> = Vec::new();
            while let Some(first) = restored.next() {
                facts.clear();
                facts.push(first.clone());
                while let Some(next) = restored.next_if(|next| next.binding == first.binding) {
                    facts.push(next.clone());
                }
                let subject = self
                    .bindings
                    .local(&first.binding)
                    .map(FlowProductSubject::Local)
                    .unwrap_or_else(|| FlowProductSubject::Captured(first.binding.clone()));
                reestablish_narrowings(&mut self.products, &subject, &facts);
            }
            for subject in &try_writes.type_changes.0 {
                if eval.entry.products.assignment(subject).single_path() {
                    continue;
                }
                let cleared = self.products.assignment(subject).with_single_path(false);
                self.products.set_assignment(subject, cleared);
            }
        }
        if !finally
            .can_fall_through
            .reaches_end(CompletionDischarge::EvaluatorRegionWalk)
        {
            if eval.pending_break_contributes_undefined && finally_break_base > break_base {
                self.observations.observe_implicit_undefined();
            }
            // Control edges remain runtime-honest: an
            // abrupt finally replaces pending try/catch
            // returns with its own return edges before an
            // OUTER finally is entered.
            let finally_returns = self.return_edges.split_off(finally_return_base);
            self.return_edges.truncate(return_base);
            self.return_edges.extend(finally_returns);
            let retained_pending_breaks: Vec<FlowBreakExit> = self.break_exits
                [break_base..finally_break_base]
                .iter()
                .filter(|exit| {
                    !exit.continues
                        && exit.target.as_ref().is_some_and(|target| {
                            eval.pending_break_following_return_targets.contains(target)
                        })
                })
                .cloned()
                .collect();
            let finally_breaks = self.break_exits.split_off(finally_break_base);
            self.break_exits.truncate(break_base);
            self.break_exits.extend(retained_pending_breaks);
            self.break_exits.extend(finally_breaks);
        }
        let path_alive = !eval.exit_states.is_empty()
            && finally
                .can_fall_through
                .reaches_end(CompletionDischarge::EvaluatorRegionWalk);
        BranchStep::Done(Ok((std::mem::take(&mut eval.own), path_alive)))
    }
}

/// A loop statement between the regions of its evaluation: its init, the
/// passes of its head analysis and its converged pass.
pub(super) struct LoopEval<'r> {
    lowered: &'r crate::flow_slice_content::SliceLoop,
    circular: Vec<SkeletonBindingId>,
    bases: ScopeBases,
    contributors: Vec<FlowContribution>,
    element: Option<(
        &'r crate::flow_slice_content::SliceLoopElement,
        SemanticNodeId,
    )>,
    entry: Option<FlowLayerState>,
    head: Option<LoopHeadAnalysis>,
    /// The converged pass's head, once the head analysis is done.
    converged: Option<FlowLayerState>,
    pass: Option<PassEval>,
}

/// The per-reference analysis of a loop's head
/// ([`FlowEvaluator::advance_loop_head`]), from an explicit stack of the
/// references under analysis.
struct LoopHeadAnalysis {
    carried: Vec<FlowProductSubject>,
    dependencies: Vec<Vec<usize>>,
    memo: rustc_hash::FxHashMap<usize, FlowProductStore>,
    head: FlowLayerState,
    /// The next carried reference whose head the head takes.
    next_subject: usize,
    in_analysis: Vec<usize>,
    references: Vec<ReferenceHead>,
}

/// One reference whose head is being analysed: the state its pass starts
/// from, the next reference of its dependency closure, and the side-output
/// mark its pass rewinds to.
struct ReferenceHead {
    subject: usize,
    start: FlowLayerState,
    next: usize,
    mark: Option<LoopPassMark>,
}

/// One pass of a loop body ([`FlowEvaluator::begin_loop_pass`]) between
/// its regions.
struct PassEval {
    head: FlowLayerState,
    break_base: usize,
    return_base: usize,
    throw_base: usize,
    shadow_base: usize,
    narrow_mark: NarrowingSnapshot,
    tested: Option<FlowLayerState>,
    contributors: Vec<FlowContribution>,
    phase: PassPhase,
}

#[derive(Clone, Copy)]
enum PassPhase {
    TestBefore,
    Body,
    Update,
    TestAfter,
}

/// What a pass needs next.
enum PassStep<'r> {
    Enter(PassEval, &'r crate::flow_slice_content::SliceRegion),
    Done(Result<LoopPass, FlowReturnFailure>),
}

impl<'d, 'b> FlowEvaluator<'d, 'b> {
    /// Begin evaluating a loop statement: its init region first.
    pub(super) fn begin_loop<'r>(
        &mut self,
        lowered: &'r crate::flow_slice_content::SliceLoop,
    ) -> BranchStep<'r> {
        let circular: Vec<SkeletonBindingId> = self
            .circular_loop_bindings(lowered)
            .into_iter()
            .filter(|binding| self.circular_inferred.insert(*binding))
            .collect();
        let bases = self.scope_bases();
        BranchStep::Enter(
            Entered::Loop(Box::new(LoopEval {
                lowered,
                circular,
                bases,
                contributors: Vec::new(),
                element: None,
                entry: None,
                head: None,
                converged: None,
                pass: None,
            })),
            &lowered.init,
        )
    }

    /// A loop statement with one of its regions evaluated.
    fn loop_region_done<'r>(
        &mut self,
        mut eval: Box<LoopEval<'r>>,
        outcome: (Result<Vec<FlowContribution>, FlowReturnFailure>, bool),
    ) -> BranchStep<'r> {
        let pass = match eval.pass.take() {
            Some(pass) => match self.pass_region_done(eval.lowered, eval.element, pass, outcome) {
                PassStep::Enter(pass, region) => {
                    eval.pass = Some(pass);
                    return BranchStep::Enter(Entered::Loop(eval), region);
                }
                PassStep::Done(pass) => pass,
            },
            None => {
                // The init region.
                let (init_result, _) = outcome;
                match init_result {
                    Ok(init_contributors) => eval.contributors.extend(init_contributors),
                    Err(failure) => return self.end_loop(eval, Err(failure)),
                }
                eval.element = eval
                    .lowered
                    .element
                    .as_ref()
                    .and_then(|element| self.loop_element_node(element));
                let entry = self.layer_state();
                let carried = loop_carried_subjects(eval.lowered);
                let dependencies = self.loop_reference_dependencies(eval.lowered, &carried);
                eval.head = Some(LoopHeadAnalysis {
                    carried,
                    dependencies,
                    memo: rustc_hash::FxHashMap::default(),
                    head: entry.clone(),
                    next_subject: 0,
                    in_analysis: Vec::new(),
                    references: Vec::new(),
                });
                eval.entry = Some(entry);
                return self.advance_loop_head(eval);
            }
        };
        let pass = match pass {
            Ok(pass) => pass,
            Err(failure) => return self.end_loop(eval, Err(failure)),
        };
        if eval.converged.is_some() {
            return self.finish_loop(eval, pass);
        }
        // A reference's head pass: its head joins the entry with the
        // pass's back edge.
        let entry = eval.entry.as_ref().expect("the loop's entry state");
        let analysis = eval.head.as_mut().expect("the head analysis");
        let reference = analysis.references.pop().expect("the reference analysed");
        self.rewind_loop_pass(reference.mark.as_ref().expect("the pass's mark"));
        let products = match pass.back {
            Some(back) => {
                self.join_continuations(
                    &[entry.clone(), back],
                    &entry.products,
                    &entry.write_observation,
                )
                .products
            }
            None => entry.products.clone(),
        };
        analysis.memo.insert(reference.subject, products.clone());
        let carried = &analysis.carried[reference.subject];
        match analysis.references.last_mut() {
            Some(parent) => parent
                .start
                .products
                .restore_reaching_from(carried, &products, None, true),
            None => {
                analysis
                    .head
                    .products
                    .restore_reaching_from(carried, &products, None, true);
                analysis.next_subject += 1;
            }
        }
        self.advance_loop_head(eval)
    }

    /// The loop-head products of a carried reference known without a pass
    /// of its own: its entry state when it enters at its declared type, or
    /// the head already analysed for it.
    fn known_reference_head(
        &mut self,
        analysis: &LoopHeadAnalysis,
        entry: &FlowLayerState,
        subject: usize,
    ) -> Option<FlowProductStore> {
        // A reference entering the loop at its declared type holds it
        // at the head: every antecedent the checker's loop label would
        // join is a subtype of it (`let i = 0 as 0 | 1; while (i < n)
        // i++` leaves `i` `0 | 1`).
        if self.loop_entry_holds_declared(&analysis.carried[subject]) {
            return Some(entry.products.clone());
        }
        analysis.memo.get(&subject).cloned()
    }

    /// Advance a loop's head analysis until a reference's pass is to run
    /// (entered), or the head is known and the converged pass begins.
    ///
    /// The state at a loop's head, typed per reference as the checker's
    /// loop label types it (`getTypeAtFlowLoopLabel`). A reference the loop
    /// writes ([`loop_carried_subjects`]) holds its entry state joined with
    /// ONE pass of the loop, in which it reads its entry type — a loop
    /// label under analysis answers with the types it has so far — and
    /// every other written reference its value depends on
    /// ([`crate::flow_slice_content::SliceLoop::writes`]) reads its own head
    /// type, analysed with this one under analysis too. A reference the
    /// loop does not write holds its entry state. A reference first typed
    /// while others are under analysis keeps that type wherever else the
    /// analysis reads it, as the checker caches each reference's loop-label
    /// type once it has one.
    fn advance_loop_head<'r>(&mut self, mut eval: Box<LoopEval<'r>>) -> BranchStep<'r> {
        loop {
            let entry = eval.entry.as_ref().expect("the loop's entry state");
            let analysis = eval.head.as_mut().expect("the head analysis");
            if let Some(reference) = analysis.references.last_mut() {
                let subject = reference.subject;
                let mut pending = None;
                while let Some(&other) = analysis.dependencies[subject].get(reference.next) {
                    reference.next += 1;
                    if analysis.in_analysis.contains(&other) {
                        continue;
                    }
                    pending = Some(other);
                    break;
                }
                if let Some(other) = pending {
                    let known = {
                        let analysis = eval.head.as_ref().expect("the head analysis");
                        self.known_reference_head(analysis, entry, other)
                    };
                    let analysis = eval.head.as_mut().expect("the head analysis");
                    match known {
                        Some(products) => {
                            let carried = analysis.carried[other].clone();
                            analysis
                                .references
                                .last_mut()
                                .expect("the reference analysed")
                                .start
                                .products
                                .restore_reaching_from(&carried, &products, None, true);
                        }
                        None => {
                            analysis.in_analysis.push(other);
                            analysis.references.push(ReferenceHead {
                                subject: other,
                                start: entry.clone(),
                                next: 0,
                                mark: None,
                            });
                        }
                    }
                    continue;
                }
                // Its dependency closure is analysed: its own pass runs.
                analysis.in_analysis.pop();
                let mark = self.loop_pass_mark();
                let start = reference.start.clone();
                reference.mark = Some(mark);
                let lowered = eval.lowered;
                let element = eval.element;
                return self.enter_loop_pass(eval, lowered, start, element);
            }
            if analysis.next_subject < analysis.carried.len() {
                let subject = analysis.next_subject;
                let known = {
                    let analysis = eval.head.as_ref().expect("the head analysis");
                    self.known_reference_head(analysis, entry, subject)
                };
                let analysis = eval.head.as_mut().expect("the head analysis");
                match known {
                    Some(products) => {
                        let carried = analysis.carried[subject].clone();
                        analysis
                            .head
                            .products
                            .restore_reaching_from(&carried, &products, None, true);
                        analysis.next_subject += 1;
                    }
                    None => {
                        analysis.in_analysis.push(subject);
                        analysis.references.push(ReferenceHead {
                            subject,
                            start: entry.clone(),
                            next: 0,
                            mark: None,
                        });
                    }
                }
                continue;
            }
            // The head is known: the converged pass runs from it.
            let head = eval.head.take().expect("the head analysis").head;
            eval.converged = Some(head.clone());
            let lowered = eval.lowered;
            let element = eval.element;
            return self.enter_loop_pass(eval, lowered, head, element);
        }
    }

    /// Begin a pass of `eval`'s loop from `head`, entering its first
    /// region, or finishing it at once.
    fn enter_loop_pass<'r>(
        &mut self,
        mut eval: Box<LoopEval<'r>>,
        lowered: &'r crate::flow_slice_content::SliceLoop,
        head: FlowLayerState,
        element: Option<(
            &'r crate::flow_slice_content::SliceLoopElement,
            SemanticNodeId,
        )>,
    ) -> BranchStep<'r> {
        match self.begin_loop_pass(lowered, head, element) {
            PassStep::Enter(pass, region) => {
                eval.pass = Some(pass);
                BranchStep::Enter(Entered::Loop(eval), region)
            }
            PassStep::Done(_) => unreachable!("a pass enters its test or its body"),
        }
    }

    /// Begin one pass of a loop body from `head`: the head's test (its
    /// writes, and a throw point when it calls), the element binding, the
    /// body under the test's positive reading, and the back edge — the
    /// body's end joined with every `continue` of this loop, run through
    /// the `for` update.
    fn begin_loop_pass<'r>(
        &mut self,
        lowered: &'r crate::flow_slice_content::SliceLoop,
        head: FlowLayerState,
        element: Option<(
            &'r crate::flow_slice_content::SliceLoopElement,
            SemanticNodeId,
        )>,
    ) -> PassStep<'r> {
        use crate::flow_slice_content::SliceLoopTest;
        #[cfg(test)]
        LOOP_PASSES.with(|count| count.set(count.get() + 1));
        let pass = PassEval {
            break_base: self.break_exits.len(),
            return_base: self.return_edges.len(),
            throw_base: self.throw_points.len(),
            shadow_base: self.scope_shadows.len(),
            narrow_mark: self.narrowing_snapshot(),
            tested: None,
            contributors: Vec::new(),
            phase: PassPhase::TestBefore,
            head,
        };
        self.restore_layer_state(pass.head.clone());
        if let SliceLoopTest::Before { .. } = &lowered.test {
            if lowered.test_throws {
                self.capture_throw_point();
            }
            return PassStep::Enter(pass, &lowered.test_effects);
        }
        self.enter_loop_body(lowered, element, pass)
    }

    /// Bind a pass's element and enter its body.
    fn enter_loop_body<'r>(
        &mut self,
        lowered: &'r crate::flow_slice_content::SliceLoop,
        element: Option<(
            &'r crate::flow_slice_content::SliceLoopElement,
            SemanticNodeId,
        )>,
        mut pass: PassEval,
    ) -> PassStep<'r> {
        if let Some((element, node)) = element {
            if let Some(element) = element.binding.as_ref() {
                let subject = FlowProductSubject::Local(element.binding);
                if element.kind != crate::flow_slice_content::SliceBindingKind::Var {
                    self.record_scope_shadow(&subject);
                }
                self.set_declared_local(&subject, element.kind, Some(node));
                self.bind_local(&subject, element.kind, node, None, false);
            } else if let Some((pattern, kind)) = element.pattern.as_ref() {
                self.bind_loop_pattern(pattern, node, *kind);
            }
        }
        pass.phase = PassPhase::Body;
        PassStep::Enter(pass, &lowered.body)
    }

    /// A pass with one of its regions evaluated.
    fn pass_region_done<'r>(
        &mut self,
        lowered: &'r crate::flow_slice_content::SliceLoop,
        element: Option<(
            &'r crate::flow_slice_content::SliceLoopElement,
            SemanticNodeId,
        )>,
        mut pass: PassEval,
        (result, falls): (Result<Vec<FlowContribution>, FlowReturnFailure>, bool),
    ) -> PassStep<'r> {
        use crate::flow_slice_content::SliceLoopTest;
        match pass.phase {
            PassPhase::TestBefore => {
                if let Err(failure) = result {
                    return PassStep::Done(Err(failure));
                }
                let SliceLoopTest::Before { guard, constant } = &lowered.test else {
                    unreachable!("the pass tested before its body")
                };
                pass.tested = Some(self.layer_state());
                // A literal `false` test never enters the body: no path
                // reaches it, and it contributes as unreachable code does.
                if *constant == Some(false) {
                    self.restore_narrowings(pass.narrow_mark);
                    return PassStep::Done(self.eval_unreachable_region(&lowered.body).map(
                        |contributors| LoopPass {
                            contributors,
                            back: None,
                            test_edge: pass.tested,
                            break_base: pass.break_base,
                        },
                    ));
                }
                self.apply_guard_scoped(guard, true);
                self.enter_loop_body(lowered, element, pass)
            }
            PassPhase::Body => {
                let contributors = match result {
                    Ok(contributors) => contributors,
                    Err(failure) => return PassStep::Done(Err(failure)),
                };
                pass.contributors = contributors;
                let shadows = self.split_scope_shadows_close_exits(
                    pass.shadow_base,
                    pass.break_base,
                    pass.return_base,
                    pass.throw_base,
                );
                let mut back_inputs = self.drain_continue_exits(pass.break_base, &lowered.labels);
                if falls {
                    let mut end = self.layer_state();
                    Self::close_lexical_scope(&mut end, &shadows);
                    back_inputs.push(end);
                }
                if back_inputs.is_empty() {
                    return self.finish_loop_pass(lowered, pass, None);
                }
                let joined = self.join_continuations(
                    &back_inputs,
                    &pass.head.products,
                    &pass.head.write_observation,
                );
                self.restore_layer_state(joined);
                pass.phase = PassPhase::Update;
                PassStep::Enter(pass, &lowered.update)
            }
            PassPhase::Update => {
                if let Err(failure) = result {
                    return PassStep::Done(Err(failure));
                }
                let back = Some(self.layer_state());
                self.finish_loop_pass(lowered, pass, back)
            }
            PassPhase::TestAfter => {
                if let Err(failure) = result {
                    return PassStep::Done(Err(failure));
                }
                let SliceLoopTest::After { guard, constant } = &lowered.test else {
                    unreachable!("the pass tests after its body")
                };
                let back = self.layer_state();
                let reentry =
                    (*constant != Some(false)).then(|| self.guarded_state(&back, guard, true));
                PassStep::Done(Ok(LoopPass {
                    contributors: pass.contributors,
                    back: reentry,
                    test_edge: Some(back),
                    break_base: pass.break_base,
                }))
            }
        }
    }

    /// A pass whose body and update ran, with its back edge `back`: a
    /// `do…while` tests after the body — the back edge re-enters only
    /// under the positive reading, and the loop exits from the same state
    /// under the negated one.
    fn finish_loop_pass<'r>(
        &mut self,
        lowered: &'r crate::flow_slice_content::SliceLoop,
        mut pass: PassEval,
        back: Option<FlowLayerState>,
    ) -> PassStep<'r> {
        use crate::flow_slice_content::SliceLoopTest;
        self.restore_narrowings(pass.narrow_mark.clone());
        match (&lowered.test, back) {
            (SliceLoopTest::After { .. }, Some(back)) => {
                self.restore_layer_state(back);
                if lowered.test_throws {
                    self.capture_throw_point();
                }
                pass.phase = PassPhase::TestAfter;
                PassStep::Enter(pass, &lowered.test_effects)
            }
            (_, back) => PassStep::Done(Ok(LoopPass {
                contributors: pass.contributors,
                back,
                test_edge: pass.tested,
                break_base: pass.break_base,
            })),
        }
    }

    /// A loop whose converged pass ran.
    fn finish_loop<'r>(&mut self, mut eval: Box<LoopEval<'r>>, pass: LoopPass) -> BranchStep<'r> {
        use crate::flow_slice_content::SliceLoopTest;
        let head = eval.converged.take().expect("the converged head");
        let entry = eval.entry.take().expect("the loop's entry state");
        let lowered = eval.lowered;
        let mut contributors = std::mem::take(&mut eval.contributors);
        contributors.extend(pass.contributors);
        // The exits: the test's (or the exhausted iteration's) edge, and
        // every `break` of the converged pass that targets this loop.
        let mut exits: Vec<FlowLayerState> = Vec::new();
        match &lowered.test {
            SliceLoopTest::Before { guard, constant } => {
                if *constant != Some(true) {
                    let tested = pass.test_edge.as_ref().unwrap_or(&head);
                    exits.push(self.guarded_state(tested, guard, false));
                }
            }
            SliceLoopTest::After { guard, constant } => {
                if let (Some(back), false) = (&pass.test_edge, *constant == Some(true)) {
                    exits.push(self.guarded_state(back, guard, false));
                }
            }
            SliceLoopTest::Never => {}
            SliceLoopTest::Exhausted => exits.push(head.clone()),
        }
        exits.extend(self.drain_break_exits(pass.break_base, None));
        // The loop's own scope (a `for` initializer's declarations)
        // closes over every state that leaves it.
        let shadows = self.close_scope_since(eval.bases);
        for exit in &mut exits {
            Self::close_lexical_scope(exit, &shadows);
        }
        if exits.is_empty() {
            self.restore_layer_state(entry);
            return self.end_loop(eval, Ok((contributors, false)));
        }
        // The exits leave from the converged head (or its back edge): a
        // binding the head already holds conditionally stays conditional.
        let joined = self.join_continuations(&exits, &head.products, &entry.write_observation);
        let written = joined.products.writes_since(&entry.write_observation);
        self.restore_layer_state(joined);
        for subject in written {
            if let Some(root) = self.narrow_root_of(&subject) {
                self.narrowing_writes
                    .push(NarrowingLedgerEntry::Cleared { root });
            }
        }
        self.end_loop(eval, Ok((contributors, true)))
    }

    /// End a loop statement's evaluation with `outcome`: its circular
    /// bindings leave the frame's circular set.
    fn end_loop<'r>(
        &mut self,
        eval: Box<LoopEval<'r>>,
        outcome: Result<(Vec<FlowContribution>, bool), FlowReturnFailure>,
    ) -> BranchStep<'r> {
        for binding in &eval.circular {
            self.circular_inferred.remove(binding);
        }
        BranchStep::Done(outcome)
    }
}
