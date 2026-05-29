//! Discriminating unit tests for the prepared-surface
//! intersection merge — the intersection-merge bug locus closed by
//! `merge_prepared_intersection_arms`.
//!
//! These tests directly exercise `merge_prepared_intersection_arms`
//! with synthesised inputs, bypassing the component-meta
//! pipeline's rescue paths. The discriminating property: reverting
//! the `PreparedSurfaceProjection::Unsupported => { /* skip */ }`
//! arm to the inverse `return PreparedSurfaceProjection::Unsupported;`
//! short-circuit makes the
//! `..._skips_unsupported_arm_when_sibling_resolves` test fail.

use super::ComponentMetaQueryEngine;
use super::*;
use crate::resolver_core::projected_surface_to_type_expr;
use crate::types::{AnalysisLevel, HostConfig};
use crate::VerterHost;
use std::sync::Arc;
use verter_semantic::analysis::type_solver::query_engine::{ProjectedMember, ProjectedSurface};
use verter_type_expr::TypeExpr;

fn surface_with_member(name: &str) -> std::sync::Arc<ProjectedSurface> {
    std::sync::Arc::new(ProjectedSurface {
        members: vec![ProjectedMember {
            name: name.to_string(),
            ty: TypeExpr::Unknown {
                raw: "unknown".to_string(),
            },
            optional: false,
            readonly: false,
            is_method: false,
            declared_in_macro_type_arg: false,
            // Synthetic test probe with no OXC declaration site — no spans/origin.
            spans: verter_type_expr::MemberSpans::default(),
            declaration_origin: None,
        }],
        call_signatures: Vec::new(),
        construct_signatures: Vec::new(),
        index_signatures: Vec::new(),
        has_index_signature: false,
    })
}

/// Discriminating test: an intersection with one Unsupported arm
/// and one Surface arm must publish the Surface arm's members.
///
/// Inverse revert — changing the `// Skip` arm in
/// `merge_prepared_intersection_arms` to
/// `return PreparedSurfaceProjection::Unsupported` — drops the
/// `present_member` and fails this assertion. The nuxt-ui bench
/// corpus exhibits this with `AuthForm.vue` / `Form.vue` /
/// `Table.vue` body members otherwise lost.
#[test]
fn merge_prepared_intersection_arms_skips_unsupported_arm_when_sibling_resolves() {
    let arms = vec![
        PreparedSurfaceProjection::Unsupported,
        PreparedSurfaceProjection::Surface(surface_with_member("present_member")),
    ];
    let merged = merge_prepared_intersection_arms(arms);
    match merged {
        PreparedSurfaceProjection::Surface(surface) => {
            let names: Vec<&str> = surface.members.iter().map(|m| m.name.as_str()).collect();
            assert!(
                names.contains(&"present_member"),
                "intersection-merge discriminating: an Unsupported arm sibling \
                 to a Surface arm MUST be skipped, NOT short-circuit \
                 the intersection. Got merged surface members: {names:?}"
            );
        }
        other => panic!(
            "intersection-merge discriminating: expected `Surface` with \
             `present_member`, got `{other:?}`. The intersection merge \
             short-circuited on the Unsupported arm — restore the `// Skip` \
             branch in `merge_prepared_intersection_arms`."
        ),
    }
}

/// All-Unsupported intersection collapses to Unsupported.
/// This guards the saw_resolved_arm == false branch — without
/// it, the intersection would silently return an empty Surface,
/// suppressing real resolution failures.
#[test]
fn merge_prepared_intersection_arms_returns_unsupported_when_every_arm_fails() {
    let arms = vec![
        PreparedSurfaceProjection::Unsupported,
        PreparedSurfaceProjection::Unsupported,
    ];
    let merged = merge_prepared_intersection_arms(arms);
    assert!(
        matches!(merged, PreparedSurfaceProjection::Unsupported),
        "intersection with no resolvable arms must collapse to Unsupported"
    );
}

/// Empty arm counts as resolved (saw_resolved_arm = true) but
/// contributes no members. Combined with an Unsupported arm,
/// the intersection must NOT be Unsupported; combined alone, it
/// stays Empty-or-equivalent — verified here as a Surface with
/// no members (the merge canonicalises via
/// `projected_surface_from_parts_intersection`).
#[test]
fn merge_prepared_intersection_arms_treats_empty_arm_as_resolved() {
    let arms = vec![
        PreparedSurfaceProjection::Empty,
        PreparedSurfaceProjection::Unsupported,
    ];
    let merged = merge_prepared_intersection_arms(arms);
    assert!(
        !matches!(merged, PreparedSurfaceProjection::Unsupported),
        "Empty + Unsupported must NOT collapse to Unsupported — Empty \
         arms count as resolved per the saw_resolved_arm contract."
    );
}

