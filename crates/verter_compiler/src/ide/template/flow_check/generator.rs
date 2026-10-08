//! Lowers a [`CheckPlan`] into one flow-transparent TypeScript check.
//!
//! Layout (all checking happens in ONE function body, so TypeScript's own
//! control flow carries every narrowing):
//!
//! ```text
//! <helpers> <declarations>
//! export function __verter_flow_check<parameters> {
//!   if (A) { … } else if (B) { … } else { … }      // each condition once
//!   { const item = __VerterFlow.each1(list); … }    // v-for / v-slot: a block
//!   __VerterFlow.check<Contract>((() => {           // one callback
//!     const __verter_o0 = <outer ref>;              //   outer-flow snapshot
//!     return (e) => { if (!narrow<…o0>(ref) || excluded<…o0>()(ref)) throw 0; body };
//!   })());
//! }
//! ```
//!
//! Chains, nested chains, `v-for` and `v-slot` frames are blocks inside the
//! one function, so a nested branch inherits every enclosing positive and
//! predecessor negation through flow alone: no condition is ever re-emitted.
//!
//! A callback is a function, and TypeScript starts a function's flow from the
//! declared type of every property-access reference. The callback is therefore
//! wrapped in an immediately invoked arrow — flow-transparent, and it passes
//! its call's contextual type to its return expression, so the authored
//! callback keeps its exact contextual parameter and return typing. The
//! wrapper snapshots each outer reference the body reads; inside the body two
//! predicates restore exactly that type: `narrow` re-narrows the reference to
//! the snapshot, and `excluded` removes any constituent `narrow` re-admitted
//! only because it is a subtype of a kept one. The cost is bounded by the
//! body's own references, never by the path that narrowed them.
//!
//! Every byte is produced by `CodeTransform`: authored spans are moved, in
//! output order, to the end anchor; synthetic text is appended there; every
//! authored byte the check does not use is removed. Synthetic text carries no
//! source mapping.

use super::deps::{Allocator, CodeTransform};
use super::seam::{
    BranchKind, Callback, CallbackFunction, CallbackSite, Chain, CheckPlan, ExprItem, Frame,
    FrameBinding, GeneratedCheck, GenerationWork, GuardSite, Item, Layout, Mapping, Piece,
    PlanError, ResolvedExpr, ScopeKey, Span,
};

/// Name of the generated check function.
pub const CHECK_FUNCTION: &str = "__verter_flow_check";

/// Helper declarations every generated check starts with. Fixed size.
pub const HELPERS: &str = "declare namespace __VerterFlow {
  type Same<A, B> = (<G>() => G extends A ? 1 : 2) extends (<G>() => G extends B ? 1 : 2) ? true : false;
  type Kept<R, S> = S extends unknown ? (Same<R, S> extends true ? true : never) : never;
  type Excluded<R, S> = R extends unknown ? ([Kept<R, S>] extends [never] ? R : never) : never;
  type Each<S> = S extends number ? [number, number, number]
    : S extends string ? [string, number, number]
    : S extends readonly (infer V)[] ? [V, number, number]
    : S extends Iterable<infer V> ? [V, number, number]
    : S extends object ? [S[keyof S], keyof S, number]
    : [never, never, never];
  function narrow<S>(reference: unknown): reference is S;
  function excluded<S>(): <R>(reference: R) => reference is Excluded<R, S>;
  function check<C>(value: C): void;
  function each1<S>(source: S): Each<S>[0];
  function each2<S>(source: S): [Each<S>[0], Each<S>[1]];
  function each3<S>(source: S): [Each<S>[0], Each<S>[1], number];
  function slot<C extends (...args: any) => any>(): Parameters<C>[0];
  const unreachable: never;
}
";

/// How a callback recovers the narrowing of its position.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GuardStrategy {
    /// Outer-flow snapshots of the references the body reads (linear).
    Snapshot,
    /// Re-emit every enclosing positive and predecessor negation inside each
    /// callback, as the current emitter does. Quadratic: kept only as the
    /// equivalence and growth oracle for [`GuardStrategy::Snapshot`].
    ReplayPath,
}

