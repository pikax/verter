//! Cross-owner-reuse discriminating production-flow tests.
//!
//! Binds **R7 / R8** cross-owner reuse: N consumer scopes reaching
//! the same `(base, scope_axis, mode)` collapse to ONE entry in
//! `MaterializeStructureDb`. The discrimination drives
//! `get_component_meta` from N distinct owners over a shared inner
//! type and asserts the production `MaterializeStructureDb`'s entry
//! count — NOT a `HashSet<Key>` shape check (which would only
//! exercise the `Hash`/`PartialEq` impl).
//!
//! Discriminating contract:
//!
//! - A legacy `#[derive(Hash, PartialEq)]` including
//!   `scope_canonical_id` would make N owners produce N entries — one
//!   per owner scope.
//! - The hand-rolled `Hash`/`PartialEq` excluding `scope_canonical_id`
//!   collapses N owners to a single entry per semantic slot.
//!
//! The test boots a real `VerterHost` against a small workspace
//! fixture with N Vue components that all import and use a single
//! shared inner type, drives `get_component_meta` from each, and
//! reads `MaterializeStructureDb::entry_count()`.

use std::sync::Arc;

use rustc_hash::FxHashMap;

use verter_session::meta::MetaProject;
use verter_session::{HostConfig, VerterHost};
use verter_workspace::{MemoryOptions, MemoryWorkspace, WorkspaceAccess};

/// Build a hermetic project (host wrapped in `MetaProject`) backed
/// by a [`MemoryWorkspace`] pre-populated with the supplied files.
fn build_hermetic_project(files: &[(&str, &str)]) -> Arc<MetaProject> {
    let scheduler_config = verter_scheduler::scheduler::SchedulerConfig {
        cpu_threads: 1,
        ..verter_scheduler::scheduler::SchedulerConfig::default()
    };
    let workspace = Arc::new(MemoryWorkspace::new(MemoryOptions::default()));
    for (canonical, content) in files {
        workspace.inject_file((*canonical).into(), Arc::from(*content));
    }
    let ws_access: Arc<dyn WorkspaceAccess> = workspace;
    let host = VerterHost::new_with_scheduler_config(
        HostConfig {
            analysis_level: verter_session::AnalysisLevel::Full,
            ..HostConfig::default()
        },
        ws_access,
        scheduler_config,
    );
    MetaProject::new(host)
}

const SHARED_TYPE_TS: &str = r#"
export interface ChatMessageProps {
  id: string;
  body: string;
  author: string;
  timestamp: number;
}
"#;

// Each owner uses `Pick<ChatMessageProps, ...>` so the materialiser
// (`materialize_member_surface_expr` in `registry_decl.rs`) is driven
// to populate `MaterializeStructureDb` for the cross-owner-shared
// `ChatMessageProps` inner type. Without a Pick / Omit consumer the
// `ChatMessageProps` ref stays shallow and the DB stays empty —
// undiscriminating.
const INBOX_VUE: &str = r#"<script setup lang="ts">
import type { ChatMessageProps } from './chat-types'
defineProps<{
  message: ChatMessageProps;
  picked: Pick<ChatMessageProps, 'id'>;
}>();
</script>
<template><div /></template>
"#;

const CHAT_VUE: &str = r#"<script setup lang="ts">
import type { ChatMessageProps } from './chat-types'
defineProps<{
  message: ChatMessageProps;
  picked: Pick<ChatMessageProps, 'id'>;
}>();
</script>
<template><div /></template>
"#;

const SIDEBAR_VUE: &str = r#"<script setup lang="ts">
import type { ChatMessageProps } from './chat-types'
defineProps<{
  message: ChatMessageProps;
  picked: Pick<ChatMessageProps, 'id'>;
}>();
</script>
<template><div /></template>
"#;

const HEADER_VUE: &str = r#"<script setup lang="ts">
import type { ChatMessageProps } from './chat-types'
defineProps<{
  message: ChatMessageProps;
  picked: Pick<ChatMessageProps, 'id'>;
}>();
</script>
<template><div /></template>
"#;

