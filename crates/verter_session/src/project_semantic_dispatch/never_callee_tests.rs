//! A statement call through a callee whose declared signatures return
//! `never` ends the path, as the checker's flow graph does, when the callee
//! is an explicitly typed reference (`getEffectsSignature`): a function
//! declaration, or a variable annotated with a function type. An
//! unannotated `const` holding a throwing arrow gives the call no effect
//! (the assertion rules), and a `void` operand is not a call statement.
//!
//! Every expected answer is TypeScript 7.0.2's, read off
//! `tsc --declaration --emitDeclarationOnly --ignoreConfig` under each
//! `strictNullChecks` × `noImplicitAny` setting; `noImplicitAny` changes
//! none.

use super::checker_probe_lane_tests::{degradation_in, mismatches_in, ProbeProject};

const NEVER: &str = "\
declare function fail(): never;
declare const cfail: () => never;
const ufail = () => { throw 0; };
const afail: () => never = () => { throw 0; };
function lfail(): never { throw 0; }
declare namespace NS { function fail(): never; }
declare const obj: { fail(): never; };
class K { static fail(): never { throw 0; } m(): never { throw 0; } }
declare let lf: () => never;
declare function fover(x: string): never;
declare function fover(x: number): void;
declare function gfail<T>(x: T): never;
declare const O: { fover(x: string): never; fover(x: number): void; gfail<T>(x: T): never; };
declare function mixb(x: string): never;
declare function mixb(x: number): string;
export function n1(x: string | number) { if (typeof x === \"string\") { return x; } fail(); }
export function n2(x: string | number) { if (typeof x === \"string\") { return x; } cfail(); }
export function n3(x: string | number) { if (typeof x === \"string\") { return x; } ufail(); }
export function n4(x: string | number) { if (typeof x === \"string\") { return x; } afail(); }
export function n5(x: string | number) { if (typeof x === \"string\") { return x; } lfail(); }
export function n6(x: string | number) { if (typeof x === \"string\") { return x; } NS.fail(); }
export function n7(x: string | number) { if (typeof x === \"string\") { return x; } obj.fail(); }
export function n8(x: string | number) { if (typeof x === \"string\") { return x; } K.fail(); }
export function n9(x: string | number) { if (typeof x === \"string\") { return x; } lf(); }
export function n10(x: string | number) { if (typeof x === \"string\") { return x; } fover(1); }
export function n11(x: string | number) { if (typeof x === \"string\") { return x; } fover(\"a\"); }
export function n12(x: string | number) { if (typeof x === \"string\") { return x; } gfail(1); }
export function n13(x: string | number) { fail(); return x; }
export function n14(x: string | number) { if (typeof x === \"string\") { return x; } new K().m(); }
export function n15(x: string | number) { if (typeof x === \"string\") { return x; } (fail)(); }
export function n16(x: string | number) { if (typeof x === \"string\") { return x; } void fail(); }
export function q1(x: string | number) { if (typeof x === \"string\") { return x; } O.fover(1); }
export function q2(x: string | number) { if (typeof x === \"string\") { return x; } O.fover(\"a\"); }
export function q3(x: string | number) { if (typeof x === \"string\") { return x; } O.gfail(1); }
export function n21(x: string | number) { if (typeof x === \"string\") { return x; } mixb(2); }
";

/// Assert each `(function, [strictNullChecks on, off])` return, complete.
fn assert_rows(rows: &[(&str, [&str; 2])]) {
    for (options, column) in [
        (r#"{ "strictNullChecks": true, "noImplicitAny": true }"#, 0),
        (r#"{ "strictNullChecks": true, "noImplicitAny": false }"#, 0),
        (r#"{ "strictNullChecks": false, "noImplicitAny": true }"#, 1),
        (
            r#"{ "strictNullChecks": false, "noImplicitAny": false }"#,
            1,
        ),
    ] {
        let project = ProbeProject {
            compiler_options: Some(options),
            ..ProbeProject::default()
        };
        let probes: Vec<(String, &str)> = rows
            .iter()
            .map(|(name, answers)| (format!("ReturnType<typeof {name}>"), answers[column]))
            .collect();
        let probe_rows: Vec<(&str, &str)> = probes
            .iter()
            .map(|(probe, checker)| (probe.as_str(), *checker))
            .collect();
        let mut failures = mismatches_in(project, NEVER, &probe_rows);
        for (name, _) in rows {
            match degradation_in(project, NEVER, name) {
                Ok(None) => {}
                other => failures.push(format!("`{name}` is not complete: {other:?}")),
            }
        }
        assert!(failures.is_empty(), "{options}:\n{}", failures.join("\n"));
    }
}

/// Measured on TypeScript 7.0.2 (`ReturnType<typeof f>`, `strictNullChecks`
/// on / off):
///
/// | function | on | off |
/// | --- | --- | --- |
/// | `n1`, `n2`, `n4`, `n5`, `n9`, `n12`, `n15` | `string` | `string` |
/// | `n3`, `n16` | `string \| undefined` | `string` |
/// | `n13` | `string \| number` | `string \| number` |
#[test]
fn a_statement_call_through_a_never_returning_callee_ends_the_path() {
    assert_rows(&[
        ("n1", ["string", "string"]),
        ("n2", ["string", "string"]),
        ("n3", ["string | undefined", "string"]),
        ("n4", ["string", "string"]),
        ("n5", ["string", "string"]),
        ("n9", ["string", "string"]),
        ("n12", ["string", "string"]),
        ("n13", ["string | number", "string | number"]),
        ("n15", ["string", "string"]),
        ("n16", ["string | undefined", "string"]),
    ]);
}

/// A qualified callee (`NS.fail()`, `obj.fail()`, `K.fail()`) whose
/// declared signature returns `never` ends the path too.
///
/// Measured on TypeScript 7.0.2: `ReturnType<typeof n6>`, `n7` and `n8` are
/// `string` under every setting.
#[test]
fn a_statement_call_through_a_qualified_never_callee_ends_the_path() {
    assert_rows(&[
        ("n6", ["string", "string"]),
        ("n7", ["string", "string"]),
        ("n8", ["string", "string"]),
    ]);
}

/// Overload resolution picks the signature whose effect a statement call
/// takes (`fover(1)` resolves the `void` overload, `fover("a")` the `never`
/// one, `mixb(2)` the `string` one), and a call through an unannotated
/// `new` expression has no effect — each complete.
///
/// Measured on TypeScript 7.0.2 (`strictNullChecks` on / off):
/// `ReturnType<typeof n10>`, `n21` and `n14` are `string | undefined` /
/// `string`, `n11` is `string`.
#[test]
fn a_statement_call_takes_the_effect_of_its_resolved_overload() {
    assert_rows(&[
        ("n10", ["string | undefined", "string"]),
        ("n21", ["string | undefined", "string"]),
        ("n11", ["string", "string"]),
        ("n14", ["string | undefined", "string"]),
    ]);
}

/// A qualified callee's effects signature is the signature the call
/// resolves when some signature returns `never` and the set is overloaded
/// or generic (`getEffectsSignature` over `getResolvedSignature`).
///
/// Measured on TypeScript 7.0.2 (`strictNullChecks` on / off): over
/// `declare const O: { fover(x: string): never; fover(x: number): void;
/// gfail<T>(x: T): never }`, `ReturnType<typeof q1>` (`O.fover(1)`) is
/// `string | undefined` / `string`, `q2` (`O.fover("a")`) and `q3`
/// (`O.gfail(1)`) `string`.
#[test]
fn a_qualified_statement_call_takes_the_effect_of_its_resolved_signature() {
    assert_rows(&[
        ("q1", ["string | undefined", "string"]),
        ("q2", ["string", "string"]),
        ("q3", ["string", "string"]),
    ]);
}
