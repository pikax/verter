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

/// The macro's `parsed_type_argument` (owned clone) for the eager-parity arm.
fn macro_type_arg(host: &VerterHost, canonical: &str, macro_index: usize) -> Arc<TypeExpr> {
    let indexed = host.ensure_indexed_ready(canonical).expect("indexed");
    let script = indexed.script_analysis.as_ref().expect("script analysis");
    Arc::clone(
        script.macros[macro_index]
            .parsed_type_argument
            .as_ref()
            .expect("type-based macro carries a parsed_type_argument"),
    )
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
    mode: ProjectionMode,
) -> SemanticNodeId {
    let lowered = dispatch
        .lower_type_expr_in_scope_with_context(
            canonical,
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
    let via_eager = eager_resolved(&dispatch, arg.as_ref(), "/C.vue", ProjectionMode::Navigate);
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
    let via_eager = eager_resolved(&dispatch, arg.as_ref(), "/B.vue", ProjectionMode::Navigate);
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
    let via_eager = eager_resolved(&dispatch, arg.as_ref(), "/N.vue", ProjectionMode::Navigate);
    assert_eq!(
        via_mirror, via_eager,
        "namespace-member macro-arg re-entry must match the eager resolution"
    );
}
