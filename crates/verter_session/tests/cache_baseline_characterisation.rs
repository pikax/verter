//! Stage-0 cache baseline characterisation.
//!
//! This test file LOCKS IN today's pre-Stage-1 cache cascade and overlay
//! serialisation behaviour. Every assertion is designed to PASS on the
//! audited base SHA `ccc05223` and to FAIL on a tree where:
//!
//! - `evict_canonical` no longer drains the documented inventory
//!   (Stage 1 / 7 territory), OR
//! - `with_overlay_target_context` no longer invokes `host.upsert` from
//!   query paths (Stage 4d territory), OR
//! - the layered cycle-guard sentinels documented in
//!   `tests/fixtures/cache_baseline/cycle_safety_failure_mode.md`
//!   are replaced by Stage 3's canonical `CycleRef` placeholder.
//!
//! These assertions are NOT inverted here; the inversion lives in
//! Stage 6d's `tests/path_precise_invalidation.rs` and Stage 5's
//! multi-candidate tests. The Stage-0 file is the read-only contract
//! that later stages diff against.
//!
//! Plan citation:
//! `D:/tmp/verter-fact-based-cache-plan.md` §"Stage 0" sub-tasks 1, 2(b), 4.
//! Architectural rules bound: R1, R2, R3, R7, R20, R27.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates/")
        .parent()
        .expect("repo root")
        .to_path_buf()
}

fn read_workspace_file(rel: &str) -> String {
    let path = workspace_root().join(rel);
    fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {}", path.display(), e))
}

fn fixture(rel_in_session: &str) -> PathBuf {
    workspace_root()
        .join("crates")
        .join("verter_session")
        .join(rel_in_session)
}

/// Find the body of `fn evict_canonical(&self, canonical_id: &str)` inside
/// `impl ProjectTypeStore` at the top of `project_type_store.rs`. Returns
/// the body lines between the opening `{` and closing `}` of the function.
///
/// The extraction is deliberately literal: it walks the source line-by-line,
/// locates the `pub fn evict_canonical(&self, canonical_id: &str) {` header
/// inside the `impl ProjectTypeStore` block, then collects lines until the
/// matching closing brace at the same brace-depth.
fn evict_canonical_body(src: &str) -> String {
    let header = "pub fn evict_canonical(&self, canonical_id: &str) {";
    let header_idx = src
        .find(header)
        .expect("evict_canonical header signature must exist on the audited base SHA");
    // From the opening `{` after the header, walk until the matching `}`.
    let after_open = header_idx + header.len();
    let bytes = src.as_bytes();
    let mut depth: i64 = 1;
    let mut idx = after_open;
    while idx < bytes.len() && depth > 0 {
        match bytes[idx] {
            b'{' => depth += 1,
            b'}' => depth -= 1,
            _ => {}
        }
        idx += 1;
    }
    assert_eq!(
        depth, 0,
        "evict_canonical body must be brace-balanced on the base SHA"
    );
    src[after_open..idx - 1].to_string()
}

/// Strip Rust line-comments (`// …`) and block-comments (`/* … */`) from
/// a snippet so the static drain-extraction does not pick up
/// `self.foo` mentions inside doc-comments.
fn strip_comments(src: &str) -> String {
    let mut out = String::with_capacity(src.len());
    let bytes = src.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if i + 1 < bytes.len() && bytes[i] == b'/' && bytes[i + 1] == b'/' {
            // line comment
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
        } else if i + 1 < bytes.len() && bytes[i] == b'/' && bytes[i + 1] == b'*' {
            // block comment
            i += 2;
            while i + 1 < bytes.len() && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                i += 1;
            }
            i += 2;
        } else {
            out.push(bytes[i] as char);
            i += 1;
        }
    }
    out
}

