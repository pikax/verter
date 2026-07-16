//! Unit tests for the Svelte client const-fold evaluator (`mixed_chunk_fold` /
//! `mixed_chunk_nullish_wrap`) — extracted from the inline `#[cfg(test)]` module to keep
//! `reactive_fold.rs` under the file-size guard. Included via `#[path]`.

use super::super::expr::{BindingRuntimeKind, BindingTable, ScopeGraph};
use super::super::reactive_fold_tristate::{ChunkFold, ConstFoldRefuse, LiveFallbackReason};

// ── mixed-template constant folding (`build_template_chunk` evaluate-fold) ──
//
// A mixed-chunk whose `scope.evaluate(value)` is statically KNOWN + byte-exact folds to the
// JS-`String()`-coerced value (`(value ?? '') + ''`); a known-but-not-byte-exact chunk
// LIVE-falls-back (ledgered); a compile-time-THROW chunk REFUSES; a non-statically-known
// chunk (a live signal, a `$props()` prop, a member, a call, a sequence) stays a plain live
// interpolation. This is the tri-state const-fold contract over a faithful port of official
// `svelte@5.56.3`'s `Evaluation` class (`phases/scope.js`) driven by the
// `build_template_chunk` evaluate-fold (`shared/utils.js`). Every expected value below was
// captured from the pinned compiler (the `id="a {expr} b"` form over a demoted
// `let d = $state(<lit>)`).

/// The tri-state classification of `expr` over the instance `instance`.
fn classify(expr: &str, instance: &str) -> ChunkFold {
    // A root scope carrying one demoted `$state` binding `d` (a `PlainLocal`).
    let mut bindings = BindingTable::new();
    let (mut scopes, root) = ScopeGraph::with_root();
    let id = bindings.push(super::super::expr::BindingInfo {
        name: "d".to_string(),
        scope: root,
        kind: BindingRuntimeKind::PlainLocal,
        state: None,
    });
    scopes.declare(root, "d", id);
    super::mixed_chunk_fold(expr, root, &bindings, &scopes, Some(instance))
}

#[test]
fn prepared_evaluator_reuses_one_initializer_index_across_interpolations() {
    use oxc_allocator::Allocator;
    use oxc_ast::ast::Statement;

    let alloc = Allocator::default();
    let instance = super::super::expr::reparse_module(&alloc, "const A = 1; const B = 2;")
        .expect("instance script parses");
    let first = super::super::expr::reparse_module(&alloc, "(A)").expect("first expression parses");
    let second =
        super::super::expr::reparse_module(&alloc, "(B)").expect("second expression parses");
    let Statement::ExpressionStatement(first) = &first.body[0] else {
        panic!("first carrier is an expression statement");
    };
    let Statement::ExpressionStatement(second) = &second.body[0] else {
        panic!("second carrier is an expression statement");
    };
    let bindings = BindingTable::new();
    let (scopes, root) = ScopeGraph::with_root();
    let evaluator = super::PreparedChunkEvaluator::new(&bindings, &scopes, Some(&instance));

    assert_eq!(evaluator.top_level_init_count(), 2);
    assert_eq!(
        evaluator.fold(&first.expression, root),
        ChunkFold::Fold("1".into())
    );
    assert_eq!(
        evaluator.fold(&second.expression, root),
        ChunkFold::Fold("2".into())
    );
    assert_eq!(
        evaluator.nullish_wrap(&first.expression, root, false),
        super::NullishCoalesce::None
    );
    assert_eq!(
        evaluator.top_level_init_count(),
        2,
        "multiple fold decisions reuse the same immutable initializer index"
    );
}

/// `Some(folded_literal)` when `expr` FOLDS exactly; `None` for a `Live` chunk (plain or
/// ledgered live-fallback). PANICS on a `Refuse` (a throwing chunk must be asserted with
/// [`refuse_reason`], never via this helper — keeps a wrong-classification loud).
fn fold(expr: &str, instance: &str) -> Option<String> {
    match classify(expr, instance) {
        ChunkFold::Fold(s) => Some(s),
        ChunkFold::Live { .. } => None,
        ChunkFold::Refuse(r) => {
            panic!(
                "expected a fold / live for `{expr}`, got Refuse({}): {}",
                r.label(),
                r.label()
            )
        }
    }
}

/// The [`ConstFoldRefuse`] reason when `expr` REFUSES (a compile-time throw); PANICS if it
/// folds or lives instead (so a missed refusal is loud).
fn refuse_reason(expr: &str, instance: &str) -> ConstFoldRefuse {
    match classify(expr, instance) {
        ChunkFold::Refuse(r) => r,
        other => panic!("expected Refuse for `{expr}`, got {other:?}"),
    }
}

/// The [`LiveFallbackReason`] when `expr` is a LEDGERED live-fallback; PANICS if it folds,
/// refuses, or is a plain (un-ledgered) live chunk (so a missed ledger reason is loud).
fn live_reason(expr: &str, instance: &str) -> LiveFallbackReason {
    match classify(expr, instance) {
        ChunkFold::Live {
            ledger: Some(reason),
        } => reason,
        other => panic!("expected a ledgered LiveFallback for `{expr}`, got {other:?}"),
    }
}

#[test]
fn demoted_state_primitive_literal_folds() {
    assert_eq!(
        fold("d", "let d = $state(5);").as_deref(),
        Some("5"),
        "a demoted $state(5) folds to '5'"
    );
    assert_eq!(
        fold("d", "let d = $state('hi');").as_deref(),
        Some("hi"),
        "a demoted $state('hi') folds to 'hi'"
    );
    assert_eq!(
        fold("d", "let d = $state(true);").as_deref(),
        Some("true"),
        "a demoted $state(true) folds to 'true'"
    );
    assert_eq!(
        fold("d", "let d = $state(null);").as_deref(),
        Some(""),
        "a demoted $state(null) folds to '' (null ?? '')"
    );
    assert_eq!(
        fold("d", "let d = $state();").as_deref(),
        Some(""),
        "a demoted $state() (undefined) folds to ''"
    );
    assert_eq!(
        fold("d", "let d = $state(-1);").as_deref(),
        Some("-1"),
        "a demoted $state(-1) folds to '-1'"
    );
}