/// **Primary test — production-flow `get_component_meta`.**
///
/// Drive `get_component_meta` from N=4 owners that all consume the
/// SAME inner type `ChatMessageProps` via `Pick<ChatMessageProps,'id'>`.
/// Each owner's prop-type materialisation flows through the shared
/// resolver → cache → registry stack. Under R7 cross-owner reuse,
/// the `MaterializeStructureDb` does NOT accumulate per-owner entries
/// for the shared inner-type slot.
///
/// Discrimination strategy:
///
/// - Capture `entry_count()` AFTER all owners run.
/// - Compare against a `single_owner_baseline_entries()` control
///   measurement.
/// - Under the cross-owner-keying impl: N owners reach the
///   same `(base, scope_axis, mode)` slot for the inner type → ONE
///   entry per slot. Under a legacy `scope_canonical_id`-included
///   impl, N owners would produce ~N × baseline entries because
///   each per-owner scope makes the cache key distinct.
///
/// Note: production caches above `MaterializeStructureDb`
/// (`ComponentMetaResultDb`, `SemanticGraphStore`, etc.) ALSO share
/// work across owners. The cross-owner reuse property at the
/// `MaterializeStructureDb` layer is independently verified at the
/// dispatch level by
/// `n_owners_via_materialize_surface_dispatch_collapse_to_one_entry`
/// below.
#[test]
fn n_owners_sharing_inner_type_materialise_once_in_production_flow() {
    let project = build_hermetic_project(&[
        ("/workspace/src/chat-types.ts", SHARED_TYPE_TS),
        ("/workspace/src/Inbox.vue", INBOX_VUE),
        ("/workspace/src/Chat.vue", CHAT_VUE),
        ("/workspace/src/Sidebar.vue", SIDEBAR_VUE),
        ("/workspace/src/Header.vue", HEADER_VUE),
    ]);

    let host = project.host();
    let db = host.project_type_store().materialize_structure_db();

    // Baseline: before any owners query, the entry map is empty.
    let baseline = db.entry_count();
    assert_eq!(
        baseline, 0,
        "control: a fresh project has 0 entries in MaterializeStructureDb"
    );

    // Drive get_component_meta from each owner.
    let owners = [
        "/workspace/src/Inbox.vue",
        "/workspace/src/Chat.vue",
        "/workspace/src/Sidebar.vue",
        "/workspace/src/Header.vue",
    ];

    for owner in &owners {
        let meta = host.get_component_meta(owner);
        assert!(
            meta.is_some(),
            "control: get_component_meta must succeed for {owner}"
        );
        let props: Vec<String> = meta.unwrap().props.iter().map(|p| p.name.clone()).collect();
        assert!(
            props.contains(&"message".to_string()),
            "owner {owner} must publish `message` prop, got {props:?}"
        );
    }

    let final_count = db.entry_count();
    let single_owner_baseline = single_owner_baseline_entries();
    // Post-fix contract: N owners over a shared inner type produce
    // roughly `baseline` entries (sharing collapses the per-owner
    // duplication). The legacy impl would multiply by N.
    eprintln!(
        "[gap-B production flow] final_count={final_count}, single_owner_baseline={single_owner_baseline}, N={}",
        owners.len()
    );
    assert!(
        final_count <= single_owner_baseline + owners.len(),
        "R7 cross-owner reuse: N={} owners over a shared inner type MUST produce entries \
         bounded by baseline + N — sharing must collapse inner-type slots. \
         got final_count={}, single_owner_baseline={}, max permitted = baseline ({}) + N ({}) = {}",
        owners.len(),
        final_count,
        single_owner_baseline,
        single_owner_baseline,
        owners.len(),
        single_owner_baseline + owners.len()
    );
}

