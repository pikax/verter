use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

// ── Helpers ──

fn workspace_root() -> PathBuf {
    super::workspace_root()
}

fn read_workspace_file(rel: &str) -> String {
    super::read_workspace_file(rel)
}

/// Walk a directory (production tree) and yield every `.rs` file
/// whose name does NOT end in `_tests.rs` and is not nested under a
/// `tests/` or `benches/` directory.
fn walk_production_rs(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let entries = match fs::read_dir(&dir) {
            Ok(it) => it,
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                if name == "tests"
                    || name == "compile_tests"
                    || name == "client_tests"
                    || name == "benches"
                    || name == "examples"
                    || name == "target"
                {
                    continue;
                }
                stack.push(path);
                continue;
            }
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if !name.ends_with(".rs") {
                continue;
            }
            if name.ends_with("_tests.rs") || name == "tests.rs" {
                continue;
            }
            out.push(path);
        }
    }
    out.sort();
    out
}

fn relative_to_root(abs: &Path) -> String {
    abs.strip_prefix(workspace_root())
        .unwrap_or(abs)
        .to_string_lossy()
        .replace('\\', "/")
}

// ── Guard 1 — no_std_fs_in_semantic_session_paths ──

/// Predicate: scan a single `.rs` file's source for direct
/// `std::fs::` references. Returns `true` when at least one match
/// exists.
pub fn file_uses_std_fs(src: &str) -> bool {
    src.contains("std::fs::")
}

/// Allowlist for guard #1, sourced from
/// `crates/verter_workspace/tool-output-allowlist.toml`. Each entry
/// is a path to a file whose `std::fs::` calls write/read
/// non-semantic output (trace artifacts, MCP baselines, profiler
/// dumps, test fixtures, TS-runtime tool-cache files, etc.).
///
/// `source-read` callsites are NOT allowlisted — they MUST route
/// through `verter_workspace::WorkspaceAccess`. Migrating a
/// `source-read` file shrinks the allowlist by deleting its entry
/// from the TOML file in the same change.
pub fn guard1_allowlist() -> BTreeSet<String> {
    load_tool_output_allowlist()
}

pub fn guard1_in_scope_dirs() -> Vec<&'static str> {
    vec![
        "crates/verter_session/src",
        "crates/verter_type_engine/src",
        "crates/verter_semantic/src",
        "crates/verter_diagnostics/src",
        "crates/verter_type_runtime/src",
        "crates/verter_lsp/src",
        "crates/verter_mcp/src",
    ]
}

/// Parse `crates/verter_workspace/tool-output-allowlist.toml` and
/// return the set of allowlisted paths, one per `[[entries]]` entry.
pub fn load_tool_output_allowlist() -> BTreeSet<String> {
    #[derive(serde::Deserialize)]
    struct Allowlist {
        entries: Vec<Entry>,
    }
    #[derive(serde::Deserialize)]
    struct Entry {
        path: String,
        #[allow(dead_code)]
        rationale: String,
    }

    let path = workspace_root().join("crates/verter_workspace/tool-output-allowlist.toml");
    let raw = fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("guard 1: could not read `{}`: {e}", path.display()));
    let parsed: Allowlist = toml::from_str(&raw)
        .unwrap_or_else(|e| panic!("guard 1: could not parse `{}`: {e}", path.display()));
    parsed.entries.into_iter().map(|e| e.path).collect()
}

/// Run the guard 1 predicate over the in-scope directories and
/// return the set of relative paths that USE `std::fs::` and are
/// NOT in the allowlist. An empty result means the guard passes.
pub fn guard1_violations(allowlist: &BTreeSet<String>) -> Vec<String> {
    let root = workspace_root();
    let mut violations = Vec::new();
    for rel_dir in guard1_in_scope_dirs() {
        for path in walk_production_rs(&root.join(rel_dir)) {
            let src = match fs::read_to_string(&path) {
                Ok(s) => s,
                Err(_) => continue,
            };
            if !file_uses_std_fs(&src) {
                continue;
            }
            let rel = relative_to_root(&path);
            if allowlist.contains(&rel) {
                continue;
            }
            violations.push(rel);
        }
    }
    violations.sort();
    violations
}

// ── Guard 2 — vfs_boundary_is_authoritative ──

/// Predicate: scan a `.rs` file for any direct OS file API
/// reference. Returns `true` when at least one such reference is
/// found.
///
/// Patterns checked:
/// - `std::fs::` (synchronous OS file API)
/// - `tokio::fs::` (async OS file API)
pub fn file_uses_os_file_api(src: &str) -> bool {
    src.contains("std::fs::") || src.contains("tokio::fs::")
}

/// Allowlist for guard #2: paths where direct OS file APIs are
/// the legitimate authority. `native_fs.rs` is the documented
/// disk boundary; `intrinsic_library.rs` is the dedicated reader
/// for ambient TypeScript SDK declarations. The remaining entries
/// are infrastructure tracked by `tool-output-allowlist.toml`.
pub fn guard2_allowlist() -> BTreeSet<String> {
    let mut set: BTreeSet<String> = [
        "crates/verter_workspace/src/native_fs.rs",
        // Declaration generator behind `required-features`, absent from
        // every default build: its one write rewrites a committed
        // declaration whose bytes a freshness guard pins. Build-time
        // source-tree output, not workspace/semantic/overlay state.
        "crates/verter_napi/src/bin/generate_host_compile_request_ts.rs",
        "crates/verter_workspace/src/config.rs",
        "crates/verter_workspace/src/snapshot_builder.rs",
        "crates/verter_workspace/src/vite_config.rs",
        "crates/verter_workspace/src/dir_index.rs",
        "crates/verter_workspace/src/filesystem.rs",
        "crates/verter_workspace/src/ambient_parse.rs",
        "crates/verter_workspace/src/intrinsic_library.rs",
        "crates/verter_scheduler/src/source_loader.rs",
        "crates/verter_tsc/src/checker.rs",
        "crates/verter_tsc/src/tsconfig.rs",
        // tsgo toolchain provisioning (the 4-tier resolver) — real-OS
        // walks of PATH dirs, project `node_modules` (flat + pnpm store),
        // the update cache, and the bundled sidecar to locate a supported
        // tsgo engine binary, and the capability smoke's minimal temp
        // project. A subprocess engine binary is real-FS by nature, never
        // workspace/semantic source (sibling of the
        // `verter_type_runtime` IPC entries).
        "crates/verter_tsgo_api/src/toolchain/discovery.rs",
        "crates/verter_tsgo_api/src/toolchain/validation.rs",
        // Linux process-liveness probe — `/proc/<pid>/stat` is volatile
        // kernel process metadata used to distinguish an exited zombie
        // from a live engine/client. It is not workspace/semantic input
        // and must not be cached by NativeFs. D14 carries the full
        // per-callsite rationale.
        "crates/verter_tsgo_api/src/process.rs",
        // Relay-shim rendezvous advertisement — the IPC file a shim
        // writes on startup so a `verter_lsp` control client can DISCOVER
        // it (create_dir_all / write / read / read_dir / remove). An IPC
        // rendezvous artifact is real-FS by nature (a separate process
        // reads it off disk); never workspace/semantic source, never
        // routable through `WorkspaceAccess` (sibling of the `spawn.rs`
        // subprocess-binary IPC entry).
        "crates/verter_tsgo_api/src/control/advertisement.rs",
        // Relay-shim control endpoint — a Unix-domain-socket file is a
        // real filesystem artifact the OS binds, so the listener owns its
        // full socket-file lifecycle (`#[cfg(unix)]`): it creates +
        // validates a PRIVATE per-session `0o700` parent subdir
        // (`DirBuilder` mode-`0o700` create + `symlink_metadata` — a real
        // dir, euid-owned, exactly owner-rwx), holds the grandparent to a
        // sticky-or-euid/root-owned secure-permissions ceiling
        // (`create_dir_all` + `metadata`), removes a stale socket before
        // bind and its own socket on drop (`std::fs::remove_file`), chmods
        // the bound socket to owner-only `0o600` (`set_permissions` /
        // `Permissions::from_mode`), and removes the now-empty private
        // subdir on `Drop` (`remove_dir`). This socket-file lifecycle is
        // real-FS by nature, never workspace source (same category as the
        // `advertisement.rs` IPC rendezvous file).
        "crates/verter_tsgo_api/src/control/transport.rs",
        // Audit substrate's `current_process_rss` reads
        // `/proc/self/statm` (Linux) for memory-delta
        // accounting; matches the historic
        // `verter_session::component_meta_audit::mod` exemption
        // that lived here before the substrate split.
        "crates/verter_audit/src/memory.rs",
        // dev/CI-only, non-published Svelte CSS-conformance corpus
        // generator — reads/writes ONLY the crate-owned committed corpus
        // (`env!("CARGO_MANIFEST_DIR")/corpus`), never workspace/semantic/
        // overlay/VFS state. `D14_ALLOW_LIST` carries the full per-callsite
        // rationale (tool/output I/O stays on `std::fs`).
        "crates/verter_svelte_conformance/src/generate.rs",
        // dev/CI-only, non-published Vue conformance corpus READER —
        // reads ONLY the crate-owned vendored/committed corpus
        // (`env!("CARGO_MANIFEST_DIR")/corpus`: cases + official
        // goldens + dispositions), never workspace/semantic/overlay/VFS
        // state. Test-fixture I/O — sibling of the
        // `verter_svelte_conformance` generator entry.
        "crates/verter_vue_conformance/src/lib.rs",
        // dev-dependency-only shared test-harness crate — the only
        // `std::fs::` calls are inside `#[cfg(test)] mod tests`, a
        // self-test of the minted scratch path. No production-path
        // call, no VerterHost/WorkspaceAccess context to route
        // through. `D14_ALLOW_LIST` carries the full rationale.
        "crates/verter_test_support/src/lib.rs",
        // test/CI-only external workload probe lane — the crate's SOLE
        // disk boundary, through which its corpus adapter, its lane and
        // its summary binary all read and write. Reads a pinned
        // third-party corpus checkout (feature-gated), its own committed
        // manifests, and its own published summary artifact; never
        // workspace/semantic/overlay/VFS state, and no production crate
        // may depend on the crate. `D14_ALLOW_LIST` carries the full
        // rationale.
        "crates/verter_validation_probe/src/disk.rs",
        // test/CI-only process supervisor for compiler probes and
        // benchmarks — the crate's SOLE disk boundary, through which its
        // backends, its result writer and its fixture binary all read and
        // write. Touches only its own result/log files, the contained
        // workload's cgroup and `/proc` pseudo-files, and temp paths; never
        // workspace/semantic/overlay/VFS state, and no production crate may
        // depend on the crate. `D14_ALLOW_LIST` carries the full rationale.
        "crates/verter_supervise/src/disk.rs",
        // dev/CI-only semantic-perf benchmark harness — the harness's SOLE
        // disk boundary. Reads its own job file and scenario inputs,
        // writes its own record (aside, then renamed), and samples
        // `/proc/<pid>/status` on Linux; never workspace/semantic/
        // overlay/VFS state. `D14_ALLOW_LIST` carries the full rationale.
        "crates/verter_bench/src/semantic_perf/disk.rs",
    ]
    .into_iter()
    .map(String::from)
    .collect();
    set.extend(guard1_allowlist());
    set
}

