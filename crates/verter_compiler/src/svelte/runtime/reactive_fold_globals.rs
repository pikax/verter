//! The pure-global call / constant tables of the Svelte client const-fold evaluator — a
//! faithful, COMPLETE port of official `svelte@5.56.3`'s `globals` + `global_constants`
//! tables (`phases/scope.js`), reworked to the tri-state const-fold contract. Extracted
//! from `reactive_fold.rs` to keep both that file and this one under the file-size guard;
//! the parent module's `evaluate_call` / `global_constant_member` arms consult
//! [`GLOBAL_CALLS`] / [`GLOBAL_CONSTANTS`].
//!
//! Each folder mirrors the corresponding JS builtin and returns a [`GlobalOutcome`] —
//! `Value` for a proven-EXACT fold, `Throws` for a global that throws under the known
//! argument (`Math.clz32(1n)` / an invalid `String.fromCodePoint`), or `Live` for a
//! known-but-not-byte-exact value (a TRANSCENDENTAL libm result, a huge-finite `ToInt32`,
//! a `parseInt`/`parseFloat` whitespace/radix gap, a lone surrogate). A
//! type-known-but-NOT-foldable global (`Math.random` / `BigInt`) has NO folder; the caller
//! yields the table's type marker ([`super::EvalValue::NumberType`] / [`StringType`] —
//! official's `NUMBER` / `STRING` symbols) for it, and likewise for any unknown argument.
//! The JS argument-coercion via `Number(...)` is the parent's [`super::number_coerce`].
//!
//! **ExactFold boundary (architect ruling):** only the IEEE-754-MANDATED / integer /
//! decimal-scan / string / constant set folds exactly. ALL TRANSCENDENTALS (`sin`/`cos`/
//! `tan`/`asin`/`acos`/`atan`/`atan2`/`exp`/`log`/`log2`/`log10`/`log1p`/`expm1`/`sinh`/
//! `cosh`/`tanh`/`asinh`/`acosh`/`atanh`/`pow`/`cbrt`) live-fall-back: Rust's system libm
//! is not provably bit-identical to V8's `fdlibm` across macOS / Windows / Linux, so a
//! folded literal could be a silent wrong value. `Math.sqrt` is IEEE-754 correctly-rounded
//! and STAYS an exact fold.

use super::{
    number_coerce, string_coerce, ConstFoldRefuse, EvalValue, GlobalOutcome, LiveFallbackReason,
};

/// A pure-global call entry: `(keypath, type-marker, optional concrete folder)`. Mirrors
/// official's `globals` table — the type marker when an argument is unknown, the concrete
/// fold when every argument is known. A folder of `None` means the call is type-known but
/// never concretely folded (e.g. `Math.random`, `BigInt`). The folder receives the KNOWN
/// concrete argument values in source order and returns the [`GlobalOutcome`].
pub(super) type GlobalFolder = fn(&[EvalValue]) -> GlobalOutcome;

/// `Number(...)` coercion of the first argument (`Number()` with no arg is `0`) — exact.
fn fold_number(args: &[EvalValue]) -> GlobalOutcome {
    GlobalOutcome::Value(EvalValue::Num(args.first().map_or(0.0, number_coerce)))
}

/// `String(...)` coercion of the first argument (`String()` with no arg is `""`) — exact.
fn fold_string(args: &[EvalValue]) -> GlobalOutcome {
    GlobalOutcome::Value(EvalValue::Str(
        args.first().map(string_coerce).unwrap_or_default(),
    ))
}

/// A single-argument EXACT numeric `Math.*` fold (an IEEE-754-mandated / integer-rounding
/// function — `floor`/`ceil`/`round`/`trunc`/`abs`/`sqrt`/`fround`/`f16round`). The JS
/// function coerces its argument with `Number(...)` first, which `number_coerce` mirrors.
/// A BigInt argument THROWS in JS (a numeric global rejects a BigInt) → refuse.
macro_rules! math1_exact {
    ($f:expr) => {
        (|args: &[EvalValue]| -> GlobalOutcome {
            match args.first() {
                Some(EvalValue::BigInt(_)) => {
                    GlobalOutcome::Throws(ConstFoldRefuse::GlobalThrowsOnKnownArg)
                }
                Some(v) => GlobalOutcome::Value(EvalValue::Num($f(number_coerce(v)))),
                None => GlobalOutcome::Value(EvalValue::Num($f(f64::NAN))),
            }
        }) as GlobalFolder
    };
}