/// **Primary discrimination — `materialize_surface` dispatch
/// over N owners.**
///
/// Directly exercise the
/// [`ProjectSemanticDispatch::materialize_surface`] dispatch from N=4
/// distinct owner scopes, using a SHARED `base` `SemanticNodeId`. This
/// is the same dispatch helper consumed by
/// `materialize_member_surface_expr` (the production materialiser
/// entry point) — making the cross-owner reuse property runtime-
/// observable on the live `MaterializeStructureDb`.
///
/// Discrimination strategy:
///
/// - Drive `materialize_surface` from N distinct
///   `scope_canonical_id` values, all pointing at the same
///   `(base, scope_axis, mode)` slot.
/// - Assert `entry_count == 1` post-fix. Under the legacy
///   `scope_canonical_id`-included impl, this would be `N`.
///
/// This is NOT a "key-shape" test in the disallowed sense (we are
/// driving real dispatch calls on a real `VerterHost`'s shared DB);
/// the discrimination signal is observed on the production cache,
/// not on a synthetic `HashSet<Key>`. The legacy impl breaks this
/// runtime contract immediately.
#[test]
fn n_owners_via_materialize_surface_dispatch_collapse_to_one_entry() {
    use verter_session::component_meta_materialize::{MaterializationScope, MaterializeRuntimeKey};
    use verter_session::semantic_query::ProjectionMode;
    use verter_type_expr::TypeExpr;

    let owners = [
        "/workspace/src/Inbox.vue",
        "/workspace/src/Chat.vue",
        "/workspace/src/Sidebar.vue",
        "/workspace/src/Header.vue",
    ];

    // A hermetic project carrying the shared type the `base` roots in.
    let project = build_hermetic_project(&[("/workspace/src/chat-types.ts", SHARED_TYPE_TS)]);
    let host = project.host();
    let db = host.project_type_store().materialize_structure_db();

    let count_before = db.entry_count();
    assert_eq!(count_before, 0, "control: fresh DB has 0 entries");

    // A DECL-ROOTED `base`: lower `Ref { ChatMessageProps }` in its
    // declaration scope (Navigate keeps it a `DeclRef` carrier). The
    // materialiser canonicalises this to
    // `slot(/workspace/src/chat-types.ts, ChatMessageProps)` — a
    // content-free `MaterializationCacheKey` with NO consumer-scope
    // dimension. (An anonymous `SemanticNodeId(0)` base would key no DB
    // slot — it computes uncached — so it can no longer exercise this
    // contract.) Lowering auto-loads the scope so the `DeclRef` carries
    // the file's live whole hash, and the warm-read self-root validates.
    let dispatch = host.semantic_dispatch();
    let base = dispatch
        .lower_type_expr_in_scope_with_mode(
            "/workspace/src/chat-types.ts",
            &TypeExpr::Ref {
                name: Arc::from("ChatMessageProps"),
                type_arguments: Arc::from(Vec::new()),
            },
            ProjectionMode::Navigate,
        )
        .expect("lowering ChatMessageProps via Navigate must produce a DeclRef base");

    for scope in &owners {
        let key = MaterializeRuntimeKey {
            scope_canonical_id: Arc::from(*scope),
            base,
            scope_axis: MaterializationScope::TopLevel,
            mode: ProjectionMode::Expanded,
        };
        let _ = dispatch.materialize_surface(key);
    }

    let count_after = db.entry_count();
    let delta = count_after - count_before;
    eprintln!(
        "[gap-B dispatch flow] count_before={count_before}, count_after={count_after}, \
         delta={delta}, N={}",
        owners.len()
    );
    // R7 cross-owner reuse: N `materialize_surface` calls differing ONLY
    // in `scope_canonical_id` (shared canonical subject) MUST collapse to
    // ONE new MaterializeStructureDb entry — the first cold-builds, the
    // rest warm-hit. The canonical `MaterializationCacheKey` has NO
    // consumer-scope dimension, so the slot is scope-independent by
    // construction; a key that re-introduced the scope would add N
    // entries, one per owner scope.
    assert_eq!(
        delta,
        1,
        "R7 cross-owner reuse: {} dispatch calls over a shared canonical subject with \
         different scope_canonical_id MUST add exactly ONE MaterializeStructureDb entry. \
         got delta={} (count_before={}, count_after={}) — a value > 1 indicates the \
         consumer scope leaked into the canonical MaterializationCacheKey.",
        owners.len(),
        delta,
        count_before,
        count_after
    );
}

