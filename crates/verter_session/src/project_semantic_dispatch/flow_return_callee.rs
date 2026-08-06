//! @ai-generated - The ONE gate between a CALLEE's return and its caller's value.
//!
//! A callee's return is expressed in the CALLEE's own type-parameter
//! binders. Handing it back verbatim publishes those binders as the
//! CALLER's value — and because the binder identity is file-scoped and
//! name-keyed, an unrelated enclosing `class Holder<T>` then substitutes
//! its own instantiation into a value that has nothing to do with it,
//! cleanly and warm.
//!
//! The rule that prevents it (a callee's own clause instantiates at the
//! call site) is not hard; keeping every route to it aligned is. The
//! rule was applied at one route, then a second, then a third, while
//! sibling routes kept handing the raw return back — so this module
//! makes the omission INEXPRESSIBLE rather than merely discouraged:
//!
//! - [`CallValue`]'s field is PRIVATE and every constructor lives here,
//!   so a value can only be minted by a function that has already
//!   decided what happens to the callee's clause;
//! - the flow evaluator's call sink is typed [`CallValue`], reached
//!   through one exhaustive match over the closed
//!   [`SliceCall`](crate::flow_slice_content::SliceCall) vocabulary, so
//!   a new call form must pick a constructor at its own arm;
//! - a hold target's contribution to the SCC fixed point is reachable
//!   only through [`HeldCallee::discharged`], so the equation's join
//!   cannot route around the rule either.
//!
//! Exactly one hold is deliberately UNINSTANTIATED
//! ([`HeldCallee::own_frame`]) — the direct self-call, whose "callee" IS
//! this frame, so its binders are this frame's own and survive by
//! contract. It is a NAMED constructor, not a defaulted one, for the
//! same reason [`GatedType::root_signature`] is: the exemption has to be
//! asked for.
//!
//! [`GatedType::root_signature`]: crate::flow_slice_content::GatedType::root_signature

use std::sync::Arc;

use super::ProjectSemanticDispatch;
use crate::semantic_query::{
    FlowReturnKey, PrimitiveKind, QueryError, SemanticNodeData, SemanticNodeId,
};

/// One parameter of a CALLEE's own type-parameter clause, as a caller
/// must apply it.
#[derive(Debug, Clone)]
pub(super) struct CalleeClauseParam {
    /// The parameter name — the spelling every occurrence in the
    /// callee's return carries, whether it interned as a resolved
    /// binder, a deferred head, or a file-scope declaration the clause
    /// shadows.
    name: Arc<str>,
    /// The parameter's DECLARED default, already lowered in the callee's
    /// scope. Present ⇒ that is what an argument-free call instantiates
    /// to; absent ⇒ `unknown`.
    default: Option<SemanticNodeId>,
}

impl CalleeClauseParam {
    /// A clause parameter with no declared default.
    pub(super) fn bare(name: Arc<str>) -> Self {
        Self {
            name,
            default: None,
        }
    }

    /// A clause parameter carrying its declared default.
    pub(super) fn with_default(name: Arc<str>, default: Option<SemanticNodeId>) -> Self {
        Self { name, default }
    }
}

/// A CALLEE's own type-parameter clause.
///
/// The clause is the caller's instantiation obligation: every parameter
/// it declares must leave the callee's return before that return can be
/// this frame's value.
#[derive(Debug, Clone, Default)]
pub(super) struct CalleeClause {
    params: Arc<[CalleeClauseParam]>,
}

impl CalleeClause {
    /// A non-generic callee: nothing to instantiate.
    pub(super) fn empty() -> Self {
        Self::default()
    }

    /// A clause from its ordered parameters.
    pub(super) fn new(params: impl IntoIterator<Item = CalleeClauseParam>) -> Self {
        Self {
            params: params.into_iter().collect(),
        }
    }

    fn is_empty(&self) -> bool {
        self.params.is_empty()
    }

    /// Substitute every declared parameter out of `node`: each parameter
    /// takes its declared DEFAULT when it has one, `unknown` otherwise.
    ///
    /// The default is the exact answer for an argument-free call
    /// (`f<T = number>()` IS `number`); `unknown` is the recorded
    /// interim for a parameter TypeScript would infer from arguments or
    /// explicit type arguments, and the exact answer for the one shape
    /// TypeScript itself cannot infer (`bare<T>(): T` called with no
    /// arguments IS `unknown`).
    ///
    /// A default that itself names a sibling clause parameter is
    /// instantiated in turn — `<A, B = A>` answers `unknown` for both,
    /// which is what TypeScript answers, and never leaks `A`.
    fn instantiate(
        &self,
        dispatch: &ProjectSemanticDispatch<'_>,
        node: SemanticNodeId,
    ) -> SemanticNodeId {
        if self.is_empty() {
            return node;
        }
        dispatch.instantiate_clause_params_at_call(
            self.params.iter().map(|param| {
                (
                    param.name.as_ref(),
                    param
                        .default
                        .map(|default| self.instantiate_default(dispatch, default)),
                )
            }),
            node,
        )
    }

    /// A DEFAULT is authored in the callee's own clause scope, so it can
    /// name sibling parameters. Those siblings are just as unbound at
    /// this call as the parameter being instantiated, so they collapse
    /// the same way — never escape as the callee's binders.
    fn instantiate_default(
        &self,
        dispatch: &ProjectSemanticDispatch<'_>,
        default: SemanticNodeId,
    ) -> SemanticNodeId {
        dispatch.instantiate_clause_params_at_call(
            self.params.iter().map(|param| (param.name.as_ref(), None)),
            default,
        )
    }
}

