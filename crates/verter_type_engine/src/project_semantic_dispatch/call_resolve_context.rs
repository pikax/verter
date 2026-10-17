//! The contextual reads of a call's arguments: whether a context-sensitive
//! argument's contextual result reads an inference, the const context of
//! a literal member whose contextual type is a `const` type parameter nested
//! in the parameter's type, and the literals an argument nests where their
//! contextual type in the parameter is a literal context for them.

use std::sync::Arc;

use rustc_hash::FxHashSet;

use super::super::dispatch_txn::ContextKeptLiteral;
use super::super::inference::session::InferenceInfoSetup;
use super::super::ProjectSemanticDispatch;
use crate::semantic_query::{
    AuthoredPropertyKey, ConstParamPolicy, LiteralValue, PrimitiveKind, SemanticNodeData,
    SemanticNodeId, SignatureKind, TypeParamDecl,
};

/// A primitive kind a literal context is decided by.
#[derive(Clone, Copy, PartialEq, Eq)]
enum LiteralKind {
    String,
    Number,
    BigInt,
    Boolean,
}

impl LiteralKind {
    fn of(value: &LiteralValue) -> Self {
        match value {
            LiteralValue::String(_) => Self::String,
            LiteralValue::Number(_) => Self::Number,
            LiteralValue::BigInt(_) => Self::BigInt,
            LiteralValue::Boolean(_) => Self::Boolean,
        }
    }

    const fn bit(self) -> u8 {
        1 << self as u8
    }
}

