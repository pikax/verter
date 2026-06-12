//! Lifecycle-cascade invariants for `host_lifecycle.rs`.
//!
//! Two pinned contracts:
//!
//! 1. `close()` is an AUTHORITY-RESET teardown. The
//!    `bump_project_generation_and_evict` reservation
//!    (`project_type_store.rs`) names `set_workspace` AND `close` as its
//!    two callers: a full teardown orphans every retained per-canonical
//!    payload, so the wide cascade must run — the per-canonical
//!    compile/derived/dependency domains drop, the query-identity DB
//!    cluster drops, and the project generation moves so a
//!    `ProjectGeneration`-rooted entry can never validate across a
//!    close→re-populate cycle.
//!
//! 2. `set_exact_resolutions` keys every operation — the workspace
//!    edge-store write AND the host-side route-mirror repair — on the
//!    NORMALIZED canonical id (the `set_import_dependencies`
//!    discipline). An alias-keyed call must behave identically to the
//!    canonical-keyed call; resolution target ids are canonicalized on
//!    admission.

use std::sync::Arc;

use crate::types::DependencyResolution;
use crate::{FileKind, HostConfig, UpsertRequest, VerterHost};

const SFC: &str = "<script setup lang=\"ts\">\n\
                   defineProps<{ alpha: string }>()\n\
                   </script>\n\
                   <template><div>{{ alpha }}</div></template>\n";

fn upsert_sfc(host: &VerterHost, canonical: &str, aliases: Vec<String>) {
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: Some(canonical.to_string()),
            input_id: canonical.to_string(),
            source: Arc::from(SFC),
            file_kind: FileKind::VueSfc,
            aliases,
        })
        .expect("upsert succeeds");
}

/// `close()` must run the authority-reset cascade
/// (`bump_project_generation_and_evict`): the per-canonical
/// derived/dependency domains are released and the project generation
/// moves. Without the cascade the heavy stores stay resident after a
/// teardown (regressing the memory-release contract `close()` exists
/// for) and `ProjectGeneration`-rooted cache entries could validate
/// against state populated before the close.
#[test]
fn close_releases_per_canonical_domains_and_moves_project_generation() {
    let host = VerterHost::new_standalone(HostConfig::default());
    upsert_sfc(&host, "/src/CloseProbe.vue", Vec::new());

    let pts = host.project_type_store();
    assert!(
        pts.derived_raw_cache().len() > 0,
        "precondition: upsert populates derived_raw_cache"
    );
    assert!(
        pts.dependency_cache().len() > 0,
        "precondition: upsert populates dependency_cache"
    );
    let generation_before = pts.current_project_generation();

    host.close();

    assert!(
        pts.derived_raw_cache().is_empty(),
        "close() must release every DerivedRawState entry (authority-reset cascade)"
    );
    assert!(
        pts.dependency_cache().is_empty(),
        "close() must release every DependencyState entry (authority-reset cascade)"
    );
    assert!(
        pts.current_project_generation() > generation_before,
        "close() must move the project generation so ProjectGeneration-rooted \
         entries cannot validate across a close→re-populate cycle \
         (before={generation_before}, after={})",
        pts.current_project_generation()
    );
}

/// An alias-keyed `set_exact_resolutions` call must behave identically
/// to the canonical-keyed call: the workspace exact table lands under
/// the CANONICAL id (so canonical-keyed import resolution sees it) and
/// the canonical's derived route mirror is cleared (owner-scoped
/// route-state repair targets the id the mirror is actually keyed by).
#[test]
fn alias_keyed_set_exact_resolutions_behaves_like_canonical_keyed() {
    let host = VerterHost::new_standalone(HostConfig::default());
    let canonical = "/src/AliasProbe.vue";
    let alias = "@probe/alias-probe.vue";
    upsert_sfc(&host, canonical, vec![alias.to_string()]);
    assert_eq!(
        host.resolve_alias_or_canonical(alias),
        canonical,
        "precondition: upsert registers the alias"
    );

    // Populate the canonical's derived route mirror through the
    // canonical-keyed route-snapshot writer.
    host.set_import_dependencies(
        canonical,
        vec![DependencyResolution {
            specifier: "route-pkg".to_string(),
            resolved_canonical_id: Some("/node_modules/route-pkg/index.d.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );
    assert!(
        host.derived_raw_cache()
            .get(canonical)
            .map(|d| !d.import_routes.is_empty())
            .unwrap_or(false),
        "precondition: route mirror populated under the canonical id"
    );

    // Alias-keyed exact-resolution push.
    host.set_exact_resolutions(
        alias,
        vec![verter_workspace::ExactResolution {
            specifier: "exact-pkg".to_string(),
            phase: verter_workspace::ResolvePhase::CodegenBlocker,
            kind: verter_workspace::ResolveRequestKind::EsmImport,
            resolved_canonical_id: Some("/node_modules/exact-pkg/index.d.ts".to_string()),
            possible_canonical_ids: vec!["/node_modules/exact-pkg/index.d.ts".to_string()],
        }],
    );

    // (a) The workspace exact table must land under the CANONICAL id:
    // canonical-keyed import resolution sees the pushed route.
    assert_eq!(
        host.resolve_import_via_workspace(canonical, "exact-pkg")
            .as_deref(),
        Some("/node_modules/exact-pkg/index.d.ts"),
        "alias-keyed set_exact_resolutions must store the exact table under \
         the canonical id, not the alias"
    );
    // (b) The canonical's route mirror must be cleared — the owner-scoped
    // repair targets the canonical-keyed DerivedRawState entry.
    assert!(
        host.derived_raw_cache()
            .get(canonical)
            .map(|d| d.import_routes.is_empty())
            .unwrap_or(true),
        "alias-keyed set_exact_resolutions must clear the CANONICAL route mirror"
    );
}

/// Resolution target ids are canonicalized on admission — the
/// `set_import_dependencies` discipline. The workspace edge store keeps
/// ids verbatim, so a Windows-style id pushed through
/// `set_exact_resolutions` must already be in canonical form when it is
/// handed back by import resolution.
#[test]
fn set_exact_resolutions_canonicalizes_resolution_target_ids() {
    let host = VerterHost::new_standalone(HostConfig::default());
    let canonical = "/src/NormProbe.vue";
    upsert_sfc(&host, canonical, Vec::new());

    host.set_exact_resolutions(
        canonical,
        vec![verter_workspace::ExactResolution {
            specifier: "win-pkg".to_string(),
            phase: verter_workspace::ResolvePhase::CodegenBlocker,
            kind: verter_workspace::ResolveRequestKind::EsmImport,
            resolved_canonical_id: Some("C:\\proj\\node_modules\\win-pkg\\index.d.ts".to_string()),
            possible_canonical_ids: vec!["C:\\proj\\node_modules\\win-pkg\\index.d.ts".to_string()],
        }],
    );

    assert_eq!(
        host.resolve_import_via_workspace(canonical, "win-pkg")
            .as_deref(),
        Some("c:/proj/node_modules/win-pkg/index.d.ts"),
        "set_exact_resolutions must canonicalize resolution target ids on \
         admission (backslashes normalized, drive letter lowercased)"
    );
}