#[test]
fn known_binary_logical_conditional_unary_chunks_fold() {
    // Official `scope.evaluate` folds ANY statically-known chunk, not only a bare
    // identifier. Each expected value is the pinned-svelte fold over `let d = $state(...)`.
    let d5 = "let d = $state(5);";
    // Binary — arithmetic + string concat + comparison + bitwise.
    assert_eq!(fold("d + 1", d5).as_deref(), Some("6"), "d + 1 → 6");
    assert_eq!(fold("d + 'x'", d5).as_deref(), Some("5x"), "d + 'x' → 5x");
    assert_eq!(fold("d | 2", d5).as_deref(), Some("7"), "d | 2 → 7");
    assert_eq!(fold("d % 3", d5).as_deref(), Some("2"), "d % 3 → 2");
    assert_eq!(fold("d > 1", d5).as_deref(), Some("true"), "d > 1 → true");
    assert_eq!(
        fold("(d + 1) * 2", d5).as_deref(),
        Some("12"),
        "(d+1)*2 → 12"
    );
    assert_eq!(
        fold("d + 'x'", "let d = $state('a');").as_deref(),
        Some("ax"),
        "string + string concat"
    );
    // Logical — &&, ||, ??.
    assert_eq!(
        fold("d && 'on'", "let d = $state(true);").as_deref(),
        Some("on"),
        "true && 'on' → on"
    );
    assert_eq!(
        fold("d || 'fallback'", "let d = $state(0);").as_deref(),
        Some("fallback"),
        "0 || 'fallback' → fallback"
    );
    assert_eq!(
        fold("d ?? 'def'", "let d = $state(null);").as_deref(),
        Some("def"),
        "null ?? 'def' → def"
    );
    assert_eq!(
        fold("d || 'x' || 'y'", "let d = $state(0);").as_deref(),
        Some("x"),
        "0 || 'x' || 'y' → x"
    );
    // Conditional — known test selects the taken branch.
    assert_eq!(
        fold("d ? 'a' : 'b'", "let d = $state(true);").as_deref(),
        Some("a"),
        "true ? 'a' : 'b' → a"
    );
    assert_eq!(
        fold("d > 3 ? d + 1 : 0", d5).as_deref(),
        Some("6"),
        "5 > 3 ? 6 : 0 → 6"
    );
    // Unary — !, -, +, typeof, void.
    assert_eq!(fold("-d", d5).as_deref(), Some("-5"), "-d → -5");
    assert_eq!(
        fold("!d", "let d = $state(true);").as_deref(),
        Some("false"),
        "!true → false"
    );
    assert_eq!(
        fold("typeof d", d5).as_deref(),
        Some("number"),
        "typeof 5 → number"
    );
    assert_eq!(
        fold("typeof d", "let d = $state('s');").as_deref(),
        Some("string"),
        "typeof 's' → string"
    );
    assert_eq!(
        fold("void d", d5).as_deref(),
        Some(""),
        "void d → undefined → ''"
    );
    // Template literal — interpolating a known value folds.
    assert_eq!(
        fold("`x${d}y`", d5).as_deref(),
        Some("x5y"),
        "a template literal interpolating a known value folds to x5y"
    );
    // Pure-global call + global constant member fold (official `globals` table).
    assert_eq!(
        fold("String(5)", d5).as_deref(),
        Some("5"),
        "String(5) → 5 (pure-global call)"
    );
    assert_eq!(
        fold("Math.PI", d5).as_deref(),
        Some("3.141592653589793"),
        "Math.PI → its JS spelling (global constant member)"
    );
}

#[test]
fn non_statically_known_chunks_do_not_fold() {
    let d5 = "let d = $state(5);";
    // A MEMBER access on a binding is NOT a global constant ⇒ official MemberExpression
    // arm yields UNKNOWN ⇒ stays live.
    assert_eq!(fold("d.x", d5), None, "a member chunk is not folded (live)");
    // A SEQUENCE has no official `Evaluation` arm ⇒ default UNKNOWN ⇒ stays live.
    assert_eq!(
        fold("(1, d)", d5),
        None,
        "a sequence chunk is not folded (official has no SequenceExpression arm)"
    );
    // A general CALL (non-global callee) is UNKNOWN ⇒ stays live.
    assert_eq!(
        fold("d.toString()", d5),
        None,
        "a method call is not folded (live)"
    );
    // A name with NO `$state` declarator (an unknown / global) does not fold.
    assert_eq!(
        fold("d", "let other = $state(5);"),
        None,
        "an unmatched name does not fold"
    );
    // A NON-primitive `$state` init has no known scalar value ⇒ not folded (refused
    // upstream anyway, but the fold predicate must not claim a known value).
    assert_eq!(
        fold("d", "let d = $state({});"),
        None,
        "a non-primitive $state init is not foldable"
    );
    assert_eq!(
        fold("d", "let d = $state(makeIt());"),
        None,
        "a call-init $state is not foldable"
    );
    // A binary chunk that mixes a known binding with an UNKNOWN member stays live
    // (one operand is not known ⇒ the binary is not known).
    assert_eq!(
        fold("d + d.x", d5),
        None,
        "a binary with an unknown member operand stays live"
    );
}

// ── live mixed-template `?? ''` coercion (`build_template_chunk` is_defined rule) ──
//
// A LIVE (un-folded) mixed-template chunk is `?? ''`-wrapped per official's
// `evaluated.is_defined`: a provably-defined value (`n + 1`, a number; `n > 1`, a
// boolean) is emitted RAW; an undecided value (`n`, `n.x`, `n && 1`) gets `?? ''`,
// parenthesized for a `&&`/`||` operand. A MEMOIZED chunk (`$N` slot) is always
// `Bare`. Verified against pinned svelte@5.56.3 (probe8/probe9/probe10).

fn wrap(expr: &str, is_memoized: bool) -> super::NullishCoalesce {
    // A scope with a LIVE `$state` signal `n` (a `StateSignal`) — the un-folded subject.
    let mut bindings = BindingTable::new();
    let (mut scopes, root) = ScopeGraph::with_root();
    let id = bindings.push(super::super::expr::BindingInfo {
        name: "n".to_string(),
        scope: root,
        kind: BindingRuntimeKind::StateSignal { raw: false },
        state: None,
    });
    scopes.declare(root, "n", id);
    super::mixed_chunk_nullish_wrap(
        expr,
        root,
        &bindings,
        &scopes,
        Some("let n = $state(0);"),
        is_memoized,
    )
}

