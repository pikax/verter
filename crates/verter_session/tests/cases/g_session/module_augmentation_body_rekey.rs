//! End-to-end regression: the `MergedDecl` augmentation BODY stitch must
//! self-heal a stale augmenter `artifact_key` after a same-canonical
//! re-key whose decl skeleton (hence `parse_stable_hash`, hence the
//! augmenter-set fingerprint) is unchanged.
//!
//! `RouteDb::get_or_compute_effective_export_set` (the NAMES stitch)
//! already self-heals a stale captured key — proven by
//! `rekeyed_augmenter_with_unchanged_skeleton_is_not_dropped` in
//! `g_misc3/module_augmentation_stitching.rs`. But the LIVE `MergedDecl`
//! body stitch (`ProjectSemanticDispatch::collect_augmentation_contributions`)
//! re-fetched the augmenter's retained inner body via
//! `artifact_store.get_artifacts(&augmenter.artifact_key)` and silently
//! `continue`d on a miss — so a real augmentation could disappear after a
//! cosmetic re-key.
//!
//! Shape: `defineProps<Cfg>()` flattens `Cfg`'s members into props. A base
//! augmenter (`aug.ts`, `declare module './types' { interface Cfg { fromAug } }`)
//! contributes the prop `fromAug`. A cosmetic edit to `aug.ts` (a leading
//! comment) changes its content hash — so the host re-keys it and drains the
//! pre-edit `FileArtifactKey` — while leaving the decl skeleton (and thus the
//! augmenter-set fingerprint) untouched, so the cached `AugmenterSet` is NOT
//! invalidated and keeps the stale `AugmenterEntry.artifact_key`.
//!
//! - **Pre-fix tree**: the body stitch's exact-key `get_artifacts` misses and
//!   `continue`s — `fromAug` silently disappears from the recomputed props.
//!   The discriminating assertion FAILS.
//! - **Post-fix tree**: the body stitch self-heals the stale key via the
//!   scheduler-authoritative current content hash (`indexed.whole_hash`,
//!   already materialised by the `ensure_indexed_ready_serve` flight), so
//!   `fromAug` survives.
//!   PASSES.

use std::sync::atomic::Ordering::Relaxed;
use std::sync::Arc;

use verter_session::meta::MetaProject;
use verter_session::{CompileErrorPolicy, FileLanguage, HostConfig, UpsertRequest, VerterHost};
use verter_workspace::{MemoryOptions, MemoryWorkspace, WorkspaceAccess};

fn workspace_project(files: &[(&str, &str)]) -> (Arc<MetaProject>, Arc<MemoryWorkspace>) {
    let workspace = Arc::new(MemoryWorkspace::new(MemoryOptions::default()));
    for (canonical, content) in files {
        workspace.inject_file((*canonical).into(), Arc::from(*content));
    }
    let ws_access: Arc<dyn WorkspaceAccess> = workspace.clone();
    let host = VerterHost::new(
        HostConfig {
            dev_mode: false,
            compile_error_policy: CompileErrorPolicy::StrictError,
            analysis_level: verter_session::AnalysisLevel::Full,
            ..HostConfig::default()
        },
        ws_access,
    );
    (MetaProject::new(host), workspace)
}

fn prop_names(
    meta: &verter_semantic::analysis::component_meta::ComponentMetaAnalysis,
) -> Vec<String> {
    let mut names: Vec<String> = meta.props.iter().map(|p| p.name.clone()).collect();
    names.sort();
    names
}

/// Debug-rendered resolved type of the prop named `name`,
/// demand-materialized from its published source through the ONE
/// shared dispatch.
fn prop_type_repr(
    host: &VerterHost,
    owner: &str,
    meta: &verter_semantic::analysis::component_meta::ComponentMetaAnalysis,
    name: &str,
) -> Option<String> {
    meta.props.iter().find(|p| p.name == name).map(|p| {
        let source = p
            .type_source
            .present()
            .unwrap_or_else(|| panic!("prop `{name}` must publish a typed source"));
        let ty =
            verter_session::test_only::semantic_source_probe::demand_type_expr(host, owner, source)
                .unwrap_or_else(|| {
                    panic!("prop `{name}`'s published source must demand-materialize")
                });
        format!("{ty:?}")
    })
}

