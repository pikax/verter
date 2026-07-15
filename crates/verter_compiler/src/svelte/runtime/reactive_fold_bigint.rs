//! BigInt arithmetic for the Svelte client const-fold evaluator — the native-JS BigInt
//! operator table reworked to the tri-state [`BigIntBinaryOutcome`], plus the CHEAP size
//! guard that refuses an oversize `<<` / `**` (matching V8's `RangeError: Maximum BigInt
//! size exceeded`) WITHOUT ever attempting the multi-gigabit allocation.
//!
//! Extracted from `reactive_fold.rs` to keep both files under the size guard. The evaluator
//! consults [`eval_bigint_binary`] from its `BinaryExpression` arm when both operands are
//! known BigInts.
//!
//! ## Why a cheap size guard (not try-and-catch)
//!
//! Official `svelte@5.56.3` evaluates two known BigInt operands by directly invoking the
//! native operator (`binary['<<'](a, b) = a << b`, `binary['**'](a, b) = a ** b`) in the
//! compiler's V8 process. V8 caps a BigInt at `kMaxLengthBits` = 2^30 significant bits; a
//! result that would exceed it throws `RangeError: Maximum BigInt size exceeded` BEFORE
//! allocating — so official compile-FAILS `1n << 4294967296n` / `2n ** 4294967296n` (probed:
//! both reject in <30 ms, no hang). Verter's `num_bigint` has NO size cap: a naive
//! `x << s` for a huge `s` attempts a multi-gigabit allocation that does not return (a
//! forward-progress violation), and there is no exception to catch. So the guard estimates
//! the RESULT bit length cheaply (operand `.bits()` is O(1)) and refuses when it would
//! exceed `kMaxLengthBits`, never materialising the value.

use num_bigint::{BigInt, Sign};
use oxc_syntax::operator::BinaryOperator as B;

// `EvalValue` is the parent's value model; `ConstFoldRefuse` is re-exported `pub(super)` on
// the parent for its child modules (same path the `globals` sibling uses).
use super::{ConstFoldRefuse, EvalValue};

/// V8's `BigInt::kMaxLengthBits` — the maximum number of significant bits a BigInt may hold.
/// A result with MORE than this many bits throws `RangeError: Maximum BigInt size exceeded`.
/// Probed on V8: `1n << 1073741823n` (result = 2^30 bits) is OK; `1n << 1073741824n`
/// (result = 2^30 + 1 bits) throws. So the throw predicate is `result_bits > MAX`.
///
/// The `+` / `-` / `&` / `|` / `^` / unary guards refuse iff the CHEAP O(1) UPPER bit-bound on
/// the result `> MAX` — so a provable `<= MAX` folds (matching V8) and only the genuinely-
/// ambiguous boundary (an operand of EXACTLY `2^30` bits, result `2^30`-vs-`2^30 + 1`) refuses.
/// TODO(follow-up): that exact-`2^30`-bit boundary is conservatively REFUSED because deciding it
/// needs the ~134 MB allocation the guard exists to avoid (a forward-progress hazard) — the
/// known divergence ledgered as D-15 in `docs/arch/svelte-native-compiler-plan.md`. A future
/// arbitrary-precision exact path (or bounded interval analyzer) could disambiguate it without
/// materializing the value; the case is absurd-input-only (a 100 MB+ BigInt template constant).
const BIGINT_MAX_BITS: u64 = 1 << 30; // 1_073_741_824

/// The outcome of folding a binary operator over two KNOWN BigInt operands.
pub(super) enum BigIntBinaryOutcome {
    /// A proven-exact BigInt arithmetic / bitwise result.
    Value(EvalValue),
    /// The op THROWS in JS (refuse): `/`/`%` by zero, `**` with a negative exponent, `>>>`
    /// (BigInt has no unsigned right shift), or a `<<` / `**` whose result exceeds V8's
    /// BigInt size limit (`Maximum BigInt size exceeded`).
    Throws(ConstFoldRefuse),
    /// A comparison / equality op — handled by the caller's cross-type comparison (a
    /// BigInt-vs-BigInt comparison is exact).
    Comparison,
}

