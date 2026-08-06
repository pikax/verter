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
//!   a new call form must pick a constructor at its own arm. That closes
//!   the CALL vocabulary; it says nothing about whether a call REACHES
//!   it, which is the content lowering's obligation and was separately
//!   open — a call in a ternary arm or an array element folded through
//!   the frame-less shallow pass into an unreduced `ReturnType<callee>`
//!   carrier and never entered this module at all. `lower_expr`'s match
//!   over `Expression` is exhaustive now, and a leaf answer that embeds
//!   that carrier fails CLOSED, so "a call that is not a `SliceCall`"
//!   publishes nothing;
//! - a hold target's contribution to the SCC fixed point is reachable
//!   only through [`HeldCallee::discharged`], so the equation's join
//!   cannot route around the rule either;
//! - a clause is either [`CalleeClause::non_generic`] — a STATEMENT
//!   about the callee, made only where the callee's clause was actually
//!   read and found empty — or a list of parameters each of which came
//!   through [`CalleeClauseParam`]'s constructors. There is no `Default`,
//!   so "no clause" cannot be reached by omission, and a route that
//!   FAILS to read the clause cannot produce one at all — not by
//!   convention but because every constructor is PRIVATE to this module.
//!   The two entrances both demand the callee's own authority as an
//!   argument: [`CalleeClause::read_from_program_entry`] /
//!   [`CalleeClause::read_from_program_entry_at_unknown`] take a
//!   `FunctionProgramEntry` (obtainable only by looking the callee up and
//!   finding it), and [`CallValue::of_signature_node`] reads the clause
//!   off a resolved `Signature` node. A serve or index miss has nothing
//!   to hand over, returns [`CalleeClauseLookup::Unavailable`], and
//!   degrades. Minting a clause from a miss used to COMPILE.
//!
//! Exactly one hold is deliberately UNINSTANTIATED
//! ([`HeldCallee::own_frame`]) — the direct self-call, whose "callee" IS
//! this frame, so its binders are this frame's own and survive by
//! contract. It is a NAMED constructor, not a defaulted one, for the
//! same reason [`GatedType::root_signature`] is: the exemption has to be
//! asked for.
//!
//! ## What a clause parameter instantiates TO
//!
//! TypeScript resolves a call's type arguments in exactly one order:
//! explicit type arguments, else inference from the supplied arguments,
//! else the declared default (`checker.ts::getInferredTypes` takes the
//! default only when inference produced NO candidate). This substrate
//! does not model the first two — that is `U6.CALL_RESOLVE` — and
//! `unknown` is its recorded interim for both.
//!
//! The default is therefore NOT an unconditional answer: applying it
//! whenever one is declared turns the honest interim into a confidently
//! WRONG concrete type, warm-admitted. `zzMismA<ZA = string>(x: ZA)`
//! called `zzMismA(1)` is `number`, not `string`. The rule shipped here
//! is TypeScript's, expressed on the two facts the call carrier
//! ([`SliceCallSite`]) exists to carry:
//!
//! - explicit type arguments present ⇒ `unknown` (the interim);
//! - otherwise a parameter with an inference CANDIDATE — it occurs in a
//!   parameter type at an ordinal the call actually supplies ⇒
//!   `unknown` (the interim);
//! - otherwise ⇒ its declared default when it has one, else `unknown`
//!   (which is also the exact answer for the one shape TypeScript itself
//!   cannot infer: `bare<T>(): T` called with no arguments IS
//!   `unknown`).
//!
//! [`GatedType::root_signature`]: crate::flow_slice_content::GatedType::root_signature

use std::sync::Arc;

use super::ProjectSemanticDispatch;
use crate::flow_slice_content::SliceCallSite;
use crate::semantic_query::{
    ClauseSpelling, FlowReturnKey, PrimitiveKind, QueryError, SemanticNodeData, SemanticNodeId,
};
use verter_semantic::analysis::function_program::{FunctionProgramEntry, FunctionProgramTypeParam};

