//! The contextual reads of a call's arguments: whether a context-sensitive
//! argument's contextual result reads an inference, the const context of
//! a literal member whose contextual type is a `const` type parameter nested
//! in the parameter's type, and the literals an argument nests where their
//! contextual type in the parameter is a literal context for them.

use std::sync::Arc;

use rustc_hash::{FxHashMap, FxHashSet};

use super::super::contextual_tuple::{ContextualTuple, TupleElementContext};
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

/// One contextual type an array literal's elements take their own
/// contextual types from: an array's element type, which types every
/// element, or a tuple, which types each by position.
enum ElementContextSource {
    Array(SemanticNodeId),
    Tuple(ContextualTuple),
}

/// The contextual types of one array literal position's elements, read
/// once for the literal: the arrays and tuples its combined contextual
/// type resolves to, and the type parameters that read crossed, which
/// every element position is read through. The last element's answers
/// and combined contextual types are kept, so the elements a tuple's
/// middle types alike share one combined value.
struct ElementContexts {
    sources: Vec<ElementContextSource>,
    through: Arc<[SemanticNodeId]>,
    answers: Vec<TupleElementContext>,
    last: Option<(Vec<TupleElementContext>, Arc<[SemanticNodeId]>)>,
    scratch: Vec<SemanticNodeId>,
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
    combined_contexts_in(&mut contexts)
}