/// Evaluate a binary operator over two KNOWN BigInt operands — the native JS BigInt operator
/// reworked to the tri-state [`BigIntBinaryOutcome`]. Arithmetic / bitwise yield a BigInt;
/// the throwing ops (`/`/`%` by zero, `**` with a negative exponent, `>>>`, an oversize
/// `<<` / `**` / `*` / `+` / `-` / bitwise) refuse; comparison / equality defer to the caller
/// (exact for BigInt-vs-BigInt). A NEGATIVE shift count is VALID in JS (`a << -b` ≡ `a >> b`,
/// `a >> -b` ≡ `a << b`) and folds via [`bigint_shift`] — NOT the Number 32-bit shift path.
///
/// EVERY magnitude-growing op is size-guarded — V8 throws `Maximum BigInt size exceeded` not
/// only for `<<` / `**` but also for an arithmetic / bitwise result reaching its digit-
/// allocation limit (`(1n << 2^29) * (1n << 2^29)`, `maxv + 1n`). The guards are CHEAP
/// bit-count predicates over `x.bits()` / `y.bits()` (O(1)); they NEVER materialise the value.
/// `<<` / `**` / `+` / `-` / bitwise use the EXACT cheaply-decidable boundary: refuse iff the
/// cheap UPPER bound on the result bit length `> 2^30` (V8 throws iff `result_bits > 2^30`, so a
/// provable `<= 2^30` FOLDS — matching V8). `*` alone keeps a CONSERVATIVE `>=` ceiling: its
/// product bit count is `b(x)+b(y)` or `b(x)+b(y)-1`, a two-bit band the cheap predicate cannot
/// pin without the allocation, so it refuses the whole boundary zone (never a real constant).
pub(super) fn eval_bigint_binary(op: B, x: &BigInt, y: &BigInt) -> BigIntBinaryOutcome {
    let big = |v: BigInt| BigIntBinaryOutcome::Value(EvalValue::BigInt(v));
    let size_throw = BigIntBinaryOutcome::Throws(ConstFoldRefuse::BigIntMaxSizeExceeded);
    match op {
        B::Addition => {
            if add_sub_exceeds_max(x.bits(), y.bits()) {
                size_throw
            } else {
                big(x + y)
            }
        }
        B::Subtraction => {
            if add_sub_exceeds_max(x.bits(), y.bits()) {
                size_throw
            } else {
                big(x - y)
            }
        }
        B::Multiplication => {
            if mul_exceeds_max(x.bits(), y.bits()) {
                size_throw
            } else {
                big(x * y)
            }
        }
        // BigInt `/` truncates toward zero; division / remainder by zero THROWS (RangeError).
        // Division / remainder only SHRINK, so they need no size guard.
        B::Division => {
            if y.sign() == Sign::NoSign {
                BigIntBinaryOutcome::Throws(ConstFoldRefuse::BigIntDivByZero)
            } else {
                big(x / y)
            }
        }
        B::Remainder => {
            if y.sign() == Sign::NoSign {
                BigIntBinaryOutcome::Throws(ConstFoldRefuse::BigIntDivByZero)
            } else {
                big(x % y)
            }
        }
        // `**` THROWS on a negative exponent (RangeError "Exponent must be positive"), and on
        // a result that would exceed V8's BigInt size limit (RangeError "Maximum BigInt size
        // exceeded"). The size guard is a CHEAP bit-length estimate — never the allocation.
        B::Exponential => bigint_pow(x, y),
        // Bitwise ops grow the magnitude by at most one bit; the conservative `>=` ceiling
        // covers V8's boundary zone.
        B::BitwiseAnd => {
            if bitwise_exceeds_max(x.bits(), y.bits()) {
                size_throw
            } else {
                big(x & y)
            }
        }
        B::BitwiseOR => {
            if bitwise_exceeds_max(x.bits(), y.bits()) {
                size_throw
            } else {
                big(x | y)
            }
        }
        B::BitwiseXOR => {
            if bitwise_exceeds_max(x.bits(), y.bits()) {
                size_throw
            } else {
                big(x ^ y)
            }
        }
        // BigInt shifts take an arbitrary-precision BigInt amount (NOT masked to 32 bits) and
        // are sign-preserving arithmetic shifts. A negative count flips the direction. A
        // left-shift result that would exceed V8's size limit refuses.
        B::ShiftLeft => bigint_shift(x, y, ShiftDir::Left),
        B::ShiftRight => bigint_shift(x, y, ShiftDir::Right),
        // BigInt does NOT support `>>>` (JS THROWS a TypeError).
        B::ShiftRightZeroFill => BigIntBinaryOutcome::Throws(ConstFoldRefuse::BigIntUnsignedShift),
        // Comparison / equality fall through to the caller's cross-type handling.
        B::Equality
        | B::Inequality
        | B::StrictEquality
        | B::StrictInequality
        | B::LessThan
        | B::LessEqualThan
        | B::GreaterThan
        | B::GreaterEqualThan
        | B::In
        | B::Instanceof => BigIntBinaryOutcome::Comparison,
    }
}

