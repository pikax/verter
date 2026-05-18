//! Shared harness for the Block 2 canary suite.
//!
//! The canary suite proves that the lazy fact-validation substrate
//! backs every cross-file invalidation scenario. The owner-upsert path
//! has no eager reverse-dependent invalidation cascade — a warm
//! consumer result is invalidated only by fact-validation on read.
//!
//! Every canary mutation routes through the skip-own-drain upsert
//! hook ([`VerterHost::upsert_skipping_own_canonical_drain_for_tests`]).
//! That hook runs the full production pipeline — parse, change
//! detection, per-domain invalidation, parse-domain fact re-emission —
//! but suppresses the post-commit own-canonical query-identity cache
//! drain (`resolver.runtime.evict_canonical`,
//! `project_type_store.evict_canonical`, `resolved_type_cache().clear()`
//! for the upserted canonical). With that drain suppressed, the ONLY
//! mechanism that can reject a stale warm entry for the edited
//! canonical — owner-self OR dependency — is lazy self-version-root
//! fact-validation on the read path. That is exactly what the canary
//! suite must prove: routing through the plain production `upsert`
//! would let the eager own-canonical drain mask a missing self-version
//! root and the canaries would pass without exercising the lazy path.
//!
//! Wiring note: the suite reaches the lazy path through ONE chokepoint
//! — the [`upsert`] helper below. When the own-canonical drain itself
//! is deleted (and the `upsert_skipping_own_canonical_drain_for_tests`
//! hook removed with it), this helper flips back to the plain
//! `VerterHost::upsert`, which by then has no drain to suppress. That
//! flip is a one-line edit in this one file — the suite's coverage is
//! unchanged because a drain-free `upsert` exercises the same lazy
//! path the hook does today.
//!
//! Helpers:
//! - [`standalone_host`] — a hermetic `new_standalone` host (relative
//!   `./dep` specifiers resolve against the owner's directory).
//! - [`workspace_host`] — a workspace-backed host rooted at
//!   `/workspace`, for scenarios that need barrel / absolute-specifier
//!   resolution.
//! - [`upsert`] — owner / dependency upsert through the skip-own-drain
//!   hook, so a same-canonical edit is rejected only by lazy
//!   fact-validation, not by the eager own-canonical drain.
//! - [`prime_compile`] — primes a warm compile slot via
//!   `get_virtual_file`.
//! - [`compile_main`] — reads the assembled `Main` virtual node, the
//!   user-visible compiled output.
//! - [`meta_misses`] / [`meta_hits`] — `ComponentMetaResultDb`
//!   provenance counters.

#![allow(dead_code)]

use std::sync::atomic::Ordering::Relaxed;
use std::sync::Arc;

use verter_session::{
    CompileProfile, FileKind, HostConfig, HostError, UpsertRequest, VerterHost,
    VirtualFileResponse, VirtualNodeKind, VirtualQuery,
};
use verter_workspace::{MemoryOptions, MemoryWorkspace, WorkspaceAccess};

/// Build a hermetic standalone host. Relative `./x` specifiers
/// resolve against the importing file's directory.
pub fn standalone_host() -> Arc<VerterHost> {
    Arc::new(VerterHost::new_standalone(HostConfig::default()))
}

/// Build a workspace-backed host rooted at `/workspace`. `files` are
/// injected into the in-memory workspace overlay before any query so
/// absolute specifiers (`/workspace/src/...`) and barrel re-exports
/// resolve.
pub fn workspace_host(files: &[(&str, &str)]) -> (Arc<MemoryWorkspace>, Arc<VerterHost>) {
    let workspace = Arc::new(MemoryWorkspace::new(MemoryOptions::default()));
    for (canonical, content) in files {
        workspace.inject_file((*canonical).into(), Arc::from(*content));
    }
    let ws_access: Arc<dyn WorkspaceAccess> = workspace.clone();
    let host = Arc::new(VerterHost::new(HostConfig::default(), ws_access));
    (workspace, host)
}

/// Upsert a file through the skip-own-drain hook
/// ([`VerterHost::upsert_skipping_own_canonical_drain_for_tests`]).
///
/// The hook runs the full production upsert pipeline but suppresses the
/// post-commit own-canonical query-identity cache drain. Two
/// invalidation mechanisms therefore stay active and one is removed:
///
/// - Active — the no-eager-cascade contract: an owner edit never
///   eagerly invalidates reverse dependents. A downstream consumer's
///   warm slot / result physically survives an upstream dependency
///   edit and is rejected only by lazy fact-validation on the next
///   read.
/// - Active — lazy self-version-root validation: a warm query-identity
///   entry for the *edited canonical itself* is rejected by its
///   current-content self-root on the cold-recompute read path.
/// - Removed (by the hook) — the eager own-canonical drain. With the
///   plain `upsert` it would evict the edited canonical's own
///   query-identity entries up front and so mask a missing
///   self-version root. The hook suppresses it, so the canaries
///   genuinely exercise the lazy path.
///
/// This is the single chokepoint the whole canary suite mutates
/// through (owner / dependency setup AND the edit under test). When the
/// own-canonical drain is deleted, this body flips back to the plain
/// `VerterHost::upsert` — a drain-free `upsert` exercises the identical
/// lazy path.
pub fn upsert(host: &VerterHost, canonical: &str, source: &str, kind: FileKind) {
    let _ = host
        .upsert_skipping_own_canonical_drain_for_tests(UpsertRequest {
            canonical_id: Some(canonical.to_string()),
            input_id: canonical.to_string(),
            source: source.into(),
            file_kind: kind,
            aliases: Vec::new(),
        })
        .expect("upsert");
}

/// Prime a warm compile slot for `canonical` at the default profile.
pub fn prime_compile(host: &VerterHost, canonical: &str) {
    let _ = host.get_virtual_file(VirtualQuery {
        raw_id: None,
        canonical_id: Some(canonical.to_string()),
        node_kind: Some(VirtualNodeKind::Script),
        compile_profile: CompileProfile::default(),
    });
}

/// Read the assembled `Main` virtual node — the user-visible compiled
/// output for the SFC.
pub fn compile_main(host: &VerterHost, canonical: &str) -> Result<VirtualFileResponse, HostError> {
    host.get_virtual_file(VirtualQuery {
        raw_id: None,
        canonical_id: Some(canonical.to_string()),
        node_kind: Some(VirtualNodeKind::Main),
        compile_profile: CompileProfile::default(),
    })
}

/// The `ComponentMetaResultDb` warm-hit miss counter.
pub fn meta_misses(host: &VerterHost) -> u64 {
    host.provenance()
        .component_meta_result_cache_misses
        .load(Relaxed)
}

/// The `ComponentMetaResultDb` warm-hit hit counter.
pub fn meta_hits(host: &VerterHost) -> u64 {
    host.provenance()
        .component_meta_result_cache_hits
        .load(Relaxed)
}
