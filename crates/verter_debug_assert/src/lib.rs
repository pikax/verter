//! The sole sanctioned `debug_assert!`/`debug_assert_eq!`/`debug_assert_ne!`
//! entry point for Verter production code.
//!
//! `crates/*/src` and `crates/*/tests` must not call
//! `std::debug_assert!`/`std::debug_assert_eq!`/`std::debug_assert_ne!`
//! directly — the workspace-root `clippy.toml` `disallowed-macros` list bans
//! all three raw spellings (including the `core::` alias, which resolves to
//! the same path) and `cargo clippy --workspace --all-targets -- -D
//! warnings` enforces it. Call [`verter_debug_assert!`],
//! [`verter_debug_assert_eq!`], or [`verter_debug_assert_ne!`] instead.
//!
//! ## Why this exists
//!
//! `debug_assert!(cond)` (and the `_eq!`/`_ne!` forms) expand to `if
//! cfg!(debug_assertions) { assert!(cond) }` — the condition expression
//! itself is never evaluated when `debug_assertions` is off (release, and
//! the gate's `no-debug-assertions` shipped-cfg profile). If `cond` performs
//! a state transition — a mutation, a counter bump, anything beyond a pure
//! predicate — that transition silently never runs in a shipped build. See
//! `docs/arch/refactor/rev11/rulings/MAINTAINER-DIRECTIVE-SINGLE-TEST-UNIVERSE.md`.
//!
//! Unlike the std macros they replace, these macros **force-evaluate their
//! argument(s) into a typed local binding BEFORE branching on
//! `cfg!(debug_assertions)`** — only the resulting inert value is fed to the
//! (possibly compiled-out) `assert!`/`assert_eq!`/`assert_ne!` check. That
//! forcing is unconditional in every profile, so a side-effecting condition
//! (or either side of an `_eq!`/`_ne!` comparison) always runs, even when
//! the assertion itself is compiled out. This is a structural property of
//! the macro expansion, not caller discipline: an author who forgets the
//! precompute-then-pass pattern still gets it for free, e.g.
//!
//! ```ignore
//! // `advance()` runs unconditionally — the macro captures its result into a
//! // local binding before deciding (via `cfg!`) whether to assert on it.
//! verter_debug_assert::verter_debug_assert!(state.advance().is_ok());
//! ```
//!
//! This crate has zero dependencies and sits below every crate that uses it
//! in the workspace dependency graph.

#![forbid(unsafe_code)]

// These macros deliberately do NOT expand through `std::debug_assert!` /
// `std::debug_assert_eq!` / `std::debug_assert_ne!` — clippy's
// `disallowed_macros` lint attributes a violation to the macro-expansion
// call site (via its macro-backtrace), not just the literal defining crate,
// so a thin wrapper that forwards to the banned macro would still trip the
// lint at every caller. Instead each macro is written out exactly the way
// `std::debug_assert!` itself is defined — `if cfg!(debug_assertions) {
// assert!(...) }` — using `cfg!`/`assert!`/`assert_eq!`/`assert_ne!`
// directly, none of which are on the disallowed list. Unlike the std forms,
// the condition/operands are captured into local bindings BEFORE the
// `cfg!(debug_assertions)` branch, so argument evaluation is unconditional;
// only the pass/panic *check* is debug-only.
//
// `$crate::__std` is a hidden re-export of `::std` rather than a hardcoded
// `::std` path in the macro bodies, so the macros stay hygienic for a future
// `no_std` (`core`-only) caller: `$crate` always resolves relative to this
// crate's own dependency graph, never to whatever `std`/`core` binding is (or
// isn't) in scope at the call site.
#[doc(hidden)]
pub use ::std as __std;

/// Debug-only assertion. See the [crate-level docs](crate) for why this
/// exists instead of `std::debug_assert!`.
///
/// The condition is evaluated unconditionally, in every profile, before the
/// debug-only pass/panic check runs.
#[macro_export]
macro_rules! verter_debug_assert {
    ($cond:expr $(,)?) => {{
        let __verter_debug_assert_cond: bool = $cond;
        if $crate::__std::cfg!(debug_assertions) {
            $crate::__std::assert!(__verter_debug_assert_cond);
        }
    }};
    ($cond:expr, $($arg:tt)+) => {{
        let __verter_debug_assert_cond: bool = $cond;
        if $crate::__std::cfg!(debug_assertions) {
            $crate::__std::assert!(__verter_debug_assert_cond, $($arg)+);
        }
    }};
}