/// The `artifact_key.content_hash` the cached augmentation index holds for
/// `augmenter_canonical`, scanned across every populated `AugmenterSet` (no
/// need to reconstruct the exact target key). `None` if no populated set lists
/// the augmenter. Lets the test observe whether the body stitch wrote the
/// healed (post-rekey) exact key back into the cached set.
fn cached_augmenter_content_hash(host: &VerterHost, augmenter_canonical: &str) -> Option<[u8; 16]> {
    let store = host.project_type_store().indexed();
    for (key, _fingerprint) in store.snapshot_augmentation_index_fingerprints() {
        if let Some(set) = store.get_augmenter_set(&key) {
            for entry in set.entries.iter() {
                if entry.canonical().as_ref() == augmenter_canonical {
                    return Some(entry.artifact_key.content_hash);
                }
            }
        }
    }
    None
}

/// The `ModuleAugmentationIndexShape` fingerprint of the (single)
/// augmenter set that lists `augmenter_canonical`. This fingerprint folds
/// each augmenter's `parse_stable_hash` (the decl skeleton) — it is
/// INVARIANT under a member's VALUE-type edit. `None` if no populated set
/// lists the augmenter. Lets the test prove the header-level fingerprint
/// rail cannot see a body-only member-type edit (so the per-augmenter
/// `FileWholeHash` self-root is the only rail that can).
fn cached_augmenter_index_shape_fp(
    host: &VerterHost,
    augmenter_canonical: &str,
) -> Option<[u8; 16]> {
    let store = host.project_type_store().indexed();
    for (key, fingerprint) in store.snapshot_augmentation_index_fingerprints() {
        if let Some(set) = store.get_augmenter_set(&key) {
            if set
                .entries
                .iter()
                .any(|entry| entry.canonical().as_ref() == augmenter_canonical)
            {
                return Some(fingerprint);
            }
        }
    }
    None
}

const AUG_PRE: &str = "import './types'\n\
     declare module './types' {\n\
     \x20 interface Cfg { fromAug: string }\n\
     }\n\
     export {}\n";

// Cosmetic edit: a leading line comment changes the byte content (hence the
// content hash → a host re-key that drains the pre-edit FileArtifactKey) while
// leaving the `declare module` / `interface Cfg { fromAug }` decl skeleton —
// and therefore `parse_stable_hash` and the augmenter-set fingerprint —
// unchanged.
const AUG_POST_COSMETIC: &str = "// cosmetic edit, decl skeleton unchanged\n\
     import './types'\n\
     declare module './types' {\n\
     \x20 interface Cfg { fromAug: string }\n\
     }\n\
     export {}\n";