/// Generate the check for `plan` over the authored `source`.
///
/// Refuses a plan whose structure, scope links, spans or callback inputs do
/// not agree (see [`PlanError`]).
pub fn generate(plan: &CheckPlan, source: &str) -> Result<GeneratedCheck, PlanError> {
    generate_with(plan, source, GuardStrategy::Snapshot)
}

/// [`generate`] with an explicit callback guard strategy.
pub fn generate_with(
    plan: &CheckPlan,
    source: &str,
    strategy: GuardStrategy,
) -> Result<GeneratedCheck, PlanError> {
    let mut validation = Validation {
        source_len: source.len() as u32,
        authored: Vec::new(),
        work: GenerationWork::default(),
    };
    validation.items(&plan.items, plan.root)?;
    let mut authored = std::mem::take(&mut validation.authored);
    authored.sort_unstable();
    for pair in authored.windows(2) {
        if pair[0].end > pair[1].start {
            return Err(PlanError::AuthoredOverlap {
                first: pair[0],
                second: pair[1],
            });
        }
    }

    let allocator = Allocator::default();
    let mut emitter = Emitter {
        ct: CodeTransform::new(source, &allocator),
        source,
        strategy,
        path: Vec::new(),
        end: source.len() as u32,
        work: validation.work,
        authored_bytes: 0,
        callbacks: Vec::new(),
        output_len: 0,
    };
    emitter.remove_unused(&authored);

    emitter.synthetic(HELPERS);
    let declarations_start = emitter.output_len;
    emitter.synthetic(&plan.declarations);
    emitter.synthetic("\nexport function ");
    emitter.synthetic(CHECK_FUNCTION);
    emitter.synthetic(&plan.parameters);
    emitter.synthetic(" {\n");
    let mut declaration_bytes = emitter.output_len - declarations_start;
    emitter.items(&plan.items);
    let footer_start = emitter.output_len;
    emitter.synthetic("}\n");
    declaration_bytes += emitter.output_len - footer_start;

    let (code, ranges) = emitter.ct.build_string_with_source_ranges();
    let mut mappings: Vec<Mapping> = ranges
        .into_iter()
        .filter(|range| range.generated_end > range.generated_start)
        .map(|range| Mapping {
            generated: Span::new(range.generated_start, range.generated_end),
            source: Span::new(range.source_start, range.source_end),
        })
        .collect();
    mappings.sort_unstable_by_key(|m| m.generated.start);

    Ok(GeneratedCheck {
        layout: Layout {
            helper_bytes: HELPERS.len(),
            declaration_bytes,
            total_bytes: code.len(),
            authored_bytes: emitter.authored_bytes,
            callbacks: emitter.callbacks,
        },
        work: emitter.work,
        mappings,
        code,
    })
}

struct Validation {
    source_len: u32,
    authored: Vec<Span>,
    work: GenerationWork,
}

impl Validation {
    fn items(&mut self, items: &[Item], scope: ScopeKey) -> Result<(), PlanError> {
        for item in items {
            match item {
                Item::Chain(chain) => self.chain(chain, scope)?,
                Item::Frame(frame) => self.frame(frame, scope)?,
                Item::Callback(callback) => self.callback(callback, scope)?,
                Item::Expr(expr) => {
                    self.scope(expr.scope, scope)?;
                    if expr.contract.as_deref().is_some_and(str::is_empty) {
                        return Err(PlanError::MissingContract { at: expr.expr.span });
                    }
                    self.expr(&expr.expr)?;
                }
            }
        }
        Ok(())
    }

    fn scope(&mut self, found: ScopeKey, expected: ScopeKey) -> Result<(), PlanError> {
        self.work.scope_links += 1;
        if found == expected {
            Ok(())
        } else {
            Err(PlanError::ScopeMismatch { expected, found })
        }
    }

