//! Reactive-text + component-context ANALYSIS for the Svelte client emitter.
//!
//! Two read-only analyses the client backend consults to match the official
//! emission topology — neither rewrites a read nor emits JS:
//!
//! - [`expr_has_call`] — the official `has_call` metadata fact that drives the
//!   MEMOIZED `$.template_effect(($0) => …, [() => expr])` deps-array form for a
//!   reactive-text chunk (vs the inline `$.set_text(text, expr)`).
//! - [`needs_context`] — the official `needs_context` analysis fact that drives the
//!   `$.push($$props, true)` … `$.pop()` component-context plumbing.
//!
//! Both are scope-aware (they reuse the shared lexical [`ShadowStack`] model in
//! [`super::expr`]) and drive purely from the OXC AST + the binding table — never a
//! source-text scan.

use oxc_allocator::Allocator;
use oxc_ast::ast::{
    ArrowFunctionExpression, BlockStatement, CallExpression, CatchClause, ChainElement, Expression,
    ForInStatement, ForOfStatement, ForStatement, Function, Program, Statement,
    VariableDeclarationKind,
};
use oxc_ast_visit::{walk, Visit};
use oxc_span::SourceType;

use super::expr::{
    arrow_scope_names, block_scope_names, collect_direct_decls, collect_pattern_names,
    collect_var_hoists, for_left_names, function_scope_names, is_effect_callee, is_props_callee,
    reparse_module, BindingRuntimeKind, BindingTable, ScopeGraph, ScopeId, ShadowStack,
    UnwrappedRootKind,
};
use super::expr_rewrite::is_signal_kind;

// ---------------------------------------------------------------------------
// `has_call` — the reactive-text memoizer trigger
// ---------------------------------------------------------------------------

/// Whether a reactive-text expression `has_call` — the official metadata fact that
/// drives the MEMOIZED `$.template_effect(($0) => …, [() => expr])` deps-array form
/// (vs the inline `$.set_text(text, expr)`).
///
/// Mirrors `phases/2-analyze/visitors/CallExpression.js`: at EACH call, in SOURCE
/// (AST-visit) order, a call sets `has_call` iff its callee is NOT statically pure
/// (`is_pure(callee)`) OR a dependency was ALREADY accumulated by that point —
/// `!is_pure(node.callee) || context.state.expression.dependencies.size > 0`. The
/// `dependencies` set is SHARED across the whole template expression and grows as
/// the visit walks it: each `Identifier` resolving to a binding (reactive or not)
/// adds a dependency at its visit point, and the call's check runs AFTER its own
/// callee + arguments are visited (`context.next()` precedes the check), so the
/// call's own callee/argument bindings count too. A call appearing BEFORE any
/// dependency in source order (e.g. `globalThis?.foo?.() + v`) is therefore NOT
/// `has_call` even though `v` is referenced later; the same call AFTER a dependency
/// (`v + globalThis?.foo?.()`) IS. `is_pure(callee)` is true only when the callee's
/// leftmost identifier resolves to a GLOBAL (no binding) — a callee rooted at a
/// declared binding (a local function, an import) is impure.
///
/// A `NewExpression` does NOT itself set `has_call` (official `NewExpression.js`
/// only sets `needs_context`); it contributes only through its inner identifiers'
/// dependencies. A `TaggedTemplateExpression` sets `has_call` only when its tag is
/// impure (official `TaggedTemplateExpression.js`: `!is_pure(node.tag)`). A bare
/// signal read / member access with NO call is NOT `has_call` (it stays the inline
/// `$.set_text` form). The scan does NOT descend into nested function bodies — a
/// handler arrow inside an interpolation is not the evaluated reactive value, and
/// official sets `expression: null` there so neither its identifiers nor its calls
/// participate.
#[must_use]
pub fn expr_has_call(
    source: &str,
    scope: ScopeId,
    bindings: &BindingTable,
    scopes: &ScopeGraph,
    declared_roots: &rustc_hash::FxHashSet<String>,
) -> bool {
    let alloc = Allocator::default();
    let wrapped = format!("({source})");
    let parsed = oxc_parser::Parser::new(&alloc, &wrapped, SourceType::tsx()).parse();
    if parsed.panicked || !parsed.errors.is_empty() {
        return false;
    }
    let Some(Statement::ExpressionStatement(stmt)) = parsed.program.body.first() else {
        return false;
    };
    let inner = match &stmt.expression {
        Expression::ParenthesizedExpression(p) => &p.expression,
        other => other,
    };
    let mut scan = HasCallScan {
        declared_roots,
        bindings,
        scopes,
        scope,
        deps: 0,
        found: false,
    };
    scan.visit_expr(inner);
    scan.found
}

// ---------------------------------------------------------------------------
// `needs_clsx` — the `class={…}` `$.clsx(…)` wrap predicate
// ---------------------------------------------------------------------------