/// Run guard 2 across every production `.rs` file in `crates/`
/// and return out-of-allowlist users of OS file APIs.
pub fn guard2_violations(allowlist: &BTreeSet<String>) -> Vec<String> {
    let crates_root = workspace_root().join("crates");
    let mut violations = Vec::new();
    let entries = match fs::read_dir(&crates_root) {
        Ok(it) => it,
        Err(_) => return violations,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let src_dir = path.join("src");
        if !src_dir.exists() {
            continue;
        }
        for file in walk_production_rs(&src_dir) {
            let src = match fs::read_to_string(&file) {
                Ok(s) => s,
                Err(_) => continue,
            };
            if !file_uses_os_file_api(&src) {
                continue;
            }
            let rel = relative_to_root(&file);
            if allowlist.contains(&rel) {
                continue;
            }
            violations.push(rel);
        }
    }
    violations.sort();
    violations
}

// ── Guard 4 — external_corpus_paths_not_present_outside_gated_tests ──

/// Predicate: scan a test file's source for path strings
/// referencing an external corpus — `.integration-tests/repos/...`
/// or a sibling third-party checkout beside the repo (`../vize/...`).
/// Returns `true` when at least one such reference is found in CODE
/// (line comments are stripped: documenting a deleted third-party
/// coupling is not a dependency) AND the file carries NO
/// external-corpus feature gate — neither a file-level `#![cfg(...)]`
/// inner attribute nor an item-level
/// `#[cfg(feature = "external-corpus")]` marker. Item-level detection
/// is best-effort textual (a gated mod exempts the whole file); the
/// discriminating target is a file with a live corpus path and no
/// gate anywhere — the historical `../vize` evasion shape.
pub fn test_file_has_ungated_external_corpus_path(src: &str) -> bool {
    let has_path = src.lines().any(|line| {
        let code = match line.find("//") {
            Some(idx) => &line[..idx],
            None => line,
        };
        code.contains(".integration-tests/repos/")
            || code.contains(".integration-tests\\repos\\")
            || code.contains("../vize")
            || code.contains("..\\vize")
            || code.contains("/vize/tests/")
            || code.contains("\\vize\\tests\\")
    });
    if !has_path {
        return false;
    }
    let gated = src.lines().any(|line| {
        let t = line.trim_start();
        t.starts_with("#![cfg(feature =")
            || t.starts_with("#![cfg(any(feature =")
            || t.starts_with("#[cfg(feature = \"external-corpus\")]")
            || t.starts_with("#[cfg(any(feature = \"external-corpus\"")
            || t.starts_with("#[cfg(all(test, feature = \"external-corpus\"")
    });
    !gated
}

/// Walk every Rust source file under `crates/<crate>/tests/` AND
/// `crates/<crate>/src/` (across all crates) and return the set of
/// files that violate the rule. `src/` is scanned because inline
/// `#[cfg(test)]` unit-test files can reference external corpora
/// just as easily as integration-test targets can — the
/// `../vize` sibling-checkout spelling historically evaded this
/// guard from `src/filesystem_tests.rs`. `architecture_guards.rs`
/// is self-exempt: it MUST hold the literal path string in the
/// predicate body, and the deliberate-violation test exercises
/// the predicate independently.
pub fn guard4_violations() -> Vec<String> {
    let crates_root = workspace_root().join("crates");
    let mut violations = Vec::new();
    let entries = match fs::read_dir(&crates_root) {
        Ok(it) => it,
        Err(_) => return violations,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let mut stack: Vec<std::path::PathBuf> = Vec::new();
        for scan_dir in ["tests", "src"] {
            let dir = path.join(scan_dir);
            if dir.exists() {
                stack.push(dir);
            }
        }
        while let Some(dir) = stack.pop() {
            let read = match fs::read_dir(&dir) {
                Ok(it) => it,
                Err(_) => continue,
            };
            for sub in read.flatten() {
                let p = sub.path();
                if p.is_dir() {
                    let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
                    if name == "fixtures" {
                        continue;
                    }
                    stack.push(p);
                    continue;
                }
                let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
                if !name.ends_with(".rs") {
                    continue;
                }
                // Self-exempt: this file MUST hold the predicate
                // literal.
                if external_corpus_guard_self_exempts(&p) {
                    continue;
                }
                let src = match fs::read_to_string(&p) {
                    Ok(s) => s,
                    Err(_) => continue,
                };
                if test_file_has_ungated_external_corpus_path(&src) {
                    violations.push(relative_to_root(&p));
                }
            }
        }
    }
    violations.sort();
    violations
}

// ── Guard 5 — verter_session_public_surface_is_minimal ──

/// Predicate: extract the set of `pub mod` and `pub use` items
/// declared at column 0 (top-level) of a Rust source file.
/// Items nested inside a block (e.g., `pub use ...` inside
/// `pub mod for_tests { ... }`) are excluded — only the
/// outermost surface is captured. Comments are ignored.
/// Returns the items in source order.
pub fn extract_top_level_pub_items(src: &str) -> Vec<String> {
    let mut items = Vec::new();
    let mut depth: i32 = 0;
    for line in src.lines() {
        // Strip trailing `//` line comment first.
        let no_comment = if let Some(idx) = line.find("//") {
            &line[..idx]
        } else {
            line
        };
        // Count brace deltas on the comment-stripped line. Track
        // depth so items inside any block are filtered out.
        let opens = no_comment.matches('{').count() as i32;
        let closes = no_comment.matches('}').count() as i32;

        let t = line.trim_start();
        let is_top = depth == 0
            && (line.starts_with("pub mod ")
                || line.starts_with("pub use ")
                || line.starts_with("pub(crate) mod "))
            && (t.starts_with("pub mod ")
                || t.starts_with("pub use ")
                || t.starts_with("pub(crate) mod "));
        if is_top {
            let stripped = if let Some(idx) = t.find("//") {
                &t[..idx]
            } else {
                t
            };
            let trimmed = stripped.trim_end_matches(';').trim_end();
            // Don't capture lines that begin a multi-line block
            // (e.g., `pub mod for_tests {`); the block-opening
            // form is a structural choice the snapshot tracks
            // separately. We still want to record the simple
            // declaration form, so check the line ends with `;`
            // OR `{`. For `{`, normalize to the bare module
            // name without the trailing `{`.
            let normalized = trimmed.trim_end_matches('{').trim_end().to_string();
            items.push(normalized);
        }
        depth += opens - closes;
        if depth < 0 {
            depth = 0;
        }
    }
    items
}

/// Snapshot of the current `verter_session` lib public surface.
/// Generated via `extract_top_level_pub_items` against
/// `crates/verter_session/src/lib.rs` at landing.
/// Updates require a deliberate edit + paired snapshot bump.
pub fn guard5_snapshot_pub_items() -> &'static [&'static str] {
    VERTER_SESSION_PUB_SURFACE_SNAPSHOT
}

