//! The inference census: rows the inference model is measured by, each
//! with TypeScript 7.0.2's answer and the checker diagnostics it reports
//! for the row, and the class the lane answers it with — exact, the
//! checker's recovery (the answer and its diagnostic), a typed gap, or
//! wrong-clean. The table is a ratchet: an exact answer or recovery that
//! becomes a gap, and any row that becomes wrong-clean, fails; a row that
//! improves fails until its class is updated here in the same change.
//!
//! Every answer is tsc's (`tsc --ignoreConfig --noEmit --strict
//! --noErrorTruncation` under each `strictNullChecks` x `noImplicitAny`
//! setting, listed strict, `strictNullChecks` off, `noImplicitAny` off,
//! both off; the library rows with `--noLib` over the library the
//! global-library differential tests register), read off TS2322 for
//! `declare const p: <probe>; const s: never = p;` or `const s: never =
//! f();`, and a return row's diagnostics are those tsc reports in `f`.

use super::differential_global_library_tests::GLOBALS_LIB;
use super::differential_harness_tests::{Matrix, Read, Verdict};

/// How the lane answers a row in one setting.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Class {
    /// The checker's answer, with no diagnostic on either side.
    Exact,
    /// The checker's recovery answer, with the diagnostics it reports.
    Recovery,
    /// A typed gap: a degraded value, a partial demand, or a gap marker.
    Gap,
    /// A complete answer that is not the checker's (a wrong type, or the
    /// right type with other diagnostics).
    WrongClean,
}

impl Class {
    /// The order a row may only climb: a wrong-clean answer is worst.
    fn rank(self) -> u8 {
        match self {
            Class::WrongClean => 0,
            Class::Gap => 1,
            Class::Recovery | Class::Exact => 2,
        }
    }
}

