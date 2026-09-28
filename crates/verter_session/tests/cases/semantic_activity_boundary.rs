//! The semantic activity gate's safety invariant, pinned at the source level.
//!
//! A semantic payload must not be reclaimed while a computation can still
//! observe its `SemanticNodeId`. The gate (`project_type_store/semantic_activity.rs`)
//! enforces it for every computation that holds a `SemanticActivityGuard`;
//! what this test pins is that every computation that could race a release
//! does hold one, by construction of the consumers rather than by convention:
//!
//! 1. **Detached scheduler work never reads semantic nodes.** Jobs that
//!    outlive the host call that submitted them (the close-time background
//!    reload, dependency auto-ingest loads, superseded loads that finish on a
//!    worker) run the Source/Analysis/Artifact stages of `host_executor.rs`,
//!    which build parse snapshots and registered envelopes only. If one of
//!    those files ever reaches the semantic graph, the gate's premise breaks.
//! 2. **A consumer that reaches the host from several threads holds it only
//!    through the guarded handle** (`verter_session::project_type_store::GuardedHost`, the
//!    language server's `SharedHost`). The language server is the one
//!    consumer that both runs concurrently and closes documents.
//! 3. **The synchronous bindings stay synchronous.** The NAPI and WASM
//!    bindings hold a raw `Arc<VerterHost>` and take no guard; that is safe
//!    only because every call runs to completion on one thread, so a close
//!    (NAPI `refreshBase`) can never overlap a read. The moment either grows
//!    an async task, a thread or a threadsafe callback, it must adopt
//!    `GuardedHost`.
//! 4. **Consumers that never close a document never queue a release.** The
//!    MCP server, the CLI checker and the baseline runner call the host from
//!    several threads without guards; nothing is ever queued on their gate
//!    because none of them calls `evict`.
//!
//! Each assertion names the file that would have to change and what to do
//! then, so a failure here is a design decision, not a mystery.

use std::fs;
use std::path::{Path, PathBuf};

fn crates_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates/ holds verter_session")
        .to_path_buf()
}

/// Non-test Rust sources of `crate_dir/src`: `*_tests.rs` files, `tests/`
/// directories and everything after a file's `#[cfg(test)] mod tests {`
/// module are left out.
fn production_sources(crate_dir: &Path) -> Vec<(PathBuf, String)> {
    let mut out = Vec::new();
    let mut stack = vec![crate_dir.join("src")];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                if path.file_name().is_some_and(|name| name == "tests") {
                    continue;
                }
                stack.push(path);
            } else if path.extension().is_some_and(|ext| ext == "rs")
                && !path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.ends_with("_tests.rs"))
            {
                let text = fs::read_to_string(&path).expect("readable source");
                out.push((path, strip_inline_test_module(&text)));
            }
        }
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

/// Drop an inline `#[cfg(test)]` module (`mod tests {` at column 0) and
/// everything after it: test helpers hold hosts however they like.
fn strip_inline_test_module(text: &str) -> String {
    let mut kept = String::new();
    let mut lines = text.lines().peekable();
    while let Some(line) = lines.next() {
        if line.trim() == "#[cfg(test)]"
            && lines
                .peek()
                .is_some_and(|next| next.starts_with("mod ") && next.trim_end().ends_with('{'))
        {
            break;
        }
        kept.push_str(line);
        kept.push('\n');
    }
    kept
}

fn offenders<'a>(
    sources: &'a [(PathBuf, String)],
    needles: &[&str],
) -> Vec<(&'a Path, usize, &'a str)> {
    let mut found = Vec::new();
    for (path, text) in sources {
        for (index, line) in text.lines().enumerate() {
            if needles.iter().any(|needle| line.contains(needle)) {
                found.push((path.as_path(), index + 1, line.trim()));
            }
        }
    }
    found
}

