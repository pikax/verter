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
    ArrowFunctionExpression, BlockStatement, CallExpression, CatchClause, Expression,
    ForInStatement, ForOfStatement, ForStatement, Function, Program, Statement,
    VariableDeclarationKind,
};
use oxc_ast_visit::{walk, Visit};
use oxc_span::SourceType;

use super::expr::{
    arrow_scope_names, block_scope_names, collect_direct_decls, collect_pattern_names,
    collect_var_hoists, for_left_names, function_scope_names, is_effect_callee, is_props_callee,
    reparse_module, BindingTable, ScopeGraph, ScopeId, ShadowStack,
};
use super::expr_rewrite::is_signal_kind;

// ---------------------------------------------------------------------------
// `has_call` — the reactive-text memoizer trigger
// ---------------------------------------------------------------------------

/// Whether a reactive-text expression `has_call` — the official metadata fact that
/// drives the MEMOIZED `$.template_effect(($0) => …, [() => expr])` deps-array form
/// (vs the inline `$.set_text(text, expr)`).
///
/// Mirrors `phases/2-analyze/visitors/CallExpression.js`: a call sets `has_call`
/// iff its callee is NOT statically pure (`is_pure(callee)`) OR the expression
/// carries a stateful dependency. `is_pure(callee)` is true only when the callee's
/// leftmost identifier resolves to a GLOBAL (no binding) — a callee rooted at a
/// declared binding (a local function, an import) is impure. A
/// TaggedTemplateExpression is always `has_call`. A bare signal read / member access
/// with NO call is NOT `has_call` (it stays the inline `$.set_text` form). The scan
/// does NOT descend into nested function bodies (a handler arrow inside an
/// interpolation is not the evaluated reactive value).
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
    // Whether the expression references ANY reactive signal (the `deps > 0`
    // approximation: a stateful dependency forces memoization of a call chunk).
    let references_signal = expr_references_signal(source, scope, bindings, scopes);
    let mut scan = HasCallScan {
        declared_roots,
        references_signal,
        found: false,
    };
    scan.visit_expr(inner);
    scan.found
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

/// Whether a reactive-text expression references any reactive SIGNAL binding (a
/// read resolving to a signal at `scope`). Drives the `deps > 0` half of the
/// `has_call` rule. Reuses the shared free-reference collector + the scope
/// resolver, so a shadowing local is not counted.
fn expr_references_signal(
    source: &str,
    scope: ScopeId,
    bindings: &BindingTable,
    scopes: &ScopeGraph,
) -> bool {
    let Ok(refs) = super::expr::collect_expr_references(source) else {
        return false;
    };
    refs.iter().any(|r| {
        bindings
            .resolve_kind(scopes, scope, &r.name)
            .is_some_and(is_signal_kind)
    })
}

/// A scan for an impure / stateful CALL inside a reactive-text expression. Does NOT
/// descend into nested function bodies (their calls are not part of the evaluated
/// reactive value).
struct HasCallScan<'a> {
    /// The component-declared root names (`is_pure` scope-resolution input).
    declared_roots: &'a rustc_hash::FxHashSet<String>,
    references_signal: bool,
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
            // A call sets has_call iff impure callee OR a stateful dependency.
            Expression::CallExpression(call) => {
                if !self.callee_is_pure(&call.callee) || self.references_signal {
                    self.found = true;
                    return;
                }
                self.visit_expr(&call.callee);
                for arg in &call.arguments {
                    if let Some(e) = arg.as_expression() {
                        self.visit_expr(e);
                    }
                }
            }
            // A tagged template is always has_call.
            Expression::TaggedTemplateExpression(_) => self.found = true,
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
                    if let Some(e) = el.as_expression() {
                        self.visit_expr(e);
                    }
                }
            }
            Expression::ObjectExpression(obj) => {
                for prop in &obj.properties {
                    if let oxc_ast::ast::ObjectPropertyKind::ObjectProperty(p) = prop {
                        self.visit_expr(&p.value);
                    }
                }
            }
            Expression::StaticMemberExpression(m) => self.visit_expr(&m.object),
            Expression::ComputedMemberExpression(m) => {
                self.visit_expr(&m.object);
                self.visit_expr(&m.expression);
            }
            Expression::TSAsExpression(e) => self.visit_expr(&e.expression),
            Expression::TSSatisfiesExpression(e) => self.visit_expr(&e.expression),
            Expression::TSNonNullExpression(e) => self.visit_expr(&e.expression),
            // A `new X()` is a constructor call (impure unless the constructor roots
            // at a global) — matches `is_pure` (a NewExpression is not in the pure
            // set, so it triggers has_call when stateful or non-global).
            Expression::NewExpression(n)
                if !self.callee_is_pure(&n.callee) || self.references_signal =>
            {
                self.found = true;
            }
            // A nested function body is NOT descended (its calls are not the
            // reactive value). Literals / identifiers carry no call.
            _ => {}
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

    /// Whether a callee / member expression's leftmost identifier is an unsafe root
    /// (so the call / member triggers `needs_context`). A leftmost identifier that
    /// is a local / global / safe binding does NOT trigger.
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
                _ => return false,
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
