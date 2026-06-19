//! Architecture guard — the upsert engine's canonical-uniqueness
//! enforcement is RELEASE-ACTIVE.
//!
//! `HostState::assert_canonicals_unique`
//! (`crates/verter_session/src/host_upsert.rs`) is the only thing standing
//! between a buggy caller and a torn atomic batch: a source-updating
//! `submit_batch_atomic` carrying a duplicated `canonical_id` would bump
//! that node's generation twice under one `dag.lock()` acquisition,
//! self-superseding the earlier admit and corrupting the batch
//! (`submit_batch_atomic` does NOT dedup). The check is computed BEFORE
//! the atomic submission and before any per-request side effect.
//!
//! Because it guards a CALLER-CONTRACT breach (a programming bug, not a
//! runtime input class), it MUST fire in release as well as debug — a
//! `debug_assert!` would be compiled out of a release build and let the
//! duplicate silently reach `submit_batch_atomic`.
//!
//! The runtime regression test
//! (`host_compile_atomic_upsert_tests.rs::upsert_duplicate_canonical_panics_before_submit_batch_atomic`)
//! proves the check EXISTS and fires BEFORE submission, but it can only do
//! so in the DEBUG test profile: it drives the scheduler's per-admit epoch
//! trace, whose hooks (`test_install_batch_admit_epoch_trace` /
//! `test_take_batch_admit_epochs`) are gated `#[cfg(any(test,
//! debug_assertions))]` in the scheduler crate, so a `--release` test run
//! does not even compile that test. It therefore CANNOT prove the check is
//! release-active.
//!
//! This static guard closes that gap. It extracts the
//! `assert_canonicals_unique` fn body from production source and asserts
//! its enforcement is a release-active macro (`assert!` / `assert_eq!` /
//! `assert_ne!` / `panic!`) and is NOT downgraded to a debug-only form
//! (`debug_assert!` / `debug_assert_eq!` / `debug_assert_ne!`). The guard
//! is consistent with this block's other static guards
//! (`no_legacy_compile_many_upsert_fanout`,
//! `scheduler_has_only_atomic_batch_api`): it scans only production source,
//! extracts the SPECIFIC enforcing region (not a whole-file `assert!`
//! grep, which would pass trivially), and ships a companion fixture
//! proving the analysis would FLAG a `debug_assert!` downgrade — so the
//! guard itself is discriminating, not a stub.

use std::path::PathBuf;

/// The production source file owning the uniqueness enforcement.
fn host_upsert_src() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("src")
        .join("host_upsert.rs")
}

/// The fn whose body must contain a release-active enforcement.
const ENFORCING_FN: &str = "fn assert_canonicals_unique";

/// Release-active enforcement macros: present in BOTH debug and release
/// builds. The body must contain at least one of these (as a real macro
/// invocation, NOT as the tail of a `debug_assert*!`).
const RELEASE_ACTIVE_MACROS: &[&str] = &["assert", "assert_eq", "assert_ne", "panic"];

/// Debug-only enforcement macros: compiled OUT of a release build. The
/// body must contain NONE of these — finding one means the enforcement was
/// downgraded and the duplicate would silently corrupt a release batch.
const DEBUG_ONLY_MACROS: &[&str] = &["debug_assert", "debug_assert_eq", "debug_assert_ne"];

/// Strip `//`/`///`/`//!` line comments and `/* */` block comments so a
/// macro name appearing only in documentation does not count as an
/// enforcement (the fn's own doc comment mentions `assert!`). Conservative;
/// sufficient for this fn's source shape (no `//` inside string literals on
/// the enforcing lines).
fn strip_comments(src: &str) -> String {
    // 1. Block comments.
    let mut no_block = String::with_capacity(src.len());
    let bytes = src.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if i + 1 < bytes.len() && bytes[i] == b'/' && bytes[i + 1] == b'*' {
            i += 2;
            while i + 1 < bytes.len() && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                i += 1;
            }
            i += 2;
            continue;
        }
        no_block.push(bytes[i] as char);
        i += 1;
    }
    // 2. Line/doc comments.
    let mut out = String::with_capacity(no_block.len());
    for line in no_block.lines() {
        let code = match line.find("//") {
            Some(idx) => &line[..idx],
            None => line,
        };
        out.push_str(code);
        out.push('\n');
    }
    out
}

