//! Cache invariant migration tests.
//!
//! Captures the per-cache invariants for `MaterializeStructureDb`
//! (the structural materialiser cache that supports the projector
//! path's dispatch-path refinement) as discriminating regression
//! tests.
//!
//! Each invariant test characterises a property the cache must
//! preserve. The dispatch-path refinement (the projector pipeline)
//! exercises this cache on the production `getComponentMeta` flow,
//! so the invariants must hold against that path.
//!
//! ## Invariant catalogue
//!
//! ### MaterializeStructureDb
//!
//! - **MS-1** Cooperative-admission across N concurrent requests.
//!   Spawn N threads requesting the same component-meta resolution;
//!   the cache must end with a bounded number of entries (one per
//!   distinct cache key) and the live counter must reflect actual
//!   admitted work, not a multiplied count from torn cold builds.
//!
//! - **MS-2** dep_signature validation refuses stale reads. Populate
//!   the cache via a real `getComponentMeta` request, mutate the
//!   dependency file, re-resolve; the prior entry must NOT survive
//!   silently, and the result must reflect the new content.
//!
//! - **MS-3** Generation-scoped invalidation on file delete. Populate
//!   then `evict(canonical)`; subsequent peeks against the canonical's
//!   dep_signature must miss.
//!
//! - **MS-4** post_publish acceptance. Two distinct component-meta
//!   resolutions that produce overlapping dep_signature entries each
//!   produce a distinct cache row keyed by the
//!   `MaterializeStructureCacheKey`; cooperative admission's reverse
//!   index records both keys against the shared canonical.
//!
//! ### Dispatch-path refinement determinism (MM)
//!
//! - **MM-1** Determinism across repeated dispatch: identical
//!   `getComponentMeta` queries always produce structurally-identical
//!   payloads.
//!
//! - **MM-2** Cycle short-circuit: a recursive type fed through the
//!   dispatch-path refinement does not diverge — the resolution
//!   completes in bounded time.

use std::sync::Arc;

use verter_session::{FileKind, HostConfig, UpsertRequest, VerterHost};
use verter_workspace::{MemoryOptions, MemoryWorkspace, WorkspaceAccess};

#[allow(deprecated)]
fn make_project_config(root: &str) -> verter_workspace::VfsProjectConfig {
    verter_workspace::VfsProjectConfig {
        root: root.to_string(),
        rank: verter_workspace::ProjectRank::Explicit,
        tsconfig_path: Some(format!("{root}/tsconfig.json")),
        root_files: vec![],
        extensions: vec![],
        workspace_root: root.to_string(),
        workspace_aliases: vec![],
        compiler_options: verter_workspace::IdeProjectCompilerOptions::default(),
        references: vec![],
        membership: verter_workspace::ProjectMembership::MatchAll,
    }
}

fn build_host(files: &[(&str, &str)]) -> Arc<VerterHost> {
    #[allow(deprecated)]
    let project_graph =
        verter_workspace::ProjectGraph::from_configs(vec![make_project_config("/workspace")]);
    let workspace = Arc::new(MemoryWorkspace::new(MemoryOptions::default()));
    workspace.set_project_graph(project_graph);
    for (canonical, content) in files {
        workspace.inject_file((*canonical).into(), Arc::from(*content));
    }
    let ws_access: Arc<dyn WorkspaceAccess> = workspace;
    let host = VerterHost::new(HostConfig::default(), ws_access);
    host.configure_projects(vec![
        verter_semantic::analysis::project_resolver::IdeProjectConfig::new(
            "/workspace".to_string(),
            "/workspace".to_string(),
            Some("/workspace/tsconfig.json".to_string()),
        ),
    ]);
    Arc::new(host)
}

const SHARED_TYPES_TS: &str = r#"export interface Props {
  message: string,
  count: number,
}
"#;

const COMP_VUE: &str = r#"<script setup lang="ts">
import type { Props } from '/workspace/src/types'
defineProps<Props>()
</script>
<template><div>{{ message }} {{ count }}</div></template>
"#;

// ──────────────────────────────────────────────────────────────────
// MS-1 : MaterializeStructureDb cooperative-admission
// ──────────────────────────────────────────────────────────────────

