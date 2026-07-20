//! Tests for the macro hot mirror producer ([`super::macro_type_arg_hot_ref`]).
//!
//! Each fixture is a REAL Vue SFC whose macro type-argument is lowered ONCE
//! through the mirror to a mode-NEUTRAL [`HotTypeRef`]. The tests assert the
//! produced carrier shape and the re-entry parity (re-entering the ONE
//! dispatch from the handle yields the SAME resolved node the eager macro-arg
//! lowering produced). Negative assertions throughout.

use std::sync::Arc;

use verter_type_expr::TypeExpr;

use super::macro_type_arg_hot_ref;
use super::MacroHotMirror;
use crate::project_semantic_dispatch::ProjectSemanticDispatch;
use crate::semantic_query::{
    HotTypeRef, PathSegment, ProjectionMode, ProjectionReductionContext, QueryResult,
    SemanticNodeData, SemanticNodeId, SemanticQueryApi, SemanticQueryKey, SemanticQueryOutput,
};
use crate::types::HostConfig;
use crate::{CompileErrorPolicy, FileLanguage, UpsertRequest, VerterHost};

fn host() -> VerterHost {
    VerterHost::new_standalone(HostConfig {
        dev_mode: false,
        compile_error_policy: CompileErrorPolicy::StrictError,
        ..HostConfig::default()
    })
}

fn upsert_ts(host: &VerterHost, id: &str, source: &str) {
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: id.to_string(),
            source: Arc::from(source),
            file_language: FileLanguage::script_ts(),
            aliases: Vec::new(),
        })
        .unwrap();
}

fn upsert_vue(host: &VerterHost, id: &str, source: &str) {
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: id.to_string(),
            source: Arc::from(source),
            file_language: FileLanguage::vue(),
            aliases: Vec::new(),
        })
        .unwrap();
}

impl MacroHotMirror {
    /// Number of demanded (filled) macro cells — test observability only,
    /// never a validity signal. A freshly published artifact reports `0`.
    pub(crate) fn demanded_count(&self) -> usize {
        self.cells
            .get()
            .map(|c| {
                c.iter()
                    .filter(|cell| cell.committed.get().is_some())
                    .count()
            })
            .unwrap_or(0)
    }
}

/// The 0-based index of the first macro in `canonical` (the SFCs below each
/// declare exactly one type-based macro).
fn first_macro_index(host: &VerterHost, canonical: &str) -> usize {
    let indexed = host
        .ensure_indexed_ready(canonical)
        .expect("owner SFC IndexedReady must materialise");
    let script = indexed
        .script_analysis
        .as_ref()
        .expect("owner SFC must carry script_analysis");
    script
        .macros
        .iter()
        .position(|m| m.is_type_based)
        .expect("owner SFC must declare a type-based macro")
}

/// The macro's parsed type argument lowered on demand for the eager-parity arm.
///
/// The argument's authored position is now a content-free locator
/// (`AnalyzedMacro.parsed_type_argument`); the eager TypeExpr is the lazy
/// body-memo lowering (`transient_macro_type_argument`) over the retained parse
/// snapshot — the SAME lowering the mirror producer consumes internally, so this
/// keeps the eager-vs-mirror parity comparing the two lower-to-node fronts over
/// an identical TypeExpr input.
fn macro_type_arg(host: &VerterHost, canonical: &str, macro_index: usize) -> Arc<TypeExpr> {
    let indexed = host.ensure_indexed_ready(canonical).expect("indexed");
    let script = indexed.script_analysis.as_ref().expect("script analysis");
    let macro_span = script.macros[macro_index].span;
    match indexed
        .shallow_state
        .decl_bodies()
        .transient_macro_type_argument(macro_span)
    {
        crate::decl_body_memo::DemandOutcome::Ready(Some(expr)) => expr,
        _ => panic!("type-based macro must lazily lower a parsed type argument"),
    }
}

fn macro_owner(
    host: &VerterHost,
    canonical: &str,
    macro_index: usize,
) -> verter_type_expr::TopLevelOwnerId {
    host.ensure_indexed_ready(canonical)
        .and_then(|indexed| {
            indexed
                .script_analysis
                .as_ref()?
                .macros
                .get(macro_index)
                .map(|mac| mac.owner)
        })
        .expect("type-based macro must have an exact owner")
}

fn node_data(
    dispatch: &ProjectSemanticDispatch<'_>,
    node: SemanticNodeId,
) -> Option<SemanticNodeData> {
    dispatch.graph().node_data(node).map(|d| (*d).clone())
}

