//! Resolution through a session overlay answers for the overlay's own
//! effective view, whatever the workspace resolved before it: an answer, its
//! witness and its validator refer to the same effective resolution
//! snapshot.

use std::sync::Arc;

use rustc_hash::FxHashMap;

use crate::resolver_core::{CanonicalCompletionOverlay, ResolverContext, SessionResolverContext};
use crate::resolver_store::HostStoreView;
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

/// One session request over `overlays` and `tombstones`: its store view and
/// its context, built the way production builds them.
fn with_session<T>(
    host: &Arc<VerterHost>,
    overlays: &[(&str, &str)],
    tombstones: &[&str],
    request: impl FnOnce(&SessionResolverContext<'_>, &HostStoreView) -> T,
) -> T {
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
    request(&ctx, &store_view)
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
    with_session(host, overlays, tombstones, |ctx, _| {
        ctx.resolve_named_type_export_target_shallow(barrel, name)
    })
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

/// The type-route target of `specifier` from `owner`, read through a
/// session context over `overlays`.
fn type_route_through_session(
    host: &Arc<VerterHost>,
    overlays: &[(&str, &str)],
    owner: &str,
    specifier: &str,
) -> Option<String> {
    with_session(host, overlays, &[], |ctx, _| {
        ctx.resolve_type_dependency_canonical(owner, specifier)
    })
}

/// The type-route target of `specifier` from `owner`, read through the
/// host's own context.
fn type_route_through_host(host: &Arc<VerterHost>, owner: &str, specifier: &str) -> Option<String> {
    let store_view = host.resolver_store_view_read().into_owned_view();
    let ctx = crate::resolver_core::HostResolverContext::new(
        host,
        &store_view,
        Arc::new(CanonicalCompletionOverlay::new()),
    );
    ctx.resolve_type_dependency_canonical(owner, specifier)
}

/// How many times the workspace's resolution producer has run.
fn producer_runs(host: &VerterHost) -> u64 {
    host.ws()
        .vfs_provenance_snapshot()
        .import_resolution_cache_miss_count
}

const BARREL: &str = "/workspace/barrel.ts";
const HELPER: &str = "/workspace/helper.ts";
const HELPER_SOURCE: &str = "export interface P { value: string }\n";
const OWNER: &str = "/workspace/owner.ts";

fn p_from_helper() -> Option<(String, String)> {
    Some((HELPER.to_string(), "P".to_string()))
}

/// A dependency only a session overlay creates is found through the
/// session whether or not the workspace has already answered the same
/// route with a miss: the warm workspace miss observed the workspace's
/// resolution facts, which say nothing about the overlay's effective view.
#[test]
fn a_warm_workspace_route_miss_never_conceals_an_overlay_created_dependency() {
    let expected = p_from_helper();

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
        p_from_helper(),
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

// ---------------------------------------------------------------------------
// Determinism: each effective snapshot answers the same under every cache
// history.
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Reader {
    Workspace,
    Session,
}

/// Workspace-first, session-first and alternating read orders, over an
/// overlay that CREATES the dependency and over one that DELETES it: every
/// read answers what a cold read of its own view answers.
#[test]
fn every_view_answers_the_same_whatever_was_read_first() {
    use Reader::{Session, Workspace};
    let orders: [[Reader; 4]; 4] = [
        [Workspace, Session, Workspace, Session],
        [Session, Workspace, Session, Workspace],
        [Session, Session, Workspace, Workspace],
        [Workspace, Workspace, Session, Session],
    ];
    // (workspace has the helper, overlay upserts, overlay deletes,
    //  workspace answer, session answer)
    type Scenario<'a> = (
        bool,
        &'a [(&'a str, &'a str)],
        &'a [&'a str],
        Option<(String, String)>,
        Option<(String, String)>,
    );
    let scenarios: [Scenario<'_>; 2] = [
        (
            false,
            &[(HELPER, HELPER_SOURCE)],
            &[],
            None,
            p_from_helper(),
        ),
        (true, &[], &[HELPER], p_from_helper(), None),
    ];
    for (has_helper, upserts, deletes, workspace_answer, session_answer) in scenarios {
        for order in orders {
            let (_, host) = make_host(&[]);
            upsert(&host, BARREL, "export * from \"./helper\";\n");
            if has_helper {
                upsert(&host, HELPER, HELPER_SOURCE);
            }
            for reader in order {
                let answer = match reader {
                    Workspace => resolve_through_host(&host, BARREL, "P"),
                    Session => resolve_through_session_with_tombstones(
                        &host, upserts, deletes, BARREL, "P",
                    ),
                };
                let expected = match reader {
                    Workspace => &workspace_answer,
                    Session => &session_answer,
                };
                assert_eq!(
                    &answer, expected,
                    "{reader:?} in order {order:?} (helper in workspace: {has_helper})"
                );
            }
        }
    }
}

/// One warm host walked through create, body edit, export change, revert
/// and reveal answers every step exactly as a cold host does — and a body
/// edit that changes no resolution fact runs no resolver.
#[test]
fn create_edit_revert_and_reveal_answer_as_cold() {
    let steps: [&[(&str, &str)]; 6] = [
        &[],
        &[(HELPER, HELPER_SOURCE)],
        &[(HELPER, "export interface P { value: number }\n")],
        &[(HELPER, "export interface Q { value: string }\n")],
        &[(HELPER, HELPER_SOURCE)],
        &[],
    ];
    let (_, warm) = make_host(&[]);
    upsert(&warm, BARREL, "export * from \"./helper\";\n");
    let mut runs_before_body_edit = None;
    for (index, overlays) in steps.iter().enumerate() {
        let (_, cold) = make_host(&[]);
        upsert(&cold, BARREL, "export * from \"./helper\";\n");
        let expected = resolve_through_session(&cold, overlays, BARREL, "P");
        if index == 2 {
            runs_before_body_edit = Some(producer_runs(&warm));
        }
        assert_eq!(
            resolve_through_session(&warm, overlays, BARREL, "P"),
            expected,
            "step {index}: {overlays:?}"
        );
        if index == 2 {
            assert_eq!(
                Some(producer_runs(&warm)),
                runs_before_body_edit,
                "a body edit of an overlaid file moves no resolution fact"
            );
        }
    }
}

/// A higher-priority candidate that only the overlay has retargets the
/// session and nothing else; the workspace keeps its own target, in any
/// order, however often the two alternate.
#[test]
fn a_higher_priority_overlay_candidate_retargets_only_the_session() {
    let (_, host) = make_host(&[
        (OWNER, "import type { P } from './helper'\n"),
        ("/workspace/helper.d.ts", "export interface P {}\n"),
    ]);
    let shadow: &[(&str, &str)] = &[(HELPER, HELPER_SOURCE)];
    for _ in 0..3 {
        assert_eq!(
            type_route_through_session(&host, shadow, OWNER, "./helper").as_deref(),
            Some(HELPER)
        );
        assert_eq!(
            type_route_through_host(&host, OWNER, "./helper").as_deref(),
            Some("/workspace/helper.d.ts")
        );
    }
}

/// A caller-supplied exact resolution change reaches the session view: the
/// session's answer — overlay lane or reused workspace candidate — observed
/// the exact fact, so the change refuses it.
#[test]
fn an_exact_resolution_change_reaches_the_session() {
    let (workspace, host) = make_host(&[
        (OWNER, "export {}\n"),
        ("/workspace/a.ts", "export const a = 1\n"),
        ("/workspace/b.ts", "export const b = 1\n"),
    ]);
    let exact = |target: &str| verter_workspace::ExactResolution {
        specifier: "dep".to_string(),
        phase: verter_semantic::resolver_core::ResolvePhase::CodegenBlocker,
        kind: verter_semantic::resolver_core::ResolveRequestKind::TypeImport,
        resolved_canonical_id: Some(target.to_string()),
        possible_canonical_ids: vec![target.to_string()],
    };
    let unrelated: &[(&str, &str)] = &[("/elsewhere/scratch.ts", "export {}\n")];
    workspace.set_exact_resolutions(OWNER, vec![exact("/workspace/a.ts")]);
    assert_eq!(
        type_route_through_session(&host, unrelated, OWNER, "dep").as_deref(),
        Some("/workspace/a.ts")
    );
    workspace.set_exact_resolutions(OWNER, vec![exact("/workspace/b.ts")]);
    assert_eq!(
        type_route_through_session(&host, unrelated, OWNER, "dep").as_deref(),
        Some("/workspace/b.ts")
    );
}

/// Sessions with different overlays resolving concurrently each answer for
/// their own view: no request adopts another view's answer.
#[test]
fn concurrent_sessions_never_adopt_each_others_answers() {
    let (_, host) = make_host(&[]);
    upsert(&host, BARREL, "export * from \"./helper\";\n");
    std::thread::scope(|scope| {
        let creating = scope.spawn(|| {
            (0..24)
                .map(|_| resolve_through_session(&host, &[(HELPER, HELPER_SOURCE)], BARREL, "P"))
                .collect::<Vec<_>>()
        });
        let without = scope.spawn(|| {
            (0..24)
                .map(|_| {
                    resolve_through_session(
                        &host,
                        &[("/elsewhere/scratch.ts", "export {}\n")],
                        BARREL,
                        "P",
                    )
                })
                .collect::<Vec<_>>()
        });
        let workspace = scope.spawn(|| {
            (0..24)
                .map(|_| resolve_through_host(&host, BARREL, "P"))
                .collect::<Vec<_>>()
        });
        assert!(creating
            .join()
            .unwrap()
            .into_iter()
            .all(|answer| answer == p_from_helper()));
        assert!(without
            .join()
            .unwrap()
            .into_iter()
            .all(|answer| answer.is_none()));
        assert!(workspace
            .join()
            .unwrap()
            .into_iter()
            .all(|answer| answer.is_none()));
    });
}

/// A session request pinned across many other overlay requests still
/// answers for its own view, and the resident overlay structures stay
/// bounded.
#[test]
fn a_pinned_session_request_outlives_overlay_churn() {
    let (_, host) = make_host(&[]);
    upsert(&host, BARREL, "export * from \"./helper\";\n");
    let mut sources: FxHashMap<String, Arc<str>> = FxHashMap::default();
    sources.insert(HELPER.to_string(), Arc::from(HELPER_SOURCE));
    let mut hashes = FxHashMap::default();
    hashes.insert(
        HELPER.to_string(),
        crate::hash::hash_16(HELPER_SOURCE.as_bytes()),
    );
    let deleted = std::collections::HashSet::new();
    let view = OverlaidViewRef::new(&host, &sources, &hashes, &deleted);
    let pinned = host
        .resolver_store_view_read()
        .into_owned_view()
        .with_session_overlay(&host, &view);
    let ctx = SessionResolverContext::new(
        &host,
        &view,
        &pinned,
        Arc::new(CanonicalCompletionOverlay::new()),
    );
    assert_eq!(
        ctx.resolve_named_type_export_target_shallow(BARREL, "P"),
        p_from_helper()
    );
    for cycle in 0..40 {
        let scratch = format!("/workspace/scratch{cycle}.ts");
        let answer =
            resolve_through_session(&host, &[(scratch.as_str(), "export {}\n")], BARREL, "P");
        assert_eq!(answer, None, "cycle {cycle}");
    }
    assert_eq!(
        ctx.resolve_named_type_export_target_shallow(BARREL, "P"),
        p_from_helper(),
        "the pinned request still answers for its own view"
    );
    let resources = host.ws().resource_snapshot();
    assert!(
        resources.overlay_resolution_slots <= 8,
        "the overlay lane holds one slot per distinct query, not per request: {resources:?}"
    );
}

// ---------------------------------------------------------------------------
// Work: repeated demands for unchanged complete queries run no producer.
// ---------------------------------------------------------------------------

/// U = the distinct complete resolution queries a request demands. Across
/// repeated requests over the same overlay the resolver runs once per query
/// the overlay reaches and never again; the queries it cannot reach reuse
/// the workspace's answers; and the overlay-side work — root construction,
/// probes, manifest parses — is counted apart so a relocated cost cannot
/// pass for an eliminated one.
#[test]
fn repeated_overlay_requests_run_each_resolution_producer_once() {
    const DEPS: usize = 6;
    let dep_paths: Vec<String> = (0..DEPS).map(|i| format!("/workspace/dep{i}.ts")).collect();
    let mut files: Vec<(String, String)> = dep_paths
        .iter()
        .map(|path| (path.clone(), "export const x = 1\n".to_string()))
        .collect();
    files.push((OWNER.to_string(), "export {}\n".to_string()));
    let files_ref: Vec<(&str, &str)> = files
        .iter()
        .map(|(path, source)| (path.as_str(), source.as_str()))
        .collect();
    let specifiers: Vec<String> = (0..DEPS)
        .map(|i| format!("./dep{i}"))
        .chain(std::iter::once("./fresh".to_string()))
        .collect();
    let overlay: &[(&str, &str)] = &[("/workspace/fresh.ts", "export const fresh = 1\n")];

    // U for the one overlay-reached specifier, measured cold on its own.
    let (_, reference) = make_host(&files_ref);
    let before = producer_runs(&reference);
    assert_eq!(
        type_route_through_session(&reference, overlay, OWNER, "./fresh").as_deref(),
        Some("/workspace/fresh.ts")
    );
    let fresh_queries = producer_runs(&reference) - before;
    assert!(
        fresh_queries > 0,
        "the overlay-reached query runs cold once"
    );

    let (_, host) = make_host(&files_ref);
    // The workspace answers every workspace specifier first.
    for specifier in specifiers.iter().take(DEPS) {
        assert!(type_route_through_host(&host, OWNER, specifier).is_some());
    }
    let warm_workspace = producer_runs(&host);

    for request in 0..4 {
        let before = producer_runs(&host);
        let work = with_session(&host, overlay, &[], |ctx, store_view| {
            for _ in 0..3 {
                for specifier in &specifiers {
                    let target = ctx.resolve_type_dependency_canonical(OWNER, specifier);
                    assert!(target.is_some(), "{specifier} resolves");
                }
            }
            store_view
                .resolution_overlay()
                .expect("an overlay-bearing session view carries its snapshot")
                .work_counts()
        });
        let ran = producer_runs(&host) - before;
        if request == 0 {
            assert_eq!(
                ran, fresh_queries,
                "the first request runs only the queries the overlay reaches; \
                 the workspace answers it cannot reach are reused"
            );
            assert!(
                work.probes > 0,
                "the cold query probes through the request's own snapshot — \
                 the one its store view validates against"
            );
        } else {
            assert_eq!(ran, 0, "request {request} repeats unchanged queries");
            assert_eq!(work.probes, 0, "request {request} probes nothing");
        }
        assert_eq!(
            work.root_builds, 1,
            "request {request} composes its effective root once"
        );
        assert_eq!(work.manifest_parses, 0);
    }
    assert!(warm_workspace > 0);
}

/// The route layer on top: repeated session requests over one overlay
/// answer an unresolved-wildcard route from the route cache, validated
/// against each request's own effective world — no resolver run, no
/// witness rebuilt, no witness replayed.
#[test]
fn repeated_session_route_reads_rebuild_nothing() {
    use crate::host_manage::import_route_witness::{
        witness_builds_for_tests, witness_replays_for_tests,
    };
    let (_, host) = make_host(&[]);
    upsert(&host, BARREL, "export * from \"./helper\";\n");
    let unrelated: &[(&str, &str)] = &[("/elsewhere/scratch.ts", "export {}\n")];
    assert_eq!(resolve_through_session(&host, unrelated, BARREL, "P"), None);
    for request in 1..4 {
        let runs = producer_runs(&host);
        let builds = witness_builds_for_tests();
        let replays = witness_replays_for_tests();
        assert_eq!(
            resolve_through_session(&host, unrelated, BARREL, "P"),
            None,
            "request {request}"
        );
        assert_eq!(
            producer_runs(&host),
            runs,
            "request {request} runs no resolver"
        );
        assert_eq!(
            witness_builds_for_tests(),
            builds,
            "request {request} builds no witness"
        );
        assert_eq!(
            witness_replays_for_tests(),
            replays,
            "request {request} replays no witness"
        );
    }
}
