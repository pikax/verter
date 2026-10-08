//! Flow-transparent `v-if` narrowing for the IDE template emitter.
//!
//! TypeScript narrows a reference only along one function's control flow. A
//! non-immediately-invoked function starts that flow from the DECLARED type of
//! every property-access reference (`checker.ts` `getFlowTypeOfReference`, the
//! `FlowStart` arm), while an immediately invoked one continues the flow of the
//! expression that calls it. The emitter therefore keeps every condition in ONE
//! flow and re-narrows only where a real callback starts a new one:
//!
//! - A `v-if` / `v-else-if` / `v-else` chain is one immediately invoked block,
//!   `{(()=>{if(A){…}else if(B){…}else{…}})()}`: each authored condition is
//!   emitted once, and everything inside a branch — nested chains included —
//!   inherits every enclosing positive and predecessor negation through flow.
//! - A `v-for` frame is an immediately invoked block too,
//!   `{(() => { const ___VERTER___vN = (<source>); { const <aliases> = ___VERTER___flowEachK(___VERTER___vN); return (…); } })()}`,
//!   like the existing scoped-slot frame (the source is evaluated before the
//!   aliases' block, so an alias never shadows a name its source reads).
//! - An authored callback (or the handler the emitter wraps around an inline
//!   statement) under a condition is re-narrowed from snapshots: the innermost
//!   statement scope enclosing it (a branch block, a frame body, or a wrapped
//!   lifted branch) declares `const ___VERTER___oN = <outer reference>;` in the
//!   narrowed flow, and the callback body opens with
//!   `if (!flowNarrow(ref, oN) || flowExcluded(oN)(ref)) throw 0;` per outer
//!   reference it reads (an expression body becomes `{ <guard> return <body>; }`,
//!   so a return-type error still lands on the authored body), so its authored
//!   return type and contextual parameter typing are unchanged and the callback
//!   itself stays an ordinary, contextually typed function.
//!
//! Generated size is bounded by a constant per branch, frame, callback and
//! outer reference plus the authored text and three copies of each outer
//! reference chain a callback reads; the depth of the condition path never
//! appears. Every byte goes through `CodeGenOutput` (and so `CodeTransform`):
//! snapshot declarations fill a prepend reserved when their scope opened.

use oxc_ast::ast::{
    ArrowFunctionExpression, AssignmentExpression, BindingPattern, ChainElement, Expression,
    FormalParameters, Function, FunctionBody, SimpleAssignmentTarget, Statement, UpdateExpression,
};
use oxc_ast_visit::{walk, Visit};
use oxc_span::{GetSpan, Span};
use oxc_syntax::operator::AssignmentOperator;
use oxc_syntax::scope::ScopeFlags;
use rustc_hash::FxHashMap;

use crate::template::code_gen::binding::BindingResolver;
use crate::template::code_gen::types::{CodeGenOutput, ReservedPrepend};
use crate::template::oxc::types::OxcParsedExpression;

/// Re-narrows a reference to the type of its snapshot.
pub(crate) const NARROW_HELPER: &str = "___VERTER___flowNarrow";
/// Removes constituents `flowNarrow` re-admits only as subtypes of kept ones.
pub(crate) const EXCLUDED_HELPER: &str = "___VERTER___flowExcluded";
/// Prefix of every snapshot declaration.
const SNAPSHOT_PREFIX: &str = "___VERTER___o";
/// Prefix of every evaluated `v-for` source.
const FRAME_SOURCE_PREFIX: &str = "___VERTER___v";

/// The `v-for` frame helper for an alias list of `arity` aliases (1..=3).
pub(crate) fn each_helper(arity: usize) -> &'static str {
    match arity {
        0 | 1 => "___VERTER___flowEach1",
        2 => "___VERTER___flowEach2",
        _ => "___VERTER___flowEach3",
    }
}

/// Where a re-narrowing guard goes inside a callback.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GuardSite {
    /// Block body: right after its opening `{` (file offset).
    Block { after: u32 },
    /// Expression body `[before, end)` (file offsets): it becomes a block
    /// body returning the authored expression.
    Expression { before: u32, end: u32 },
}