/// Whether a single-expression `class={expr}` base value needs the `$.clsx(expr)`
/// wrap — the official `phases/2-analyze/visitors/Attribute.js` `needs_clsx` rule.
///
/// Official sets `node.metadata.needs_clsx = true` for a `class={…}` attribute whose
/// value expression is NOT a `Literal`, NOT a `TemplateLiteral`, and NOT a
/// `BinaryExpression`:
///
/// ```js
/// if (
///   node.name === 'class' &&
///   !Array.isArray(node.value) &&
///   node.value !== true &&
///   node.value.expression.type !== 'Literal' &&
///   node.value.expression.type !== 'TemplateLiteral' &&
///   node.value.expression.type !== 'BinaryExpression'
/// ) { node.metadata.needs_clsx = true; }
/// ```
///
/// So `class={a + b}` (a `BinaryExpression`, e.g. string concatenation), `class={'x'}`
/// (a `Literal`), and `` class={`a${b}`} `` (a `TemplateLiteral`) emit the value WITHOUT
/// the `$.clsx` wrap; an `Identifier` / `CallExpression` / `ConditionalExpression` /
/// `LogicalExpression` / `ObjectExpression` / `ArrayExpression` / `MemberExpression`
/// (and every other shape) IS wrapped. A `Mixed`-string class (`class="a {x} b"`) is
/// lowered to a TemplateLiteral upstream, so it is handled by its own (non-clsx) path
/// and never reaches this predicate.
///
/// The KIND is read from the TRANSPARENT-ROOT-UNWRAPPED top-level expression — NOT the
/// paren-wrapped root. Official runs the value expression through its AST printer (which
/// drops transparent author parens) BEFORE the `needs_clsx` node-type check, so a
/// parenthesized literal / template / binary (`class={('x')}` / `class={((a + b))}` /
/// `` class={(`x${a}`)} ``) stays UNWRAPPED. The kind comes from the shared
/// [`UnwrappedRootKind`] analysis fact ([`AnalyzedExpr::unwrapped_root_kind`]) — one parse
/// per expression, no reparse, no string sniffing. This `$.clsx` DECISION is the behavioral
/// use of the unwrapped root; the emitted class value TEXT routes through the value printer
/// separately, which is source-preserving (it keeps the author's parens verbatim and rewrites
/// only signal/prop reads + strips TS — a kept redundant paren is a behavior-preserving
/// cosmetic difference the minifier collapses).
///
/// [`UnwrappedRootKind`]: super::expr::UnwrappedRootKind
/// [`AnalyzedExpr::unwrapped_root_kind`]: super::expr::AnalyzedExpr::unwrapped_root_kind
#[must_use]
pub fn class_value_needs_clsx(kind: UnwrappedRootKind) -> bool {
    !matches!(
        kind,
        UnwrappedRootKind::Literal
            | UnwrappedRootKind::TemplateLiteral
            | UnwrappedRootKind::BinaryExpression
    )
}

/// Collect the COMPONENT-DECLARED root names — every top-level declaration name in
/// the module + instance scripts (imports, `let`/`const`/`var`, function / class
/// declarations, `$props()` destructure names). A callee rooted at one of these is
/// a DECLARED binding (impure under `is_pure`); a callee rooted at a name NOT in
/// this set is a GLOBAL (pure). This is the `is_pure` scope-resolution input for
/// the `has_call` decision.
#[must_use]
pub fn collect_declared_root_names(
    alloc: &Allocator,
    module_source: Option<&str>,
    instance_source: Option<&str>,
) -> rustc_hash::FxHashSet<String> {
    let mut out = rustc_hash::FxHashSet::default();
    for src in [module_source, instance_source].into_iter().flatten() {
        if let Some(program) = reparse_module(alloc, src) {
            collect_program_top_level_names(&program, &mut out);
        }
    }
    out
}

/// Collect a program's top-level declared names into `out`.
fn collect_program_top_level_names(program: &Program<'_>, out: &mut rustc_hash::FxHashSet<String>) {
    collect_direct_decls(&program.body, out);
    collect_var_hoists(&program.body, out);
    for stmt in &program.body {
        match stmt {
            Statement::ImportDeclaration(import) => {
                if let Some(specifiers) = &import.specifiers {
                    for spec in specifiers {
                        out.insert(spec.local().name.to_string());
                    }
                }
            }
            Statement::VariableDeclaration(decl) => {
                for d in &decl.declarations {
                    let mut names = Vec::new();
                    collect_pattern_names(&d.id, &mut names);
                    out.extend(names);
                }
            }
            _ => {}
        }
    }
}

/// Whether a template expression references any reactive STATE binding — a read
/// resolving (scope-awarely) to either a reactive `$state` signal OR a `$props()`
/// prop at `scope`. This is the official `metadata.expression.has_state` signal: a
/// dynamic attribute / class / style value with `has_state` joins the combined
/// `$.template_effect`; a value with no reactive read is a one-shot init (the
/// `has_state ? update : init` split in `RegularElement.js`). A PROP read counts as
/// state because props are reactive (`$$props.x` can change), matching the
/// text-interpolation reactivity classifier (`NoDefaultPropRead` is reactive) and
/// official. It also drives the `deps > 0` half of the reactive-text `has_call`
/// memoize rule. Reuses the shared free-reference collector + the scope resolver, so
/// a shadowing local is not counted; a prop is NOT a signal (it emits `$$props.x`,
/// not `$.get`), so [`is_signal_kind`] stays prop-free.
#[must_use]
pub(super) fn expr_references_signal(
    source: &str,
    scope: ScopeId,
    bindings: &BindingTable,
    scopes: &ScopeGraph,
) -> bool {
    let Ok(facts) = super::expr::collect_expr_references(source) else {
        return false;
    };
    facts.references.iter().any(|r| {
        bindings
            .resolve_kind(scopes, scope, &r.name)
            .is_some_and(|k| is_signal_kind(k) || k == BindingRuntimeKind::Prop)
    })
}

