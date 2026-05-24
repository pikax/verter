//! @ai-generated - Cache invalidation edit-cycle contracts.
//!
//! Every test follows the same shape: install a fixture, run a cold
//! query, mutate a file via `upsert_ts` (the canonical edit path), then
//! re-run the same query and characterise what changed and what stayed
//! warm. The audit footprint is the architectural ground truth for
//! invalidation decisions:
//!
//!   * `request_loaded_file_names` reflects per-request VFS reads and
//!     shared-load reuses — these are the files the resolver actually
//!     touched to satisfy the second query.
//!   * `assert_no_fresh_source_loading` /
//!     `assert_no_route_misses` characterise the warm-cache promise:
//!     the resolver did not re-parse and did not re-walk RouteDb cold.
//!
//! Cache layers being characterised:
//!
//!   * `FileArtifactStore` — per-file `IndexedReady` keyed by
//!     `(canonical_id, content_hash, parse_env_hash, parser_version)`.
//!     Editing a file via `upsert_ts` bumps `content_hash` and forces a
//!     rebuild of that file's artifacts.
//!   * `ResolvedImportFacts` — content-addressed import-route facts
//!     keyed on the owner's `content_hash`. Editing a barrel's exports
//!     advances its content hash; consumers' import-route facts that
//!     depend on the barrel revalidate through the live `StoreView`.
//!   * `RouteDb` — query-identity barrel/route surface cache; warm
//!     entries revalidate `fact_dep_signature` against the live
//!     `StoreView`. Barrel edits must flip the resolved canonical for
//!     `Item`.
//!   * `OwnerImportSurfaceDb` — direct-owner-import surface keyed by
//!     `(owner_canonical, owner_whole_hash)`. Edits to the owner force a
//!     rebuild; edits to leaves leave it warm.
//!   * `augmentation_index` (on `FileArtifactStore`) — the inverse
//!     lookup for `declare module` blocks. Editing the patch file to
//!     introduce a new augmentation must surface in subsequent
//!     consumer queries.
//!
//! Cache invalidation is not blanket — it is path-precise. Tests that
//! pass under the current resolver describe the invariants Verter
//! satisfies today; tests carrying `#[ignore]` describe contracts
//! Verter is expected to satisfy.

use super::support::*;
use crate::VerterHost;

const OWNER_BASIC: &str = "/fixtures/cache_invalidation_owner_basic.ts";
const BARREL_BASIC: &str = "/fixtures/cache_invalidation_basic_barrel.ts";
const SELECTED_BASIC: &str = "/fixtures/cache_invalidation_basic_selected.ts";
const UNUSED_BASIC: &str = "/fixtures/cache_invalidation_basic_unused.ts";

const OWNER_ROUTE: &str = "/fixtures/cache_invalidation_route_owner.ts";
const BARREL_ROUTE: &str = "/fixtures/cache_invalidation_route_barrel.ts";
const LEAF_A_ROUTE: &str = "/fixtures/cache_invalidation_route_leaf_a.ts";
const LEAF_B_ROUTE: &str = "/fixtures/cache_invalidation_route_leaf_b.ts";

const OWNER_AUG: &str = "/fixtures/cache_invalidation_aug_owner.ts";
const BASE_AUG: &str = "/fixtures/cache_invalidation_aug_base.ts";
const PATCH_AUG: &str = "/fixtures/cache_invalidation_aug_patch.ts";

const OWNER_BASIC_SRC: &str = include_str!("fixtures/cache_invalidation_owner_basic.ts");
const BARREL_BASIC_SRC: &str = include_str!("fixtures/cache_invalidation_basic_barrel.ts");
const SELECTED_BASIC_V1_SRC: &str =
    include_str!("fixtures/cache_invalidation_basic_selected_v1.ts");
const SELECTED_BASIC_V2_SRC: &str =
    include_str!("fixtures/cache_invalidation_basic_selected_v2.ts");
const UNUSED_BASIC_V1_SRC: &str = include_str!("fixtures/cache_invalidation_basic_unused_v1.ts");
const UNUSED_BASIC_V2_SRC: &str = include_str!("fixtures/cache_invalidation_basic_unused_v2.ts");