/// A guard to splice, unmapped, into an authored callback.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct GuardInjection {
    pub source_offset: u32,
    pub text: String,
    /// For an expression body, the block close at the body end. Emit it after
    /// the body's own prepends so it follows any text they add at that offset.
    pub close: Option<(u32, &'static str)>,
}

/// The callback an authored directive value forms.
#[derive(Clone, Copy)]
pub(crate) enum CallbackSource<'e, 'alloc> {
    /// An arrow or function expression the author wrote.
    Function(&'e Expression<'alloc>),
    /// An inline statement the emitter wraps as `(<parameters>) => { … }`;
    /// the guard goes at `body_start`. `event` declares `$event`.
    Handler {
        body: HandlerBody<'e, 'alloc>,
        body_start: u32,
        event: bool,
    },
}

/// The authored body of a handler the emitter wraps.
#[derive(Clone, Copy)]
pub(crate) enum HandlerBody<'e, 'alloc> {
    Expression(&'e Expression<'alloc>),
    Statements(&'e [Statement<'alloc>]),
}

impl HandlerBody<'_, '_> {
    /// The handler body of a parsed `v-on` value, if it parsed.
    pub(crate) fn of<'e, 'alloc>(
        parsed: &'e OxcParsedExpression<'alloc>,
    ) -> Option<HandlerBody<'e, 'alloc>> {
        match (&parsed.expression, parsed.statements) {
            (Some(expression), _) => Some(HandlerBody::Expression(expression)),
            (None, Some(program)) => Some(HandlerBody::Statements(&program.body)),
            (None, None) => None,
        }
    }

    fn span(&self) -> Span {
        match self {
            HandlerBody::Expression(expression) => expression.span(),
            HandlerBody::Statements(statements) => match (statements.first(), statements.last()) {
                (Some(first), Some(last)) => Span::new(first.span().start, last.span().end),
                _ => Span::default(),
            },
        }
    }
}

/// One reference chain (`__props.user.name`) a callback reads in its own flow.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct OuterRef {
    /// Resolved text. In TypeScript output a non-null assertion precedes each
    /// non-optional step, so the synthetic copies never repeat a diagnostic the
    /// authored chain already reports; JavaScript output has no assertion
    /// syntax, and a repeated diagnostic there lands in unmapped scaffolding.
    pub text: String,
    /// One authored occurrence of the chain inside the callback (file offsets).
    pub occurrence: Span,
}

/// The guard site of an authored function value, if `expression` is one.
pub(crate) fn function_guard_site(expression: &Expression<'_>, base: u32) -> Option<GuardSite> {
    match expression.without_parentheses() {
        Expression::ArrowFunctionExpression(arrow) => Some(match arrow.get_expression() {
            Some(body) => GuardSite::Expression {
                before: base + body.span().start,
                end: base + body.span().end,
            },
            None => GuardSite::Block {
                after: base + arrow.body.span().start + 1,
            },
        }),
        Expression::FunctionExpression(function) => {
            function.body.as_ref().map(|body| GuardSite::Block {
                after: base + body.span.start + 1,
            })
        }
        _ => None,
    }
}

struct FlowScope {
    slot: ReservedPrepend,
    shape: ScopeShape,
    declarations: String,
    snapshots: FxHashMap<String, u32>,
}