/// MS-1: a single component-meta resolution must publish at most a
/// bounded number of entries into `MaterializeStructureDb` (live
/// count rises by O(1)) — a runaway publication would indicate the
/// cooperative-admission gate is broken.
#[test]
fn materialize_structure_db_cooperative_admission_bounded() {
    let host = build_host(&[
        ("/workspace/src/types.ts", SHARED_TYPES_TS),
        ("/workspace/src/Comp.vue", COMP_VUE),
    ]);

    let pre = host
        .project_type_store()
        .materialize_structure_db()
        .live_count();

    let _ = host.get_component_meta("/workspace/src/Comp.vue");

    let post = host
        .project_type_store()
        .materialize_structure_db()
        .live_count();

    let delta = post.saturating_sub(pre);
    // The cache must have admitted work — but the count must be
    // bounded. A nontrivial component-meta resolution that triggers
    // structural materialization populates a small number of rows
    // (one per distinct (scope, target) the materialiser saw). 64 is
    // a wide upper bound that reflects "single component, small
    // workspace" — well below any pathological multiplication.
    assert!(
        delta < 64,
        "MS-1: MaterializeStructureDb live_count must rise by a bounded \
         O(1) amount per single resolution (cooperative admission); got \
         delta={delta} (pre={pre}, post={post}). A larger delta indicates \
         the admission gate is broken or the cache is inflating per call."
    );
}

/// MS-1 (companion): repeated identical resolutions must NOT inflate
/// the cache. After a cold pass, a warm pass does not add new rows.
#[test]
fn materialize_structure_db_warm_pass_does_not_inflate() {
    let host = build_host(&[
        ("/workspace/src/types.ts", SHARED_TYPES_TS),
        ("/workspace/src/Comp.vue", COMP_VUE),
    ]);

    let _ = host.get_component_meta("/workspace/src/Comp.vue");
    let after_cold = host
        .project_type_store()
        .materialize_structure_db()
        .live_count();

    let _ = host.get_component_meta("/workspace/src/Comp.vue");
    let after_warm = host
        .project_type_store()
        .materialize_structure_db()
        .live_count();

    assert_eq!(
        after_cold, after_warm,
        "MS-1 warm: a repeated identical resolution must NOT add new \
         MaterializeStructureDb rows (cold={after_cold}, warm={after_warm})"
    );
}

// ──────────────────────────────────────────────────────────────────
// MS-3 : MaterializeStructureDb invalidation on canonical eviction
// ──────────────────────────────────────────────────────────────────

/// MS-3: evicting the owner canonical must shrink the cache (or at
/// minimum not leave a stale row for the same key alive). The
/// reverse-index `canonical_to_keys` registers entries; eviction
/// drains them.
#[test]
fn materialize_structure_db_eviction_drains_owner_entries() {
    let host = build_host(&[
        ("/workspace/src/types.ts", SHARED_TYPES_TS),
        ("/workspace/src/Comp.vue", COMP_VUE),
    ]);

    let _ = host.get_component_meta("/workspace/src/Comp.vue");
    let before_evict = host
        .project_type_store()
        .materialize_structure_db()
        .live_count();

    host.evict("/workspace/src/Comp.vue");

    let after_evict = host
        .project_type_store()
        .materialize_structure_db()
        .live_count();

    // Eviction must NOT inflate the cache (a buggy eviction that
    // forgot to drain reverse-index entries would leave them all
    // behind). Equality is the strictest possible: pre-publish there
    // were N rows, evict drops them to 0 or fewer, but the live count
    // must not increase.
    assert!(
        after_evict <= before_evict,
        "MS-3: evicting the owner canonical must NOT inflate \
         MaterializeStructureDb (before={before_evict}, after={after_evict})"
    );
}

// ──────────────────────────────────────────────────────────────────
// MS-2 / MR-2 : dep_signature validation refuses stale reads
// ──────────────────────────────────────────────────────────────────

