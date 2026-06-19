//! Architecture guard: ANTI-BINARY-GROWTH integration-test layout.
//!
//! Verter's test suite is consolidated so each crate compiles AT MOST ONE
//! `tests/main.rs` integration-test binary (extra cases live under
//! `tests/cases/` and are wired through `main.rs`). A second top-level
//! `tests/*.rs` auto-becomes its own test binary at compile time and
//! re-balloons the gate — exactly the regression this guard mechanically
//! prevents.
//!
//! This is the in-gate Rust DURABILITY mirror of the fast-fail CI Node check
//! `scripts/check-integration-test-layout.mjs`. BOTH read the SAME committed
//! allowlist (`scripts/integration-test-layout-allowlist.json`), so the
//! exception set is a single source of truth and cannot drift between them.
//!
//! The guard drives `cargo metadata --format-version 1 --no-deps` and, for
//! EVERY workspace package, FAILS unless the package has:
//!   * 0 integration-test targets, OR
//!   * exactly 1 integration-test target whose `src_path` normalizes to
//!     `<pkg>/tests/main.rs`, PLUS any EXACTLY-allowlisted targets.
//!
//! PLUS the structural checks (so the bare "exactly 1 main.rs" rule cannot be
//! evaded):
//!   1. `<pkg>/tests/main.rs` on disk ⇒ metadata MUST report that target.
//!   2. immediate `<pkg>/tests/*.rs` present AND zero metadata test targets ⇒
//!      FAIL (catches `autotests = false` hiding tests).
//!   3. an explicit `[[test]]` whose src is not `tests/main.rs` ⇒ FAIL unless
//!      EXACTLY allowlisted.
//!   4. a stray IMMEDIATE `<pkg>/tests/*.rs` other than `main.rs` (and other
//!      than an allowlisted src file) ⇒ FAIL. Files UNDER `tests/cases/` (or
//!      any subdir) are fine — only the immediate `tests/*.rs` level is
//!      constrained.
//!   5. MORE THAN ONE metadata test target whose src is `<pkg>/tests/main.rs`
//!      ⇒ FAIL (two `[[test]]` blocks both pointing at `tests/main.rs` still
//!      compile two binaries).
//!   6. every cargo-AUTO-DISCOVERABLE position (immediate `tests/*.rs` AND
//!      `tests/<dir>/main.rs` one subdir deep) without a matching metadata
//!      target ⇒ FAIL (catches a hidden nested `tests/rogue/main.rs` under
//!      `autotests = false`, even when another valid target exists).
//!
//! The allowlist is EXACT (package + target + repo-relative forward-slash
//! `src_path`), with NO globs / prefixes / package-wide switches, and is
//! STALE-FAILING: an allowlisted `(package, target)` that no longer exists in
//! `cargo metadata` (or whose `src_path` moved) makes the guard FAIL, so a
//! removed binary cannot leave a dead exception.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Repo root = parent of `crates/` (this crate's manifest dir is
/// `crates/verter_session`).
fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates/")
        .parent()
        .expect("repo root")
        .to_path_buf()
}

/// Normalize OS path separators to forward slashes (no rebasing).
fn to_posix(p: &Path) -> String {
    p.to_string_lossy().replace('\\', "/")
}

/// Repo-relative, forward-slash path for `abs`.
fn repo_rel_posix(abs: &Path) -> String {
    let root = workspace_root();
    match abs.strip_prefix(&root) {
        Ok(rel) => to_posix(rel),
        Err(_) => to_posix(abs),
    }
}

/// One exact allowlist exception. Mirrors the JSON object shape in
/// `scripts/integration-test-layout-allowlist.json` (the single source of
/// truth, shared with the Node check).
#[derive(Debug, Clone, PartialEq, Eq)]
struct AllowEntry {
    package: String,
    target: String,
    /// Repo-relative, forward-slash normalized.
    src_path: String,
}

/// Load + validate the central allowlist. PANICS (fails the guard) if the
/// file is missing / malformed — a broken allowlist must not silently
/// degrade into "no exceptions".
fn load_allowlist() -> Vec<AllowEntry> {
    let path = workspace_root().join("scripts/integration-test-layout-allowlist.json");
    let raw = fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("read allowlist {}: {e}", repo_rel_posix(&path)));
    parse_allowlist(&raw)
}

