//! The widening a variable declaration's inferred type takes at each
//! position its initializer closes in a mutable location and reads a value
//! binding at (`getWidenedLiteralType` over the initializer's type).
//!
//! A read is as fresh as its binding: `const s = "a"` is read as the fresh
//! `"a"`, so `let x = s` is `string` and `let x = { v: s }` is `{ v: string
//! }`, while a read of `let sl: "a"` keeps `"a"`. A fresh literal the
//! position holds beside a regular read of the same literal is that regular
//! literal (`let x = c ? "a" : sl` is `"a"`). Only the binding's resolution
//! knows its freshness, so the producer keeps such a position's fresh
//! literals as written and records the position
//! ([`verter_type_expr::DeclaredReadProvenance`]); the declaration's
//! `typeof` widens it here, after its type is lowered.

use std::sync::Arc;

use crate::semantic_query::{
    ObjectConstructionEffect, ObjectSpreadProgram, PartialReasonSet, PrimitiveKind,
    ProjectionReductionContext, SemanticNodeData, SemanticNodeId, SurfaceKeyProjection,
    SurfaceMember,
};
use verter_type_expr::{DeclaredReadPosition, DeclaredReadStep};

use super::ProjectSemanticDispatch;

impl<C: crate::resolver_core::ResolverCapabilities> ProjectSemanticDispatch<'_, C> {
    /// `node`, the type the value declaration `prepared` lowers to (or the
    /// object of the own members a member read lowers), with each position
    /// its initializer closes in a mutable
    /// location widened by the freshness of the bindings it reads. A
    /// declaration whose inferred type defers no such widening
    /// (`defers_read_widening`) is `node` as it is. The positions are the
    /// declaration's transient value parts'; a broken lease on them folds
    /// a transient partial, so the unwidened type is never warm-admitted.
    pub(super) fn widen_declared_read_positions(
        &self,
        node: SemanticNodeId,
        prepared: &verter_session_query::type_solver::PreparedValueDecl,
    ) -> SemanticNodeId {
        if !prepared.type_annotation.defers_read_widening {
            return node;
        }
        let identity = &prepared.root_identity;
        let (canonical, owner, symbol) = (
            identity.canonical_id.as_ref(),
            identity.owner,
            identity.symbol_name.as_ref(),
        );
        let Some(serve) = self.ctx.ensure_indexed_ready_serve(canonical) else {
            self.fold_local_partial_completeness(PartialReasonSet::UNSTABLE_STATE);
            return node;
        };
        let parts =
            match self
                .ctx
                .transient_value_parts(&serve.indexed.shallow_state, owner, symbol)
            {
                verter_session_query::source::demand::DemandOutcome::Ready(Some(parts)) => parts,
                verter_session_query::source::demand::DemandOutcome::Ready(None) => return node,
                verter_session_query::source::demand::DemandOutcome::LeaseMiss => {
                    self.fold_local_partial_completeness(PartialReasonSet::UNSTABLE_STATE);
                    return node;
                }
            };
        let Some(provenance) = parts.read_provenance.as_ref() else {
            return node;
        };
        // The positions at one path — the elements of one array literal —
        // widen together; a position nested in another widens first, so
        // the outer position's rebuild holds the widened inner one.
        let mut groups: Vec<(&[DeclaredReadStep], Vec<&DeclaredReadPosition>)> = Vec::new();
        for position in provenance.positions.iter() {
            match groups
                .iter_mut()
                .find(|(path, _)| *path == position.path.as_ref())
            {
                Some((_, group)) => group.push(position),
                None => groups.push((position.path.as_ref(), vec![position])),
            }
        }
        groups.sort_by_key(|(path, _)| std::cmp::Reverse(path.len()));
        groups.into_iter().fold(node, |node, (path, group)| {
            let widen = self.declared_widening_literals(&group, canonical, owner);
            if widen.is_empty() {
                return node;
            }
            self.rewrite_declared_position(node, path, &|at| self.widen_literals_at(at, &widen))
        })
    }

    /// `node` with the position `path` addresses rewritten by `rewrite`: an
    /// object's member by its key and the authored property declaring it,
    /// an array's element type, through each member of a union. A path that
    /// addresses nothing leaves `node` as it is.
    fn rewrite_declared_position(
        &self,
        node: SemanticNodeId,
        path: &[DeclaredReadStep],
        rewrite: &dyn Fn(SemanticNodeId) -> SemanticNodeId,
    ) -> SemanticNodeId {
        let Some((step, rest)) = path.split_first() else {
            return rewrite(node);
        };
        let graph = self.graph();
        let data = graph.node_data(node);
        match (data.as_deref(), step) {
            (Some(SemanticNodeData::Union(members)), _) => {
                let members: Vec<SemanticNodeId> = members.iter().copied().collect();
                drop(data);
                let rewritten: Vec<SemanticNodeId> = members
                    .iter()
                    .map(|&member| self.rewrite_declared_position(member, path, rewrite))
                    .collect();
                if rewritten == members {
                    node
                } else {
                    self.intern_normalized_union_or_intersection(&rewritten, true)
                }
            }
            (Some(SemanticNodeData::Object(surface)), DeclaredReadStep::Member { key, at }) => {
                let surface = surface.clone();
                drop(data);
                let SurfaceKeyProjection::Exact(member) = surface.project_known_key(key) else {
                    return node;
                };
                if !written_at(&member.spans, *at) {
                    return node;
                }
                let Some(index) = surface
                    .positive_members()
                    .iter()
                    .position(|candidate| std::ptr::eq(candidate, member))
                else {
                    return node;
                };
                let value = self.rewrite_declared_position(member.value, rest, rewrite);
                if value == member.value {
                    return node;
                }
                let mut members: Vec<SurfaceMember> = surface
                    .positive_members()
                    .iter()
                    .map(SurfaceMember::clone)
                    .collect();
                members[index].value = value;
                graph.intern_preserving_scope(
                    node,
                    SemanticNodeData::Object(
                        surface.with_positive_members(Arc::from(members.into_boxed_slice())),
                    ),
                )
            }
            // An object that spreads another addresses a member by the
            // authored property writing it: its own write of the key, which
            // a later spread may override but never rewrites, or a member of
            // a spread's operand, which the object holds as its own.
            (
                Some(SemanticNodeData::ObjectSpreadProgram(program)),
                DeclaredReadStep::Member { key, at },
            ) => {
                let mut effects: Vec<ObjectConstructionEffect> = program
                    .effects
                    .iter()
                    .map(ObjectConstructionEffect::copy)
                    .collect();
                drop(data);
                let mut changed = false;
                for effect in &mut effects {
                    match effect {
                        ObjectConstructionEffect::DirectProperty(property)
                            if property.key.cloned_known().as_ref() == Some(key)
                                && written_at(&property.spans, *at) =>
                        {
                            let value =
                                self.rewrite_declared_position(property.value, rest, rewrite);
                            changed |= value != property.value;
                            property.value = value;
                        }
                        ObjectConstructionEffect::Spread(operand) => {
                            let value = self.rewrite_declared_position(*operand, path, rewrite);
                            changed |= value != *operand;
                            *operand = value;
                        }
                        _ => {}
                    }
                }
                if !changed {
                    return node;
                }
                graph.intern_preserving_scope(
                    node,
                    SemanticNodeData::ObjectSpreadProgram(ObjectSpreadProgram {
                        effects: crate::semantic_query::PayloadArray::from(effects),
                    }),
                )
            }
            (Some(SemanticNodeData::Array { element, readonly }), DeclaredReadStep::Element) => {
                let (element, readonly) = (*element, *readonly);
                drop(data);
                let rewritten = self.rewrite_declared_position(element, rest, rewrite);
                if rewritten == element {
                    return node;
                }
                graph.intern_preserving_scope(
                    node,
                    SemanticNodeData::Array {
                        element: rewritten,
                        readonly,
                    },
                )
            }
            _ => node,
        }
    }

    /// The literals the positions `group` hold that a mutable location
    /// widens (`getWidenedLiteralType`): each position's fresh literals and
    /// the literals of the reads whose bindings keep a widening literal,
    /// except the ones the same position holds as regular — written
    /// asserted or in a const context, or held by a read whose binding
    /// keeps no widening literal.
    fn declared_widening_literals(
        &self,
        group: &[&DeclaredReadPosition],
        canonical: &str,
        owner: verter_type_expr::TopLevelOwnerId,
    ) -> Vec<SemanticNodeId> {
        let graph = self.graph();
        let read_literals = |read: &verter_type_expr::ValueRef| -> Vec<SemanticNodeId> {
            let Some(read_type) = self.lower_type_expr_in_owner_scope_with_context(
                canonical,
                owner,
                &verter_type_expr::TypeExpr::TypeOf(read.clone()),
                ProjectionReductionContext::structural_transit(),
            ) else {
                return Vec::new();
            };
            literal_like_members(graph, read_type)
        };
        let mut widen = Vec::new();
        for position in group {
            let (widening, regular_reads): (Vec<_>, Vec<_>) = position
                .reads
                .iter()
                .partition(|read| self.value_read_widens(canonical, owner, &read.path));
            if position.fresh.is_empty() && widening.is_empty() {
                continue;
            }
            let intern = |literal: &crate::semantic_query::LiteralValue| {
                graph.intern_node(SemanticNodeData::Literal(literal.clone()))
            };
            let mut regular: Vec<SemanticNodeId> = position.regular.iter().map(intern).collect();
            for read in regular_reads {
                regular.extend(read_literals(read));
            }
            let candidates = position
                .fresh
                .iter()
                .map(intern)
                .chain(widening.into_iter().flat_map(&read_literals));
            for candidate in candidates {
                let listed = |nodes: &[SemanticNodeId]| {
                    nodes
                        .iter()
                        .any(|&node| same_literal(graph, node, candidate))
                };
                if !listed(&regular) && !listed(&widen) {
                    widen.push(candidate);
                }
            }
        }
        widen
    }

    /// `at` with each of its literals in `widen` widened to its primitive
    /// (an enum member's literal to its enum), a literal beside its widened
    /// primitive being that primitive.
    fn widen_literals_at(&self, at: SemanticNodeId, widen: &[SemanticNodeId]) -> SemanticNodeId {
        let graph = self.graph();
        let members: Vec<SemanticNodeId> = match graph.node_data(at).as_deref() {
            Some(SemanticNodeData::Union(members)) => members.iter().copied().collect(),
            _ => vec![at],
        };
        let widened: Vec<SemanticNodeId> = members
            .iter()
            .map(|&member| {
                if widen
                    .iter()
                    .any(|&literal| same_literal(graph, literal, member))
                {
                    self.widened_literal(member)
                } else {
                    member
                }
            })
            .collect();
        if widened == members {
            return at;
        }
        let held: Vec<PrimitiveKind> = widened
            .iter()
            .filter_map(|&member| match graph.node_data(member).as_deref() {
                Some(SemanticNodeData::Primitive(kind)) => Some(*kind),
                _ => None,
            })
            .collect();
        let mut reduced: Vec<SemanticNodeId> = Vec::with_capacity(widened.len());
        for member in widened {
            let absorbed = matches!(
                graph.node_data(member).as_deref(),
                Some(SemanticNodeData::Literal(literal)) if held.contains(&literal_primitive(literal))
            );
            if !absorbed && !reduced.contains(&member) {
                reduced.push(member);
            }
        }
        match reduced.as_slice() {
            [one] => *one,
            _ => self.intern_normalized_union_or_intersection(&reduced, true),
        }
    }
}

