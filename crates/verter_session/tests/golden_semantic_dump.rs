//! Tier 0 Step 0.2 — semantic-graph eager-snapshot dump.
//!
//! Dumps the [`SemanticGraphStore`] interned-key set + per-key result hash
//! + `dep_signature` to
//! `crates/verter_session/tests/perf_bounds/golden-semantic/keys-eager.json`
//! after running `getComponentMeta` against 32 representative
//! `nuxt-ui-codex-bench` fixtures (or as many as fit in the
//! 10-min hard timeout per the orchestrator's Tier 0 directive).
//!
//! Gated behind the `external-corpus` Cargo feature — the default
//! `cargo test --workspace --tests` run MUST stay hermetic. Run with:
//!
//! ```bash
//! cargo test -p verter_session --tests --features external-corpus dump_semantic_keys_eager -- --ignored --nocapture
//! ```
//!
//! The audit eager dump is exposed via
//! `SemanticGraphStore::audit_eager_key_dump()` (gated `#[doc(hidden)]`,
//! safe to call mid-request, no hot-path impact).

#![cfg(feature = "external-corpus")]

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use verter_session::audited_request::AuditedRequest;
use verter_session::{HostConfig, VerterHost};
use verter_workspace::{
    FilesystemOptions, FilesystemWorkspace, ProjectGraph, ViteConfigOptions, WorkspaceAccess,
};

const TARGET_FIXTURE_COUNT: usize = 32;

/// Output root: the workspace this test was compiled against. Output
/// files (the `keys-eager.json` snapshot) go here so they live alongside
/// the source under `crates/verter_session/tests/perf_bounds/`. From
/// `CARGO_MANIFEST_DIR` (= `crates/verter_session/`) ascend two levels.
fn output_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .map(|p| p.to_path_buf())
        .expect("workspace root reachable from CARGO_MANIFEST_DIR")
}

/// Corpus root: the checkout that contains
/// `.integration-tests/repos/nuxt-ui-codex-bench`. When this test is
/// compiled inside a `git worktree`, the worktree may not vendor its
/// own integration-tests directory — ascend past the worktree to find
/// the upstream checkout that does.
fn corpus_root() -> PathBuf {
    // Try the output root first; fall back to ascending until we find
    // the corpus checkout.
    let direct = output_root();
    if direct
        .join(".integration-tests/repos/nuxt-ui-codex-bench")
        .exists()
    {
        return direct;
    }
    let mut p = direct;
    loop {
        if p.join(".integration-tests/repos/nuxt-ui-codex-bench")
            .exists()
        {
            return p;
        }
        if !p.pop() {
            panic!(
                "external-corpus test could not locate \
                 `.integration-tests/repos/nuxt-ui-codex-bench` from `{}`",
                env!("CARGO_MANIFEST_DIR"),
            );
        }
    }
}

fn discover_fixtures(project_root: &Path) -> Vec<String> {
    let runtime = project_root.join("src").join("runtime");
    let components_root = runtime.join("components");
    let mut names = Vec::new();
    let mut stack: Vec<PathBuf> = vec![runtime.clone()];
    while let Some(dir) = stack.pop() {
        let entries = match fs::read_dir(&dir) {
            Ok(it) => it,
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let ftype = match entry.file_type() {
                Ok(t) => t,
                Err(_) => continue,
            };
            if ftype.is_dir() {
                let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
                if name.starts_with('.') || name == "node_modules" {
                    continue;
                }
                stack.push(path);
                continue;
            }
            if !ftype.is_file() || path.extension().and_then(|s| s.to_str()) != Some("vue") {
                continue;
            }
            let spec = match path.strip_prefix(&components_root) {
                Ok(rel) => rel.with_extension("").to_string_lossy().replace('\\', "/"),
                Err(_) => match path.strip_prefix(project_root) {
                    Ok(rel) => rel.to_string_lossy().replace('\\', "/"),
                    Err(_) => path.to_string_lossy().replace('\\', "/"),
                },
            };
            names.push(spec);
        }
    }
    names.sort();
    names
}

fn path_to_host_id(path: &Path) -> String {
    let s = path.to_string_lossy().replace('\\', "/");
    let trimmed = s.trim_end_matches('/').to_string();
    if let Some(rest) = trimmed.strip_prefix("//?/UNC/") {
        format!("//{rest}")
    } else if let Some(rest) = trimmed.strip_prefix("//?/") {
        rest.to_string()
    } else {
        trimmed
    }
}

fn target_id_for_name(project_root: &Path, name: &str) -> String {
    let candidate = project_root
        .join("src")
        .join("runtime")
        .join("components")
        .join(name)
        .with_extension("vue");
    let canonical = candidate.canonicalize().unwrap_or(candidate);
    path_to_host_id(&canonical)
}

fn build_host(project_root: &Path) -> Arc<VerterHost> {
    let project_root_id = path_to_host_id(project_root);
    let ws = FilesystemWorkspace::new(FilesystemOptions {
        roots: vec![project_root_id.clone()],
        ..Default::default()
    });
    let graph_result = ProjectGraph::from_workspace_roots(
        &ws,
        std::slice::from_ref(&project_root_id),
        &ViteConfigOptions::default(),
    );
    ws.set_project_graph(graph_result.graph);
    let ws_access: Arc<dyn WorkspaceAccess> = Arc::new(ws);
    Arc::new(VerterHost::new(
        HostConfig {
            audit_enabled: true,
            footprint_capture: true,
            ..HostConfig::default()
        },
        ws_access,
    ))
}

