use super::*;

#[test]
fn no_off_store_host_caches() {
    use syn::parse_file;

    let allow_list = phase_8_allow_list();

    // Verify the source file we're parsing has not had a Phase-6b mirror
    // field re-added by name. This is independent of the syn walk and is
    // a belt-and-suspenders check against re-introducing the deleted
    // F7 field name.
    let lib_src = read_workspace_file("crates/verter_session/src/lib.rs");
    {
        let forbidden = "route_owned_shallow_cache";
        let declaration_pattern = format!("pub(crate) {forbidden}:");
        assert!(
            !lib_src.contains(&declaration_pattern),
            "Phase 8 regression: VerterHost field `{forbidden}` was \
             deleted by Phase 6b and must not be re-introduced. Found \
             declaration `{declaration_pattern}` in lib.rs."
        );
    }

    // Parse lib.rs via syn and walk VerterHost fields.
    let parsed = parse_file(&lib_src).expect("parse verter_session/src/lib.rs via syn");
    let (violations, surveyed_cache_fields) =
        no_off_store_host_caches_inner(&parsed, "VerterHost", &allow_list);

    // Discriminator-coverage check: the syn walk MUST surface at least
    // one cache-shape field. If the count is zero, either the cache-shape
    // detector is broken (the guard cannot detect re-introductions) or
    // every cache-shape field has been moved — both worth flagging.
    assert!(
        !surveyed_cache_fields.is_empty(),
        "no_off_store_host_caches: the syn walk found ZERO cache-shape \
         fields on VerterHost, which means either the cache-shape \
         detector is broken or every cache-shape field has been moved. \
         Investigate before re-running."
    );

    assert!(
        violations.is_empty(),
        "no_off_store_host_caches violations:\n{}\n\nAllow-list reference \
         (each entry must cite a phase-report rationale):\n{}",
        violations.join("\n"),
        allow_list
            .iter()
            .map(|(k, v)| format!("  {k}: {v}"))
            .collect::<Vec<_>>()
            .join("\n")
    );
}

#[test]
fn no_off_store_host_caches_discriminator_self_test() {
    // Self-test: hand-craft a synthetic struct with one allow-listed
    // cache field (must pass) and one un-allow-listed cache field (must
    // fail). This proves the inner algorithm discriminates between the
    // good and bad cases — i.e., the guard would actually catch a
    // re-introduction. Without this, the empty-violations result of
    // `no_off_store_host_caches` against the integration tip is
    // indistinguishable from a broken detector that always passes.
    //
    // CLAUDE.md "Stub Prevention" — characterization-style discriminator
    // test for the guard itself.
    use syn::parse_file;
    let allow_list = phase_8_allow_list();

    // (a) Synthetic struct with ONLY allow-listed fields — must produce
    //     zero violations. `query_profile` is allow-listed (execution-
    //     policy state); `workspace` is allow-listed (re-pointable
    //     handle, not a hashmap-cache).
    let synthetic_pass = r#"
        pub struct SyntheticHost {
            pub(crate) instance_id: u64,
            pub(crate) query_profile: parking_lot::Mutex<verter_semantic::profile::QueryProfile>,
            pub(crate) workspace: Arc<parking_lot::RwLock<Arc<dyn WorkspaceAccess>>>,
            pub(crate) tick: AtomicU64,
        }
    "#;
    let parsed_pass = parse_file(synthetic_pass).expect("parse synthetic_pass");
    let (pass_violations, pass_surveyed) =
        no_off_store_host_caches_inner(&parsed_pass, "SyntheticHost", &allow_list);
    assert!(
        pass_violations.is_empty(),
        "discriminator self-test: synthetic_pass should produce zero \
         violations (query_profile and workspace are allow-listed, the \
         others are non-cache shapes), but got:\n{}",
        pass_violations.join("\n")
    );
    // Both allow-listed fields must be SURVEYED (otherwise the cache
    // detector is failing to flag them as candidates in the first place).
    let surveyed_names: Vec<String> = pass_surveyed.iter().map(|(n, _)| n.clone()).collect();
    assert!(
        surveyed_names.contains(&"query_profile".to_string()),
        "discriminator self-test: synthetic_pass must surface \
         `query_profile` as a cache-shape candidate; surveyed: {surveyed_names:?}"
    );
    assert!(
        surveyed_names.contains(&"workspace".to_string()),
        "discriminator self-test: synthetic_pass must surface \
         `workspace` as a cache-shape candidate; surveyed: {surveyed_names:?}"
    );

    // (b) Synthetic struct with ONE un-allow-listed cache-shape field —
    //     must produce exactly one violation, naming that field.
    let synthetic_fail = r#"
        pub struct SyntheticHost {
            pub(crate) instance_id: u64,
            pub(crate) p8_probe_cache: parking_lot::Mutex<rustc_hash::FxHashMap<String, u64>>,
            pub(crate) tick: AtomicU64,
        }
    "#;
    let parsed_fail = parse_file(synthetic_fail).expect("parse synthetic_fail");
    let (fail_violations, fail_surveyed) =
        no_off_store_host_caches_inner(&parsed_fail, "SyntheticHost", &allow_list);
    assert_eq!(
        fail_violations.len(),
        1,
        "discriminator self-test: synthetic_fail should produce exactly \
         one violation (`p8_probe_cache` is a cache-shape field that is \
         not allow-listed and not on ProjectTypeStore), but got {} \
         violations:\n{}\nSurveyed: {fail_surveyed:?}",
        fail_violations.len(),
        fail_violations.join("\n")
    );
    assert!(
        fail_violations[0].contains("p8_probe_cache"),
        "discriminator self-test: the violation must name the offending \
         field (`p8_probe_cache`), but the message was: {}",
        fail_violations[0]
    );

    // (c) Synthetic struct with a cache-shape field whose type points at
    //     ProjectTypeStore — must NOT violate (the destination
    //     allowance). Forward-looking branch.
    let synthetic_destination = r#"
        pub struct SyntheticHost {
            pub(crate) instance_id: u64,
            pub(crate) future_db: Arc<crate::project_type_store::ProjectTypeStore>,
            pub(crate) future_cache: parking_lot::Mutex<crate::project_type_store::ProjectTypeStore>,
        }
    "#;
    let parsed_destination =
        parse_file(synthetic_destination).expect("parse synthetic_destination");
    let (dest_violations, _) =
        no_off_store_host_caches_inner(&parsed_destination, "SyntheticHost", &allow_list);
    assert!(
        dest_violations.is_empty(),
        "discriminator self-test: synthetic_destination should produce \
         zero violations (future_cache is a Mutex<ProjectTypeStore>, which \
         is the destination allowance), but got:\n{}",
        dest_violations.join("\n")
    );
}

#[test]
fn every_db_field_in_project_type_store_appears_in_inventory() {
    let src = read_workspace_file("crates/verter_session/src/project_type_store.rs");
    let inventory = verter_session::project_type_store::PROJECT_TYPE_STORE_DB_INVENTORY;

    let unregistered = unregistered_db_fields_in_struct(&src, "ProjectTypeStore", inventory);

    assert!(
        unregistered.is_empty(),
        "guard 8: DB-typed field(s) on ProjectTypeStore are not in \
         PROJECT_TYPE_STORE_DB_INVENTORY: {unregistered:?}. \
         Adding a DB-typed field requires updating the inventory + \
         all_dbs_for_invalidation() in lockstep. See \
         crates/verter_session/src/project_type_store.rs."
    );
}

#[test]
fn guard8_predicate_rejects_unregistered_db_field() {
    // Deliberate-violation fixture: a struct with a DB-shape field
    // missing from the registered list. The predicate must surface
    // the offending field.
    let fixture_src = r#"
        pub struct FakeProjectTypeStore {
            pub indexed: FileArtifactStore,
            pub analysis: AnalysisReadyDb,
            pub forgotten_field: ForgottenStore,
        }
    "#;
    let registered = ["indexed", "analysis"];
    let unregistered =
        unregistered_db_fields_in_struct(fixture_src, "FakeProjectTypeStore", &registered);
    assert_eq!(
        unregistered,
        vec!["forgotten_field".to_string()],
        "guard 8 predicate must catch the unregistered DB field. \
         If this assertion fails, the cache-shape detector is too \
         narrow OR the registered-set check is broken."
    );
}

#[test]
fn every_db_field_implements_invalidation_by_canonical() {
    let src = read_workspace_file("crates/verter_session/src/project_type_store.rs");
    let heads = db_field_type_heads_in_struct(&src, "ProjectTypeStore");

    let crate_root = workspace_path("crates/verter_session/src");
    let engine_root = workspace_path("crates/verter_type_engine/src");
    let mut missing: Vec<String> = Vec::new();
    for head in &heads {
        if !invalidation_by_canonical_impl_exists(&crate_root, head)
            && !invalidation_by_canonical_impl_exists(&engine_root, head)
        {
            missing.push(head.clone());
        }
    }

    assert!(
        missing.is_empty(),
        "guard 9: DB-typed field(s) on ProjectTypeStore have no \
         corresponding `impl InvalidationByCanonical for ...` block \
         in `crates/verter_session/src/` or `crates/verter_type_engine/src/`: \
         {missing:?}. Plan §12.A12 \
         requires every DB to implement the per-canonical drain \
         trait so the cascade `invalidate_canonical_across_all_dbs` \
         can dispatch monomorphically."
    );
}

#[test]
fn guard9_predicate_rejects_missing_invalidation_by_canonical_impl() {
    // Deliberate-violation fixture: a struct with a DB-shape field
    // whose head identifier is NOT in the workspace's
    // verter_session src tree (so `invalidation_by_canonical_impl_exists`
    // returns false). Predicate must surface the missing impl.
    let fixture_src = r#"
        pub struct FakeProjectTypeStore {
            pub forgotten_field: NonExistentSentinelGuard9Db,
        }
    "#;
    let heads = db_field_type_heads_in_struct(fixture_src, "FakeProjectTypeStore");
    assert_eq!(
        heads,
        vec!["NonExistentSentinelGuard9Db".to_string()],
        "guard 9 predicate must extract the field's type head id",
    );
    let exists = ["crates/verter_session/src", "crates/verter_type_engine/src"]
        .iter()
        .any(|root| {
            invalidation_by_canonical_impl_exists(
                &workspace_path(root),
                "NonExistentSentinelGuard9Db",
            )
        });
    assert!(
        !exists,
        "guard 9 predicate must report `false` for a head identifier \
         that has no corresponding impl block in the source tree",
    );
}

// ════════════════════════════════════════════════════════════════════════════
// Tier 1A architecture guards (§3.2.5)
//
// Four guards landing with Step 1A:
//
// 1. `no_thread_local_oxc_caches` — rejects reintroduction of the
//    `HOST_PARSED_*_CACHE` thread-locals (D44 lowering boundary).
// 2. `no_direct_oxc_parser_calls_outside_scheduler_path` — only the
//    allow-listed parse sites may invoke the OXC parser (fully
//    qualified `oxc_parser::Parser::new` OR an imported/aliased bare
//    `Parser::new`).
// 3. `no_owned_artifact_holds_borrowed_lifetime` — `OwnedEvalProgram`
//    is `Send + Sync + 'static`.
// 4. `macro_impacting_constructs_fail_lowering_not_silent_skip` (D107)
//    — exercises the lowering on representative macro-impacting
//    fixtures and asserts `Err(LoweringError::*)` instead of an empty
//    `OwnedEvalProgram`.
// ════════════════════════════════════════════════════════════════════════════

/// Tier 1A guard 1 — the `HOST_PARSED_EVAL_PROGRAM_CACHE` and
/// `HOST_PARSED_TYPE_CONTEXT_CACHE` thread-locals were retired in §3.2.4
/// because their cached values held the OXC parser arena alive past the
/// lowering boundary, making the host caches `!Send`.
///
/// Live design: an eval program is parsed once per cold
/// `ensure_indexed_ready_serve` materialise and threaded by reference within
/// the flight; the derived `EvalEnv` lives on the published
/// `IndexedReady`. Only `Send + Sync + 'static` owned-artifact forms
/// (`OwnedEvalProgram`) are admissible in host-owned typed DBs.
///
/// This guard rejects any reintroduction of an OXC-parser-arena
/// thread-local cache. It scans every `.rs` file under
/// `crates/verter_session/src/` and `crates/verter_type_engine/src/` (excluding `_tests.rs` and `tests.rs`)
/// for the literal cache names — a discriminating identifier is more
/// reliable here than a generic `thread_local!\s*\{` regex which would
/// match the legitimate per-thread depth counters in
/// `RESOLUTION_DEPTH`, `LAST_BUDGET_EXCEEDED`, etc.
#[test]
fn no_thread_local_oxc_caches() {
    let banned_idents = [
        "HOST_PARSED_EVAL_PROGRAM_CACHE",
        "HOST_PARSED_TYPE_CONTEXT_CACHE",
    ];
    let mut hits: Vec<(String, &str)> = Vec::new();
    // The session crate and the type engine it builds on.
    let crate_roots = [
        workspace_path("crates/verter_session/src"),
        workspace_path("crates/verter_type_engine/src"),
    ];
    for root in &crate_roots {
        assert!(root.is_dir(), "source root {} is missing", root.display());
    }
    for entry in crate_roots
        .iter()
        .flat_map(|root| walkdir::WalkDir::new(root).into_iter())
        .filter_map(Result::ok)
        .filter(|e| e.path().is_file())
    {
        let path = entry.path();
        let path_str = path.to_string_lossy().replace('\\', "/");
        // Skip test sources — only production rs files participate.
        if path_str.ends_with("_tests.rs")
            || path_str.ends_with("/tests.rs")
            || path_str.contains("/tests/")
        {
            continue;
        }
        if path.ends_with("crates/verter_session/tests/cases/architecture/cache.rs") {
            continue;
        }
        if path.extension().and_then(|e| e.to_str()) != Some("rs") {
            continue;
        }
        let body = std::fs::read_to_string(path).unwrap_or_else(|err| {
            panic!(
                "guard scanner could not read {path_str}: {err} — an \
                 unreadable file must fail the guard, not silently pass"
            )
        });
        for ident in banned_idents {
            // Only count hits OUTSIDE comments. The retirement note
            // in `host_manage.rs` references the names in a
            // documentation comment; that's not a re-introduction.
            for (lineno, line) in body.lines().enumerate() {
                if !line.contains(ident) {
                    continue;
                }
                let trimmed = line.trim_start();
                if trimmed.starts_with("//") || trimmed.starts_with("///") {
                    continue;
                }
                hits.push((format!("{path_str}:{}", lineno + 1), ident));
            }
        }
    }
    assert!(
        hits.is_empty(),
        "Tier 1A guard `no_thread_local_oxc_caches`: forbidden thread-local OXC caches \
         re-introduced in production source: {hits:#?}"
    );
}