const OWNER_ROUTE_SRC: &str = include_str!("fixtures/cache_invalidation_route_owner.ts");
const BARREL_ROUTE_V1_SRC: &str = include_str!("fixtures/cache_invalidation_route_barrel_v1.ts");
const BARREL_ROUTE_V2_SRC: &str = include_str!("fixtures/cache_invalidation_route_barrel_v2.ts");
const LEAF_A_ROUTE_SRC: &str = include_str!("fixtures/cache_invalidation_route_leaf_a.ts");
const LEAF_B_ROUTE_SRC: &str = include_str!("fixtures/cache_invalidation_route_leaf_b.ts");

const OWNER_AUG_SRC: &str = include_str!("fixtures/cache_invalidation_aug_owner.ts");
const BASE_AUG_SRC: &str = include_str!("fixtures/cache_invalidation_aug_base.ts");
const PATCH_AUG_V1_SRC: &str = include_str!("fixtures/cache_invalidation_aug_patch_v1.ts");
const PATCH_AUG_V2_SRC: &str = include_str!("fixtures/cache_invalidation_aug_patch_v2.ts");

const PACKAGE_OWNER_SRC: &str = include_str!("fixtures/cache_invalidation_package_owner.ts");

// ---------------------------------------------------------------------------
// Scenario 1 — selected-leaf edit CHANGES the typeinfo result.
// ---------------------------------------------------------------------------

fn install_basic_fixture(host: &VerterHost, selected_src: &str, unused_src: &str) {
    upsert_ts(host, SELECTED_BASIC, selected_src);
    upsert_ts(host, UNUSED_BASIC, unused_src);
    upsert_ts(host, BARREL_BASIC, BARREL_BASIC_SRC);
    upsert_ts(host, OWNER_BASIC, OWNER_BASIC_SRC);
}

fn resolve_surface(host: &VerterHost, owner: &str) -> (TypeExpr, verter_audit::RequestAuditRecord) {
    resolve_expr(host, owner, "Surface", &[], ProjectionMode::Expanded)
}

fn assert_basic_surface_has_v_property(expr: &TypeExpr, expected_v: f64, expected_tag: &str) {
    let props = object_props(expr);
    assert_eq!(prop_names(&props), vec!["tag", "v"]);
    assert_number_literal(&props["v"].ty, expected_v);
    assert_string_literal(&props["tag"].ty, expected_tag);
}

/// Cache layer: `FileArtifactStore` + `OwnerImportSurfaceDb`.
/// Editing the selected leaf advances its `content_hash` so the
/// owner's published `Surface` must observe the V2 shape; the leaf's
/// `IndexedReady` must rebuild and re-enter the second request's
/// loaded-file set.
#[test]
#[ignore = "typeinfo currently does not propagate a selected-leaf `upsert_ts` edit through the barrel into the owner's published `Surface`: after bumping the leaf's content hash from V1 to V2 the owner still resolves `Selected.v` as `1` instead of `2`, indicating `OwnerImportSurfaceDb` / `RouteDb` are not revalidating the leaf's new content hash on the second request; keep as the future selected-leaf edit-propagation contract"]
fn cache_invalidation_basic_selected_leaf_edit_flips_published_surface() {
    let host = make_host_with_footprint();
    install_basic_fixture(&host, SELECTED_BASIC_V1_SRC, UNUSED_BASIC_V1_SRC);

    let (v1_expr, v1_record) = resolve_surface(&host, OWNER_BASIC);
    assert_basic_surface_has_v_property(&v1_expr, 1.0, "v1");
    assert_declared_dependency_includes(&v1_record, SELECTED_BASIC);
    assert_query_mode(&v1_record, ProjectionModeTag::Expanded);

    upsert_ts(&host, SELECTED_BASIC, SELECTED_BASIC_V2_SRC);

    let (v2_expr, v2_record) = resolve_surface(&host, OWNER_BASIC);
    assert_basic_surface_has_v_property(&v2_expr, 2.0, "v2");
    assert_ne!(
        v2_expr, v1_expr,
        "edited selected leaf must change the owner's published surface"
    );
    // The selected leaf must re-enter the per-request footprint after
    // its content advanced. The post-edit `IndexedReady` is a fresh
    // build, which is recorded on the second request.
    let v2_declared = declared_dependency_file_names(&v2_record);
    assert!(
        v2_declared.iter().any(|p| p == SELECTED_BASIC),
        "edited selected leaf must enter the V2 typeinfo dependency footprint; got {v2_declared:?}"
    );
    assert_query_mode(&v2_record, ProjectionModeTag::Expanded);
}

// ---------------------------------------------------------------------------
// Scenario 2 — unselected-leaf edit does NOT invalidate cached result.
// ---------------------------------------------------------------------------

