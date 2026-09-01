//! Leaf relation predicates for the semantic-node assignability engine.
//!
//! These are the structural, non-owning predicate functions the relation
//! worklist ([`super::relation`]) calls to decide individual sub-judgements:
//! primitive widening, literal equality, `RelationResult` AND/OR
//! combination, and the index-signature domain predicates. They operate on
//! [`SemanticNodeData`] exclusively and never reach into the arena.
//!
//! The structural object/function predicates are METHODS on
//! [`super::ProjectSemanticDispatch`] (`relate_objects` /
//! `relate_property_pair` / `relate_function` / … in [`super::relation`]):
//! every recursive sub-relation re-enters the SAME full-key
//! `execute(SemanticQueryKey::Relate)` authority through
//! `ProjectSemanticDispatch::relate_member` — there is no hidden recursion
//! path and no private drill-down.

use std::sync::Arc;

use crate::semantic_query::{
    InferBinding, LiteralValue, PrimitiveKind, RelationResult, SemanticNodeData, SemanticNodeId,
};
use crate::semantic_query_memo::SemanticGraphStore;

pub(super) fn assignable(bindings: &[InferBinding]) -> RelationResult {
    RelationResult::Assignable {
        bindings: Arc::from(bindings.to_vec().into_boxed_slice()),
    }
}

pub(super) fn is_deferred(data: &SemanticNodeData) -> bool {
    matches!(
        data,
        SemanticNodeData::KeyOf { .. }
            | SemanticNodeData::IndexedAccess { .. }
            | SemanticNodeData::Mapped { .. }
            | SemanticNodeData::Conditional { .. }
            | SemanticNodeData::TypeOf(_)
            | SemanticNodeData::TemplateLiteral { .. }
            // The unresolved-reference carriers (`DeclRef` / `InstantiationRef`
            // / `BareRef` / `ImportType`) are references whose concrete content
            // depends on demand-time carrier resolution / instantiation. Treat
            // them as deferred at the recursive-pair level so callers
            // (especially build_conditional) preserve both branches when the
            // carrier appears in the check position, and so a recursive
            // `expand_pair` defers to `Unknown` instead of falling through to
            // the `NotAssignable` "different concrete kinds" default. The OUTER
            // `decide_relation_with_dispatch` call unwraps `DeclRef` /
            // `InstantiationRef` via `unwrap_identity_carrier_for_relation`, but
            // that unwrap fires once at the top — recursive `expand_pair` calls
            // see carriers verbatim. Treating them as deferred keeps the
            // relation engine safely conservative there. A RESOLVED root
            // (`Object` / `Primitive` / `Function` / …) is matched by its own
            // arm before the default, so this classification never changes a
            // resolved-root verdict.
            | SemanticNodeData::DeclRef { .. }
            | SemanticNodeData::InstantiationRef { .. }
            | SemanticNodeData::BareRef(_)
            | SemanticNodeData::ImportType(_)
    )
}

pub(super) fn relate_primitives(
    source: PrimitiveKind,
    target: PrimitiveKind,
    bindings: &[InferBinding],
) -> RelationResult {
    if source == target || (source == PrimitiveKind::Undefined && target == PrimitiveKind::Void) {
        assignable(bindings)
    } else {
        RelationResult::NotAssignable
    }
}

pub(super) fn relate_literal_to_primitive(
    literal: &LiteralValue,
    target: PrimitiveKind,
    bindings: &[InferBinding],
) -> RelationResult {
    let widens = matches!(
        (literal, target),
        (LiteralValue::String(_), PrimitiveKind::String)
            | (LiteralValue::Number(_), PrimitiveKind::Number)
            | (LiteralValue::Boolean(_), PrimitiveKind::Boolean)
            | (LiteralValue::BigInt(_), PrimitiveKind::BigInt)
    );
    if widens {
        assignable(bindings)
    } else {
        RelationResult::NotAssignable
    }
}

