//! Compound statements lowered as frames of their region's lowering.
//!
//! An `if`'s arms, a labeled statement's body, a `switch`'s clauses, a
//! `try`'s clauses and a loop's body are regions of their own. Lowering
//! each one inside the statement's own lowering took a native level per
//! nested statement. Instead the statement suspends its region's frame
//! ([`LowerEntered`]): [`Lowerer::lower_region`] lowers the entered region
//! as its own frame and the statement resumes with it
//! ([`Lowerer::resume_entered_lowering`]), entering its next region or
//! finishing into the region's statements.
use verter_session_query::flow::slice::SliceCatchClause;
use verter_session_query::flow::slice::SliceSwitchTest;

use super::*;

/// The statements of one arm: a block's own list, or the one statement.
pub(super) fn arm_statements<'s, 'x>(statement: &'s Statement<'x>) -> &'s [Statement<'x>] {
    match statement {
        Statement::BlockStatement(block) => &block.body,
        _ => std::slice::from_ref(statement),
    }
}

/// A statement suspended at a region it entered, with what it resumes
/// with once that region is lowered.
pub(super) enum LowerEntered<'s, 'x> {
    /// A block statement.
    Block,
    /// The statements after a statement no path passes, lowered as
    /// unreachable code.
    Unreachable,
    If(Box<IfLower<'s, 'x>>),
    Labeled(Box<LabeledLower>),
    Switch(Box<SwitchLower<'s, 'x>>),
    Try(Box<TryLower<'s, 'x>>),
    Loop(Box<LoopLower<'s, 'x>>),
}

/// What a compound statement's lowering needs next.
pub(super) enum LowerStep<'s, 'x> {
    /// This region lowered, the statement resuming as `LowerEntered`.
    Enter(LowerEntered<'s, 'x>, &'s [Statement<'x>]),
    /// Nothing: the statement is lowered into the region.
    Done,
}

/// The region a statement lowers into: its statements so far and its path
/// facts.
pub(super) struct LowerAcc<'r> {
    pub(super) out: &'r mut Vec<SliceStatement>,
    pub(super) can_fall_through: &'r mut bool,
    pub(super) hit_unsupported: &'r mut bool,
    pub(super) may_break: &'r mut Vec<SliceBreakTarget>,
}

/// An `if` between its arms.
pub(super) struct IfLower<'s, 'x> {
    test: &'s Expression<'x>,
    alternate: Option<&'s Statement<'x>>,
    guard: SliceGuard,
    guard_bindings: Vec<SkeletonBindingId>,
    active_guard_base: usize,
    unprovable_guard: bool,
    unprovable_control_call: bool,
    consequent: Option<LoweredRegion>,
}

/// A labeled statement whose body is being lowered.
pub(super) struct LabeledLower {
    label: Arc<str>,
    direct_wrap: bool,
    pending_base: usize,
}

/// A `switch` between its clauses.
pub(super) struct SwitchLower<'s, 'x> {
    switch: &'s oxc_ast::ast::SwitchStatement<'x>,
    unprovable_switch_effect: bool,
    has_default: bool,
    discriminant: Option<SliceNarrowSubject>,
    active_guard_base: usize,
    cases: Vec<SliceSwitchCase>,
    /// The test of the clause being lowered.
    test: Option<SliceSwitchTest>,
}

/// A `try` statement between its clauses.
pub(super) struct TryLower<'s, 'x> {
    try_stmt: &'s oxc_ast::ast::TryStatement<'x>,
    block: Option<SliceRegion>,
    clause_may_break: Vec<SliceBreakTarget>,
    catch: Option<Box<SliceCatchClause>>,
    /// The catch parameter's name, read before its clause lowers.
    param: Option<Arc<str>>,
    phase: TryLowerPhase,
}

#[derive(Clone, Copy)]
enum TryLowerPhase {
    Block,
    Catch,
    Finally,
}

