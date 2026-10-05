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
    AuthoredPropertyKey, PrimitiveKind, SemanticNodeData, SemanticNodeId, SurfaceMember,
    SurfaceView,
};
use verter_session_query::flow::policy::NullabilityPolicy;

use super::ProjectSemanticDispatch;

/// One step of [`ProjectSemanticDispatch::normalize_widened_object_literals`]'s
/// walk: a node to widen, or a node whose widened parts are the last values
/// the walk produced, to rebuild from them.
enum Widening {
    /// Widen `node`: as a whole value when `siblings` is `None`, and in the
    /// context of `siblings` — the values the same position holds in the
    /// context's other object literals — otherwise.
    Visit {
        node: SemanticNodeId,
        siblings: Option<Arc<[SemanticNodeId]>>,
    },
    /// A union rebuilt from its members' widened values.
    Union {
        node: SemanticNodeId,
        members: Vec<SemanticNodeId>,
    },
    /// An array rebuilt from its element's widened value.
    Array {
        node: SemanticNodeId,
        element: SemanticNodeId,
        readonly: bool,
    },
    /// A lone literal rebuilt from the widened values of the members
    /// `widened` marks, in member order.
    LoneLiteral {
        node: SemanticNodeId,
        view: SurfaceView,
        widened: Vec<bool>,
    },
    /// A literal widened in `context` (its object-literal siblings), rebuilt
    /// from every member's widened value, with the properties its siblings
    /// name and it lacks added.
    LiteralInContext {
        node: SemanticNodeId,
        view: SurfaceView,
        context: Vec<SurfaceView>,
    },
}