fn describe(found: &[(&Path, usize, &str)]) -> String {
    found
        .iter()
        .map(|(path, line, text)| format!("  {}:{line}: {text}", path.display()))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Invariant 1: the stage executor and everything the Source stage builds
/// from never touch the semantic graph.
#[test]
fn detached_scheduler_stages_never_reach_the_semantic_graph() {
    let session = crates_root().join("verter_session");
    let sources: Vec<(PathBuf, String)> = production_sources(&session)
        .into_iter()
        .filter(|(path, _)| {
            let relative = path.strip_prefix(session.join("src")).expect("under src");
            let relative = relative.to_string_lossy().replace('\\', "/");
            relative == "host_executor.rs"
                || relative == "parse.rs"
                || relative == "block_content.rs"
                || relative.starts_with("carrier_publication_store/")
        })
        .collect();
    assert!(
        sources.len() >= 3,
        "the stage executor sources moved; point this test at them again"
    );
    let found = offenders(
        &sources,
        &[
            "semantic_graph()",
            "node_data(",
            "node_scope(",
            "project_semantic_dispatch",
            "semantic_query_memo",
        ],
    );
    assert!(
        found.is_empty(),
        "a scheduler stage that can outlive its host call now reaches the semantic graph, \
         so a close-time release can race it; run those reads under a SemanticActivityGuard \
         (the stage would have to be given the store's guard) or keep them out of the stages:\n{}",
        describe(&found)
    );
}

/// Invariant 2: the language server holds the host only through the guarded
/// handle. A raw `Arc<VerterHost>` may appear only where the handle is built
/// or where the host is constructed and threaded to that construction
/// (`main.rs`), and in the provider adapters that read project identity and
/// admission epochs only (never semantic nodes).
#[test]
fn the_language_server_holds_the_host_through_the_guarded_handle() {
    let lsp = crates_root().join("verter_lsp");
    let sources = production_sources(&lsp);
    assert!(
        !sources.is_empty(),
        "verter_lsp sources are next to verter_session"
    );
    let allowed = |path: &Path| {
        let relative = path
            .strip_prefix(lsp.join("src"))
            .expect("under src")
            .to_string_lossy()
            .replace('\\', "/");
        matches!(
            relative.as_str(),
            // Builds the host and hands it to the registry's `SharedHost`.
            "main.rs" | "lib.rs" | "audit_harness.rs" | "documents/mod.rs"
            // Test-only host construction compiled into the crate.
            | "test_utils.rs"
            // The handle itself (a re-export of `verter_session::project_type_store::GuardedHost`).
            | "documents/guarded_host.rs"
            // Provider adapters: `AdmissionEpoch::current`, `resolve_carrier`
            // and project identity; they never read semantic nodes.
            | "tsgo/composite.rs" | "tsgo/project_binding.rs" | "tsserver/project_router.rs"
        )
    };
    let found: Vec<_> = offenders(
        &sources,
        &["Arc<VerterHost>", "Arc<verter_session::VerterHost>"],
    )
    .into_iter()
    .filter(|(path, _, _)| !allowed(path))
    .collect();
    assert!(
        found.is_empty(),
        "verter_lsp stores a raw Arc<VerterHost> outside the allowed files; hold it as \
         `SharedHost` (verter_session::project_type_store::GuardedHost) so every call runs under a guard:\n{}",
        describe(&found)
    );
}

/// Invariant 3: the synchronous bindings stay synchronous.
#[test]
fn the_synchronous_bindings_grow_no_concurrency() {
    for binding in ["verter_napi", "verter_wasm"] {
        let dir = crates_root().join(binding);
        if !dir.join("src").is_dir() {
            continue;
        }
        let sources = production_sources(&dir);
        let found = offenders(
            &sources,
            &[
                "AsyncTask",
                "ThreadsafeFunction",
                "std::thread::spawn",
                "thread::spawn(",
                "tokio::spawn",
                "spawn_blocking",
                "rayon::spawn",
                "wasm_bindgen_futures",
            ],
        );
        assert!(
            found.is_empty(),
            "{binding} holds a raw Arc<VerterHost> and takes no SemanticActivityGuard, which is \
             safe only while every call runs to completion on one thread; it now spawns \
             concurrent work, so hold the host as `verter_session::project_type_store::GuardedHost` and call it \
             through `host()`:\n{}",
            describe(&found)
        );
    }
}

/// Invariant 4: the consumers that take no guard never close a document.
#[test]
fn unguarded_consumers_never_close_a_document() {
    for consumer in ["verter_mcp", "verter_tsc", "verter_dx_baseline"] {
        let dir = crates_root().join(consumer);
        if !dir.join("src").is_dir() {
            continue;
        }
        let sources = production_sources(&dir);
        let found = offenders(&sources, &[".evict(", "defer_release_canonical("]);
        assert!(
            found.is_empty(),
            "{consumer} calls the host from several threads without a SemanticActivityGuard, \
             which is safe only because it never closes a document (nothing is ever queued on \
             its gate); it now closes one, so hold the host as `verter_session::project_type_store::GuardedHost` \
             and call it through `host()`:\n{}",
            describe(&found)
        );
    }
}
