//! The shared summary-constructor layer for the node-domain shape engine —
//! the SINGLE source of the per-arm fact + tag + root-class formulas. Both
//! [`RaisedShapeAlg`](super::RaisedShapeAlg) (which additionally interns a
//! structural key) and [`RaisedFactsAlg`](super::RaisedFactsAlg) (which does
//! not), plus the root-only [`project_root_summary`](super::project_root_summary),
//! build their per-arm [`RaisedShapeSummary`](super::RaisedShapeSummary) through
//! these pure functions, so the `materialized` / `expanded_surface` / `tag` /
//! `root_kind` rules can never drift across the consumers. The functions take
//! ONLY the child facts they fold (never a key or interner).

use super::{
    FactShapeTag, RaisedRootKind, RaisedShapeFacts, RaisedShapeSummary, SEMANTIC_MISS,
    SEMANTIC_OBJECT_SURFACE,
};

/// Assemble a summary from the three facts + tag. `can_shell_raise` is
/// ALWAYS `true` for any value the fold produces (a `Some(result)`).
fn summary(materialized: bool, expanded_surface: bool, tag: FactShapeTag) -> RaisedShapeSummary {
    RaisedShapeSummary {
        facts: RaisedShapeFacts {
            can_shell_raise: true,
            materialized,
            expanded_surface,
        },
        tag,
        // Only the two sentinel-leaf constructors (`unknown` / `opaque_sentinel`)
        // set these true; every compound / non-sentinel term is `false` (its
        // ROOT is not a sentinel, even when a child is).
        root_unmaterialized_sentinel: false,
        root_semantic_miss_sentinel: false,
        // Default `Other`; the per-arm constructors that map to a root mirror
        // class (`reference_leaf` / `type_of` / `empty_object` / `key_of` /
        // `indexed_access` / `conditional` / `mapped` / `object_from_members`)
        // override it.
        root_kind: RaisedRootKind::Other,
    }
}

/// A materialized, expanded leaf with no special tag and no root-mirror class
/// (Primitive, Literal, Infer, RecursiveRef, SyntheticSlotBinding, ImportType,
/// TemplateLiteral, TypeParameter). A `Ref` carrier uses [`reference_leaf`]
/// instead so it carries [`RaisedRootKind::Reference`].
pub(super) fn materialized_expanded_leaf() -> RaisedShapeSummary {
    summary(true, true, FactShapeTag::Other)
}

/// A `Ref`-carrier leaf (`DeclRef` / `InstantiationRef` / `BareRef` /
/// `DeclPlaceholder`): facts/tag identical to [`materialized_expanded_leaf`]
/// (materialized + expanded, `Other` tag) — only `root_kind` differs, marking
/// the root as a `TypeExpr::Ref` (a published-operator surface root).
pub(super) fn reference_leaf() -> RaisedShapeSummary {
    let mut s = summary(true, true, FactShapeTag::Other);
    s.root_kind = RaisedRootKind::Reference;
    s
}

/// `Unknown { raw }`: materialized iff the raw is NOT an unmaterialized
/// sentinel; an expanded leaf; tagged `ObjectSurfaceSentinel` iff the raw is
/// exactly the object-surface sentinel (dropped from an intersection).
pub(super) fn unknown(raw: &str) -> RaisedShapeSummary {
    let materialized =
        !crate::project_semantic_dispatch::raise_sentinel::raw_is_unmaterialized_sentinel(raw);
    let tag = if raw == SEMANTIC_OBJECT_SURFACE {
        FactShapeTag::ObjectSurfaceSentinel
    } else {
        FactShapeTag::Other
    };
    let mut s = summary(materialized, true, tag);
    // The ROOT term IS this `Unknown { raw }`, so it is a root sentinel iff the
    // raw reads unmaterialised (`!materialized`), and the NARROWER miss-root iff
    // the raw is EXACTLY the `semanticMiss` spelling.
    s.root_unmaterialized_sentinel = !materialized;
    s.root_semantic_miss_sentinel = raw == SEMANTIC_MISS;
    s
}