/// Where the node a clause instantiates into was LOWERED, which decides
/// which SPELLINGS of a clause parameter that node can contain.
///
/// One clause parameter can reach a caller under three spellings: the
/// bound `TypeParam` binder, a DEFERRED `BareRef` head, and a RESOLVED
/// `DeclRef`. The third only exists because a declaration's own clause
/// is NOT in scope where its DECLARED return locator lowers (file owner
/// scope): `first<Item>(xs: Item[]): Item` beside `interface Item {}`
/// interns its declared return as the INTERFACE, and a caller that did
/// not claim that spelling would publish an unrelated symbol.
///
/// The inverse is just as real. A BODY-DERIVED return is evaluated in
/// the callee's own frame, where the clause IS bound, so every
/// occurrence of a clause parameter there is already a `TypeParam` — and
/// a resolved `DeclRef` reached through such an arm is, by construction,
/// a DIFFERENT symbol that merely shares the name. Claiming it by name
/// destroys an exactly-correct arm (`aye<QQ>` returning `bee(): QQ`
/// answers `1 | QQ`, not `unknown`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ReturnOrigin {
    /// Lowered where the callee's own clause was NOT in scope — its
    /// DECLARED return locator in file owner scope, or a resolved callee
    /// VALUE TYPE composed from one. A same-named resolved declaration
    /// there IS the clause parameter, misresolved.
    OwnerScopeDeclared,
    /// Evaluated where the clause WAS bound — a body-derived flow return
    /// or a nested function value's composed signature. A same-named
    /// resolved declaration there is a FOREIGN symbol.
    ClauseScoped,
}

impl ReturnOrigin {
    fn spelling(self) -> ClauseSpelling {
        match self {
            Self::OwnerScopeDeclared => ClauseSpelling::WithOwnerScopeResolution,
            Self::ClauseScoped => ClauseSpelling::WithDeferredHeads,
        }
    }
}

/// Whether ARGUMENT INFERENCE could produce a candidate for one clause
/// parameter at one call site.
///
/// The single question that decides whether a declared default applies.
/// Two producers answer it from the two shapes a callee reaches this
/// module in — a shallow function-program clause fact, and a resolved
/// `Signature` node — but both answer it through
/// [`Self::at_call`], so the RULE has one home even though the oracle
/// does not.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ClauseParamInference {
    /// The call supplies an argument at a parameter position whose type
    /// names this parameter: TypeScript infers there, so the declared
    /// default is not the answer and `unknown` is the interim.
    HasCandidate,
    /// No supplied argument position names this parameter: inference
    /// produces nothing, so the callee's own declaration resolves it.
    NoCandidate,
}

impl ClauseParamInference {
    /// The verdict for a parameter whose smallest occurrence in the
    /// callee's parameter list is `first_parameter_occurrence`
    /// (`None` = it occurs in no parameter type at all).
    ///
    /// A REST parameter occupies its own ordinal and covers every later
    /// one, which the `supplies_parameter_ordinal` predicate already
    /// models, and a SPREADING call is treated as supplying every
    /// ordinal.
    pub(super) fn at_call(site: SliceCallSite, first_parameter_occurrence: Option<u32>) -> Self {
        match first_parameter_occurrence {
            Some(ordinal) if site.supplies_parameter_ordinal(ordinal) => Self::HasCandidate,
            _ => Self::NoCandidate,
        }
    }
}

/// One parameter of a CALLEE's own type-parameter clause, as a caller
/// must apply it.
#[derive(Debug, Clone)]
pub(super) struct CalleeClauseParam {
    /// The parameter name — the spelling every occurrence in the
    /// callee's return carries, whether it interned as a resolved
    /// binder, a deferred head, or a file-scope declaration the clause
    /// shadows.
    name: Arc<str>,
    /// What this parameter instantiates to: its DECLARED default when
    /// the call site leaves inference with nothing to produce, and
    /// `None` (⇒ `unknown`) in every other case. The decision is made by
    /// the constructors below and nowhere else.
    substitution: Option<SemanticNodeId>,
}

impl CalleeClauseParam {
    /// A clause parameter with no declared default: `unknown`, always.
    fn bare(name: Arc<str>) -> Self {
        Self {
            name,
            substitution: None,
        }
    }