impl<C: crate::resolver_core::ResolverCapabilities> ProjectSemanticDispatch<'_, C> {
    /// The inference setup of a call candidate's type parameters, each with
    /// its const policy.
    pub(super) fn call_inference_infos(
        &self,
        type_parameters: &[TypeParamDecl],
    ) -> Arc<[InferenceInfoSetup]> {
        type_parameters
            .iter()
            .map(|decl| {
                InferenceInfoSetup::for_call(
                    decl.param,
                    Arc::clone(&decl.name),
                    if decl.is_const {
                        ConstParamPolicy::Const
                    } else {
                        ConstParamPolicy::NonConst
                    },
                    decl.constraint.is_some(),
                )
            })
            .collect()
    }

    /// The literals the bare literal argument `source` nests at authored
    /// positions whose contextual type in the parameter type `target` is a
    /// literal context for them (`checkExpressionForMutableLocation`
    /// through `getWidenedLiteralLikeTypeForContextualType`). The
    /// argument's object members and array elements are its authored
    /// expressions, and each is decided WHOLE against its own contextual
    /// type before any deposit composes it: `{ v: c ? "a" : 1 }` against
    /// `{ v: T }` with `T extends string | {}` keeps `"a" | 1`, while
    /// `["a", 1]` against `T[]` keeps `"a"` alone. `declared` supplies the
    /// constraint of a candidate type parameter whose node carries none.
    /// The argument itself is no nested position: a naked top-level
    /// deposit keeps its literal regardless.
    ///
    /// Each kept literal is tied to the candidate type parameters its
    /// position's contextual type names, the only parameters a deposit
    /// from that position reaches: `{ a: "x", b: "x" }` against
    /// `{ a: T; b: U }` with `T extends string` keeps `"x"` for `T`
    /// alone, so `U` still infers `string`.
    pub(in crate::project_semantic_dispatch) fn nested_literal_context_keeps(
        &self,
        source: SemanticNodeId,
        target: SemanticNodeId,
        declared: &[TypeParamDecl],
    ) -> Vec<ContextKeptLiteral> {
        let graph = self.graph();
        let mut keeps = Vec::new();
        let mut pending = vec![(source, target, true)];
        let mut seen = FxHashSet::default();
        while let Some((source, target, root)) = pending.pop() {
            if !seen.insert((source, target)) {
                continue;
            }
            let data = graph.node_data(source);
            match data.as_deref() {
                Some(SemanticNodeData::Object(surface)) => {
                    let members: Vec<(AuthoredPropertyKey, SemanticNodeId)> = surface
                        .positive_members()
                        .iter()
                        .map(|member| (member.key.clone(), member.value))
                        .collect();
                    drop(data);
                    for (key, value) in members {
                        for contextual in self.contextual_member_types(target, &key) {
                            pending.push((value, contextual, false));
                        }
                    }
                }
                Some(SemanticNodeData::Tuple { elements, .. }) => {
                    // A position past a spread is no longer the element's own.
                    let values: Vec<SemanticNodeId> = elements
                        .iter()
                        .take_while(|element| !element.rest)
                        .map(|element| element.value)
                        .collect();
                    drop(data);
                    for (index, value) in values.into_iter().enumerate() {
                        for contextual in self.contextual_element_types(target, index) {
                            pending.push((value, contextual, false));
                        }
                    }
                }
                Some(SemanticNodeData::Literal(_) | SemanticNodeData::Union(_)) if !root => {
                    drop(data);
                    if self.literal_of_contextual_type(source, target, declared) {
                        let literals = literal_members(graph, source);
                        for decl in declared {
                            if self.mentions_node(target, decl.param) {
                                keeps.extend(literals.iter().map(|&literal| ContextKeptLiteral {
                                    literal,
                                    param: decl.param,
                                }));
                            }
                        }
                    }
                }
                _ => {}
            }
        }
        keeps
    }

    /// The contextual types an object literal member named `key` takes
    /// from `contextual` (`getTypeOfPropertyOfContextualType`): the
    /// declared member of each constituent that lists it, else the value
    /// type of the constituent's index signatures applicable to the key
    /// (`findApplicableIndexInfo`: a non-`string` one — `number` for a
    /// numeric name — before a `string` one).
    fn contextual_member_types(
        &self,
        contextual: SemanticNodeId,
        key: &AuthoredPropertyKey,
    ) -> Vec<SemanticNodeId> {
        let graph = self.graph();
        let contextual = self.resolved_reduction_view(contextual);
        let data = graph.node_data(contextual);
        let surface = match data.as_deref() {
            Some(composite @ (SemanticNodeData::Union(_) | SemanticNodeData::Intersection(_))) => {
                let members: Vec<SemanticNodeId> = composite
                    .composite_members()
                    .expect("composite arm")
                    .iter()
                    .copied()
                    .collect();
                drop(data);
                return members
                    .into_iter()
                    .flat_map(|member| self.contextual_member_types(member, key))
                    .collect();
            }
            Some(SemanticNodeData::Object(surface)) => surface.clone(),
            None
            | Some(
                SemanticNodeData::Primitive(_)
                | SemanticNodeData::Literal(_)
                | SemanticNodeData::TypeParam { .. }
                | SemanticNodeData::Tuple { .. }
                | SemanticNodeData::Array { .. },
            ) => return Vec::new(),
            Some(_) => {
                drop(data);
                match self.resolve_typeinfo_surface_view(
                    contextual,
                    crate::semantic_query::ProjectionReductionContext::structural_transit(),
                ) {
                    Some(surface) => surface,
                    None => return Vec::new(),
                }
            }
        };
        let named: Vec<SemanticNodeId> = surface
            .positive_members()
            .iter()
            .filter(|member| member.key == *key)
            .map(|member| member.value)
            .collect();
        if !named.is_empty() {
            return named;
        }
        let Some(known) = key.cloned_known() else {
            return Vec::new();
        };
        let applicable: Vec<(bool, SemanticNodeId)> = surface
            .index_signatures
            .iter()
            .filter(|signature| {
                super::super::relation_predicates::index_signature_applies_to_property(
                    graph,
                    signature.key_type,
                    &known,
                )
            })
            .map(|signature| {
                let string_key = matches!(
                    graph.node_data(signature.key_type).as_deref(),
                    Some(SemanticNodeData::Primitive(PrimitiveKind::String))
                );
                (string_key, signature.value_type)
            })
            .collect();
        let narrow = applicable.iter().any(|(string_key, _)| !string_key);
        applicable
            .into_iter()
            .filter(|(string_key, _)| !(narrow && *string_key))
            .map(|(_, value)| value)
            .collect()
    }

    /// The contextual types the array literal element at `index` takes
    /// from `contextual` (`getContextualTypeForElementExpression`): an
    /// array's element type, or a tuple's element at the position (its
    /// rest element's type past its fixed elements), per constituent.
    fn contextual_element_types(
        &self,
        contextual: SemanticNodeId,
        index: usize,
    ) -> Vec<SemanticNodeId> {
        let graph = self.graph();
        let contextual = self.resolved_reduction_view(contextual);
        let data = graph.node_data(contextual);
        match data.as_deref() {
            Some(composite @ (SemanticNodeData::Union(_) | SemanticNodeData::Intersection(_))) => {
                let members: Vec<SemanticNodeId> = composite
                    .composite_members()
                    .expect("composite arm")
                    .iter()
                    .copied()
                    .collect();
                drop(data);
                members
                    .into_iter()
                    .flat_map(|member| self.contextual_element_types(member, index))
                    .collect()
            }
            Some(SemanticNodeData::Array { element, .. }) => vec![*element],
            Some(SemanticNodeData::Tuple { elements, .. }) => {
                let fixed = elements.iter().take_while(|element| !element.rest).count();
                if index < fixed {
                    return vec![elements[index].value];
                }
                let Some(rest) = elements.iter().find(|element| element.rest) else {
                    return Vec::new();
                };
                match graph.node_data(rest.value).as_deref() {
                    Some(SemanticNodeData::Array { element, .. }) => vec![*element],
                    _ => Vec::new(),
                }
            }
            _ => Vec::new(),
        }
    }

    /// The checker's `isLiteralOfContextualType`: whether `contextual`
    /// keeps a literal `candidate` (a literal, or a union holding literals)
    /// — some constituent of `contextual` is a literal (or a `boolean`, a
    /// template literal, a `keyof`) of a kind the candidate holds, or a
    /// type parameter whose base constraint is that primitive kind or is
    /// itself such a literal context. A constraint is read through its
    /// named carriers (`T extends Text`, `type Text = string`) and through
    /// the constraints of the type parameters it names; `declared`
    /// supplies the constraint of a type parameter whose node carries none.
    pub(in crate::project_semantic_dispatch) fn literal_of_contextual_type(
        &self,
        candidate: SemanticNodeId,
        contextual: SemanticNodeId,
        declared: &[TypeParamDecl],
    ) -> bool {
        let graph = self.graph();
        let held = literal_members(graph, candidate)
            .into_iter()
            .fold(0u8, |held, literal| {
                match graph.node_data(literal).as_deref() {
                    Some(SemanticNodeData::Literal(value)) => held | LiteralKind::of(value).bit(),
                    _ => held,
                }
            });
        held != 0
            && self.literal_context_admits(held, contextual, declared, &mut FxHashSet::default())
    }

    /// Whether `contextual` is a literal context for a literal of one of
    /// the kinds `held` (a [`LiteralKind`] bit set). `visited` holds the
    /// nodes already read: a circular constraint (`T extends U`,
    /// `U extends T`, which the checker rejects) contributes no context,
    /// while an acyclic chain is followed to its end however long.
    fn literal_context_admits(
        &self,
        held: u8,
        contextual: SemanticNodeId,
        declared: &[TypeParamDecl],
        visited: &mut FxHashSet<SemanticNodeId>,
    ) -> bool {
        let graph = self.graph();
        let authored = contextual;
        let contextual = self.resolved_reduction_view(contextual);
        if !visited.insert(contextual) {
            return false;
        }
        let data = graph.node_data(contextual);
        let kinds = match data.as_deref() {
            Some(composite @ (SemanticNodeData::Union(_) | SemanticNodeData::Intersection(_))) => {
                let members: Vec<SemanticNodeId> = composite
                    .composite_members()
                    .expect("composite arm")
                    .iter()
                    .copied()
                    .collect();
                drop(data);
                return members
                    .into_iter()
                    .any(|member| self.literal_context_admits(held, member, declared, visited));
            }
            Some(SemanticNodeData::TypeParam { constraint, .. }) => {
                let constraint = constraint.or_else(|| {
                    declared
                        .iter()
                        .find(|decl| decl.param == contextual)
                        .and_then(|decl| decl.constraint)
                });
                drop(data);
                let Some(constraint) = constraint else {
                    return false;
                };
                return held & self.base_constraint_primitive_kinds(constraint, declared) != 0
                    || self.literal_context_admits(held, constraint, declared, visited);
            }
            Some(SemanticNodeData::Literal(value)) => LiteralKind::of(value).bit(),
            Some(SemanticNodeData::TemplateLiteral { .. } | SemanticNodeData::KeyOf { .. }) => {
                LiteralKind::String.bit()
            }
            // `boolean` is the union `true | false` to the checker.
            Some(SemanticNodeData::Primitive(PrimitiveKind::Boolean)) => LiteralKind::Boolean.bit(),
            _ => {
                drop(data);
                // A string mapping (`Uppercase<string>`) is a string
                // literal context, as a template literal is.
                if self.string_mapping_of(authored).is_some()
                    || self.string_mapping_of(contextual).is_some()
                {
                    LiteralKind::String.bit()
                } else {
                    0
                }
            }
        };
        held & kinds != 0
    }

    /// The primitive kinds `constraint`'s base constraint may be
    /// (`maybeTypeOfKind` over `getBaseConstraintOfType`): its `string`,
    /// `number` and `bigint` constituents, read through named carriers and
    /// the constraints of the type parameters it names; a generic `keyof`
    /// may be a `string` or a `number`.
    fn base_constraint_primitive_kinds(
        &self,
        constraint: SemanticNodeId,
        declared: &[TypeParamDecl],
    ) -> u8 {
        let graph = self.graph();
        let mut kinds = 0u8;
        let mut pending = vec![constraint];
        let mut seen = FxHashSet::default();
        while let Some(node) = pending.pop() {
            let node = self.resolved_reduction_view(node);
            if !seen.insert(node) {
                continue;
            }
            let data = graph.node_data(node);
            match data.as_deref() {
                Some(SemanticNodeData::Primitive(PrimitiveKind::String)) => {
                    kinds |= LiteralKind::String.bit();
                }
                Some(SemanticNodeData::Primitive(PrimitiveKind::Number)) => {
                    kinds |= LiteralKind::Number.bit();
                }
                Some(SemanticNodeData::Primitive(PrimitiveKind::BigInt)) => {
                    kinds |= LiteralKind::BigInt.bit();
                }
                Some(
                    composite @ (SemanticNodeData::Union(_) | SemanticNodeData::Intersection(_)),
                ) => pending.extend(
                    composite
                        .composite_members()
                        .expect("composite arm")
                        .iter()
                        .copied(),
                ),
                Some(SemanticNodeData::TypeParam { constraint, .. }) => {
                    pending.extend(constraint.or_else(|| {
                        declared
                            .iter()
                            .find(|decl| decl.param == node)
                            .and_then(|decl| decl.constraint)
                    }));
                }
                Some(SemanticNodeData::KeyOf { base, .. }) => {
                    let base = *base;
                    drop(data);
                    if matches!(
                        graph
                            .node_data(self.resolved_reduction_view(base))
                            .as_deref(),
                        Some(SemanticNodeData::TypeParam { .. })
                    ) {
                        kinds |= LiteralKind::String.bit() | LiteralKind::Number.bit();
                    }
                }
                _ => {}
            }
        }
        kinds
    }

    /// The candidate a NESTED call-argument deposit takes: each literal
    /// the argument's contextual typing kept (`keeps`) as written, every
    /// other literal widened under `policy`, a union member by member.
    pub(in crate::project_semantic_dispatch) fn contextual_literal_candidate(
        &self,
        bound: SemanticNodeId,
        keeps: &[SemanticNodeId],
        policy: ConstParamPolicy,
    ) -> SemanticNodeId {
        let graph = self.graph();
        let kept = |node: SemanticNodeId| {
            keeps.contains(&node)
                && matches!(
                    graph.node_data(node).as_deref(),
                    Some(SemanticNodeData::Literal(_))
                )
        };
        if kept(bound) {
            return bound;
        }
        let members = match graph.node_data(bound).as_deref() {
            Some(SemanticNodeData::Union(members)) if members.iter().any(|m| kept(*m)) => {
                members.iter().copied().collect::<Vec<_>>()
            }
            _ => return self.call_inference_candidate(bound, policy),
        };
        let members: Vec<SemanticNodeId> = members
            .into_iter()
            .map(|member| {
                if kept(member) {
                    member
                } else {
                    self.call_inference_candidate(member, policy)
                }
            })
            .collect();
        self.relation_combine_candidates(&members, crate::semantic_query::VariancePhase::Covariant)
    }

    /// An unfixed binder retains its constraint as a contextual literal type.
    pub(super) fn contextual_uninferred_parameter(
        &self,
        param: SemanticNodeId,
        constraint: Option<SemanticNodeId>,
    ) -> SemanticNodeId {
        let Some(constraint) = constraint else {
            return param;
        };
        let graph = self.graph();
        let Some(SemanticNodeData::TypeParam {
            decl,
            param_index,
            default,
            display_name,
            constraint: None,
        }) = graph
            .node_data(param)
            .as_deref()
            .map(crate::semantic_query::SemanticNodeData::clone)
        else {
            return param;
        };
        graph.intern_node(SemanticNodeData::TypeParam {
            decl,
            param_index,
            default,
            display_name,
            constraint: Some(constraint),
        })
    }
    /// The object literal argument `argument` with each member whose
    /// position in the parameter type `target` is one of the `const` type
    /// parameters `const_params` read as that member of the argument's const
    /// view (`isConstContext` holds for a literal whose contextual type is a
    /// `const` type parameter, however deeply the parameter's type nests it).
    /// `argument` itself when no member is.
    pub(super) fn nested_const_context_view(
        &self,
        argument: SemanticNodeId,
        const_view: SemanticNodeId,
        target: SemanticNodeId,
        const_params: &[SemanticNodeId],
    ) -> SemanticNodeId {
        if const_params.contains(&target) {
            return const_view;
        }
        let graph = self.graph();
        let (
            Some(SemanticNodeData::Object(argument_view)),
            Some(SemanticNodeData::Object(view)),
            Some(SemanticNodeData::Object(target_view)),
        ) = (
            graph
                .node_data(argument)
                .as_deref()
                .map(crate::semantic_query::SemanticNodeData::clone),
            graph
                .node_data(const_view)
                .as_deref()
                .map(crate::semantic_query::SemanticNodeData::clone),
            graph
                .node_data(target)
                .as_deref()
                .map(crate::semantic_query::SemanticNodeData::clone),
        )
        else {
            return argument;
        };
        let member_value =
            |surface: &crate::semantic_query::SurfaceView,
             key: &crate::semantic_query::AuthoredPropertyKey| {
                surface
                    .positive_members()
                    .iter()
                    .find(|member| member.key == *key)
                    .map(|member| member.value)
            };
        let mut changed = false;
        let entries: Vec<crate::semantic_query::SurfaceEntry> = argument_view
            .entries
            .iter()
            .map(|entry| match entry {
                crate::semantic_query::SurfaceEntry::Member(member) => {
                    let (Some(const_value), Some(target_value)) = (
                        member_value(&view, &member.key),
                        member_value(&target_view, &member.key),
                    ) else {
                        return entry.clone();
                    };
                    let value = self.nested_const_context_view(
                        member.value,
                        const_value,
                        target_value,
                        const_params,
                    );
                    if value == member.value {
                        return entry.clone();
                    }
                    changed = true;
                    let mut member = member.clone();
                    member.value = value;
                    crate::semantic_query::SurfaceEntry::Member(member)
                }
                other => other.clone(),
            })
            .collect();
        if !changed {
            return argument;
        }
        graph.intern_preserving_scope(
            argument,
            SemanticNodeData::Object(crate::semantic_query::SurfaceView::from_entries(
                entries,
                argument_view.keyspace,
                argument_view.closed().has_index_signature(),
            )),
        )
    }

    /// Whether the result of `target`'s call signatures — its return, or
    /// the type its predicate names, the position the checker infers a
    /// guard's type parameter from (`value is S`) — mentions one of
    /// `params`.
    pub(super) fn contextual_return_mentions(
        &self,
        target: SemanticNodeId,
        params: &[SemanticNodeId],
    ) -> bool {
        match self.shared_signature_nodes(target, SignatureKind::Call) {
            super::super::signature_discovery::SharedSignatureNodes::Nodes(nodes) => {
                nodes.iter().any(|node| {
                    let (return_type, predicate) = match self.graph().node_data(*node).as_deref() {
                        Some(SemanticNodeData::Signature {
                            return_type,
                            predicate,
                            ..
                        }) => (*return_type, predicate.and_then(|predicate| predicate.ty)),
                        _ => return false,
                    };
                    params.iter().any(|param| {
                        self.mentions_node(return_type, *param)
                            || predicate.is_some_and(|target| self.mentions_node(target, *param))
                    })
                })
            }
            super::super::signature_discovery::SharedSignatureNodes::Incomplete(_) => true,
        }
    }
}

