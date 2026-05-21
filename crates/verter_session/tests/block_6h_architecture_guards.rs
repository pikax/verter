//! Block 6.h architecture-guard tests.
//!
//! These guards pin the architectural decisions Block 6.h enforces:
//!  1. `peek_member_shape_known` exists in `meta_resolve::projectors`
//!     and is annotated with `debug_assert!(is_request_bound)`. The
//!     primitive is the Rule-6 enforcement substrate.
//!  2. `reduce_field_type_expr` consults `peek_member_shape_known`
//!     before the existing package-backed gate / cycle guard / reducer.
//!  3. `member_shape_peek_or_compute` consults `peek_member_shape_known`
//!     between the raise and the shallow gates.
//!
//! Each guard greps the relevant file for both the expected production
//! shape AND the surrounding context anchors. A future commit cannot
//! silently revert the peek adoption without failing one of these
//! tests.

use std::fs;
use std::path::PathBuf;

fn workspace_root() -> PathBuf {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    // CARGO_MANIFEST_DIR is `crates/verter_session/`; the workspace
    // root is two parents up.
    manifest_dir
        .parent()
        .and_then(|p| p.parent())
        .map(|p| p.to_path_buf())
        .unwrap_or(manifest_dir)
}

fn read_projectors_mod() -> String {
    let path = workspace_root().join("crates/verter_session/src/meta_resolve/projectors/mod.rs");
    fs::read_to_string(&path).unwrap_or_else(|_| panic!("could not read {}", path.display()))
}

// ---------------------------------------------------------------------------
// Guard 1: `peek_member_shape_known` exists with the `debug_assert!`
//          on `is_request_bound`.
// ---------------------------------------------------------------------------
#[test]
fn peek_member_shape_known_exists_with_request_bound_assert() {
    let content = read_projectors_mod();
    assert!(
        content.contains("pub(crate) fn peek_member_shape_known("),
        "Block 6.h Commit A guard: `peek_member_shape_known` must exist \
         in `crates/verter_session/src/meta_resolve/projectors/mod.rs` \
         as the Rule-6 type-peek primitive substrate.",
    );
    assert!(
        content.contains("debug_assert!(")
            && content.contains("is_request_bound()")
            && content.contains("peek_member_shape_known invoked from bare-host context"),
        "Block 6.h Commit A guard: `peek_member_shape_known` MUST guard \
         entry with `debug_assert!(query_engine.ctx.is_request_bound())`. \
         Reaching the peek from a bare-host context would force a \
         workspace snapshot rebuild — the cost driver Block 6.g closed.",
    );
}

// ---------------------------------------------------------------------------
// Guard 2: `reduce_field_type_expr` adopts `peek_member_shape_known`
//          BEFORE the package-backed-route gate.
// ---------------------------------------------------------------------------
#[test]
fn reduce_field_type_expr_peeks_before_route_gate() {
    let content = read_projectors_mod();
    // Locate the start of the function body.
    let fn_start = content
        .find("pub(crate) fn reduce_field_type_expr(")
        .expect("reduce_field_type_expr must exist in projectors/mod.rs");
    // Slice from the function start to the next top-level function
    // (`/// ` doc-comment opening at column 0).
    let after_fn = &content[fn_start..];
    // Find the next function definition or end of file.
    let body_end = after_fn[1..]
        .find("\npub(crate) fn ")
        .map(|i| i + 1)
        .or_else(|| after_fn[1..].find("\npub fn ").map(|i| i + 1))
        .unwrap_or(after_fn.len());
    let body = &after_fn[..body_end];

    let peek_idx = body.find("peek_member_shape_known(").expect(
        "Block 6.h Commit B guard: `reduce_field_type_expr` MUST \
                 invoke `peek_member_shape_known` before the route gate.",
    );
    let route_gate_idx = body
        .find("type_expr_has_package_backed_object_like_root(")
        .expect(
            "Block 6.h guard: route gate `type_expr_has_package_backed_object_like_root` \
                 must remain present in `reduce_field_type_expr` as the cold fallback.",
        );
    assert!(
        peek_idx < route_gate_idx,
        "Block 6.h Commit B guard: `peek_member_shape_known` MUST be \
         invoked BEFORE `type_expr_has_package_backed_object_like_root` \
         in `reduce_field_type_expr` — peek-before-reduce eliminates \
         the workspace lookup cost on warm operator-shape hits.",
    );
}