pub static VERTER_SESSION_PUB_SURFACE_SNAPSHOT: &[&str] = &[
        // Each retained `pub mod` MUST cite at least one downstream
        // consumer (verter_lsp, verter_mcp, verter_napi, verter_wasm,
        // verter_ffi, verter_diagnostics, verter_type_runtime,
        // verter_tsc) OR a verter_session integration test. Items
        // demoted to `pub(crate)` by Phase 12.A9 do not appear here.
        // ─── public modules: cited consumers ────────────────────────
        // tests/cases/g_misc0/audited_request_e2e.rs
        "pub mod audited_request",
        // Family-A `BinderIdentityFacts` substrate (scope tree /
        // `DeclarationSlotSeed`s / provenance + the artifact store).
        // Public so `tests/cases/g_binder/binder_identity_facts.rs` can
        // drive the demand producer + read the artifact payload.
        "pub mod binder_identity_facts",
        // Frozen eight-field carrier-owned compatibility cohort consumed by
        // B2 persistence/adoption and exercised by its owning module tests.
        "pub mod carrier_artifact_cohort",
        // T-B R5 §2 carrier-only publication identity/store and typed interim outcomes.
        "pub mod carrier_publication_store",
        // verter_lsp::features::hover_provenance,
        // `assemble_vue_main_module` — re-exported so the Vue conformance
        // seed harness (`verter_vue_conformance/tests/cases/seed_conformance.rs`)
        // and session assembly tests drive the GENUINE shipped runtime-Main
        // assembly (compile → bundle → assemble) instead of a hand copy.
        // Test-support public API (consumer: verter_vue_conformance dev-dep).
        // Alongside it, the typed code-plus-map result and `MapFragment`
        // (named by `AssembleMapFailure`'s own variants, still reachable
        // through `VueMainAssemblyFailure::InputMap`): both appear in the
        // assembler's own signature, so a caller outside this crate cannot
        // name its return type without them.
        "pub use compile::{assemble_vue_main_module, AssembleMapFailure, AssembledVueModule, MapFragment}",
        // Sealed request-scoped native host binding substrate
        // (`BoundNativeHostRequest` + its typed unavailable outcomes).
        // Public so the out-of-crate seal is provable: the trybuild
        // fixture `tests/cases/compile-fail/native_host_binding_sealed.rs`
        // must NAME the type to prove it is not Clone/Copy/serializable
        // and that the framework-specific host binding is unreachable
        // outside the single by-value consumption seam.
        "pub use host_resolve::native_host_binding::",
        // The exhaustive uncomposable-input-map taxonomy carried by
        // `AssembleMapFailure`, so a caller can discriminate the exact sub-code
        // and its family rather than matching on a rendered message. Also
        // `SfcRewriteRefusal`, carried by `AssembleMapFailure::InvalidSfcExportPlacement`
        // for the same reason: a caller needs to name the reason type to
        // discriminate it, not just match on a rendered message.
        "pub use compile::{SfcRewriteRefusal, UncomposableCode, UncomposableFamily}",
        // `VueMainAssemblyFailure` — the fail-closed outcome
        // `assemble_vue_main_module` actually returns (every internal
        // failure category — input-map/`__sfc__`-rewrite, fragment-grammar,
        // composition, publication — propagates through this ONE typed enum
        // rather than a panic), so a caller outside this crate must be able
        // to name it to match on it.
        "pub use compile::VueMainAssemblyFailure",
        // Stamped handoff callers hash code and maps with the host-owned domain.
        "pub use block_content::hash_block_content",
        // verter_napi::meta, verter_wasm::tests::audit
        "pub mod component_meta_audit",
        // verter_napi::meta
        "pub mod component_meta_host",
        // host_audit_runtime — owns the AuditRecordsStore + AuditConfig
        // snapshot + active-request registry. Public so integration
        // tests can call `host.host_audit_runtime().snapshot()` and
        // so `AuditRequestRegistration::new` resolves through the
        // public accessor.
        "pub mod host_audit_runtime",
        // Re-exports the new audit-runtime types at the crate root
        // (`verter_session::HostAuditRuntime`, `AuditRequestRegistration`,
        // `AuditRuntimeSnapshot`) so callers of the audited
        // entry-points do not need to reach into the module path.
        "pub use host_audit_runtime::",
        // tests/type_resolution_audit_*.rs — public audited
        // entry-point that wires `VerterHost::resolve_type_with_audit`
        // for type-resolution requests. Producer side of the
        // `RequestKind::TypeResolution` audit kind.
        "pub mod host_resolve_type_audit",
        // Public audited entry-point that wires
        // `VerterHost::get_flow_return_type_with_audit` (the single
        // public flow-return seam) and its typed `FlowReturnError`.
        // Producer side of the `RequestKind::FlowReturnInference`
        // audit kind.
        "pub mod host_flow_return_audit",
        // verter_ffi::convert (host::cross_file::CrossFileResult)
        "pub mod cross_file",
        // The content-mapper projection plane. Public because the plane is a
        // named boundary of the carrier-projection contract in its own right —
        // one carrier surface's retained geometry plus the fail-closed wire
        // semantics for asking it a position question — and because it must be
        // reachable, and provable, without any type engine running. No
        // production route inside this crate reaches it yet.
        "pub mod content_mapper",
        // Project-bound external-TypeScript-engine contract: the
        // provider-neutral three-layer seam (`ExternalTsProjectResolver` /
        // `CarrierRegistry` / `EngineBackend`) in which a config-less op for a
        // production carrier source is unrepresentable. Additive — the contract
        // is defined but not yet wired live over the inferred LSP path.
        "pub mod external_ts",
        // Fact-based cache architecture (R5, R6, R28, R29) — new
        // content-addressed file artifact store + per-file structural
        // hash, both consumed by tests and by future-stage host paths.
        "pub mod file_artifact_store",
        // Framework adapter substrate: host-level language
        // classification (`HostLanguageClassifier` composing the static
        // `LanguageRegistry` with the `ProjectCapabilitySnapshot`).
        // Consumed by host construction, the scheduler SourceLoader
        // seam, and session-level classification call sites.
        "pub mod framework",
        // The open language descriptor surface re-exported from the
        // leaf routing authority so host-API consumers (UpsertRequest
        // construction, FFI conversion, LSP/MCP) name one definition
        // without a direct `verter_language` dependency.
        "pub use verter_language::",
        "pub mod parse_stable_hash",
        // Parse-time fact-emission producer (R10–R16, R28, R29) —
        // walks `IndexedReady.shallow_state` and populates the
        // per-file `FactRegistry` + module-augmentation facts.
        // Consumed by tests/fact_*.rs + tests/cases/g_misc2/module_augmentation.rs +
        // tests/cases/g_misc2/shallow_walk_invariant.rs + future host paths.
        "pub mod fact_emission",
        // Complete global contributor snapshots published at artifact
        // ingestion. Cited by tests/cases/g_block/semantic_determinism_matrix.rs.
        "pub mod global_contributors",
        // Lazy member-body fact stores (R13, R28) — semantic and
        // display fingerprint stores keyed differently so cosmetic
        // edits invalidate display-bearing materialisations only.
        // Consumed by tests/cases/g_fact/fact_semantic_display_split.rs +
        // tests/cases/g_misc1/member_presence_vs_member.rs + future resolver
        // / materialiser admission paths.
        "pub mod member_display_fact_store",
        "pub mod member_semantic_fact_store",
        // Resolve-domain authoritative cache for resolved import /
        // re-export bindings + per-specifier resolutions (R5, R12,
        // R21, R28). Public because
        // tests/cases/g_resolved/resolved_import_facts_invariants.rs and
        // tests/cases/g_resolved/resolved_import_facts_key_shape.rs consume
        // `ResolvedImportFactsDb`, `ResolvedImportFactsKey`, and
        // `RESOLVED_IMPORT_FACTS_RESOLVER_VERSION` through the
        // canonical module path; future resolver consumers (RouteDb
        // fact-validation, materialiser observations) reach the
        // same module.
        "pub mod resolved_import_facts",
        // tests/cases/g_resolved/resolved_import_facts_producer_real.rs +
        // tests/cases/g_resolved/resolved_import_facts_unresolved_admitted.rs +
        // tests/cases/g_resolved/resolved_import_facts_validator_real_path.rs +
        // tests/cases/g_resolved/resolved_import_facts_lane_population.rs +
        // tests/cases/g_resolved/resolved_import_facts_namespace_space_admitted.rs —
        // production producer for
        // `ResolvedImportFactsDb`. Reads
        // `script_analysis.imports` (with `is_type_only` +
        // `ImportBindingKind::{Named, Default, Namespace}`),
        // classifies each binding into
        // `SymbolSpace::{Type, Value, Namespace}` (v8
        // AMENDMENT-S), composes the cache key from the
        // per-canonical env hashes, constructs one
        // `ResolvedImportClauseEntry` per `(binding, space)` pair,
        // and admits the bundle through
        // `ResolvedImportFactsDb::insert_if_absent`. Negative
        // (unresolved) imports are admitted as entries with
        // `resolved_canonical: None` and a sentinel-keyed `Fact`
        // so the validator can detect when an unresolved binding
        // becomes resolved on workspace bump.
        "pub mod resolved_import_facts_producer",
        // tests/cases/g_misc0/semantic_analysis_audit_e2e.rs +
        // tests/cases/g_misc0/semantic_analysis_audit_tls_propagation.rs — public
        // audited entry-point that wires
        // `VerterHost::analyze_with_audit` for semantic-analysis
        // requests. Producer side of the
        // `RequestKind::SemanticAnalysis` audit kind.
        "pub mod host_analyze_audit",
        // verter_session::host_audit_bridge — owns the
        // `MacroExpansionDiagnostics → AuditDiagnosticEntry` projection
        // consumed by the audited component-meta entry-point. Public
        // so the in-process bridge tests + the audited consumer
        // wiring (component_meta_audit producer) can pick the helper
        // up by canonical path.
        "pub mod host_audit_bridge",
        // tests/cases/g_misc0/host_tests.rs (host_compile module surface)
        "pub mod host_compile",
        // `compile_with_audit` entry-point. Public because
        // tests/compile_audit_*.rs and tests/cases/g_misc0/tls_harness_cross_crate.rs
        // drive the audited compile path; consumer crates (verter_napi,
        // verter_lsp) pick this up through their audited surfaces.
        "pub mod host_compile_audit",
        // verter_lsp::server::nav_features_audit drives audited LSP
        // handlers through `VerterHost::lsp_audit_begin` exposed by
        // this module. Public because `verter_lsp::audit_harness` and
        // the `tests/lsp_audit_*.rs` integration tests reach
        // `LspAuditSession` through the canonical module path.
        "pub mod host_lsp_audit",
        // tests/cases/g_misc0/host_tests.rs (host_manage::* APIs in integration tests)
        "pub mod host_manage",
        // Slice 3.F — `audit_mcp_tool_call` entry-point. Public so
        // verter_mcp tool handlers can route their audited body
        // through the wrapper and land RequestKind::Mcp records on
        // the host store. Tests in tests/cases/g_misc0/mcp_audit_e2e.rs and
        // verter_mcp/tests/cases/mcp_tool_audit_integration.rs exercise
        // the production wiring.
        "pub mod host_mcp_audit",
        // verter_type_runtime::backend::tests via meta_resolve types,
        // tests/cases/g_misc0/host_tests.rs
        "pub mod meta_resolve",
        // `OwnedEvalProgram` owned-artifact module. Public so the
        // owned post-lowering IR is nameable by consumers in
        // `verter_type_runtime` / `verter_napi` and by the Tier 1A
        // lowering-boundary guards.
        "pub mod owned_artifacts",
        // Tier 1B — selective component-meta surface API + BFS bridge
        // wire types (D102 + D125). Public because verter_napi calls
        // `TypeHandle::from_proto` / `to_proto`, surface envelope
        // round-trip, and `BridgeError`/`TypeHandleError` envelopes
        // through this module's public types.
        "pub mod component_meta_payload",
        // Tier 1B — `MetaSession` made public so the selective surface
        // API methods (`get_component_meta_surface`,
        // `get_component_meta_type_expansion`,
        // `get_component_meta_payload_via_bridge`) are reachable from
        // verter_napi (NapiMetaSession), the LSP custom-method handler
        // chain, and the integration test target
        // `tests/cases/g_misc0/selective_component_meta_api.rs`.
        "pub mod meta",
        // tests/cases/g_misc0/host_tests.rs (project_type_store::*)
        "pub mod project_type_store",
        // verter_type_runtime, verter_napi (TypeExpander API);
        // tests/cases/g_misc0/host_tests.rs
        "pub mod resolver_core",
        // verter_semantic route extraction consumes the owned snapshot this
        // module builds: the extractors take `&RouteAnalysisInputs`
        // instead of a live `&dyn WorkspaceRead`.
        "pub mod route_analysis_inputs",
        // The TypeScript semantic capability closure (dormant until TCM4) —
        // the closed capability catalog plus the certified engine binding
        // that is the sole route a TypeScript engine's answer enters the
        // semantic plane. Public for the same reason `content_mapper` is: a
        // named dual-plane boundary that must stay reachable, and provable,
        // with no engine running and no production route inside this crate
        // reaching it yet. Cited by
        // tests/g_extts/semantic_capability_closure.rs and the
        // tests/cases/compile-fail/certified_binding_struct_literal_forge.rs
        // trybuild fixture.
        "pub mod semantic_capability",
        // tests/cases/g_session/committed_input_basis.rs — session bind of
        // one committed InputBasis + SnapshotFence (crate-root re-exports
        // wrap, so the line-based extractor records the bare prefix).
        "pub mod input_basis",
        "pub use input_basis::",
        // crates/verter_wasm/src/input_snapshot.rs — the browser
        // acquisition boundary commits asynchronously-acquired rows
        // through the session handoff core (AcquiredFile /
        // CommittedInputHandoff / HandoffObserve, imported via the
        // module path; no crate-root re-export of this family exists).
        "pub mod input_handoff",
        // crates/verter_wasm/src/lib.rs (input_snapshot_receipt) — the
        // wasm commitInputSnapshot export binds its receipt to the
        // host's platform-services profile through
        // VerterHost::platform_services(), so PortableHostServices and
        // its route vocabulary are named outside this crate.
        "pub mod platform_services",
        // tests/cases/g_session/cooperative_drive_seam.rs — the host
        // load-seam discriminators (cancelled drive refuses
        // ensure_loaded admission; constructor-time yield hook
        // installed through HostConfig::cooperative_yield) drive
        // VerterHost::cooperative_drive and implement its
        // CooperativeYield hook from the integration binary.
        "pub mod cooperative_scheduler",
        // Block 1.H Track 2.4 — `AppConfigNoOverrideProofKey`,
        // `AppConfigNoOverrideProofEntry`, `AppConfigNoOverrideProofDb`
        // surface for the family_bcd_* integration tests that drive
        // the production producer end-to-end via
        // `for_tests::app_config_no_override_proof_get_or_compute_for_tests`.
        "pub mod app_config_proof_db",
        // verter_napi::typeinfo, verter_wasm::typeinfo, packages/typeinfo
        // — the §5 Phase 3 typeinfo public host substrate
        // (list_file_symbols, resolve_named_symbol*, evaluate_type_expression*)
        // exposed via `pub mod typeinfo`. Required by NAPI/WASM bindings
        // and the `@verter/typeinfo` TS package.
        "pub mod typeinfo",
        // ─── B-C5 territory (separate ownership), kept `pub` ────────
        "pub mod component_meta_resolution_policy",
        // ─── crate-private modules (already non-public) ─────────────
        // tests/cases/g_cache/cache_invariant_migration.rs — the W0.5 schema-bump
        // cohort fixture exercises `ComponentMetaResultDb::evict_if_schema_mismatch`.
        "pub mod component_meta_result_db",
        // `#[cfg(test)]`-only acceptance suite for the flow-return
        // projector admission (`ReturnType<typeof callee>` published
        // member demand). `pub(crate)` so the typed guard-registry
        // lib binding table (`typeinfo_guard_bindings_tests.rs`) can
        // bind `no_flow_slot_in_published_type_surface` by fn path;
        // never compiled into the production lib (cfg(test)-gated).
        "pub(crate) mod component_meta_flow_return_admission_tests",
        // R3/R26/R28 compile-tier fact-observation helper module.
        // Wraps the compile cold-compute pass in
        // `with_fact_tracer` and emits per-`Member`/`MemberPresence`,
        // `ImportRef`, and `ModuleAugmentationIndexShape` observations
        // so cross-file edits invalidate the consumer's CompileSlot
        // via warm-hit fact-validation without eager invalidation.
        "pub(crate) mod compile_fact_emission",
        // Compile-cache mode classifier — the sole authority for the
        // `CompileCacheMode` downgrade decision. `pub(crate)`: the
        // classification type is an in-crate implementation detail; the
        // public compile result surface carries only the projected
        // `actual_mode` + `Option<DowngradeReason>` fields.
        "pub(crate) mod compile_cache_mode",
        // host batch-coordinator primitive — the single owner of
        // outer-coordinator batch fan-out (component-meta batch + batch
        // compile route through it). Crate-internal: callers reach it
        // via `VerterHost::batch_coordinator()`.
        "pub(crate) mod host_batch_coordinator",
        "pub(crate) mod host_executor",
        "pub(crate) mod host_test_audit",
        // Host-specific per-host force-injection knobs (`TestForceKnobs`), grouped
        // off the root `VerterHost` so the struct stays thin. The module and the
        // `VerterHost` field are test-support gated, so a release build carries
        // none of it; the `mod` declaration itself is the only public-surface trace.
        "pub(crate) mod host_test_force",
        // tests/cases/g_cache/cache_invariant_migration.rs — the W0.5 schema-bump
        // cohort fixture exercises `OwnerImportSurfaceDb::evict_if_schema_mismatch`.
        "pub mod owner_import_surface",
        // The session-side implementation of the query-owned host port
        // (`verter_session_query::QueryHostPort`): `pub` because the
        // composition root above BOTH crates constructs
        // `SessionQueryHostPort` and hands it to the query layer — the
        // inversion-of-control seam is a public surface by design (caller
        // wiring lands with the query-layer adoption).
        "pub mod query_host_port",
        // Stage 4a SessionView trait surface — `HostView` and
        // `OverlaidView` impls. `pub` because the integration smoke
        // test `tests/cases/g_session/session_view_smoke.rs` consumes the trait
        // directly via `verter_session::session_view::SessionView`.
        // Stages 4b/4c thread the trait through `ResolverContext`
        // and `HostFenceValidator`; Stage 4d retires the
        // overlay-mutation machinery the trait replaces.
        "pub mod session_view",
        "pub(crate) mod template_convert",
        // ─── test-only re-export shim ──────────────────────────────
        "pub mod for_tests",
        // ─── test-support submodules (gated cfg(any(test, debug_assertions))) ──
        // Hosts the reusable TLS observer-propagation harness consumed
        // by `tests/cases/g_misc0/tls_harness_in_crate.rs` and
        // `tests/cases/g_misc0/tls_harness_cross_crate.rs`. The harness is
        // `pub mod tests` (rather than `pub(crate)`) only so the
        // integration tests under `crates/verter_session/tests/*.rs`
        // can reach it; release builds drop the entire module
        // because `debug_assertions` is OFF in release.
        "pub mod tests",
        // Criterion-only projection primitive harness. The module is compiled
        // solely by the explicit `test-support` feature used by the checked-in
        // projection safety benchmark; default production builds do not expose
        // it. Body now lives in `for_tests.rs` (re-exported here at the
        // crate root so `benches/projection_safety_bench.rs` keeps a
        // stable `verter_session::projection_bench_support` path).
        "pub use for_tests::projection_bench_support",
        // Test-support-only shared execution substrate used by the generated
        // component-meta corpus chunks. The public re-export exists only under
        // `cfg(any(test, feature = "test-support"))`; ordinary production builds
        // do not expose the pool identities or constructors.
        "pub use test_worker_pools::",
        // Test-only probe substrate for the content-addressed
        // `MapperFingerprint` primitive. Consumed by
        // `tests/cases/g_misc3/mapper_fingerprint_content_addressed.rs`. The
        // module is `#[doc(hidden)]` and wraps the internal
        // `pub(crate)` `MapperFingerprint` / `MapperBinderRegistry`
        // in a newtype so production callers cannot reach the
        // inner types through it. Not a production API.
        "pub mod test_only",
        // ─── public re-exports ─────────────────────────────────────
        // re-exports the canonical data types (HostConfig, VerterHost,
        // UpsertRequest, FileLanguage, CompileProfile, CompileErrorPolicy,
        // DependencyResolution, DiagnosticsSnapshot, HostDiagnostic,
        // HostSeverity, FileAnalysisSnapshot, ...) — universally used.
        "pub use types::*",
        // verter_lsp::background_init,
        // verter_type_runtime::tsserver::ipc, verter_type_runtime::tsgo::ipc
        // + verter_lsp::server::nav_features_navigation (the GlobalComponents
        // fallback-const NAV-PROBE locator — the compiler-owned emission-
        // contract reader backing global-component tag go-to-definition)
        "pub use verter_compiler::{global_component_nav_probe_offset, VERTER_TYPES_STANDALONE_DTS}",
        // verter_lsp::lib (public-API projection subject on the
        // profile-aware projection entry) — the dependency-neutral
        // failure/subject carrier re-exported so adapters do not
        // depend on verter_protocol internals.
        "pub use verter_protocol::types::PublicApiProjectionSubject",
        // tests/cases/g_misc0/relative_path_session_parity.rs
        "pub use id::resolve_external",
        // TS7 oracle harness snapshot GENERATOR entries — `pub` ONLY under the
        // `oracle-gen` feature (off the default closure), so the
        // `src/bin/oracle_gen` + `src/bin/oracle_upgrade` binaries (separate
        // crates that see only non-test `pub` lib items) can invoke them. The
        // default build never compiles them. `upgrade_snapshots_to_v4` is the
        // schema-v4 tsgo-free re-key (supersedes the v2→v3 re-key export).
        "pub use crate::typeinfo::oracle_core::gen::{run_oracle_gen, upgrade_snapshots_to_v4, GenError}",
        // Resolver-store instrumentation re-exports, one wrapped statement:
        // the per-call-site `HostStoreView::from_host` attribution table
        // (`dump_from_host_call_sites` / `reset_from_host_call_sites`,
        // dumped by crates/verter_bench/examples/audit_real_component_meta.rs
        // via the `#[track_caller]` rail) plus the base-view sweep counter
        // (`store_view_coherent_build_sweeps` +
        // `reset_store_view_coherent_build_sweeps`, one bump per
        // `build_coherent` sweep — a batch-saturation gate asserts a warm
        // batch collapses onto ~O(1) full-workspace sweeps). The statement
        // wraps to multiple lines (long symbol names), so the line-based
        // surface extractor normalizes it to the bare
        // `pub use resolver_store::` prefix.
        "pub use resolver_store::",
        // The committed source content a foreground request records as dependency
        // evidence, consumed by verter_lsp's `SourceFeatureDocumentCapture::expected_source_hash`
        // and the child-contract snapshot fields.
        "pub use host_views::CommittedSourceContent",
        // The captured host authority a foreground request settles against
        // (`VerterHost::capture_authority_view`), consumed by verter_lsp's
        // foreground request context and imported-child-contract freshness
        // key.
        "pub use resolver_store::{HostAuthority, HostAuthorityView}",
        // NOTE: the session-overlay copy-on-write counter is intentionally
        // ABSENT from this surface. It was retired as a process-global
        // re-export and rehomed PER-HOST onto
        // `VerterHost::provenance().session_overlay_cows`
        // (`crate::types::MetaProvenance`) so the batch regression gate
        // measures only its own host's overlay COWs — worker-side per-job
        // COWs included, other hosts' (other tests') excluded. No
        // `pub use resolver_store::{*session_overlay_cows*}` entry exists.
        // ─── crate-private host modules ──────────────────────────────
        "pub(crate) mod compile_output_node",
        "pub(crate) mod component_meta_cached_result",
        "pub(crate) mod component_meta_result_admission",
        "pub(crate) mod host_source_demand",
        "pub(crate) mod meta_provenance",
        "pub(crate) mod output_sinks",
        "pub(crate) mod session_attachment",
        "pub(crate) mod session_vfs_sink",
        // `ReadSetSignature` — the return type of the public fact-signature
        // inspectors, named by the host crate's integration tests.
        "pub use verter_session_query::facts::fact_cache::ReadSetSignature",
    ];