/// How a scope's reserved text is laid out around its declarations.
enum ScopeShape {
    /// Declarations alone, at a statement position.
    Statements,
    /// A frame's declaration end (`head`) and body opening (`tail`), always
    /// emitted, with the declarations between them on their own lines.
    Frame { head: String, tail: &'static str },
    /// A lifted branch without a statement position of its own: when it needs
    /// snapshots it is wrapped in an immediately invoked arrow closed at `close`.
    Wrapped { close: u32 },
}

/// Balances one [`FlowNarrowing::open`] with its [`FlowNarrowing::close`].
#[must_use = "every opened flow scope must be closed"]
pub(crate) struct ScopeToken {
    pushed: bool,
    condition: bool,
}

/// What a scope being opened is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ScopeKind<'t> {
    /// A chain branch block: narrowed by its condition path, statements allowed.
    Branch,
    /// A `v-for` or scoped-slot frame: `head` ends its declaration, `tail`
    /// opens its body; the frame's snapshot declarations go between them.
    Frame { head: &'t str, tail: &'static str },
    /// A lifted (ternary) branch whose element has no statement position.
    LiftedBranch { close: u32 },
    /// A lifted (ternary) branch whose element opens its own frame scope.
    LiftedCondition,
}

/// The narrowing state of one template walk.
pub(crate) struct FlowNarrowing {
    /// Conditions (chain branches) enclosing the current position.
    conditions: u32,
    scopes: Vec<FlowScope>,
    next_snapshot: u32,
    next_frame_source: u32,
    /// TypeScript output (non-null assertions are available).
    typescript: bool,
}

impl FlowNarrowing {
    pub(crate) fn new(typescript: bool) -> Self {
        Self {
            conditions: 0,
            scopes: Vec::new(),
            next_snapshot: 0,
            next_frame_source: 0,
            typescript,
        }
    }

    /// A fresh name for the evaluated source of one `v-for` frame.
    pub(crate) fn frame_source_name(&mut self) -> String {
        let index = self.next_frame_source;
        self.next_frame_source += 1;
        format!("{FRAME_SOURCE_PREFIX}{index}")
    }

    /// Open a scope at `at`. Its text lands after every ordered prepend
    /// already recorded at `at` and before every one recorded later.
    pub(crate) fn open(
        &mut self,
        out: &mut CodeGenOutput<'_>,
        at: u32,
        kind: ScopeKind<'_>,
    ) -> ScopeToken {
        record(|work| work.scopes += 1);
        let condition = !matches!(kind, ScopeKind::Frame { .. });
        if condition {
            self.conditions += 1;
        }
        let shape = match kind {
            ScopeKind::Branch => ScopeShape::Statements,
            ScopeKind::Frame { head, tail } => ScopeShape::Frame {
                head: head.to_string(),
                tail,
            },
            ScopeKind::LiftedBranch { close } => ScopeShape::Wrapped { close },
            ScopeKind::LiftedCondition => {
                return ScopeToken {
                    pushed: false,
                    condition,
                }
            }
        };
        self.scopes.push(FlowScope {
            slot: out.reserve_ordered_unmapped(at),
            shape,
            declarations: String::new(),
            snapshots: FxHashMap::default(),
        });
        ScopeToken {
            pushed: true,
            condition,
        }
    }

    /// Close the scope `token` opened, emitting its text.
    pub(crate) fn close(&mut self, out: &mut CodeGenOutput<'_>, token: ScopeToken) {
        if token.condition {
            self.conditions -= 1;
        }
        if !token.pushed {
            return;
        }
        let scope = self.scopes.pop().expect("a pushed flow scope is open");
        match scope.shape {
            ScopeShape::Frame { head, tail } if scope.declarations.is_empty() => {
                out.fill_reserved(scope.slot, &format!("{head}{tail}"));
            }
            ScopeShape::Frame { head, tail } => {
                out.fill_reserved(scope.slot, &format!("{head}\n{}{tail}", scope.declarations));
            }
            _ if scope.declarations.is_empty() => {}
            ScopeShape::Statements => out.fill_reserved(scope.slot, &scope.declarations),
            ScopeShape::Wrapped { close } => {
                let open = format!("(() => {{ {}return (", scope.declarations);
                out.fill_reserved(scope.slot, &open);
                out.prepend_alloc(close, "); })()");
            }
        }
    }