#[test]
fn defined_live_chunks_emit_raw_without_coalesce() {
    use super::NullishCoalesce::None as Raw;
    // Arithmetic / string-concat / comparison / conditional / unary / typeof results are
    // provably defined ⇒ NO `?? ''`.
    assert_eq!(wrap("n + 1", false), Raw, "n + 1 is a number → raw");
    assert_eq!(wrap("n + 'x'", false), Raw, "n + 'x' is a string → raw");
    assert_eq!(wrap("n > 1", false), Raw, "n > 1 is a boolean → raw");
    assert_eq!(
        wrap("n ? 1 : 2", false),
        Raw,
        "conditional of defined branches → raw"
    );
    assert_eq!(wrap("-n", false), Raw, "-n is a number → raw");
    assert_eq!(wrap("!n", false), Raw, "!n is a boolean → raw");
    assert_eq!(wrap("typeof n", false), Raw, "typeof n is a string → raw");
}

#[test]
fn undecided_live_chunks_coalesce_with_correct_parens() {
    use super::NullishCoalesce::{Bare, Parenthesized};
    // A bare signal / member / sequence is not provably defined ⇒ `?? ''`, no parens.
    assert_eq!(wrap("n", false), Bare, "a live signal read → `?? ''`");
    assert_eq!(wrap("n.x", false), Bare, "a member → `?? ''`");
    assert_eq!(
        wrap("(n, 1)", false),
        Bare,
        "a sequence carries its own parens → bare `?? ''`"
    );
    assert_eq!(
        wrap("n ?? 1", false),
        Bare,
        "a `??` chain needs no extra parens"
    );
    // A `&&` / `||` operand needs parens before `?? ''` (JS forbids the bare mix).
    assert_eq!(
        wrap("n && 1", false),
        Parenthesized,
        "a `&&` operand is parenthesized → `(n && 1) ?? ''`"
    );
    assert_eq!(
        wrap("n || 1", false),
        Parenthesized,
        "a `||` operand is parenthesized → `(n || 1) ?? ''`"
    );
}

#[test]
fn memoized_chunks_are_always_bare_coalesce() {
    use super::NullishCoalesce::Bare;
    // A memoized chunk is the `$N` slot — official evaluates that identifier to UNKNOWN, so
    // it is always `$N ?? ''`, never raw and never parenthesized, regardless of the inner
    // expression's type (`String(n)` is a string but the memo slot is unknown).
    assert_eq!(
        wrap("String(n)", true),
        Bare,
        "memoized String(n) → `$0 ?? ''`"
    );
    assert_eq!(
        wrap("n.toString()", true),
        Bare,
        "memoized method call → `$0 ?? ''`"
    );
    assert_eq!(
        wrap("n && f()", true),
        Bare,
        "memoized `&&` with a call → `$0 ?? ''` (no parens)"
    );
}

// ── comprehensive evaluator verification (residual-probe closure) ──
//
// Every expected value below is the EXACT folded output of the pinned
// `svelte@5.56.3` compiler over `id="a {EXPR} b"` (the multi-chunk
// `build_template_chunk` evaluate-fold) with the matching demoted-`$state`
// subject — captured by running the pinned compiler and reading the cooked
// `set_attribute(div, 'id', 'a <V> b')` literal. They lock the faithful port to
// official's `scope.js` `Evaluation` across the coercion edges (number / string /
// boolean / bigint), the FULL globals table, every operator (binary / logical /
// conditional / unary incl. delete / typeof / void), and the tricky values
// (Infinity / NaN / -0 / 0.1+0.2 / 2**53). A `None` means official keeps the
// chunk LIVE (does not fold).
//
// `d` is the demoted `$state` subject; the second argument supplies its
// initializer, so each row picks the literal kind it needs.

#[test]
fn number_coercion_edges_match_official() {
    // JS `Number(string)` coercion via arithmetic (`d - 0`) over a demoted string
    // `$state` — official folds each to the JS `Number()` value, NOT Rust's parse.
    let n = |lit: &str| fold("(d - 0)", &format!("let d = $state({lit});"));
    assert_eq!(n("'0x10'").as_deref(), Some("16"), "hex string → 16");
    assert_eq!(n("'0o17'").as_deref(), Some("15"), "octal string → 15");
    assert_eq!(n("'0b101'").as_deref(), Some("5"), "binary string → 5");
    assert_eq!(n("''").as_deref(), Some("0"), "empty string → 0");
    assert_eq!(n("'   '").as_deref(), Some("0"), "whitespace string → 0");
    assert_eq!(n("' 15 '").as_deref(), Some("15"), "trimmed ' 15 ' → 15");
    assert_eq!(n("'Infinity'").as_deref(), Some("Infinity"), "'Infinity'");
    assert_eq!(
        n("'-Infinity'").as_deref(),
        Some("-Infinity"),
        "'-Infinity'"
    );
    assert_eq!(n("'+1.5e3'").as_deref(), Some("1500"), "'+1.5e3' → 1500");
    assert_eq!(n("'.5'").as_deref(), Some("0.5"), "'.5' → 0.5");
    assert_eq!(n("'5.'").as_deref(), Some("5"), "'5.' → 5");
    assert_eq!(n("'-0'").as_deref(), Some("0"), "'-0' → 0 (String(-0))");
    // INVALID strings coerce to NaN (the bug: Rust `.parse()` rejected differently).
    assert_eq!(n("'a 15 b'").as_deref(), Some("NaN"), "'a 15 b' → NaN");
    assert_eq!(n("'+'").as_deref(), Some("NaN"), "sign-only → NaN");
}

#[test]
fn string_coercion_edges_match_official() {
    // JS `String(x)` coercion via concat (`d + ''`) over the various subject kinds.
    let s = |lit: &str| fold("(d + '')", &format!("let d = $state({lit});"));
    assert_eq!(s("-0").as_deref(), Some("0"), "String(-0) → '0'");
    assert_eq!(s("1/0").as_deref(), Some("Infinity"), "String(Infinity)");
    assert_eq!(s("-1/0").as_deref(), Some("-Infinity"), "String(-Infinity)");
    assert_eq!(s("0/0").as_deref(), Some("NaN"), "String(NaN) → 'NaN'");
    assert_eq!(s("true").as_deref(), Some("true"), "String(true)");
    assert_eq!(s("false").as_deref(), Some("false"), "String(false)");
    assert_eq!(s("null").as_deref(), Some("null"), "String(null) → 'null'");
    assert_eq!(s("5n").as_deref(), Some("5"), "String(5n) → '5'");
    assert_eq!(s("-5n").as_deref(), Some("-5"), "String(-5n) → '-5'");
}

