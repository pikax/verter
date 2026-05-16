//! Shared harness for the Block 2 canary suite.
//!
//! The canary suite proves that the lazy fact-validation substrate
//! backs every cross-file invalidation scenario WITHOUT the eager
//! reverse-dependent invalidation cascade. Every canary test mutates
//! the dependency through [`VerterHost::upsert_without_dependent_eviction`]
//! so the eager cascade does NOT run — only fact-validation can
//! invalidate a warm consumer result.
//!
//! Helpers:
//! - [`standalone_host`] — a hermetic `new_standalone` host (relative
//!   `./dep` specifiers resolve against the owner's directory).
//! - [`workspace_host`] — a workspace-backed host rooted at
//!   `/workspace`, for scenarios that need barrel / absolute-specifier
//!   resolution.
//! - [`upsert`] / [`upsert_no_evict`] — owner / dependency upserts;
//!   `upsert_no_evict` is the cascade-suppressing dependency edit.
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

/// Upsert a file through the normal path (the eager cascade runs).
/// Used for the initial owner + dependency setup before priming.
pub fn upsert(host: &VerterHost, canonical: &str, source: &str, kind: FileKind) {
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: Some(canonical.to_string()),
            input_id: canonical.to_string(),
            source: source.into(),
            file_kind: kind,
            aliases: Vec::new(),
        })
        .expect("upsert");
}

/// Upsert a dependency WITHOUT the eager reverse-dependent cascade.
///
/// This is the canary suite's core mechanism: the dependency's own
/// caches are drained (so the resolver re-emits fresh facts) but the
/// reverse-dep cascade — the path that would physically evict the
/// consumer's warm slot / result — is skipped. With the cascade
/// suppressed, the ONLY mechanism that can invalidate the consumer is
/// fact-validation against the freshly emitted dependency facts.
pub fn upsert_no_evict(host: &VerterHost, canonical: &str, source: &str, kind: FileKind) {
    let _ = host
        .upsert_without_dependent_eviction(UpsertRequest {
            canonical_id: Some(canonical.to_string()),
            input_id: canonical.to_string(),
            source: source.into(),
            file_kind: kind,
            aliases: Vec::new(),
        })
        .expect("upsert_without_dependent_eviction");
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
