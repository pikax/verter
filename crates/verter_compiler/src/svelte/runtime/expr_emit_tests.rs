//! Unit tests for the AST-backed client expression emitter.
//!
//! These pin the scope-aware read/write/compound-assign/update rewrites and the
//! proxy distinction directly at the expression-emitter surface, INDEPENDENT of
//! the full module assembly. Each test is discriminating: it asserts both the
//! rewritten form AND a negative (the wrong rewrite is absent).

use oxc_allocator::Allocator;

use crate::svelte::runtime::expr::{
    BindTargetFact, BindTargetKind, BindingInfo, BindingRuntimeKind, BindingTable, BindingUseSet,
    ScopeGraph, ScopeId, StateClassification, StateLowering, StateRuneKind,
};
use crate::svelte::runtime::expr_emit::{
    self, props_shape, state_decl_shape, PropsShape, StateDeclShape,
};
use crate::svelte::runtime::expr_rewrite::{rewrite_expression, RewriteRole};

/// Build a one-binding table + scope graph declaring `name` with `kind` (and a
/// `$state` classification for the signal/proxy kinds) at the root scope.
fn single_binding(name: &str, kind: BindingRuntimeKind) -> (BindingTable, ScopeGraph, ScopeId) {
    let (mut scopes, root) = ScopeGraph::with_root();
    let mut bindings = BindingTable::new();
    let state = match kind {
        BindingRuntimeKind::StateSignal { .. } => Some(StateClassification {
            declared: StateRuneKind::State,
            proxiable: false,
            uses: BindingUseSet {
                reassigned: true,
                deep_mutated: false,
            },
            lowering: StateLowering::StateSignal,
        }),
        BindingRuntimeKind::BareProxy => Some(StateClassification {
            declared: StateRuneKind::State,
            proxiable: true,
            uses: BindingUseSet {
                reassigned: false,
                deep_mutated: true,
            },
            lowering: StateLowering::BareProxy,
        }),
        BindingRuntimeKind::StateProxy => Some(StateClassification {
            declared: StateRuneKind::State,
            proxiable: true,
            uses: BindingUseSet {
                reassigned: true,
                deep_mutated: false,
            },
            lowering: StateLowering::StateProxy,
        }),
        _ => None,
    };
    let id = bindings.push(BindingInfo {
        name: name.to_string(),
        scope: root,
        kind,
        state,
    });
    scopes.declare(root, name, id);
    (bindings, scopes, root)
}

/// Rewrite `expr` against a single root-scope binding of `name`/`kind`. The
/// rewriter is fallible; these tests exercise SUPPORTED forms, so the helper
/// unwraps (a refusal in a supported-form test is a genuine failure).
fn rewrite_with(expr: &str, name: &str, kind: BindingRuntimeKind) -> String {
    let (bindings, scopes, root) = single_binding(name, kind);
    rewrite_expression(expr, root, &bindings, &scopes, RewriteRole::Value)
        .expect("supported expression rewrite")
        .text
}

#[test]
fn state_signal_read_becomes_get() {
    let out = rewrite_with(
        "count",
        "count",
        BindingRuntimeKind::StateSignal { raw: false },
    );
    assert_eq!(out, "$.get(count)");
}

#[test]
fn state_signal_compound_assign_becomes_set_get() {
    // `count += 1` → `$.set(count, $.get(count) + 1)`. DISCRIMINATING against a
    // rewriter that leaves the compound assign untouched.
    let out = rewrite_with(
        "count += 1",
        "count",
        BindingRuntimeKind::StateSignal { raw: false },
    );
    assert_eq!(out, "$.set(count, $.get(count) + 1)");
    assert!(
        !out.contains("count += 1"),
        "the bare compound assign must be gone"
    );
}

#[test]
fn state_signal_increment_becomes_update() {
    let out = rewrite_with(
        "count++",
        "count",
        BindingRuntimeKind::StateSignal { raw: false },
    );
    assert_eq!(out, "$.update(count)");
}

#[test]
fn state_signal_decrement_becomes_update_minus_one() {
    let out = rewrite_with(
        "count--",
        "count",
        BindingRuntimeKind::StateSignal { raw: false },
    );
    assert_eq!(out, "$.update(count, -1)");
}

#[test]
fn state_signal_plain_reassign_becomes_set() {
    let out = rewrite_with(
        "count = 5",
        "count",
        BindingRuntimeKind::StateSignal { raw: false },
    );
    assert_eq!(out, "$.set(count, 5)");
}

#[test]
fn bare_proxy_read_stays_plain_never_get() {
    // A bare `$.proxy` read is PLAIN access — NEVER `$.get(o)`. DISCRIMINATING
    // against a proxy-blind rewriter that wraps every state read in `$.get`.
    let out = rewrite_with("o.a", "o", BindingRuntimeKind::BareProxy);
    assert_eq!(out, "o.a");
    assert!(
        !out.contains("$.get(o)"),
        "a bare proxy must NOT read via $.get"
    );
}