/// Compare the live surface against the snapshot; report any
/// differences (missing or added items).
pub fn guard5_drift(live: &[String], snapshot: &[&str]) -> (Vec<String>, Vec<String>) {
    let live_set: BTreeSet<&str> = live.iter().map(|s| s.as_str()).collect();
    let snap_set: BTreeSet<&str> = snapshot.iter().copied().collect();
    let added: Vec<String> = live_set
        .difference(&snap_set)
        .map(|s| (*s).to_string())
        .collect();
    let removed: Vec<String> = snap_set
        .difference(&live_set)
        .map(|s| (*s).to_string())
        .collect();
    (added, removed)
}

// ── Member-visibility constructor guard ──
//
// B4.5 makes silent-Public member construction IMPOSSIBLE in production:
// the implicit-Public `ObjectProperty`/`MethodSignature` constructors were
// split into intent-explicit names (`synthetic_public_key` / `with_key_spans_public`
// for genuinely source-less public origins; `synthetic_key_with_visibility` /
// `with_key_visibility` for source-derived reconstruction that threads the
// member's declared accessibility). This guard pins that split: a bare
// `ObjectProperty::synthetic(` / `MethodSignature::synthetic(` /
// `ObjectProperty::with_spans(` / `MethodSignature::with_spans(` in any
// production source file is banned, so a future reconstruction site cannot
// silently mint a non-public member as `Public` (the recurring leak class
// three review rounds chased site-by-site). `IndexSignature::synthetic(` /
// `::with_spans(` are NOT banned — index signatures carry no accessibility.