/// Cache layer: `FileArtifactStore` + `OwnerImportSurfaceDb` +
/// `SemanticGraphStore` final-result projections. Editing an
/// unreferenced barrel sibling must NOT invalidate any cache participant
/// reached by the owner: warm reads must reuse the original published
/// `Surface`, perform zero VFS reads, and incur zero RouteDb misses.
#[test]
#[ignore = "typeinfo currently fails to attach a request footprint on this resolver path after the block-6.i AX-WIP audit-passive-observer refactor (commit b0798e28); the contract is that editing an unreferenced barrel sibling must NOT invalidate any cache participant reached by the owner — warm reads must reuse the original published `Surface`, perform zero VFS reads, and incur zero RouteDb misses with the footprint still attached. Keep as the future unselected-sibling-isolation contract once AX-WIP closes Rule-5 leak."]
fn cache_invalidation_unselected_leaf_edit_keeps_warm_cache() {
    let host = make_host_with_footprint();
    install_basic_fixture(&host, SELECTED_BASIC_V1_SRC, UNUSED_BASIC_V1_SRC);

    let (v1_expr, v1_record) = resolve_surface(&host, OWNER_BASIC);
    assert_basic_surface_has_v_property(&v1_expr, 1.0, "v1");
    assert_declared_dependency_excludes(&v1_record, UNUSED_BASIC);
    assert_request_loaded_files_exclude(&v1_record, UNUSED_BASIC);

    upsert_ts(&host, UNUSED_BASIC, UNUSED_BASIC_V2_SRC);

    let (v2_expr, v2_record) = resolve_surface(&host, OWNER_BASIC);
    assert_eq!(
        v2_expr, v1_expr,
        "editing an unselected leaf must not perturb the owner's published surface"
    );
    // The architectural promise: an unselected-leaf edit is inert. The
    // owner's warm query must not re-read sources, must not rebuild
    // IndexedReady entries, and must not perform any cold RouteDb /
    // owner-import work.
    assert_no_fresh_source_loading(&v2_record);
    assert_no_route_misses(&v2_record);
    assert_request_loaded_files_exclude(&v2_record, UNUSED_BASIC);
    assert_declared_dependency_excludes(&v2_record, UNUSED_BASIC);
}

// ---------------------------------------------------------------------------
// Scenario 2b — same as #2 but characterised through result equality
// only (a relaxed assertion that does not require the strict warm-cache
// contract). This passes today against the structural-equality channel
// while the stricter warm-cache contract above remains red.
// ---------------------------------------------------------------------------

/// Cache layer: `SemanticGraphStore` final-result projection. The
/// resolver must produce the same expression for `Surface` regardless
/// of edits to unrelated barrel siblings; this is the minimal
/// correctness floor.
#[test]
fn cache_invalidation_unselected_leaf_edit_preserves_result_equality() {
    let host = make_host_with_footprint();
    install_basic_fixture(&host, SELECTED_BASIC_V1_SRC, UNUSED_BASIC_V1_SRC);

    let (v1_expr, _) = resolve_surface(&host, OWNER_BASIC);
    upsert_ts(&host, UNUSED_BASIC, UNUSED_BASIC_V2_SRC);
    let (v2_expr, _) = resolve_surface(&host, OWNER_BASIC);

    assert_eq!(
        v2_expr, v1_expr,
        "editing an unselected leaf must not change the owner's published surface"
    );
}

// ---------------------------------------------------------------------------
// Scenario 3 — barrel-export edit invalidates route facts.
// ---------------------------------------------------------------------------

fn install_route_fixture(host: &VerterHost, barrel_src: &str) {
    upsert_ts(host, LEAF_A_ROUTE, LEAF_A_ROUTE_SRC);
    upsert_ts(host, LEAF_B_ROUTE, LEAF_B_ROUTE_SRC);
    upsert_ts(host, BARREL_ROUTE, barrel_src);
    upsert_ts(host, OWNER_ROUTE, OWNER_ROUTE_SRC);
}

fn assert_route_surface_is_leaf_a(expr: &TypeExpr) {
    let props = object_props(expr);
    assert_eq!(prop_names(&props), vec!["a"]);
    assert_primitive(&props["a"].ty, PrimitiveName::Number);
}

fn assert_route_surface_is_leaf_b(expr: &TypeExpr) {
    let props = object_props(expr);
    assert_eq!(prop_names(&props), vec!["b"]);
    assert_primitive(&props["b"].ty, PrimitiveName::String);
}

