//! A call through a module-level callee in a control test or at statement
//! position. The checker reads its narrowing from the callee's declared
//! signatures alone: a type predicate narrows in a control test (an
//! unannotated variable holding a predicate function included), an
//! assertion narrows at statement position only through an explicitly
//! typed callee, and any other signature narrows nothing and does not end
//! the path. A callee whose declaration set this file closes — a variable,
//! a module's unexported function or overload group — is read through
//! `SignaturesOfType`; at statement position an imported, exported or
//! script-global function, whose set a module augmentation or another
//! script can extend, keeps the typed guard-narrowing gap. A control test
//! the call executor resolves (an imported predicate included) narrows by
//! the signature the call selects.
//!
//! Every expected answer is TypeScript 7.0.2's, read off
//! `tsc --declaration --emitDeclarationOnly --ignoreConfig --moduleResolution
//! bundler` under each `strictNullChecks` × `noImplicitAny` setting; the
//! four settings agree.

use super::checker_probe_lane_tests::{degradation_in, mismatches_in, ProbeProject};

const LIB: &str = "\
export function check(): boolean { return true; }
export declare function isStr(v: unknown): v is string;
export declare function assertStr(v: unknown): asserts v is string;
";

const FIXTURE: &str = "\
import { check, isStr, assertStr } from \"./lib\";
declare function dcheck(): boolean;
declare const more: () => boolean;
declare const pred: (v: unknown) => v is string;
declare function dover(x: string): boolean;
declare function dover(x: number): boolean;
declare let lcheck: () => boolean;
declare const annotatedAssert: (v: unknown) => asserts v is string;
const unannotatedAssert = function (v: unknown): asserts v is string {};
export function w1(x: string | number) { if (dcheck()) { return x; } return 0; }
export function w2() { let x: string | number = \"a\"; while (more()) { x = 1; } return x; }
export function w5(x: unknown) { if (pred(x)) { return x; } return 0; }
export function w8(x: string | number) { if (more()) { return x; } return 0; }
export function w9(x: string | number) { return more() ? x : 0; }
export function w12(x: string | number) { if (lcheck()) { return x; } return 0; }
export function w13() { let x: string | number = \"a\"; do { x = 1; } while (more()); return x; }
export function w14(x: unknown) { if (dover(1 as number)) { return x; } return 0; }
export function s6(x: unknown) { if (!pred(x)) { return 0; } return x; }
export function s8(x: string | number) { if (lcheck() && typeof x === \"string\") { return x; } return 0; }
export function a1(x: unknown) { annotatedAssert(x); return x; }
export function a2(x: string | number) { unannotatedAssert(x); return x; }
export function a3(x: string | number) { more(); return x; }
export function i3(x: string | number) { if (check()) { return x; } return 0; }
export function i4(x: unknown) { if (isStr(x)) { return x; } return 0; }
export function i5(x: unknown) { assertStr(x); return x; }
";

fn project() -> ProbeProject<'static> {
    ProbeProject {
        files: &[("lib.ts", LIB)],
        ..ProbeProject::default()
    }
}

/// A callee whose declaration set this file closes narrows exactly as its
/// declared signatures say, complete.
///
/// Measured on TypeScript 7.0.2 (`ReturnType<typeof f>`):
///
/// | function | checker |
/// | --- | --- |
/// | `w1`, `w2`, `w8`, `w9`, `w12`, `a2`, `a3` | `string \| number` |
/// | `w5`, `s6`, `s8` | `string \| 0` |
/// | `w13` | `number` |
/// | `w14` | `unknown` |
/// | `a1` | `string` |
#[test]
fn a_closed_callee_narrows_through_its_declared_signatures() {
    let rows: &[(&str, &str)] = &[
        ("w1", "string | number"),
        ("w2", "string | number"),
        ("w5", "string | 0"),
        ("w8", "string | number"),
        ("w9", "string | number"),
        ("w12", "string | number"),
        ("w13", "number"),
        ("w14", "unknown"),
        ("s6", "string | 0"),
        ("s8", "string | 0"),
        ("a1", "string"),
        ("a2", "string | number"),
        ("a3", "string | number"),
    ];
    let probes: Vec<(String, &str)> = rows
        .iter()
        .map(|(name, checker)| (format!("ReturnType<typeof {name}>"), *checker))
        .collect();
    let probe_rows: Vec<(&str, &str)> = probes
        .iter()
        .map(|(probe, checker)| (probe.as_str(), *checker))
        .collect();
    let mut failures = mismatches_in(project(), FIXTURE, &probe_rows);
    for (name, _) in rows {
        match degradation_in(project(), FIXTURE, name) {
            Ok(None) => {}
            other => failures.push(format!("`{name}` is not complete: {other:?}")),
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// A control call through an imported function is resolved by the call
/// executor over the module binding's checker-visible signature set: a
/// call handing no reference narrows nothing (`i3`), and a predicate
/// narrows its argument (`i4`). A statement call's assertion through an
/// imported function, whose declared signatures a `declare module`
/// augmentation can extend, keeps the typed guard-narrowing gap (`i5`).
///
/// Measured on TypeScript 7.0.2: `ReturnType<typeof i3>` is `string |
/// number`, `ReturnType<typeof i4>` is `string | 0` and `ReturnType<typeof
/// i5>` is `string`.
#[test]
fn an_imported_function_callee_keeps_the_typed_gap() {
    let failures = mismatches_in(
        project(),
        FIXTURE,
        &[
            ("ReturnType<typeof i3>", "string | number"),
            ("ReturnType<typeof i4>", "string | 0"),
        ],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    for name in ["i3", "i4"] {
        assert_eq!(degradation_in(project(), FIXTURE, name), Ok(None), "{name}");
    }
    assert_eq!(
        degradation_in(project(), FIXTURE, "i5"),
        Ok(Some(crate::semantic_query::FlowReturnDegradation::FlowGap(
            crate::semantic_query::FlowGap::GuardNarrowing
        ))),
        "i5"
    );
}
