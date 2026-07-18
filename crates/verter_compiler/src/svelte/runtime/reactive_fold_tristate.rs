//! The tri-state const-fold CONTRACT for the Svelte client mixed-template emitter.
//!
//! Per the architect ruling (the decidable convergence for the mixed-template const-fold surface), a mixed-chunk
//! constant expression (`id="a {EXPR} b"` over demoted-`$state` / literals) classifies
//! as EXACTLY one of three outcomes — Verter does NOT reimplement full JS-semantic
//! exactness:
//!
//! - [`ChunkFold::Fold`] — emit the literal. Allowed ONLY when EXPR is traversed exactly
//!   as Svelte `Evaluation` (INCLUDING its eagerness — both logical operands and both
//!   conditional branches are evaluated before a value is selected; template literals
//!   stop after the first unknown interpolation), every evaluated operation / global is
//!   in the checked-in [`ExactFold`] allow-list for the concrete operand classes, the
//!   result is byte-exactly emittable with Verter's value model + printer, AND throw
//!   status is PROVEN non-throwing.
//! - [`ChunkFold::LiveFallback`] — emit the live expression (the existing `?? ''` path).
//!   Allowed ONLY when Svelte would have a known NON-THROWING value but Verter cannot
//!   prove byte-exact emission. It is LEDGERED ([`LiveFallbackReason`]) — never an
//!   untracked byte-parity miss.
//! - [`ChunkFold::Refuse`] — a DETERMINISTIC compile refusal ([`ConstFoldRefuse`]),
//!   NEVER live code, NEVER a fold. MANDATORY when the Svelte evaluator would call native
//!   JS and THROW (so the official compiler compile-FAILS), or when Verter has known
//!   operands but cannot prove non-throwing. Live emission is FORBIDDEN here — it would
//!   convert official's compile-failure into a runtime crash.
//!
//! Stopping rule: **wrong fold is forbidden; a known compile-time throw must refuse;
//! non-throwing exactness gaps may live-fallback only with a ledger reason row.**
//!
//! The [`ExactFold`] and [`LiveFallbackReason`] / [`ConstFoldRefuse`] vocabularies are
//! CHECKED-IN explicit allow-lists (auditable). Typed-IR only — the evaluator walks the
//! OXC typed AST; no string eval / regex.

/// The classification of one mixed-chunk constant expression — the tri-state contract.
///
/// The [`Self::Live`] arm folds together TWO emission-identical outcomes: a plain
/// not-statically-known chunk (a signal read, a member, a call — `ledger: None`, the
/// normal live interpolation official also keeps live) and a LEDGERED live-fallback
/// (`ledger: Some(reason)` — Svelte WOULD have a known non-throwing value, but Verter
/// cannot prove byte-exact emission). Both emit the live `?? ''` expression; the ledger
/// gate asserts the `Some` cases carry their checked-in reason.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum ChunkFold {
    /// A proven-exact fold: the byte-exact cooked literal text Verter emits in place of
    /// the live interpolation (`id="a {d + 1} b"` over `$state(5)` → `'a 6 b'`).
    Fold(String),
    /// Emit the LIVE expression (the existing `?? ''` path). `ledger` is `Some(reason)`
    /// for a LEDGERED live-fallback (a known-but-not-byte-exact value), `None` for a plain
    /// not-foldable chunk (the normal live interpolation).
    Live {
        /// The checked-in ledger reason when this is a live-FALLBACK (a known-but-not-
        /// byte-exact value); `None` for a plain not-statically-known chunk.
        ledger: Option<LiveFallbackReason>,
    },
    /// A compile-time throw (or unprovable throw status) — a deterministic compile
    /// refusal, NEVER live code.
    Refuse(ConstFoldRefuse),
}