/// Two Surface arms merge into a single surface containing the
/// union of member names. Order-stable per
/// `projected_surface_from_parts_intersection`'s sort.
#[test]
fn merge_prepared_intersection_arms_merges_two_surface_arms() {
    let arms = vec![
        PreparedSurfaceProjection::Surface(surface_with_member("alpha")),
        PreparedSurfaceProjection::Surface(surface_with_member("beta")),
    ];
    let merged = merge_prepared_intersection_arms(arms);
    match merged {
        PreparedSurfaceProjection::Surface(surface) => {
            let names: std::collections::HashSet<&str> =
                surface.members.iter().map(|m| m.name.as_str()).collect();
            assert!(names.contains("alpha"));
            assert!(names.contains("beta"));
        }
        other => panic!("expected `Surface` with `alpha` and `beta`, got {other:?}"),
    }
}

/// D1 span threading (positive): `projected_surface_to_type_expr` re-emits the
/// REAL member spans carried on `ProjectedMember` onto the reconstructed IR
/// property — it does NOT drop them to `MemberSpans::default()`.
///
/// Discriminating: reverting the reconstruction to pass `MemberSpans::default()`
/// (the pre-D1 state) makes every `Some(..)` below `None`, failing the asserts.
#[test]
fn projected_surface_to_type_expr_reemits_member_spans() {
    use verter_span::Span;
    use verter_type_expr::{MemberSpans, ObjectMember};

    // A member carrying real OXC declaration-site spans (as the graph
    // `SurfaceMember` / `PreparedMember` / IR source would).
    let member = ProjectedMember {
        name: "label".to_string(),
        ty: TypeExpr::Primitive(verter_type_expr::PrimitiveName::String),
        optional: false,
        readonly: false,
        is_method: false,
        declared_in_macro_type_arg: false,
        spans: MemberSpans {
            declaration: Some(Span::new(10, 24)),
            name: Some(Span::new(10, 15)),
            type_annotation: Some(Span::new(17, 23)),
        },
        declaration_origin: Some(std::sync::Arc::from("/decl.ts")),
    };
    let surface = ProjectedSurface {
        members: vec![member],
        call_signatures: Vec::new(),
        construct_signatures: Vec::new(),
        index_signatures: Vec::new(),
        has_index_signature: false,
    };

    let expr = projected_surface_to_type_expr(&surface)
        .expect("a one-member surface should reconstruct to an object type");
    let TypeExpr::Object(object) = &expr else {
        panic!("expected an object type, got {expr:?}");
    };
    let ObjectMember::Property(property) = &object.properties[0] else {
        panic!("expected a property member, got {:?}", object.properties[0]);
    };
    assert_eq!(
        property.spans.declaration,
        Some(Span::new(10, 24)),
        "the threaded declaration span must round-trip onto the IR property"
    );
    assert_eq!(
        property.spans.name,
        Some(Span::new(10, 15)),
        "the threaded name span must round-trip onto the IR property"
    );
    assert_eq!(
        property.spans.type_annotation,
        Some(Span::new(17, 23)),
        "the threaded type-annotation span must round-trip onto the IR property"
    );
}

/// D1 span threading (negative): the GENUINELY synthetic open-surface index
/// signature stays span-`None`. `ProjectedSurface` carries only a
/// `has_index_signature: bool` — no declared key/value nodes, hence no single
/// OXC declaration site — so the reconstruction must NOT fabricate spans.
///
/// Discriminating: a reconstruction that fabricated a non-`None` span here
/// (e.g. a byte-0 placeholder) would fail these `None` asserts.
#[test]
fn projected_surface_to_type_expr_keeps_synthetic_index_signature_span_none() {
    use verter_type_expr::ObjectMember;

    let surface = ProjectedSurface {
        members: Vec::new(),
        call_signatures: Vec::new(),
        construct_signatures: Vec::new(),
        index_signatures: Vec::new(),
        has_index_signature: true,
    };

    let expr = projected_surface_to_type_expr(&surface)
        .expect("an open surface should reconstruct to an object type");
    let TypeExpr::Object(object) = &expr else {
        panic!("expected an object type, got {expr:?}");
    };
    let index = object
        .properties
        .iter()
        .find_map(|member| match member {
            ObjectMember::IndexSignature(sig) => Some(sig),
            _ => None,
        })
        .expect("open surface must reconstruct an index signature");
    assert_eq!(
        index.spans.declaration, None,
        "synthetic open-surface index signature has no source site — must NOT fabricate a span"
    );
    assert_eq!(
        index.spans.key, None,
        "synthetic open-surface index signature key has no source site"
    );
    assert_eq!(
        index.spans.value, None,
        "synthetic open-surface index signature value has no source site"
    );
}