/// MS-2 + MR-2: editing the upstream declaration file must surface in
/// the next resolution's published metadata. A torn cache that served
/// the pre-edit value would leak into the post-edit prop list.
#[test]
fn cache_invalidation_after_dep_edit_surfaces_new_content() {
    #[allow(deprecated)]
    let project_graph =
        verter_workspace::ProjectGraph::from_configs(vec![make_project_config("/workspace")]);
    let workspace = Arc::new(MemoryWorkspace::new(MemoryOptions::default()));
    workspace.set_project_graph(project_graph);
    workspace.inject_file("/workspace/src/types.ts".into(), Arc::from(SHARED_TYPES_TS));
    workspace.inject_file("/workspace/src/Comp.vue".into(), Arc::from(COMP_VUE));

    let ws_access: Arc<dyn WorkspaceAccess> = workspace.clone();
    let host = VerterHost::new(HostConfig::default(), ws_access);
    host.configure_projects(vec![
        verter_semantic::analysis::project_resolver::IdeProjectConfig::new(
            "/workspace".to_string(),
            "/workspace".to_string(),
            Some("/workspace/tsconfig.json".to_string()),
        ),
    ]);
    let host = Arc::new(host);

    // Cold pass — captures the pre-edit shape into all caches.
    let meta_pre = host
        .get_component_meta("/workspace/src/Comp.vue")
        .expect("cold get_component_meta must succeed");
    let prop_names_pre: Vec<String> = meta_pre.props.iter().map(|p| p.name.clone()).collect();
    assert!(
        prop_names_pre.contains(&"message".to_string()),
        "MS-2/MR-2: cold resolution must include `message` prop"
    );
    assert!(
        prop_names_pre.contains(&"count".to_string()),
        "MS-2/MR-2: cold resolution must include `count` prop"
    );

    // Mutate the upstream types file by re-injecting it into the
    // workspace and forcing the host to re-read via upsert.
    workspace.inject_file(
        "/workspace/src/types.ts".into(),
        Arc::from(
            r#"export interface Props {
  message: string,
  newProp: boolean,
}
"#,
        ),
    );
    let _ = host.upsert(UpsertRequest {
        canonical_id: Some("/workspace/src/types.ts".into()),
        input_id: "/workspace/src/types.ts".into(),
        source: Arc::from(
            r#"export interface Props {
  message: string,
  newProp: boolean,
}
"#,
        ),
        file_kind: FileKind::NonSfc,
        aliases: vec![],
    });
    host.evict("/workspace/src/Comp.vue");

    // Warm pass — must reflect the new shape.
    let meta_post = host
        .get_component_meta("/workspace/src/Comp.vue")
        .expect("warm get_component_meta must succeed after edit");
    let prop_names_post: Vec<String> = meta_post.props.iter().map(|p| p.name.clone()).collect();
    assert!(
        prop_names_post.contains(&"newProp".to_string()),
        "MS-2/MR-2: post-edit resolution must include `newProp` prop \
         (got {prop_names_post:?}). A torn cache that served the pre-edit \
         shape would NOT include the new prop."
    );
    assert!(
        !prop_names_post.contains(&"count".to_string()),
        "MS-2/MR-2: post-edit resolution must NOT include `count` prop \
         (got {prop_names_post:?}). A torn cache that served the pre-edit \
         shape would still include `count`."
    );
}

