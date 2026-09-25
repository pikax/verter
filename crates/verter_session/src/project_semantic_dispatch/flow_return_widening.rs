//! The checker's object-literal normalisation of a widened value
//! (`getWidenedTypeWithContext` over fresh object literals).
//!
//! Widening a union of object-literal types gives every literal the
//! properties its object-literal siblings name and it lacks, each as an
//! optional `undefined` member: `c ? { a: 1 } : { a: 1, b: 2 }` returns
//! `{ a: number; b?: undefined } | { a: number; b: number }`. A property
//! of a literal widened in such a context is widened in the context of
//! the same property of its siblings, so the normalisation reaches nested
//! literals (`c ? { o: { a: 1 } } : { o: { a: 1, b: 2 } }`). A spread-built
//! literal receives the properties it lacks but names none to its
//! siblings; a type that is not an object literal (a declared object
//! type) neither contributes a property nor receives one.
//!
//! Where the widening applies is measured on TypeScript 7.0.2: a return
//! or yield join is widened as a whole; an array is widened through its
//! element as a whole; a lone object literal widens a union-valued
//! property as a whole, but not a union two object-literal properties
//! down (`{ o: { p: c ? { a: 1 } : { a: 1, b: 2 } } }` keeps `p`
//! un-normalised).

use std::sync::Arc;

use crate::semantic_query::{
    AuthoredPropertyKey, NullabilityPolicy, PrimitiveKind, SemanticNodeData, SemanticNodeId,
    SurfaceMember, SurfaceView,
};

use super::ProjectSemanticDispatch;