/// `x ** y` over two BigInts. A negative exponent throws `RangeError` (distinct reason); a
/// result exceeding V8's `kMaxLengthBits` throws `RangeError: Maximum BigInt size exceeded`
/// (detected cheaply, never allocated). `y` is known non-negative after the sign check, so
/// the `u64` conversion only fails for an exponent ≥ 2^64 — which the size guard would have
/// already refused (any base ≥ 2 with such an exponent is astronomically over the limit),
/// and for base magnitude ≤ 1 (`0n` / `1n` / `-1n`) the result is bounded so it folds.
fn bigint_pow(x: &BigInt, y: &BigInt) -> BigIntBinaryOutcome {
    if y.sign() == Sign::Minus {
        return BigIntBinaryOutcome::Throws(ConstFoldRefuse::BigIntNegativeExponent);
    }
    if pow_exceeds_max(x, y) {
        return BigIntBinaryOutcome::Throws(ConstFoldRefuse::BigIntMaxSizeExceeded);
    }
    // The size guard proved the result fits under 2^30 bits, so the exponent fits a `u32`
    // for every base with magnitude ≥ 2 (`(B-1)*e + 1 <= 2^30` ⇒ `e <= 2^30`). For a base
    // magnitude ≤ 1 the value is bounded regardless of the exponent, so a `> u32::MAX`
    // exponent there still produces `0n` / `±1n`.
    match u32::try_from(y) {
        Ok(exp) => BigIntBinaryOutcome::Value(EvalValue::BigInt(x.pow(exp))),
        // A base magnitude ≤ 1 with a `> u32::MAX` exponent: the result is `0n` (|x|==0),
        // `1n` (x==1 or even exponent over -1), or `-1n` (x==-1, odd exponent). Compute it
        // WITHOUT the huge `pow` (which the guard already proved bounded by magnitude).
        Err(_) => BigIntBinaryOutcome::Value(EvalValue::BigInt(bounded_pow_low_base(x, y))),
    }
}

/// The result of `x ** y` when `|x| <= 1` (so the value is bounded regardless of `y`): `0n`
/// for `x == 0`, `1n` for `x == 1`, and `±1n` for `x == -1` by the exponent's parity. Only
/// reached when `y` overflows `u32` AND the size guard passed (which it always does for
/// `|x| <= 1`).
fn bounded_pow_low_base(x: &BigInt, y: &BigInt) -> BigInt {
    if x.sign() == Sign::NoSign {
        return BigInt::from(0);
    }
    if x == &BigInt::from(1) {
        return BigInt::from(1);
    }
    // x == -1: `(-1) ** y` is `1` for an even `y`, `-1` for an odd `y`.
    if (y & BigInt::from(1)).sign() == Sign::NoSign {
        BigInt::from(1)
    } else {
        BigInt::from(-1)
    }
}