#[test]
fn merged_decl_body_stitch_self_heals_rekeyed_augmenter() {
    let (project, workspace) = workspace_project(&[
        (
            "/workspace/src/types.ts",
            "export interface Cfg { base: string }\n",
        ),
        ("/workspace/src/aug.ts", AUG_PRE),
        (
            "/workspace/src/Comp.vue",
            "<script setup lang=\"ts\">\n\
             import type { Cfg } from '/workspace/src/types'\n\
             import '/workspace/src/aug'\n\
             defineProps<Cfg>()\n\
             </script>\n\
             <template><div/></template>\n",
        ),
    ]);

    // Cold pass — populates the augmentation index with `aug.ts`'s pre-edit
    // exact `FileArtifactKey`, and surfaces the augmenter member as a prop.
    let pre = prop_names(
        &project
            .host()
            .get_component_meta("/workspace/src/Comp.vue")
            .expect("cold component-meta returns Some"),
    );
    assert!(
        pre.contains(&"base".to_string()),
        "control: own `base` prop present pre-edit: {pre:?}"
    );
    assert!(
        pre.contains(&"fromAug".to_string()),
        "control: base augmenter member `fromAug` present pre-edit: {pre:?}"
    );

    // Capture the augmenter's cached exact-key content hash BEFORE the edit.
    // The cold body stitch populated the augmentation index with `aug.ts`'s
    // pre-edit `FileArtifactKey`, so this is the PRE-edit content hash.
    let pre_key_hash = cached_augmenter_content_hash(project.host(), "/workspace/src/aug.ts")
        .expect("augmenter must be in the augmentation index after the cold pass");

    // Cosmetic re-key of the augmenter: re-inject the new bytes into the
    // workspace and force the host to re-read via `upsert`. The content hash
    // advances (the pre-edit `FileArtifactKey` is drained), but the decl
    // skeleton — hence the augmenter-set fingerprint — does not, so the cached
    // `AugmenterSet` keeps its now-stale `artifact_key`.
    workspace.inject_file("/workspace/src/aug.ts".into(), Arc::from(AUG_POST_COSMETIC));
    let _ = project.host().upsert(UpsertRequest {
        canonical_id: Some("/workspace/src/aug.ts".into()),
        input_id: "/workspace/src/aug.ts".into(),
        source: Arc::from(AUG_POST_COSMETIC),
        file_language: FileLanguage::script_ts(),
        aliases: vec![],
    });
    // Force the owner to recompute its meta so the body stitch re-runs against
    // the cached (stale-key) augmenter set.
    project.host().evict("/workspace/src/Comp.vue");

    // Warm pass after the re-key. Pre-fix the stale exact-key miss is a silent
    // `continue`, dropping `fromAug`; post-fix the body stitch self-heals.
    let post = prop_names(
        &project
            .host()
            .get_component_meta("/workspace/src/Comp.vue")
            .expect("post-rekey component-meta returns Some"),
    );
    assert!(
        post.contains(&"base".to_string()),
        "own `base` prop must survive the augmenter re-key: {post:?}"
    );
    assert!(
        post.contains(&"fromAug".to_string()),
        "DISCRIMINATING: the re-keyed augmenter's member `fromAug` MUST still \
         surface — the `MergedDecl` body stitch must self-heal the stale \
         captured `artifact_key` via the scheduler-authoritative current \
         content hash, NOT silently drop the augmenter: {post:?}"
    );

    // The re-key ACTUALLY occurred AND the healed key was written back: the
    // cached augmentation-index entry for `aug.ts` now carries a DIFFERENT
    // content hash than the pre-edit one. This proves (a) the cosmetic edit
    // genuinely drained the pre-edit `FileArtifactKey` (so the heal path was
    // exercised, not bypassed), and (b) the body stitch persisted the healed
    // exact key back into the cached `AugmenterSet` instead of re-healing on
    // every call. Without the write-back the cached entry would still hold the
    // stale pre-edit hash and this assertion would FAIL.
    let post_key_hash = cached_augmenter_content_hash(project.host(), "/workspace/src/aug.ts")
        .expect("augmenter must still be in the augmentation index after the warm pass");
    assert_ne!(
        pre_key_hash, post_key_hash,
        "the cosmetic re-key must advance the augmenter's content hash AND the \
         body stitch must write the healed exact key back into the cached \
         AugmenterSet (pre={pre_key_hash:?} post={post_key_hash:?})"
    );
}