#[test]
fn bigint_folds_concrete() {
    // A BigInt is a CONCRETE value: it folds on its own and through arithmetic /
    // typeof, matching official (`NUMBER` includes BigInt but a literal stores `1n`).
    assert_eq!(
        fold("d", "let d = $state(1n);").as_deref(),
        Some("1"),
        "a demoted $state(1n) folds to '1'"
    );
    assert_eq!(
        fold("d + 1n", "let d = $state(5n);").as_deref(),
        Some("6"),
        "5n + 1n → 6n → '6'"
    );
    assert_eq!(
        fold("d * 2n", "let d = $state(5n);").as_deref(),
        Some("10"),
        "5n * 2n → 10n"
    );
    assert_eq!(
        fold("typeof d", "let d = $state(5n);").as_deref(),
        Some("bigint"),
        "typeof 5n → 'bigint'"
    );
    assert_eq!(
        fold("-d", "let d = $state(5n);").as_deref(),
        Some("-5"),
        "-5n → -5n → '-5'"
    );
    // BigInt + String concatenates (`5n + 'x'` → '5x').
    assert_eq!(
        fold("d + 'x'", "let d = $state(5n);").as_deref(),
        Some("5x"),
        "5n + 'x' → '5x'"
    );
    // Mixing BigInt with a Number in arithmetic THROWS in JS → official compile-fails →
    // Verter REFUSES (never live code that would crash at runtime).
    assert_eq!(
        refuse_reason("d + 1", "let d = $state(5n);"),
        ConstFoldRefuse::BigIntMixedArith,
        "5n + 1 throws → Refuse(BigIntMixedArith)"
    );
    // A bare unary `+` on a BigInt throws → Refuse.
    assert_eq!(
        refuse_reason("+d", "let d = $state(5n);"),
        ConstFoldRefuse::BigIntUnaryPlus,
        "+5n throws → Refuse(BigIntUnaryPlus)"
    );
}

#[test]
fn bigint_small_shift_and_exponent_fold_to_exact_value() {
    // A SMALL BigInt shift / exponent folds to the exact JS BigInt value (the result fits
    // far under V8's 2^30-bit limit). Every value is the pinned-svelte fold over a demoted
    // `let d = $state(<lit>)` (`1n << 4n` === `16n`, `2n ** 10n` === `1024n`, …).
    assert_eq!(
        fold("d << 4n", "let d = $state(1n);").as_deref(),
        Some("16"),
        "1n << 4n → 16n"
    );
    assert_eq!(
        fold("d >> 2n", "let d = $state(256n);").as_deref(),
        Some("64"),
        "256n >> 2n → 64n"
    );
    assert_eq!(
        fold("d ** 10n", "let d = $state(2n);").as_deref(),
        Some("1024"),
        "2n ** 10n → 1024n"
    );
    assert_eq!(
        fold("d ** 0n", "let d = $state(7n);").as_deref(),
        Some("1"),
        "7n ** 0n → 1n"
    );
    // A negative-operand shift stays arithmetic (sign-preserving) BigInt — `-5n << 3n` is
    // `-40n`, `-256n >> 2n` is `-64n`.
    assert_eq!(
        fold("d << 3n", "let d = $state(-5n);").as_deref(),
        Some("-40"),
        "-5n << 3n → -40n"
    );
    assert_eq!(
        fold("d >> 2n", "let d = $state(-256n);").as_deref(),
        Some("-64"),
        "-256n >> 2n → -64n (arithmetic shift)"
    );
    // `0n << huge` is `0n` (no bits → no allocation → never exceeds the size limit).
    assert_eq!(
        fold("d << 4294967296n", "let d = $state(0n);").as_deref(),
        Some("0"),
        "0n << 4294967296n → 0n (no allocation)"
    );
}

#[test]
fn bigint_negative_shift_folds_via_js_bigint_semantics() {
    // A NEGATIVE BigInt shift count is VALID in JS (no throw): `a << -b` ≡ `a >> b` and
    // `a >> -b` ≡ `a << b` (arbitrary-precision arithmetic shift). Verter must fold the
    // CORRECT value via JS BigInt semantics — NOT fall to the Number 32-bit shift path
    // (which would mask the count to `& 31` and produce a WRONG value). Pinned-svelte folds
    // each of these to a literal.
    assert_eq!(
        fold("d << -2n", "let d = $state(256n);").as_deref(),
        Some("64"),
        "256n << -2n ≡ 256n >> 2n → 64n"
    );
    assert_eq!(
        fold("d >> -2n", "let d = $state(256n);").as_deref(),
        Some("1024"),
        "256n >> -2n ≡ 256n << 2n → 1024n"
    );
    assert_eq!(
        fold("d << -2n", "let d = $state(1n);").as_deref(),
        Some("0"),
        "1n << -2n ≡ 1n >> 2n → 0n"
    );
    // A negative-OPERAND value with a negative shift: `-8n << -1n` ≡ `-8n >> 1n` → `-4n`
    // (arithmetic right shift rounds toward -inf).
    assert_eq!(
        fold("d << -1n", "let d = $state(-8n);").as_deref(),
        Some("-4"),
        "-8n << -1n ≡ -8n >> 1n → -4n"
    );
}

#[test]
fn bigint_oversize_shift_and_exponent_refuse() {
    // A BigInt `<<` / `**` whose RESULT would exceed V8's `kMaxLengthBits` (2^30 bits)
    // THROWS `RangeError: Maximum BigInt size exceeded` at compile time → official
    // compile-FAILS → Verter must REFUSE (never live code, NEVER attempt the multi-gigabit
    // allocation — the size guard is a cheap bit-length check). Probed: official rejects
    // `1n << 4294967296n` and `2n ** 4294967296n`.
    assert_eq!(
        refuse_reason("d << 4294967296n", "let d = $state(1n);"),
        ConstFoldRefuse::BigIntMaxSizeExceeded,
        "1n << 4294967296n exceeds 2^30 bits → Refuse(BigIntMaxSizeExceeded)"
    );
    assert_eq!(
        refuse_reason("d ** 4294967296n", "let d = $state(2n);"),
        ConstFoldRefuse::BigIntMaxSizeExceeded,
        "2n ** 4294967296n exceeds 2^30 bits → Refuse(BigIntMaxSizeExceeded)"
    );
    // A negative shift whose effective opposite-direction LEFT shift would overflow also
    // refuses: `1n >> -4294967296n` ≡ `1n << 4294967296n` → exceeds → Refuse.
    assert_eq!(
        refuse_reason("d >> -4294967296n", "let d = $state(1n);"),
        ConstFoldRefuse::BigIntMaxSizeExceeded,
        "1n >> -4294967296n ≡ 1n << 4294967296n exceeds 2^30 bits → Refuse"
    );
    // A large non-power-of-two exponent is rejected exactly at the V8 boundary too
    // (`3n ** 4294967296n` is astronomically over 2^30 bits).
    assert_eq!(
        refuse_reason("d ** 4294967296n", "let d = $state(3n);"),
        ConstFoldRefuse::BigIntMaxSizeExceeded,
        "3n ** 4294967296n exceeds 2^30 bits → Refuse(BigIntMaxSizeExceeded)"
    );
}