/// Drive a node through the dispatch as the base of an empty-path
/// `ProjectPath` query in `mode`, returning the resolved subject node.
fn resolve_subject(
    dispatch: &ProjectSemanticDispatch<'_>,
    base: SemanticNodeId,
    mode: ProjectionMode,
) -> SemanticNodeId {
    let empty_path: Arc<[PathSegment]> = Arc::from(Vec::new().into_boxed_slice());
    match dispatch.execute_type_node(SemanticQueryKey::ProjectPath {
        base,
        path: empty_path,
        context: ProjectionReductionContext::published(mode),
    }) {
        QueryResult::Value(SemanticQueryOutput { value: id, .. }) => id,
        QueryResult::Recursive(id) => id,
        other => panic!("ProjectPath over a subject did not yield a node: {other:?}"),
    }
}

/// Lower the macro arg through the eager `lower_type_expr_in_scope_with_context`
/// path (structural-transit Navigate), then resolve it through the empty-path
/// terminal. This is the parity oracle the mirror re-entry must match.
fn eager_resolved(
    dispatch: &ProjectSemanticDispatch<'_>,
    expr: &TypeExpr,
    canonical: &str,
    owner: verter_type_expr::TopLevelOwnerId,
    mode: ProjectionMode,
) -> SemanticNodeId {
    let lowered = dispatch
        .lower_type_expr_in_owner_scope_with_context(
            canonical,
            owner,
            expr,
            ProjectionReductionContext::structural_transit_with_mode(mode),
        )
        .expect("eager lowering must succeed");
    resolve_subject(dispatch, lowered, mode)
}

/// Resolve a mirror handle through the empty-path terminal in `mode`.
fn mirror_resolved(
    dispatch: &ProjectSemanticDispatch<'_>,
    handle: HotTypeRef,
    mode: ProjectionMode,
) -> SemanticNodeId {
    resolve_subject(dispatch, handle.node(), mode)
}

// ── Mirror produces the structural carrier shape ────────────────────────────

#[test]
fn bare_ref_macro_arg_mirrors_to_bare_ref_carrier_and_reentry_matches_eager() {
    let host = host();
    upsert_ts(
        &host,
        "/types.ts",
        "export type Props = { a: string; b: number };\n",
    );
    upsert_vue(
        &host,
        "/C.vue",
        "<script setup lang=\"ts\">\nimport type { Props } from './types'\ndefineProps<Props>()\n</script>\n<template><div /></template>\n",
    );
    let dispatch = ProjectSemanticDispatch::new(&host);
    let macro_index = first_macro_index(&host, "/C.vue");

    let handle = macro_type_arg_hot_ref(&host, "/C.vue", macro_index)
        .expect("the BareRef macro arg must mirror to a hot ref");

    // The mode-neutral mirror root is the UNRESOLVED BareRef carrier — never a
    // pre-resolved DeclRef and never an Opaque miss.
    let root = node_data(&dispatch, handle.node());
    assert!(
        matches!(root, Some(SemanticNodeData::BareRef(_))),
        "the mirror root for a bare named macro arg must be a BareRef carrier, got {root:?}"
    );

    // Re-entry through the ONE dispatch resolves the carrier head to Props'
    // body — identical to the eager macro-arg lowering.
    let arg = macro_type_arg(&host, "/C.vue", macro_index);
    let via_mirror = mirror_resolved(&dispatch, handle, ProjectionMode::Navigate);
    let wrong_owner = eager_resolved(
        &dispatch,
        arg.as_ref(),
        "/C.vue",
        verter_type_expr::TopLevelOwnerId::ordinary_file(),
        ProjectionMode::Navigate,
    );
    assert_ne!(
        via_mirror, wrong_owner,
        "an ordinary-script lowering must not resolve a script-setup import"
    );
    assert!(
        matches!(
            node_data(&dispatch, wrong_owner),
            Some(SemanticNodeData::BareRef(_)) | Some(SemanticNodeData::Opaque(_))
        ),
        "the wrong-owner control must remain an unresolved carrier"
    );
    let macro_owner = macro_owner(&host, "/C.vue", macro_index);
    let via_eager = eager_resolved(
        &dispatch,
        arg.as_ref(),
        "/C.vue",
        macro_owner,
        ProjectionMode::Navigate,
    );
    assert_eq!(
        via_mirror, via_eager,
        "re-entering the dispatch from the mirror handle must match the eager macro-arg resolution"
    );
    assert!(
        !matches!(
            node_data(&dispatch, via_mirror),
            Some(SemanticNodeData::Opaque(_))
        ),
        "`Props` is workspace-owned and must resolve, not miss"
    );
}