/// A single-argument TRANSCENDENTAL `Math.*` fold (`sin`/`cos`/`log`/`exp`/`cbrt`/…). The
/// value is correct but Rust's system libm is not provably bit-identical to V8's `fdlibm`
/// cross-platform → a ledgered LIVE-fallback (never a fold). A BigInt argument THROWS in
/// JS → refuse.
macro_rules! math1_transcendental {
    ($f:expr) => {
        (|args: &[EvalValue]| -> GlobalOutcome {
            match args.first() {
                Some(EvalValue::BigInt(_)) => {
                    GlobalOutcome::Throws(ConstFoldRefuse::GlobalThrowsOnKnownArg)
                }
                Some(v) => GlobalOutcome::Live(
                    EvalValue::Num($f(number_coerce(v))),
                    LiveFallbackReason::TranscendentalLibm,
                ),
                None => GlobalOutcome::Live(
                    EvalValue::Num($f(f64::NAN)),
                    LiveFallbackReason::TranscendentalLibm,
                ),
            }
        }) as GlobalFolder
    };
}

/// A predicate-returning `Number.*` fold (`isInteger`, `isFinite`, `isNaN`,
/// `isSafeInteger`) — JS does NOT coerce the argument (a non-number is always `false`),
/// so the fold reads the f64 ONLY when the argument is concretely a `Num`, else `false`.
/// Exact (a pure f64 predicate).
macro_rules! number_pred {
    ($f:expr) => {
        (|args: &[EvalValue]| -> GlobalOutcome {
            GlobalOutcome::Value(EvalValue::Bool(match args.first() {
                Some(EvalValue::Num(n)) => $f(*n),
                _ => false,
            }))
        }) as GlobalFolder
    };
}

