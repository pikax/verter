//! Unit tests for the BigInt const-fold arithmetic + the cheap size guard. These exercise
//! the helpers DIRECTLY (the `mixed_chunk_fold` end-to-end path is covered in
//! `reactive_fold_tests.rs`), pinning the V8-`kMaxLengthBits` boundary and the negative-shift
//! JS BigInt semantics. CRITICAL: none of these construct or materialise a huge BigInt — the
//! guard is asserted via the cheap bit-length predicate, so the suite NEVER hangs.

use super::*;
use num_bigint::BigInt;
use oxc_syntax::operator::BinaryOperator as B;

/// The folded BigInt value of `x <op> y`, or `None` for a non-`Value` outcome.
fn fold_value(op: B, x: i64, y: i64) -> Option<BigInt> {
    match eval_bigint_binary(op, &BigInt::from(x), &BigInt::from(y)) {
        BigIntBinaryOutcome::Value(EvalValue::BigInt(v)) => Some(v),
        _ => None,
    }
}

/// The folded BigInt value of `x <op> y` where `y` is supplied as a `BigInt` (for huge
/// counts that overflow `i64`), or `None`.
fn fold_value_big(op: B, x: &BigInt, y: &BigInt) -> Option<BigInt> {
    match eval_bigint_binary(op, x, y) {
        BigIntBinaryOutcome::Value(EvalValue::BigInt(v)) => Some(v),
        _ => None,
    }
}

/// Whether `x <op> y` (with `y` a `BigInt`) refuses, and with which reason.
fn refuse_of(op: B, x: &BigInt, y: &BigInt) -> Option<ConstFoldRefuse> {
    match eval_bigint_binary(op, x, y) {
        BigIntBinaryOutcome::Throws(r) => Some(r),
        _ => None,
    }
}

#[test]
fn small_shifts_and_exponents_fold_exactly() {
    assert_eq!(fold_value(B::ShiftLeft, 1, 4), Some(BigInt::from(16)));
    assert_eq!(fold_value(B::ShiftRight, 256, 2), Some(BigInt::from(64)));
    assert_eq!(fold_value(B::Exponential, 2, 10), Some(BigInt::from(1024)));
    assert_eq!(fold_value(B::Exponential, 7, 0), Some(BigInt::from(1)));
    // Sign-preserving arithmetic shifts over negative operands.
    assert_eq!(fold_value(B::ShiftLeft, -5, 3), Some(BigInt::from(-40)));
    assert_eq!(fold_value(B::ShiftRight, -256, 2), Some(BigInt::from(-64)));
    // `0n << anything` is `0n` (no bits, no allocation).
    assert_eq!(fold_value(B::ShiftLeft, 0, 1000), Some(BigInt::from(0)));
}

#[test]
fn negative_shift_counts_fold_via_js_bigint_semantics() {
    // `a << -b` ≡ `a >> b`; `a >> -b` ≡ `a << b` — arbitrary-precision arithmetic shift, NOT
    // the Number 32-bit masked path.
    assert_eq!(fold_value(B::ShiftLeft, 256, -2), Some(BigInt::from(64)));
    assert_eq!(fold_value(B::ShiftRight, 256, -2), Some(BigInt::from(1024)));
    assert_eq!(fold_value(B::ShiftLeft, 1, -2), Some(BigInt::from(0)));
    // Negative operand, negative count: `-8n << -1n` ≡ `-8n >> 1n` → `-4n` (rounds to -inf).
    assert_eq!(fold_value(B::ShiftLeft, -8, -1), Some(BigInt::from(-4)));
    // A right shift saturates a negative value to `-1n`, a non-negative to `0n`.
    assert_eq!(fold_value(B::ShiftRight, -1, 1000), Some(BigInt::from(-1)));
    assert_eq!(fold_value(B::ShiftRight, 1, 1000), Some(BigInt::from(0)));
}