    /// The guard for a callback at the current position, registering its
    /// snapshots in the innermost scope. `None` when no condition encloses it
    /// or it reads no outer reference.
    pub(crate) fn callback_guard(
        &mut self,
        source: &str,
        parsed: &OxcParsedExpression<'_>,
        callback: CallbackSource<'_, '_>,
        resolver: &BindingResolver<'_>,
    ) -> Option<GuardInjection> {
        if self.conditions == 0 || self.scopes.is_empty() {
            return None;
        }
        let site = match callback {
            CallbackSource::Function(expression) => function_guard_site(expression, parsed.offset)?,
            CallbackSource::Handler { body_start, .. } => GuardSite::Block { after: body_start },
        };
        let refs = outer_refs(source, parsed, callback, resolver, self.typescript);
        if refs.is_empty() {
            return None;
        }
        record(|work| work.callbacks += 1);
        let scope = self.scopes.last_mut().expect("checked non-empty");
        let mut terms = String::new();
        for outer in &refs {
            let index = match scope.snapshots.get(&outer.text) {
                Some(&index) => index,
                None => {
                    let index = self.next_snapshot;
                    self.next_snapshot += 1;
                    scope.declarations.push_str(&format!(
                        "const {SNAPSHOT_PREFIX}{index} = {};\n",
                        outer.text
                    ));
                    scope.snapshots.insert(outer.text.clone(), index);
                    record(|work| work.snapshots += 1);
                    index
                }
            };
            record(|work| work.outer_refs += 1);
            if !terms.is_empty() {
                terms.push_str(" || ");
            }
            terms.push_str(&format!(
                "!{NARROW_HELPER}({text}, {SNAPSHOT_PREFIX}{index}) || \
                 {EXCLUDED_HELPER}({SNAPSHOT_PREFIX}{index})({text})",
                text = outer.text
            ));
        }
        Some(match site {
            GuardSite::Block { after } => GuardInjection {
                source_offset: after,
                text: format!("if ({terms}) throw 0; "),
                close: None,
            },
            GuardSite::Expression { before, end } => GuardInjection {
                source_offset: before,
                text: format!("{{ if ({terms}) throw 0; return "),
                close: Some((end, "; }")),
            },
        })
    }
}

/// Construction work of the emitter's narrowing, counted where it happens.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[cfg_attr(
    not(any(test, feature = "semantic-observe")),
    allow(dead_code, reason = "counted only in measurement builds")
)]
pub struct FlowWork {
    /// Authored chain conditions resolved and emitted.
    pub conditions: u64,
    /// Chain members read while planning chains (each names its predecessor).
    pub chain_members: u64,
    /// Narrowing scopes opened (branch blocks, frames, lifted branches).
    pub scopes: u64,
    /// Callbacks given a re-narrowing guard.
    pub callbacks: u64,
    /// Outer-reference re-narrowings emitted inside callbacks.
    pub outer_refs: u64,
    /// Snapshot declarations emitted.
    pub snapshots: u64,
}

#[cfg(any(test, feature = "semantic-observe"))]
thread_local! {
    static FLOW_WORK: std::cell::Cell<FlowWork> = const {
        std::cell::Cell::new(FlowWork {
            conditions: 0,
            chain_members: 0,
            scopes: 0,
            callbacks: 0,
            outer_refs: 0,
            snapshots: 0,
        })
    };
}

#[cfg(any(test, feature = "semantic-observe"))]
#[inline]
pub(crate) fn record(update: impl FnOnce(&mut FlowWork)) {
    FLOW_WORK.with(|cell| {
        let mut work = cell.get();
        update(&mut work);
        cell.set(work);
    });
}

#[cfg(not(any(test, feature = "semantic-observe")))]
#[inline(always)]
pub(crate) fn record(_update: impl FnOnce(&mut FlowWork)) {}

/// Read and reset the per-thread narrowing work counters.
#[cfg(any(test, feature = "semantic-observe"))]
pub fn take_flow_work() -> FlowWork {
    FLOW_WORK.with(|cell| cell.replace(FlowWork::default()))
}

