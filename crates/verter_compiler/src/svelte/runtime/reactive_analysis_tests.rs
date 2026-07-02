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

/// The `class_value_needs_clsx` decision over a value-expression SOURCE: parse it the
/// shared way (the same single parse the producers use) and read the unwrapped-root KIND
/// fact, so a parenthesized literal / binary / template is classified on its UNWRAPPED
/// root. Mirrors the production call site (which reads `analyzed.unwrapped_root_kind`).
fn needs_clsx(src: &str) -> bool {
    let facts = super::super::expr::collect_expr_references(src).expect("class value parses");
    super::class_value_needs_clsx(facts.unwrapped_root_kind)
}

#[test]
fn binary_literal_template_class_values_do_not_need_clsx() {
    // A BinaryExpression (string concatenation) — NOT wrapped (the headline fix).
    assert!(
        !needs_clsx("a + b"),
        "a binary-expression class value must NOT need clsx"
    );
    // A string / numeric / boolean Literal — NOT wrapped.
    assert!(
        !needs_clsx("'x'"),
        "a string-literal class value must NOT need clsx"
    );
    assert!(
        !needs_clsx("42"),
        "a numeric-literal class value must NOT need clsx"
    );
    // A TemplateLiteral — NOT wrapped.
    assert!(
        !needs_clsx("`a${b}c`"),
        "a template-literal class value must NOT need clsx"
    );
    // PARENTHESIZED literal / binary / template — the unwrapped-root decision sees through
    // the author parens, so these are STILL not wrapped (the cycle-4 fix).
    assert!(
        !needs_clsx("('x')"),
        "a parenthesized string literal must NOT need clsx (decision on the unwrapped root)"
    );
    assert!(
        !needs_clsx("((a + b))"),
        "a doubly-parenthesized binary must NOT need clsx (decision on the unwrapped root)"
    );
    assert!(
        !needs_clsx("(`x${a}`)"),
        "a parenthesized template literal must NOT need clsx (decision on the unwrapped root)"
    );
}

#[test]
fn other_class_value_shapes_need_clsx() {
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
            needs_clsx(src),
            "class value `{src}` must need clsx (not Literal/Template/Binary)"
        );
    }
    // A PARENTHESIZED conditional is STILL wrapped — the unwrapped root is a conditional,
    // not a no-wrap kind (the clsx-YES boundary).
    assert!(
        needs_clsx("(on ? 'a' : 'b')"),
        "a parenthesized conditional must STILL need clsx (unwrapped root is a conditional)"
    );
}

// ── binding impurity: member `has_state` (`MemberExpression.js`'s `!is_pure(node)`)
//    + the assignment/update MUTATION half ──
//
// A member access rooted at a declared binding (`v`, a `PlainLocal` demoted
// `$state`) is impure ⇒ has_state; a member rooted at a GLOBAL (`Math`,
// `globalThis`) is pure ⇒ NOT has_state; a bare identifier (no member) carries no
// member ⇒ NOT covered here. The scan ALSO covers the assignment/update MUTATION
// half of `has_state`: a write whose TARGET LEAF roots at a binding (`v = 1`,
// `v.x = 1`, `v++`, `v.x++`) is impure ⇒ has_state; a write to a GLOBAL leaf
// (`globalThis.x = 1`, `foo = 1`) is a plain init, but a binding member in the
// RHS or an evaluated LHS key/default (`globalThis[v.y] = 1`) still counts.
// Verified against pinned svelte@5.56.3 (a `{d.x}` over a demoted state joins the
// `$.template_effect`; a bare `{d}` stays inline).

fn has_binding_impurity(src: &str) -> bool {
    let (bindings, scopes, root, _declared) = scope_with_v();
    super::expr_has_binding_impurity(src, root, &bindings, &scopes)
}

#[test]
fn member_rooted_at_binding_is_state() {
    // `v.x` / `v?.x` / `v.a.b` / `v[k]` — all root at the declared binding `v`.
    assert!(
        has_binding_impurity("v.x"),
        "a member on a binding root is has_state"
    );
    assert!(
        has_binding_impurity("v?.x"),
        "an optional member on a binding root is has_state"
    );
    assert!(
        has_binding_impurity("v.a.b"),
        "a deep member chain on a binding root is has_state"
    );
    assert!(
        has_binding_impurity("v[k]"),
        "a computed member on a binding root is has_state"
    );
    // A member nested inside a larger expression still counts.
    assert!(
        has_binding_impurity("'p-' + v.x"),
        "a member on a binding nested in a binary is has_state"
    );
}

