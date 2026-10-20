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
    ArgumentLiteralProvenance, ArgumentPathStep, ArgumentRegularPosition, AuthoredPropertyKey,
    ConstParamPolicy, LiteralValue, PrimitiveKind, PropertyKey, SemanticNodeData, SemanticNodeId,
    SignatureKind, TypeParamDecl,
};

/// How a call argument's read of a value binding contributes its literals,
/// resolved in the scope the call is in.
#[derive(Clone, Copy, Debug)]
pub(in crate::project_semantic_dispatch) enum ArgumentReadLiterals {
    /// The read names a binding of the call's owner scope: its declaration
    /// decides whether it widens, and its type is the owner-scope `typeof`.
    OwnerScope,
    /// The read names a binding of the function body the call is in (a
    /// parameter, a local), typed `node` there: regular unless the binding
    /// keeps a widening literal.
    Frame { widens: bool, node: SemanticNodeId },
    /// A read the call's scope does not model: its literals stay fresh.
    Unmodeled,
}

/// The type of one regular contributor to an argument position.
enum RegularContributor {
    /// An authored type, lowered in the owner scope.
    Authored(verter_type_expr::TypeExpr),
    /// A frame binding's type.
    Resolved(SemanticNodeId),
    /// A const-context composite, which holds no literal at its own level.
    Composite,
}

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
/// was read through, its steps from the argument, whether it is the
/// argument itself, and whether it is in a const context.
#[derive(Clone, PartialEq, Eq, Hash)]
struct ArgumentPosition {
    node: SemanticNodeId,
    contexts: Arc<[SemanticNodeId]>,
    through: Arc<[SemanticNodeId]>,
    path: Arc<[ArgumentPathStep]>,
    root: bool,
    /// The checker's `isConstContext`: an enclosing object or array
    /// literal of the argument is contextually typed by a `const` type
    /// parameter, so the position's literals are kept as written.
    const_context: bool,
}

