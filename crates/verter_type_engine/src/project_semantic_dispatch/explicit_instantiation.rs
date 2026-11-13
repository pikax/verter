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
//! - inside an overload set's continuation, on the walk the machine holds
//!   ([`ProjectSemanticDispatch::begin_explicit_instantiation`]): a template
//!   reduction the walk reads is the set's read, and the machine resumes
//!   from what it retained with its delivery.
//!
//! [`ProjectSemanticDispatch::explicit_clause_fit`] is the read-free half of
//! the drop authority: whether a candidate's clause binds a given number of
//! explicit arguments, decided from the clause alone, exactly when the
//! machine drops it.

use std::sync::Arc;

use super::continuation_stages::{
    unsettled_overload_set, ListProgress, OverloadSetBuild, OverloadSetStage,
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

/// One explicit instantiation between its steps: the bindings fixed so far
/// in declaration order, where the clause stands, and the substitution walk
/// it stands in, empty between substitutions.
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
    Finished,
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
    Done(SemanticNodeId),
}

/// Where an instantiation driven as a continuation stands after its latest
/// step.
pub(super) enum InstantiationStep {
    /// The instantiated candidate, or the `Opaque(Miss)` a dropped one is.
    Done(SemanticNodeId),
    /// It reads this template reduction next, holding everything it
    /// computed.
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
            }
        }
    }

    /// Begin instantiating `node` with `args` as a continuation, on the
    /// walk `instantiation` holds: its result, or the template read a
    /// substitution stands at. Its caller holds the connected demand the
    /// walk's recordings belong to.
    pub(super) fn begin_explicit_instantiation(
        &self,
        instantiation: &mut ExplicitInstantiation,
        node: SemanticNodeId,
        args: &[SemanticNodeId],
    ) -> InstantiationStep {
        let need = self.begin_instantiation_need(instantiation, node, args);
        self.drive_explicit_instantiation(instantiation, args, need)
    }

    /// Resume `instantiation`, standing at its walk's template read, with
    /// `deliver`, that read: the substitution it stood in goes on from what
    /// it retained, and the instantiation from the step that named it.
    pub(super) fn advance_explicit_instantiation(
        &self,
        instantiation: &mut ExplicitInstantiation,
        args: &[SemanticNodeId],
        deliver: &mut dyn FnMut() -> CacheRead<QueryResult<SemanticNodeId>>,
    ) -> InstantiationStep {
        match self.resume_substitution(&mut instantiation.walk, deliver) {
            SubstitutionStep::Read(key) => InstantiationStep::Read(key),
            SubstitutionStep::Done(value) => {
                let need = self.step_instantiation_need(instantiation, args, value);
                self.drive_explicit_instantiation(instantiation, args, need)
            }
        }
    }

    fn drive_explicit_instantiation(
        &self,
        instantiation: &mut ExplicitInstantiation,
        args: &[SemanticNodeId],
        mut need: InstantiationNeed,
    ) -> InstantiationStep {
        loop {
            match need {
                InstantiationNeed::Done(result) => return InstantiationStep::Done(result),
                InstantiationNeed::Substitute(node, parameter, arg) => {
                    match self.begin_substitution(&mut instantiation.walk, node, parameter, arg) {
                        SubstitutionStep::Read(key) => return InstantiationStep::Read(key),
                        SubstitutionStep::Done(value) => {
                            need = self.step_instantiation_need(instantiation, args, value);
                        }
                    }
                }
            }
        }
    }

    /// The first step of instantiating `node` with `args`: an honest miss
    /// for a node with no clause that admits them, a signature-bearing
    /// object's buckets instantiated in place, or the clause's first
    /// substitution.
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
            return InstantiationNeed::Done(
                self.instantiate_object_signatures(node, &surface, args),
            );
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
            InstantiationStage::Finished => unreachable!("a finished instantiation names nothing"),
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
            InstantiationStage::Finished => unreachable!("a finished instantiation names nothing"),
        };
        match need {
            InstantiationNeed::Substitute(..) => {
                self.graph().record_binder_substitution();
            }
            InstantiationNeed::Done(_) => {
                *stage = InstantiationStage::Finished;
                bound.clear();
            }
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
                let mut narrowed = || super::narrow_value_cache_read(deliver());
                let read: &mut dyn FnMut() -> CacheRead<QueryResult<SemanticNodeId>> =
                    &mut narrowed;
                self.advance_explicit_candidates(callee, kind, type_args, candidates, Some(read))
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
        deliver: Option<&mut dyn FnMut() -> CacheRead<QueryResult<SemanticNodeId>>>,
    ) -> FamilyStep {
        let (demand, _) = self.enter_connected_demand(false);
        let mut step = if let Some(deliver) = deliver {
            // PLANTED: the candidate is begun again from its first stage.
            let candidate = candidates.raw[candidates.next];
            let current = candidates
                .current
                .as_mut()
                .expect("a set is delivered a read when a candidate stands at it");
            self.release_substitution_walk(&mut current.walk);
            *current = ExplicitInstantiation::new();
            self.graph().record_explicit_candidate();
            let mut restarted = self.begin_explicit_instantiation(current, candidate, &type_args);
            let mut delivery = Some(deliver);
            while let InstantiationStep::Read(key) = restarted {
                restarted = match delivery.take() {
                    Some(deliver) => self.advance_explicit_instantiation(current, &type_args, deliver),
                    None => {
                        let mut key = Some(key);
                        self.advance_explicit_instantiation(current, &type_args, &mut || {
                            self.execute_read(key.take().expect("a read is made once"))
                        })
                    }
                };
            }
            Some(restarted)
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
        Some(self.begin_explicit_instantiation(current, candidate, type_args))
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