/// Extract every `self.<field>.<method>` reference inside the (comment-stripped)
/// `evict_canonical` body. Returns the set of `(field, method)` pairs that
/// `evict_canonical` actually invokes.
///
/// The regex is anchored on `self.` followed by an identifier, optional whitespace,
/// optional newline, optional `.lock()` or `.` chain, and a method name.
fn extract_drained_dbs(body: &str) -> BTreeSet<(String, String)> {
    // Two patterns to handle:
    //   self.field.method(canonical_id);
    //   self.field
    //       .method(canonical_id);
    //   self.field.lock().method(canonical_id);   (semantic_db)
    //
    // Strategy: split on `self.`, then for each fragment after the first,
    // parse `<field>[\s\n]*\.[\s\n]*<method>` (with optional `.lock()`
    // between).
    let cleaned = strip_comments(body);
    let mut out: BTreeSet<(String, String)> = BTreeSet::new();
    let pat = regex::Regex::new(
        r"self\.([a-zA-Z_][a-zA-Z0-9_]*)[\s\n]*\.[\s\n]*(?:lock\(\)[\s\n]*\.[\s\n]*)?([a-zA-Z_][a-zA-Z0-9_]*)\s*\(",
    )
    .expect("regex compiles");
    for cap in pat.captures_iter(&cleaned) {
        let field = cap[1].to_string();
        let method = cap[2].to_string();
        // The `lock` of `self.semantic_db.lock()` would otherwise be captured
        // as method; we use a non-capturing `lock()` prefix on the regex.
        // For semantic_db the actual method name is `invalidate` after the
        // lock; mirror the inventory's recorded `lock().invalidate`.
        if field == "semantic_db" && method == "invalidate" {
            out.insert((field, "lock().invalidate".to_string()));
        } else {
            out.insert((field, method));
        }
    }
    out
}

/// Parse the committed `evict_canonical_inventory.json` and return the set of
/// `(field, method)` pairs it claims `evict_canonical` drains.
fn inventory_drained_dbs() -> BTreeSet<(String, String)> {
    let path = fixture("tests/fixtures/cache_baseline/evict_canonical_inventory.json");
    let raw = fs::read_to_string(&path).expect("read inventory JSON");
    let parsed: serde_json::Value = serde_json::from_str(&raw).expect("parse inventory JSON");
    let entries = parsed
        .get("databases_drained")
        .and_then(|v| v.as_array())
        .expect("databases_drained array");
    let mut out: BTreeSet<(String, String)> = BTreeSet::new();
    for entry in entries {
        let field = entry
            .get("field")
            .and_then(|v| v.as_str())
            .expect("field");
        let method = entry
            .get("method")
            .and_then(|v| v.as_str())
            .expect("method");
        out.insert((field.to_string(), method.to_string()));
    }
    out
}

/// CHARACTERISATION 1 — `evict_canonical` drain inventory matches the
/// committed JSON.
///
/// This test PASSES on the audited base SHA (the committed JSON reflects
/// today's behaviour); it FAILS if the cascade ever gains/loses a DB drain
/// without the inventory being updated, OR if the cascade is deleted (Stage 7).
#[test]
fn evict_canonical_drain_inventory_matches_source_body() {
    let src = read_workspace_file("crates/verter_session/src/project_type_store.rs");
    let body = evict_canonical_body(&src);
    let extracted = extract_drained_dbs(&body);
    let committed = inventory_drained_dbs();

    // Discriminating: today the cascade drains exactly 21 (field, method)
    // pairs (semantic_graph contributes two distinct method calls; see the
    // inventory JSON's `databases_drained`). A stub that returned an empty
    // set would not match.
    assert!(
        extracted.len() >= 20,
        "evict_canonical body must drain ≥ 20 (field, method) pairs on the audited base \
         SHA — got {} ({:?}). If the cascade was thinned out, update \
         tests/fixtures/cache_baseline/evict_canonical_inventory.json in the same \
         commit and document the change in plan §Stage 7.",
        extracted.len(),
        extracted
    );

    let missing_from_inventory: Vec<_> = extracted.difference(&committed).cloned().collect();
    let missing_from_source: Vec<_> = committed.difference(&extracted).cloned().collect();

    assert!(
        missing_from_inventory.is_empty(),
        "evict_canonical body drains entries that the committed inventory does NOT list: {:?}. \
         Update tests/fixtures/cache_baseline/evict_canonical_inventory.json.",
        missing_from_inventory
    );
    assert!(
        missing_from_source.is_empty(),
        "Committed inventory lists entries the evict_canonical body does NOT drain: {:?}. \
         Either restore the drain or remove the entry from \
         tests/fixtures/cache_baseline/evict_canonical_inventory.json.",
        missing_from_source
    );
}