/// Pure allowlist parser/validator (no I/O) so the duplicate-key rejection is
/// unit-testable. PANICS on a malformed or duplicate-keyed allowlist — a broken
/// allowlist must not silently degrade into "no exceptions", and a duplicate
/// `(package, target)` would let a STALE duplicate hide behind a correct one in
/// the matched-set bookkeeping below.
fn parse_allowlist(raw: &str) -> Vec<AllowEntry> {
    let json: serde_json::Value =
        serde_json::from_str(raw).unwrap_or_else(|e| panic!("allowlist is not valid JSON: {e}"));
    let arr = json
        .get("allow")
        .and_then(|v| v.as_array())
        .unwrap_or_else(|| panic!("allowlist missing `allow` array"));
    let mut out = Vec::new();
    // Reject duplicate keys at load: a duplicate `(package, target)` would let a
    // stale duplicate be masked by a correct one (the matched-set is keyed by
    // `(package, target)`); we also reject an exact `(package, target, src_path)`
    // triplet duplicate for full hygiene.
    let mut seen_pkg_target: BTreeSet<(String, String)> = BTreeSet::new();
    let mut seen_triplet: BTreeSet<(String, String, String)> = BTreeSet::new();
    for e in arr {
        let package = e.get("package").and_then(|v| v.as_str());
        let target = e.get("target").and_then(|v| v.as_str());
        let src_path = e.get("src_path").and_then(|v| v.as_str());
        let reason = e.get("reason").and_then(|v| v.as_str());
        match (package, target, src_path, reason) {
            (Some(package), Some(target), Some(src_path), Some(_reason)) => {
                assert!(
                    !src_path.contains('\\') && !src_path.starts_with('/'),
                    "allowlist src_path must be repo-relative + forward-slash: {src_path:?}"
                );
                let triplet = (
                    package.to_string(),
                    target.to_string(),
                    src_path.to_string(),
                );
                assert!(
                    seen_triplet.insert(triplet),
                    "duplicate allowlist entry (package `{package}`, target `{target}`, \
                     src_path `{src_path}`): each exception must appear exactly once"
                );
                assert!(
                    seen_pkg_target.insert((package.to_string(), target.to_string())),
                    "duplicate allowlist (package, target) key (package `{package}`, target \
                     `{target}`): a `(package, target)` may appear at most once, otherwise a \
                     stale duplicate could be masked by a correct one"
                );
                out.push(AllowEntry {
                    package: package.to_string(),
                    target: target.to_string(),
                    src_path: src_path.to_string(),
                });
            }
            _ => panic!(
                "allowlist entry malformed (needs string package/target/src_path/reason): {e}"
            ),
        }
    }
    out
}

/// A package's integration-test surface, distilled from `cargo metadata` +
/// the on-disk `tests/` directory. Kept as a plain struct so the pure
/// `compute_failures` checker is unit-testable without running cargo.
#[derive(Debug, Clone)]
struct PackageLayout {
    name: String,
    /// `<manifest_dir>/tests/main.rs`, forward-slash normalized.
    expected_main_src_posix: String,
    /// Whether `<manifest_dir>/tests/main.rs` exists on disk.
    main_rs_exists: bool,
    /// Integration-test targets reported by cargo metadata:
    /// `(target_name, repo_rel_posix_src)`.
    test_targets: Vec<(String, String)>,
    /// IMMEDIATE (non-recursive) `tests/*.rs` files: repo-rel-posix paths.
    immediate_test_files: Vec<String>,
    /// Cargo-AUTO-DISCOVERABLE integration-test source positions:
    /// every immediate `tests/*.rs` PLUS every `tests/*/main.rs` exactly one
    /// subdirectory deep (repo-rel-posix). These are the positions cargo turns
    /// into a compiled test binary on its own; a candidate WITHOUT a matching
    /// `test_targets` src is a binary `cargo metadata` does not report (the
    /// `autotests = false` hiding case), even if the package has other targets.
    /// Files deeper than one level, or wired as modules under `tests/main.rs`,
    /// are NOT auto-discoverable and are deliberately excluded.
    auto_discoverable_candidates: Vec<String>,
}