#[test]
fn bare_proxy_member_increment_stays_plain() {
    // `o.a++` → plain `o.a++` (a deep mutation of the proxy, never `$.set`).
    let out = rewrite_with("o.a++", "o", BindingRuntimeKind::BareProxy);
    assert_eq!(out, "o.a++");
    assert!(
        !out.contains("$.set"),
        "a bare-proxy member mutation must NOT be $.set"
    );
    assert!(
        !out.contains("$.update"),
        "a bare-proxy member mutation must NOT be $.update"
    );
}

#[test]
fn bare_proxy_method_call_stays_plain() {
    let out = rewrite_with("o.push(1)", "o", BindingRuntimeKind::BareProxy);
    assert_eq!(out, "o.push(1)");
    assert!(!out.contains("$.get(o)"));
}

#[test]
fn state_proxy_member_read_is_get_then_member() {
    // A reassigned object `$state` (StateProxy) reads as `$.get(o).a`.
    let out = rewrite_with("o.a", "o", BindingRuntimeKind::StateProxy);
    assert_eq!(out, "$.get(o).a");
}

#[test]
fn state_proxy_reassign_carries_trailing_true() {
    // `o = { a: 2 }` for a StateProxy → `$.set(o, { a: 2 }, true)`.
    let out = rewrite_with("o = { a: 2 }", "o", BindingRuntimeKind::StateProxy);
    assert_eq!(out, "$.set(o, { a: 2 }, true)");
    assert!(
        out.ends_with(", true)"),
        "a StateProxy reassign carries the trailing true"
    );
}

#[test]
fn arrow_param_shadows_signal() {
    // `(count) => count + 1` — the arrow PARAM `count` shadows the signal, so the
    // body read is NOT rewritten. DISCRIMINATING against a scope-blind rewriter.
    let out = rewrite_with(
        "(count) => count + 1",
        "count",
        BindingRuntimeKind::StateSignal { raw: false },
    );
    assert_eq!(out, "(count) => count + 1");
    assert!(
        !out.contains("$.get(count)"),
        "the shadowing arrow param must NOT be rewritten"
    );
}

#[test]
fn nested_block_let_shadows_signal() {
    // `() => { let count = 0; count += 1; }` — the inner `let count` shadows the
    // signal, so the inner write is NOT `$.set(count, ...)`.
    let out = rewrite_with(
        "() => { let count = 0; count += 1; }",
        "count",
        BindingRuntimeKind::StateSignal { raw: false },
    );
    assert!(
        out.contains("count += 1"),
        "the shadowed local write stays plain, got: {out}"
    );
    assert!(
        !out.contains("$.set(count"),
        "a shadowed local must NOT become $.set, got: {out}"
    );
}

#[test]
fn outer_signal_read_inside_arrow_is_rewritten() {
    // `() => count + 1` — `count` is the OUTER signal (no shadow), so it IS
    // rewritten inside the arrow body.
    let out = rewrite_with(
        "() => count + 1",
        "count",
        BindingRuntimeKind::StateSignal { raw: false },
    );
    assert_eq!(out, "() => $.get(count) + 1");
}

#[test]
fn free_identifier_is_untouched() {
    // A name with no binding row is free — emitted verbatim.
    let alloc = Allocator::default();
    let _ = alloc;
    let out = rewrite_with(
        "other",
        "count",
        BindingRuntimeKind::StateSignal { raw: false },
    );
    assert_eq!(out, "other");
}

#[test]
fn props_shape_no_default_basic_destructure_is_supported() {
    // ONLY a NO-DEFAULT basic destructure (identifier / alias / string keys) is
    // supported; a default-bearing member is demoted (see below).
    assert_eq!(
        props_shape("let { name, count } = $props();"),
        PropsShape::BasicDestructure
    );
    assert_eq!(
        props_shape("let { foo: bar, \"data-x\": x } = $props();"),
        PropsShape::BasicDestructure
    );
}

#[test]
fn props_shape_any_default_is_advanced() {
    // ANY `$props()` member default (even a constant literal) is the demoted
    // props-default surface — `$props() default`, NOT `BasicDestructure`.
    assert_eq!(
        props_shape("let { name = 'world', count = 0 } = $props();"),
        PropsShape::Advanced {
            rune: "$props() default"
        }
    );
    assert_eq!(
        props_shape("let { a = 1 } = $props();"),
        PropsShape::Advanced {
            rune: "$props() default"
        }
    );
}

#[test]
fn props_shape_rest_is_advanced() {
    assert_eq!(
        props_shape("let { name, ...rest } = $props();"),
        PropsShape::Advanced {
            rune: "$props() rest"
        }
    );
}

#[test]
fn props_shape_whole_object_is_advanced() {
    assert_eq!(
        props_shape("let p = $props();"),
        PropsShape::Advanced {
            rune: "$props() whole-object"
        }
    );
}

#[test]
fn props_shape_bindable_is_advanced() {
    assert_eq!(
        props_shape("let { value = $bindable(0) } = $props();"),
        PropsShape::Advanced { rune: "$bindable" }
    );
}