/// CHARACTERISATION 2 — the cascade still contains the load-bearing
/// project-global DB drains that Stage 1+ tests will assert ARE deleted
/// in Stage 7.
///
/// These specific DBs are named by the plan's Cache layers + Legacy
/// Deletions tables — they are the ones whose drain semantics change
/// under the fact-based model. The test pins their PRESENCE today;
/// Stage 7's test will pin their ABSENCE.
#[test]
fn evict_canonical_cascade_contains_load_bearing_dbs_today() {
    let src = read_workspace_file("crates/verter_session/src/project_type_store.rs");
    let body = evict_canonical_body(&src);
    let extracted = extract_drained_dbs(&body);

    // These are the specific drains the plan calls out by name in its
    // "Legacy Deletions" table under Stage 7.
    let must_be_present: &[(&str, &str)] = &[
        // R5 → FileArtifactStore replaces IndexedReadyDb at Stage 1
        ("indexed", "remove"),
        // R7 → MaterializeStructureDb rekey at Stage 5 Sub-task D
        ("materialize_structure_db", "invalidate_for_canonical"),
        // R3 → public invalidate_canonical surface deleted at Stage 7
        ("semantic_graph", "invalidate_canonical"),
        ("component_meta_results", "invalidate_owner"),
        // R8 (owner-keyed payloads only)
        ("owner_import_surfaces", "remove"),
        // Per-canonical engine caches deleted at Stage 7
        ("imported_registry_db", "invalidate_canonical"),
        ("declaration_lookup_db", "invalidate_canonical"),
        ("ref_cycle_db", "invalidate_for_canonical"),
        // Semantic-fact cache that the fact-based model replaces
        ("semantic_db", "lock().invalidate"),
    ];

    for (field, method) in must_be_present {
        let key = (field.to_string(), method.to_string());
        assert!(
            extracted.contains(&key),
            "Stage-0 baseline: evict_canonical MUST drain self.{}.{}(...) on the audited \
             base SHA, but the extracted body does not contain that call. This characterisation \
             pins the pre-Stage-7 cascade so the cutover commit reduces the set deliberately, \
             not by accident. Extracted set:\n{:#?}",
            field,
            method,
            extracted
        );
    }
}

/// CHARACTERISATION 3 — `with_overlay_target_context` invokes
/// `host.upsert` from query paths today (the multi-candidate proxy).
///
/// Per the plan's R17 / R20: today's overlay model serialises concurrent
/// sessions via CAS, with each ownership rotation forcing
/// `host.upsert(base_or_overlay)` calls. Stage 4d deletes this. The
/// pre-Stage-4d invariant is "the CAS path WRITES to the host."
#[test]
fn overlay_path_today_calls_host_upsert_from_query_path() {
    let session_runtime_src =
        read_workspace_file("crates/verter_session/src/session_runtime.rs");
    let meta_src = read_workspace_file("crates/verter_session/src/meta.rs");

    // The three methods documented in plan §"Legacy Deletions" under
    // Stage 4d.
    let must_call_upsert = ["apply_own_overlays", "revert_other_session_overlays", "reapply_overlay_target"];

    for method in must_call_upsert {
        let body = extract_method_body(&session_runtime_src, method)
            .unwrap_or_else(|| {
                panic!(
                    "Stage-0 baseline: SessionRuntime::{} must exist on the audited base SHA \
                     so Stage 4d can delete it as documented in the Legacy Deletions table.",
                    method
                )
            });
        let body_stripped = strip_comments(&body);
        assert!(
            body_stripped.contains("self.host().upsert(") || body_stripped.contains("self.host().remove("),
            "Stage-0 baseline: SessionRuntime::{} must invoke self.host().upsert(...) or \
             self.host().remove(...) from a query-path codepath today (the CAS swap loop \
             documented in tests/fixtures/cache_baseline/multi_candidate_proxy.md). Stage 4d \
             deletes this. Body was: {}",
            method,
            body_stripped
        );
    }

    // The CAS atomic itself is the second observable.
    assert!(
        meta_src.contains("active_overlay_session: AtomicU64"),
        "Stage-0 baseline: MetaProject must carry the active_overlay_session AtomicU64 today \
         (the CAS oracle Stage 4d retires)."
    );
    assert!(
        meta_src.contains("compare_exchange(current, self.id"),
        "Stage-0 baseline: MetaSession::with_overlay_target_context must claim the active \
         overlay slot via compare_exchange(current, self.id, …) today."
    );
}