/// The reference chains a callback reads in its own flow (not inside a
/// nested function), each with every prefix listed before it.
pub(crate) fn outer_refs(
    source: &str,
    parsed: &OxcParsedExpression<'_>,
    callback: CallbackSource<'_, '_>,
    resolver: &BindingResolver<'_>,
    non_null_steps: bool,
) -> Vec<OuterRef> {
    let base = parsed.offset;
    let (step_text, index_step) = if non_null_steps {
        ("!.", "![")
    } else {
        (".", "[")
    };
    let region = match callback {
        CallbackSource::Function(expression) => expression.span(),
        CallbackSource::Handler { body, .. } => body.span(),
    };
    let region = Span::new(base + region.start, base + region.end);
    let chains = verter_parser::oxc_parse::with_span_stack(source, region, || {
        let mut locals = Locals::default();
        match callback {
            CallbackSource::Function(expression) => match expression.without_parentheses() {
                Expression::ArrowFunctionExpression(arrow) => {
                    locals.function(
                        arrow.span,
                        None,
                        &arrow.params,
                        arrow.get_function_body(),
                        arrow.get_expression(),
                    );
                    let mut refs = Refs::new(&locals);
                    refs.parameters(&arrow.params);
                    match (arrow.get_function_body(), arrow.get_expression()) {
                        (_, Some(expression)) => refs.visit_expression(expression),
                        (Some(body), None) => refs.statements(&body.statements),
                        (None, None) => {}
                    }
                    refs.chains
                }
                Expression::FunctionExpression(function) => {
                    locals.function(
                        function.span,
                        function.id.as_ref().map(|id| id.name.as_str()),
                        &function.params,
                        function.body.as_deref(),
                        None,
                    );
                    let mut refs = Refs::new(&locals);
                    refs.parameters(&function.params);
                    if let Some(body) = &function.body {
                        refs.statements(&body.statements);
                    }
                    refs.chains
                }
                _ => Vec::new(),
            },
            CallbackSource::Handler { body, event, .. } => {
                locals.handler(body, event);
                let mut refs = Refs::new(&locals);
                match body {
                    HandlerBody::Expression(expression) => refs.visit_expression(expression),
                    HandlerBody::Statements(statements) => refs.statements(statements),
                }
                refs.chains
            }
        }
    });

    let mut seen: FxHashMap<String, ()> = FxHashMap::default();
    let mut out: Vec<(usize, OuterRef)> = Vec::new();
    for chain in chains {
        let mut text = root_text(parsed, resolver, base + chain.root.start, chain.name);
        let mut occurrence = Span::new(base + chain.root.start, base + chain.root.end);
        for depth in 0..=chain.steps.len() {
            if depth > 0 {
                let step = &chain.steps[depth - 1];
                match step.key {
                    StepKey::Name(name) => {
                        text.push_str(if step.optional { "?." } else { step_text });
                        text.push_str(name);
                    }
                    StepKey::Index(span) => {
                        text.push_str(if step.optional { "?.[" } else { index_step });
                        text.push_str(
                            &source[(base + span.start) as usize..(base + span.end) as usize],
                        );
                        text.push(']');
                    }
                }
                occurrence.end = base + step.end;
            }
            if seen.insert(text.clone(), ()).is_none() {
                out.push((
                    depth,
                    OuterRef {
                        text: text.clone(),
                        occurrence,
                    },
                ));
            }
        }
    }
    out.sort_by_key(|(depth, _)| *depth);
    out.into_iter().map(|(_, outer)| outer).collect()
}

/// Resolved text of a free root identifier: its accessor prefix when the
/// template parse classified it as an outer binding, the bare name for a
/// template-scope local.
fn root_text(
    parsed: &OxcParsedExpression<'_>,
    resolver: &BindingResolver<'_>,
    pos: u32,
    name: &str,
) -> String {
    let outer = parsed
        .bindings
        .as_ref()
        .and_then(|b| b.bindings.iter().find(|b| b.pos == pos))
        .is_some_and(|b| !b.ignore);
    if outer {
        format!(
            "{}{}{}",
            resolver.resolve_prefix(name),
            name,
            resolver.resolve_suffix(name)
        )
    } else {
        name.to_string()
    }
}