// `no_legacy_walker_in_production_code` retired post-§7.3 cutover —
// the legacy walker family is fully deleted. Coverage moves to the
// `tests/cases/g_misc0/no_legacy_walker.rs` `RETIRED_SYMBOLS` gate which scans
// the entire workspace, not just `crates/verter_session/src/host_manage/`.

/// R22 + reachability-GC rename guard.
///
/// Production source must reference the unified
/// `evict_unreachable_artifacts` reachability sweep, NOT the
/// historical `evict_unreachable_indexed_ready` name. The store the
/// sweep operates on holds `IndexedReady`, `FileFacts`, and
/// augmentations under one key, so the broader name is the
/// correct one. Doc-comment back-references in non-production paths
/// (e.g. `.phase-markers/`, `tools/orchestrator/reports/`, plan docs)
/// are out of scope.
#[test]
fn reachability_gc_uses_unified_artifact_name() {
    use std::fs;
    let scan_dirs = [
        "crates/verter_session/src",
        "crates/verter_type_engine/src",
        "crates/verter_workspace/src",
    ];
    let mut violations: Vec<String> = Vec::new();
    for dir in &scan_dirs {
        let root = workspace_root().join(dir);
        let mut stack = vec![root.clone()];
        while let Some(p) = stack.pop() {
            let read = match fs::read_dir(&p) {
                Ok(rd) => rd,
                Err(_) => continue,
            };
            for entry in read.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    stack.push(path);
                    continue;
                }
                if path.extension().and_then(|e| e.to_str()) != Some("rs") {
                    continue;
                }
                let src = match fs::read_to_string(&path) {
                    Ok(s) => s,
                    Err(_) => continue,
                };
                for (idx, line) in src.lines().enumerate() {
                    if line.contains("evict_unreachable_indexed_ready") {
                        violations.push(format!("{}:{}: {}", path.display(), idx + 1, line.trim()));
                    }
                }
            }
        }
    }
    assert!(
        violations.is_empty(),
        "reachability_gc_uses_unified_artifact_name: production source \
         still references the legacy `evict_unreachable_indexed_ready` \
         name. The unified sweep is `evict_unreachable_artifacts`. \
         Violations:\n{}",
        violations.join("\n")
    );
}

/// Architecture guard — direct content-agnostic `FileArtifactStore`
/// reads (`indexed().get_any` / `indexed().get_artifacts_any`) are
/// banned in `verter_session` production source outside a named
/// intent-specific helper allowlist.
///
/// `FileArtifactStore::get_any` and `get_artifacts_any` are
/// content-agnostic, canonical-only lookups: they return *whichever*
/// cached artifact matches the canonical, regardless of content
/// version. With the own-canonical drain retired (Block 2.S /
/// retry-item-2), a stale pre-edit `IndexedReady` / `FileArtifacts` can
/// linger past a same-canonical content edit. A producer that reads it
/// through `get_any` would feed a stale observed-content identity into
/// a provenance-pure `fact_dep_signature` builder — defeating
/// query-identity self-version-rooting at its root.
///
/// Correctness-sensitive readers MUST instead use a content-pinned
/// named helper:
/// - [`crate::VerterHost::current_content_pinned_indexed`] — the
///   scheduler-pinned `IndexedReady` read.
/// - [`crate::VerterHost::artifact_current_indexed`] — the artifact-only
///   `IndexedReady` authority for a canonical the scheduler does not
///   track.
/// - [`crate::VerterHost::current_content_pinned_artifacts`] — the
///   `FileArtifacts` analogue (scheduler-pinned, artifact-only
///   fallback).
///
/// The few legitimate direct `get_any` / `get_artifacts_any` call sites
/// (those helpers' own bodies, plus pure existence/diagnostics probes
/// whose stale answers do not affect a value or its validation) are
/// listed in [`GET_ANY_ALLOWLIST`] with the reason each is exempt. A
/// new direct call site outside the allowlist fails this guard.
#[cfg(test)]
mod content_pinned_artifact_read_guards {
    use std::fs;
    use std::path::PathBuf;

    /// Files permitted to call `indexed().get_any(` /
    /// `indexed().get_artifacts_any(` directly. Each entry pairs the
    /// repo-relative path with the reason the direct read is legitimate.
    ///
    /// Every other production `verter_session/src` file MUST route
    /// `FileArtifactStore` reads through a content-pinned named helper
    /// (`current_content_pinned_indexed` / `artifact_current_indexed` /
    /// `current_content_pinned_artifacts`).
    const GET_ANY_ALLOWLIST: &[(&str, &str)] = &[(
        "crates/verter_session/src/host_manage/analysis_io.rs",
        "Defines the content-pinned named helpers themselves \
             (`artifact_current_indexed`, `current_content_pinned_artifacts`) \
             — their bodies are the artifact-only authority. Also the \
             documented permissive `get_whole_hash` accessor, whose strict \
             sibling is `authoritative_current_content_hash`.",
    )];

    /// Strip `//` line comments and `/* */` block comments so a
    /// `get_any` mention inside a doc-comment is not flagged as a call
    /// site. Character-level scan; string-literal contents are left
    /// intact (a `get_any` substring inside a string literal is not a
    /// production concern this guard cares about, and none exists).
    fn strip_comments(src: &str) -> String {
        let bytes = src.as_bytes();
        let mut out = String::with_capacity(src.len());
        let mut i = 0;
        let mut in_string = false;
        let mut in_line_comment = false;
        let mut in_block_comment = false;
        while i < bytes.len() {
            let c = bytes[i] as char;
            let next = bytes.get(i + 1).map(|b| *b as char);
            if in_line_comment {
                if c == '\n' {
                    in_line_comment = false;
                    out.push('\n');
                }
                i += 1;
                continue;
            }
            if in_block_comment {
                if c == '*' && next == Some('/') {
                    in_block_comment = false;
                    i += 2;
                    continue;
                }
                if c == '\n' {
                    out.push('\n');
                }
                i += 1;
                continue;
            }
            if in_string {
                out.push(c);
                if c == '\\' {
                    if let Some(n) = next {
                        out.push(n);
                    }
                    i += 2;
                    continue;
                }
                if c == '"' {
                    in_string = false;
                }
                i += 1;
                continue;
            }
            if c == '/' && next == Some('/') {
                in_line_comment = true;
                i += 2;
                continue;
            }
            if c == '/' && next == Some('*') {
                in_block_comment = true;
                i += 2;
                continue;
            }
            if c == '"' {
                in_string = true;
            }
            out.push(c);
            i += 1;
        }
        out
    }

    /// True when `src` (already comment-stripped) contains a direct
    /// `indexed()` → `.get_any(` / `.get_artifacts_any(` call chain.
    ///
    /// Whitespace and newlines between `indexed()` and the method call
    /// are tolerated — the call chain is frequently split across lines
    /// by `rustfmt`. A `get_any(` chain rooted on a different DB
    /// accessor is deliberately NOT matched: this guard targets
    /// `FileArtifactStore` reads only.
    ///
    /// ## Known limitation — fluent chains only
    ///
    /// This scanner matches only the **fluent** form where `.get_any(` /
    /// `.get_artifacts_any(` immediately follows `indexed()` (modulo
    /// whitespace). A variable-bound read —
    /// `let s = …indexed(); s.get_any(c)` — splits the `indexed()`
    /// receiver from the call across a binding and is NOT flagged.
    /// Detecting that form by text is not reliable: a bare
    /// `.get_any(`/`.get_artifacts_any(` on a binding cannot be
    /// attributed to a `FileArtifactStore` without false positives
    /// against the identically-named methods on other dbs
    /// (`member_display_facts()`, `analysis()`, …) — that
    /// needs real name resolution, not a scanner. No current
    /// `verter_session` production file uses the var-bound form, and
    /// the structural guard in
    /// `tests/cases/g_misc3/structural_carrier_no_get_any_guard.rs` covers the
    /// carrier-type angle. If a var-bound `FileArtifactStore` read is
    /// ever introduced, convert it to the fluent form (so this guard
    /// catches it) or route it through a content-pinned named helper.
    fn has_direct_file_artifact_get_any(src: &str) -> bool {
        let needle = "indexed()";
        let mut search_from = 0;
        while let Some(rel) = src[search_from..].find(needle) {
            let after = search_from + rel + needle.len();
            let tail = src[after..].trim_start();
            if tail.starts_with(".get_any(") || tail.starts_with(".get_artifacts_any(") {
                return true;
            }
            search_from = after;
        }
        false
    }

    /// Repo-relative `.rs` files under `crates/verter_session/src` and
    /// `crates/verter_type_engine/src` (the type engine the session builds
    /// on), excluding test files (`*_tests.rs`, `tests.rs`).
    fn verter_session_production_rs_files() -> Vec<(PathBuf, String)> {
        let root = super::super::workspace_root();
        let mut files: Vec<PathBuf> = Vec::new();
        for krate in ["crates/verter_session/src", "crates/verter_type_engine/src"] {
            let src_dir = root.join(krate);
            assert!(
                src_dir.is_dir(),
                "source root {} is missing",
                src_dir.display()
            );
            walk_rs(&src_dir, &mut files);
        }
        let mut out: Vec<(PathBuf, String)> = Vec::new();
        for f in files {
            let rel = f
                .strip_prefix(&root)
                .unwrap_or(&f)
                .to_string_lossy()
                .replace('\\', "/");
            let basename = rel.rsplit('/').next().unwrap_or("");
            if basename.ends_with("_tests.rs") || basename == "tests.rs" || rel.contains("/tests/")
            {
                continue;
            }
            out.push((f, rel));
        }
        out
    }

    fn walk_rs(dir: &std::path::Path, out: &mut Vec<PathBuf>) {
        if !dir.is_dir() {
            return;
        }
        for entry in fs::read_dir(dir).unwrap_or_else(|e| panic!("read_dir {}: {e}", dir.display()))
        {
            let path = entry.expect("dir entry").path();
            if path.is_dir() {
                walk_rs(&path, out);
            } else if path.extension().and_then(|e| e.to_str()) == Some("rs") {
                out.push(path);
            }
        }
    }

    /// The scan algorithm — testable in isolation against synthetic
    /// input. Returns the sorted set of repo-relative production files
    /// that hold a direct `FileArtifactStore` `get_any` /
    /// `get_artifacts_any` call AND are not on `allowlist`.
    fn unallowlisted_get_any_files(files: &[(String, String)], allowlist: &[&str]) -> Vec<String> {
        let mut violations: Vec<String> = files
            .iter()
            .filter(|(rel, src)| {
                !allowlist.contains(&rel.as_str())
                    && has_direct_file_artifact_get_any(&strip_comments(src))
            })
            .map(|(rel, _)| rel.clone())
            .collect();
        violations.sort();
        violations.dedup();
        violations
    }

    /// Static allowlist guard — no `verter_session` production file
    /// outside [`GET_ANY_ALLOWLIST`] calls `indexed().get_any(` /
    /// `indexed().get_artifacts_any(` directly.
    #[test]
    fn no_direct_file_artifact_get_any_outside_allowlist() {
        let allow: Vec<&str> = GET_ANY_ALLOWLIST.iter().map(|(p, _)| *p).collect();
        let files: Vec<(String, String)> = verter_session_production_rs_files()
            .into_iter()
            .map(|(path, rel)| {
                let src = fs::read_to_string(&path)
                    .unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
                (rel, src)
            })
            .collect();
        let violations = unallowlisted_get_any_files(&files, &allow);
        assert!(
            violations.is_empty(),
            "content-agnostic `FileArtifactStore` reads found outside the \
             named-helper allowlist:\n  {}\n\nA stale pre-edit artifact can \
             linger past a same-canonical edit (own-canonical drain \
             retired); a `get_any` / `get_artifacts_any` read feeds a stale \
             observed-content identity into the fact-signature builders. \
             Route the read through `current_content_pinned_indexed` / \
             `artifact_current_indexed` / `current_content_pinned_artifacts` \
             — or, if the read is a genuine existence/diagnostics probe, \
             add the file to `GET_ANY_ALLOWLIST` with the reason.",
            violations.join("\n  "),
        );

        // Every allowlisted file MUST still actually contain a direct
        // call — a stale allowlist entry (the call was removed / pinned)
        // must be deleted so the allowlist cannot silently grow
        // permission it no longer needs.
        let by_rel: std::collections::BTreeMap<&str, &str> = files
            .iter()
            .map(|(rel, src)| (rel.as_str(), src.as_str()))
            .collect();
        for (allowed_path, _reason) in GET_ANY_ALLOWLIST {
            let src = by_rel.get(allowed_path).unwrap_or_else(|| {
                panic!(
                    "GET_ANY_ALLOWLIST entry {allowed_path} is not a \
                     verter_session production source file"
                )
            });
            assert!(
                has_direct_file_artifact_get_any(&strip_comments(src)),
                "GET_ANY_ALLOWLIST entry {allowed_path} no longer contains a \
                 direct `indexed().get_any(` / `.get_artifacts_any(` call — \
                 remove the stale allowlist entry.",
            );
        }
    }