/// Whether a template expression contains a MEMBER access whose leftmost identifier
/// resolves (scope-awarely) to a declared binding — the official
/// `phases/2-analyze/visitors/MemberExpression.js` `has_state ||= !is_pure(node)` rule.
///
/// `is_pure(member)` walks a member chain to its leftmost identifier and is pure ONLY
/// when that root resolves to a GLOBAL (no binding); a root that is a declared binding
/// (a signal, a prop, a DEMOTED `$state`, a plain local) makes the member impure, which
/// official records as `has_state`. So `{d.x}` / `{d?.x}` over a demoted `let d = $state(0)`
/// (lowered to `let d = 0`) is a reactive value (joins the `$.template_effect`), even
/// though `d` itself is not a live signal — official wraps `$.set_attribute(div, 'id',
/// d.x)` in a `template_effect`. A member rooted at a GLOBAL (`Math.PI`, `globalThis.x`)
/// is pure → NOT has_state. This is the member half of `has_state` that the signal/prop
/// reference scan ([`expr_references_signal`]) alone misses (a member rooted at a signal
/// or prop is ALREADY has_state via that scan; this adds the demoted/plain-binding root).
///
/// Optional members (`d?.x`) and private/computed members are all member accesses for
/// this rule. A bare identifier read (`{d}`, no member) is NOT covered here — official's
/// IDENTIFIER rule gates it on `!is_known`, and a demoted constant `d = 0` is known, so a
/// bare `{d}` stays a non-reactive inline write (the existing signal/prop scan correctly
/// leaves it alone). Typed-IR only: re-parses the borrowed expression source through OXC
/// (the same reparse the other analyses use) and walks the member chain.
#[must_use]
pub(super) fn expr_member_roots_at_binding(
    source: &str,
    scope: ScopeId,
    bindings: &BindingTable,
    scopes: &ScopeGraph,
) -> bool {
    let alloc = Allocator::default();
    let wrapped = format!("({source})");
    let parsed = oxc_parser::Parser::new(&alloc, &wrapped, SourceType::tsx()).parse();
    if parsed.panicked || !parsed.errors.is_empty() {
        return false;
    }
    let Some(Statement::ExpressionStatement(stmt)) = parsed.program.body.first() else {
        return false;
    };
    let mut scan = MemberRootScan {
        bindings,
        scopes,
        scope,
        found: false,
    };
    scan.visit_expr(&stmt.expression);
    scan.found
}

/// Walks an expression tree for a MEMBER access whose leftmost identifier resolves to a
/// declared binding (the `!is_pure(member)` `has_state` member rule). Does NOT descend
/// into nested function bodies (official sets `expression: null` there).
struct MemberRootScan<'a> {
    bindings: &'a BindingTable,
    scopes: &'a ScopeGraph,
    scope: ScopeId,
    found: bool,
}