/// Names a callback declares, each with the authored range it is visible in.
#[derive(Default)]
struct Locals<'alloc> {
    declared: Vec<(&'alloc str, Span)>,
    /// Innermost lexical range first; the function range at the bottom.
    blocks: Vec<Span>,
    function: Option<Span>,
}

impl<'alloc> Locals<'alloc> {
    fn function(
        &mut self,
        span: Span,
        name: Option<&'alloc str>,
        params: &FormalParameters<'alloc>,
        body: Option<&FunctionBody<'alloc>>,
        expression: Option<&Expression<'alloc>>,
    ) {
        self.function = Some(span);
        self.blocks.push(span);
        if let Some(name) = name {
            self.declare(name, span);
        }
        for param in &params.items {
            self.pattern(&param.pattern, span);
        }
        if let Some(rest) = &params.rest {
            self.pattern(&rest.rest.argument, span);
        }
        if let Some(body) = body {
            for statement in &body.statements {
                self.visit_statement(statement);
            }
        }
        if let Some(expression) = expression {
            self.visit_expression(expression);
        }
    }

    fn handler(&mut self, body: HandlerBody<'_, 'alloc>, event: bool) {
        let span = body.span();
        self.function = Some(span);
        self.blocks.push(span);
        if event {
            self.declare("$event", span);
        }
        match body {
            HandlerBody::Expression(expression) => self.visit_expression(expression),
            HandlerBody::Statements(statements) => {
                for statement in statements {
                    self.visit_statement(statement);
                }
            }
        }
    }

    fn declare(&mut self, name: &'alloc str, span: Span) {
        self.declared.push((name, span));
    }

    fn pattern(&mut self, pattern: &BindingPattern<'alloc>, scope: Span) {
        match pattern {
            BindingPattern::BindingIdentifier(ident) => self.declare(ident.name.as_str(), scope),
            BindingPattern::ObjectPattern(object) => {
                for property in &object.properties {
                    self.pattern(&property.value, scope);
                }
                if let Some(rest) = &object.rest {
                    self.pattern(&rest.argument, scope);
                }
            }
            BindingPattern::ArrayPattern(array) => {
                for element in array.elements.iter().flatten() {
                    self.pattern(element, scope);
                }
                if let Some(rest) = &array.rest {
                    self.pattern(&rest.argument, scope);
                }
            }
            BindingPattern::AssignmentPattern(assign) => self.pattern(&assign.left, scope),
        }
    }

    fn block(&self) -> Span {
        *self.blocks.last().expect("a callback scope is open")
    }

    fn is_local(&self, name: &str, at: u32) -> bool {
        self.declared
            .iter()
            .any(|(declared, scope)| *declared == name && scope.start <= at && at < scope.end)
    }

    fn scoped(&mut self, span: Span, visit: impl FnOnce(&mut Self)) {
        self.blocks.push(span);
        visit(self);
        self.blocks.pop();
    }
}