/// TypeScript literal-type identity for two literal payloads.
///
/// Numeric payloads compare SameValueZero — the keying the checker's
/// literal-type interning uses, where `0` and `-0` are ONE literal type —
/// NOT f64 bit identity. The producer boundary normalizes `-0.0`, so this
/// is the second line of defence: a stray unnormalized `-0` payload must
/// never be read as a DIFFERENT literal type (which would publish a wrong
/// `NotAssignable`). The same oracle backs the disjointness proof, so the
/// relation and canonical-algebra halves cannot disagree about which pairs
/// are one literal type.
pub(super) fn literals_equal(a: &LiteralValue, b: &LiteralValue) -> bool {
    match (a, b) {
        (LiteralValue::String(s1), LiteralValue::String(s2)) => s1 == s2,
        (LiteralValue::Number(n1), LiteralValue::Number(n2)) => {
            !crate::project_semantic_dispatch::canonical_algebra::numeric_literal_values_disjoint(
                *n1, *n2,
            )
        }
        (LiteralValue::Boolean(b1), LiteralValue::Boolean(b2)) => b1 == b2,
        (LiteralValue::BigInt(s1), LiteralValue::BigInt(s2)) => s1 == s2,
        _ => false,
    }
}

pub(super) fn result_and(a: RelationResult, b: RelationResult) -> RelationResult {
    match (a, b) {
        (RelationResult::NotAssignable, _) | (_, RelationResult::NotAssignable) => {
            RelationResult::NotAssignable
        }
        (RelationResult::Unknown, _) | (_, RelationResult::Unknown) => RelationResult::Unknown,
        (
            RelationResult::Assignable { bindings: a },
            RelationResult::Assignable { bindings: b },
        ) => {
            let mut merged: Vec<InferBinding> = a.iter().cloned().collect();
            for binding in b.iter() {
                if !merged
                    .iter()
                    .any(|existing| existing.param == binding.param)
                {
                    merged.push(binding.clone());
                }
            }
            RelationResult::Assignable {
                bindings: Arc::from(merged.into_boxed_slice()),
            }
        }
    }
}

pub(super) fn result_or(a: RelationResult, b: RelationResult) -> RelationResult {
    match (a, b) {
        (RelationResult::Assignable { bindings: a }, _) => {
            RelationResult::Assignable { bindings: a }
        }
        (_, RelationResult::Assignable { bindings: b }) => {
            RelationResult::Assignable { bindings: b }
        }
        (RelationResult::Unknown, _) | (_, RelationResult::Unknown) => RelationResult::Unknown,
        (RelationResult::NotAssignable, RelationResult::NotAssignable) => {
            RelationResult::NotAssignable
        }
    }
}

pub(super) fn index_domains_overlap(
    graph: &SemanticGraphStore,
    source_key: SemanticNodeId,
    target_key: SemanticNodeId,
) -> bool {
    let Some(target_data) = graph.node_data(target_key) else {
        return false;
    };
    let Some(source_data) = graph.node_data(source_key) else {
        return false;
    };
    match &*target_data {
        SemanticNodeData::Primitive(PrimitiveKind::String | PrimitiveKind::Any) => matches!(
            &*source_data,
            SemanticNodeData::Primitive(
                PrimitiveKind::String
                    | PrimitiveKind::Number
                    | PrimitiveKind::Any
                    | PrimitiveKind::Unknown
            ) | SemanticNodeData::Literal(_)
                | SemanticNodeData::Union(_)
        ),
        SemanticNodeData::Primitive(PrimitiveKind::Number) => matches!(
            &*source_data,
            // A string index covers the number domain for value relating:
            // numeric keys are strings at runtime, so tsc and the legacy
            // `relate_target_index_signature` path both accept
            // `{[k: string]: X} <= {[k: number]: X}` (the payload relation
            // still applies — string-valued vs number-valued rejects).
            SemanticNodeData::Primitive(
                PrimitiveKind::String
                    | PrimitiveKind::Number
                    | PrimitiveKind::Any
                    | PrimitiveKind::Unknown
            ) | SemanticNodeData::Literal(LiteralValue::Number(_))
                | SemanticNodeData::Union(_)
        ),
        SemanticNodeData::Literal(LiteralValue::String(name)) => {
            index_signature_applies_to_property(
                graph,
                source_key,
                &crate::semantic_query::PropertyKey::string_literal(name.as_str()),
            )
        }
        SemanticNodeData::Literal(LiteralValue::Number(n)) => index_signature_applies_to_property(
            graph,
            source_key,
            &crate::semantic_query::PropertyKey::from_js_number(*n),
        ),
        SemanticNodeData::Union(members) => {
            let members = members.members_arc();
            drop(target_data);
            members
                .iter()
                .any(|&member| index_domains_overlap(graph, source_key, member))
        }
        _ => false,
    }
}

