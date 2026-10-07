use super::*;

#[test]
fn no_std_fs_in_semantic_session_paths() {
    let violations = guard1_violations(&guard1_allowlist());
    assert!(
        violations.is_empty(),
        "Guard 1 (`no_std_fs_in_semantic_session_paths`) violations:\n  {}\n\n\
             A new file outside the allowlist uses `std::fs::`. Either route the I/O\n\
             through `verter_workspace::WorkspaceAccess` (preferred) or, if the file\n\
             writes/reads non-semantic tool output (trace artifacts, MCP baselines,\n\
             test fixtures, TS-runtime tool-cache files, etc.), add the path with a\n\
             rationale to `crates/verter_workspace/tool-output-allowlist.toml`.",
        violations.join("\n  "),
    );
}

#[test]
fn guard1_allowlist_paths_exist() {
    // Every allowlist entry must point to a real production source
    // file in the tree. Stale entries (file deleted, renamed, or
    // never existed) are violations because they silently disarm
    // the guard.
    let root = workspace_root();
    let mut missing = Vec::new();
    for path in load_tool_output_allowlist() {
        let abs = root.join(&path);
        if !abs.exists() {
            missing.push(path);
        }
    }
    assert!(
        missing.is_empty(),
        "tool-output-allowlist.toml entries refer to paths that do not exist:\n  {}\n\n\
             Update or remove these entries; a stale allowlist silently disarms the guard.",
        missing.join("\n  "),
    );
}

#[test]
fn guard1_predicate_rejects_deliberate_violation() {
    // Discriminating: a fabricated source string that uses
    // `std::fs::` MUST be flagged as a violation by the
    // predicate.
    let bad = "use std::fs::File;\nfn read() { let _ = std::fs::read_to_string(\"foo\"); }";
    assert!(
        file_uses_std_fs(bad),
        "guard 1 predicate must flag direct `std::fs::` references",
    );

    let good = "use crate::workspace::WorkspaceAccess;\nfn read(ws: &dyn WorkspaceAccess) { let _ = ws.read_file(\"foo\"); }";
    assert!(
        !file_uses_std_fs(good),
        "guard 1 predicate must NOT flag code that goes through WorkspaceAccess",
    );
}

#[test]
fn vfs_boundary_is_authoritative() {
    let violations = guard2_violations(&guard2_allowlist());
    assert!(
        violations.is_empty(),
        "Guard 2 (`vfs_boundary_is_authoritative`) violations:\n  {}\n\n\
             Direct OS file APIs (std::fs::, tokio::fs::) appearing outside\n\
             `crates/verter_workspace/src/native_fs.rs` (the documented disk boundary)\n\
             must route through `verter_workspace::WorkspaceAccess`.",
        violations.join("\n  "),
    );
}

#[test]
fn guard2_predicate_rejects_deliberate_violation() {
    let bad_std = "use std::fs::read_to_string;";
    let bad_tokio = "use tokio::fs::File;";
    assert!(file_uses_os_file_api(bad_std), "guard 2 must flag std::fs");
    assert!(
        file_uses_os_file_api(bad_tokio),
        "guard 2 must flag tokio::fs"
    );
    assert!(
        !file_uses_os_file_api("use crate::workspace::WorkspaceAccess;"),
        "guard 2 must NOT flag WorkspaceAccess users",
    );
}

#[test]
pub(crate) fn external_corpus_paths_not_present_outside_gated_tests() {
    let violations = guard4_violations();
    assert!(
        violations.is_empty(),
        "Guard 4 (`external_corpus_paths_not_present_outside_gated_tests`) violations:\n  {}\n\n\
             Test files that reference `.integration-tests/repos/...` must be gated behind a\n\
             Cargo feature (e.g., `#![cfg(feature = \"external-corpus\")]`). Vendor fixtures into\n\
             `tests/<feature>/fixtures/` for the default workspace test run.",
        violations.join("\n  "),
    );
}