impl MemberRootScan<'_> {
    /// Whether a member chain's leftmost identifier resolves to a declared binding (a
    /// non-global root). Mirrors `is_pure`'s leftmost-walk: a `Foo.bar.baz` roots at
    /// `Foo`; a binding root ⇒ impure ⇒ has_state.
    fn member_root_is_binding(&self, object: &Expression<'_>) -> bool {
        let mut node = object;
        loop {
            match node {
                Expression::StaticMemberExpression(m) => node = &m.object,
                Expression::ComputedMemberExpression(m) => node = &m.object,
                Expression::PrivateFieldExpression(m) => node = &m.object,
                Expression::ParenthesizedExpression(p) => node = &p.expression,
                Expression::TSNonNullExpression(e) => node = &e.expression,
                Expression::Identifier(id) => {
                    return self
                        .bindings
                        .resolve_kind(self.scopes, self.scope, id.name.as_str())
                        .is_some();
                }
                _ => return false,
            }
        }
    }

    fn visit_expr(&mut self, expr: &Expression<'_>) {
        if self.found {
            return;
        }
        match expr {
            Expression::StaticMemberExpression(m) => {
                if self.member_root_is_binding(&m.object) {
                    self.found = true;
                    return;
                }
                self.visit_expr(&m.object);
            }
            Expression::ComputedMemberExpression(m) => {
                if self.member_root_is_binding(&m.object) {
                    self.found = true;
                    return;
                }
                self.visit_expr(&m.object);
                self.visit_expr(&m.expression);
            }
            Expression::PrivateFieldExpression(m) => {
                if self.member_root_is_binding(&m.object) {
                    self.found = true;
                    return;
                }
                self.visit_expr(&m.object);
            }
            Expression::ChainExpression(chain) => self.visit_chain_element(&chain.expression),
            Expression::CallExpression(call) => {
                self.visit_expr(&call.callee);
                for arg in &call.arguments {
                    if let Some(e) = arg.as_expression() {
                        self.visit_expr(e);
                    } else if let oxc_ast::ast::Argument::SpreadElement(s) = arg {
                        self.visit_expr(&s.argument);
                    }
                }
            }
            Expression::NewExpression(n) => {
                self.visit_expr(&n.callee);
                for arg in &n.arguments {
                    if let Some(e) = arg.as_expression() {
                        self.visit_expr(e);
                    } else if let oxc_ast::ast::Argument::SpreadElement(s) = arg {
                        self.visit_expr(&s.argument);
                    }
                }
            }
            Expression::ParenthesizedExpression(p) => self.visit_expr(&p.expression),
            Expression::BinaryExpression(b) => {
                self.visit_expr(&b.left);
                self.visit_expr(&b.right);
            }
            Expression::LogicalExpression(l) => {
                self.visit_expr(&l.left);
                self.visit_expr(&l.right);
            }
            Expression::ConditionalExpression(c) => {
                self.visit_expr(&c.test);
                self.visit_expr(&c.consequent);
                self.visit_expr(&c.alternate);
            }
            Expression::SequenceExpression(s) => {
                for e in &s.expressions {
                    self.visit_expr(e);
                }
            }
            Expression::UnaryExpression(u) => self.visit_expr(&u.argument),
            Expression::AwaitExpression(a) => self.visit_expr(&a.argument),
            Expression::TemplateLiteral(t) => {
                for e in &t.expressions {
                    self.visit_expr(e);
                }
            }
            Expression::TaggedTemplateExpression(t) => {
                self.visit_expr(&t.tag);
                for e in &t.quasi.expressions {
                    self.visit_expr(e);
                }
            }
            Expression::ArrayExpression(arr) => {
                for el in &arr.elements {
                    if let oxc_ast::ast::ArrayExpressionElement::SpreadElement(s) = el {
                        self.visit_expr(&s.argument);
                    } else if let Some(e) = el.as_expression() {
                        self.visit_expr(e);
                    }
                }
            }
            Expression::ObjectExpression(obj) => {
                for prop in &obj.properties {
                    match prop {
                        oxc_ast::ast::ObjectPropertyKind::ObjectProperty(p) => {
                            if p.computed {
                                if let Some(key) = p.key.as_expression() {
                                    self.visit_expr(key);
                                }
                            }
                            self.visit_expr(&p.value);
                        }
                        oxc_ast::ast::ObjectPropertyKind::SpreadProperty(s) => {
                            self.visit_expr(&s.argument);
                        }
                    }
                }
            }
            Expression::TSAsExpression(e) => self.visit_expr(&e.expression),
            Expression::TSSatisfiesExpression(e) => self.visit_expr(&e.expression),
            Expression::TSNonNullExpression(e) => self.visit_expr(&e.expression),
            // A bare identifier / literal carries no member; nested function bodies are
            // not descended (official sets `expression: null` there).
            _ => {}
        }
    }

    fn visit_chain_element(&mut self, element: &ChainElement<'_>) {
        if self.found {
            return;
        }
        match element {
            ChainElement::CallExpression(call) => {
                self.visit_expr(&call.callee);
                for arg in &call.arguments {
                    if let Some(e) = arg.as_expression() {
                        self.visit_expr(e);
                    } else if let oxc_ast::ast::Argument::SpreadElement(s) = arg {
                        self.visit_expr(&s.argument);
                    }
                }
            }
            ChainElement::TSNonNullExpression(e) => self.visit_expr(&e.expression),
            ChainElement::StaticMemberExpression(m) => {
                if self.member_root_is_binding(&m.object) {
                    self.found = true;
                    return;
                }
                self.visit_expr(&m.object);
            }
            ChainElement::ComputedMemberExpression(m) => {
                if self.member_root_is_binding(&m.object) {
                    self.found = true;
                    return;
                }
                self.visit_expr(&m.object);
                self.visit_expr(&m.expression);
            }
            ChainElement::PrivateFieldExpression(m) => {
                if self.member_root_is_binding(&m.object) {
                    self.found = true;
                    return;
                }
                self.visit_expr(&m.object);
            }
        }
    }
}

/// A SOURCE-ORDER scan for an impure / stateful CALL inside a reactive-text
/// expression. It mirrors the official analyze-pass traversal: `dependencies` grows
/// as identifiers are visited (in AST-visit order), and each call's `has_call` check
/// runs AFTER its own children are visited, against the dependencies accumulated SO
/// FAR. Does NOT descend into nested function bodies (official sets `expression:
/// null` there, so their identifiers/calls do not participate).
struct HasCallScan<'a> {
    /// The component-declared root names (`is_pure` scope-resolution input).
    declared_roots: &'a rustc_hash::FxHashSet<String>,
    /// The binding table + scope graph + expression scope — the scope-aware
    /// resolution input for the per-identifier dependency accumulation (an identifier
    /// resolving to a binding row at `scope` is a dependency).
    bindings: &'a BindingTable,
    scopes: &'a ScopeGraph,
    scope: ScopeId,
    /// The dependency count accumulated SO FAR, in source/visit order — the official
    /// `context.state.expression.dependencies.size`. Every referenced binding (a
    /// local, a prop, a signal, a demoted `$state`, an import, a module/local
    /// function) contributes one, at its visit point.
    deps: usize,
    found: bool,
}

