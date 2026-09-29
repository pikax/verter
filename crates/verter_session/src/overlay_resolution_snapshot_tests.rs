//! Resolution through a session overlay answers for the overlay's own
//! effective view, whatever the workspace resolved before it: an answer, its
//! witness and its validator refer to the same effective resolution
//! snapshot.

use std::sync::Arc;

use rustc_hash::FxHashMap;

use crate::resolver_core::{CanonicalCompletionOverlay, ResolverContext, SessionResolverContext};
use crate::session_view::OverlaidViewRef;
use crate::{HostConfig, UpsertRequest, VerterHost};

fn make_host(files: &[(&str, &str)]) -> (Arc<verter_workspace::MemoryWorkspace>, Arc<VerterHost>) {
    let workspace = Arc::new(verter_workspace::MemoryWorkspace::new(
        verter_workspace::MemoryOptions::default(),
    ));
    for (path, source) in files {
        workspace.inject_file((*path).to_string(), Arc::from(*source));
    }
    let host = Arc::new(VerterHost::new(HostConfig::default(), workspace.clone()));
    (workspace, host)
}

fn upsert(host: &VerterHost, canonical_id: &str, source: &str) {
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: Some(canonical_id.to_string()),
            input_id: canonical_id.to_string(),
            source: Arc::from(source),
            file_language: verter_language::LanguageRegistry::global()
                .classify_static(canonical_id)
                .static_resolution(),
            aliases: Vec::new(),
        })
        .expect("upsert must succeed");
}

/// `name` exported from `barrel`, read through a session context over
/// `overlays`.
fn resolve_through_session(
    host: &Arc<VerterHost>,
    overlays: &[(&str, &str)],
    barrel: &str,
    name: &str,
) -> Option<(String, String)> {
    resolve_through_session_with_tombstones(host, overlays, &[], barrel, name)
}

/// [`resolve_through_session`] with session deletes too.
fn resolve_through_session_with_tombstones(
    host: &Arc<VerterHost>,
    overlays: &[(&str, &str)],
    tombstones: &[&str],
    barrel: &str,
    name: &str,
) -> Option<(String, String)> {
    let mut sources: FxHashMap<String, Arc<str>> = FxHashMap::default();
    let mut hashes = FxHashMap::default();
    for (canonical, source) in overlays {
        sources.insert((*canonical).to_string(), Arc::from(*source));
        hashes.insert(
            (*canonical).to_string(),
            crate::hash::hash_16(source.as_bytes()),
        );
    }
    let deleted: std::collections::HashSet<String> = tombstones
        .iter()
        .map(|canonical| (*canonical).to_string())
        .collect();
    let view = OverlaidViewRef::new(host, &sources, &hashes, &deleted);
    let store_view = host
        .resolver_store_view_read()
        .into_owned_view()
        .with_session_overlay(host, &view);
    let ctx = SessionResolverContext::new(
        host,
        &view,
        &store_view,
        Arc::new(CanonicalCompletionOverlay::new()),
    );
    ctx.resolve_named_type_export_target_shallow(barrel, name)
}

/// `name` exported from `barrel`, read through the host's own context.
fn resolve_through_host(
    host: &Arc<VerterHost>,
    barrel: &str,
    name: &str,
) -> Option<(String, String)> {
    let store_view = host.resolver_store_view_read().into_owned_view();
    let ctx = crate::resolver_core::HostResolverContext::new(
        host,
        &store_view,
        Arc::new(CanonicalCompletionOverlay::new()),
    );
    ctx.resolve_named_type_export_target_shallow(barrel, name)
}

const BARREL: &str = "/workspace/barrel.ts";
const HELPER: &str = "/workspace/helper.ts";
const HELPER_SOURCE: &str = "export interface P { value: string }\n";

/// A dependency only a session overlay creates is found through the
/// session whether or not the workspace has already answered the same
/// route with a miss: the warm workspace miss observed the workspace's
/// resolution facts, which say nothing about the overlay's effective view.
#[test]
fn a_warm_workspace_route_miss_never_conceals_an_overlay_created_dependency() {
    let expected = Some((HELPER.to_string(), "P".to_string()));

    // Cold: the session answers first.
    let (_, cold_host) = make_host(&[]);
    upsert(&cold_host, BARREL, "export * from \"./helper\";\n");
    assert_eq!(
        resolve_through_session(&cold_host, &[(HELPER, HELPER_SOURCE)], BARREL, "P"),
        expected,
        "a cold session read finds the overlay-created helper"
    );

    // Warm: the workspace answers first, caching its miss.
    let (_, warm_host) = make_host(&[]);
    upsert(&warm_host, BARREL, "export * from \"./helper\";\n");
    assert_eq!(
        resolve_through_host(&warm_host, BARREL, "P"),
        None,
        "precondition: the workspace has no helper"
    );
    assert_eq!(
        resolve_through_session(&warm_host, &[(HELPER, HELPER_SOURCE)], BARREL, "P"),
        expected,
        "the session answers the same as a cold read, whatever the workspace cached"
    );
    // And the session's answer never leaks into the workspace's.
    assert_eq!(resolve_through_host(&warm_host, BARREL, "P"), None);
}