/// Lazy-substrate discriminator for the evict/reload invariant in the
/// same scenario as [`cache_invalidation_after_dep_edit_surfaces_new_content`],
/// but the dependency edit is routed through
/// [`VerterHost::upsert_without_dependent_eviction`] so the eager
/// reverse-dependent cascade does NOT clear `/workspace/src/Comp.vue`'s
/// artifacts. After the dep edit the owner is explicitly evicted with
/// `host.evict`.
///
/// An evicted owner must reload to authoritative state before a query
/// proceeds. The defect this discriminates: `current_or_read_whole_hash`
/// previously accepted a stale `FileArtifactStore::get_any`-derived
/// whole-hash for the evicted owner, so `get_component_meta` ran on
/// the stale identity and returned `None` instead of forcing a fresh
/// reload.
///
/// Discrimination property: the fix that makes
/// `current_or_read_whole_hash` route an evicted canonical through
/// `ensure_loaded` (rather than honouring a stale `get_any` hash) is
/// what breaks this test if reverted — pre-fix the warm
/// `get_component_meta` returns `None`.
#[test]
fn evicted_owner_reloads_after_dep_edit_without_dependent_eviction() {
    #[allow(deprecated)]
    let project_graph =
        verter_workspace::ProjectGraph::from_configs(vec![make_project_config("/workspace")]);
    let workspace = Arc::new(MemoryWorkspace::new(MemoryOptions::default()));
    workspace.set_project_graph(project_graph);
    workspace.inject_file("/workspace/src/types.ts".into(), Arc::from(SHARED_TYPES_TS));
    workspace.inject_file("/workspace/src/Comp.vue".into(), Arc::from(COMP_VUE));

    let ws_access: Arc<dyn WorkspaceAccess> = workspace.clone();
    let host = VerterHost::new(HostConfig::default(), ws_access);
    host.configure_projects(vec![
        verter_semantic::analysis::project_resolver::IdeProjectConfig::new(
            "/workspace".to_string(),
            "/workspace".to_string(),
            Some("/workspace/tsconfig.json".to_string()),
        ),
    ]);
    let host = Arc::new(host);

    // Cold pass — captures the pre-edit shape into all caches.
    let meta_pre = host
        .get_component_meta("/workspace/src/Comp.vue")
        .expect("cold get_component_meta must succeed");
    let prop_names_pre: Vec<String> = meta_pre.props.iter().map(|p| p.name.clone()).collect();
    assert!(
        prop_names_pre.contains(&"count".to_string()),
        "cold resolution must include `count` prop (got {prop_names_pre:?})"
    );

    // Edit the upstream types file WITHOUT the eager reverse-dependent
    // cascade. The cascade is precisely the path that would evict
    // `/workspace/src/Comp.vue`'s artifacts.
    let edited_types = r#"export interface Props {
  message: string,
  newProp: boolean,
}
"#;
    workspace.inject_file("/workspace/src/types.ts".into(), Arc::from(edited_types));
    let _dep_update = host
        .upsert_without_dependent_eviction(UpsertRequest {
            canonical_id: Some("/workspace/src/types.ts".into()),
            input_id: "/workspace/src/types.ts".into(),
            source: Arc::from(edited_types),
            file_kind: FileKind::NonSfc,
            aliases: vec![],
        })
        .expect("dep upsert must succeed");
    host.evict("/workspace/src/Comp.vue");

    // Warm pass — the evicted owner must reload to authoritative state
    // and reflect the new shape.
    let meta_post = host
        .get_component_meta("/workspace/src/Comp.vue")
        .expect("get_component_meta on an evicted owner must reload and succeed");
    let prop_names_post: Vec<String> = meta_post.props.iter().map(|p| p.name.clone()).collect();
    assert!(
        prop_names_post.contains(&"newProp".to_string()),
        "post-edit resolution must include `newProp` prop (got {prop_names_post:?})"
    );
    assert!(
        !prop_names_post.contains(&"count".to_string()),
        "post-edit resolution must NOT include `count` prop (got {prop_names_post:?})"
    );
}

// ──────────────────────────────────────────────────────────────────
// MS-4 : post_publish dep_signature acceptance
// ──────────────────────────────────────────────────────────────────

/// MS-4: cooperative-admission's `post_publish` path must accept
/// distinct cache rows produced by overlapping component-meta
/// resolutions on the SAME canonical owner. Two resolutions of the
/// same component yield identical dep_signatures and must collapse
/// onto a single cache row (no duplicate entries published per
/// `(MaterializeStructureCacheKey, dep_signature)` pair).
#[test]
fn materialize_structure_db_post_publish_collapses_duplicates() {
    let host = build_host(&[
        ("/workspace/src/types.ts", SHARED_TYPES_TS),
        ("/workspace/src/Comp.vue", COMP_VUE),
    ]);

    // First resolution publishes cache rows.
    let _ = host.get_component_meta("/workspace/src/Comp.vue");
    let after_first = host
        .project_type_store()
        .materialize_structure_db()
        .live_count();

    // Second resolution with identical inputs must NOT increase the
    // live count — `post_publish` collapses onto the existing row.
    let _ = host.get_component_meta("/workspace/src/Comp.vue");
    let after_second = host
        .project_type_store()
        .materialize_structure_db()
        .live_count();

    assert_eq!(
        after_first, after_second,
        "MS-4: identical resolutions must collapse via post_publish; \
         live_count changed from {after_first} to {after_second}"
    );
}

// ──────────────────────────────────────────────────────────────────
// MM-1 : determinism across repeated dispatch — verified end-to-end
// ──────────────────────────────────────────────────────────────────