/// The COMPLETE port of official `scope.js`'s `globals` table — EVERY entry, in the
/// official order: `BigInt` (type-only, never folds), the full `Math.*` function set,
/// `Number` + the `Number.*` set, `String` + the `String.*` set. The 2nd field is the
/// type marker yielded when an argument is unknown (official's `NUMBER` / `STRING`); the
/// 3rd is the concrete folder used when every argument is known (`None` ⇒ type-only, like
/// `BigInt` / `Math.random`, which official lists WITHOUT a `fn`).
pub(super) const GLOBAL_CALLS: &[(&str, EvalValue, Option<GlobalFolder>)] = &[
    // `BigInt` is listed `[NUMBER]` with NO fn ⇒ it yields the NUMBER marker (never folds
    // to a concrete value: `BigInt(5)` stays live).
    ("BigInt", EvalValue::NumberType, None),
    (
        "Math.min",
        EvalValue::NumberType,
        Some(js_min as GlobalFolder),
    ),
    (
        "Math.max",
        EvalValue::NumberType,
        Some(js_max as GlobalFolder),
    ),
    ("Math.random", EvalValue::NumberType, None),
    (
        "Math.floor",
        EvalValue::NumberType,
        Some(math1_exact!(f64::floor)),
    ),
    // `Math.f16round` rounds to the nearest IEEE-754 half-precision value (round-to-even) —
    // an exactly-defined rounding (binary16 is bit-identical cross-platform).
    (
        "Math.f16round",
        EvalValue::NumberType,
        Some(math1_exact!(js_f16round)),
    ),
    // JS `Math.round` rounds half toward +Infinity (`Math.floor(x + 0.5)`), NOT Rust's
    // round-half-away-from-zero — they diverge at `.5` of a negative.
    (
        "Math.round",
        EvalValue::NumberType,
        Some(math1_exact!(js_round)),
    ),
    (
        "Math.abs",
        EvalValue::NumberType,
        Some(math1_exact!(f64::abs)),
    ),
    // Transcendentals — Rust system libm vs V8 fdlibm is not provably bit-identical → LIVE.
    (
        "Math.acos",
        EvalValue::NumberType,
        Some(math1_transcendental!(f64::acos)),
    ),
    (
        "Math.asin",
        EvalValue::NumberType,
        Some(math1_transcendental!(f64::asin)),
    ),
    (
        "Math.atan",
        EvalValue::NumberType,
        Some(math1_transcendental!(f64::atan)),
    ),
    (
        "Math.atan2",
        EvalValue::NumberType,
        Some(js_atan2 as GlobalFolder),
    ),
    (
        "Math.ceil",
        EvalValue::NumberType,
        Some(math1_exact!(f64::ceil)),
    ),
    (
        "Math.cos",
        EvalValue::NumberType,
        Some(math1_transcendental!(f64::cos)),
    ),
    (
        "Math.sin",
        EvalValue::NumberType,
        Some(math1_transcendental!(f64::sin)),
    ),
    (
        "Math.tan",
        EvalValue::NumberType,
        Some(math1_transcendental!(f64::tan)),
    ),
    (
        "Math.exp",
        EvalValue::NumberType,
        Some(math1_transcendental!(f64::exp)),
    ),
    (
        "Math.log",
        EvalValue::NumberType,
        Some(math1_transcendental!(f64::ln)),
    ),
    (
        "Math.pow",
        EvalValue::NumberType,
        Some(js_pow as GlobalFolder),
    ),
    // `Math.sqrt` is IEEE-754 correctly-rounded (bit-identical everywhere) → EXACT.
    (
        "Math.sqrt",
        EvalValue::NumberType,
        Some(math1_exact!(f64::sqrt)),
    ),
    (
        "Math.clz32",
        EvalValue::NumberType,
        Some(js_clz32 as GlobalFolder),
    ),
    (
        "Math.imul",
        EvalValue::NumberType,
        Some(js_imul as GlobalFolder),
    ),
    // JS `Math.sign` is `0`/`-0`/`±1`/`NaN`, NOT Rust's `signum` (which is `±1` for zero) —
    // an exact integer/sign function.
    (
        "Math.sign",
        EvalValue::NumberType,
        Some(math1_exact!(js_sign)),
    ),
    (
        "Math.log10",
        EvalValue::NumberType,
        Some(math1_transcendental!(f64::log10)),
    ),
    (
        "Math.log2",
        EvalValue::NumberType,
        Some(math1_transcendental!(f64::log2)),
    ),
    (
        "Math.log1p",
        EvalValue::NumberType,
        Some(math1_transcendental!(f64::ln_1p)),
    ),
    (
        "Math.expm1",
        EvalValue::NumberType,
        Some(math1_transcendental!(f64::exp_m1)),
    ),
    (
        "Math.cosh",
        EvalValue::NumberType,
        Some(math1_transcendental!(f64::cosh)),
    ),
    (
        "Math.sinh",
        EvalValue::NumberType,
        Some(math1_transcendental!(f64::sinh)),
    ),
    (
        "Math.tanh",
        EvalValue::NumberType,
        Some(math1_transcendental!(f64::tanh)),
    ),
    (
        "Math.acosh",
        EvalValue::NumberType,
        Some(math1_transcendental!(f64::acosh)),
    ),
    (
        "Math.asinh",
        EvalValue::NumberType,
        Some(math1_transcendental!(f64::asinh)),
    ),
    (
        "Math.atanh",
        EvalValue::NumberType,
        Some(math1_transcendental!(f64::atanh)),
    ),
    (
        "Math.trunc",
        EvalValue::NumberType,
        Some(math1_exact!(f64::trunc)),
    ),
    // `Math.fround` rounds to the nearest IEEE-754 single-precision value — exact rounding.
    (
        "Math.fround",
        EvalValue::NumberType,
        Some(math1_exact!(|x: f64| x as f32 as f64)),
    ),
    (
        "Math.cbrt",
        EvalValue::NumberType,
        Some(math1_transcendental!(f64::cbrt)),
    ),
    ("Number", EvalValue::NumberType, Some(fold_number)),
    (
        "Number.isInteger",
        EvalValue::NumberType,
        Some(number_pred!(|n: f64| n.is_finite() && n.fract() == 0.0)),
    ),
    (
        "Number.isFinite",
        EvalValue::NumberType,
        Some(number_pred!(f64::is_finite)),
    ),
    (
        "Number.isNaN",
        EvalValue::NumberType,
        Some(number_pred!(f64::is_nan)),
    ),
    (
        "Number.isSafeInteger",
        EvalValue::NumberType,
        Some(number_pred!(|n: f64| n.is_finite()
            && n.fract() == 0.0
            && n.abs() <= 9_007_199_254_740_991.0)),
    ),
    (
        "Number.parseFloat",
        EvalValue::NumberType,
        Some(|args| js_parse_float(args.first())),
    ),
    (
        "Number.parseInt",
        EvalValue::NumberType,
        Some(|args| js_parse_int(args.first(), args.get(1))),
    ),
    ("String", EvalValue::StringType, Some(fold_string)),
    (
        "String.fromCharCode",
        EvalValue::StringType,
        Some(js_from_char_code as GlobalFolder),
    ),
    (
        "String.fromCodePoint",
        EvalValue::StringType,
        Some(js_from_code_point as GlobalFolder),
    ),
];

