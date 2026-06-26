//! Architecture-guard tests for the graph-native type-peek primitive
//! and its adoption in the projector pipeline.
//!
//! Each guard is paired: a static structural assertion (anchoring the
//! source-level invariant in a single place) plus, where the invariant
//! is behavioural, a pointer to the discriminating tests in
//! `crates/verter_session/src/meta_resolve/projectors_peek_tests.rs`.
//!
//! The behavioural tests are the load-bearing checks: they discriminate
//! "peek returns the wrong variant", "the package-backed gate is skipped
//! on cached hits", and similar regressions that a pure source grep
//! cannot detect.

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

fn read_projectors_mod() -> String {
    let path = workspace_root().join("crates/verter_session/src/meta_resolve/projectors/mod.rs");
    fs::read_to_string(&path).unwrap_or_else(|_| panic!("could not read {}", path.display()))
}

/// Read the terminal `output_sink` sink module. The boundary-consuming
/// publication functions (`member_shape_peek_or_compute`,
/// `reduce_field_type_expr_with_mode`, `surface_member_to_expanded_field`,
/// `project_model`, `reduce_published_field_types`) live HERE — the only module
/// that touches the projectors reverse-materialization boundary — NOT in the
/// parent `projectors/mod.rs`. The peek primitive (`peek_member_shape_known` /
/// `PeekedShape`) stays in `mod.rs` because it never unwraps a carrier.
fn read_output_sink() -> String {
    let path =
        workspace_root().join("crates/verter_session/src/meta_resolve/projectors/output_sink.rs");
    fs::read_to_string(&path).unwrap_or_else(|_| panic!("could not read {}", path.display()))
}

fn fn_body_slice<'a>(content: &'a str, signature_anchor: &str) -> &'a str {
    let fn_start = content.find(signature_anchor).unwrap_or_else(|| {
        panic!(
            "guard: function with signature anchor `{}` must exist in the read source",
            signature_anchor
        )
    });
    let after_fn = &content[fn_start..];
    let body_end = after_fn[1..]
        .find("\npub(crate) fn ")
        .map(|i| i + 1)
        .or_else(|| after_fn[1..].find("\npub fn ").map(|i| i + 1))
        .or_else(|| after_fn[1..].find("\nfn ").map(|i| i + 1))
        .unwrap_or(after_fn.len());
    &after_fn[..body_end]
}

// ---------------------------------------------------------------------------
// Guard 1: `peek_member_shape_known` exists with the `debug_assert!`
//          enforcing request-bound context. The debug_assert is the
//          load-bearing check: reaching the peek from a bare-host
//          context would force a workspace snapshot rebuild.
// ---------------------------------------------------------------------------
#[test]
fn peek_member_shape_known_exists_with_request_bound_assert() {
    let content = read_projectors_mod();
    assert!(
        content.contains("pub(crate) fn peek_member_shape_known("),
        "guard: `peek_member_shape_known` must exist in \
         `crates/verter_session/src/meta_resolve/projectors/mod.rs` \
         as the type-peek primitive substrate.",
    );

    let body = fn_body_slice(&content, "pub(crate) fn peek_member_shape_known(");
    assert!(
        body.contains("debug_assert!(")
            && body.contains("is_request_bound()")
            && body.contains("peek_member_shape_known invoked from bare-host context"),
        "guard: `peek_member_shape_known` MUST guard entry with \
         `debug_assert!(query_engine.ctx.is_request_bound())`. \
         Reaching the peek from a bare-host context would force a \
         workspace snapshot rebuild.",
    );
}

// ---------------------------------------------------------------------------
// Guard 2: `reduce_field_type_expr` consults peek BEFORE the route gate
//          for the structural short-circuits (Leaf / BareCarrier). The
//          peek call site MUST precede the route gate so primitive /
//          bare-alias inputs return without a workspace lookup. Cached
//          short-circuits run after the route gate (see Guard 2b).
// ---------------------------------------------------------------------------
#[test]
fn reduce_field_type_expr_peeks_before_route_gate() {
    // `reduce_field_type_expr_with_mode` is now SINK-PRIVATE in the terminal
    // `output_sink` module (the only module that unwraps a sealed carrier). The
    // peek-before-gate ordering invariant lives with the body there.
    let content = read_output_sink();
    let body = fn_body_slice(&content, "fn reduce_field_type_expr_with_mode(");

    let peek_idx = body
        .find("peek_member_shape_known(")
        .expect("guard: `reduce_field_type_expr_with_mode` MUST invoke `peek_member_shape_known`.");
    let route_gate_idx = body
        .find("type_expr_has_package_backed_object_like_root(")
        .expect("guard: route gate must remain present in `reduce_field_type_expr_with_mode`.");
    assert!(
        peek_idx < route_gate_idx,
        "guard: `peek_member_shape_known` MUST be invoked BEFORE \
         `type_expr_has_package_backed_object_like_root` in \
         `reduce_field_type_expr_with_mode` so primitive / bare-alias \
         inputs (`PeekedShape::Leaf` / `PeekedShape::BareCarrier`) \
         short-circuit without the workspace-rebuilding route lookup.",
    );
}

