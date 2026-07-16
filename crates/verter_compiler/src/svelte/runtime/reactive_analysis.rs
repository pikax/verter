//! Reactive-text ANALYSIS for the Svelte client emitter — read-only facts the
//! client backend consults to match the official emission topology (none rewrites
//! a read or emits JS):
//!
//! - [`expr_has_call`] — the official `has_call` metadata fact that drives the
//!   MEMOIZED `$.template_effect(($0) => …, [() => expr])` deps-array form for a
//!   reactive-text chunk (vs the inline `$.set_text(text, expr)`).
//! - [`expr_references_signal`] / [`prop_value_has_state`] — the official
//!   `has_state` facts for dynamic attributes and component-prop values.
//!
//! All are scope-aware (they reuse the shared lexical `ShadowStack` model in
//! [`super::expr`]) and drive purely from the OXC AST + the binding table — never a
//! source-text scan. The sibling `needs_context` analysis (the `$.push($$props,
//! true)` … `$.pop()` component-context trigger) lives in
//! [`super::needs_context`].

use oxc_allocator::Allocator;
use oxc_ast::ast::{CallExpression, ChainElement, Expression, Statement};
use oxc_span::SourceType;

use super::expr::{
    is_signal_kind, BindingRuntimeKind, BindingTable, ScopeGraph, ScopeId, UnwrappedRootKind,
};

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
/// FAIL-CLOSED recovery contract: this analysis re-derives its facts by
/// re-walking the expression source (the scope-aware source-order rule cannot
/// be finalized at canonical-parse time — binding kinds finalize after
/// lowering); a recovery failure (the source fails to parse as an
/// expression) returns `Err(())` so the caller surfaces a PRECISE unsupported
/// diagnostic — never a silent `false` that would degrade a `BuildExpression`
/// surface to raw.
pub fn expr_has_call(
    source: &str,
    scope: ScopeId,
    bindings: &BindingTable,
    scopes: &ScopeGraph,
    declared_roots: &rustc_hash::FxHashSet<String>,
) -> Result<bool, ()> {
    let alloc = Allocator::default();
    let wrapped = format!("({source})");
    let parsed = oxc_parser::Parser::new(&alloc, &wrapped, SourceType::tsx()).parse();
    if parsed.panicked || !parsed.errors.is_empty() {
        return Err(());
    }
    let Some(Statement::ExpressionStatement(stmt)) = parsed.program.body.first() else {
        return Err(());
    };
    let inner = match &stmt.expression {
        Expression::ParenthesizedExpression(p) => &p.expression,
        other => other,
    };
    Ok(expr_has_call_parsed(
        inner,
        scope,
        bindings,
        scopes,
        declared_roots,
    ))
}