/// A loop whose body is being lowered: its lowering up to the body.
pub(super) struct LoopLower<'s, 'x> {
    pub(super) statement: &'s Statement<'x>,
    pub(super) update: Option<&'s Expression<'x>>,
    pub(super) test_gap: bool,
    pub(super) init: Vec<SliceStatement>,
    pub(super) test: SliceLoopTest,
    pub(super) test_effects: Vec<SliceStatement>,
    pub(super) element: Option<SliceLoopElement>,
    pub(super) labels: Arc<[Arc<str>]>,
    pub(super) test_throws: bool,
    pub(super) completes: bool,
    pub(super) active_guard_base: usize,
    pub(super) enclosing_direct_labels: Vec<Arc<str>>,
}

impl<'a> Lowerer<'a> {
    /// Resume the statement `entered` with the lowering of the region it
    /// entered.
    pub(super) fn resume_entered_lowering<'s, 'x>(
        &mut self,
        entered: LowerEntered<'s, 'x>,
        lowered: LoweredRegion,
        acc: LowerAcc<'_>,
    ) -> LowerStep<'s, 'x> {
        match entered {
            LowerEntered::Block | LowerEntered::Unreachable => {
                unreachable!("a block resumes in its region's walk")
            }
            LowerEntered::If(state) => self.if_arm_lowered(state, lowered, acc),
            LowerEntered::Labeled(state) => self.labeled_body_lowered(*state, lowered, acc),
            LowerEntered::Switch(state) => self.switch_clause_lowered(state, lowered, acc),
            LowerEntered::Try(state) => self.try_clause_lowered(state, lowered, acc),
            LowerEntered::Loop(state) => {
                let lowered = self.finish_lower_loop(state, lowered);
                *acc.hit_unsupported = lowered.hit_unsupported;
                acc.may_break.extend(lowered.may_break);
                if !lowered.completes || *acc.hit_unsupported {
                    *acc.can_fall_through = false;
                }
                acc.out
                    .push(SliceStatement::Loop(Box::new(lowered.lowered)));
                LowerStep::Done
            }
        }
    }

    /// Begin lowering an `if` whose test holds no write.
    pub(super) fn begin_if_lowering<'s, 'x>(
        &mut self,
        if_stmt: &'s oxc_ast::ast::IfStatement<'x>,
        acc: LowerAcc<'_>,
    ) -> LowerStep<'s, 'x> {
        // The evaluator never consumes the test's VALUE, so no
        // test content lowers — but its narrowing facts do,
        // through the ONE guard authority both control
        // spellings share.
        let guard = self.lower_guard(&if_stmt.test);
        // A guard form the lowering REFUSED (an unprovable
        // `instanceof` constructor) flagged the gap while the
        // test lowered. Take it here, ahead of the arms: an arm
        // region's own statement loop would otherwise drain it
        // INTO the arm.
        let unprovable_guard = std::mem::take(&mut self.control_test_gap);
        // A call in the TEST is decided above ONLY when its
        // result provably cannot control the arms' narrowing;
        // a predicate call takes evaluator evidence at guard
        // application, and an unprovable callee degrades the
        // demand through the typed guard-narrowing gap below.
        // An entered `asserts` call in the test narrows once
        // the test has run, ahead of both arms.
        let mut unprovable_control_call = false;
        let test_assertions = self.collecting_entered_assertions(|this| {
            unprovable_control_call = this.record_control_position_calls(&if_stmt.test);
        });
        acc.out.extend(test_assertions);
        let active_guard_base = self.active_guard_bindings.len();
        let guard_bindings = self.guard_bindings(&guard, if_stmt.test.span());
        self.active_guard_bindings
            .extend(guard_bindings.iter().copied());
        LowerStep::Enter(
            LowerEntered::If(Box::new(IfLower {
                test: &if_stmt.test,
                alternate: if_stmt.alternate.as_ref(),
                guard,
                guard_bindings,
                active_guard_base,
                unprovable_guard,
                unprovable_control_call,
                consequent: None,
            })),
            arm_statements(&if_stmt.consequent),
        )
    }

