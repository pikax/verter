//! Overlay-only parent-directory index imports resolve through the
//! session view — for EVERY TS `pathIsRelative` spelling.
//!
//! The session-view materialiser resolves an owner's import edges
//! through the workspace resolver first and falls back to
//! `resolve_relative_overlay_candidate` for helpers that exist ONLY as
//! session overlays (unsaved buffers with no base/VFS presence). That
//! fallback joins the specifier against the owner canonical through
//! `verter_workspace::relative_path::join_relative` — a SECOND join
//! path, distinct from the resolver's `join_paths`. The two must agree
//! on the full TS `pathIsRelative` class (`/^\.\.?($|[\\/])/`): bare
//! `.`/`..` plus the `./`, `../`, `.\`, `..\` prefixes, with `\`
//! treated as a separator (TS `normalizeSlashes`).
//!
//! Shape (mirrors the reka-ui surface-arm class): a component in a
//! subdirectory imports its heritage parent type from the PARENT
//! DIRECTORY INDEX, and that index exists only in the session overlay.
//! `defineProps<Props>()` with `interface Props extends PrimitiveProps`
//! flattens the parent surface into props, so the inherited member
//! (`as`) appears iff the parent-index import RESOLVED through the
//! overlay.
//!
//! Three spellings, one invariant — but NOT one seam: only the
//! `'..\index'` backslash spelling exercises the separator rewrite this
//! suite regression-guards (the `.vue` source text must spell it
//! `'..\\index'` because verter reads OXC's COOKED string value — a raw
//! `'..\index'` cooks to `..index`). The `'../index'` control and the
//! bare-`'..'` case are behavior/refutation PINS: the overlay fallback's
//! pre-existing `starts_with('.')` gate plus the bare-`..` join already
//! covered them, so they pass pre- and post-fix by design and pin that
//! the fixed seam did not regress them.

use std::sync::Arc;

use verter_session::meta::MetaProject;
use verter_session::{CompileErrorPolicy, HostConfig, VerterHost};
use verter_workspace::{MemoryOptions, MemoryWorkspace, WorkspaceAccess};

