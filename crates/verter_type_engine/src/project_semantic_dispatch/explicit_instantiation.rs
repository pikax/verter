//! Explicit instantiation: a callable's own clause bound to explicit type
//! arguments — an instantiation expression's (`typeof f<string>`) or a call
//! site's (`f<string>(…)`) — as one owned machine.
//!
//! The machine ([`ExplicitInstantiation`]) binds the clause parameter by
//! parameter: the supplied argument, or the declared default instantiated
//! under the bindings fixed before it, then that argument substituted for
//! every occurrence of the parameter's binder. A sealed deferred callable
//! substitutes its own parameters and source images instead. Each step that
//! substitutes names its substitution, which the machine's driver makes:
//!
//! - in place ([`ProjectSemanticDispatch::apply_typeof_instantiation_args`]),
//!   through the public substitution entry, which reads where it stands;
//! - inside an overload set's continuation or a type lowering, on the walk
//!   the machine holds ([`ProjectSemanticDispatch::begin_explicit_instantiation`]):
//!   a template reduction the walk reads is its holder's read, and the
//!   machine resumes from what it retained with its delivery.
//!
//! A walk standing at a read keeps its levels' cost recordings open on the
//! connected demand, so the demand must stay open until the walk answers:
//! a root demand the read entered would clear them, and the completed
//! substitution could not publish its priced memo entry. Its holder says who
//! keeps it ([`InstantiationDemand`]); the machine itself never keeps a guard
//! across a read.
//!
//! A signature-bearing object retains the shared signature-list continuation
//! for each bucket, call then construct, before instantiating its candidates.
//! Both drivers consume those same list steps and their original value domain.
//!
//! [`ProjectSemanticDispatch::explicit_clause_fit`] is the read-free half of
//! the drop authority: whether a candidate's clause binds a given number of
//! explicit arguments, decided from the clause alone, exactly when the
//! machine drops it.

use std::sync::Arc;

use super::continuation_stages::{
    unsettled_overload_set, ListProgress, OverloadSetBuild, OverloadSetStage, SignatureListRead,
};
use super::family_continuation::{FamilyBuild, FamilyStep};
use super::substitute::{SubstitutionStep, SubstitutionWalk};
use super::ProjectSemanticDispatch;
use crate::semantic_query::{
    CacheRead, FunctionParam, QueryError, QueryResult, SemanticNodeData, SemanticNodeId,
    SemanticQueryKey, SemanticQueryValue, TypeParamDecl,
};

/// Whether a candidate's clause binds a number of explicit type arguments,
/// decided without reading anything.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ClauseFit {
    /// A generic signature or deferred callable that takes the arguments:
    /// every parameter past the supplied ones has a default.
    Binds,
    /// The candidate drops: no clause, too many arguments, or a parameter
    /// neither supplied nor defaulted.
    Drops,
    /// A signature-bearing object, whose fit is its signature buckets'.
    ReadsBuckets,
}

/// Whether `clause` admits `supplied` explicit arguments at all: it is
/// non-empty and takes at least that many parameters.
fn clause_admits(clause: &[TypeParamDecl], supplied: usize) -> bool {
    !clause.is_empty() && supplied <= clause.len()
}

/// Whether `clause` binds `supplied` explicit arguments: it admits them,
/// and every parameter past them has a default.
fn clause_fit(clause: &[TypeParamDecl], supplied: usize) -> ClauseFit {
    if clause_admits(clause, supplied)
        && clause[supplied..].iter().all(|decl| decl.default.is_some())
    {
        ClauseFit::Binds
    } else {
        ClauseFit::Drops
    }
}

/// One explicit instantiation between its steps: its retained signature
/// bucket or clause, the bindings fixed in declaration order, and the
/// substitution walk it stands in, empty between substitutions.
pub(super) struct ExplicitInstantiation {
    bound: Vec<(SemanticNodeId, SemanticNodeId)>,
    stage: InstantiationStage,
    walk: SubstitutionWalk,
}

static_assertions::assert_impl_all!(ExplicitInstantiation: Send);
static_assertions::assert_not_impl_any!(ExplicitInstantiation: Clone, Copy);

impl ExplicitInstantiation {
    pub(super) fn new() -> Self {
        Self {
            bound: Vec::new(),
            stage: InstantiationStage::Finished,
            walk: SubstitutionWalk::default(),
        }
    }
}

enum InstantiationStage {
    Signature(Box<SignatureClause>),
    Callable(Box<CallableClause>),
    Object(Box<ObjectSignatures>),
    Finished,
}

/// The object's call list, then its construct list. A settled call bucket
/// stays owned while the construct list settles; candidate instantiation
/// starts only after both buckets are complete.
struct ObjectSignatures {
    node: SemanticNodeId,
    surface: crate::semantic_query::SurfaceView,
    calls: Option<Vec<SemanticNodeId>>,
    list: SignatureListRead,
}

