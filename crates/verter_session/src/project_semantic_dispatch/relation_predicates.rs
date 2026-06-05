//! Leaf relation predicates for the semantic-node assignability engine.
//!
//! These are the structural, non-owning predicate functions the relation
//! worklist (`decide_relation` / `expand_pair` in [`super::relation`]) calls
//! to decide individual sub-judgements: primitive widening, literal equality,
//! `RelationResult` AND/OR combination, object/property/index-signature
//! structural relation, and function-shape variance. They operate on
//! [`SemanticNodeData`] exclusively and never reach into the arena.
//!
//! The structural object/function predicates recurse back into the worklist
//! authority via [`super::relation::decide_relation`], so source/target
//! members and call signatures are decided through the same memoised engine
//! rather than a private drill-down path.

use std::sync::Arc;

use super::relation::decide_relation;
use crate::semantic_query::{
    FunctionParam, IndexSignature, InferBinding, LiteralValue, PrimitiveKind, RelationResult,
    SemanticNodeData, SemanticNodeId, SurfaceMember, SurfaceView,
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
            | SemanticNodeData::TypeOf { .. }
            | SemanticNodeData::TemplateLiteral { .. }
            // DeclRef/InstantiationRef carriers are
            // unresolved references whose concrete content depends on
            // instantiation. Treat as deferred at the recursive-pair
            // level so callers (especially build_conditional) preserve
            // both branches when the carrier appears in the check
            // position. The OUTER `decide_relation_with_dispatch` call
            // unwraps via `unwrap_identity_carrier_for_relation`, but
            // that unwrap fires once at the top — recursive `expand_pair`
            // calls see carriers verbatim. Treating them as deferred
            // keeps the relation engine safely conservative there.
            | SemanticNodeData::DeclRef { .. }
            | SemanticNodeData::InstantiationRef { .. }
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

pub(super) fn literals_equal(a: &LiteralValue, b: &LiteralValue) -> bool {
    match (a, b) {
        (LiteralValue::String(s1), LiteralValue::String(s2)) => s1 == s2,
        (LiteralValue::Number(n1), LiteralValue::Number(n2)) => n1.to_bits() == n2.to_bits(),
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
                if !merged.iter().any(|existing| existing.name == binding.name) {
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

/// Relate two object `SurfaceView`s structurally. Every required target
/// member must be satisfied by a matching source member (or an
/// applicable source index signature). Optional target members accept
/// absence.
pub(super) fn relate_objects(
    graph: &SemanticGraphStore,
    source: &SurfaceView,
    target: &SurfaceView,
    bindings: &mut Vec<InferBinding>,
) -> RelationResult {
    let mut acc = RelationResult::Assignable {
        bindings: Arc::from(Vec::new().into_boxed_slice()),
    };
    for t_prop in target.members.iter() {
        let prop_result =
            if let Some(s_prop) = source.members.iter().find(|p| p.name == t_prop.name) {
                relate_property_pair(graph, s_prop, t_prop, bindings)
            } else if let Some(index_result) =
                relate_property_via_source_index(graph, source, t_prop, bindings)
            {
                index_result
            } else if t_prop.optional {
                assignable(bindings)
            } else {
                RelationResult::NotAssignable
            };
        acc = result_and(acc, prop_result);
        if matches!(acc, RelationResult::NotAssignable) {
            return RelationResult::NotAssignable;
        }
    }
    for t_index in target.index_signatures.iter() {
        let index_result = relate_target_index_signature(graph, source, t_index, bindings);
        acc = result_and(acc, index_result);
        if matches!(acc, RelationResult::NotAssignable) {
            return RelationResult::NotAssignable;
        }
    }
    // Call signatures: every target signature must have an assignable
    // source counterpart.
    for t_sig in target.call_signatures.iter() {
        let sig_ok = source.call_signatures.iter().any(|s_sig| {
            matches!(
                decide_relation(graph, *s_sig, *t_sig, bindings),
                RelationResult::Assignable { .. }
            )
        });
        if !sig_ok {
            acc = result_and(acc, RelationResult::NotAssignable);
            return acc;
        }
    }
    for t_sig in target.construct_signatures.iter() {
        let sig_ok = source.construct_signatures.iter().any(|s_sig| {
            matches!(
                decide_relation(graph, *s_sig, *t_sig, bindings),
                RelationResult::Assignable { .. }
            )
        });
        if !sig_ok {
            acc = result_and(acc, RelationResult::NotAssignable);
            return acc;
        }
    }
    acc
}

pub(super) fn relate_property_pair(
    graph: &SemanticGraphStore,
    source: &SurfaceMember,
    target: &SurfaceMember,
    bindings: &mut Vec<InferBinding>,
) -> RelationResult {
    // Readonly widening: if target is mutable but source is readonly,
    // the property would allow writing through the target interface.
    if !target.readonly && source.readonly {
        return RelationResult::NotAssignable;
    }
    decide_relation(graph, source.value, target.value, bindings)
}

pub(super) fn relate_property_via_source_index(
    graph: &SemanticGraphStore,
    source: &SurfaceView,
    target_prop: &SurfaceMember,
    bindings: &mut Vec<InferBinding>,
) -> Option<RelationResult> {
    let mut matched = false;
    let mut acc = RelationResult::Assignable {
        bindings: Arc::from(Vec::new().into_boxed_slice()),
    };
    for s_index in source.index_signatures.iter() {
        if !index_signature_applies_to_property(graph, s_index.key_type, target_prop.name.as_ref())
        {
            continue;
        }
        matched = true;
        let r = decide_relation(graph, s_index.value_type, target_prop.value, bindings);
        acc = result_and(acc, r);
        if matches!(acc, RelationResult::NotAssignable) {
            return Some(RelationResult::NotAssignable);
        }
    }
    matched.then_some(acc)
}

pub(super) fn relate_target_index_signature(
    graph: &SemanticGraphStore,
    source: &SurfaceView,
    target_index: &IndexSignature,
    bindings: &mut Vec<InferBinding>,
) -> RelationResult {
    let mut acc = RelationResult::Assignable {
        bindings: Arc::from(Vec::new().into_boxed_slice()),
    };
    for s_index in source.index_signatures.iter() {
        if !index_domains_overlap(graph, s_index.key_type, target_index.key_type) {
            continue;
        }
        let r = decide_relation(graph, s_index.value_type, target_index.value_type, bindings);
        acc = result_and(acc, r);
        if matches!(acc, RelationResult::NotAssignable) {
            return RelationResult::NotAssignable;
        }
    }
    for prop in source.members.iter() {
        if !index_signature_applies_to_property(graph, target_index.key_type, prop.name.as_ref()) {
            continue;
        }
        let r = decide_relation(graph, prop.value, target_index.value_type, bindings);
        acc = result_and(acc, r);
        if matches!(acc, RelationResult::NotAssignable) {
            return RelationResult::NotAssignable;
        }
    }
    acc
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
            SemanticNodeData::Primitive(
                PrimitiveKind::Number | PrimitiveKind::Any | PrimitiveKind::Unknown
            ) | SemanticNodeData::Literal(LiteralValue::Number(_))
                | SemanticNodeData::Union(_)
        ),
        SemanticNodeData::Literal(LiteralValue::String(name)) => {
            index_signature_applies_to_property(graph, source_key, name)
        }
        SemanticNodeData::Literal(LiteralValue::Number(n)) => {
            index_signature_applies_to_property(graph, source_key, &format_numeric_property(*n))
        }
        SemanticNodeData::Union(members) => {
            let members = Arc::clone(members);
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
    property_name: &str,
) -> bool {
    let Some(data) = graph.node_data(key_type) else {
        return false;
    };
    match &*data {
        SemanticNodeData::Primitive(PrimitiveKind::String | PrimitiveKind::Any) => true,
        SemanticNodeData::Primitive(PrimitiveKind::Number) => property_name.parse::<u64>().is_ok(),
        SemanticNodeData::Literal(LiteralValue::String(name)) => name == property_name,
        SemanticNodeData::Literal(LiteralValue::Number(n)) => {
            format_numeric_property(*n) == property_name
        }
        SemanticNodeData::Union(members) => {
            let members = Arc::clone(members);
            drop(data);
            members
                .iter()
                .any(|&member| index_signature_applies_to_property(graph, member, property_name))
        }
        _ => false,
    }
}

pub(super) fn format_numeric_property(value: f64) -> String {
    if value.fract() == 0.0 {
        format!("{value:.0}")
    } else {
        value.to_string()
    }
}

/// Relate two [`SemanticNodeData::Function`] shells. Parameter variance
/// is contravariant, return is covariant, arity is checked (target may
/// be narrower — missing leading params are not allowed, but target
/// may have fewer trailing required params than source).
pub(super) fn relate_function(
    graph: &SemanticGraphStore,
    source_params: &[FunctionParam],
    source_return: SemanticNodeId,
    target_params: &[FunctionParam],
    target_return: SemanticNodeId,
    bindings: &mut Vec<InferBinding>,
) -> RelationResult {
    // TS function assignability: target's required parameter count may
    // exceed source's (source accepts more args than target forwards);
    // source's required count may not exceed target's.
    let source_required = source_params
        .iter()
        .filter(|p| !p.optional && !p.rest)
        .count();
    let target_required = target_params
        .iter()
        .filter(|p| !p.optional && !p.rest)
        .count();
    if target_required < source_required {
        return RelationResult::NotAssignable;
    }
    let mut acc = RelationResult::Assignable {
        bindings: Arc::from(Vec::new().into_boxed_slice()),
    };
    for (s_param, t_param) in source_params.iter().zip(target_params.iter()) {
        // Contravariant: target param ≤ source param.
        let r = decide_relation(graph, t_param.ty, s_param.ty, bindings);
        acc = result_and(acc, r);
        if matches!(acc, RelationResult::NotAssignable) {
            return RelationResult::NotAssignable;
        }
    }
    // Covariant return.
    let r = decide_relation(graph, source_return, target_return, bindings);
    result_and(acc, r)
}

/// Relate a function source against an object target that carries call
/// signatures. The object is assignable iff at least one of its call
/// signatures is satisfied by the source function's signature.
pub(super) fn relate_function_to_object(
    graph: &SemanticGraphStore,
    source_params: &[FunctionParam],
    source_return: SemanticNodeId,
    target: &SurfaceView,
    bindings: &mut Vec<InferBinding>,
) -> RelationResult {
    if target.call_signatures.is_empty() && target.members.is_empty() {
        return assignable(bindings);
    }
    // For every declared member on the target object, the source
    // function (which has no own properties) cannot satisfy it unless
    // it's optional.
    for m in target.members.iter() {
        if !m.optional {
            return RelationResult::NotAssignable;
        }
    }
    // Call-signature assignment: find at least one target signature
    // the source function shape matches.
    if target.call_signatures.is_empty() {
        return assignable(bindings);
    }
    for t_sig in target.call_signatures.iter() {
        let Some(t_sig_data) = graph.node_data(*t_sig) else {
            continue;
        };
        if let SemanticNodeData::Function {
            params: t_params,
            return_type: t_ret,
            ..
        } = &*t_sig_data
        {
            let t_params = Arc::clone(t_params);
            let t_ret = *t_ret;
            drop(t_sig_data);
            let r = relate_function(
                graph,
                source_params,
                source_return,
                &t_params,
                t_ret,
                bindings,
            );
            if matches!(r, RelationResult::Assignable { .. }) {
                return r;
            }
        }
    }
    RelationResult::NotAssignable
}