/// One census row.
struct Row {
    read: Read<'static>,
    /// tsc's answer in each setting.
    tsc: [&'static str; 4],
    /// The diagnostics tsc reports for the row in each setting.
    diagnostics: [&'static [u32]; 4],
    /// The lane's class in each setting.
    baseline: [Class; 4],
}

/// The class of one setting's verdict against tsc's diagnostics.
fn class_of(verdict: &Verdict, tsc_diagnostics: &[u32]) -> Class {
    if verdict.matched {
        let mut lane = verdict.diagnostics.clone();
        lane.sort_unstable();
        let mut tsc = tsc_diagnostics.to_vec();
        tsc.sort_unstable();
        return match (lane == tsc, tsc.is_empty()) {
            (true, true) => Class::Exact,
            (true, false) => Class::Recovery,
            (false, _) => Class::WrongClean,
        };
    }
    match verdict.class {
        "WRONG-CLEAN" => Class::WrongClean,
        "GAP" => Class::Gap,
        other => panic!("a census row may not {other}: {}", verdict.lane),
    }
}

/// Measure `rows` over `matrix` and compare each setting to its baseline.
fn census(name: &str, matrix: Matrix<'_>, rows: &[Row]) -> Vec<String> {
    let reads: Vec<(Read<'_>, Vec<&str>)> = rows
        .iter()
        .map(|row| (row.read, row.tsc.to_vec()))
        .collect();
    let verdicts = matrix.verdicts(&reads);
    let labels = [
        "strict",
        "strictNullChecks off",
        "noImplicitAny off",
        "both off",
    ];
    let mut failures = Vec::new();
    let mut observed = Vec::new();
    for (row, verdicts) in rows.iter().zip(&verdicts) {
        let probe = match row.read {
            Read::Type(text) | Read::Return(text) => text,
        };
        let mut classes = Vec::new();
        for (setting, verdict) in verdicts.iter().enumerate() {
            let class = class_of(verdict, row.diagnostics[setting]);
            classes.push(format!("{class:?}"));
            let expected = row.baseline[setting];
            if class != expected {
                let direction = if class.rank() < expected.rank() {
                    "REGRESSED"
                } else {
                    "moved (update the census)"
                };
                failures.push(format!(
                    "{name} `{probe}` [{}]: {direction} from {expected:?} to {class:?}; tsc answers `{}` with diagnostics {:?}, {} with diagnostics {:?}",
                    labels[setting],
                    row.tsc[setting],
                    row.diagnostics[setting],
                    verdict.lane,
                    verdict.diagnostics,
                ));
            }
        }
        observed.push(format!("CENSUS {name} {probe} => {}", classes.join(",")));
    }
    if std::env::var_os("VERTER_PRINT_CENSUS").is_some() {
        for line in &observed {
            eprintln!("{line}");
        }
    }
    failures
}

/// The fixture of [`call_inference`].
const CALL_INFERENCE: &str = r##"
declare function pw<T extends string>(a: { v: T }): T;
declare function pu<T>(a: { v: T }): T;
declare function pa<T extends string>(a: T[]): T;
declare function co<T>(f: (x: T) => void, g: (x: T) => void): T;
declare function mix<T>(x: T, f: (x: T) => void): T;
declare function mix2<T>(f: (x: T) => void, x: T): T;
declare function ret<T>(f: () => T, x: T): T;
declare function ni<T>(x: T, y: NoInfer<T>): T;
declare function nin<T>(x: T, y: { v: NoInfer<T> }): T;
declare function cr<const T extends readonly unknown[]>(...args: T): T;
declare function un<T>(x: T | undefined): T;
declare function arr<T>(x: T[], y: T[]): T;
declare function box<T>(x: { a: T; b: T }): T;
declare function fnr<T>(f: (x: number) => T): T;
declare function both<T>(x: T, f: (x: T) => T): T;
export function r1() { return pw({ v: "x" }); }
export function r2() { const o = { v: "x" }; return pw(o); }
export function r3() { return pu({ v: "x" }); }
export function r4() { return pa(["x"]); }
export function r5() { return co((x: string) => {}, (x: "a") => {}); }
export function r6() { return mix("a", (x: string) => {}); }
export function r7() { return mix2((x: string) => {}, "a"); }
export function r8() { return ret(() => 1, "a"); }
export function r9() { return ni("a", "b"); }
export function r10() { return nin(1, { v: 2 }); }
export function r11() { return cr("x", 1, { v: "x" }); }
export function r12() { return un(undefined); }
export function r13() { return arr([1], ["a"]); }
export function r14() { return box({ a: 1, b: "x" }); }
export function r15() { return fnr((x) => x > 0); }
export function r16() { return both(1, (x) => x + 1); }
"##;

/// The census over [`CALL_INFERENCE`].
#[test]
fn call_inference() {
    let rows = [
        Row {
            read: Read::Return(r#"r1"#),
            tsc: [r#""x""#, r#""x""#, r#""x""#, r#""x""#],
            diagnostics: [&[], &[], &[], &[]],
            baseline: [
                Class::WrongClean,
                Class::WrongClean,
                Class::WrongClean,
                Class::WrongClean,
            ],
        },
        Row {
            read: Read::Return(r#"r2"#),
            tsc: [r#"string"#, r#"string"#, r#"string"#, r#"string"#],
            diagnostics: [&[], &[], &[], &[]],
            baseline: [Class::Exact, Class::Exact, Class::Exact, Class::Exact],
        },
        Row {
            read: Read::Return(r#"r3"#),
            tsc: [r#"string"#, r#"string"#, r#"string"#, r#"string"#],
            diagnostics: [&[], &[], &[], &[]],
            baseline: [Class::Exact, Class::Exact, Class::Exact, Class::Exact],
        },
        Row {
            read: Read::Return(r#"r4"#),
            tsc: [r#""x""#, r#""x""#, r#""x""#, r#""x""#],
            diagnostics: [&[], &[], &[], &[]],
            baseline: [
                Class::WrongClean,
                Class::WrongClean,
                Class::WrongClean,
                Class::WrongClean,
            ],
        },
        Row {
            read: Read::Return(r#"r5"#),
            tsc: [r#""a""#, r#""a""#, r#""a""#, r#""a""#],
            diagnostics: [&[], &[], &[], &[]],
            baseline: [Class::Exact, Class::Exact, Class::Exact, Class::Exact],
        },
        Row {
            read: Read::Return(r#"r6"#),
            tsc: [r#"string"#, r#"string"#, r#"string"#, r#"string"#],
            diagnostics: [&[], &[], &[], &[]],
            baseline: [Class::Exact, Class::Exact, Class::Exact, Class::Exact],
        },
        Row {
            read: Read::Return(r#"r7"#),
            tsc: [r#"string"#, r#"string"#, r#"string"#, r#"string"#],
            diagnostics: [&[], &[], &[], &[]],
            baseline: [Class::Exact, Class::Exact, Class::Exact, Class::Exact],
        },
        Row {
            read: Read::Return(r#"r8"#),
            tsc: [r#"number"#, r#"number"#, r#"number"#, r#"number"#],
            diagnostics: [&[2345], &[2345], &[2345], &[2345]],
            baseline: [
                Class::Recovery,
                Class::Recovery,
                Class::Recovery,
                Class::Recovery,
            ],
        },
        Row {
            read: Read::Return(r#"r9"#),
            tsc: [r#"string"#, r#"string"#, r#"string"#, r#"string"#],
            diagnostics: [&[2345], &[2345], &[2345], &[2345]],
            baseline: [
                Class::Recovery,
                Class::Recovery,
                Class::Recovery,
                Class::Recovery,
            ],
        },
        Row {
            read: Read::Return(r#"r10"#),
            tsc: [r#"number"#, r#"number"#, r#"number"#, r#"number"#],
            diagnostics: [&[2322], &[2322], &[2322], &[2322]],
            baseline: [
                Class::WrongClean,
                Class::WrongClean,
                Class::WrongClean,
                Class::WrongClean,
            ],
        },
        Row {
            read: Read::Return(r#"r11"#),
            tsc: [
                r#"readonly ["x", 1, { readonly v: "x"; }]"#,
                r#"readonly ["x", 1, { readonly v: "x"; }]"#,
                r#"readonly ["x", 1, { readonly v: "x"; }]"#,
                r#"readonly ["x", 1, { readonly v: "x"; }]"#,
            ],
            diagnostics: [&[], &[], &[], &[]],
            baseline: [Class::Exact, Class::Exact, Class::Exact, Class::Exact],
        },
        Row {
            read: Read::Return(r#"r12"#),
            tsc: [r#"undefined"#, r#"any"#, r#"undefined"#, r#"any"#],
            diagnostics: [&[], &[], &[], &[]],
            baseline: [
                Class::Exact,
                Class::WrongClean,
                Class::Exact,
                Class::WrongClean,
            ],
        },
        Row {
            read: Read::Return(r#"r13"#),
            tsc: [r#"number"#, r#"number"#, r#"number"#, r#"number"#],
            diagnostics: [&[2322], &[2322], &[2322], &[2322]],
            baseline: [
                Class::WrongClean,
                Class::WrongClean,
                Class::WrongClean,
                Class::WrongClean,
            ],
        },
        Row {
            read: Read::Return(r#"r14"#),
            tsc: [r#"number"#, r#"number"#, r#"number"#, r#"number"#],
            diagnostics: [&[2322], &[2322], &[2322], &[2322]],
            baseline: [
                Class::WrongClean,
                Class::WrongClean,
                Class::WrongClean,
                Class::WrongClean,
            ],
        },
        Row {
            read: Read::Return(r#"r15"#),
            tsc: [r#"boolean"#, r#"boolean"#, r#"boolean"#, r#"boolean"#],
            diagnostics: [&[], &[], &[], &[]],
            baseline: [Class::Exact, Class::Exact, Class::Exact, Class::Exact],
        },
        Row {
            read: Read::Return(r#"r16"#),
            tsc: [r#"number"#, r#"number"#, r#"number"#, r#"number"#],
            diagnostics: [&[], &[], &[], &[]],
            baseline: [
                Class::WrongClean,
                Class::WrongClean,
                Class::WrongClean,
                Class::WrongClean,
            ],
        },
    ];
    let failures = census("call_inference", Matrix::new(CALL_INFERENCE), &rows);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// The fixture of [`conditional_inference`].
const CONDITIONAL_INFERENCE: &str = r##"
type Box<X> = { v: X };
type PE<M, L> = { type: "ParsingError"; message: M; lineNumber: L };
type F<X> = [X] extends [PE<infer M, any>] ? M : "ok";
type MV<T> = T extends { a: infer U; b: (x: infer U) => void } ? U : 0;
type CO<T> = T extends { a: (x: infer U) => void; b: (x: infer U) => void } ? U : 0;
type R<T> = { [K in keyof T as K]: T[K] };
type U<T> = T extends R<infer X> ? X : "no";
type Rep<T> = T extends [infer A, infer A] ? A : 0;
"##;

/// The census over [`CONDITIONAL_INFERENCE`].
#[test]
fn conditional_inference() {
    let rows = [
        Row {
            read: Read::Type(r#"[1] extends [infer X extends string] ? X : 0"#),
            tsc: [r#"0"#, r#"0"#, r#"0"#, r#"0"#],
            diagnostics: [&[], &[], &[], &[]],
            baseline: [Class::Gap, Class::Gap, Class::Gap, Class::Gap],
        },
        Row {
            read: Read::Type(r#"["x"] extends [infer X extends string] ? X : 0"#),
            tsc: [r#""x""#, r#""x""#, r#""x""#, r#""x""#],
            diagnostics: [&[], &[], &[], &[]],
            baseline: [Class::Gap, Class::Gap, Class::Gap, Class::Gap],
        },
        Row {
            read: Read::Type(r#"["x" | 1] extends [infer X extends string] ? X : 0"#),
            tsc: [r#"0"#, r#"0"#, r#"0"#, r#"0"#],
            diagnostics: [&[], &[], &[], &[]],
            baseline: [Class::Gap, Class::Gap, Class::Gap, Class::Gap],
        },
        Row {
            read: Read::Type(r#""12px" extends `${infer N extends number}px` ? N : 0"#),
            tsc: [r#"12"#, r#"12"#, r#"12"#, r#"12"#],
            diagnostics: [&[], &[], &[], &[]],
            baseline: [Class::Gap, Class::Gap, Class::Gap, Class::Gap],
        },
        Row {
            read: Read::Type(r#""1e3px" extends `${infer N extends number}px` ? N : 0"#),
            tsc: [r#"number"#, r#"number"#, r#"number"#, r#"number"#],
            diagnostics: [&[], &[], &[], &[]],
            baseline: [Class::Gap, Class::Gap, Class::Gap, Class::Gap],
        },
        Row {
            read: Read::Type(r#""3" extends `${infer N extends 1 | 2}` ? N : 0"#),
            tsc: [r#"0"#, r#"0"#, r#"0"#, r#"0"#],
            diagnostics: [&[], &[], &[], &[]],
            baseline: [Class::Gap, Class::Gap, Class::Gap, Class::Gap],
        },
        Row {
            read: Read::Type(r#"Box<"a"> extends Box<infer P> ? P : 0"#),
            tsc: [r#""a""#, r#""a""#, r#""a""#, r#""a""#],
            diagnostics: [&[], &[], &[], &[]],
            baseline: [Class::Gap, Class::Gap, Class::Gap, Class::Gap],
        },
        Row {
            read: Read::Type(r#"[Box<"a">] extends [Box<infer P>] ? P : 0"#),
            tsc: [r#""a""#, r#""a""#, r#""a""#, r#""a""#],
            diagnostics: [&[], &[], &[], &[]],
            baseline: [Class::Gap, Class::Gap, Class::Gap, Class::Gap],
        },
        Row {
            read: Read::Type(r#"F<PE<"m", 1>>"#),
            tsc: [r#""m""#, r#""m""#, r#""m""#, r#""m""#],
            diagnostics: [&[], &[], &[], &[]],
            baseline: [Class::Gap, Class::Gap, Class::Gap, Class::Gap],
        },
        Row {
            read: Read::Type(r#"MV<{ a: "x"; b: (x: string) => void }>"#),
            tsc: [r#""x""#, r#""x""#, r#""x""#, r#""x""#],
            diagnostics: [&[], &[], &[], &[]],
            baseline: [Class::Exact, Class::Exact, Class::Exact, Class::Exact],
        },
        Row {
            read: Read::Type(r#"CO<{ a: (x: "x") => void; b: (x: string) => void }>"#),
            tsc: [r#""x""#, r#""x""#, r#""x""#, r#""x""#],
            diagnostics: [&[], &[], &[], &[]],
            baseline: [Class::Exact, Class::Exact, Class::Exact, Class::Exact],
        },
        Row {
            read: Read::Type(r#"U<{ x: 1 }>"#),
            tsc: [r#"unknown"#, r#"unknown"#, r#"unknown"#, r#"unknown"#],
            diagnostics: [&[], &[], &[], &[]],
            baseline: [Class::Gap, Class::Gap, Class::Gap, Class::Gap],
        },
        Row {
            read: Read::Type(r#"U<R<{ x: 1 }>>"#),
            tsc: [
                r#"{ x: 1; }"#,
                r#"{ x: 1; }"#,
                r#"{ x: 1; }"#,
                r#"{ x: 1; }"#,
            ],
            diagnostics: [&[], &[], &[], &[]],
            baseline: [Class::Gap, Class::Gap, Class::Gap, Class::Gap],
        },
        Row {
            read: Read::Type(r#"Rep<[1, 2]>"#),
            tsc: [r#"1 | 2"#, r#"1 | 2"#, r#"1 | 2"#, r#"1 | 2"#],
            diagnostics: [&[], &[], &[], &[]],
            baseline: [Class::Exact, Class::Exact, Class::Exact, Class::Exact],
        },
        Row {
            read: Read::Type(r#"Rep<[1, 1]>"#),
            tsc: [r#"1"#, r#"1"#, r#"1"#, r#"1"#],
            diagnostics: [&[], &[], &[], &[]],
            baseline: [Class::Exact, Class::Exact, Class::Exact, Class::Exact],
        },
    ];
    let failures = census(
        "conditional_inference",
        Matrix::new(CONDITIONAL_INFERENCE),
        &rows,
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// The fixture of [`library_inference`].
const LIBRARY_INFERENCE: &str = r##"
declare const p1: Promise<number>;
declare function thenOf<T>(x: PromiseLike<T>): T;
export function l1() { return thenOf(p1); }
export function l2() { return p1.then((x) => [x]); }
export function l3() { return Promise.resolve(p1); }
"##;

/// The census over [`LIBRARY_INFERENCE`].
#[test]
fn library_inference() {
    let rows = [
        Row {
            read: Read::Type(r#"Promise<Promise<"x">> extends Promise<infer U> ? U : 0"#),
            tsc: [
                r#"Promise<"x">"#,
                r#"Promise<"x">"#,
                r#"Promise<"x">"#,
                r#"Promise<"x">"#,
            ],
            diagnostics: [&[], &[], &[], &[]],
            baseline: [Class::Gap, Class::Gap, Class::Gap, Class::Gap],
        },
        Row {
            read: Read::Type(r#"Awaited<Promise<Promise<"x">>>"#),
            tsc: [r#""x""#, r#""x""#, r#""x""#, r#""x""#],
            diagnostics: [&[], &[], &[], &[]],
            baseline: [Class::Exact, Class::Exact, Class::Exact, Class::Exact],
        },
        Row {
            read: Read::Type(r#"[Promise<1>] extends [PromiseLike<number>] ? 1 : 2"#),
            tsc: [r#"1"#, r#"1"#, r#"1"#, r#"1"#],
            diagnostics: [&[], &[], &[], &[]],
            baseline: [Class::Gap, Class::Gap, Class::Gap, Class::Gap],
        },
        Row {
            read: Read::Return(r#"l1"#),
            tsc: [r#"number"#, r#"number"#, r#"number"#, r#"number"#],
            diagnostics: [&[], &[], &[], &[]],
            baseline: [Class::Gap, Class::Gap, Class::Gap, Class::Gap],
        },
        Row {
            read: Read::Return(r#"l2"#),
            tsc: [
                r#"Promise<number[]>"#,
                r#"Promise<number[]>"#,
                r#"Promise<number[]>"#,
                r#"Promise<number[]>"#,
            ],
            diagnostics: [&[], &[], &[], &[]],
            baseline: [Class::Gap, Class::Gap, Class::Gap, Class::Gap],
        },
        Row {
            read: Read::Return(r#"l3"#),
            tsc: [
                r#"Promise<number>"#,
                r#"Promise<number>"#,
                r#"Promise<number>"#,
                r#"Promise<number>"#,
            ],
            diagnostics: [&[], &[], &[], &[]],
            baseline: [
                Class::WrongClean,
                Class::WrongClean,
                Class::WrongClean,
                Class::WrongClean,
            ],
        },
    ];
    let failures = census(
        "library_inference",
        Matrix::new(LIBRARY_INFERENCE).lib(GLOBALS_LIB),
        &rows,
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