impl<'alloc> Visit<'alloc> for Locals<'alloc> {
    fn visit_block_statement(&mut self, it: &oxc_ast::ast::BlockStatement<'alloc>) {
        self.scoped(it.span, |this| walk::walk_block_statement(this, it));
    }

    fn visit_for_statement(&mut self, it: &oxc_ast::ast::ForStatement<'alloc>) {
        self.scoped(it.span, |this| walk::walk_for_statement(this, it));
    }

    fn visit_for_in_statement(&mut self, it: &oxc_ast::ast::ForInStatement<'alloc>) {
        self.scoped(it.span, |this| walk::walk_for_in_statement(this, it));
    }

    fn visit_for_of_statement(&mut self, it: &oxc_ast::ast::ForOfStatement<'alloc>) {
        self.scoped(it.span, |this| walk::walk_for_of_statement(this, it));
    }

    fn visit_switch_statement(&mut self, it: &oxc_ast::ast::SwitchStatement<'alloc>) {
        self.scoped(it.span, |this| walk::walk_switch_statement(this, it));
    }

    fn visit_catch_clause(&mut self, it: &oxc_ast::ast::CatchClause<'alloc>) {
        self.scoped(it.span, |this| {
            if let Some(param) = &it.param {
                let scope = this.block();
                this.pattern(&param.pattern, scope);
            }
            this.visit_block_statement(&it.body);
        });
    }

    fn visit_variable_declaration(&mut self, it: &oxc_ast::ast::VariableDeclaration<'alloc>) {
        let scope = if it.kind.is_var() {
            self.function.expect("a callback scope is open")
        } else {
            self.block()
        };
        for declarator in &it.declarations {
            self.pattern(&declarator.id, scope);
            if let Some(init) = &declarator.init {
                self.visit_expression(init);
            }
        }
    }

    fn visit_function(&mut self, it: &Function<'alloc>, _flags: ScopeFlags) {
        if let Some(id) = &it.id {
            if it.is_declaration() {
                let scope = self.block();
                self.declare(id.name.as_str(), scope);
            }
        }
    }

    fn visit_class(&mut self, it: &oxc_ast::ast::Class<'alloc>) {
        if let Some(id) = &it.id {
            if it.is_declaration() {
                let scope = self.block();
                self.declare(id.name.as_str(), scope);
            }
        }
    }

    fn visit_arrow_function_expression(&mut self, _it: &ArrowFunctionExpression<'alloc>) {}
}

/// One reference chain: a free root identifier followed by static or
/// literal-indexed steps. Spans are relative to the parsed expression.
struct RefChain<'alloc> {
    name: &'alloc str,
    root: Span,
    steps: Vec<Step<'alloc>>,
}

struct Step<'alloc> {
    optional: bool,
    key: StepKey<'alloc>,
    /// End of this step in the parsed expression.
    end: u32,
}

enum StepKey<'alloc> {
    Name(&'alloc str),
    /// Span of a string or numeric literal index.
    Index(Span),
}

struct Refs<'l, 'alloc> {
    locals: &'l Locals<'alloc>,
    chains: Vec<RefChain<'alloc>>,
}

impl<'l, 'alloc> Refs<'l, 'alloc> {
    fn new(locals: &'l Locals<'alloc>) -> Self {
        Self {
            locals,
            chains: Vec::new(),
        }
    }

    fn parameters(&mut self, params: &FormalParameters<'alloc>) {
        for param in &params.items {
            if let Some(initializer) = &param.initializer {
                self.visit_expression(initializer);
            }
        }
    }