#[test]
fn ts_cast_is_stripped_from_a_rewritten_expression() {
    // A TS `as` cast inside an expression is DROPPED (the §F strip), leaving the
    // rewritten runtime expression. `count as number` → `$.get(count)`.
    let out = rewrite_with(
        "count as number",
        "count",
        BindingRuntimeKind::StateSignal { raw: false },
    );
    assert_eq!(out, "$.get(count)");
    assert!(
        !out.contains("as number"),
        "the TS cast must be stripped:\n{out}"
    );
}

#[test]
fn ts_non_null_is_stripped() {
    let out = rewrite_with(
        "count!",
        "count",
        BindingRuntimeKind::StateSignal { raw: false },
    );
    assert_eq!(out, "$.get(count)");
    assert!(
        !out.contains('!'),
        "the TS non-null assertion must be stripped:\n{out}"
    );
}

#[test]
fn signal_read_inside_ternary_is_rewritten_in_both_arms() {
    // THE keystone F1 regression: `count > 0 ? count : 0` must rewrite the signal
    // read in BOTH the condition and the consequent. Verified against svelte@5.56.3:
    // `$.get(count) > 0 ? $.get(count) : 0`. RED against the verbatim `_ =>` arm
    // (which emitted raw `count > 0 ? count : 0`).
    let out = rewrite_with(
        "count > 0 ? count : 0",
        "count",
        BindingRuntimeKind::StateSignal { raw: false },
    );
    assert_eq!(out, "$.get(count) > 0 ? $.get(count) : 0");
    // NEGATIVE: no unrewritten bare `count` token remains.
    assert!(
        !out.contains("> 0 ? count"),
        "the ternary consequent read must be rewritten, not raw `count`:\n{out}"
    );
}

#[test]
fn signal_read_inside_template_literal_is_rewritten() {
    // `` `v=${count}` `` → `` `v=${$.get(count)}` ``. RED against the verbatim arm
    // (a TemplateLiteral fell through to raw source). The `?? ''` is text-node-only
    // (mixed-run), NOT applied to a user-authored template literal expression.
    let out = rewrite_with(
        "`v=${count}`",
        "count",
        BindingRuntimeKind::StateSignal { raw: false },
    );
    assert_eq!(out, "`v=${$.get(count)}`");
    assert!(
        !out.contains("${count}"),
        "raw `${{count}}` must be gone:\n{out}"
    );
    assert!(
        !out.contains("?? ''"),
        "no text-node `?? ''` in a user template literal:\n{out}"
    );
}

#[test]
fn signal_read_inside_logical_is_rewritten() {
    // `a && count` → the signal read is rewritten; the non-signal `a` is untouched.
    // Verified against svelte@5.56.3.
    let out = rewrite_with(
        "a && count",
        "count",
        BindingRuntimeKind::StateSignal { raw: false },
    );
    assert_eq!(out, "a && $.get(count)");
}

#[test]
fn signal_read_inside_array_and_object_literals_is_rewritten() {
    // `[count]` → `[$.get(count)]`; `({ x: count })` → `({ x: $.get(count) })`.
    // RED against the verbatim arm (Array/Object literals fell through).
    let arr = rewrite_with(
        "[count]",
        "count",
        BindingRuntimeKind::StateSignal { raw: false },
    );
    assert_eq!(arr, "[$.get(count)]");
    let obj = rewrite_with(
        "({ x: count })",
        "count",
        BindingRuntimeKind::StateSignal { raw: false },
    );
    assert_eq!(obj, "({ x: $.get(count) })");
}

#[test]
fn signal_read_inside_unary_and_paren_and_conditional_chain_is_rewritten() {
    // A nested mix: `!(count > 0) ? -count : count + 1`. Every read rewrites.
    let out = rewrite_with(
        "!(count > 0) ? -count : count + 1",
        "count",
        BindingRuntimeKind::StateSignal { raw: false },
    );
    assert_eq!(
        out,
        "!($.get(count) > 0) ? -$.get(count) : $.get(count) + 1"
    );
}

#[test]
fn prefix_increment_in_value_position_becomes_update_pre() {
    // `++count` used in VALUE position (a call arg) → `$.update_pre(count)`.
    // Verified against svelte@5.56.3 (`f(++count)` → `f($.update_pre(count))`).
    // RED against the prefix-blind rewriter (which emitted `$.update(count)`).
    let out = rewrite_with(
        "f(++count)",
        "count",
        BindingRuntimeKind::StateSignal { raw: false },
    );
    assert_eq!(out, "f($.update_pre(count))");
    assert!(
        !out.contains("$.update(count)"),
        "a prefix update in value position must be $.update_pre, not $.update:\n{out}"
    );
}

#[test]
fn prefix_decrement_in_value_position_becomes_update_pre_minus_one() {
    let out = rewrite_with(
        "f(--count)",
        "count",
        BindingRuntimeKind::StateSignal { raw: false },
    );
    assert_eq!(out, "f($.update_pre(count, -1))");
}