#[test]
fn import_type_macro_arg_mirrors_to_import_type_carrier() {
    let host = host();
    upsert_ts(&host, "/m.ts", "export type Q = { x: boolean };\n");
    upsert_vue(
        &host,
        "/I.vue",
        "<script setup lang=\"ts\">\ndefineProps<import('./m').Q>()\n</script>\n<template><div /></template>\n",
    );
    let dispatch = ProjectSemanticDispatch::new(&host);
    let macro_index = first_macro_index(&host, "/I.vue");

    let handle = macro_type_arg_hot_ref(&host, "/I.vue", macro_index)
        .expect("the ImportType macro arg must mirror to a hot ref");
    let root = node_data(&dispatch, handle.node());
    assert!(
        matches!(root, Some(SemanticNodeData::ImportType(_))),
        "the mirror root for an import('m').Q macro arg must be an ImportType carrier, got {root:?}"
    );
}

#[test]
fn inline_props_macro_arg_mirrors_to_object_with_macro_own_body_provenance() {
    let host = host();
    upsert_vue(
        &host,
        "/Inline.vue",
        "<script setup lang=\"ts\">\ndefineProps<{ a: string; b?: number }>()\n</script>\n<template><div /></template>\n",
    );
    let dispatch = ProjectSemanticDispatch::new(&host);
    let macro_index = first_macro_index(&host, "/Inline.vue");

    let handle = macro_type_arg_hot_ref(&host, "/Inline.vue", macro_index)
        .expect("the inline-object macro arg must mirror to a hot ref");
    let root = node_data(&dispatch, handle.node());
    match root {
        Some(SemanticNodeData::Object(view)) => {
            let names: Vec<&str> = view.members.iter().map(|m| m.name.as_ref()).collect();
            assert!(
                names.contains(&"a") && names.contains(&"b"),
                "inline props object must carry both members, got {names:?}"
            );
            // Provenance survives the mirror path: DefineProps requests the
            // macro-T own-body provenance on its direct members.
            assert!(
                view.members
                    .iter()
                    .all(|m| m.declared_in_macro_type_arg.get()),
                "inline DefineProps members must carry `declared_in_macro_type_arg = true` \
                 through the mirror path"
            );
        }
        other => panic!("inline props macro arg must mirror to an Object surface, got {other:?}"),
    }
}

#[test]
fn keyof_macro_arg_mirrors_to_deferred_keyof_shell() {
    let host = host();
    upsert_ts(&host, "/k.ts", "export type Base = { a: 1; b: 2 };\n");
    upsert_vue(
        &host,
        "/K.vue",
        "<script setup lang=\"ts\">\nimport type { Base } from './k'\ndefineEmits<keyof Base>()\n</script>\n<template><div /></template>\n",
    );
    let dispatch = ProjectSemanticDispatch::new(&host);
    let macro_index = first_macro_index(&host, "/K.vue");

    let handle = macro_type_arg_hot_ref(&host, "/K.vue", macro_index)
        .expect("the keyof macro arg must mirror to a hot ref");
    let root = node_data(&dispatch, handle.node());
    assert!(
        matches!(root, Some(SemanticNodeData::KeyOf { .. })),
        "the mirror root for `keyof Base` must be a deferred KeyOf shell, got {root:?}"
    );
}

#[test]
fn typeof_macro_arg_mirrors_to_deferred_typeof_shell() {
    let host = host();
    upsert_vue(
        &host,
        "/T.vue",
        "<script setup lang=\"ts\">\nconst v = { a: 1 }\ndefineProps<typeof v>()\n</script>\n<template><div /></template>\n",
    );
    let dispatch = ProjectSemanticDispatch::new(&host);
    let macro_index = first_macro_index(&host, "/T.vue");

    let handle = macro_type_arg_hot_ref(&host, "/T.vue", macro_index)
        .expect("the typeof macro arg must mirror to a hot ref");
    let root = node_data(&dispatch, handle.node());
    assert!(
        matches!(root, Some(SemanticNodeData::TypeOf { .. })),
        "the mirror root for `typeof v` must be a deferred TypeOf shell, got {root:?}"
    );
}

#[test]
fn conditional_macro_arg_mirrors_to_deferred_conditional_shell() {
    let host = host();
    upsert_vue(
        &host,
        "/Cond.vue",
        "<script setup lang=\"ts\" generic=\"T\">\ndefineProps<T extends string ? { s: T } : { n: number }>()\n</script>\n<template><div /></template>\n",
    );
    let dispatch = ProjectSemanticDispatch::new(&host);
    let macro_index = first_macro_index(&host, "/Cond.vue");

    let handle = macro_type_arg_hot_ref(&host, "/Cond.vue", macro_index)
        .expect("the conditional macro arg must mirror to a hot ref");
    let root = node_data(&dispatch, handle.node());
    assert!(
        matches!(root, Some(SemanticNodeData::Conditional { .. })),
        "the mirror root for a conditional macro arg must be a deferred Conditional shell, got {root:?}"
    );
}

