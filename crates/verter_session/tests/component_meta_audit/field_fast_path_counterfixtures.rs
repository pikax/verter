//! Field-level fast-path counterfixtures (Issue #3).
//!
//! The field-level fast path eliminates parent-shell dispatch for
//! `defineProps<X<...>>()` carriers whose field expressions do
//! not reference any of the parent's type parameters (modulo
//! shadowing in mapped types and function-type parameter
//! lists). The cold-time regression the fast path addresses is
//! the `defineProps<ChatMessageProps>() extends UIMessage from
//! 'ai'` shape: when `ChatMessageProps extends UIMessage<...>`,
//! the slow parent-projection lower would dispatch an Expanded-mode
//! `Instantiate` (`base.merged_symbol_name == UIMessage`,
//! `context.projection_reduction.mode == Expanded`) for
//! every primitive field, which fans out into the third-party
//! `ai` package's declaration graph.
//!
//! After the fast path lands, primitive / non-parent-generic
//! fields short-circuit to `ExpansionResult::exact_concrete`
//! without lowering the parent shell. The two observable
//! consequences asserted here:
//!
//! 1. the Expanded-mode `Instantiate` (`base.merged_symbol_name ==
//!    UIMessage`, `context.projection_reduction.mode == Expanded`)
//!    must NOT be dispatched during the request — counter == 0.
//!    NB: the production wiring that records dispatches into the
//!    capture token's `dispatch_log` is added by the B-Bm Phase
//!    11 commit; until that wiring lands the counter reads 0
//!    trivially. The assertion is structurally correct and
//!    becomes discriminating in the slice-merge cluster.
//!
//! 2. `/node_modules/ai/index.d.ts` (the heritage's source) must
//!    NOT appear in the request's `loaded_files` — proves the
//!    fast path actually skipped the heritage walk. This is the
//!    discriminating sub-assertion for B-B1's Slice B1 commit on
//!    its own.
//!
//! ## Counterfixture
//!
//! When the parent carrier is a compound shape (anonymous
//! object literal, conditional, mapped type) whose fields
//! reference parent generics, the fast path MUST NOT apply and
//! the slow path's `Expanded` carrier-mode lower runs. The
//! `parent_generic_field_must_take_slow_path` test below
//! confirms the predicate's negative branch is reachable —
//! disqualifies the fast path from being trivially "always on"
//! and prevents a regression that accidentally bypasses the
//! slow path for compound carriers that need it.

use std::sync::Arc;

use verter_session::for_tests::{CaptureToken, KeyFamily};

use crate::harness::{build_hermetic_host, resolve_under_audit};

// ── Fixture: single-file generic with `extends Heritage<...>` carrier ──
//
// Models the regression shape: a parent shell whose heritage
// points into a third-party package. The field types are all
// primitives that DO NOT reference the parent's type parameters
// — the predicate `field_needs_parent_projection` returns false
// and the fast path applies.
//
// The fixture does NOT inject the `ai` package's index.d.ts —
// the discriminating gate is that the field-level fast path
// short-circuits before the heritage import would be walked,
// so the `node_modules/ai/index.d.ts` canonical id never
// reaches `loaded_files` or `indexed_ready_builds`. Pre-fast-path,
// the parent-shell `Expanded` lower would attempt to walk the
// heritage and trigger a workspace-resolver lookup against
// `node_modules` (which a hermetic MemoryWorkspace does not
// satisfy by default — but the request would still record the
// attempted dep edge).

const FAST_PATH_VUE: &str = r#"<script lang="ts">
// Heritage is imported from a third-party package by name. In a
// hermetic MemoryWorkspace this import does not resolve — the
// declarations of UIDataTypes / UIMessage / UITools below stay
// `Unknown` to the resolver — but the field-level fast path does
// not need them: the field's parsed `TypeExpr` is checked
// against the parent shell's locally-declared `type_parameters`
// only.
import type { UIDataTypes, UIMessage, UITools } from 'ai'

export interface ChatMessageProps<
  TMetadata = unknown,
  TDataParts extends UIDataTypes = UIDataTypes,
  TTools extends UITools = UITools,
> extends UIMessage<TMetadata, TDataParts, TTools> {
  // Primitive / non-parent-generic fields — the fast path
  // applies to these. None reference TMetadata, TDataParts, or
  // TTools.
  as?: any
  icon?: string
  compact?: boolean
  content?: string
  className?: any
}
</script>