/// Tier 0 Step 0.2: dump interned `SemanticQueryKey` set + per-key result
/// hash + `dep_signature` to
/// `crates/verter_session/tests/perf_bounds/golden-semantic/keys-eager.json`.
///
/// Marked `#[ignore]` per orchestrator brief — runs only when explicitly
/// requested with `--ignored`. Default `cargo test` skips it.
#[test]
#[ignore]
fn dump_semantic_keys_eager() {
    let started = Instant::now();
    let project_root = corpus_root().join(".integration-tests/repos/nuxt-ui-codex-bench");
    let out_dir = output_root().join("crates/verter_session/tests/perf_bounds/golden-semantic");
    fs::create_dir_all(&out_dir).expect("create golden-semantic dir");

    let mut all_fixtures = discover_fixtures(&project_root);
    all_fixtures.truncate(TARGET_FIXTURE_COUNT);
    let target_count = all_fixtures.len();
    eprintln!(
        "Tier 0 Step 0.2: dumping eager keys for {} fixtures (target {})",
        target_count, TARGET_FIXTURE_COUNT
    );

    let host = build_host(&project_root);
    let mut fixtures_processed = 0usize;
    let mut errors: Vec<String> = Vec::new();
    // Budget is split between fixture run-time (300s) and a per-fixture
    // soft cap (45s). Both are tighter than the orchestrator's 10-min
    // hard timeout so the test always reaches the JSON-write step. If
    // a single fixture exceeds the per-fixture cap, the test moves on
    // to the next; the partial-progress state is recorded in the JSON.
    let hard_deadline = started + std::time::Duration::from_secs(300);
    let per_fixture_soft_cap = std::time::Duration::from_secs(45);
    // Skip fixtures whose name is on the "known heavy" list. ChatMessage
    // and ChatMessages exceed the per-fixture cap on cold runs (each
    // ~5min via materialize). Tier 1B BFS bridge will reduce this and
    // these fixtures can be re-included in a later regen.
    let known_heavy = ["ChatMessage", "ChatMessages"];

    for (idx, fixture_name) in all_fixtures.iter().enumerate() {
        if Instant::now() >= hard_deadline {
            errors.push(format!(
                "5-min run cap hit; stopping at fixture {}/{}",
                idx, target_count
            ));
            break;
        }
        if known_heavy.iter().any(|h| *h == fixture_name) {
            errors.push(format!(
                "{fixture_name}: skipped (known-heavy; pre-bridge resolver \
                 takes >45s on cold; expected after Tier 1B)"
            ));
            continue;
        }
        let canonical = target_id_for_name(&project_root, fixture_name);
        eprintln!(
            "  ({:>2}/{:>2}) {} ...",
            idx + 1,
            target_count,
            fixture_name
        );
        let fixture_started = Instant::now();
        let outcome = AuditedRequest::builder()
            .attach_to(Arc::clone(&host))
            .resolve(&canonical);
        let fixture_elapsed = fixture_started.elapsed();
        match outcome {
            Ok(_) => {
                fixtures_processed += 1;
                if fixture_elapsed > per_fixture_soft_cap {
                    errors.push(format!(
                        "{fixture_name}: completed in {:.1}s (over 45s soft cap; \
                         consider adding to known_heavy)",
                        fixture_elapsed.as_secs_f64()
                    ));
                }
            }
            Err(e) => {
                errors.push(format!("{fixture_name}: {e}"));
            }
        }
    }

    // Drain the host's SemanticGraphStore via the audit-eager dump method
    // we added in Tier 0. The dump is sorted by key-debug-string for
    // deterministic JSON output.
    let store = host.project_type_store().semantic_graph();
    let rows = store.audit_eager_key_dump();
    let memo_count = store.memo_entry_count();

    eprintln!(
        "Tier 0 Step 0.2: drained {} keys ({} memo entries reported), elapsed {:?}",
        rows.len(),
        memo_count,
        started.elapsed()
    );

    // Assemble the JSON payload.
    let mut keys_json = Vec::with_capacity(rows.len());
    for row in &rows {
        keys_json.push(serde_json::json!({
            "key_repr": row.key_repr,
            "result_hash": row.result_hash,
            "dep_signature": row.dep_signature,
        }));
    }
    let payload = serde_json::json!({
        "fixtures_processed": fixtures_processed,
        "fixtures_attempted": all_fixtures.len(),
        "expected": TARGET_FIXTURE_COUNT,
        "memo_entry_count": memo_count,
        "keys_count": rows.len(),
        "wall_clock_ms": started.elapsed().as_millis() as u64,
        "errors": errors,
        "keys": keys_json,
    });
    let json = serde_json::to_string_pretty(&payload).expect("serialize");
    let out_path = out_dir.join("keys-eager.json");
    fs::write(&out_path, json).expect("write keys-eager.json");
    eprintln!("Tier 0 Step 0.2: wrote {}", out_path.display());

    assert!(
        fixtures_processed >= 1,
        "Step 0.2 produced no completed fixtures — vacuous result is a stop condition"
    );
    assert!(
        !rows.is_empty(),
        "Step 0.2 drained zero semantic-graph keys after processing {} fixtures — \
         the audit-eager dump is broken",
        fixtures_processed
    );
}
