//! Static architecture guards.
//!
//! Companion to `block_6i_runtime_arch_guards.rs`. These are cheap
//! source-text scans that catch regressions on the architectural
//! invariants the projector / registry / cache / NAPI boundaries
//! establish.

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
// The path-precise projection demand substrate
// (`crates/verter_session/src/meta_resolve/projection_demand.rs`)
// requires the registry walker to thread this cursor through its
// recursive descent — landing the signature is the load-bearing
// structural change. Without it, path-precision is impossible.
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
         `ProjectionCursor<'_>` parameter in its signature so the \
         path-precise registry walk threads through. Header:\n{header}",
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
// `PublishedSurfaceKind` are the path-precise projection architectural
// vocabulary. The module being present + naming all of these is the
// minimal contract subsequent passes depend on.
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
// `ShapeCacheDb` replaces the previously-split `MaterializeMemoDb`
// (TypeExpr-keyed) and `MemberShapeCacheDb` (SemanticNode-keyed)
// with a single cache whose key carries a `ShapeSubject`
// discriminant + a `ShapeDemand` (path + mode + filter + surface).
// One cache, not two. Source-text guard asserts the structural shape
// landed.
// ---------------------------------------------------------------------------
#[test]
fn shape_cache_db_replaces_split_caches() {
    let src = read_workspace_file("crates/verter_session/src/component_meta_caches.rs");

    for symbol in [
        "pub struct ShapeCacheDb",
        "pub enum ShapeSubject",
        "pub struct ShapeDemand",
        "pub struct ShapeCacheKey",
    ] {
        assert!(
            src.contains(symbol),
            "guard B.1: `component_meta_caches.rs` MUST declare `{symbol}` — the \
             universal-cache architectural contract.",
        );
    }

    // The shape cache stores the runtime carrier
    // `cache_runtime::CacheEntry<MaterializedTypeExpr>`, NOT a bespoke
    // `ShapeCacheEntry`: every validity rail (the fact signature, the
    // self-root canonicals, the compute-time generation) lives on the
    // shared cache-runtime entry. A regression reintroducing a bespoke
    // per-cache carrier fails here.
    assert!(
        src.contains("DashMap<ShapeCacheKey, Arc<CacheEntry<MaterializedTypeExpr>>>"),
        "guard B.1: `ShapeCacheDb.entries` MUST store \
         `Arc<CacheEntry<MaterializedTypeExpr>>` (the shared cache-runtime carrier).",
    );
    assert!(
        !src.contains("pub struct ShapeCacheEntry"),
        "guard B.1: the bespoke `ShapeCacheEntry` carrier MUST be retired — the shape \
         cache stores `cache_runtime::CacheEntry<MaterializedTypeExpr>`.",
    );

    // The legacy split caches MUST be retired (no public surface).
    for retired_ty in [
        "pub struct MaterializeMemoDb",
        "pub struct MemberShapeCacheDb",
    ] {
        assert!(
            !src.contains(retired_ty),
            "guard B.1: legacy split-cache type `{retired_ty}` MUST be retired — \
             replaced by `ShapeCacheDb`.",
        );
    }
}

// ---------------------------------------------------------------------------
// Guard B.2 — peek primitive's `Leaf` / `BareCarrier` arms admit to
//             the universal cache.
//
// Universal-caching invariant: every successful shape compute admits,
// regardless of how cheap the compute was. The peek primitive's
// `Leaf` and `BareCarrier` arms in
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
         `admit_type_expr_shape_if_possible` — the universal-caching admission \
         helper for the `reduce_field_type_expr` peek's `Leaf` / `BareCarrier` arms.",
    );
    assert!(
        src.contains("fn admit_member_shape_if_possible"),
        "guard B.2: `meta_resolve::projectors::mod` MUST define \
         `admit_member_shape_if_possible` — the universal-caching admission \
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
// The path-walker narrowing closes the
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

    // (1) The narrowing path must per-key materialise via the
    // shared `materialize_selected_key_mapped_value_with_node`
    // substrate, which internally performs substitute + evaluate +
    // Instantiate + **trailing Conditional reduction**. An alternative
    // textual surface inlines `substitute_semantic_type_param` +
    // `evaluate_deferred_semantic_node`; the shared helper factors those
    // out so the synthesise + path-walker callers converge on identical
    // per-key semantics. Either textual surface (the shared helper OR
    // the inline pair) satisfies the F.1 contract: per-key narrowing
    // must actually fire, not be replaced by the whole-surface
    // MappedType dispatch alone.
    let uses_round7_helper = arm_body.contains("materialize_selected_key_mapped_value_with_node");
    let uses_inline_substitute_and_evaluate = arm_body.contains("substitute_semantic_type_param")
        && arm_body.contains("evaluate_deferred_semantic_node");
    assert!(
        uses_round7_helper || uses_inline_substitute_and_evaluate,
        "guard F.1: `PathWalker`'s Mapped arm MUST per-key narrow — either via the shared \
         `materialize_selected_key_mapped_value_with_node` substrate (which internally does \
         substitute + evaluate + Instantiate + trailing Conditional reduction) OR via the \
         inline `substitute_semantic_type_param` + `evaluate_deferred_semantic_node` \
         pair (operator-level narrowing). The MappedType dispatch alone \
         enumerates every key and leaks `Tool<INPUT, OUTPUT>['outputSchema']`-shaped queries \
         into the audit footprint.",
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
         whole-surface path-walker behaviour for the narrowed key.",
    );
}