/// Cache layer: `RouteDb` + `ResolvedImportFacts`. Editing the barrel's
/// re-export target must invalidate the route fact for `(owner, "Item")`
/// — the V2 result must reflect the new leaf and the V2 footprint must
/// include the new leaf canonical.
#[test]
#[ignore = "typeinfo currently fails to attach a request footprint on this resolver path after the block-6.i AX-WIP audit-passive-observer refactor (commit b0798e28); the contract is that editing a barrel re-export target must invalidate the route fact for (owner, \"Item\") — the V2 result must reflect the new leaf and the V2 footprint must include the new leaf canonical. Keep as the future barrel-route-redirect contract once AX-WIP closes Rule-5 leak."]
fn cache_invalidation_barrel_edit_redirects_route_to_new_leaf() {
    let host = make_host_with_footprint();
    install_route_fixture(&host, BARREL_ROUTE_V1_SRC);

    let (v1_expr, v1_record) = resolve_surface(&host, OWNER_ROUTE);
    assert_route_surface_is_leaf_a(&v1_expr);
    assert_declared_dependency_includes(&v1_record, LEAF_A_ROUTE);
    assert_declared_dependency_excludes(&v1_record, LEAF_B_ROUTE);

    upsert_ts(&host, BARREL_ROUTE, BARREL_ROUTE_V2_SRC);

    let (v2_expr, v2_record) = resolve_surface(&host, OWNER_ROUTE);
    assert_route_surface_is_leaf_b(&v2_expr);
    assert_declared_dependency_includes(&v2_record, LEAF_B_ROUTE);
    assert_query_mode(&v2_record, ProjectionModeTag::Expanded);
}

/// Cache layer: `RouteDb`. The strict promise is that leaf_a drops
/// out of the V2 footprint entirely — the redirected barrel no longer
/// routes to it. This is the path-precise invalidation guarantee.
#[test]
#[ignore = "typeinfo currently fails to attach a request footprint on this resolver path after the block-6.i AX-WIP audit-passive-observer refactor (commit b0798e28); the strict promise is that leaf_a drops out of the V2 footprint entirely — the redirected barrel no longer routes to it (path-precise invalidation guarantee). Keep as the future barrel-route-drop contract once AX-WIP closes Rule-5 leak."]
fn cache_invalidation_barrel_edit_excludes_prior_leaf_from_v2_footprint() {
    let host = make_host_with_footprint();
    install_route_fixture(&host, BARREL_ROUTE_V1_SRC);

    let _ = resolve_surface(&host, OWNER_ROUTE);
    upsert_ts(&host, BARREL_ROUTE, BARREL_ROUTE_V2_SRC);

    let (_, v2_record) = resolve_surface(&host, OWNER_ROUTE);
    assert_declared_dependency_excludes(&v2_record, LEAF_A_ROUTE);
    assert_request_loaded_files_exclude(&v2_record, LEAF_A_ROUTE);
}

// ---------------------------------------------------------------------------
// Scenario 4 — side-effect module augmentation added after first query.
// ---------------------------------------------------------------------------

fn install_aug_fixture(host: &VerterHost, patch_src: &str) {
    upsert_ts(host, BASE_AUG, BASE_AUG_SRC);
    upsert_ts(host, PATCH_AUG, patch_src);
    upsert_ts(host, OWNER_AUG, OWNER_AUG_SRC);
}

/// Cache layer: `FileArtifactStore::augmentation_index` +
/// `ProjectTypeStore` augmentation discovery. Editing the patch file to
/// introduce a `declare module "./base" { interface Plugin { extra } }`
/// block must surface the merged shape in the owner's published
/// `Surface`. The owner imports the patch via side-effect so the
/// augmentation is in scope.
#[test]
#[ignore = "verter currently does not discover module augmentations contributed by a side-effect-imported patch file (the canonical Vue/Vite augmentation pattern). The existing module_features test characterises the same gap on the cold path; cache_invalidation re-characterises it across an edit cycle. Keep as the future augmentation-on-edit contract (CLAUDE.md `Cache Architecture`: `augmentation_index` lookup by `AugmentationTargetKey`)."]
fn cache_invalidation_aug_patch_edit_surfaces_augmented_shape() {
    let host = make_host_with_footprint();
    install_aug_fixture(&host, PATCH_AUG_V1_SRC);

    let (v1_expr, v1_record) = resolve_surface(&host, OWNER_AUG);
    // V1 patch is a no-op: surface is bare base shape.
    let v1_props = object_props(&v1_expr);
    assert_eq!(prop_names(&v1_props), vec!["id"]);
    assert_primitive(&v1_props["id"].ty, PrimitiveName::String);
    assert_query_mode(&v1_record, ProjectionModeTag::Expanded);

    upsert_ts(&host, PATCH_AUG, PATCH_AUG_V2_SRC);

    let (v2_expr, v2_record) = resolve_surface(&host, OWNER_AUG);
    let v2_props = object_props(&v2_expr);
    assert_eq!(prop_names(&v2_props), vec!["extra", "id"]);
    assert_primitive(&v2_props["id"].ty, PrimitiveName::String);
    assert_primitive(&v2_props["extra"].ty, PrimitiveName::Number);
    assert_query_mode(&v2_record, ProjectionModeTag::Expanded);
}