#[test]
fn member_rooted_at_global_or_bare_ident_is_not_state() {
    // A member rooted at a GLOBAL (no binding) is pure ⇒ NOT has_state.
    assert!(
        !has_binding_impurity("Math.PI"),
        "a member on a global root is NOT has_state"
    );
    assert!(
        !has_binding_impurity("globalThis.x.y"),
        "a deep member on a global root is NOT has_state"
    );
    // A BARE identifier (no member access) is not covered by the member rule.
    assert!(
        !has_binding_impurity("v"),
        "a bare identifier read (no member) is not a member-rule has_state"
    );
    // A pure literal / call on a global carries no binding-rooted member.
    assert!(
        !has_binding_impurity("'x'"),
        "a literal is not a binding-rooted member"
    );
    assert!(
        !has_binding_impurity("String('x')"),
        "a pure-global call is not a binding-rooted member"
    );
}

#[test]
fn assignment_and_update_are_has_state() {
    // WRITE half of has_state: an assignment/update MUTATION is impure ⇒ has_state — for a
    // member target (`v.x = 1` / `v.x++`) AND a bare-binding target (`v = 1` / `v++`). A
    // mutation DEFERRED inside a function body is not descended (stays pure). Verified
    // against pinned svelte@5.56.3.
    assert!(
        has_binding_impurity("v.x = 1"),
        "member assignment target is has_state"
    );
    assert!(
        has_binding_impurity("v = 1"),
        "bare-binding assignment is has_state"
    );
    assert!(has_binding_impurity("v.x++"), "member update is has_state");
    assert!(
        has_binding_impurity("v++"),
        "bare-binding update is has_state"
    );
    assert!(
        !has_binding_impurity("() => v.x = 1"),
        "assignment inside an arrow body is not has_state"
    );
    assert!(
        !has_binding_impurity("() => v++"),
        "update inside an arrow body is not has_state"
    );
}

#[test]
fn global_and_undeclared_mutations_are_not_state() {
    // The over-fire guard: a MUTATION whose write TARGET is rooted at a GLOBAL / undeclared
    // name is PURE ⇒ NOT has_state — official keeps `globalThis.x = 1`, `globalThis.x++`,
    // `foo = 1` (undeclared), and `String(globalThis.x = 1)` as a PLAIN init. Only a
    // binding-rooted write is stateful. Verified against pinned svelte@5.56.3.
    assert!(
        !has_binding_impurity("globalThis.x = 1"),
        "a member assignment rooted at a GLOBAL is NOT has_state"
    );
    assert!(
        !has_binding_impurity("globalThis.x++"),
        "a member update rooted at a GLOBAL is NOT has_state"
    );
    assert!(
        !has_binding_impurity("foo = 1"),
        "an assignment to an UNDECLARED (global) name is NOT has_state"
    );
    assert!(
        !has_binding_impurity("String(globalThis.x = 1)"),
        "a global mutation nested in a pure-global call is NOT has_state"
    );
    // A computed key that is itself a GLOBAL/bare read (not a binding member) keeps the
    // whole global-target write plain — official leaves `globalThis[gk] = 1` a plain init.
    assert!(
        !has_binding_impurity("globalThis[gk] = 1"),
        "a global-target write with a global computed key is NOT has_state"
    );
}

#[test]
fn binding_impurity_in_evaluated_position_of_global_write_is_state() {
    // Beyond the write leaf, a binding MEMBER appearing in any EVALUATED read position of a
    // global-target assignment/update is still reported (official's `MemberExpression.js`
    // `!is_pure` fires over the whole tree): the RHS, an evaluated LHS computed key, an
    // update-target computed key, and a destructuring default / computed key. Verified
    // against pinned svelte@5.56.3 (all STATE at both call sites).
    assert!(
        has_binding_impurity("globalThis.x = v.y"),
        "a binding-member RHS of a global-target write is has_state"
    );
    assert!(
        has_binding_impurity("globalThis[v.y] = 1"),
        "a binding-member LHS computed key of a global-target write is has_state"
    );
    assert!(
        has_binding_impurity("globalThis[v.y]++"),
        "a binding-member computed key of a global-target UPDATE is has_state"
    );
    assert!(
        has_binding_impurity("[foo = v.y] = arr"),
        "a binding-member destructuring DEFAULT (global write leaf) is has_state"
    );
    assert!(
        has_binding_impurity("({ [v.y]: foo } = src)"),
        "a binding-member destructuring COMPUTED KEY (global write leaf) is has_state"
    );
}