    fn if_arm_lowered<'s, 'x>(
        &mut self,
        mut state: Box<IfLower<'s, 'x>>,
        lowered: LoweredRegion,
        acc: LowerAcc<'_>,
    ) -> LowerStep<'s, 'x> {
        self.active_guard_bindings.truncate(state.active_guard_base);
        let (consequent, alternate) = match state.consequent.take() {
            None => {
                if let Some(alternate) = state.alternate {
                    self.active_guard_bindings
                        .extend(state.guard_bindings.iter().copied());
                    state.consequent = Some(lowered);
                    return LowerStep::Enter(LowerEntered::If(state), arm_statements(alternate));
                }
                (lowered, None)
            }
            Some(consequent) => (consequent, Some(lowered)),
        };
        let IfLower {
            test,
            guard,
            unprovable_guard,
            unprovable_control_call,
            ..
        } = *state;
        *acc.can_fall_through = consequent
            .region
            .can_fall_through
            .reaches_end(CompletionDischarge::RegionComposition)
            || alternate
                .as_ref()
                .map(|region| {
                    region
                        .region
                        .can_fall_through
                        .reaches_end(CompletionDischarge::RegionComposition)
                })
                .unwrap_or(true);
        *acc.hit_unsupported = consequent.hit_unsupported
            || alternate
                .as_ref()
                .is_some_and(|region| region.hit_unsupported);
        // An `if` absorbs no `break` either: a conditional exit
        // (`if (f) break;`) is still an exit of the region.
        acc.may_break.extend(consequent.may_break);
        // A call in the TEST is a throw point BEFORE either
        // arm — the test lowers to guard facts only, so the
        // marker carries the point (ahead of the `if`, where
        // the test evaluates).
        if unprovable_control_call || unprovable_guard {
            acc.out.push(SliceStatement::Gap(
                verter_session_query::flow::policy::FlowGap::GuardNarrowing,
            ));
        }
        if verter_semantic::analysis::flow::expression_contains_call(test) {
            acc.out.push(SliceStatement::ThrowPoint);
        }
        self.lower_test_updates(test, acc.out);
        if let Some(alternate) = alternate {
            acc.may_break.extend(alternate.may_break);
            acc.out.push(SliceStatement::If {
                guard,
                consequent: Box::new(consequent.region),
                alternate: Some(Box::new(alternate.region)),
            });
        } else {
            acc.out.push(SliceStatement::If {
                guard,
                consequent: Box::new(consequent.region),
                alternate: None,
            });
        }
        LowerStep::Done
    }

    /// Begin lowering a labeled statement.
    pub(super) fn begin_labeled_lowering<'s, 'x>(
        &mut self,
        labeled: &'s oxc_ast::ast::LabeledStatement<'x>,
    ) -> LowerStep<'s, 'x> {
        // The label is a break target for its OWN body in both
        // paths: a `break` naming it exits to after the
        // statement, which the absorption below folds into the
        // statement's reachability.
        let label: Arc<str> = Arc::from(labeled.label.name.as_str());
        self.break_targets.push(Some(Arc::clone(&label)));
        self.break_target_followed_by_return
            .push(self.current_statement_followed_by_return);
        // A label chain directly wrapping a loop names the
        // loop's own exit/iteration edge: record it so the
        // loop's transparency classification treats a jump to
        // it as local rather than an escaping transfer.
        let direct_wrap = label_directly_wraps_loop(&labeled.body);
        let pending_base = self.pending_loop_labels.len();
        if direct_wrap {
            self.loop_direct_labels.push(Arc::clone(&label));
            self.pending_loop_labels.push(Arc::clone(&label));
        }
        LowerStep::Enter(
            LowerEntered::Labeled(Box::new(LabeledLower {
                label,
                direct_wrap,
                pending_base,
            })),
            arm_statements(&labeled.body),
        )
    }