    /// Discriminating self-test — proves the scan algorithm
    /// distinguishes a real direct `get_any` call from a comment, a
    /// `member_display_facts().get_any` (different db), and a
    /// content-pinned helper call.
    ///
    /// Without this, an empty-violations result of the guard above is
    /// indistinguishable from a detector that always passes.
    #[test]
    fn get_any_guard_discriminator_self_test() {
        // (a) A file with a direct `indexed().get_any(` call, NOT
        //     allowlisted → must be flagged.
        let bad = (
            "crates/verter_session/src/synthetic_offender.rs".to_string(),
            "fn read(&self) { let _ = self.project_type_store.indexed().get_any(c); }".to_string(),
        );
        let flagged = unallowlisted_get_any_files(std::slice::from_ref(&bad), &[]);
        assert_eq!(
            flagged,
            vec!["crates/verter_session/src/synthetic_offender.rs".to_string()],
            "discriminator: a direct `indexed().get_any(` call in a \
             non-allowlisted file MUST be flagged"
        );

        // (a') The SAME file, now allowlisted → must NOT be flagged.
        let flagged_allowed = unallowlisted_get_any_files(
            std::slice::from_ref(&bad),
            &["crates/verter_session/src/synthetic_offender.rs"],
        );
        assert!(
            flagged_allowed.is_empty(),
            "discriminator: an allowlisted file's direct call MUST NOT be \
             flagged"
        );

        // (b) The newline-split call chain (`rustfmt` form) → must be
        //     flagged.
        let split = (
            "crates/verter_session/src/synthetic_split.rs".to_string(),
            "fn read(&self) {\n    let _ = self\n        .project_type_store\n        \
             .indexed()\n        .get_artifacts_any(c);\n}"
                .to_string(),
        );
        assert_eq!(
            unallowlisted_get_any_files(std::slice::from_ref(&split), &[]),
            vec!["crates/verter_session/src/synthetic_split.rs".to_string()],
            "discriminator: a newline-split `indexed()\\n.get_artifacts_any(` \
             chain MUST be flagged"
        );

        // (c) A `get_any` mention inside a comment → must NOT be
        //     flagged (comment-stripping works).
        let comment_only = (
            "crates/verter_session/src/synthetic_comment.rs".to_string(),
            "// callers MUST NOT use indexed().get_any(c) here\n/* indexed().get_any(x) */\n\
             fn ok(&self) { let _ = self.current_content_pinned_indexed(c); }"
                .to_string(),
        );
        assert!(
            unallowlisted_get_any_files(std::slice::from_ref(&comment_only), &[]).is_empty(),
            "discriminator: a `get_any` mention inside a `//` or `/* */` \
             comment MUST NOT be flagged"
        );

        // (d) `member_display_facts().get_any(` is a DIFFERENT db —
        //     must NOT be flagged.
        let other_db = (
            "crates/verter_session/src/synthetic_other_db.rs".to_string(),
            "fn read(&self) { let _ = self.project_type_store.member_display_facts().get_any(c); }"
                .to_string(),
        );
        assert!(
            unallowlisted_get_any_files(std::slice::from_ref(&other_db), &[]).is_empty(),
            "discriminator: `member_display_facts().get_any(` targets a \
             different db and MUST NOT be flagged by the FileArtifactStore guard"
        );

        // (e) A file that only calls the content-pinned helpers → must
        //     NOT be flagged.
        let pinned = (
            "crates/verter_session/src/synthetic_pinned.rs".to_string(),
            "fn read(&self) { let _ = self.current_content_pinned_indexed(c)\n        \
             .or_else(|| self.artifact_current_indexed(c)); }"
                .to_string(),
        );
        assert!(
            unallowlisted_get_any_files(std::slice::from_ref(&pinned), &[]).is_empty(),
            "discriminator: a file using only the content-pinned named \
             helpers MUST NOT be flagged"
        );
    }

    // ──────────────────────────────────────────────────────────────
    // Named-currency-oracle closure.
    //
    // The `get_any` allowlist guard above bans only the two literal
    // fluent call chains `indexed().get_any(` / `.get_artifacts_any(`.
    // A *named* `FileArtifactStore` method with the identical
    // content-agnostic first-match scan body — `content_hash_for_canonical`,
    // `latest_artifacts_for_canonical` — is outside that guard's
    // textual reach: a content-agnostic "currency oracle" can be
    // reintroduced under any new method name. The two guards below
    // close that gap:
    //
    // - `no_named_currency_oracle_calls_in_production` — a call-site
    //   ban on the two named oracles, so even if a future change
    //   re-adds them, production code cannot call them.
    // - `file_artifact_store_defines_no_unpinned_currency_oracle` — a
    //   *definition-shape* guard: any `FileArtifactStore` method with a
    //   canonical-only parameter (no content-hash pin), a singular
    //   `Option<...>` return, that scans `self.artifacts`, is a
    //   currency oracle and is banned at the definition site. Only the
    //   two intentional low-level escapes (`get_any` /
    //   `get_artifacts_any`, themselves guarded at every call site) are
    //   allowlisted.
    // ──────────────────────────────────────────────────────────────

    /// Method names that ARE the content-agnostic currency-oracle
    /// shape and are intentionally retained as low-level escapes. Their
    /// every call site is independently guarded by
    /// [`no_direct_file_artifact_get_any_outside_allowlist`] +
    /// `tests/cases/g_misc3/structural_carrier_no_get_any_guard.rs`.
    const CURRENCY_ORACLE_DEFINITION_ALLOWLIST: &[&str] = &["get_any", "get_artifacts_any"];

    /// Banned named currency oracles — a canonical-only
    /// `Option`-returning `FileArtifactStore` accessor cannot be
    /// content-pinned, so it must not exist in production use. The set
    /// is empty of production callers after the named content-agnostic
    /// currency oracles (`content_hash_for_canonical` /
    /// `latest_artifacts_for_canonical`) were removed; this guard keeps
    /// it that way.
    const BANNED_NAMED_CURRENCY_ORACLES: &[&str] = &[
        ".content_hash_for_canonical(",
        ".latest_artifacts_for_canonical(",
    ];