/// A TYPED resolver-control sentinel (`Opaque(QueryError)` reaching the
/// reverse boundary, or a converted `fold_node` control arm): the
/// node-domain counterpart of [`unknown`], but classified DIRECTLY from the
/// typed [`QueryError`] via the shared sentinel authority instead of
/// re-spelling a raw string. `materialized` comes from the domain-neutral
/// `query_error_is_unmaterialized_sentinel`; the `tag` is mapped HERE — this
/// is where [`FactShapeTag`] lives — from the domain-neutral
/// `query_error_is_object_surface_sentinel` predicate, exactly mirroring the
/// `raw == SEMANTIC_OBJECT_SURFACE` tag rule [`unknown`] applies (the
/// `UnrepresentableSurface` carrier round-trips to that spelling natively, and
/// a text-bearing `Other("semanticObjectSurface")` payload round-trips to it
/// via the predicate's delegation — both tag `ObjectSurfaceSentinel`, exactly
/// as the raw rule would). Both predicates are held byte-for-byte in agreement
/// with the raw recogniser `unknown` uses (the no-drift contract), so this
/// path and the raw-string path classify a sentinel identically.
/// `expanded_surface` is always `true`, exactly as `unknown` passes.
pub(super) fn opaque_sentinel(err: &crate::semantic_query::QueryError) -> RaisedShapeSummary {
    use crate::project_semantic_dispatch::raise_sentinel::{
        query_error_is_object_surface_sentinel, query_error_is_semantic_miss_sentinel,
        query_error_is_unmaterialized_sentinel,
    };
    let materialized = !query_error_is_unmaterialized_sentinel(err);
    let tag = if query_error_is_object_surface_sentinel(err) {
        FactShapeTag::ObjectSurfaceSentinel
    } else {
        FactShapeTag::Other
    };
    let mut s = summary(materialized, true, tag);
    // The ROOT term IS this typed sentinel, so it is a root sentinel iff the
    // error reads unmaterialised (`!materialized`), and the NARROWER miss-root
    // iff the error round-trips to the `semanticMiss` spelling — classified
    // DIRECTLY from the typed variant via the shared authority, never by
    // re-spelling a raw string.
    s.root_unmaterialized_sentinel = !materialized;
    s.root_semantic_miss_sentinel = query_error_is_semantic_miss_sentinel(err);
    s
}

/// `TypeOf`: a materialized leaf but NOT an expanded surface.
pub(super) fn type_of() -> RaisedShapeSummary {
    let mut s = summary(true, false, FactShapeTag::Other);
    s.root_kind = RaisedRootKind::TypeOf;
    s
}

/// `Union`: materialized / expanded are the AND over all members.
pub(super) fn union(member_facts: impl Iterator<Item = RaisedShapeFacts>) -> RaisedShapeSummary {
    let (mut materialized, mut expanded) = (true, true);
    for f in member_facts {
        materialized &= f.materialized;
        expanded &= f.expanded_surface;
    }
    summary(materialized, expanded, FactShapeTag::Other)
}

/// `Intersection`: materialized / expanded are the AND over all surviving
/// arms (the fold has already dropped sentinel / empty-object arms).
pub(super) fn intersection(
    arm_facts: impl Iterator<Item = RaisedShapeFacts>,
) -> RaisedShapeSummary {
    let (mut materialized, mut expanded) = (true, true);
    for f in arm_facts {
        materialized &= f.materialized;
        expanded &= f.expanded_surface;
    }
    summary(materialized, expanded, FactShapeTag::Other)
}

/// The representable empty object `{}` — raises to `TypeExpr::Object([])`.
pub(super) fn empty_object() -> RaisedShapeSummary {
    let mut s = summary(true, true, FactShapeTag::EmptyObject);
    s.root_kind = RaisedRootKind::Object;
    s
}

/// `Array`: recurses `materialized` into its element; an expanded surface.
pub(super) fn array(element: RaisedShapeFacts) -> RaisedShapeSummary {
    summary(element.materialized, true, FactShapeTag::Other)
}