/// The value of a CALL expression inside a flow frame.
///
/// The field is private and every constructor below decides what happens
/// to the callee's own type-parameter clause, so "hand a callee's return
/// back to its caller untouched" cannot be written at a call site — the
/// no-op is reachable only by naming an EMPTY clause
/// ([`CalleeClause::empty`]), which is a statement about the callee, not
/// an omission.
#[derive(Debug, Clone, Copy)]
pub(super) struct CallValue(SemanticNodeId);

/// The outcome of taking a resolved SIGNATURE node's call value.
///
/// Three distinct states rather than an `Option`: a node that is not
/// callable at all and a signature whose return position is a semantic
/// MISS degrade differently at every call site, and collapsing them is
/// how a failed nested demand becomes a warm contributor.
pub(super) enum SignatureCall {
    /// The signature's call value, instantiated.
    Value(CallValue),
    /// The node is not a signature.
    NotCallable,
    /// The signature's return position is a semantic-miss carrier — a
    /// DEGRADED nested demand (an in-flight / failed callee), never a
    /// contributor.
    ReturnMiss,
}

impl CallValue {
    /// The call value of a resolved SIGNATURE node — the one reader of a
    /// `Signature`'s return position for call purposes.
    ///
    /// The clause comes from the signature node itself, so the rule is
    /// self-supplying: an IIFE's inline signature, a function-typed
    /// binding's signature, and a resolved callee value type all answer
    /// through this one constructor and therefore answer alike.
    pub(super) fn of_signature_node(
        dispatch: &ProjectSemanticDispatch<'_>,
        function_node: SemanticNodeId,
    ) -> SignatureCall {
        let graph = dispatch.graph();
        let Some(data) = graph.node_data(function_node) else {
            return SignatureCall::NotCallable;
        };
        let SemanticNodeData::Signature {
            return_type,
            type_parameters,
            ..
        } = &*data
        else {
            return SignatureCall::NotCallable;
        };
        let return_type = *return_type;
        let clause = CalleeClause::new(
            type_parameters
                .iter()
                .map(|decl| CalleeClauseParam::with_default(Arc::clone(&decl.name), decl.default)),
        );
        drop(data);
        if matches!(
            graph.node_data(return_type).as_deref(),
            Some(SemanticNodeData::Opaque(QueryError::Miss))
        ) {
            return SignatureCall::ReturnMiss;
        }
        SignatureCall::Value(Self(clause.instantiate(dispatch, return_type)))
    }

    /// The call value of a callee SERVED as a flow position — the rail
    /// whose callee answers with a bare return node and never composes a
    /// signature, so its clause is supplied separately.
    pub(super) fn of_served_return(
        dispatch: &ProjectSemanticDispatch<'_>,
        clause: &CalleeClause,
        return_node: SemanticNodeId,
    ) -> Self {
        Self(clause.instantiate(dispatch, return_node))
    }

    /// A call with NO callee clause in play: the implicit-`any` call of
    /// an unbound or `any`-typed binding, and the modeled `any` of a
    /// degraded callee. `any` declares no binders, so there is nothing
    /// to instantiate and nothing to leak.
    pub(super) fn modeled_any(dispatch: &ProjectSemanticDispatch<'_>) -> Self {
        Self(
            dispatch
                .graph()
                .intern_node(SemanticNodeData::Primitive(PrimitiveKind::Any)),
        )
    }

    /// The evaluated node.
    pub(super) fn into_node(self) -> SemanticNodeId {
        self.0
    }
}

/// One coinductive HOLD: a callee whose result is still provisional when
/// this frame pops, together with the instantiation obligation the SCC
/// fixed point owes it.
///
/// The clause rides WITH the key because the discharge runs long after
/// the call arm that recorded it: joining a hold target's admitted
/// return is the same transfer the call arm performs, so it must apply
/// the same rule. Carrying only the key is what let the fixed point undo
/// the call arm's instantiation.
#[derive(Debug, Clone)]
pub(super) struct HeldCallee {
    key: FlowReturnKey,
    clause: CalleeClause,
}

impl HeldCallee {
    /// A hold on ANOTHER served position — a foreign callee, whose
    /// clause the discharge must instantiate.
    pub(super) fn foreign(key: FlowReturnKey, clause: CalleeClause) -> Self {
        Self { key, clause }
    }

    /// A hold on THIS frame's own slot (direct self-recursion) — the
    /// DELIBERATELY uninstantiated one, and the only one.
    ///
    /// A direct self-call's "callee" is the frame doing the calling, so
    /// the binders in its return ARE this frame's own binders. They are
    /// not foreign, nothing aliases them, and instantiating them would
    /// erase a generic function's own parameter from its own recursive
    /// return (`f<T>(x: T): T` answering `T | unknown` instead of `T`).
    /// The empty clause states exactly that, by name.
    pub(super) fn own_frame(key: FlowReturnKey) -> Self {
        Self {
            key,
            clause: CalleeClause::empty(),
        }
    }

    /// The held target's flow identity.
    pub(super) fn key(&self) -> &FlowReturnKey {
        &self.key
    }

    /// The target's admitted return, as an arm of this entry's fixed
    /// point — the ONLY accessor that turns a hold target's result into
    /// a node, so the equation cannot join a raw callee return.
    pub(super) fn discharged(
        &self,
        dispatch: &ProjectSemanticDispatch<'_>,
        return_node: SemanticNodeId,
    ) -> CallValue {
        CallValue::of_served_return(dispatch, &self.clause, return_node)
    }
}