#[test]
fn bigint_negative_exponent_still_refuses_distinctly() {
    // A NEGATIVE BigInt exponent throws `RangeError: Exponent must be positive` — a DISTINCT
    // throw from the oversize case. It must keep refusing with its own reason (not collapse
    // into the size-exceeded variant).
    assert_eq!(
        refuse_reason("d ** -1n", "let d = $state(2n);"),
        ConstFoldRefuse::BigIntNegativeExponent,
        "2n ** -1n throws (negative exponent) → Refuse(BigIntNegativeExponent)"
    );
    // BigInt `>>>` still refuses (BigInt has no unsigned right shift) — unchanged.
    assert_eq!(
        refuse_reason("d >>> 0n", "let d = $state(6n);"),
        ConstFoldRefuse::BigIntUnsignedShift,
        "6n >>> 0n throws → Refuse(BigIntUnsignedShift)"
    );
}

#[test]
fn tricky_number_values_match_official() {
    let d0 = "let d = $state(0);";
    assert_eq!(
        fold("(0.1 + 0.2)", d0).as_deref(),
        Some("0.30000000000000004"),
        "0.1 + 0.2 → its full f64 spelling"
    );
    assert_eq!(fold("(1/0)", d0).as_deref(), Some("Infinity"), "1/0");
    assert_eq!(fold("(-1/0)", d0).as_deref(), Some("-Infinity"), "-1/0");
    assert_eq!(fold("(0/0)", d0).as_deref(), Some("NaN"), "0/0 → NaN");
    // The `**` operator over Numbers is exponentiation (the same fdlibm `pow` as
    // `Math.pow`) — Rust's system libm is not provably bit-identical to V8's cross-platform
    // → LIVE-fallback (NOT a fold), even though `2 ** 53` is an exact integer power here.
    assert_eq!(
        live_reason("(2**53)", d0),
        LiveFallbackReason::TranscendentalLibm,
        "2**53 → LiveFallback(TranscendentalLibm)"
    );
    assert_eq!(fold("(-0)", d0).as_deref(), Some("0"), "-0 → '0'");
}

#[test]
fn full_globals_table_matches_official() {
    let d0 = "let d = $state(0);";
    let g = |e: &str| fold(e, d0);
    // Math.* TRANSCENDENTALS — Rust system libm vs V8 fdlibm is not provably bit-identical
    // cross-platform → LIVE-fallback (NOT a fold), per the architect's ExactFold boundary.
    let lr = |e: &str| live_reason(e, d0);
    assert_eq!(
        lr("Math.log(10)"),
        LiveFallbackReason::TranscendentalLibm,
        "Math.log(10) → LiveFallback(TranscendentalLibm)"
    );
    assert_eq!(
        lr("Math.atan2(1, 1)"),
        LiveFallbackReason::TranscendentalLibm,
        "Math.atan2(1,1) → LiveFallback"
    );
    assert_eq!(
        lr("Math.pow(2, 10)"),
        LiveFallbackReason::TranscendentalLibm,
        "Math.pow → LiveFallback"
    );
    assert_eq!(
        lr("Math.cbrt(27)"),
        LiveFallbackReason::TranscendentalLibm,
        "Math.cbrt → LiveFallback"
    );
    assert_eq!(
        lr("Math.log2(8)"),
        LiveFallbackReason::TranscendentalLibm,
        "Math.log2(8) → LiveFallback"
    );
    assert_eq!(
        lr("Math.log10(1000)"),
        LiveFallbackReason::TranscendentalLibm,
        "Math.log10 → LiveFallback"
    );
    // Math.* EXACT functions (IEEE-754-mandated / integer / bit ops) → fold.
    assert_eq!(
        g("Math.sqrt(16)").as_deref(),
        Some("4"),
        "Math.sqrt (IEEE exact)"
    );
    assert_eq!(g("Math.sign(-5)").as_deref(), Some("-1"), "Math.sign(-5)");
    assert_eq!(g("Math.clz32(1)").as_deref(), Some("31"), "Math.clz32(1)");
    assert_eq!(g("Math.imul(3, 4)").as_deref(), Some("12"), "Math.imul");
    assert_eq!(
        g("Math.f16round(1.1)").as_deref(),
        Some("1.099609375"),
        "Math.f16round(1.1)"
    );
    assert_eq!(g("Math.trunc(4.7)").as_deref(), Some("4"), "Math.trunc");
    assert_eq!(g("Math.min(3, 1, 2)").as_deref(), Some("1"), "Math.min");
    assert_eq!(g("Math.max(3, 1, 2)").as_deref(), Some("3"), "Math.max");
    // Number.* functions.
    assert_eq!(
        g("Number.isInteger(5)").as_deref(),
        Some("true"),
        "Number.isInteger(5)"
    );
    assert_eq!(
        g("Number.isInteger(5.5)").as_deref(),
        Some("false"),
        "Number.isInteger(5.5)"
    );
    assert_eq!(
        g("Number.isNaN(0/0)").as_deref(),
        Some("true"),
        "Number.isNaN(NaN)"
    );
    assert_eq!(
        g("Number.isFinite(1/0)").as_deref(),
        Some("false"),
        "Number.isFinite(Infinity)"
    );
    // `isSafeInteger` folds exactly over a clean literal arg; `9007199254740992` (2^53) is
    // NOT a safe integer (safe integers are ≤ 2^53 − 1). (A `2**53` arg would itself
    // live-fall-back as a transcendental and poison the call — exercised separately.)
    assert_eq!(
        g("Number.isSafeInteger(9007199254740992)").as_deref(),
        Some("false"),
        "Number.isSafeInteger(2^53) → false"
    );
    assert_eq!(
        g("Number.isSafeInteger(42)").as_deref(),
        Some("true"),
        "Number.isSafeInteger(42) → true"
    );
    assert_eq!(
        g("Number.parseFloat('3.14xy')").as_deref(),
        Some("3.14"),
        "Number.parseFloat('3.14xy')"
    );
    assert_eq!(
        g("Number.parseInt('0x1F')").as_deref(),
        Some("31"),
        "Number.parseInt('0x1F')"
    );
    assert_eq!(
        g("Number.parseInt('10', 2)").as_deref(),
        Some("2"),
        "Number.parseInt('10', 2)"
    );
    // String.* functions.
    assert_eq!(
        g("String.fromCharCode(65, 66)").as_deref(),
        Some("AB"),
        "String.fromCharCode(65,66)"
    );
    assert_eq!(
        g("String.fromCodePoint(128512)").as_deref(),
        Some("😀"),
        "String.fromCodePoint(128512)"
    );
    // Globals official does NOT fold stay LIVE.
    assert_eq!(
        g("BigInt(5)"),
        None,
        "BigInt(n) has no fn → not folded (live)"
    );
    assert_eq!(
        g("Math.hypot(3, 4)"),
        None,
        "Math.hypot not in table → live"
    );
    assert_eq!(g("Math.random()"), None, "Math.random has no fn → live");
}