/// Predicate: returns `true` when `line` references one of the four banned
/// implicit-Public member constructors. The explicit `_public` /
/// `_with_key_visibility` suffixed forms are allowed (the needle `synthetic(`
/// does not substring-match `synthetic_public_key(`, because the byte after
/// `synthetic` is `_`, not `(`; likewise `with_spans(` vs
/// `with_key_spans_public(`). `with_key_visibility(` shares no banned needle.
pub fn line_has_banned_visibility_constructor(line: &str) -> bool {
    const BANNED: &[&str] = &[
        "ObjectProperty::synthetic(",
        "MethodSignature::synthetic(",
        "ObjectProperty::with_spans(",
        "MethodSignature::with_spans(",
    ];
    BANNED.iter().any(|needle| line.contains(needle))
}

/// Walk the production tree and return `(rel_path, line_no, line)` triples
/// for every banned-constructor reference.
pub fn member_visibility_constructor_violations() -> Vec<(String, usize, String)> {
    let crates_root = workspace_root().join("crates");
    let mut violations = Vec::new();
    let entries = match fs::read_dir(&crates_root) {
        Ok(it) => it,
        Err(_) => return violations,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let src_dir = path.join("src");
        if !src_dir.exists() {
            continue;
        }
        for file in walk_production_rs(&src_dir) {
            let src = match fs::read_to_string(&file) {
                Ok(s) => s,
                Err(_) => continue,
            };
            let rel = relative_to_root(&file);
            for (idx, line) in src.lines().enumerate() {
                if line_has_banned_visibility_constructor(line) {
                    violations.push((rel.clone(), idx + 1, line.to_string()));
                }
            }
        }
    }
    violations.sort();
    violations
}

// ── Same-crate member struct-literal guard (B4.5 by-construction) ──
//
// `ObjectProperty` / `MethodSignature` are `#[non_exhaustive]` and carry a
// mandatory `visibility` field with no `Default`, so DOWNSTREAM crates
// cannot construct them with a struct literal (they must route through the
// visibility-threading constructors `synthetic_public_key` /
// `synthetic_key_with_visibility` / `with_key_spans_public` / `with_key_visibility`).
// `#[non_exhaustive]` does NOT apply WITHIN the defining crate, so a future
// SAME-CRATE site in `verter_type_expr` could still write
// `ObjectProperty { .. }` / `MethodSignature { .. }` directly and silently
// mint a member with an unconsidered visibility — re-opening the leak class
// the downstream guard closed. This guard pins the same-crate gap: inside
// `crates/verter_type_expr/src/**`, the ONLY permitted occurrence of
// `ObjectProperty {` / `MethodSignature {` is the `pub struct <Name> {`
// type DEFINITION; the constructors build via `Self { .. }`, so a named
// struct literal anywhere in the crate is a violation. Together with the
// downstream constructor guard above, the member-visibility construction
// surface is now COMPLETE (no construction/struct-literal bypass, in any
// crate).

/// Predicate: returns `true` when `line` is a SAME-CRATE named struct
/// literal of `ObjectProperty` / `MethodSignature` (the banned construction
/// form inside `verter_type_expr`), and `false` for the `pub struct <Name>
/// {` type definition (the sole allowed `<Name> {` occurrence) and for any
/// other line. The constructors use `Self { .. }`, which shares no needle
/// with `ObjectProperty {` / `MethodSignature {`.
pub fn line_has_same_crate_member_struct_literal(line: &str) -> bool {
    let trimmed = line.trim_start();
    // The type DEFINITION (`pub struct <Name> {`) and the inherent-impl
    // opener (`impl <Name> {`) are the allowed `<Name> {` occurrences — the
    // constructors inside the impl build via `Self { .. }`, never the named
    // form. Allow both `struct` and `impl` headers for either type.
    for header in ["pub struct ", "struct ", "impl "] {
        if trimmed.starts_with(&format!("{header}ObjectProperty"))
            || trimmed.starts_with(&format!("{header}MethodSignature"))
        {
            return false;
        }
    }
    // A function RETURN type whose body opens on the same line
    // (`fn foo() -> ObjectProperty {`) names the type, it does not construct
    // it — the `{` is the fn body brace, not a struct-literal opener. Allow
    // `-> <Name> {`.
    if trimmed.contains("-> ObjectProperty {") || trimmed.contains("-> MethodSignature {") {
        return false;
    }
    // Any other line containing a named struct-literal opener is banned.
    // `ObjectProperty {` does NOT substring-match `ObjectProperty::`
    // (constructor calls) because the byte after the name is `:`, not
    // ` {`, and does NOT match `ObjectPropertyOrigin {` because the needle
    // includes the trailing space + brace.
    line.contains("ObjectProperty {") || line.contains("MethodSignature {")
}

/// Walk `crates/verter_type_expr/src/**` production files and return
/// `(rel_path, line_no, line)` triples for every same-crate member
/// struct-literal violation.
pub fn same_crate_member_struct_literal_violations() -> Vec<(String, usize, String)> {
    let src_dir = workspace_root()
        .join("crates")
        .join("verter_type_expr")
        .join("src");
    let mut violations = Vec::new();
    for file in walk_production_rs(&src_dir) {
        let src = match fs::read_to_string(&file) {
            Ok(s) => s,
            Err(_) => continue,
        };
        let rel = relative_to_root(&file);
        for (idx, line) in src.lines().enumerate() {
            if line_has_same_crate_member_struct_literal(line) {
                violations.push((rel.clone(), idx + 1, line.to_string()));
            }
        }
    }
    violations.sort();
    violations
}

// ── Guard D14 — no_std_fs_outside_native_fs_or_allow_list ──
//
// The NativeFs invariant lock. The single legitimate disk-touch
// boundary is `crates/verter_workspace/src/native_fs.rs` (the
// `NativeFs` wrapper). Every other production-source file that
// contains a `std::fs::` reference must appear in `ALLOW_LIST`
// below with an explicit justification. New escapes from NativeFs
// are visible as one new constant entry per file in the diff;
// there is no opaque TOML or external-file route around the
// invariant.
//
// The ALLOW_LIST coexists with guard 1 (TOML-driven, scoped to
// `verter_session` / `verter_semantic` / etc.) and guard 2
// (in-source allowlist, broader OS-file-API scope including
// `tokio::fs::`). This guard is the strictest of the three and
// the ONLY one whose justifications live next to the path in
// source code, by design — the brief (D14) requires per-callsite
// visibility for the lock.