    /// No `verter_session` production file calls a banned named
    /// currency oracle. There is no allowlist — a canonical-only
    /// `Option<Hash16>` / `Option<Arc<FileArtifacts>>` accessor is
    /// unpinnable by construction; a caller needing current identity
    /// uses the scheduler authority, a caller needing artifacts uses an
    /// exact key.
    #[test]
    fn no_named_currency_oracle_calls_in_production() {
        let files = verter_session_production_rs_files();
        let mut violations: Vec<String> = Vec::new();
        for (path, rel) in &files {
            let src =
                fs::read_to_string(path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
            let stripped = strip_comments(&src);
            for banned in BANNED_NAMED_CURRENCY_ORACLES {
                if stripped.contains(banned) {
                    violations.push(format!("{rel}: calls `{banned}`"));
                }
            }
        }
        violations.sort();
        assert!(
            violations.is_empty(),
            "named content-agnostic currency-oracle calls found in \
             production:\n  {}\n\nA canonical-only `Option`-returning \
             `FileArtifactStore` accessor cannot be content-pinned — with \
             lazy cache invalidation it can surface a stale pre-edit \
             artifact. Resolve current identity through the scheduler \
             authority (`authoritative_current_content_hash`); read \
             artifacts through an exact `FileArtifactKey`.",
            violations.join("\n  "),
        );
    }

    /// Extract the brace-balanced body (including the outer braces) of
    /// the first `pub fn <name>` / `pub(crate) fn <name>` whose
    /// signature begins at `decl_start`. Returns
    /// `(signature, body, end_offset)` where `signature` is the text
    /// from `fn` to the opening brace and `end_offset` is the index in
    /// `src` one past the closing brace.
    fn balanced_fn_after(src: &str, decl_start: usize) -> Option<(String, String, usize)> {
        let after = &src[decl_start..];
        let brace_rel = after.find('{')?;
        let signature = after[..brace_rel].trim().to_string();
        let bytes = after.as_bytes();
        let mut depth = 0usize;
        let mut idx = brace_rel;
        while idx < bytes.len() {
            match bytes[idx] {
                b'{' => depth += 1,
                b'}' => {
                    depth -= 1;
                    if depth == 0 {
                        return Some((
                            signature,
                            after[brace_rel..=idx].to_string(),
                            decl_start + idx + 1,
                        ));
                    }
                }
                _ => {}
            }
            idx += 1;
        }
        None
    }

    /// Extract the method parameter list — the text between the first
    /// `(` after the `fn` keyword and its **brace-matching** `)`. This
    /// is robust to a `where R: Fn(&str) -> T` clause, whose inner
    /// parentheses would otherwise confuse a `rfind(')')`-based span.
    /// Returns `(params, after_params)` where `after_params` is the
    /// signature tail (return arrow + `where` clause).
    fn split_signature_params(signature: &str) -> Option<(String, String)> {
        let open = signature.find('(')?;
        let bytes = signature.as_bytes();
        let mut depth = 0usize;
        let mut idx = open;
        while idx < bytes.len() {
            match bytes[idx] {
                b'(' => depth += 1,
                b')' => {
                    depth -= 1;
                    if depth == 0 {
                        return Some((
                            signature[open + 1..idx].to_string(),
                            signature[idx + 1..].to_string(),
                        ));
                    }
                }
                _ => {}
            }
            idx += 1;
        }
        None
    }

    /// True when `signature` (the `fn name(params) -> Ret` slice) is a
    /// canonical-only accessor: it takes a `&str` parameter whose name
    /// contains `canonical` and carries NO content-hash pin parameter
    /// (`Hash16` or a parameter whose name contains `content_hash` /
    /// `whole_hash` / `parse_stable_hash`). A `&FileArtifactKey`
    /// parameter IS a content-pin (the exact key carries the content
    /// hash) — `FileArtifactKey` is recognised as a pin.
    fn is_canonical_only_signature(signature: &str) -> bool {
        let Some((params, _after)) = split_signature_params(signature) else {
            return false;
        };
        let takes_canonical_str = params.contains("canonical") && params.contains("&str");
        let has_hash_pin = params.contains("Hash16")
            || params.contains("FileArtifactKey")
            || params.contains("content_hash")
            || params.contains("whole_hash")
            || params.contains("parse_stable_hash")
            || params.contains("discriminator");
        takes_canonical_str && !has_hash_pin
    }

    /// True when `signature`'s return type is a singular `Option<...>`
    /// (NOT `Vec<...>` — a full enumeration is a legitimate
    /// canonical-wide scan, not a currency oracle). The return type is
    /// read from the signature tail AFTER the brace-matched parameter
    /// list, so an arrow inside a `where R: Fn(..) -> T` clause is not
    /// mistaken for the method's own return type. A `where`-clause-only
    /// tail with no top-level `->` (a `()`-returning method) is not an
    /// `Option` return.
    fn returns_singular_option(signature: &str) -> bool {
        let Some((_params, after)) = split_signature_params(signature) else {
            return false;
        };
        // The method return type, if any, is the `-> ...` segment
        // BEFORE any `where` clause. `where` introduces generic bounds
        // (which may themselves contain `-> ` inside `Fn(..) -> T`).
        // Split on the `where` keyword as a whitespace-delimited word
        // (the clause may be newline-separated from the param list).
        let where_at = after
            .match_indices("where")
            .find(|(i, _)| {
                let before_ok = after[..*i]
                    .chars()
                    .next_back()
                    .is_none_or(char::is_whitespace);
                let after_ok = after[i + 5..]
                    .chars()
                    .next()
                    .is_none_or(char::is_whitespace);
                before_ok && after_ok
            })
            .map(|(i, _)| i);
        let head = match where_at {
            Some(w) => &after[..w],
            None => &after,
        };
        match head.split_once("->") {
            Some((_, ret)) => {
                let ret = ret.trim();
                ret.starts_with("Option<") && !ret.contains("Vec<")
            }
            None => false,
        }
    }

    /// Scan a `FileArtifactStore`-method `(signature, body)` pair and
    /// classify it: a method is an **unpinned currency oracle** when it
    /// is canonical-only ([`is_canonical_only_signature`]), returns a
    /// singular `Option` ([`returns_singular_option`]), and its body
    /// iterates `self.artifacts` directly (`self.artifacts.iter()`) or
    /// delegates to another `self.<method>(` that does.
    fn is_unpinned_currency_oracle(signature: &str, body: &str) -> bool {
        if !is_canonical_only_signature(signature) || !returns_singular_option(signature) {
            return false;
        }
        // The body must touch the artifact collection — either a direct
        // scan or a delegation to a sibling accessor. A direct
        // `self.artifacts.iter()` is the scan; a `self.get_artifacts_any(`
        // / `self.get_any(` delegation inherits the scan.
        body.contains("self.artifacts.iter()")
            || body.contains("self.get_artifacts_any(")
            || body.contains("self.get_any(")
    }

    /// Every `pub` / `pub(crate)` method defined inside `impl
    /// FileArtifactStore` in `file_artifact_store.rs`, as
    /// `(name, signature, body)` triples. Methods on other `impl`
    /// blocks in the same file (`FileArtifactKey`, `FileArtifacts`,
    /// `AugmenterEntry`, …) are excluded.
    fn file_artifact_store_methods() -> Vec<(String, String, String)> {
        let root = super::super::workspace_root();
        let path = root.join("crates/verter_session/src/file_artifact_store.rs");
        let src =
            fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
        let stripped = strip_comments(&src);
        // Bound the scan to the `impl FileArtifactStore {` block.
        let impl_start = stripped
            .find("impl FileArtifactStore {")
            .expect("file_artifact_store.rs must define `impl FileArtifactStore`");
        // Find the matching close brace of the impl block.
        let bytes = stripped.as_bytes();
        let block_open = impl_start + stripped[impl_start..].find('{').unwrap();
        let mut depth = 0usize;
        let mut idx = block_open;
        let mut impl_end = stripped.len();
        while idx < bytes.len() {
            match bytes[idx] {
                b'{' => depth += 1,
                b'}' => {
                    depth -= 1;
                    if depth == 0 {
                        impl_end = idx;
                        break;
                    }
                }
                _ => {}
            }
            idx += 1;
        }
        let impl_body = &stripped[block_open..=impl_end];

        let mut out: Vec<(String, String, String)> = Vec::new();
        let mut cursor = 0usize;
        while let Some(rel) = impl_body[cursor..].find("fn ") {
            let fn_kw = cursor + rel;
            // Require the `fn` to be a method declaration: preceded
            // (modulo whitespace) by `pub`, `pub(crate)`, `const`,
            // `unsafe`, or be a bare `fn`. We only care about `pub` /
            // `pub(crate)` methods — private helpers are not API.
            let prefix = &impl_body[..fn_kw];
            let is_pub =
                prefix.trim_end().ends_with("pub") || prefix.trim_end().ends_with("pub(crate)");
            // Method name: the identifier right after `fn `.
            let name_start = fn_kw + 3;
            let name: String = impl_body[name_start..]
                .chars()
                .take_while(|c| c.is_alphanumeric() || *c == '_')
                .collect();
            if let Some((signature, body, end_offset)) = balanced_fn_after(impl_body, fn_kw) {
                // Advance past this fn body before the (possible) move
                // of `signature` / `body` into `out`.
                cursor = end_offset;
                if is_pub && !name.is_empty() {
                    out.push((name, signature, body));
                }
            } else {
                cursor = fn_kw + 3;
            }
        }
        out
    }

    /// Definition-shape guard — `FileArtifactStore` defines NO unpinned
    /// currency-oracle method outside the intentional-escape allowlist.
    ///
    /// This catches the *next* `content_hash_for_canonical` regardless
    /// of the name it is given: the ban is on the shape (canonical-only
    /// parameter + singular `Option` return + `self.artifacts` scan),
    /// not a name list.
    #[test]
    fn file_artifact_store_defines_no_unpinned_currency_oracle() {
        let methods = file_artifact_store_methods();
        // Sanity: the scan found a non-trivial method surface — guards
        // against a parser regression silently passing vacuously.
        assert!(
            methods.len() > 10,
            "the `impl FileArtifactStore` scan found only {} methods — \
             the parser likely failed; the guard would pass vacuously",
            methods.len(),
        );
        let mut violations: Vec<String> = Vec::new();
        for (name, signature, body) in &methods {
            if CURRENCY_ORACLE_DEFINITION_ALLOWLIST.contains(&name.as_str()) {
                continue;
            }
            if is_unpinned_currency_oracle(signature, body) {
                violations.push(format!("`{name}` — signature `{signature}`"));
            }
        }
        violations.sort();
        assert!(
            violations.is_empty(),
            "unpinned content-agnostic currency oracle(s) defined on \
             `FileArtifactStore`:\n  {}\n\nA canonical-only accessor with a \
             singular `Option` return that scans `self.artifacts` answers \
             \"the current X for this canonical\" — but with lazy cache \
             invalidation a stale pre-edit artifact lingers, so a \
             first-match scan can return it. Either pin the read to a \
             content hash (add a `Hash16` parameter, use an exact \
             `FileArtifactKey`), return a `Vec` (a full enumeration is \
             not a currency oracle), or — for a genuine low-level escape \
             — add the method to `CURRENCY_ORACLE_DEFINITION_ALLOWLIST` \
             AND guard its every call site.",
            violations.join("\n  "),
        );

        // Every allowlisted escape MUST still be defined — a stale
        // allowlist entry (the method was removed / pinned) must be
        // dropped so the allowlist cannot silently grow permission.
        let defined: std::collections::BTreeSet<&str> =
            methods.iter().map(|(n, _, _)| n.as_str()).collect();
        for allowed in CURRENCY_ORACLE_DEFINITION_ALLOWLIST {
            assert!(
                defined.contains(allowed),
                "CURRENCY_ORACLE_DEFINITION_ALLOWLIST entry `{allowed}` is no \
                 longer a defined `FileArtifactStore` method — remove the \
                 stale allowlist entry.",
            );
        }
    }

    /// Discriminating self-test for the definition-shape scanner — it
    /// must flag the currency-oracle shape and clear the pinned /
    /// enumeration / non-scanning shapes. Without this, an
    /// empty-violations result is indistinguishable from a vacuous pass.
    #[test]
    fn currency_oracle_definition_scanner_discriminates() {
        // (a) The exact `content_hash_for_canonical` shape — canonical-
        //     only param, `Option<Hash16>` return, `self.artifacts`
        //     scan → MUST be flagged.
        let oracle_sig = "fn content_hash_for_canonical(&self, canonical: &str) -> Option<Hash16>";
        let oracle_body = "{ for entry in self.artifacts.iter() { if entry.key().canonical.as_ref() == canonical { return Some(entry.key().content_hash); } } None }";
        assert!(
            is_unpinned_currency_oracle(oracle_sig, oracle_body),
            "self-test: the `content_hash_for_canonical` shape MUST be flagged",
        );

        // (a') The `latest_artifacts_for_canonical` delegation shape →
        //      MUST be flagged (delegates to `self.get_artifacts_any(`).
        let alias_sig =
            "fn latest_artifacts_for_canonical(&self, canonical: &str) -> Option<Arc<FileArtifacts>>";
        let alias_body = "{ self.get_artifacts_any(canonical) }";
        assert!(
            is_unpinned_currency_oracle(alias_sig, alias_body),
            "self-test: the `latest_artifacts_for_canonical` delegation MUST be flagged",
        );

        // (b) A content-pinned read — carries a `Hash16` parameter →
        //     MUST NOT be flagged.
        let pinned_sig =
            "fn get(&self, canonical_id: &str, expected_whole_hash: Hash16) -> Option<Arc<IndexedReady>>";
        let pinned_body = "{ let key = FileArtifactKey::base(Arc::from(canonical_id), expected_whole_hash); self.artifacts.get(&key) }";
        assert!(
            !is_unpinned_currency_oracle(pinned_sig, pinned_body),
            "self-test: a content-pinned read (carries a `Hash16` pin) MUST NOT be flagged",
        );

        // (c) A full-enumeration scan — returns `Vec<...>` → MUST NOT
        //     be flagged (a caller wants the whole set, not "current").
        let enum_sig = "fn keys(&self) -> Vec<(Arc<str>, Hash16)>";
        let enum_body = "{ self.artifacts.iter().map(|e| e.key().clone()).collect() }";
        assert!(
            !is_unpinned_currency_oracle(enum_sig, enum_body),
            "self-test: a `Vec`-returning full enumeration MUST NOT be flagged",
        );

        // (d) A canonical-only `Option` accessor that does NOT scan
        //     `self.artifacts` → MUST NOT be flagged (no scan = no
        //     currency-oracle hazard).
        let no_scan_sig = "fn last_access_tick(&self, canonical: &str) -> Option<u64>";
        let no_scan_body = "{ self.last_access.get(canonical).map(|v| *v) }";
        assert!(
            !is_unpinned_currency_oracle(no_scan_sig, no_scan_body),
            "self-test: a canonical-only `Option` accessor that does not scan \
             `self.artifacts` MUST NOT be flagged",
        );

        // (e) An overlay-scoped read — carries a `discriminator`
        //     content-pin parameter → MUST NOT be flagged.
        let overlay_sig = "fn get_overlay_scoped(&self, canonical_id: &str, expected_whole_hash: Hash16, discriminator: Hash16) -> Option<Arc<IndexedReady>>";
        let overlay_body = "{ let key = FileArtifactKey::overlay_scoped(Arc::from(canonical_id), expected_whole_hash, discriminator); self.artifacts.get(&key) }";
        assert!(
            !is_unpinned_currency_oracle(overlay_sig, overlay_body),
            "self-test: an overlay-scoped read (carries a pin) MUST NOT be flagged",
        );

        // (f) The call-site scanner: a banned named-oracle call MUST be
        //     detected; a clean scheduler-authority call MUST NOT.
        let banned_call = "let h = store.content_hash_for_canonical(canonical);";
        assert!(
            BANNED_NAMED_CURRENCY_ORACLES
                .iter()
                .any(|b| banned_call.contains(b)),
            "self-test: a `.content_hash_for_canonical(` call MUST be detected",
        );
        let clean_call = "let h = base.authoritative_current_content_hash(canonical);";
        assert!(
            !BANNED_NAMED_CURRENCY_ORACLES
                .iter()
                .any(|b| clean_call.contains(b)),
            "self-test: a scheduler-authority call MUST NOT be flagged",
        );

        // (g) A `where`-clause method whose `where R: Fn(&str, &str) ->
        //     Option<T>` bound contains both a `&str` and an `-> Option<...>`
        //     → MUST NOT be flagged. The signature parser must not mistake the
        //     `Fn` bound's params / arrow for the method's own. This mirrors
        //     the augmentation-index resolver-hook shape
        //     (`ensure_augmentation_index_populated`).
        let where_clause_sig = "fn resolve_augmenter_set<R>(&self, key: &AugmentationTargetKey, resolve_relative_canonical: R) where R: Fn(&str, &str) -> Option<Arc<str>>";
        assert!(
            !returns_singular_option(where_clause_sig),
            "self-test: a method whose `where` clause carries \
             `Fn(..) -> Option<T>` MUST NOT be read as Option-returning",
        );
        assert!(
            !is_canonical_only_signature(where_clause_sig),
            "self-test: a `&AugmentationTargetKey`-taking method keys on a \
             structured query key, not a bare `&str` canonical — MUST NOT be \
             flagged canonical-only, and the `Fn(&str)` bound's `&str` must \
             not be mistaken for the method's own canonical parameter",
        );
        let where_clause_body = "{ for e in self.artifacts.iter() { } }";
        assert!(
            !is_unpinned_currency_oracle(where_clause_sig, where_clause_body),
            "self-test: the `where`-clause augmentation-index resolver-hook \
             shape MUST NOT be flagged as a currency oracle",
        );
    }
}

// ===========================================================================
// Per-member graph-native materialiser cache wire-up guard.
//
// `surface_member_to_expanded_field` MUST peek the per-member slot of
// `ShapeCacheDb` (indexed by `ShapeSubject::MemberValueNode` via
// `ShapeCacheKey::surface_member_value_whole_with_context`) BEFORE calling
// `raise_node_to_type_expr`. The guard pins the contract so a future
// refactor cannot accidentally swap the peek-before-raise wire-up for
// a raise-then-reduce wrapper (which would re-introduce the per-member
// regression characterised when this contract was first locked down).
//
// Discriminating property: the source-grep for the helper call name
// (`member_shape_peek_or_compute`) must occur BEFORE any
// `raise_node_to_type_expr` call inside the function body. The body
// is extracted by literal string slicing rather than parsing — a
// rename of the helper (or an inversion of the peek/raise order)
// fails the guard.
// ===========================================================================

#[test]
fn surface_member_field_consults_member_shape_cache_before_round_trip() {
    // `surface_member_to_expanded_field` is the HIGH-LEVEL publication API on
    // the terminal `output_sink` sink module (it unwraps a sealed carrier
    // through the module-private boundary primitive and returns an
    // `ExpandedField` DTO), so the peek-before-raise contract is anchored there.
    let source =
        read_workspace_file("crates/verter_session/src/meta_resolve/projectors/output_sink.rs");

    // Extract the surface_member_to_expanded_field body via brace
    // matching from the signature marker through the function close.
    let fn_marker = "pub(crate) fn surface_member_to_expanded_field(";
    let fn_start = source
        .find(fn_marker)
        .expect("surface_member_to_expanded_field must exist in projectors/output_sink.rs");
    // Find the opening brace of the function body.
    let open_brace_offset = source[fn_start..]
        .find(") -> ExpandedField {")
        .expect("function signature must terminate at `) -> ExpandedField {`")
        + fn_start;
    let body_start = open_brace_offset + ") -> ExpandedField {".len();
    // Brace-match to find the function's closing brace.
    let bytes = source.as_bytes();
    let mut depth: i32 = 1;
    let mut idx = body_start;
    while idx < bytes.len() {
        match bytes[idx] {
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    break;
                }
            }
            _ => {}
        }
        idx += 1;
    }
    assert!(
        depth == 0,
        "must find the closing brace of surface_member_to_expanded_field"
    );
    let body = &source[body_start..idx];

    // Strip line comments (`//...`) so the guard does not match
    // example-form mentions inside docstrings. We deliberately do NOT
    // strip block comments (`/* */`) — none exist in this file's
    // function body — and we deliberately do NOT parse Rust tokens
    // since a structural parse would mask the literal-call check we
    // are trying to perform.
    let mut stripped = String::with_capacity(body.len());
    for line in body.lines() {
        if let Some(comment_idx) = line.find("//") {
            stripped.push_str(&line[..comment_idx]);
        } else {
            stripped.push_str(line);
        }
        stripped.push('\n');
    }
    let body = stripped.as_str();

    // (1) The peek-before-raise helper must be invoked inside the body.
    let cache_call_offset = body
        .find("member_shape_peek_or_compute(")
        .unwrap_or_else(|| {
            panic!(
                "surface_member_to_expanded_field MUST call \
             `member_shape_peek_or_compute(...)` for the type reduction path \
             (Block 6.d wire-up). A future refactor that bypasses the cache \
             will re-introduce the +52% regression Block 6.c surfaced."
            )
        });

    // (2) Any `raise_node_to_type_expr(member.value)` call MUST occur
    // AFTER the cache peek. The exactness path's
    // `resolve_member_value_for_classification` call is allowed
    // anywhere; only the literal raise-of-member.value is restricted
    // (the exactness path does not call raise_node_to_type_expr on
    // member.value).
    let raise_of_member_value = body.find("raise_node_to_type_expr(member.value)");
    if let Some(raise_offset) = raise_of_member_value {
        assert!(
            cache_call_offset < raise_offset,
            "surface_member_to_expanded_field MUST peek `member_shape_peek_or_compute` \
             BEFORE any `raise_node_to_type_expr(member.value)` call (Block 6.d \
             contract). The current order has the raise at offset {raise_offset} \
             and the cache peek at offset {cache_call_offset}.",
        );
    }
}