/// A generic signature's clause being bound, `result` the signature
/// substituted so far.
struct SignatureClause {
    clause: Arc<[TypeParamDecl]>,
    index: usize,
    result: SemanticNodeId,
    at: ClauseStep,
}

/// Where binding the parameter at `index` stands.
enum ClauseStep {
    /// Choosing its argument.
    Argument,
    /// Instantiating its declared default under `bound[bound_at..]`.
    Default {
        value: SemanticNodeId,
        bound_at: usize,
    },
    /// Substituting `arg` for each of its binder occurrences, collected once.
    Binders {
        arg: SemanticNodeId,
        binders: Vec<SemanticNodeId>,
        at: usize,
    },
}

/// A sealed deferred callable's clause being bound into its own parameters
/// and source images.
struct CallableClause {
    callable: crate::semantic_query::DeferredCallable,
    clause: Arc<[TypeParamDecl]>,
    params: Vec<FunctionParam>,
    index: usize,
    at: CallableStep,
}

/// Where binding the callable's parameter at `index` stands, then its
/// images.
enum CallableStep {
    Argument,
    Default {
        value: SemanticNodeId,
        bound_at: usize,
    },
    /// Substituting `arg` into each of the callable's parameters.
    Param {
        arg: SemanticNodeId,
        at: usize,
    },
    /// Substituting every binding into each source image in turn.
    Images {
        images: Vec<SemanticNodeId>,
        image: usize,
        bound_at: usize,
    },
}

/// What an instantiation does next: the substitution it makes, or its
/// result.
enum InstantiationNeed {
    Substitute(SemanticNodeId, SemanticNodeId, SemanticNodeId),
    Read(SemanticQueryKey),
    Done(SemanticNodeId),
}

/// Who keeps the connected demand an instantiation's substitution walk
/// records into open while the walk stands at a read.
pub(super) enum InstantiationDemand<'s, 'g> {
    /// The holder keeps the enclosing connected demand open across every
    /// read the instantiation yields, for its whole extent: an overload
    /// set's stage, a suspending lowering.
    Held,
    /// An in-place driver's slot, over the span the public substitution
    /// entry holds: a substitution begun while it is empty enters the
    /// connected demand into it, and leaves it with its answer.
    InPlace(&'s mut Option<super::connected_demand::ConnectedDemandGuard<'g>>),
}

impl<'g> InstantiationDemand<'_, 'g> {
    /// The same holder, for one more step.
    pub(super) fn reborrow(&mut self) -> InstantiationDemand<'_, 'g> {
        match self {
            Self::Held => InstantiationDemand::Held,
            Self::InPlace(slot) => InstantiationDemand::InPlace(slot),
        }
    }

    /// Before a substitution begins: an in-place driver's slot enters the
    /// demand unless an earlier substitution still holds it.
    fn enter<C: crate::resolver_core::ResolverCapabilities>(
        &mut self,
        dispatch: &'g ProjectSemanticDispatch<'_, C>,
    ) {
        if let Self::InPlace(slot) = self {
            if slot.is_none() {
                **slot = Some(dispatch.enter_connected_demand(false).0);
            }
        }
    }

    /// Once a substitution answers: the slot leaves the demand it entered.
    fn leave(&mut self) {
        if let Self::InPlace(slot) = self {
            **slot = None;
        }
    }

    /// Whether the slot holds the demand a suspended walk records into.
    fn holds(&self) -> bool {
        matches!(self, Self::InPlace(slot) if slot.is_some())
    }
}

/// Where an instantiation driven as a continuation stands after its latest
/// step.
pub(super) enum InstantiationStep {
    /// The instantiated candidate, or the `Opaque(Miss)` a dropped one is.
    Done(SemanticNodeId),
    /// It reads this signature list, composite result or template reduction
    /// next, holding everything it computed.
    Read(SemanticQueryKey),
}

/// An overload set's candidates being instantiated under its explicit type
/// arguments: the settled list in resolution order, the candidate being
/// instantiated, and the survivors so far, in order.
pub(super) struct ExplicitCandidates {
    raw: Vec<SemanticNodeId>,
    next: usize,
    instantiated: Vec<SemanticNodeId>,
    /// The candidate standing at a read.
    current: Option<ExplicitInstantiation>,
}

impl ExplicitCandidates {
    fn over(raw: Vec<SemanticNodeId>) -> Box<Self> {
        Box::new(Self {
            raw,
            next: 0,
            instantiated: Vec::new(),
            current: None,
        })
    }

