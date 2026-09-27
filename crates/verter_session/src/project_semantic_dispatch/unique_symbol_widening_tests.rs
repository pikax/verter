//! Where a `unique symbol` type widens to `symbol`. An unannotated
//! declaration widens one another declaration created (`const l = u` is
//! `symbol`, a union holding one keeps it); a return type that is one
//! `unique symbol` — a unit type — widens; and a mutable location with no
//! contextual type (an object-literal member, an array element) widens every
//! one, union arms included, as does an assignment to an unannotated
//! variable its initializer declared. A declared return, `as const`, a
//! union return and an auto-typed variable's assignment keep it.
//!
//! Every expected answer is TypeScript 7.0.2's, read off
//! `tsc --declaration --emitDeclarationOnly --ignoreConfig` under each
//! `strictNullChecks` × `noImplicitAny` setting; the four settings agree
//! except where a table says otherwise.

use super::checker_probe_lane_tests::{degradation_in, mismatches_in, ProbeProject};

const FIXTURE: &str = "\
declare const u: unique symbol;
export function retU() { return u; }
export function retLocalConst() { const l = u; return l; }
export function retLocalLet() { let l = u; return l; }
export function retLocalVar() { var l = u; return l; }
export function retAnnotated(): typeof u { return u; }
export function retLocalAnnotated() { const l: typeof u = u; return l; }
export function retObj() { return { u }; }
export function retObjConst() { const l = u; return { l }; }
export function retArr() { return [u]; }
export function retCond(c: boolean) { return c ? u : 1; }
export function retUnionArm(c: boolean) { if (c) { return u; } return \"s\"; }
export function retTwice(c: boolean) { if (c) { return u; } return u; }
export function retArrow() { return () => u; }
export function nestedReturn() { const f = () => u; return f(); }
export function letUnion(c: boolean) { let x = c ? u : 1; return x; }
export function constUnion(c: boolean) { const x = c ? u : 1; return x; }
export function objUnion(c: boolean) { return { k: c ? u : 1 }; }
export function arrMixed() { return [u, 1]; }
export function objConst() { return { u } as const; }
export function retFallthrough(c: boolean) { if (c) { return u; } }
export function autoLet() { let x; x = u; return x; }
export function constLocalUnion(c: boolean) { const l = u; return c ? l : 1; }
export function letLocalUnion(c: boolean) { let l = u; return c ? l : 1; }
export function varLocalUnion(c: boolean) { var l = u; return c ? l : 1; }
export function letReassignU(c: boolean) { let l = u; l = u; return c ? l : 1; }
export function autoLetUnion(c: boolean) { let x; x = u; return c ? x : 1; }
";

fn failures_in(project: ProbeProject<'_>, rows: &[(&str, &str)]) -> Vec<String> {
    let probes: Vec<(String, &str)> = rows
        .iter()
        .map(|(name, checker)| (format!("ReturnType<typeof {name}>"), *checker))
        .collect();
    let probe_rows: Vec<(&str, &str)> = probes
        .iter()
        .map(|(probe, checker)| (probe.as_str(), *checker))
        .collect();
    let mut failures = mismatches_in(project, FIXTURE, &probe_rows);
    for (name, _) in rows {
        match degradation_in(project, FIXTURE, name) {
            Ok(None) => {}
            other => failures.push(format!("`{name}` is not complete: {other:?}")),
        }
    }
    failures
}

/// Measured on TypeScript 7.0.2 (`ReturnType<typeof f>`):
///
/// | function | checker |
/// | --- | --- |
/// | `retU`, `retLocalConst`, `retLocalLet`, `retLocalVar`, `retLocalAnnotated`, `retTwice`, `nestedReturn` | `symbol` |
/// | `retAnnotated` | `typeof u` (the probe reads `symbol`, below) |
/// | `retObj` | `{ u: symbol; }` |
/// | `retObjConst` | `{ l: symbol; }` |
/// | `retArr` | `symbol[]` |
/// | `retCond`, `constUnion` | `1 \| typeof u` |
/// | `retUnionArm` | `"s" \| typeof u` |
/// | `retArrow` | `() => symbol` |
/// | `letUnion` | `number \| typeof u` |
/// | `objUnion` | `{ k: number \| symbol; }` |
/// | `arrMixed` | `(number \| symbol)[]` |
/// | `objConst` | `{ readonly u: typeof u; }` |
/// | `constLocalUnion`, `letLocalUnion`, `varLocalUnion`, `letReassignU` | `symbol \| 1` |
///
/// | function | `strictNullChecks` on | off |
/// | --- | --- | --- |
/// | `retFallthrough` | `typeof u \| undefined` | `symbol` |
///
/// | function | `noImplicitAny` on | off |
/// | --- | --- | --- |
/// | `autoLet` | `symbol` | `any` |
/// | `autoLetUnion` | `1 \| typeof u` | `any` |
///
/// A probe reads its type as the return of a function holding a `const`
/// annotated with it, and that return widens a lone `typeof u` as any
/// return does: `retAnnotated` is `typeof u`, and the probe function over
/// `ReturnType<typeof retAnnotated>` (`function probe() { const p:
/// ReturnType<typeof retAnnotated> = null as any; return p; }`) is
/// `symbol` on 7.0.2, all four settings — the answer the probe reads.
#[test]
fn a_unique_symbol_widens_where_the_checker_widens_it() {
    let rows: &[(&str, &str)] = &[
        ("retU", "symbol"),
        ("retLocalConst", "symbol"),
        ("retLocalLet", "symbol"),
        ("retLocalVar", "symbol"),
        ("retLocalAnnotated", "symbol"),
        ("retObj", "{ u: symbol; }"),
        ("retObjConst", "{ l: symbol; }"),
        ("retArr", "symbol[]"),
        ("retCond", "1 | typeof u"),
        ("retUnionArm", "\"s\" | typeof u"),
        ("retTwice", "symbol"),
        ("retArrow", "() => symbol"),
        ("nestedReturn", "symbol"),
        ("letUnion", "number | typeof u"),
        ("constUnion", "1 | typeof u"),
        ("objUnion", "{ k: number | symbol; }"),
        ("arrMixed", "(number | symbol)[]"),
        ("objConst", "{ readonly u: typeof u; }"),
        ("constLocalUnion", "symbol | 1"),
        ("letLocalUnion", "symbol | 1"),
        ("varLocalUnion", "symbol | 1"),
        ("letReassignU", "symbol | 1"),
    ];
    for (options, fallthrough, auto_let, auto_let_union) in [
        (
            r#"{ "strictNullChecks": true, "noImplicitAny": true }"#,
            "typeof u | undefined",
            "symbol",
            "1 | typeof u",
        ),
        (
            r#"{ "strictNullChecks": true, "noImplicitAny": false }"#,
            "typeof u | undefined",
            "any",
            "any",
        ),
        (
            r#"{ "strictNullChecks": false, "noImplicitAny": true }"#,
            "symbol",
            "symbol",
            "1 | typeof u",
        ),
        (
            r#"{ "strictNullChecks": false, "noImplicitAny": false }"#,
            "symbol",
            "any",
            "any",
        ),
    ] {
        let project = ProbeProject {
            compiler_options: Some(options),
            ..ProbeProject::default()
        };
        let mut failures = failures_in(project, rows);
        failures.extend(failures_in(
            project,
            &[
                ("retFallthrough", fallthrough),
                ("autoLet", auto_let),
                ("autoLetUnion", auto_let_union),
            ],
        ));
        failures.extend(mismatches_in(
            project,
            FIXTURE,
            &[("ReturnType<typeof retAnnotated>", "symbol")],
        ));
        match degradation_in(project, FIXTURE, "retAnnotated") {
            Ok(None) => {}
            other => failures.push(format!("`retAnnotated` is not complete: {other:?}")),
        }
        assert!(failures.is_empty(), "{options}:\n{}", failures.join("\n"));
    }
}
