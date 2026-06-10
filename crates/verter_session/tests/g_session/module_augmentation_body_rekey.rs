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
//!   already materialised by `ensure_indexed_ready`), so `fromAug` survives.
//!   PASSES.

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