#[test]
fn mapped_macro_arg_mirrors_to_deferred_mapped_shell() {
    let host = host();
    upsert_ts(
        &host,
        "/src.ts",
        "export type Src = { a: string; b: number };\n",
    );
    upsert_vue(
        &host,
        "/Mapped.vue",
        "<script setup lang=\"ts\">\nimport type { Src } from './src'\ndefineProps<{ [K in keyof Src]: boolean }>()\n</script>\n<template><div /></template>\n",
    );
    let dispatch = ProjectSemanticDispatch::new(&host);
    let macro_index = first_macro_index(&host, "/Mapped.vue");

    let handle = macro_type_arg_hot_ref(&host, "/Mapped.vue", macro_index)
        .expect("the mapped macro arg must mirror to a hot ref");
    let root = node_data(&dispatch, handle.node());
    assert!(
        matches!(root, Some(SemanticNodeData::Mapped { .. })),
        "the mirror root for a mapped macro arg must be a deferred Mapped shell, got {root:?}"
    );
}

// ── Script-setup generic seeding (THE correctness point) ────────────────────

#[test]
fn script_setup_generic_macro_arg_seeds_typeparam_binder_not_bare_ref() {
    let host = host();
    upsert_vue(
        &host,
        "/G.vue",
        "<script setup lang=\"ts\" generic=\"T\">\ndefineProps<T>()\n</script>\n<template><div /></template>\n",
    );
    let dispatch = ProjectSemanticDispatch::new(&host);
    let macro_index = first_macro_index(&host, "/G.vue");

    let handle = macro_type_arg_hot_ref(&host, "/G.vue", macro_index)
        .expect("the generic macro arg must mirror to a hot ref");
    let root = node_data(&dispatch, handle.node());

    // DISCRIMINATING: without the seed binder frame, `T` would lower to a
    // BareRef(T). With the seed frame, it resolves to its TypeParam binder.
    match root {
        Some(SemanticNodeData::TypeParam {
            display_name,
            param_index,
            ..
        }) => {
            assert_eq!(
                display_name.as_ref(),
                "T",
                "the seeded binder must carry the script-setup display name"
            );
            assert_eq!(
                param_index, 0,
                "the first script-setup generic has ordinal 0"
            );
        }
        other => panic!(
            "`defineProps<T>()` in a `generic=\"T\"` SFC must mirror to a TypeParam binder, \
             NOT a BareRef — got {other:?}"
        ),
    }
    assert!(
        !matches!(
            node_data(&dispatch, handle.node()),
            Some(SemanticNodeData::BareRef(_))
        ),
        "the seeded generic must NOT leak as a BareRef(T)"
    );
}

#[test]
fn script_setup_generic_with_constraint_seeds_typeparam_with_lowered_constraint() {
    let host = host();
    upsert_ts(&host, "/foo.ts", "export type Foo = { x: string };\n");
    upsert_vue(
        &host,
        "/GC.vue",
        "<script setup lang=\"ts\" generic=\"T extends Foo\">\nimport type { Foo } from './foo'\ndefineProps<T>()\n</script>\n<template><div /></template>\n",
    );
    let dispatch = ProjectSemanticDispatch::new(&host);
    let macro_index = first_macro_index(&host, "/GC.vue");

    let handle = macro_type_arg_hot_ref(&host, "/GC.vue", macro_index)
        .expect("the constrained generic macro arg must mirror to a hot ref");
    let root = node_data(&dispatch, handle.node());
    match root {
        Some(SemanticNodeData::TypeParam {
            display_name,
            constraint,
            ..
        }) => {
            assert_eq!(display_name.as_ref(), "T");
            // The constraint lowered (it is `Some`) — a `BareRef(Foo)` carrier
            // by structural lowering.
            let constraint = constraint.expect("`T extends Foo` must lower a constraint node");
            assert!(
                matches!(
                    node_data(&dispatch, constraint),
                    Some(SemanticNodeData::BareRef(_))
                ),
                "the constraint `Foo` must lower to a BareRef carrier on the seeded TypeParam"
            );
        }
        other => panic!("constrained generic must mirror to a TypeParam binder, got {other:?}"),
    }
}