/// Path (relative to workspace root) that is exempt from the
/// guard. Calls to `std::fs::` here ARE the canonical disk
/// boundary that `NativeFs` wraps.
pub const D14_NATIVE_FS_PATH: &str = "crates/verter_workspace/src/native_fs.rs";

/// `(file_path, justification)` enumeration of every production
/// file outside `native_fs.rs` that legitimately contains a
/// `std::fs::` reference. Adding an entry must be paired with a
/// rationale that the reviewer can read at the diff.
///
/// File-path strings use forward slashes and are relative to the
/// workspace root (matches `relative_to_root`).
///
/// Adding a new entry is a deliberate, visible widening of the
/// NativeFs invariant. Removing an entry must be paired with a
/// code change that routes the I/O through `NativeFs` /
/// `WorkspaceAccess` (or a deletion of the callsite).
pub const D14_ALLOW_LIST: &[(&str, &str)] = &[
        (
            "crates/verter_bench/src/semantic_perf/disk.rs",
            "Semantic-perf benchmark harness disk boundary (dev/CI-only bench crate, never published) — the SOLE module in the harness that touches the disk; its CLI, runner and process sampler all route through it. It reads the harness's own job file and generated scenario inputs, writes its own result record (written aside, then renamed over the target), and reads the kernel pseudo-file `/proc/<pid>/status` on Linux to sample the measured process. Benchmark tooling I/O on its own artifacts, never workspace, semantic, overlay or VFS state; routing it through the disk boundary would give a measurement harness a session it has no other use for and put a path cache in the measured path.",
        ),
        (
            "crates/verter_napi/src/bin/generate_host_compile_request_ts.rs",
            "Declaration generator, not a runtime path. It is a `[[bin]]` behind `required-features = [\"generate-host-request-ts\"]`, so it is absent from every default build including the published addon; a developer runs it to rewrite one committed file whose bytes a freshness guard then pins. The SOLE `std::fs::` call writes that generated declaration to a path derived from the manifest directory. It touches no workspace, semantic, overlay or VFS state, and routing a build-time source-tree write through the disk boundary would give a generator a session it has no other use for.",
        ),
        (
            "crates/verter_bench/src/css_gate.rs",
            "Measurement-runner provenance probe (dev/CI-only bench crate, never published). The SOLE `std::fs::` call reads `/proc/loadavg` on Linux to stamp system load into a captured measurement record, mirroring the macOS branch that shells out to `sysctl -n vm.loadavg`. A kernel-synthesised pseudo-file describing the MACHINE, not workspace, semantic, overlay or VFS state — and routing it through the disk boundary would key a path cache on a file whose contents differ on every read. The runner's own artifact I/O (reading a committed baseline, writing a captured record) routes through `NativeFs` and is deliberately NOT covered by this entry.",
        ),
        (
            "crates/verter_session/src/typeinfo/oracle_core/driver.rs",
            "TS7 oracle harness consumption driver (`#[cfg(test)] mod typeinfo_tests`) — loads checked-in snapshot TEST FIXTURES + re-enumerates the vendored env corpus via runtime `std::fs::read`, the mechanism the locked design (the TS7 oracle contract §Q1) mandates and the `snapshot_loading_is_runtime_fs` guard pins. Not a NativeFs/VFS disk-boundary bypass — it reads in-repo test fixtures, never workspace/semantic state.",
        ),
        (
            "crates/verter_session/src/typeinfo/typeinfo_tests/oracle_toolchain_lock.rs",
            "Oracle toolchain LOCK (`#[cfg(test)]` typeinfo test). Re-reads the checked-in toolchain record TEST FIXTURE (oracle_toolchain.json), the workspace package.json pin, and the INSTALLED `@typescript/typescript-<platform>` package bytes under node_modules to re-hash them against the record — the evidence manifest's installed-bytes clause. External-toolchain + in-repo test-fixture I/O with no VerterHost/NativeFs context, never workspace/semantic state — the same category as the TS7 oracle harness fixture readers above.",
        ),
        (
            "crates/verter_session/src/typeinfo/typeinfo_tests/oracle_gen_spike.rs",
            "TS7 oracle harness §4 GENERATION SPIKE (`#[cfg(all(test, feature = \"oracle-gen\"))]`, excluded from the default gate). Writes a tsconfig + fixture into a temp dir for the EXTERNAL tsgo subprocess to read off real disk (tsgo cannot read Verter's in-memory VFS), then re-validates the design's BLOCKING tsgo assumptions. External-tool scaffolding, not a NativeFs/VFS disk-boundary bypass — never workspace/semantic state.",
        ),
        (
            "crates/verter_session/src/bin/oracle_lift.rs",
            "TS7 oracle harness AUDITED LIFT COMMAND (`#[bin]` behind `required-features = [\"oracle-lift\"]`, excluded from the default build and the default gate). Reads the ORIGINAL `#[ignore]`d row body and the checked-in snapshot TEST FIXTURES, and rewrites the row source + the retained `LIFTED_ROW_MIGRATIONS` provenance table via `std::fs` — the build/test-time lift step the locked design (the TS7 oracle contract §Q4) mandates. Repository tooling over in-repo source + test fixtures, not a NativeFs/VFS disk-boundary bypass — never workspace/semantic state.",
        ),
        (
            "crates/verter_session/src/typeinfo/oracle_core/gen.rs",
            "TS7 oracle harness snapshot GENERATOR (`#[cfg(feature = \"oracle-gen\")]`, excluded from the default gate). Seeds a hermetic temp tsgo sandbox + WRITES the checked-in snapshot TEST FIXTURES + enumerates/copies the vendored env corpus via `std::fs` — the build/test-time generation step the locked design (the TS7 oracle contract §2, §4) mandates. External-tool scaffolding (tsgo cannot read Verter's in-memory VFS), not a NativeFs/VFS disk-boundary bypass — never workspace/semantic state.",
        ),
        (
            "crates/verter_session/src/typeinfo/oracle_core/relation_driver.rs",
            "TS7 oracle harness v4 `relation_verdict` consumption driver (`#[cfg(test)]` in oracle_core) — loads the checked-in relation snapshot TEST FIXTURES via runtime `std::fs::read`, the same design-mandated mechanism (the TS7 oracle contract §Q1) the v3 consumption driver above is listed for. Not a NativeFs/VFS disk-boundary bypass — in-repo test fixtures, never workspace/semantic state.",
        ),
        (
            "crates/verter_session/src/typeinfo/typeinfo_tests/relation_verdict_oracle.rs",
            "TS7 oracle harness v4 `relation_verdict` rows (`#[cfg(test)]` typeinfo tests) — load checked-in relation snapshot TEST FIXTURES via runtime `std::fs::read` for the capture-backed assertions (one true/false wire shape, ordered bindings), the same runtime-fs mechanism the design mandates. Not a NativeFs/VFS disk-boundary bypass — in-repo test fixtures, never workspace/semantic state.",
        ),
        (
            "crates/verter_lsp/src/audit_harness.rs",
            "LSP audit telemetry — `VERTER_LSP_AUDIT_TRACE_OUT` JSON-lines drainer. Off by default and gated behind the env var at the call site; mirrors the existing `VERTER_COMPONENT_META_AUDIT_JSON_OUT` drainer in `verter_session::component_meta_audit`.",
        ),
        (
            "crates/verter_lsp/src/interaction_trace.rs",
            "LSP interaction-trace telemetry — `VERTER_LSP_INTERACTION_TRACE_DUMP` JSON-lines drainer (writes the session Unix-ms correlation anchor, then one line per terminal trace under the traces lock). Off by default and gated behind the env var at construction; the same env-gated drainer category as `VERTER_LSP_AUDIT_TRACE_OUT` in `verter_lsp::audit_harness`. Harness tool output consumed by the WSP1L capture producer, never workspace/semantic state.",
        ),
        (
            "crates/verter_lsp/src/background_init.rs",
            "background-initialization unit tests stage tsconfig/source files in temp directories to verify snapshot publication; test fixtures only.",
        ),
        (
            "crates/verter_lsp/src/svelte_assets.rs",
            "materializes the Verter-owned `@verter/svelte-jsx` shim into the host data directory for TSGO/inferred-project resolution (tool setup, not semantic input); reads it back via byte-compare; never the user workspace. Test fixtures use temp-dir scratch space.",
        ),
        (
            "crates/verter_lsp/src/external_ts/carrier_publish_store.rs",
            "materializes the Verter-owned carrier-snapshot store (content-addressed blobs + atomic manifest) into a per-host temp directory for the tsserver plugin to read synchronously (tool output mirroring the in-memory ProviderSurfaceStore, not semantic input); never the user workspace. Test fixtures use temp-dir scratch space.",
        ),
        (
            "crates/verter_lsp/src/external_ts/carrier_publish_journal.rs",
            "reads the Verter-owned carrier-snapshot store's head, base snapshot and append-only journal from the per-host temp directory for the tsserver plugin (tool output mirroring the in-memory ProviderSurfaceStore, not semantic input); the read half of `carrier_publish_store.rs`; never the user workspace. Test fixtures use temp-dir scratch space.",
        ),
        (
            "crates/verter_lsp/src/config.rs",
            "test fixtures only (`#[cfg(test)] mod tests` blocks set up tmp directories for `discover_lint_config` tests). No production-path call.",
        ),
        (
            "crates/verter_lsp/src/test_harness.rs",
            "LSP integration test harness — sets up scratch worktrees and reads fixture files for end-to-end tests.",
        ),
        (
            "crates/verter_lsp/src/test_harness_fixture_dependencies.rs",
            "real-provider test harness materializes declaration-only framework dependencies under gitignored fixture node_modules; test setup only.",
        ),
        (
            "crates/verter_lsp/src/test_utils.rs",
            "LSP unit-test utilities — temp workspace creation and `canonicalize` for fixture path resolution.",
        ),
        (
            "crates/verter_lsp/src/type_provider/project_sync.rs",
            "managed-tsgo project-sync tests (`#[cfg(test)] mod tests`) stage temporary dependency packages and carrier paths to assert virtual-fallback and exact-delivery behavior; test fixtures only.",
        ),
        (
            "crates/verter_lsp/src/vue_assets.rs",
            "managed-tsgo Vue JSX authority — reads the installed `node_modules/vue` manifest + `jsx-runtime` declarations and materializes an owner-bound classic-JSX adapter file on the REAL OS filesystem for the EXTERNAL tsgo subprocess to resolve (a subprocess reads real files, not the in-memory VFS). Real-FS tool-cache by nature; same category as the `verter_tsgo_api/toolchain` + `verter_type_runtime/tsgo/ipc.rs` entries. Test fixtures use temp-dir scratch space. Not a NativeFs/VFS disk-boundary bypass, never the user workspace.",
        ),
        (
            "crates/verter_mcp/src/baseline.rs",
            "MCP baseline output — reads/writes JSON snapshots for regression diffing of MCP tool responses; not semantic state.",
        ),
        (
            "crates/verter_scheduler/src/source_loader.rs",
            "scheduler source-loader fallback — reads disk only when the workspace overlay/snapshot is absent for a host-loaded path; transitional pending the full WorkspaceAccess integration.",
        ),
        (
            "crates/verter_audit/src/memory.rs",
            "audit telemetry — `/proc/self/statm` resource sample for memory-delta accounting (Linux RSS branch). Off by default and gated behind audit_enabled at the call site.",
        ),
        (
            "crates/verter_session/src/component_meta_audit/mod.rs",
            "audit telemetry — JSON dump file output for footprint capture (`emit_audit_trace`); off by default and gated behind `VERTER_COMPONENT_META_AUDIT_JSON_OUT`.",
        ),
        (
            "crates/verter_tsc/src/checker.rs",
            "verter-tsc binary CLI — writes the consolidated diagnostics report file at the end of a checker run.",
        ),

        (
            "crates/verter_tsc/src/tsconfig.rs",
            "verter-tsc binary CLI — reads tsconfig files outside the host's WorkspaceAccess (separate from the LSP/session tsconfig path). Doc comment also references `std::fs::canonicalize` behaviour for documentation.",
        ),
        (
            "crates/verter_tsgo_api/src/toolchain/discovery.rs",
            "tsgo toolchain provisioning (the 4-tier resolver) — walks PATH directories, the bound project's ancestor `node_modules` (flat + pnpm store layouts), the temp update cache, and the bundled sidecar directory on the REAL OS filesystem to locate a supported tsgo engine binary, with symlink/reparse-point and cache-root trust checks (`symlink_metadata`/`metadata`). Real-FS by nature (a subprocess engine binary cannot be enumerated through the in-memory VFS); same category as `verter_type_runtime/src/tsgo/ipc.rs`. Not a NativeFs/VFS disk-boundary bypass — never reads workspace/semantic state.",
        ),
        (
            "crates/verter_tsgo_api/src/toolchain/validation.rs",
            "tsgo candidate capability validation — stages a minimal configured project (a temp `tsconfig.json` + `index.ts`) on the REAL OS filesystem for the throwaway `--api` smoke engine to open. A subprocess engine reads real files, not the VFS; same category as `discovery.rs`. Not a NativeFs/VFS disk-boundary bypass — never reads workspace/semantic state.",
        ),
        (
            "crates/verter_tsgo_api/src/control/advertisement.rs",
            "relay-shim rendezvous advertisement — the shim (a standalone editor-spawned process, NOT a session/VFS host) writes an advertisement JSON into `--control-dir` (`create_dir_all` + `write`) so a SEPARATE `verter_lsp` control client can discover + verify it (`read` / `read_dir`) and remove it on teardown (`remove_advertisement`). An IPC rendezvous file is real-FS by nature — a different process reads it off disk — and cannot be routed through the in-memory VFS/`WorkspaceAccess`. Same category as `spawn.rs` + `verter_type_runtime/src/tsgo/ipc.rs`; not a NativeFs/VFS disk-boundary bypass, never workspace/semantic state.",
        ),
        (
            "crates/verter_tsgo_api/src/control/transport.rs",
            "relay-shim control endpoint transport — the control protocol rides same-user local IPC (a Windows named pipe / a Unix-domain socket). On Unix a UDS is a real filesystem artifact the OS binds, so the listener owns its full socket-file lifecycle (`#[cfg(unix)]`): it creates + validates a PRIVATE per-session `0o700` parent subdir (`std::fs::DirBuilder` mode-`0o700` create + `std::fs::symlink_metadata` — a real dir, euid-owned, exactly owner-rwx), holds the grandparent `control_dir` to a secure-permissions ceiling (`std::fs::create_dir_all` + `std::fs::metadata` — owned by us with no group/other write, or sticky and owned by us or root), removes a stale socket before `bind` and its own socket on `Drop` (`std::fs::remove_file`), chmods the bound socket to owner-only `0o600` (`std::fs::set_permissions` / `Permissions::from_mode`), and removes the now-empty private subdir on `Drop` (`std::fs::remove_dir`). This whole socket-file lifecycle is real-FS by nature (the pipe/UDS namespace is the OS, not the VFS); same category as `advertisement.rs` + `spawn.rs`, not a NativeFs/VFS disk-boundary bypass, never workspace/semantic state.",
        ),
        (
            "crates/verter_tsgo_api/src/fake_engine.rs",
            "fake `--api` engine test double — writes a process-id rendezvous file and manages the local IPC pipe lifecycle (`std::fs::File::from_raw_handle` over a Windows named pipe, `std::fs::remove_file` on the pipe path) so the test harness can drive the API protocol off real IPC. Real-FS IPC by nature; same category as `control/advertisement.rs` + `control/transport.rs`. Not a NativeFs/VFS disk-boundary bypass, never workspace/semantic state.",
        ),
        (
            "crates/verter_tsgo_api/src/process.rs",
            "Linux process-liveness probe — reads the kernel-synthesised `/proc/<pid>/stat` state so an exited zombie is not mistaken for a live engine/client merely because `kill(pid, 0)` still finds its unreaped pid. Process lifecycle metadata, not workspace, semantic, overlay, or VFS state; routing it through NativeFs would incorrectly cache a per-process state transition.",
        ),
        (
            "crates/verter_type_runtime/src/discovery.rs",
            "TypeScript SDK install discovery for the type-runtime tool layer (tsserver/tsgo binary lookup, package.json reads inside the SDK directory).",
        ),
        (
            "crates/verter_type_runtime/src/provider_adapter.rs",
            "type-runtime tool-cache and shim file management — separate from semantic state; reads/writes the per-runtime scratch dir used by tsserver/tsgo.",
        ),
        (
            "crates/verter_type_runtime/src/trace.rs",
            "trace artifact writer for tsserver/tsgo IPC debugging; gated behind a debug flag and writes only to a process-local trace file.",
        ),
        (
            "crates/verter_type_runtime/src/tsgo/ipc.rs",
            "tsgo subprocess IPC — pnpm virtual-store walk, scratch-dir setup, and direct disk reads of files the tsgo subprocess will consume next; orchestrates the external runtime.",
        ),
        (
            "crates/verter_type_runtime/src/tsserver/ipc.rs",
            "tsserver subprocess IPC — pnpm virtual-store walk, scratch-dir setup, and direct disk reads of files the tsserver subprocess will consume next; orchestrates the external runtime.",
        ),
        (
            "crates/verter_workspace/src/intrinsic_library.rs",
            "ambient TypeScript SDK reader (`lib*.d.ts`) — companion to NativeFs for SDK declaration files. The verter_session intrinsic_registry consumes this single reader.",
        ),
        (
            "crates/verter_compiler/src/svelte_oracle.rs",
            "Svelte conformance-oracle comparison engine, gated behind the `svelte-oracle` feature (excluded from the default gate). `load_golden` / `load_all_goldens` read the committed golden JSON TEST FIXTURES off disk for the conformance consumers to diff a normalized candidate against — in-repo test corpus, never workspace/semantic state, with no `VerterHost` / `WorkspaceAccess` context. Not a NativeFs/VFS disk-boundary bypass.",
        ),
        (
            "crates/verter_svelte_conformance/src/generate.rs",
            "dev/CI-only, non-published (`publish = false`) Svelte CSS-conformance corpus generator. `write_corpus` / `check_corpus` materialize and reconcile ONLY the crate-owned committed corpus under `env!(\"CARGO_MANIFEST_DIR\")/corpus` for the CLI (`cargo run -p verter_svelte_conformance -- write`) and the crate's own tests — never user workspace, semantic, overlay, or VFS state. `WorkspaceAccess` governs user workspace source/config; tool/output I/O stays on `std::fs` (the same tooling precedent as the `oracle-gen` snapshot generator and `verter_tsc`). Not a NativeFs/VFS disk-boundary bypass.",
        ),
        (
            "crates/verter_vue_conformance/src/lib.rs",
            "dev/CI-only, non-published Vue conformance corpus READER (`read_text_normalized` + case-dir enumeration) — reads ONLY the crate-owned vendored/committed corpus (`env!(\"CARGO_MANIFEST_DIR\")/corpus`: cases, official goldens, known-divergences), never workspace/semantic/VFS state. Test-fixture I/O, not a NativeFs/VFS disk-boundary bypass — sibling of the `verter_svelte_conformance/src/generate.rs` exemption.",
        ),
        (
            "crates/verter_session/src/compile/map_equality_tests/bf2_seed_matrix.rs",
            "BF2 seed-matrix conformance harness (`#[cfg(test, feature = \"bf2-authoritative\")]`) — reads the committed golden manifest/records and Vue fixture TEST FIXTURES off disk, and reads/writes the committed per-cell code-digest baseline JSON (a TEST FIXTURE, not semantic/VFS state) for the assembled-map composition regression pin. Not a NativeFs/VFS disk-boundary bypass — never workspace/semantic state.",
        ),
        (
            "crates/verter_session/src/compile/map_equality_tests/svelte_official_conformance_matrix.rs",
            "Svelte official-conformance golden inventory (`#[cfg(test, feature = \"bf2-authoritative\")]`) — reads the committed golden manifest, each DIGEST-ADDRESSED record under `goldens/records/`, and the Svelte fixture source each record names, all TEST FIXTURES off disk. The record set is data-driven from the manifest at runtime, so the reads cannot be `include_str!` (a digest lookup is not a compile-time path); each read is followed by a SHA-256 verification, which is the point of reading the real bytes. Sibling of the `bf2_seed_matrix.rs` exemption above, whose Vue half does the same for the same reason. Not a NativeFs/VFS disk-boundary bypass — never workspace/semantic state.",
        ),
        (
            "crates/verter_session/src/compile/map_equality_tests/public_api_typescript_observation.rs",
            "PublicApi/TSC/declaration observation under the real TypeScript compiler (`#[cfg(test, feature = \"bf2-authoritative\")]`) — reads the committed golden manifest and the digest-addressed Vue records it names, TEST FIXTURES off disk, to assert every record pins the same official framework version the observation runs against. Data-driven from the manifest at runtime, so not expressible as `include_str!`. Not a NativeFs/VFS disk-boundary bypass — never workspace/semantic state.",
        ),
        (
            "crates/verter_session/src/compile/map_equality_tests/vector_inventory.rs",
            "layer-2 vector-inventory reproduction (`#[cfg(test)]`) — reads the committed `assembled-map-composition.vectors.json` TEST FIXTURE off disk to drive every frozen vector through the production assembler. Not a NativeFs/VFS disk-boundary bypass — never workspace/semantic state.",
        ),
        (
            "crates/verter_session/src/compile/map_equality_tests/nested_v_for_runtime_proof.rs",
            "nested v-for/v-if runtime-execution proof (`#[cfg(test, feature = \"bf2-authoritative\")]`) — writes the compiled module's own generated code to a scratch temp file so an EXTERNAL Node/jsdom subprocess can read it off real disk (a subprocess cannot read Verter's in-memory VFS), then removes it. External-tool scaffolding, sibling of the `oracle_gen_spike.rs`/`vue_assets.rs` entries above — not a NativeFs/VFS disk-boundary bypass, never workspace/semantic state.",
        ),
        (
            "crates/verter_session/src/compile/map_equality_tests/vue_css_vars_client_mount.rs",
            "CSS v-bind client-mount runtime proof (`#[cfg(test, feature = \"bf2-authoritative\")]`) — writes the fixture source, the assembled module's own generated code, and the prop-state JSON to a scratch temp dir so an EXTERNAL Node/jsdom subprocess can read them off real disk (a subprocess cannot read Verter's in-memory VFS), then removes the dir. External-tool scaffolding, sibling of the `nested_v_for_runtime_proof.rs` entry above — not a NativeFs/VFS disk-boundary bypass, never workspace/semantic state.",
        ),
        (
            "crates/verter_test_support/src/lib.rs",
            "dev-dependency-only shared test-harness crate (`unique_temp_dir` etc.), never depended on by production code. The only `std::fs::` calls are inside `#[cfg(test)] mod tests` — a self-test that the minted path is actually a writable scratch dir. No production-path call, and the crate has no `VerterHost`/`WorkspaceAccess` context to route through — sibling of the `verter_lsp/src/config.rs` and `verter_lsp/src/test_utils.rs` test-fixture entries above.",
        ),
        (
            "crates/verter_validation_probe/src/disk.rs",
            "test/CI-only validation-probe lane, and the crate's SOLE disk boundary — the corpus adapter, the lane and the summary binary all route through this one module, so the whole crate is this single entry rather than one per reader. It reads a pinned third-party corpus checkout under `.integration-tests/repos/` (gated behind `feature = \"external-corpus\"`), its own committed `manifest/*.toml`, and its own published `target/validation-probe/summary.json`. None of that is workspace, semantic, overlay or VFS state, and no production crate may depend on this crate (the dependency-layer guard lists it among the harnesses) — sibling of the `verter_vue_conformance/src/lib.rs` and `verter_svelte_conformance/src/generate.rs` corpus-tooling entries.",
        ),
        (
            "crates/verter_supervise/src/disk.rs",
            "test/CI-only process supervisor (`verter-supervise`) that runs compiler probes and benchmark arms under process-tree memory containment, and the crate's SOLE disk boundary — its Windows/Linux/macOS backends, its result writer and its `verter-supervise-fixture` test binary all route through this one module, so the whole crate is this single entry. It touches only its own `result.json` and the child's stdout/stderr log files, the contained workload's cgroup v2 control files and `/proc` pseudo-files, and temp paths its tests and fixture use. None of that is workspace, semantic, overlay or VFS state, and no production crate may depend on this crate (the dependency-layer guard lists it among the harnesses) — sibling of the `verter_validation_probe/src/disk.rs` tooling entry.",
        ),
];