/// CHARACTERISATION 4 — the layered cycle-guard mechanism is present
/// today.
///
/// Per `cycle_safety_failure_mode.md`, today's cycle handling is layered:
/// (1) policy walker active_refs, (2) MaterializeInFlightGuard same-key
/// re-entry, (3) MAX_DEPTH defensive fuse. Stage 3 replaces all three
/// with R27's stack-safe explicit worklist + CycleRef placeholder.
///
/// This test pins the three layers exist today so Stage 3 deliberately
/// removes them (not silently).
#[test]
fn cycle_guard_layers_present_today() {
    let materialize_src =
        read_workspace_file("crates/verter_session/src/component_meta_materialize.rs");

    // Layer 2: MaterializeInFlightGuard same-key re-entry.
    assert!(
        materialize_src.contains("MaterializeInFlightGuard"),
        "Stage-0 baseline: MaterializeInFlightGuard must exist today (layer-2 cycle guard)."
    );
    assert!(
        materialize_src.contains("MaterializeInFlightGuard::contains_key(&key)"),
        "Stage-0 baseline: same-key thread-local re-entry detection must call \
         MaterializeInFlightGuard::contains_key today."
    );
    assert!(
        materialize_src.contains("MaterializeOutcome::Recursive("),
        "Stage-0 baseline: same-key re-entry must produce MaterializeOutcome::Recursive(...) today."
    );

    // Layer 3: MAX_DEPTH defensive fuse with the documented value.
    assert!(
        materialize_src.contains("pub const MAX_DEPTH: usize = 4096"),
        "Stage-0 baseline: MAX_DEPTH = 4096 defensive fuse must be in place today."
    );
    assert!(
        materialize_src.contains("MaterializeInFlightGuard::current_depth() >= MAX_DEPTH"),
        "Stage-0 baseline: depth fuse must compare current_depth() to MAX_DEPTH today."
    );
    assert!(
        materialize_src.contains("MaterializeOutcome::Tainted("),
        "Stage-0 baseline: depth-fuse trip must produce MaterializeOutcome::Tainted today."
    );

    // The recursive type alias tests that prove the layered guard works
    // today must be present.
    let cycle_tests_src = read_workspace_file(
        "crates/verter_session/src/component_meta_resolution_policy_cycle_tests.rs",
    );
    assert!(
        cycle_tests_src.contains("recursive_pick_local_alias_terminates_via_semantic_miss"),
        "Stage-0 baseline: the recursive Pick alias termination characterisation test must \
         already exist on the audited base SHA."
    );
    assert!(
        cycle_tests_src.contains("recursive_omit_self_referential_alias_terminates"),
        "Stage-0 baseline: the recursive Omit alias termination characterisation test must \
         already exist."
    );
}