/// STRUCTURAL guard for the augmentation-index under-invalidation class
/// AND the artifact retention lease.
///
/// Every path that removes an entry from `self.artifacts` in
/// `file_artifact_store.rs` MUST route through the single retirement
/// chokepoint `retire_artifact_keys`, which is the only site allowed to
/// call `self.artifacts.remove(...)` / `.retain(...)` / `.drain(...)` /
/// `.swap_remove(...)` / `.pop(...)` / `.clear(...)`. The chokepoint
/// always (a) moves the removed version into the retired chain under a
/// fresh membership epoch, so no `FileArtifactRoot` loses reachability
/// to a world it captured, and (b) collects the removed entries'
/// augmentation facts and feeds them to the index invalidation under
/// that SAME epoch. Both are therefore impossible to bypass by
/// construction.
///
/// `self.artifacts.clear()` has NO exception any more: a whole-store
/// reset (the schema-mismatch path) is a retirement like every other
/// removal — clearing the map would free versions a live root still
/// addresses. That is a STRENGTHENING of the previous contract, where
/// the schema reset was allowed to clear the map outright.
///
/// This closes both classes as compile-time invariants: a 3rd, 4th, …
/// future removal site that bypasses the chokepoint fails this guard
/// instead of silently reintroducing the round-6 P1 under-invalidation
/// bug or silently revoking a captured root's lease.
#[test]
fn artifact_removal_routes_through_single_chokepoint() {
    let src = read_workspace_file("crates/verter_session/src/file_artifact_store.rs");

    let (choke_start, choke_end) = fn_body_span(&src, "retire_artifact_keys");

    // Mutating-removal operations that drop entries from the map.
    let removal_ops = [
        "self.artifacts.remove(",
        "self.artifacts.retain(",
        "self.artifacts.drain(",
        "self.artifacts.swap_remove(",
        "self.artifacts.pop(",
        "self.artifacts.clear(",
    ];
    for op in removal_ops {
        let mut search_from = 0usize;
        while let Some(rel) = src[search_from..].find(op) {
            let at = search_from + rel;
            search_from = at + op.len();
            let inside_chokepoint = at >= choke_start && at < choke_end;
            assert!(
                inside_chokepoint,
                "`{op}` at byte {at} is OUTSIDE the `retire_artifact_keys` \
                 chokepoint (bytes {choke_start}..{choke_end}). Every \
                 `self.artifacts` removal MUST route through that chokepoint so \
                 (a) the removed version is RETIRED rather than freed — a live \
                 `FileArtifactRoot` must keep reaching the world it captured — \
                 and (b) the augmentation index is invalidated for the removed \
                 augmenters, else a stale `AugmenterSet` survives an \
                 artifact-only eviction (round-6 P1 under-invalidation class). \
                 Route this removal through `retire_artifact_keys` / \
                 `drop_artifact_entry`."
            );
        }
    }

    // The chokepoint must actually retain AND invalidate — not a
    // vacuous wrapper in either direction.
    let choke_body = &src[choke_start..choke_end];
    assert!(
        choke_body.contains("invalidate_augmentation_index_at_epoch"),
        "`retire_artifact_keys` MUST call \
         `invalidate_augmentation_index_at_epoch` — the chokepoint exists \
         precisely to make removal and index-invalidation inseparable, under \
         ONE membership epoch."
    );
    let publish_call = choke_body.find("self.publish_retired_version(");
    assert!(
        publish_call.is_some(),
        "`retire_artifact_keys` MUST move the removed version into the \
         retired chain (via `publish_retired_version`) — a removal that frees \
         the payload revokes the lease every live `FileArtifactRoot` holds on \
         its captured world."
    );
    // PUBLISH BEFORE RETRACT. A version MOVES between two maps and a
    // root-relative reader consults them in sequence holding neither, so
    // retracting first opens a window where the version is in NEITHER —
    // and the first-writer-wins `CanonicalView` memo freezes that
    // never-existed world into every request the racing view serves.
    let remove_call = choke_body
        .find("self.artifacts.remove(")
        .expect("the chokepoint performs the removal");
    assert!(
        publish_call.is_some_and(|publish| publish < remove_call),
        "`retire_artifact_keys` MUST publish the retired version BEFORE it \
         removes the live entry — the reverse order lets a concurrent \
         root-relative read find the version in neither map."
    );
    assert!(
        choke_body.contains("reserve_membership_epoch"),
        "`retire_artifact_keys` MUST reserve a retirement epoch, else the \
         retired version has no visibility window and no root can address it."
    );
    // The reservation must outlive the application: releasing it before
    // the last mutation lands would let a capture name a half-applied
    // epoch.
    let reserve_at = choke_body
        .find("reserve_membership_epoch")
        .expect("checked above");
    let release_at = choke_body
        .find("drop(reservation)")
        .expect("`retire_artifact_keys` MUST release its epoch reservation");
    assert!(
        reserve_at < remove_call && remove_call < release_at,
        "the epoch reservation MUST span the whole application — a capture \
         may never name an epoch whose mutation is still in flight."
    );
}

// ─────────────────────────────────────────────────────────────────────────
// Demand-scoped declaration-body lowering: storage-shape guard.
//
// `IndexedReady` is a shallow declaration INDEX plus body locators — never
// a body store: no whole-file `EvalEnv` field, no eagerly lowered
// `TypeDeclBody` storage. Declaration bodies live exclusively in the lazy
// `DeclBodyMemo` (demand-materialised through the scheduler-retained parse
// snapshot); `ShallowFileState` may hold only the memo handle, a per-name
// dependency-EDGE cache (`ClassifiedTypeDeps` — dependency edges only, no
// body product), and the eager macro-producer synthesised `.vue`-default
// value HEADER + its dedicated `LoweredValueDecl` body map.
//
// The shallow symbol STRUCTS (`ShallowTypeSymbol` / `ShallowValueSymbol`)
// are SLIM HEADER views — kind / member-names / type-param-names /
// contributor-count — never lowered-body handles and never body products.
// Body data is read through the memo accessors (`type_decl` / `value_decl`);
// dependency edges through `type_deps`.
// ─────────────────────────────────────────────────────────────────────────
mod lazy_decl_body_storage_guards {
    use std::path::PathBuf;

    fn read_production_source(rel: &str) -> String {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(rel);
        std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("guard must read {}: {e}", path.display()))
    }

    /// Strip `//` line comments and `/* */` block comments (string
    /// literals are irrelevant to the struct bodies scanned here).
    fn strip_comments(src: &str) -> String {
        let mut out = String::with_capacity(src.len());
        let bytes = src.as_bytes();
        let mut i = 0;
        while i < bytes.len() {
            if bytes[i] == b'/' && i + 1 < bytes.len() && bytes[i + 1] == b'/' {
                while i < bytes.len() && bytes[i] != b'\n' {
                    i += 1;
                }
            } else if bytes[i] == b'/' && i + 1 < bytes.len() && bytes[i + 1] == b'*' {
                i += 2;
                while i + 1 < bytes.len() && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                    i += 1;
                }
                i = (i + 2).min(bytes.len());
            } else {
                out.push(bytes[i] as char);
                i += 1;
            }
        }
        out
    }

    /// The brace-balanced body of `struct <name> { ... }` in
    /// comment-stripped `src`. Panics (guard failure) when absent.
    fn struct_body(src: &str, name: &str) -> String {
        let needle = format!("struct {name} {{");
        let start = src
            .find(&needle)
            .unwrap_or_else(|| panic!("guard must find `struct {name}`"));
        let mut depth = 0usize;
        let body_start = start + needle.len();
        for (offset, ch) in src[body_start..].char_indices() {
            match ch {
                '{' => depth += 1,
                '}' => {
                    if depth == 0 {
                        return src[body_start..body_start + offset].to_string();
                    }
                    depth -= 1;
                }
                _ => {}
            }
        }
        panic!("guard must find the closing brace of `struct {name}`");
    }

    #[test]
    fn no_indexed_ready_eval_env_or_type_decl_body_storage() {
        // ── IndexedReady: index + locators, never a body store ──
        let store_src = strip_comments(&read_production_source("src/project_type_store.rs"));
        let indexed = struct_body(&store_src, "IndexedReady");
        assert!(
            indexed.contains("shallow_state"),
            "anti-vacuity: the extracted IndexedReady body carries its \
             known fields"
        );
        for forbidden in ["eval_env", "EvalEnv", "TypeDeclBody"] {
            assert!(
                !indexed.contains(forbidden),
                "`IndexedReady` must not store `{forbidden}` — it is a \
                 shallow declaration index plus body locators; declaration \
                 bodies are owned by the lazy `DeclBodyMemo` and lower on \
                 first semantic demand"
            );
        }

        // ── ShallowFileState: bodies only behind the memo; deps in a
        //    dedicated dependency-edge cache ──
        let state_src = strip_comments(&read_production_source(
            "src/resolver_core/shallow_file_state.rs",
        ));
        let state = struct_body(&state_src, "ShallowFileState");
        assert!(
            state.contains("decl_bodies"),
            "`ShallowFileState` must own the lazy `DeclBodyMemo` handle — \
             it is the sole declaration-body authority"
        );
        assert!(
            !state.contains("EvalEnv"),
            "`ShallowFileState` must not store a whole-file `EvalEnv` — \
             the env is a demand product of the memo (`whole_env()`)"
        );
        // The per-name dependency-edge cache stores `ClassifiedTypeDeps`
        // (dependency edges ONLY, never a body product).
        assert!(
            state.contains("ClassifiedTypeDeps"),
            "anti-vacuity: `ShallowFileState` must own the
             `ClassifiedTypeDeps` dependency-edge cache"
        );
        // Header fields live in the shallow input assembly the worker owns.
        // Scan both parts of the worker shape; the assembly may own syntax
        // facts but never the worker's body/cache authority.
        let inputs_src = strip_comments(&read_production_source(
            "../verter_session_query/src/inputs/shallow.rs",
        ));
        let inputs = struct_body(&inputs_src, "ShallowInputAssembly");
        assert!(
            state.contains("inputs: ShallowInputAssembly"),
            "worker owns the shallow input assembly"
        );
        assert!(
            inputs.contains("headers") && inputs.contains("synthesised_value_symbols"),
            "pure record retains the header inventory"
        );
        for forbidden in [
            "DeclBodyMemo",
            "LoweredTypeDecl",
            "LoweredValueDecl",
            "EvalEnv",
            "DashMap",
            "TypeDeclBody",
        ] {
            assert!(
                !inputs.contains(forbidden),
                "ShallowInputAssembly must not carry worker/body authority `{forbidden}`"
            );
        }
        let state_and_inputs = format!("{state},{inputs}");
        // Fields split on depth-0 commas (types span multiple lines).
        let mut fields: Vec<String> = Vec::new();
        let mut nesting = 0i32;
        let mut current = String::new();
        for ch in state_and_inputs.chars() {
            match ch {
                '<' | '(' | '[' | '{' => nesting += 1,
                '>' | ')' | ']' | '}' => nesting -= 1,
                ',' if nesting == 0 => {
                    fields.push(std::mem::take(&mut current));
                    continue;
                }
                _ => {}
            }
            current.push(ch);
        }
        if !current.trim().is_empty() {
            fields.push(current);
        }
        // No body-backed `materialized_*` shallow-symbol cache may exist:
        // the slim symbols carry no body, so caching them as bodies is a
        // category error. The only `Shallow*Symbol`-typed field permitted
        // is the eager synthesised `.vue`-default HEADER record
        // (`synthesised_value_symbols`).
        let mut symbol_fields = 0usize;
        for field in &fields {
            let field_name = field
                .split(':')
                .next()
                .map(|n| n.trim().trim_start_matches("pub ").trim())
                .unwrap_or_default()
                .to_string();
            assert!(
                !field_name.starts_with("materialized_"),
                "`ShallowFileState` must NOT carry a body-backed \
                 `materialized_*` shallow-symbol cache; found `{field_name}` \
                 — the slim header symbols own no body, dependency edges live \
                 in the `ClassifiedTypeDeps` cache, bodies in the memo"
            );
            if field.contains("ShallowTypeSymbol") || field.contains("ShallowValueSymbol") {
                symbol_fields += 1;
                assert!(
                    field_name == "synthesised_value_symbols",
                    "the only `Shallow*Symbol`-typed `ShallowFileState` field \
                     permitted is the synthesised `.vue`-default header record \
                     (`synthesised_value_symbols`); found `{field_name}`"
                );
            }
        }
        assert!(
            symbol_fields == 1,
            "anti-vacuity: the scan must see exactly the synthesised \
             `.vue`-default header record (found {symbol_fields})"
        );

        // ── Shallow symbol STRUCTS are SLIM HEADER views, never body
        //    stores and never lowered-body handles ──
        //
        // `ShallowTypeSymbol` / `ShallowValueSymbol` must carry ONLY
        // header facts (kind, member/param NAMES, contributor count,
        // provenance flag). They must NOT hold a lowered-body handle
        // (`Arc<Lowered*Decl>`) and must NOT own a body product
        // (`TypeDeclBody`, `FunctionSignature`, `ObjectExpr`, an owned
        // `member_deps` map, a bare `type_annotation` / `signatures`, a
        // `Vec<TypeParam>` / `type_parameters`, `enum_members`). Body data
        // is read through the memo accessors; dependency EDGES live in
        // `ClassifiedTypeDeps`, not inline on the header symbol.
        for (struct_name, required_headers, forbidden_fields) in [
            (
                "ShallowTypeSymbol",
                &[
                    "kind",
                    "type_param_names",
                    "member_names",
                    "contributor_count",
                ][..],
                &[
                    "LoweredTypeDecl",
                    "TypeDeclBody",
                    "Vec<TypeParam>",
                    "type_parameters",
                    "member_deps",
                    "FunctionSignature",
                    "ObjectExpr",
                    "local_deps",
                    "external_deps",
                ][..],
            ),
            (
                "ShallowValueSymbol",
                &[
                    "kind",
                    "object_member_headers",
                    "is_synthesised_component_default",
                ][..],
                &[
                    "LoweredValueDecl",
                    "FunctionSignature",
                    "ObjectExpr",
                    "type_annotation",
                    "signatures",
                    "TypeDeclBody",
                    "Vec<TypeParam>",
                    "enum_members",
                ][..],
            ),
        ] {
            let body = struct_body(&inputs_src, struct_name);
            for forbidden in forbidden_fields {
                assert!(
                    !body.contains(forbidden),
                    "`{struct_name}` must NOT carry `{forbidden}` — it is a \
                     SLIM HEADER view; declaration bodies are owned by the \
                     lazy `DeclBodyMemo` (read through `type_decl`/`value_decl`) \
                     and dependency edges by `ClassifiedTypeDeps` (read through \
                     `type_deps`); found struct body:\n{body}"
                );
            }
            for required in required_headers {
                assert!(
                    body.contains(required),
                    "anti-vacuity: `{struct_name}` must carry the header field \
                     `{required}`; found struct body:\n{body}"
                );
            }
        }

        // ── ClassifiedTypeDeps stores dependency EDGES only, never a body
        //    product ──
        let deps = struct_body(&inputs_src, "ClassifiedTypeDeps");
        assert!(
            deps.contains("local_deps") && deps.contains("external_deps"),
            "anti-vacuity: `ClassifiedTypeDeps` must carry the \
             `local_deps` / `external_deps` dependency edges"
        );
        for forbidden in [
            "LoweredTypeDecl",
            "TypeDeclBody",
            "FunctionSignature",
            "ObjectExpr",
            "member_deps",
        ] {
            assert!(
                !deps.contains(forbidden),
                "`ClassifiedTypeDeps` must store dependency edges ONLY, not \
                 the body product `{forbidden}`"
            );
        }
    }
}