#[test]
fn postfix_increment_stays_update() {
    // `f(count++)` → `f($.update(count))` (postfix is plain `$.update`). Verified
    // against svelte@5.56.3.
    let out = rewrite_with(
        "f(count++)",
        "count",
        BindingRuntimeKind::StateSignal { raw: false },
    );
    assert_eq!(out, "f($.update(count))");
    assert!(
        !out.contains("update_pre"),
        "a postfix update must NOT be update_pre:\n{out}"
    );
}

#[test]
fn ts_cast_inside_a_ternary_arm_is_stripped() {
    // A nontrivial F3 case INSIDE a recursive position: `(count as number) > 0 ?
    // count : 0`. The cast is stripped AND the reads rewritten.
    let out = rewrite_with(
        "(count as number) > 0 ? count : 0",
        "count",
        BindingRuntimeKind::StateSignal { raw: false },
    );
    assert!(
        !out.contains("as number"),
        "TS cast must be stripped:\n{out}"
    );
    assert_eq!(out, "($.get(count)) > 0 ? $.get(count) : 0");
}

#[test]
fn classify_bind_target_is_structural() {
    // The bind-target lvalue shape is classified STRUCTURALLY from the parsed node (the
    // shared `BindTargetFact`, computed once per expression), not a `source.contains('.')`
    // text scan.
    let alloc = Allocator::default();
    let kind = |s: &str| BindTargetFact::from_source(&alloc, s).kind;
    // A bare identifier → reassignment.
    assert_eq!(kind("name"), Some(BindTargetKind::Identifier));
    // A member target → deep mutation.
    assert_eq!(kind("o.x"), Some(BindTargetKind::Member));
    // A computed member → deep mutation.
    assert_eq!(kind("arr[i]"), Some(BindTargetKind::Member));
    // A parenthesised / non-null-asserted identifier still classifies as the bare
    // identifier (the text heuristic would have no `.` here, but the structural
    // path unwraps to the lvalue core).
    assert_eq!(kind("(name)"), Some(BindTargetKind::Identifier));
    assert_eq!(kind("name!"), Some(BindTargetKind::Identifier));
    // A NON-LVALUE (a call / a binary expression) is rejected — the old
    // `contains('.')` heuristic would have mis-classified `f().x` as a member
    // target; the structural path returns the member core, but a bare call /
    // literal is `None`.
    assert_eq!(kind("f()"), None);
    assert_eq!(kind("a + b"), None);
    assert_eq!(kind("42"), None);
}