/// Whether any argument is a concrete BigInt (a numeric `Math.*` global THROWS on a BigInt
/// argument — "Cannot convert a BigInt value to a number").
fn any_bigint(args: &[EvalValue]) -> bool {
    args.iter().any(|v| matches!(v, EvalValue::BigInt(_)))
}

/// JS `Math.min`: `+Infinity` with no args; `NaN` if ANY arg is `NaN`; else the minimum
/// (exact). A BigInt argument THROWS → refuse.
fn js_min(args: &[EvalValue]) -> GlobalOutcome {
    if any_bigint(args) {
        return GlobalOutcome::Throws(ConstFoldRefuse::GlobalThrowsOnKnownArg);
    }
    GlobalOutcome::Value(EvalValue::Num(args.iter().map(number_coerce).fold(
        f64::INFINITY,
        |acc, x| {
            if acc.is_nan() || x.is_nan() {
                f64::NAN
            } else {
                acc.min(x)
            }
        },
    )))
}

/// JS `Math.max`: `-Infinity` with no args; `NaN` if ANY arg is `NaN`; else the maximum
/// (exact). A BigInt argument THROWS → refuse.
fn js_max(args: &[EvalValue]) -> GlobalOutcome {
    if any_bigint(args) {
        return GlobalOutcome::Throws(ConstFoldRefuse::GlobalThrowsOnKnownArg);
    }
    GlobalOutcome::Value(EvalValue::Num(args.iter().map(number_coerce).fold(
        f64::NEG_INFINITY,
        |acc, x| {
            if acc.is_nan() || x.is_nan() {
                f64::NAN
            } else {
                acc.max(x)
            }
        },
    )))
}

/// JS `Math.atan2(y, x)` — a TRANSCENDENTAL (Rust system libm vs V8 fdlibm not provably
/// bit-identical) → LIVE-fallback. A BigInt argument THROWS → refuse.
fn js_atan2(args: &[EvalValue]) -> GlobalOutcome {
    if any_bigint(args) {
        return GlobalOutcome::Throws(ConstFoldRefuse::GlobalThrowsOnKnownArg);
    }
    let y = args.first().map_or(f64::NAN, number_coerce);
    let x = args.get(1).map_or(f64::NAN, number_coerce);
    GlobalOutcome::Live(
        EvalValue::Num(y.atan2(x)),
        LiveFallbackReason::TranscendentalLibm,
    )
}

/// JS `Math.pow(base, exp)` — a TRANSCENDENTAL (Rust `powf` vs V8 fdlibm not provably
/// bit-identical) → LIVE-fallback. A BigInt argument THROWS → refuse.
fn js_pow(args: &[EvalValue]) -> GlobalOutcome {
    if any_bigint(args) {
        return GlobalOutcome::Throws(ConstFoldRefuse::GlobalThrowsOnKnownArg);
    }
    let base = args.first().map_or(f64::NAN, number_coerce);
    let exp = args.get(1).map_or(f64::NAN, number_coerce);
    GlobalOutcome::Live(
        EvalValue::Num(base.powf(exp)),
        LiveFallbackReason::TranscendentalLibm,
    )
}

/// JS `Math.round`: round half toward +Infinity (`Math.floor(x + 0.5)`), preserving the
/// `NaN`/`±Infinity` passthrough.
fn js_round(x: f64) -> f64 {
    if x.is_nan() || x.is_infinite() {
        return x;
    }
    (x + 0.5).floor()
}