/// FIX 1 (positive): a REAL `[k: string]: number` index signature carried
/// structurally on `ProjectedSurface::index_signatures` round-trips its
/// declared key/value SHAPE *and* its real OXC spans through
/// `projected_surface_to_type_expr` — it is NOT collapsed to the synthetic
/// open-surface placeholder (`[key: string]: <projectedOpenSurface>` with
/// span-`None`).
///
/// The surface is projected from a REAL source declaration through the live
/// engine (mirroring the cross-file
/// `projected_member_declaration_origin_points_at_cross_file_declaration`
/// test), so the index signature's spans are genuine OXC offsets — NOT
/// fabricated constants. The discriminating asserts SLICE the source at those
/// offsets: the declaration span must slice to the index-signature text, the
/// key span to the index-parameter declaration `k: string`, and the value span
/// to `number`.
///
/// Discriminating: the pre-fix reconstruction emitted `IndexSignature::synthetic`
/// for any surface with `has_index_signature`, discarding the real value type
/// (`number`, replaced by the `projectedOpenSurface` placeholder) and every
/// span (forced to `None`). Against that tree the value-type assert and every
/// `Some(..)`/slice assert below fail.
#[test]
fn projected_surface_to_type_expr_reemits_real_index_signature_shape_and_spans() {
    use verter_type_expr::{ObjectMember, PrimitiveName};

    // A REAL type-literal alias carrying a concrete `[k: string]: number` index
    // signature. A `type X = { .. }` alias body is a `TypeExpr::Object`, so the
    // engine projects it through the IR object-expr path
    // (`projected_surface_from_object_expr`), which preserves the SOURCE key
    // parameter name (`k`) and the real OXC spans lowered by
    // `verter_type_expr_oxc::lower_ts_type`. (The graph `SurfaceView` path
    // hardcodes `key_name = "key"`; we deliberately drive the IR path here so
    // the asserted `key_name` is the source-declared `k`.)
    const SRC: &str = "export type Indexed = { [k: string]: number }\n";
    const FILE: &str = "/src/indexed.ts";

    let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
        verter_workspace::MemoryOptions::default(),
    ));
    ws.inject_file(FILE.to_string(), Arc::from(SRC));

    let host = VerterHost::new(
        HostConfig {
            analysis_level: AnalysisLevel::Full,
            ..HostConfig::default()
        },
        ws,
    );
    assert!(host.ensure_loaded(FILE));

    let mut engine = ComponentMetaQueryEngine::new(&host);
    let surface = engine
        .project_prepared_root_surface(FILE, "Indexed")
        .expect("`type Indexed = { [k: string]: number }` must project a surface");

    // Sanity: the projected surface carries exactly one CONCRETE index signature
    // (not merely the open-surface bool) sourced from the real declaration.
    assert_eq!(
        surface.index_signatures.len(),
        1,
        "the concrete `[k: string]: number` must be carried structurally, got {:?}",
        surface.index_signatures
    );

    let expr = projected_surface_to_type_expr(&surface)
        .expect("a surface with a concrete index signature reconstructs to an object type");
    let TypeExpr::Object(object) = &expr else {
        panic!("expected an object type, got {expr:?}");
    };
    let index = object
        .properties
        .iter()
        .find_map(|member| match member {
            ObjectMember::IndexSignature(sig) => Some(sig),
            _ => None,
        })
        .expect("the concrete index signature must reconstruct an index signature");

    // Shape: the real `string` key and `number` value survive — NOT the open
    // `projectedOpenSurface` placeholder value.
    assert_eq!(
        index.key_type,
        TypeExpr::Primitive(PrimitiveName::String),
        "the declared key type must round-trip"
    );
    assert_eq!(
        index.value_type,
        TypeExpr::Primitive(PrimitiveName::Number),
        "the declared value type must round-trip (not the open `projectedOpenSurface` placeholder)"
    );

    // key_name: the IR object-expr path preserves the SOURCE parameter name `k`
    // (the graph path would have forced `key`). This proves we drove the IR
    // path and that the source name survived the reconstruction.
    assert_eq!(
        index.key_name, "k",
        "the IR object-expr path must preserve the source key parameter name `k`"
    );

    // Spans are REAL OXC offsets: slicing SRC at each span yields the exact
    // source token. A synthetic placeholder (the pre-fix state) carries
    // span-`None`, so each `.expect(..)` below would panic on revert.
    let declaration_span = index
        .spans
        .declaration
        .expect("a concrete index signature must carry a real declaration span");
    assert_eq!(
        &SRC[declaration_span.start as usize..declaration_span.end as usize],
        "[k: string]: number",
        "the declaration span must slice SRC to the full index-signature text"
    );

    // The OXC `key` span is the index PARAMETER declaration (`k: string` — the
    // binding name plus its type annotation), per
    // `verter_type_expr_oxc::lower_ts_type` (`param.span`). Slicing SRC at that
    // span yields the full parameter text, which contains both the source key
    // name `k` and the key type `string`.
    let key_span = index
        .spans
        .key
        .expect("a concrete index signature must carry a real key span");
    assert_eq!(
        &SRC[key_span.start as usize..key_span.end as usize],
        "k: string",
        "the key span must slice SRC to the index-parameter declaration `k: string`"
    );

    let value_span = index
        .spans
        .value
        .expect("a concrete index signature must carry a real value span");
    assert_eq!(
        &SRC[value_span.start as usize..value_span.end as usize],
        "number",
        "the value-type span must slice SRC to `number`"
    );

    // The value span sits WITHIN the declaration span — the fabricated-constant
    // version had the value extending past the declaration end, which a real
    // OXC span never does.
    assert!(
        value_span.start >= declaration_span.start && value_span.end <= declaration_span.end,
        "the real value span ({value_span:?}) must be contained in the declaration span \
         ({declaration_span:?})"
    );

    // Exactly one index signature — the concrete one. The open placeholder must
    // NOT also be appended.
    let index_count = object
        .properties
        .iter()
        .filter(|member| matches!(member, ObjectMember::IndexSignature(_)))
        .count();
    assert_eq!(
        index_count, 1,
        "a concrete index signature must NOT additionally emit the open placeholder"
    );
}