impl HasCallScan<'_> {
    /// Whether a callee is statically PURE (`is_pure`): the leftmost identifier is a
    /// GLOBAL (NOT a component-declared name). A callee rooted at a declared name (a
    /// local / module fn, an import, a signal) is impure.
    fn callee_is_pure(&self, callee: &Expression<'_>) -> bool {
        let mut node = callee;
        loop {
            match node {
                Expression::StaticMemberExpression(m) => node = &m.object,
                Expression::ComputedMemberExpression(m) => node = &m.object,
                Expression::PrivateFieldExpression(m) => node = &m.object,
                Expression::ParenthesizedExpression(p) => node = &p.expression,
                Expression::TSNonNullExpression(e) => node = &e.expression,
                // The leftmost identifier: a GLOBAL (not component-declared) is pure;
                // a declared name (local / module fn / import / signal) is impure.
                Expression::Identifier(id) => {
                    return !self.declared_roots.contains(id.name.as_str());
                }
                // A call as the callee root (`f()()`) is impure.
                Expression::CallExpression(_) => return false,
                // Any other leftmost form is not a pure global chain.
                _ => return false,
            }
        }
    }

    fn visit_expr(&mut self, expr: &Expression<'_>) {
        if self.found {
            return;
        }
        match expr {
            // An identifier in reference position: if it resolves (scope-awarely) to a
            // binding, it is a dependency (official `Identifier.js`:
            // `expression.dependencies.add(binding)` for EVERY resolved binding,
            // reactive or not). Accumulated at THIS visit point so a later call sees it.
            Expression::Identifier(id) => self.count_reference(id.name.as_str()),
            // A call: descend the callee + arguments FIRST (so their identifier
            // dependencies accumulate, matching official's `context.next()` preceding
            // the deps check), THEN apply the call rule against the deps-so-far.
            Expression::CallExpression(call) => self.visit_call(call),
            // An optional chain (`foo?.()`, `obj?.method(x)`, `a().b?.c()`). In OXC an
            // optional CALL is an `Expression::ChainExpression` wrapping
            // `ChainElement::CallExpression` — the bare-`CallExpression` arm never sees
            // it. Official `CallExpression.js` fires for an optional call exactly as for
            // a plain one (`is_pure(callee)` recurses on the callee regardless of
            // optionality), so the optional call carries the SAME source-order
            // `has_call` rule. A plain optional MEMBER (`c?.x`) is NOT a call and must
            // not trigger — the member arm only descends into the chain base.
            Expression::ChainExpression(chain) => self.visit_chain_element(&chain.expression),
            // A tagged template sets has_call only when its TAG is impure (official
            // `TaggedTemplateExpression.js`: `!is_pure(node.tag)`). A pure-global tag
            // (`String.raw`…) does not. The template's own `${…}` interpolations are
            // descended for nested calls regardless. (`is_pure` ignores deps-so-far
            // here — the tagged-template rule is tag-purity only.)
            Expression::TaggedTemplateExpression(t) => {
                if !self.callee_is_pure(&t.tag) {
                    self.found = true;
                    return;
                }
                for e in &t.quasi.expressions {
                    self.visit_expr(e);
                }
            }
            Expression::ParenthesizedExpression(p) => self.visit_expr(&p.expression),
            Expression::BinaryExpression(b) => {
                self.visit_expr(&b.left);
                self.visit_expr(&b.right);
            }
            Expression::LogicalExpression(l) => {
                self.visit_expr(&l.left);
                self.visit_expr(&l.right);
            }
            Expression::ConditionalExpression(c) => {
                self.visit_expr(&c.test);
                self.visit_expr(&c.consequent);
                self.visit_expr(&c.alternate);
            }
            Expression::SequenceExpression(s) => {
                for e in &s.expressions {
                    self.visit_expr(e);
                }
            }
            Expression::UnaryExpression(u) => self.visit_expr(&u.argument),
            Expression::AwaitExpression(a) => self.visit_expr(&a.argument),
            Expression::TemplateLiteral(t) => {
                for e in &t.expressions {
                    self.visit_expr(e);
                }
            }
            Expression::ArrayExpression(arr) => {
                for el in &arr.elements {
                    // An array SPREAD element (`[...x]`) unconditionally sets `has_call`
                    // (official `SpreadElement.js`); a plain element is descended.
                    if let oxc_ast::ast::ArrayExpressionElement::SpreadElement(s) = el {
                        self.visit_spread(&s.argument);
                    } else if let Some(e) = el.as_expression() {
                        self.visit_expr(e);
                    }
                }
            }
            Expression::ObjectExpression(obj) => {
                for prop in &obj.properties {
                    match prop {
                        oxc_ast::ast::ObjectPropertyKind::ObjectProperty(p) => {
                            // A COMPUTED key (`{ [v]: 1 }`) is an evaluated reference; a
                            // plain/literal key is not. The value is always visited.
                            if p.computed {
                                if let Some(key) = p.key.as_expression() {
                                    self.visit_expr(key);
                                }
                            }
                            self.visit_expr(&p.value);
                        }
                        // An object SPREAD property (`{...x}`) unconditionally sets
                        // `has_call` (official `SpreadElement.js`).
                        oxc_ast::ast::ObjectPropertyKind::SpreadProperty(s) => {
                            self.visit_spread(&s.argument);
                        }
                    }
                }
            }
            // A member access: descend the object (and a computed key) so its
            // identifier dependencies accumulate. A static `.prop` name is NOT a
            // reference. The member itself is not a call (official `MemberExpression.js`
            // never sets `has_call`).
            Expression::StaticMemberExpression(m) => self.visit_expr(&m.object),
            Expression::ComputedMemberExpression(m) => {
                self.visit_expr(&m.object);
                self.visit_expr(&m.expression);
            }
            Expression::TSAsExpression(e) => self.visit_expr(&e.expression),
            Expression::TSSatisfiesExpression(e) => self.visit_expr(&e.expression),
            Expression::TSNonNullExpression(e) => self.visit_expr(&e.expression),
            // A `new X()` does NOT itself set has_call (official `NewExpression.js`
            // only sets `needs_context`). Descend its callee + arguments so their
            // identifier dependencies accumulate (a `new Foo()` makes `Foo` a dep,
            // which a SUBSEQUENT call observes), and so a call nested in an argument is
            // still found.
            Expression::NewExpression(n) => {
                self.visit_expr(&n.callee);
                for arg in &n.arguments {
                    self.visit_argument(arg);
                }
            }
            // A nested function body is NOT descended (official sets `expression: null`
            // there, so its identifiers/calls do not participate). Literals carry no
            // dependency.
            _ => {}
        }
    }

    /// Count a referenced identifier as a dependency if it resolves (scope-awarely) to
    /// a binding row at the expression scope — the official `Identifier.js`
    /// `dependencies.add(binding)` for EVERY resolved binding (a local, a prop, a
    /// signal, a demoted `$state`, an import, a module/local function), reactive or
    /// not. A free name (a global like `Boolean` / `Math`) resolves to `None` and is
    /// not a dependency.
    fn count_reference(&mut self, name: &str) {
        if self
            .bindings
            .resolve_kind(self.scopes, self.scope, name)
            .is_some()
        {
            self.deps += 1;
        }
    }

    /// Apply the official `CallExpression` `has_call` rule to a call node, in SOURCE
    /// order (shared by the plain-call arm and the optional-chain-wrapped call). The
    /// callee + arguments are visited FIRST so their identifier dependencies
    /// accumulate (mirroring official's `context.next()` preceding the deps check) —
    /// the call's OWN callee/argument bindings count too. THEN the call sets has_call
    /// iff its callee is NOT statically pure OR a dependency has accumulated so far
    /// (`!is_pure(callee) || dependencies.size > 0`). Descending first also finds a
    /// nested impure call inside the callee or an argument.
    fn visit_call(&mut self, call: &CallExpression<'_>) {
        self.visit_expr(&call.callee);
        for arg in &call.arguments {
            self.visit_argument(arg);
        }
        if self.found {
            return;
        }
        if !self.callee_is_pure(&call.callee) || self.deps > 0 {
            self.found = true;
        }
    }

    /// Visit a call/new ARGUMENT — a plain expression OR a spread (`...x`). A spread
    /// element UNCONDITIONALLY sets `has_call` (official `SpreadElement.js`: `has_call =
    /// true; has_state = true` for ANY spread — "treat e.g. `[...x]` the same as
    /// `[...x.values()]`"), so a spread-call (`String(...arr)`, even over a pure global
    /// like `String(...globalThis.items)`) memoizes. Its inner expression is still
    /// descended so a nested impure call / dependency in the spread argument also
    /// participates.
    fn visit_argument(&mut self, arg: &oxc_ast::ast::Argument<'_>) {
        match arg {
            oxc_ast::ast::Argument::SpreadElement(s) => self.visit_spread(&s.argument),
            other => {
                if let Some(e) = other.as_expression() {
                    self.visit_expr(e);
                }
            }
        }
    }

    /// Visit a SPREAD element's inner expression and mark `has_call`. Official's
    /// `SpreadElement` visitor sets `has_call = true` (and `has_state = true`) for EVERY
    /// spread, regardless of what it spreads — a spread is treated as an implicit
    /// iterator call. Shared by the call/new-argument, array-element, and object-property
    /// spread positions.
    fn visit_spread(&mut self, argument: &Expression<'_>) {
        self.visit_expr(argument);
        self.found = true;
    }

    /// Scan one element of an optional chain. An optional CALL
    /// (`ChainElement::CallExpression`) carries the full call rule; a `!` assertion
    /// descends to its inner expression; a member element is NOT a call but its
    /// object is descended so a call nested in the chain base (`a().b?.c`) is still
    /// found.
    fn visit_chain_element(&mut self, element: &ChainElement<'_>) {
        if self.found {
            return;
        }
        match element {
            ChainElement::CallExpression(call) => self.visit_call(call),
            ChainElement::TSNonNullExpression(e) => self.visit_expr(&e.expression),
            ChainElement::StaticMemberExpression(m) => self.visit_expr(&m.object),
            ChainElement::ComputedMemberExpression(m) => {
                self.visit_expr(&m.object);
                self.visit_expr(&m.expression);
            }
            ChainElement::PrivateFieldExpression(m) => self.visit_expr(&m.object),
        }
    }
}

