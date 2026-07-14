//! Architecture guards for graph-native member-shape cache admission.
//!
//! These guards pin warm-hit ordering, cache-admission ordering, and the
//! package-backed shallow gate around the single graph-native reducer.

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

/// Read the terminal `output_sink` sink module. The boundary-consuming
/// publication functions (`member_shape_peek_or_compute`,
/// `reduce_field_value_node`, `surface_member_to_expanded_field`,
/// `project_model`, `reduce_published_field_types`) live HERE — the only module
/// that touches the projectors reverse-materialization boundary — NOT in the
/// parent `projectors/mod.rs`.
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
// Guard 2: `member_shape_peek_or_compute` peeks the `ShapeCacheDb` per-member
//          slot BEFORE any node-domain gate. The warm path must pay zero
//          raise/gate cost — moving the peek after the package-backed gate
//          would re-run the workspace-touching gate on every warm hit.
//          (Successor of the retired `reduce_field_type_expr_with_mode`
//          TypeExpr-peek ordering guard: the per-field reducer was reworked
//          onto node-domain sources and the sink-level peek is now the
//          `ShapeCacheDb` slot peek.)
// ---------------------------------------------------------------------------
#[test]
fn member_shape_peek_or_compute_peeks_cache_before_node_gates() {
    let content = read_output_sink();
    let body = fn_body_slice(&content, "fn member_shape_peek_or_compute(");

    let peek_idx = body
        .find("cache.peek(&key")
        .expect("guard: `member_shape_peek_or_compute` MUST peek the `ShapeCacheDb` slot.");
    let route_gate_idx = body
        .find("node_package_backed_object_like_root_with_fence(")
        .expect(
            "guard: the node-domain package-backed gate must remain present in \
             `member_shape_peek_or_compute`.",
        );
    assert!(
        peek_idx < route_gate_idx,
        "guard: the `ShapeCacheDb` peek MUST run BEFORE \
         `node_package_backed_object_like_root_with_fence` in \
         `member_shape_peek_or_compute` so warm per-member hits return in \
         peek time without paying the node-gate cost.",
    );
}

// ---------------------------------------------------------------------------
// Guard 2b: `member_shape_peek_or_compute` admits to the shared shape cache
//          ONLY AFTER the package-backed gate has run. Every
//          `admit_member_shape_if_possible` call site must follow the gate:
//          admitting before it could publish a warm entry that leaks a
//          reduced shape past the shallow-by-default gate for shared cache
//          entries. (Successor of the retired cached-peek-after-gate guard
//          on `reduce_field_type_expr_with_mode`: the node-domain rework
//          replaced re-checking the gate on warm hits with gate-cleared-only
//          admission.)
// ---------------------------------------------------------------------------
#[test]
fn member_shape_admissions_run_only_after_package_backed_gate() {
    let content = read_output_sink();
    let body = fn_body_slice(&content, "fn member_shape_peek_or_compute(");

    let route_gate_idx = body
        .find("node_package_backed_object_like_root_with_fence(")
        .expect("guard: the node-domain package-backed gate must remain present.");
    let first_admit_idx = body.find("admit_member_shape_if_possible(").expect(
        "guard: `member_shape_peek_or_compute` MUST admit gate-cleared shapes \
         through `admit_member_shape_if_possible`.",
    );
    assert!(
        route_gate_idx < first_admit_idx,
        "guard: every `admit_member_shape_if_possible` call in \
         `member_shape_peek_or_compute` MUST run AFTER the node-domain \
         package-backed gate. Admitting before the gate would let a warm \
         `ShapeCacheDb` entry leak a reduced shape past the \
         shallow-by-default gate for shared cache entries.",
    );
}

// ---------------------------------------------------------------------------
// Guard 3: `member_shape_peek_or_compute` decides the shallow gates on the
//          member-value NODE (node-domain facts) and runs the package-backed
//          gate BEFORE the graph-native reducer. The gate-before-reducer
//          ordering is the load-bearing correctness invariant:
//          `MaterializeMemoDb` is shared across the typed-IR materialiser
//          callers (model / registry paths) that do NOT apply the projector's
//          shallow gate, so reducing first would publish the reduced body for a
//          package-backed root — violating the shallow-by-default invariant. The
//          decisions run on the NODE, never on a raised TypeExpr: there is no
//          up-front `shell_raise_to_type_expr(member_value)` and no shared
//          `peek_member_shape_known` operator-shape peek.
// ---------------------------------------------------------------------------
#[test]
fn member_shape_peek_or_compute_runs_node_gates_before_graph_native_reducer() {
    // `member_shape_peek_or_compute` lives IN the terminal `output_sink` sink
    // module; its gates decide on the member-value NODE through the node-domain
    // fact APIs, and materialisation happens only at the terminal seal / the
    // graph-native reducer.
    let content = read_output_sink();
    let body = fn_body_slice(&content, "fn member_shape_peek_or_compute(");

    // NODE-DOMAIN: no up-front raise of member_value to a TypeExpr, no shared
    // operator-shape peek — re-introducing either reverts to materialize-then-gate.
    assert!(
        !body.contains("shell_raise_to_type_expr(&dispatch, member_value)"),
        "guard: `member_shape_peek_or_compute` must NOT raise member_value to a TypeExpr up front \
         — the package-backed / reducibility / cycle gates decide on the NODE.",
    );
    assert!(
        !body.contains("peek_member_shape_known("),
        "guard: `member_shape_peek_or_compute` must NOT consult the shared \
         `peek_member_shape_known` operator-shape cache (the graph-native reducer's own memo \
         covers it); re-adding it reverts to a materialize-then-decide shared-slot peek.",
    );

    // The node-domain package-backed gate.
    let gate_idx = body
        .find("node_package_backed_object_like_root_with_fence(")
        .expect(
            "guard: the node-domain package-backed gate \
             (`node_package_backed_object_like_root_with_fence`) must remain in \
             `member_shape_peek_or_compute`.",
        );
    let reducer_idx = body
        .find("reduce_member_value_graph_native_with_context(")
        .expect(
            "guard: `member_shape_peek_or_compute` MUST reduce the reducible / generic case \
             through the graph-native reducer.",
        );

    assert!(
        gate_idx < reducer_idx,
        "guard: the node-domain package-backed gate \
         (`node_package_backed_object_like_root_with_fence`) MUST run BEFORE the graph-native \
         reducer (`reduce_member_value_graph_native_with_context`). `MaterializeMemoDb` is shared \
         across non-projector callers that do not apply this shallow gate; reducing first would \
         publish the reduced body for a package-backed root, violating the shallow-by-default \
         invariant.",
    );
}