/// G4.4 static guard — `IndexKey::Number` convention must remain
/// unified across producers and consumers.
///
/// Pre-G4.4, `lower::shallow_lower_type_expr` stored numeric
/// literals in `IndexKey::Number(i64)` using the bit-pattern
/// convention (`n.to_bits() as i64`), while every other producer
/// (`evaluate::normalized_index_key_node`,
/// `substitute::substitute_index_key_with_change_tracking`) used
/// the integer convention (`*number as i64`). The asymmetry was a
/// latent soundness gap — a substitution-produced
/// `IndexKey::Number(1)` would be mis-decoded by the walker's
/// bit-pattern recovery (`f64::from_bits(1u64)` = 5e-324) instead
/// of 1.0.
///
/// G4.4 unifies on the integer convention. This guard pins the
/// invariant by:
///   1. Verifying `lower.rs` does NOT use `n.to_bits()` in its
///      `IndexKey::Number` construction.
///   2. Verifying `walk.rs`'s `Index(Number)` arm does NOT use
///      `f64::from_bits` to decode.
///
/// Any reversion to the bit-pattern convention at either site
/// would trip this guard.
#[test]
fn index_key_number_convention_is_unified_integer() {
    let lower_src =
        read_workspace_file("crates/verter_session/src/project_semantic_dispatch/lower.rs");
    let walk_src =
        read_workspace_file("crates/verter_session/src/project_semantic_dispatch/walk.rs");

    // Producer side — lower.rs MUST NOT call `to_bits()` adjacent
    // to any `IndexKey::Number(...)` constructor. We scan the
    // entire file for the pattern; the file owns one
    // `IndexKey::Number` constructor (in the indexed-access
    // lowering arm) and any future call site must follow the
    // unified integer convention.
    //
    // The check is structural: assert that no `IndexKey::Number(`
    // call site contains a `.to_bits()` invocation within its
    // immediate expression context. We approximate "immediate
    // expression context" with a 80-char window around the
    // constructor (the constructor body is a single expression).
    let mut search_start = 0usize;
    let needle = "IndexKey::Number(";
    let mut found_any = false;
    while let Some(idx) = lower_src[search_start..].find(needle) {
        let abs = search_start + idx;
        let hi = (abs + needle.len() + 80).min(lower_src.len());
        let local = &lower_src[abs..hi];
        assert!(
            !local.contains("to_bits()"),
            "G4.4 guard: lower.rs MUST NOT store `n.to_bits() as i64` in \
             `IndexKey::Number` — that is the pre-G4.4 bit-pattern convention. \
             Use the integer convention (`*n as i64`) gated by \
             `n.fract() == 0.0 && i64 range` so the convention matches \
             `evaluate::normalized_index_key_node`, \
             `substitute::substitute_index_key_with_change_tracking`, and \
             `raise::raise_index_key_to_type_expr`. (offset {abs}, snippet: {local})"
        );
        found_any = true;
        search_start = abs + needle.len();
    }
    assert!(
        found_any,
        "G4.4 guard anchor: lower.rs must contain at least one `IndexKey::Number(` \
         constructor (the indexed-access literal-number lowering arm)"
    );

    // Consumer side — walk.rs `Index(Number)` arm that produces a
    // `LiteralKey::Number`. There are multiple `IndexKey::Number(n)`
    // patterns in walk.rs (e.g. the needle-text rendering arm uses
    // `n.to_string()`); the discriminating one for G4.4 is the
    // Mapped-narrowing literal-key arm that produces
    // `LiteralKey::Number`. Anchor on `LiteralKey::Number(` to land
    // in the right region.
    let walk_anchor = "LiteralKey::Number(";
    let walk_window_start = walk_src.find(walk_anchor).expect(
        "G4.4 guard anchor: walk.rs must construct a `LiteralKey::Number(...)` in the \
         Mapped-narrowing literal-key arm",
    );
    // 200 chars is enough to capture the constructor call's payload
    // expression. Backtrack 100 chars so the assertion also catches
    // a regression that fences `LiteralKey::Number(` from a
    // `from_bits` line above it.
    let window_lo = walk_window_start.saturating_sub(100);
    let window_hi = walk_window_start.saturating_add(200).min(walk_src.len());
    let walk_window = &walk_src[window_lo..window_hi];
    assert!(
        !walk_window.contains("f64::from_bits"),
        "G4.4 guard: walk.rs's `LiteralKey::Number(...)` constructor MUST NOT decode via \
         `f64::from_bits` — that is the pre-G4.4 bit-pattern consumer. Use the integer-\
         convention recovery (`*n as f64`) to match the unified producer convention. \
         (window: {walk_window})"
    );
}