/// Retained-AST form of [`expr_has_call`]. The lowering pipeline uses this
/// entry point so final binding kinds can be applied without reparsing the
/// authored expression.
#[must_use]
pub(super) fn expr_has_call_parsed(
    expression: &Expression<'_>,
    scope: ScopeId,
    bindings: &BindingTable,
    scopes: &ScopeGraph,
    declared_roots: &rustc_hash::FxHashSet<String>,
) -> bool {
    let mut scan = HasCallScan {
        declared_roots,
        bindings,
        scopes,
        scope,
        deps: 0,
        found: false,
    };
    scan.visit_expr(expression);
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

/// Whether a template expression references any reactive STATE binding — a read
/// resolving (scope-awarely) to either a reactive `$state` signal OR a `$props()`
/// prop OR an `$effect.tracking()` const at `scope`. This is the official
/// `metadata.expression.has_state` signal: a dynamic attribute / class / style
/// value with `has_state` joins the combined `$.template_effect`; a value with no
/// reactive read is a one-shot init (the `has_state ? update : init` split in
/// `RegularElement.js`). A PROP read counts as state because props are reactive
/// (`$$props.x` can change), matching the text-interpolation reactivity
/// classifier (`PropRead` is reactive) and official. An
/// `EffectTrackingConst` read counts as state because official cannot
/// static-fold a call-init const (`Identifier.js`: `!scope.evaluate(node)
/// .is_known` sets `has_state`), so `disabled={t}` joins the template effect
/// while still reading the const PLAIN (never `$.get`) — the same disposition
/// the text path (`PlainLiveIdentRead`) already has. An IMPORTED local
/// (`ImportedValue` / `ComponentImport`) counts as state for the same
/// `!is_known` reason (imports are live bindings): `disabled={x}` from an
/// import joins the `$.template_effect`, read plain (oracle-verified against
/// svelte@5.56.3). It also drives the
/// `deps > 0` half of the reactive-text `has_call` memoize rule. Reuses the
/// shared free-reference collector + the scope resolver, so a shadowing local is
/// not counted; a prop / tracking const / import is NOT a signal (it emits
/// `$$props.x` / a plain read, not `$.get`), so [`is_signal_kind`] stays free of
/// them.
/// Consumes the expression's STORED reference facts (populated once by the
/// canonical analysis parse) — no reparse, no fail-open recovery.
#[must_use]
pub(super) fn expr_references_signal(
    references: &[super::expr::ExprReference],
    scope: ScopeId,
    bindings: &BindingTable,
    scopes: &ScopeGraph,
) -> bool {
    references.iter().any(|r| {
        bindings
            .resolve_kind(scopes, scope, &r.name)
            .is_some_and(|k| {
                is_signal_kind(k)
                    || k == BindingRuntimeKind::Prop
                    || k == BindingRuntimeKind::BindableProp
                    || k == BindingRuntimeKind::EffectTrackingConst
                    || k == BindingRuntimeKind::PropsIdConst
                    || super::expr::is_import_binding(k)
            })
    })
}

/// Whether a COMPONENT-PROP value expression `has_state` — the official
/// `metadata.expression.has_state` for the `Component.js` / `SvelteBoundary.js`
/// getter-vs-init decision (`has_state ? get name() {…} : name: value`). DISTINCT
/// from [`expr_references_signal`] in three ways:
///
/// 1. SYNCHRONOUS-only: a read INSIDE a nested function / arrow body is DEFERRED and
///    does NOT count, so `onclick={() => x}` is a plain prop init (`has_state = false`)
///    while `b={x}` / `depth={depth - 1}` are reactive.
/// 2. A `{#snippet}` NAME reference counts as state (a snippet passed as a prop emits
///    the getter `get tmpl() { return tmpl; }`, matching the pinned svelte@5.56.3
///    snippet-prop shape).
/// 3. It includes the BINDING-IMPURITY half ([`expr_has_binding_impurity`], official
///    `MemberExpression.js`'s `!is_pure` plus the mutation rule): a member rooted at
///    ANY declared binding — a plain local, a deep-proxied `$state` object, a prop — is
///    state-bearing (⇒ getter), so `failed={obj.failed}` / `x={obj.y}` emit `get name()
///    { return obj.…; }` even though `obj` itself is neither a signal nor a prop; AND an
///    assignment/update MUTATION whose write TARGET is rooted at a binding
///    (`failed={obj.x = 1}` / `failed={plain++}`) is impure (a write ⇒ getter) — the scan
///    covers binding-rooted mutations, not only member reads. A write to a GLOBAL /
///    undeclared target (`failed={globalThis.x = 1}` / `failed={foo = 1}`) stays a plain
///    init. The scan does NOT descend into nested function bodies, so `{() => obj.x}`
///    stays a plain init (rule 1 is preserved). Verified against pinned svelte@5.56.3
///    (component + boundary emit identically).
///
/// Consumes the expression's STORED reference facts for the synchronous-read
/// half (no reparse); the binding-impurity half still re-walks `source`
/// (scope-aware member/mutation structure) and FAILS CLOSED — `Err(())` on a
/// recovery failure, so the caller surfaces a precise diagnostic instead of a
/// silent plain-init downgrade.
pub(super) fn prop_value_has_state(
    references: &[super::expr::ExprReference],
    source: &str,
    scope: ScopeId,
    bindings: &BindingTable,
    scopes: &ScopeGraph,
) -> Result<bool, ()> {
    let sync_state = references.iter().any(|r| {
        !r.in_function
            && bindings
                .resolve_kind(scopes, scope, &r.name)
                .is_some_and(|k| {
                    is_signal_kind(k)
                        || k == BindingRuntimeKind::Prop
                        || k == BindingRuntimeKind::BindableProp
                        || k == BindingRuntimeKind::SnippetName
                        // An `$effect.tracking()` / `$props.id()` const: a
                        // call-init const is not statically known, so official
                        // marks its read `has_state` (the same rule that puts
                        // `disabled={t}` in the template effect) — a
                        // component-prop value reading it emits the getter form.
                        || k == BindingRuntimeKind::EffectTrackingConst
                        || k == BindingRuntimeKind::PropsIdConst
                        // An IMPORTED local is a live binding — not statically
                        // known, so a component-prop value reading it emits the
                        // getter form (`get name() { return x; }`).
                        || super::expr::is_import_binding(k)
                })
    });
    if sync_state {
        return Ok(true);
    }
    expr_has_binding_impurity(source, scope, bindings, scopes)
}

/// Whether a template expression carries a BINDING IMPURITY — a MEMBER access whose
/// leftmost identifier resolves (scope-awarely) to a declared binding (the official
/// `phases/2-analyze/visitors/MemberExpression.js` `has_state ||= !is_pure(node)` rule)
/// OR an assignment/update MUTATION (the write half of `has_state`).
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
///
/// The scan ALSO reports the MUTATION half of `has_state`: an assignment/update whose
/// write TARGET LEAF is rooted at a declared binding is impure (a write), so `plain = 1` /
/// `obj.x = 1` / `obj.x++` are `has_state` even when the target is a plain local binding
/// (not a live signal). A write to a GLOBAL / undeclared leaf (`globalThis.x = 1` /
/// `foo = 1`) is pure ⇒ NOT `has_state`, though a binding member appearing in an EVALUATED
/// read position of the same expression is still reported — the RHS (`globalThis.x = obj.y`),
/// an evaluated LHS computed key (`globalThis[obj.y] = 1`), or a destructuring default /
/// computed key (`[foo = obj.y] = g` / `({ [obj.y]: foo } = g)`). Function bodies are not
/// descended, so a mutation deferred inside `{() => plain = 1}` stays a plain init.
/// FAIL-CLOSED recovery contract: the member/mutation structure is re-walked
/// from `source` (scope-aware, not finalizable at canonical-parse time); a
/// recovery failure returns `Err(())` so the caller surfaces a precise
/// diagnostic — never a silent `false`.
pub(super) fn expr_has_binding_impurity(
    source: &str,
    scope: ScopeId,
    bindings: &BindingTable,
    scopes: &ScopeGraph,
) -> Result<bool, ()> {
    let alloc = Allocator::default();
    let wrapped = format!("({source})");
    let parsed = oxc_parser::Parser::new(&alloc, &wrapped, SourceType::tsx()).parse();
    if parsed.panicked || !parsed.errors.is_empty() {
        return Err(());
    }
    let Some(Statement::ExpressionStatement(stmt)) = parsed.program.body.first() else {
        return Err(());
    };
    Ok(expr_has_binding_impurity_parsed(
        &stmt.expression,
        scope,
        bindings,
        scopes,
    ))
}

/// Retained-AST form of [`expr_has_binding_impurity`].
#[must_use]
pub(super) fn expr_has_binding_impurity_parsed(
    expression: &Expression<'_>,
    scope: ScopeId,
    bindings: &BindingTable,
    scopes: &ScopeGraph,
) -> bool {
    let mut scan = BindingImpurityScan {
        bindings,
        scopes,
        scope,
        found: false,
    };
    scan.visit_expr(expression);
    scan.found
}

/// Walks an expression tree for the IMPURE portion of `has_state`: a MEMBER access
/// rooted at a declared binding (`!is_pure`) OR an assignment/update MUTATION whose write
/// TARGET is rooted at a declared binding (a global-target write stays pure). Does NOT
/// descend into nested function bodies (official sets `expression: null` there).
struct BindingImpurityScan<'a> {
    bindings: &'a BindingTable,
    scopes: &'a ScopeGraph,
    scope: ScopeId,
    found: bool,
}

