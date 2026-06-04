//! End-to-end session-overlay augmentation isolation oracle (SCOPE-LOCK 15e).
//!
//! B2 proved overlay isolation at the INDEX level
//! (`session_overlay_augmenter_isolated_from_base_index` in
//! `g_misc3/module_augmentation_stitching.rs`). This is the END-TO-END oracle
//! through the PUBLIC consumer (`MetaProject` / `MetaSession::get_component_meta`,
//! whose session-scoped resolution runs under the `HostStoreView::from_session_id`
//! overlay-aware view): a session-overlay `declare module` augmenter must surface
//! its member under the SESSION view and NOT under the BASE view — and a base
//! re-query after the session must stay un-poisoned.
//!
//! Shape: `defineProps<Cfg>()` flattens `Cfg`'s members into props, so an
//! augmented member of `Cfg` becomes a NEW PROP. A base augmenter file
//! (`aug.ts`, `declare module './types' { interface Cfg { fromBase } }`)
//! contributes `fromBase`; a session OVERLAYS that same augmenter file to instead
//! contribute `sessionOnly`. The base meta surfaces prop `fromBase` (never
//! `sessionOnly`); the session meta surfaces prop `sessionOnly` (never
//! `fromBase`); the post-session base re-query is unchanged.
//!
//! Against a non-overlay-aware index (B2 pre-change: base-only `is_legacy()`
//! scan, no `population` dimension) the session scan never sees the overlay
//! augmenter version, so the session meta would carry `fromBase` (the base
//! augmenter) and NOT `sessionOnly` — the discriminating session assertion
//! FAILS. Post-change it PASSES.

use std::sync::Arc;

use verter_session::meta::MetaProject;
use verter_session::{CompileErrorPolicy, HostConfig, VerterHost};
use verter_workspace::{MemoryOptions, MemoryWorkspace, WorkspaceAccess};

/// Build a workspace-backed `MetaProject` rooted at `/workspace`. `files` are
/// injected into the in-memory workspace so absolute specifiers resolve.
fn workspace_project(files: &[(&str, &str)]) -> Arc<MetaProject> {
    let workspace = Arc::new(MemoryWorkspace::new(MemoryOptions::default()));
    for (canonical, content) in files {
        workspace.inject_file((*canonical).into(), Arc::from(*content));
    }
    let ws_access: Arc<dyn WorkspaceAccess> = workspace;
    let host = VerterHost::new(
        HostConfig {
            dev_mode: false,
            compile_error_policy: CompileErrorPolicy::StrictError,
            analysis_level: verter_session::AnalysisLevel::Full,
            ..HostConfig::default()
        },
        ws_access,
    );
    MetaProject::new(host)
}

/// Sorted prop names from a component-meta result.
fn prop_names(
    meta: &verter_semantic::analysis::component_meta::ComponentMetaAnalysis,
) -> Vec<String> {
    let mut names: Vec<String> = meta.props.iter().map(|p| p.name.clone()).collect();
    names.sort();
    names
}

#[test]
fn session_overlay_augmentation_isolated_from_base_meta() {
    let project = workspace_project(&[
        (
            "/workspace/src/types.ts",
            "export interface Cfg { base: string }\n",
        ),
        (
            // Base augmenter: establishes the reverse-dep edge to `./types`
            // AND contributes the BASE-ONLY member `fromBase`.
            "/workspace/src/aug.ts",
            "import './types'\n\
             declare module './types' {\n\
             \x20 interface Cfg { fromBase: string }\n\
             }\n\
             export {}\n",
        ),
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

    // BASE: the base augmenter contributes `fromBase`; `sessionOnly` is absent.
    let base_meta = project
        .host()
        .get_component_meta("/workspace/src/Comp.vue")
        .expect("base host component-meta returns Some");
    let base = prop_names(&base_meta);
    assert!(
        base.contains(&"base".to_string()),
        "control: own `base` prop present under base view: {base:?}"
    );
    assert!(
        base.contains(&"fromBase".to_string()),
        "control: base augmenter member `fromBase` present under base view: {base:?}"
    );
    assert!(
        !base.contains(&"sessionOnly".to_string()),
        "base view MUST NOT see the session-only augmenter member: {base:?}"
    );

    // SESSION: overlay the SAME augmenter file to contribute `sessionOnly`
    // instead of `fromBase`. The owner `Comp.vue` and the base `types.ts` are
    // untouched.
    let session = project.open_session().expect("open session");
    session
        .upsert(
            "/workspace/src/aug.ts",
            "import './types'\n\
             declare module './types' {\n\
             \x20 interface Cfg { sessionOnly: string }\n\
             }\n\
             export {}\n"
                .into(),
        )
        .expect("session overlay augmenter upsert");

    let session_meta = session
        .get_component_meta("/workspace/src/Comp.vue")
        .expect("session query returns Ok")
        .expect("session has overlay-derived meta for Comp.vue");
    let sess = prop_names(&session_meta);
    assert!(
        sess.contains(&"base".to_string()),
        "own `base` prop present under session view: {sess:?}"
    );
    assert!(
        sess.contains(&"sessionOnly".to_string()),
        "DISCRIMINATING: the session view surfaces the overlay augmenter member \
         `sessionOnly` (overlay-aware index, Session population): {sess:?}"
    );
    assert!(
        !sess.contains(&"fromBase".to_string()),
        "the session overlay REPLACES the augmenter file content, so the base \
         augmenter member `fromBase` is gone under the session view: {sess:?}"
    );

    // BASE re-query AFTER the session: the base view stays isolated — the
    // session overlay never poisoned the base augmentation index / meta cache.
    let base_after = prop_names(
        &project
            .host()
            .get_component_meta("/workspace/src/Comp.vue")
            .expect("base host component-meta after session returns Some"),
    );
    assert!(
        !base_after.contains(&"sessionOnly".to_string()),
        "base cache MUST NOT be poisoned by the session overlay augmenter: {base_after:?}"
    );
    assert!(
        base_after.contains(&"fromBase".to_string()),
        "base view stable after the session closed over it: {base_after:?}"
    );
}