    /// Release the walk the candidate standing at a read holds.
    pub(super) fn release<C: crate::resolver_core::ResolverCapabilities>(
        &mut self,
        dispatch: &ProjectSemanticDispatch<'_, C>,
    ) {
        if let Some(current) = self.current.as_mut() {
            dispatch.release_substitution_walk(&mut current.walk);
        }
    }
}

impl<'a, C: crate::resolver_core::ResolverCapabilities> ProjectSemanticDispatch<'a, C> {
    /// Whether `node`'s clause binds `supplied` explicit type arguments,
    /// read from the clause alone: the drop authority's read-free half,
    /// which the instantiation itself follows parameter by parameter.
    pub(super) fn explicit_clause_fit(&self, node: SemanticNodeId, supplied: usize) -> ClauseFit {
        let Some(data) = self.graph().node_data(node) else {
            return ClauseFit::Drops;
        };
        match data.as_ref() {
            SemanticNodeData::Signature {
                type_parameters, ..
            } => clause_fit(type_parameters.as_arc(), supplied),
            SemanticNodeData::DeferredCallable(callable) => clause_fit(
                callable
                    .parts(&crate::semantic_query::ResolveCallConsumer::witness())
                    .type_parameters,
                supplied,
            ),
            SemanticNodeData::Object(_) => ClauseFit::ReadsBuckets,
            _ => ClauseFit::Drops,
        }
    }

    /// Whether a call's raw candidate takes the call's explicit type
    /// arguments: the drop the call's visible candidates took in their
    /// overload set, decided from the clause where it can be.
    pub(super) fn explicit_candidate_fits(
        &self,
        candidate: SemanticNodeId,
        type_args: &[SemanticNodeId],
    ) -> bool {
        match self.explicit_clause_fit(candidate, type_args.len()) {
            ClauseFit::Binds => true,
            ClauseFit::Drops => false,
            ClauseFit::ReadsBuckets => self
                .instantiate_call_candidate(candidate, type_args)
                .is_some(),
        }
    }

    /// Apply a call site's explicit type arguments to ONE candidate
    /// signature. `None` when the candidate cannot accept the argument
    /// list, which drops it from that call site's ordered bucket.
    ///
    /// The single authority for that drop: the ordered bucket a call site
    /// sees and the raw bucket its executor pairs each candidate back to
    /// must be filtered identically, or a surviving candidate is paired
    /// with a dropped candidate's binders.
    pub(super) fn instantiate_call_candidate(
        &self,
        signature: SemanticNodeId,
        type_args: &[SemanticNodeId],
    ) -> Option<SemanticNodeId> {
        let instantiated = self.apply_typeof_instantiation_args(signature, type_args);
        let dropped = matches!(
            self.graph().node_data(instantiated).as_deref(),
            Some(SemanticNodeData::Opaque(_))
        );
        (!dropped).then_some(instantiated)
    }

    /// Apply instantiation-expression type arguments
    /// (`typeof C.make<string>`) to a resolved generic signature node:
    /// each of the function's OWN type parameters substitutes positionally
    /// to the matching argument (an unfilled trailing parameter takes its
    /// declared default when present), and the instantiated signature is
    /// re-interned WITHOUT the consumed type parameters — an instantiation
    /// expression yields a non-generic signature. A signature-bearing
    /// object instantiates every signature it carries
    /// ([`Self::instantiate_object_signatures`]); a sealed deferred callable
    /// substitutes its own parameters and images. Returns the deferred
    /// `Opaque(Miss)` shell when the node is not a generic function or the
    /// arguments cannot satisfy the parameter list (an honest miss, never
    /// a partially-substituted signature).
    ///
    /// The machine driven in place: each substitution it names is made
    /// through the public entry.
    pub(super) fn apply_typeof_instantiation_args(
        &self,
        node: SemanticNodeId,
        args: &[SemanticNodeId],
    ) -> SemanticNodeId {
        let mut instantiation = ExplicitInstantiation::new();
        let mut need = self.begin_instantiation_need(&mut instantiation, node, args);
        loop {
            match need {
                InstantiationNeed::Done(result) => return result,
                InstantiationNeed::Substitute(node, parameter, arg) => {
                    let value = self.substitute_semantic_type_param(node, parameter, arg);
                    need = self.step_instantiation_need(&mut instantiation, args, value);
                }
                InstantiationNeed::Read(key) => {
                    let read = self.execute_via_cold_build_helper(key);
                    need = self.resume_object_signatures(&mut instantiation, args, read);
                }
            }
        }
    }

