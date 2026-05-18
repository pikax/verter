//! Shared harness for the Block 2 canary suite.
//!
//! The canary suite proves that the lazy fact-validation substrate
//! backs every cross-file invalidation scenario. The owner-upsert path
//! has no eager reverse-dependent invalidation cascade and no eager
//! own-canonical cache drain — a warm consumer result, and a warm
//! query-identity entry for the edited canonical itself, are both
//! invalidated only by fact-validation on read.
//!
//! Every canary mutation routes through the plain production
//! [`VerterHost::upsert`]. That path runs parse, change detection,
//! per-domain invalidation, and parse-domain fact re-emission, and
//! performs no own-canonical query-identity cache drain. So the ONLY
//! mechanism that can reject a stale warm entry for the edited
//! canonical — owner-self OR dependency — is lazy self-version-root
//! fact-validation on the read path. That is exactly what the canary
//! suite proves.
//!
//! Helpers:
//! - [`standalone_host`] — a hermetic `new_standalone` host (relative
//!   `./dep` specifiers resolve against the owner's directory).
//! - [`workspace_host`] — a workspace-backed host rooted at
//!   `/workspace`, for scenarios that need barrel / absolute-specifier
//!   resolution.
//! - [`upsert`] — owner / dependency upsert through the production
//!   `upsert` path, so a same-canonical edit is rejected only by lazy
//!   fact-validation.
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

/// Upsert a file through the production [`VerterHost::upsert`] path.
///
/// The production upsert runs parse, change detection, per-domain
/// invalidation, and parse-domain fact re-emission, and performs no
/// own-canonical query-identity cache drain. Two invalidation
/// mechanisms therefore drive every canary scenario:
///
/// - The no-eager-cascade contract: an owner edit never eagerly
///   invalidates reverse dependents. A downstream consumer's warm slot
///   / result physically survives an upstream dependency edit and is
///   rejected only by lazy fact-validation on the next read.
/// - Lazy self-version-root validation: a warm query-identity entry
///   for the *edited canonical itself* is rejected by its
///   current-content self-root on the cold-recompute read path.
///
/// This is the single chokepoint the whole canary suite mutates
/// through (owner / dependency setup AND the edit under test).
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
