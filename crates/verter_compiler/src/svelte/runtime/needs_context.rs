//! The `needs_context` component-context analysis for the Svelte client emitter —
//! the official analysis fact that drives the `$.push($$props, true)` … `$.pop()`
//! component-context plumbing. Read-only (it neither rewrites a read nor emits JS);
//! scope-aware via the shared lexical [`ShadowStack`] model in [`super::expr`];
//! driven purely from the OXC AST + the collected unsafe-root names — never a
//! source-text scan. Extracted from `reactive_analysis.rs` (the file-size guard
//! boundary); the reactive-text `has_call` / `has_state` analyses stay there.

use oxc_allocator::Allocator;
use oxc_ast::ast::{
    ArrowFunctionExpression, BlockStatement, CallExpression, CatchClause, ChainElement, Expression,
    ForInStatement, ForOfStatement, ForStatement, Function, Program, Statement,
    VariableDeclarationKind,
};
use oxc_ast_visit::{walk, Visit};

use super::expr::{
    arrow_scope_names, block_scope_names, collect_direct_decls, collect_pattern_names,
    collect_var_hoists, for_left_names, function_scope_names, is_props_callee,
    is_user_effect_family_call, reparse_module, ShadowStack,
};

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
/// - a USER-effect rune call — `$effect(...)` or `$effect.pre(...)` (Svelte needs
///   the runes-mode context for a user effect); `$effect.root(...)` /
///   `$effect.tracking()` alone do NOT trigger (oracle-verified: root/tracking
///   never force the frame — only a nested user effect inside them does);
/// - a `$inspect(...).with(...)` chain (unshadowed `$inspect`) — the elided
///   `.with` statement still forces the official production frame
///   (`$.push($$props, true)` / `$.pop()` + the `$$props` param); plain
///   `$inspect(x)` / `$inspect.trace()` never trigger.
///
/// The "unsafe root names" (imports + prop names) come from the instance script
/// PLUS the `<script module>` import locals (module imports resolve up the lexical
/// chain, so a member/call rooted at one is exactly as unsafe as an instance
/// import — oracle: a module-slot `{NS.z}` frames with `$.push($$props, true)`);
/// the scan is scope-aware (a local shadowing an unsafe name is safe). This is the
/// SOLE `$.push` determinant — keying it on `$effect` alone would under-push for
/// imported / `new` / context calls.
///
/// `render_callee_sources` are the `{@render}` DYNAMIC-callee expressions (each
/// the whole inner snippet call, `callee(args)`): the OUTER snippet call itself
/// is NOT an unsafe-call trigger (official excludes the render call — a
/// prop-rooted `{@render children?.()}` stays frame-free), so each is PEELED to
/// its callee and only the callee expression is scanned
/// ([`render_callee_needs_context`]). Render ARGUMENTS are separate analyzed
/// expressions that ride `template_expr_sources` — they are never exempted.
#[must_use]
pub fn needs_context(
    alloc: &Allocator,
    instance_source: Option<&str>,
    module_source: Option<&str>,
    template_expr_sources: &[&str],
    render_callee_sources: &[&str],
) -> bool {
    // The UNSAFE root names: top-level `import` bindings (instance AND module
    // slots — module imports resolve up the lexical chain into template
    // expressions) + `$props()` destructure names. A call/member rooted at one of
    // these is unsafe (needs context). The MODULE program contributes ROOTS only —
    // module statements run at module scope, outside the component context, so
    // they are never scanned for triggers themselves.
    let mut unsafe_roots: rustc_hash::FxHashSet<String> = rustc_hash::FxHashSet::default();
    if let Some(module) = module_source {
        if let Some(program) = reparse_module(alloc, module) {
            collect_unsafe_root_names(&program, &mut unsafe_roots);
        }
    }
    if let Some(instance) = instance_source {
        let Some(program) = reparse_module(alloc, instance) else {
            return false;
        };
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
    }
    // Scan every template expression (handler / interpolation / bind) with the same
    // unsafe-root set (a handler `onclick={() => f(count)}` reads the instance
    // import `f`; an interpolation `{NS.z}` reads the module import `NS`). Each is
    // wrapped so it parses as a statement. A `{@render}` dynamic callee scans
    // through the peel instead (only its callee expression).
    template_expr_sources
        .iter()
        .any(|src| expr_needs_context(alloc, src, &unsafe_roots))
        || render_callee_sources
            .iter()
            .any(|src| render_callee_needs_context(alloc, src, &unsafe_roots))
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

/// Whether a `{@render}` DYNAMIC-callee expression (the whole inner snippet
/// call, `callee(args)`) triggers `needs_context` under the given unsafe-root
/// set.
///
/// The OUTER snippet call is NOT an unsafe-call trigger — official excludes the
/// render call itself from the `is_safe_identifier` call check, so a prop-rooted
/// `{@render children?.()}` / `{@render (cond ? a : b)()}` stays frame-free —
/// but the CALLEE expression inside it scans NORMALLY: the terminal call is
/// peeled (an AST descent through transparent parens and the optional-chain
/// wrapper to `CallExpression.callee` — never a text slice) and only the peeled
/// callee is visited. A member/call/`new`-rooted callee (`$host().snip`,
/// `imported.snip`, `unsafeImport()`, `(new Date())`, `({ snip(){} }).snip`)
/// still opens the frame; an identifier / safe-local-rooted callee stays safe.
/// A source with no terminal call scans whole — the conservative fallback
/// (unreachable for an emitted render: the projection refuses an uncalled
/// render before the plan consults this scan).
fn render_callee_needs_context(
    alloc: &Allocator,
    expr_src: &str,
    unsafe_roots: &rustc_hash::FxHashSet<String>,
) -> bool {
    let wrapped = format!("({expr_src});");
    let Some(program) = reparse_module(alloc, &wrapped) else {
        return false;
    };
    let Some(Statement::ExpressionStatement(stmt)) = program.body.first() else {
        return false;
    };
    let mut expr = &stmt.expression;
    while let Expression::ParenthesizedExpression(p) = expr {
        expr = &p.expression;
    }
    // Peel the terminal snippet call: a plain `CallExpression` or the
    // `CallExpression` inside a `ChainExpression` (the optional `fn?.()` form).
    let scanned: &Expression = match expr {
        Expression::CallExpression(call) => &call.callee,
        Expression::ChainExpression(chain) => match &chain.expression {
            ChainElement::CallExpression(call) => &call.callee,
            _ => expr,
        },
        _ => expr,
    };
    let mut scan = NeedsContextScan {
        found: false,
        unsafe_roots,
        scopes: ShadowStack::default(),
    };
    scan.visit_expression(scanned);
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
        // A USER-effect rune call — `$effect(...)` / `$effect.pre(...)`
        // (unshadowed) — needs the runes-mode context; `$effect.root` /
        // `$effect.tracking` are excluded by the shared family classifier's
        // kind match (oracle-verified: they never force the frame on their
        // own).
        if is_user_effect_family_call(it) && !self.scopes.is_shadowed("$effect") {
            self.found = true;
        } else if is_bindable_call_expression(it) && !self.scopes.is_shadowed("$bindable") {
            // A `$bindable(...)` call (unshadowed) — official sets `needs_context`
            // on the call itself (purely syntactic presence in the instance
            // script), so EVERY bindable component gains the frame — including
            // the read-only-no-default case that emits no `$.prop` at all.
            self.found = true;
        } else if !is_rune_call(it) && self.root_is_unsafe(&it.callee) {
            // A NON-rune call whose callee roots at an unsafe binding (import / prop)
            // is unsafe. A rune call (`$state`/`$derived`/`$props`/…) is never an
            // unsafe runtime call here. This arm fires for the
            // `$inspect(...).with(...)` chain, which FORCES the component frame in
            // official production output (`App($$anchor, $$props)` +
            // `$.push($$props, true)` / `$.pop()`) even though the statement itself
            // is elided: the `.with` callee's OBJECT is a CallExpression (not an
            // identifier), so the chain is not a rune call and its call-result root
            // is unsafe — shadowed or not, matching the official
            // `is_safe_identifier` semantics. (The `.with` member is ALSO reached by
            // `visit_static_member_expression` below, whose `root_is_unsafe(&object)`
            // independently forces the frame — the coverage is redundant by design, so
            // removing either path preserves the frame.) Plain `$inspect(x)` and
            // `$inspect.trace()` are rune calls and never trigger context.
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

/// Whether a call's callee is the BARE `$bindable` identifier — the
/// `needs_context` trigger arm's callee match (shadowing is the CALLER's check,
/// through the shared scope stack).
fn is_bindable_call_expression(call: &CallExpression<'_>) -> bool {
    matches!(&call.callee, Expression::Identifier(id) if id.name.as_str() == "$bindable")
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
#[path = "needs_context_tests.rs"]
mod tests;