#[test]
fn global_constants_match_official() {
    let d0 = "let d = $state(0);";
    assert_eq!(
        fold("Math.PI", d0).as_deref(),
        Some("3.141592653589793"),
        "Math.PI"
    );
    assert_eq!(
        fold("Math.E", d0).as_deref(),
        Some("2.718281828459045"),
        "Math.E"
    );
    assert_eq!(
        fold("Math.SQRT2", d0).as_deref(),
        Some("1.4142135623730951"),
        "Math.SQRT2"
    );
    // A member NOT in the global-constants table stays live.
    assert_eq!(
        fold("Number.MAX_SAFE_INTEGER", d0),
        None,
        "Number.MAX_SAFE_INTEGER is a member not in global_constants → live"
    );
}

#[test]
fn all_operators_fold_match_official() {
    let d0 = "let d = $state(0);";
    let o = |e: &str| fold(e, d0);
    // Bitwise / shift.
    assert_eq!(o("(1 << 4)").as_deref(), Some("16"), "1<<4");
    assert_eq!(o("(7 >> 1)").as_deref(), Some("3"), "7>>1");
    assert_eq!(o("(-1 >>> 0)").as_deref(), Some("4294967295"), "-1>>>0");
    assert_eq!(o("(5 & 3)").as_deref(), Some("1"), "5&3");
    assert_eq!(o("(5 | 2)").as_deref(), Some("7"), "5|2");
    assert_eq!(o("(5 ^ 1)").as_deref(), Some("4"), "5^1");
    assert_eq!(o("~5").as_deref(), Some("-6"), "~5");
    // Comparison / equality.
    assert_eq!(o("('a' < 'b')").as_deref(), Some("true"), "'a'<'b'");
    assert_eq!(o("(1 === '1')").as_deref(), Some("false"), "1==='1'");
    assert_eq!(o("(1 == '1')").as_deref(), Some("true"), "1=='1'");
    assert_eq!(
        o("(null == undefined)").as_deref(),
        Some("true"),
        "null==undefined"
    );
    // BigInt CROSS-TYPE comparison: the COERCING ops (`<`/`==`) need exact
    // mathematical-value comparison Verter's f64 coercion cannot prove byte-exact → LIVE.
    // STRICT equality (`===`) never coerces (distinct types) → exact `false` → fold.
    assert_eq!(
        live_reason("(d < 6)", "let d = $state(5n);"),
        LiveFallbackReason::BigIntNumberPrecisionCompare,
        "5n < 6 → LiveFallback (cross-type coercing compare)"
    );
    assert_eq!(
        fold("(d === 5)", "let d = $state(5n);").as_deref(),
        Some("false"),
        "5n === 5 → false (distinct types, no coercion → exact fold)"
    );
    assert_eq!(
        live_reason("(d == 5)", "let d = $state(5n);"),
        LiveFallbackReason::BigIntNumberPrecisionCompare,
        "5n == 5 → LiveFallback (cross-type coercing compare)"
    );
    // typeof / void.
    assert_eq!(o("typeof 5").as_deref(), Some("number"), "typeof 5");
    assert_eq!(o("typeof 'x'").as_deref(), Some("string"), "typeof 'x'");
    assert_eq!(o("typeof true").as_deref(), Some("boolean"), "typeof true");
    assert_eq!(o("void 5").as_deref(), Some(""), "void 5 → undefined → ''");
    // Nested conditional.
    assert_eq!(
        o("true ? (false ? 1 : 2) : 3").as_deref(),
        Some("2"),
        "nested conditional"
    );
}

// ── `delete` unit coverage (svelte refuses `delete <localvar>` in strict mode, so
//    the corpus cannot host it — verify the unary table directly). ──