/// R6 content-free synthetic-deepening CACHE identity. The explicit-deepen
/// route roots on the content-free `SyntheticBindingId` via
/// `ShapeCacheKey::synthetic_binding_whole`, NOT on a `value_node` arena
/// ordinal. Three structural pins:
///   (a) the `ShapeSubject::SyntheticBinding` variant carries ONLY
///       `id: SyntheticBindingId` — no `value_node`, no `SemanticNodeId`;
///   (b) the `synthetic_binding_whole` constructor builds
///       `ShapeSubject::SyntheticBinding { id }`;
///   (c) the `ShapeSubject::TypeExpr` structural-hash arm and its raw-TypeExpr
///       classifier are DELETED, so all live non-synthetic subjects are already
///       settled graph nodes and no carrier can fold its `value_node` into a
///       keyed subject.
#[test]
fn synthetic_binding_cache_subject_is_content_free_and_carrier_sealed() {
    let src = read_workspace_file("crates/verter_type_engine/src/component_meta_caches.rs");

    // (a) The `ShapeSubject::SyntheticBinding` variant body is content-free.
    let variant_body = enum_variant_struct_body(&src, "SyntheticBinding");
    // Anti-vacuity: the extractor found the real variant (its `id` field).
    assert!(
        variant_body.contains("id"),
        "guard must extract the real ShapeSubject::SyntheticBinding variant body"
    );
    assert!(
        variant_body.contains("SyntheticBindingId"),
        "the ShapeSubject::SyntheticBinding variant must key on the content-free \
         `SyntheticBindingId`"
    );
    for forbidden in ["value_node", "whole_hash", "content_hash"] {
        assert!(
            !variant_body.contains(forbidden),
            "ShapeSubject::SyntheticBinding must be content-free (R6) — found \
             `{forbidden}` in its field list. The arena ordinal is value-side \
             provenance on the SemanticNodeData::SyntheticBinding carrier, never \
             on the cache subject."
        );
    }
    // The variant must not carry a `SemanticNodeId` (the retired ordinal key).
    // Match at an identifier boundary so a substring of another identifier does
    // not false-positive.
    assert!(
        !variant_body
            .split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
            .any(|tok| tok == "SemanticNodeId"),
        "ShapeSubject::SyntheticBinding must NOT carry a `SemanticNodeId` — the \
         synthetic-deepen identity is the content-free `SyntheticBindingId`, not \
         an arena ordinal."
    );
    // (a.2) The variant RETAINS the module-private `_seal: ConstructionSeal`
    // field — the structural construction seal that blocks an external
    // struct-literal build of `ShapeSubject::SyntheticBinding`. Pin both the
    // field name and its sealing type so an accidental future removal (which
    // would re-open external construction of the synthetic subject) trips RED.
    assert!(
        variant_body
            .split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
            .any(|tok| tok == "_seal"),
        "ShapeSubject::SyntheticBinding must retain its module-private `_seal: \
         ConstructionSeal` field; this guard verifies the source-level seal shape \
         used to block external struct-literal construction."
    );
    assert!(
        variant_body
            .split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
            .any(|tok| tok == "ConstructionSeal"),
        "ShapeSubject::SyntheticBinding's `_seal` must be typed `ConstructionSeal` \
         (the module-private marker) — the seal's type, not just the field name, \
         is what makes the variant non-constructible outside this module."
    );

    // (b) The `synthetic_binding_whole` constructor builds the variant.
    assert!(
        src.contains("fn synthetic_binding_whole"),
        "the content-free synthetic-deepen key constructor \
         `ShapeCacheKey::synthetic_binding_whole` must exist"
    );
    // Whitespace-collapsed so the construction-presence check matches BOTH
    // the bare `{ id }` form and the sealed multi-line form
    // (`ShapeSubject::SyntheticBinding {\n    id,\n    _seal: ConstructionSeal,\n}`)
    // rustfmt produces once the module-private construction-seal marker
    // field is added. The forbidden-token teeth in (a) still pin the
    // variant to the content-free `id` — the seal adds no payload.
    let src_ws_collapsed = src.split_whitespace().collect::<Vec<_>>().join(" ");
    assert!(
        src_ws_collapsed.contains("ShapeSubject::SyntheticBinding { id }")
            || src_ws_collapsed.contains("ShapeSubject::SyntheticBinding { id,"),
        "the synthetic-binding constructor(s) must build \
         `ShapeSubject::SyntheticBinding {{ id, .. }}` (the content-free `id` \
         is the cache key; a module-private `_seal` may follow)"
    );

    // (c) The raw-TypeExpr subject and classifier are deleted. Live callers key
    // already-settled nodes (`MemberValueNode`) or the content-free synthetic
    // identity, so no expression hash can fold a `value_node` into the key.
    assert!(
        !src.contains("ShapeSubject::TypeExpr"),
        "the `ShapeSubject::TypeExpr` structural-hash arm is deleted — a raw \
         `TypeExpr` must never re-enter the ShapeCacheKey subject (the \
         live route keys its already-settled node instead)"
    );
    assert!(
        !src.contains("fn classify_type_expr_shape_subject")
            && !src.contains("type_expr_contains_synthetic_slot_binding")
            && !src.contains("UnkeyableNested"),
        "the retired raw-TypeExpr shape-subject classifier and nested-carrier \
         verdict must stay absent; live cache subjects are graph-native"
    );

    // Self-discrimination: the variant extractor detects a planted
    // `value_node` field — RED if a bare ordinal is re-introduced on the
    // cache subject.
    let synthetic = "enum E { SyntheticBinding { id: SyntheticBindingId, value_node: u64 }, }";
    assert!(
        enum_variant_struct_body(synthetic, "SyntheticBinding").contains("value_node"),
        "scanner self-test: a planted `value_node` field on the variant must be \
         detected"
    );
    // And the boundary-aware `SemanticNodeId` detector trips on a planted ordinal.
    let synthetic_node = "enum E { SyntheticBinding { node: SemanticNodeId }, }";
    assert!(
        enum_variant_struct_body(synthetic_node, "SyntheticBinding")
            .split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
            .any(|tok| tok == "SemanticNodeId"),
        "scanner self-test: a planted `SemanticNodeId` field on the variant must \
         be detected"
    );
    // Self-discrimination for the `_seal` retention check: a variant WITH the
    // seal exposes the `_seal` / `ConstructionSeal` tokens, and a variant
    // WITHOUT it does not — so dropping `_seal` would make the (a.2) assertion
    // trip RED.
    let sealed = "enum E { SyntheticBinding { id: SyntheticBindingId, _seal: ConstructionSeal }, }";
    let sealed_body = enum_variant_struct_body(sealed, "SyntheticBinding");
    assert!(
        sealed_body
            .split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
            .any(|tok| tok == "_seal")
            && sealed_body
                .split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
                .any(|tok| tok == "ConstructionSeal"),
        "scanner self-test: a sealed variant must expose `_seal: ConstructionSeal`"
    );
    let unsealed = "enum E { SyntheticBinding { id: SyntheticBindingId }, }";
    assert!(
        !enum_variant_struct_body(unsealed, "SyntheticBinding")
            .split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
            .any(|tok| tok == "_seal"),
        "scanner self-test: a variant with the `_seal` field DROPPED must FAIL the \
         retention check (proving the (a.2) assertion discriminates a seal removal)"
    );
}

// =============================================================================
// Component-meta hot paths obtain `ScopeShadowing` from the per-scope memo.
//
// `ComponentMetaQueryEngine::scope_shadowing_for_scope(scope) ->
// Arc<ScopeShadowing>` builds ONE shadow set per scope and memoizes it, so the
// Pick/Omit package-root gate (`is_builtin_pick_or_omit`) can probe it O(1) per
// published field. A component-meta hot path that instead builds a fresh
// `ScopeShadowing` per field — folding the scope's local type names ∪
// script-setup type bindings ∪ resolved import-binding keys into a new set each
// time — is O(fields × scope-names) reclone on the publication hot path.
//
// This guard pins that the production `crate::meta_resolve::**` hot paths obtain
// the shadow set from that per-scope memo, with EXACTLY ONE sanctioned
// exception: the genuinely engine-less `None`-arm fallback in
// `project_expr_class_a_via_dispatch_threaded`, which runs only when no
// `ComponentMetaQueryEngine` is threaded in (a transient, engine-less call) and
// therefore has no memo to consult. The allowlist is ARM-PRECISE — an
// unconditional / pre-match direct build in the SAME fn is still flagged.
//
// The detector is a `syn` path/visitor source scan (mirroring the established
// `dispatch_to_type_expr_method_body` family). A plain identifier-count scan
// cannot express the arm-precise allowance, so the per-arm `None`-fallback
// context is tracked structurally.
// =============================================================================
pub(super) mod component_meta_scope_shadowing_memo {
    use syn::visit::Visit;
    use syn::{ExprPath, ItemMod, Type};

    use super::super::{attrs_test_gate, read_workspace_file, workspace_root};

    /// The four `ScopeShadowing` associated constructors a component-meta hot
    /// path must NOT call directly — it must obtain the per-scope
    /// `Arc<ScopeShadowing>` from
    /// `ComponentMetaQueryEngine::scope_shadowing_for_scope` instead.
    pub(in super::super) const SCOPE_SHADOWING_CTORS: &[&str] = &[
        "from_scope_payload",
        "from_host_scope",
        "from_prepared_decl_bundle",
        "empty",
    ];