#[test]
fn script_setup_incremental_generic_constraint_resolves_earlier_binder_through_mirror() {
    // `<script setup generic="T, U extends T">` — an INCREMENTAL constraint:
    // `U`'s constraint references the EARLIER generic `T`. Seeding binds `T`
    // before lowering `U`'s constraint, so `extends T` must resolve to the
    // `T` TypeParam binder (NOT a BareRef(T)). `defineProps<U>()`'s `U` must
    // itself lower to its OWN TypeParam binder (ordinal 1), not BareRef(U).
    let host = host();
    upsert_vue(
        &host,
        "/GU.vue",
        "<script setup lang=\"ts\" generic=\"T, U extends T\">\ndefineProps<U>()\n</script>\n<template><div /></template>\n",
    );
    let dispatch = ProjectSemanticDispatch::new(&host);
    let macro_index = first_macro_index(&host, "/GU.vue");

    let handle = macro_type_arg_hot_ref(&host, "/GU.vue", macro_index)
        .expect("the second generic macro arg must mirror to a hot ref");
    let root = node_data(&dispatch, handle.node());
    match root {
        Some(SemanticNodeData::TypeParam {
            display_name,
            param_index,
            constraint,
            ..
        }) => {
            assert_eq!(
                display_name.as_ref(),
                "U",
                "`defineProps<U>()` must mirror to the `U` binder"
            );
            assert_eq!(
                param_index, 1,
                "the second script-setup generic has ordinal 1"
            );
            // DISCRIMINATING: `U extends T` — the constraint must resolve the
            // EARLIER generic `T` to ITS TypeParam binder, proving incremental
            // seeding (earlier generic in scope for a later one's constraint).
            let constraint = constraint.expect("`U extends T` must lower a constraint node");
            match node_data(&dispatch, constraint) {
                Some(SemanticNodeData::TypeParam {
                    display_name: c_name,
                    param_index: c_idx,
                    ..
                }) => {
                    assert_eq!(
                        c_name.as_ref(),
                        "T",
                        "`U extends T`'s constraint must resolve the earlier `T` binder"
                    );
                    assert_eq!(c_idx, 0, "the earlier generic `T` has ordinal 0");
                }
                other => panic!(
                    "`U extends T`'s constraint must resolve `T` to its TypeParam binder \
                     (incremental seeding), NOT a BareRef — got {other:?}"
                ),
            }
        }
        other => panic!(
            "`defineProps<U>()` in a `generic=\"T, U extends T\"` SFC must mirror to a \
             TypeParam binder, NOT a BareRef — got {other:?}"
        ),
    }
    assert!(
        !matches!(
            node_data(&dispatch, handle.node()),
            Some(SemanticNodeData::BareRef(_))
        ),
        "the seeded generic `U` must NOT leak as a BareRef(U)"
    );
}

#[test]
fn script_setup_generic_default_references_earlier_binder_through_mirror() {
    // `<script setup generic="T, U = T">` — a DEFAULT referencing an earlier
    // generic. `U`'s default `= T` must resolve the earlier `T` binder
    // (earlier generic in scope for a later one's default), and
    // `defineProps<U>()`'s `U` must lower to its OWN TypeParam binder.
    let host = host();
    upsert_vue(
        &host,
        "/GD.vue",
        "<script setup lang=\"ts\" generic=\"T, U = T\">\ndefineProps<U>()\n</script>\n<template><div /></template>\n",
    );
    let dispatch = ProjectSemanticDispatch::new(&host);
    let macro_index = first_macro_index(&host, "/GD.vue");

    let handle = macro_type_arg_hot_ref(&host, "/GD.vue", macro_index)
        .expect("the second generic macro arg must mirror to a hot ref");
    let root = node_data(&dispatch, handle.node());
    match root {
        Some(SemanticNodeData::TypeParam {
            display_name,
            param_index,
            default,
            ..
        }) => {
            assert_eq!(display_name.as_ref(), "U");
            assert_eq!(
                param_index, 1,
                "the second script-setup generic has ordinal 1"
            );
            // DISCRIMINATING: `U = T` — the default must resolve the EARLIER
            // generic `T` to its TypeParam binder, proving incremental seeding.
            let default = default.expect("`U = T` must lower a default node");
            match node_data(&dispatch, default) {
                Some(SemanticNodeData::TypeParam {
                    display_name: d_name,
                    param_index: d_idx,
                    ..
                }) => {
                    assert_eq!(
                        d_name.as_ref(),
                        "T",
                        "`U = T`'s default must resolve the earlier `T` binder"
                    );
                    assert_eq!(d_idx, 0, "the earlier generic `T` has ordinal 0");
                }
                other => panic!(
                    "`U = T`'s default must resolve `T` to its TypeParam binder \
                     (incremental seeding), NOT a BareRef — got {other:?}"
                ),
            }
        }
        other => panic!(
            "`defineProps<U>()` in a `generic=\"T, U = T\"` SFC must mirror to a \
             TypeParam binder, NOT a BareRef — got {other:?}"
        ),
    }
}

// ── Laziness / singleflight ─────────────────────────────────────────────────