impl<C: crate::resolver_core::ResolverCapabilities> ProjectSemanticDispatch<'_, C> {
    /// `node` as the checker's widening of a value it types, with the
    /// union algebra of `nullability` for every rebuilt union.
    ///
    /// The walk runs from a work list, rebuilding each node once its parts
    /// are widened, so a value nested any number of levels deep (arrays of
    /// arrays, literals in literals) costs no native level per level.
    pub(super) fn normalize_widened_object_literals(
        &self,
        node: SemanticNodeId,
        nullability: NullabilityPolicy,
    ) -> SemanticNodeId {
        let mut tasks = vec![Widening::Visit {
            node,
            siblings: None,
        }];
        let mut values: Vec<SemanticNodeId> = Vec::new();
        while let Some(task) = tasks.pop() {
            match task {
                Widening::Visit { node, siblings } => {
                    self.widening_visit(node, siblings, &mut tasks, &mut values);
                }
                Widening::Union { node, members } => {
                    let widened = values.split_off(values.len() - members.len());
                    values.push(if widened == members {
                        node
                    } else {
                        self.intern_normalized_union(&widened, nullability)
                    });
                }
                Widening::Array {
                    node,
                    element,
                    readonly,
                } => {
                    let widened = values.pop().expect("the array's widened element");
                    values.push(if widened == element {
                        node
                    } else {
                        self.graph().intern_node(SemanticNodeData::Array {
                            element: widened,
                            readonly,
                        })
                    });
                }
                Widening::LoneLiteral {
                    node,
                    view,
                    widened,
                } => {
                    let count = widened.iter().filter(|widened| **widened).count();
                    let mut parts = values.split_off(values.len() - count).into_iter();
                    let members: Vec<SurfaceMember> = view
                        .positive_members()
                        .iter()
                        .zip(&widened)
                        .map(|(member, widened)| SurfaceMember {
                            value: if *widened {
                                parts.next().expect("the member's widened value")
                            } else {
                                member.value
                            },
                            ..member.clone()
                        })
                        .collect();
                    values.push(self.rebuilt_literal(node, &view, members));
                }
                Widening::LiteralInContext {
                    node,
                    view,
                    context,
                } => {
                    let parts = values.split_off(values.len() - view.positive_members().len());
                    let mut members: Vec<SurfaceMember> = view
                        .positive_members()
                        .iter()
                        .zip(parts)
                        .map(|(member, value)| SurfaceMember {
                            value,
                            ..member.clone()
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
                    values.push(self.rebuilt_literal(node, &view, members));
                }
            }
        }
        values.pop().expect("the widened value")
    }

    /// Widen `node` (see [`Widening::Visit`]): its value when it has no
    /// part to widen; otherwise its rebuild, after the parts to widen first.
    ///
    /// A union widens each member in the context of its siblings (the
    /// union's own members for a whole value), an array its element as a
    /// whole value. A lone object literal widens each union- or
    /// array-valued property as a whole value and keeps a nested literal
    /// as it is; in a context, a literal widens each property in the
    /// context of the same property of its siblings and receives the
    /// properties they name and it lacks.
    fn widening_visit(
        &self,
        node: SemanticNodeId,
        siblings: Option<Arc<[SemanticNodeId]>>,
        tasks: &mut Vec<Widening>,
        values: &mut Vec<SemanticNodeId>,
    ) {
        let data = self.graph().node_data(node);
        match data.as_deref() {
            Some(SemanticNodeData::Union(members)) => {
                let members: Vec<SemanticNodeId> = members.iter().copied().collect();
                drop(data);
                let siblings = siblings.unwrap_or_else(|| Arc::from(members.as_slice()));
                let visits: Vec<Widening> = members
                    .iter()
                    .rev()
                    .map(|member| Widening::Visit {
                        node: *member,
                        siblings: Some(Arc::clone(&siblings)),
                    })
                    .collect();
                tasks.push(Widening::Union { node, members });
                tasks.extend(visits);
            }
            Some(SemanticNodeData::Array { element, readonly }) => {
                let (element, readonly) = (*element, *readonly);
                drop(data);
                tasks.push(Widening::Array {
                    node,
                    element,
                    readonly,
                });
                tasks.push(Widening::Visit {
                    node: element,
                    siblings: None,
                });
            }
            Some(SemanticNodeData::Object(view)) if object_literal_surface(view) => {
                let view = view.clone();
                drop(data);
                match siblings {
                    None => {
                        let widened: Vec<bool> = view
                            .positive_members()
                            .iter()
                            .map(|member| {
                                matches!(
                                    self.graph().node_data(member.value).as_deref(),
                                    Some(
                                        SemanticNodeData::Union(_) | SemanticNodeData::Array { .. }
                                    )
                                )
                            })
                            .collect();
                        let visits: Vec<Widening> = view
                            .positive_members()
                            .iter()
                            .zip(&widened)
                            .filter(|(_, widened)| **widened)
                            .rev()
                            .map(|(member, _)| Widening::Visit {
                                node: member.value,
                                siblings: None,
                            })
                            .collect();
                        tasks.push(Widening::LoneLiteral {
                            node,
                            view,
                            widened,
                        });
                        tasks.extend(visits);
                    }
                    Some(siblings) => {
                        let context: Vec<SurfaceView> = siblings
                            .iter()
                            .filter_map(|sibling| {
                                match self.graph().node_data(*sibling).as_deref() {
                                    Some(SemanticNodeData::Object(sibling))
                                        if object_literal_surface(sibling) =>
                                    {
                                        Some(sibling.clone())
                                    }
                                    _ => None,
                                }
                            })
                            .collect();
                        let visits: Vec<Widening> = view
                            .positive_members()
                            .iter()
                            .rev()
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
                                Widening::Visit {
                                    node: member.value,
                                    siblings: Some(Arc::from(property_siblings)),
                                }
                            })
                            .collect();
                        tasks.push(Widening::LiteralInContext {
                            node,
                            view,
                            context,
                        });
                        tasks.extend(visits);
                    }
                }
            }
            // A spread-built literal names no property to its siblings, but
            // receives the ones it lacks (`c ? { a: 1, q: 1 } : { ...s, a: 2
            // }` gives the spread arm `q?: undefined`): each is one more
            // direct write after its construction program. As a whole value
            // it is kept as it is.
            Some(SemanticNodeData::ObjectSpreadProgram(program)) => {
                let program = program.clone();
                drop(data);
                values.push(match siblings {
                    Some(siblings) => self.spread_literal_in_context(node, &program, &siblings),
                    None => node,
                });
            }
            _ => {
                drop(data);
                values.push(node);
            }
        }
    }

    /// The spread-built literal `node` (construction `program`) receiving
    /// the properties its object-literal `siblings` name and it lacks.
    fn spread_literal_in_context(
        &self,
        node: SemanticNodeId,
        program: &crate::semantic_query::ObjectSpreadProgram,
        siblings: &[SemanticNodeId],
    ) -> SemanticNodeId {
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
                let named = |member: &SurfaceMember| same_property(&member.key, &candidate.key);
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