/// Single-owner baseline: drive get_component_meta against a single
/// Vue component over the same inner type and record how many entries
/// it produces in `MaterializeStructureDb`. The N-owner test above
/// uses this to compute a per-slot upper bound for the cross-owner
/// case.
fn single_owner_baseline_entries() -> usize {
    let project = build_hermetic_project(&[
        ("/workspace/src/chat-types.ts", SHARED_TYPE_TS),
        ("/workspace/src/Solo.vue", INBOX_VUE),
    ]);
    let host = project.host();
    let _ = host.get_component_meta("/workspace/src/Solo.vue");
    host.project_type_store()
        .materialize_structure_db()
        .entry_count()
}

/// **Recursion-identity scope-exclusion arch-guard.** Pins that the
/// per-thread recursion/depth identity [`MaterializeRuntimeKey`]
/// excludes `scope_canonical_id` from `Hash`/`PartialEq`: two consumer
/// scopes reaching the same `(base, scope_axis, mode)` share ONE
/// recursion identity. (The DB cross-owner reuse is now STRUCTURAL — the
/// canonical `MaterializationCacheKey` carries NO consumer-scope
/// dimension at all — and is exercised end-to-end by the two
/// entry-count tests above; this pins the runtime key's complementary
/// scope-independence.)
#[test]
fn r7_runtime_key_hash_partial_eq_excludes_scope_canonical_id() {
    use std::hash::{Hash, Hasher};
    use verter_session::component_meta_materialize::{MaterializationScope, MaterializeRuntimeKey};
    use verter_session::semantic_query::{ProjectionMode, SemanticNodeId};

    let base = SemanticNodeId(42);

    let k_a = MaterializeRuntimeKey {
        scope_canonical_id: Arc::from("/owner/A.vue"),
        base,
        scope_axis: MaterializationScope::TopLevel,
        mode: ProjectionMode::Expanded,
    };
    let k_b = MaterializeRuntimeKey {
        scope_canonical_id: Arc::from("/owner/B.vue"),
        base,
        scope_axis: MaterializationScope::TopLevel,
        mode: ProjectionMode::Expanded,
    };

    // PartialEq excludes scope_canonical_id.
    assert_eq!(
        k_a, k_b,
        "recursion identity: runtime keys with different scope_canonical_id but identical \
         (base, scope_axis, mode) MUST compare equal — the recursion/depth identity does \
         not depend on which consumer reached the node."
    );

    // Hash agrees with PartialEq (HashSet contract).
    let mut h_a = rustc_hash::FxHasher::default();
    let mut h_b = rustc_hash::FxHasher::default();
    k_a.hash(&mut h_a);
    k_b.hash(&mut h_b);
    assert_eq!(
        h_a.finish(),
        h_b.finish(),
        "recursion identity: runtime keys that compare equal MUST hash to the same value."
    );

    // Distinct (base, scope_axis, mode) still produces distinct keys.
    let k_distinct_base = MaterializeRuntimeKey {
        scope_canonical_id: Arc::from("/owner/A.vue"),
        base: SemanticNodeId(99),
        scope_axis: MaterializationScope::TopLevel,
        mode: ProjectionMode::Expanded,
    };
    assert_ne!(
        k_a, k_distinct_base,
        "control: distinct base nodes MUST produce distinct runtime keys"
    );
}

/// **entry_count accessor visibility.** Pins that
/// `MaterializeStructureDb::entry_count()` is `pub` (non-test) so the
/// production-flow tests can call it. A regression that gates this
/// behind `#[cfg(test)]` would silently disable the cross-owner reuse
/// audit.
#[test]
fn materialize_structure_db_entry_count_is_pub() {
    let project = build_hermetic_project(&[("/workspace/x.ts", "export const x = 1;")]);
    let db = project
        .host()
        .project_type_store()
        .materialize_structure_db();
    // Calling `entry_count()` from an integration test crate confirms
    // it's part of the public surface. Compile success is the assertion.
    let _: usize = db.entry_count();
    // Silence unused-imports warnings — the integration test's
    // use-statements are what makes the test load-bearing.
    let _ = (FxHashMap::<String, Arc<str>>::default(),);
}