    fn statements(&mut self, statements: &[Statement<'alloc>]) {
        for statement in statements {
            self.visit_statement(statement);
        }
    }

    fn free(&self, name: &str, at: u32) -> bool {
        !matches!(name, "undefined" | "NaN" | "Infinity" | "arguments")
            && !self.locals.is_local(name, at)
    }

    /// Record the longest reference chain ending at `expression`, visiting
    /// whatever is not part of it. Returns whether `expression` was a chain.
    fn chain(&mut self, expression: &Expression<'alloc>) -> bool {
        self.chain_from(expression, Vec::new())
    }

    /// [`Self::chain`] continued from `steps` already read off an outer member
    /// (innermost last).
    fn chain_from(
        &mut self,
        expression: &Expression<'alloc>,
        mut steps: Vec<Step<'alloc>>,
    ) -> bool {
        let mut current = expression;
        loop {
            match current {
                Expression::StaticMemberExpression(member) => {
                    steps.push(Step {
                        optional: member.optional,
                        key: StepKey::Name(member.property.name.as_str()),
                        end: member.span.end,
                    });
                    current = &member.object;
                }
                Expression::ComputedMemberExpression(member) => match &member.expression {
                    Expression::StringLiteral(_) | Expression::NumericLiteral(_) => {
                        steps.push(Step {
                            optional: member.optional,
                            key: StepKey::Index(member.expression.span()),
                            end: member.span.end,
                        });
                        current = &member.object;
                    }
                    other => {
                        self.visit_expression(other);
                        steps.clear();
                        current = &member.object;
                    }
                },
                Expression::ChainExpression(chain) => match &chain.expression {
                    ChainElement::StaticMemberExpression(member) => {
                        steps.push(Step {
                            optional: member.optional,
                            key: StepKey::Name(member.property.name.as_str()),
                            end: member.span.end,
                        });
                        current = &member.object;
                    }
                    ChainElement::ComputedMemberExpression(member)
                        if matches!(
                            member.expression,
                            Expression::StringLiteral(_) | Expression::NumericLiteral(_)
                        ) =>
                    {
                        steps.push(Step {
                            optional: member.optional,
                            key: StepKey::Index(member.expression.span()),
                            end: member.span.end,
                        });
                        current = &member.object;
                    }
                    _ => {
                        walk::walk_chain_expression(self, chain);
                        return true;
                    }
                },
                Expression::TSNonNullExpression(non_null) => current = &non_null.expression,
                Expression::ParenthesizedExpression(paren) => current = &paren.expression,
                Expression::Identifier(ident) => {
                    if self.free(ident.name.as_str(), ident.span.start) {
                        steps.reverse();
                        self.chains.push(RefChain {
                            name: ident.name.as_str(),
                            root: ident.span,
                            steps,
                        });
                    }
                    return true;
                }
                other => {
                    if steps.is_empty() && std::ptr::eq(other, expression) {
                        return false;
                    }
                    self.visit_expression(other);
                    return true;
                }
            }
        }
    }
}

impl<'alloc> Refs<'_, 'alloc> {
    /// An assignment target the operation also READS (`x++`, `x += 1`): its
    /// whole reference chain is an outer reference like any other read.
    fn read_target(&mut self, target: &SimpleAssignmentTarget<'alloc>) {
        match target {
            SimpleAssignmentTarget::AssignmentTargetIdentifier(ident) => {
                if self.free(ident.name.as_str(), ident.span.start) {
                    self.chains.push(RefChain {
                        name: ident.name.as_str(),
                        root: ident.span,
                        steps: Vec::new(),
                    });
                }
            }
            SimpleAssignmentTarget::StaticMemberExpression(member) => {
                let step = Step {
                    optional: false,
                    key: StepKey::Name(member.property.name.as_str()),
                    end: member.span.end,
                };
                self.chain_from(&member.object, vec![step]);
            }
            SimpleAssignmentTarget::ComputedMemberExpression(member)
                if matches!(
                    member.expression,
                    Expression::StringLiteral(_) | Expression::NumericLiteral(_)
                ) =>
            {
                let step = Step {
                    optional: false,
                    key: StepKey::Index(member.expression.span()),
                    end: member.span.end,
                };
                self.chain_from(&member.object, vec![step]);
            }
            other => walk::walk_simple_assignment_target(self, other),
        }
    }
}

impl<'alloc> Visit<'alloc> for Refs<'_, 'alloc> {
    fn visit_expression(&mut self, it: &Expression<'alloc>) {
        if !self.chain(it) {
            walk::walk_expression(self, it);
        }
    }

    fn visit_update_expression(&mut self, it: &UpdateExpression<'alloc>) {
        self.read_target(&it.argument);
    }

    fn visit_assignment_expression(&mut self, it: &AssignmentExpression<'alloc>) {
        match (&it.operator, it.left.as_simple_assignment_target()) {
            (operator, Some(target)) if *operator != AssignmentOperator::Assign => {
                self.read_target(target);
            }
            _ => self.visit_assignment_target(&it.left),
        }
        self.visit_expression(&it.right);
    }

    fn visit_function(&mut self, _it: &Function<'alloc>, _flags: ScopeFlags) {}

    fn visit_class(&mut self, _it: &oxc_ast::ast::Class<'alloc>) {}

    fn visit_arrow_function_expression(&mut self, _it: &ArrowFunctionExpression<'alloc>) {}
}