// ---------------------------------------------------------------------------
// Carrier-preserving per-member publication.
//
// This closes the Rule-5 depth leak: a macro publishes every
// top-level member NAME, but each member's type body is published as
// a CARRIER (`Navigate` mode) — NOT breadth-enumerated. The cursor
// threaded through the projector pipeline + macro-shape helpers must
// be LOAD-BEARING: an inert `let _ = cursor;` parameter leaves the
// leak open.
//
// These guards are DISCRIMINATING — they FAIL against a
// threaded-but-unused cursor (`let _ = cursor;` in every body) and PASS
// against the carrier-preserving narrowing.
// ---------------------------------------------------------------------------

/// Extract a function body (the brace-balanced span from the `{`
/// after the signature to its matching `}`). `anchor` is the
/// signature prefix (e.g. `"pub(crate) fn project_props("`).
///
/// Tracks `"..."` string literals (with `\` escapes) so braces
/// inside `format!` strings do not unbalance the count. Rust
/// lifetimes (`'_`, `'a`) make `'`-based char-literal tracking
/// unreliable, so `'` is ignored — function bodies in this codebase
/// do not contain unbalanced braces inside char literals.
fn extract_fn_body(src: &str, anchor: &str) -> String {
    let fn_idx = src
        .find(anchor)
        .unwrap_or_else(|| panic!("guard: anchor `{anchor}` must exist"));
    let open_rel = src[fn_idx..]
        .find('{')
        .unwrap_or_else(|| panic!("guard: `{anchor}` body must have an opening brace"));
    let body_start = fn_idx + open_rel;
    let bytes = src.as_bytes();
    let mut depth = 0usize;
    let mut end = body_start;
    let mut in_str = false;
    let mut prev = 0u8;
    for (i, &b) in bytes.iter().enumerate().skip(body_start) {
        let escaped = prev == b'\\';
        match b {
            b'"' if !escaped => in_str = !in_str,
            b'{' if !in_str => depth += 1,
            b'}' if !in_str => {
                depth -= 1;
                if depth == 0 {
                    end = i + 1;
                    break;
                }
            }
            _ => {}
        }
        prev = if escaped { 0 } else { b };
    }
    assert!(
        end > body_start,
        "guard: `{anchor}` body brace-match failed"
    );
    src[body_start..end].to_string()
}

/// The cursor-threaded production functions that must be made
/// load-bearing. `(label, rel_path, signature_anchor)`.
///
/// The retired macro-object materialiser cluster
/// (`produce_one_macro_object_shape` / `project_named_ref_*_shape` in
/// `materialize/macro_shapes.rs`) is DELETED — `define_*` shapes are
/// produced by the dispatch projectors below. The surviving
/// cursor-threading authority is the projector set; their absence is
/// guarded by `no_legacy_walker.rs::RETIRED_SYMBOLS`.
fn ax_cursor_target_set() -> Vec<(&'static str, &'static str, &'static str)> {
    vec![
        (
            "project_props",
            "crates/verter_session/src/meta_resolve/projectors/props.rs",
            "pub(crate) fn project_props(",
        ),
        (
            "project_emits",
            "crates/verter_session/src/meta_resolve/projectors/emits.rs",
            "pub(crate) fn project_emits(",
        ),
        (
            "project_slots",
            "crates/verter_session/src/meta_resolve/projectors/slots.rs",
            "pub(crate) fn project_slots(",
        ),
        (
            "project_exposed",
            "crates/verter_session/src/meta_resolve/projectors/exposed.rs",
            "pub(crate) fn project_exposed(",
        ),
        (
            "project_options",
            "crates/verter_session/src/meta_resolve/projectors/options.rs",
            "pub(crate) fn project_options(",
        ),
        (
            "project_model",
            "crates/verter_session/src/meta_resolve/projectors/model.rs",
            "pub(crate) fn project_model(",
        ),
    ]
}