/// Body-ONLY edit of an augmenter member: the member's TYPE changes
/// (`fromAug: string` → `fromAug: number`) while its name, kind, and the
/// member COUNT stay put — so the augmenter's decl skeleton (hence
/// `parse_stable_hash` and the augmenter-set fingerprint) is UNCHANGED.
///
/// The ONLY rail that can invalidate a WARM consumer here is the
/// per-augmenter `FileWholeHash` self-root recorded in the cold stitch's
/// read-set. This test characterises that rail HONESTLY — it does NOT
/// evict the owner before the post-edit read, so the post-edit
/// `get_component_meta` exercises the genuine WARM path:
///
/// 1. A second pre-edit `get_component_meta` is a warm HIT
///    (`component_meta_result_cache_hits` advances) — proving a warm
///    `ComponentMetaResultDb` entry exists to be invalidated.
/// 2. The augmenter-set `ModuleAugmentationIndexShape` fingerprint is
///    UNCHANGED across the member-VALUE-type edit (`fp_before == fp_after`)
///    — proving the header-level fingerprint rail CANNOT see the edit, so
///    the per-augmenter `FileWholeHash` self-root is the only rail that can.
/// 3. The post-edit `get_component_meta` is a genuine warm MISS + recompute
///    (`component_meta_result_cache_misses` advances), and — the end-to-end
///    correctness signal — that recompute observes the NEW member type
///    (`number`, not a stale lower-layer `string`).
///
/// It would FAIL — the recompute would pull the stale `MergedDecl` and
/// serve `string` — if that per-augmenter `FileWholeHash` self-root were
/// ever dropped from the semantic stitch's read-set. Mirrors the
/// compile-tier sibling
/// `compile_slot_invalidates_on_external_augmenter_member_type_edit`
/// (`fp_before == fp_after` proof) and the component-meta sibling
/// `imported_prop_type_edit_misses_warm_component_meta` (warm hit/miss
/// counter deltas, no eviction).
#[test]
fn augmenter_member_type_only_edit_invalidates_warm_consumer() {
    const AUG_TYPE_PRE: &str = "import './types'\n\
         declare module './types' {\n\
         \x20 interface Cfg { fromAug: string }\n\
         }\n\
         export {}\n";
    // Same member name `fromAug`, same kind (property), same member count —
    // ONLY the annotated type changes. `parse_stable_hash` is invariant
    // under member-type edits, so the augmenter-set fingerprint does not
    // move; only the per-augmenter `FileWholeHash` differs.
    const AUG_TYPE_POST: &str = "import './types'\n\
         declare module './types' {\n\
         \x20 interface Cfg { fromAug: number }\n\
         }\n\
         export {}\n";

    let (project, workspace) = workspace_project(&[
        (
            "/workspace/src/types.ts",
            "export interface Cfg { base: string }\n",
        ),
        ("/workspace/src/aug.ts", AUG_TYPE_PRE),
        (
            "/workspace/src/Comp.vue",
            "<script setup lang=\"ts\">\n\
             import type { Cfg } from '/workspace/src/types'\n\
             import '/workspace/src/aug'\n\
             defineProps<Cfg>()\n\
             </script>\n\
             <template><div/></template>\n",
        ),
    ]);

    // Cold pass: `fromAug` surfaces with its PRE type (`string`).
    let pre_meta = project
        .host()
        .get_component_meta("/workspace/src/Comp.vue")
        .expect("cold component-meta returns Some");
    let pre_type = prop_type_repr(
        project.host(),
        "/workspace/src/Comp.vue",
        &pre_meta,
        "fromAug",
    )
    .expect("augmenter member `fromAug` must surface as a prop pre-edit");
    assert!(
        pre_type.contains("String"),
        "control: pre-edit augmenter member type is string: {pre_type}"
    );

    // Warm sanity — an unedited second query must round-trip a warm HIT, so a
    // warm `ComponentMetaResultDb` entry exists to be invalidated and the
    // post-edit miss-delta is a discriminating signal. (No eviction: the
    // post-edit read MUST exercise the genuine warm path.)
    let prov = project.host().provenance();
    let hits_before = prov.component_meta_result_cache_hits.load(Relaxed);
    let _ = project
        .host()
        .get_component_meta("/workspace/src/Comp.vue")
        .expect("warm pre-edit component-meta returns Some");
    assert!(
        prov.component_meta_result_cache_hits.load(Relaxed) > hits_before,
        "warm sanity: an unedited second get_component_meta must hit the warm \
         ComponentMetaResultDb — without a round-tripping warm hit the \
         post-edit miss-delta is not discriminating"
    );

    // The augmenter-set (decl-skeleton) fingerprint BEFORE the edit. It folds
    // each augmenter's `parse_stable_hash`, invariant under a member
    // VALUE-type edit.
    let fp_before = cached_augmenter_index_shape_fp(project.host(), "/workspace/src/aug.ts")
        .expect("augmenter must be in the augmentation index after the cold + warm passes");
    let misses_before = prov.component_meta_result_cache_misses.load(Relaxed);

    // Body-only edit of the augmenter: member type string → number, decl
    // skeleton unchanged. The owner-upsert path has NO eager reverse-dependent
    // cascade, and we deliberately do NOT evict the owner — so its warm
    // `ComponentMetaResultDb` entry survives and fact-validation (the
    // per-augmenter `FileWholeHash` self-root) is the SOLE invalidation rail.
    workspace.inject_file("/workspace/src/aug.ts".into(), Arc::from(AUG_TYPE_POST));
    let _ = project.host().upsert(UpsertRequest {
        canonical_id: Some("/workspace/src/aug.ts".into()),
        input_id: "/workspace/src/aug.ts".into(),
        source: Arc::from(AUG_TYPE_POST),
        file_language: FileLanguage::script_ts(),
        aliases: vec![],
    });

    // Warm pass after the body-only edit — NO eviction.
    let post_meta = project
        .host()
        .get_component_meta("/workspace/src/Comp.vue")
        .expect("post-edit component-meta returns Some");
    let misses_after = prov.component_meta_result_cache_misses.load(Relaxed);

    // Warm-path sanity — top-level warm-miss: the augmenter edit MUST
    // invalidate the owner's top-level warm `ComponentMetaResultDb` entry, so
    // the post-edit read is a genuine MISS + recompute, NOT a pure stale
    // top-level hit. (This proves the warm path was exercised; it does NOT by
    // itself prove the recompute is CORRECT — a top-level miss can still pull a
    // stale lower-layer `MergedDecl`, which is exactly what the type assertion
    // below catches.)
    assert!(
        misses_after > misses_before,
        "warm-path sanity: a body-only augmenter member-type edit MUST \
         invalidate the owner's top-level warm ComponentMetaResultDb entry — \
         the post-edit read must be a genuine MISS + recompute, not a pure \
         stale top-level hit (misses {misses_before} -> {misses_after})"
    );

    // PRIMARY DISCRIMINATING signal — end-to-end correctness: the recompute
    // MUST observe the NEW member type (`number`), and the stale `string` must
    // NOT leak through. The per-augmenter `FileWholeHash` self-root lives on
    // the SEMANTIC `MergedDecl` memo entry; dropping it lets that memo validate
    // falsely so the recompute returns the stale `string` — this assertion is
    // the one that fails when the self-root is removed.
    let post_type = prop_type_repr(
        project.host(),
        "/workspace/src/Comp.vue",
        &post_meta,
        "fromAug",
    )
    .expect("augmenter member `fromAug` must still surface post-edit");
    assert!(
        post_type.contains("Number"),
        "DISCRIMINATING: a body-only augmenter member-type edit \
         (string → number, decl skeleton unchanged) MUST invalidate the warm \
         consumer via the per-augmenter FileWholeHash self-root — the stale \
         `string` type must NOT survive: got {post_type}"
    );
    assert!(
        !post_type.contains("String"),
        "the stale pre-edit `string` type must not leak into the recomputed \
         prop: {post_type}"
    );

    // SUPPORTING isolation proof: the augmenter-set `ModuleAugmentationIndexShape`
    // fingerprint is UNCHANGED across the member-VALUE-type edit
    // (`parse_stable_hash` is invariant). The recompute the warm-miss above
    // forced re-populated this index entry, so it reads back current. Proving
    // `fp_before == fp_after` rules out the alternative explanation that the
    // header-level fingerprint rail (not the `FileWholeHash`) caught the edit.
    let fp_after = cached_augmenter_index_shape_fp(project.host(), "/workspace/src/aug.ts")
        .expect("augmenter must still be in the augmentation index after the warm recompute");
    assert_eq!(
        fp_before, fp_after,
        "the augmenter-set ModuleAugmentationIndexShape fingerprint MUST stay \
         equal across a member-VALUE-type edit (parse_stable_hash is invariant) \
         — proving the per-augmenter FileWholeHash self-root is the only rail \
         that can catch this edit (pre={fp_before:?} post={fp_after:?})"
    );
}