/// Find the body of a free function or method by name. Returns `Some(body)`
/// when located, `None` otherwise. The locator is intentionally simple:
/// find `fn <name>(`, then walk to the next `{` and brace-match. It will
/// not handle nested same-name fns (none exist for the names we look for
/// in `session_runtime.rs`).
fn extract_method_body(src: &str, fn_name: &str) -> Option<String> {
    let needle = format!("fn {}(", fn_name);
    let header_idx = src.find(&needle)?;
    let body_start = src[header_idx..].find('{')? + header_idx;
    let bytes = src.as_bytes();
    let mut depth: i64 = 1;
    let mut idx = body_start + 1;
    while idx < bytes.len() && depth > 0 {
        match bytes[idx] {
            b'{' => depth += 1,
            b'}' => depth -= 1,
            _ => {}
        }
        idx += 1;
    }
    Some(src[body_start + 1..idx - 1].to_string())
}

/// CHARACTERISATION 5 — internal coherence of the inventory JSON.
///
/// The JSON file must declare the schema_version and captured_at_sha so
/// drift across Stage commits is auditable. This is a precondition for
/// Stage 1+ tests reading the JSON.
#[test]
fn inventory_json_is_well_formed_and_pins_to_a_sha() {
    let path = fixture("tests/fixtures/cache_baseline/evict_canonical_inventory.json");
    let raw = fs::read_to_string(&path).expect("read inventory JSON");
    let parsed: serde_json::Value = serde_json::from_str(&raw).expect("inventory parses as JSON");

    let schema = parsed
        .get("schema_version")
        .and_then(|v| v.as_u64())
        .expect("schema_version present");
    assert_eq!(
        schema, 1,
        "inventory schema_version must be 1 on Stage-0; future schema bumps require a \
         documented amendment in the plan + a coordinated test update."
    );

    let sha = parsed
        .get("captured_at_sha")
        .and_then(|v| v.as_str())
        .expect("captured_at_sha present");
    assert_eq!(
        sha.len(),
        40,
        "captured_at_sha must be a 40-char full git SHA"
    );

    // Pin to the Stage-0-pre base SHA so any update to the inventory
    // includes an explicit re-pin to the new audited base.
    assert_eq!(
        sha, "ccc0522309091c532d6fba756da392598eab059c",
        "captured_at_sha must match the Stage-0 base SHA. If you regenerated the inventory \
         against a newer base, update captured_at_sha AND document the move in plan amendment."
    );

    let trigger = parsed.get("upsert_trigger").expect("upsert_trigger");
    assert!(
        trigger.get("canonical").is_some(),
        "upsert_trigger.canonical present"
    );
    assert!(
        trigger.get("content_hash_before").is_some(),
        "upsert_trigger.content_hash_before present"
    );
    assert!(
        trigger.get("content_hash_after").is_some(),
        "upsert_trigger.content_hash_after present"
    );
}

/// CHARACTERISATION 6 — the failure-mode investigation MD is present
/// and asserts (c) layered cooperative cycle guards, not (a) or (b).
///
/// This is the documented Stage 0 sub-task 2(a) outcome. A Stage 3
/// agent reading the file must find the conclusion explicit.
#[test]
fn cycle_safety_failure_mode_documents_layered_cooperative_guards() {
    let path = fixture("tests/fixtures/cache_baseline/cycle_safety_failure_mode.md");
    let body = fs::read_to_string(&path).expect("read cycle_safety_failure_mode.md");
    // The investigation must explicitly call out "neither (a) nor (b)" so a
    // Stage 3 reader cannot accidentally read the file as endorsing (a) or
    // (b). The marker is a load-bearing claim, not boilerplate.
    assert!(
        body.contains("Today's failure mode is neither (a) nor (b)"),
        "cycle_safety_failure_mode.md must state explicitly that today's failure mode is \
         neither (a) recursion-limit panic nor (b) content-hash memoisation brittleness."
    );
    assert!(
        body.contains("layered cooperative cycle guards"),
        "cycle_safety_failure_mode.md must name the actual failure mode: layered cooperative \
         cycle guards."
    );
    assert!(
        body.contains("MaterializeInFlightGuard"),
        "Investigation must cite MaterializeInFlightGuard (layer-2)."
    );
    assert!(
        body.contains("active_refs"),
        "Investigation must cite the policy walker active_refs guard (layer-1)."
    );
    assert!(
        body.contains("MAX_DEPTH"),
        "Investigation must cite the defensive depth fuse (layer-3)."
    );
}

