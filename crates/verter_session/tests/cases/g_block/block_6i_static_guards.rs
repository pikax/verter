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

/// Strip line comments, (nested) block comments, and string/char
/// literal CONTENTS from Rust source so structural guards scan code
/// tokens only — a mention inside a comment or a string cannot
/// satisfy (or trip) a token assertion.
fn strip_comments_and_strings(src: &str) -> String {
    let bytes = src.as_bytes();
    let n = bytes.len();
    let mut out = String::with_capacity(n);
    let mut i = 0usize;
    while i < n {
        let c = bytes[i];
        // Raw string: r"..." / r#"..."# / r##"..."## — drop contents.
        if c == b'r' {
            let mut j = i + 1;
            let mut hashes = 0usize;
            while j < n && bytes[j] == b'#' {
                hashes += 1;
                j += 1;
            }
            if j < n && bytes[j] == b'"' {
                out.push_str("\"\"");
                let close: Vec<u8> = std::iter::once(b'"')
                    .chain(std::iter::repeat_n(b'#', hashes))
                    .collect();
                let mut k = j + 1;
                while k + close.len() <= n && &bytes[k..k + close.len()] != close.as_slice() {
                    k += 1;
                }
                i = (k + close.len()).min(n);
                continue;
            }
        }
        // String literal — drop contents, keep empty quotes.
        if c == b'"' {
            out.push_str("\"\"");
            let mut k = i + 1;
            while k < n {
                if bytes[k] == b'\\' && k + 1 < n {
                    k += 2;
                    continue;
                }
                if bytes[k] == b'"' {
                    k += 1;
                    break;
                }
                k += 1;
            }
            i = k;
            continue;
        }
        // Char literal ('x' / '\x') — distinguish from lifetimes
        // ('a in `&'a str` has no closing quote at the expected spot).
        if c == b'\'' {
            let close = if i + 2 < n && bytes[i + 1] == b'\\' {
                let mut k = i + 2;
                while k < n && bytes[k] != b'\'' && k - i < 8 {
                    k += 1;
                }
                (k < n && bytes[k] == b'\'').then_some(k)
            } else if i + 2 < n && bytes[i + 2] == b'\'' && bytes[i + 1] != b'\'' {
                Some(i + 2)
            } else {
                None
            };
            if let Some(k) = close {
                out.push_str("' '");
                i = k + 1;
                continue;
            }
        }
        // Line comment.
        if c == b'/' && i + 1 < n && bytes[i + 1] == b'/' {
            while i < n && bytes[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        // Block comment — Rust block comments nest.
        if c == b'/' && i + 1 < n && bytes[i + 1] == b'*' {
            let mut depth = 1usize;
            let mut k = i + 2;
            while k + 1 < n && depth > 0 {
                if bytes[k] == b'/' && bytes[k + 1] == b'*' {
                    depth += 1;
                    k += 2;
                } else if bytes[k] == b'*' && bytes[k + 1] == b'/' {
                    depth -= 1;
                    k += 2;
                } else {
                    k += 1;
                }
            }
            out.push(' ');
            i = if depth == 0 { k } else { n };
            continue;
        }
        out.push(c as char);
        i += 1;
    }
    out
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
    // `cache_runtime::CacheEntry<MaterializedOutputTypeExpr>`, NOT a bespoke
    // `ShapeCacheEntry`: every validity rail (the fact signature, the
    // self-root canonicals, the compute-time generation) lives on the
    // shared cache-runtime entry. A regression reintroducing a bespoke
    // per-cache carrier fails here. (The payload carrier is the sealed
    // `MaterializedOutputTypeExpr` — the output-materialization capability
    // fence renamed the former all-`pub`-field `MaterializedTypeExpr`.)
    assert!(
        src.contains("DashMap<ShapeCacheKey, Arc<CacheEntry<MaterializedOutputTypeExpr>>>"),
        "guard B.1: `ShapeCacheDb.entries` MUST store \
         `Arc<CacheEntry<MaterializedOutputTypeExpr>>` (the shared cache-runtime carrier).",
    );
    assert!(
        !src.contains("pub struct ShapeCacheEntry"),
        "guard B.1: the bespoke `ShapeCacheEntry` carrier MUST be retired — the shape \
         cache stores `cache_runtime::CacheEntry<MaterializedOutputTypeExpr>`.",
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
// Guard B.2 — `member_shape_peek_or_compute`'s gate-short-circuit arms
//             admit to the universal cache.
//
// Universal-caching invariant: every successful shape compute admits,
// regardless of how cheap the compute was. The gate-short-circuit arms in
// `member_shape_peek_or_compute` MUST route through
// `admit_member_shape_if_possible`. (The TypeExpr-side helper
// `admit_type_expr_shape_if_possible` was retired together with the
// `reduce_field_type_expr_with_mode` TypeExpr reducer when the per-field
// reduce was reworked onto node-domain sources; the member-shape helper is
// the sole sink-side admission path.)
// ---------------------------------------------------------------------------
#[test]
fn peek_primitive_arms_admit_to_cache() {
    // The admission helper + the boundary-consuming function that calls it
    // (`member_shape_peek_or_compute`) live in the terminal `output_sink`
    // sink module.
    let src =
        read_workspace_file("crates/verter_session/src/meta_resolve/projectors/output_sink.rs");

    // The admission helper must exist.
    assert!(
        src.contains("fn admit_member_shape_if_possible"),
        "guard B.2: `meta_resolve::projectors::output_sink` MUST define \
         `admit_member_shape_if_possible` — the universal-caching admission \
         helper for `member_shape_peek_or_compute`'s gate-short-circuit arms.",
    );

    // Every successful shape outcome must be wrapped in the admission
    // helper, not returned bare. Source-text grep on the call count
    // gives a coarse but discriminating signal. Exclude the definition
    // itself from the count.
    let admit_member_calls = src.matches("admit_member_shape_if_possible(").count()
        - src.matches("fn admit_member_shape_if_possible(").count();
    assert!(
        admit_member_calls >= 3,
        "guard B.2: `admit_member_shape_if_possible` MUST be called at least three times \
         (package-backed gate arm, cycle-gate arm, non-reducible stable-shape arm) inside \
         `member_shape_peek_or_compute`. Observed call count: {admit_member_calls}.",
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

/// G4.4 — the `IndexKey::Number` bounded integer convention is
/// enforced at the LANGUAGE level, not by a textual classifier.
///
/// History: pre-G4.4, producers disagreed on the payload encoding
/// (bit-pattern vs integer convention) — a latent mis-decode class.
/// G4.4 unified on the integer convention with the canonical-spelling
/// bound (`js_number_to_string(v) == (v as i64).to_string()`), and a
/// textual classifier swept every `IndexKey::Number` construction for
/// conformance. The classifier kept losing its discrimination claim to
/// structural evasions (hoisted casts, same-statement predicate
/// laundering, textual identity-copies), so the invariant moved INTO
/// the type system: `IndexKey::Number` carries the proof-carrying
/// [`verter_session::semantic_query::CanonicalIndexInt`] newtype whose
/// field is PRIVATE to `semantic_query::index_key`. The only ways to
/// construct one are the module's two blessed constructors — the
/// f64-checked fold `integer_convention_index_key` and the
/// `Display`-checked `CanonicalIndexInt::from_canonical_i64` — so a raw
/// `IndexKey::Number(n as i64)` anywhere else in the workspace is a
/// COMPILE ERROR. The directory-wide textual sweep is obsolete; rustc
/// is the sweep.
///
/// What still earns a textual pin (the residual assertions below):
/// 1. the payload TYPE stays the newtype — reverting the variant to
///    `Number(i64)` would compile and silently reopen every lane;
/// 2. the owning module keeps the field private and its constructor
///    inventory closed — a `From<i64>`/`Deserialize`/`new` impl or a
///    `pub` field qualifier would reopen an unchecked construction
///    lane without touching any consumer file;
/// 3. the walker's `Index(Number)` value recovery stays on the integer
///    convention (`.get() as f64`), never `f64::from_bits` — the
///    pre-G4.4 bit-pattern consumer is a VALUE question the type
///    system cannot see.
#[test]
fn index_key_number_convention_is_type_enforced() {
    // (1) Payload pin.
    let semantic_query = strip_comments_and_strings(&read_workspace_file(
        "crates/verter_session/src/semantic_query.rs",
    ));
    assert!(
        semantic_query.contains("Number(CanonicalIndexInt)"),
        "G4.4 guard: `IndexKey::Number` must carry the proof-carrying \
         `CanonicalIndexInt` payload — the private-field newtype is what makes \
         unbounded construction a compile error."
    );
    assert!(
        !semantic_query.contains("Number(i64)"),
        "G4.4 guard: `IndexKey::Number(i64)` is the retired raw payload — \
         reverting it reopens every unbounded-construction lane the newtype \
         closed."
    );

    // (2) Owning-module privacy + closed constructor inventory.
    let owner = strip_comments_and_strings(&read_workspace_file(
        "crates/verter_session/src/semantic_query/index_key.rs",
    ));
    assert!(
        owner.contains("pub struct CanonicalIndexInt(i64);"),
        "G4.4 guard: `CanonicalIndexInt`'s field must stay PRIVATE (tuple \
         field with no `pub` qualifier) — privacy is the enforcement \
         mechanism."
    );
    assert!(
        owner.contains("pub fn from_canonical_i64")
            && owner.contains("pub(crate) fn integer_convention_index_key"),
        "G4.4 guard: the two blessed constructors (`integer_convention_index_key`, \
         `from_canonical_i64`) must exist in the owning module."
    );
    // Exactly two raw tuple-construction tokens: the struct declaration
    // and the fold's single `CanonicalIndexInt(candidate)`. A third is a
    // new construction site that must be reviewed (and blessed) HERE.
    assert_eq!(
        owner.matches("CanonicalIndexInt(").count(),
        2,
        "G4.4 guard: the owning module must construct `CanonicalIndexInt` \
         exactly once (plus the struct declaration) — found a new raw \
         construction site."
    );
    assert_eq!(
        owner.matches("Self(").count(),
        0,
        "G4.4 guard: no `Self(..)` construction — keep every construction on \
         the named, countable `CanonicalIndexInt(..)` token."
    );
    // No alternate construction lanes: conversion traits and serde
    // deserialization would mint values without entering a blessed
    // constructor.
    for forbidden in ["impl From<", "impl TryFrom<", "Deserialize", "fn new("] {
        assert!(
            !owner.contains(forbidden),
            "G4.4 guard: `{forbidden}` in the owning module would reopen an \
             unchecked construction lane around the blessed constructors."
        );
    }

    // (3) Consumer-side decode convention — EVERY walk.rs site that
    // produces a `LiteralKey::Number` recovers the value by integer
    // cast, never the pre-G4.4 bit-pattern decode. Both `Index(Number)`
    // decode arms (the direct-payload arm and the
    // `normalized_index_key_node` re-dispatch arm) must hold the
    // convention; pinning only the first window would let the second
    // revert silently.
    let walk_src =
        read_workspace_file("crates/verter_session/src/project_semantic_dispatch/walk.rs");
    let walk_anchor = "LiteralKey::Number(";
    let clamp_to_char_boundary = |src: &str, mut idx: usize| {
        while !src.is_char_boundary(idx) {
            idx -= 1;
        }
        idx
    };
    let walk_windows: Vec<&str> = walk_src
        .match_indices(walk_anchor)
        .map(|(start, _)| {
            let lo = clamp_to_char_boundary(&walk_src, start.saturating_sub(100));
            let hi =
                clamp_to_char_boundary(&walk_src, start.saturating_add(200).min(walk_src.len()));
            &walk_src[lo..hi]
        })
        .collect();
    assert!(
        !walk_windows.is_empty(),
        "G4.4 guard anchor: walk.rs must construct a `LiteralKey::Number(...)` in the \
         Mapped-narrowing literal-key arms"
    );
    for walk_window in &walk_windows {
        assert!(
            !walk_window.contains("f64::from_bits"),
            "G4.4 guard: no walk.rs `LiteralKey::Number(...)` constructor may decode via \
             `f64::from_bits` — that is the pre-G4.4 bit-pattern consumer. Use the integer-\
             convention recovery (`.get() as f64`) to match the producer convention. \
             (window: {walk_window})"
        );
    }
    // Exactly two of the constructors are `CanonicalIndexInt` DECODE
    // sites (the third copies an already-recovered f64 literal out of a
    // resolved `Literal` node); both must recover via the
    // integer-convention cast. A drop below two is a from_bits-style
    // revert (or a deleted decode arm); above two is a new decode site
    // that must be reviewed here.
    assert_eq!(
        walk_windows
            .iter()
            .filter(|w| w.contains(".get() as f64"))
            .count(),
        2,
        "G4.4 guard: walk.rs must hold exactly two `CanonicalIndexInt` decode sites \
         recovering via the integer-convention `.get() as f64` cast."
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
            // `project_model` relocated into the terminal `output_sink` sink
            // module (it raises a payload through the module-private boundary
            // primitive); the cursor-descent invariant is anchored there.
            "project_model",
            "crates/verter_session/src/meta_resolve/projectors/output_sink.rs",
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
            // `project_model` relocated into the terminal `output_sink` sink
            // module (it raises a payload through the module-private boundary
            // primitive); the cursor-descent invariant is anchored there.
            "project_model",
            "crates/verter_session/src/meta_resolve/projectors/output_sink.rs",
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
    // The per-member publication site `surface_member_to_expanded_field`, the
    // sink-private node-domain per-field reducer `reduce_field_value_node`, and
    // the high-level published-field driver `reduce_published_field_types` all
    // live in the terminal `output_sink` sink module (the only module that
    // touches the reverse-materialization boundary).
    let src =
        read_workspace_file("crates/verter_session/src/meta_resolve/projectors/output_sink.rs");
    let body = extract_fn_body(&src, "pub(crate) fn surface_member_to_expanded_field(");
    assert!(
        body.contains("admitted.cursor().terminal_publication_mode()"),
        "publication-mode guard: `surface_member_to_expanded_field` MUST derive \
         the per-member publication mode from \
         `admitted.cursor().terminal_publication_mode()` — publishing a \
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

    // The node-domain per-field reducer must exist (SINK-PRIVATE in
    // `output_sink`; successor of the retired TypeExpr reducer
    // `reduce_field_type_expr_with_mode` after the publication reduce was
    // reworked onto node-domain sources) and the published-field second pass
    // must reduce props/emits in `Navigate` carrier mode.
    assert!(
        src.contains("fn reduce_field_value_node("),
        "publication-mode guard: `reduce_field_value_node` (the node-domain \
         per-field reducer, sink-private in `output_sink`) MUST exist."
    );
    // `reduce_published_field_types` is the HIGH-LEVEL publication API on the
    // `output_sink` sink's `published_finalize` CHILD module (inside the same
    // capability mint scope; it wraps the sink-private per-field reducer).
    // The demand context owns carrier-stop; the second pass MUST still reduce
    // props/emits in `Navigate` carrier mode.
    let finalize_src = read_workspace_file(
        "crates/verter_session/src/meta_resolve/projectors/output_sink/published_finalize.rs",
    );
    let second_pass = extract_fn_body(&finalize_src, "pub(crate) fn reduce_published_field_types(");
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

    // `SemanticQueryKey::Instantiate` carries the sealed `InstantiateKey`
    // payload (a tuple variant), which in turn embeds an `InstantiateContext`
    // that EMBEDS a `projection_reduction: ProjectionReductionContext` (plus
    // the `resolve_env_hash` env dim). Assert both the sealed carrier and the
    // embed.
    assert!(
        src.contains("Instantiate(InstantiateKey)")
            && src.contains("pub struct InstantiateKey")
            && src.contains("context: InstantiateContext"),
        "reduction-context guard: `SemanticQueryKey::Instantiate` MUST carry the \
         sealed `InstantiateKey` payload, which MUST embed \
         `context: InstantiateContext`."
    );

    // The remaining context-bearing variants stay struct variants that embed a
    // reduction context. `KeyOf` / `MappedType` / `ProjectPath` carry the bare
    // shared `ProjectionReductionContext`.
    for (variant_anchor, expected_context) in [
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

    // The graph-native reducible-operator predicate lives in the sibling
    // `published_reducer` module (no reverse-boundary access, so it stays a
    // free-standing helper). The published-surface field-type driver
    // `reduce_published_field_types` is the HIGH-LEVEL publication API and lives
    // in the terminal `output_sink` sink module alongside the now-sink-private
    // per-field reducer it wraps (`reduce_field_type_expr_with_mode`) — the sink
    // is the ONLY module that touches the reverse-materialization boundary.
    let reducer_src = read_workspace_file(
        "crates/verter_session/src/meta_resolve/projectors/published_reducer.rs",
    );
    let published_finalize_src = read_workspace_file(
        "crates/verter_session/src/meta_resolve/projectors/output_sink/published_finalize.rs",
    );
    assert!(
        published_finalize_src.contains("pub(crate) fn reduce_published_field_types("),
        "name-predicate-retired guard: `projectors/output_sink/published_finalize.rs` MUST host \
         `reduce_published_field_types` (the high-level publication API on the sink's \
         finalize child module; the per-field reducer it wraps is sink-private)."
    );
    assert!(
        reducer_src.contains("pub(crate) fn node_contains_reducible_operator("),
        "name-predicate-retired guard: `projectors/published_reducer.rs` MUST host \
         the graph-native `node_contains_reducible_operator`."
    );
    assert!(
        !reducer_src.contains("pub(crate) fn type_expr_contains_reducible_operator("),
        "name-predicate-retired guard: the duplicate TypeExpr predicate must stay absent"
    );

    // The mod.rs MUST re-export the helpers so existing callers reach the
    // pure predicate (`published_reducer`) and the high-level publication APIs
    // (`output_sink`) through `crate::meta_resolve::projectors::*`.
    assert!(
        mod_src.contains("pub(crate) use published_reducer::"),
        "name-predicate-retired guard: `projectors/mod.rs` MUST re-export the \
         reducible-operator predicate from the `published_reducer` sibling module."
    );
    assert!(
        mod_src.contains("pub(crate) use output_sink::"),
        "name-predicate-retired guard: `projectors/mod.rs` MUST re-export the \
         high-level publication APIs from the `output_sink` sink module."
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
