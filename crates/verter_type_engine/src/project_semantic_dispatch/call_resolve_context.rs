//! The contextual reads of a call's arguments: whether a context-sensitive
//! argument's contextual result reads an inference, the const context of
//! a literal member whose contextual type is a `const` type parameter nested
//! in the parameter's type, and the literals an argument nests where their
//! contextual type in the parameter is a literal context for them.

use std::sync::Arc;

use rustc_hash::{FxHashMap, FxHashSet};

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

/// Whether a contextual type is a literal context for a literal: it is
/// (`Keep`), it is known not to be (`Widen`), or it holds a type this
/// reader does not classify (`Unknown`), which decides nothing.
#[derive(Clone, Copy, PartialEq, Eq)]
enum LiteralContext {
    Keep,
    Widen,
    Unknown,
}

impl LiteralContext {
    /// The context of a union or intersection of contextual types: a
    /// literal context when any constituent is one.
    fn or(self, other: Self) -> Self {
        match (self, other) {
            (Self::Keep, _) | (_, Self::Keep) => Self::Keep,
            (Self::Unknown, _) | (_, Self::Unknown) => Self::Unknown,
            (Self::Widen, Self::Widen) => Self::Widen,
        }
    }
}

/// One authored position of a call argument: the expression's type, its
/// combined contextual type (the constituents of the parameter type that
/// type it), the type parameters whose constraints that contextual type
/// was read through, and whether it is the argument itself.
#[derive(Clone, PartialEq, Eq, Hash)]
struct ArgumentPosition {
    node: SemanticNodeId,
    contexts: Arc<[SemanticNodeId]>,
    through: Arc<[SemanticNodeId]>,
    root: bool,
}

impl ArgumentPosition {
    fn nested(
        node: SemanticNodeId,
        contexts: Arc<[SemanticNodeId]>,
        through: Arc<[SemanticNodeId]>,
    ) -> Self {
        Self {
            node,
            contexts,
            through,
            root: false,
        }
    }

    /// The type parameters a child position's contextual type was read
    /// through: this position's, and those its own read crossed.
    fn child_through(&self, crossed: FxHashSet<SemanticNodeId>) -> Arc<[SemanticNodeId]> {
        if crossed.is_empty() {
            return Arc::clone(&self.through);
        }
        combined_contexts(self.through.iter().copied().chain(crossed).collect())
    }
}

/// One step of typing a call argument's authored positions: read a
/// position, or rebuild it once its child positions (by member, element
/// or arm index) are typed, widening its own literals when decided so.
enum PositionStep {
    Enter(ArgumentPosition),
    Rebuild(ArgumentPosition, Vec<(usize, ArgumentPosition)>, bool),
}

/// The constraint a nested position reads the type parameter `param`
/// through: the one its node carries, else its declaration's in
/// `declared`. None for a `const` type parameter: every position under it
/// is a const context (`isConstContext`), whose literals stay as written
/// whatever its constraint says.
fn contextual_constraint(
    param: SemanticNodeId,
    constraint: Option<SemanticNodeId>,
    declared: &[TypeParamDecl],
) -> Option<SemanticNodeId> {
    let decl = declared.iter().find(|decl| decl.param == param);
    if decl.is_some_and(|decl| decl.is_const) {
        return None;
    }
    constraint.or_else(|| decl.and_then(|decl| decl.constraint))
}