impl ArgumentPosition {
    /// The position one `step` (none for a conditional's branch) inside
    /// this one.
    fn nested(
        &self,
        step: Option<ArgumentPathStep>,
        node: SemanticNodeId,
        contexts: Arc<[SemanticNodeId]>,
        through: Arc<[SemanticNodeId]>,
        const_context: bool,
    ) -> Self {
        Self {
            node,
            contexts,
            through,
            path: match step {
                Some(step) => self.path.iter().copied().chain([step]).collect(),
                None => Arc::clone(&self.path),
            },
            root: false,
            const_context,
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
/// or arm index) are typed, widening the literals of its own decided so.
enum PositionStep {
    Enter(ArgumentPosition),
    Rebuild(
        ArgumentPosition,
        Vec<(usize, ArgumentPosition)>,
        Vec<SemanticNodeId>,
    ),
}

/// The shape of an array literal a contextual tuple types element by
/// element (`getContextualTypeForElementExpression`'s `length`,
/// `firstSpreadIndex` and `lastSpreadIndex`): its element count, when
/// known, and the positions of its first and last spread.
#[derive(Clone, Copy, Debug, Default)]
pub(in crate::project_semantic_dispatch) struct ArrayLiteralShape {
    pub(in crate::project_semantic_dispatch) length: Option<usize>,
    pub(in crate::project_semantic_dispatch) first_spread: Option<usize>,
    pub(in crate::project_semantic_dispatch) last_spread: Option<usize>,
}

impl ArrayLiteralShape {
    /// The shape of the array literal an argument tuple of `elements` is:
    /// a rest element is a spread.
    fn of_tuple(elements: &[crate::semantic_query::TupleElement]) -> Self {
        Self {
            length: Some(elements.len()),
            first_spread: elements.iter().position(|element| element.rest),
            last_spread: elements.iter().rposition(|element| element.rest),
        }
    }
}

/// The contextual types a tuple whose elements are `elements` gives
/// the array literal element at `index` of a literal shaped `shape`
/// (the tuple arm of `getContextualTypeForElementExpression`): the
/// tuple's own fixed element while no spread came before it; counted
/// from the end, the tuple's trailing fixed element at the same
/// distance when the literal's length is known and no spread follows
/// the element; otherwise the element types of the tuple's middle — its
/// rest element's, and its trailing fixed elements' unless the
/// literal's length places the element before them. Empty when no
/// element types the position. `rest_element` reads a rest element's
/// element type off its array type.
pub(in crate::project_semantic_dispatch) fn tuple_element_contexts(
    elements: &[crate::semantic_query::TupleElement],
    index: usize,
    shape: ArrayLiteralShape,
    rest_element: impl Fn(SemanticNodeId) -> Option<SemanticNodeId>,
) -> Vec<SemanticNodeId> {
    let fixed = elements.iter().take_while(|element| !element.rest).count();
    if shape.first_spread.is_none_or(|spread| index < spread) && index < fixed {
        return vec![elements[index].value];
    }
    let variadic = elements.iter().any(|element| element.rest);
    let offset = match shape.length {
        Some(length) if shape.last_spread.is_none_or(|spread| index > spread) => {
            length.saturating_sub(index)
        }
        _ => 0,
    };
    let fixed_end = if offset > 0 && variadic {
        elements
            .iter()
            .rev()
            .take_while(|element| !element.rest)
            .count()
    } else {
        0
    };
    if offset > 0 && offset <= fixed_end {
        return vec![elements[elements.len() - offset].value];
    }
    let start = shape.first_spread.map_or(fixed, |spread| spread.min(fixed));
    let end = elements.len() - if shape.length.is_some() { fixed_end } else { 0 };
    elements
        .get(start..end)
        .unwrap_or_default()
        .iter()
        .filter_map(|element| {
            if element.rest {
                rest_element(element.value)
            } else {
                Some(element.value)
            }
        })
        .collect()
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
    ///
    /// Only a position's FRESH literals are decided so: a literal
    /// `provenance` records as regular (`{ v: "a" as "a" }`, `{ v: sl }`
    /// for `sl: "a"`) keeps its type at any contextual type, and is kept
    /// as written for every parameter the argument reaches.
    pub(in crate::project_semantic_dispatch) fn checked_literal_argument(
        &self,
        source: SemanticNodeId,
        target: SemanticNodeId,
        declared: &[TypeParamDecl],
        provenance: Option<&ArgumentLiteralProvenance>,
    ) -> (SemanticNodeId, Vec<ContextKeptLiteral>) {
        let graph = self.graph();
        let mut keeps = Vec::new();
        let mut typed: FxHashMap<ArgumentPosition, SemanticNodeId> = FxHashMap::default();
        let mut open: FxHashSet<ArgumentPosition> = FxHashSet::default();
        let root = ArgumentPosition {
            node: source,
            contexts: Arc::from([target]),
            through: Arc::from([]),
            path: Arc::from([]),
            root: true,
            const_context: false,
        };
        // The parameters the argument deposits into: a regular literal is
        // written as authored into each of them.
        let reached: Vec<SemanticNodeId> = if provenance.is_some() {
            declared
                .iter()
                .map(|decl| decl.param)
                .filter(|&param| self.mentions_node(target, param))
                .collect()
        } else {
            Vec::new()
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
                    let mut widen = Vec::new();
                    // The position's own regular literals, when it lists
                    // some; `None` inside, when every literal at and inside
                    // the position is regular.
                    let regular_here = provenance
                        .and_then(|provenance| provenance.position(&position.path))
                        .map(|regular| regular.literals.clone());
                    // A position whose literals are all regular is written
                    // as authored, nested composites included: it keeps its
                    // type, and every parameter the argument deposits into
                    // takes it as it is.
                    if matches!(regular_here, Some(None)) && !position.root {
                        for literal in std::iter::once(node).chain(literal_members(graph, node)) {
                            keeps.extend(
                                reached
                                    .iter()
                                    .map(|&param| ContextKeptLiteral { literal, param }),
                            );
                        }
                        open.remove(&position);
                        typed.insert(position, node);
                        continue;
                    }
                    // The positions this one's object or array literal holds
                    // are in a const context when it is, or when its own
                    // contextual type is a `const` type parameter.
                    let nested_const = position.const_context
                        || position
                            .contexts
                            .iter()
                            .any(|&contextual| self.is_const_type_variable(contextual, declared));
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
                                let step = key
                                    .cloned_known()
                                    .and_then(|key| self.argument_member_step(&key))
                                    .map(ArgumentPathStep::Member);
                                children.push((
                                    index,
                                    position.nested(step, value, contexts, through, nested_const),
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
                            let shape = ArrayLiteralShape::of_tuple(elements);
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
                                                shape,
                                                declared,
                                                &mut crossed,
                                            )
                                        })
                                        .collect(),
                                );
                                let through = position.child_through(crossed);
                                let step = u32::try_from(index).ok().map(ArgumentPathStep::Element);
                                children.push((
                                    index,
                                    position.nested(step, value, contexts, through, nested_const),
                                ));
                            }
                        }
                        // An array literal holding a spread or an elision: its
                        // elements join into one element type, typed by the
                        // contextual type of any element.
                        Some(SemanticNodeData::Array { element, .. }) => {
                            let element = *element;
                            drop(data);
                            let shape = ArrayLiteralShape {
                                length: None,
                                first_spread: Some(0),
                                last_spread: Some(0),
                            };
                            let mut crossed = FxHashSet::default();
                            let contexts = combined_contexts(
                                position
                                    .contexts
                                    .iter()
                                    .flat_map(|&contextual| {
                                        self.contextual_element_types(
                                            contextual,
                                            0,
                                            shape,
                                            declared,
                                            &mut crossed,
                                        )
                                    })
                                    .collect(),
                            );
                            let through = position.child_through(crossed);
                            children.push((
                                0,
                                position.nested(
                                    Some(ArgumentPathStep::Elements),
                                    element,
                                    contexts,
                                    through,
                                    nested_const,
                                ),
                            ));
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
                                        position.nested(
                                            None,
                                            member,
                                            Arc::clone(&position.contexts),
                                            Arc::clone(&position.through),
                                            position.const_context,
                                        ),
                                    ));
                                }
                            }
                            if !position.root {
                                widen = self.decide_literal_position(
                                    &position,
                                    regular_here.flatten().as_deref(),
                                    declared,
                                    &reached,
                                    &mut keeps,
                                );
                            }
                        }
                        Some(SemanticNodeData::Literal(_)) if !position.root => {
                            drop(data);
                            widen = self.decide_literal_position(
                                &position,
                                regular_here.flatten().as_deref(),
                                declared,
                                &reached,
                                &mut keeps,
                            );
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
                self.rebuild_argument_position(position.node, &children, &widen, |child| {
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
    /// `"a"` for `T`). The literals of the position that widen in the
    /// argument's type — its fresh ones, when every constituent of its
    /// contextual type is known to be no literal context for them.
    ///
    /// A regular literal (one in `regular`) is no fresh literal a mutable
    /// location widens: it keeps its type at any contextual type, and each
    /// parameter the argument deposits into (`reached`) keeps it as
    /// written.
    fn decide_literal_position(
        &self,
        position: &ArgumentPosition,
        regular: Option<&[SemanticNodeId]>,
        declared: &[TypeParamDecl],
        reached: &[SemanticNodeId],
        keeps: &mut Vec<ContextKeptLiteral>,
    ) -> Vec<SemanticNodeId> {
        let (node, contexts) = (position.node, &position.contexts);
        let (regular, fresh): (Vec<SemanticNodeId>, Vec<SemanticNodeId>) =
            literal_members(self.graph(), node)
                .into_iter()
                .partition(|literal| regular.is_some_and(|regular| regular.contains(literal)));
        for &literal in &regular {
            keeps.extend(
                reached
                    .iter()
                    .map(|&param| ContextKeptLiteral { literal, param }),
            );
        }
        if fresh.is_empty() {
            return Vec::new();
        }
        // A position no constituent of the parameter types holds its fresh
        // literals with no contextual type, which widens them
        // (`getWidenedLiteralLikeTypeForContextualType` without one). A
        // deposit widens them as well, unless a regular literal kept for
        // every parameter the argument reaches is equal to one of them:
        // the decision is then made here, at the position it is for (`{ v:
        // "a" as "a", w: "a" }` deposited into a naked `T` keeps `v` alone).
        // A const context keeps them as written whatever the argument
        // reaches (`boxc<const T>(o: { a: T })` over `{ a: [v, "lit"] }`).
        if contexts.is_empty() {
            return if reached.is_empty() || position.const_context {
                Vec::new()
            } else {
                fresh
            };
        }
        match self.literal_context_of(node, contexts, declared) {
            LiteralContext::Keep => {
                for decl in declared {
                    if position.through.contains(&decl.param)
                        || contexts
                            .iter()
                            .any(|&contextual| self.mentions_node(contextual, decl.param))
                    {
                        keeps.extend(fresh.iter().map(|&literal| ContextKeptLiteral {
                            literal,
                            param: decl.param,
                        }));
                    }
                }
                Vec::new()
            }
            LiteralContext::Widen => fresh,
            LiteralContext::Unknown => Vec::new(),
        }
    }

    /// The checker's `isConstTypeVariable` over the call's declared type
    /// parameters: `contextual` is a `const` type parameter, or a union or
    /// intersection holding one.
    fn is_const_type_variable(
        &self,
        contextual: SemanticNodeId,
        declared: &[TypeParamDecl],
    ) -> bool {
        let is_const = |node: SemanticNodeId| {
            declared
                .iter()
                .any(|decl| decl.is_const && decl.param == node)
        };
        is_const(contextual)
            || self
                .graph()
                .node_data(contextual)
                .as_deref()
                .and_then(SemanticNodeData::composite_members)
                .is_some_and(|members| members.iter().any(|&member| is_const(member)))
    }

    /// The step into an object literal's member keyed `key`: the literal
    /// type of its string or numeric key. None for a symbol key.
    fn argument_member_step(&self, key: &PropertyKey) -> Option<SemanticNodeId> {
        let literal = match key {
            PropertyKey::String(name) => LiteralValue::String(name.to_string()),
            #[allow(clippy::cast_precision_loss)]
            PropertyKey::Number(index) => LiteralValue::Number(index.get() as f64),
            PropertyKey::UniqueSymbol(_) => return None,
        };
        Some(self.graph().intern_node(SemanticNodeData::Literal(literal)))
    }

    /// The literal provenance `authored` records for a call argument,
    /// resolved in the argument's scope (`canonical`, `owner`): a read
    /// whose binding keeps a widening literal (`const s = "a"`) is a fresh
    /// contributor after all, and a position another contributor writes
    /// too lists the literals its regular contributors' types hold. A
    /// literal one arm writes regular and another arm of the position
    /// writes fresh is fresh: the arm writing it fresh widens it, and the
    /// arms' types join into one position (`c ? { v: "a" as "a" } : { v:
    /// "a" }` infers `string`).
    pub(in crate::project_semantic_dispatch) fn resolve_argument_literal_provenance(
        &self,
        canonical: &str,
        owner: verter_type_expr::TopLevelOwnerId,
        authored: Option<&verter_type_expr::IndexedLiteralProvenance>,
    ) -> Option<Arc<ArgumentLiteralProvenance>> {
        self.resolve_scoped_argument_literal_provenance(canonical, owner, authored, &|_, _| {
            ArgumentReadLiterals::OwnerScope
        })
    }

    /// [`Self::resolve_argument_literal_provenance`] for a call whose
    /// argument reads may name bindings of an enclosing function body:
    /// `reads` resolves each read, by its value path and the span of the
    /// identifier it starts from, in the call's own scope.
    pub(in crate::project_semantic_dispatch) fn resolve_scoped_argument_literal_provenance(
        &self,
        canonical: &str,
        owner: verter_type_expr::TopLevelOwnerId,
        authored: Option<&verter_type_expr::IndexedLiteralProvenance>,
        reads: &dyn Fn(&verter_type_expr::ValueRef, verter_span::Span) -> ArgumentReadLiterals,
    ) -> Option<Arc<ArgumentLiteralProvenance>> {
        use verter_type_expr::{IndexedLiteralStep, IndexedRegularLiteralSource, TypeExpr};
        let authored = authored?;
        let graph = self.graph();
        let lower = |ty: &TypeExpr| {
            self.lower_type_expr_in_owner_scope_with_context(
                canonical,
                owner,
                ty,
                crate::semantic_query::ProjectionReductionContext::structural_transit(),
            )
        };
        let type_of = |contributor: &RegularContributor| match contributor {
            RegularContributor::Authored(ty) => lower(ty),
            RegularContributor::Resolved(node) => Some(*node),
            RegularContributor::Composite => None,
        };
        let mut positions = Vec::new();
        'positions: for position in authored.positions.iter() {
            let mut path = Vec::with_capacity(position.path.len());
            for step in position.path.iter() {
                path.push(match step {
                    IndexedLiteralStep::Member(key) => match self.argument_member_step(key) {
                        Some(key) => ArgumentPathStep::Member(key),
                        None => continue 'positions,
                    },
                    IndexedLiteralStep::Element(index) => ArgumentPathStep::Element(*index),
                    IndexedLiteralStep::Elements => ArgumentPathStep::Elements,
                });
            }
            let separate = position.arms.len() > 1;
            let mut mixed = false;
            // Each arm's regular contributors' types, and the fresh
            // literals it writes when another arm can hold them regular.
            let mut arms: Vec<(Vec<RegularContributor>, Vec<SemanticNodeId>)> =
                Vec::with_capacity(position.arms.len());
            for arm in position.arms.iter() {
                mixed |= arm.mixed;
                let mut regular = Vec::with_capacity(arm.regular.len());
                let mut fresh = Vec::new();
                if separate {
                    fresh.extend(arm.fresh.iter().map(|literal| {
                        graph.intern_node(SemanticNodeData::Literal(literal.clone()))
                    }));
                }
                for source in arm.regular.iter() {
                    match source {
                        IndexedRegularLiteralSource::Pinned(ty) => {
                            regular.push(RegularContributor::Authored(ty.clone()));
                        }
                        IndexedRegularLiteralSource::PinnedComposite => {
                            regular.push(RegularContributor::Composite);
                        }
                        IndexedRegularLiteralSource::Read { value, root } => {
                            let (widens, contributor) = match reads(value, *root) {
                                ArgumentReadLiterals::OwnerScope => (
                                    self.value_read_widens(canonical, owner, &value.path),
                                    RegularContributor::Authored(TypeExpr::TypeOf(value.clone())),
                                ),
                                ArgumentReadLiterals::Frame { widens, node } => {
                                    (widens, RegularContributor::Resolved(node))
                                }
                                ArgumentReadLiterals::Unmodeled => {
                                    mixed = true;
                                    continue;
                                }
                            };
                            if widens {
                                mixed = true;
                                if separate {
                                    if let Some(node) = type_of(&contributor) {
                                        fresh.extend(literal_members(graph, node));
                                    }
                                }
                            } else {
                                regular.push(contributor);
                            }
                        }
                    }
                }
                arms.push((regular, fresh));
            }
            if arms.iter().all(|(regular, _)| regular.is_empty()) {
                continue;
            }
            let literals = if mixed {
                let mut literals = Vec::new();
                for (index, (regular, _)) in arms.iter().enumerate() {
                    for node in regular.iter().filter_map(&type_of) {
                        for literal in literal_members(graph, node) {
                            let fresh_elsewhere = arms.iter().enumerate().any(|(other, arm)| {
                                other != index
                                    && arm
                                        .1
                                        .iter()
                                        .any(|&fresh| same_literal(graph, fresh, literal))
                            });
                            if !fresh_elsewhere && !literals.contains(&literal) {
                                literals.push(literal);
                            }
                        }
                    }
                }
                if literals.is_empty() {
                    continue;
                }
                Some(Arc::from(literals.into_boxed_slice()))
            } else {
                None
            };
            positions.push(ArgumentRegularPosition {
                path: Arc::from(path.into_boxed_slice()),
                literals,
            });
        }
        (!positions.is_empty()).then(|| {
            Arc::new(ArgumentLiteralProvenance {
                positions: Arc::from(positions.into_boxed_slice()),
            })
        })
    }

    /// The type of the authored position `node` once its `children`
    /// positions are typed (`typed_child`) and its own literals in `widen`
    /// widened. `node` itself when nothing changed.
    fn rebuild_argument_position(
        &self,
        node: SemanticNodeId,
        children: &[(usize, ArgumentPosition)],
        widen: &[SemanticNodeId],
        typed_child: impl Fn(&ArgumentPosition) -> SemanticNodeId,
    ) -> SemanticNodeId {
        if children.is_empty() && widen.is_empty() {
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
            Some(SemanticNodeData::Array { element, readonly }) => {
                PositionShape::Array(*element, *readonly)
            }
            _ => return node,
        };
        match shape {
            PositionShape::Literal if widen.contains(&node) => self.widened_literal(node),
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
            PositionShape::Array(element, readonly) => {
                let typed = children
                    .first()
                    .map_or(element, |(_, child)| typed_child(child));
                if typed == element {
                    return node;
                }
                graph.intern_preserving_scope(
                    node,
                    SemanticNodeData::Array {
                        element: typed,
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
                for member in &mut members {
                    if widen.contains(member) {
                        *member = self.widened_literal(*member);
                        changed = true;
                    }
                }
                if !changed {
                    return node;
                }
                self.intern_normalized_union_or_intersection(&members, true)
            }
        }
    }

    /// The type parameters a context-sensitive argument's first-pass
    /// `reading` relates a withheld member's placeholder to as a naked
    /// inference position of `target`. A placeholder is an `any` of the
    /// reading where the argument's own type `argument` holds no `any`: the
    /// checker's non-inferrable `anyFunctionType`, which infers nothing for
    /// the parameter it lands in, where an authored `any` is a candidate.
    /// `hc({ s: 1, r: { inc(a) {} } })` over `hc<S, R extends { inc: (a: S)
    /// => void }>(o: { s: S; r: R })` reads `r` as a placeholder for `R`;
    /// `f({ a: anyv, m(x) { return x; } })` over `f<T, U>(o: { a: T; m: (x:
    /// T) => U })` reads `a` as authored. A member related to a type
    /// parameter whole relates nothing nested.
    pub(super) fn first_pass_withheld_targets(
        &self,
        reading: SemanticNodeId,
        argument: SemanticNodeId,
        target: SemanticNodeId,
    ) -> Vec<SemanticNodeId> {
        let graph = self.graph();
        let is_any = |node: SemanticNodeId| {
            matches!(
                graph
                    .node_data(self.resolved_reduction_view(node))
                    .as_deref(),
                Some(SemanticNodeData::Primitive(PrimitiveKind::Any))
            )
        };
        let members = |node: SemanticNodeId| match graph
            .node_data(self.resolved_reduction_view(node))
            .as_deref()
        {
            Some(SemanticNodeData::Object(surface)) => Some(
                surface
                    .positive_members()
                    .iter()
                    .map(|member| (member.key.clone(), member.value))
                    .collect::<Vec<_>>(),
            ),
            _ => None,
        };
        let mut withheld = Vec::new();
        let mut pending = vec![(reading, argument, target)];
        let mut seen = FxHashSet::default();
        while let Some((reading, argument, target)) = pending.pop() {
            if !seen.insert((reading, argument, target)) {
                continue;
            }
            if is_any(reading) {
                if !is_any(argument) {
                    withheld.extend(self.top_level_type_param_targets(target));
                }
                continue;
            }
            let resolved = self.resolved_reduction_view(target);
            let data = graph.node_data(resolved);
            match data.as_deref() {
                Some(
                    composite @ (SemanticNodeData::Union(_) | SemanticNodeData::Intersection(_)),
                ) => {
                    let arms: Vec<SemanticNodeId> = composite
                        .composite_members()
                        .expect("composite arm")
                        .iter()
                        .copied()
                        .collect();
                    drop(data);
                    pending.extend(arms.into_iter().map(|arm| (reading, argument, arm)));
                    continue;
                }
                Some(SemanticNodeData::TypeParam { .. }) | None => continue,
                Some(_) => drop(data),
            }
            let (Some(read), Some(own)) = (members(reading), members(argument)) else {
                continue;
            };
            for (key, value) in read {
                let Some(own_value) = own
                    .iter()
                    .find(|(own_key, _)| *own_key == key)
                    .map(|(_, own_value)| *own_value)
                else {
                    continue;
                };
                let mut crossed = FxHashSet::default();
                pending.extend(
                    self.contextual_member_types(resolved, &key, &[], &mut crossed)
                        .into_iter()
                        .map(|member_target| (value, own_value, member_target)),
                );
            }
        }
        withheld.sort_unstable();
        withheld.dedup();
        withheld
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
    /// array's element type, or a tuple's element at the position
    /// ([`tuple_element_contexts`]), per constituent; a type
    /// parameter's through its base constraint, as
    /// [`Self::contextual_member_types`] reads it.
    fn contextual_element_types(
        &self,
        contextual: SemanticNodeId,
        index: usize,
        shape: ArrayLiteralShape,
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
                        self.contextual_element_types(member, index, shape, declared, visited)
                    })
                    .collect()
            }
            Some(SemanticNodeData::TypeParam { constraint, .. }) => {
                let constraint = contextual_constraint(contextual, *constraint, declared);
                drop(data);
                match constraint {
                    Some(constraint) if visited.insert(contextual) => {
                        self.contextual_element_types(constraint, index, shape, declared, visited)
                    }
                    _ => Vec::new(),
                }
            }
            Some(SemanticNodeData::Array { element, .. }) => vec![*element],
            Some(SemanticNodeData::Tuple { elements, .. }) => {
                tuple_element_contexts(elements, index, shape, |rest| {
                    match graph.node_data(rest).as_deref() {
                        Some(SemanticNodeData::Array { element, .. }) => Some(*element),
                        _ => None,
                    }
                })
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

    /// An unfixed binder retains its constraint as a contextual literal type,
    /// the constraint instantiated with the `inferences` made so far (the
    /// checker reads a contextual type's apparent type under its non-fixing
    /// mapper): `hc({ s: 1, r: { inc(a) {} } })` over `hc<S, R extends {
    /// inc: (a: S) => void }>(o: { s: S; r: R })` types `a` as `number`.
    pub(super) fn contextual_uninferred_parameter(
        &self,
        param: SemanticNodeId,
        constraint: Option<SemanticNodeId>,
        inferences: &crate::semantic_query::CanonicalTypeSubstitution,
    ) -> SemanticNodeId {
        let Some(constraint) = constraint else {
            return param;
        };
        let constraint = self.substitute_canonical(constraint, inferences);
        let graph = self.graph();
        let Some(SemanticNodeData::TypeParam {
            decl,
            param_index,
            default,
            display_name,
            ..
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

/// Whether `a` and `b` are the same literal type, whatever scope each
/// node was interned in.
fn same_literal(
    graph: &crate::semantic_query_memo::SemanticGraphStore,
    a: SemanticNodeId,
    b: SemanticNodeId,
) -> bool {
    a == b
        || matches!(
            (graph.node_data(a).as_deref(), graph.node_data(b).as_deref()),
            (Some(SemanticNodeData::Literal(a)), Some(SemanticNodeData::Literal(b))) if a == b
        )
}

/// The parts of an authored position's type a rebuild replaces.
enum PositionShape {
    Literal,
    Object(crate::semantic_query::SurfaceView),
    Tuple(Vec<crate::semantic_query::TupleElement>, bool),
    Array(SemanticNodeId, bool),
    Union(Vec<SemanticNodeId>),
}