    /// A clause parameter carrying a DECLARED DEFAULT, resolved against
    /// the call site — the ONE place TypeScript's default rule is
    /// applied.
    ///
    /// `default` is lowered LAZILY and only when the rule actually needs
    /// it, so a callee whose default the call site can never use never
    /// pays to lower it. A `None` from the producer is a genuine
    /// recovery failure at a point where the default WAS needed, so it
    /// is reported through [`ClauseParamOutcome::DefaultUnavailable`]
    /// rather than silently degrading to `unknown` — an `unknown` there
    /// is indistinguishable from the honest interim and would be warm
    /// admitted.
    fn with_default(
        name: Arc<str>,
        site: SliceCallSite,
        inference: ClauseParamInference,
        default: impl FnOnce() -> Option<SemanticNodeId>,
    ) -> ClauseParamOutcome {
        if site.has_explicit_type_arguments() || inference == ClauseParamInference::HasCandidate {
            return ClauseParamOutcome::Param(Self {
                name,
                substitution: None,
            });
        }
        match default() {
            Some(default) => ClauseParamOutcome::Param(Self {
                name,
                substitution: Some(default),
            }),
            None => ClauseParamOutcome::DefaultUnavailable,
        }
    }
}

/// The outcome of resolving one defaulted clause parameter.
enum ClauseParamOutcome {
    /// The parameter, resolved.
    Param(CalleeClauseParam),
    /// The declared default was NEEDED by the call-site rule and could
    /// not be recovered (a broken lease pin, a locator miss). Never a
    /// fabricated `unknown`.
    DefaultUnavailable,
}

/// A CALLEE's own type-parameter clause.
///
/// The clause is the caller's instantiation obligation: every parameter
/// it declares must leave the callee's return before that return can be
/// this frame's value.
///
/// Deliberately NOT `Default`: an empty clause is a STATEMENT about the
/// callee ([`Self::non_generic`]), not something a route can fall into.
#[derive(Debug, Clone)]
pub(super) struct CalleeClause {
    params: Arc<[CalleeClauseParam]>,
}

impl CalleeClause {
    /// A NON-GENERIC callee: its clause was read and found empty, so
    /// there is nothing to instantiate. The name is the point — a route
    /// that could not read the clause must not reach this constructor,
    /// and now CANNOT: this is private to the module, reachable only
    /// through a reader that was handed the callee's own authority.
    fn non_generic() -> Self {
        Self {
            params: Arc::from(Vec::new().into_boxed_slice()),
        }
    }

    /// A clause from its ordered parameters. Private for the same
    /// reason [`Self::non_generic`] is.
    fn new(params: impl IntoIterator<Item = CalleeClauseParam>) -> Self {
        Self {
            params: params.into_iter().collect(),
        }
    }

    /// READ a callee's own clause from the shallow function-program
    /// ENTRY the index answered with — the one producer on the direct
    /// rail, and the reason that rail cannot fabricate a clause.
    ///
    /// The `entry` reference IS the witness. There is no way to obtain a
    /// [`FunctionProgramEntry`] except by looking the callee up in the
    /// per-file index and finding it, so a serve miss or an index miss
    /// has nothing to pass here and returns
    /// [`CalleeClauseLookup::Unavailable`] by calling nothing at all.
    /// Before this, the caller assembled the clause itself out of
    /// `non_generic()` / `new(…)` / `bare(…)`, every one of which was
    /// reachable from a MISS — the module's claim that "a route that
    /// fails to read the clause cannot produce one" described the one
    /// existing caller rather than an enforced invariant.
    ///
    /// `default_of` lowers ONE declared default, by ordinal, and is
    /// called only for a parameter whose default the call-site rule
    /// actually needs.
    pub(super) fn read_from_program_entry(
        entry: &FunctionProgramEntry,
        site: SliceCallSite,
        mut default_of: impl FnMut(usize, &FunctionProgramTypeParam) -> Option<SemanticNodeId>,
    ) -> CalleeClauseLookup {
        if entry.type_parameters.is_empty() {
            return CalleeClauseLookup::Clause(Self::non_generic());
        }
        let mut params = Vec::with_capacity(entry.type_parameters.len());
        for (ordinal, param) in entry.type_parameters.iter().enumerate() {
            if !param.has_default {
                params.push(CalleeClauseParam::bare(Arc::clone(&param.name)));
                continue;
            }
            let inference = ClauseParamInference::at_call(site, param.first_parameter_occurrence);
            match CalleeClauseParam::with_default(Arc::clone(&param.name), site, inference, || {
                default_of(ordinal, param)
            }) {
                ClauseParamOutcome::Param(param) => params.push(param),
                ClauseParamOutcome::DefaultUnavailable => return CalleeClauseLookup::Unavailable,
            }
        }
        CalleeClauseLookup::Clause(Self::new(params))
    }