// ---------------------------------------------------------------------------
// `needs_context` — the `$.push`/`$.pop` component-context trigger
// ---------------------------------------------------------------------------

/// Whether the component needs a COMPONENT CONTEXT (`$.push($$props, true)` …
/// `$.pop()`) — the official `needs_context` analysis fact.
///
/// Mirrors the official analyze-pass `needs_context` triggers
/// (`phases/2-analyze/visitors/{NewExpression, CallExpression, MemberExpression}.js`):
/// the analyzed tree (the instance script body PLUS every template expression —
/// handlers, interpolations, binds) contains at least one of:
///
/// - a `new X()` expression (`NewExpression` always sets it);
/// - a NON-rune call whose callee is NOT a "safe identifier" — the callee's
///   leftmost identifier resolves to an UNSAFE binding (an `import`, a `$props()`
///   prop). A call to a LOCAL function or a GLOBAL (`console.log`, `Math.max`) is
///   safe;
/// - a member expression whose leftmost identifier resolves to such an unsafe
///   binding;
/// - a `$effect(...)` rune (Svelte needs the runes-mode context for it).
///
/// The "unsafe root names" (imports + prop names) come from the instance script;
/// the scan is scope-aware (a local shadowing an unsafe name is safe). This is the
/// SOLE `$.push` determinant — keying it on `$effect` alone would under-push for
/// imported / `new` / context calls.
#[must_use]
pub fn needs_context(
    alloc: &Allocator,
    instance_source: Option<&str>,
    template_expr_sources: &[&str],
) -> bool {
    let Some(instance) = instance_source else {
        // No instance script ⇒ no imports/props ⇒ a `new`/call in a template
        // handler is the only possible trigger, but with no unsafe roots a call is
        // safe; a `new X()` in a template handler still triggers. Scan the template
        // expressions with an EMPTY unsafe-root set.
        return template_expr_sources
            .iter()
            .any(|src| expr_needs_context(alloc, src, &rustc_hash::FxHashSet::default()));
    };
    let Some(program) = reparse_module(alloc, instance) else {
        return false;
    };
    // The UNSAFE root names: top-level `import` bindings + `$props()` destructure
    // names. A call/member rooted at one of these is unsafe (needs context).
    let mut unsafe_roots: rustc_hash::FxHashSet<String> = rustc_hash::FxHashSet::default();
    collect_unsafe_root_names(&program, &mut unsafe_roots);

    // Scan the instance program itself.
    let mut scan = NeedsContextScan {
        found: false,
        unsafe_roots: &unsafe_roots,
        scopes: ShadowStack::default(),
    };
    scan.visit_program(&program);
    if scan.found {
        return true;
    }
    // Scan every template expression (handler / interpolation / bind) with the same
    // unsafe-root set (a handler `onclick={() => f(count)}` reads the instance
    // import `f`). Each is wrapped so it parses as a statement.
    template_expr_sources
        .iter()
        .any(|src| expr_needs_context(alloc, src, &unsafe_roots))
}