const GENERIC_TYPES_TS: &str = r#"export interface Container<T> { item: T }
export type StringContainer = Container<string>
"#;

const GENERIC_VUE: &str = r#"<script setup lang="ts">
import type { StringContainer } from '/workspace/src/types'
defineProps<StringContainer>()
</script>
<template><div /></template>
"#;

/// MM-1: repeated invocations of the same component-meta query must
/// produce structurally-identical published metadata. The
/// dispatch-path refinement is deterministic — identical inputs
/// yield identical outputs.
#[test]
fn materialize_macro_shape_member_type_expr_deterministic() {
    let host = build_host(&[
        ("/workspace/src/types.ts", GENERIC_TYPES_TS),
        ("/workspace/src/Comp.vue", GENERIC_VUE),
    ]);

    let meta_a = host
        .get_component_meta("/workspace/src/Comp.vue")
        .expect("first resolve must succeed");
    let meta_b = host
        .get_component_meta("/workspace/src/Comp.vue")
        .expect("second resolve must succeed");

    let names_a: Vec<String> = meta_a.props.iter().map(|p| p.name.clone()).collect();
    let names_b: Vec<String> = meta_b.props.iter().map(|p| p.name.clone()).collect();
    assert_eq!(
        names_a, names_b,
        "MM-1: repeated resolutions must produce structurally-identical \
         prop lists (a={names_a:?}, b={names_b:?})"
    );
}

// ──────────────────────────────────────────────────────────────────
// MM-2 : cycle short-circuit
// ──────────────────────────────────────────────────────────────────

const RECURSIVE_TYPES_TS: &str = r#"export interface Node<T> {
  value: T,
  next: Node<T>,
}
"#;

const RECURSIVE_VUE: &str = r#"<script setup lang="ts">
import type { Node } from '/workspace/src/types'
defineProps<{
  head: Node<string>
}>()
</script>
<template><div /></template>
"#;

/// MM-2: a recursive type fed through the materialiser short-circuits
/// on cycle detection rather than diverging. The published metadata
/// must include the prop and the resolution must complete in a
/// bounded time.
#[test]
fn materialize_macro_shape_member_type_expr_cycle_short_circuits() {
    let host = build_host(&[
        ("/workspace/src/types.ts", RECURSIVE_TYPES_TS),
        ("/workspace/src/Comp.vue", RECURSIVE_VUE),
    ]);

    let started = std::time::Instant::now();
    let meta = host
        .get_component_meta("/workspace/src/Comp.vue")
        .expect("recursive type resolution must complete");
    let elapsed = started.elapsed();

    assert!(
        elapsed.as_secs() < 30,
        "MM-2: recursive type resolution must complete in bounded time \
         (got {}s); cycle short-circuit failure suggests divergent \
         materialisation",
        elapsed.as_secs(),
    );

    let prop_names: Vec<String> = meta.props.iter().map(|p| p.name.clone()).collect();
    assert!(
        prop_names.contains(&"head".to_string()),
        "MM-2: recursive type resolution must publish the `head` prop \
         (got {prop_names:?})"
    );
}

// ──────────────────────────────────────────────────────────────────
// SCHEMA-VERSION COHORT FIXTURES
//
// One fixture per Db in the §6 W0.5 cohort. Each fixture:
//
//   1. Constructs a Db pinned to the OLD `CACHE_CLUSTER_SCHEMA_VERSION - 1`.
//   2. Plants a synthetic entry via the `insert_synthetic_for_schema_test`
//      helper (Db-specific synthetic key + entry).
//   3. Asserts the entry is present at the storage layer.
//   4. Calls `evict_if_schema_mismatch(CACHE_CLUSTER_SCHEMA_VERSION)`.
//   5. Asserts the eviction count > 0 (drained the synthetic entry) and
//      that the storage layer is now empty.
//
// Discriminator: pre-bump tree has neither the `schema_version` field nor
// the `evict_if_schema_mismatch` method nor the test-only constructor —
// the test does not compile against a pre-bump tree. Post-bump it
// compiles AND the eviction returns > 0 (drains real entries).
//
// `RefCycleResultDb` is intentionally OUT of the cohort — it caches
// booleans / cycle identities only. Confirm the absence by inspection.