/// JS `Math.sign`: `NaN` for `NaN`, the value itself for `±0` (preserving the sign), else
/// `±1`.
fn js_sign(x: f64) -> f64 {
    if x.is_nan() {
        f64::NAN
    } else if x == 0.0 {
        x // preserves -0
    } else if x > 0.0 {
        1.0
    } else {
        -1.0
    }
}

/// JS `Math.f16round`: round to the nearest IEEE-754 half-precision (binary16) value with
/// round-to-nearest-even, then back to f64 (`Math.f16round(1.1)` → `1.099609375`).
fn js_f16round(x: f64) -> f64 {
    f64::from(half::f16::from_f64(x))
}

/// JS `Math.clz32`: the count of leading zero bits in the ToUint32 of the argument
/// (`Math.clz32(1)` → `31`, `Math.clz32(0)` → `32`). A BigInt arg THROWS → refuse; a
/// huge-finite arg (`|x| >= 2^53`) is not byte-exact via the truncating `ToUint32`
/// (`Math.clz32(1e20)` is `1` in JS) → ledgered LIVE-fallback.
fn js_clz32(args: &[EvalValue]) -> GlobalOutcome {
    if any_bigint(args) {
        return GlobalOutcome::Throws(ConstFoldRefuse::GlobalThrowsOnKnownArg);
    }
    let x = args.first().map_or(f64::NAN, number_coerce);
    let value = EvalValue::Num(f64::from(to_u32(x).leading_zeros()));
    if large_for_to_uint32(x) {
        GlobalOutcome::Live(value, LiveFallbackReason::LargeToInt32)
    } else {
        GlobalOutcome::Value(value)
    }
}

/// JS `Math.imul`: 32-bit integer multiplication (`Math.imul(3, 4)` → `12`,
/// `Math.imul(-5, 256)` → `-1280`) — both args ToUint32, wrapping-multiplied, reinterpreted
/// as a signed i32. A BigInt arg THROWS → refuse; a huge-finite arg is not byte-exact via
/// the truncating `ToUint32` → ledgered LIVE-fallback.
fn js_imul(args: &[EvalValue]) -> GlobalOutcome {
    if any_bigint(args) {
        return GlobalOutcome::Throws(ConstFoldRefuse::GlobalThrowsOnKnownArg);
    }
    let a = args.first().map_or(f64::NAN, number_coerce);
    let b = args.get(1).map_or(f64::NAN, number_coerce);
    let value = EvalValue::Num(f64::from((to_u32(a).wrapping_mul(to_u32(b))) as i32));
    if large_for_to_uint32(a) || large_for_to_uint32(b) {
        GlobalOutcome::Live(value, LiveFallbackReason::LargeToInt32)
    } else {
        GlobalOutcome::Value(value)
    }
}

/// ECMAScript `ToUint32` of an f64 (`NaN`/`±Infinity` → 0; else the value truncated toward
/// zero, modulo 2^32). Shared by `Math.clz32` / `Math.imul`.
fn to_u32(x: f64) -> u32 {
    if x.is_nan() || x.is_infinite() {
        0
    } else {
        x.trunc() as i64 as u32
    }
}

/// Whether a finite Number is too large for an exact `ToUint32` via a truncating `as i64`
/// cast (`|x| >= 2^53`, where the f64 no longer represents the integer exactly so the
/// modulo-2^32 result diverges from JS). A non-finite value coerces to `0` exactly.
fn large_for_to_uint32(x: f64) -> bool {
    x.is_finite() && x.abs() >= 9_007_199_254_740_992.0
}

/// Whether a `parseInt` radix argument needs JS `ToInt32` wrapping that Verter's direct
/// `trunc` range check does not reproduce — i.e. its magnitude reaches the signed-32-bit
/// wrap boundary (`|r| >= 2^31`). Below that, `ToInt32(r) == trunc(r)` so the `2..=36`
/// check matches JS exactly; at/above it, `ToInt32` may wrap a huge radix INTO the valid
/// range (`ToInt32(4294967298) == 2`), which Verter must live-fall-back rather than reject.
/// A non-finite radix is `ToInt32`-`0` (the infer case) so it never needs the wrap.
fn radix_needs_to_int32_wrap(r: f64) -> bool {
    r.is_finite() && r.abs() >= 2_147_483_648.0
}