#[test]
fn bind_target_lvalue_ts_detection_is_structural_anywhere() {
    // The TS-in-lvalue fact is STRUCTURAL (the shared `BindTargetFact`) and catches — ANYWHERE
    // in the would-be lvalue spine (the spine TOP, a member-OBJECT chain link, OR a
    // computed-INDEX expression) — a node whose TSX parse diverges from a plain-MJS parse: a
    // TS-ONLY operator (`!` / `as` / `satisfies` / `<T>` / a bare `f<T>` instantiation) OR a
    // call / new / tagged-template carrying TS type arguments (`g<a,b>(c)`). A clean lvalue is
    // NOT flagged; a SEQUENCE (function-pair) is excluded (its TS rejection is owned by the
    // plain-JS function-pair lane).
    //
    // TYPE-ARGUMENT boundary (oracle-verified svelte@5.56.3): a node carrying TS type arguments
    // IS flagged (fail-closed) — both a BARE instantiation (`f<T>` / `arr[g<T>]`, an OXC
    // `TSInstantiationExpression` with no trailing call) AND a CALL / new / tagged-template that
    // carries type arguments (`arr[g<a,b>(c)]`, an OXC `CallExpression` with `type_arguments`).
    // Official PARSE-REJECTS the bare instantiation (`js_parse_error`) and parses the call form
    // as the plain-JS relational/comma `arr[(g < a, b > c)]` — so Verter's TSX-strip lane would
    // otherwise DELETE the type arguments and emit the DIVERGENT index `arr[g(c)]` (a behavioral
    // divergence). Failing both closed is never-wrong; the exact relational emit is owned by the
    // shared plain-MJS template-expression authority (D-26), not this lvalue fail-close scan.
    let alloc = Allocator::default();
    let lvalue_ts = |s: &str| BindTargetFact::from_source(&alloc, s).lvalue_contains_ts;
    // ── TS anywhere (true) ──
    // Root wrappers (the postfix non-null + the `as` / `satisfies` operators; the prefix
    // `<T>x` assertion is JSX in TSX mode, handled at the integration boundary).
    assert!(lvalue_ts("name!"));
    assert!(lvalue_ts("(name!)"));
    assert!(lvalue_ts("name as string"));
    assert!(lvalue_ts("name satisfies string"));
    assert!(lvalue_ts("o.x!"));
    assert!(lvalue_ts("((o.x as T))"));
    // NON-ROOT TS: a member-OBJECT non-null, a computed-INDEX cast, a computed-INDEX
    // non-null — all caught by the spine walk (the F1 fix vs the prior top-node-only gate).
    assert!(lvalue_ts("o!.x"));
    assert!(lvalue_ts("a[x as T]"));
    assert!(lvalue_ts("a[i!]"));
    // A BARE instantiation (`f<T>` root, `arr[g<T>]` index — OXC `TSInstantiationExpression`,
    // no trailing call) IS flagged: official svelte@5.56.3 REJECTS both (`js_parse_error`),
    // so the structural fail-close agrees with official (it does NOT over-refuse here).
    assert!(
        lvalue_ts("f<T>"),
        "a bare `f<T>` instantiation root is flagged (official rejects it: js_parse_error)"
    );
    assert!(
        lvalue_ts("arr[g<T>]"),
        "a bare `g<T>` instantiation index is flagged (official rejects it: js_parse_error)"
    );
    // A CALL / new / tagged-template carrying TS type arguments — `arr[g<a,b>(c)]` (computed
    // index) / `f<a,b>(c)` (root), each an OXC `CallExpression` with `type_arguments` — IS
    // flagged: the TSX-strip lane would otherwise DELETE the type arguments and emit a DIVERGENT
    // index (`arr[g(c)]`), whereas official parses the plain-JS relational form. Failing closed
    // is never-wrong (the exact relational emit stays D-26).
    assert!(
        lvalue_ts("arr[g<a,b>(c)]"),
        "a type-argument call-index must be flagged (the TSX-strip lane would emit a divergent index)"
    );
    assert!(
        lvalue_ts("f<a,b>(c)"),
        "a type-argument call root must be flagged (the TSX-strip lane would emit a divergent index)"
    );
    // A TS-only node embedded in a SUB-expression of the index — a typed arrow / function-expr
    // param, or a typed local inside an IIFE body — is also flagged: the surrounding JS is
    // valid, so the TSX-strip lane would DELETE the annotation and emit a divergent setter,
    // whereas official parses it as plain JS and rejects the TS. The wholesale scan closes this
    // class by construction (any TS / non-ECMAScript node), not per-form enumeration.
    assert!(
        lvalue_ts("arr[((x: number) => x)(0)]"),
        "a typed arrow param inside the index must be flagged (the strip lane would delete `: number`)"
    );
    assert!(
        lvalue_ts("arr[(function(y: number){ return y; })(0)]"),
        "a typed function-expr param inside the index must be flagged"
    );
    assert!(
        lvalue_ts("arr[(() => { const k: number = 0; return k; })()]"),
        "a typed local inside an IIFE-body index must be flagged"
    );
    // ── NOT TS anywhere (false) — clean lvalues ──
    assert!(!lvalue_ts("name"));
    assert!(!lvalue_ts("(name)"));
    assert!(!lvalue_ts("o.x"));
    assert!(!lvalue_ts("arr[i]"));
    assert!(!lvalue_ts("obj.a.b"));
    // A plain CALL index WITHOUT type arguments (`f(c)` — an OXC `CallExpression` with no
    // `type_arguments`) is plain JS and stays UNflagged: only the type-argument class fails
    // closed, never all calls.
    assert!(
        !lvalue_ts("arr[f(c)]"),
        "a plain `f(c)` call-index (no type arguments) must NOT be flagged TS (no over-refusal)"
    );
    // A plain (untyped) IIFE index has NO TS node and stays UNflagged — the wholesale scan is
    // precise (valid JS sub-expressions are never over-refused).
    assert!(
        !lvalue_ts("arr[(() => 0)()]"),
        "a plain untyped IIFE index must NOT be flagged TS (no over-refusal)"
    );
    // A SEQUENCE (function-pair) target is NOT an lvalue spine — excluded even if an element
    // carries TS (the plain-JS function-pair lane owns that rejection).
    assert!(!lvalue_ts("get, set"));
}

#[test]
fn bind_target_fact_carries_the_consolidated_bundle() {
    // F3: the SINGLE `BindTargetFact` carries EVERY datum the bind consumers previously
    // re-derived with a per-consumer reparse — kind, sequence presence, TS-wrapper
    // validity, root identifier, and the plain-JS function-pair slices — from ONE parse.
    let alloc = Allocator::default();

    // A member target: kind=Member, root="o", not a sequence, no function pair.
    let member = BindTargetFact::from_source(&alloc, "o.x.y");
    assert_eq!(member.kind, Some(BindTargetKind::Member));
    assert_eq!(member.root_ident.as_deref(), Some("o"));
    assert!(!member.is_sequence);
    assert!(member.function_pair.is_none());

    // A two-element function-pair: kind=FunctionPair, is_sequence, the two element slices.
    let pair = BindTargetFact::from_source(&alloc, "get, set");
    assert_eq!(pair.kind, Some(BindTargetKind::FunctionPair));
    assert!(pair.is_sequence);
    assert_eq!(
        pair.function_pair,
        Some(("get".to_string(), "set".to_string()))
    );

    // A 3-element sequence: is_sequence=true (the F1 identifier/member-only policy signal)
    // but kind=None and NO valid two-element function pair.
    let triple = BindTargetFact::from_source(&alloc, "a, b, c");
    assert!(triple.is_sequence);
    assert_eq!(triple.kind, None);
    assert!(triple.function_pair.is_none());

    // A non-lvalue: every field empty/false.
    let non_lvalue = BindTargetFact::from_source(&alloc, "f()");
    assert_eq!(non_lvalue.kind, None);
    assert!(!non_lvalue.is_sequence);
    assert!(non_lvalue.root_ident.is_none());
    assert!(non_lvalue.function_pair.is_none());
}