#[test]
fn oversize_left_shift_refuses_without_allocating() {
    // V8: `1n << N` throws iff result bits (N+1) > 2^30, i.e. N >= 2^30. The boundary:
    let max = BigInt::from(1u64 << 30); // 2^30
    let one = BigInt::from(1);
    let below = &max - &one; // 2^30 - 1  → result bits = 2^30 → OK (largest allowed)
    let at = max.clone(); // 2^30      → result bits = 2^30 + 1 → THROWS
                          // `1n << (2^30 - 1)` is allowed by V8 — but folding it would materialise a 2^30-bit
                          // BigInt, which we must NOT do in a unit test. We only assert the GUARD predicate here
                          // (the helper decides without allocating); the end-to-end fold of such a value is not a
                          // realistic template constant. So assert the guard via the public refuse path at `2^30`
                          // and one beyond, and assert the just-below case is NOT refused (it would fold, but we
                          // do not force the fold — `refuse_of` returns None without materialising).
    assert_eq!(
        refuse_of(B::ShiftLeft, &one, &at),
        Some(ConstFoldRefuse::BigIntMaxSizeExceeded),
        "1n << 2^30 → result 2^30+1 bits → Refuse"
    );
    assert_eq!(
        refuse_of(B::ShiftLeft, &one, &(&at + &one)),
        Some(ConstFoldRefuse::BigIntMaxSizeExceeded),
        "1n << (2^30 + 1) → Refuse"
    );
    assert!(
        !left_shift_exceeds_max(&one, below.magnitude()),
        "1n << (2^30 - 1) → result exactly 2^30 bits → within the limit (not refused)"
    );
    assert!(
        left_shift_exceeds_max(&one, at.magnitude()),
        "1n << 2^30 → result 2^30+1 bits → exceeds the limit"
    );
    // A huge count (4294967296n = 2^32) far over the limit → Refuse, no allocation, no hang.
    let huge = BigInt::from(4_294_967_296u64);
    assert_eq!(
        refuse_of(B::ShiftLeft, &one, &huge),
        Some(ConstFoldRefuse::BigIntMaxSizeExceeded),
        "1n << 2^32 → Refuse (matches official RangeError)"
    );
}

#[test]
fn oversize_exponent_refuses_without_allocating() {
    let huge = BigInt::from(4_294_967_296u64); // 2^32
    assert_eq!(
        refuse_of(B::Exponential, &BigInt::from(2), &huge),
        Some(ConstFoldRefuse::BigIntMaxSizeExceeded),
        "2n ** 2^32 → Refuse"
    );
    assert_eq!(
        refuse_of(B::Exponential, &BigInt::from(3), &huge),
        Some(ConstFoldRefuse::BigIntMaxSizeExceeded),
        "3n ** 2^32 (non-power-of-two base) → Refuse"
    );
    // The exact power-of-two exponent boundary: `2n ** E` has E+1 result bits. E = 2^30
    // gives 2^30 + 1 bits → throws; E = 2^30 - 1 gives 2^30 bits → allowed.
    let two = BigInt::from(2);
    assert!(
        pow_exceeds_max(&two, &BigInt::from(1u64 << 30)),
        "2n ** 2^30 → 2^30+1 result bits → exceeds"
    );
    assert!(
        !pow_exceeds_max(&two, &BigInt::from((1u64 << 30) - 1)),
        "2n ** (2^30 - 1) → exactly 2^30 result bits → within the limit"
    );
    // A non-power-of-two base whose result lower bound already exceeds the limit.
    assert!(
        pow_exceeds_max(&BigInt::from(3), &BigInt::from(1u64 << 30)),
        "3n ** 2^30 → far over the limit"
    );
}

#[test]
fn negative_exponent_and_unsigned_shift_keep_distinct_reasons() {
    let two = BigInt::from(2);
    let neg = BigInt::from(-1);
    assert_eq!(
        refuse_of(B::Exponential, &two, &neg),
        Some(ConstFoldRefuse::BigIntNegativeExponent),
        "2n ** -1n → negative-exponent throw (DISTINCT from the size-exceeded reason)"
    );
    assert_eq!(
        refuse_of(B::ShiftRightZeroFill, &BigInt::from(6), &BigInt::from(0)),
        Some(ConstFoldRefuse::BigIntUnsignedShift),
        "6n >>> 0n → unsigned-shift throw (unchanged)"
    );
}

#[test]
fn small_base_with_overflowing_exponent_folds_bounded() {
    // |x| <= 1 ⇒ the result is bounded regardless of the exponent, even when the exponent
    // overflows u32 / u64. Never refuse, never allocate.
    let huge = BigInt::from(4_294_967_296u64); // 2^32 (overflows u32)
    assert_eq!(
        fold_value_big(B::Exponential, &BigInt::from(0), &huge),
        Some(BigInt::from(0)),
        "0n ** 2^32 → 0n"
    );
    assert_eq!(
        fold_value_big(B::Exponential, &BigInt::from(1), &huge),
        Some(BigInt::from(1)),
        "1n ** 2^32 → 1n"
    );
    // (-1) ** even → 1; (-1) ** odd → -1.
    assert_eq!(
        fold_value_big(B::Exponential, &BigInt::from(-1), &huge),
        Some(BigInt::from(1)),
        "(-1n) ** 2^32 (even) → 1n"
    );
    let huge_odd = &huge + BigInt::from(1); // 2^32 + 1 (odd)
    assert_eq!(
        fold_value_big(B::Exponential, &BigInt::from(-1), &huge_odd),
        Some(BigInt::from(-1)),
        "(-1n) ** (2^32 + 1) (odd) → -1n"
    );
}