/// Pure checker: given the per-package layouts + the allowlist, return the
/// ordered list of `(package, message)` failures. EMPTY ⇒ conformant. No I/O —
/// the discrimination self-test feeds synthetic layouts through this directly.
fn compute_failures(packages: &[PackageLayout], allowlist: &[AllowEntry]) -> Vec<(String, String)> {
    let allowlist_rel = "scripts/integration-test-layout-allowlist.json";
    let mut failures: Vec<(String, String)> = Vec::new();

    // (package, target) -> entry, for exact lookup.
    let mut allow_by_key: BTreeMap<(String, String), &AllowEntry> = BTreeMap::new();
    for e in allowlist {
        allow_by_key.insert((e.package.clone(), e.target.clone()), e);
    }
    // Which allowlist entries matched a real metadata target (stale-failing).
    let mut matched_allow: BTreeSet<(String, String)> = BTreeSet::new();

    for pkg in packages {
        // ---- Exactly ONE tests/main.rs binary. Two `[[test]]` blocks both
        //      `path = "tests/main.rs"` make cargo metadata report TWO targets
        //      with identical src; each individually `continue`s on the
        //      sanctioned-main path below, so without this count the second
        //      compiled binary slips past.
        let main_targets: Vec<&String> = pkg
            .test_targets
            .iter()
            .filter(|(_, src)| *src == pkg.expected_main_src_posix)
            .map(|(name, _)| name)
            .collect();
        if main_targets.len() > 1 {
            let names = main_targets
                .iter()
                .map(|n| n.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            failures.push((
                pkg.name.clone(),
                format!(
                    "package `{}` has {} tests/main.rs integration-test targets ({names}) — \
                     exactly one tests/main.rs binary is allowed; a second [[test]] pointing \
                     at tests/main.rs still compiles a separate binary.",
                    pkg.name,
                    main_targets.len(),
                ),
            ));
        }

        // ---- metadata test targets: each must be tests/main.rs OR allowlisted.
        for (tname, tsrc_rel) in &pkg.test_targets {
            let tsrc_posix = tsrc_rel.clone();
            if tsrc_posix == pkg.expected_main_src_posix {
                continue; // sanctioned consolidated target
            }
            match allow_by_key.get(&(pkg.name.clone(), tname.clone())) {
                None => failures.push((
                    pkg.name.clone(),
                    format!(
                        "integration-test target `{tname}` (src {tsrc_rel}) is not \
                         tests/main.rs and is not allowlisted. Consolidate it into \
                         tests/main.rs (e.g. under tests/cases/), or add an exact \
                         allowlist entry to {allowlist_rel} if it genuinely needs a \
                         separate test process."
                    ),
                )),
                Some(entry) => {
                    if entry.src_path != *tsrc_rel {
                        failures.push((
                            pkg.name.clone(),
                            format!(
                                "allowlisted target `{tname}` src_path moved: allowlist \
                                 expects `{}` but cargo metadata reports `{tsrc_rel}`. \
                                 Update {allowlist_rel} to the new path.",
                                entry.src_path
                            ),
                        ));
                    }
                    matched_allow.insert((pkg.name.clone(), tname.clone()));
                }
            }
        }

        // ---- GOV-D4 (1): tests/main.rs on disk ⇒ metadata MUST report it.
        if pkg.main_rs_exists {
            let has_main = pkg
                .test_targets
                .iter()
                .any(|(_, src)| *src == pkg.expected_main_src_posix);
            if !has_main {
                failures.push((
                    pkg.name.clone(),
                    format!(
                        "{} exists on disk but cargo metadata does NOT report a \
                         tests/main.rs integration-test target — a missing or \
                         misconfigured [[test]] / autotests setting is hiding it.",
                        pkg.expected_main_src_posix
                    ),
                ));
            }
        }

        // ---- GOV-D4 (2): immediate tests/*.rs present but ZERO metadata targets.
        if !pkg.immediate_test_files.is_empty() && pkg.test_targets.is_empty() {
            let mut names = pkg.immediate_test_files.clone();
            names.sort();
            failures.push((
                pkg.name.clone(),
                format!(
                    "{} immediate tests/*.rs file(s) exist ({}) but cargo metadata \
                     reports ZERO integration-test targets — `autotests = false` (or \
                     an equivalent misconfig) is hiding compiled test binaries.",
                    names.len(),
                    names.join(", ")
                ),
            ));
        }

        // ---- GOV-D4 (4): stray immediate tests/*.rs other than main.rs / allowlisted src.
        let mut allowed_immediate: BTreeSet<String> =
            BTreeSet::from([pkg.expected_main_src_posix.clone()]);
        // `expected_main_src_posix` is `<dir>/tests/main.rs`; the immediate
        // `tests/` prefix is `<dir>/tests/`.
        if let Some(tests_prefix) = pkg.expected_main_src_posix.strip_suffix("main.rs") {
            for e in allowlist {
                if e.package != pkg.name {
                    continue;
                }
                // Only "allowed immediate" if the src is DIRECTLY under this
                // package's `tests/` (i.e. `<tests_prefix><basename>` with no
                // further `/`). A subdir src is not at the immediate level.
                if let Some(tail) = e.src_path.strip_prefix(tests_prefix) {
                    if !tail.is_empty() && !tail.contains('/') {
                        allowed_immediate.insert(e.src_path.clone());
                    }
                }
            }
        }
        for f in &pkg.immediate_test_files {
            if allowed_immediate.contains(f) {
                continue;
            }
            failures.push((
                pkg.name.clone(),
                format!(
                    "stray immediate test file {f} — only tests/main.rs (plus exactly \
                     allowlisted files) may live at the top tests/*.rs level, because \
                     each such file auto-becomes its own test binary. Move it under \
                     tests/cases/ (or another subdirectory) and wire it through \
                     tests/main.rs."
                ),
            ));
        }

        // ---- HIDDEN AUTO-DISCOVERABLE BINARY: every cargo-auto-discoverable
        //      position (`tests/*.rs` and `tests/*/main.rs` one subdir deep)
        //      must correspond to a metadata test target. A candidate WITHOUT a
        //      matching target is a binary cargo compiles but metadata does not
        //      report — the `autotests = false` hiding case. This fires PER
        //      CANDIDATE even when the package has OTHER metadata targets, so it
        //      catches a hidden `tests/rogue/main.rs` next to a valid
        //      `tests/main.rs` (which the GOV-D4(2) zero-targets rule cannot).
        let reported_srcs: BTreeSet<&str> = pkg
            .test_targets
            .iter()
            .map(|(_, src)| src.as_str())
            .collect();
        for cand in &pkg.auto_discoverable_candidates {
            if reported_srcs.contains(cand.as_str()) {
                continue;
            }
            failures.push((
                pkg.name.clone(),
                format!(
                    "{cand} is a cargo-auto-discoverable integration-test position \
                     (tests/*.rs or tests/<dir>/main.rs) but cargo metadata reports no \
                     integration-test target for it — `autotests = false` (or an explicit \
                     [[test]] that omits it) is hiding a separately-compiled test binary. \
                     Wire it through tests/main.rs (e.g. as a module under tests/cases/) or \
                     remove it."
                ),
            ));
        }
    }

    // ---- STALE-FAILING: every allowlist entry must have matched a real target.
    let known_packages: BTreeSet<&str> = packages.iter().map(|p| p.name.as_str()).collect();
    for e in allowlist {
        let key = (e.package.clone(), e.target.clone());
        if !matched_allow.contains(&key) {
            let reason = if known_packages.contains(e.package.as_str()) {
                format!(
                    "cargo metadata reports no integration-test target named `{}` for \
                     package `{}` (it was removed or renamed)",
                    e.target, e.package
                )
            } else {
                format!(
                    "package `{}` is not a workspace member in cargo metadata",
                    e.package
                )
            };
            failures.push((
                e.package.clone(),
                format!(
                    "STALE allowlist entry: {reason}. Remove the dead exception from \
                     {allowlist_rel} (an allowlisted binary that no longer exists must \
                     not leave a lingering exception)."
                ),
            ));
        }
    }

    failures
}

/// Enumerate the IMMEDIATE (non-recursive) `*.rs` files directly under
/// `tests_dir`, as repo-rel-posix paths. Only the top tests/*.rs level
/// auto-becomes a separate binary, so files under subdirs are excluded.
fn immediate_test_rs_files(tests_dir: &Path) -> Vec<String> {
    let mut out = Vec::new();
    let Ok(read_dir) = fs::read_dir(tests_dir) else {
        return out;
    };
    for entry in read_dir.flatten() {
        let p = entry.path();
        let is_file = entry.file_type().map(|t| t.is_file()).unwrap_or(false);
        if is_file && p.extension().and_then(|e| e.to_str()) == Some("rs") {
            out.push(repo_rel_posix(&p));
        }
    }
    out
}

/// Enumerate every cargo-AUTO-DISCOVERABLE integration-test source position
/// under `tests_dir`, as repo-rel-posix paths: every immediate `tests/*.rs`
/// PLUS every `tests/*/main.rs` exactly one subdirectory deep. Cargo compiles
/// each of these into its OWN test binary, so each is a candidate the metadata
/// targets must account for. Files deeper than one level (e.g.
/// `tests/cases/correctness/foo.rs`) and non-`main.rs` files inside a
/// subdirectory (e.g. `tests/cases/architecture_guards.rs`) are NOT
/// auto-discovered — they only compile when wired as modules under
/// `tests/main.rs` — so they are deliberately excluded.
fn auto_discoverable_test_candidates(tests_dir: &Path) -> Vec<String> {
    let mut out = immediate_test_rs_files(tests_dir);
    let Ok(read_dir) = fs::read_dir(tests_dir) else {
        return out;
    };
    for entry in read_dir.flatten() {
        let sub = entry.path();
        let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
        if !is_dir {
            continue;
        }
        let nested_main = sub.join("main.rs");
        if nested_main.is_file() {
            out.push(repo_rel_posix(&nested_main));
        }
    }
    out
}

/// Locate the cargo binary: prefer the `CARGO` env var cargo sets during a
/// test run; otherwise fall back to `cargo` on PATH. Cross-platform — no
/// hardcoded per-OS binary name.
fn cargo_bin() -> String {
    std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string())
}

