//! Unit tests for the Svelte client reactive-analysis predicates (`has_call` /
//! `needs_context`) — extracted from the inline `#[cfg(test)]` module to keep
//! `reactive_analysis.rs` under the file-size guard. Included via `#[path]`.

use super::super::expr::{BindingInfo, BindingRuntimeKind, BindingTable, ScopeGraph};
use super::expr_has_call;

/// A root scope carrying one DEMOTED `$state` binding `v` (a `PlainLocal`), plus
/// the declared-root name set. The callee `v.method` roots at `v` (a declared
/// binding) → `is_pure === false`; a free name (`globalThis`) roots at no binding
/// → pure global.
fn scope_with_v() -> (
    BindingTable,
    ScopeGraph,
    super::super::expr::ScopeId,
    rustc_hash::FxHashSet<String>,
) {
    let mut bindings = BindingTable::new();
    let (mut scopes, root) = ScopeGraph::with_root();
    let id = bindings.push(BindingInfo {
        name: "v".to_string(),
        scope: root,
        kind: BindingRuntimeKind::PlainLocal,
        state: None,
    });
    scopes.declare(root, "v", id);
    let declared_roots: rustc_hash::FxHashSet<String> = ["v".to_string()].into_iter().collect();
    (bindings, scopes, root, declared_roots)
}

fn has_call(src: &str) -> bool {
    let (bindings, scopes, root, declared_roots) = scope_with_v();
    expr_has_call(src, root, &bindings, &scopes, &declared_roots)
}

#[test]
fn optional_chain_call_is_a_call() {
    // `v?.startsWith?.('x')` — an optional METHOD call whose callee roots at the
    // declared binding `v` (impure). Official `CallExpression.js` fires for an
    // optional call exactly as for a plain one → has_call.
    assert!(
        has_call("v?.startsWith?.('x')"),
        "an optional method call on a declared binding must be has_call"
    );
    // `v?.()` — an optional call directly on `v` (impure callee) → has_call.
    assert!(
        has_call("v?.()"),
        "an optional call on a declared binding must be has_call"
    );
}

#[test]
fn optional_member_is_not_a_call() {
    // `v?.x` — an optional MEMBER, NOT a call. It carries no call node, so even
    // though `v` is a referenced binding (deps > 0), the `deps` half only applies
    // INSIDE a detected call. Official never sets has_call here (and with a demoted
    // `v` it stays a one-shot init). The chain-member arm must not over-trigger.
    assert!(
        !has_call("v?.x"),
        "a plain optional member is not a call and must not be has_call"
    );
    // `v?.x?.y` — a deeper optional member chain, still no call.
    assert!(
        !has_call("v?.x?.y"),
        "a deeper optional member chain is still not a call"
    );
}

#[test]
fn pure_global_optional_call_with_no_deps_is_not_a_call() {
    // `globalThis?.foo?.()` — an optional call whose leftmost root (`globalThis`)
    // resolves to NO binding (a pure global), and the expression references no
    // declared binding (deps == 0). Official: `is_pure && deps == 0` → NOT
    // memoized. The chain-call arm must respect the pure-global path.
    assert!(
        !has_call("globalThis?.foo?.()"),
        "a pure-global optional call with no deps must not be has_call"
    );
}

#[test]
fn call_nested_in_optional_chain_base_is_found() {
    // `v.make()?.x` — the optional MEMBER `?.x` is not a call, but its chain BASE
    // is a real call `v.make()` (impure callee rooted at `v`). The member arm
    // descends the base, so the nested impure call is still found → has_call.
    assert!(
        has_call("v.make()?.x"),
        "an impure call nested in the optional chain base must be has_call"
    );
}

// ── Source-order dependency accumulation (the per-call `deps > 0` half) ──
//
// Official `CallExpression.js` sets `has_call` iff `!is_pure(callee) ||
// expression.dependencies.size > 0`, where `dependencies` accumulates AS the
// analysis walks the expression in AST-visit order, and the check runs AFTER the
// call's own children are visited. So a PURE call appearing BEFORE any dependency
// is NOT has_call; the SAME pure call AFTER a dependency IS. Verified against the
// pinned `svelte@5.56.3` compiler (a pure-call-before-dep emits an inline init; a
// dep-before-pure-call emits the `$.template_effect(($0) => …, [() => …])` form).