#[test]
fn ts_wrapper_member_root_is_state() {
    // A TS skin (`as` / `satisfies` / `!`) is transparent for root resolution: a member /
    // write rooted at a binding through a cast is impure ⇒ has_state. Official marks
    // `(obj as any).y` and `(obj as any).x = 1` stateful; verified against pinned
    // svelte@5.56.3.
    assert!(
        has_binding_impurity("(v as any).y"),
        "a member read through a TS `as` cast on a binding root is has_state"
    );
    assert!(
        has_binding_impurity("(v as any).x = 1"),
        "a write through a TS `as` cast on a binding root is has_state"
    );
    assert!(
        has_binding_impurity("(v satisfies unknown).y"),
        "a member read through a TS `satisfies` on a binding root is has_state"
    );
    // A TS cast over a GLOBAL root stays pure.
    assert!(
        !has_binding_impurity("(globalThis as any).y"),
        "a member read through a TS cast on a GLOBAL root is NOT has_state"
    );
}

// ── `prop_value_has_state` (the `Component.js` / `SvelteBoundary.js` getter-vs-init
//    discriminator) ──
//
// Official's `metadata.expression.has_state` for a component / boundary prop value is
// the SYNCHRONOUS signal/prop/snippet reference scan PLUS the member-root half
// (`MemberExpression.js`'s `!is_pure`): a member rooted at ANY declared binding — a
// plain local, a deep-proxied `$state` object, a prop — makes the value state-bearing
// (⇒ the `get name() { return <expr>; }` getter member); a bare plain-local ident or a
// member read deferred inside a nested function body stays a plain `name: <expr>` init.
// Verified against pinned svelte@5.56.3 (boundary + component emit identically):
// `failed={obj.failed}` → getter; `onerror={() => obj.failed}` / `onerror={f}` → init.

/// A root scope for the prop getter-vs-init predicate: a plain local `obj`, a
/// deep-proxied `$state` object `st` (a `BareProxy`), a prop `failed`, and a plain
/// local `f`.
fn prop_scope() -> (BindingTable, ScopeGraph, super::super::expr::ScopeId) {
    let mut bindings = BindingTable::new();
    let (mut scopes, root) = ScopeGraph::with_root();
    for (name, kind) in [
        ("obj", BindingRuntimeKind::PlainLocal),
        ("st", BindingRuntimeKind::BareProxy),
        ("failed", BindingRuntimeKind::Prop),
        ("f", BindingRuntimeKind::PlainLocal),
    ] {
        let id = bindings.push(BindingInfo {
            name: name.to_string(),
            scope: root,
            kind,
            state: None,
        });
        scopes.declare(root, name, id);
    }
    (bindings, scopes, root)
}

fn prop_has_state(src: &str) -> bool {
    let (bindings, scopes, root) = prop_scope();
    super::prop_value_has_state(src, root, &bindings, &scopes)
}

#[test]
fn prop_value_member_rooted_at_binding_is_state() {
    // `obj.failed` — a member rooted at a PLAIN LOCAL is impure (`!is_pure`) ⇒ the prop
    // emits the getter (official: `get failed() { return obj.failed; }`).
    assert!(
        prop_has_state("obj.failed"),
        "a member rooted at a plain local must be state-bearing (getter)"
    );
    // `st.failed` — a member rooted at a deep-proxied `$state` object (a `BareProxy`,
    // whose reads are plain member access, not `$.get`) is ALSO impure ⇒ getter.
    assert!(
        prop_has_state("st.failed"),
        "a member rooted at a deep-proxied $state object must be state-bearing (getter)"
    );
    // A deeper chain still roots at the binding.
    assert!(
        prop_has_state("obj.a.b"),
        "a deep member chain rooted at a plain local must be state-bearing (getter)"
    );
}

#[test]
fn prop_value_member_inside_nested_fn_is_not_state() {
    // `() => obj.failed` — the member read is DEFERRED inside the arrow body; official
    // keeps the plain init (`onerror: () => obj.failed`). The member-root half must not
    // descend into nested function bodies (the sync-only rule is preserved).
    assert!(
        !prop_has_state("() => obj.failed"),
        "a member read inside a nested function body must stay a plain init"
    );
    assert!(
        !prop_has_state("function () { return st.failed; }"),
        "a member read inside a function expression body must stay a plain init"
    );
}

#[test]
fn prop_value_bare_local_and_global_member_are_not_state() {
    // A BARE plain-local identifier (`onerror={f}`) is a plain init — the member rule
    // does not cover bare reads, and a plain local is not a signal/prop/snippet.
    assert!(
        !prop_has_state("f"),
        "a bare plain-local ident must stay a plain init"
    );
    // A member rooted at a GLOBAL stays pure ⇒ plain init.
    assert!(
        !prop_has_state("Math.PI"),
        "a member rooted at a global must stay a plain init"
    );
    // The existing reference-scan half still fires: a PROP ident is state-bearing.
    assert!(
        prop_has_state("failed"),
        "a prop reference must stay state-bearing (getter)"
    );
}