// ---------------------------------------------------------------------------
// Cursor-consumption guard — the cursor is CONSUMED, not `let _ = cursor;`-discarded.
//
// Discriminating: a threaded-but-discarded cursor (`let _ = cursor;`
// in every body) FAILS this guard.
// ---------------------------------------------------------------------------
#[test]
fn ax_cursor_is_consumed_not_discarded() {
    for (label, rel, anchor) in ax_cursor_target_set() {
        let src = read_workspace_file(rel);
        let body = extract_fn_body(&src, anchor);

        assert!(
            !body.contains("let _ = cursor;"),
            "cursor-consumption guard ({label}): `cursor` MUST NOT be discarded with \
             `let _ = cursor;`. The cursor MUST gate per-member \
             publication — an inert cursor \
             leaves the Rule-5 `outputSchema`/`execute` depth leak open."
        );

        // The cursor must be consumed in a load-bearing position:
        // either descended (`descend_published_member` /
        // `descend`), queried, or forwarded to a callee.
        let used = body.contains("cursor.descend_published_member(")
            || body.contains("cursor.descend(")
            || body.contains("cursor.admits_key(")
            || body.contains("cursor.terminal_publication_mode(")
            || body.contains("cursor.terminal_mode(")
            || body.contains("cursor.is_terminal(")
            || body.contains(", cursor)")
            || body.contains(", cursor,")
            || body.contains("(cursor)")
            || body.contains("(cursor,");
        assert!(
            used,
            "cursor-consumption guard ({label}): `cursor` MUST be consumed — \
             descended via `descend_published_member`, queried, or \
             forwarded to a callee. Observed an unused parameter."
        );
    }
}

// ---------------------------------------------------------------------------
// Member-descent guard — every macro projector gates published members through
// `cursor.descend_published_member`.
//
// The retired macro-shape materialiser (`finalize_macro_shape_through_cursor`
// in `materialize/macro_shapes.rs`) is DELETED; the per-member breadth
// gate now lives ONLY in the dispatch projectors. Each projector
// admits a surface member by descending the publication cursor
// (`cursor.descend_published_member(name)`); a member the cursor does
// not admit (`None`) is dropped from the surface. This is the live
// Rule-5 per-member descent — if a projector stops calling
// `descend_published_member`, the `outputSchema`/`execute` depth leak
// re-opens. (Retired-materialiser ABSENCE is covered by
// `no_legacy_walker.rs`; this guard covers projector PRESENCE.)
// ---------------------------------------------------------------------------
#[test]
fn ax_projectors_descend_published_member() {
    // Each surviving macro projector and the body anchor whose
    // surface-member loop MUST descend the publication cursor.
    let projector_set: &[(&str, &str, &str)] = &[
        (
            "project_props",
            "crates/verter_session/src/meta_resolve/projectors/props.rs",
            "pub(crate) fn project_props(",
        ),
        (
            "project_emits",
            "crates/verter_session/src/meta_resolve/projectors/emits.rs",
            "pub(crate) fn project_emits(",
        ),
        (
            "project_slots",
            "crates/verter_session/src/meta_resolve/projectors/slots.rs",
            "pub(crate) fn project_slots(",
        ),
        (
            "project_exposed",
            "crates/verter_session/src/meta_resolve/projectors/exposed.rs",
            "pub(crate) fn project_exposed(",
        ),
        (
            "project_options",
            "crates/verter_session/src/meta_resolve/projectors/options.rs",
            "pub(crate) fn project_options(",
        ),
        (
            "project_model",
            "crates/verter_session/src/meta_resolve/projectors/model.rs",
            "pub(crate) fn project_model(",
        ),
    ];

    for (label, rel, anchor) in projector_set {
        let src = read_workspace_file(rel);
        let body = extract_fn_body(&src, anchor);
        assert!(
            body.contains("cursor.descend_published_member("),
            "member-descent guard ({label}): the projector MUST descend each \
             admitted surface member through \
             `cursor.descend_published_member(` — that is the live \
             per-member breadth gate. Dropping it re-opens the Rule-5 \
             `outputSchema`/`execute` depth leak the macro-shape \
             materialiser used to close."
        );
    }
}

