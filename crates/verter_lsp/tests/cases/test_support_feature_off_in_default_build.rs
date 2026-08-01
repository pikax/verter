//! Architecture guard: the `test-support` Cargo feature is OFF in every
//! DEFAULT build — it is NOT in (and not transitively pulled in by) the
//! `[features] default` of `crates/verter_lsp/Cargo.toml`.
//!
//! `test-support` is the gate that keeps the delivered-ledger read
//! [`ProjectSync::delivered_provider_content`] (`#[cfg(any(test, feature =
//! "test-support"))]`) out of the DEFAULT compilation of the shipped lib
//! (plain `cargo build`, `pnpm run build:lsp`, release) while keeping it
//! reachable from this crate's own test targets via the `[dev-dependencies]`
//! self-edge (`verter_lsp = { path = ".", features = ["test-support"] }`).
//!
//! What this guard proves — and deliberately does NOT claim: the invariant is
//! DEFAULT-OFF, not unconditional unreachability. An explicit opt-in still
//! compiles the seam (`cargo build -p verter_lsp --all-features`, or a future
//! dependent declaring `features = ["test-support"]`); no guard can forbid an
//! explicit feature request. What CAN regress silently is the default: if a
//! contributor ever added `test-support` to `default` (directly OR via another
//! default-on feature that lists it), the seam would compile into every
//! production build with no opt-in anywhere. This guard is the executable
//! proof of that default-off invariant, cited by the `test-support` feature
//! comment in `crates/verter_lsp/Cargo.toml`. It mirrors `verter_session`'s
//! guard of the same shape
//! (`crates/verter_session/tests/cases/g_misc1/test_support_feature_off_in_default_build.rs`).
//!
//! The check is a structural parse of the `[features]` table (transitive
//! closure of `default`), NOT a substring scan — so a `default = ["foo"]`
//! where `foo = ["test-support"]` is caught too. The paired self-test feeds a
//! synthetic features table with `default = ["test-support"]` and proves the
//! closure-membership predicate FIRES, and the real (production) shape PASSES.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

fn lsp_cargo_toml() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml")
}

/// Parse a `[features]` table into a `name -> [dependent feature names]` map.
/// Only feature-name arms are followed for the closure (the `dep:`/`crate/feat`
/// activation forms are NOT feature names within THIS crate's `default` chain,
/// so they cannot re-introduce `test-support` as a same-crate feature — they
/// are recorded as opaque leaves and never expanded).
fn features_table(cargo_toml_src: &str) -> BTreeMap<String, Vec<String>> {
    let parsed: toml::Value =
        toml::from_str(cargo_toml_src).expect("verter_lsp/Cargo.toml must parse as TOML");
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
/// (`dep:foo`, `bar/baz`) is an opaque leaf and is not expanded. A crate with
/// NO `default` key (verter_lsp's production shape) yields the bare
/// `{"default"}` closure — nothing is default-on.
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
    let src = std::fs::read_to_string(lsp_cargo_toml()).expect("read crates/verter_lsp/Cargo.toml");
    let table = features_table(&src);

    // `test-support` MUST be a declared feature (anti-vacuity: if the feature
    // were renamed/removed, the gate it backs is gone — fail loudly rather
    // than pass on a missing key).
    assert!(
        table.contains_key("test-support"),
        "expected a `test-support` feature in crates/verter_lsp/Cargo.toml `[features]`; it is \
         the gate the delivered-ledger accessor `ProjectSync::delivered_provider_content` relies \
         on. If it was renamed, update this guard AND the `#[cfg(any(test, feature = \
         \"test-support\"))]` gates AND the Cargo.toml citation."
    );

    let closure = default_feature_closure(&table);
    assert!(
        !closure.contains("test-support"),
        "`test-support` is transitively enabled by `[features] default` in \
         crates/verter_lsp/Cargo.toml (default closure = {:?}) — this makes the delivered-ledger \
         accessor `ProjectSync::delivered_provider_content` COMPILE into every default build of \
         the shipped lib (plain `cargo build` / `pnpm run build:lsp` / release) with no opt-in \
         anywhere. `test-support` must be activated ONLY by an explicit opt-in — the \
         `[dev-dependencies]` self-edge for this crate's own tests — never by `default`.",
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

    // PASS: verter_lsp's actual production shape — NO `default` key at all,
    // `test-support` declared independently. Nothing is default-on.
    let good = r#"
[features]
hotpath = ["dep:hotpath"]
test-support = []
"#;
    let table = features_table(good);
    let closure = default_feature_closure(&table);
    assert!(
        table.contains_key("test-support"),
        "self-test: the good shape MUST still declare `test-support`"
    );
    assert!(
        !closure.contains("test-support"),
        "self-test: with no `default` key and an independent `test-support = []`, `test-support` \
         MUST NOT be in the default closure (verter_lsp's production shape passes)"
    );

    // PASS: an unrelated default feature does not drag in test-support.
    let good2 = r#"
[features]
default = ["hotpath"]
hotpath = ["dep:hotpath"]
test-support = []
"#;
    assert!(
        !default_feature_closure(&features_table(good2)).contains("test-support"),
        "self-test: a `default = [\"hotpath\"]` (unrelated) MUST NOT pull in test-support"
    );
}
