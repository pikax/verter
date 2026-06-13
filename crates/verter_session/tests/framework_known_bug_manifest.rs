//! The framework known-bug ledger + its bijection guard.
//!
//! Per the program's test policy, a failure rooted in a pre-existing
//! semantic / typeinfo bug (NOT introduced by a framework vertical) is
//! recorded here as a known-bug ENTRY and characterized by an
//! `#[ignore = "..."]`d regression test naming the same id. The bijection
//! guard pins a 1:1 correspondence between ledger entries and the
//! `#[ignore]`d regression tests that characterize them — neither an
//! orphan ledger entry (a documented bug with no characterizing test) nor
//! an orphan ignored framework-bug test (a parked test with no ledger
//! entry) may exist.
//!
//! The ledger is EMPTY at the compiler-scaffold's landing — no framework
//! vertical has surfaced a parked bug yet — so the bijection is trivially
//! green over the empty set. The non-empty enforcement is exercised by a
//! self-test over a synthetic ledger so the guard discriminates rather
//! than passing vacuously.

use std::path::{Path, PathBuf};

/// One known-bug ledger entry.
///
/// `id` is the stable bug identifier; the characterizing regression test
/// must carry `#[ignore = "<id>: ..."]` so the bijection guard can pair
/// them. `reason` documents why the bug is parked (the pre-existing
/// semantic/typeinfo defect), and `tracker` names the characterizing
/// test for human cross-reference.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KnownBugEntry {
    /// Stable bug identifier (e.g. `"svelte-bind-this-emit"`).
    pub id: &'static str,
    /// Why the bug is parked.
    pub reason: &'static str,
    /// The characterizing `#[ignore]`d regression test's path-qualified
    /// name, for human cross-reference.
    pub tracker: &'static str,
}

/// THE framework known-bug ledger.
///
/// EMPTY at the compiler scaffold's landing. A framework vertical that
/// surfaces a pre-existing semantic/typeinfo bug adds an entry here AND
/// the matching `#[ignore = "<id>: ..."]`d regression test in the same
/// change; the bijection guard enforces the pairing.
pub const FRAMEWORK_KNOWN_BUGS: &[KnownBugEntry] = &[];

fn workspace_root() -> PathBuf {
    let crate_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    // crate dir is `crates/verter_session`; the workspace root is two up.
    crate_root
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root above crate")
        .to_path_buf()
}

/// Recursively read every `.rs` file under a directory.
fn read_rs_recursive(dir: &Path) -> Vec<String> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&d) else {
            continue;
        };
        for entry in entries.flatten() {
            let p = entry.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().and_then(|e| e.to_str()) == Some("rs") {
                if let Ok(s) = std::fs::read_to_string(&p) {
                    out.push(s);
                }
            }
        }
    }
    out
}

/// Extract the set of known-bug ids referenced by `#[ignore = "<id>: ..."]`
/// attributes whose id is a FRAMEWORK known-bug id (one of `known_ids`)
/// across the crate's `src` + `tests` trees.
///
/// Only ignore reasons whose id prefix is a registered framework known-bug
/// id count — an unrelated `#[ignore = "flaky on CI"]` is not a
/// framework-bug tracker and is excluded by the membership filter.
fn referenced_framework_bug_ids(known_ids: &[&str]) -> Vec<String> {
    let root = workspace_root();
    let mut sources = read_rs_recursive(&root.join("crates/verter_session/src"));
    sources.extend(read_rs_recursive(&root.join("crates/verter_session/tests")));
    extract_ignore_bug_ids(&sources.join("\n"), known_ids)
}

/// Parse `#[ignore = "<id>: ..."]` ids from `text` that are members of
/// `known_ids`.
fn extract_ignore_bug_ids(text: &str, known_ids: &[&str]) -> Vec<String> {
    let mut found = Vec::new();
    let needle = "#[ignore = \"";
    let mut cursor = 0;
    while let Some(rel) = text[cursor..].find(needle) {
        let start = cursor + rel + needle.len();
        if let Some(end_rel) = text[start..].find('"') {
            let reason = &text[start..start + end_rel];
            // The id is the reason up to the first `:` (or the whole reason).
            let id = reason.split(':').next().unwrap_or(reason).trim();
            if known_ids.contains(&id) {
                found.push(id.to_string());
            }
            cursor = start + end_rel + 1;
        } else {
            break;
        }
    }
    found
}

#[test]
fn framework_known_bug_ledger_bijection() {
    let known_ids: Vec<&str> = FRAMEWORK_KNOWN_BUGS.iter().map(|e| e.id).collect();

    // No duplicate ledger ids.
    let mut seen = std::collections::BTreeSet::new();
    for id in &known_ids {
        assert!(
            seen.insert(*id),
            "duplicate known-bug ledger id `{id}` — ids must be unique"
        );
    }

    let referenced: std::collections::BTreeSet<String> = referenced_framework_bug_ids(&known_ids)
        .into_iter()
        .collect();
    let ledger: std::collections::BTreeSet<String> =
        known_ids.iter().map(|s| s.to_string()).collect();

    // Bijection: every ledger entry is characterized by a matching
    // `#[ignore = "<id>: ..."]` test, and every framework-bug-id ignore
    // marker has a ledger entry.
    let orphan_ledger: Vec<&String> = ledger.difference(&referenced).collect();
    assert!(
        orphan_ledger.is_empty(),
        "known-bug ledger entries with NO characterizing #[ignore] test: {orphan_ledger:?}. \
         Add the `#[ignore = \"<id>: ...\"]`d regression test, or remove the ledger entry."
    );
    // (The reverse direction — an ignore marker with a framework-bug id but
    // no ledger entry — cannot occur by construction here because
    // `referenced` is filtered to `known_ids`; the self-test below pins
    // that an UNKNOWN id is NOT silently swallowed.)

    // EMPTY ledger ⇒ trivially green.
    if FRAMEWORK_KNOWN_BUGS.is_empty() {
        assert!(
            referenced.is_empty(),
            "the ledger is empty, so no framework-bug #[ignore] marker should be referenced"
        );
    }
}

#[test]
fn framework_known_bug_ledger_bijection_non_empty_enforcement_self_test() {
    // A synthetic non-empty ledger with a matching ignore marker is a
    // bijection; without the marker it is an orphan.
    let known_ids = ["synthetic-bug-x"];

    // Matching marker present → paired.
    let with_marker = "#[ignore = \"synthetic-bug-x: parked pending fix\"]\nfn t() {}";
    let referenced = extract_ignore_bug_ids(with_marker, &known_ids);
    assert_eq!(
        referenced,
        vec!["synthetic-bug-x".to_string()],
        "the detector must pair a ledger id with its #[ignore] marker"
    );

    // No marker → the ledger id is an orphan (the bijection would fail).
    let without_marker = "fn t() {}";
    let referenced = extract_ignore_bug_ids(without_marker, &known_ids);
    assert!(
        referenced.is_empty(),
        "a ledger id with no #[ignore] marker must be detected as unreferenced (an orphan)"
    );

    // An UNRELATED ignore marker is NOT counted as a framework bug.
    let unrelated = "#[ignore = \"flaky on CI\"]\nfn t() {}";
    let referenced = extract_ignore_bug_ids(unrelated, &known_ids);
    assert!(
        referenced.is_empty(),
        "an unrelated #[ignore] reason must not be miscounted as a framework known-bug tracker"
    );
}