    /// READ a callee's clause for the signature-UTILITY policy, from the
    /// same [`FunctionProgramEntry`] witness the call-site reader takes.
    ///
    /// The utility policy is `unknown` for EVERY declared parameter,
    /// defaults included (`ReturnType<typeof f>` for `f<T = number>(): T`
    /// is `unknown`), so this reader states that by naming
    /// [`CalleeClauseParam::bare`] for each — it is not the call-site
    /// rule with the defaults switched off.
    ///
    /// It exists so the utility route cannot express the asymmetry it
    /// used to: a clause it FAILED to read left the callee's return
    /// untouched — the callee's own binder, published warm — while the
    /// call-site route degraded on the identical miss.
    pub(super) fn read_from_program_entry_at_unknown(entry: &FunctionProgramEntry) -> Self {
        Self::new(
            entry
                .type_parameters
                .iter()
                .map(|param| CalleeClauseParam::bare(Arc::clone(&param.name))),
        )
    }

    /// The clause parameter NAMES, in declaration order — the read-only
    /// projection the signature-UTILITY policy instantiates at `unknown`.
    pub(super) fn param_names(&self) -> impl Iterator<Item = &str> {
        self.params.iter().map(|param| param.name.as_ref())
    }

    /// Whether the callee declares NO parameters — a statement about the
    /// callee, distinct from "the clause could not be read", which has no
    /// `CalleeClause` at all.
    pub(super) fn is_empty(&self) -> bool {
        self.params.is_empty()
    }

    /// Substitute every declared parameter out of `node`: each parameter
    /// takes the substitution its constructor decided, `unknown`
    /// otherwise.
    ///
    /// `origin` selects which SPELLINGS of a clause parameter this node
    /// can contain — see [`ReturnOrigin`].
    ///
    /// A default that itself names a sibling clause parameter is
    /// instantiated in turn — see [`Self::instantiate_default`] for what
    /// that claims and what it deliberately does not.
    fn instantiate(
        &self,
        dispatch: &ProjectSemanticDispatch<'_>,
        node: SemanticNodeId,
        origin: ReturnOrigin,
    ) -> SemanticNodeId {
        if self.is_empty() {
            return node;
        }
        dispatch.instantiate_clause_params_at_call(
            self.params.iter().map(|param| {
                (
                    param.name.as_ref(),
                    param
                        .substitution
                        .map(|substitution| self.instantiate_default(dispatch, substitution)),
                )
            }),
            node,
            origin.spelling(),
        )
    }

    /// A DEFAULT is authored in the callee's own clause scope, so it can
    /// name sibling parameters, and every such sibling collapses to
    /// `unknown` here rather than escaping as the callee's binder.
    ///
    /// The default itself lowered in the callee's OWNER scope, where the
    /// clause is invisible, so every spelling is claimed here.
    ///
    /// The collapse is UNCONDITIONAL, which is exact for a sibling that
    /// is itself unresolved and an OVER-collapse for one that is not.
    /// `<A, B = A>` called with nothing answers `unknown` for both, which
    /// is TypeScript's own answer; `<SA = string, SB = SA>` answers
    /// `string` in TypeScript and `unknown` here, because `SA`'s own
    /// resolved default is not consulted. Substituting each sibling's
    /// DECIDED substitution instead is not a local change: the
    /// substitution of a sibling whose default names a further sibling is
    /// itself a fixed point, and a mutually-defaulted clause
    /// (`<A = B, B = A>`) resolves to a sibling BINDER under any depth-
    /// bounded version — the exact leak this module exists to prevent.
    /// The interim is the honest over-collapse, never a leaked binder.
    /// Owned by `U6.CALL_RESOLVE`, which owns clause resolution
    /// generally.
    fn instantiate_default(
        &self,
        dispatch: &ProjectSemanticDispatch<'_>,
        default: SemanticNodeId,
    ) -> SemanticNodeId {
        dispatch.instantiate_clause_params_at_call(
            self.params.iter().map(|param| (param.name.as_ref(), None)),
            default,
            ClauseSpelling::WithOwnerScopeResolution,
        )
    }
}

/// The outcome of READING a callee's own clause.
///
/// A failure to read the clause is NOT an empty clause: handing the
/// callee's return back with nothing instantiated is exactly the leak
/// this module exists to make inexpressible, and it would be warm
/// admitted with no degradation. The two states are therefore distinct
/// types, and only the success arm carries a [`CalleeClause`].
pub(super) enum CalleeClauseLookup {
    /// The callee's clause, read from an authority that answers for it.
    Clause(CalleeClause),
    /// The clause could NOT be read (the file is not served at this
    /// version, the position is not indexed, or a needed default could
    /// not be recovered). The caller degrades; it cannot proceed with a
    /// clause it does not have.
    Unavailable,
}

