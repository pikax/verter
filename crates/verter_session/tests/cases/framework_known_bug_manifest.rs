//! The framework known-bug ledger + its bijection guard.
//!
//! Per the program's test policy, a failure rooted in a pre-existing
//! semantic / typeinfo bug (NOT introduced by a framework vertical) is
//! recorded here as a known-bug ENTRY and characterized by an
//! `#[ignore = "..."]`d regression test naming the same id. The bijection
//! guard pins a BIDIRECTIONAL 1:1 correspondence between ledger entries and the
//! `#[ignore]`d regression tests that characterize them:
//! - FORWARD: every ledger entry has a matching `#[ignore = "<id>: ..."]` test
//!   (no orphan ledger entry — a documented bug with no characterizing test);
//! - REVERSE: every FRAMEWORK-SHAPED `#[ignore = "<framework>-...: ..."]` marker
//!   has a ledger entry (no orphan ignored framework-bug test — a parked test
//!   with no ledger row). The reverse scan is ledger-independent (it matches the
//!   `<framework>-` id prefix), so it catches a gap the forward direction alone
//!   cannot.
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

/// The framework name prefixes a known-bug id begins with (`<framework>-...`).
/// A `#[ignore = "<framework>-...: ..."]` marker carrying one of these prefixes
/// is a FRAMEWORK-bug tracker — the REVERSE-direction discriminator the
/// bidirectional bijection uses to catch an orphan ignored test with no ledger
/// entry. Adding a framework vertical adds its prefix here.
const FRAMEWORK_BUG_ID_PREFIXES: &[&str] = &["vue-", "svelte-", "react-", "solid-"];

/// Whether an ignore id is FRAMEWORK-SHAPED (begins with a known framework
/// prefix). The reverse-direction filter — an unrelated `#[ignore = "flaky"]`
/// is not framework-shaped and is excluded.
fn is_framework_shaped_bug_id(id: &str) -> bool {
    FRAMEWORK_BUG_ID_PREFIXES
        .iter()
        .any(|prefix| id.starts_with(prefix))
}

/// Extract EVERY framework-shaped `#[ignore = "<id>: ..."]` id from `text` (the
/// reverse-direction scan — independent of the ledger). The `<id>: <text>` shape
/// is required (a bare `#[ignore = "flaky"]` has no `:` and is not a tracker).
fn extract_framework_shaped_ignore_ids(text: &str) -> Vec<String> {
    let mut found = Vec::new();
    let needle = "#[ignore = \"";
    let mut cursor = 0;
    while let Some(rel) = text[cursor..].find(needle) {
        let start = cursor + rel + needle.len();
        if let Some(end_rel) = text[start..].find('"') {
            let reason = &text[start..start + end_rel];
            if let Some((id, _rest)) = reason.split_once(':') {
                let id = id.trim();
                if is_framework_shaped_bug_id(id) {
                    found.push(id.to_string());
                }
            }
            cursor = start + end_rel + 1;
        } else {
            break;
        }
    }
    found
}

/// Every framework-shaped ignored-bug id across the crate's `src` + `tests`
/// trees — the reverse-direction set the bijection checks against the ledger.
fn all_framework_shaped_ignore_ids() -> Vec<String> {
    let root = workspace_root();
    let mut sources = read_rs_recursive(&root.join("crates/verter_session/src"));
    sources.extend(read_rs_recursive(&root.join("crates/verter_session/tests")));
    extract_framework_shaped_ignore_ids(&sources.join("\n"))
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

    // FORWARD direction: every ledger entry is characterized by a matching
    // `#[ignore = "<id>: ..."]` test.
    let orphan_ledger: Vec<&String> = ledger.difference(&referenced).collect();
    assert!(
        orphan_ledger.is_empty(),
        "known-bug ledger entries with NO characterizing #[ignore] test: {orphan_ledger:?}. \
         Add the `#[ignore = \"<id>: ...\"]`d regression test, or remove the ledger entry."
    );

    // REVERSE direction (bidirectional bijection): every FRAMEWORK-SHAPED
    // `#[ignore = "<framework>-...: ..."]` marker MUST have a ledger entry. This
    // catches an ORPHAN ignored framework-bug test parked with no ledger row —
    // a gap the forward direction alone (which filters `referenced` to
    // `known_ids`) cannot detect. The framework-shape scan is ledger-independent.
    let framework_shaped: std::collections::BTreeSet<String> =
        all_framework_shaped_ignore_ids().into_iter().collect();
    let orphan_ignored: Vec<&String> = framework_shaped.difference(&ledger).collect();
    assert!(
        orphan_ignored.is_empty(),
        "framework-shaped #[ignore] markers with NO ledger entry: {orphan_ignored:?}. \
         Add the matching `FRAMEWORK_KNOWN_BUGS` ledger entry, or fix the bug so the test \
         un-ignores. (OUT-OF-SCOPE matrix rows are NOT known-bug rows — they must NOT be \
         parked as `<framework>-...`-prefixed ignored tests.)"
    );

    // EMPTY ledger ⇒ trivially green (no framework-shaped ignore markers exist).
    if FRAMEWORK_KNOWN_BUGS.is_empty() {
        assert!(
            referenced.is_empty(),
            "the ledger is empty, so no framework-bug #[ignore] marker should be referenced"
        );
        assert!(
            framework_shaped.is_empty(),
            "the ledger is empty, so no framework-shaped #[ignore] marker may exist: \
             {framework_shaped:?}"
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