/// Build a workspace-backed `MetaProject` rooted at `/workspace` with an
/// EMPTY base tree — every file in these tests is session-overlay-only.
fn empty_workspace_project() -> Arc<MetaProject> {
    let workspace = Arc::new(MemoryWorkspace::new(MemoryOptions::default()));
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

/// Session-only pair: `/workspace/src/index.ts` (the parent index,
/// exporting the heritage parent `PrimitiveProps`) and
/// `/workspace/src/Listbox/Filter.vue` importing it via
/// `specifier_source_text` (the RAW `.vue` source spelling). Returns the
/// sorted prop names of the session component-meta.
fn session_prop_names_for_specifier(specifier_source_text: &str) -> Vec<String> {
    let project = empty_workspace_project();
    let session = project.open_session().expect("open session");
    session
        .upsert(
            "/workspace/src/index.ts",
            "export interface PrimitiveProps { as?: string }\n".to_string(),
        )
        .expect("session overlay parent index upsert");
    session
        .upsert(
            "/workspace/src/Listbox/Filter.vue",
            format!(
                "<script setup lang=\"ts\">\n\
                 import type {{ PrimitiveProps }} from '{specifier_source_text}'\n\
                 interface Props extends PrimitiveProps {{ modelValue?: string }}\n\
                 const props = defineProps<Props>()\n\
                 </script>\n\
                 <template><div>{{{{ props.modelValue }}}}</div></template>\n"
            ),
        )
        .expect("session overlay component upsert");

    let meta = session
        .get_component_meta("/workspace/src/Listbox/Filter.vue")
        .expect("session query returns Ok")
        .expect("session has overlay-derived meta for Filter.vue");
    let mut names: Vec<String> = meta.props.iter().map(|p| p.name.clone()).collect();
    names.sort();
    names
}

fn assert_parent_index_surface(names: &[String], spelling: &str) {
    assert!(
        names.contains(&"modelValue".to_string()),
        "control: own prop `modelValue` present for the {spelling} spelling: {names:?}"
    );
    assert!(
        names.contains(&"as".to_string()),
        "the heritage parent imported from the overlay-only parent index via \
         {spelling} must resolve — inherited prop `as` missing means the \
         session-lane relative join diverged from the resolver's \
         `pathIsRelative` class: {names:?}"
    );
}

/// Control — behavior/refutation PIN, passes pre- and post-fix by
/// design: the `'../index'` spelling has always been handled by both
/// join paths. Pins that the harness shape (overlay-only pair +
/// heritage flattening) works at all; the backslash sibling below
/// carries the regression discrimination for the fixed seam.
#[test]
fn overlay_only_parent_index_resolves_via_dot_dot_slash_index_import() {
    let names = session_prop_names_for_specifier("../index");
    assert_parent_index_surface(&names, "'../index'");
}

/// Bare `'..'` — TS `pathIsRelative` classifies it relative (parent
/// directory index module). Behavior/refutation PIN, passes pre- and
/// post-fix by design: the overlay fallback's pre-existing
/// `starts_with('.')` gate plus the bare-`..` join already covered this
/// spelling on the session lane (the resolver-side bare-`..` fix is
/// covered by `resolver_tests.rs`); the backslash sibling below carries
/// the regression discrimination for the join_relative seam.
#[test]
fn overlay_only_parent_index_resolves_via_bare_dot_dot_import() {
    let names = session_prop_names_for_specifier("..");
    assert_parent_index_surface(&names, "bare '..'");
}

/// Backslash separator `'..\index'` — the same `pathIsRelative` class
/// (`[\\/]`); TS `normalizeSlashes` treats `\` as `/`. The `.vue`
/// source text spells it `'..\\index'` so the COOKED specifier value is
/// `..\index`.
#[test]
fn overlay_only_parent_index_resolves_via_backslash_index_import() {
    let names = session_prop_names_for_specifier("..\\\\index");
    assert_parent_index_surface(&names, "'..\\index'");
}

/// NEGATIVE: a dot-prefixed specifier OUTSIDE the TS `pathIsRelative`
/// class — `.alias\types` (no separator after the leading `.`; TS
/// classifies it package-ish and reports a resolution error) — must NOT
/// be separator-rewritten into a joinable relative path by the overlay
/// fallback's `join_relative`. A decoy helper is planted at exactly the
/// canonical an erroneous unconditional `\` → `/` rewrite would produce
/// (`/workspace/src/Listbox/.alias/types.ts`); resolving it would
/// fabricate BOTH a wrong type surface and a wrong dependency edge,
/// diverging from TS. The honest outcome is a resolution miss: meta
/// publishes WITHOUT the decoy's `as` member (the unresolved heritage
/// arm surfaces as a missing-macro-type-dep diagnostic, not a silently
/// wrong surface).
#[test]
fn dot_prefixed_package_like_specifier_never_resolves_the_slash_rewritten_decoy() {
    let project = empty_workspace_project();
    let session = project.open_session().expect("open session");
    // Decoy at the path the erroneous rewrite would produce.
    session
        .upsert(
            "/workspace/src/Listbox/.alias/types.ts",
            "export interface PrimitiveProps { as?: string }\n".to_string(),
        )
        .expect("session overlay decoy upsert");
    // The `.vue` source spells the specifier `'.alias\\types'`, cooking
    // to `.alias\types` (verter reads OXC's COOKED string value).
    session
        .upsert(
            "/workspace/src/Listbox/Filter.vue",
            "<script setup lang=\"ts\">\n\
             import type { PrimitiveProps } from '.alias\\\\types'\n\
             interface Props extends PrimitiveProps { modelValue?: string }\n\
             const props = defineProps<Props>()\n\
             </script>\n\
             <template><div>{{ props.modelValue }}</div></template>\n"
                .to_string(),
        )
        .expect("session overlay component upsert");

    // The honest outcome for the unresolvable package-ish specifier in
    // this session-overlay lane: meta publishes with the component's OWN
    // props only — the unresolved heritage arm contributes nothing.
    let meta = session
        .get_component_meta("/workspace/src/Listbox/Filter.vue")
        .expect("session query returns Ok")
        .expect("session publishes meta for Filter.vue");
    let names: Vec<String> = meta.props.iter().map(|p| p.name.clone()).collect();
    assert!(
        names.contains(&"modelValue".to_string()),
        "control: own prop `modelValue` present: {names:?}"
    );
    assert!(
        !names.contains(&"as".to_string()),
        "the non-relative specifier '.alias\\types' silently resolved the \
         slash-rewritten decoy — the `\\` → `/` rewrite in join_relative \
         must be gated on the pathIsRelative class: {names:?}"
    );
}