// ---------------------------------------------------------------------------
// Publication-mode guard — the projector pipeline publishes members at the
// cursor's `terminal_publication_mode()`, NOT a hard-coded
// `ProjectionMode::Expanded`.
//
// `surface_member_to_expanded_field` is the per-member publication
// site. The hard-coded `Expanded` mode was replaced with the
// cursor-derived publication mode (`Navigate` carrier by default).
// A hard-coded `Expanded` is the depth-leak signature.
// ---------------------------------------------------------------------------
#[test]
fn ax_projector_uses_terminal_publication_mode() {
    let src = read_workspace_file("crates/verter_session/src/meta_resolve/projectors/mod.rs");
    let body = extract_fn_body(&src, "pub(crate) fn surface_member_to_expanded_field(");
    assert!(
        body.contains("member_cursor.terminal_publication_mode()"),
        "publication-mode guard: `surface_member_to_expanded_field` MUST derive \
         the per-member publication mode from \
         `member_cursor.terminal_publication_mode()` — publishing a \
         macro member at a carrier (`Navigate`) mode is the Rule-5 \
         depth-leak fix."
    );
    assert!(
        !body.contains("ProjectionMode::Expanded"),
        "publication-mode guard: `surface_member_to_expanded_field` MUST NOT \
         hard-code `ProjectionMode::Expanded` for the per-member \
         materialise — that re-opens the `outputSchema`/`execute` \
         depth leak. Use the cursor's publication mode."
    );

    // The carrier-aware reducer must exist and the published-field
    // second pass must reduce props/emits in `Navigate` carrier mode.
    assert!(
        src.contains("pub(crate) fn reduce_field_type_expr_with_mode("),
        "publication-mode guard: `reduce_field_type_expr_with_mode` (the \
         carrier-aware field reducer) MUST exist."
    );
    // `reduce_published_field_types` and
    // `type_expr_contains_reducible_operator` live in the
    // `published_reducer.rs` sibling module (split out from the
    // retired `field_reduce.rs` so `projectors/mod.rs` stays under
    // the no-oversize-files guard). The demand context owns
    // carrier-stop; the second pass MUST still reduce props/emits in
    // `Navigate` carrier mode.
    let reducer_src = read_workspace_file(
        "crates/verter_session/src/meta_resolve/projectors/published_reducer.rs",
    );
    let second_pass = extract_fn_body(&reducer_src, "pub(crate) fn reduce_published_field_types(");
    assert!(
        second_pass.contains("ProjectionMode::Navigate"),
        "publication-mode guard: `reduce_published_field_types` MUST reduce \
         published macro props/emits in `ProjectionMode::Navigate` \
         (carrier) mode so the second pass does not re-expand \
         generic instantiations the projector kept shallow."
    );
}

// ---------------------------------------------------------------------------
// Cursor-threading guard — the cursor-threaded production functions still carry
// the `ProjectionCursor` parameter (threading must not regress).
// ---------------------------------------------------------------------------
#[test]
fn ax_cursor_threaded_functions_keep_parameter() {
    for (label, rel, anchor) in ax_cursor_target_set() {
        let src = read_workspace_file(rel);
        let header_start = src.find(anchor).unwrap_or_else(|| {
            panic!("cursor-threading guard ({label}): anchor `{anchor}` must exist")
        });
        // Bound the header by the body's opening brace — the
        // signature `) ->` can appear inside an `impl Fn(..) -> ..`
        // parameter type, so a `find(") ->")` is unsafe. Everything
        // between the anchor and the first `{` is the signature.
        let body_open = src[header_start..]
            .find('{')
            .map(|n| header_start + n)
            .unwrap_or_else(|| {
                panic!("cursor-threading guard ({label}): function body brace not found")
            });
        let header = &src[header_start..body_open];
        assert!(
            header.contains("ProjectionCursor"),
            "cursor-threading guard ({label}): the production function MUST keep \
             its `ProjectionCursor` parameter. \
             Header observed:\n{header}"
        );
    }
}

// ---------------------------------------------------------------------------
// Demand-bounded generic reduction.
//
// These guards lock the carrier-stop authority on the dispatch demand
// context (no projector-layer name predicates). If a future change
// reintroduces nominal carrier checks, the guards fail loudly.
// ---------------------------------------------------------------------------