/// [`combined_contexts`] over a reused buffer, which it leaves sorted.
fn combined_contexts_in(contexts: &mut Vec<SemanticNodeId>) -> Arc<[SemanticNodeId]> {
    contexts.sort_unstable();
    contexts.dedup();
    Arc::from(contexts.as_slice())
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
                    decl.constraint,
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
        let reached: Vec<SemanticNodeId> =
            if provenance.is_some_and(|provenance| !provenance.positions.is_empty()) {
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
                        for literal in
                            std::iter::once(node).chain(literal_members(graph.store(), node))
                        {
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
                            let mut element_contexts = None;
                            for (index, value) in values.into_iter().enumerate() {
                                let element_contexts = element_contexts.get_or_insert_with(|| {
                                    self.element_contexts(&position, declared)
                                });
                                let contexts =
                                    self.element_contexts_at(element_contexts, index, shape);
                                let through = Arc::clone(&element_contexts.through);
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
                            let mut element_contexts = self.element_contexts(&position, declared);
                            let contexts =
                                self.element_contexts_at(&mut element_contexts, 0, shape);
                            let through = Arc::clone(&element_contexts.through);
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
                            // Joined literals are a conditional's or a read's,
                            // never one literal expression of their own.
                            if !position.root {
                                widen = self.decide_literal_position(
                                    &position,
                                    false,
                                    regular_here.flatten().as_deref(),
                                    declared,
                                    &reached,
                                    &mut keeps,
                                );
                            }
                        }
                        Some(SemanticNodeData::Literal(_)) if !position.root => {
                            drop(data);
                            let authored_literal = !provenance
                                .is_some_and(|provenance| provenance.reads_fresh(&position.path));
                            widen = self.decide_literal_position(
                                &position,
                                authored_literal,
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
    ///
    /// `authored_literal` says the position's expression is a literal of
    /// its own (`isValidConstAssertionArgument`), not a read or a
    /// conditional: only such a position is a const context through a
    /// `const` type parameter that types it.
    fn decide_literal_position(
        &self,
        position: &ArgumentPosition,
        authored_literal: bool,
        regular: Option<&[SemanticNodeId]>,
        declared: &[TypeParamDecl],
        reached: &[SemanticNodeId],
        keeps: &mut Vec<ContextKeptLiteral>,
    ) -> Vec<SemanticNodeId> {
        let (node, contexts) = (position.node, &position.contexts);
        let (regular, fresh): (Vec<SemanticNodeId>, Vec<SemanticNodeId>) =
            literal_members(self.graph().store(), node)
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
        // A literal whose contextual type is a `const` type parameter is a
        // const context (`isConstContext`), whose fresh literals are written
        // as they are whatever the parameter's constraint, and every
        // parameter the contextual type names keeps them (`f<const T>(x: {
        // v: T })` over `{ v: "a" }` is `"a"`). A read or a conditional
        // there is none: the parameter is then no literal context by itself
        // (`isLiteralOfContextualType`), and `{ v: s }` or `{ v: c ? "a" :
        // "b" }` infers `string`.
        let context = if position.const_context
            || authored_literal
                && contexts
                    .iter()
                    .any(|&contextual| self.is_const_type_variable(contextual, declared))
        {
            LiteralContext::Keep
        } else {
            self.literal_context_of(node, contexts, declared)
        };
        match context {
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
    /// intersection holding one at any depth (`(T & {}) | undefined`).
    /// TypeScript's routine (`checker.ts`, TypeScript 5.4) visits a union's
    /// or intersection's members at the depth of the composite; its bound of
    /// five counts only indexed-access, conditional and mapped hops, which
    /// this walk does not take, so no composite nesting stops it.
    fn is_const_type_variable(
        &self,
        contextual: SemanticNodeId,
        declared: &[TypeParamDecl],
    ) -> bool {
        let graph = self.graph();
        let mut pending = vec![contextual];
        while let Some(node) = pending.pop() {
            if declared
                .iter()
                .any(|decl| decl.is_const && decl.param == node)
            {
                return true;
            }
            if let Some(members) = graph
                .node_data(node)
                .as_deref()
                .and_then(SemanticNodeData::composite_members)
            {
                pending.extend(members.iter().copied());
            }
        }
        false
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
        let mut fresh_reads: Vec<Arc<[ArgumentPathStep]>> = Vec::new();
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
            let path: Arc<[ArgumentPathStep]> = Arc::from(path.into_boxed_slice());
            let separate = position.arms.len() > 1;
            let mut mixed = false;
            let mut reads_fresh = false;
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
                                reads_fresh = true;
                                if separate {
                                    if let Some(node) = type_of(&contributor) {
                                        fresh.extend(literal_members(graph.store(), node));
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
            if reads_fresh {
                fresh_reads.push(Arc::clone(&path));
            }
            if arms.iter().all(|(regular, _)| regular.is_empty()) {
                continue;
            }
            let literals = if mixed {
                let mut literals = Vec::new();
                for (index, (regular, _)) in arms.iter().enumerate() {
                    for node in regular.iter().filter_map(&type_of) {
                        for literal in literal_members(graph.store(), node) {
                            let fresh_elsewhere = arms.iter().enumerate().any(|(other, arm)| {
                                other != index
                                    && arm
                                        .1
                                        .iter()
                                        .any(|&fresh| same_literal(graph.store(), fresh, literal))
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
            positions.push(ArgumentRegularPosition { path, literals });
        }
        (!positions.is_empty() || !fresh_reads.is_empty()).then(|| {
            Arc::new(ArgumentLiteralProvenance {
                positions: Arc::from(positions.into_boxed_slice()),
                fresh_reads: Arc::from(fresh_reads.into_boxed_slice()),
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
    /// T) => U })` reads `a` as authored. A parameter an authored `any`
    /// also lands in is none of them: `f({ a: anyv, b(x) { return x; }, m(x)
    /// { return x; } })` over `f<T, U>(o: { a: T; b: T; m: (x: T) => U })`
    /// infers `any` for `T`. A member related to a type parameter whole
    /// relates nothing nested.
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
        let mut authored = Vec::new();
        let mut pending = vec![(reading, argument, target)];
        let mut seen = FxHashSet::default();
        while let Some((reading, argument, target)) = pending.pop() {
            if !seen.insert((reading, argument, target)) {
                continue;
            }
            if is_any(reading) {
                if is_any(argument) {
                    authored.extend(self.top_level_type_param_targets(target));
                } else {
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
        // A parameter an authored `any` also lands in holds that `any` as a
        // candidate already, which the placeholder's `any` only repeats.
        withheld.retain(|param| !authored.contains(param));
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
                    graph.store(),
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

    /// The contextual types the elements of the array literal at
    /// `position` take from its combined contextual type
    /// (`getContextualTypeForElementExpression`), read once for the
    /// literal: per constituent, an array's element type or a tuple with
    /// its shape; a type parameter's through its base constraint, as
    /// [`Self::contextual_member_types`] reads it.
    fn element_contexts(
        &self,
        position: &ArgumentPosition,
        declared: &[TypeParamDecl],
    ) -> ElementContexts {
        let mut sources = Vec::new();
        let mut crossed = FxHashSet::default();
        for &contextual in position.contexts.iter() {
            self.element_context_sources(contextual, declared, &mut crossed, &mut sources);
        }
        ElementContexts {
            sources,
            through: position.child_through(crossed),
            answers: Vec::new(),
            last: None,
            scratch: Vec::new(),
        }
    }

    /// The combined contextual type `contexts` give the element at
    /// `index` of a literal shaped `shape`, in one canonical order.
    fn element_contexts_at(
        &self,
        contexts: &mut ElementContexts,
        index: usize,
        shape: ArrayLiteralShape,
    ) -> Arc<[SemanticNodeId]> {
        contexts.answers.clear();
        for source in &contexts.sources {
            contexts.answers.push(match source {
                ElementContextSource::Array(element) => TupleElementContext::Element(*element),
                ElementContextSource::Tuple(tuple) => {
                    self.tuple_element_context(tuple, index, shape)
                }
            });
        }
        if let Some((answers, combined)) = &contexts.last {
            if answers.len() == contexts.answers.len()
                && answers
                    .iter()
                    .zip(&contexts.answers)
                    .all(|(last, answer)| last.same_as(answer))
            {
                return Arc::clone(combined);
            }
        }
        contexts.scratch.clear();
        for answer in &contexts.answers {
            match answer {
                TupleElementContext::None => {}
                TupleElementContext::Element(element) => contexts.scratch.push(*element),
                TupleElementContext::Middle(middle) => {
                    contexts.scratch.extend_from_slice(middle);
                }
            }
        }
        let combined = combined_contexts_in(&mut contexts.scratch);
        let answers = std::mem::take(&mut contexts.answers);
        contexts.last = Some((answers, Arc::clone(&combined)));
        combined
    }

    /// Push the arrays and tuples `contextual` gives array literal
    /// elements their contextual types from to `sources`, recording each
    /// type parameter read through its constraint in `visited`.
    fn element_context_sources(
        &self,
        contextual: SemanticNodeId,
        declared: &[TypeParamDecl],
        visited: &mut FxHashSet<SemanticNodeId>,
        sources: &mut Vec<ElementContextSource>,
    ) {
        let graph = self.graph();
        let contextual = self.resolved_reduction_view(contextual);
        let Some(data) = graph.node_data(contextual) else {
            return;
        };
        match &*data {
            composite @ (SemanticNodeData::Union(_) | SemanticNodeData::Intersection(_)) => {
                let members: Vec<SemanticNodeId> = composite
                    .composite_members()
                    .expect("composite arm")
                    .iter()
                    .copied()
                    .collect();
                drop(data);
                for member in members {
                    self.element_context_sources(member, declared, visited, sources);
                }
            }
            SemanticNodeData::TypeParam { constraint, .. } => {
                let constraint = contextual_constraint(contextual, *constraint, declared);
                drop(data);
                if let Some(constraint) = constraint {
                    if visited.insert(contextual) {
                        self.element_context_sources(constraint, declared, visited, sources);
                    }
                }
            }
            SemanticNodeData::Array { element, .. } => {
                sources.push(ElementContextSource::Array(*element));
            }
            SemanticNodeData::Tuple { elements, .. } => {
                sources.push(ElementContextSource::Tuple(
                    self.contextual_tuple(contextual, elements),
                ));
            }
            _ => {}
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
        let held =
            literal_members(graph.store(), candidate)
                .into_iter()
                .fold(0u8, |held, literal| {
                    match graph.node_data(literal).as_deref() {
                        Some(SemanticNodeData::Literal(value)) => {
                            held | LiteralKind::of(value).bit()
                        }
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

/// The two types of a failed relation as the checker's message names them.
pub(in crate::project_semantic_dispatch) struct RelationOperands {
    pub(in crate::project_semantic_dispatch) source: String,
    pub(in crate::project_semantic_dispatch) target: String,
    /// Both types spell the same name even fully qualified: the checker
    /// tells them apart with TS2719's message.
    pub(in crate::project_semantic_dispatch) same_name: bool,
}

/// The file name TypeScript names a source file's module symbol by
/// (`removeFileExtension`).
fn module_specifier(canonical: &str) -> &str {
    [
        ".d.ts", ".d.mts", ".d.cts", ".mjs", ".mts", ".cjs", ".cts", ".ts", ".js", ".tsx", ".jsx",
        ".json",
    ]
    .iter()
    .find_map(|extension| canonical.strip_suffix(extension))
    .unwrap_or(canonical)
}

impl<C: crate::resolver_core::ResolverCapabilities> ProjectSemanticDispatch<'_, C> {
    /// The two types of a failed relation as the checker prints them in its
    /// message.
    pub(in crate::project_semantic_dispatch) fn print_relation_operands(
        &self,
        source: SemanticNodeId,
        target: SemanticNodeId,
    ) -> Option<(String, String)> {
        let operands = self.relation_operands(source, target)?;
        Some((operands.source, operands.target))
    }

    /// The two types of a failed relation as every message about the pair
    /// names them (`getTypeNamesForErrorDisplay`): two types that spell the
    /// same text are printed fully qualified.
    pub(in crate::project_semantic_dispatch) fn relation_type_names(
        &self,
        source: SemanticNodeId,
        target: SemanticNodeId,
    ) -> Option<(String, String)> {
        let printed_source = self.print_diagnostic_type(source)?;
        let printed_target = self.print_diagnostic_type(target)?;
        if printed_source != printed_target {
            return Some((printed_source, printed_target));
        }
        Some((
            self.print_qualified_diagnostic_type(source)?,
            self.print_qualified_diagnostic_type(target)?,
        ))
    }

    /// The two types of a failed relation as the checker names them in a
    /// message relating the pair (`reportRelationError`): their
    /// [`Self::relation_type_names`], and a literal source printed as its
    /// base primitive unless the target could itself hold a literal at its
    /// top level, where the literal is what tells the two apart.
    pub(in crate::project_semantic_dispatch) fn relation_operands(
        &self,
        source: SemanticNodeId,
        target: SemanticNodeId,
    ) -> Option<RelationOperands> {
        // The checker keeps the literal against `never` so an exhaustiveness
        // check (`assertNever(k)`) names the value that escaped.
        let target_is_never = matches!(
            self.graph().node_data(target).as_deref(),
            Some(SemanticNodeData::Primitive(PrimitiveKind::Never))
        );
        let generalized = if target_is_never
            || self.could_hold_top_level_singleton(target, SINGLETON_SCAN_DEPTH)
        {
            None
        } else {
            self.literal_base_spelling(source)?
        };
        let (printed_source, printed_target) = self.relation_type_names(source, target)?;
        let same_name = printed_source == printed_target;
        Some(RelationOperands {
            source: generalized.unwrap_or(printed_source),
            target: printed_target,
            same_name,
        })
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
                        self.print_diagnostic_type_clipped(*source),
                        self.print_diagnostic_type_clipped(*target),
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
        let (diagnostic, arguments) = self.checker_recovery_arguments(node)?;
        let code = diagnostic.code;
        Some(format!("TS{}: {}", code.code(), code.format(&arguments)))
    }

    /// The diagnostic a recovery node carries with the printed values of its
    /// message's placeholders, in order: TS2456 names the alias its origin
    /// refers to, a message with no placeholder takes none. `None` for any
    /// other node, and for a recovery whose message needs an argument it
    /// does not carry — never a message with a placeholder left unfilled.
    pub fn checker_recovery_arguments(
        &self,
        node: SemanticNodeId,
    ) -> Option<(crate::semantic_query::CheckerDiagnostic, Vec<String>)> {
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
        Some((*diagnostic, arguments))
    }

    /// The occurrence of the diagnostic a recovery node carries, bound NOW
    /// to where the checker reports it for the body of `canonical`: its
    /// bound message arguments ([`Self::checker_recovery_arguments`]) and
    /// its authored report location. A recovery with an authored location
    /// (TS2456, at the circular alias's name) is reported there, and only
    /// by the body of the alias's own file — a name span of another file
    /// is no position of this one; any other recovery is reported at the
    /// call's `call_span`. The location is read freshly on every bind,
    /// never kept from the first caller. `None` when the recovery has no
    /// complete message or no position in this file.
    pub fn bind_checker_recovery(
        &self,
        node: SemanticNodeId,
        canonical: &str,
        call_span: verter_span::Span,
    ) -> Option<crate::semantic_query::CheckerDiagnosticOccurrence> {
        let (diagnostic, arguments) = self.checker_recovery_arguments(node)?;
        let span = match diagnostic.code {
            crate::semantic_query::CheckerDiagnosticCode::CircularTypeAlias => {
                let (alias_file, span) = self.checker_recovery_location(node)?;
                if alias_file.as_ref() != canonical {
                    return None;
                }
                span
            }
            _ => call_span,
        };
        Some(crate::semantic_query::CheckerDiagnosticOccurrence::new(
            span.start,
            span.end,
            diagnostic,
            arguments
                .into_iter()
                .map(String::into_boxed_str)
                .collect::<Vec<_>>()
                .into_boxed_slice(),
            Box::default(),
        ))
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

    /// One type as a message prints it, never withheld. For a message with
    /// no occurrence to withhold (a refused relation's two types): where
    /// the checker's abbreviation is not known exactly
    /// ([`Self::print_diagnostic_type`] is `None`) the type prints every
    /// part, clipped where `typeToString` clips.
    fn print_diagnostic_type_clipped(&self, node: SemanticNodeId) -> String {
        self.print_diagnostic_type(node).unwrap_or_else(|| {
            let strict_null_checks = self.relation_strict_config().strict_null_checks;
            crate::semantic_query::display::clip_checker_text(
                self.print_diagnostic_type_text(node, false, || {
                    crate::semantic_query::display::DisplayContext::for_checker(strict_null_checks)
                        .unabbreviated()
                })
                .0,
            )
        })
    }

    /// One type as a diagnostic message prints it (`typeToString`),
    /// abbreviated as the checker's node builder abbreviates it: past the
    /// truncation length a union's or an intersection's remaining members
    /// print as `... N more ...` before its last, an object's remaining
    /// properties likewise, an object type it begins past the length, a
    /// type-argument or tuple list as `...`, and the whole text is clipped at
    /// twice that length (TypeScript 7.1.0-beta).
    /// `None` when the print cannot be the checker's: a part whose
    /// node-builder count is not modelled could move where it abbreviates,
    /// or the display depth cap cut a part the message keeps.
    pub(in crate::project_semantic_dispatch) fn print_diagnostic_type(
        &self,
        node: SemanticNodeId,
    ) -> Option<String> {
        let strict_null_checks = self.relation_strict_config().strict_null_checks;
        let (printed, context) = self.print_diagnostic_type_text(node, false, || {
            crate::semantic_query::display::DisplayContext::for_checker(strict_null_checks)
        });
        context.checker_text(printed)
    }

    /// One type as the checker prints it to tell apart two types that spell
    /// the same text (`getTypeNameForErrorDisplay`, `UseFullyQualifiedType`).
    /// `None` when the print cannot be the checker's, including a declaration
    /// whose symbol chain is not known.
    fn print_qualified_diagnostic_type(&self, node: SemanticNodeId) -> Option<String> {
        let strict_null_checks = self.relation_strict_config().strict_null_checks;
        let (printed, context) = self.print_diagnostic_type_text(node, true, || {
            crate::semantic_query::display::DisplayContext::for_checker(strict_null_checks)
        });
        context.checker_text(printed)
    }

    /// The symbol chain `typeToString` prints a declaration through under
    /// `UseFullyQualifiedType`: a module export is named through its module
    /// (`getAliasForSymbolInContainer` prefers the declaration's own name),
    /// while a global and an unexported declaration have no container.
    fn declaration_chain(
        &self,
        declaration: &crate::semantic_query::display::DeclarationName,
    ) -> crate::semantic_query::display::DeclarationChain {
        use crate::semantic_query::display::DeclarationChain;
        use verter_session_query::inputs::contributors::{
            classify_shallow_module_kind, FileModuleKind,
        };
        use verter_session_query::inputs::shallow::ExportTarget;
        if declaration.canonical_id.is_empty() {
            return DeclarationChain::Unqualified;
        }
        let Some(indexed) = self.transaction_indexed_record(&declaration.canonical_id) else {
            return DeclarationChain::Unknown;
        };
        let shallow = &indexed.shallow_state;
        if !shallow
            .headers
            .namespace_member_is_exported(declaration.owner, &declaration.decl_name)
        {
            // `getAccessibleSymbolChain` stops at the innermost private
            // member. Its containing namespaces cannot name it from here.
            let end = declaration
                .decl_name
                .match_indices('.')
                .map(|(dot, _)| dot)
                .skip(1)
                .chain([declaration.decl_name.len()])
                .filter(|&end| {
                    shallow.headers.namespace_private_members.contains(
                        &verter_type_expr::DeclBindingKey::new(
                            declaration.owner,
                            &declaration.decl_name[..end],
                        ),
                    )
                })
                .last()
                .expect("the visibility inventory names a private segment");
            let start = declaration.decl_name[..end]
                .rfind('.')
                .map_or(0, |dot| dot + 1);
            return DeclarationChain::NamespaceLocal {
                name: declaration.decl_name[start..].to_owned(),
            };
        }
        if classify_shallow_module_kind(shallow) != FileModuleKind::Module {
            return DeclarationChain::Unqualified;
        }
        let (head, path) = match declaration.decl_name.split_once('.') {
            Some((head, path)) => (head, Some(path)),
            None => (declaration.decl_name.as_ref(), None),
        };
        // `export =` makes the declaration the module itself, leaving only
        // the path below it to print.
        if declaration.owner == verter_type_expr::TopLevelOwnerId::ordinary_file()
            && shallow.export_assignment_target() == Some(head)
        {
            let specifier = module_specifier(&declaration.canonical_id).to_owned();
            return match path {
                Some(path) => DeclarationChain::Module {
                    specifier,
                    name: path.to_owned(),
                },
                None => DeclarationChain::AssignedModule { specifier },
            };
        }
        let exports_head = |target: &ExportTarget| matches!(target, ExportTarget::Local { owner, symbol_name } if *owner == declaration.owner && symbol_name == head);
        let export = if shallow.exports.get(head).is_some_and(exports_head) {
            head
        } else {
            let mut aliases = shallow
                .exports
                .iter()
                .filter(|(_, target)| exports_head(target));
            match (aliases.next(), aliases.next()) {
                (None, _) => return DeclarationChain::Unqualified,
                (Some((alias, _)), None) => alias.as_str(),
                // The checker takes the first in declaration order, which
                // the export table does not record.
                (Some(_), Some(_)) => return DeclarationChain::Unknown,
            }
        };
        DeclarationChain::Module {
            specifier: module_specifier(&declaration.canonical_id).to_owned(),
            name: match path {
                Some(path) => format!("{export}.{path}"),
                None => export.to_owned(),
            },
        }
    }

    /// Prepare evaluated forms for a read-only print: closed mapped types,
    /// spread alternatives and merged declarations. A prepared surface may
    /// contain further unevaluated types, so repeat until none is new.
    fn print_diagnostic_type_text(
        &self,
        node: SemanticNodeId,
        qualified: bool,
        context: impl Fn() -> crate::semantic_query::display::DisplayContext,
    ) -> (String, crate::semantic_query::display::DisplayContext) {
        use crate::semantic_query::{DisplayFacet, DisplayNeeds};
        let mut needs = DisplayNeeds::single(DisplayFacet::CheckerSpelling)
            .union(DisplayNeeds::single(DisplayFacet::IncludeReadonlyModifier));
        if qualified {
            needs = needs.union(DisplayNeeds::single(DisplayFacet::QualifyNames));
        }
        let mut settled = FxHashMap::default();
        let mut decisions = FxHashMap::default();
        let mut chains = FxHashMap::default();
        let mut tried = FxHashSet::default();
        loop {
            let mut printing = context()
                .with_settled_types(settled, decisions)
                .with_declaration_chains(chains);
            let printed = crate::semantic_query::display::display_type_node(
                self.graph().store(),
                node,
                needs,
                crate::semantic_query::display::MAX_DISPLAY_DEPTH,
                &mut printing,
            )
            .0;
            let newly_settled: Vec<(SemanticNodeId, SemanticNodeId)> = printing
                .unsettled_types()
                .iter()
                .copied()
                .filter(|mapped| tried.insert(*mapped))
                .filter_map(|mapped| Some((mapped, self.settled_diagnostic_type(mapped)?)))
                .collect();
            let newly_decided: Vec<_> = printing
                .unsettled_infer_constraints()
                .iter()
                .map(|request| {
                    (
                        request.clone(),
                        self.omit_diagnostic_infer_constraint(request),
                    )
                })
                .collect();
            let newly_chained: Vec<_> = printing
                .unchained_declarations()
                .iter()
                .map(|declaration| (declaration.clone(), self.declaration_chain(declaration)))
                .collect();
            if newly_settled.is_empty() && newly_decided.is_empty() && newly_chained.is_empty() {
                return (printed, printing);
            }
            chains = printing.take_declaration_chains();
            chains.extend(newly_chained);
            (settled, decisions) = printing.into_settled_types();
            settled.extend(newly_settled);
            decisions.extend(newly_decided);
        }
    }

    /// Infer constraints print only when they differ from the constraint
    /// inferred from their declaration positions. Use the shared canonical
    /// algebra and identity relation, including aliases and use-site frames.
    fn omit_diagnostic_infer_constraint(
        &self,
        request: &crate::semantic_query::display::InferConstraintRequest,
    ) -> bool {
        use crate::semantic_query::display::ImplicitInferConstraint;
        use crate::semantic_query::{PendingSubstitutionFrame, PrimitiveKind};
        let primitive = |kind| self.graph().intern_node(SemanticNodeData::Primitive(kind));
        let inferred: Vec<_> = request.inferred.iter().map(|constraint| match *constraint {
            ImplicitInferConstraint::String => primitive(PrimitiveKind::String),
            ImplicitInferConstraint::UnknownArray => self.graph().intern_node(SemanticNodeData::Array {
                element: primitive(PrimitiveKind::Unknown), readonly: false,
            }),
            ImplicitInferConstraint::PropertyKey => {
                let members = [primitive(PrimitiveKind::String), primitive(PrimitiveKind::Number), primitive(PrimitiveKind::Symbol)];
                crate::project_semantic_dispatch::canonical_algebra::intern_ordered_union_charged(self.graph(), &members,
                    verter_session_query::flow::policy::NullabilityPolicy::Strict,
                    self.construction_meter()).node
            }
            ImplicitInferConstraint::MappedValue { value, parameter, key_space } =>
                self.substitute_semantic_type_param(value, parameter, key_space),
        }).collect();
        let inferred = crate::project_semantic_dispatch::canonical_algebra::intern_ordered_intersection_charged(
            self.graph(), &inferred, self.construction_meter()).node;
        let mut frame = PendingSubstitutionFrame::empty();
        for &(parameter, argument) in &request.substitutions {
            frame = frame.append(parameter, argument);
        }
        let actual = self.apply_pending_substitution_frame(request.actual, &frame);
        let inferred = self.apply_pending_substitution_frame(inferred, &frame);
        self.relate_type_identity(actual, inferred) == Some(true)
    }

    /// The evaluated form the checker prints. Use the shared mapped, spread
    /// and peer-merge authorities; retain generic mapped types as such.
    fn settled_diagnostic_type(&self, mapped: SemanticNodeId) -> Option<SemanticNodeId> {
        let data = self.graph().node_data(mapped)?;
        if matches!(data.as_ref(), SemanticNodeData::TypeOf(_)) {
            return Some(self.evaluate_deferred_semantic_node(mapped));
        }
        if let SemanticNodeData::MergedDecl { contributors } = data.as_ref() {
            return Some(self.reduce_merged_decl(contributors));
        }
        if matches!(data.as_ref(), SemanticNodeData::ObjectSpreadProgram(_)) {
            return self.evaluated_object_spread_type(mapped);
        }
        let SemanticNodeData::Mapped { source, mapper } = data.as_ref() else {
            return None;
        };
        let object = self.mapped_object_for_relation(
            *source,
            mapper,
            crate::semantic_query::ProjectionReductionContext::structural_transit(),
        )?;
        matches!(
            self.graph().node_data(object).as_deref(),
            Some(SemanticNodeData::Object(_))
        )
        .then_some(object)
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
                let node = self.regularized_empty_object_program(node);
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
                    .any(|present| same_property_key(&present.key, &member.key))
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
            | SemanticNodeData::TemplateLiteral { .. }
            | SemanticNodeData::IntrinsicApplication { .. }
            | SemanticNodeData::TypeOfNominal(_) => true,
            // Under `strictNullChecks: false` the checker never builds the
            // `null` and `undefined` types, so neither holds a literal.
            SemanticNodeData::Primitive(PrimitiveKind::Null | PrimitiveKind::Undefined) => {
                self.relation_strict_config().strict_null_checks
            }
            _ => false,
        }
    }

    /// The base type a literal source prints as
    /// (`getBaseTypeOfLiteralType`), always fully qualified
    /// (`getTypeNameForErrorDisplay`): a literal's primitive, an enum member's
    /// enum through its symbol chain, and each member of a union of them
    /// mapped so, in order and once. An alias or a reference to an enum is
    /// what it settles to, as the checker reads the type a name declares.
    /// `Some(None)` for any type that is not a literal, an enum member or a
    /// union of them; `None` when the print cannot be the checker's, for an
    /// enum whose symbol chain is not known or a union of bases long enough
    /// for the node builder to abbreviate.
    fn literal_base_spelling(&self, source: SemanticNodeId) -> Option<Option<String>> {
        use crate::semantic_query::display::{clip_checker_text, DeclarationName};
        let source = self.settle_through_carriers(source).unwrap_or(source);
        let base = |node: SemanticNodeId| -> Option<Option<String>> {
            let Some(data) = self.graph().node_data(node) else {
                return Some(None);
            };
            Some(match data.as_ref() {
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
                    let declaration = DeclarationName {
                        owner: literal.enum_decl.owner,
                        canonical_id: literal.enum_decl.canonical_id.clone(),
                        decl_name: literal.enum_decl.decl_name.clone(),
                    };
                    let chain = self.declaration_chain(&declaration);
                    Some(chain.spelled(&literal.enum_decl.decl_name)?.0)
                }
                _ => None,
            })
        };
        if let Some(single) = base(source)? {
            return Some(Some(clip_checker_text(single)));
        }
        let Some(data) = self.graph().node_data(source) else {
            return Some(None);
        };
        let SemanticNodeData::Union(members) = data.as_ref() else {
            return Some(None);
        };
        let strict = self.relation_strict_config().strict_null_checks;
        let (mut has_null, mut has_undefined) = (false, false);
        let mut bases: Vec<String> = Vec::new();
        for &member in members.iter() {
            // `null` and `undefined` are unit types the base mapping leaves
            // as they are; without `strictNullChecks` the checker never
            // builds them, so they leave the union.
            match self.graph().node_data(member).as_deref() {
                Some(SemanticNodeData::Primitive(PrimitiveKind::Null)) => {
                    has_null = strict;
                    continue;
                }
                Some(SemanticNodeData::Primitive(PrimitiveKind::Undefined)) => {
                    has_undefined = strict;
                    continue;
                }
                _ => {}
            }
            // Inside a union the checker holds `boolean` as `true | false`,
            // two unit types whose base is `boolean`.
            let spelled = match self.graph().node_data(member).as_deref() {
                Some(SemanticNodeData::Primitive(PrimitiveKind::Boolean)) => "boolean".to_owned(),
                _ => match base(member)? {
                    Some(spelled) => spelled,
                    None => return Some(None),
                },
            };
            if !bases.contains(&spelled) {
                bases.push(spelled);
            }
        }
        if bases.is_empty() {
            return Some(None);
        }
        // The checker prints the bases in its intrinsic type-id order, the
        // enums a program declares after them.
        bases.sort_by_key(|base| {
            ["string", "number", "bigint", "boolean"]
                .iter()
                .position(|intrinsic| intrinsic == base)
                .unwrap_or(4)
        });
        // `typeToString` prints a union's nullish members last, `null`
        // before `undefined`.
        if has_null {
            bases.push("null".to_owned());
        }
        if has_undefined {
            bases.push("undefined".to_owned());
        }
        let printed = bases.join(" | ");
        if bases.len() == 1 {
            return Some(Some(clip_checker_text(printed)));
        }
        (printed.len() <= crate::semantic_query::checker_policy::MAXIMUM_TRUNCATION_LENGTH)
            .then_some(Some(printed))
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

/// Whether two authored keys address the same JavaScript property: equal, or
/// a numeric key and the string spelling of its canonical index (`1` and
/// `"1"`).
pub(super) fn same_property_key(a: &AuthoredPropertyKey, b: &AuthoredPropertyKey) -> bool {
    a == b
        || a.as_known()
            .zip(b.as_known())
            .is_some_and(|(a, b)| a.element_access_collides(&b))
}