/// Which direction a shift moves (a negative count flips it).
enum ShiftDir {
    Left,
    Right,
}

/// `x << y` / `x >> y` over BigInts with an arbitrary-precision (sign-preserving, arithmetic)
/// shift count. A NEGATIVE count flips the direction (`a << -b` ≡ `a >> b`, `a >> -b` ≡
/// `a << b`) — JS BigInt semantics, NOT the Number 32-bit masked shift. A LEFT shift (after
/// direction resolution) whose result would exceed V8's `kMaxLengthBits` throws `RangeError:
/// Maximum BigInt size exceeded`, detected cheaply (operand bits + shift amount) so it
/// refuses instead of attempting a multi-gigabit allocation that would not return. A RIGHT
/// shift only shrinks (or saturates a non-negative value to `0n`, a negative value to `-1n`),
/// so it never overflows and folds directly.
fn bigint_shift(x: &BigInt, y: &BigInt, dir: ShiftDir) -> BigIntBinaryOutcome {
    // Resolve the effective direction + non-negative magnitude. A negative count flips the
    // direction; `y.magnitude()` is the absolute shift amount.
    let effective_left = match (dir, y.sign()) {
        (ShiftDir::Left, Sign::Minus) => false,
        (ShiftDir::Left, _) => true,
        (ShiftDir::Right, Sign::Minus) => true,
        (ShiftDir::Right, _) => false,
    };
    let amount = y.magnitude();

    if effective_left {
        // A left shift grows the magnitude: result bits = bit_length(|x|) + amount. Guard it
        // BEFORE shifting. A zero operand has no bits → `0n` regardless of the amount.
        if x.sign() == Sign::NoSign {
            return BigIntBinaryOutcome::Value(EvalValue::BigInt(BigInt::from(0)));
        }
        if left_shift_exceeds_max(x, amount) {
            return BigIntBinaryOutcome::Throws(ConstFoldRefuse::BigIntMaxSizeExceeded);
        }
        // The guard proved `bit_length(|x|) + amount <= 2^30`, so `amount <= 2^30` fits a
        // `u64` (and a `usize` on every supported target — `usize` is ≥ 32 bits, and
        // 2^30 < 2^32). The conversion cannot fail here.
        let shift = usize::try_from(amount.clone())
            .expect("shift amount bounded under 2^30 by the size guard");
        BigIntBinaryOutcome::Value(EvalValue::BigInt(x << shift))
    } else {
        // A right shift only shrinks; it never overflows. A huge amount saturates: a
        // non-negative value to `0n`, a negative value to `-1n` (arithmetic shift rounds
        // toward -inf). `num_bigint`'s `>>` already saturates correctly for any in-range
        // `usize`; for an amount exceeding `usize::MAX` (≥ 2^64) saturate explicitly.
        match usize::try_from(amount.clone()) {
            Ok(shift) => BigIntBinaryOutcome::Value(EvalValue::BigInt(x >> shift)),
            Err(_) => {
                let saturated = if x.sign() == Sign::Minus {
                    BigInt::from(-1)
                } else {
                    BigInt::from(0)
                };
                BigIntBinaryOutcome::Value(EvalValue::BigInt(saturated))
            }
        }
    }
}

/// Whether `x << amount` would exceed V8's `kMaxLengthBits`. EXACT and cheap: a left shift of
/// a non-zero `x` produces a value whose significant-bit count is `bit_length(|x|) + amount`
/// (the MSB moves up by `amount`). Refuse iff that exceeds 2^30. (`x == 0` is handled by the
/// caller — `0n << n` is `0n`, never overflows.)
fn left_shift_exceeds_max(x: &BigInt, amount: &num_bigint::BigUint) -> bool {
    let operand_bits = x.bits(); // == bit_length(|x|); 0 for x == 0 (caller-excluded).
                                 // `amount` as a saturating u64 — an amount ≥ 2^64 trivially exceeds the limit (operand
                                 // bits are ≤ 2^30, so any amount ≥ 2^30 already exceeds it).
    let amount_u64 = biguint_to_u64_saturating(amount);
    // Saturating add: if either side is huge the sum saturates to u64::MAX, which is > MAX.
    operand_bits.saturating_add(amount_u64) > BIGINT_MAX_BITS
}