/// Whether the member `spans` locate is the one the authored property
/// declared at `at` writes; a member without a declaration site is
/// addressed by its key alone.
fn written_at(spans: &verter_type_expr::MemberSpans, at: verter_span::Span) -> bool {
    spans
        .declaration
        .is_none_or(|declaration| declaration == at)
}

/// Whether `a` and `b` are the same literal type, whatever scope each
/// node was interned in.
fn same_literal(
    graph: &crate::semantic_query_memo::SemanticGraphStore,
    a: SemanticNodeId,
    b: SemanticNodeId,
) -> bool {
    a == b
        || match (graph.node_data(a).as_deref(), graph.node_data(b).as_deref()) {
            (Some(SemanticNodeData::Literal(a)), Some(SemanticNodeData::Literal(b))) => a == b,
            (Some(SemanticNodeData::EnumLiteral(a)), Some(SemanticNodeData::EnumLiteral(b))) => {
                a == b
            }
            _ => false,
        }
}

/// The literal constituents of `node` — scalar or enum-member literals —
/// itself when a literal, its literal members when a union.
fn literal_like_members(
    graph: &crate::semantic_query_memo::SemanticGraphStore,
    node: SemanticNodeId,
) -> Vec<SemanticNodeId> {
    let is_literal = |node: SemanticNodeId| {
        matches!(
            graph.node_data(node).as_deref(),
            Some(SemanticNodeData::Literal(_) | SemanticNodeData::EnumLiteral(_))
        )
    };
    match graph.node_data(node).as_deref() {
        Some(SemanticNodeData::Union(members)) => members
            .iter()
            .copied()
            .filter(|member| is_literal(*member))
            .collect(),
        _ if is_literal(node) => vec![node],
        _ => Vec::new(),
    }
}

/// The primitive a literal widens to.
pub(super) fn literal_primitive(literal: &crate::semantic_query::LiteralValue) -> PrimitiveKind {
    match literal {
        crate::semantic_query::LiteralValue::String(_) => PrimitiveKind::String,
        crate::semantic_query::LiteralValue::Number(_) => PrimitiveKind::Number,
        crate::semantic_query::LiteralValue::Boolean(_) => PrimitiveKind::Boolean,
        crate::semantic_query::LiteralValue::BigInt(_) => PrimitiveKind::BigInt,
    }
}