    /// Begin instantiating `node` with `args` as a continuation: its result,
    /// or the read its signature list or substitution walk stands at.
    /// `demand` keeps the connected demand the walk's recordings belong to
    /// open while the walk stands at a read.
    pub(super) fn begin_explicit_instantiation<'g>(
        &'g self,
        instantiation: &mut ExplicitInstantiation,
        node: SemanticNodeId,
        args: &[SemanticNodeId],
        demand: InstantiationDemand<'_, 'g>,
    ) -> InstantiationStep {
        let need = self.begin_instantiation_need(instantiation, node, args);
        self.drive_explicit_instantiation(instantiation, args, need, demand)
    }

    /// Resume the retained signature list or substitution walk with its
    /// original query-value delivery. Only a substitution narrows to a node.
    pub(super) fn advance_explicit_instantiation<'g>(
        &'g self,
        instantiation: &mut ExplicitInstantiation,
        args: &[SemanticNodeId],
        deliver: &mut dyn FnMut() -> CacheRead<QueryResult<SemanticQueryValue>>,
        mut demand: InstantiationDemand<'_, 'g>,
    ) -> InstantiationStep {
        if matches!(instantiation.stage, InstantiationStage::Object(_)) {
            let need = self.resume_object_signatures(instantiation, args, deliver());
            return self.drive_explicit_instantiation(instantiation, args, need, demand);
        }
        let mut narrowed = || super::narrow_value_cache_read(deliver());
        match self.resume_substitution(&mut instantiation.walk, &mut narrowed) {
            SubstitutionStep::Read(key) => self.substitution_read(key, &demand),
            SubstitutionStep::Done(value) => {
                demand.leave();
                let need = self.step_instantiation_need(instantiation, args, value);
                self.drive_explicit_instantiation(instantiation, args, need, demand)
            }
        }
    }

    fn drive_explicit_instantiation<'g>(
        &'g self,
        instantiation: &mut ExplicitInstantiation,
        args: &[SemanticNodeId],
        mut need: InstantiationNeed,
        mut demand: InstantiationDemand<'_, 'g>,
    ) -> InstantiationStep {
        loop {
            match need {
                InstantiationNeed::Done(result) => return InstantiationStep::Done(result),
                InstantiationNeed::Read(key) => return InstantiationStep::Read(key),
                InstantiationNeed::Substitute(node, parameter, arg) => {
                    demand.enter(self);
                    match self.begin_substitution(&mut instantiation.walk, node, parameter, arg) {
                        SubstitutionStep::Read(key) => {
                            return self.substitution_read(key, &demand);
                        }
                        SubstitutionStep::Done(value) => {
                            demand.leave();
                            need = self.step_instantiation_need(instantiation, args, value);
                        }
                    }
                }
            }
        }
    }

    /// The read a substitution walk stands at, its levels' recordings open
    /// on the connected demand its holder keeps.
    fn substitution_read(
        &self,
        key: SemanticQueryKey,
        demand: &InstantiationDemand<'_, '_>,
    ) -> InstantiationStep {
        verter_debug_assert!(
            demand.holds() || self.connected_demand.is_active(),
            "a substitution walk suspends only inside a connected demand its holder keeps"
        );
        InstantiationStep::Read(key)
    }

    /// The first step of instantiating `node` with `args`: an honest miss
    /// for a node with no clause that admits them, a signature-bearing
    /// object's first signature-list read, or the clause's first substitution.
    fn begin_instantiation_need(
        &self,
        instantiation: &mut ExplicitInstantiation,
        node: SemanticNodeId,
        args: &[SemanticNodeId],
    ) -> InstantiationNeed {
        let Some(data) = self.graph().node_data(node) else {
            return InstantiationNeed::Done(self.opaque(QueryError::Miss));
        };
        // The sealed index-composed carrier instantiates in place: its
        // parameters and binders substitute, the occurrence and the
        // deferral are preserved.
        if let SemanticNodeData::DeferredCallable(callable) = &*data {
            let callable = callable.clone();
            drop(data);
            let parts = callable.parts(&crate::semantic_query::ResolveCallConsumer::witness());
            let clause = Arc::clone(parts.type_parameters);
            if !clause_admits(&clause, args.len()) {
                return InstantiationNeed::Done(self.opaque(QueryError::Miss));
            }
            let params = parts.params.iter().map(FunctionParam::copy).collect();
            instantiation.stage = InstantiationStage::Callable(Box::new(CallableClause {
                callable,
                clause,
                params,
                index: 0,
                at: CallableStep::Argument,
            }));
            return self.next_instantiation_need(instantiation, args);
        }
        if let SemanticNodeData::Object(surface) = &*data {
            let surface = surface.clone();
            drop(data);
            let (list, key) =
                SignatureListRead::begin(node, crate::semantic_query::SignatureKind::Call, false);
            instantiation.stage = InstantiationStage::Object(Box::new(ObjectSignatures {
                node,
                surface,
                calls: None,
                list,
            }));
            return InstantiationNeed::Read(key);
        }
        let SemanticNodeData::Signature {
            type_parameters, ..
        } = &*data
        else {
            return InstantiationNeed::Done(self.opaque(QueryError::Miss));
        };
        let clause = Arc::clone(type_parameters.as_arc());
        drop(data);
        if !clause_admits(&clause, args.len()) {
            return InstantiationNeed::Done(self.opaque(QueryError::Miss));
        }
        instantiation.stage = InstantiationStage::Signature(Box::new(SignatureClause {
            clause,
            index: 0,
            result: node,
            at: ClauseStep::Argument,
        }));
        self.next_instantiation_need(instantiation, args)
    }

    /// Consume one bucket's delivery through the shared list owner. An
    /// unsettled call bucket stops before a construct read. Composite result
    /// reads retain the list's current-epoch refusal and rebind policy.
    fn resume_object_signatures(
        &self,
        instantiation: &mut ExplicitInstantiation,
        args: &[SemanticNodeId],
        read: CacheRead<QueryResult<SemanticQueryValue>>,
    ) -> InstantiationNeed {
        let InstantiationStage::Object(object) = &mut instantiation.stage else {
            unreachable!("only an object's signature list awaits a bucket delivery")
        };
        match self
            .continuation_stage()
            .advance_signature_list(&mut object.list, read)
        {
            ListProgress::Read(key) => InstantiationNeed::Read(key),
            ListProgress::Nodes(nodes) if object.calls.is_none() => {
                object.calls = Some(nodes);
                let (list, key) = SignatureListRead::begin(
                    object.node,
                    crate::semantic_query::SignatureKind::Construct,
                    false,
                );
                object.list = list;
                InstantiationNeed::Read(key)
            }
            progress => {
                let InstantiationStage::Object(object) =
                    std::mem::replace(&mut instantiation.stage, InstantiationStage::Finished)
                else {
                    unreachable!("the object retains both buckets until completion")
                };
                InstantiationNeed::Done(match progress {
                    ListProgress::Nodes(constructs) => self.instantiate_object_signatures(
                        object.node,
                        &object.surface,
                        object
                            .calls
                            .as_deref()
                            .expect("the call bucket settled first"),
                        &constructs,
                        args,
                    ),
                    ListProgress::Unsettled => self.opaque(QueryError::Miss),
                    ListProgress::Read(_) => unreachable!("a pending list stays retained"),
                })
            }
        }
    }

    /// Take `value`, the substitution the instantiation named last, and
    /// name its next step.
    fn step_instantiation_need(
        &self,
        instantiation: &mut ExplicitInstantiation,
        args: &[SemanticNodeId],
        value: SemanticNodeId,
    ) -> InstantiationNeed {
        match &mut instantiation.stage {
            InstantiationStage::Signature(clause) => match &mut clause.at {
                ClauseStep::Default {
                    value: default,
                    bound_at,
                } => {
                    *default = value;
                    *bound_at += 1;
                }
                ClauseStep::Binders { at, .. } => {
                    clause.result = value;
                    *at += 1;
                }
                ClauseStep::Argument => {
                    unreachable!("an argument is chosen without a substitution")
                }
            },
            InstantiationStage::Callable(clause) => match &mut clause.at {
                CallableStep::Default {
                    value: default,
                    bound_at,
                } => {
                    *default = value;
                    *bound_at += 1;
                }
                CallableStep::Param { at, .. } => {
                    clause.params[*at].ty = value;
                    *at += 1;
                }
                CallableStep::Images {
                    images,
                    image,
                    bound_at,
                } => {
                    images[*image] = value;
                    *bound_at += 1;
                }
                CallableStep::Argument => {
                    unreachable!("an argument is chosen without a substitution")
                }
            },
            InstantiationStage::Object(_) | InstantiationStage::Finished => {
                unreachable!("only a clause names a substitution")
            }
        }
        self.next_instantiation_need(instantiation, args)
    }

    /// The instantiation's next substitution, or its result, which ends it.
    fn next_instantiation_need(
        &self,
        instantiation: &mut ExplicitInstantiation,
        args: &[SemanticNodeId],
    ) -> InstantiationNeed {
        let ExplicitInstantiation { bound, stage, .. } = instantiation;
        let need = match stage {
            InstantiationStage::Signature(clause) => self.next_signature_need(clause, bound, args),
            InstantiationStage::Callable(clause) => self.next_callable_need(clause, bound, args),
            InstantiationStage::Object(_) | InstantiationStage::Finished => {
                unreachable!("only a clause names a substitution")
            }
        };
        match need {
            InstantiationNeed::Substitute(..) => {
                self.graph().record_binder_substitution();
            }
            InstantiationNeed::Done(_) => {
                *stage = InstantiationStage::Finished;
                bound.clear();
            }
            InstantiationNeed::Read(_) => unreachable!("a clause names no signature-list read"),
        }
        need
    }

    /// A generic signature's clause, parameter by parameter: the supplied
    /// argument, or the declared default instantiated under the bindings
    /// fixed before it — a default may REFERENCE an earlier parameter
    /// (`<T, U = T>`), which by then is substituted away — then that
    /// argument substituted for each occurrence of the parameter's binder.
    /// The walked root IS the signature whose own parameters are being
    /// instantiated, so its own clause entry for the name does not shadow;
    /// a NESTED function re-declaring the name does. A parameter neither
    /// supplied nor defaulted is an honest miss. Once every parameter is
    /// bound the clause is stripped.
    fn next_signature_need(
        &self,
        clause: &mut SignatureClause,
        bound: &mut Vec<(SemanticNodeId, SemanticNodeId)>,
        args: &[SemanticNodeId],
    ) -> InstantiationNeed {
        loop {
            let arg = match &clause.at {
                ClauseStep::Argument => {
                    let Some(decl) = clause.clause.get(clause.index) else {
                        return InstantiationNeed::Done(
                            self.strip_instantiated_clause(clause.result),
                        );
                    };
                    match (args.get(clause.index), decl.default) {
                        (Some(&arg), _) => arg,
                        (None, Some(value)) => {
                            clause.at = ClauseStep::Default { value, bound_at: 0 };
                            continue;
                        }
                        // Unsatisfied non-defaulted parameter — honest miss.
                        (None, None) => {
                            return InstantiationNeed::Done(self.opaque(QueryError::Miss))
                        }
                    }
                }
                ClauseStep::Default { value, bound_at } => match bound.get(*bound_at) {
                    Some(&(parameter, argument)) => {
                        return InstantiationNeed::Substitute(*value, parameter, argument)
                    }
                    None => *value,
                },
                ClauseStep::Binders { arg, binders, at } => match binders.get(*at) {
                    Some(&binder) => {
                        return InstantiationNeed::Substitute(clause.result, binder, *arg)
                    }
                    None => {
                        clause.index += 1;
                        clause.at = ClauseStep::Argument;
                        continue;
                    }
                },
            };
            let decl = &clause.clause[clause.index];
            bound.push((decl.param, arg));
            let binders = self.collect_type_param_nodes_by_name(
                clause.result,
                decl.name.as_ref(),
                /* root_is_own_signature */ true,
                crate::semantic_query::ClauseSpelling::Bound,
            );
            clause.at = ClauseStep::Binders {
                arg,
                binders,
                at: 0,
            };
        }
    }

    /// The instantiated signature, non-generic: its consumed clause
    /// stripped. Instantiation PRESERVES the occurrence; a declared carrier
    /// retargets the instantiated return, and the substituted predicate
    /// target rides through — an instantiated `x is T` narrows to the
    /// argument.
    fn strip_instantiated_clause(&self, result: SemanticNodeId) -> SemanticNodeId {
        match self.graph().node_data(result).as_deref() {
            Some(SemanticNodeData::Signature {
                kind,
                params,
                return_type,
                occurrence,
                return_carrier,
                signature_span,
                return_type_span,
                predicate,
                is_abstract,
                ..
            }) => self.graph().intern_node(SemanticNodeData::Signature {
                kind: *kind,
                params: params.share(),
                return_type: *return_type,
                type_parameters: crate::semantic_query::PayloadArray::from(
                    Vec::<TypeParamDecl>::new().into_boxed_slice(),
                ),
                occurrence: occurrence.clone(),
                return_carrier: match return_carrier {
                    crate::semantic_query::SignatureReturnCarrier::Declared(_) => {
                        crate::semantic_query::SignatureReturnCarrier::Declared(*return_type)
                    }
                    crate::semantic_query::SignatureReturnCarrier::Function(source) => {
                        crate::semantic_query::SignatureReturnCarrier::Function(source.clone())
                    }
                    crate::semantic_query::SignatureReturnCarrier::DeferredFlow(_)
                    | crate::semantic_query::SignatureReturnCarrier::EvaluatedFlow(_) => {
                        return_carrier.clone()
                    }
                },
                signature_span: *signature_span,
                return_type_span: *return_type_span,
                predicate: *predicate,
                is_abstract: *is_abstract,
            }),
            _ => result,
        }
    }

    /// A sealed deferred callable's clause: per parameter, its argument (a
    /// default instantiated as a signature's is) substituted into every
    /// parameter of the callable, then every binding into each source
    /// image. The consumed clause is stripped, and the occurrence and the
    /// deferred return carrier are preserved — an instantiated candidate is
    /// the same occurrence with the same equation edge.
    fn next_callable_need(
        &self,
        clause: &mut CallableClause,
        bound: &mut Vec<(SemanticNodeId, SemanticNodeId)>,
        args: &[SemanticNodeId],
    ) -> InstantiationNeed {
        loop {
            match &mut clause.at {
                CallableStep::Argument => {
                    let Some(decl) = clause.clause.get(clause.index) else {
                        let mut images = Vec::new();
                        clause
                            .callable
                            .for_each_source_value(|image| images.push(image));
                        clause.at = CallableStep::Images {
                            images,
                            image: 0,
                            bound_at: 0,
                        };
                        continue;
                    };
                    clause.at = match (args.get(clause.index), decl.default) {
                        (Some(&arg), _) => CallableStep::Param { arg, at: 0 },
                        (None, Some(value)) => CallableStep::Default { value, bound_at: 0 },
                        (None, None) => {
                            return InstantiationNeed::Done(self.opaque(QueryError::Miss))
                        }
                    };
                }
                CallableStep::Default { value, bound_at } => match bound.get(*bound_at) {
                    Some(&(parameter, argument)) => {
                        return InstantiationNeed::Substitute(*value, parameter, argument)
                    }
                    None => {
                        let arg = *value;
                        clause.at = CallableStep::Param { arg, at: 0 };
                    }
                },
                CallableStep::Param { arg, at } => {
                    let binder = clause.clause[clause.index].param;
                    if let Some(param) = clause.params.get(*at) {
                        return InstantiationNeed::Substitute(param.ty, binder, *arg);
                    }
                    bound.push((binder, *arg));
                    clause.index += 1;
                    clause.at = CallableStep::Argument;
                }
                CallableStep::Images {
                    images,
                    image,
                    bound_at,
                } => {
                    if *image == images.len() {
                        return InstantiationNeed::Done(
                            self.finish_instantiated_callable(clause, bound),
                        );
                    }
                    match bound.get(*bound_at) {
                        Some(&(binder, argument)) => {
                            return InstantiationNeed::Substitute(images[*image], binder, argument)
                        }
                        None => {
                            *image += 1;
                            *bound_at = 0;
                        }
                    }
                }
            }
        }
    }

    /// The instantiated deferred callable: its substituted parameters and
    /// images, its clause stripped and recorded as the result substitution
    /// its deferred return applies.
    fn finish_instantiated_callable(
        &self,
        clause: &mut CallableClause,
        bound: &[(SemanticNodeId, SemanticNodeId)],
    ) -> SemanticNodeId {
        let CallableStep::Images { images, .. } =
            std::mem::replace(&mut clause.at, CallableStep::Argument)
        else {
            unreachable!("a callable finishes after its images")
        };
        let mut images = images.into_iter();
        let source = clause.callable.map_source_values(|_| {
            images
                .next()
                .expect("one substituted value per source image")
        });
        verter_debug_assert!(
            images.next().is_none(),
            "every substituted source image is mapped back"
        );
        let source = source.with_instantiated_clause(
            clause
                .clause
                .iter()
                .zip(bound.iter())
                .map(|(decl, (_, argument))| (Arc::<str>::from(&decl.name), *argument)),
        );
        let params = std::mem::take(&mut clause.params);
        self.graph()
            .intern_node(SemanticNodeData::DeferredCallable(source.with_substituted(
                Arc::from(params.into_boxed_slice()),
                Arc::from(Vec::<TypeParamDecl>::new().into_boxed_slice()),
            )))
    }

    /// Resume an overload set's resolution with the read it waited on: its
    /// callee's signature list, the result a composite candidate applies,
    /// or the template reduction the candidate it instantiates stands at.
    /// Without explicit type arguments the settled list is the set; with
    /// them each candidate is instantiated in turn, the ones that cannot
    /// take the arguments dropping.
    #[inline(never)]
    pub(super) fn resume_overload_set(
        &self,
        build: OverloadSetBuild,
        deliver: &mut dyn FnMut() -> CacheRead<QueryResult<SemanticQueryValue>>,
    ) -> FamilyStep {
        let OverloadSetBuild {
            callee,
            kind,
            type_args,
            stage,
        } = build;
        match stage {
            OverloadSetStage::List(mut list) => {
                match self
                    .continuation_stage()
                    .advance_signature_list(&mut list, deliver())
                {
                    ListProgress::Read(key) => FamilyStep::Need(
                        key,
                        Box::new(FamilyBuild::OverloadSet(OverloadSetBuild {
                            callee,
                            kind,
                            type_args,
                            stage: OverloadSetStage::List(list),
                        })),
                    ),
                    ListProgress::Unsettled => FamilyStep::Done(Box::new(unsettled_overload_set())),
                    ListProgress::Nodes(nodes) if type_args.is_empty() || nodes.is_empty() => {
                        FamilyStep::Done(Box::new(
                            self.overload_set_output(callee, kind, nodes, &type_args),
                        ))
                    }
                    ListProgress::Nodes(nodes) => self.advance_explicit_candidates(
                        callee,
                        kind,
                        type_args,
                        ExplicitCandidates::over(nodes),
                        None,
                    ),
                }
            }
            OverloadSetStage::Candidates(candidates) => {
                self.advance_explicit_candidates(callee, kind, type_args, candidates, Some(deliver))
            }
        }
    }

    /// Instantiate an overload set's candidates in turn from where they
    /// stand — the candidate at a read resumed with `deliver` — until one
    /// stands at a read, which is the set's next, or every candidate is
    /// done: the set over the survivors, or the honest miss when none
    /// survives. The stage enters the connected demand its set's holder
    /// keeps, never retaining it across the read.
    fn advance_explicit_candidates(
        &self,
        callee: SemanticNodeId,
        kind: crate::semantic_query::SignatureKind,
        type_args: Arc<[SemanticNodeId]>,
        mut candidates: Box<ExplicitCandidates>,
        deliver: Option<&mut dyn FnMut() -> CacheRead<QueryResult<SemanticQueryValue>>>,
    ) -> FamilyStep {
        let (demand, _) = self.enter_connected_demand(false);
        let mut step = if let Some(deliver) = deliver {
            let current = candidates
                .current
                .as_mut()
                .expect("a set is delivered a read when a candidate stands at it");
            Some(self.advance_explicit_instantiation(
                current,
                &type_args,
                deliver,
                InstantiationDemand::Held,
            ))
        } else {
            verter_debug_assert!(
                candidates.current.is_none(),
                "a candidate standing at a read resumes with its delivery"
            );
            self.begin_explicit_candidate(&mut candidates, &type_args)
        };
        let built = loop {
            match step {
                None => {
                    let instantiated = std::mem::take(&mut candidates.instantiated);
                    break FamilyStep::Done(Box::new(self.overload_set_output(
                        callee,
                        kind,
                        instantiated,
                        &type_args,
                    )));
                }
                Some(InstantiationStep::Read(key)) => {
                    break FamilyStep::Need(
                        key,
                        Box::new(FamilyBuild::OverloadSet(OverloadSetBuild {
                            callee,
                            kind,
                            type_args,
                            stage: OverloadSetStage::Candidates(candidates),
                        })),
                    );
                }
                Some(InstantiationStep::Done(node)) => {
                    candidates.current = None;
                    candidates.next += 1;
                    // A candidate that cannot take the arguments is the
                    // honest miss, and drops from the set.
                    if !matches!(
                        self.graph().node_data(node).as_deref(),
                        Some(SemanticNodeData::Opaque(_))
                    ) {
                        candidates.instantiated.push(node);
                    }
                    step = self.begin_explicit_candidate(&mut candidates, &type_args);
                }
            }
        };
        let root = demand.is_root();
        drop(demand);
        verter_debug_assert!(
            !(root && matches!(built, FamilyStep::Need(..))),
            "an overload set suspends only inside a connected demand its holder keeps"
        );
        built
    }

    /// Begin instantiating the set's next candidate, or `None` once every
    /// candidate is done.
    fn begin_explicit_candidate(
        &self,
        candidates: &mut ExplicitCandidates,
        type_args: &[SemanticNodeId],
    ) -> Option<InstantiationStep> {
        let candidate = *candidates.raw.get(candidates.next)?;
        self.graph().record_explicit_candidate();
        let current = candidates.current.insert(ExplicitInstantiation::new());
        Some(self.begin_explicit_instantiation(
            current,
            candidate,
            type_args,
            InstantiationDemand::Held,
        ))
    }

    /// Whether `node`'s clause binds `supplied` explicit type arguments
    /// read from the clause alone: `Some(true)` when it binds them,
    /// `Some(false)` when it drops, `None` when its signature buckets
    /// decide.
    #[cfg(any(test, feature = "test-support"))]
    pub fn explicit_clause_fit_for_tests(
        &self,
        node: SemanticNodeId,
        supplied: usize,
    ) -> Option<bool> {
        match self.explicit_clause_fit(node, supplied) {
            ClauseFit::Binds => Some(true),
            ClauseFit::Drops => Some(false),
            ClauseFit::ReadsBuckets => None,
        }
    }

    /// `signature` instantiated with `type_args` as a call candidate, or
    /// `None` when it drops.
    #[cfg(any(test, feature = "test-support"))]
    pub fn instantiate_call_candidate_for_tests(
        &self,
        signature: SemanticNodeId,
        type_args: &[SemanticNodeId],
    ) -> Option<SemanticNodeId> {
        self.instantiate_call_candidate(signature, type_args)
    }
}