#[test]
fn pure_call_before_dependency_is_not_has_call() {
    // `globalThis?.foo?.() || (v > 0)` — the pure-global optional call comes FIRST
    // in source order; at the call, no dependency has accumulated yet (deps == 0),
    // and the callee roots at a global (pure) → NOT has_call. The later `v` is a
    // dependency, but it is observed AFTER the call. Official emits an inline init.
    assert!(
        !has_call("globalThis?.foo?.() || (v > 0)"),
        "a pure call appearing BEFORE its first dependency must NOT be has_call"
    );
    // The same with a string concatenation (`class={…}`-shaped).
    assert!(
        !has_call("(globalThis?.x?.() ?? '') + v"),
        "a pure call before a concatenated dependency must NOT be has_call"
    );
    // And a plain (non-optional) pure global call before the dep.
    assert!(
        !has_call("String('a') + v"),
        "a plain pure-global call before a dependency must NOT be has_call"
    );
}

#[test]
fn dependency_before_pure_call_is_has_call() {
    // `(v > 0) || globalThis?.foo?.()` — the dependency `v` is visited FIRST, so at
    // the pure call deps == 1 > 0 → has_call. Official memoizes into the deps array.
    assert!(
        has_call("(v > 0) || globalThis?.foo?.()"),
        "a pure call appearing AFTER a dependency must be has_call"
    );
    // The string-concatenation form.
    assert!(
        has_call("v + (globalThis?.x?.() ?? '')"),
        "a pure call after a concatenated dependency must be has_call"
    );
    // A plain (non-optional) pure global call after the dep.
    assert!(
        has_call("v + String('a')"),
        "a plain pure-global call after a dependency must be has_call"
    );
}

#[test]
fn pure_call_with_spread_binding_argument_is_has_call() {
    // `String(...v)` — the pure-global call SPREADS the binding `v` as an argument.
    // Official visits the spread's inner expression (`v` is a reference → a
    // dependency), so at the call deps == 1 > 0 → has_call. The spread element's
    // argument must be descended (an `arg.as_expression()`-only walk would miss it).
    assert!(
        has_call("String(...v)"),
        "a pure-global call spreading a binding argument must be has_call"
    );
}

// ── SpreadElement sets has_call UNCONDITIONALLY (official `SpreadElement.js:8`) ──
//
// Official's `SpreadElement` analyze-visitor sets `has_call = true` (and
// `has_state = true`) on the containing expression for ANY spread element, treating
// `[...x]` the same as `[...x.values()]`. So a spread under a PURE-GLOBAL callee
// with NO binding dependency anywhere STILL memoizes — the `deps > 0 || impure
// callee` rule does NOT gate the spread case. Verified against pinned svelte@5.56.3:
// each of `String(...globalThis.things)`, `[...globalThis.cls]`,
// `{ ...globalThis.opts }` emits the memoized `$.template_effect(($0) => …, [() =>
// …])` deps-array form.

#[test]
fn call_arg_spread_of_global_is_has_call() {
    // `String(...globalThis.things)` — a spread of a NON-binding global under the
    // pure callee `String`. deps == 0 and the callee is pure, so the call rule alone
    // would NOT memoize; but the SpreadElement forces has_call. The sole memoize
    // lever here is the spread itself.
    assert!(
        has_call("String(...globalThis.things)"),
        "a call-argument spread (even of a global, pure callee) must be has_call"
    );
}

#[test]
fn array_spread_of_global_is_has_call() {
    // `[...globalThis.cls]` — an ARRAY spread of a global, no call at all. The array
    // arm previously used `as_expression()` and silently dropped the spread element;
    // the SpreadElement must still force has_call.
    assert!(
        has_call("[...globalThis.cls]"),
        "an array spread (even of a global) must be has_call"
    );
}

#[test]
fn object_spread_of_global_is_has_call() {
    // `({ ...globalThis.opts })` — an OBJECT spread of a global, no call. The object
    // arm previously handled only `ObjectProperty` and dropped the `SpreadProperty`;
    // the spread must still force has_call.
    assert!(
        has_call("({ ...globalThis.opts })"),
        "an object spread (even of a global) must be has_call"
    );
}

#[test]
fn no_spread_pure_global_call_stays_not_has_call() {
    // NEGATIVE control: the SAME pure-global callee WITHOUT a spread (`String('x')`)
    // and no dependency stays NOT has_call — the spread is the discriminator, not the
    // call. Guards against a fix that over-broadens to every call.
    assert!(
        !has_call("String('x')"),
        "a pure-global call with a non-spread arg and no dep must NOT be has_call"
    );
    // And a bare array/object with no spread + no binding is not has_call.
    assert!(
        !has_call("[1, 2, 3]"),
        "a plain array literal with no spread is not has_call"
    );
    assert!(
        !has_call("({ a: 1, b: 2 })"),
        "a plain object literal with no spread is not has_call"
    );
}

