//! Isolation tests for the extracted shared binder-frame builder
//! ([`super::build_script_setup_seed_frames`]).
//!
//! These call the EXTRACTED helper DIRECTLY (not through the macro hot mirror)
//! to prove the shared `pub(in crate::structural_carrier_producer)` entry — the
//! helper the mirror's macro-arg builder builds the seed binder shape from —
//! produces the correct binder shape. Each lowers a bare `Ref` through the
//! returned frame and
//! asserts it resolves to the script-setup `TypeParam` binder rather than an
//! unbound `BareRef` — the exact contract a `<script setup generic="…">` SFC's
//! open generics depend on.

use std::sync::Arc;

use verter_type_expr::TypeExpr;

use super::build_script_setup_seed_frames;
use crate::semantic_query::{NodeScopeId, SemanticNodeData, SemanticNodeId};
use crate::structural_carrier_producer::lower::{self, BinderScope, StructuralLowerContext};
use crate::structural_carrier_producer::macro_surface::MacroProducerWitness;
use crate::types::HostConfig;
use crate::{FileLanguage, UpsertRequest, VerterHost};

fn host() -> VerterHost {
    VerterHost::new_standalone(HostConfig::default())
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

/// Lower a bare `Ref(name)` through `frames` and return the interned node's
/// data — the binder lookup is the only "resolution" the structural lowerer
/// performs, so a name bound in `frames` resolves to its `TypeParam` node and
/// an unbound name lowers to a `BareRef`.
fn lower_ref_through(
    host: &VerterHost,
    frames: &[BinderScope],
    scope: &NodeScopeId,
    name: &str,
) -> SemanticNodeData {
    let graph = host.project_type_store().semantic_graph();
    let expr = TypeExpr::Ref {
        name: Arc::from(name),
        type_arguments: Arc::from(Vec::new()),
    };
    let ctx = StructuralLowerContext::new(frames);
    let handle = lower::emit_macro_arg(
        graph,
        &expr,
        scope.clone(),
        &ctx,
        &MacroProducerWitness::new(),
    )
    .expect("a bare Ref must lower structurally");
    let node: SemanticNodeId = handle.node();
    (*graph
        .node_data(node)
        .expect("the lowered ref node must be interned"))
    .clone()
}

#[test]
fn extracted_builder_seeds_typeparam_binders_for_script_setup_generics() {
    let host = host();
    // A `<script setup generic="T, U extends T">` SFC: two generics, the second
    // constrained by the first (incremental scoping).
    upsert_vue(
        &host,
        "/G.vue",
        "<script setup lang=\"ts\" generic=\"T, U extends T\">\n\
         defineProps<{ x: T; y: U }>()\n\
         </script>\n\
         <template><div /></template>\n",
    );

    let indexed = host
        .ensure_indexed_ready("/G.vue")
        .expect("the SFC must materialise an IndexedReady");
    let graph = host.project_type_store().semantic_graph();
    let scope = NodeScopeId::File {
        canonical_id: Arc::from("/G.vue"),
        whole_hash: indexed.whole_hash,
        local_scope: None,
    };

    // Call the EXTRACTED helper directly — the shared
    // `pub(in crate::structural_carrier_producer)` entry the mirror's macro-arg
    // builder builds the seed binder shape from (the decl-body structural
    // producer would build the same shape but does NOT call it today).
    let frames = build_script_setup_seed_frames(&indexed, graph, &scope);
    assert_eq!(
        frames.len(),
        1,
        "a non-empty script-setup generic clause seeds exactly one binder frame"
    );

    // `T` resolves to its TypeParam binder (ordinal 0), NOT a BareRef.
    match lower_ref_through(&host, &frames, &scope, "T") {
        SemanticNodeData::TypeParam {
            display_name,
            param_index,
            ..
        } => {
            assert_eq!(display_name.as_ref(), "T");
            assert_eq!(param_index, 0, "the first generic has ordinal 0");
        }
        other => panic!("`T` must resolve to its script-setup TypeParam binder, got {other:?}"),
    }

    // `U` resolves to its TypeParam binder (ordinal 1) with a lowered
    // constraint (it `extends T`) — the incremental-scoping contract.
    match lower_ref_through(&host, &frames, &scope, "U") {
        SemanticNodeData::TypeParam {
            display_name,
            param_index,
            constraint,
            ..
        } => {
            assert_eq!(display_name.as_ref(), "U");
            assert_eq!(param_index, 1, "the second generic has ordinal 1");
            assert!(
                constraint.is_some(),
                "`U extends T` must lower a constraint node (the earlier binder is in scope)"
            );
        }
        other => panic!("`U` must resolve to its script-setup TypeParam binder, got {other:?}"),
    }

    // DISCRIMINATING: an unbound name lowers to a BareRef (proving the binders
    // above are the seed frame's doing, not a global default).
    assert!(
        matches!(
            lower_ref_through(&host, &frames, &scope, "Unbound"),
            SemanticNodeData::BareRef(_)
        ),
        "an unbound name must lower to a BareRef, not a TypeParam binder"
    );
}

#[test]
fn extracted_builder_returns_empty_frames_without_script_setup_generics() {
    let host = host();
    // No `generic="…"` clause — the builder seeds no binder frame.
    upsert_vue(
        &host,
        "/Plain.vue",
        "<script setup lang=\"ts\">\ndefineProps<{ x: number }>()\n</script>\n<template><div /></template>\n",
    );

    let indexed = host
        .ensure_indexed_ready("/Plain.vue")
        .expect("the SFC must materialise an IndexedReady");
    let graph = host.project_type_store().semantic_graph();
    let scope = NodeScopeId::File {
        canonical_id: Arc::from("/Plain.vue"),
        whole_hash: indexed.whole_hash,
        local_scope: None,
    };

    let frames = build_script_setup_seed_frames(&indexed, graph, &scope);
    assert!(
        frames.is_empty(),
        "an SFC with no `generic=\"…\"` clause seeds NO binder frame, got {} frame(s)",
        frames.len()
    );
}