    fn chain(&mut self, chain: &Chain, scope: ScopeKey) -> Result<(), PlanError> {
        let Some(first) = chain.branches.first() else {
            return Err(PlanError::EmptyChain);
        };
        if first.kind != BranchKind::If {
            return Err(PlanError::ChainMustOpenWithIf { branch: first.key });
        }
        let last = chain.branches.len() - 1;
        let mut previous = None;
        for (index, branch) in chain.branches.iter().enumerate() {
            self.scope(branch.scope, scope)?;
            self.work.predecessor_links += 1;
            if branch.predecessor != previous || (index > 0 && branch.kind == BranchKind::If) {
                return Err(PlanError::PredecessorMismatch {
                    branch: branch.key,
                    expected: previous,
                    found: branch.predecessor,
                });
            }
            match (branch.kind, &branch.condition) {
                (BranchKind::Else, Some(_)) => {
                    return Err(PlanError::ElseWithCondition { branch: branch.key })
                }
                (BranchKind::Else, None) if index != last => {
                    return Err(PlanError::ElseNotLast { branch: branch.key })
                }
                (BranchKind::If | BranchKind::ElseIf, None) => {
                    return Err(PlanError::MissingCondition { branch: branch.key })
                }
                (_, Some(condition)) => self.expr(condition)?,
                (BranchKind::Else, None) => {}
            }
            self.items(&branch.items, branch.scope)?;
            previous = Some(branch.key);
        }
        Ok(())
    }

    fn frame(&mut self, frame: &Frame, scope: ScopeKey) -> Result<(), PlanError> {
        self.work.scope_links += 1;
        if frame.parent != scope {
            return Err(PlanError::FrameParentMismatch {
                frame: frame.scope,
                expected: scope,
                found: frame.parent,
            });
        }
        match &frame.binding {
            FrameBinding::VFor {
                aliases,
                arity,
                source,
            } => {
                if !(1..=3).contains(arity) {
                    return Err(PlanError::BadVForArity { arity: *arity });
                }
                self.authored(*aliases)?;
                self.expr(source)?;
            }
            FrameBinding::Slot { pattern, contract } => {
                if contract.is_empty() {
                    return Err(PlanError::MissingContract { at: *pattern });
                }
                self.authored(*pattern)?;
            }
        }
        self.items(&frame.items, frame.scope)
    }

    fn callback(&mut self, callback: &Callback, scope: ScopeKey) -> Result<(), PlanError> {
        self.scope(callback.scope, scope)?;
        let function = match &callback.function {
            CallbackFunction::Authored { expr, guard } => {
                self.expr(expr)?;
                let site = match *guard {
                    GuardSite::Block { after } => after,
                    GuardSite::Expression { before } => before,
                };
                if site < expr.span.start || site > expr.span.end {
                    return Err(PlanError::GuardSiteOutsideFunction {
                        function: expr.span,
                        site,
                    });
                }
                expr.span
            }
            CallbackFunction::Handler { body, .. } => {
                self.expr(body)?;
                body.span
            }
        };
        if callback.contract.is_empty() {
            return Err(PlanError::MissingContract { at: function });
        }
        for outer in &callback.outer_refs {
            if outer.text.is_empty() {
                return Err(PlanError::EmptyOuterRef {
                    occurrence: outer.occurrence,
                });
            }
            if !function.contains(outer.occurrence) {
                return Err(PlanError::OuterRefOutsideCallback {
                    function,
                    occurrence: outer.occurrence,
                });
            }
        }
        Ok(())
    }

    fn expr(&mut self, expr: &ResolvedExpr) -> Result<(), PlanError> {
        if expr.span.start > expr.span.end || expr.span.end > self.source_len {
            return Err(PlanError::SpanOutsideSource { span: expr.span });
        }
        let mut cursor = expr.span.start;
        for piece in &expr.pieces {
            if let Piece::Authored(span) = piece {
                if span.start != cursor || span.end < span.start || span.end > expr.span.end {
                    return Err(PlanError::PiecesDoNotCoverSpan { span: expr.span });
                }
                cursor = span.end;
            }
        }
        if cursor != expr.span.end {
            return Err(PlanError::PiecesDoNotCoverSpan { span: expr.span });
        }
        self.authored(expr.span)
    }

    fn authored(&mut self, span: Span) -> Result<(), PlanError> {
        if span.start > span.end || span.end > self.source_len {
            return Err(PlanError::SpanOutsideSource { span });
        }
        if !span.is_empty() {
            self.authored.push(span);
        }
        Ok(())
    }
}