/// Reduction-context guard — the three operator keys carry a
/// `ProjectionReductionContext`.
///
/// `Instantiate`, `KeyOf`, `MappedType` keys MUST embed the
/// reduction-demand context so a `StructuralTransit/Shallow` query
/// does not poison a `Published/Shallow` cache slot.
#[test]
fn ax_hybrid_three_keys_carry_reduction_context() {
    let src = read_workspace_file("crates/verter_session/src/semantic_query.rs");

    for symbol in [
        "pub enum ReductionDemand",
        "pub struct ProjectionReductionContext",
        "pub const fn may_reduce_operator",
    ] {
        assert!(
            src.contains(symbol),
            "reduction-context guard: `semantic_query.rs` MUST declare `{symbol}` \
             — the reduction-demand substrate."
        );
    }

    // Each context-bearing variant must embed a reduction context.
    // `Instantiate` carries an `InstantiateContext` that EMBEDS a
    // `projection_reduction: ProjectionReductionContext` (plus the
    // `resolve_env_hash` env dim); `KeyOf` / `MappedType` / `ProjectPath`
    // carry the bare shared `ProjectionReductionContext`.
    for (variant_anchor, expected_context) in [
        (
            "Instantiate {\n        base: ResolvedDeclSlotIdentity,",
            "context: InstantiateContext",
        ),
        (
            "KeyOf {\n        base: SemanticNodeId,",
            "context: ProjectionReductionContext",
        ),
        (
            "MappedType {\n        source: SemanticNodeId,",
            "context: ProjectionReductionContext",
        ),
        (
            "ProjectPath {\n        base: SemanticNodeId,",
            "context: ProjectionReductionContext",
        ),
    ] {
        let pos = src.find(variant_anchor).unwrap_or_else(|| {
            panic!("reduction-context guard: variant anchor not found: {variant_anchor}")
        });
        let close = src[pos..]
            .find("    },")
            .map(|n| pos + n)
            .unwrap_or(src.len());
        let body = &src[pos..close];
        assert!(
            body.contains(expected_context),
            "reduction-context guard: variant at `{variant_anchor}` MUST embed \
             `{expected_context}`. Body:\n{body}"
        );
    }

    // The `Instantiate` context carrier itself must embed a
    // `projection_reduction: ProjectionReductionContext` (the wrapper
    // keeps the reduction-demand identity, adds only `resolve_env_hash`).
    assert!(
        src.contains("pub struct InstantiateContext")
            && src.contains("pub projection_reduction: ProjectionReductionContext"),
        "reduction-context guard: `InstantiateContext` MUST embed \
         `projection_reduction: ProjectionReductionContext`."
    );
}

/// Structural-transit guard — the relation engine unwraps under `StructuralTransit`.
#[test]
fn ax_hybrid_relation_engine_uses_structural_transit() {
    let src =
        read_workspace_file("crates/verter_session/src/project_semantic_dispatch/relation.rs");

    let count = src
        .matches("ProjectionReductionContext::structural_transit()")
        .count();
    assert!(
        count >= 2,
        "structural-transit guard: `relation.rs` MUST call \
         `ProjectionReductionContext::structural_transit()` at least \
         twice. Observed: {count}."
    );

    assert!(
        !src.contains("body_mode: ProjectionMode::Expanded")
            && !src.contains("body_mode: crate::semantic_query::ProjectionMode::Expanded"),
        "structural-transit guard: `relation.rs` MUST NOT hard-code \
         `body_mode: ProjectionMode::Expanded`."
    );

    assert!(
        src.contains("evaluate_deferred_semantic_node_with_context"),
        "structural-transit guard: `relation.rs` MUST consult \
         `evaluate_deferred_semantic_node_with_context`."
    );
}

/// Carrier-stop guard — `build_key_of` / `build_mapped_type` carrier-stop
/// via the demand context.
#[test]
fn ax_hybrid_carrier_stop_uses_demand_context_not_name_predicate() {
    let src = read_workspace_file("crates/verter_session/src/project_semantic_dispatch/build.rs");

    let key_of_body = extract_fn_body(&src, "pub(super) fn build_key_of(");
    assert!(
        key_of_body.contains("may_reduce_operator(context)"),
        "carrier-stop guard: `build_key_of` MUST gate keyspace reification \
         on `may_reduce_operator(context)`."
    );
    assert!(
        key_of_body.contains("SemanticNodeData::KeyOf { base }"),
        "carrier-stop guard: `build_key_of` MUST return a deferred \
         `SemanticNodeData::KeyOf` carrier on carrier-stop."
    );

    let mapped_body = extract_fn_body(&src, "pub(super) fn build_mapped_type(");
    assert!(
        mapped_body.contains("may_reduce_operator(context)"),
        "carrier-stop guard: `build_mapped_type` MUST gate member \
         materialisation on `may_reduce_operator(context)`."
    );
    assert!(
        mapped_body.contains("SemanticNodeData::Mapped"),
        "carrier-stop guard: `build_mapped_type` MUST return a \
         `SemanticNodeData::Mapped` carrier on carrier-stop."
    );

    for forbidden in [
        "BuiltinUtility::from_name",
        "is_builtin_utility_instantiation",
        "generic_instantiation_body_is_object",
    ] {
        assert!(
            !src.contains(forbidden),
            "carrier-stop guard: `build.rs` MUST NOT use the nominal \
             carrier predicate `{forbidden}`."
        );
    }
}