/// The ASCII whitespace subset Verter's prefix-scan trims (`[' ', '\t', '\n', '\r']`).
const ASCII_TRIM: [char; 4] = [' ', '\t', '\n', '\r'];

/// Whether the string's LEADING whitespace contains a JS `StrWhiteSpace` character outside
/// Verter's ASCII trim set (vertical tab ``, form feed ``, NBSP ` `, BOM, the
/// Unicode space separators) — the cases where Verter's ASCII-only trim diverges from JS's
/// full-whitespace trim (`Number.parseFloat('\u{A0}3.5')` is `3.5` in JS, `NaN` in Verter),
/// so `parseInt`/`parseFloat` must LIVE-fall-back rather than fold a wrong `NaN`.
fn has_nonascii_leading_whitespace(s: &str) -> bool {
    for c in s.chars() {
        if ASCII_TRIM.contains(&c) {
            continue; // an ASCII-trim char — Verter strips it too
        }
        // The first non-ASCII-trim char: if it is STILL a JS whitespace char, Verter's
        // trim would have stopped early (a divergence); otherwise the leading whitespace
        // was entirely ASCII-clean.
        return is_js_whitespace(c);
    }
    false
}

/// Whether `c` is a JS `StrWhiteSpace` / `LineTerminator` character (the set
/// `String.prototype.trim` / `parseInt` / `parseFloat` / `Number()` strip). Used only to
/// DETECT a non-ASCII-whitespace prefix Verter cannot byte-exactly reproduce.
fn is_js_whitespace(c: char) -> bool {
    matches!(
        c,
        '\u{0009}'
            | '\u{000A}'
            | '\u{000B}'
            | '\u{000C}'
            | '\u{000D}'
            | '\u{0020}'
            | '\u{00A0}'
            | '\u{1680}'
            | '\u{2000}'
            ..='\u{200A}'
                | '\u{2028}'
                | '\u{2029}'
                | '\u{202F}'
                | '\u{205F}'
                | '\u{3000}'
                | '\u{FEFF}'
    )
}

/// JS `Number.parseFloat`: parse the longest leading prefix of the (string-coerced)
/// argument that is a decimal float; `NaN` if none. `Infinity` / `-Infinity` prefixes are
/// recognized. (`Number.parseFloat('3.14xy')` → `3.14`.) Returns `GlobalOutcome::Live` when
/// the leading whitespace contains a JS-whitespace char Verter's ASCII trim misses
/// (`'\u{A0}3.5'`) — the byte-exactness gap; an exact `Value` otherwise.
fn js_parse_float(arg: Option<&EvalValue>) -> GlobalOutcome {
    let raw = arg.map(string_coerce).unwrap_or_default();
    if has_nonascii_leading_whitespace(&raw) {
        // The live value Verter would compute is `NaN` (its ASCII trim stops at the
        // non-ASCII whitespace); emit live so JS's full-whitespace trim runs at runtime.
        return GlobalOutcome::Live(
            EvalValue::Num(f64::NAN),
            LiveFallbackReason::ParseIntRadixOrWhitespace,
        );
    }
    let s = raw.trim_start_matches(ASCII_TRIM);
    // Recognize a leading signed `Infinity`.
    for (prefix, value) in [
        ("Infinity", f64::INFINITY),
        ("+Infinity", f64::INFINITY),
        ("-Infinity", f64::NEG_INFINITY),
    ] {
        if s.starts_with(prefix) {
            return GlobalOutcome::Value(EvalValue::Num(value));
        }
    }
    // Scan the maximal leading `[+-]?digits[.digits][(e|E)[+-]?digits]` prefix.
    let bytes = s.as_bytes();
    let mut i = 0usize;
    if i < bytes.len() && (bytes[i] == b'+' || bytes[i] == b'-') {
        i += 1;
    }
    let int_start = i;
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        i += 1;
    }
    let had_int = i > int_start;
    let mut had_frac = false;
    if i < bytes.len() && bytes[i] == b'.' {
        i += 1;
        let frac_start = i;
        while i < bytes.len() && bytes[i].is_ascii_digit() {
            i += 1;
        }
        had_frac = i > frac_start;
    }
    if !had_int && !had_frac {
        return GlobalOutcome::Value(EvalValue::Num(f64::NAN));
    }
    // Optional exponent (only consumed if it has at least one digit).
    if i < bytes.len() && (bytes[i] == b'e' || bytes[i] == b'E') {
        let mut j = i + 1;
        if j < bytes.len() && (bytes[j] == b'+' || bytes[j] == b'-') {
            j += 1;
        }
        let exp_start = j;
        while j < bytes.len() && bytes[j].is_ascii_digit() {
            j += 1;
        }
        if j > exp_start {
            i = j;
        }
    }
    GlobalOutcome::Value(EvalValue::Num(s[..i].parse::<f64>().unwrap_or(f64::NAN)))
}