/// Whether a single (wrapped) template expression triggers `needs_context` under
/// the given unsafe-root set.
fn expr_needs_context(
    alloc: &Allocator,
    expr_src: &str,
    unsafe_roots: &rustc_hash::FxHashSet<String>,
) -> bool {
    let wrapped = format!("({expr_src});");
    let Some(program) = reparse_module(alloc, &wrapped) else {
        return false;
    };
    let mut scan = NeedsContextScan {
        found: false,
        unsafe_roots,
        scopes: ShadowStack::default(),
    };
    scan.visit_program(&program);
    scan.found
}

/// Collect the UNSAFE top-level root names of an instance program: every `import`
/// binding name (default / named / namespace) and every `$props()` destructure
/// name. A call / member rooted at one of these is unsafe (`is_safe_identifier`
/// returns false for an `import` / `prop` binding).
fn collect_unsafe_root_names(program: &Program<'_>, out: &mut rustc_hash::FxHashSet<String>) {
    for stmt in &program.body {
        match stmt {
            Statement::ImportDeclaration(import) => {
                if let Some(specifiers) = &import.specifiers {
                    for spec in specifiers {
                        out.insert(spec.local().name.to_string());
                    }
                }
            }
            Statement::VariableDeclaration(decl) => {
                for d in &decl.declarations {
                    let Some(Expression::CallExpression(call)) = &d.init else {
                        continue;
                    };
                    if is_props_callee(&call.callee) {
                        let mut names = Vec::new();
                        collect_pattern_names(&d.id, &mut names);
                        out.extend(names);
                    }
                }
            }
            _ => {}
        }
    }
}

/// A scope-aware scan for the `needs_context` triggers (`new X()` / an unsafe call
/// / an unsafe member / `$effect`). It tracks the SAME lexical `ShadowStack` model
/// as the other syntax-side collectors, so a LOCAL shadowing an unsafe root name is
/// safe (its references do not trigger context).
struct NeedsContextScan<'a> {
    found: bool,
    unsafe_roots: &'a rustc_hash::FxHashSet<String>,
    scopes: ShadowStack,
}

impl NeedsContextScan<'_> {
    /// Whether `name` is an unsafe root that is NOT shadowed by a local.
    fn is_unsafe_root(&self, name: &str) -> bool {
        self.unsafe_roots.contains(name) && !self.scopes.is_shadowed(name)
    }

    /// Whether a callee / member expression is UNSAFE (so the call / member triggers
    /// `needs_context`) — the official `is_safe_identifier` rule. Walks the member chain
    /// (peeling transparent paren / TS-non-null skins, the OXC equivalent of estree's
    /// paren-elision / TS-stripping) to the LEFTMOST leaf and:
    /// - a NON-identifier leftmost leaf (an object literal `{x:1}.m`, an array, a call
    ///   result, a template, `this`) is UNSAFE — the official `if (node.type !==
    ///   'Identifier') return false`;
    /// - an identifier leftmost leaf is unsafe only when it roots at an unsafe binding
    ///   (an `import` / `$props()` destructure name) — a local / global / safe binding is
    ///   safe.
    fn root_is_unsafe(&self, expr: &Expression<'_>) -> bool {
        let mut node = expr;
        loop {
            match node {
                Expression::StaticMemberExpression(m) => node = &m.object,
                Expression::ComputedMemberExpression(m) => node = &m.object,
                Expression::PrivateFieldExpression(m) => node = &m.object,
                Expression::ParenthesizedExpression(p) => node = &p.expression,
                Expression::TSNonNullExpression(e) => node = &e.expression,
                Expression::Identifier(id) => return self.is_unsafe_root(id.name.as_str()),
                // A member / call rooted at a NON-identifier leaf (an object literal, an
                // array, a call result, a template, `this`) is not a safe identifier — the
                // official `is_safe_identifier` returns `false` for it (so `{x:1}.m`,
                // `{x:1}['m']`, `{f(){}}.f()` all need component context).
                _ => return true,
            }
        }
    }
}