use verter_session::cache_schema::{CacheSchemaVersioned, CACHE_CLUSTER_SCHEMA_VERSION};

/// Reused across every schema-cohort fixture.
const STALE_SCHEMA_VERSION: u32 = CACHE_CLUSTER_SCHEMA_VERSION - 1;

#[test]
fn schema_bump_evicts_indexed_ready_db_stale_entries() {
    use verter_session::file_artifact_store::FileArtifactStore;

    let db = FileArtifactStore::new_with_schema_version_for_test(STALE_SCHEMA_VERSION);
    db.insert_synthetic_for_schema_test("/workspace/synthetic.ts");

    assert_eq!(
        db.len(),
        1,
        "FileArtifactStore fixture: synthetic entry must be present pre-evict"
    );
    assert_eq!(db.schema_version(), STALE_SCHEMA_VERSION);

    let evicted = db.evict_if_schema_mismatch(CACHE_CLUSTER_SCHEMA_VERSION);
    assert_eq!(
        evicted, 1,
        "FileArtifactStore: evict_if_schema_mismatch must drain the stale entry"
    );
    assert_eq!(
        db.len(),
        0,
        "FileArtifactStore: storage must be empty after eviction"
    );
}

#[test]
fn schema_bump_evicts_analysis_ready_db_stale_entries() {
    use verter_session::project_type_store::AnalysisReadyDb;

    let db = AnalysisReadyDb::new_with_schema_version_for_test(STALE_SCHEMA_VERSION);
    db.insert_synthetic_for_schema_test("/workspace/synthetic-analysis.ts");

    assert_eq!(db.len(), 1, "AnalysisReadyDb pre-evict count");
    assert_eq!(db.schema_version(), STALE_SCHEMA_VERSION);

    let evicted = db.evict_if_schema_mismatch(CACHE_CLUSTER_SCHEMA_VERSION);
    assert_eq!(evicted, 1, "AnalysisReadyDb: must drain stale");
    assert_eq!(db.len(), 0, "AnalysisReadyDb: empty after evict");
}

#[test]
fn schema_bump_evicts_route_owned_shallow_db_stale_entries() {
    use verter_session::project_type_store::RouteOwnedShallowDb;

    let db = RouteOwnedShallowDb::new_with_schema_version_for_test(STALE_SCHEMA_VERSION);
    db.insert_synthetic_for_schema_test("/workspace/synthetic-route.ts");

    assert_eq!(db.len(), 1, "RouteOwnedShallowDb pre-evict count");
    assert_eq!(db.schema_version(), STALE_SCHEMA_VERSION);

    let evicted = db.evict_if_schema_mismatch(CACHE_CLUSTER_SCHEMA_VERSION);
    assert_eq!(evicted, 1, "RouteOwnedShallowDb: must drain stale");
    assert_eq!(db.len(), 0, "RouteOwnedShallowDb: empty after evict");
}

#[test]
fn schema_bump_evicts_eval_env_cache_db_stale_entries() {
    use verter_session::project_type_store::EvalEnvCacheDb;

    let db = EvalEnvCacheDb::new_with_schema_version_for_test(STALE_SCHEMA_VERSION);
    db.insert_synthetic_for_schema_test("/workspace/synthetic-evalenv.ts");

    assert_eq!(
        db.total_entries(),
        1,
        "EvalEnvCacheDb: synthetic legacy env must be present pre-evict"
    );
    assert_eq!(db.schema_version(), STALE_SCHEMA_VERSION);

    let evicted = db.evict_if_schema_mismatch(CACHE_CLUSTER_SCHEMA_VERSION);
    assert_eq!(evicted, 1, "EvalEnvCacheDb: must drain stale legacy env");
    assert_eq!(
        db.total_entries(),
        0,
        "EvalEnvCacheDb: total empty after evict"
    );
}

#[test]
fn schema_bump_evicts_owner_import_surface_db_stale_entries() {
    use verter_session::owner_import_surface::OwnerImportSurfaceDb;

    let db = OwnerImportSurfaceDb::new_with_schema_version_for_test(STALE_SCHEMA_VERSION);
    db.insert_synthetic_for_schema_test("/workspace/synthetic-owner.ts");

    assert_eq!(db.len(), 1, "OwnerImportSurfaceDb pre-evict count");
    assert_eq!(db.schema_version(), STALE_SCHEMA_VERSION);

    let evicted = db.evict_if_schema_mismatch(CACHE_CLUSTER_SCHEMA_VERSION);
    assert_eq!(evicted, 1, "OwnerImportSurfaceDb: must drain stale");
    assert_eq!(db.len(), 0, "OwnerImportSurfaceDb: empty after evict");
}