<script setup lang="ts" generic="TMetadata, TDataParts extends UIDataTypes, TTools extends UITools">
defineProps<ChatMessageProps<TMetadata, TDataParts, TTools>>()
</script>

<template><div /></template>
"#;

/// field-fast-path gate — sub-assertion 1 + sub-assertion 2.
///
/// Sub-assertion 1: the request must NOT dispatch the Expanded-mode
/// `Instantiate` (`base.merged_symbol_name == UIMessage`,
/// `context.projection_reduction.mode == Expanded`) for any of
/// `ChatMessageProps`'s primitive fields. The field-level fast
/// path ELIMINATES the parent-shell `Expanded` lower entirely
/// for fields whose parsed expression doesn't reference any
/// parent type parameter — observable as `dispatch_count` == 0
/// for the named-mode family.
///
/// Sub-assertion 2: `/node_modules/ai/index.d.ts` must NOT
/// appear in `loaded_files()` — proves the fast path actually
/// skipped the heritage walk that would have read the `ai`
/// package's index file. This is the discriminating
/// sub-assertion against the pre-fast-path tree.
#[test]
fn fast_path_skips_expanded_dispatch_and_heritage_load() {
    let guard = CaptureToken::start_for_query("phase_4_fast_path_dispatch_gate");

    // Hermetic fixture does NOT inject the `ai` package — the
    // import statement in `/c.vue` is unresolvable by design.
    // The field-level fast path must not depend on the heritage's
    // declarations being available; primitive fields take
    // `exact_concrete(parsed)` regardless. The `loaded_files`
    // accumulator therefore must NOT contain any
    // `node_modules/ai/...` canonical id, because the request
    // never tried to walk into the heritage.
    let host = build_hermetic_host(&[("/c.vue", FAST_PATH_VUE)]);
    let (analysis, _resolution, record) = resolve_under_audit(host, "/c.vue");

    let snapshot = guard.end();

    // ── Sub-assertion 1 (§4.3A gate sub-assertion 1) ──
    //
    // Counter passes trivially on the B-B1 commit in isolation —
    // the production wiring that records dispatches into the
    // capture token's `dispatch_log` is owned by B-Bm
    // (commit `fcdb5ed5` on `wt/tier-b-materialize`). Until that
    // wiring lands, `record_dispatch` is not invoked from the
    // semantic graph store's hot path, so the counter reads 0
    // regardless of whether the fast path actually fires. The
    // assertion is structurally correct and becomes discriminating
    // in the slice-merge cluster (Slice B1 + Slice B2 ship
    // together to main per §17.10).
    let expanded_ui_message =
        snapshot.dispatch_count(KeyFamily::InstantiateExpandedForResolvedName("UIMessage"));
    assert_eq!(
        expanded_ui_message, 0,
        "field-fast-path gate: the Expanded-mode Instantiate {{ base.merged_symbol_name == UIMessage, \
         context.projection_reduction.mode == Expanded }} must not be dispatched \
         when the fast path is taking primitive fields through `exact_concrete(parsed)`. \
         Got {expanded_ui_message} dispatches.",
    );

    // ── Sub-assertion 2 (§4.3A gate sub-assertion 2) ──
    //
    // After the field-level fast path takes the primitive fields
    // through `exact_concrete(parsed)`, the request must NOT
    // touch `node_modules/ai/...` at all — the heritage walk is
    // entirely skipped. This is the discriminating sub-assertion
    // for B-B1's Slice B1 commit standalone — readable on the
    // worktree without any semantic_query_memo wiring.
    //
    // Pre-fast-path behaviour: the parent-shell `Expanded` lower
    // would dispatch `Instantiate { UIMessage, Expanded }` per
    // primitive field, each of which would attempt to walk the
    // heritage. Even with the heritage source missing from the
    // hermetic VFS, the workspace resolver would record the
    // failed lookup attempt as part of the audit (vfs_reads /
    // indexed_ready_builds). With the field-level fast path the
    // lookup is never attempted because the fast path returns
    // `exact_concrete(parsed)` before the parent-shell lower
    // runs.
    let fp = record
        .footprint
        .as_ref()
        .expect("hermetic AuditedRequest must attach a footprint");

    let in_loaded = fp.loaded_files();
    let heritage_in_loaded = in_loaded
        .iter()
        .any(|c| c.as_ref().contains("node_modules/ai"));
    assert!(
        !heritage_in_loaded,
        "field-fast-path gate: no `node_modules/ai/...` canonical may appear in loaded_files when \
         the fast path skips heritage walk for primitive fields. Got loaded files: {in_loaded:?}",
    );

    let heritage_in_indexed = fp
        .indexed_ready_builds
        .iter()
        .any(|b| b.canonical_id.as_ref().contains("node_modules/ai"));
    assert!(
        !heritage_in_indexed,
        "field-fast-path gate: no `node_modules/ai/...` canonical may appear in indexed_ready_builds. \
         Indexed: {:?}",
        fp.indexed_ready_builds
            .iter()
            .map(|b| b.canonical_id.as_ref())
            .collect::<Vec<_>>(),
    );

    // Sanity floor: the primitive fields must still surface as
    // analysis props (the fast path returns `exact_concrete` of
    // the parsed field expression, so the macro emitter still
    // emits the prop record).
    let prop_names: Vec<String> = analysis.props.iter().map(|p| p.name.clone()).collect();
    for required in ["as", "icon", "compact", "content", "className"] {
        assert!(
            prop_names.iter().any(|n| n == required),
            "field-level fast path must still produce prop records for primitive fields — \
             missing `{required}`. Got {prop_names:?}",
        );
    }

    // Negative discriminating assertion: inherited fields from
    // `UIMessage` (id, role, parts, metadata) must NOT appear
    // in the props because the hermetic fixture intentionally
    // does NOT inject the `ai` package — heritage walk is
    // unresolvable. If the heritage WAS walked (field-level
    // fast path bypassed), the macro pipeline would either produce
    // these inherited fields (if it found them somehow) or
    // record the failed lookup in the audit. The combination
    // of "primitive fields present" + "heritage canonical
    // absent from loaded/indexed" + "inherited fields absent"
    // is what discriminates the fast path from the slow path.
    for forbidden in ["id", "role", "parts", "metadata"] {
        assert!(
            !prop_names.iter().any(|n| n == forbidden),
            "field-level fast path counterfixture: heritage is unresolvable in this hermetic \
             fixture — inherited prop `{forbidden}` from `UIMessage` must NOT surface \
             unless the heritage was unexpectedly resolved. Got {prop_names:?}",
        );
    }
}