/// Debug-only equality assertion. See the [crate-level docs](crate) for why
/// this exists instead of `std::debug_assert_eq!`.
///
/// Both operands are evaluated unconditionally, in every profile, before the
/// debug-only pass/panic check runs. Operands are captured BY REFERENCE
/// (never moved) — same as `std::assert_eq!`'s own `(&$left, &$right)`
/// internal match — so a non-`Copy`, non-cloned operand that the caller
/// still uses afterward keeps compiling exactly as it did against
/// `std::debug_assert_eq!`.
#[macro_export]
macro_rules! verter_debug_assert_eq {
    ($left:expr, $right:expr $(,)?) => {{
        let __verter_debug_assert_eq_left = &$left;
        let __verter_debug_assert_eq_right = &$right;
        if $crate::__std::cfg!(debug_assertions) {
            $crate::__std::assert_eq!(
                __verter_debug_assert_eq_left,
                __verter_debug_assert_eq_right
            );
        }
    }};
    ($left:expr, $right:expr, $($arg:tt)+) => {{
        let __verter_debug_assert_eq_left = &$left;
        let __verter_debug_assert_eq_right = &$right;
        if $crate::__std::cfg!(debug_assertions) {
            $crate::__std::assert_eq!(
                __verter_debug_assert_eq_left,
                __verter_debug_assert_eq_right,
                $($arg)+
            );
        }
    }};
}

/// Debug-only inequality assertion. See the [crate-level docs](crate) for
/// why this exists instead of `std::debug_assert_ne!`.
///
/// Both operands are evaluated unconditionally, in every profile, before the
/// debug-only pass/panic check runs. Operands are captured BY REFERENCE
/// (never moved) — same as `std::assert_ne!`'s own `(&$left, &$right)`
/// internal match — so a non-`Copy`, non-cloned operand that the caller
/// still uses afterward keeps compiling exactly as it did against
/// `std::debug_assert_ne!`.
#[macro_export]
macro_rules! verter_debug_assert_ne {
    ($left:expr, $right:expr $(,)?) => {{
        let __verter_debug_assert_ne_left = &$left;
        let __verter_debug_assert_ne_right = &$right;
        if $crate::__std::cfg!(debug_assertions) {
            $crate::__std::assert_ne!(
                __verter_debug_assert_ne_left,
                __verter_debug_assert_ne_right
            );
        }
    }};
    ($left:expr, $right:expr, $($arg:tt)+) => {{
        let __verter_debug_assert_ne_left = &$left;
        let __verter_debug_assert_ne_right = &$right;
        if $crate::__std::cfg!(debug_assertions) {
            $crate::__std::assert_ne!(
                __verter_debug_assert_ne_left,
                __verter_debug_assert_ne_right,
                $($arg)+
            );
        }
    }};
}

#[cfg(test)]
mod tests {
    #[test]
    fn verter_debug_assert_passes_on_true() {
        verter_debug_assert!(1 + 1 == 2);
    }

    #[test]
    #[cfg_attr(not(debug_assertions), ignore = "debug_assert is a no-op in release")]
    #[should_panic]
    fn verter_debug_assert_panics_on_false() {
        verter_debug_assert!(1 + 1 == 3, "arithmetic must hold");
    }

    #[test]
    fn verter_debug_assert_eq_passes_on_equal() {
        verter_debug_assert_eq!(2 + 2, 4);
    }