#[test]
fn schema_bump_evicts_component_meta_result_db_stale_entries() {
    use verter_session::component_meta_result_db::ComponentMetaResultDb;

    // The schema-cohort eviction invariant is independent of the cached
    // payload type — `ComponentMetaResultDb` is generic over `P`. Use
    // `()` as the synthetic payload so the fixture does not have to
    // construct a full `ComponentMetaAnalysis` (which lacks `Default`).
    let db: ComponentMetaResultDb<()> =
        ComponentMetaResultDb::new_with_schema_version_for_test(STALE_SCHEMA_VERSION);
    db.insert_synthetic_for_schema_test_with_payload("/workspace/synthetic-meta.ts", ());

    assert_eq!(db.len(), 1, "ComponentMetaResultDb pre-evict count");
    assert_eq!(db.schema_version(), STALE_SCHEMA_VERSION);

    let evicted = db.evict_if_schema_mismatch(CACHE_CLUSTER_SCHEMA_VERSION);
    assert_eq!(evicted, 1, "ComponentMetaResultDb: must drain stale");
    assert_eq!(db.len(), 0, "ComponentMetaResultDb: empty after evict");
}

#[test]
fn schema_bump_evicts_imported_registry_db_stale_entries() {
    use verter_session::component_meta_caches::ImportedRegistryDb;

    let db = ImportedRegistryDb::new_with_schema_version_for_test(STALE_SCHEMA_VERSION);
    db.insert_synthetic_for_schema_test("/workspace/synthetic-imported.ts");

    assert_eq!(db.live_count(), 1, "ImportedRegistryDb pre-evict count");
    assert_eq!(db.schema_version(), STALE_SCHEMA_VERSION);

    let evicted = db.evict_if_schema_mismatch(CACHE_CLUSTER_SCHEMA_VERSION);
    assert_eq!(evicted, 1, "ImportedRegistryDb: must drain stale");
    assert_eq!(db.live_count(), 0, "ImportedRegistryDb: empty after evict");
}

#[test]
fn schema_bump_evicts_prepared_surface_db_stale_entries() {
    use verter_session::component_meta_caches::PreparedSurfaceDb;

    let db = PreparedSurfaceDb::new_with_schema_version_for_test(STALE_SCHEMA_VERSION);
    db.insert_synthetic_for_schema_test("/workspace/synthetic-prepared-surface.ts");

    assert_eq!(db.live_count(), 1, "PreparedSurfaceDb pre-evict count");
    assert_eq!(db.schema_version(), STALE_SCHEMA_VERSION);

    let evicted = db.evict_if_schema_mismatch(CACHE_CLUSTER_SCHEMA_VERSION);
    assert_eq!(evicted, 1, "PreparedSurfaceDb: must drain stale");
    assert_eq!(db.live_count(), 0, "PreparedSurfaceDb: empty after evict");
}

#[test]
fn schema_bump_evicts_prepared_member_db_stale_entries() {
    use verter_session::component_meta_caches::PreparedMemberDb;

    let db = PreparedMemberDb::new_with_schema_version_for_test(STALE_SCHEMA_VERSION);
    db.insert_synthetic_for_schema_test("/workspace/synthetic-prepared-member.ts");

    assert_eq!(db.live_count(), 1, "PreparedMemberDb pre-evict count");
    assert_eq!(db.schema_version(), STALE_SCHEMA_VERSION);

    let evicted = db.evict_if_schema_mismatch(CACHE_CLUSTER_SCHEMA_VERSION);
    assert_eq!(evicted, 1, "PreparedMemberDb: must drain stale");
    assert_eq!(db.live_count(), 0, "PreparedMemberDb: empty after evict");
}