#[test]
fn pure_call_with_binding_argument_is_has_call() {
    // `globalThis?.foo?.(v)` — the pure-global call's OWN ARGUMENT references the
    // binding `v`. Official visits the call's arguments (via `context.next()`)
    // BEFORE the deps check, so `v` is counted and deps == 1 > 0 → has_call. Proves
    // the call's own callee/argument bindings participate (not only PRIOR ones).
    assert!(
        has_call("globalThis?.foo?.(v) ?? false"),
        "a pure-global call whose own argument is a dependency must be has_call"
    );
    // The plain-call form.
    assert!(
        has_call("String(v)"),
        "a plain pure-global call whose own argument is a dependency must be has_call"
    );
}

#[test]
fn pure_call_with_no_dependency_anywhere_is_not_has_call() {
    // `String(42) + globalThis.x` — no binding anywhere; every call is pure-global
    // and deps stays 0 → NOT has_call. Official emits an inline init.
    assert!(
        !has_call("String(42) + globalThis.x"),
        "pure-global calls with no dependency anywhere must NOT be has_call"
    );
}

#[test]
fn conditional_dependency_order_follows_source_order() {
    // `v ? globalThis?.x?.() : 'n'` — the dependency `v` is in the TEST (visited
    // first), so the pure call in the consequent sees deps == 1 → has_call.
    assert!(
        has_call("v ? globalThis?.x?.() : 'n'"),
        "a dependency in the conditional test precedes a call in a branch → has_call"
    );
    // `(globalThis?.x?.() ?? 'y') ? 'a' : v` — the pure call is in the TEST (visited
    // first, deps == 0 there), and the dependency `v` is only in the alternate
    // (visited after). The call must NOT be has_call. Verified against pinned svelte.
    assert!(
        !has_call("(globalThis?.x?.() ?? 'y') ? 'a' : v"),
        "a call in the conditional test precedes its dependency in a branch → NOT has_call"
    );
}

#[test]
fn second_call_observes_first_calls_argument_dependency() {
    // `(globalThis?.foo?.() ?? false) && String(v)` — the FIRST call
    // (`globalThis?.foo?.()`) sees deps == 0 (pure, nothing before it) → not
    // has_call on its own; but the SECOND call `String(v)` has `v` in its argument →
    // deps == 1 → has_call. The whole expression is has_call. Verified against
    // pinned svelte (memoized).
    assert!(
        has_call("(globalThis?.foo?.() ?? false) && String(v)"),
        "a later call whose argument is a dependency makes the expression has_call"
    );
}

#[test]
fn new_expression_alone_is_not_has_call() {
    // `new Date()` — a `NewExpression` rooted at a GLOBAL with no dependency.
    // Official `NewExpression.js` sets only `needs_context`, NOT `has_call`, so a
    // bare `new GlobalCtor()` stays an inline init (verified against pinned svelte:
    // `$.set_attribute(input, 'title', new Date())`). The scan must NOT treat it as
    // a call.
    assert!(
        !has_call("new Date()"),
        "a bare `new GlobalCtor()` with no dependency must NOT be has_call"
    );
    // `new Foo(v)` — wrapped in an OUTER pure call so the New's argument dependency
    // `v` is observed by that call → has_call. (A bare `new Foo(v)` is the has_state
    // path, a separate predicate; here the outer call makes it has_call.)
    assert!(
        has_call("String(new Date(v))"),
        "an outer call observing a New-argument dependency is has_call"
    );
}

#[test]
fn new_local_constructor_is_a_dependency_for_a_later_call() {
    // `new v()` makes `v` a dependency (the constructor identifier resolves to a
    // binding), so a SUBSEQUENT call observes it. Here the outer `String(...)` call
    // sees deps == 1 (from `v` in the New callee) → has_call. Verified against
    // pinned svelte (`new Foo()` inside `String(...)` memoizes).
    assert!(
        has_call("String(new v())"),
        "a New rooted at a binding makes it a dependency for the enclosing call"
    );
}

#[test]
fn pure_global_tagged_template_is_not_has_call() {
    // ``String.raw`abc` `` — a tagged template whose TAG is a pure global. Official
    // `TaggedTemplateExpression.js` sets has_call only when `!is_pure(node.tag)`, so
    // a pure-global tag does NOT (verified against pinned svelte: inline init).
    assert!(
        !has_call("String.raw`abc`"),
        "a pure-global tagged template must NOT be has_call"
    );
    // `` v`abc` `` — a tag rooted at the declared binding `v` (impure) → has_call.
    assert!(
        has_call("v`abc`"),
        "a tagged template with a binding-rooted (impure) tag must be has_call"
    );
    // `` String.raw`${v.x?.()}` `` — pure tag, but the `${…}` interpolation contains
    // an impure call → has_call (the interpolation is descended).
    assert!(
        has_call("String.raw`a${v.x()}b`"),
        "an impure call inside a tagged-template interpolation must be has_call"
    );
}