/// Run `cargo metadata` and distill the per-package layouts.
fn collect_package_layouts() -> Vec<PackageLayout> {
    let output = Command::new(cargo_bin())
        .args(["metadata", "--format-version", "1", "--no-deps"])
        .current_dir(workspace_root())
        .output()
        .expect("run `cargo metadata`");
    assert!(
        output.status.success(),
        "`cargo metadata` failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("parse `cargo metadata` output");

    let workspace_members: BTreeSet<String> = json
        .get("workspace_members")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();

    let packages = json
        .get("packages")
        .and_then(|v| v.as_array())
        .expect("`cargo metadata` packages array");

    let mut layouts = Vec::new();
    for pkg in packages {
        let id = pkg.get("id").and_then(|v| v.as_str()).unwrap_or_default();
        if !workspace_members.contains(id) {
            continue;
        }
        let name = pkg
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();
        let manifest_path = pkg
            .get("manifest_path")
            .and_then(|v| v.as_str())
            .expect("package manifest_path");
        let manifest_dir = Path::new(manifest_path)
            .parent()
            .expect("manifest dir")
            .to_path_buf();
        let tests_dir = manifest_dir.join("tests");
        let main_rs = tests_dir.join("main.rs");
        let expected_main_src_posix = repo_rel_posix(&main_rs);

        let mut test_targets = Vec::new();
        if let Some(targets) = pkg.get("targets").and_then(|v| v.as_array()) {
            for t in targets {
                let is_test = t
                    .get("kind")
                    .and_then(|v| v.as_array())
                    .map(|k| k.iter().any(|x| x.as_str() == Some("test")))
                    .unwrap_or(false);
                if !is_test {
                    continue;
                }
                let tname = t
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_string();
                let tsrc = t
                    .get("src_path")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default();
                let tsrc_rel = repo_rel_posix(Path::new(tsrc));
                test_targets.push((tname, tsrc_rel));
            }
        }

        layouts.push(PackageLayout {
            name,
            expected_main_src_posix,
            main_rs_exists: main_rs.is_file(),
            test_targets,
            immediate_test_files: immediate_test_rs_files(&tests_dir),
            auto_discoverable_candidates: auto_discoverable_test_candidates(&tests_dir),
        });
    }
    layouts
}

/// THE GUARD: the real consolidated workspace MUST be conformant.
///
/// Mirror of `scripts/check-integration-test-layout.mjs`. Discriminating —
/// see `layout_checker_discriminates_stray_and_stale` for the proof that a
/// second top-level `tests/*.rs` (or a stale allowlist entry) makes the same
/// pure checker FAIL.
#[test]
fn integration_test_layout_is_consolidated() {
    let allowlist = load_allowlist();
    let packages = collect_package_layouts();
    let failures = compute_failures(&packages, &allowlist);
    assert!(
        failures.is_empty(),
        "ANTI-BINARY-GROWTH GUARD — every workspace package must expose AT MOST one \
         tests/main.rs integration-test binary (extra cases live under tests/cases/ \
         and are wired through main.rs), plus any EXACTLY-allowlisted exceptions in \
         scripts/integration-test-layout-allowlist.json. A new top-level tests/*.rs \
         auto-becomes a separate binary and re-balloons the gate. Violations:\n{}",
        failures
            .iter()
            .map(|(p, m)| format!("  {p}: {m}"))
            .collect::<Vec<_>>()
            .join("\n")
    );
}

/// The committed allowlist must be exactly the two genuine
/// "needs-a-separate-test-process" exceptions, in agreement with the Node
/// guard's expectation. If a third entry is added (or one removed) without a
/// corresponding architecture decision, this fails loudly — the exception set
/// is small and audited, not free-form.
#[test]
fn allowlist_is_the_two_known_process_isolated_targets() {
    let allowlist = load_allowlist();
    let actual: BTreeSet<(String, String, String)> = allowlist
        .iter()
        .map(|e| (e.package.clone(), e.target.clone(), e.src_path.clone()))
        .collect();
    let expected: BTreeSet<(String, String, String)> = BTreeSet::from([
        (
            "verter_session".to_string(),
            "allocator_canaries".to_string(),
            "crates/verter_session/tests/allocator_canaries.rs".to_string(),
        ),
        (
            "verter_lsp".to_string(),
            "lsp_audit_trace_out_env_var".to_string(),
            "crates/verter_lsp/tests/lsp_audit_trace_out_env_var.rs".to_string(),
        ),
    ]);
    assert_eq!(
        actual, expected,
        "the integration-test-layout allowlist drifted from the two known \
         process-isolated targets (allocator_canaries + lsp_audit_trace_out_env_var). \
         Adding/removing an exception is an architecture decision: update BOTH this \
         guard's expectation and scripts/integration-test-layout-allowlist.json, and \
         justify the new entry's process-global isolation need."
    );
}

/// DISCRIMINATION SELF-TEST: the pure checker must FAIL on a second top-level
/// `tests/*.rs` AND on a stale allowlist entry — the two regressions this
/// guard exists to catch. Without this, a permissive `compute_failures` could
/// silently pass everything.
#[test]
fn layout_checker_discriminates_stray_and_stale() {
    // A conformant baseline: one package with exactly tests/main.rs.
    let main_src = "crates/demo/tests/main.rs".to_string();
    let conformant = PackageLayout {
        name: "demo".to_string(),
        expected_main_src_posix: main_src.clone(),
        main_rs_exists: true,
        test_targets: vec![("main".to_string(), main_src.clone())],
        immediate_test_files: vec![main_src.clone()],
        auto_discoverable_candidates: vec![main_src.clone()],
    };
    let no_allow: Vec<AllowEntry> = Vec::new();
    assert!(
        compute_failures(std::slice::from_ref(&conformant), &no_allow).is_empty(),
        "baseline conformant layout must produce zero failures"
    );

    // (a) STRAY: a second top-level tests/*.rs becomes its own binary.
    let with_stray = PackageLayout {
        test_targets: vec![
            ("main".to_string(), main_src.clone()),
            (
                "rogue".to_string(),
                "crates/demo/tests/rogue.rs".to_string(),
            ),
        ],
        immediate_test_files: vec![main_src.clone(), "crates/demo/tests/rogue.rs".to_string()],
        ..conformant.clone()
    };
    let stray_failures = compute_failures(&[with_stray], &no_allow);
    assert!(
        stray_failures
            .iter()
            .any(|(_, m)| m.contains("not allowlisted")),
        "a second top-level tests/*.rs MUST be flagged as a non-allowlisted target; \
         got: {stray_failures:?}"
    );
    assert!(
        stray_failures
            .iter()
            .any(|(_, m)| m.contains("stray immediate test file")),
        "a second top-level tests/*.rs MUST be flagged as a stray immediate file; \
         got: {stray_failures:?}"
    );

    // (b) STALE: an allowlist entry whose target does not exist in metadata.
    let stale_allow = vec![AllowEntry {
        package: "demo".to_string(),
        target: "ghost".to_string(),
        src_path: "crates/demo/tests/ghost.rs".to_string(),
    }];
    let stale_failures = compute_failures(std::slice::from_ref(&conformant), &stale_allow);
    assert!(
        stale_failures
            .iter()
            .any(|(_, m)| m.contains("STALE allowlist entry")),
        "an allowlist entry naming a non-existent target MUST be flagged stale; \
         got: {stale_failures:?}"
    );

    // (c) AUTOTESTS HIDE: immediate tests/*.rs but zero metadata targets.
    let autotests_off = PackageLayout {
        test_targets: vec![],
        ..conformant.clone()
    };
    let hide_failures = compute_failures(&[autotests_off], &no_allow);
    assert!(
        hide_failures
            .iter()
            .any(|(_, m)| m.contains("ZERO integration-test targets")),
        "tests/*.rs present with zero metadata targets MUST be flagged \
         (autotests=false hiding tests); got: {hide_failures:?}"
    );

    // (d) MISSING MAIN: tests/main.rs on disk but absent from metadata.
    let missing_main = PackageLayout {
        main_rs_exists: true,
        test_targets: vec![(
            "allocator_canaries".to_string(),
            "crates/demo/tests/allocator_canaries.rs".to_string(),
        )],
        immediate_test_files: vec!["crates/demo/tests/allocator_canaries.rs".to_string()],
        // The auto-discoverable candidate (the allowlisted canary) HAS a matching
        // metadata target, so the only signal that fires here is missing-main.
        auto_discoverable_candidates: vec!["crates/demo/tests/allocator_canaries.rs".to_string()],
        ..conformant.clone()
    };
    // allowlist the canary so ONLY the missing-main signal fires.
    let canary_allow = vec![AllowEntry {
        package: "demo".to_string(),
        target: "allocator_canaries".to_string(),
        src_path: "crates/demo/tests/allocator_canaries.rs".to_string(),
    }];
    let missing_failures = compute_failures(&[missing_main], &canary_allow);
    assert!(
        missing_failures
            .iter()
            .any(|(_, m)| m.contains("does NOT report a tests/main.rs")),
        "tests/main.rs on disk but absent from metadata MUST be flagged; \
         got: {missing_failures:?}"
    );

    // The allowlisted-canary path must NOT itself be flagged as a stray/un-allowlisted
    // target in (d): exactly-allowlisted targets are exempt.
    assert!(
        !missing_failures
            .iter()
            .any(|(_, m)| m.contains("not allowlisted")),
        "an exactly-allowlisted target must be exempt from the 'not allowlisted' \
         failure; got: {missing_failures:?}"
    );

    // (e) DUPLICATE MAIN: two `[[test]]` blocks both `path = "tests/main.rs"`
    // make cargo metadata report TWO targets, both with src == tests/main.rs.
    // Each one individually `continue`s on the sanctioned-main path, so without
    // a count check the second binary slips past. Exactly one tests/main.rs
    // binary is allowed.
    let duplicate_main = PackageLayout {
        test_targets: vec![
            ("main_a".to_string(), main_src.clone()),
            ("main_b".to_string(), main_src.clone()),
        ],
        ..conformant.clone()
    };
    let dup_main_failures = compute_failures(std::slice::from_ref(&duplicate_main), &no_allow);
    assert!(
        dup_main_failures.iter().any(
            |(_, m)| m.contains("tests/main.rs integration-test targets")
                && m.contains("exactly one")
        ),
        "two [[test]] blocks both pointing at tests/main.rs MUST be flagged — a \
         second binary still compiles even though both share the sanctioned src; \
         got: {dup_main_failures:?}"
    );
    // The single-main baseline must NOT trip the duplicate-main check.
    assert!(
        !compute_failures(std::slice::from_ref(&conformant), &no_allow)
            .iter()
            .any(|(_, m)| m.contains("tests/main.rs integration-test targets")),
        "a single tests/main.rs target must not be flagged as a duplicate"
    );

    // (f) HIDDEN NESTED MAIN: `tests/rogue/main.rs` is cargo-auto-discovered as
    // its OWN binary, but with `autotests = false` + an explicit
    // `[[test]] path = "tests/main.rs"` it is INVISIBLE to cargo metadata. The
    // package has a valid main target, so GOV-D4(2) (which needs ZERO targets)
    // never fires; only the per-candidate auto-discoverable check catches it.
    let hidden_nested_main_src = "crates/demo/tests/rogue/main.rs".to_string();
    let hidden_nested_main = PackageLayout {
        // metadata reports ONLY the sanctioned main target (the nested-main is hidden).
        test_targets: vec![("main".to_string(), main_src.clone())],
        immediate_test_files: vec![main_src.clone()],
        // disk has the sanctioned main PLUS a nested rogue main — both are
        // cargo-auto-discoverable positions.
        auto_discoverable_candidates: vec![main_src.clone(), hidden_nested_main_src.clone()],
        ..conformant.clone()
    };
    let hidden_failures = compute_failures(std::slice::from_ref(&hidden_nested_main), &no_allow);
    assert!(
        hidden_failures
            .iter()
            .any(|(_, m)| m.contains(&hidden_nested_main_src)
                && m.contains("cargo metadata reports no integration-test target")),
        "a hidden nested tests/<dir>/main.rs (autotests=false) MUST be flagged as a \
         compiled-but-unreported binary even when another target exists; \
         got: {hidden_failures:?}"
    );
    // The conformant baseline (its sole candidate IS reported) must NOT trip the
    // auto-discoverable check.
    assert!(
        !compute_failures(std::slice::from_ref(&conformant), &no_allow)
            .iter()
            .any(|(_, m)| m.contains("cargo metadata reports no integration-test target")),
        "an auto-discoverable candidate that HAS a matching metadata target must not \
         be flagged as hidden"
    );
}

/// G5 LOAD-LEVEL DISCRIMINATION: a duplicate `(package, target)` allowlist entry
/// must be rejected at parse time (it would otherwise let a STALE duplicate hide
/// behind a correct one in the matched-set bookkeeping).
#[test]
#[should_panic(expected = "duplicate allowlist")]
fn parse_allowlist_rejects_duplicate_package_target_key() {
    // Same (package, target) twice, differing only in src_path — must panic on
    // the `(package, target)` duplicate.
    let raw = r#"{
      "allow": [
        { "package": "p", "target": "t", "src_path": "crates/p/tests/a.rs", "reason": "x" },
        { "package": "p", "target": "t", "src_path": "crates/p/tests/b.rs", "reason": "y" }
      ]
    }"#;
    let _ = parse_allowlist(raw);
}

/// G5 (exact-triplet): a fully-identical duplicate entry is also rejected.
#[test]
#[should_panic(expected = "duplicate allowlist entry")]
fn parse_allowlist_rejects_exact_duplicate_entry() {
    let raw = r#"{
      "allow": [
        { "package": "p", "target": "t", "src_path": "crates/p/tests/a.rs", "reason": "x" },
        { "package": "p", "target": "t", "src_path": "crates/p/tests/a.rs", "reason": "x" }
      ]
    }"#;
    let _ = parse_allowlist(raw);
}

/// A well-formed, duplicate-free allowlist parses cleanly (negative control for
/// the duplicate-rejection tests above — proves the panic is the duplicate, not
/// the parse).
#[test]
fn parse_allowlist_accepts_distinct_entries() {
    let raw = r#"{
      "allow": [
        { "package": "p", "target": "t1", "src_path": "crates/p/tests/a.rs", "reason": "x" },
        { "package": "p", "target": "t2", "src_path": "crates/p/tests/b.rs", "reason": "y" }
      ]
    }"#;
    let entries = parse_allowlist(raw);
    assert_eq!(entries.len(), 2, "two distinct entries must both load");
}