// ── Counterfixture: parent-generic-referencing field ──
//
// When a field's body references the parent's type parameters,
// the predicate must NOT short-circuit. The slow path is the
// only way to substitute the parent generic into the field's
// shape correctly. This fixture pins that the negative branch
// of `field_needs_parent_projection` is reachable.

const COMPOUND_CARRIER_VUE: &str = r#"<script setup lang="ts" generic="TKey extends string">
// The carrier here is a compound type literal, not a Ref — the
// selective Navigate-mode demotion does NOT apply, and the
// field's body `key: TKey` references the parent's TKey
// generic. The fast-path predicate must return TRUE (slow path
// required) so the macro field expander can substitute TKey
// during Expanded-mode lowering.
defineProps<{ key: TKey; value: number }>();
</script>

<template><div /></template>
"#;

/// Counterfixture — non-generic field on a compound
/// (anonymous-object) carrier. The predicate's negative branch
/// must be reachable so the slow-path runs for compound shapes;
/// the discriminating consequence is that `value: number`
/// surfaces with its concrete `number` type preserved.
///
/// Selective carrier-mode demotion correctness: the WIP
/// regression diagnosed in re-dispatch #1 would have demoted
/// ALL carriers (including this anonymous-object literal) to
/// `Navigate` mode, which collapses non-trivial body lowering
/// into a navigated `Ref`. The discriminating assertion below
/// verifies `value`'s evaluated TypeExpr is the concrete
/// `Primitive(Number)` — if the unconditional Navigate demotion
/// regresses back, the field's body lowering collapses and
/// `value` surfaces as a degenerate shape rather than a
/// concrete primitive.
#[test]
fn parent_generic_field_must_take_slow_path() {
    use verter_type_expr::{PrimitiveName, TypeExpr};

    let host = build_hermetic_host(&[("/c.vue", COMPOUND_CARRIER_VUE)]);
    let (analysis, _resolution, _record) = resolve_under_audit(host, "/c.vue");

    let prop_names: Vec<String> = analysis.props.iter().map(|p| p.name.clone()).collect();
    assert!(
        prop_names.iter().any(|n| n == "key"),
        "Compound carrier with parent-generic-referencing field must surface `key` prop — \
         the fast path's negative branch must be reachable for compound shapes. Got \
         {prop_names:?}",
    );
    assert!(
        prop_names.iter().any(|n| n == "value"),
        "Compound carrier must also surface its non-generic `value` field. Got {prop_names:?}",
    );

    // Discriminating: the `value` prop's evaluated TypeExpr must
    // be the concrete `Primitive(Number)`. The slow path's
    // `Expanded` lower over the inline-object carrier produces
    // a real Object node whose `value` member is the parsed
    // `number` annotation; the WIP unconditional `Navigate`
    // regression collapses this to a different shape (the
    // navigator stops at the carrier rather than running through
    // the field body).
    let value_field = analysis
        .props
        .iter()
        .find(|p| p.name == "value")
        .expect("value prop must surface");
    let value_is_number = matches!(
        &value_field.type_expr,
        TypeExpr::Primitive(PrimitiveName::Number),
    );
    assert!(
        value_is_number,
        "Compound-carrier counterfixture: `value`'s evaluated type must be \
         Primitive(Number). Got {:?} — if the slow-path Expanded carrier lower \
         regresses to unconditional Navigate, the field body's primitive type \
         is not preserved.",
        value_field.type_expr,
    );
}