impl ProjectSemanticDispatch<'_> {
    /// `node` as the checker's widening of a value it types, with the
    /// union algebra of `nullability` for every rebuilt union.
    pub(super) fn normalize_widened_object_literals(
        &self,
        node: SemanticNodeId,
        nullability: NullabilityPolicy,
    ) -> SemanticNodeId {
        let data = self.graph().node_data(node);
        match data.as_deref() {
            Some(SemanticNodeData::Union(members)) => {
                let members: Vec<SemanticNodeId> = members.iter().copied().collect();
                drop(data);
                self.widen_union_in_context(node, &members, &members, nullability)
            }
            Some(SemanticNodeData::Array { element, readonly }) => {
                let (element, readonly) = (*element, *readonly);
                drop(data);
                self.widen_array_element(node, element, readonly, nullability)
            }
            Some(SemanticNodeData::Object(view)) if object_literal_surface(view) => {
                let view = view.clone();
                drop(data);
                // A lone literal widens each property as a whole value: a
                // union or an array property normalises, a nested literal
                // is kept as it is.
                let members: Vec<SurfaceMember> = view
                    .positive_members()
                    .iter()
                    .map(|member| {
                        let value = match self.graph().node_data(member.value).as_deref() {
                            Some(SemanticNodeData::Union(_) | SemanticNodeData::Array { .. }) => {
                                self.normalize_widened_object_literals(member.value, nullability)
                            }
                            _ => member.value,
                        };
                        SurfaceMember {
                            value,
                            ..member.clone()
                        }
                    })
                    .collect();
                self.rebuilt_literal(node, &view, members)
            }
            _ => node,
        }
    }

    /// `node` (a union with `members`) widened in the context of
    /// `siblings`: each member widened in that same context.
    fn widen_union_in_context(
        &self,
        node: SemanticNodeId,
        members: &[SemanticNodeId],
        siblings: &[SemanticNodeId],
        nullability: NullabilityPolicy,
    ) -> SemanticNodeId {
        let widened: Vec<SemanticNodeId> = members
            .iter()
            .map(|member| self.widen_in_context(*member, siblings, nullability))
            .collect();
        if widened.as_slice() == members {
            return node;
        }
        self.intern_normalized_union(&widened, nullability)
    }

    /// `node` widened in the context of `siblings` — the values the
    /// same position holds in the context's other object literals.
    fn widen_in_context(
        &self,
        node: SemanticNodeId,
        siblings: &[SemanticNodeId],
        nullability: NullabilityPolicy,
    ) -> SemanticNodeId {
        let data = self.graph().node_data(node);
        match data.as_deref() {
            Some(SemanticNodeData::Union(members)) => {
                let members: Vec<SemanticNodeId> = members.iter().copied().collect();
                drop(data);
                self.widen_union_in_context(node, &members, siblings, nullability)
            }
            Some(SemanticNodeData::Array { element, readonly }) => {
                let (element, readonly) = (*element, *readonly);
                drop(data);
                self.widen_array_element(node, element, readonly, nullability)
            }
            Some(SemanticNodeData::Object(view)) if object_literal_surface(view) => {
                let view = view.clone();
                drop(data);
                let context: Vec<SurfaceView> = siblings
                    .iter()
                    .filter_map(
                        |sibling| match self.graph().node_data(*sibling).as_deref() {
                            Some(SemanticNodeData::Object(sibling))
                                if object_literal_surface(sibling) =>
                            {
                                Some(sibling.clone())
                            }
                            _ => None,
                        },
                    )
                    .collect();
                let mut members: Vec<SurfaceMember> =
                    view.positive_members()
                        .iter()
                        .map(|member| {
                            let property_siblings: Vec<SemanticNodeId> = context
                                .iter()
                                .filter_map(|sibling| {
                                    sibling.positive_members().iter().find(|candidate| {
                                        same_property(&candidate.key, &member.key)
                                    })
                                })
                                .flat_map(|candidate| self.constituents(candidate.value))
                                .collect();
                            SurfaceMember {
                                value: self.widen_in_context(
                                    member.value,
                                    &property_siblings,
                                    nullability,
                                ),
                                ..member.clone()
                            }
                        })
                        .collect();
                let undefined = self
                    .graph()
                    .intern_node(SemanticNodeData::Primitive(PrimitiveKind::Undefined));
                for sibling in &context {
                    for candidate in sibling.positive_members() {
                        if members
                            .iter()
                            .any(|member| same_property(&member.key, &candidate.key))
                        {
                            continue;
                        }
                        members.push(SurfaceMember {
                            value: undefined,
                            optional: true,
                            readonly: false,
                            method_kind: None,
                            has_implementation_body: false,
                            ..candidate.clone()
                        });
                    }
                }
                self.rebuilt_literal(node, &view, members)
            }
            // A spread-built literal names no property to its siblings, but
            // receives the ones it lacks (`c ? { a: 1, q: 1 } : { ...s, a: 2
            // }` gives the spread arm `q?: undefined`): each is one more
            // direct write after its construction program.
            Some(SemanticNodeData::ObjectSpreadProgram(program)) => {
                let program = program.clone();
                drop(data);
                let Some(surface) = self.resolve_typeinfo_surface_view(
                    node,
                    crate::semantic_query::ProjectionReductionContext::structural_transit(),
                ) else {
                    return node;
                };
                let undefined = self
                    .graph()
                    .intern_node(SemanticNodeData::Primitive(PrimitiveKind::Undefined));
                let mut added: Vec<SurfaceMember> = Vec::new();
                for sibling in siblings {
                    let Some(SemanticNodeData::Object(sibling)) =
                        self.graph().node_data(*sibling).as_deref().cloned()
                    else {
                        continue;
                    };
                    if !object_literal_surface(&sibling) {
                        continue;
                    }
                    for candidate in sibling.positive_members() {
                        let named =
                            |member: &SurfaceMember| same_property(&member.key, &candidate.key);
                        if surface.positive_members().iter().any(named) || added.iter().any(named) {
                            continue;
                        }
                        added.push(SurfaceMember {
                            value: undefined,
                            optional: true,
                            readonly: false,
                            method_kind: None,
                            has_implementation_body: false,
                            ..candidate.clone()
                        });
                    }
                }
                if added.is_empty() {
                    return node;
                }
                let effects: Vec<crate::semantic_query::ObjectConstructionEffect> = program
                    .effects
                    .iter()
                    .cloned()
                    .chain(
                        added
                            .iter()
                            .map(super::object_spread_program_lowering::direct_effect_from_member),
                    )
                    .collect();
                self.graph()
                    .intern_node(SemanticNodeData::ObjectSpreadProgram(
                        crate::semantic_query::ObjectSpreadProgram {
                            effects: Arc::from(effects.into_boxed_slice()),
                        },
                    ))
            }
            _ => node,
        }
    }

    /// An array widened through its element as a whole value.
    fn widen_array_element(
        &self,
        node: SemanticNodeId,
        element: SemanticNodeId,
        readonly: bool,
        nullability: NullabilityPolicy,
    ) -> SemanticNodeId {
        let widened = self.normalize_widened_object_literals(element, nullability);
        if widened == element {
            return node;
        }
        self.graph().intern_node(SemanticNodeData::Array {
            element: widened,
            readonly,
        })
    }

    /// The union constituents of `node`, or `node` itself.
    fn constituents(&self, node: SemanticNodeId) -> Vec<SemanticNodeId> {
        match self.graph().node_data(node).as_deref() {
            Some(SemanticNodeData::Union(members)) => members.iter().copied().collect(),
            _ => vec![node],
        }
    }

    /// The literal `node` (surface `view`) with `members`, or `node`
    /// itself when they are its own.
    fn rebuilt_literal(
        &self,
        node: SemanticNodeId,
        view: &SurfaceView,
        members: Vec<SurfaceMember>,
    ) -> SemanticNodeId {
        if members.as_slice() == view.positive_members() {
            return node;
        }
        self.graph().intern_node(SemanticNodeData::Object(
            crate::semantic_query::surface_view! {
                members: Arc::from(members.into_boxed_slice()),
                call_signatures: Arc::clone(&view.call_signatures),
                construct_signatures: Arc::clone(&view.construct_signatures),
                index_signatures: Arc::clone(&view.index_signatures),
                keyspace: view.keyspace,
                has_index_signature: view.has_known_index_signature(),
            },
        ))
    }
}

/// Whether `view` is an object literal's own type: a surface whose every
/// member the literal wrote directly (`FreshOwn`), with no signature. An
/// empty surface names no literal member and is not recognised as one.
fn object_literal_surface(view: &SurfaceView) -> bool {
    view.call_signatures.is_empty()
        && view.construct_signatures.is_empty()
        && view.index_signatures.is_empty()
        && !view.positive_members().is_empty()
        && view.positive_members().iter().all(|member| {
            member.excess_origin == verter_type_expr::ExcessPropertyOrigin::FreshOwn
                && member.key.as_known().is_some()
        })
}

/// Whether two member keys name the same property.
fn same_property(a: &AuthoredPropertyKey, b: &AuthoredPropertyKey) -> bool {
    match (a.as_known(), b.as_known()) {
        (Some(a), Some(b)) => a.element_access_collides(&b),
        _ => false,
    }
}