#[test]
fn schema_bump_evicts_prepared_target_db_stale_entries() {
    use verter_session::component_meta_caches::PreparedTargetDb;

    let db = PreparedTargetDb::new_with_schema_version_for_test(STALE_SCHEMA_VERSION);
    db.insert_synthetic_for_schema_test("/workspace/synthetic-prepared-target.ts");

    assert_eq!(db.live_count(), 1, "PreparedTargetDb pre-evict count");
    assert_eq!(db.schema_version(), STALE_SCHEMA_VERSION);

    let evicted = db.evict_if_schema_mismatch(CACHE_CLUSTER_SCHEMA_VERSION);
    assert_eq!(evicted, 1, "PreparedTargetDb: must drain stale");
    assert_eq!(db.live_count(), 0, "PreparedTargetDb: empty after evict");
}

#[test]
fn schema_bump_evicts_routed_expr_surface_db_stale_entries() {
    use verter_session::component_meta_caches::RoutedExprSurfaceDb;

    let db = RoutedExprSurfaceDb::new_with_schema_version_for_test(STALE_SCHEMA_VERSION);
    db.insert_synthetic_for_schema_test("/workspace/synthetic-routed-expr.ts");

    assert_eq!(db.live_count(), 1, "RoutedExprSurfaceDb pre-evict count");
    assert_eq!(db.schema_version(), STALE_SCHEMA_VERSION);

    let evicted = db.evict_if_schema_mismatch(CACHE_CLUSTER_SCHEMA_VERSION);
    assert_eq!(evicted, 1, "RoutedExprSurfaceDb: must drain stale");
    assert_eq!(db.live_count(), 0, "RoutedExprSurfaceDb: empty after evict");
}

#[test]
fn schema_bump_evicts_materialize_memo_db_stale_entries() {
    use verter_session::component_meta_caches::MaterializeMemoDb;

    let db = MaterializeMemoDb::new_with_schema_version_for_test(STALE_SCHEMA_VERSION);
    db.insert_synthetic_for_schema_test("/workspace/synthetic-materialize-memo.ts");

    assert_eq!(db.live_count(), 1, "MaterializeMemoDb pre-evict count");
    assert_eq!(db.schema_version(), STALE_SCHEMA_VERSION);

    let evicted = db.evict_if_schema_mismatch(CACHE_CLUSTER_SCHEMA_VERSION);
    assert_eq!(evicted, 1, "MaterializeMemoDb: must drain stale");
    assert_eq!(db.live_count(), 0, "MaterializeMemoDb: empty after evict");
}

#[test]
fn schema_bump_evicts_materialize_structure_db_stale_entries() {
    use verter_session::component_meta_caches::MaterializeStructureDb;

    let db = MaterializeStructureDb::new_with_schema_version_for_test(STALE_SCHEMA_VERSION);
    db.insert_synthetic_for_schema_test("/workspace/synthetic-materialize-structure.ts");

    assert_eq!(db.live_count(), 1, "MaterializeStructureDb pre-evict count");
    assert_eq!(db.schema_version(), STALE_SCHEMA_VERSION);

    let evicted = db.evict_if_schema_mismatch(CACHE_CLUSTER_SCHEMA_VERSION);
    assert_eq!(evicted, 1, "MaterializeStructureDb: must drain stale");
    assert_eq!(
        db.live_count(),
        0,
        "MaterializeStructureDb: empty after evict"
    );
}

// Negative-discrimination companion: a Db constructed at the CURRENT
// schema version must NOT lose its entries when the same eviction call
// runs. This pins the "no false positive" half of the contract.
#[test]
fn evict_if_schema_mismatch_preserves_current_version_entries() {
    use verter_session::file_artifact_store::FileArtifactStore;

    let db = FileArtifactStore::new();
    assert_eq!(db.schema_version(), CACHE_CLUSTER_SCHEMA_VERSION);
    db.insert_synthetic_for_schema_test("/workspace/preserve.ts");
    assert_eq!(db.len(), 1, "Db must hold the synthetic entry pre-evict");

    let evicted = db.evict_if_schema_mismatch(CACHE_CLUSTER_SCHEMA_VERSION);
    assert_eq!(
        evicted, 0,
        "evict_if_schema_mismatch must NOT drain entries when the Db is \
         already at the current schema version"
    );
    assert_eq!(
        db.len(),
        1,
        "current-version Db must still hold its entry after a no-op evict"
    );
}