    #[test]
    #[cfg_attr(
        not(debug_assertions),
        ignore = "debug_assert_eq is a no-op in release"
    )]
    #[should_panic]
    fn verter_debug_assert_eq_panics_on_unequal() {
        verter_debug_assert_eq!(2 + 2, 5);
    }

    #[test]
    fn verter_debug_assert_ne_passes_on_unequal() {
        verter_debug_assert_ne!(2 + 2, 5);
    }

    #[test]
    #[cfg_attr(
        not(debug_assertions),
        ignore = "debug_assert_ne is a no-op in release"
    )]
    #[should_panic]
    fn verter_debug_assert_ne_panics_on_equal() {
        verter_debug_assert_ne!(2 + 2, 4);
    }

    // ------------------------------------------------------------------
    // Seeded-defect proof: the argument-forcing fix. These tests are NOT
    // `ignore`d under `not(debug_assertions)` — that is the point. They
    // assert only that side-effecting arguments run exactly once, never
    // that the assertion itself panics/passes, so they hold identically in
    // every build profile.
    //
    // The pre-fix macro (`if cfg!(debug_assertions) { assert!($($arg:tt)*)
    // } `) only evaluates its argument(s) when `debug_assertions` is TRUE —
    // these tests FAIL against that macro on a `no-debug-assertions`-profile
    // build (`cargo test -p verter_debug_assert --profile
    // no-debug-assertions`, verified against the pre-fix macro body before
    // landing this fix) and PASS against it on a normal debug build, which
    // is exactly the non-discriminating gap this fix closes. Against the
    // fixed macro they pass in BOTH profiles, because the condition/operands
    // are forced into local bindings before the `cfg!(debug_assertions)`
    // branch runs.
    // ------------------------------------------------------------------

    #[test]
    fn verter_debug_assert_condition_evaluates_unconditionally() {
        use std::cell::Cell;
        let calls = Cell::new(0u32);
        verter_debug_assert!({
            calls.set(calls.get() + 1);
            true
        });
        assert_eq!(
            calls.get(),
            1,
            "the condition passed to verter_debug_assert! must run exactly \
             once in every profile, including when debug_assertions is off \
             and the resulting assert! check is compiled out"
        );
    }

    #[test]
    fn verter_debug_assert_eq_operands_evaluate_unconditionally() {
        use std::cell::Cell;
        let left_calls = Cell::new(0u32);
        let right_calls = Cell::new(0u32);
        verter_debug_assert_eq!(
            {
                left_calls.set(left_calls.get() + 1);
                1
            },
            {
                right_calls.set(right_calls.get() + 1);
                1
            }
        );
        assert_eq!(
            (left_calls.get(), right_calls.get()),
            (1, 1),
            "both operands passed to verter_debug_assert_eq! must run exactly \
             once in every profile, including when debug_assertions is off \
             and the resulting assert_eq! check is compiled out"
        );
    }

    #[test]
    fn verter_debug_assert_ne_operands_evaluate_unconditionally() {
        use std::cell::Cell;
        let left_calls = Cell::new(0u32);
        let right_calls = Cell::new(0u32);
        verter_debug_assert_ne!(
            {
                left_calls.set(left_calls.get() + 1);
                1
            },
            {
                right_calls.set(right_calls.get() + 1);
                2
            }
        );
        assert_eq!(
            (left_calls.get(), right_calls.get()),
            (1, 1),
            "both operands passed to verter_debug_assert_ne! must run exactly \
             once in every profile, including when debug_assertions is off \
             and the resulting assert_ne! check is compiled out"
        );
    }

    // ------------------------------------------------------------------
    // Same seeded-defect proof, but for the CUSTOM-MESSAGE macro_rules arm
    // (`($cond:expr, $($arg:tt)+)` / `($left:expr, $right:expr,
    // $($arg:tt)+)`) — a SEPARATE arm from the one the three tests above
    // exercise. Each arm binds its condition/operands independently before
    // branching on `cfg!(debug_assertions)`, so a future edit that
    // reintroduces the unconditional-evaluation bug in only the
    // message-carrying arm would leave the three tests above green while
    // this one catches it. The custom message itself is inert (never
    // formatted unless the assertion actually fires, which it does not
    // here) — only the forced-evaluation property is under test.
    // ------------------------------------------------------------------

    #[test]
    fn verter_debug_assert_with_message_condition_evaluates_unconditionally() {
        use std::cell::Cell;
        let calls = Cell::new(0u32);
        verter_debug_assert!(
            {
                calls.set(calls.get() + 1);
                true
            },
            "custom message, must never affect argument evaluation"
        );
        assert_eq!(
            calls.get(),
            1,
            "the condition passed to the custom-message arm of verter_debug_assert! must run \
             exactly once in every profile, including when debug_assertions is off and the \
             resulting assert! check is compiled out"
        );
    }

    #[test]
    fn verter_debug_assert_eq_with_message_operands_evaluate_unconditionally() {
        use std::cell::Cell;
        let left_calls = Cell::new(0u32);
        let right_calls = Cell::new(0u32);
        verter_debug_assert_eq!(
            {
                left_calls.set(left_calls.get() + 1);
                1
            },
            {
                right_calls.set(right_calls.get() + 1);
                1
            },
            "custom message, must never affect argument evaluation"
        );
        assert_eq!(
            (left_calls.get(), right_calls.get()),
            (1, 1),
            "both operands passed to the custom-message arm of verter_debug_assert_eq! must run \
             exactly once in every profile, including when debug_assertions is off and the \
             resulting assert_eq! check is compiled out"
        );
    }

    #[test]
    fn verter_debug_assert_ne_with_message_operands_evaluate_unconditionally() {
        use std::cell::Cell;
        let left_calls = Cell::new(0u32);
        let right_calls = Cell::new(0u32);
        verter_debug_assert_ne!(
            {
                left_calls.set(left_calls.get() + 1);
                1
            },
            {
                right_calls.set(right_calls.get() + 1);
                2
            },
            "custom message, must never affect argument evaluation"
        );
        assert_eq!(
            (left_calls.get(), right_calls.get()),
            (1, 1),
            "both operands passed to the custom-message arm of verter_debug_assert_ne! must run \
             exactly once in every profile, including when debug_assertions is off and the \
             resulting assert_ne! check is compiled out"
        );
    }
}