/// The REASON a non-throwing chunk live-falls-back instead of folding — the checked-in
/// `LiveFallback` ledger. Each variant names a SPECIFIC byte-exactness gap Verter's
/// current value model / printer cannot close; the live emission is always behaviorally
/// correct (official's folded literal and the live expression evaluate to the same
/// runtime value), only the constant-folding cosmetic differs.
///
/// Every variant has a row in the checked-in [`LIVE_FALLBACK_LEDGER`] the ledger gate
/// asserts is complete — a live-fallback is NEVER an untracked byte-parity miss.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum LiveFallbackReason {
    /// A relational / equality comparison whose operands mix a BigInt with a Number /
    /// numeric string. Official compares the EXACT mathematical values; Verter's value
    /// model coerces the BigInt to f64 first, losing precision past 2^53
    /// (`9007199254740993n == 9007199254740992` is `false` in JS but `true` after the
    /// f64 round). LIVE-fallback rather than fold a wrong boolean.
    BigIntNumberPrecisionCompare,
    /// A bitwise / shift operation (or a `ToInt32`/`ToUint32`-bearing global like
    /// `Math.clz32` / `Math.imul`) over a finite Number too large for an exact `as i64`
    /// truncation — JS applies the modulo-2^32 `ToInt32`/`ToUint32`, which Verter's
    /// truncating cast does not reproduce for huge finite magnitudes (`Math.clz32(1e20)`
    /// is `1` in JS). LIVE-fallback rather than fold a wrong integer.
    LargeToInt32,
    /// `Number.parseInt` / `Number.parseFloat` whitespace trimming. JS trims the full
    /// ECMAScript `StrWhiteSpaceChar` set (NBSP ` `, vertical tab ``, …);
    /// Verter trims only the ASCII subset, so a leading NBSP yields `NaN` where JS finds
    /// the number. LIVE-fallback rather than fold a wrong `NaN`.
    ParseIntRadixOrWhitespace,
    /// A `String.fromCharCode` / `String.fromCodePoint` whose code unit / code point is a
    /// LONE SURROGATE (`0xD800..=0xDFFF`). JS strings are UTF-16 and hold lone surrogates;
    /// Verter's `String` value model is UTF-8 (no lone-surrogate representation), so the
    /// folded literal would mis-encode (a replacement char). LIVE-fallback until a UTF-16
    /// value model exists.
    LoneSurrogate,
    /// A TRANSCENDENTAL `Math.*` global (`sin`/`cos`/`tan`/`asin`/`acos`/`atan`/`atan2`/
    /// `exp`/`log`/`log2`/`log10`/`log1p`/`expm1`/`sinh`/`cosh`/`tanh`/`asinh`/`acosh`/
    /// `atanh`/`pow`/`cbrt`). Official's folded literal is V8's `fdlibm` result captured at
    /// the official compiler's build time; Rust's `f64::sin`/… lower to the SYSTEM libm,
    /// whose transcendental results are NOT IEEE-754-mandated-correctly-rounded and can
    /// differ in the last ULP across platforms and vs V8 — so the fold is NOT provably
    /// byte-identical on macOS / Windows / Linux. (`Math.sqrt` is IEEE-754
    /// correctly-rounded and stays an exact fold; it is NOT in this set.) LIVE-fallback
    /// rather than risk a silent wrong literal.
    TranscendentalLibm,
}

/// The CHECKED-IN `LiveFallback` LEDGER — the auditable table of every live-fallback
/// reason variant paired with its stable, human-readable justification. A live-fallback is
/// NEVER an untracked byte-parity miss: this table IS the ledger the architect requires,
/// and the `live_fallback_ledger_is_complete_distinct_and_nonempty` coverage gate asserts
/// it covers every variant with a distinct non-empty reason. Exposed crate-public (rendered
/// to `(label, reason)` rows) via [`live_fallback_ledger`].
pub(super) const LIVE_FALLBACK_LEDGER: &[(LiveFallbackReason, &str)] = &[
    (
        LiveFallbackReason::BigIntNumberPrecisionCompare,
        "BigInt-vs-Number/String comparison needs exact mathematical-value comparison; \
         Verter's f64 coercion loses precision past 2^53 — emit live rather than fold a \
         wrong boolean",
    ),
    (
        LiveFallbackReason::LargeToInt32,
        "a bitwise / ToInt32 / ToUint32 op over a huge finite Number needs JS modulo-2^32 \
         semantics Verter's truncating cast does not reproduce — emit live rather than \
         fold a wrong integer",
    ),
    (
        LiveFallbackReason::ParseIntRadixOrWhitespace,
        "Number.parseInt / parseFloat needs the full ECMAScript whitespace set (NBSP / \
         vertical-tab / …) and ToInt32 radix; Verter trims only ASCII — emit live rather \
         than fold a wrong NaN",
    ),
    (
        LiveFallbackReason::LoneSurrogate,
        "String.fromCharCode / fromCodePoint produced a lone surrogate (0xD800..=0xDFFF) \
         that Verter's UTF-8 value model cannot byte-exactly represent — emit live until a \
         UTF-16 string model exists",
    ),
    (
        LiveFallbackReason::TranscendentalLibm,
        "a transcendental Math.* (sin/cos/tan/exp/log/pow/cbrt/…) folds to V8's fdlibm \
         result; Rust's system libm is not provably bit-identical cross-platform — emit \
         live rather than risk a wrong literal (Math.sqrt is IEEE-754 exact and still \
         folds)",
    ),
];