#[test]
fn publish_lowers_zero_macro_mirrors_then_first_demand_fills_one_cell() {
    let host = host();
    upsert_vue(
        &host,
        "/Z.vue",
        "<script setup lang=\"ts\">\ndefineProps<{ a: string }>()\n</script>\n<template><div /></template>\n",
    );
    // Publishing the artifact (the ensure above) must NOT have produced any
    // mirror handle.
    let indexed = host.ensure_indexed_ready("/Z.vue").expect("indexed");
    assert_eq!(
        indexed.macro_hot_mirror.demanded_count(),
        0,
        "publishing an IndexedReady artifact must lower ZERO macro mirrors"
    );

    let macro_index = first_macro_index(&host, "/Z.vue");
    let h1 = macro_type_arg_hot_ref(&host, "/Z.vue", macro_index).expect("first demand");

    // First demand filled exactly one cell; a second demand returns the SAME
    // handle (singleflight — lowered once).
    let indexed2 = host.ensure_indexed_ready("/Z.vue").expect("indexed");
    assert_eq!(
        indexed2.macro_hot_mirror.demanded_count(),
        1,
        "first demand must fill exactly one macro cell"
    );
    let h2 = macro_type_arg_hot_ref(&host, "/Z.vue", macro_index).expect("second demand");
    assert_eq!(
        h1.node(),
        h2.node(),
        "the mirror must lower the macro arg ONCE"
    );
}

// ── Carrier-head re-entry parity for a barrel re-export (NON-CLOSING smoke) ──
//
// A macro-root carrier flowing through the mirror+dispatch over a barrel
// re-export resolves like the eager path. This is a NON-CLOSING smoke test of
// the mirror flow over a barrel; it does NOT revalidate the latent barrel
// carrier-head equivalence debt (the dedicated latent guard stays green,
// untouched).
#[test]
fn barrel_reexport_macro_arg_flows_through_mirror_smoke() {
    let host = host();
    upsert_ts(&host, "/inner.ts", "export type Inner = { z: string };\n");
    upsert_ts(
        &host,
        "/barrel.ts",
        "export type { Inner } from './inner';\n",
    );
    upsert_vue(
        &host,
        "/B.vue",
        "<script setup lang=\"ts\">\nimport type { Inner } from './barrel'\ndefineProps<Inner>()\n</script>\n<template><div /></template>\n",
    );
    let dispatch = ProjectSemanticDispatch::new(&host);
    let macro_index = first_macro_index(&host, "/B.vue");

    let handle = macro_type_arg_hot_ref(&host, "/B.vue", macro_index)
        .expect("the barrel-reexport macro arg must mirror to a hot ref");
    let root = node_data(&dispatch, handle.node());
    assert!(
        matches!(root, Some(SemanticNodeData::BareRef(_))),
        "the mode-neutral mirror root stays an unresolved BareRef carrier over a barrel, got {root:?}"
    );

    // Re-entry matches eager (the shared dispatch walks the barrel hop).
    let arg = macro_type_arg(&host, "/B.vue", macro_index);
    let via_mirror = mirror_resolved(&dispatch, handle, ProjectionMode::Navigate);
    let macro_owner = macro_owner(&host, "/B.vue", macro_index);
    let via_eager = eager_resolved(
        &dispatch,
        arg.as_ref(),
        "/B.vue",
        macro_owner,
        ProjectionMode::Navigate,
    );
    assert_eq!(
        via_mirror, via_eager,
        "barrel-reexport macro-arg re-entry must match the eager resolution"
    );
}

// ── Namespace-member re-entry parity (NON-CLOSING smoke) ────────────────────
#[test]
fn namespace_member_macro_arg_flows_through_mirror_smoke() {
    let host = host();
    upsert_ts(
        &host,
        "/ns.ts",
        "export namespace NS { export type Member = { q: number } }\n",
    );
    upsert_vue(
        &host,
        "/N.vue",
        "<script setup lang=\"ts\">\nimport { NS } from './ns'\ndefineProps<NS.Member>()\n</script>\n<template><div /></template>\n",
    );
    let dispatch = ProjectSemanticDispatch::new(&host);
    let macro_index = first_macro_index(&host, "/N.vue");

    let handle = macro_type_arg_hot_ref(&host, "/N.vue", macro_index)
        .expect("the namespace-member macro arg must mirror to a hot ref");
    let arg = macro_type_arg(&host, "/N.vue", macro_index);
    let via_mirror = mirror_resolved(&dispatch, handle, ProjectionMode::Navigate);
    let macro_owner = macro_owner(&host, "/N.vue", macro_index);
    let via_eager = eager_resolved(
        &dispatch,
        arg.as_ref(),
        "/N.vue",
        macro_owner,
        ProjectionMode::Navigate,
    );
    assert_eq!(
        via_mirror, via_eager,
        "namespace-member macro-arg re-entry must match the eager resolution"
    );
}

