//! Shared helpers for the external-TS ownership guards: build a real
//! `WorkspaceSnapshot` from an in-memory workspace through the production parse +
//! supported-extension-expansion chain, and drive the contract resolver.
//!
//! `build_workspace_snapshot` discovers tsconfigs with a real-filesystem
//! `walkdir`, which an in-memory workspace has no presence on, so these helpers
//! assemble the snapshot via `build_workspace_snapshot_simple` + the real
//! `load_project_membership` / `membership_to_spec` expansion.

#![allow(dead_code)]

use std::sync::Arc;

use rustc_hash::FxHashSet;
use verter_session::external_ts::{
    EnvDims, ExternalTsProjectResolver, ProjectResolution, WorkspaceProjectResolver,
};
use verter_session::file_artifact_store::ProjectIdentity;
use verter_workspace::canonical_path::CanonicalPath;
use verter_workspace::config::{
    load_compiler_options, load_project_membership, load_project_references,
};
use verter_workspace::membership::ConfiguredMembership;
use verter_workspace::memory::{MemoryOptions, MemoryWorkspace};
use verter_workspace::snapshot_builder::{
    build_workspace_snapshot_simple, membership_to_spec, supported_extensions_for,
};
use verter_workspace::workspace_snapshot::{
    OwnershipProject, ProjectId, ProjectPayload, SnapshotGeneration, WorkspaceSnapshot,
};

pub const WORKSPACE_ROOT: &str = "d:/ws";

/// The carrier extensions the live registry registers (`.vue`, `.svelte`, …),
/// WITHOUT a leading dot. Guards are adapter-parameterized over these.
pub fn carrier_exts() -> Vec<String> {
    verter_language::LanguageRegistry::global()
        .carrier_extensions()
        .iter()
        .map(|e| (*e).to_string())
        .collect()
}

/// Build a `MemoryWorkspace` with the given `(path, content)` files.
pub fn workspace_with(files: &[(&str, &str)]) -> MemoryWorkspace {
    let ws = MemoryWorkspace::new(MemoryOptions {
        roots: vec![WORKSPACE_ROOT.to_string()],
        default_resolve_extensions: None,
    });
    for (path, content) in files {
        ws.inject_file((*path).to_string(), Arc::<str>::from(*content));
    }
    ws
}

fn tsconfig_dir(tsconfig: &str) -> String {
    tsconfig
        .rsplit_once('/')
        .map(|(dir, _)| dir.to_string())
        .unwrap_or_else(|| tsconfig.to_string())
}

/// Deterministic non-zero R21 env dims (a stand-in for the host's per-project
/// env-hash reader). Non-zero so guards never depend on a default/zero env
/// identity; distinct per axis.
fn test_env_dims(_tsconfig_uri: &str) -> EnvDims {
    EnvDims {
        parse_env_hash: [1u8; 16],
        resolve_env_hash: [2u8; 16],
        lib_env_hash: [3u8; 16],
        project_identity: ProjectIdentity([4u8; 16]),
    }
}

/// Build a real `WorkspaceSnapshot` from the named tsconfigs (parsed from the
/// in-memory workspace through the production membership/expansion chain).
pub fn snapshot_from_tsconfigs(ws: &MemoryWorkspace, tsconfigs: &[&str]) -> WorkspaceSnapshot {
    let mut projects = Vec::new();
    for (i, tsconfig) in tsconfigs.iter().enumerate() {
        let root = CanonicalPath::new(&tsconfig_dir(tsconfig));
        let raw_membership = load_project_membership(ws, tsconfig);
        let compiler_options = load_compiler_options(ws, tsconfig);
        let supported = supported_extensions_for(&compiler_options);
        let spec = membership_to_spec(&root, &raw_membership, &supported);
        let references = load_project_references(ws, tsconfig)
            .into_iter()
            .map(|r| CanonicalPath::new(&r))
            .collect();
        projects.push(OwnershipProject {
            id: ProjectId(i as u32),
            root: root.clone(),
            workspace_root: CanonicalPath::new(WORKSPACE_ROOT),
            payload: ProjectPayload::Configured {
                tsconfig_path: CanonicalPath::new(tsconfig),
                membership: ConfiguredMembership {
                    spec,
                    // Empty materialized set ⇒ bridge mode ⇒ the
                    // supported-extension-expanded include globs are the
                    // membership decision under test.
                    materialized_files: FxHashSet::default(),
                },
                compiler_options,
                references,
                workspace_aliases: Vec::new(),
            },
        });
    }
    build_workspace_snapshot_simple(projects, SnapshotGeneration(1))
}

/// Resolve `source_uri` against a fresh snapshot built from the given files and
/// the single workspace-root tsconfig.
pub fn resolve_with(
    files: &[(&str, &str)],
    tsconfigs: &[&str],
    source_uri: &str,
) -> ProjectResolution {
    let ws = workspace_with(files);
    let snap = snapshot_from_tsconfigs(&ws, tsconfigs);
    let resolver =
        WorkspaceProjectResolver::new(&snap, &ws, "7.0.1", &(test_env_dims as fn(&str) -> EnvDims));
    resolver.resolve(source_uri)
}