/// Extract the brace-delimited body of the named fn from `src`, INCLUDING
/// its signature line up to the matching closing brace. Returns `None` if
/// the fn or its opening brace is not found. Comment-agnostic on input;
/// callers pass already-stripped source when they want comments ignored.
fn extract_fn_region(src: &str, fn_sig_needle: &str) -> Option<String> {
    let start = src.find(fn_sig_needle)?;
    let rest = &src[start..];
    // Find the first `{` after the signature (the body opener). Anything
    // before it (params, where-clause) carries no braces in this fn.
    let open_rel = rest.find('{')?;
    let mut depth: i32 = 0;
    let mut end_rel = None;
    for (off, ch) in rest[open_rel..].char_indices() {
        match ch {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    end_rel = Some(open_rel + off + ch.len_utf8());
                    break;
                }
            }
            _ => {}
        }
    }
    let end_rel = end_rel?;
    Some(rest[..end_rel].to_string())
}

/// Count macro invocations of `name!` in `region`, EXCLUDING those that are
/// actually the tail of a longer macro ident (so `assert` does not match
/// inside `debug_assert!`). A match requires `name` immediately followed by
/// `!` and immediately preceded by a non-ident char (start-of-string,
/// whitespace, or punctuation) — i.e. `name` is a whole identifier.
fn macro_invocation_count(region: &str, name: &str) -> usize {
    let needle = format!("{name}!");
    let bytes = region.as_bytes();
    let mut count = 0;
    let mut from = 0;
    while let Some(rel) = region[from..].find(&needle) {
        let at = from + rel;
        // The char immediately before `name` must NOT be an ident char,
        // else `name` is the tail of a longer ident (e.g. `debug_assert`
        // ends with `assert`).
        let preceded_by_ident = at
            .checked_sub(1)
            .map(|p| {
                let c = bytes[p];
                c == b'_' || c.is_ascii_alphanumeric()
            })
            .unwrap_or(false);
        if !preceded_by_ident {
            count += 1;
        }
        from = at + needle.len();
    }
    count
}

/// The verdict of analysing one enforcing-fn region.
#[derive(Debug, PartialEq, Eq)]
enum Verdict {
    /// At least one release-active macro and no debug-only macro: GOOD.
    ReleaseActive,
    /// A `debug_assert*!` is present: the enforcement is compiled out of
    /// release — the guard must FAIL.
    DebugOnlyDowngrade,
    /// Neither a release-active enforcement nor a debug-only one: the fn no
    /// longer enforces anything (e.g. body gutted) — the guard must FAIL.
    NoEnforcement,
}

/// Classify an enforcing-fn region (already comment-stripped). This is the
/// single discriminating analysis exercised by BOTH the real guard and the
/// companion fixtures.
fn classify_enforcement(region: &str) -> Verdict {
    let debug_only: usize = DEBUG_ONLY_MACROS
        .iter()
        .map(|m| macro_invocation_count(region, m))
        .sum();
    if debug_only > 0 {
        return Verdict::DebugOnlyDowngrade;
    }
    let release_active: usize = RELEASE_ACTIVE_MACROS
        .iter()
        .map(|m| macro_invocation_count(region, m))
        .sum();
    if release_active > 0 {
        Verdict::ReleaseActive
    } else {
        Verdict::NoEnforcement
    }
}

