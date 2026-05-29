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

use super::*;
use crate::resolver_core::projected_surface_to_type_expr;
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
/// Discriminating: the pre-fix reconstruction emitted `IndexSignature::synthetic`
/// for any surface with `has_index_signature`, discarding the real value type
/// (here `number`) and every span. Against that tree the value-type and span
/// asserts below fail.
#[test]
fn projected_surface_to_type_expr_reemits_real_index_signature_shape_and_spans() {
    use verter_semantic::analysis::type_solver::query_engine::ProjectedIndexSignature;
    use verter_span::Span;
    use verter_type_expr::{IndexSignatureSpans, ObjectMember, PrimitiveName};

    // A concrete index signature `[k: string]: number` with real OXC spans, as
    // `surface_view_to_projected_surface` / the object-expr projection would
    // carry it.
    let surface = ProjectedSurface {
        members: Vec::new(),
        call_signatures: Vec::new(),
        construct_signatures: Vec::new(),
        index_signatures: vec![ProjectedIndexSignature {
            key_name: "k".to_string(),
            key_type: TypeExpr::Primitive(PrimitiveName::String),
            value_type: TypeExpr::Primitive(PrimitiveName::Number),
            readonly: false,
            spans: IndexSignatureSpans {
                declaration: Some(Span::new(30, 48)),
                key: Some(Span::new(31, 41)),
                value: Some(Span::new(44, 50)),
            },
            declaration_origin: Some(std::sync::Arc::from("/decl.ts")),
        }],
        // The surface HAS an index signature, but it is a CONCRETE one — so the
        // reconstruction must emit the real signature, NOT the open placeholder.
        has_index_signature: true,
    };

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
        "the declared value type must round-trip (not the open placeholder)"
    );

    // Spans: the real OXC declaration/key/value spans survive verbatim.
    assert_eq!(
        index.spans.declaration,
        Some(Span::new(30, 48)),
        "the real index-signature declaration span must round-trip"
    );
    assert_eq!(
        index.spans.key,
        Some(Span::new(31, 41)),
        "the real index-signature key span must round-trip"
    );
    assert_eq!(
        index.spans.value,
        Some(Span::new(44, 50)),
        "the real index-signature value span must round-trip"
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
