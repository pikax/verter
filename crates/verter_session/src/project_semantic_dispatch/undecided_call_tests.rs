//! A generic call whose inference the call executor cannot decide is a
//! typed gap: the direct-call rail's fallback instantiates the callee's
//! clause at `unknown` (or a declared default), which is the checker's
//! answer only for a call supplying no inference evidence.

use super::differential_global_library_tests::GLOBALS_LIB;
use super::differential_harness_tests::{Matrix, Read, STRICT};

const SOURCE: &str = r##"
declare const p1: Promise<number>;
declare function thenOf<T>(x: PromiseLike<T>): T;
declare function thenBox<T>(x: PromiseLike<T>): { v: T };
declare function thenNum<T>(x: PromiseLike<T>): number;
declare function id<T>(x: T): T;
declare function aw<T>(x: T): Awaited<T>;
export function w1() { return thenOf(p1); }
export function w2() { return thenBox(p1); }
export function w3() { return thenNum(p1); }
export function w4() { return id(p1); }
export function w5() { return aw(p1); }
"##;

/// tsc 7.0.2 over the global-library tests' library (`--noLib`), in all
/// four settings: `w1` is `number`, `w2` `{ v: number; }`, `w3` `number`,
/// `w4` `Promise<number>` and `w5` `number`.
///
/// The lane cannot relate the library's `Promise<number>` to
/// `PromiseLike<T>`, so `w1` and `w2` are the typed `UnrepresentableCallee`
/// gap, never the fallback's `unknown` / `{ v: unknown; }` they published
/// before; `w3`'s return reads no clause parameter, so the fallback is the
/// checker's answer. A binder takes the argument's carrier as written, so
/// `w4` and `w5` infer `T := Promise<number>` (they published `unknown`).
#[test]
fn an_undecided_generic_call_is_a_typed_gap_never_its_fallback() {
    let rows: Vec<(Read<'_>, Vec<&str>)> = vec![
        (Read::Return("w1"), vec!["number"]),
        (Read::Return("w2"), vec!["{ v: number; }"]),
        (Read::Return("w3"), vec!["number"]),
        (Read::Return("w4"), vec!["Promise<number>"]),
        (Read::Return("w5"), vec!["number"]),
    ];
    let verdicts = Matrix::new(SOURCE)
        .lib(GLOBALS_LIB)
        .settings(&[STRICT])
        .verdicts(&rows);
    let mut failures = Vec::new();
    for ((read, _), row) in rows.iter().zip(&verdicts) {
        let Read::Return(name) = read else {
            unreachable!("return rows only")
        };
        let verdict = &row[0];
        let expected_gap = matches!(*name, "w1" | "w2");
        let as_expected = if expected_gap {
            verdict.class == "GAP" && verdict.lane.contains("UnrepresentableCallee")
        } else {
            verdict.matched
        };
        if !as_expected {
            failures.push(format!("{name}: {} {}", verdict.class, verdict.lane));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
