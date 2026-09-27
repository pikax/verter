//! `keyof` over a named declaration or application whose keys settle. The
//! checker's type IS the union of the keys — `keyof Box<string>` is
//! `"value" | "size"` — and only its print keeps the `keyof` origin, so an
//! assignment narrows a binding declared with it and `Exclude` filters it.
//!
//! Every expected answer is TypeScript 7.0.2's, read off
//! `tsc --declaration --emitDeclarationOnly --ignoreConfig` under each
//! `strictNullChecks` × `noImplicitAny` setting; the four settings agree.

use super::checker_probe_lane_tests::{degradation_in, mismatches, ProbeProject};

const FIXTURE: &str = "\
interface Box<T> { value: T; size: number }
type Pair<T> = { first: T; second: T };
interface Plain { a: 1; b: 2 }
export function kAssign() { const k: keyof Box<string> = \"value\"; return k; }
export function kLet() { let k: keyof Box<string> = \"value\"; return k; }
export function kPair() { let k: keyof Pair<number> = \"first\"; return k; }
export function kPlain() { let k: keyof Plain = \"a\"; return k; }
export function kReassign() { let k: keyof Box<string> = \"value\"; k = \"size\"; return k; }
export function kGuard(k: keyof Box<string>) { if (k === \"value\") { return k; } return k; }
export function kSwitch(k: keyof Box<string>) { switch (k) { case \"size\": return 1; default: return k; } }
export function kExclude(k: Exclude<keyof Box<string>, \"size\">) { return k; }
export function kExtract(k: Extract<keyof Pair<number>, \"second\" | \"third\">) { return k; }
export function kCond() { const x: keyof Box<string> extends \"value\" | \"size\" ? 1 : 2 = null as any; return x; }
export function kIndex() { const x: Box<string>[keyof Box<string>] = null as any; return x; }
";

/// A binding declared as a settled `keyof` narrows by assignment and by
/// guards over its keys, and `Exclude` / `Extract` filter those keys.
///
/// Measured on TypeScript 7.0.2 (`ReturnType<typeof f>`):
///
/// | function | checker |
/// | --- | --- |
/// | `kAssign`, `kLet`, `kExclude` | `"value"` |
/// | `kPair` | `"first"` |
/// | `kPlain` | `"a"` |
/// | `kReassign` | `"size"` |
/// | `kGuard` | `"size" \| "value"` |
/// | `kSwitch` | `"value" \| 1` |
/// | `kExtract` | `"second"` |
/// | `kCond` | `1` |
/// | `kIndex` | `string \| number` |
#[test]
fn a_settled_keyof_is_the_union_of_its_keys() {
    let rows = [
        ("kAssign", "\"value\""),
        ("kLet", "\"value\""),
        ("kPair", "\"first\""),
        ("kPlain", "\"a\""),
        ("kReassign", "\"size\""),
        ("kGuard", "\"size\" | \"value\""),
        ("kSwitch", "\"value\" | 1"),
        ("kExclude", "\"value\""),
        ("kExtract", "\"second\""),
        ("kCond", "1"),
        ("kIndex", "string | number"),
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

const WHOLE: &str = "\
interface Box<T> { value: T; size: number }
export function kWhole() { const k: keyof Box<string> = null as any; return k; }
export function kParam(k: keyof Box<string>) { let j: keyof Box<string> = k; return j; }
";

/// An assignment that keeps every key is the declared type itself, printed
/// by its `keyof` origin as the checker prints it.
///
/// Measured on TypeScript 7.0.2, every setting: `ReturnType<typeof
/// kWhole>` and `ReturnType<typeof kParam>` are `keyof Box<string>`.
#[test]
fn an_assignment_keeping_every_key_keeps_the_keyof_print() {
    let failures = mismatches(
        WHOLE,
        &[
            ("ReturnType<typeof kWhole>", "keyof Box<string>"),
            ("ReturnType<typeof kParam>", "keyof Box<string>"),
        ],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