/// `Tuple`: materialized is the AND over all elements; an expanded surface.
pub(super) fn tuple(element_facts: impl Iterator<Item = RaisedShapeFacts>) -> RaisedShapeSummary {
    let materialized = element_facts.fold(true, |acc, f| acc & f.materialized);
    summary(materialized, true, FactShapeTag::Other)
}

/// `KeyOf`: recurses `materialized` into its inner; NOT an expanded surface.
pub(super) fn key_of(base: RaisedShapeFacts) -> RaisedShapeSummary {
    let mut s = summary(base.materialized, false, FactShapeTag::Other);
    s.root_kind = RaisedRootKind::KeyOf;
    s
}

/// `IndexedAccess`: materialized iff BOTH object + index are; NOT an
/// expanded surface.
pub(super) fn indexed_access(
    object: RaisedShapeFacts,
    index: RaisedShapeFacts,
) -> RaisedShapeSummary {
    let mut s = summary(
        object.materialized && index.materialized,
        false,
        FactShapeTag::Other,
    );
    s.root_kind = RaisedRootKind::IndexedAccess;
    s
}

/// `Conditional`: materialized iff ALL of check / extends / true / false
/// are; NOT an expanded surface.
pub(super) fn conditional(
    check: RaisedShapeFacts,
    extends: RaisedShapeFacts,
    true_type: RaisedShapeFacts,
    false_type: RaisedShapeFacts,
) -> RaisedShapeSummary {
    let materialized = check.materialized
        && extends.materialized
        && true_type.materialized
        && false_type.materialized;
    let mut s = summary(materialized, false, FactShapeTag::Other);
    s.root_kind = RaisedRootKind::Conditional;
    s
}

/// `Mapped`: materialized iff source + value (+ name_type, when present)
/// are; NOT an expanded surface. `value_root_semantic_miss` is the mapped
/// VALUE's OWN raised-root `semanticMiss` fact, carried into the root class so
/// the published-operator classifier suppresses EXACTLY the
/// `value == Unknown { raw == "semanticMiss" }` carrier the `TypeExpr`
/// predicate suppresses (publishing for any other value).
pub(super) fn mapped(
    source: RaisedShapeFacts,
    value: RaisedShapeFacts,
    name_type: Option<RaisedShapeFacts>,
    value_root_semantic_miss: bool,
) -> RaisedShapeSummary {
    let materialized =
        source.materialized && value.materialized && name_type.is_none_or(|n| n.materialized);
    let mut s = summary(materialized, false, FactShapeTag::Other);
    s.root_kind = RaisedRootKind::Mapped {
        value_is_semantic_miss: value_root_semantic_miss,
    };
    s
}

/// `Function`: carries the function's folded `materialized` fact; an
/// expanded surface; tagged `Function` (the `out_as_function` extraction
/// subject + the constructor-rewrap signature child).
pub(super) fn function(materialized: bool) -> RaisedShapeSummary {
    summary(materialized, true, FactShapeTag::Function)
}

/// `ConstructorType`: carries the signature's folded `materialized` fact; an
/// expanded surface; tagged `Other` (the rewrap reads the SIGNATURE child,
/// never the constructor itself, so it must NOT tag `Function`).
pub(super) fn constructor(materialized: bool) -> RaisedShapeSummary {
    summary(materialized, true, FactShapeTag::Other)
}

/// `Object` from surviving members: materialized is the AND over members; an
/// expanded surface; tagged `EmptyObject` when zero members survive
/// (defensive — mirrors the interner-readback of `Object([])`), else `Other`.
pub(super) fn object_from_members(
    member_materialized: impl Iterator<Item = bool>,
    is_empty: bool,
) -> RaisedShapeSummary {
    let materialized = member_materialized.fold(true, |acc, m| acc & m);
    let tag = if is_empty {
        FactShapeTag::EmptyObject
    } else {
        FactShapeTag::Other
    };
    let mut s = summary(materialized, true, tag);
    s.root_kind = RaisedRootKind::Object;
    s
}