// ── `class={…}` `needs_clsx` predicate (official `Attribute.js`) ──
//
// Official sets `needs_clsx` for a single-expression `class={…}` UNLESS the value
// is a `Literal` / `TemplateLiteral` / `BinaryExpression`. Verified against pinned
// svelte@5.56.3: `class={a + b}` emits `$.set_class(el, 1, a + b)` (no clsx);
// `class={c}` / `class={f()}` / `class={[…]}` / `class={{…}}` emit `$.clsx(…)`.

#[test]
fn binary_literal_template_class_values_do_not_need_clsx() {
    use super::class_value_needs_clsx;
    // A BinaryExpression (string concatenation) — NOT wrapped (the headline fix).
    assert!(
        !class_value_needs_clsx("a + b"),
        "a binary-expression class value must NOT need clsx"
    );
    // A string / numeric / boolean Literal — NOT wrapped.
    assert!(
        !class_value_needs_clsx("'x'"),
        "a string-literal class value must NOT need clsx"
    );
    assert!(
        !class_value_needs_clsx("42"),
        "a numeric-literal class value must NOT need clsx"
    );
    // A TemplateLiteral — NOT wrapped.
    assert!(
        !class_value_needs_clsx("`a${b}c`"),
        "a template-literal class value must NOT need clsx"
    );
}

#[test]
fn other_class_value_shapes_need_clsx() {
    use super::class_value_needs_clsx;
    // An Identifier, a CallExpression, a ConditionalExpression, a LogicalExpression,
    // an ObjectExpression, an ArrayExpression, and a MemberExpression — all wrapped.
    for src in [
        "c",
        "String(c)",
        "on ? 'a' : 'b'",
        "a || b",
        "a ?? b",
        "{ active: on }",
        "['a', cond && 'b']",
        "obj.cls",
    ] {
        assert!(
            class_value_needs_clsx(src),
            "class value `{src}` must need clsx (not Literal/Template/Binary)"
        );
    }
}

// ── member `has_state` (`MemberExpression.js`'s `!is_pure(node)`) ──
//
// A member access rooted at a declared binding (`v`, a `PlainLocal` demoted
// `$state`) is impure ⇒ has_state; a member rooted at a GLOBAL (`Math`,
// `globalThis`) is pure ⇒ NOT has_state; a bare identifier (no member) carries no
// member ⇒ NOT covered here. Verified against pinned svelte@5.56.3 (a `{d.x}` over a
// demoted state joins the `$.template_effect`; a bare `{d}` stays inline).

fn member_roots(src: &str) -> bool {
    let (bindings, scopes, root, _declared) = scope_with_v();
    super::expr_member_roots_at_binding(src, root, &bindings, &scopes)
}

#[test]
fn member_rooted_at_binding_is_state() {
    // `v.x` / `v?.x` / `v.a.b` / `v[k]` — all root at the declared binding `v`.
    assert!(
        member_roots("v.x"),
        "a member on a binding root is has_state"
    );
    assert!(
        member_roots("v?.x"),
        "an optional member on a binding root is has_state"
    );
    assert!(
        member_roots("v.a.b"),
        "a deep member chain on a binding root is has_state"
    );
    assert!(
        member_roots("v[k]"),
        "a computed member on a binding root is has_state"
    );
    // A member nested inside a larger expression still counts.
    assert!(
        member_roots("'p-' + v.x"),
        "a member on a binding nested in a binary is has_state"
    );
}

#[test]
fn member_rooted_at_global_or_bare_ident_is_not_state() {
    // A member rooted at a GLOBAL (no binding) is pure ⇒ NOT has_state.
    assert!(
        !member_roots("Math.PI"),
        "a member on a global root is NOT has_state"
    );
    assert!(
        !member_roots("globalThis.x.y"),
        "a deep member on a global root is NOT has_state"
    );
    // A BARE identifier (no member access) is not covered by the member rule.
    assert!(
        !member_roots("v"),
        "a bare identifier read (no member) is not a member-rule has_state"
    );
    // A pure literal / call on a global carries no binding-rooted member.
    assert!(
        !member_roots("'x'"),
        "a literal is not a binding-rooted member"
    );
    assert!(
        !member_roots("String('x')"),
        "a pure-global call is not a binding-rooted member"
    );
}
