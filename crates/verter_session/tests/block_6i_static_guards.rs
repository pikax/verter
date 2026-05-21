//! Block 6.i — static architecture guards.
//!
//! Companion to `block_6i_runtime_arch_guards.rs`. These are cheap
//! source-text scans that catch regressions on the architectural
//! invariants Commits A → F establish at the projector / registry /
//! cache / NAPI boundaries.
//!
//! Each guard is named after the commit that introduces it; the
//! commit that lands the rule MUST also un-ignore the guard.

use std::fs;
use std::path::PathBuf;

fn workspace_root() -> PathBuf {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest_dir
        .parent()
        .and_then(|p| p.parent())
        .map(|p| p.to_path_buf())
        .unwrap_or(manifest_dir)
}

fn read_workspace_file(rel: &str) -> String {
    fs::read_to_string(workspace_root().join(rel)).unwrap_or_else(|e| panic!("read {rel}: {e}"))
}

// ---------------------------------------------------------------------------
// Guard A.1 — `collect_component_meta_registry_refs` accepts a
//             `ProjectionCursor` parameter.
//
// Block 6.i Commit A introduces the path-precise projection demand
// substrate (`crates/verter_session/src/meta_resolve/projection_demand.rs`).
// The registry walker MUST thread this cursor through its recursive
// descent — landing the signature is the load-bearing structural
// change. Without it, post-A path-precision is impossible.
// ---------------------------------------------------------------------------
#[test]
fn collect_component_meta_registry_refs_requires_cursor() {
    let src =
        read_workspace_file("crates/verter_session/src/resolver_core/component_meta_registry.rs");

    // Locate the function signature.
    let signature_idx = src
        .find("pub(crate) fn collect_component_meta_registry_refs(")
        .expect(
            "guard A.1: function `collect_component_meta_registry_refs` must exist in \
             `crates/verter_session/src/resolver_core/component_meta_registry.rs`",
        );

    // Bound the search to the signature header (between `(` and `)`).
    let header_start = signature_idx;
    let header_end = src[header_start..]
        .find(") {")
        .map(|i| header_start + i)
        .expect("guard A.1: function signature must close with `) {`");
    let header = &src[header_start..header_end];

    assert!(
        header.contains("ProjectionCursor"),
        "guard A.1: `collect_component_meta_registry_refs` MUST accept a \
         `ProjectionCursor<'_>` parameter in its signature so Block 6.i \
         Commit A's path-precise registry walk threads through. Header:\n{header}",
    );

    // Soft-check: the parameter name `cursor` appears so callers can
    // rely on a consistent forwarding name.
    assert!(
        header.contains("cursor: ") || header.contains("_cursor: "),
        "guard A.1: registry walker's projection-cursor parameter should be named \
         `cursor` (or `_cursor` if temporarily unused). Header:\n{header}",
    );
}

// ---------------------------------------------------------------------------
// Guard A.2 — `projection_demand` module exists with the substrate types.
//
// `SurfaceProjection`, `ProjectionNode`, `KeyFilter`, `PathSegment`
// (re-used from `semantic_query`), `ProjectionCursor`,
// `PublishedSurfaceKind` are the Block 6.i Commit A architectural
// vocabulary. The module being present + naming all of these is the
// minimal contract that subsequent commits depend on.
// ---------------------------------------------------------------------------
#[test]
fn projection_demand_substrate_present() {
    let src = read_workspace_file("crates/verter_session/src/meta_resolve/projection_demand.rs");

    for symbol in [
        "pub(crate) struct SurfaceProjection",
        "pub(crate) struct ProjectionNode",
        "pub(crate) enum KeyFilter",
        "pub(crate) enum PublishedSurfaceKind",
        "pub(crate) struct ProjectionCursor",
        "pub(crate) fn descend",
        "pub(crate) fn is_terminal",
        "pub(crate) fn admits_key",
    ] {
        assert!(
            src.contains(symbol),
            "guard A.2: `projection_demand` module MUST declare `{symbol}`",
        );
    }
}