#[test]
fn uniqueness_check_is_release_active() {
    let path = host_upsert_src();
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("guard cannot read {}: {e}", path.display()));
    let stripped = strip_comments(&text);

    let region = extract_fn_region(&stripped, ENFORCING_FN).unwrap_or_else(|| {
        panic!(
            "guard could not locate `{ENFORCING_FN}` in {} — the \
             canonical-uniqueness enforcement fn was renamed or removed. \
             If it moved, update this guard to track its new home; the \
             enforcement must remain release-active.",
            path.display()
        )
    });

    match classify_enforcement(&region) {
        Verdict::ReleaseActive => {}
        Verdict::DebugOnlyDowngrade => panic!(
            "§6c release-active guard: `{ENFORCING_FN}` in {} uses a \
             debug-only `debug_assert*!`. A duplicated canonical guards a \
             CALLER-CONTRACT breach and MUST fire in release — a \
             `debug_assert!` is compiled out of a release build, letting \
             the duplicate reach `submit_batch_atomic` and self-supersede \
             its own admit under one DAG-lock, silently corrupting the \
             batch. Use a release-active `assert!`/`panic!`.\nfn region:\n{region}",
            path.display()
        ),
        Verdict::NoEnforcement => panic!(
            "§6c release-active guard: `{ENFORCING_FN}` in {} contains no \
             enforcement macro at all (`assert!`/`assert_eq!`/`assert_ne!`/\
             `panic!`). The canonical-uniqueness caller contract is \
             unenforced — a duplicated canonical would silently corrupt the \
             atomic batch.\nfn region:\n{region}",
            path.display()
        ),
    }
}

// ---------------------------------------------------------------------------
// Discriminating-property fixtures: the SAME analysis used by the guard
// above must FLAG a `debug_assert!` downgrade, FLAG a gutted body, and PASS
// the real release-active form. If these hold, the guard is discriminating
// — it would catch a regression that downgraded the production check.
// ---------------------------------------------------------------------------

/// (a) The real release-active form classifies as `ReleaseActive`.
#[test]
fn fixture_release_active_assert_passes() {
    let good = r#"
        fn assert_canonicals_unique(canonicals: &[String]) {
            let mut seen = std::collections::HashSet::with_capacity(canonicals.len());
            for canonical in canonicals {
                assert!(
                    seen.insert(canonical.as_str()),
                    "duplicate canonical_id `{canonical}`"
                );
            }
        }
    "#;
    let region = extract_fn_region(&strip_comments(good), ENFORCING_FN).expect("fn region");
    assert_eq!(
        classify_enforcement(&region),
        Verdict::ReleaseActive,
        "a release-active `assert!` enforcement must classify as ReleaseActive"
    );
}

/// (b) A `debug_assert!` DOWNGRADE classifies as `DebugOnlyDowngrade` —
/// this is the regression the guard exists to catch. Critically, the
/// downgraded body still TEXTUALLY contains `assert!` (as the tail of
/// `debug_assert!`), so a naive whole-file "contains assert!" grep would
/// PASS it; the structural analysis must FLAG it.
#[test]
fn fixture_debug_assert_downgrade_is_flagged() {
    let downgraded = r#"
        fn assert_canonicals_unique(canonicals: &[String]) {
            let mut seen = std::collections::HashSet::with_capacity(canonicals.len());
            for canonical in canonicals {
                debug_assert!(
                    seen.insert(canonical.as_str()),
                    "duplicate canonical_id `{canonical}`"
                );
            }
        }
    "#;
    let region = extract_fn_region(&strip_comments(downgraded), ENFORCING_FN).expect("fn region");

    // Sanity: the downgraded body DOES contain the substring `assert!`
    // (it is the tail of `debug_assert!`), so substring grepping is fooled.
    assert!(
        region.contains("assert!"),
        "precondition: the debug_assert! body textually contains `assert!`"
    );
    // The structural analysis is NOT fooled — it flags the downgrade.
    assert_eq!(
        classify_enforcement(&region),
        Verdict::DebugOnlyDowngrade,
        "a `debug_assert!` downgrade MUST be flagged as DebugOnlyDowngrade — \
         the guard would FAIL on this, catching the regression"
    );
    // And it does NOT mis-count the `assert` tail of `debug_assert!` as a
    // release-active `assert!` invocation.
    assert_eq!(
        macro_invocation_count(&region, "assert"),
        0,
        "`assert` must not match inside `debug_assert!` — whole-ident only"
    );
    assert_eq!(
        macro_invocation_count(&region, "debug_assert"),
        1,
        "the `debug_assert!` invocation must be counted exactly once"
    );
}