/// A const-fold compile-time THROW — the `Refuse` reason. Each variant names a SPECIFIC
/// native-JS operation the Svelte `Evaluation` would invoke that THROWS at compile time
/// (so the official compiler compile-FAILS the component). Verter refuses deterministically
/// rather than emit live code (which would convert the compile-failure into a runtime
/// crash). The EAGER traversal detects throws even in non-selected logical operands /
/// conditional branches (`false && (1n / 0n)`), matching official's `Evaluation`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ConstFoldRefuse {
    /// Mixing a BigInt with a Number in arithmetic (`2 + 1n`) or a bitwise op (`1n & 3`)
    /// throws a `TypeError` ("Cannot mix BigInt and other types").
    BigIntMixedArith,
    /// BigInt division / remainder by `0n` (`1n / 0n`) throws a `RangeError` ("Division by
    /// zero").
    BigIntDivByZero,
    /// BigInt unsigned right shift (`1n >>> 0n`) throws a `TypeError` ("BigInts have no
    /// unsigned right shift").
    BigIntUnsignedShift,
    /// A negative BigInt exponent (`2n ** -1n`) throws a `RangeError` ("Exponent must be
    /// non-negative").
    BigIntNegativeExponent,
    /// A BigInt left-shift / exponentiation whose RESULT would exceed V8's BigInt size limit
    /// (`kMaxLengthBits` = 2^30 significant bits) throws a `RangeError` ("Maximum BigInt size
    /// exceeded") — `1n << 4294967296n`, `2n ** 4294967296n`. Detected by a CHEAP result-
    /// bit-length estimate (never the multi-gigabit allocation), so a huge fold refuses
    /// deterministically instead of attempting an allocation that would not return.
    BigIntMaxSizeExceeded,
    /// Unary `+` on a BigInt (`+1n`) throws a `TypeError` ("Cannot convert a BigInt value
    /// to a number").
    BigIntUnaryPlus,
    /// The `in` operator with a non-object (known-primitive) RHS (`'x' in 'abc'`) throws a
    /// `TypeError` ("Cannot use 'in' operator to search for 'x' in 'abc'").
    InOnPrimitive,
    /// The `instanceof` operator with a non-callable (known-primitive) RHS (`1 instanceof
    /// 2`) throws a `TypeError` ("Right-hand side of 'instanceof' is not callable").
    InstanceofPrimitive,
    /// A foldable global called with a known argument JS rejects (`Math.clz32(1n)` — a
    /// BigInt arg to a numeric global throws a `TypeError`; `String.fromCodePoint(-1 |
    /// 1.5 | 0x110000)` throws a `RangeError` "Invalid code point").
    GlobalThrowsOnKnownArg,
}

/// One row of the const-fold `LiveFallback` ledger exposed across the crate boundary (for
/// the corpus / ledger gate): a stable variant LABEL plus its checked-in reason text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LiveFallbackLedgerRow {
    /// The stable variant label (`bigint-number-precision-compare`, `transcendental-libm`,
    /// …) — the corpus's live-fallback bucket pins this so a drift fails the gate.
    pub label: &'static str,
    /// The checked-in human-readable reason.
    pub reason: &'static str,
}

/// The full checked-in const-fold `LiveFallback` ledger as crate-public rows — the
/// auditable table the corpus's `live-fallback` bucket and the ledger gate cross-check.
/// Every [`LiveFallbackReason`] variant contributes exactly one row (the exhaustive match
/// inside `label_for` makes a new variant without a label a COMPILE error).
#[must_use]
pub fn live_fallback_ledger() -> Vec<LiveFallbackLedgerRow> {
    LIVE_FALLBACK_LEDGER
        .iter()
        .map(|(reason, text)| LiveFallbackLedgerRow {
            label: reason.label(),
            reason: text,
        })
        .collect()
}

impl LiveFallbackReason {
    /// A stable kebab-case label for the variant (the corpus's live-fallback bucket pins
    /// it). The exhaustive match makes a new variant without a label a COMPILE error.
    #[must_use]
    pub(super) fn label(self) -> &'static str {
        match self {
            Self::BigIntNumberPrecisionCompare => "bigint-number-precision-compare",
            Self::LargeToInt32 => "large-to-int32",
            Self::ParseIntRadixOrWhitespace => "parseint-radix-or-whitespace",
            Self::LoneSurrogate => "lone-surrogate",
            Self::TranscendentalLibm => "transcendental-libm",
        }
    }
}

impl ConstFoldRefuse {
    /// A short, deterministic reason label (NOT V8's error text — the contract requires a
    /// deterministic refusal, not error-text reproduction) carried by the
    /// [`super::super::unsupported::UnsupportedSvelteRuntimeSurface::ConstFoldThrow`]
    /// diagnostic.
    #[must_use]
    pub(super) fn label(self) -> &'static str {
        match self {
            Self::BigIntMixedArith => "BigInt mixed with a Number in arithmetic / bitwise",
            Self::BigIntDivByZero => "BigInt division / remainder by zero",
            Self::BigIntUnsignedShift => "BigInt unsigned right shift `>>>`",
            Self::BigIntNegativeExponent => "BigInt exponentiation with a negative exponent",
            Self::BigIntMaxSizeExceeded => "BigInt `<<` / `**` result exceeds the maximum size",
            Self::BigIntUnaryPlus => "unary `+` on a BigInt",
            Self::InOnPrimitive => "`in` operator with a primitive right-hand side",
            Self::InstanceofPrimitive => "`instanceof` with a non-callable right-hand side",
            Self::GlobalThrowsOnKnownArg => "a foldable global throwing under known arguments",
        }
    }
}