/// Name-predicate-retired guard — projector-layer name predicates retired.
#[test]
fn ax_hybrid_projector_layer_name_predicates_retired() {
    let field_reduce_path =
        workspace_root().join("crates/verter_session/src/meta_resolve/projectors/field_reduce.rs");
    assert!(
        !field_reduce_path.exists(),
        "name-predicate-retired guard: `field_reduce.rs` MUST be deleted \
         (projector-layer carrier check retired)."
    );

    let mod_src = read_workspace_file("crates/verter_session/src/meta_resolve/projectors/mod.rs");
    for forbidden in [
        "is_builtin_utility_instantiation",
        "generic_instantiation_body_is_object",
    ] {
        assert!(
            !mod_src.contains(forbidden),
            "name-predicate-retired guard: `projectors/mod.rs` MUST NOT reference \
             `{forbidden}`."
        );
    }

    // The migrated helpers live in the sibling `published_reducer`
    // module — splitting them out keeps `projectors/mod.rs` under the
    // workspace-wide `no_oversize_files` guard.
    let reducer_src = read_workspace_file(
        "crates/verter_session/src/meta_resolve/projectors/published_reducer.rs",
    );
    assert!(
        reducer_src.contains("pub(crate) fn reduce_published_field_types("),
        "name-predicate-retired guard: `projectors/published_reducer.rs` MUST host \
         `reduce_published_field_types`."
    );
    assert!(
        reducer_src.contains("pub(crate) fn type_expr_contains_reducible_operator("),
        "name-predicate-retired guard: `projectors/published_reducer.rs` MUST host \
         `type_expr_contains_reducible_operator`."
    );

    // The mod.rs MUST re-export both helpers so existing callers
    // (`projectors/model.rs`, the post-projection driver) reach them
    // through `super::*`.
    assert!(
        mod_src.contains("pub(crate) use published_reducer::"),
        "name-predicate-retired guard: `projectors/mod.rs` MUST re-export the \
         migrated helpers from the `published_reducer` sibling module."
    );
}

/// Context-explicit guard — `evaluate_deferred_semantic_node` is context-explicit.
#[test]
fn ax_hybrid_evaluate_is_context_explicit() {
    let src =
        read_workspace_file("crates/verter_session/src/project_semantic_dispatch/evaluate.rs");

    assert!(
        src.contains("pub(super) fn evaluate_deferred_semantic_node_with_context("),
        "context-explicit guard: `evaluate.rs` MUST define \
         `evaluate_deferred_semantic_node_with_context`."
    );

    assert!(
        !src.contains("body_mode: crate::semantic_query::ProjectionMode::Expanded")
            && !src.contains("body_mode: ProjectionMode::Expanded"),
        "context-explicit guard: `evaluate.rs` MUST NOT hard-code \
         `body_mode: Expanded` on DeclPlaceholder unwrap."
    );

    let implicit_body = extract_fn_body(&src, "pub(super) fn evaluate_deferred_semantic_node(");
    // The legacy implicit overload routes through the context-explicit
    // overload with an EXPLICIT publication context — no more
    // implicit `body_mode: Expanded` hand-rolled at the DeclPlaceholder
    // unwrap. Structural-transit callers must opt in via
    // `evaluate_deferred_semantic_node_with_context` (relation.rs's
    // structural-transit arms do this).
    assert!(
        implicit_body.contains("ProjectionReductionContext::published(ProjectionMode::Expanded)"),
        "context-explicit guard: `evaluate_deferred_semantic_node` MUST route \
         through `_with_context` with an EXPLICIT \
         `ProjectionReductionContext::published(ProjectionMode::Expanded)` \
         context — the implicit Expanded unwrap is retired \
         (now context-explicit)."
    );
    assert!(
        implicit_body.contains("evaluate_deferred_semantic_node_with_context"),
        "context-explicit guard: legacy `evaluate_deferred_semantic_node` MUST \
         delegate to `_with_context` (context-explicit dispatch)."
    );
}