impl BindingImpurityScan<'_> {
    /// Whether `name` resolves (scope-awarely) to a declared binding — the shared
    /// "non-global root" test used by both the member-read half and the
    /// assignment/update write-target half. A GLOBAL / undeclared name (`globalThis`,
    /// `Math`, an undeclared `foo`) resolves to `None`.
    fn ident_is_binding(&self, name: &str) -> bool {
        self.bindings
            .resolve_kind(self.scopes, self.scope, name)
            .is_some()
    }

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
                // TS skins are transparent for root resolution: `(obj as any).y`,
                // `(obj satisfies T).y`, `obj!.y`, `(<T>obj).y` all root at `obj`.
                Expression::TSNonNullExpression(e) => node = &e.expression,
                Expression::TSAsExpression(e) => node = &e.expression,
                Expression::TSSatisfiesExpression(e) => node = &e.expression,
                Expression::TSTypeAssertion(e) => node = &e.expression,
                Expression::Identifier(id) => return self.ident_is_binding(id.name.as_str()),
                _ => return false,
            }
        }
    }

    /// Whether an ASSIGNMENT target writes to a leaf rooted at a declared BINDING — the
    /// mutation half of `has_state`. A bare-identifier / member target roots at its
    /// leftmost identifier; a destructuring pattern (`[a] = …` / `{ a } = …`) roots at
    /// ANY of its write leaves. A write to a GLOBAL / undeclared leaf (`globalThis.x`,
    /// `foo`) does NOT — matching official (`globalThis.x = 1` / `foo = 1` stay plain
    /// init; `obj.x = 1` / `plain = 1` are stateful).
    fn assignment_target_roots_at_binding(
        &self,
        target: &oxc_ast::ast::AssignmentTarget<'_>,
    ) -> bool {
        use oxc_ast::ast::AssignmentTarget as AT;
        match target {
            AT::AssignmentTargetIdentifier(id) => self.ident_is_binding(id.name.as_str()),
            AT::StaticMemberExpression(m) => self.member_root_is_binding(&m.object),
            AT::ComputedMemberExpression(m) => self.member_root_is_binding(&m.object),
            AT::PrivateFieldExpression(m) => self.member_root_is_binding(&m.object),
            AT::TSAsExpression(e) => self.lvalue_expr_roots_at_binding(&e.expression),
            AT::TSSatisfiesExpression(e) => self.lvalue_expr_roots_at_binding(&e.expression),
            AT::TSNonNullExpression(e) => self.lvalue_expr_roots_at_binding(&e.expression),
            AT::TSTypeAssertion(e) => self.lvalue_expr_roots_at_binding(&e.expression),
            AT::ArrayAssignmentTarget(arr) => {
                arr.elements
                    .iter()
                    .flatten()
                    .any(|el| self.maybe_default_roots_at_binding(el))
                    || arr
                        .rest
                        .as_ref()
                        .is_some_and(|r| self.assignment_target_roots_at_binding(&r.target))
            }
            AT::ObjectAssignmentTarget(obj) => {
                obj.properties
                    .iter()
                    .any(|p| self.target_property_roots_at_binding(p))
                    || obj
                        .rest
                        .as_ref()
                        .is_some_and(|r| self.assignment_target_roots_at_binding(&r.target))
            }
        }
    }

    /// The SIMPLE-target (`UpdateExpression` argument — identifier / member, no
    /// destructuring) form of [`Self::assignment_target_roots_at_binding`].
    fn simple_target_roots_at_binding(
        &self,
        target: &oxc_ast::ast::SimpleAssignmentTarget<'_>,
    ) -> bool {
        use oxc_ast::ast::SimpleAssignmentTarget as ST;
        match target {
            ST::AssignmentTargetIdentifier(id) => self.ident_is_binding(id.name.as_str()),
            ST::StaticMemberExpression(m) => self.member_root_is_binding(&m.object),
            ST::ComputedMemberExpression(m) => self.member_root_is_binding(&m.object),
            ST::PrivateFieldExpression(m) => self.member_root_is_binding(&m.object),
            ST::TSAsExpression(e) => self.lvalue_expr_roots_at_binding(&e.expression),
            ST::TSSatisfiesExpression(e) => self.lvalue_expr_roots_at_binding(&e.expression),
            ST::TSNonNullExpression(e) => self.lvalue_expr_roots_at_binding(&e.expression),
            ST::TSTypeAssertion(e) => self.lvalue_expr_roots_at_binding(&e.expression),
        }
    }

    /// The write-leaf-root test for a TS-cast target's inner value expression.
    fn lvalue_expr_roots_at_binding(&self, expr: &Expression<'_>) -> bool {
        match expr {
            Expression::Identifier(id) => self.ident_is_binding(id.name.as_str()),
            Expression::StaticMemberExpression(m) => self.member_root_is_binding(&m.object),
            Expression::ComputedMemberExpression(m) => self.member_root_is_binding(&m.object),
            Expression::PrivateFieldExpression(m) => self.member_root_is_binding(&m.object),
            Expression::ParenthesizedExpression(p) => {
                self.lvalue_expr_roots_at_binding(&p.expression)
            }
            Expression::TSNonNullExpression(e) => self.lvalue_expr_roots_at_binding(&e.expression),
            Expression::TSAsExpression(e) => self.lvalue_expr_roots_at_binding(&e.expression),
            Expression::TSSatisfiesExpression(e) => {
                self.lvalue_expr_roots_at_binding(&e.expression)
            }
            Expression::TSTypeAssertion(e) => self.lvalue_expr_roots_at_binding(&e.expression),
            _ => false,
        }
    }

    /// Whether a destructuring element (`[a]` / `[a = default]`) writes a leaf rooted
    /// at a binding. The default initializer is a VALUE read, not a write leaf.
    fn maybe_default_roots_at_binding(
        &self,
        el: &oxc_ast::ast::AssignmentTargetMaybeDefault<'_>,
    ) -> bool {
        use oxc_ast::ast::AssignmentTargetMaybeDefault as MD;
        match el {
            MD::AssignmentTargetWithDefault(wd) => {
                self.assignment_target_roots_at_binding(&wd.binding)
            }
            other => other
                .as_assignment_target()
                .is_some_and(|t| self.assignment_target_roots_at_binding(t)),
        }
    }

    /// Whether an object-destructuring property (`{ a }` / `{ k: t }`) writes a leaf
    /// rooted at a binding.
    fn target_property_roots_at_binding(
        &self,
        prop: &oxc_ast::ast::AssignmentTargetProperty<'_>,
    ) -> bool {
        use oxc_ast::ast::AssignmentTargetProperty as P;
        match prop {
            P::AssignmentTargetPropertyIdentifier(id) => {
                self.ident_is_binding(id.binding.name.as_str())
            }
            P::AssignmentTargetPropertyProperty(pp) => {
                self.maybe_default_roots_at_binding(&pp.binding)
            }
        }
    }

    /// Descend the EVALUATED READ subexpressions of an assignment TARGET — computed-member
    /// keys, destructuring computed keys, and destructuring default initializers — so a
    /// binding impurity in the LHS of a GLOBAL-target write is still reported. The write
    /// LEAF itself is scored by [`Self::assignment_target_roots_at_binding`]; this covers
    /// the remaining read positions official's `MemberExpression.js` rule fires on
    /// (`globalThis[obj.y] = 1`, `[foo = obj.y] = g`, `({ [obj.y]: foo } = g)` are stateful,
    /// while `globalThis[gk] = 1` over a GLOBAL key stays plain).
    fn visit_assignment_target_reads(&mut self, target: &oxc_ast::ast::AssignmentTarget<'_>) {
        use oxc_ast::ast::AssignmentTarget as AT;
        match target {
            AT::AssignmentTargetIdentifier(_) => {}
            AT::StaticMemberExpression(m) => self.visit_expr(&m.object),
            AT::ComputedMemberExpression(m) => {
                self.visit_expr(&m.object);
                self.visit_expr(&m.expression);
            }
            AT::PrivateFieldExpression(m) => self.visit_expr(&m.object),
            AT::TSAsExpression(e) => self.visit_expr(&e.expression),
            AT::TSSatisfiesExpression(e) => self.visit_expr(&e.expression),
            AT::TSNonNullExpression(e) => self.visit_expr(&e.expression),
            AT::TSTypeAssertion(e) => self.visit_expr(&e.expression),
            AT::ArrayAssignmentTarget(arr) => {
                for el in arr.elements.iter().flatten() {
                    self.visit_maybe_default_reads(el);
                }
                if let Some(rest) = &arr.rest {
                    self.visit_assignment_target_reads(&rest.target);
                }
            }
            AT::ObjectAssignmentTarget(obj) => {
                for prop in &obj.properties {
                    self.visit_target_property_reads(prop);
                }
                if let Some(rest) = &obj.rest {
                    self.visit_assignment_target_reads(&rest.target);
                }
            }
        }
    }

    /// The SIMPLE-target (`UpdateExpression` argument) form of
    /// [`Self::visit_assignment_target_reads`] — descend a global-rooted update target's
    /// computed key (`globalThis[obj.y]++` is stateful).
    fn visit_simple_target_reads(&mut self, target: &oxc_ast::ast::SimpleAssignmentTarget<'_>) {
        use oxc_ast::ast::SimpleAssignmentTarget as ST;
        match target {
            ST::AssignmentTargetIdentifier(_) => {}
            ST::StaticMemberExpression(m) => self.visit_expr(&m.object),
            ST::ComputedMemberExpression(m) => {
                self.visit_expr(&m.object);
                self.visit_expr(&m.expression);
            }
            ST::PrivateFieldExpression(m) => self.visit_expr(&m.object),
            ST::TSAsExpression(e) => self.visit_expr(&e.expression),
            ST::TSSatisfiesExpression(e) => self.visit_expr(&e.expression),
            ST::TSNonNullExpression(e) => self.visit_expr(&e.expression),
            ST::TSTypeAssertion(e) => self.visit_expr(&e.expression),
        }
    }

    /// Descend a destructuring element's read positions: nested target reads plus the
    /// default initializer (`[foo = obj.y]` reads `obj.y`).
    fn visit_maybe_default_reads(&mut self, el: &oxc_ast::ast::AssignmentTargetMaybeDefault<'_>) {
        use oxc_ast::ast::AssignmentTargetMaybeDefault as MD;
        match el {
            MD::AssignmentTargetWithDefault(wd) => {
                self.visit_assignment_target_reads(&wd.binding);
                self.visit_expr(&wd.init);
            }
            other => {
                if let Some(t) = other.as_assignment_target() {
                    self.visit_assignment_target_reads(t);
                }
            }
        }
    }

    /// Descend an object-destructuring property's read positions: a computed key
    /// (`{ [obj.y]: foo }` reads `obj.y`), a shorthand default, and the nested binding.
    fn visit_target_property_reads(&mut self, prop: &oxc_ast::ast::AssignmentTargetProperty<'_>) {
        use oxc_ast::ast::AssignmentTargetProperty as P;
        match prop {
            P::AssignmentTargetPropertyIdentifier(id) => {
                if let Some(init) = &id.init {
                    self.visit_expr(init);
                }
            }
            P::AssignmentTargetPropertyProperty(pp) => {
                if pp.computed {
                    if let Some(key) = pp.name.as_expression() {
                        self.visit_expr(key);
                    }
                }
                self.visit_maybe_default_reads(&pp.binding);
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
                    if matches!(arg, oxc_ast::ast::Argument::SpreadElement(_)) {
                        // Spread PRESENCE is has_state (official `SpreadElement.js`:
                        // `has_state = true` unconditionally in expression context).
                        self.found = true;
                        return;
                    }
                    if let Some(e) = arg.as_expression() {
                        self.visit_expr(e);
                    }
                }
            }
            Expression::NewExpression(n) => {
                self.visit_expr(&n.callee);
                for arg in &n.arguments {
                    if matches!(arg, oxc_ast::ast::Argument::SpreadElement(_)) {
                        // Spread PRESENCE is has_state (official `SpreadElement.js`).
                        self.found = true;
                        return;
                    }
                    if let Some(e) = arg.as_expression() {
                        self.visit_expr(e);
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
                    if matches!(el, oxc_ast::ast::ArrayExpressionElement::SpreadElement(_)) {
                        // Spread PRESENCE is has_state (official `SpreadElement.js`:
                        // "treat e.g. `[...x]` the same as `[...x.values()]`" —
                        // `has_state = true` regardless of the argument's purity).
                        self.found = true;
                        return;
                    }
                    if let Some(e) = el.as_expression() {
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
                        // An object spread is the same estree `SpreadElement` presence
                        // rule — has_state unconditionally.
                        oxc_ast::ast::ObjectPropertyKind::SpreadProperty(_) => {
                            self.found = true;
                            return;
                        }
                    }
                }
            }
            Expression::TSAsExpression(e) => self.visit_expr(&e.expression),
            Expression::TSSatisfiesExpression(e) => self.visit_expr(&e.expression),
            Expression::TSNonNullExpression(e) => self.visit_expr(&e.expression),
            // An ASSIGNMENT or UPDATE expression is a MUTATION. Official marks the
            // containing expression `has_state` when the write TARGET LEAF is rooted at a
            // declared BINDING (`obj.x = 1`, `plain = 1`, `obj.x++`, `plain++`,
            // `[obj.x] = arr`); a write to a GLOBAL / undeclared leaf (`globalThis.x = 1`,
            // `globalThis.x++`, `foo = 1`, `String(globalThis.x = 1)`) is a plain init.
            // BEYOND the write leaf, every EVALUATED subexpression still participates in
            // the member rule: the RHS AND the LHS's computed keys / destructuring
            // keys+defaults (`globalThis.x = obj.y`, `globalThis[obj.y] = 1`,
            // `globalThis[obj.y]++`, `[foo = obj.y] = g`, `({ [obj.y]: foo } = g)` are all
            // stateful, while `globalThis[gk] = 1` over a GLOBAL key stays plain). Nested
            // function bodies are never descended, so a mutation inside `{() => obj.x = 1}`
            // is not reached and stays a plain init. Verified against pinned svelte@5.56.3.
            Expression::AssignmentExpression(a) => {
                if self.assignment_target_roots_at_binding(&a.left) {
                    self.found = true;
                    return;
                }
                self.visit_assignment_target_reads(&a.left);
                self.visit_expr(&a.right);
            }
            Expression::UpdateExpression(u) => {
                if self.simple_target_roots_at_binding(&u.argument) {
                    self.found = true;
                    return;
                }
                self.visit_simple_target_reads(&u.argument);
            }
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
                    if matches!(arg, oxc_ast::ast::Argument::SpreadElement(_)) {
                        // Spread PRESENCE is has_state (official `SpreadElement.js`).
                        self.found = true;
                        return;
                    }
                    if let Some(e) = arg.as_expression() {
                        self.visit_expr(e);
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
    ///
    /// A well-formed zero-arg `$effect.tracking()` call is has_call BY the official
    /// `is_pure` rule itself (`2-analyze/visitors/shared/utils.js` explicitly
    /// special-cases the `$effect.tracking` rune as IMPURE): the memoized dep
    /// re-evaluates the call INSIDE the `$.template_effect` — a tracking context —
    /// where a construction-time one-shot would return a DIFFERENT boolean (a
    /// SEMANTIC divergence, not merely structural). A scope-resolved `$effect`
    /// binding is not the rune (official `get_rune` returns null there), so the
    /// special case is skipped for a shadowed name.
    fn visit_call(&mut self, call: &CallExpression<'_>) {
        self.visit_expr(&call.callee);
        for arg in &call.arguments {
            self.visit_argument(arg);
        }
        if self.found {
            return;
        }
        if matches!(
            super::expr::effect_family_call_fact(call),
            Some(fact)
                if fact.well_formed
                    && fact.kind == super::expr::EffectFamilyCallKind::EffectTracking
        ) && self
            .bindings
            .resolve_kind(self.scopes, self.scope, "$effect")
            .is_none()
        {
            self.found = true;
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

#[cfg(test)]
#[path = "reactive_analysis_tests.rs"]
mod tests;