    fn labeled_body_lowered<'s, 'x>(
        &mut self,
        state: LabeledLower,
        child: LoweredRegion,
        acc: LowerAcc<'_>,
    ) -> LowerStep<'s, 'x> {
        let LabeledLower {
            label,
            direct_wrap,
            pending_base,
        } = state;
        self.pending_loop_labels.truncate(pending_base);
        if direct_wrap {
            self.loop_direct_labels.pop();
        }
        self.break_targets.pop();
        self.break_target_followed_by_return.pop();
        let mut absorbed = false;
        for target in child.may_break {
            match target {
                SliceBreakTarget::Named(name) if name == label => absorbed = true,
                other => acc.may_break.push(other),
            }
        }
        // The body lowers identically whether or not it bears a
        // return: the label wraps an ordinary statement whose
        // own rail decides (a block's hoisted `var`s, a loop's
        // escaping `var` fail-close, an `if` arm's conditional
        // binding, `switch` / `try` / `with` unsupported), and
        // the EVALUATOR needs the label's name either way —
        // the absorbed `break` is what lets execution reach
        // past the statement even when the body itself cannot,
        // and its captured state is that edge's layer state.
        *acc.can_fall_through = child
            .region
            .can_fall_through
            .reaches_end(CompletionDischarge::RegionComposition)
            || absorbed;
        *acc.hit_unsupported = child.hit_unsupported;
        acc.out.push(SliceStatement::Labeled {
            label,
            body: Box::new(child.region),
        });
        LowerStep::Done
    }

    /// Begin lowering a `switch`.
    ///
    /// Each case clause lowers as its own region with the switch on the
    /// break-target stack: a `break` ends the case's path and is absorbed
    /// into the clause's `breaks` flag; a `break` naming an OUTER labeled
    /// statement propagates through the switch untouched. The discriminant
    /// lowers no value content; when it is a narrowable reference it rides
    /// the statement so the evaluator can narrow it per dispatch edge, and
    /// each literal case test rides its clause for the same purpose (a
    /// non-literal test narrows nothing).
    ///
    /// Both positions still EXECUTE, so their effects take the fail-closed
    /// scan: the discriminant's value feeds the dispatch but never the
    /// demanded answer (the discarded-operand discipline — only an
    /// `asserts` callee narrows what follows), and each case TEST is a
    /// control position exactly as an `if` test is — `switch (true) { case
    /// isString(x): … }` narrows `x` inside the clause in the checker.
    ///
    /// The scan alone is NOT completeness evidence, because a case relation
    /// narrows with no call and no write at all: `switch (true) { case
    /// typeof x === "string": }` narrows `x` inside the clause, and `case
    /// K:` against a literal-typed `const` narrows the discriminant. The
    /// MODELED dispatches are a represented discriminant against a LITERAL
    /// case relation, a `switch (typeof x)` string case, and a `switch
    /// (true)` case whose condition is a guard; every other clause with a
    /// test mints the typed gap.
    ///
    /// The gap belongs AHEAD of the switch: a case test evaluates whether
    /// or not its clause is entered, so the flag must not drain into a
    /// clause region's own statement loop.
    pub(super) fn begin_switch_lowering<'s, 'x>(
        &mut self,
        switch: &'s oxc_ast::ast::SwitchStatement<'x>,
        acc: LowerAcc<'_>,
    ) -> LowerStep<'s, 'x> {
        let unprovable_switch_effect = self.record_discarded_operand_calls(&switch.discriminant);
        let has_default = switch.cases.iter().any(|case| case.test.is_none());
        let discriminant = self.narrow_subject_of(&switch.discriminant);
        self.break_targets.push(None);
        self.break_target_followed_by_return
            .push(SuffixReturn::NotGuaranteed);
        // A clause body evaluates under the dispatch narrow
        // of the discriminant, so a closure created there
        // takes the closure-capture rail the `if` arms and
        // the ternary's arms take.
        let active_guard_base = self.active_guard_bindings.len();
        if let Some(subject) = discriminant.as_ref() {
            let bindings = self.subject_bindings(subject, switch.discriminant.span());
            self.active_guard_bindings.extend(bindings);
        }
        self.switch_next_clause(
            Box::new(SwitchLower {
                switch,
                unprovable_switch_effect,
                has_default,
                discriminant,
                active_guard_base,
                cases: Vec::with_capacity(switch.cases.len()),
                test: None,
            }),
            acc,
        )
    }