// ---------------------------------------------------------------------------
// Implicit `lower_type_expr_in_scope` wrapper retired. Every caller
// of dispatch lowering MUST go through `lower_type_expr_in_scope_with_mode`
// and state mode explicitly.
//
// Why this guard: the implicit default was `ProjectionMode::Expanded`,
// which silently eagerly-reduced `keyof T` / `MappedType<T>` operators
// at every intermediate-base lowering site (the root cause of the
// ChatMessages cold-seq `outputSchema|execute = 62` audit leak).
// A regression that brings back the wrapper would reintroduce the
// leak silently — this guard fails the build.
// ---------------------------------------------------------------------------
#[test]
fn block_6i_commit_ax_no_implicit_lower_type_expr_in_scope_wrapper() {
    let dispatch_mod =
        read_workspace_file("crates/verter_session/src/project_semantic_dispatch/mod.rs");

    // The wrapper signature must NOT exist (it is deleted; every
    // caller passes mode explicitly).
    assert!(
        !dispatch_mod.contains("pub fn lower_type_expr_in_scope(\n")
            && !dispatch_mod.contains("pub fn lower_type_expr_in_scope("),
        "the implicit `lower_type_expr_in_scope` wrapper in \
         `project_semantic_dispatch/mod.rs` MUST remain deleted. \
         Reintroducing it brings back the implicit \
         `ProjectionMode::Expanded` default (root cause of the \
         ChatMessages outputSchema/execute audit leak)."
    );

    // The mode-aware sibling MUST exist (it's the sole entry point).
    assert!(
        dispatch_mod.contains("pub fn lower_type_expr_in_scope_with_mode("),
        "`lower_type_expr_in_scope_with_mode` is the sole lowering \
         entry point and MUST remain present on `ProjectSemanticDispatch`."
    );
}

// ---------------------------------------------------------------------------
// Audit is a PASSIVE OBSERVER.
//
// Production semantic work (lowering, projection, dispatch) must run
// REGARDLESS of `audit_enabled`. The audit captures node ids that
// production already produced; it does not re-derive them via a
// side-channel dispatch round-trip. The retired sidecar in
// `eval_env.rs` is the canonical example — it called
// `dispatch.lower_type_expr_in_scope(canonical, &expansion.value.expr)`
// only when `audit_enabled`, causing audit-on/off to diverge.
//
// This guard scans `compute_evaluated_types_via_dispatch` for the
// pattern that would reintroduce the sidecar: an `if audit_enabled`
// branch followed (in the same enclosing function) by a
// `dispatch.lower_type_expr_in_scope_with_mode` or
// `dispatch.execute(SemanticQueryKey::` call inside that branch.
// ---------------------------------------------------------------------------
#[test]
fn block_6i_commit_ax_audit_is_passive_observer() {
    let eval_env = read_workspace_file("crates/verter_session/src/host_manage/eval_env.rs");

    // The retired sidecar in `eval_env.rs` is gone — its hallmark was
    // `dispatch.lower_type_expr_in_scope(canonical, &expansion.value.expr)`
    // inside an `if audit_enabled` block.
    assert!(
        !eval_env.contains("dispatch.lower_type_expr_in_scope(canonical, &expansion.value.expr)"),
        "the audit-only re-lowering sidecar in `eval_env.rs` MUST \
         remain retired. Audit must capture production-path \
         SemanticNodeId values (set in each dispatch branch above as \
         `produced_node_id = Some(...)`), NOT re-lower via a \
         side-channel call that makes audit-on do extra semantic work."
    );

    // The replacement marker — production capture variable — MUST
    // exist.
    assert!(
        eval_env.contains("produced_node_id: Option<crate::semantic_query::SemanticNodeId>"),
        "`compute_evaluated_types_via_dispatch` \
         MUST declare a `produced_node_id` capture variable so each \
         production dispatch branch records its terminal node id. \
         Without this variable, the audit sidecar regression is \
         impossible to fix without re-lowering."
    );

    // Slot-binding terminal-id variant — production identity exposed
    // before raise.
    let dispatch_mod =
        read_workspace_file("crates/verter_session/src/project_semantic_dispatch/mod.rs");
    assert!(
        dispatch_mod.contains("project_slot_binding_member_with_terminal_id"),
        "`project_slot_binding_member_with_terminal_id` \
         MUST remain present so the audit record captures the \
         slot-binding terminal `SemanticNodeId` from the production \
         dispatch path (no audit-only re-derive)."
    );
}

// ---------------------------------------------------------------------------
// Known limitation — a cursor-aware shallow macro-surface helper that
// would replace `project_expr_class_a_via_dispatch[_threaded]` in
// `macro_shapes.rs` is not yet present.
//
// A `Shallow` terminal projection drops imported-mapped slot
// enumerations (`defineSlots<PricingPlansSlots<{...}>>()` with
// `{ [K in keyof PricingPlanSlots]?: ... } & { default: ... }`).
// The `imported_mapped_slots_reach_*` regression tests fail when the
// carrier publishes at `Shallow` because the Shallow synthesiser bails
// on `Mapped { source: Opaque(DeclPlaceholder) }`. Routing
// `synthesise_mapped_surface` through the shared `key_names_from_base_node`
// (the same enumerator `build_mapped_type` uses) does not close the
// failure either — the deferred carrier surfaces upstream of synthesis.
//
// The macro_shapes rewire is deferred until the Shallow walker supports
// `Mapped { source: deferred }`.
// ---------------------------------------------------------------------------
