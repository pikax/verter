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

/// An interface heritage diamond `levels` deep: `D0 { root: 0 }`, then for
/// each level `i` an `Ai` and a `Bi` that both extend `D(i-1)` and add one
/// key, joined again by `Di extends Ai, Bi {}`. Every `Di` reaches `D0`
/// along `2^i` heritage paths. `diamond(10)` is the module the semantic
/// benchmark's `scale-diamond-keyof-10` scenario checks.
fn diamond(levels: usize) -> String {
    let mut source = String::from("type __BenchInit = 0;\ninterface D0 { root: 0 }\n");
    for level in 1..=levels {
        let base = level - 1;
        source.push_str(&format!(
            "interface A{level} extends D{base} {{ a{level}: {level} }}\n\
             interface B{level} extends D{base} {{ b{level}: {level} }}\n\
             interface D{level} extends A{level}, B{level} {{}}\n"
        ));
    }
    source
}

/// A narrowing over `keyof D<level>` that leaves every inherited key but
/// `root`.
fn diamond_key_guard(level: usize) -> String {
    format!(
        "export function k{level}(k: keyof D{level}) {{ if (k === \"root\") {{ return 1; }} return k; }}\n"
    )
}

/// `keyof` over a heritage diamond has every inherited key at any depth,
/// each shared base inherited once however many paths reach it.
///
/// Measured on TypeScript 7.0.2 (`ReturnType<typeof f>`), every setting:
///
/// | function | checker |
/// | --- | --- |
/// | `k9` | `"a1" \| … \| "a9" \| "b1" \| … \| "b9" \| 1` |
/// | `k10` | `"a1" \| … \| "a10" \| "b1" \| … \| "b10" \| 1` |
/// | `k16` | `"a1" \| … \| "a16" \| "b1" \| … \| "b16" \| 1` |
#[test]
fn keyof_over_a_heritage_diamond_has_every_inherited_key() {
    let mut source = diamond(16);
    let levels = [9, 10, 16];
    for level in levels {
        source.push_str(&diamond_key_guard(level));
    }
    let probes: Vec<(String, String)> = levels
        .iter()
        .map(|level| {
            let keys: Vec<String> = (1..=*level)
                .flat_map(|i| [format!("\"a{i}\""), format!("\"b{i}\"")])
                .collect();
            (
                format!("ReturnType<typeof k{level}>"),
                format!("{} | 1", keys.join(" | ")),
            )
        })
        .collect();
    let rows: Vec<(&str, &str)> = probes
        .iter()
        .map(|(probe, checker)| (probe.as_str(), checker.as_str()))
        .collect();
    let mut failures = mismatches(&source, &rows);
    for level in levels {
        let name = format!("k{level}");
        match degradation_in(ProbeProject::default(), &source, &name) {
            Ok(None) => {}
            other => failures.push(format!("`{name}` is not complete: {other:?}")),
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// A walk with a node cap of a few nodes per level settles a 32-level
/// diamond's keys: each shared base is walked once however many heritage
/// paths reach it, so the nodes it visits grow with the levels (about 11
/// a level), where a walk per path would visit `2^32`.
#[test]
fn a_heritage_diamond_walks_each_shared_base_once() {
    use super::evaluate::StructuralFactDemandOutcome;
    let levels = 32;
    let mut config = crate::HostConfig {
        analysis_level: crate::types::AnalysisLevel::Full,
        audit_enabled: true,
        ..crate::HostConfig::default()
    };
    config.recursion_budget_overrides.walker_pathological_cap = Some(16 * levels);
    let host = std::sync::Arc::new(crate::VerterHost::new_standalone(config));
    let outcome = super::checker_probe_lane_tests::with_probe_outcome_on_host(
        &host,
        ProbeProject::default(),
        &diamond(levels),
        &format!("Exclude<keyof D{levels}, \"root\">"),
        |_, outcome| outcome,
    );
    assert!(
        matches!(outcome, StructuralFactDemandOutcome::Complete(_)),
        "the diamond's keys settle within a cap linear in its levels, got {outcome:?}"
    );
}

/// A shallow walk stopped by its node cap leaves the diamond's keys
/// incomplete: reading them is a partial that stays partial on a second
/// read, never the empty key set `never`.
#[test]
fn a_stopped_heritage_walk_is_partial_never_an_empty_key_set() {
    use super::evaluate::StructuralFactDemandOutcome;
    let mut config = crate::HostConfig {
        analysis_level: crate::types::AnalysisLevel::Full,
        audit_enabled: true,
        ..crate::HostConfig::default()
    };
    // `D3`'s heritage walk reaches more distinct nodes than this.
    config.recursion_budget_overrides.walker_pathological_cap = Some(8);
    let host = std::sync::Arc::new(crate::VerterHost::new_standalone(config));
    let source = diamond(3);
    for read in ["first", "second"] {
        let outcome = super::checker_probe_lane_tests::with_probe_outcome_on_host(
            &host,
            ProbeProject::default(),
            &source,
            "Exclude<keyof D3, \"root\">",
            |_, outcome| outcome,
        );
        assert!(
            matches!(outcome, StructuralFactDemandOutcome::Partial(_)),
            "the {read} read of the stopped keys must be partial, got {outcome:?}"
        );
    }
}