// ---------------------------------------------------------------------------
// Guard B.1 — `ShapeCacheDb` is the unified shape cache.
//
// Block 6.i Commit B replaces the previously-split `MaterializeMemoDb`
// (TypeExpr-keyed) and `MemberShapeCacheDb` (SemanticNode-keyed)
// with a single `ShapeCacheDb` whose key carries a `ShapeSubject`
// discriminant + a `ShapeDemand` (path + mode + filter + surface).
// One cache, not two. Source-text guard asserts the structural shape
// landed.
// ---------------------------------------------------------------------------
#[test]
fn shape_cache_db_replaces_split_caches() {
    let src = read_workspace_file("crates/verter_session/src/component_meta_caches.rs");

    for symbol in [
        "pub struct ShapeCacheDb",
        "pub struct ShapeCacheEntry",
        "pub enum ShapeSubject",
        "pub struct ShapeDemand",
        "pub struct ShapeCacheKey",
    ] {
        assert!(
            src.contains(symbol),
            "guard B.1: `component_meta_caches.rs` MUST declare `{symbol}` — Block 6.i \
             Commit B universal-cache architectural contract.",
        );
    }

    // The legacy split caches MUST be retired (no public surface).
    for retired in [
        "pub struct MaterializeMemoDb",
        "pub struct MemberShapeCacheDb",
    ] {
        assert!(
            !src.contains(retired),
            "guard B.1: legacy split-cache type `{retired}` MUST be retired in Block \
             6.i Commit B — replaced by `ShapeCacheDb`.",
        );
    }
}

// ---------------------------------------------------------------------------
// Guard B.2 — peek primitive's `Leaf` / `BareCarrier` arms admit to
//             the universal cache.
//
// Block 6.i universal-caching invariant (codex STOP trigger #3):
// every successful shape compute admits, regardless of how cheap the
// compute was. The peek primitive's `Leaf` and `BareCarrier` arms in
// `reduce_field_type_expr` and the gate-short-circuit arms in
// `member_shape_peek_or_compute` MUST route through
// `admit_type_expr_shape_if_possible` / `admit_member_shape_if_possible`.
// ---------------------------------------------------------------------------
#[test]
fn peek_primitive_arms_admit_to_cache() {
    let src = read_workspace_file("crates/verter_session/src/meta_resolve/projectors/mod.rs");

    // The admission helpers must exist.
    assert!(
        src.contains("fn admit_type_expr_shape_if_possible"),
        "guard B.2: `meta_resolve::projectors::mod` MUST define \
         `admit_type_expr_shape_if_possible` — the Block 6.i universal-caching admission \
         helper for the `reduce_field_type_expr` peek's `Leaf` / `BareCarrier` arms.",
    );
    assert!(
        src.contains("fn admit_member_shape_if_possible"),
        "guard B.2: `meta_resolve::projectors::mod` MUST define \
         `admit_member_shape_if_possible` — the Block 6.i universal-caching admission \
         helper for `member_shape_peek_or_compute`'s gate-short-circuit + `Leaf` / \
         `BareCarrier` arms.",
    );

    // Every successful shape outcome must be wrapped in an admission
    // helper, not returned bare. Source-text grep on the call count
    // gives a coarse but discriminating signal.
    let admit_type_calls = src.matches("admit_type_expr_shape_if_possible(").count();
    assert!(
        admit_type_calls >= 2,
        "guard B.2: `admit_type_expr_shape_if_possible` MUST be called at least twice \
         (one for the `Leaf` arm, one for the `BareCarrier` arm of `reduce_field_type_expr`'s \
         peek). Observed call count: {admit_type_calls}.",
    );
    let admit_member_calls = src.matches("admit_member_shape_if_possible(").count();
    assert!(
        admit_member_calls >= 4,
        "guard B.2: `admit_member_shape_if_possible` MUST be called at least four times \
         (package-backed gate, cycle gate, non-reducible shape arm, peek `Leaf` arm, peek \
         `BareCarrier` arm) inside `member_shape_peek_or_compute`. Observed call count: \
         {admit_member_calls}.",
    );
}