/// The literal constituents of `node`: itself when a literal, its literal
/// members when a union.
fn literal_members(
    graph: &crate::semantic_query_memo::SemanticGraphStore,
    node: SemanticNodeId,
) -> Vec<SemanticNodeId> {
    let is_literal = |node: SemanticNodeId| {
        matches!(
            graph.node_data(node).as_deref(),
            Some(SemanticNodeData::Literal(_))
        )
    };
    match graph.node_data(node).as_deref() {
        Some(SemanticNodeData::Literal(_)) => vec![node],
        Some(SemanticNodeData::Union(members)) => members
            .iter()
            .copied()
            .filter(|member| is_literal(*member))
            .collect(),
        _ => Vec::new(),
    }
}

/// How many levels of unions, intersections and constraints a relation's
/// target is read through to decide whether it could hold a literal.
const SINGLETON_SCAN_DEPTH: u32 = 16;

impl<C: crate::resolver_core::ResolverCapabilities> ProjectSemanticDispatch<'_, C> {
    /// The two types of a failed relation as the checker prints them in its
    /// message (`reportRelationError`): a literal source is printed as its
    /// base primitive unless the target could itself hold a literal at its
    /// top level, where the literal is what tells the two apart.
    pub(in crate::project_semantic_dispatch) fn print_relation_operands(
        &self,
        source: SemanticNodeId,
        target: SemanticNodeId,
    ) -> (String, String) {
        let generalized = if self.could_hold_top_level_singleton(target, SINGLETON_SCAN_DEPTH) {
            None
        } else {
            self.literal_base_spelling(source)
        };
        (
            generalized.unwrap_or_else(|| self.print_diagnostic_type(source)),
            self.print_diagnostic_type(target),
        )
    }

    /// One type as a diagnostic message prints it.
    pub(in crate::project_semantic_dispatch) fn print_diagnostic_type(
        &self,
        node: SemanticNodeId,
    ) -> String {
        crate::semantic_query::display::display_type_node(
            self.graph(),
            node,
            crate::semantic_query::DisplayNeeds::single(
                crate::semantic_query::DisplayFacet::CheckerSpelling,
            )
            .union(crate::semantic_query::DisplayNeeds::single(
                crate::semantic_query::DisplayFacet::IncludeReadonlyModifier,
            )),
            crate::semantic_query::display::MAX_DISPLAY_DEPTH,
            &mut crate::semantic_query::display::DisplayContext::default(),
        )
        .0
    }

    /// Whether `target` could hold a literal or other unit type at its top
    /// level (`typeCouldHaveTopLevelSingletonTypes`), read `depth` levels
    /// of unions, intersections and constraints deep; past that (a
    /// constraint chain that comes round again) the literal is kept.
    fn could_hold_top_level_singleton(&self, target: SemanticNodeId, depth: u32) -> bool {
        let Some(data) = self.graph().node_data(target) else {
            return true;
        };
        let Some(depth) = depth.checked_sub(1) else {
            return true;
        };
        match data.as_ref() {
            SemanticNodeData::Primitive(PrimitiveKind::Boolean) => false,
            SemanticNodeData::Union(members) => members
                .iter()
                .any(|&member| self.could_hold_top_level_singleton(member, depth)),
            SemanticNodeData::Intersection(members) => members
                .iter()
                .any(|&member| self.could_hold_top_level_singleton(member, depth)),
            SemanticNodeData::TypeParam { constraint, .. } => constraint
                .is_some_and(|constraint| self.could_hold_top_level_singleton(constraint, depth)),
            SemanticNodeData::Literal(_)
            | SemanticNodeData::Primitive(PrimitiveKind::Null | PrimitiveKind::Undefined)
            | SemanticNodeData::TemplateLiteral { .. } => true,
            _ => false,
        }
    }

    /// The base primitive a literal source prints as
    /// (`getBaseTypeOfLiteralType`): each literal of a union of literals
    /// mapped to its primitive, in order and once. `None` for any type that
    /// is not a literal or a union of them.
    fn literal_base_spelling(&self, source: SemanticNodeId) -> Option<String> {
        let base = |node: SemanticNodeId| match self.graph().node_data(node)?.as_ref() {
            SemanticNodeData::Literal(value) => Some(match LiteralKind::of(value) {
                LiteralKind::String => "string",
                LiteralKind::Number => "number",
                LiteralKind::BigInt => "bigint",
                LiteralKind::Boolean => "boolean",
            }),
            _ => None,
        };
        if let Some(single) = base(source) {
            return Some(single.to_owned());
        }
        let data = self.graph().node_data(source)?;
        let SemanticNodeData::Union(members) = data.as_ref() else {
            return None;
        };
        let mut bases: Vec<&str> = Vec::new();
        for &member in members.iter() {
            let spelled = base(member)?;
            if !bases.contains(&spelled) {
                bases.push(spelled);
            }
        }
        Some(bases.join(" | "))
    }
}