/// JS `Number.parseInt`: parse the leading integer prefix of the (string-coerced) argument
/// in the given radix (default 10; a `0x`/`0X` prefix forces 16 when radix is 0/16); `NaN`
/// if no digits. (`Number.parseInt('0x1F')` → `31`, `Number.parseInt('10', 2)` → `2`.)
/// Returns `GlobalOutcome::Live` when the leading whitespace contains a non-ASCII
/// JS-whitespace char OR the radix needs JS `ToInt32` (a huge radix like `4294967298` whose
/// `ToInt32` is a VALID radix `2` — Verter's direct range check would wrongly yield `NaN`);
/// an exact `Value` otherwise.
fn js_parse_int(arg: Option<&EvalValue>, radix_arg: Option<&EvalValue>) -> GlobalOutcome {
    let raw = arg.map(string_coerce).unwrap_or_default();
    if has_nonascii_leading_whitespace(&raw) {
        return GlobalOutcome::Live(
            EvalValue::Num(f64::NAN),
            LiveFallbackReason::ParseIntRadixOrWhitespace,
        );
    }
    // The radix arg needs JS `ToInt32` before the `2..=36` check (`parseInt('10',
    // 4294967298)` → radix `ToInt32(4294967298) == 2` → `2`). Verter's direct `trunc`
    // range check matches JS only when `ToInt32(radix) == trunc(radix)` — i.e. the radix
    // magnitude is below 2^31 (no signed-32-bit wrap). A radix whose magnitude needs the
    // modulo-2^32 wrap → emit live rather than fold a wrong value.
    if let Some(v) = radix_arg {
        let r = number_coerce(v);
        if radix_needs_to_int32_wrap(r) {
            return GlobalOutcome::Live(
                EvalValue::Num(f64::NAN),
                LiveFallbackReason::ParseIntRadixOrWhitespace,
            );
        }
    }
    let s = raw.trim_start_matches(ASCII_TRIM);
    let mut chars = s.chars().peekable();
    let mut sign = 1.0_f64;
    match chars.peek() {
        Some('+') => {
            chars.next();
        }
        Some('-') => {
            sign = -1.0;
            chars.next();
        }
        _ => {}
    }
    // The requested radix (0 / absent ⇒ infer). A non-integer / out-of-range radix → NaN.
    // (The huge-radix `ToInt32` case is handled as a live-fallback before this; here the
    // radix magnitude is < 2^53, so the truncated range check matches JS exactly.)
    let mut radix = match radix_arg {
        Some(v) => {
            let r = number_coerce(v).trunc();
            if r == 0.0 {
                0
            } else if (2.0..=36.0).contains(&r) {
                r as u32
            } else {
                return GlobalOutcome::Value(EvalValue::Num(f64::NAN));
            }
        }
        None => 0,
    };
    // A `0x`/`0X` prefix selects radix 16 (when radix is unset or already 16).
    let rest: String = chars.clone().collect();
    if (radix == 0 || radix == 16) && (rest.starts_with("0x") || rest.starts_with("0X")) {
        chars.next();
        chars.next();
        radix = 16;
    } else if radix == 0 {
        radix = 10;
    }
    let mut acc = 0.0_f64;
    let mut any = false;
    let radix_f = f64::from(radix);
    for c in chars {
        match c.to_digit(radix) {
            Some(d) => {
                acc = acc * radix_f + f64::from(d);
                any = true;
            }
            None => break,
        }
    }
    GlobalOutcome::Value(EvalValue::Num(if any { sign * acc } else { f64::NAN }))
}