#[test]
fn delete_of_known_operand_folds_to_true() {
    // Official `unary.delete: () => true` — a `delete` whose argument is KNOWN folds to
    // `true` (`delete 5` → `'true'`, verified against pinned svelte). A `delete d.x` over a
    // member is UNKNOWN-argument → 2-valued → NOT folded (the `non_statically_known` case).
    assert_eq!(
        fold("delete 5", "let d = $state(0);").as_deref(),
        Some("true"),
        "delete 5 (known operand) → true"
    );
    assert_eq!(
        fold("delete d.x", "let d = $state(0);"),
        None,
        "delete d.x (unknown member operand) → 2-valued → not folded"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Tri-state const-fold contract — the REFUSE family (compile-time throws), the
// ledgered LIVE-FALLBACK family (known-but-not-byte-exact), and the EAGER
// `Evaluation` semantics (a throw in a non-selected position still refuses). Each
// expectation is grounded against pinned svelte@5.56.3's `scope.js` `Evaluation`
// (the named throws compile-FAIL official; the live-fallback cases fold to an
// official literal Verter cannot prove byte-exact cross-platform).
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn throwing_bigint_ops_refuse_not_fold_or_live() {
    let b5 = "let d = $state(5n);";
    // Mixing BigInt + Number in arithmetic / bitwise → TypeError.
    assert_eq!(
        refuse_reason("d + 1", b5),
        ConstFoldRefuse::BigIntMixedArith,
        "5n + 1 (arith) → Refuse"
    );
    assert_eq!(
        refuse_reason("d & 1", b5),
        ConstFoldRefuse::BigIntMixedArith,
        "5n & 1 (bitwise) → Refuse"
    );
    assert_eq!(
        refuse_reason("d << 1", b5),
        ConstFoldRefuse::BigIntMixedArith,
        "5n << 1 (mixed shift) → Refuse"
    );
    // BigInt division / remainder by zero → RangeError.
    assert_eq!(
        refuse_reason("d / 0n", b5),
        ConstFoldRefuse::BigIntDivByZero,
        "5n / 0n → Refuse"
    );
    assert_eq!(
        refuse_reason("d % 0n", b5),
        ConstFoldRefuse::BigIntDivByZero,
        "5n % 0n → Refuse"
    );
    // BigInt unsigned right shift `>>>` → TypeError (BigInt has no `>>>`).
    assert_eq!(
        refuse_reason("d >>> 0n", b5),
        ConstFoldRefuse::BigIntUnsignedShift,
        "5n >>> 0n → Refuse"
    );
    // BigInt negative exponent → RangeError.
    assert_eq!(
        refuse_reason("d ** -1n", b5),
        ConstFoldRefuse::BigIntNegativeExponent,
        "5n ** -1n → Refuse"
    );
    // Unary `+` on a BigInt → TypeError.
    assert_eq!(
        refuse_reason("+d", b5),
        ConstFoldRefuse::BigIntUnaryPlus,
        "+5n → Refuse"
    );
}

#[test]
fn in_and_instanceof_over_primitive_rhs_refuse() {
    let d0 = "let d = $state(0);";
    // `'x' in 'abc'` — a primitive RHS → TypeError ("Cannot use 'in' operator…").
    assert_eq!(
        refuse_reason("'x' in 'abc'", d0),
        ConstFoldRefuse::InOnPrimitive,
        "'x' in 'abc' → Refuse"
    );
    // `1 instanceof 2` — a non-callable RHS → TypeError.
    assert_eq!(
        refuse_reason("1 instanceof 2", d0),
        ConstFoldRefuse::InstanceofPrimitive,
        "1 instanceof 2 → Refuse"
    );
}

#[test]
fn throwing_globals_over_known_args_refuse() {
    let d0 = "let d = $state(0);";
    // A BigInt argument to a numeric global throws ("Cannot convert a BigInt value to a
    // number").
    assert_eq!(
        refuse_reason("Math.clz32(1n)", d0),
        ConstFoldRefuse::GlobalThrowsOnKnownArg,
        "Math.clz32(1n) → Refuse"
    );
    assert_eq!(
        refuse_reason("Math.floor(1n)", d0),
        ConstFoldRefuse::GlobalThrowsOnKnownArg,
        "Math.floor(1n) → Refuse"
    );
    // `String.fromCodePoint` with an out-of-range / non-integer code point → RangeError.
    assert_eq!(
        refuse_reason("String.fromCodePoint(-1)", d0),
        ConstFoldRefuse::GlobalThrowsOnKnownArg,
        "String.fromCodePoint(-1) → Refuse"
    );
    assert_eq!(
        refuse_reason("String.fromCodePoint(1.5)", d0),
        ConstFoldRefuse::GlobalThrowsOnKnownArg,
        "String.fromCodePoint(1.5) → Refuse"
    );
    assert_eq!(
        refuse_reason("String.fromCodePoint(1114112)", d0),
        ConstFoldRefuse::GlobalThrowsOnKnownArg,
        "String.fromCodePoint(0x110000) → Refuse"
    );
}

#[test]
fn eager_evaluation_refuses_throw_in_nonselected_position() {
    let d0 = "let d = $state(0);";
    // Official's `Evaluation` is NOT a runtime short-circuit interpreter: it evaluates BOTH
    // logical operands and BOTH conditional branches before selecting, so a throw in a
    // non-selected position STILL compile-fails official → Verter must REFUSE.
    assert_eq!(
        refuse_reason("false && (1n / 0n)", d0),
        ConstFoldRefuse::BigIntDivByZero,
        "false && (1n/0n) — the non-selected RHS throws → Refuse"
    );
    assert_eq!(
        refuse_reason("true || (1n / 0n)", d0),
        ConstFoldRefuse::BigIntDivByZero,
        "true || (1n/0n) — the non-selected RHS throws → Refuse"
    );
    assert_eq!(
        refuse_reason("true ? 1 : (1n / 0n)", d0),
        ConstFoldRefuse::BigIntDivByZero,
        "true ? 1 : (1n/0n) — the non-selected ALTERNATE throws → Refuse"
    );
    assert_eq!(
        refuse_reason("false ? (1n / 0n) : 0", d0),
        ConstFoldRefuse::BigIntDivByZero,
        "false ? (1n/0n) : 0 — the non-selected CONSEQUENT throws → Refuse"
    );
    // A throw nested in a folded template-literal interpolation also refuses.
    assert_eq!(
        refuse_reason("`x${2 + 1n}y`", d0),
        ConstFoldRefuse::BigIntMixedArith,
        "a template-literal interpolation `2 + 1n` throws → Refuse"
    );
}

#[test]
fn refuse_takes_priority_over_live_fallback() {
    let d0 = "let d = $state(0);";
    // A subtree that BOTH live-falls-back (a transcendental) AND throws (a BigInt mix) must
    // REFUSE — the throw wins (refuse > live-fallback > fold).
    assert_eq!(
        refuse_reason("true ? (2 + 1n) : Math.log(2)", d0),
        ConstFoldRefuse::BigIntMixedArith,
        "a throw in one branch + a transcendental in the other → Refuse wins"
    );
}

#[test]
fn bigint_number_precision_compare_live_fallbacks() {
    // BigInt-vs-Number COERCING comparison needs exact mathematical-value comparison; the
    // f64 coercion loses precision past 2^53 (the rev6 counterexample). Verter LIVE-falls-
    // back rather than fold a wrong boolean — even for a small case it cannot prove exact.
    let big = "let d = $state(9007199254740993n);";
    assert_eq!(
        live_reason("d == 9007199254740992", big),
        LiveFallbackReason::BigIntNumberPrecisionCompare,
        "9007199254740993n == 9007199254740992 → LiveFallback (not a wrong `true`)"
    );
    assert_eq!(
        live_reason("d > 9007199254740992", big),
        LiveFallbackReason::BigIntNumberPrecisionCompare,
        "9007199254740993n > 9007199254740992 → LiveFallback"
    );
    // A BigInt-vs-BigInt comparison is EXACT (no coercion) → folds.
    assert_eq!(
        fold("d > 1n", "let d = $state(5n);").as_deref(),
        Some("true"),
        "5n > 1n (same-type) → exact fold"
    );
}

#[test]
fn large_to_int32_bitwise_live_fallbacks() {
    let d0 = "let d = $state(0);";
    // A 32-bit bitwise / shift op over a huge-finite Number needs JS modulo-2^32 `ToInt32`
    // Verter's truncating cast does not reproduce (`Math.clz32(1e20)` is `1` in JS).
    assert_eq!(
        live_reason("Math.clz32(1e20)", d0),
        LiveFallbackReason::LargeToInt32,
        "Math.clz32(1e20) → LiveFallback (huge ToUint32)"
    );
    assert_eq!(
        live_reason("(1e20 | 0)", d0),
        LiveFallbackReason::LargeToInt32,
        "1e20 | 0 → LiveFallback (huge ToInt32)"
    );
    assert_eq!(
        live_reason("(1e20 << 1)", d0),
        LiveFallbackReason::LargeToInt32,
        "1e20 << 1 → LiveFallback"
    );
    assert_eq!(
        live_reason("~1e20", d0),
        LiveFallbackReason::LargeToInt32,
        "~1e20 → LiveFallback (huge ToInt32)"
    );
    // A SMALL bitwise op still folds exactly.
    assert_eq!(
        fold("(5 | 2)", d0).as_deref(),
        Some("7"),
        "5 | 2 (small) → fold"
    );
    assert_eq!(fold("~5", d0).as_deref(), Some("-6"), "~5 (small) → fold");
}

#[test]
fn parse_int_radix_and_nonascii_whitespace_live_fallback() {
    let d0 = "let d = $state(0);";
    // A huge radix needs JS `ToInt32` (`parseInt('10', 4294967298)` → radix 2 → `2`);
    // Verter's direct range check would wrongly yield `NaN` → LIVE.
    assert_eq!(
        live_reason("Number.parseInt('10', 4294967298)", d0),
        LiveFallbackReason::ParseIntRadixOrWhitespace,
        "parseInt('10', 4294967298) → LiveFallback (radix ToInt32)"
    );
    // A leading NON-ASCII JS-whitespace char (NBSP / vertical tab) Verter's ASCII trim
    // misses → LIVE (official's full-whitespace trim finds the number).
    assert_eq!(
        live_reason("Number.parseFloat('\\u00A03.5x')", d0),
        LiveFallbackReason::ParseIntRadixOrWhitespace,
        "parseFloat('\\u00A03.5x') (NBSP) → LiveFallback"
    );
    assert_eq!(
        live_reason("Number.parseInt('\\u000B10')", d0),
        LiveFallbackReason::ParseIntRadixOrWhitespace,
        "parseInt('\\u000B10') (vertical tab) → LiveFallback"
    );
    // A CLEAN ASCII input with a standard radix still folds exactly.
    assert_eq!(
        fold("Number.parseInt('0x1F')", d0).as_deref(),
        Some("31"),
        "parseInt('0x1F') (clean) → fold"
    );
    assert_eq!(
        fold("Number.parseFloat('3.14xy')", d0).as_deref(),
        Some("3.14"),
        "parseFloat('3.14xy') (clean) → fold"
    );
    // A leading ASCII space/tab/newline is still trimmed exactly (it folds).
    assert_eq!(
        fold("Number.parseInt(' 42 ')", d0).as_deref(),
        Some("42"),
        "parseInt(' 42 ') (ASCII whitespace) → fold"
    );
}

#[test]
fn lone_surrogate_string_globals_live_fallback() {
    let d0 = "let d = $state(0);";
    // A lone surrogate (0xD800..=0xDFFF) is a valid JS string code unit / code point Verter's
    // UTF-8 value model cannot byte-exactly represent → LIVE until a UTF-16 model exists.
    assert_eq!(
        live_reason("String.fromCharCode(55296)", d0),
        LiveFallbackReason::LoneSurrogate,
        "String.fromCharCode(0xD800) → LiveFallback"
    );
    assert_eq!(
        live_reason("String.fromCodePoint(55296)", d0),
        LiveFallbackReason::LoneSurrogate,
        "String.fromCodePoint(0xD800) → LiveFallback"
    );
    // A NON-surrogate code unit / code point still folds.
    assert_eq!(
        fold("String.fromCharCode(65, 66)", d0).as_deref(),
        Some("AB"),
        "String.fromCharCode(65,66) → fold"
    );
    assert_eq!(
        fold("String.fromCodePoint(128512)", d0).as_deref(),
        Some("😀"),
        "String.fromCodePoint(128512) → fold"
    );
}

#[test]
fn live_fallback_ledger_is_complete_distinct_and_nonempty() {
    use super::super::reactive_fold_tristate::{live_fallback_ledger, LIVE_FALLBACK_LEDGER};
    // Every `LiveFallbackReason` variant has a checked-in ledger row with a distinct,
    // non-empty label + reason — a live-fallback is NEVER an untracked byte-parity miss.
    let rows = live_fallback_ledger();
    assert!(!rows.is_empty(), "the ledger must not be empty");
    for row in &rows {
        assert!(
            !row.label.is_empty(),
            "every ledger label must be non-empty"
        );
        assert!(
            !row.reason.is_empty(),
            "every ledger reason must be non-empty"
        );
    }
    let labels: std::collections::BTreeSet<&str> = rows.iter().map(|r| r.label).collect();
    let reasons: std::collections::BTreeSet<&str> = rows.iter().map(|r| r.reason).collect();
    assert_eq!(labels.len(), rows.len(), "ledger labels must be distinct");
    assert_eq!(reasons.len(), rows.len(), "ledger reasons must be distinct");

    // EXHAUSTIVENESS: every variant the evaluator can produce has a ledger row. Each
    // variant is mapped through its `label()` and looked up — a variant added to the enum
    // without a `LIVE_FALLBACK_LEDGER` row would be absent here.
    use LiveFallbackReason::*;
    for variant in [
        BigIntNumberPrecisionCompare,
        LargeToInt32,
        ParseIntRadixOrWhitespace,
        LoneSurrogate,
        TranscendentalLibm,
    ] {
        let label = variant.label();
        assert!(
            LIVE_FALLBACK_LEDGER.iter().any(|(r, _)| *r == variant),
            "LiveFallbackReason::{label} must have a LIVE_FALLBACK_LEDGER row"
        );
    }

    // Every `ConstFoldRefuse` carries a non-empty label too.
    use ConstFoldRefuse::*;
    for r in [
        BigIntMixedArith,
        BigIntDivByZero,
        BigIntUnsignedShift,
        BigIntNegativeExponent,
        BigIntUnaryPlus,
        InOnPrimitive,
        InstanceofPrimitive,
        GlobalThrowsOnKnownArg,
    ] {
        assert!(
            !r.label().is_empty(),
            "every refuse label must be non-empty"
        );
    }
}