// ---------------------------------------------------------------------------
// Guard 3: `member_shape_peek_or_compute` adopts `peek_member_shape_known`
//          AFTER the raise. The package-backed gate runs BEFORE the
//          cached-peek short-circuit so the cache (shared with the
//          non-projector materialiser callers) cannot publish a reduced
//          shape past the shallow gate.
// ---------------------------------------------------------------------------
#[test]
fn member_shape_peek_or_compute_runs_gates_before_cached_peek() {
    let content = read_projectors_mod();
    let fn_start = content
        .find("fn member_shape_peek_or_compute(")
        .expect("member_shape_peek_or_compute must exist in projectors/mod.rs");
    let after_fn = &content[fn_start..];
    let body_end = after_fn[1..]
        .find("\npub(crate) fn ")
        .map(|i| i + 1)
        .or_else(|| after_fn[1..].find("\nfn ").map(|i| i + 1))
        .unwrap_or(after_fn.len());
    let body = &after_fn[..body_end];

    let raise_idx = body
        .find("raise_node_to_type_expr(member_value)")
        .expect("Block 6.h guard: raise call must remain in member_shape_peek_or_compute");
    let gate_idx = body
        .find("type_expr_has_package_backed_object_like_root(")
        .expect("Block 6.h guard: package-backed gate must remain as cold fallback");
    let peek_idx = body.find("peek_member_shape_known(").expect(
        "Block 6.h Commit C guard: `member_shape_peek_or_compute` MUST \
                 invoke `peek_member_shape_known` after the raise.",
    );

    assert!(
        raise_idx < gate_idx,
        "guard: package-backed gate must follow the raise so the gate \
         operates on the raised TypeExpr.",
    );
    assert!(
        gate_idx < peek_idx,
        "Block 6.h fix-cycle guard: the package-backed gate \
         (`type_expr_has_package_backed_object_like_root`) MUST run \
         BEFORE the `peek_member_shape_known` cached-shape short-circuit. \
         `MaterializeMemoDb` is shared across the typed-IR materialiser \
         callers (model / registry paths) that do not apply this \
         shallow gate; consulting the cache first would publish the \
         reduced body for a package-backed root, violating the \
         shallow-by-default invariant.",
    );
}

// ---------------------------------------------------------------------------
// Guard 4: `PeekedShape` enum carries the three documented variants.
// ---------------------------------------------------------------------------
#[test]
fn peeked_shape_enum_has_three_variants() {
    let content = read_projectors_mod();
    assert!(
        content.contains("pub(crate) enum PeekedShape {"),
        "PeekedShape enum must exist",
    );
    // The three variants are Leaf, BareCarrier, Cached. The brief
    // architecture-guarantee is that all three remain.
    assert!(
        content.contains("BareCarrier {"),
        "PeekedShape::BareCarrier variant must exist",
    );
    assert!(
        content.contains("Leaf("),
        "PeekedShape::Leaf variant must exist",
    );
    assert!(
        content.contains("Cached("),
        "PeekedShape::Cached variant must exist",
    );
}

// ---------------------------------------------------------------------------
// Guard 5: `peek_member_shape_known` does NOT consult RouteDb,
//          OwnerImportSurfaceDb (those rebuild HostStoreView).
// ---------------------------------------------------------------------------
#[test]
fn peek_does_not_consult_route_db_or_owner_import_surface_db() {
    let content = read_projectors_mod();
    let fn_start = content
        .find("pub(crate) fn peek_member_shape_known(")
        .expect("peek_member_shape_known must exist");
    let after_fn = &content[fn_start..];
    let body_end = after_fn[1..]
        .find("\npub(crate) fn ")
        .map(|i| i + 1)
        .or_else(|| after_fn[1..].find("\nfn ").map(|i| i + 1))
        .unwrap_or(after_fn.len());
    let body = &after_fn[..body_end];

    assert!(
        !body.contains("route_db("),
        "Block 6.h Commit A guard: `peek_member_shape_known` MUST NOT \
         consult `RouteDb` — RouteDb would force a workspace snapshot \
         rebuild, defeating the peek's cost-elimination purpose.",
    );
    assert!(
        !body.contains("owner_import_surface_db("),
        "Block 6.h Commit A guard: `peek_member_shape_known` MUST NOT \
         consult `OwnerImportSurfaceDb` for the same workspace-rebuild \
         reason.",
    );
    assert!(
        body.contains("materialize_memo_db("),
        "Block 6.h Commit A guard: `peek_member_shape_known` MUST \
         consult `MaterializeMemoDb` for operator-shape lookups.",
    );
}