    /// Enter the next clause of a `switch`, or finish it.
    fn switch_next_clause<'s, 'x>(
        &mut self,
        mut state: Box<SwitchLower<'s, 'x>>,
        acc: LowerAcc<'_>,
    ) -> LowerStep<'s, 'x> {
        let switch = state.switch;
        let Some(case) = switch.cases.get(state.cases.len()) else {
            return self.finish_switch_lowering(*state, acc);
        };
        if let Some(test) = case.test.as_ref() {
            state.unprovable_switch_effect |= self.record_control_position_calls(test);
        }
        // The MODELED dispatch is exactly one pair: a
        // represented discriminant against a LITERAL case
        // relation. Anything else — a `typeof` or
        // equality relation under a non-reference
        // discriminant, a case test naming a constant, a
        // template case, a discriminant this half cannot
        // represent — establishes a clause narrow the
        // checker applies and this lowering carries
        // nothing for, so the switch degrades. The
        // unrecognized clause keeps its OWN carrier: it
        // must never be dispatched as the default edge.
        let test = match case.test.as_ref() {
            None => SliceSwitchTest::Default,
            // `switch (typeof x)`: each string case is the
            // `typeof x === "…"` guard.
            Some(test)
                if state.discriminant.is_none()
                    && self
                        .typeof_guard(&switch.discriminant, test, false)
                        .is_some() =>
            {
                SliceSwitchTest::Guard(Box::new(
                    self.typeof_guard(&switch.discriminant, test, false)
                        .expect("the guard was just lowered"),
                ))
            }
            // `switch (true)`: each case is its condition.
            Some(test)
                if matches!(
                    unwrap_parenthesized(&switch.discriminant),
                    Expression::BooleanLiteral(literal) if literal.value
                ) =>
            {
                match self.lower_guard(test) {
                    SliceGuard::None => {
                        state.unprovable_switch_effect = true;
                        SliceSwitchTest::Unmodeled
                    }
                    guard => SliceSwitchTest::Guard(Box::new(guard)),
                }
            }
            Some(test) => {
                match guard_literal_of(test, self.source)
                    .or_else(|| self.guard_value_path_of(test))
                    .filter(|_| state.discriminant.is_some())
                {
                    Some(literal) => SliceSwitchTest::Literal(literal),
                    None => {
                        state.unprovable_switch_effect = true;
                        SliceSwitchTest::Unmodeled
                    }
                }
            }
        };
        state.test = Some(test);
        LowerStep::Enter(LowerEntered::Switch(state), &case.consequent)
    }

    fn switch_clause_lowered<'s, 'x>(
        &mut self,
        mut state: Box<SwitchLower<'s, 'x>>,
        lowered: LoweredRegion,
        acc: LowerAcc<'_>,
    ) -> LowerStep<'s, 'x> {
        *acc.hit_unsupported |= lowered.hit_unsupported;
        let mut breaks = false;
        for target in lowered.may_break {
            match target {
                SliceBreakTarget::Anonymous => breaks = true,
                named => acc.may_break.push(named),
            }
        }
        state.cases.push(SliceSwitchCase {
            region: lowered.region,
            breaks: NormalCompletion::minted(breaks, CompletionConstruction::SwitchCaseBreak),
            test: state.test.take().expect("the clause's test"),
        });
        self.switch_next_clause(state, acc)
    }

    fn finish_switch_lowering<'s, 'x>(
        &mut self,
        state: SwitchLower<'s, 'x>,
        acc: LowerAcc<'_>,
    ) -> LowerStep<'s, 'x> {
        let SwitchLower {
            unprovable_switch_effect,
            has_default,
            discriminant,
            active_guard_base,
            cases,
            ..
        } = state;
        self.active_guard_bindings.truncate(active_guard_base);
        self.break_targets.pop();
        self.break_target_followed_by_return.pop();
        // Past the switch is reachable when no `default`
        // exists (a non-matching discriminant skips every
        // case), when the LAST clause falls off the end of the
        // switch, or when any clause exits via `break`.
        *acc.can_fall_through = !has_default
            || cases.last().is_some_and(|case| {
                case.region
                    .can_fall_through
                    .reaches_end(CompletionDischarge::RegionComposition)
            })
            || cases.iter().any(|case| {
                case.breaks
                    .reaches_end(CompletionDischarge::RegionComposition)
            });
        if unprovable_switch_effect {
            acc.out.push(SliceStatement::Gap(
                verter_session_query::flow::policy::FlowGap::GuardNarrowing,
            ));
        }
        acc.out.push(SliceStatement::Switch {
            discriminant,
            cases: Arc::from(cases.into_boxed_slice()),
            has_default,
        });
        LowerStep::Done
    }

