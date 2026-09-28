//! Accessors of an object literal. A get / set pair is ONE property: its
//! type is the getter's annotation, else the setter's parameter
//! annotation, else the getter's inferred return (`getTypeOfAccessors`),
//! `readonly` when there is no setter. A literal holding a method or an
//! accessor is evaluated whole — its members read each other through
//! `this` — so a read of any one member sees every other, and a relation
//! compares the property, never the accessor functions.
//!
//! Every expected answer is TypeScript 7.0.2's, read off
//! `tsc --declaration --emitDeclarationOnly --ignoreConfig` (and the TS2322
//! message of `declare const p: <probe>; export const s: null = p;` for the
//! type probes) under each `strictNullChecks` × `noImplicitAny` setting; the
//! four settings agree.

use super::checker_probe_lane_tests::{degradation_in, mismatches, ProbeProject};

const FIXTURE: &str = "\
export function g1() { return { get v() { return 1; } }; }
export function g3() { const o = { get v() { return \"s\"; } }; return o.v; }
export function g4() { const o = { get v() { return 1; }, set v(x: number) {} }; return o.v; }
export function g5() { const o = { get v(): string | number { return 1; }, set v(x: string | number) {} }; return o.v; }
export function g6() { const o = { set v(x: string) {} }; return o.v; }
export function g7() { const o = { get v() { return 1; }, set v(x) {} }; return o; }
export function g8() { const o = { n: 2, get v() { return this.n; } }; return o.v; }
export function g9() { const o = { n: \"a\" as const, get v() { return this.n; } }; return o; }
export function g12() { const o = { get v() { return 1 as const; } }; return o.v; }
export function g13(c: boolean) { const o = { get v() { if (c) { return 1; } return \"s\"; } }; return o.v; }
export function g14() { const o = { set v(x: string) {}, get v() { return \"a\"; } }; return o; }
export function h6() { const o = { n: 1, get v() { return 2; } }; return o.v; }
export function h7() { const o = { n: 1, m() { return 2; } }; return o.m(); }
export function h8() { const o = { get v() { return 2; }, n: 1 }; return o.v; }
export function p1() { const o = { get v() { return \"a\"; }, set v(x: string | number) {} }; return o.v; }
export function p2() { const o = { set v(x: string | number) {}, get v() { return \"a\"; } }; return o.v; }
export function p3() { const o = { get v(): \"a\" { return \"a\"; }, set v(x: string) {} }; return o.v; }
export function p4() { const o = { get v() { return 1; }, set v(x) {} }; return o.v; }
export function p5() { const o = { get v() { return 1; }, set v(x: any) {} }; return o.v; }
";

/// A member read of an accessor reads the property type, and a literal
/// holding an accessor or a method is read whole, complete.
///
/// Measured on TypeScript 7.0.2 (`ReturnType<typeof f>`):
///
/// | function | checker |
/// | --- | --- |
/// | `g3`, `g6` | `string` |
/// | `g4`, `g8`, `h6`, `h7`, `h8`, `p4` | `number` |
/// | `g5`, `p1`, `p2` | `string \| number` |
/// | `g12` | `1` |
/// | `g13` | `"s" \| 1` |
/// | `p3` | `"a"` |
/// | `p5` | `any` |
#[test]
fn an_accessor_read_is_its_property_type() {
    let rows: &[(&str, &str)] = &[
        ("g3", "string"),
        ("g4", "number"),
        ("g5", "string | number"),
        ("g6", "string"),
        ("g8", "number"),
        ("g12", "1"),
        ("g13", "\"s\" | 1"),
        ("h6", "number"),
        ("h7", "number"),
        ("h8", "number"),
        ("p1", "string | number"),
        ("p2", "string | number"),
        ("p3", "\"a\""),
        ("p4", "number"),
        ("p5", "any"),
    ];
    let probes: Vec<(String, &str)> = rows
        .iter()
        .map(|(name, checker)| (format!("ReturnType<typeof {name}>"), *checker))
        .collect();
    let probe_rows: Vec<(&str, &str)> = probes
        .iter()
        .map(|(probe, checker)| (probe.as_str(), *checker))
        .collect();
    let mut failures = mismatches(FIXTURE, &probe_rows);
    for (name, _) in rows {
        match degradation_in(ProbeProject::default(), FIXTURE, name) {
            Ok(None) => {}
            other => failures.push(format!("`{name}` is not complete: {other:?}")),
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// The checker prints a literal's accessor as the property it is — `{
/// readonly v: number; }` for `g1`, `{ v: number; }` for `g7`, `{ n: "a";
/// readonly v: "a"; }` for `g9`, `{ v: string; }` for `g14` — where the
/// lane keeps the get / set members. Every reader agrees with the
/// property: its indexed access, its keys, and assignability both ways
/// with the printed object.
///
/// Measured on TypeScript 7.0.2: `ReturnType<typeof g1>["v"]` and
/// `ReturnType<typeof g7>["v"]` are `number`, `ReturnType<typeof
/// g14>["v"]` is `string`, `ReturnType<typeof g9>["v"]` is `"a"`, `keyof`
/// each is `"v"`, and each `extends` its printed object and back is `1`.
#[test]
fn an_accessor_surface_reads_and_relates_as_its_property() {
    let failures = mismatches(
        FIXTURE,
        &[
            ("ReturnType<typeof g1>[\"v\"]", "number"),
            ("ReturnType<typeof g7>[\"v\"]", "number"),
            ("ReturnType<typeof g14>[\"v\"]", "string"),
            ("ReturnType<typeof g9>[\"v\"]", "\"a\""),
            ("keyof ReturnType<typeof g1>", "\"v\""),
            ("keyof ReturnType<typeof g7>", "\"v\""),
            ("keyof ReturnType<typeof g14>", "\"v\""),
            (
                "ReturnType<typeof g1> extends { readonly v: number } ? 1 : 2",
                "1",
            ),
            (
                "{ readonly v: number } extends ReturnType<typeof g1> ? 1 : 2",
                "1",
            ),
            ("ReturnType<typeof g7> extends { v: number } ? 1 : 2", "1"),
            ("{ v: number } extends ReturnType<typeof g7> ? 1 : 2", "1"),
            ("ReturnType<typeof g14> extends { v: string } ? 1 : 2", "1"),
            ("{ v: string } extends ReturnType<typeof g14> ? 1 : 2", "1"),
            (
                "ReturnType<typeof g9> extends { n: \"a\"; readonly v: \"a\" } ? 1 : 2",
                "1",
            ),
            (
                "{ n: \"a\"; readonly v: \"a\" } extends ReturnType<typeof g9> ? 1 : 2",
                "1",
            ),
            ("ReturnType<typeof g7> extends { v: string } ? 1 : 2", "2"),
        ],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