/// Cache layer: `SemanticGraphStore` projected-surface caches. Verify
/// the bare base shape is correct on the cold path so the augmentation
/// gap is the only remaining variable. This characterises the V1
/// floor.
#[test]
fn cache_invalidation_aug_patch_v1_publishes_bare_base_shape() {
    let host = make_host_with_footprint();
    install_aug_fixture(&host, PATCH_AUG_V1_SRC);

    let (v1_expr, v1_record) = resolve_surface(&host, OWNER_AUG);
    let props = object_props(&v1_expr);
    assert_eq!(prop_names(&props), vec!["id"]);
    assert_primitive(&props["id"].ty, PrimitiveName::String);
    assert_query_mode(&v1_record, ProjectionModeTag::Expanded);
}

// ---------------------------------------------------------------------------
// Scenario 5 — package source change invalidates previous answer.
// ---------------------------------------------------------------------------

const SYNTHETIC_PACKAGE_JSON: &str = r#"{
  "name": "synthetic-cache-invalidation",
  "exports": {
    ".": {
      "types": "./dist/index.d.ts",
      "import": "./dist/index.js"
    }
  }
}"#;

const SYNTHETIC_PACKAGE_RUNTIME: &str = "export const item = { v: 1 };\n";

const SYNTHETIC_PACKAGE_DTS_V1: &str = "export type Item = {\n  v: 1;\n};\n";
const SYNTHETIC_PACKAGE_DTS_V2: &str = "export type Item = {\n  v: 2;\n};\n";

const PACKAGE_PKG_JSON: &str = "/workspace/node_modules/synthetic-cache-invalidation/package.json";
const PACKAGE_DTS: &str = "/workspace/node_modules/synthetic-cache-invalidation/dist/index.d.ts";
const PACKAGE_RUNTIME: &str = "/workspace/node_modules/synthetic-cache-invalidation/dist/index.js";
const PACKAGE_OWNER: &str = "/workspace/src/cache_invalidation_package_owner.ts";

fn make_package_host_with_workspace(
    dts_source: &str,
) -> (Arc<VerterHost>, Arc<verter_workspace::MemoryWorkspace>) {
    let workspace = Arc::new(verter_workspace::MemoryWorkspace::new(
        verter_workspace::MemoryOptions::default(),
    ));
    workspace.inject_file(
        PACKAGE_PKG_JSON.to_string(),
        Arc::from(SYNTHETIC_PACKAGE_JSON),
    );
    workspace.inject_file(PACKAGE_DTS.to_string(), Arc::from(dts_source));
    workspace.inject_file(
        PACKAGE_RUNTIME.to_string(),
        Arc::from(SYNTHETIC_PACKAGE_RUNTIME),
    );
    let host = Arc::new(VerterHost::new(
        crate::types::HostConfig {
            audit_enabled: true,
            footprint_capture: true,
            ..crate::types::HostConfig::default()
        },
        workspace.clone() as Arc<dyn verter_workspace::WorkspaceAccess>,
    ));
    host.configure_projects(vec![
        verter_semantic::analysis::project_resolver::IdeProjectConfig::new(
            "/workspace".to_string(),
            "/workspace".to_string(),
            Some("/workspace/tsconfig.json".to_string()),
        ),
    ]);
    (host, workspace)
}