    /// Begin lowering a `try` statement.
    pub(super) fn begin_try_lowering<'s, 'x>(
        &mut self,
        try_stmt: &'s oxc_ast::ast::TryStatement<'x>,
    ) -> LowerStep<'s, 'x> {
        LowerStep::Enter(
            LowerEntered::Try(Box::new(TryLower {
                try_stmt,
                block: None,
                clause_may_break: Vec::new(),
                catch: None,
                param: None,
                phase: TryLowerPhase::Block,
            })),
            &try_stmt.block.body,
        )
    }

    fn try_clause_lowered<'s, 'x>(
        &mut self,
        mut state: Box<TryLower<'s, 'x>>,
        region: LoweredRegion,
        acc: LowerAcc<'_>,
    ) -> LowerStep<'s, 'x> {
        let try_stmt = state.try_stmt;
        let finally = match state.phase {
            TryLowerPhase::Block => {
                *acc.hit_unsupported |= region.hit_unsupported;
                state.clause_may_break = region.may_break;
                state.block = Some(region.region);
                if let Some(handler) = try_stmt.handler.as_ref() {
                    state.param = handler
                        .param
                        .as_ref()
                        .and_then(|param| match &param.pattern {
                            BindingPattern::BindingIdentifier(id) => {
                                Some(Arc::from(id.name.as_str()))
                            }
                            // A destructured catch parameter binds
                            // nothing this frame can name.
                            _ => None,
                        });
                    state.phase = TryLowerPhase::Catch;
                    return LowerStep::Enter(LowerEntered::Try(state), &handler.body.body);
                }
                None
            }
            TryLowerPhase::Catch => {
                let handler = try_stmt
                    .handler
                    .as_ref()
                    .expect("a catch clause is lowered");
                *acc.hit_unsupported |= region.hit_unsupported;
                state.clause_may_break.extend(region.may_break);
                let declared = handler.param.as_ref().and_then(|param| {
                    param.type_annotation.as_ref().map(|annotation| {
                        self.gate(
                            lower_ts_type(&annotation.type_annotation, self.source),
                            param.pattern.span(),
                            &[],
                        )
                    })
                });
                state.catch = Some(Box::new(SliceCatchClause {
                    declared,
                    binding: handler
                        .param
                        .as_ref()
                        .and_then(|param| match &param.pattern {
                            BindingPattern::BindingIdentifier(id) => {
                                self.bindings.declaration_at_span(self.rebase(id.span))
                            }
                            _ => None,
                        }),
                    param: state.param.take(),
                    region: region.region,
                }));
                None
            }
            TryLowerPhase::Finally => {
                *acc.hit_unsupported |= region.hit_unsupported;
                Some((Box::new(region.region), region.may_break))
            }
        };
        if finally.is_none() && !matches!(state.phase, TryLowerPhase::Finally) {
            if let Some(finalizer) = try_stmt.finalizer.as_ref() {
                state.phase = TryLowerPhase::Finally;
                return LowerStep::Enter(LowerEntered::Try(state), &finalizer.body);
            }
        }
        self.finish_try_lowering(*state, finally, acc)
    }