/// CHARACTERISATION 7 — multi_candidate_proxy.md pins the CAS-serialised
/// overlay swap as today's behaviour Stage 5 will invert.
#[test]
fn multi_candidate_proxy_documents_cas_serialisation() {
    let path = fixture("tests/fixtures/cache_baseline/multi_candidate_proxy.md");
    let body = fs::read_to_string(&path).expect("read multi_candidate_proxy.md");
    assert!(
        body.contains("active_overlay_session"),
        "multi_candidate_proxy.md must cite the active_overlay_session CAS field."
    );
    assert!(
        body.contains("CAS"),
        "multi_candidate_proxy.md must name CAS as the serialisation oracle."
    );
    assert!(
        body.contains("host.upsert"),
        "multi_candidate_proxy.md must identify host.upsert calls from query paths \
         as the contention signal Stage 4d / Stage 5 inverts."
    );
    assert!(
        body.contains("MaterializeStructureDb"),
        "multi_candidate_proxy.md must observe that MaterializeStructureDb carries one entry \
         per key today (vs. up-to-4 candidates post-Stage-5)."
    );
}

// -------------------------------------------------------------------------
// Self-discrimination checks: every test above must observe a real fact
// about the audited base SHA. The helpers themselves must be correct or
// every assertion is bogus.
// -------------------------------------------------------------------------

#[test]
fn extract_drained_dbs_picks_up_realistic_drain_calls() {
    // Sanity: the helper recognises both the inline-chain shape
    // (`self.field.method(...)`) and the multi-line-chain shape that
    // appears in the current source.
    let synthetic = r#"
        self.indexed.remove(canonical_id);
        self.semantic_graph
            .invalidate_canonical(canonical_id);
        self.semantic_db.lock().invalidate(canonical_id);
        // self.unrelated.do_not_count(); — this is a comment, must NOT pick up
        /* self.also_unrelated.also_no(); */
    "#;
    let extracted = extract_drained_dbs(synthetic);
    assert!(extracted.contains(&("indexed".to_string(), "remove".to_string())));
    assert!(extracted.contains(&(
        "semantic_graph".to_string(),
        "invalidate_canonical".to_string()
    )));
    assert!(extracted.contains(&(
        "semantic_db".to_string(),
        "lock().invalidate".to_string()
    )));
    // The line-commented and block-commented `self.unrelated.…` references
    // must NOT be picked up.
    let has_unrelated = extracted.iter().any(|(f, _)| f == "unrelated" || f == "also_unrelated");
    assert!(
        !has_unrelated,
        "strip_comments must hide `self.X.Y(...)` inside line/block comments — \
         this test catches a regex that ignores the comment stripper. Extracted: {:?}",
        extracted
    );
}

#[test]
fn evict_canonical_body_locator_finds_the_correct_function() {
    let src = read_workspace_file("crates/verter_session/src/project_type_store.rs");
    let body = evict_canonical_body(&src);
    // The body must (a) be non-empty, (b) contain at least one
    // `self.<field>.<method>` call, (c) NOT contain the verbatim header
    // (proving the slice excludes the function signature).
    assert!(!body.is_empty(), "evict_canonical body is non-empty");
    assert!(
        body.contains("self.indexed.remove"),
        "evict_canonical body locator must capture the body that drains self.indexed"
    );
    assert!(
        !body.contains("pub fn evict_canonical(&self, canonical_id: &str)"),
        "evict_canonical body locator must not include the function header in the slice"
    );
}

#[test]
fn extract_method_body_locates_session_runtime_apply_own_overlays() {
    let src = read_workspace_file("crates/verter_session/src/session_runtime.rs");
    let body = extract_method_body(&src, "apply_own_overlays").expect("apply_own_overlays must exist");
    assert!(
        body.contains("self.host()"),
        "apply_own_overlays must reference self.host()"
    );
}