#[test]
fn division_and_remainder_by_zero_still_refuse() {
    let zero = BigInt::from(0);
    assert_eq!(
        refuse_of(B::Division, &BigInt::from(6), &zero),
        Some(ConstFoldRefuse::BigIntDivByZero)
    );
    assert_eq!(
        refuse_of(B::Remainder, &BigInt::from(6), &zero),
        Some(ConstFoldRefuse::BigIntDivByZero)
    );
}

#[test]
fn arithmetic_bitwise_boundary_guards_match_v8_digit_zone() {
    // V8 ALSO throws `Maximum BigInt size exceeded` on `*` / `+` / `-` / bitwise whose result
    // EXCEEDS the size limit — NOT only `<<` / `**`. V8's throw predicate is `result_bits >
    // 2^30` (a result of EXACTLY 2^30 bits folds). The guards are CHEAP bit-count predicates
    // (no allocation): `<<`/`**`/`+`/`-`/bitwise use the EXACT cheaply-decidable UPPER bound
    // (`> 2^30`); `*` alone keeps a conservative `>= 2^30` ceiling (its product band is two bits
    // wide and cannot be pinned cheaply). Tested on synthetic bit counts (the operator wrappers
    // feed `x.bits()`).
    let max = BIGINT_MAX_BITS;

    // `*`: product significant bits = b(x) + b(y) (or -1). Refuse iff b(x)+b(y) >= LIMIT.
    assert!(
        mul_exceeds_max(max / 2, (max / 2) + 2),
        "~2^30-bit product → refuse"
    );
    assert!(mul_exceeds_max(max, 1), "2^30-bit operand × any → refuse");
    assert!(!mul_exceeds_max(64, 64), "a tiny product (128 bits) → fold");
    assert!(
        !mul_exceeds_max(0, max),
        "0n × anything → 0n (no bits) → fold"
    );

    // `+` / `-`: result bits <= max(b(x), b(y)) + 1. Refuse iff that UPPER bound > LIMIT.
    // The provable boundary: an operand of `2^30 - 1` bits ⇒ upper bound exactly 2^30 ⇒ the
    // result is provably <= 2^30 bits ⇒ V8 FOLDS ⇒ we MUST fold (no over-refusal).
    assert!(
        !add_sub_exceeds_max(max - 1, max - 1),
        "two operands of 2^30 - 1 bits → result upper bound exactly 2^30 → provably folds"
    );
    // The genuinely-AMBIGUOUS boundary: an operand of EXACTLY 2^30 bits ⇒ the true result is
    // 2^30 (folds) or 2^30 + 1 (throws) and cannot be decided without the ~134 MB allocation ⇒
    // Refuse (contract-compliant: never emit maybe-throwing code as live).
    assert!(
        add_sub_exceeds_max(max, 1),
        "a 2^30-bit operand (+ carry) → upper bound 2^30 + 1 → ambiguous → refuse"
    );
    assert!(!add_sub_exceeds_max(1000, 1000), "small operands → fold");

    // `&` / `|` / `^`: result bits <= max(b(x), b(y)) + 1; same exact upper-bound rule.
    assert!(
        !bitwise_exceeds_max(max - 1, max - 1),
        "two operands of 2^30 - 1 bits → upper bound exactly 2^30 → provably folds"
    );
    assert!(
        bitwise_exceeds_max(max, max),
        "two 2^30-bit operands → upper bound 2^30 + 1 → ambiguous → refuse"
    );
    assert!(!bitwise_exceeds_max(64, 1), "tiny bitwise → fold");

    // unary `-` / `~`: result bits <= b(x) + 1. Refuse iff that upper bound > LIMIT.
    assert!(
        !unary_exceeds_max(max - 1),
        "an operand of 2^30 - 1 bits → upper bound exactly 2^30 → provably folds"
    );
    assert!(
        unary_exceeds_max(max),
        "a 2^30-bit operand → upper bound 2^30 + 1 → ambiguous → refuse"
    );
    assert!(!unary_exceeds_max(1000), "a small operand → fold");
}

#[test]
fn comparison_ops_defer_to_caller() {
    // The relational / equality ops return `Comparison` (the caller does the exact
    // BigInt-vs-BigInt comparison).
    for op in [
        B::Equality,
        B::Inequality,
        B::StrictEquality,
        B::LessThan,
        B::GreaterEqualThan,
    ] {
        assert!(
            matches!(
                eval_bigint_binary(op, &BigInt::from(1), &BigInt::from(2)),
                BigIntBinaryOutcome::Comparison
            ),
            "op {op:?} defers to the caller's comparison"
        );
    }
}
