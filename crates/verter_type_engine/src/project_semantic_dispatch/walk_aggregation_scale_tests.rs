//! Surface merges aggregate members through one canonical property-identity
//! index: the key work of a merge grows linearly with the members merged,
//! for disjoint arms and for arms whose every member collides under JS
//! property identity (`1` in one arm, `"1"` in the other).

use super::{
    merge_intersection_surfaces_with_graph, merge_union_surfaces, merge_union_surfaces_for_macro,
    ShallowSurface, ShallowSurfaceEntry, ShallowSurfaceMember,
};
use crate::semantic_query::{
    key_scan_comparisons_for_tests, AuthoredPropertyKey, CanonicalIndexInt, PrimitiveKind,
    SemanticNodeData,
};
use crate::semantic_query_memo::SemanticGraphStore;

const SIZES: [usize; 4] = [128, 256, 512, 1024];

#[derive(Clone, Copy, Debug)]
enum Arms {
    /// Arm one declares `a0..an`, arm two `b0..bn`.
    Disjoint,
    /// Arm one declares the numeric keys `0..n`, arm two their string
    /// spellings `"0".."n"`.
    Colliding,
}

#[derive(Clone, Copy, Debug)]
enum Merge {
    Intersection,
    UnionCommon,
    UnionMacro,
}

fn member(
    graph: &SemanticGraphStore,
    key: AuthoredPropertyKey,
    value: PrimitiveKind,
) -> ShallowSurfaceMember {
    ShallowSurfaceMember {
        excess_origin: verter_type_expr::ExcessPropertyOrigin::NonLiteral,
        key,
        value: graph.intern_node(SemanticNodeData::Primitive(value)),
        optional: false,
        readonly: false,
        method_kind: None,
        has_implementation_body: false,
        visibility: verter_type_expr::MemberVisibility::Public,
        declared_in_macro_type_arg: crate::semantic_query::MacroOwnBodyStamp::NEUTRAL,
        merge_role: crate::semantic_query::MergeRoleStamp::NEUTRAL,
        spans: verter_type_expr::MemberSpans::default(),
        declaration_origin: None,
    }
}

fn arms(graph: &SemanticGraphStore, shape: Arms, size: usize) -> Vec<Option<ShallowSurface>> {
    let surface = |keys: Vec<AuthoredPropertyKey>, value| {
        Some(ShallowSurface::from_entries(
            keys.into_iter()
                .map(|key| ShallowSurfaceEntry::Member(member(graph, key, value)))
                .collect(),
            None,
        ))
    };
    let (first, second): (Vec<_>, Vec<_>) = match shape {
        Arms::Disjoint => (0..size)
            .map(|i| {
                (
                    AuthoredPropertyKey::string(format!("a{i}")),
                    AuthoredPropertyKey::string(format!("b{i}")),
                )
            })
            .unzip(),
        Arms::Colliding => (0..size)
            .map(|i| {
                (
                    AuthoredPropertyKey::Number(
                        CanonicalIndexInt::from_canonical_i64(i as i64).unwrap(),
                    ),
                    AuthoredPropertyKey::string(i.to_string()),
                )
            })
            .unzip(),
    };
    vec![
        surface(first, PrimitiveKind::String),
        surface(second, PrimitiveKind::Number),
    ]
}

fn merge(
    graph: &SemanticGraphStore,
    kind: Merge,
    arms: &[Option<ShallowSurface>],
) -> ShallowSurface {
    let evidence = &mut Default::default();
    match kind {
        Merge::Intersection => merge_intersection_surfaces_with_graph(graph, arms, evidence),
        Merge::UnionCommon => merge_union_surfaces(graph, arms, evidence),
        Merge::UnionMacro => merge_union_surfaces_for_macro(graph, arms, evidence),
    }
    .expect("two object arms merge")
}

/// The merged rows a merge must publish: a colliding property is ONE row
/// under its first authored key (the numeric arm-one key); disjoint
/// properties are common to no arm, so only the union-common rule drops them.
fn assert_rows(
    graph: &SemanticGraphStore,
    merged: &ShallowSurface,
    kind: Merge,
    shape: Arms,
    size: usize,
) {
    let expected: Vec<(AuthoredPropertyKey, bool)> = match (shape, kind) {
        (Arms::Disjoint, Merge::UnionCommon) => Vec::new(),
        (Arms::Disjoint, Merge::Intersection | Merge::UnionMacro) => (0..size)
            .map(|i| AuthoredPropertyKey::string(format!("a{i}")))
            .chain((0..size).map(|i| AuthoredPropertyKey::string(format!("b{i}"))))
            .map(|key| (key, matches!(kind, Merge::UnionMacro)))
            .collect(),
        (Arms::Colliding, _) => (0..size)
            .map(|i| {
                (
                    AuthoredPropertyKey::Number(
                        CanonicalIndexInt::from_canonical_i64(i as i64).unwrap(),
                    ),
                    false,
                )
            })
            .collect(),
    };
    let actual: Vec<(AuthoredPropertyKey, bool)> = merged
        .members
        .iter()
        .map(|member| (member.key.clone(), member.optional))
        .collect();
    assert_eq!(actual, expected, "{kind:?} over {shape:?} arms of {size}");
    if let Arms::Colliding = shape {
        // The colliding rows combine BOTH arms' values, never one arm's.
        assert!(
            merged.members.iter().all(|member| !matches!(
                graph.node_data(member.value).as_deref(),
                Some(SemanticNodeData::Primitive(
                    PrimitiveKind::String | PrimitiveKind::Number
                ))
            )),
            "{kind:?}: a colliding row folds both arms' values"
        );
    }
}

#[test]
fn surface_merges_grow_key_work_linearly_for_disjoint_and_colliding_members() {
    for kind in [Merge::Intersection, Merge::UnionCommon, Merge::UnionMacro] {
        for shape in [Arms::Disjoint, Arms::Colliding] {
            let mut work = Vec::new();
            for size in SIZES {
                let graph = SemanticGraphStore::new();
                let arms = arms(&graph, shape, size);
                let before = key_scan_comparisons_for_tests();
                let merged = merge(&graph, kind, &arms);
                let spent = key_scan_comparisons_for_tests() - before;
                assert_rows(&graph, &merged, kind, shape, size);
                assert!(
                    spent >= 2 * size,
                    "{kind:?} over {shape:?} arms of {size}: every member is keyed, spent {spent}"
                );
                work.push(spent);
            }
            for pair in work.windows(2) {
                assert!(
                    pair[1] <= 2 * pair[0] + 16,
                    "{kind:?} over {shape:?} arms: key work must double at most when the \
                     members double: {work:?} at sizes {SIZES:?}"
                );
            }
        }
    }
}