#[test]
fn fixture_paths_resolve_to_real_files() {
    let baseline_paths = [
        "tests/fixtures/cache_baseline/cycle_safety_failure_mode.md",
        "tests/fixtures/cache_baseline/evict_canonical_inventory.json",
        "tests/fixtures/cache_baseline/multi_candidate_proxy.md",
        "tests/fixtures/cache_baseline/baseline.json",
    ];
    for rel in baseline_paths {
        let p: &Path = &fixture(rel);
        assert!(
            p.exists(),
            "Stage-0 fixture must be committed at {}",
            p.display()
        );
    }
}

/// CHARACTERISATION 8 — the committed Stage-0 baseline JSON declares the
/// pre-Stage-1 counter shape Stage 7 canary diffs against.
///
/// The structural fields are deterministic (component count, miss
/// count, materialise cardinality, candidate set histogram) and are
/// pinned here. The timing fields are intentionally `null` in the
/// committed snapshot — they vary per host and are measured locally
/// at canary time.
#[test]
fn committed_baseline_json_declares_pre_stage1_counter_shape() {
    let path = fixture("tests/fixtures/cache_baseline/baseline.json");
    let raw = fs::read_to_string(&path).expect("read baseline.json");
    let json: serde_json::Value = serde_json::from_str(&raw).expect("parse baseline.json");

    assert_eq!(
        json.get("schema_version").and_then(|v| v.as_u64()),
        Some(1),
        "baseline.json schema_version must be 1 on Stage 0"
    );

    let sha = json
        .get("captured_at_sha")
        .and_then(|v| v.as_str())
        .expect("captured_at_sha");
    assert_eq!(
        sha, "ccc0522309091c532d6fba756da392598eab059c",
        "baseline.json captured_at_sha must match the Stage-0 base SHA"
    );

    let fixture_block = json.get("fixture").expect("fixture block present");
    assert_eq!(
        fixture_block
            .get("num_components")
            .and_then(|v| v.as_u64()),
        Some(16),
        "baseline.json must pin num_components = 16 (the bench fixture's constant)"
    );

    let aggregates = json.get("aggregates").expect("aggregates block");
    assert_eq!(
        aggregates
            .get("fact_validation_warm_hit_count")
            .and_then(|v| v.as_u64()),
        Some(0),
        "Pre-Stage-1: fact_validation_warm_hit_count must be 0 (no fact-based cache yet)"
    );
    assert_eq!(
        aggregates
            .get("fact_validation_miss_count")
            .and_then(|v| v.as_u64()),
        Some(16),
        "Pre-Stage-1: fact_validation_miss_count == num_components on cold pass"
    );
    assert_eq!(
        aggregates
            .get("materialise_cardinality_per_owner")
            .and_then(|v| v.as_f64()),
        Some(16.0),
        "Pre-Stage-5: materialise_cardinality_per_owner == N (one entry per owner-instance \
         of the shared dep); Stage 5 inverts to 1.0"
    );

    let histogram = aggregates
        .get("candidate_set_size_histogram")
        .expect("candidate_set_size_histogram");
    assert_eq!(
        histogram.get("1").and_then(|v| v.as_u64()),
        Some(16),
        "Pre-Stage-5: only the \"1\" bin is populated; every slot has exactly one entry"
    );
    // The histogram MUST NOT carry any other bins on the pre-change tree.
    let histogram_keys: Vec<&String> = histogram
        .as_object()
        .map(|m| m.keys().collect())
        .unwrap_or_default();
    assert_eq!(
        histogram_keys.len(),
        1,
        "Pre-Stage-5: candidate_set_size_histogram has exactly one bin (\"1\"); the test \
         catches a regression that pre-populates bins 2..4 before Stage 5 lands."
    );

    // The canary contract block must enumerate the Stage 7 thresholds the
    // canary commit will check against this snapshot.
    let canary = json
        .get("canary_diff_contract")
        .expect("canary_diff_contract block present");
    assert!(canary.get("stage_6d_must_change").is_some());
    assert!(canary.get("stage_7_canary_thresholds").is_some());
}