/// A route the session answered through its overlay never serves the
/// workspace. The session deletes a dependency the workspace has, so the
/// session's answer differs from the workspace's in resolution facts alone
/// — no overlay parse fact is there to reject it by accident.
#[test]
fn a_session_route_answer_never_serves_the_workspace_view() {
    let (_, host) = make_host(&[]);
    upsert(&host, BARREL, "export * from \"./helper\";\n");
    upsert(&host, HELPER, HELPER_SOURCE);
    assert_eq!(
        resolve_through_session_with_tombstones(&host, &[], &[HELPER], BARREL, "P"),
        None,
        "the session deleted the helper"
    );
    assert_eq!(
        resolve_through_host(&host, BARREL, "P"),
        Some((HELPER.to_string(), "P".to_string())),
        "the workspace still has the helper, whatever the session cached"
    );
}

/// The overlay type-route resolves an ESM fallback to its declaration
/// companion exactly as the workspace type-route does: one policy, whether
/// or not the overlay is empty.
#[test]
fn the_overlay_type_route_normalizes_an_esm_fallback_like_the_workspace_one() {
    let owner = "/workspace/index.ts";
    let (workspace, host) = make_host(&[
        (owner, "export {}\n"),
        ("/workspace/runtime.js", "export const runtime = true\n"),
        ("/workspace/runtime.d.ts", "export type Runtime = boolean\n"),
    ]);
    workspace.set_exact_resolutions(
        owner,
        vec![verter_workspace::ExactResolution {
            specifier: "runtimedep".to_string(),
            phase: verter_semantic::resolver_core::ResolvePhase::CodegenBlocker,
            kind: verter_semantic::resolver_core::ResolveRequestKind::EsmImport,
            resolved_canonical_id: Some("/workspace/runtime.js".to_string()),
            possible_canonical_ids: vec!["/workspace/runtime.js".to_string()],
        }],
    );
    let workspace_route = match host.resolve_type_dependency_canonical(owner, "runtimedep") {
        verter_workspace::ResolutionPublication::Admitted(admitted) => admitted.into_result(),
        verter_workspace::ResolutionPublication::Refused(refusal) => {
            panic!("precondition: the workspace type-route admits: {refusal:?}")
        }
    };
    assert_eq!(
        workspace_route.as_deref(),
        Some("/workspace/runtime.d.ts"),
        "precondition: the workspace type-route normalizes to the companion"
    );
    // Through a session context: once with no overlay at all, once with an
    // overlay that touches nothing the route reads.
    for overlays in [&[][..], &[("/workspace/unrelated.ts", "export {}\n")][..]] {
        assert_eq!(
            type_route_through_session(&host, overlays, owner, "runtimedep"),
            workspace_route,
            "the session type-route is the workspace policy (overlay: {overlays:?})"
        );
    }
}

/// The type-route target of `specifier` from `owner`, read through a
/// session context over `overlays`.
fn type_route_through_session(
    host: &Arc<VerterHost>,
    overlays: &[(&str, &str)],
    owner: &str,
    specifier: &str,
) -> Option<String> {
    let mut sources: FxHashMap<String, Arc<str>> = FxHashMap::default();
    let mut hashes = FxHashMap::default();
    for (canonical, source) in overlays {
        sources.insert((*canonical).to_string(), Arc::from(*source));
        hashes.insert(
            (*canonical).to_string(),
            crate::hash::hash_16(source.as_bytes()),
        );
    }
    let deleted = std::collections::HashSet::new();
    let view = OverlaidViewRef::new(host, &sources, &hashes, &deleted);
    let store_view = host
        .resolver_store_view_read()
        .into_owned_view()
        .with_session_overlay(host, &view);
    let ctx = SessionResolverContext::new(
        host,
        &view,
        &store_view,
        Arc::new(CanonicalCompletionOverlay::new()),
    );
    ctx.resolve_type_dependency_canonical(owner, specifier)
}