/// Whether `x * y` would reach V8's BigInt size limit — CONSERVATIVE: the product's
/// significant bits are `b(x) + b(y)` or `b(x) + b(y) - 1`, and V8 throws once the result
/// reaches its digit-allocation boundary, so refuse iff `b(x) + b(y) >= 2^30`. The `>=`
/// (not `>`) covers V8's carry/word-rounding zone; the only over-refused products are
/// ~2^30-bit values (never a real template constant). A zero operand (`0` bits) makes the
/// product `0n` regardless — never refused.
fn mul_exceeds_max(x_bits: u64, y_bits: u64) -> bool {
    if x_bits == 0 || y_bits == 0 {
        return false; // 0n * anything == 0n
    }
    x_bits.saturating_add(y_bits) >= BIGINT_MAX_BITS
}

/// Whether `x + y` / `x - y` would exceed V8's BigInt size limit — EXACT at the cheaply-
/// decidable boundary: the result's significant bits are at most `max(b(x), b(y)) + 1`. Refuse
/// iff that cheap UPPER bound `> 2^30` (V8's throw predicate is `result_bits > 2^30`; a result
/// of exactly 2^30 bits FOLDS). For an operand of `2^30 - 1` bits the upper bound is exactly
/// 2^30 ⇒ provably non-throwing ⇒ FOLD (V8 folds it too). The only Refuse-with-upper-`> 2^30`
/// case that is genuinely AMBIGUOUS — an operand of exactly 2^30 bits, where the true result is
/// either 2^30 bits (folds) or 2^30 + 1 (throws) and cannot be decided without the ~134 MB
/// allocation — stays Refuse, which is contract-compliant ("Refuse when cannot prove
/// non-throwing"; a maybe-throw must never become runtime-crashing live code).
fn add_sub_exceeds_max(x_bits: u64, y_bits: u64) -> bool {
    x_bits.max(y_bits).saturating_add(1) > BIGINT_MAX_BITS
}

/// Whether `x & y` / `x | y` / `x ^ y` would exceed V8's BigInt size limit — EXACT at the
/// cheaply-decidable boundary: a bitwise result has at most `max(b(x), b(y)) + 1` significant
/// bits (the `+1` covers the two's-complement sign extension of a negative operand). Refuse iff
/// that cheap UPPER bound `> 2^30` (a result of exactly 2^30 bits FOLDS — V8's throw predicate
/// is `result_bits > 2^30`). An operand of exactly 2^30 bits stays Refuse: the true result is
/// 2^30 or 2^30 + 1 bits and cannot be decided without the huge allocation — contract-compliant.
fn bitwise_exceeds_max(x_bits: u64, y_bits: u64) -> bool {
    x_bits.max(y_bits).saturating_add(1) > BIGINT_MAX_BITS
}

/// Whether a unary `-x` / `~x` would exceed V8's BigInt size limit — EXACT at the cheaply-
/// decidable boundary: `~x` is `-(x + 1)`, which adds at most one significant bit. Refuse iff
/// the cheap UPPER bound `b(x) + 1 > 2^30` (a result of exactly 2^30 bits FOLDS — V8's throw
/// predicate is `result_bits > 2^30`). For `b(x) = 2^30 - 1` the upper bound is exactly 2^30 ⇒
/// FOLD. An operand of exactly 2^30 bits stays Refuse (the `~` result is 2^30 or 2^30 + 1 bits,
/// undecidable without the huge allocation — contract-compliant). (Unary negation `-x` keeps the
/// same magnitude, so the shared `+1` ceiling is slightly conservative for `-x`, but only at the
/// same absurd-input boundary.)
pub(super) fn unary_exceeds_max(x_bits: u64) -> bool {
    x_bits.saturating_add(1) > BIGINT_MAX_BITS
}