pub(super) fn index_signature_applies_to_property(
    graph: &SemanticGraphStore,
    key_type: SemanticNodeId,
    property_key: &crate::semantic_query::PropertyKey,
) -> bool {
    let Some(data) = graph.node_data(key_type) else {
        return false;
    };
    match &*data {
        SemanticNodeData::Primitive(PrimitiveKind::Any) => true,
        SemanticNodeData::Primitive(PrimitiveKind::String) => {
            matches!(
                property_key,
                crate::semantic_query::PropertyKey::String(_)
                    | crate::semantic_query::PropertyKey::Number(_)
            )
        }
        SemanticNodeData::Primitive(PrimitiveKind::Symbol) => {
            matches!(
                property_key,
                crate::semantic_query::PropertyKey::UniqueSymbol(_)
            )
        }
        SemanticNodeData::Primitive(PrimitiveKind::Number) => match property_key {
            crate::semantic_query::PropertyKey::Number(_) => true,
            crate::semantic_query::PropertyKey::String(name) => is_numeric_literal_name(name),
            crate::semantic_query::PropertyKey::UniqueSymbol(_) => false,
        },
        SemanticNodeData::Literal(LiteralValue::String(name)) => match property_key {
            crate::semantic_query::PropertyKey::String(property_name) => {
                name.as_str() == property_name.as_ref()
            }
            crate::semantic_query::PropertyKey::Number(number) => {
                name.as_str() == number.to_string()
            }
            crate::semantic_query::PropertyKey::UniqueSymbol(_) => false,
        },
        SemanticNodeData::Literal(LiteralValue::Number(n)) => {
            let spelling = super::build::js_number_to_string(*n);
            match property_key {
                crate::semantic_query::PropertyKey::String(property_name) => {
                    spelling == property_name.as_ref()
                }
                crate::semantic_query::PropertyKey::Number(number) => {
                    spelling == number.to_string()
                }
                crate::semantic_query::PropertyKey::UniqueSymbol(_) => false,
            }
        }
        SemanticNodeData::Union(members) => {
            let members = members.members_arc();
            drop(data);
            members
                .iter()
                .any(|&member| index_signature_applies_to_property(graph, member, property_key))
        }
        _ => false,
    }
}

/// TS numeric-literal-name rule for a `[n: number]` index signature:
/// a property name is numeric iff it round-trips through the JS number
/// canonicalizer — `String(Number(name)) === name` (pinned tsgo,
/// probe16 d1–d13: `"1.5"` / `"1e+21"` / `"-1"` / `"NaN"` /
/// `"Infinity"` are numeric names; `"01"` / `"1e21"` / `" 1"` / `"-0"`
/// are not). Routed through the single `js_number_to_string`
/// canonicalizer — never an integer-only parse, never a second
/// formatting path.
fn is_numeric_literal_name(property_name: &str) -> bool {
    property_name
        .parse::<f64>()
        .is_ok_and(|value| super::build::js_number_to_string(value) == property_name)
}

#[cfg(test)]
mod literal_identity_tests {
    use super::literals_equal;
    use verter_type_expr::LiteralValue;

    /// TypeScript interns `0` and `-0` as ONE numeric literal type (the
    /// checker keys literal types by `toString()`), so the relation
    /// engine's literal-identity oracle must judge them EQUAL. Raw f64 bit
    /// identity does not: `0.0f64.to_bits() != (-0.0f64).to_bits()`, which
    /// would publish a wrong `NotAssignable` for a stray unnormalized `-0`
    /// payload. The producer boundary normalizes `-0.0`; this oracle is the
    /// second line of defence, and it must not be selectively applied —
    /// the disjointness proof already compares SameValueZero.
    #[test]
    fn numeric_literal_identity_is_same_value_zero_not_bit_identity() {
        assert!(
            literals_equal(&LiteralValue::Number(0.0), &LiteralValue::Number(-0.0_f64)),
            "`0` and `-0` are one TypeScript literal type"
        );
        assert!(
            literals_equal(&LiteralValue::Number(-0.0_f64), &LiteralValue::Number(0.0)),
            "the oracle is symmetric"
        );
        // The guard must not widen identity anywhere else.
        assert!(!literals_equal(
            &LiteralValue::Number(0.0),
            &LiteralValue::Number(1.0)
        ));
        assert!(!literals_equal(
            &LiteralValue::Number(1.0),
            &LiteralValue::Number(-1.0)
        ));
        assert!(literals_equal(
            &LiteralValue::Number(1.5),
            &LiteralValue::Number(1.5)
        ));
        assert!(!literals_equal(
            &LiteralValue::Number(0.0),
            &LiteralValue::String("0".to_string())
        ));
    }
}