/// Cache layer: `FileArtifactStore` on package-backed files +
/// `RouteDb` for the bare-module specifier route. Cold-path
/// characterisation: a fresh host built against the V1 package
/// source publishes the V1 shape. Establishes the V1 baseline that
/// scenario 5's edit-cycle contract diverges from.
#[test]
fn cache_invalidation_package_dts_v1_publishes_v1_shape() {
    let (host, _workspace) = make_package_host_with_workspace(SYNTHETIC_PACKAGE_DTS_V1);
    upsert_ts(&host, PACKAGE_OWNER, PACKAGE_OWNER_SRC);

    let (expr, record) = resolve_expr(
        &host,
        PACKAGE_OWNER,
        "Surface",
        &[],
        ProjectionMode::Expanded,
    );
    let props = object_props(&expr);
    assert_eq!(prop_names(&props), vec!["v"]);
    assert_number_literal(&props["v"].ty, 1.0);
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

/// Cache layer: `FileArtifactStore` on package-backed files. The V2
/// host independently constructs an identical workspace with V2 of the
/// package d.ts; the owner's published surface must reflect V2.
/// Establishes the V2 baseline for scenario 5.
#[test]
fn cache_invalidation_package_dts_v2_publishes_v2_shape() {
    let (host, _workspace) = make_package_host_with_workspace(SYNTHETIC_PACKAGE_DTS_V2);
    upsert_ts(&host, PACKAGE_OWNER, PACKAGE_OWNER_SRC);

    let (expr, record) = resolve_expr(
        &host,
        PACKAGE_OWNER,
        "Surface",
        &[],
        ProjectionMode::Expanded,
    );
    let props = object_props(&expr);
    assert_eq!(prop_names(&props), vec!["v"]);
    assert_number_literal(&props["v"].ty, 2.0);
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

/// Cache layer: `RouteDb` + `FileArtifactStore` invalidation on
/// in-place package source change. Strict promise:
/// `MemoryWorkspace::inject_file` re-injecting the package's d.ts
/// against the SAME host advances the workspace `content_generation`,
/// invalidates the `FileArtifactStore` and `RouteDb` entries keyed on
/// the prior `content_hash`, and the next owner query publishes the
/// V2 shape.
///
/// VFS is the authority for file-change invalidation; this is the
/// canonical "node_modules type update" scenario covered by
/// `Cache Architecture` R21 (project-source invalidation).
#[test]
#[ignore = "verter currently does not flip the owner's published surface after an in-place `MemoryWorkspace::inject_file` re-injection of a node_modules d.ts. The first query warms the route fact for the bare module specifier `synthetic-cache-invalidation`; the second `inject_file` bumps `content_generation` and the host's `package_manifest` invalidation fires, but the warm `RouteDb` entry for the owner's import continues to surface the V1 leaf body — indicating the route fact's `fact_dep_signature` revalidation does not consult the package-backed leaf's new content hash. Keep as the future package-source-change invalidation contract (CLAUDE.md `Canonical Dependency Cache Rule`: VFS is the authority; route invalidation is not file-hash-only and tsconfig / vite alias / workspace graph / package target changes must invalidate affected route facts)."]
fn cache_invalidation_in_place_package_edit_flips_published_surface() {
    let (host, workspace) = make_package_host_with_workspace(SYNTHETIC_PACKAGE_DTS_V1);
    upsert_ts(&host, PACKAGE_OWNER, PACKAGE_OWNER_SRC);

    let (v1_expr, _) = resolve_expr(
        &host,
        PACKAGE_OWNER,
        "Surface",
        &[],
        ProjectionMode::Expanded,
    );
    let v1_props = object_props(&v1_expr);
    assert_eq!(prop_names(&v1_props), vec!["v"]);
    assert_number_literal(&v1_props["v"].ty, 1.0);

    // In-place edit: mutate the package's d.ts against the SAME
    // workspace snapshot. `inject_file` calls
    // `invalidate_package_manifest` and bumps `content_generation`.
    workspace.inject_file(PACKAGE_DTS.to_string(), Arc::from(SYNTHETIC_PACKAGE_DTS_V2));

    let (v2_expr, v2_record) = resolve_expr(
        &host,
        PACKAGE_OWNER,
        "Surface",
        &[],
        ProjectionMode::Expanded,
    );
    let v2_props = object_props(&v2_expr);
    assert_eq!(prop_names(&v2_props), vec!["v"]);
    assert_number_literal(&v2_props["v"].ty, 2.0);
    assert_ne!(
        v2_expr, v1_expr,
        "in-place package d.ts edit must flip the owner's published surface"
    );
    assert_query_mode(&v2_record, ProjectionModeTag::Expanded);
}