/// Whether `x ** y` (with `y >= 0`) would exceed V8's `kMaxLengthBits` — option C: an EXACT
/// bit-length test that never over-refuses a valid fold (which would itself be a defect) and
/// never under-refuses a throwing case (which would attempt the huge allocation).
///
/// The result `|x|^y` has exactly `floor(y * log2|x|) + 1` significant bits. Cases:
/// - `y == 0` ⇒ result is `1n` (1 bit) — never exceeds.
/// - `|x| <= 1` ⇒ result magnitude is `0` / `1` (≤ 1 bit) — never exceeds.
/// - `|x|` a power of two (`|x| == 2^(B-1)`) ⇒ the bit length is EXACTLY `(B-1)*y + 1`.
/// - otherwise ⇒ the bit length is `floor(y * log2|x|) + 1`, bracketed by the cheap integer
///   bounds `(B-1)*y + 1 <= bits <= B*y`. Refuse iff the lower bound already exceeds MAX;
///   allow iff the upper bound is within MAX; in the razor-thin band between them (only
///   reachable for astronomically large `y`, never a real template constant) refuse rather
///   than allocate to disambiguate.
fn pow_exceeds_max(x: &BigInt, y: &BigInt) -> bool {
    if y.sign() == Sign::NoSign {
        return false; // x ** 0 == 1
    }
    let base_bits = x.bits(); // bit_length(|x|)
    if base_bits <= 1 {
        // |x| <= 1 → result magnitude is 0 or 1, bounded.
        return false;
    }
    let y_u128 = match biguint_to_u128(y.magnitude()) {
        Some(v) => v,
        // y >= 2^128 with |x| >= 2: the result is astronomically over the limit.
        None => return true,
    };
    let b = u128::from(base_bits);
    // Lower bound on the result bit length: (B-1)*y + 1 (EXACT when |x| is a power of two).
    let lower = (b - 1).saturating_mul(y_u128).saturating_add(1);
    if lower > u128::from(BIGINT_MAX_BITS) {
        return true; // even the smallest possible result exceeds the limit.
    }
    // Upper bound: B*y. If even the largest possible result fits, it cannot exceed.
    let upper = b.saturating_mul(y_u128);
    if upper <= u128::from(BIGINT_MAX_BITS) {
        return false;
    }
    // The boundary band: lower <= MAX < upper. |x| is exactly a power of two ⇒ the lower
    // bound is EXACT, so the decision is exact. Otherwise the true bit length is somewhere in
    // the band; this is only reachable for `y` near 2^30 / log2|x| (~10^8–10^9), a result of
    // ~1 Gbit that is never a real byte-emittable template constant — refuse rather than
    // allocate to disambiguate (the size guard's purpose is to NEVER materialise such a
    // value).
    if x.magnitude().count_ones() == 1 {
        // Power of two: lower bound is exact — it is `<= MAX` here, so the result fits.
        false
    } else {
        true
    }
}

/// A `BigUint` as a saturating `u64` (`u64::MAX` when it does not fit).
fn biguint_to_u64_saturating(v: &num_bigint::BigUint) -> u64 {
    let digits = v.to_u64_digits();
    match digits.len() {
        0 => 0,
        1 => digits[0],
        _ => u64::MAX,
    }
}

/// A `BigUint` as a `u128`, or `None` when it exceeds `u128::MAX`.
fn biguint_to_u128(v: &num_bigint::BigUint) -> Option<u128> {
    let digits = v.to_u64_digits();
    match digits.len() {
        0 => Some(0),
        1 => Some(u128::from(digits[0])),
        2 => Some(u128::from(digits[0]) | (u128::from(digits[1]) << 64)),
        _ => None,
    }
}

#[cfg(test)]
#[path = "reactive_fold_bigint_tests.rs"]
mod tests;