/// Predicate the test reuses: does this file's source contain
/// any `std::fs::` reference? Identical to guard 1's predicate
/// for the `std::fs::` half — extracted here so the deliberate
/// violation tests can characterize this guard independently.
pub fn d14_file_uses_std_fs(src: &str) -> bool {
    src.contains("std::fs::")
}

/// Materialize the allow list into a set of allowed paths +
/// the implicit native_fs path. Returns the canonical set of
/// "files where `std::fs::` is permitted by D14".
pub fn d14_permitted_paths() -> BTreeSet<String> {
    let mut set: BTreeSet<String> = BTreeSet::new();
    set.insert(D14_NATIVE_FS_PATH.to_string());
    for (path, _justification) in D14_ALLOW_LIST {
        set.insert((*path).to_string());
    }
    set
}

/// Walk the workspace's production `.rs` tree under `crates/*/src/`
/// and return paths of files that contain `std::fs::` and are not
/// in the permitted set (native_fs.rs ∪ ALLOW_LIST).
pub fn d14_violations(permitted: &BTreeSet<String>) -> Vec<String> {
    let crates_root = workspace_root().join("crates");
    let mut violations = Vec::new();
    let entries = match fs::read_dir(&crates_root) {
        Ok(it) => it,
        Err(_) => return violations,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let src_dir = path.join("src");
        if !src_dir.exists() {
            continue;
        }
        for file in walk_production_rs(&src_dir) {
            let src = match fs::read_to_string(&file) {
                Ok(s) => s,
                Err(_) => continue,
            };
            if !d14_file_uses_std_fs(&src) {
                continue;
            }
            let rel = relative_to_root(&file);
            if permitted.contains(&rel) {
                continue;
            }
            violations.push(rel);
        }
    }
    violations.sort();
    violations
}