// ---------------------------------------------------------------------------
// Guard F.1 — `PathWalker` does not resolve `Mapped` through
//             `build_mapped_type` when a literal-keyed path is
//             available (operator-level Mapped narrowing).
//
// Block 6.i Commit F's path-walker narrowing closes the
// `Tool<INPUT, OUTPUT>['outputSchema']` leak by substituting K = path
// segment directly into `mapper.value_expr` and evaluating, rather
// than dispatching `SemanticQueryKey::MappedType` (which would
// enumerate every key in the source surface and emit per-key
// `ProjectMember` edges into the audit footprint regardless of
// consumer demand).
//
// The guard scans the `PathWalker` Mapped arm (`walk.rs`'s
// `SemanticNodeData::Mapped` match) and asserts:
// 1. A per-key substitute path exists (calls
//    `substitute_semantic_type_param` + `evaluate_deferred_semantic_node`
//    on the mapper's `value_expr` BEFORE the fall-through
//    `MappedType` dispatch).
// 2. The fall-through `MappedType` dispatch is gated on the absence
//    of a narrowable literal segment (a `can_narrow`-style predicate
//    or equivalent control-flow guard).
// ---------------------------------------------------------------------------
#[test]
fn pathwalker_does_not_resolve_mapped_through_build_mapped_type() {
    let src = read_workspace_file("crates/verter_session/src/project_semantic_dispatch/walk.rs");

    // Locate the `SemanticNodeData::Mapped` arm inside `PathWalker::advance_step`.
    let mapped_arm_idx = src
        .find("SemanticNodeData::Mapped { source, mapper }")
        .expect("guard F.1: `PathWalker`'s `SemanticNodeData::Mapped` arm must exist in walk.rs");

    // Bound the search to the arm body (until the next outer
    // `SemanticNodeData::` match arm at the same indentation level).
    // The Mapped arm ends before the next `SemanticNodeData::` arm,
    // which in this file is `SemanticNodeData::TypeOf`.
    let arm_end_idx = src[mapped_arm_idx..]
        .find("SemanticNodeData::TypeOf")
        .map(|i| mapped_arm_idx + i)
        .expect("guard F.1: TypeOf arm must follow the Mapped arm in walk.rs");
    let arm_body = &src[mapped_arm_idx..arm_end_idx];

    // (1) The narrowing path must substitute + evaluate
    // mapper.value_expr per the brief's Commit F contract.
    assert!(
        arm_body.contains("substitute_semantic_type_param"),
        "guard F.1: `PathWalker`'s Mapped arm MUST substitute the mapper's parameter via \
         `substitute_semantic_type_param` for per-key path-precision (Block 6.i Commit F \
         operator-level narrowing). The MappedType dispatch alone enumerates every key and \
         leaks `Tool<INPUT, OUTPUT>['outputSchema']`-shaped queries into the audit \
         footprint.",
    );
    assert!(
        arm_body.contains("evaluate_deferred_semantic_node"),
        "guard F.1: `PathWalker`'s Mapped arm MUST evaluate the substituted node via \
         `evaluate_deferred_semantic_node` so the per-key value resolves without \
         enumerating the whole mapped surface.",
    );

    // (2) The narrowing must inspect a remaining path segment + the
    // mapper's `name_remap` field. Without these, the per-key
    // substitution is unsound (a post-remap surface name does not
    // index directly back to the iteration key).
    assert!(
        arm_body.contains("name_remap.is_none()"),
        "guard F.1: `PathWalker`'s Mapped narrowing MUST gate on \
         `mapper.name_remap.is_none()`: when the mapper carries an `as <expr>` clause, the \
         iteration key is NOT the post-remap surface name and per-key substitution would \
         project the wrong value. The whole-surface MappedType fallback is correct in that \
         case.",
    );
    assert!(
        arm_body.contains("PathSegment::Member") && arm_body.contains("PathSegment::Index"),
        "guard F.1: `PathWalker`'s Mapped narrowing MUST handle both `PathSegment::Member` \
         and `PathSegment::Index` literal keys.",
    );

    // (3) The walker MUST still record a `ProjectMember` or
    // `ProjectIndex` edge for the narrowed step so downstream origin
    // graph consumers see the per-key contribution.
    assert!(
        arm_body.contains("OriginEdgeKind::ProjectMember")
            || arm_body.contains("OriginEdgeKind::ProjectIndex"),
        "guard F.1: `PathWalker`'s Mapped narrowing MUST emit a `ProjectMember` (or \
         `ProjectIndex`) edge on the per-key step so the origin graph mirrors the \
         pre-Commit-F path-walker behaviour for the narrowed key.",
    );
}