/// FIX 2 (U3b fix-cycle, codex#2 P1): a SYNTHESIZED union common-member has NO
/// single OXC declaration site, so its spans + declaration file must be cleared
/// (`None`) — NOT carried verbatim from the FIRST arm.
///
/// `{ a: string } | { a: number }` with each arm's `a` carrying DISTINCT real
/// spans + a distinct declaration file. The merged `a: string | number` member
/// is multi-origin: its spans must all be `None` and its `declaration_origin`
/// `None`.
///
/// Discriminating: pre-fix the union merge merged the second arm into the FIRST
/// arm's `ProjectedMember` (rewriting only `ty` / `optional`), leaving the first
/// arm's spans + declaration file in place. Against that tree the merged member
/// reports the `string` arm's span/file and these `None` asserts fail.
#[test]
fn projected_surface_from_parts_union_clears_synthesized_member_span_and_origin() {
    use verter_span::Span;
    use verter_type_expr::{MemberSpans, PrimitiveName};

    fn arm(
        value: PrimitiveName,
        spans: MemberSpans,
        origin: &str,
    ) -> std::sync::Arc<ProjectedSurface> {
        std::sync::Arc::new(ProjectedSurface {
            members: vec![ProjectedMember {
                name: "a".to_string(),
                ty: TypeExpr::Primitive(value),
                optional: false,
                readonly: false,
                is_method: false,
                declared_in_macro_type_arg: false,
                spans,
                declaration_origin: Some(std::sync::Arc::from(origin)),
            }],
            call_signatures: Vec::new(),
            construct_signatures: Vec::new(),
            index_signatures: Vec::new(),
            has_index_signature: false,
        })
    }

    // First arm `{ a: string }` and second arm `{ a: number }` — DISTINCT span
    // ranges + DISTINCT declaration files. Pre-fix, the first arm's
    // span/file leaks onto the synthesized union member.
    let first = arm(
        PrimitiveName::String,
        MemberSpans {
            declaration: Some(Span::new(2, 11)),
            name: Some(Span::new(2, 3)),
            type_annotation: Some(Span::new(5, 11)),
        },
        "/src/a.ts",
    );
    let second = arm(
        PrimitiveName::Number,
        MemberSpans {
            declaration: Some(Span::new(40, 49)),
            name: Some(Span::new(40, 41)),
            type_annotation: Some(Span::new(43, 49)),
        },
        "/src/b.ts",
    );

    let merged = match projected_surface_from_parts_union(vec![first, second]) {
        PreparedSurfaceProjection::Surface(surface) => surface,
        other => panic!("union of two non-empty arms must produce a Surface, got {other:?}"),
    };
    let member = merged
        .members
        .iter()
        .find(|m| m.name == "a")
        .expect("the common `a` member must survive the union merge");

    // The member's type IS the union (sanity: the merge did compose it).
    assert!(
        matches!(&member.ty, TypeExpr::Union(_)),
        "the merged `a` must be `string | number`, got {:?}",
        member.ty
    );

    // INVARIANT: a synthesized multi-origin member carries NO span/origin.
    assert_eq!(
        member.spans,
        MemberSpans::default(),
        "synthesized union common-member must clear ALL spans to None (not the first arm's)"
    );
    assert_eq!(
        member.declaration_origin, None,
        "synthesized union common-member must clear declaration_origin to None"
    );
}