#[test]
fn bind_target_keypath_matches_official_extract_all_identifiers() {
    // Finding A (R4): `target_keypath` mirrors svelte's
    // `extract_all_identifiers_from_expression` keypath — the `bind:group` accumulator
    // grouping identity. Pinned, structural (NEVER raw-source): a bare identifier and a
    // static-member chain serialize their dotted names; a DIRECT identifier/literal computed
    // index is bracketed (`[i]` / `["x"]` / `[0]`); a NON-TRIVIAL index (`i+j`, `f()`, `b.c`)
    // surfaces its inner identifiers as plain VALUE-position names, so it is OPERATOR- and
    // WHITESPACE-insensitive.
    let alloc = Allocator::default();
    let keypath = |s: &str| BindTargetFact::from_source(&alloc, s).target_keypath;

    assert_eq!(keypath("v").as_deref(), Some("v"));
    assert_eq!(keypath("o.x.y").as_deref(), Some("o.x.y"));
    assert_eq!(keypath("a[i]").as_deref(), Some("a.[i]"));
    assert_eq!(keypath("a[0]").as_deref(), Some("a.[0]"));
    // `a.x` (static) and `a["x"]` (computed string) stay DISTINCT — the distinction
    // official preserves.
    assert_eq!(keypath("a.x").as_deref(), Some("a.x"));
    assert_eq!(keypath("a[\"x\"]").as_deref(), Some("a.[\"x\"]"));
    assert_ne!(keypath("a.x"), keypath("a[\"x\"]"));
    // OPERATOR- and WHITESPACE-insensitive: `g[i+j]`, `g[i + j]`, `g[i*j]` collapse to ONE
    // key (the operator is not an identifier, so it never enters the keypath).
    assert_eq!(keypath("g[i+j]").as_deref(), Some("g.i.j"));
    assert_eq!(keypath("g[i + j]").as_deref(), Some("g.i.j"));
    assert_eq!(keypath("g[i*j]").as_deref(), Some("g.i.j"));
    assert_eq!(keypath("g[i+j]"), keypath("g[i*j]"));
    // A parenthesized index keys the same as the bare index (parens are transparent).
    assert_eq!(keypath("a[(i)]").as_deref(), Some("a.[i]"));
    // A computed call index surfaces the callee identifier in value position.
    assert_eq!(keypath("g[f()]").as_deref(), Some("g.f"));
    // NEGATIVE: the keypath is NOT the old per-index serialization (`a[i]`, never `a.i`-less)
    // and NEVER the raw source spelling.
    assert_ne!(keypath("g[i+j]").as_deref(), Some("g[i+j]"));
}

#[test]
fn signal_write_inside_switch_statement_is_rewritten() {
    // R3 (exhaustive statement traversal): a signal write inside a `switch`
    // statement's case body must be rewritten — the prior hand-enumerated walk
    // bailed on `SwitchStatement` and left the write RAW. Verified against
    // svelte@5.56.3 (a handler `switch (x) { case 1: count++ }` rewrites `count++`
    // to `$.update(count)`).
    let out = rewrite_with(
        "() => { switch (k) { case 1: count++; break; default: count = 0; } }",
        "count",
        BindingRuntimeKind::StateSignal { raw: false },
    );
    assert!(
        out.contains("$.update(count)"),
        "the switch-case signal update must be rewritten:\n{out}"
    );
    assert!(
        out.contains("$.set(count, 0)"),
        "the switch-default signal reassign must be rewritten:\n{out}"
    );
    // NEGATIVE: no raw unrewritten write survives.
    assert!(
        !out.contains("count++"),
        "no raw `count++` may survive the switch traversal:\n{out}"
    );
    assert!(
        !out.contains("count = 0"),
        "no raw `count = 0` may survive the switch traversal:\n{out}"
    );
}

#[test]
fn signal_write_inside_try_catch_finally_is_rewritten() {
    // R3: a signal write inside `try` / `catch` / `finally` bodies is rewritten.
    // The prior walk bailed on `TryStatement` entirely.
    let out = rewrite_with(
        "() => { try { count++; } catch (e) { count = 1; } finally { count += 2; } }",
        "count",
        BindingRuntimeKind::StateSignal { raw: false },
    );
    assert!(out.contains("$.update(count)"), "try body:\n{out}");
    assert!(out.contains("$.set(count, 1)"), "catch body:\n{out}");
    assert!(
        out.contains("$.set(count, $.get(count) + 2)"),
        "finally body:\n{out}"
    );
    assert!(!out.contains("count++"), "no raw try write:\n{out}");
}