/// (c) `debug_assert_eq!` / `debug_assert_ne!` downgrades are also flagged.
#[test]
fn fixture_debug_assert_eq_downgrade_is_flagged() {
    let downgraded = r#"
        fn assert_canonicals_unique(canonicals: &[String]) {
            for c in canonicals {
                debug_assert_eq!(dedup(c), true, "dup `{c}`");
            }
        }
    "#;
    let region = extract_fn_region(&strip_comments(downgraded), ENFORCING_FN).expect("fn region");
    assert_eq!(classify_enforcement(&region), Verdict::DebugOnlyDowngrade);
}

/// (d) A gutted body (enforcement deleted entirely) classifies as
/// `NoEnforcement` — also a guard failure.
#[test]
fn fixture_gutted_body_is_flagged() {
    let gutted = r#"
        fn assert_canonicals_unique(canonicals: &[String]) {
            let _ = canonicals;
        }
    "#;
    let region = extract_fn_region(&strip_comments(gutted), ENFORCING_FN).expect("fn region");
    assert_eq!(
        classify_enforcement(&region),
        Verdict::NoEnforcement,
        "an enforcement-free body must classify as NoEnforcement"
    );
}

/// (e) A `panic!`-based enforcement (an equally release-active alternative
/// form) classifies as `ReleaseActive`, so the guard does not over-fit to
/// the literal `assert!` macro.
#[test]
fn fixture_panic_form_passes() {
    let panic_form = r#"
        fn assert_canonicals_unique(canonicals: &[String]) {
            let mut seen = std::collections::HashSet::new();
            for c in canonicals {
                if !seen.insert(c.as_str()) {
                    panic!("duplicate canonical_id `{c}`");
                }
            }
        }
    "#;
    let region = extract_fn_region(&strip_comments(panic_form), ENFORCING_FN).expect("fn region");
    assert_eq!(classify_enforcement(&region), Verdict::ReleaseActive);
}

/// (f) Comment-only mentions of `debug_assert!` do NOT trip the guard —
/// the analysis strips comments first, so documenting the rejected form in
/// prose is safe.
#[test]
fn fixture_comment_mention_of_debug_assert_is_ignored() {
    let documented = r#"
        fn assert_canonicals_unique(canonicals: &[String]) {
            // Historically a debug_assert! here was compiled out in release.
            /* debug_assert_eq! must never come back. */
            let mut seen = std::collections::HashSet::new();
            for c in canonicals {
                assert!(seen.insert(c.as_str()), "dup `{c}`");
            }
        }
    "#;
    let region = extract_fn_region(&strip_comments(documented), ENFORCING_FN).expect("fn region");
    assert_eq!(
        classify_enforcement(&region),
        Verdict::ReleaseActive,
        "comment-only mentions of debug_assert! must be stripped before analysis"
    );
}

/// (g) `extract_fn_region` brace-matches CORRECTLY — it returns the named
/// fn's body and stops at its matching close brace, not at the first inner
/// `}` and not bleeding into a following fn.
#[test]
fn fixture_extract_fn_region_brace_matches() {
    let two_fns = r#"
        fn other_before() { let _ = { 1 }; }
        fn assert_canonicals_unique(canonicals: &[String]) {
            for c in canonicals {
                if true { assert!(dedup(c), "dup"); }
            }
        }
        fn other_after() { debug_assert!(false); }
    "#;
    let region = extract_fn_region(&strip_comments(two_fns), ENFORCING_FN).expect("fn region");
    // The region must contain the enforcing fn's own `assert!`...
    assert!(region.contains("assert!(dedup(c)"));
    // ...balance its braces (open == close)...
    assert_eq!(
        region.matches('{').count(),
        region.matches('}').count(),
        "extracted region must be brace-balanced"
    );
    // ...and must NOT bleed into `other_after`'s `debug_assert!`, which
    // would otherwise produce a false DebugOnlyDowngrade verdict.
    assert!(
        !region.contains("other_after"),
        "extraction must stop at the enforcing fn's close brace"
    );
    assert_eq!(
        classify_enforcement(&region),
        Verdict::ReleaseActive,
        "the enforcing fn alone is release-active; the trailing fn's \
         debug_assert! must not leak into the analysed region"
    );
}
