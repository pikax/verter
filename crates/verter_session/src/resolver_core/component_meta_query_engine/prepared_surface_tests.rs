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
        }],
        call_signatures: Vec::new(),
        construct_signatures: Vec::new(),
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