    /// One non-allowlisted direct `ScopeShadowing` constructor call found in a
    /// `meta_resolve` production source file.
    #[derive(Debug)]
    pub(in super::super) struct Violation {
        pub(in super::super) file: String,
        pub(in super::super) fn_name: String,
        pub(in super::super) method: String,
    }

    impl std::fmt::Display for Violation {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(
                f,
                "{} :: fn `{}` builds `ScopeShadowing::{}(...)` directly",
                self.file, self.fn_name, self.method
            )
        }
    }

    /// Returns the matched constructor when `ident` names one of
    /// [`SCOPE_SHADOWING_CTORS`].
    fn matched_ctor(ident: &str) -> Option<&'static str> {
        SCOPE_SHADOWING_CTORS.iter().copied().find(|c| *c == ident)
    }

    /// Returns the matched constructor name when `path`'s final two segments are
    /// `ScopeShadowing :: <ctor>` for one of [`SCOPE_SHADOWING_CTORS`]. Matches
    /// the fully-qualified
    /// `crate::resolver_core::scope_shadowing::ScopeShadowing::from_host_scope`
    /// form, the module-qualified form, and a bare (use-imported)
    /// `ScopeShadowing::from_host_scope` form, because only the trailing
    /// `ScopeShadowing` + constructor segment pair is inspected.
    fn scope_shadowing_ctor_of_path(path: &syn::Path) -> Option<&'static str> {
        let n = path.segments.len();
        if n < 2 {
            return None;
        }
        if path.segments[n - 2].ident != "ScopeShadowing" {
            return None;
        }
        matched_ctor(&path.segments[n - 1].ident.to_string())
    }

    /// Returns the matched constructor name for a call-position path, covering
    /// BOTH spellings that name the real `ScopeShadowing` type:
    ///
    /// 1. The plain path form `[crate::…::]ScopeShadowing::<ctor>` — the
    ///    `ScopeShadowing` ident is the path's second-to-last segment.
    /// 2. The UFCS / qself form `<[crate::…::]ScopeShadowing>::<ctor>` — the
    ///    `ScopeShadowing` ident is laundered into the path's `qself` type,
    ///    leaving only the `<ctor>` segment in `ep.path`, so the plain-path
    ///    check bails (`segments.len() < 2`). This form evades WITHOUT renaming,
    ///    so it is matched on the qself type's final ident.
    fn scope_shadowing_ctor_of_expr_path(ep: &ExprPath) -> Option<&'static str> {
        if let Some(ctor) = scope_shadowing_ctor_of_path(&ep.path) {
            return Some(ctor);
        }
        let qself = ep.qself.as_ref()?;
        let ctor = matched_ctor(&ep.path.segments.last()?.ident.to_string())?;
        match qself.ty.as_ref() {
            Type::Path(tp)
                if tp
                    .path
                    .segments
                    .last()
                    .is_some_and(|s| s.ident == "ScopeShadowing") =>
            {
                Some(ctor)
            }
            _ => None,
        }
    }

    /// `true` when `pat` is the bare `None` pattern (`Some`/`None` arm of an
    /// `Option` match). syn parses a lone `None` as a `Pat::Ident`; a
    /// path-qualified `Option::None` lands as a `Pat::Path`.
    fn pat_is_none(pat: &syn::Pat) -> bool {
        match pat {
            syn::Pat::Ident(pi) => pi.ident == "None" && pi.subpat.is_none(),
            syn::Pat::Path(pp) => pp.path.segments.last().is_some_and(|s| s.ident == "None"),
            _ => false,
        }
    }

    /// `true` when `expr` is EXACTLY the sanctioned engine-less scrutinee
    /// `engine.as_deref_mut()` — a no-argument `as_deref_mut` method call whose
    /// receiver is the bare `engine` binding (`Expr::Path` `is_ident("engine")`,
    /// no qself), modulo enclosing parens / groups around the scrutinee. This is
    /// the ONE production shape whose `None` arm provably runs with no engine
    /// present, so its `None` arm may host the single sanctioned direct build.
    ///
    /// Any other `engine`-rooted scrutinee — `engine.filter(..)`,
    /// `engine.and_then(..)`, a longer chain, a bare `engine` path, or a field
    /// access (`self.engine` / `other.engine`) — can take its `None` arm with an
    /// engine PRESENT, so it is NOT the sanctioned shape and a direct build in
    /// its `None` arm fires.
    fn expr_is_engine_as_deref_mut(expr: &syn::Expr) -> bool {
        match expr {
            syn::Expr::Paren(p) => expr_is_engine_as_deref_mut(&p.expr),
            syn::Expr::Group(g) => expr_is_engine_as_deref_mut(&g.expr),
            syn::Expr::MethodCall(mc) => {
                mc.method == "as_deref_mut"
                    && mc.args.is_empty()
                    && matches!(
                        mc.receiver.as_ref(),
                        syn::Expr::Path(p) if p.qself.is_none() && p.path.is_ident("engine")
                    )
            }
            _ => false,
        }
    }

    /// Walks a single production source file's AST, tracking the enclosing fn
    /// name, the cfg-test depth (items whose `#[cfg(...)]` ENTAILS test are
    /// skipped via `attrs_test_gate` — the guard is production-only; a bare
    /// `mod tests` with no cfg gate is NOT skipped), the match-nesting depth,
    /// and whether the cursor is inside the `None` arm of a TOP-LEVEL match
    /// whose scrutinee is exactly `engine.as_deref_mut()`, and records every
    /// non-allowlisted direct `ScopeShadowing` constructor call.
    struct Scanner {
        rel_path: String,
        is_dispatch_helpers: bool,
        fn_stack: Vec<String>,
        cfg_test_depth: u32,
        match_depth: u32,
        in_engine_none_arm: bool,
        sanctioned_build_consumed: bool,
        violations: Vec<Violation>,
    }

    impl<'ast> Visit<'ast> for Scanner {
        fn visit_item_mod(&mut self, node: &'ast ItemMod) {
            // A module is test-only ONLY when its `#[cfg(...)]` ENTAILS test
            // (via `attrs_test_gate`). A bare `mod tests` with no cfg gate
            // compiles into production Rust and IS scanned — the module name
            // alone is not a build gate.
            let entered_test = attrs_test_gate(&node.attrs);
            if entered_test {
                self.cfg_test_depth += 1;
            }
            syn::visit::visit_item_mod(self, node);
            if entered_test {
                self.cfg_test_depth -= 1;
            }
        }

        fn visit_item_fn(&mut self, node: &'ast syn::ItemFn) {
            let entered_test = attrs_test_gate(&node.attrs);
            if entered_test {
                self.cfg_test_depth += 1;
            }
            self.fn_stack.push(node.sig.ident.to_string());
            // A fn body starts a fresh match context — save/restore so match
            // state never leaks across a (possibly nested) fn item.
            let saved = (
                self.match_depth,
                self.in_engine_none_arm,
                self.sanctioned_build_consumed,
            );
            self.match_depth = 0;
            self.in_engine_none_arm = false;
            self.sanctioned_build_consumed = false;
            syn::visit::visit_item_fn(self, node);
            self.match_depth = saved.0;
            self.in_engine_none_arm = saved.1;
            self.sanctioned_build_consumed = saved.2;
            self.fn_stack.pop();
            if entered_test {
                self.cfg_test_depth -= 1;
            }
        }

        fn visit_impl_item_fn(&mut self, node: &'ast syn::ImplItemFn) {
            let entered_test = attrs_test_gate(&node.attrs);
            if entered_test {
                self.cfg_test_depth += 1;
            }
            self.fn_stack.push(node.sig.ident.to_string());
            let saved = (
                self.match_depth,
                self.in_engine_none_arm,
                self.sanctioned_build_consumed,
            );
            self.match_depth = 0;
            self.in_engine_none_arm = false;
            self.sanctioned_build_consumed = false;
            syn::visit::visit_impl_item_fn(self, node);
            self.match_depth = saved.0;
            self.in_engine_none_arm = saved.1;
            self.sanctioned_build_consumed = saved.2;
            self.fn_stack.pop();
            if entered_test {
                self.cfg_test_depth -= 1;
            }
        }

        fn visit_item_impl(&mut self, node: &'ast syn::ItemImpl) {
            // A `#[cfg(test)] impl` block is test-only — its methods carry no
            // cfg of their own, so the test gate lives on the impl. Track the
            // cfg-test depth here so inner builds are skipped (the guard is
            // production-only).
            let entered_test = attrs_test_gate(&node.attrs);
            if entered_test {
                self.cfg_test_depth += 1;
            }
            syn::visit::visit_item_impl(self, node);
            if entered_test {
                self.cfg_test_depth -= 1;
            }
        }

        fn visit_item_const(&mut self, node: &'ast syn::ItemConst) {
            // A `#[cfg(test)] const` initializer (e.g. a closure body) is
            // test-only; gate its constructions out of the production scope.
            let entered_test = attrs_test_gate(&node.attrs);
            if entered_test {
                self.cfg_test_depth += 1;
            }
            syn::visit::visit_item_const(self, node);
            if entered_test {
                self.cfg_test_depth -= 1;
            }
        }

        fn visit_item_static(&mut self, node: &'ast syn::ItemStatic) {
            // A `#[cfg(test)] static` initializer is test-only; gate its
            // constructions out of the production scope (parity with const).
            let entered_test = attrs_test_gate(&node.attrs);
            if entered_test {
                self.cfg_test_depth += 1;
            }
            syn::visit::visit_item_static(self, node);
            if entered_test {
                self.cfg_test_depth -= 1;
            }
        }

        // CFG-TEST CONTAINER COVERAGE (80/20). The cfg-test depth gate on the
        // visitors above covers the REALISTIC production container set: `mod`,
        // free `fn`, `impl`-method (`ImplItemFn`), `impl`, top-level `const`, and
        // `static`. Exotic cfg-gated containers — a trait default-method body
        // (`TraitItemFn`) / `TraitItemConst`, an `ImplItemConst`, or a
        // statement-level `#[cfg(test)]` — are NOT depth-gated. That is FP-SAFE: a
        // sanctioned build inside one would OVER-fire (a loud spurious flag on
        // genuinely test-only exotic code), never silently miss a production
        // build. None exist in-tree; the 80/20 ruling bounds further AST-shape
        // coverage to the tracked structural end-state.
        fn visit_expr_match(&mut self, node: &'ast syn::ExprMatch) {
            // The allowlisted construction is the `None` arm of a TOP-LEVEL
            // (depth-0) match whose scrutinee is EXACTLY `engine.as_deref_mut()`
            // — the one production shape whose `None` arm provably runs with no
            // engine present. A nested match (depth >= 1) resets the flag so an
            // inner `None` arm cannot inherit OR re-establish the allowance.
            let top_level = self.match_depth == 0;
            let scrutinee_is_engine = expr_is_engine_as_deref_mut(&node.expr);

            // The scrutinee is evaluated OUTSIDE any arm — clear the flag for it.
            let saved = self.in_engine_none_arm;
            self.in_engine_none_arm = false;
            self.visit_expr(&node.expr);

            self.match_depth += 1;
            for arm in &node.arms {
                if let Some((_, guard)) = &arm.guard {
                    // A guard expression is not the `None`-arm body.
                    self.in_engine_none_arm = false;
                    self.visit_expr(guard);
                }
                self.in_engine_none_arm = top_level && scrutinee_is_engine && pat_is_none(&arm.pat);
                self.visit_expr(&arm.body);
            }
            self.match_depth -= 1;
            self.in_engine_none_arm = saved;
        }

        fn visit_expr_call(&mut self, node: &'ast syn::ExprCall) {
            if self.cfg_test_depth == 0 {
                if let syn::Expr::Path(p) = node.func.as_ref() {
                    if let Some(method) = scope_shadowing_ctor_of_expr_path(p) {
                        let fn_name = self
                            .fn_stack
                            .last()
                            .cloned()
                            .unwrap_or_else(|| "<file-scope>".to_string());
                        // The allowlisted direct construction: the FIRST
                        // engine-less `None`-arm `from_host_scope` fallback inside
                        // either class-A dispatch-threaded sibling — the `TypeExpr`
                        // form (`project_expr_class_a_via_dispatch_threaded`) and the
                        // node form
                        // (`project_expr_class_a_node_via_dispatch_threaded`), which
                        // share the identical `match engine.as_deref_mut()` shadow
                        // gate (memo on `Some`, direct build only on the engine-less
                        // `None`). The allowance covers AT MOST ONE build per
                        // sanctioned fn — a second fires.
                        let in_sanctioned_context = self.is_dispatch_helpers
                            && matches!(
                                fn_name.as_str(),
                                "project_expr_class_a_via_dispatch_threaded"
                                    | "project_expr_class_a_node_via_dispatch_threaded"
                            )
                            && method == "from_host_scope"
                            && self.in_engine_none_arm;
                        let allowlisted = in_sanctioned_context && !self.sanctioned_build_consumed;
                        if in_sanctioned_context {
                            self.sanctioned_build_consumed = true;
                        }
                        if !allowlisted {
                            self.violations.push(Violation {
                                file: self.rel_path.clone(),
                                fn_name,
                                method: method.to_string(),
                            });
                        }
                    }
                }
            }
            syn::visit::visit_expr_call(self, node);
        }
    }

    /// Run the detector over one production source file's text.
    pub(in super::super) fn violations_in(rel_path: &str, src: &str) -> Vec<Violation> {
        let file = syn::parse_file(src).unwrap_or_else(|e| {
            panic!("scope-shadowing memo guard: parse `{rel_path}` failed: {e}")
        });
        let mut scanner = Scanner {
            rel_path: rel_path.to_string(),
            is_dispatch_helpers: rel_path.ends_with("meta_resolve/dispatch_helpers.rs"),
            fn_stack: Vec::new(),
            cfg_test_depth: 0,
            match_depth: 0,
            in_engine_none_arm: false,
            sanctioned_build_consumed: false,
            violations: Vec::new(),
        };
        scanner.visit_file(&file);
        scanner.violations
    }

    /// The production sources in scope: the `meta_resolve` shell module plus
    /// every `.rs` under `meta_resolve/`, EXCLUDING colocated test modules
    /// (`*_tests.rs` / `tests.rs`).
    pub(in super::super) fn production_sources() -> Vec<(String, String)> {
        let root = workspace_root();
        let mut out: Vec<(String, String)> = Vec::new();
        const SHELL: &str = "crates/verter_session/src/meta_resolve.rs";
        out.push((SHELL.to_string(), read_workspace_file(SHELL)));
        // The `meta_resolve` module tree spans the session crate and the type
        // engine it builds on.
        let dirs = [
            root.join("crates/verter_session/src/meta_resolve"),
            root.join("crates/verter_type_engine/src/meta_resolve"),
        ];
        for dir in &dirs {
            assert!(
                dir.is_dir(),
                "meta_resolve root {} is missing",
                dir.display()
            );
        }
        for entry in dirs
            .iter()
            .flat_map(|dir| walkdir::WalkDir::new(dir).into_iter())
            .filter_map(Result::ok)
        {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("rs") {
                continue;
            }
            let rel = path
                .strip_prefix(&root)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/");
            // Production source only — colocated test modules are out of scope.
            if rel.ends_with("_tests.rs") || rel.ends_with("/tests.rs") || rel.contains("/tests/") {
                continue;
            }
            let src = std::fs::read_to_string(path)
                .unwrap_or_else(|e| panic!("scope-shadowing memo guard: read `{rel}` failed: {e}"));
            out.push((rel, src));
        }
        out
    }
}