/// The value of a CALL expression inside a flow frame.
///
/// The field is private and every constructor below decides what happens
/// to the callee's own type-parameter clause, so "hand a callee's return
/// back to its caller untouched" cannot be written at a call site — the
/// no-op is reachable only by naming a NON-GENERIC callee
/// ([`CalleeClause::non_generic`]), which is a statement about the
/// callee, not an omission.
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
    /// The signature declares a defaulted clause parameter the call site
    /// needs, and the default could not be recovered.
    ClauseUnavailable,
}

impl CallValue {
    /// The call value of a resolved SIGNATURE node — the one reader of a
    /// `Signature`'s return position for call purposes.
    ///
    /// The clause comes from the signature node itself, so the rule is
    /// self-supplying: an IIFE's inline signature, a function-typed
    /// binding's signature, and a resolved callee value type all answer
    /// through this one constructor and therefore answer alike.
    ///
    /// The inference oracle is the signature's OWN parameter list: a
    /// clause parameter has a candidate exactly when it occurs in the
    /// type of a parameter the call supplies, which is the same question
    /// the shallow function-program fact answers for the direct rail,
    /// asked of the resolved shape instead of the authored one.
    pub(super) fn of_signature_node(
        dispatch: &ProjectSemanticDispatch<'_>,
        function_node: SemanticNodeId,
        site: SliceCallSite,
        origin: ReturnOrigin,
    ) -> SignatureCall {
        let graph = dispatch.graph();
        let Some(data) = graph.node_data(function_node) else {
            return SignatureCall::NotCallable;
        };
        let SemanticNodeData::Signature {
            return_type,
            type_parameters,
            params,
            ..
        } = &*data
        else {
            return SignatureCall::NotCallable;
        };
        let return_type = *return_type;
        let type_parameters = Arc::clone(type_parameters);
        let params = Arc::clone(params);
        drop(data);
        let mut clause_params = Vec::with_capacity(type_parameters.len());
        for decl in type_parameters.iter() {
            let param = match decl.default {
                None => CalleeClauseParam::bare(Arc::clone(&decl.name)),
                Some(default) => {
                    let inference = ClauseParamInference::at_call(
                        site,
                        dispatch.first_signature_param_occurrence(&params, decl.name.as_ref()),
                    );
                    match CalleeClauseParam::with_default(
                        Arc::clone(&decl.name),
                        site,
                        inference,
                        || Some(default),
                    ) {
                        ClauseParamOutcome::Param(param) => param,
                        // A signature node carries its default as an
                        // already-resolved node, so this arm is
                        // unreachable today; it stays a typed outcome
                        // rather than an `unwrap` so a future producer
                        // that CAN fail degrades instead of fabricating.
                        ClauseParamOutcome::DefaultUnavailable => {
                            return SignatureCall::ClauseUnavailable
                        }
                    }
                }
            };
            clause_params.push(param);
        }
        let clause = CalleeClause::new(clause_params);
        if matches!(
            graph.node_data(return_type).as_deref(),
            Some(SemanticNodeData::Opaque(QueryError::Miss))
        ) {
            return SignatureCall::ReturnMiss;
        }
        SignatureCall::Value(Self(clause.instantiate(dispatch, return_type, origin)))
    }

    /// The call value of a callee SERVED as a flow position — the rail
    /// whose callee answers with a bare return node and never composes a
    /// signature, so its clause is supplied separately.
    pub(super) fn of_served_return(
        dispatch: &ProjectSemanticDispatch<'_>,
        clause: &CalleeClause,
        return_node: SemanticNodeId,
        origin: ReturnOrigin,
    ) -> Self {
        Self(clause.instantiate(dispatch, return_node, origin))
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
    ///
    /// Every hold is recorded on the flow (body-derived) rail, so the
    /// discharge's origin is [`ReturnOrigin::ClauseScoped`] by
    /// construction.
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
    /// The non-generic clause states exactly that, by name.
    pub(super) fn own_frame(key: FlowReturnKey) -> Self {
        Self {
            key,
            clause: CalleeClause::non_generic(),
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
        CallValue::of_served_return(
            dispatch,
            &self.clause,
            return_node,
            ReturnOrigin::ClauseScoped,
        )
    }
}