#[test]
fn guard4_predicate_rejects_deliberate_violation() {
    let root = workspace_root().join("crates/verter_session/tests/cases/architecture");
    for file in ["foundations/mod.rs", "hermeticity.rs"] {
        assert!(external_corpus_guard_self_exempts(&root.join(file)));
    }
    for file in ["cache.rs", "future_guard.rs"] {
        assert!(!external_corpus_guard_self_exempts(&root.join(file)));
    }
    // Construct the forbidden literal at runtime so the source
    // of `architecture_guards.rs` itself does NOT contain the
    // string `.integration-tests/repos/` — otherwise the guard
    // we just defined would scan its own source and report this
    // file as a violation.
    let forbidden_segment = format!(".{}{}{}/repos/", "integration", "-", "tests");
    let bad = format!(
        "#[test]\nfn t() {{ let _ = include_str!(\"../{}nuxt-ui/src/Foo.vue\"); }}",
        forbidden_segment
    );
    let gated = format!(
            "#![cfg(feature = \"external-corpus\")]\n#[test]\nfn t() {{ let _ = include_str!(\"../{}nuxt-ui/src/Foo.vue\"); }}",
            forbidden_segment
        );
    let local_only =
        "#[test]\nfn t() { let _ = include_str!(\"./fixtures/Foo.vue\"); }".to_string();
    assert!(
        test_file_has_ungated_external_corpus_path(&bad),
        "guard 4 must flag ungated external-corpus references",
    );
    assert!(
        !test_file_has_ungated_external_corpus_path(&gated),
        "guard 4 must NOT flag references inside a feature-gated file",
    );
    assert!(
        !test_file_has_ungated_external_corpus_path(&local_only),
        "guard 4 must NOT flag tests that use vendored fixtures",
    );

    // Sibling-checkout spelling (`../<repo>` beside this repo) — the
    // historical guard evasion. Construct at runtime per the pattern
    // above.
    let sibling = format!("..{}{}", "/", "vize");
    let bad_sibling = format!(
        "#[test]\nfn t() {{ let p = std::path::PathBuf::from(\"{}/tests/_fixtures/x\"); }}",
        sibling
    );
    let gated_sibling = format!(
            "#![cfg(feature = \"external-corpus\")]\n#[test]\nfn t() {{ let p = std::path::PathBuf::from(\"{}/tests/_fixtures/x\"); }}",
            sibling
        );
    assert!(
        test_file_has_ungated_external_corpus_path(&bad_sibling),
        "guard 4 must flag ungated sibling-checkout references",
    );
    assert!(
        !test_file_has_ungated_external_corpus_path(&gated_sibling),
        "guard 4 must NOT flag sibling-checkout references inside a feature-gated file",
    );

    // Item-level gating (`#[cfg(feature = "external-corpus")] mod ...`)
    // counts as gated; comment-only mentions are not dependencies.
    let item_gated = format!(
            "#[cfg(feature = \"external-corpus\")]\nmod external_corpus {{\n    const P: &str = \"{}nuxt-ui\";\n}}",
            forbidden_segment
        );
    let comment_only = format!(
        "// the old test read {}nuxt-ui and was deleted\nfn t() {{}}",
        forbidden_segment
    );
    assert!(
        !test_file_has_ungated_external_corpus_path(&item_gated),
        "guard 4 must NOT flag references inside an item-gated external-corpus mod",
    );
    assert!(
        !test_file_has_ungated_external_corpus_path(&comment_only),
        "guard 4 must NOT flag comment-only mentions of corpus paths",
    );
}

#[test]
fn no_std_fs_outside_native_fs_or_allow_list() {
    // D14 NativeFs invariant lock. Production-source files that
    // contain `std::fs::` must either BE the canonical disk
    // boundary (`native_fs.rs`) or appear in `D14_ALLOW_LIST`
    // with an explicit justification.
    let permitted = d14_permitted_paths();
    let violations = d14_violations(&permitted);
    assert!(
        violations.is_empty(),
        "D14 (`no_std_fs_outside_native_fs_or_allow_list`) violations:\n  {}\n\n\
             Each survivor is a production file outside\n\
             `crates/verter_workspace/src/native_fs.rs` that uses `std::fs::`\n\
             without an explicit `D14_ALLOW_LIST` entry. To resolve, EITHER\n\
             route the I/O through `verter_workspace::NativeFs` /\n\
             `WorkspaceAccess`, OR add an `(path, justification)` tuple to\n\
             `D14_ALLOW_LIST` in `crates/verter_session/tests/cases/architecture/foundations/mod.rs`.",
        violations.join("\n  "),
    );
}

#[test]
fn d14_allow_list_paths_exist_and_actually_use_std_fs() {
    // Every D14 ALLOW_LIST entry must:
    //   1. Point at a real production source file.
    //   2. Actually contain `std::fs::` — a stale entry silently
    //      disarms the guard for an unrelated path that may later
    //      be reused.
    let root = workspace_root();
    let mut missing: Vec<String> = Vec::new();
    let mut clean: Vec<String> = Vec::new();
    for (path, _justification) in D14_ALLOW_LIST {
        let abs = root.join(path);
        if !abs.exists() {
            missing.push((*path).to_string());
            continue;
        }
        let src = match fs::read_to_string(&abs) {
            Ok(s) => s,
            Err(_) => {
                missing.push((*path).to_string());
                continue;
            }
        };
        if !d14_file_uses_std_fs(&src) {
            clean.push((*path).to_string());
        }
    }
    assert!(
        missing.is_empty(),
        "D14 ALLOW_LIST entries refer to paths that do not exist:\n  {}\n\n\
             Update or remove these entries; a stale allow-list silently\n\
             disarms the NativeFs invariant lock.",
        missing.join("\n  "),
    );
    assert!(
        clean.is_empty(),
        "D14 ALLOW_LIST entries refer to files that no longer use `std::fs::`:\n  {}\n\n\
             Delete these entries; they advertise an exemption that is no\n\
             longer warranted.",
        clean.join("\n  "),
    );
}

#[test]
fn d14_native_fs_path_actually_contains_std_fs() {
    // Sanity counter-fixture: the canonical disk boundary file
    // MUST itself contain `std::fs::` calls (otherwise the
    // exemption is meaningless and a typo in the path constant
    // would silently disarm the guard).
    let abs = workspace_root().join(D14_NATIVE_FS_PATH);
    let src = fs::read_to_string(&abs)
        .unwrap_or_else(|e| panic!("D14 native_fs path must be readable: {e}"));
    assert!(
            d14_file_uses_std_fs(&src),
            "D14 NATIVE_FS_PATH (`{D14_NATIVE_FS_PATH}`) must contain `std::fs::` callsites; it is the canonical disk boundary the lock pivots on. If this assertion fires, either the path constant is stale or NativeFs has been refactored — update `D14_NATIVE_FS_PATH` accordingly.",
        );
}