// ---------------------------------------------------------------------------
// Guard 2b: `reduce_field_type_expr` ALSO consults the cached-shape
//          variant of the peek AFTER the package-backed gate clears.
//          The cached short-circuit must follow the gate so a warm
//          `MaterializeMemoDb` entry cannot leak a reduced shape past
//          the shallow-by-default gate for shared cache entries.
// ---------------------------------------------------------------------------
#[test]
fn reduce_field_type_expr_consults_cached_peek_after_gate() {
    // Body lives in `reduce_field_type_expr_with_mode`, now sink-private in the
    // terminal `output_sink` module.
    let content = read_output_sink();
    let body = fn_body_slice(&content, "fn reduce_field_type_expr_with_mode(");

    let route_gate_idx = body
        .find("type_expr_has_package_backed_object_like_root(")
        .expect("guard: route gate must remain present.");
    let after_gate = &body[route_gate_idx..];
    assert!(
        after_gate.contains("PeekedShape::Cached(materialized)"),
        "guard: `reduce_field_type_expr_with_mode` MUST re-consult the \
         cached operator-shape peek AFTER the package-backed gate clears. \
         A warm `MaterializeMemoDb` entry can otherwise leak a reduced \
         shape past the shallow-by-default gate for shared cache entries.",
    );
}

// ---------------------------------------------------------------------------
// Guard 3: `member_shape_peek_or_compute` runs the package-backed gate
//          BEFORE the cached operator-shape peek. The gate-before-cache
//          ordering is the load-bearing correctness invariant:
//          `MaterializeMemoDb` is shared across the typed-IR materialiser
//          callers (model / registry paths) that do NOT apply the
//          projector's shallow gate, so consulting the cache first would
//          publish the reduced body for a package-backed root —
//          violating the shallow-by-default invariant.
// ---------------------------------------------------------------------------
#[test]
fn member_shape_peek_or_compute_runs_gates_before_cached_peek() {
    // `member_shape_peek_or_compute` now lives IN the terminal `output_sink`
    // sink module (the only module that touches the reverse boundary), so the
    // raise step calls the now-MODULE-PRIVATE `shell_raise_to_type_expr`
    // primitive directly (same-module — no `output_sink::` prefix).
    let content = read_output_sink();
    let body = fn_body_slice(&content, "fn member_shape_peek_or_compute(");

    // The raise-to-TypeExpr step goes through the sink-private boundary
    // primitive `shell_raise_to_type_expr(&dispatch, member_value)`. Match the
    // primitive call, with defensive fallbacks to the older inline boundary-fn
    // spellings so the guard survives a future reshape; the ANCHOR is "the
    // raise-to-TypeExpr step" whatever its current spelling.
    let raise_idx = body
        .find("shell_raise_to_type_expr(&dispatch, member_value)")
        .or_else(|| body.find("shell_raise_to_type_expr("))
        .or_else(|| body.find("materialize_output_type_expr(member_value)"))
        .or_else(|| body.find("raise_node_to_type_expr(member_value)"))
        .expect(
            "guard: the output raise step (shell_raise_to_type_expr(&dispatch, \
             member_value)) must remain in `member_shape_peek_or_compute`.",
        );
    // The gate is invoked via the `_with_fence` variant
    // so the gate's cross-file dep facts thread into the admit's
    // `fact_dep_signature`. Match either the bare or the `_with_fence`
    // form so the guard survives a hypothetical
    // future rename back. Document the intent: do NOT remove the gate
    // entirely; rename only to a fence-bearing successor.
    let gate_idx = body
        .find("type_expr_has_package_backed_object_like_root_with_fence(")
        .or_else(|| body.find("type_expr_has_package_backed_object_like_root("))
        .expect("guard: package-backed gate must remain as cold fallback.");
    let peek_idx = body
        .find("peek_member_shape_known(")
        .expect("guard: `member_shape_peek_or_compute` MUST invoke `peek_member_shape_known`.");

    assert!(
        raise_idx < gate_idx,
        "guard: the package-backed gate must follow the raise step so \
         it operates on the raised TypeExpr.",
    );
    assert!(
        gate_idx < peek_idx,
        "guard: the package-backed gate \
         (`type_expr_has_package_backed_object_like_root`) MUST run \
         BEFORE the `peek_member_shape_known` cached-shape short-circuit. \
         `MaterializeMemoDb` is shared across non-projector callers \
         that do not apply this shallow gate; consulting the cache \
         first would publish the reduced body for a package-backed \
         root, violating the shallow-by-default invariant.",
    );
}