/// Whether a UTF-16 code unit is a LONE SURROGATE (`0xD800..=0xDFFF`) — a value JS strings
/// hold but Verter's UTF-8 `String` value model cannot byte-exactly represent.
fn is_surrogate_unit(u: u16) -> bool {
    (0xD800..=0xDFFF).contains(&u)
}

/// JS `String.fromCharCode`: each arg ToUint16 → a UTF-16 code unit; the units form the
/// string (`String.fromCharCode(65, 66)` → `"AB"`). Returns `GlobalOutcome::Live` when any
/// produced code unit is a LONE SURROGATE (Verter's UTF-8 value model would mis-encode it
/// as a replacement char) — emit live until a UTF-16 value model exists; an exact `Value`
/// otherwise. (`String.fromCharCode` never throws — out-of-range args wrap via ToUint16.)
fn js_from_char_code(args: &[EvalValue]) -> GlobalOutcome {
    let units: Vec<u16> = args
        .iter()
        .map(|v| {
            let n = number_coerce(v);
            if n.is_nan() || n.is_infinite() {
                0
            } else {
                n.trunc() as i64 as u64 as u16
            }
        })
        .collect();
    if units.iter().copied().any(is_surrogate_unit) {
        // The (lossy) live value is what Verter's UTF-8 reconstruction would produce; emit
        // live so the surrogate is preserved at runtime.
        return GlobalOutcome::Live(
            EvalValue::Str(String::from_utf16_lossy(&units)),
            LiveFallbackReason::LoneSurrogate,
        );
    }
    GlobalOutcome::Value(EvalValue::Str(String::from_utf16_lossy(&units)))
}

/// JS `String.fromCodePoint`: each arg is a Unicode code point (`0..=0x10FFFF`); an
/// out-of-range / non-integer arg THROWS a RangeError in JS → refuse
/// (`String.fromCodePoint(128512)` → `"😀"`, `String.fromCodePoint(-1 | 1.5 | 0x110000)`
/// throws). A LONE-SURROGATE code point (`0xD800..=0xDFFF`) is a VALID arg JS accepts but
/// Verter's UTF-8 value model cannot byte-exactly represent → ledgered LIVE-fallback.
fn js_from_code_point(args: &[EvalValue]) -> GlobalOutcome {
    let mut out = String::new();
    let mut any_surrogate = false;
    for v in args {
        let n = number_coerce(v);
        // An out-of-range / non-integer code point THROWS in JS.
        if !n.is_finite() || n.fract() != 0.0 || !(0.0..=1_114_111.0).contains(&n) {
            return GlobalOutcome::Throws(ConstFoldRefuse::GlobalThrowsOnKnownArg);
        }
        let cp = n as u32;
        if (0xD800..=0xDFFF).contains(&cp) {
            // A lone surrogate is a valid `fromCodePoint` arg but not UTF-8-representable;
            // mark it for a live-fallback (push the replacement char so `out` stays valid).
            any_surrogate = true;
            out.push('\u{FFFD}');
        } else if let Some(c) = char::from_u32(cp) {
            out.push(c);
        } else {
            // Unreachable for a non-surrogate ≤ 0x10FFFF, but stay safe: refuse rather than
            // silently drop.
            return GlobalOutcome::Throws(ConstFoldRefuse::GlobalThrowsOnKnownArg);
        }
    }
    if any_surrogate {
        GlobalOutcome::Live(EvalValue::Str(out), LiveFallbackReason::LoneSurrogate)
    } else {
        GlobalOutcome::Value(EvalValue::Str(out))
    }
}

/// The global numeric CONSTANTS official folds (`global_constants`), with their exact f64
/// values. `js_number_to_string` renders each to its JS spelling (`Math.PI` →
/// `"3.141592653589793"`).
pub(super) const GLOBAL_CONSTANTS: &[(&str, f64)] = &[
    ("Math.PI", std::f64::consts::PI),
    ("Math.E", std::f64::consts::E),
    ("Math.LN10", std::f64::consts::LN_10),
    ("Math.LN2", std::f64::consts::LN_2),
    ("Math.LOG10E", std::f64::consts::LOG10_E),
    ("Math.LOG2E", std::f64::consts::LOG2_E),
    ("Math.SQRT2", std::f64::consts::SQRT_2),
    ("Math.SQRT1_2", std::f64::consts::FRAC_1_SQRT_2),
];