/// The contextual types of one position, in one canonical order.
fn combined_contexts(mut contexts: Vec<SemanticNodeId>) -> Arc<[SemanticNodeId]> {
    contexts.sort_unstable();
    contexts.dedup();
    Arc::from(contexts)
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
                    Arc::<str>::from(&decl.name),
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

    /// The type the bare literal argument `source` has against the
    /// parameter type `target` under one call candidate, with the literals
    /// it keeps (`checkExpressionForMutableLocation` through
    /// `getWidenedLiteralLikeTypeForContextualType`). The argument's
    /// authored object members, array elements and conditional arms are its
    /// authored expressions, and each is decided WHOLE against its combined
    /// contextual type (every constituent of the parameter type that types
    /// the position) before any deposit composes it: `{ v: c ? "a" : 1 }`
    /// against `{ v: T }` with `T extends string | {}` keeps `"a" | 1`,
    /// while `["a", 1]` against `T[]` keeps `"a"` alone, and `{ v: "x" }`
    /// against `{ v: T } | { v: "a" }` keeps `"x"`. `declared` supplies the
    /// constraint of a candidate type parameter whose node carries none.
    /// The argument itself is no nested position: a naked top-level
    /// deposit keeps its literal regardless.
    ///
    /// A position whose contextual type is no literal context for its
    /// literals has their widened type in the returned argument type, so
    /// the decision stays with the position it was made for: `{ a: "x",
    /// b: "x" }` against `{ a: T | "a"; b: T }` is `{ a: "x"; b: string }`.
    /// A position whose contextual type this reader cannot classify keeps
    /// its authored type, and its deposits widen. Each kept literal is tied
    /// to the candidate type parameters its position's contextual type
    /// names, the only parameters a deposit from that position reaches.
    pub(in crate::project_semantic_dispatch) fn checked_literal_argument(
        &self,
        source: SemanticNodeId,
        target: SemanticNodeId,
        declared: &[TypeParamDecl],
    ) -> (SemanticNodeId, Vec<ContextKeptLiteral>) {
        let graph = self.graph();
        let mut keeps = Vec::new();
        let mut typed: FxHashMap<ArgumentPosition, SemanticNodeId> = FxHashMap::default();
        let mut open: FxHashSet<ArgumentPosition> = FxHashSet::default();
        let root = ArgumentPosition {
            node: source,
            contexts: Arc::from([target]),
            through: Arc::from([]),
            root: true,
        };
        let mut steps = vec![PositionStep::Enter(root.clone())];
        while let Some(step) = steps.pop() {
            let (position, children, widen) = match step {
                PositionStep::Enter(position) => {
                    if typed.contains_key(&position) || !open.insert(position.clone()) {
                        continue;
                    }
                    let node = position.node;
                    let mut children = Vec::new();
                    let mut widen = false;
                    let data = graph.node_data(node);
                    match data.as_deref() {
                        Some(SemanticNodeData::Object(surface)) => {
                            // Only an authored member is an expression of the
                            // argument; a spread's are its operand's.
                            let members: Vec<(usize, AuthoredPropertyKey, SemanticNodeId)> =
                                surface
                                    .positive_members()
                                    .iter()
                                    .enumerate()
                                    .filter(|(_, member)| {
                                        member.excess_origin
                                            == verter_type_expr::ExcessPropertyOrigin::FreshOwn
                                    })
                                    .map(|(index, member)| {
                                        (index, member.key.clone(), member.value)
                                    })
                                    .collect();
                            drop(data);
                            for (index, key, value) in members {
                                let mut crossed = FxHashSet::default();
                                let contexts = combined_contexts(
                                    position
                                        .contexts
                                        .iter()
                                        .flat_map(|&contextual| {
                                            self.contextual_member_types(
                                                contextual,
                                                &key,
                                                declared,
                                                &mut crossed,
                                            )
                                        })
                                        .collect(),
                                );
                                let through = position.child_through(crossed);
                                children.push((
                                    index,
                                    ArgumentPosition::nested(value, contexts, through),
                                ));
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
                                let mut crossed = FxHashSet::default();
                                let contexts = combined_contexts(
                                    position
                                        .contexts
                                        .iter()
                                        .flat_map(|&contextual| {
                                            self.contextual_element_types(
                                                contextual,
                                                index,
                                                declared,
                                                &mut crossed,
                                            )
                                        })
                                        .collect(),
                                );
                                let through = position.child_through(crossed);
                                children.push((
                                    index,
                                    ArgumentPosition::nested(value, contexts, through),
                                ));
                            }
                        }
                        Some(SemanticNodeData::Union(members)) => {
                            let members: Vec<SemanticNodeId> = members.iter().copied().collect();
                            drop(data);
                            // A conditional's object and array arms are its
                            // authored expressions against the same context;
                            // its literal arms are decided together, as the
                            // one expression they are.
                            for (index, &member) in members.iter().enumerate() {
                                if matches!(
                                    graph.node_data(member).as_deref(),
                                    Some(
                                        SemanticNodeData::Object(_)
                                            | SemanticNodeData::Tuple { .. }
                                    )
                                ) {
                                    children.push((
                                        index,
                                        ArgumentPosition::nested(
                                            member,
                                            Arc::clone(&position.contexts),
                                            Arc::clone(&position.through),
                                        ),
                                    ));
                                }
                            }
                            if !position.root {
                                widen =
                                    self.decide_literal_position(&position, declared, &mut keeps);
                            }
                        }
                        Some(SemanticNodeData::Literal(_)) if !position.root => {
                            drop(data);
                            widen = self.decide_literal_position(&position, declared, &mut keeps);
                        }
                        _ => {}
                    }
                    if !children.is_empty() {
                        steps.push(PositionStep::Rebuild(position, children.clone(), widen));
                        for (_, child) in children.into_iter().rev() {
                            steps.push(PositionStep::Enter(child));
                        }
                        continue;
                    }
                    (position, children, widen)
                }
                PositionStep::Rebuild(position, children, widen) => (position, children, widen),
            };
            let typed_node =
                self.rebuild_argument_position(position.node, &children, widen, |child| {
                    typed.get(child).copied().unwrap_or(child.node)
                });
            open.remove(&position);
            typed.insert(position, typed_node);
        }
        (typed.get(&root).copied().unwrap_or(source), keeps)
    }

    /// Decide the literals of the authored `position` against its combined
    /// contextual type: a literal context records them in `keeps` for every
    /// candidate type parameter the contextual type names or was read
    /// through (`{ v: "a" }` against `T extends { v: "a" | "b" }` keeps
    /// `"a"` for `T`). Whether the position's literals widen in the
    /// argument's type — only when every constituent of its contextual
    /// type is known to be no literal context for them.
    fn decide_literal_position(
        &self,
        position: &ArgumentPosition,
        declared: &[TypeParamDecl],
        keeps: &mut Vec<ContextKeptLiteral>,
    ) -> bool {
        let (node, contexts) = (position.node, &position.contexts);
        let literals = literal_members(self.graph(), node);
        if literals.is_empty() || contexts.is_empty() {
            return false;
        }
        match self.literal_context_of(node, contexts, declared) {
            LiteralContext::Keep => {
                for decl in declared {
                    if position.through.contains(&decl.param)
                        || contexts
                            .iter()
                            .any(|&contextual| self.mentions_node(contextual, decl.param))
                    {
                        keeps.extend(literals.iter().map(|&literal| ContextKeptLiteral {
                            literal,
                            param: decl.param,
                        }));
                    }
                }
                false
            }
            LiteralContext::Widen => true,
            LiteralContext::Unknown => false,
        }
    }

    /// The type of the authored position `node` once its `children`
    /// positions are typed (`typed_child`) and, when `widen`, its own
    /// literals widened. `node` itself when nothing changed.
    fn rebuild_argument_position(
        &self,
        node: SemanticNodeId,
        children: &[(usize, ArgumentPosition)],
        widen: bool,
        typed_child: impl Fn(&ArgumentPosition) -> SemanticNodeId,
    ) -> SemanticNodeId {
        if children.is_empty() && !widen {
            return node;
        }
        let graph = self.graph();
        let shape = match graph.node_data(node).as_deref() {
            Some(SemanticNodeData::Literal(_)) => PositionShape::Literal,
            Some(SemanticNodeData::Object(surface)) => PositionShape::Object(surface.clone()),
            Some(SemanticNodeData::Tuple { elements, readonly }) => PositionShape::Tuple(
                elements
                    .iter()
                    .map(crate::semantic_query::TupleElement::copy)
                    .collect(),
                *readonly,
            ),
            Some(SemanticNodeData::Union(members)) => {
                PositionShape::Union(members.iter().copied().collect())
            }
            _ => return node,
        };
        match shape {
            PositionShape::Literal if widen => self.widened_literal(node),
            PositionShape::Literal => node,
            PositionShape::Object(surface) => {
                let mut members: Vec<crate::semantic_query::SurfaceMember> = surface
                    .positive_members()
                    .iter()
                    .map(crate::semantic_query::SurfaceMember::clone)
                    .collect();
                let mut changed = false;
                for (index, child) in children {
                    let value = typed_child(child);
                    if members[*index].value != value {
                        members[*index].value = value;
                        changed = true;
                    }
                }
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
            PositionShape::Tuple(mut elements, readonly) => {
                let mut changed = false;
                for (index, child) in children {
                    let value = typed_child(child);
                    if elements[*index].value != value {
                        elements[*index].value = value;
                        changed = true;
                    }
                }
                if !changed {
                    return node;
                }
                graph.intern_preserving_scope(
                    node,
                    SemanticNodeData::Tuple {
                        elements: crate::semantic_query::PayloadArray::from(
                            elements.into_boxed_slice(),
                        ),
                        readonly,
                    },
                )
            }
            PositionShape::Union(mut members) => {
                let mut changed = false;
                for (index, child) in children {
                    let value = typed_child(child);
                    if members[*index] != value {
                        members[*index] = value;
                        changed = true;
                    }
                }
                if widen {
                    for member in &mut members {
                        if matches!(
                            graph.node_data(*member).as_deref(),
                            Some(SemanticNodeData::Literal(_))
                        ) {
                            *member = self.widened_literal(*member);
                            changed = true;
                        }
                    }
                }
                if !changed {
                    return node;
                }
                self.intern_normalized_union_or_intersection(&members, true)
            }
        }
    }

    /// The contextual types an object literal member named `key` takes
    /// from `contextual` (`getTypeOfPropertyOfContextualType`): the
    /// declared member of each constituent that lists it under JS property
    /// identity (`1` and `"1"` name one property), else the value type of
    /// the constituent's index signatures applicable to the key
    /// (`findApplicableIndexInfo`: a non-`string` one — `number` for a
    /// numeric name — before a `string` one). A type parameter types it
    /// through its base constraint, as the checker reads the apparent type
    /// of a contextual type (`getApparentTypeOfContextualType`); `visited`
    /// holds the type parameters already read, so a circular constraint
    /// contributes nothing.
    fn contextual_member_types(
        &self,
        contextual: SemanticNodeId,
        key: &AuthoredPropertyKey,
        declared: &[TypeParamDecl],
        visited: &mut FxHashSet<SemanticNodeId>,
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
                    .flat_map(|member| self.contextual_member_types(member, key, declared, visited))
                    .collect();
            }
            Some(SemanticNodeData::Object(surface)) => surface.clone(),
            Some(SemanticNodeData::TypeParam { constraint, .. }) => {
                let constraint = contextual_constraint(contextual, *constraint, declared);
                drop(data);
                return match constraint {
                    Some(constraint) if visited.insert(contextual) => {
                        self.contextual_member_types(constraint, key, declared, visited)
                    }
                    _ => Vec::new(),
                };
            }
            None
            | Some(
                SemanticNodeData::Primitive(_)
                | SemanticNodeData::Literal(_)
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
        let known = key.cloned_known();
        let named: Vec<SemanticNodeId> = surface
            .positive_members()
            .iter()
            .filter(|member| {
                member.key == *key
                    || member.key.as_known().zip(known.as_ref()).is_some_and(
                        |(member_key, known)| member_key.element_access_collides(&known.as_ref()),
                    )
            })
            .map(|member| member.value)
            .collect();
        if !named.is_empty() {
            return named;
        }
        let Some(known) = known else {
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
    /// rest element's type past its fixed elements), per constituent; a
    /// type parameter's through its base constraint, as
    /// [`Self::contextual_member_types`] reads it.
    fn contextual_element_types(
        &self,
        contextual: SemanticNodeId,
        index: usize,
        declared: &[TypeParamDecl],
        visited: &mut FxHashSet<SemanticNodeId>,
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
                    .flat_map(|member| {
                        self.contextual_element_types(member, index, declared, visited)
                    })
                    .collect()
            }
            Some(SemanticNodeData::TypeParam { constraint, .. }) => {
                let constraint = contextual_constraint(contextual, *constraint, declared);
                drop(data);
                match constraint {
                    Some(constraint) if visited.insert(contextual) => {
                        self.contextual_element_types(constraint, index, declared, visited)
                    }
                    _ => Vec::new(),
                }
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

    /// Whether a context-sensitive argument's first-pass reading infers
    /// into its parameter type `target`: not when `target` is itself one
    /// of the candidate's type parameters (`declared`). The reading's
    /// withheld members make it non-inferrable (`NonInferrableType`), which
    /// the checker never deposits into a naked type parameter; the
    /// parameter then stays its constraint for the second pass's contextual
    /// typing.
    pub(super) fn first_pass_infers_into(
        &self,
        target: SemanticNodeId,
        declared: &[TypeParamDecl],
    ) -> bool {
        let target = self.resolved_reduction_view(target);
        !declared.iter().any(|decl| decl.param == target)
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
        self.literal_context_of(candidate, &[contextual], declared) == LiteralContext::Keep
    }

    /// [`Self::literal_of_contextual_type`] against the combined
    /// contextual type `contexts`, telling a contextual type known to be
    /// no literal context from one this reader does not classify.
    fn literal_context_of(
        &self,
        candidate: SemanticNodeId,
        contexts: &[SemanticNodeId],
        declared: &[TypeParamDecl],
    ) -> LiteralContext {
        let graph = self.graph();
        let held = literal_members(graph, candidate)
            .into_iter()
            .fold(0u8, |held, literal| {
                match graph.node_data(literal).as_deref() {
                    Some(SemanticNodeData::Literal(value)) => held | LiteralKind::of(value).bit(),
                    _ => held,
                }
            });
        if held == 0 {
            return LiteralContext::Widen;
        }
        let mut visited = FxHashSet::default();
        let mut context = LiteralContext::Widen;
        for &contextual in contexts {
            context =
                context.or(self.literal_context_admits(held, contextual, declared, &mut visited));
            if context == LiteralContext::Keep {
                break;
            }
        }
        context
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
    ) -> LiteralContext {
        let graph = self.graph();
        let authored = contextual;
        let contextual = self.resolved_reduction_view(contextual);
        if !visited.insert(contextual) {
            return LiteralContext::Widen;
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
                let mut context = LiteralContext::Widen;
                for member in members {
                    context =
                        context.or(self.literal_context_admits(held, member, declared, visited));
                    if context == LiteralContext::Keep {
                        break;
                    }
                }
                return context;
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
                    return LiteralContext::Widen;
                };
                if held & self.base_constraint_primitive_kinds(constraint, declared) != 0 {
                    return LiteralContext::Keep;
                }
                return self.literal_context_admits(held, constraint, declared, visited);
            }
            Some(SemanticNodeData::Literal(value)) => LiteralKind::of(value).bit(),
            Some(SemanticNodeData::TemplateLiteral { .. } | SemanticNodeData::KeyOf { .. }) => {
                LiteralKind::String.bit()
            }
            // `boolean` is the union `true | false` to the checker.
            Some(SemanticNodeData::Primitive(PrimitiveKind::Boolean)) => LiteralKind::Boolean.bit(),
            // No literal context, whatever the literal.
            Some(
                SemanticNodeData::Primitive(_)
                | SemanticNodeData::Object(_)
                | SemanticNodeData::Array { .. }
                | SemanticNodeData::Tuple { .. }
                | SemanticNodeData::Signature { .. },
            ) => 0,
            _ => {
                drop(data);
                // A string mapping (`Uppercase<string>`) is a string
                // literal context, as a template literal is.
                if self.string_mapping_of(authored).is_some()
                    || self.string_mapping_of(contextual).is_some()
                {
                    LiteralKind::String.bit()
                } else {
                    return LiteralContext::Unknown;
                }
            }
        };
        if held & kinds != 0 {
            LiteralContext::Keep
        } else {
            LiteralContext::Widen
        }
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

    /// The candidate a call-argument deposit that is no naked literal
    /// takes: each literal the argument's contextual typing kept (`keeps`)
    /// as written — nested in a whole object or array argument too — every
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
            _ => return self.call_inference_candidate_keeping(bound, policy, keeps),
        };
        let members: Vec<SemanticNodeId> = members
            .into_iter()
            .map(|member| {
                if kept(member) {
                    member
                } else {
                    self.call_inference_candidate_keeping(member, policy, keeps)
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

/// Whether `name` is written bare as a property name: an identifier.
pub(super) fn is_identifier_name(name: &str) -> bool {
    let mut chars = name.chars();
    chars
        .next()
        .is_some_and(|first| first.is_alphabetic() || first == '_' || first == '$')
        && chars.all(|c| c.is_alphanumeric() || c == '_' || c == '$')
}

/// A property's name as a message names it (`symbolToString`): an
/// identifier bare, any other string double-quoted, a number as written.
/// `None` for a symbol or computed key.
pub(super) fn checker_property_name(key: &AuthoredPropertyKey) -> Option<String> {
    match key {
        AuthoredPropertyKey::String(name) if is_identifier_name(name) => Some(name.to_string()),
        AuthoredPropertyKey::String(name) => Some(format!("{:?}", &**name)),
        AuthoredPropertyKey::Number(number) => Some(number.to_string()),
        AuthoredPropertyKey::UniqueSymbol(_) | AuthoredPropertyKey::Computed(_) => None,
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

    /// The checker's diagnostic for one walker diagnostic the checker
    /// itself reports, fully formatted as `TS<code>: <message>`: a relation
    /// refused at its comparison allowance is TS2859 naming its two types.
    /// `None` for a walker diagnostic the checker has no code for.
    pub fn checker_message(
        &self,
        diagnostic: &super::super::walk::ShallowDiagnostic,
    ) -> Option<String> {
        match diagnostic {
            super::super::walk::ShallowDiagnostic::RelationTooComplex { source, target } => {
                let code = crate::semantic_query::CheckerDiagnosticCode::RelationTooComplex;
                Some(format!(
                    "TS{}: {}",
                    code.code(),
                    code.format(&[
                        self.print_diagnostic_type(*source),
                        self.print_diagnostic_type(*target),
                    ])
                ))
            }
            _ => None,
        }
    }

    /// The checker's diagnostic a recovery node carries, fully formatted as
    /// `TS<code>: <message>`: the error type the checker continues with
    /// after a diagnostic it recovers from
    /// ([`QueryError::CheckerRecovery`](crate::semantic_query::QueryError::CheckerRecovery)).
    /// TS2456 names the alias its origin refers to. `None` for any other
    /// node, and for a recovery whose message needs an argument it does not
    /// carry.
    pub fn checker_recovery_message(&self, node: SemanticNodeId) -> Option<String> {
        let data = self.graph().node_data(node)?;
        let SemanticNodeData::Opaque(crate::semantic_query::QueryError::CheckerRecovery {
            diagnostic,
            origin,
            ..
        }) = data.as_ref()
        else {
            return None;
        };
        let code = diagnostic.code;
        let arguments: Vec<String> = match code {
            crate::semantic_query::CheckerDiagnosticCode::CircularTypeAlias => {
                match self.graph().node_data((*origin)?)?.as_ref() {
                    SemanticNodeData::DeclRef { identity } => {
                        vec![identity.decl_name.to_string()]
                    }
                    _ => return None,
                }
            }
            _ if !code.message().contains('{') => Vec::new(),
            _ => return None,
        };
        Some(format!("TS{}: {}", code.code(), code.format(&arguments)))
    }

    /// Where the checker reports the diagnostic a recovery node carries
    /// ([`Self::checker_recovery_message`]): the file and span of the node
    /// it reports at — TS2456 at the circular alias's name. `None` for any
    /// other node, and for a recovery carrying no such origin.
    pub fn checker_recovery_location(
        &self,
        node: SemanticNodeId,
    ) -> Option<(std::sync::Arc<str>, verter_span::Span)> {
        let data = self.graph().node_data(node)?;
        let SemanticNodeData::Opaque(crate::semantic_query::QueryError::CheckerRecovery {
            diagnostic,
            origin,
            ..
        }) = data.as_ref()
        else {
            return None;
        };
        if diagnostic.code != crate::semantic_query::CheckerDiagnosticCode::CircularTypeAlias {
            return None;
        }
        let origin = self.graph().node_data((*origin)?)?;
        let SemanticNodeData::DeclRef { identity } = origin.as_ref() else {
            return None;
        };
        let canonical: std::sync::Arc<str> = std::sync::Arc::from(identity.canonical_id.as_ref());
        let serve = self.ctx.ensure_indexed_ready_serve(&canonical)?;
        let span = serve
            .indexed
            .shallow_state
            .headers
            .type_header_in(identity.owner, identity.decl_name.as_ref())?
            .name_span;
        Some((canonical, span))
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

    /// The properties `target` requires that `source` lacks, named as the
    /// checker's missing-properties elaboration names them
    /// (`getUnmatchedProperties`), in the target's member order: an
    /// identifier name bare, any other string name double-quoted. `None`
    /// unless both resolve to object surfaces and the target names every
    /// missing member by a string or number key.
    pub(in crate::project_semantic_dispatch) fn missing_required_properties(
        &self,
        source: SemanticNodeId,
        target: SemanticNodeId,
    ) -> Option<Vec<String>> {
        let transit =
            crate::semantic_query::ProjectionReductionContext::structural_transit_with_mode(
                crate::semantic_query::ProjectionMode::Navigate,
            );
        let surface = |node| match self.normalize_node_for_structural_fact_demand(node, transit) {
            super::super::evaluate::StructuralFactDemandOutcome::Complete(node) => {
                match self.graph().node_data(node)?.as_ref() {
                    SemanticNodeData::Object(view) => Some(view.clone()),
                    _ => None,
                }
            }
            _ => None,
        };
        let (source, target) = (surface(source)?, surface(target)?);
        let mut missing = Vec::new();
        for member in target.positive_members() {
            if member.optional
                || source
                    .positive_members()
                    .iter()
                    .any(|present| present.key == member.key)
            {
                continue;
            }
            missing.push(checker_property_name(&member.key)?);
        }
        Some(missing)
    }

    /// Whether `target` could hold a literal or other unit type at its top
    /// level (`typeCouldHaveTopLevelSingletonTypes`), read `depth` levels
    /// of unions, intersections and constraints deep; past that (a
    /// constraint chain that comes round again) the literal is kept. A
    /// `boolean` is no such type, but inside a union the checker holds it as
    /// `true | false`, whose members are.
    fn could_hold_top_level_singleton(&self, target: SemanticNodeId, depth: u32) -> bool {
        // A carrier — an alias or declaration reference to an enum or a
        // named literal union — classifies as what it settles to, exactly as
        // the checker reads the type the declaration names.
        let Some(settled) = self.settle_through_carriers(target) else {
            return true;
        };
        let Some(data) = self.graph().node_data(settled) else {
            return true;
        };
        let Some(depth) = depth.checked_sub(1) else {
            return true;
        };
        match data.as_ref() {
            SemanticNodeData::Primitive(PrimitiveKind::Boolean) => false,
            SemanticNodeData::Union(members) => members.iter().any(|&member| {
                matches!(
                    self.graph().node_data(member).as_deref(),
                    Some(SemanticNodeData::Primitive(PrimitiveKind::Boolean))
                ) || self.could_hold_top_level_singleton(member, depth)
            }),
            SemanticNodeData::Intersection(members) => members
                .iter()
                .any(|&member| self.could_hold_top_level_singleton(member, depth)),
            SemanticNodeData::TypeParam { constraint, .. } => constraint
                .is_some_and(|constraint| self.could_hold_top_level_singleton(constraint, depth)),
            SemanticNodeData::Literal(_)
            | SemanticNodeData::EnumLiteral(_)
            | SemanticNodeData::Primitive(PrimitiveKind::Null | PrimitiveKind::Undefined)
            | SemanticNodeData::TemplateLiteral { .. }
            | SemanticNodeData::IntrinsicApplication { .. } => true,
            _ => false,
        }
    }

    /// The base type a literal source prints as
    /// (`getBaseTypeOfLiteralType`): a literal's primitive, an enum member's
    /// enum, and each member of a union of them mapped so, in order and
    /// once. `None` for any type that is not a literal, an enum member or a
    /// union of them.
    fn literal_base_spelling(&self, source: SemanticNodeId) -> Option<String> {
        let base = |node: SemanticNodeId| -> Option<String> {
            match self.graph().node_data(node)?.as_ref() {
                SemanticNodeData::Literal(value) => Some(
                    match LiteralKind::of(value) {
                        LiteralKind::String => "string",
                        LiteralKind::Number => "number",
                        LiteralKind::BigInt => "bigint",
                        LiteralKind::Boolean => "boolean",
                    }
                    .to_owned(),
                ),
                SemanticNodeData::EnumLiteral(literal) => {
                    Some(literal.enum_decl.decl_name.to_string())
                }
                _ => None,
            }
        };
        if let Some(single) = base(source) {
            return Some(single);
        }
        let data = self.graph().node_data(source)?;
        let SemanticNodeData::Union(members) = data.as_ref() else {
            return None;
        };
        let mut bases: Vec<String> = Vec::new();
        for &member in members.iter() {
            let spelled = base(member)?;
            if !bases.contains(&spelled) {
                bases.push(spelled);
            }
        }
        Some(bases.join(" | "))
    }
}

/// The parts of an authored position's type a rebuild replaces.
enum PositionShape {
    Literal,
    Object(crate::semantic_query::SurfaceView),
    Tuple(Vec<crate::semantic_query::TupleElement>, bool),
    Union(Vec<SemanticNodeId>),
}