#[test]
fn signal_write_inside_for_of_and_for_in_is_rewritten() {
    // R3: a signal write inside `for-of` / `for-in` bodies is rewritten; a `for-of`
    // LEFT binding of the SAME name shadows the signal (its write stays plain).
    let out = rewrite_with(
        "() => { for (const x of list) { count += x; } }",
        "count",
        BindingRuntimeKind::StateSignal { raw: false },
    );
    assert!(
        out.contains("$.set(count, $.get(count) + x)"),
        "for-of body signal write:\n{out}"
    );
    // A `for-of` LEFT binding named `count` shadows the signal.
    let shadowed = rewrite_with(
        "() => { for (const count of list) { count = 5; } }",
        "count",
        BindingRuntimeKind::StateSignal { raw: false },
    );
    assert!(
        shadowed.contains("count = 5"),
        "a shadowing for-of binding stays plain:\n{shadowed}"
    );
    assert!(
        !shadowed.contains("$.set(count"),
        "a shadowing for-of binding must NOT become $.set:\n{shadowed}"
    );
}

#[test]
fn signal_write_inside_do_while_and_throw_is_rewritten() {
    // R3: `do { count++ } while (cond)` and `throw f(count)` are traversed.
    let out = rewrite_with(
        "() => { do { count++; } while (count < 10); }",
        "count",
        BindingRuntimeKind::StateSignal { raw: false },
    );
    assert!(out.contains("$.update(count)"), "do-while body:\n{out}");
    assert!(
        out.contains("$.get(count) < 10"),
        "do-while test read:\n{out}"
    );
    let thrown = rewrite_with(
        "() => { throw count; }",
        "count",
        BindingRuntimeKind::StateSignal { raw: false },
    );
    assert!(
        thrown.contains("throw $.get(count)"),
        "throw argument read:\n{thrown}"
    );
}

#[test]
fn signal_read_inside_for_init_and_default_param_is_rewritten() {
    // R3: a signal read in a `for`-loop INIT and in a default-parameter expression
    // is rewritten (the prior walk skipped both).
    let for_init = rewrite_with(
        "() => { for (let i = count; i > 0; i--) {} }",
        "count",
        BindingRuntimeKind::StateSignal { raw: false },
    );
    assert!(
        for_init.contains("let i = $.get(count)"),
        "for-init read:\n{for_init}"
    );
    // A default param reads the outer signal.
    let default_param = rewrite_with(
        "() => { const g = (a = count) => a; return g(); }",
        "count",
        BindingRuntimeKind::StateSignal { raw: false },
    );
    assert!(
        default_param.contains("$.get(count)"),
        "default-param read:\n{default_param}"
    );
}

#[test]
fn signal_read_inside_labeled_and_class_body_is_rewritten() {
    // R3: a labeled statement body and a class method body are traversed.
    let labeled = rewrite_with(
        "() => { outer: for (;;) { count++; break outer; } }",
        "count",
        BindingRuntimeKind::StateSignal { raw: false },
    );
    assert!(
        labeled.contains("$.update(count)"),
        "labeled-statement body:\n{labeled}"
    );
    let class_body = rewrite_with(
        "() => { class C { m() { return count; } } return C; }",
        "count",
        BindingRuntimeKind::StateSignal { raw: false },
    );
    assert!(
        class_body.contains("$.get(count)"),
        "class method body read:\n{class_body}"
    );
}

// NOTE: the broad instance-script LOWERING (`lower_instance_declarations`) was
// removed in favor of the strict finite `SupportedInstanceScriptItem` allowlist
// (`classify_supported_instance_items` + `lower_simple_instance_item`). The
// removed-path characterization tests (TS-strip of a non-primitive `$state` init /
// a bare typed `let` / a `$props()` default; the one-hop object-state proxy follow
// inside an instance FUNCTION body) covered shapes that now FAIL CLOSED — a
// non-primitive `$state` init, a TS-annotated / unused bare `let`, a default-bearing
// `$props()`, and a top-level function are out-of-allowlist and refused at the
// classifier (asserted in `svelte_client_fail_matrix.rs` +
// `svelte_instance_script_boundary.rs`). The one-hop proxy follow for the SUPPORTED
// surface (a `$state`-write onclick arrow, e.g. `runes/proxy_gating`) is exercised by
// the template-side rewriter tests below + the topology golden.