/// LB3 — the macro hot mirror must NOT freeze a transient broken-lease miss as a
/// permanent negative. `build_macro_hot_ref` returns a typed `MacroHotRefOutcome`,
/// so a `transient_macro_type_argument` `LeaseMiss` leaves the write-once mirror
/// slot VACANT, marks the generalized non-cacheability rail, and lets the next
/// demand RETRY — instead of the pre-fix `OnceLock::get_or_init` committing `None`
/// permanently.
///
/// DISCRIMINATING: pin the retained parse-snapshot lease with one successful
/// decl-body demand, break it out-of-band, then demand the macro hot ref. Post-fix
/// the demand marks non-cacheability and leaves the slot vacant (`demanded_count`
/// stays 0), so a SECOND demand RE-RUNS and re-marks. Pre-fix the first demand
/// commits `None` into the `OnceLock` WITHOUT marking (`demanded_count == 1`), and
/// the second demand warm-returns that committed `None` (no re-run, no mark).
#[test]
fn broken_lease_macro_arg_leaves_mirror_slot_vacant_and_marks_non_cacheability() {
    let host = host();
    upsert_vue(
        &host,
        "/L.vue",
        "<script setup lang=\"ts\">\ntype Local = { x: number };\ndefineProps<{ a: string }>()\n</script>\n<template><div /></template>\n",
    );
    let indexed = host
        .ensure_indexed_ready("/L.vue")
        .expect("owner SFC IndexedReady must materialise");
    let macro_index = first_macro_index(&host, "/L.vue");

    // Pin the retained parse-snapshot lease with one successful decl-body demand,
    // then break it so the macro-arg transient demand lease-misses.
    let memo = indexed.shallow_state.decl_bodies();
    // The `<script setup>` local type declares under the setup Instance
    // owner in the owner-aware inventory.
    assert!(
        memo.type_decl_in(verter_type_expr::TopLevelOwnerId::instance(0), "Local",)
            .is_some(),
        "the local type body must lower under a live lease (this pins the retained snapshot)"
    );
    memo.release_retained_snapshot_for_test();

    // 1st demand under the broken lease: returns None, marks non-cacheability, and
    // leaves the mirror slot VACANT (retryable), never a committed permanent None.
    let (h1, rs1) = host.with_fact_tracer(|| macro_type_arg_hot_ref(&host, "/L.vue", macro_index));
    assert!(
        h1.is_none(),
        "a broken-lease macro-arg demand cannot build a hot ref"
    );
    assert!(
        rs1.non_cacheable_read_observed(),
        "the broken-lease macro-arg demand MUST mark the generalized non-cacheability rail — \
         pre-fix build_macro_hot_ref returned None with no mark"
    );
    assert_eq!(
        indexed.macro_hot_mirror.demanded_count(),
        0,
        "the mirror slot must stay VACANT after a LeaseMiss (retryable) — pre-fix get_or_init \
         committed None into the slot (demanded_count == 1), freezing a transient miss as a \
         permanent negative"
    );

    // 2nd demand (lease still broken): the vacant slot RE-RUNS and re-marks — proof
    // the transient miss was not frozen. Pre-fix the committed None short-circuits
    // (no re-run, no mark).
    let (h2, rs2) = host.with_fact_tracer(|| macro_type_arg_hot_ref(&host, "/L.vue", macro_index));
    assert!(h2.is_none(), "still broken lease → still None");
    assert!(
        rs2.non_cacheable_read_observed(),
        "the 2nd macro-arg demand must RE-RUN the vacant slot and re-mark non-cacheability \
         (retry) — pre-fix it warm-returns the committed None WITHOUT re-running or marking"
    );
}

/// LB3 — the CENTRAL `DemandOutcome::into_option` collapse marks the generalized
/// non-cacheability rail on a broken decl-body lease. This is the ONE structural
/// collapse point for the plain type / value / augmentation decl-body accessors
/// (`type_decl` / `value_decl` / augmentation) that the carrier & frontier
/// consumers ride, so a transient `LeaseMiss` consumed by an enclosing traced
/// compute refuses that compute's shared-cache admission.
///
/// DISCRIMINATING: pin the retained lease with one successful demand, break it, then
/// demand a DIFFERENT not-yet-lowered symbol through the plain `type_decl` accessor
/// inside a fact tracer. Post-fix the tracer observed a non-cacheable read; pre-fix
/// (`into_option` collapsed `LeaseMiss` to `None` with no mark) it did not.
#[test]
fn broken_lease_type_decl_accessor_marks_non_cacheability_via_into_option() {
    let host = host();
    upsert_ts(
        &host,
        "/d.ts",
        "export type A = { x: number };\nexport type B = { y: string };\n",
    );
    let indexed = host.ensure_indexed_ready("/d.ts").expect("indexed");
    let memo = indexed.shallow_state.decl_bodies();

    // Pin the retained parse-snapshot lease with A, then break it so a demand for a
    // DIFFERENT not-yet-lowered symbol (B) lease-misses through `into_option`.
    assert!(
        memo.type_decl_in(verter_type_expr::TopLevelOwnerId::ordinary_file(), "A")
            .is_some(),
        "A's body must lower under a live lease (pins the retained snapshot)"
    );
    memo.release_retained_snapshot_for_test();

    let (result, read_set) = host.with_fact_tracer(|| {
        memo.type_decl_in(verter_type_expr::TopLevelOwnerId::ordinary_file(), "B")
    });
    assert!(
        result.is_none(),
        "a broken-lease type_decl demand reads as None (fail-closed)"
    );
    assert!(
        read_set.non_cacheable_read_observed(),
        "the central DemandOutcome::into_option LeaseMiss arm MUST mark the generalized \
         non-cacheability rail so an enclosing traced compute (the carrier / frontier / plain \
         accessor consumers) refuses shared-cache admission — pre-fix into_option collapsed \
         LeaseMiss to None with NO mark"
    );
}