    /// A `try` statement with every clause lowered.
    fn finish_try_lowering<'s, 'x>(
        &mut self,
        state: TryLower<'s, 'x>,
        finally: Option<(Box<SliceRegion>, Vec<SliceBreakTarget>)>,
        acc: LowerAcc<'_>,
    ) -> LowerStep<'s, 'x> {
        let TryLower {
            block,
            clause_may_break,
            catch,
            ..
        } = state;
        let block = block.expect("the try block is lowered");
        // A `finally` that CANNOT fall through completes
        // abruptly on every path, and abrupt completion
        // discards the try/catch's pending exits — pending
        // returns AND pending `break`s alike. A finally that
        // CAN fall through overrides nothing on that path: a
        // pending break proceeds past the try when the finally
        // does not return, so the try/catch clauses' break
        // exits propagate whenever the finally has a
        // fall-through path (or does not exist). The finally
        // clause's OWN break exits always propagate: they fire
        // after every override decision, they are never
        // pending.
        let finally_blocks_exits = finally.as_ref().is_some_and(|(region, _)| {
            !region
                .can_fall_through
                .reaches_end(CompletionDischarge::RegionComposition)
        });
        // A named break crossing this try for any enclosing
        // label remains an authored return-inference path even
        // when blocks or inner labels wrap the try. An abrupt
        // finally replaces the runtime edge, but not that
        // implicit-`undefined` inference contribution.
        let target_followed_by_return = |name: &Arc<str>| {
            self.break_targets
                .iter()
                .zip(self.break_target_followed_by_return.iter())
                .rev()
                .find(|(entry, _)| entry.as_ref() == Some(name))
                .map(|(_, followed_by_return)| *followed_by_return)
        };
        // An anonymous break's destination is the innermost
        // anonymous breakable's continuation.
        let anonymous_followed_by_return = || {
            self.break_targets
                .iter()
                .zip(self.break_target_followed_by_return.iter())
                .rev()
                .find(|(entry, _)| entry.is_none())
                .map(|(_, followed_by_return)| *followed_by_return)
        };
        // The destination decides the contribution, and an
        // UNDECIDED destination decides nothing: the value keeps
        // the derivation it always had, and the gap below makes
        // the result return without ever being admitted.
        let pending_break_destination = |state: SuffixReturn| {
            finally_blocks_exits
                && clause_may_break.iter().any(|target| match target {
                    SliceBreakTarget::Named(name) => target_followed_by_return(name) == Some(state),
                    SliceBreakTarget::Anonymous => anonymous_followed_by_return() == Some(state),
                })
        };
        let pending_break_destination_undecided =
            pending_break_destination(SuffixReturn::Undecided);
        let pending_break_contributes_undefined =
            pending_break_destination(SuffixReturn::NotGuaranteed)
                || pending_break_destination_undecided;
        let mut pending_break_following_return_targets: Vec<Arc<str>> = Vec::new();
        if finally_blocks_exits {
            for target in &clause_may_break {
                let SliceBreakTarget::Named(name) = target else {
                    continue;
                };
                if target_followed_by_return(name) == Some(SuffixReturn::Guaranteed)
                    && !pending_break_following_return_targets.contains(name)
                {
                    pending_break_following_return_targets.push(Arc::clone(name));
                }
            }
        }
        if !finally_blocks_exits {
            acc.may_break.extend(clause_may_break);
        } else {
            // When the crossed target is followed by a guaranteed
            // return, inference keeps that suffix return instead
            // of the implicit-undefined contribution. Propagate
            // the named exit until its label absorbs it; the
            // qualifier is inherited through intervening labels
            // and blocks by `lower_region`.
            acc.may_break.extend(
                pending_break_following_return_targets
                    .iter()
                    .cloned()
                    .map(SliceBreakTarget::Named),
            );
        }
        if let Some((_, finally_may_break)) = &finally {
            acc.may_break.extend(finally_may_break.iter().cloned());
        }
        let pre_finally_fall_through = block
            .can_fall_through
            .reaches_end(CompletionDischarge::RegionComposition)
            || catch.as_ref().is_some_and(|catch| {
                catch
                    .region
                    .can_fall_through
                    .reaches_end(CompletionDischarge::RegionComposition)
            });
        *acc.can_fall_through = pre_finally_fall_through
            && finally.as_ref().is_none_or(|(region, _)| {
                region
                    .can_fall_through
                    .reaches_end(CompletionDischarge::RegionComposition)
            });
        if pending_break_destination_undecided {
            // A pending break whose destination this lowering
            // cannot classify. The contribution above is a
            // derivation, not a proof, so the slice carries the
            // typed gap ahead of the try: the evaluation returns
            // the value and refuses to warm it.
            acc.out.push(SliceStatement::Gap(
                verter_session_query::flow::policy::FlowGap::AbruptCompletion,
            ));
        }
        acc.out.push(SliceStatement::Try {
            block: Box::new(block),
            catch,
            finally: finally.map(|(region, _)| region),
            pending_break_contributes_undefined,
            pending_break_following_return_targets: Arc::from(
                pending_break_following_return_targets.into_boxed_slice(),
            ),
        });
        LowerStep::Done
    }
}
