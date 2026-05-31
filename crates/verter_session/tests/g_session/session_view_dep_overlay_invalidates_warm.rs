//! Discriminating test — the view-aware warm-hit path must validate
//! cache facts against the SESSION-OVERLAYED store view, not the base
//! host's store view.
//!
//! Codex P1 (Block 6.B fix round 1): when a session overlays (or
//! tombstones) a DEPENDENCY of an owner SFC while the owner's own
//! whole-hash is UNCHANGED, the view-aware warm-hit path in
//! [`crate::host_manage::component_meta_entry::try_component_meta_cache_hit_with_view`]
//! must NOT return the base-host's component-meta result. The cached
//! candidate's `read_set_signature.facts` pin every cross-file dep
//! (parse / resolve-imports / route-surface facts) to the BASE
//! content. The validator must observe the session overlay so an
//! overlay-shifted dep is rejected by `validates_fact_signature`.
//!
//! Pre-fix shape: `try_component_meta_cache_hit_with_view` calls
//! `self.resolver_store_view()` — the BASE `HostStoreView`, which
//! reflects only base content. The dep's base parse fact validates,
//! and the warm hit returns the STALE base analysis (the overlay's
//! prop type never reaches the consumer).
//!
//! Post-fix shape: the validator threads the session view via
//! `HostStoreView::with_session_overlay`. The overlay-rooted dep's
//! parse fact misses the session-overlayed view's re-rooted
//! per-canonical snapshots, the warm hit returns `None`, and the cold
//! recompute under the session yields the OVERLAY analysis.
//!
//! This test fails at HEAD `863a4a25c` (returns the stale base prop
//! type) and passes after the minimal restoration is in place.

use std::sync::Arc;

use verter_session::meta::MetaProject;
use verter_session::{CompileErrorPolicy, HostConfig, VerterHost};
use verter_type_expr::{PrimitiveName, TypeExpr};
use verter_workspace::{MemoryOptions, MemoryWorkspace, WorkspaceAccess};

/// Build a workspace-backed host rooted at `/workspace`. `files` are
/// injected into the in-memory workspace overlay so absolute
/// specifiers (`/workspace/src/...`) resolve.
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

/// Resolve the named prop's `TypeExpr` from a component-meta result.
fn prop_type<'a>(
    meta: &'a verter_semantic::analysis::component_meta::ComponentMetaAnalysis,
    name: &str,
) -> &'a TypeExpr {
    &meta
        .props
        .iter()
        .find(|p| p.name == name)
        .unwrap_or_else(|| panic!("missing prop `{name}`"))
        .type_expr
}

/// Owner SFC imports `Props` from a sibling `.ts` file. The session
/// overlays the dependency to change the prop type; the owner itself
/// is untouched, so its whole-hash is identical between base and
/// session.
///
/// Discrimination: the warm-hit path's fact validator must reject the
/// base candidate because the dep's overlay parse fact no longer
/// matches base content. Pre-fix the base view is consulted and the
/// stale `msg: string` survives; post-fix the session-overlayed view
/// rejects the candidate, the cold recompute under the session
/// observes the overlay, and the returned prop type is `number`.
#[test]
fn view_aware_warm_hit_rejects_base_when_session_overlays_a_dependency() {
    let project = workspace_project(&[
        (
            "/workspace/src/types.ts",
            "export interface Props { msg: string }\n",
        ),
        (
            "/workspace/src/Comp.vue",
            "<script setup lang=\"ts\">\n\
             import type { Props } from '/workspace/src/types'\n\
             defineProps<Props>()\n\
             </script>\n\
             <template><div/></template>\n",
        ),
    ]);

    // 1. Warm the base host's component-meta cache for `/Comp.vue`.
    //    This populates a `ComponentMetaResultDb` candidate whose
    //    `read_set_signature.facts` pin the dep `/types.ts` to its
    //    BASE content.
    let base_meta = project
        .host()
        .get_component_meta("/workspace/src/Comp.vue")
        .expect("base host warm path returns Some");
    assert_eq!(
        prop_type(&base_meta, "msg"),
        &TypeExpr::Primitive(PrimitiveName::String),
        "control: the base host's `msg` prop must resolve to `string` \
         (base `/types.ts` declares `msg: string`)",
    );

    // 2. Open a session and overlay the DEPENDENCY only. The owner
    //    `/Comp.vue` is NOT re-upserted, so its base content (and
    //    therefore its base whole-hash) is unchanged. The base
    //    `ComponentMetaResultDb` candidate is the only one keyed on
    //    the owner's content hash.
    let session = project.open_session().expect("open session");
    session
        .upsert(
            "/workspace/src/types.ts",
            "export interface Props { msg: number }\n".into(),
        )
        .expect("session dep overlay");

    // 3. Query through the session. The view-aware warm-hit path
    //    derives the cache key from `view.content_hash_for(owner)`
    //    (which falls back to the base whole-hash — owner unchanged),
    //    finds the base candidate, and validates its fact signature.
    //
    //    Pre-fix: validation routes through `self.resolver_store_view()`
    //    — the BASE view — which still reports the dep `/types.ts`'s
    //    BASE parse fact. The warm hit returns the stale base
    //    analysis and the assertion below fails (it sees `string`,
    //    not `number`).
    //
    //    Post-fix: validation routes through the session-overlayed
    //    view (`with_session_overlay`), which re-roots the dep's
    //    parse fact at the overlay content hash. The base candidate's
    //    `Parse(/types.ts, BASE_HASH)` fact misses, the warm hit
    //    returns `None`, the cold recompute under the session observes
    //    `interface Props { msg: number }`, and the assertion below
    //    passes.
    let session_meta = session
        .get_component_meta("/workspace/src/Comp.vue")
        .expect("session query returns Ok")
        .expect("session has overlay-derived meta for Comp.vue");

    assert_eq!(
        prop_type(&session_meta, "msg"),
        &TypeExpr::Primitive(PrimitiveName::Number),
        "DISCRIMINATING: the session overlays `/types.ts` so the dep's \
         exported `Props.msg` type becomes `number`. The session's \
         view-aware warm-hit path MUST validate cache facts against the \
         session-overlayed store view; otherwise the base candidate's \
         dep parse fact (pinned to base content) survives and the \
         consumer observes the STALE `string`. Observed prop type: {:?}",
        prop_type(&session_meta, "msg"),
    );
}