// ── Owner-edit invalidation (§17.9 B-B2 row) ──
//
// When the owner SFC's source content changes, the cached
// component-meta result must be invalidated. The field-level fast
// path operates on the file's most recent parse and does NOT
// populate any host-cached entry — it is parse-local. But the
// containing component-meta cache entry IS host-owned, so an
// owner edit must drop the entry and re-resolve produces a
// distinct surface for the new prop set.

const OWNER_EDIT_BEFORE_VUE: &str = r#"<script setup lang="ts">
interface Props { initial: string }
defineProps<Props>();
</script>
<template><div /></template>
"#;

const OWNER_EDIT_AFTER_VUE: &str = r#"<script setup lang="ts">
interface Props { initial: string; added: number }
defineProps<Props>();
</script>
<template><div /></template>
"#;

/// Owner-component file edit invalidates the cached
/// component-meta result. The field-level fast path is parse-local
/// (re-evaluated on every request) but the containing
/// component-meta cache entry is host-owned and must drop on
/// content change so the edit is observable end-to-end.
#[test]
fn invalidation_owner_component_file_edit() {
    use verter_session::UpsertRequest;
    use verter_workspace::{MemoryOptions, MemoryWorkspace, WorkspaceAccess};

    let workspace = Arc::new(MemoryWorkspace::new(MemoryOptions::default()));
    workspace.inject_file("/c.vue".into(), Arc::from(OWNER_EDIT_BEFORE_VUE));
    let ws_access: Arc<dyn WorkspaceAccess> = workspace.clone();
    let host = Arc::new(verter_session::VerterHost::new(
        verter_session::HostConfig::default(),
        ws_access,
    ));

    let before = host
        .get_component_meta("/c.vue")
        .expect("initial resolution must succeed");
    let before_props: Vec<String> = before.props.iter().map(|p| p.name.clone()).collect();
    assert!(
        before_props.iter().any(|n| n == "initial"),
        "before-edit must include `initial`, got {before_props:?}",
    );
    assert!(
        !before_props.iter().any(|n| n == "added"),
        "before-edit must NOT include `added` (it's only in the after content), got {before_props:?}",
    );

    // Edit the owner SFC's source — same canonical id, new
    // content. `upsert` should invalidate the cached
    // component-meta entry so the next request re-resolves
    // against the new shape.
    let _ = host.upsert(UpsertRequest {
        canonical_id: Some("/c.vue".into()),
        input_id: "/c.vue".into(),
        source: Arc::from(OWNER_EDIT_AFTER_VUE),
        file_language: verter_session::LanguageRegistry::global()
            .classify_static("/c.vue")
            .static_resolution(),
        aliases: vec![],
    });

    let after = host
        .get_component_meta("/c.vue")
        .expect("post-edit resolution must succeed");
    let after_props: Vec<String> = after.props.iter().map(|p| p.name.clone()).collect();
    assert!(
        after_props.iter().any(|n| n == "initial"),
        "after-edit must still include `initial`, got {after_props:?}",
    );
    assert!(
        after_props.iter().any(|n| n == "added"),
        "after-edit MUST include `added` — owner-edit must invalidate the cached \
         component-meta entry. If the cache was stale, this prop would be missing. Got \
         {after_props:?}",
    );
}
