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
    AuthoredPropertyKey, ObjectConstructionEffect, ObjectSpreadProgram, PartialReasonSet,
    PrimitiveKind, ProjectionReductionContext, SemanticNodeData, SemanticNodeId,
    SurfaceKeyProjection, SurfaceMember, SurfaceView,
};
use verter_type_expr::{DeclaredReadPosition, DeclaredReadStep, TypeExpr};

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
        let positions: Vec<Addressed<'_>> = provenance
            .positions
            .iter()
            .map(|position| (position.path.as_ref(), position))
            .collect();
        let widening = DeclaredWidening {
            canonical,
            owner,
            arms: std::cell::RefCell::new(Vec::new()),
        };
        self.widen_positions_at(node, &positions, &widening)
    }

    /// `node` with each of `positions` it reaches widened: an object's
    /// member by its key and the authored property declaring it (by that
    /// property alone under a computed key), an array's element type, each
    /// member of a union — of a conditional arm's union, only the members
    /// the arm holds. The positions reaching one node — the elements of one
    /// array literal — widen together, after every position nested in it,
    /// so its rebuild holds the widened inner ones. A position that
    /// addresses nothing leaves `node` as it is.
    fn widen_positions_at(
        &self,
        node: SemanticNodeId,
        positions: &[Addressed<'_>],
        widening: &DeclaredWidening<'_>,
    ) -> SemanticNodeId {
        if positions.is_empty() {
            return node;
        }
        let graph = self.graph();
        let union_members: Option<Vec<SemanticNodeId>> = match graph.node_data(node).as_deref() {
            Some(SemanticNodeData::Union(members)) => Some(members.iter().copied().collect()),
            _ => None,
        };
        let mut here: Vec<&DeclaredReadPosition> = Vec::new();
        let mut deeper: Vec<Addressed<'_>> = Vec::new();
        let node = match union_members {
            Some(members) => {
                for &(path, position) in positions {
                    if path.is_empty() {
                        here.push(position);
                    } else {
                        deeper.push((path, position));
                    }
                }
                let mut reached = vec![false; deeper.len()];
                let mut rewritten = Vec::with_capacity(members.len());
                for &member in &members {
                    let mut at_member = Vec::new();
                    for (index, &(path, position)) in deeper.iter().enumerate() {
                        if let Some(rest) = self.arms_holding(path, member, widening) {
                            reached[index] = true;
                            at_member.push((rest, position));
                        }
                    }
                    rewritten.push(self.widen_positions_at(member, &at_member, widening));
                }
                // An arm holds the members its written type lowers to; one
                // holding none of them is a lowering this union does not
                // hold, never a position to drop silently.
                if reached.contains(&false) {
                    self.fold_local_partial_completeness(PartialReasonSet::UNSTABLE_STATE);
                }
                if rewritten == members {
                    node
                } else {
                    self.intern_normalized_union_or_intersection(&rewritten, true)
                }
            }
            None => {
                for &(path, position) in positions {
                    match self.arms_holding(path, node, widening) {
                        Some([]) => here.push(position),
                        Some(rest) => deeper.push((rest, position)),
                        None => {
                            self.fold_local_partial_completeness(PartialReasonSet::UNSTABLE_STATE)
                        }
                    }
                }
                self.widen_parts_at(node, &deeper, widening)
            }
        };
        if here.is_empty() {
            return node;
        }
        let widen = self.declared_widening_literals(&here, widening.canonical, widening.owner);
        if widen.is_empty() {
            return node;
        }
        self.widen_literals_at(node, &widen)
    }

    /// `path` past the conditional arms it starts with when each of them
    /// holds `node` — a member its written type lowers to, the same
    /// structure whichever lowering interned it — else `None`. An arm
    /// whose written type lowers to nothing holds nothing.
    fn arms_holding<'p>(
        &self,
        mut path: &'p [DeclaredReadStep],
        node: SemanticNodeId,
        widening: &DeclaredWidening<'_>,
    ) -> Option<&'p [DeclaredReadStep]> {
        while let Some((DeclaredReadStep::Arm { written }, rest)) = path.split_first() {
            let cached = widening
                .arms
                .borrow()
                .iter()
                .find(|(arm, _)| Arc::ptr_eq(arm, written))
                .map(|(_, members)| members.clone());
            let members = match cached {
                Some(members) => members,
                None => {
                    let members: Option<Arc<[SemanticNodeId]>> = self
                        .lower_type_expr_in_owner_scope_with_context(
                            widening.canonical,
                            widening.owner,
                            written,
                            ProjectionReductionContext::structural_transit(),
                        )
                        .map(|arm| self.canonical_written(arm))
                        .map(|arm| match self.graph().node_data(arm).as_deref() {
                            Some(SemanticNodeData::Union(members)) => {
                                members.iter().copied().collect()
                            }
                            _ => Arc::from([arm]),
                        });
                    widening
                        .arms
                        .borrow_mut()
                        .push((Arc::clone(written), members.clone()));
                    members
                }
            };
            let graph = self.graph();
            let held = members.is_some_and(|members| {
                members.iter().any(|&member| {
                    member == node
                        || crate::semantic_query::stable_key::provably_equal(graph, member, node)
                })
            });
            if !held {
                return None;
            }
            path = rest;
        }
        Some(path)
    }

    /// `node`, a written type lowered as authored, with its unions and
    /// intersections reduced as the declaration's inferred type holds them —
    /// through array elements and object members — so a member of that type
    /// is the same structure as the written part it lowers.
    fn canonical_written(&self, node: SemanticNodeId) -> SemanticNodeId {
        let graph = self.graph();
        let data = graph.node_data(node);
        let composite: Option<(Vec<SemanticNodeId>, bool)> = match data.as_deref() {
            Some(SemanticNodeData::Union(members)) => {
                Some((members.iter().copied().collect(), true))
            }
            Some(SemanticNodeData::Intersection(members)) => {
                Some((members.iter().copied().collect(), false))
            }
            _ => None,
        };
        if let Some((members, union)) = composite {
            drop(data);
            let members: Vec<SemanticNodeId> = members
                .into_iter()
                .map(|member| self.canonical_written(member))
                .collect();
            return self.intern_normalized_union_or_intersection(&members, union);
        }
        match data.as_deref() {
            Some(SemanticNodeData::Array { element, readonly }) => {
                let (element, readonly) = (*element, *readonly);
                drop(data);
                let canonical = self.canonical_written(element);
                if canonical == element {
                    return node;
                }
                graph.intern_preserving_scope(
                    node,
                    SemanticNodeData::Array {
                        element: canonical,
                        readonly,
                    },
                )
            }
            Some(SemanticNodeData::Object(surface)) => {
                let surface = surface.clone();
                drop(data);
                let mut changed = false;
                let members: Vec<SurfaceMember> = surface
                    .positive_members()
                    .iter()
                    .map(|member| {
                        let mut member = SurfaceMember::clone(member);
                        let value = self.canonical_written(member.value);
                        changed |= value != member.value;
                        member.value = value;
                        member
                    })
                    .collect();
                if !changed {
                    return node;
                }
                graph.intern_preserving_scope(
                    node,
                    SemanticNodeData::Object(
                        surface.with_positive_members(Arc::from(members.into_boxed_slice())),
                    ),
                )
            }
            _ => node,
        }
    }

    /// `node` (no union) with each of `positions` widened in the part its
    /// first step addresses: an object's member, the property or spread an
    /// object that spreads another writes it by, an array's element type.
    fn widen_parts_at(
        &self,
        node: SemanticNodeId,
        positions: &[Addressed<'_>],
        widening: &DeclaredWidening<'_>,
    ) -> SemanticNodeId {
        if positions.is_empty() {
            return node;
        }
        let graph = self.graph();
        let data = graph.node_data(node);
        match data.as_deref() {
            Some(SemanticNodeData::Object(surface)) => {
                let surface = surface.clone();
                drop(data);
                let mut members: Option<Vec<SurfaceMember>> = None;
                for (index, member) in surface.positive_members().iter().enumerate() {
                    let at_member: Vec<Addressed<'_>> = positions
                        .iter()
                        .filter_map(|&(path, position)| {
                            let (step, rest) = path.split_first()?;
                            (addressed_member(&surface, step) == Some(index))
                                .then_some((rest, position))
                        })
                        .collect();
                    let value = self.widen_positions_at(member.value, &at_member, widening);
                    if value != member.value {
                        members.get_or_insert_with(|| {
                            surface
                                .positive_members()
                                .iter()
                                .map(SurfaceMember::clone)
                                .collect()
                        })[index]
                            .value = value;
                    }
                }
                let Some(members) = members else {
                    return node;
                };
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
            Some(SemanticNodeData::ObjectSpreadProgram(program)) => {
                let is_literal = program.is_literal;
                let mut effects: Vec<ObjectConstructionEffect> = program
                    .effects
                    .iter()
                    .map(ObjectConstructionEffect::copy)
                    .collect();
                drop(data);
                let mut changed = false;
                for effect in &mut effects {
                    match effect {
                        ObjectConstructionEffect::DirectProperty(property) => {
                            let at_property: Vec<Addressed<'_>> = positions
                                .iter()
                                .filter_map(|&(path, position)| {
                                    let (step, rest) = path.split_first()?;
                                    addresses(step, &property.key, &property.spans)
                                        .then_some((rest, position))
                                })
                                .collect();
                            let value =
                                self.widen_positions_at(property.value, &at_property, widening);
                            changed |= value != property.value;
                            property.value = value;
                        }
                        ObjectConstructionEffect::Spread(operand) => {
                            let at_operand: Vec<Addressed<'_>> = positions
                                .iter()
                                .copied()
                                .filter(|(path, _)| {
                                    matches!(
                                        path.first(),
                                        Some(
                                            DeclaredReadStep::Member { .. }
                                                | DeclaredReadStep::ComputedMember { .. }
                                        )
                                    )
                                })
                                .collect();
                            let value = self.widen_positions_at(*operand, &at_operand, widening);
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
                        is_literal,
                        effects: crate::semantic_query::PayloadArray::from(effects),
                    }),
                )
            }
            Some(SemanticNodeData::Array { element, readonly }) => {
                let (element, readonly) = (*element, *readonly);
                drop(data);
                let at_element: Vec<Addressed<'_>> = positions
                    .iter()
                    .filter_map(|&(path, position)| match path.split_first() {
                        Some((DeclaredReadStep::Element, rest)) => Some((rest, position)),
                        _ => None,
                    })
                    .collect();
                let rewritten = self.widen_positions_at(element, &at_element, widening);
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

/// A position and the steps left from the node it has reached.
type Addressed<'p> = (&'p [DeclaredReadStep], &'p DeclaredReadPosition);

/// A conditional arm's written type and the members it lowers to (`None`
/// when it lowers to nothing).
type ArmMembers = (Arc<TypeExpr>, Option<Arc<[SemanticNodeId]>>);

/// The declaration whose positions widen, and the union members each
/// conditional arm it writes holds (its written type lowered once).
struct DeclaredWidening<'d> {
    canonical: &'d str,
    owner: verter_type_expr::TopLevelOwnerId,
    arms: std::cell::RefCell<Vec<ArmMembers>>,
}

/// The index of the member of `surface` the member step `step` addresses
/// ([`addresses`]).
fn addressed_member(surface: &SurfaceView, step: &DeclaredReadStep) -> Option<usize> {
    let member = match step {
        DeclaredReadStep::Member { key, .. } => match surface.project_known_key(key) {
            SurfaceKeyProjection::Exact(member) => member,
            _ => return None,
        },
        DeclaredReadStep::ComputedMember { at } => surface
            .positive_members()
            .iter()
            .find(|member| member.spans.declaration == Some(*at))?,
        DeclaredReadStep::Element | DeclaredReadStep::Arm { .. } => return None,
    };
    if !addresses(step, &member.key, &member.spans) {
        return None;
    }
    surface
        .positive_members()
        .iter()
        .position(|candidate| std::ptr::eq(candidate, member))
}

/// Whether the member step `step` addresses the member of `key` that
/// `spans` locate: a member of the step's key the authored property
/// declared at its site writes — a member without a declaration site is
/// addressed by its key alone — or, under a computed key, the member that
/// property writes, whatever its key resolves to.
fn addresses(
    step: &DeclaredReadStep,
    key: &AuthoredPropertyKey,
    spans: &verter_type_expr::MemberSpans,
) -> bool {
    match step {
        DeclaredReadStep::Member { key: step_key, at } => {
            key.cloned_known().as_ref() == Some(step_key)
                && spans
                    .declaration
                    .is_none_or(|declaration| declaration == *at)
        }
        DeclaredReadStep::ComputedMember { at } => spans.declaration == Some(*at),
        DeclaredReadStep::Element | DeclaredReadStep::Arm { .. } => false,
    }
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
fn literal_primitive(literal: &crate::semantic_query::LiteralValue) -> PrimitiveKind {
    match literal {
        crate::semantic_query::LiteralValue::String(_) => PrimitiveKind::String,
        crate::semantic_query::LiteralValue::Number(_) => PrimitiveKind::Number,
        crate::semantic_query::LiteralValue::Boolean(_) => PrimitiveKind::Boolean,
        crate::semantic_query::LiteralValue::BigInt(_) => PrimitiveKind::BigInt,
    }
}