#[test]
fn logical_assign_to_object_state_proxies() {
    // R9: `o ||= {b:2}` / `o ??= {}` / `o &&= {}` on object state carry the trailing
    // `, true` (the official `is_non_coercive_operator` set extends beyond `=`).
    // Verified against svelte@5.56.3 (`$.set(o, $.get(o) || { b: 2 }, true)`).
    let or_assign = rewrite_with("o ||= { b: 2 }", "o", BindingRuntimeKind::StateProxy);
    assert!(
        or_assign.contains("$.set(o, $.get(o) || { b: 2 }, true)"),
        "`||=` to object state must proxy:\n{or_assign}"
    );
    let nullish = rewrite_with("o ??= { b: 2 }", "o", BindingRuntimeKind::StateProxy);
    assert!(
        nullish.contains("$.set(o, $.get(o) ?? { b: 2 }, true)"),
        "`??=` to object state must proxy:\n{nullish}"
    );
    let and_assign = rewrite_with("o &&= { b: 2 }", "o", BindingRuntimeKind::StateProxy);
    assert!(
        and_assign.contains("$.set(o, $.get(o) && { b: 2 }, true)"),
        "`&&=` to object state must proxy:\n{and_assign}"
    );
    // NEGATIVE: a COERCIVE compound (`+=`) never proxies.
    let plus = rewrite_with("o += 1", "o", BindingRuntimeKind::StateProxy);
    assert!(
        !plus.contains(", true)"),
        "a coercive `+=` must NOT proxy:\n{plus}"
    );
}

#[test]
fn destructured_state_object_classifies_as_advanced() {
    // R1: a destructured `let { a } = $state({a:1})` is classified ADVANCED (5g) —
    // NOT a basic supported state declarator (which would route into
    // `lower_state_declarator` and panic). The full fail-closed is asserted at the
    // `compile_client` integration level; here we pin the shape gate.
    assert!(
        matches!(
            state_decl_shape("let { a } = $state({ a: 1 });"),
            StateDeclShape::Advanced { .. }
        ),
        "a destructured object `$state` is advanced (fail-closed)"
    );
    assert!(
        matches!(
            state_decl_shape("let [x] = $state([1]);"),
            StateDeclShape::Advanced { .. }
        ),
        "a destructured array `$state` is advanced (fail-closed)"
    );
    // NEGATIVE: a plain identifier `$state` is still a basic supported declarator.
    assert!(
        matches!(
            state_decl_shape("let c = $state(0);"),
            StateDeclShape::Identifier
        ),
        "a plain identifier `$state` stays basic-supported"
    );
}

#[test]
fn state_init_unshadowed_undefined_is_a_primitive_literal() {
    // `$state(undefined)` with NO local `undefined` shadow is the void-0 primitive
    // form — supported. (And the no-arg `$state()` is also the undefined primitive.)
    assert!(
        matches!(
            state_decl_shape("let x = $state(undefined);"),
            StateDeclShape::Identifier
        ),
        "unshadowed $state(undefined) is the primitive void-0 form"
    );
    assert!(
        matches!(
            state_decl_shape("let x = $state();"),
            StateDeclShape::Identifier
        ),
        "no-arg $state() is the primitive void-0 form"
    );
}

#[test]
fn state_init_shadowed_undefined_is_not_a_primitive_literal() {
    // `let undefined = $state(0); let x = $state(undefined)` — `undefined` is SHADOWED
    // by a local, so the `$state(undefined)` init is a real reference (official reads
    // the shadow), NOT the void-0 primitive. It must fail closed as ADVANCED (5g) so
    // Verter never emits the divergent `$.state(undefined)` (raw signal box) where
    // official emits `$.state($.proxy($.get(undefined)))`.
    assert!(
        matches!(
            state_decl_shape("let undefined = $state(0); let x = $state(undefined);"),
            StateDeclShape::Advanced { .. }
        ),
        "a $state over a SHADOWED `undefined` is advanced (fail-closed)"
    );
    // Even a plain non-state `undefined` shadow makes the reference non-literal.
    assert!(
        matches!(
            state_decl_shape("let undefined = 5; let x = $state(undefined);"),
            StateDeclShape::Advanced { .. }
        ),
        "a $state over a plain-local `undefined` shadow is advanced (fail-closed)"
    );
}

#[test]
fn state_init_nan_and_infinity_are_not_primitive_literals() {
    // NaN / Infinity are bare global identifier references — official wraps them in
    // `$.proxy(…)` (the deep-reactive non-literal form), so they are NOT primitive
    // literals and fail closed as advanced (never the bare `$.state(NaN)` literal form).
    assert!(matches!(
        state_decl_shape("let x = $state(NaN);"),
        StateDeclShape::Advanced { .. }
    ));
    assert!(matches!(
        state_decl_shape("let x = $state(Infinity);"),
        StateDeclShape::Advanced { .. }
    ));
}

#[test]
fn script_uses_effect_detects_top_level_effect() {
    let alloc = Allocator::default();
    assert!(expr_emit::script_uses_effect(
        &alloc,
        "let c = $state(0); $effect(() => { console.log(c); });"
    ));
    assert!(
        !expr_emit::script_uses_effect(&alloc, "let c = $state(0); let d = $derived(c * 2);"),
        "a script with $derived but no $effect must not require push/pop"
    );
}
