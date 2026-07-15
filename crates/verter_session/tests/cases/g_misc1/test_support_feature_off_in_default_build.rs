//! Architecture guard: the `test-support` Cargo feature is PRODUCTION-
//! UNREACHABLE — it is NOT in (and not transitively pulled in by) the
//! `[features] default` of `crates/verter_session/Cargo.toml`.
//!
//! `test-support` is the load-bearing gate that keeps the OutputProjector
//! carrier `_for_test` accessors (the capability-free reverse-materialization
//! unwrap) COMPILE-ABSENT from every production build (plain `cargo build`,
//! `pnpm run build:lsp`, release) while keeping them reachable from the
//! integration-test binary via the `[dev-dependencies]` self-edge
//! (`verter_session = { path = ".", features = ["test-support"] }`). If a
//! contributor ever added `test-support` to `default` (directly OR via another
//! default-on feature that lists it), those `#[cfg(any(test, feature =
//! "test-support"))]` accessors would compile into the shipped lib — a
//! reverse-materialization carrier-unwrap hole in production. This guard is the
//! executable proof of the debug-build-unreachability invariant cited at
//! `crates/verter_session/Cargo.toml` (the `test-support` feature comment).
//!
//! The check is a structural parse of the `[features]` table (transitive
//! closure of `default`), NOT a substring scan — so a `default = ["foo"]`
//! where `foo = ["test-support"]` is caught too. The paired self-test feeds a
//! synthetic features table with `default = ["test-support"]` and proves the
//! closure-membership predicate FIRES, and the real (production) shape PASSES.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

fn session_cargo_toml() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml")
}

/// Parse a `[features]` table into a `name -> [dependent feature names]` map.
/// Only feature-name arms are followed for the closure (the `dep:`/`crate/feat`
/// activation forms are NOT feature names within THIS crate's `default` chain,
/// so they cannot re-introduce `test-support` as a same-crate feature — they
/// are recorded as opaque leaves and never expanded).
fn features_table(cargo_toml_src: &str) -> BTreeMap<String, Vec<String>> {
    let parsed: toml::Value =
        toml::from_str(cargo_toml_src).expect("verter_session/Cargo.toml must parse as TOML");
    let Some(features) = parsed.get("features").and_then(|f| f.as_table()) else {
        // No `[features]` table at all ⇒ `test-support` cannot be default-on.
        return BTreeMap::new();
    };
    let mut out = BTreeMap::new();
    for (name, val) in features {
        let deps: Vec<String> = val
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default();
        out.insert(name.clone(), deps);
    }
    out
}

/// The transitive closure of features enabled when `default` is on — i.e.
/// `default` itself plus every feature reachable by following SAME-CRATE
/// feature-name edges. An activation token that is not a key in the table
/// (`dep:foo`, `bar/baz`) is an opaque leaf and is not expanded.
fn default_feature_closure(table: &BTreeMap<String, Vec<String>>) -> BTreeSet<String> {
    let mut closure: BTreeSet<String> = BTreeSet::new();
    let mut stack: Vec<String> = vec!["default".to_string()];
    while let Some(feat) = stack.pop() {
        if !closure.insert(feat.clone()) {
            continue; // already visited
        }
        if let Some(deps) = table.get(&feat) {
            for d in deps {
                if !closure.contains(d) {
                    stack.push(d.clone());
                }
            }
        }
    }
    closure
}

#[test]
fn test_support_feature_is_not_in_default_build() {
    let src = std::fs::read_to_string(session_cargo_toml())
        .expect("read crates/verter_session/Cargo.toml");
    let table = features_table(&src);

    // `test-support` MUST be a declared feature (anti-vacuity: if the feature
    // were renamed/removed, the gate it backs is gone — fail loudly rather
    // than pass on a missing key).
    assert!(
        table.contains_key("test-support"),
        "expected a `test-support` feature in crates/verter_session/Cargo.toml `[features]`; it is \
         the gate the OutputProjector carrier `_for_test` accessors rely on. If it was renamed, \
         update this guard AND the `_for_test` gates AND the Cargo.toml citation."
    );

    let closure = default_feature_closure(&table);
    assert!(
        !closure.contains("test-support"),
        "`test-support` is transitively enabled by `[features] default` in \
         crates/verter_session/Cargo.toml (default closure = {:?}) — this makes the OutputProjector \
         carrier `_for_test` reverse-materialization accessors COMPILE into the shipped production \
         lib (plain `cargo build` / `pnpm run build:lsp` / release), re-opening the carrier-unwrap \
         hole. `test-support` must be activated ONLY by the `[dev-dependencies]` self-edge, never \
         by `default`.",
        closure
    );
}

#[test]
fn test_support_default_guard_self_test_discriminates() {
    // FIRE (RED): a synthetic `[features]` table where `default` pulls in
    // `test-support` (directly) — the closure-membership check MUST flag it.
    let direct = r#"
[features]
default = ["test-support"]
test-support = []
"#;
    let closure = default_feature_closure(&features_table(direct));
    assert!(
        closure.contains("test-support"),
        "self-test: `default = [\"test-support\"]` MUST put `test-support` in the default closure \
         (the regression this guard exists to catch)"
    );

    // FIRE (RED): a TRANSITIVE pull-in — `default` enables `extras`, and
    // `extras` lists `test-support`. A substring scan of `default = [...]`
    // alone would MISS this; the closure walk catches it.
    let transitive = r#"
[features]
default = ["extras"]
extras = ["test-support"]
test-support = []
"#;
    let closure = default_feature_closure(&features_table(transitive));
    assert!(
        closure.contains("test-support"),
        "self-test: a TRANSITIVE `default -> extras -> test-support` chain MUST be caught by the \
         closure walk"
    );

    // PASS: the production shape — `default = []`, `test-support = []`
    // independent. `test-support` is NOT in the default closure.
    let good = r#"
[features]
default = []
session_metrics = []
test-support = []
oracle-gen = ["dep:verter_type_runtime"]
"#;
    let table = features_table(good);
    let closure = default_feature_closure(&table);
    assert!(
        table.contains_key("test-support"),
        "self-test: the good shape MUST still declare `test-support`"
    );
    assert!(
        !closure.contains("test-support"),
        "self-test: with `default = []` and an independent `test-support = []`, `test-support` MUST \
         NOT be in the default closure (the production shape passes)"
    );

    // PASS: an unrelated default feature does not drag in test-support.
    let good2 = r#"
[features]
default = ["session_metrics"]
session_metrics = []
test-support = []
"#;
    assert!(
        !default_feature_closure(&features_table(good2)).contains("test-support"),
        "self-test: a `default = [\"session_metrics\"]` (unrelated) MUST NOT pull in test-support"
    );
}