struct Emitter<'a> {
    ct: CodeTransform<'a>,
    source: &'a str,
    strategy: GuardStrategy,
    /// Rendered condition terms of the current path ([`GuardStrategy::ReplayPath`]).
    path: Vec<String>,
    /// The end-of-source anchor every output piece is appended at.
    end: u32,
    work: GenerationWork,
    authored_bytes: usize,
    callbacks: Vec<CallbackSite>,
    output_len: usize,
}

impl Emitter<'_> {
    /// Remove every authored byte outside the (sorted, disjoint) used spans.
    fn remove_unused(&mut self, used: &[Span]) {
        let mut cursor = 0;
        for span in used {
            self.remove(cursor, span.start);
            cursor = span.end;
        }
        self.remove(cursor, self.end);
    }

    fn remove(&mut self, start: u32, end: u32) {
        if start < end {
            self.ct.remove(start, end);
            self.work.transform_ops += 1;
        }
    }

    fn synthetic(&mut self, text: &str) {
        if !text.is_empty() {
            self.ct.append_left(self.end, text);
            self.work.transform_ops += 1;
            self.output_len += text.len();
        }
    }

    fn authored(&mut self, span: Span) {
        if !span.is_empty() {
            self.ct.move_slice(span.start, span.end, self.end);
            self.work.transform_ops += 1;
            self.output_len += span.len() as usize;
            self.authored_bytes += span.len() as usize;
        }
    }

    fn expr(&mut self, expr: &ResolvedExpr) {
        for piece in &expr.pieces {
            match piece {
                Piece::Authored(span) => self.authored(*span),
                Piece::Synthetic(text) => self.synthetic(text),
            }
        }
    }

    /// Emit `expr` with `inserted` placed at authored offset `at`. An insert at
    /// the end of an authored run lands before the synthetic text that follows
    /// it, so an accessor prefix stays attached to its identifier.
    fn expr_with_insert(&mut self, expr: &ResolvedExpr, at: u32, inserted: &str) {
        let mut pending = true;
        if at == expr.span.start {
            self.synthetic(inserted);
            pending = false;
        }
        for piece in &expr.pieces {
            match piece {
                Piece::Authored(span) if pending && span.start < at && at < span.end => {
                    self.authored(Span::new(span.start, at));
                    self.synthetic(inserted);
                    self.authored(Span::new(at, span.end));
                    pending = false;
                }
                Piece::Authored(span) => {
                    self.authored(*span);
                    if pending && span.end == at {
                        self.synthetic(inserted);
                        pending = false;
                    }
                }
                Piece::Synthetic(text) => self.synthetic(text),
            }
        }
    }

    fn items(&mut self, items: &[Item]) {
        for item in items {
            match item {
                Item::Chain(chain) => self.chain(chain),
                Item::Frame(frame) => self.frame(frame),
                Item::Callback(callback) => self.callback(callback),
                Item::Expr(expr) => self.value(expr),
            }
        }
    }

    fn chain(&mut self, chain: &Chain) {
        let path_len = self.path.len();
        let replay = self.strategy == GuardStrategy::ReplayPath;
        let mut negations: Vec<String> = Vec::new();
        for branch in &chain.branches {
            match branch.kind {
                BranchKind::If => self.synthetic("if ("),
                BranchKind::ElseIf => self.synthetic("} else if ("),
                BranchKind::Else => self.synthetic("} else {\n"),
            }
            let mut rendered = None;
            if let Some(condition) = &branch.condition {
                self.work.conditions += 1;
                self.expr(condition);
                self.synthetic(") {\n");
                if replay {
                    rendered = Some(self.render(condition));
                }
            }
            if replay {
                self.path.truncate(path_len);
                self.work.replayed_terms += negations.len() as u64;
                self.path.extend(negations.iter().cloned());
                if let Some(text) = &rendered {
                    self.path.push(format!("({text})"));
                }
            }
            self.items(&branch.items);
            if let Some(text) = rendered {
                negations.push(format!("!({text})"));
            }
        }
        self.path.truncate(path_len);
        self.synthetic("}\n");
    }

    /// The text `expr` generates, for replayed path terms.
    fn render(&self, expr: &ResolvedExpr) -> String {
        let mut text = String::new();
        for piece in &expr.pieces {
            match piece {
                Piece::Authored(span) => {
                    text.push_str(&self.source[span.start as usize..span.end as usize])
                }
                Piece::Synthetic(synthetic) => text.push_str(synthetic),
            }
        }
        text
    }

    fn frame(&mut self, frame: &Frame) {
        self.synthetic("{\nconst ");
        match &frame.binding {
            FrameBinding::VFor {
                aliases,
                arity,
                source,
            } => {
                if *arity > 1 {
                    self.synthetic("[");
                }
                self.authored(*aliases);
                if *arity > 1 {
                    self.synthetic("]");
                }
                self.synthetic(match arity {
                    1 => " = __VerterFlow.each1(",
                    2 => " = __VerterFlow.each2(",
                    _ => " = __VerterFlow.each3(",
                });
                self.expr(source);
                self.synthetic(");\n");
            }
            FrameBinding::Slot { pattern, contract } => {
                self.authored(*pattern);
                self.synthetic(" = __VerterFlow.slot<");
                self.synthetic(contract);
                self.synthetic(">();\n");
            }
        }
        self.items(&frame.items);
        self.synthetic("}\n");
    }

    fn value(&mut self, item: &ExprItem) {
        match &item.contract {
            Some(contract) => {
                self.synthetic("__VerterFlow.check<");
                self.synthetic(contract);
                self.synthetic(">(");
            }
            None => self.synthetic("("),
        }
        self.expr(&item.expr);
        self.synthetic(");\n");
    }

    fn callback(&mut self, callback: &Callback) {
        self.work.callbacks += 1;
        let start = self.output_len;
        self.synthetic("__VerterFlow.check<");
        self.synthetic(&callback.contract);
        self.synthetic(">(");
        let (terms, wrapped) = match self.strategy {
            GuardStrategy::Snapshot => (snapshot_terms(callback), !callback.outer_refs.is_empty()),
            GuardStrategy::ReplayPath => (replay_terms(&self.path), false),
        };
        if wrapped {
            self.synthetic("(() => {\n");
            for (index, outer) in callback.outer_refs.iter().enumerate() {
                self.work.outer_refs += 1;
                self.synthetic(&format!("const __verter_o{index} = {};\n", outer.text));
            }
            self.synthetic("return ");
        }
        let (block_guard, expression_guard) = match &terms {
            Some(terms) => (
                format!("if ({terms}) throw 0; "),
                format!("({terms}) ? __VerterFlow.unreachable : "),
            ),
            None => (String::new(), String::new()),
        };
        let authored = match &callback.function {
            CallbackFunction::Authored { expr, guard } => {
                match *guard {
                    GuardSite::Block { after } => self.expr_with_insert(expr, after, &block_guard),
                    GuardSite::Expression { before } => {
                        self.expr_with_insert(expr, before, &expression_guard)
                    }
                }
                expr.span
            }
            CallbackFunction::Handler { parameters, body } => {
                self.synthetic("(");
                self.synthetic(parameters);
                self.synthetic(") => ");
                self.synthetic(&expression_guard);
                self.synthetic("(");
                self.expr(body);
                self.synthetic(")");
                body.span
            }
        };
        if wrapped {
            self.synthetic(";\n})()");
        }
        self.synthetic(");\n");
        self.callbacks.push(CallbackSite {
            authored,
            generated: Span::new(start as u32, self.output_len as u32),
            outer_refs: callback.outer_refs.len(),
        });
    }
}

/// `!narrow<…>(ref) || excluded<…>()(ref)` per outer reference, prefixes first.
fn snapshot_terms(callback: &Callback) -> Option<String> {
    if callback.outer_refs.is_empty() {
        return None;
    }
    let mut terms = String::new();
    for (index, outer) in callback.outer_refs.iter().enumerate() {
        if index > 0 {
            terms.push_str(" || ");
        }
        terms.push_str(&format!(
            "!__VerterFlow.narrow<typeof __verter_o{index}>({text}) || \
             __VerterFlow.excluded<typeof __verter_o{index}>()({text})",
            text = outer.text
        ));
    }
    Some(terms)
}

/// `!(<every path term>)`: the whole path, re-emitted.
fn replay_terms(path: &[String]) -> Option<String> {
    (!path.is_empty()).then(|| format!("!({})", path.join(" && ")))
}