impl<'a> Visit<'a> for NeedsContextScan<'_> {
    fn visit_program(&mut self, it: &Program<'a>) {
        let mut frame = rustc_hash::FxHashSet::default();
        collect_direct_decls(&it.body, &mut frame);
        collect_var_hoists(&it.body, &mut frame);
        // The UNSAFE root bindings (the `$props()` destructure names) are declared at
        // THIS program scope — they must NOT shadow THEMSELVES. Without this, a
        // top-level `let { cb } = $props(); cb()` would treat the prop read `cb` as
        // shadowed by its own declaration frame and miss `needs_context` (the
        // self-shadow bug). A NESTED local of the same name (an arrow param `cb`)
        // still shadows — only the program-scope self-shadow is removed. (This
        // mirrors `ScriptUseCollector::visit_program`, which removes its tracked
        // names from the program frame for the same reason.)
        for root in self.unsafe_roots {
            frame.remove(root);
        }
        self.scopes.push(frame);
        walk::walk_program(self, it);
        self.scopes.pop();
    }

    fn visit_function(&mut self, it: &Function<'a>, flags: oxc_syntax::scope::ScopeFlags) {
        self.scopes.push(function_scope_names(it));
        walk::walk_function(self, it, flags);
        self.scopes.pop();
    }

    fn visit_arrow_function_expression(&mut self, it: &ArrowFunctionExpression<'a>) {
        self.scopes.push(arrow_scope_names(it));
        walk::walk_arrow_function_expression(self, it);
        self.scopes.pop();
    }

    fn visit_block_statement(&mut self, it: &BlockStatement<'a>) {
        self.scopes.push(block_scope_names(it));
        walk::walk_block_statement(self, it);
        self.scopes.pop();
    }

    fn visit_catch_clause(&mut self, it: &CatchClause<'a>) {
        let mut frame = rustc_hash::FxHashSet::default();
        if let Some(param) = &it.param {
            let mut names = Vec::new();
            collect_pattern_names(&param.pattern, &mut names);
            frame.extend(names);
        }
        self.scopes.push(frame);
        walk::walk_catch_clause(self, it);
        self.scopes.pop();
    }

    fn visit_for_statement(&mut self, it: &ForStatement<'a>) {
        let mut frame = rustc_hash::FxHashSet::default();
        if let Some(oxc_ast::ast::ForStatementInit::VariableDeclaration(decl)) = &it.init {
            if !matches!(decl.kind, VariableDeclarationKind::Var) {
                for d in &decl.declarations {
                    let mut names = Vec::new();
                    collect_pattern_names(&d.id, &mut names);
                    frame.extend(names);
                }
            }
        }
        self.scopes.push(frame);
        walk::walk_for_statement(self, it);
        self.scopes.pop();
    }

    fn visit_for_of_statement(&mut self, it: &ForOfStatement<'a>) {
        self.scopes.push(for_left_names(&it.left));
        walk::walk_for_of_statement(self, it);
        self.scopes.pop();
    }

    fn visit_for_in_statement(&mut self, it: &ForInStatement<'a>) {
        self.scopes.push(for_left_names(&it.left));
        walk::walk_for_in_statement(self, it);
        self.scopes.pop();
    }

    fn visit_new_expression(&mut self, it: &oxc_ast::ast::NewExpression<'a>) {
        // A `new X()` ALWAYS sets needs_context.
        self.found = true;
        walk::walk_new_expression(self, it);
    }

    fn visit_call_expression(&mut self, it: &CallExpression<'a>) {
        // A `$effect(...)` rune (unshadowed) needs the runes-mode context.
        if is_effect_callee(&it.callee) && !self.scopes.is_shadowed("$effect") {
            self.found = true;
        } else if !is_rune_call(it) && self.root_is_unsafe(&it.callee) {
            // A NON-rune call whose callee roots at an unsafe binding (import / prop)
            // is unsafe. A rune call (`$state`/`$derived`/`$props`/…) is never an
            // unsafe runtime call here.
            self.found = true;
        }
        walk::walk_call_expression(self, it);
    }

    fn visit_static_member_expression(&mut self, it: &oxc_ast::ast::StaticMemberExpression<'a>) {
        if self.root_is_unsafe(&it.object) {
            self.found = true;
        }
        walk::walk_static_member_expression(self, it);
    }

    fn visit_computed_member_expression(
        &mut self,
        it: &oxc_ast::ast::ComputedMemberExpression<'a>,
    ) {
        if self.root_is_unsafe(&it.object) {
            self.found = true;
        }
        walk::walk_computed_member_expression(self, it);
    }
}

/// Whether a call's callee is a Svelte rune root (`$state` / `$derived` / `$props`
/// / `$effect` / `$bindable` / `$inspect` / `$host` / a `$rune.member`) — a rune
/// call is never an unsafe RUNTIME call for the `needs_context` member/call gate
/// (its own rune handling decides context). A SHADOWED rune name is a normal local
/// and is treated as a non-rune call.
fn is_rune_call(call: &CallExpression<'_>) -> bool {
    let root = match &call.callee {
        Expression::Identifier(id) => id.name.as_str(),
        Expression::StaticMemberExpression(m) => match &m.object {
            Expression::Identifier(id) => id.name.as_str(),
            _ => return false,
        },
        _ => return false,
    };
    matches!(
        root,
        "$state" | "$derived" | "$props" | "$effect" | "$bindable" | "$inspect" | "$host"
    )
}

#[cfg(test)]
#[path = "reactive_analysis_tests.rs"]
mod tests;