/// SF3 — the per-slot build lock restores the SINGLEFLIGHT guarantee: concurrent
/// FIRST demands of ONE macro's hot ref collapse onto a SINGLE cold build.
///
/// The `OnceLock::get_or_init` → check/build/set rewrite (which restored the
/// vacancy-on-`LeaseMiss` retry semantics) LOST the per-slot singleflight — two
/// threads could both pass the lock-free `committed.get() == None` warm check and
/// both run `build_macro_hot_ref` before one commits. The per-slot `build_lock`
/// re-serialises the cold build while KEEPING the lock-free warm read and the
/// vacancy-on-`LeaseMiss` retry.
///
/// DISCRIMINATING — and DETERMINISTICALLY so: a barrier placed at the entry alone
/// is NOT enough. It releases the threads BEFORE their lock-free warm-miss check,
/// so the scheduler is free to let one thread build and commit while the others are
/// still approaching the check; they then warm-HIT and a lock-free check/build/set
/// slot reports a single lowering — the pre-change code passes and the test proves
/// nothing.
///
/// The rendezvous therefore sits at the POST-WARM-MISS seam inside
/// `macro_type_arg_hot_ref`: every thread must have MISSED the lock-free committed
/// read before ANY of them may proceed, so all N are irrevocably committed to the
/// cold path. From that state the slot's behaviour is fully determined: with the
/// per-slot build lock exactly ONE `build_macro_hot_ref` runs (the rest re-check
/// under the lock and find the commit); with the pre-change check/build/set slot ALL
/// N build. RED-pre the counter is N, never 1.
#[test]
fn concurrent_first_macro_arg_demands_singleflight_one_cold_build() {
    use std::sync::atomic::Ordering;

    let host = host();
    upsert_vue(
        &host,
        "/C.vue",
        "<script setup lang=\"ts\">\ndefineProps<{ a: string; b?: number }>()\n</script>\n<template><div /></template>\n",
    );
    let macro_index = first_macro_index(&host, "/C.vue");
    // Warm the owner's IndexedReady up front so the concurrent burst races the
    // per-slot cold build, not the owner materialisation.
    host.ensure_indexed_ready("/C.vue").expect("owner indexes");
    host.macro_hot_lowering_count.store(0, Ordering::Relaxed);

    const N: usize = 16;
    // The POST-WARM-MISS rendezvous: N threads, released only once every one of them
    // has observed the vacant slot. This is what makes the race deterministic rather
    // than scheduler-dependent.
    *host.test_force.macro_hot_post_warm_miss_barrier.lock() =
        Some(std::sync::Arc::new(std::sync::Barrier::new(N)));
    let handles: Vec<Option<crate::semantic_query::HotTypeRef>> = std::thread::scope(|scope| {
        let joins: Vec<_> = (0..N)
            .map(|_| scope.spawn(|| macro_type_arg_hot_ref(&host, "/C.vue", macro_index)))
            .collect();
        joins.into_iter().map(|j| j.join().unwrap()).collect()
    });
    *host.test_force.macro_hot_post_warm_miss_barrier.lock() = None;

    // Every concurrent demand received the committed hot ref, all identical.
    assert!(
        handles.iter().all(|h| h.is_some()),
        "every concurrent demand must receive the committed hot ref"
    );
    let first = handles[0].expect("first handle").node();
    assert!(
        handles.iter().all(|h| h.expect("handle").node() == first),
        "every concurrent demand must observe the SAME committed node (one lowering)"
    );
    // THE PIN: the per-slot build lock collapsed the whole burst onto ONE cold build.
    assert_eq!(
        host.macro_hot_lowering_count.load(Ordering::Relaxed),
        1,
        "SINGLEFLIGHT: {N} concurrent first demands of one macro must collapse onto ONE \
         `build_macro_hot_ref` — a count > 1 means the per-slot build lock is not serialising \
         the cold build (the `check/build/set` regression double-lowers)"
    );
}