/// Component-meta hot paths obtain `ScopeShadowing` from the per-scope memo.
///
/// Production component-meta hot paths in `crate::meta_resolve::**` must NOT
/// build a `ScopeShadowing` directly per field — they must obtain the per-scope
/// `Arc<ScopeShadowing>` from
/// `ComponentMetaQueryEngine::scope_shadowing_for_scope`, which builds one
/// shadow set per scope and memoizes it. Building it directly per published
/// field is an O(fields × scope-names) reclone on the publication hot path.
///
/// SCOPE: production source only — `crates/verter_session/src/meta_resolve.rs`
/// and `crates/{verter_session,verter_type_engine}/src/meta_resolve/**/*.rs` (NOT
/// `project_semantic_dispatch/**`, NOT `resolver_core/**`, NOT tests).
///
/// ALLOWLIST: the FIRST engine-less `None`-arm
/// `ScopeShadowing::from_host_scope(ctx, scope)` fallback inside EITHER class-A
/// dispatch-threaded sibling — the `TypeExpr` form
/// (`project_expr_class_a_via_dispatch_threaded`) and the node form
/// (`project_expr_class_a_node_via_dispatch_threaded`), which share the identical
/// `match engine.as_deref_mut()` shadow gate (memo on `Some`, direct build only on
/// the engine-less `None`) — dispatch_helpers.rs. One sanctioned build per sibling.
/// That arm runs ONLY when no `ComponentMetaQueryEngine` is threaded in, so there
/// is no memo to consult and a direct build is correct. The allowance is PRECISE: the
/// `None` arm must belong to a TOP-LEVEL match whose scrutinee is EXACTLY
/// `engine.as_deref_mut()` (enclosing parens / groups around it are fine). Any
/// other `engine`-rooted scrutinee — `engine.filter(..)`, `engine.and_then(..)`,
/// a bare `engine` path, or a field access like `other.engine` — can take its
/// `None` arm with an engine PRESENT, so it is NOT sanctioned. The allowance
/// covers AT MOST ONE build (a second fires) and does not inherit into a nested
/// match — an unconditional / pre-match / second / non-canonical-scrutinee /
/// nested-match build is STILL flagged (proven by the
/// `..._unconditional_dispatch_build_fires`, `..._second_none_arm_build_fires`,
/// `..._non_engine_scrutinee_none_arm_fires`,
/// `..._non_canonical_engine_scrutinee_fires`, and
/// `..._nested_engine_match_none_arm_fires` self-tests).
///
/// ENFORCED SURFACE: a literal `ScopeShadowing::<ctor>` path-call — bare
/// (`ScopeShadowing::from_host_scope`), module-qualified, or fully-qualified
/// (`crate::resolver_core::scope_shadowing::ScopeShadowing::from_host_scope`) —
/// AND the UFCS / qself form `<ScopeShadowing>::<ctor>` (the type ident in the
/// path's `qself`). Both are matched structurally on the `ScopeShadowing` ident
/// plus a `SCOPE_SHADOWING_CTORS` constructor segment; inline `#[cfg(test)]`
/// items are out of scope (the guard is production-only).
///
/// DISCLOSED RESIDUAL (inherent to any name-based syntactic source scanner; NOT
/// enforced). The 80/20 bound leaves THREE residual classes to the structural
/// end-state rather than chasing further AST shapes:
///
/// 1. Identity-laundering forms that do not spell `ScopeShadowing` at the call
///    site — a renamed `use ...ScopeShadowing as SS; SS::from_host_scope(...)`
///    import, a `type SS = ScopeShadowing;` alias, a function-pointer / value
///    capture or other call-form wrapper INCLUDING a parenthesized callee
///    (`let f = ScopeShadowing::from_host_scope; f(...)`,
///    `(ScopeShadowing::from_host_scope)(...)`), and macro-expanded construction.
///    These launder the type identity past the literal path-call match.
///
/// 2. Runtime / control-flow MULTIPLICITY inside the sanctioned `None` arm — the
///    scanner enforces ONE syntactic direct call expression in the allowlisted
///    arm, NOT "at most once at runtime". A single sanctioned call placed inside
///    a loop or a closure within that `None` arm builds many times at runtime
///    while reading as one syntactic call, and is not detected.
///
/// 3. Binding-identity / parameter-shadowing laundering — a deliberately
///    shadowed local `engine` (e.g. a
///    `let engine = None::<&mut ComponentMetaQueryEngine>;` ahead of
///    `match engine.as_deref_mut() { None => ScopeShadowing::from_host_scope(..) }`
///    inside the sanctioned fn) reads syntactically as the engine-less fallback
///    even though it DISCARDS a present engine. A syntactic scanner cannot tell
///    the shadowed local from the unshadowed `engine` parameter without name
///    resolution — the SAME inherent name-scanner class as the aliases / macros
///    of (1).
///
/// Universal confinement of ALL THREE classes belongs to the tracked structural
/// end-state, NOT this scanner: the shared-`ResolverContext` per-scope memo that
/// lets the constructors seal to `pub(in crate::resolver_core)`, recorded in
/// `.claude/skills/type-resolution/SKILL.md`. An OBSERVED such
/// evasion is a laundering escape that freezes the scanner per
/// Structural-Confinement-First and makes that structural migration the required
/// fix.
///
/// ── SC-first guard-local record ────────────────────────────────────────────
/// `scanner_invariant`: `component_meta_hot_path_scope_shadowing_memo_routing`.
///
/// `scanner_justification`: Rust visibility cannot separate the `meta_resolve`
///   hot paths from their legitimate visibility-peers — the engine-less
///   `project_semantic_dispatch` lowering layer, which builds a `ScopeShadowing`
///   once per lowering op (NOT per field) and holds no engine to memoize
///   through. `ScopeShadowing` + its `pub(crate)` constructors, the per-scope
///   memo on `ComponentMetaQueryEngine`, and the hot paths live in three SIBLING
///   top-level modules (`resolver_core::scope_shadowing`,
///   `resolver_core::component_meta_query_engine`, `meta_resolve`); a
///   `pub(in crate::resolver_core)` seal would admit the memo but BREAK the
///   engine-less dispatch builds, and any `pub(crate)` escape hatch the dispatch
///   peer uses, the hot paths can use too. Token / type-state cannot encode
///   "only when `engine` is `None`" or "not per-field": the distinction is
///   semantic / cadence-based (engine-available hot path vs engine-less
///   lowering), not a module boundary — so the compiler cannot express it within
///   these owner boundaries, and a source guard supplements it.
///
/// `mechanism_ruling`: `SCF-SCOPE-SHADOWING-MEMO-2026-06-28` — a neutral codex
///   SC-first architecture ruling, dated 2026-06-28, decided the mechanism is
///   SCANNER-SUPPLEMENTED, not compiler-sealed: route the hot sites through the
///   memo and add this discriminating source guard for the residual. The
///   structural END-STATE the ruling named is a shared request-context
///   (`ResolverContext`) per-scope shadow memo that ALL consumers — including
///   the engine-less `project_semantic_dispatch` builds — consult, after which
///   the constructors can seal to `pub(in crate::resolver_core)` and this guard
///   is no longer needed. That migration is a SEPARATE follow-up, not this
///   guard's debt.
///
/// `hardening_rounds`: 0 (adoption).
///
/// `hardening_history`: adoption under the 2026-06-28 codex SC-first
///   architecture ruling. Pre-land (block-branch, pre-integration) review-driven
///   soundness corrections — the cfg-entailment classification, the bare
///   `mod tests` scan, the exact `engine.as_deref_mut()` scrutinee, and the
///   impl / const / static cfg gating — are ADOPTION SHAPING and do NOT
///   increment `hardening_rounds`; the counter starts once the scanner is
///   integrated / landed (or a landed scanner is later broadened). No laundering
///   escape has occurred.
/// ────────────────────────────────────────────────────────────────────────────
#[test]
fn component_meta_hot_paths_obtain_scope_shadowing_from_the_per_scope_memo() {
    use component_meta_scope_shadowing_memo as guard;
    let mut messages: Vec<String> = Vec::new();
    for (rel, src) in guard::production_sources() {
        for v in guard::violations_in(&rel, &src) {
            // Best-effort source line hint (the workspace's `syn` dev-dep has
            // `proc-macro2/span-locations` off, so the AST carries no line
            // numbers): list the lines whose text bears the constructor call.
            let hint: Vec<usize> = src
                .lines()
                .enumerate()
                .filter(|(_, l)| l.contains(&format!("{}(", v.method)))
                .map(|(i, _)| i + 1)
                .collect();
            messages.push(format!("{v} (construction line(s): {hint:?})"));
        }
    }
    assert!(
        messages.is_empty(),
        "component-meta hot paths in `crate::meta_resolve::**` must obtain \
         `ScopeShadowing` from the per-scope memo \
         (`ComponentMetaQueryEngine::scope_shadowing_for_scope`), not build it \
         directly per field. The ONLY sanctioned direct construction is the \
         engine-less `None`-arm `ScopeShadowing::from_host_scope(ctx, scope)` \
         fallback inside either class-A dispatch-threaded sibling \
         (`project_expr_class_a_via_dispatch_threaded` / \
         `project_expr_class_a_node_via_dispatch_threaded`). \
         Non-allowlisted direct construction(s):\n{}",
        messages.join("\n")
    );
}

#[test]
fn component_meta_hot_paths_self_test_clean_memo_path_passes() {
    // The field-types hot path obtains the shadow set from the memo — no direct
    // build — and the dispatch helper uses the engine-aware match (`Some` arm →
    // memo, `None` arm → the single allowlisted engine-less build). Both PASS.
    let clean_field_types = r#"
        fn materialize_component_meta_type_expr_until_stable_full(
            query_engine: &mut ComponentMetaQueryEngine,
            scope_canonical_id: &str,
        ) {
            let scope_payload = query_engine.scope_payload_for_scope(scope_canonical_id);
            let shadowing = query_engine.scope_shadowing_for_scope(scope_canonical_id);
            let _ = (scope_payload, shadowing);
        }
    "#;
    assert!(
        component_meta_scope_shadowing_memo::violations_in(
            "crates/verter_session/src/meta_resolve/materialize/field_types.rs",
            clean_field_types,
        )
        .is_empty(),
        "the memo-routed field-types hot path must pass"
    );

    let clean_dispatch = r#"
        fn project_expr_class_a_via_dispatch_threaded(
            ctx: &dyn ResolverContext,
            mut engine: Option<&mut ComponentMetaQueryEngine>,
            scope_canonical_id: &str,
        ) {
            let shadowing = match engine.as_deref_mut() {
                Some(e) => e.scope_shadowing_for_scope(scope_canonical_id),
                None => std::sync::Arc::new(
                    crate::resolver_core::scope_shadowing::ScopeShadowing::from_host_scope(
                        ctx,
                        scope_canonical_id,
                    ),
                ),
            };
            let _ = shadowing;
        }
    "#;
    assert!(
        component_meta_scope_shadowing_memo::violations_in(
            "crates/verter_session/src/meta_resolve/dispatch_helpers.rs",
            clean_dispatch,
        )
        .is_empty(),
        "the engine-aware dispatch helper (memo `Some` arm + single allowlisted \
         `None`-arm build) must pass"
    );
}