// ---------------------------------------------------------------------------
// Guard 4: `PeekedShape` enum carries exactly three documented variants
//          (`Leaf`, `BareCarrier`, `Cached`). Each variant covers a
//          structural peek outcome the projector relies on.
// ---------------------------------------------------------------------------
#[test]
fn peeked_shape_enum_has_three_variants() {
    let content = read_projectors_mod();
    assert!(
        content.contains("pub(crate) enum PeekedShape {"),
        "guard: `PeekedShape` enum must exist as the type-peek return value.",
    );
    assert!(
        content.contains("BareCarrier {"),
        "guard: `PeekedShape::BareCarrier` variant must exist for bare-alias inputs.",
    );
    assert!(
        content.contains("Leaf("),
        "guard: `PeekedShape::Leaf` variant must exist for primitive / literal inputs.",
    );
    assert!(
        content.contains("Cached("),
        "guard: `PeekedShape::Cached` variant must exist for warm \
         `MaterializeMemoDb` operator-shape hits.",
    );
}

// ---------------------------------------------------------------------------
// Guard 5: `peek_member_shape_known` does NOT consult `RouteDb` or
//          `OwnerImportSurfaceDb` (those rebuild HostStoreView and
//          defeat the peek's cost-elimination purpose) but DOES consult
//          `MaterializeMemoDb` for operator-shape lookups.
// ---------------------------------------------------------------------------
#[test]
fn peek_does_not_consult_route_db_or_owner_import_surface_db() {
    let content = read_projectors_mod();
    let body = fn_body_slice(&content, "pub(crate) fn peek_member_shape_known(");

    assert!(
        !body.contains("route_db("),
        "guard: `peek_member_shape_known` MUST NOT consult `RouteDb` — \
         RouteDb would force a workspace snapshot rebuild, defeating \
         the peek's cost-elimination purpose.",
    );
    assert!(
        !body.contains("owner_import_surface_db("),
        "guard: `peek_member_shape_known` MUST NOT consult \
         `OwnerImportSurfaceDb` for the same workspace-rebuild reason.",
    );
    assert!(
        body.contains("shape_cache_db("),
        "guard: `peek_member_shape_known` MUST consult `ShapeCacheDb` \
         (the universal cache, replaces `MaterializeMemoDb`) \
         as the operator-shape lookup substrate.",
    );
}

// ---------------------------------------------------------------------------
// Guard 6: discriminating behavioural tests for the peek primitive's
//          per-shape semantics live in `projectors_peek_tests.rs`.
//          Source-text greps in this file are anchoring tests; the
//          load-bearing per-variant discrimination is in the behavioural
//          module. This guard pins the behavioural-test module's
//          presence and a stable subset of its test names.
// ---------------------------------------------------------------------------
#[test]
fn projector_peek_behavioural_tests_present() {
    let path =
        workspace_root().join("crates/verter_session/src/meta_resolve/projectors_peek_tests.rs");
    assert!(
        path.exists(),
        "guard: discriminating behavioural tests for `peek_member_shape_known` \
         MUST exist at {}. The peek primitive's per-variant semantics \
         cannot be discriminated by source-text greps alone.",
        path.display()
    );

    let test_module =
        fs::read_to_string(&path).unwrap_or_else(|_| panic!("could not read {}", path.display()));
    let expected_test_names = [
        "peek_primitive_returns_leaf",
        "peek_bare_ref_returns_bare_carrier",
        "peek_operator_shape_cold_memo_returns_none",
        "peek_operator_shape_warm_memo_returns_cached",
    ];
    for name in expected_test_names {
        assert!(
            test_module.contains(name),
            "guard: behavioural test `{}` must exist in projectors_peek_tests.rs. \
             Removing or renaming the test would erase the discrimination \
             between the peek primitive's per-variant semantics.",
            name,
        );
    }
}