// ── Guard — origin-edge dep signatures are not an invalidation source ──
//
// Invariant:
//
//   Origin-edge dep signatures are not an invalidation source.
//   No production code may reconstruct a CompletionFence from
//   DerivationStore origin edges.
//
// The `DerivationStore` origin layer keeps an `edge_dep_signature`
// snapshot of the publishing builder's fence purely for the audit
// origin-graph trace. It is bounded best-effort provenance — the
// FIFO `edge_budget` evicts the oldest buckets, so the snapshots are
// NOT load-bearing for invalidation. The load-bearing invalidation
// record is the memo entry's own `ReadSetSignature` carrier — the
// path-precise fact rail validated strictly on every warm read via
// `ReadSetSignature::validate_with_self_roots`.
//
// Reconstructing a `CompletionFence` from origin-edge dep signatures
// would couple correctness to a best-effort, FIFO-evicted structure
// — a latent footgun. The retired `SemanticGraphStore::origins_with_fence`
// did exactly that (`fence.merge_signature(&edge.edge_dep_signature)`)
// and had zero production callers; this guard fails if it — or any
// equivalent origin-edge-into-fence merge — is reintroduced into
// non-test production source.

/// Predicate: does this single production source line reintroduce an
/// origin-edge-into-`CompletionFence` merge? Returns `true` for a
/// match. Two shapes are forbidden — (a) any mention of the retired
/// `origins_with_fence` API, and (b) a `merge_signature(...)` call
/// whose argument is an `edge_dep_signature` (folding a
/// `DerivationStore` origin edge's dep-signature snapshot into a
/// fence). Comment lines are NOT exempt: a doc comment that
/// re-documents an `origins_with_fence`-style API is itself the
/// reintroduction this guard forbids.
pub fn line_reconstructs_fence_from_origin_edge(line: &str) -> bool {
    if line.contains("origins_with_fence") {
        return true;
    }
    // The forbidden merge shape: a `merge_signature` call on the
    // same line as an `edge_dep_signature` operand — the exact
    // `fence.merge_signature(&edge.edge_dep_signature)` pattern the
    // retired API used. Requiring both needles on one line keeps the
    // guard focused: a legitimate `merge_signature` of a memo
    // entry's own carrier never names `edge_dep_signature`.
    line.contains("merge_signature") && line.contains("edge_dep_signature")
}

/// Walk the production tree and return `(rel_path, line_no, line)`
/// triples for every origin-edge-into-fence reconstruction.
pub fn origin_fence_reconstruction_violations() -> Vec<(String, usize, String)> {
    let crates_root = workspace_root().join("crates");
    let mut violations = Vec::new();
    let entries = match fs::read_dir(&crates_root) {
        Ok(it) => it,
        Err(_) => return violations,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let src_dir = path.join("src");
        if !src_dir.exists() {
            continue;
        }
        for file in walk_production_rs(&src_dir) {
            let src = match fs::read_to_string(&file) {
                Ok(s) => s,
                Err(_) => continue,
            };
            let rel = relative_to_root(&file);
            // This guard test file is exempt — it names the
            // forbidden patterns in its own predicate + self-test.
            if rel == "crates/verter_session/tests/cases/architecture/foundations/mod.rs" {
                continue;
            }
            for (idx, line) in src.lines().enumerate() {
                if line_reconstructs_fence_from_origin_edge(line) {
                    violations.push((rel.clone(), idx + 1, line.to_string()));
                }
            }
        }
    }
    violations.sort();
    violations
}
mod cache;
mod capabilities;
#[path = "../hermeticity.rs"]
pub(crate) mod hermeticity;
mod mappings;

fn external_corpus_guard_self_exempts(path: &std::path::Path) -> bool {
    ["foundations/mod.rs", "hermeticity.rs"].iter().any(|file| {
        path == workspace_root()
            .join("crates/verter_session/tests/cases/architecture")
            .join(file)
    })
}
