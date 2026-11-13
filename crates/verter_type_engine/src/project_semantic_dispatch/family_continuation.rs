//! Family builds as owned sequential continuations.
//!
//! A family whose build reads other semantic queries is written as a
//! continuation: its first stage either answers or names the one query it
//! reads next, retaining everything it has computed so far, and each later
//! stage resumes from that retained state with the read's answer. One
//! continuation is driven two ways:
//!
//! - the synchronous producer — the cold build every synchronous entry
//!   runs — drives it in place ([`ProjectSemanticDispatch::drive_family_in_place`]),
//!   reading each named query where the build stands, exactly as the build's
//!   nested reads were always made;
//! - a frame on the continuation runtime drives it by needing each named
//!   query (`query_frames`), so a read another task is producing suspends
//!   the frame, with the build's state owned by the frame, instead of
//!   waiting that producer out on the polling thread. The frame resumes with
//!   the read's delivery; no stage that completed runs again.
//!
//! A stage reads no other query itself and waits on nothing: the reads are
//! the transitions between stages. That is enforced, not documented: a stage
//! is a method of [`ContinuationStage`], which holds the dispatcher in a
//! field private to this module and offers a stage only what cannot wait —
//! graph construction, the construction meter, canonical-composite
//! normalization, the project generation a fence pins and the self-roots its
//! nodes observed. Only the two drivers here hold the dispatcher. Each stage
//! function stays out of line, so the native frame of whichever driver steps
//! it does not grow with the families it may step.

use std::sync::Arc;

use verter_session_query::inputs::indexed::IndexedInputServe;

use crate::project_semantic_dispatch::walk::QueryBuildOutput;
use crate::semantic_query::{
    CacheRead, QueryResult, SemanticNodeId, SemanticQueryKey, SemanticQueryValue,
};

use super::continuation_stages::{
    ApparentTypeBuild, ClassInstanceSurfaceBuild, ClassStaticSurfaceBuild, MacroPayloadBuild,
    OverloadSetBuild, ReduceIntersectionBuild,
};
use super::port_continuations::{PortAnswer, PortRead, ResolveDeclBuild};
use super::ProjectSemanticDispatch;

/// The capability a family continuation's stage runs with: everything a
/// stage computes with, and nothing that reads another query, reads or
/// loads a file, joins another task's producer or drives an execution.
/// A stage names each such dependency as a [`FamilyStep`], which its driver
/// answers — needing it on the continuation runtime, or reading it in place
/// on the synchronous producer.
pub struct ContinuationStage<'s, 'a, C: crate::resolver_core::ResolverCapabilities> {
    dispatch: &'s ProjectSemanticDispatch<'a, C>,
}

impl<'s, 'a: 's, C: crate::resolver_core::ResolverCapabilities> ContinuationStage<'s, 'a, C> {
    /// The semantic graph the stage constructs its nodes in.
    pub fn graph_store(&self) -> &'s crate::semantic_query_memo::SemanticGraphStore {
        self.dispatch.graph().store()
    }

    /// The semantic graph, with this execution's construction accounting.
    pub(super) fn graph(&self) -> crate::semantic_query_memo::GraphIn<'s, C::Observe> {
        self.dispatch.graph()
    }

    /// An opaque node carrying `error`.
    pub(super) fn opaque(&self, error: crate::semantic_query::QueryError) -> SemanticNodeId {
        self.dispatch.opaque(error)
    }

    /// The meter constructions are charged to.
    pub(super) fn construction_meter(&self) -> super::canonical_algebra::ConstructionMeter<'s> {
        self.dispatch.construction_meter()
    }

    /// The canonical composite's output over `members`, with its origin
    /// edges and evidence.
    pub(super) fn build_normalize_composite(
        &self,
        members: &Arc<[SemanticNodeId]>,
        composite: super::canonical_algebra::CanonicalComposite,
    ) -> QueryBuildOutput {
        self.dispatch.build_normalize_composite(members, composite)
    }

    /// The fence over the current project generation.
    pub(super) fn project_generation_signature(&self) -> crate::semantic_query::DepSignature {
        self.dispatch.project_generation_signature()
    }

    /// Whether `canonical`'s project checks under `strictBindCallApply`.
    pub(super) fn strict_bind_call_apply(&self, canonical: &str) -> bool {
        self.dispatch
            .ctx
            .semantic_compiler_options_for(canonical)
            .strict_bind_call_apply
    }

    /// The instantiation of the global interface `name` that `declaration`
    /// declares, as an apparent surface reads it.
    pub(super) fn apparent_surface_key(
        &self,
        declaration: &verter_session_query::type_solver::host::ResolvedRootIdentity,
        name: &'static str,
    ) -> SemanticQueryKey {
        let slot = self.dispatch.type_slot_for(
            declaration.canonical_id.clone(),
            declaration.owner,
            Arc::from(name),
        );
        SemanticQueryKey::Instantiate(crate::semantic_query::InstantiateKey::new(
            slot,
            Arc::from(Vec::new().into_boxed_slice()),
            self.dispatch.instantiate_context_for(
                declaration.canonical_id.as_ref(),
                crate::semantic_query::ProjectionReductionContext::published(
                    crate::semantic_query::ProjectionMode::Expanded,
                ),
            ),
        ))
    }

    /// The instantiation a class's instance surface reads: the TYPE-space
    /// half of `decl_slot` under `Shallow`. The dual-space selector is the
    /// surface's side, so the slot's symbol space is canonicalized to
    /// `Type` for the `Instantiate` base.
    pub(super) fn class_instance_key(
        &self,
        decl_slot: &crate::semantic_query::ResolvedDeclSlotIdentity,
        type_args: &Arc<[SemanticNodeId]>,
    ) -> SemanticQueryKey {
        SemanticQueryKey::Instantiate(crate::semantic_query::InstantiateKey::new(
            decl_slot.with_symbol_space(crate::semantic_query::SemanticSymbolSpace::Type),
            Arc::clone(type_args),
            self.dispatch.instantiate_context_for(
                &decl_slot.defining_canonical,
                crate::semantic_query::ProjectionReductionContext::published(
                    crate::semantic_query::ProjectionMode::Shallow,
                ),
            ),
        ))
    }

    /// The skeleton read for one declaration in the cycle gate's FIFO walk.
    pub(super) fn cycle_gate_hop_key(
        &self,
        current: &crate::semantic_query::DeclIdentity,
    ) -> SemanticQueryKey {
        SemanticQueryKey::Instantiate(crate::semantic_query::InstantiateKey::new(
            self.dispatch.type_slot_for(
                current.canonical_id.clone(),
                current.owner,
                current.decl_name.clone(),
            ),
            Arc::from([]),
            self.dispatch.instantiate_context_for(
                &current.canonical_id,
                crate::semantic_query::ProjectionReductionContext::structural_transit_with_mode(
                    crate::semantic_query::ProjectionMode::Skeleton,
                ),
            ),
        ))
    }

    /// The project generation a fence pins.
    pub(super) fn project_generation(&self) -> u64 {
        self.dispatch.snapshot.current_project_generation()
    }

    /// The dependency signature of `canonical_id` at the content version
    /// `hash`, under the current project generation.
    pub(super) fn dep_signature_for(
        &self,
        canonical_id: impl Into<verter_type_expr::OwnedStr>,
        hash: verter_session_query::analysis::types::Hash16,
    ) -> crate::semantic_query::DepSignature {
        self.dispatch.dep_signature_for(canonical_id, hash)
    }

    /// The file versions `nodes` were lowered from.
    pub(super) fn observed_self_roots_from_nodes(
        &self,
        nodes: impl IntoIterator<Item = SemanticNodeId>,
    ) -> Vec<crate::semantic_query_memo::ObservedGraphSelfRoot> {
        self.dispatch.observed_self_roots_from_nodes(nodes)
    }

    /// The candidates of a dispatched signature set, pinned in the order
    /// call resolution tries them, or with `resolution` off in declaration
    /// order.
    pub(super) fn pin_signature_list(
        &self,
        kind: crate::semantic_query::SignatureKind,
        value: &crate::signature_kernel::SignatureSetValue,
        resolution: bool,
    ) -> Result<
        super::signature_discovery::PinnedSignatureList,
        super::signature_discovery::SignatureSetReadFailure,
    > {
        self.dispatch.pin_signature_list(kind, value, resolution)
    }

    /// Whether a signature list read earlier may still be read: it names no
    /// retired kernel epoch, the gate every warm read of it applies.
    pub(super) fn serves_signature_list(
        &self,
        value: &crate::signature_kernel::SignatureSetValue,
    ) -> bool {
        self.graph_store().serves_stored_value(&QueryResult::Value(
            SemanticQueryValue::SignatureSet(value.clone()),
        ))
    }

    /// The result read the node form of `list`'s next composite candidate
    /// waits on, or `None` once every candidate is a node.
    pub(super) fn prepare_next_composite(
        &self,
        list: &mut super::signature_discovery::PinnedSignatureList,
    ) -> Result<
        Option<crate::signature_kernel::ReadSignatureResultKey>,
        super::signature_discovery::SignatureSetReadFailure,
    > {
        self.dispatch.prepare_next_composite(list)
    }

    /// Settle `list`'s prepared composite candidate with its applied result.
    pub(super) fn settle_composite_signature(
        &self,
        list: &mut super::signature_discovery::PinnedSignatureList,
        applied: crate::signature_kernel::AppliedResultId,
    ) -> Result<(), super::signature_discovery::SignatureSetReadFailure> {
        self.dispatch.settle_composite_signature(list, applied)
    }

    /// The overload set over `callee`'s settled candidates, without explicit
    /// type arguments.
    pub(super) fn overload_set_output(
        &self,
        callee: SemanticNodeId,
        kind: crate::semantic_query::SignatureKind,
        sigs: Vec<SemanticNodeId>,
    ) -> QueryBuildOutput {
        self.dispatch.overload_set_output(
            callee,
            kind,
            sigs,
            &Arc::from(Vec::new().into_boxed_slice()),
        )
    }
}

/// A family build between two of its stages: what the stages before the
/// read it waits on computed.
pub(super) enum FamilyBuild {
    TemplateUnion(
        Box<QueryBuildOutput>,
        Box<crate::semantic_query::SemanticNodeData>,
    ),
    ResolveDecl(Box<ResolveDeclBuild>),
    CycleGate(Box<super::cycle_gate::CycleGateBuild>),
    MacroPayload(MacroPayloadBuild),
    ReduceIntersection(ReduceIntersectionBuild),
    OverloadSet(OverloadSetBuild),
    ApparentType(ApparentTypeBuild),
    ClassInstanceSurface(ClassInstanceSurfaceBuild),
    ClassStaticSurface(ClassStaticSurfaceBuild),
    /// A path projection's walk, standing at the read a hop named.
    ProjectPath(Box<super::walk::ProjectPathBuild>),
}

impl FamilyBuild {
    /// Whether the queries this build reads are counted as dispatch intents
    /// where a driver reads them in place: an apparent type's reads are
    /// consumer entries, each counted once whichever driver makes it, as a
    /// frame's need is counted at its entry. A path walk counts what its
    /// in-place reads always counted: a carrier resolution's reads but its
    /// `typeof` value's, and a callable's apparent-type probe
    /// ([`super::walk::WalkState::counts_awaited_intent`]).
    fn counts_need_intent(&self, key: &SemanticQueryKey) -> bool {
        match self {
            Self::ApparentType(_) => true,
            Self::ProjectPath(build) => build.walk.counts_awaited_intent(key),
            _ => false,
        }
    }

    /// Whether the read this build waits on was named from kernel handles a
    /// replacement retired while it waited — a composite candidate's result,
    /// prepared from a signature list the replacement retired. No read of it
    /// answers the build, whichever driver makes it: the build is resumed
    /// with [`retired_read`] instead, never with the read, whose rails the
    /// staleness alone would taint, and it reads its list again.
    pub(super) fn awaits_retired_read<C: crate::resolver_core::ResolverCapabilities>(
        &self,
        stage: &ContinuationStage<'_, '_, C>,
    ) -> bool {
        match self {
            Self::OverloadSet(build) => build.awaits_retired_read(stage),
            Self::ApparentType(build) => build.awaits_retired_read(stage),
            Self::MacroPayload(_)
            | Self::ReduceIntersection(_)
            | Self::ClassInstanceSurface(_)
            | Self::ClassStaticSurface(_)
            | Self::CycleGate(_)
            | Self::ResolveDecl(_)
            | Self::TemplateUnion(..)
            | Self::ProjectPath(_) => false,
        }
    }

    /// Record `key`, a read this build names, in the test dispatch trace
    /// as the read a synchronous build makes is recorded: a path walk's
    /// hop reads are recorded wherever its driver reads them.
    #[cfg(any(test, feature = "test-support"))]
    pub(super) fn trace_need(&self, key: &SemanticQueryKey) {
        if matches!(self, Self::ProjectPath(_)) {
            super::raise::DISPATCH_TRACE.with(|t| {
                t.borrow_mut()
                    .push(super::raise::query_key_discriminant(key))
            });
        }
    }

    #[cfg(not(any(test, feature = "test-support")))]
    pub(super) fn trace_need(&self, _key: &SemanticQueryKey) {}

    /// Release what a build stopped between its stages retained on its
    /// computation's recording: a path walk's hop standing in a deferred
    /// evaluation abandons the recordings that evaluation opened above the
    /// build's own, and its open prefix recording puts back what the build
    /// charged before its first prefix, so the frame abandoning that
    /// recording leaves those charges its consumer's.
    pub(super) fn release_retained_accounting<C: crate::resolver_core::ResolverCapabilities>(
        &mut self,
        dispatch: &ProjectSemanticDispatch<'_, C>,
    ) {
        if let Self::ProjectPath(build) = self {
            build.walk.release(dispatch);
        }
    }

    /// The resolver port this build stands at ([`FamilyStep::Port`]): what
    /// it reads next, which its driver answers in place over the files the
    /// build loaded.
    pub(super) fn port_read(&self) -> PortRead<'_> {
        match self {
            Self::ClassStaticSurface(build) => PortRead::ClassStatic(build.declaration_read()),
            Self::ApparentType(build) => build.global_port(),
            Self::ResolveDecl(build) => build.port_read(),
            Self::ProjectPath(build) => build.walk.port_read(),
            Self::MacroPayload(_)
            | Self::ReduceIntersection(_)
            | Self::OverloadSet(_)
            | Self::ClassInstanceSurface(_)
            | Self::CycleGate(_)
            | Self::TemplateUnion(..) => unreachable!("this family reads no resolver port"),
        }
    }
}

/// The answer a build waiting on a read named from retired kernel handles
/// resumes with ([`FamilyBuild::awaits_retired_read`]): a miss with no rails
/// and no receipt, which its stage takes for the retirement it is.
pub(super) fn retired_read() -> CacheRead<QueryResult<SemanticQueryValue>> {
    CacheRead::from_value_and_signature(
        QueryResult::Error(crate::semantic_query::QueryError::Miss),
        super::empty_signature(),
    )
}

/// Where a family build stands after its latest stage.
pub(super) enum FamilyStep {
    /// The build's output.
    Done(Box<QueryBuildOutput>),
    /// A completed family whose result has its own value domain.
    Value(Box<QueryBuildOutput<SemanticQueryValue>>),
    /// The build reads this query next, holding everything it computed.
    Need(SemanticQueryKey, Box<FamilyBuild>),
    /// The build reads this file's indexed input next, holding everything
    /// it computed.
    Load(Arc<str>, Box<FamilyBuild>),
    /// The build makes this file IndexedReady next without reading it — a
    /// contributor pending ingestion, indexed ahead of the population read
    /// that would otherwise index it in place — holding everything it
    /// computed. The file's content version is not observed: the build
    /// depends on what it reads afterwards, not on the file. The build
    /// resumes as from a `Load` that served nothing.
    Index(Arc<str>, Box<FamilyBuild>),
    /// The build reads a resolver port next — a declaration, route or
    /// global lookup over files it loaded ([`FamilyBuild::port_read`]) —
    /// holding everything it computed. Its driver answers the port in
    /// place over the deliveries the build retained
    /// ([`ProjectSemanticDispatch::answer_port_in_place`]); a port whose
    /// file is still loading on a production poll's drive waits on that
    /// load first.
    Port(Box<FamilyBuild>),
}

// A suspended family build is owned state its frame carries to whichever
// thread resumes it: the build and the step naming its next dependency own
// what they retained, borrowing nothing of the dispatcher that stepped them.
// Each is advanced by value, one stage at a time, so no two drivers can step
// the same build.
static_assertions::assert_impl_all!(FamilyBuild: Send);
static_assertions::assert_impl_all!(FamilyStep: Send);
static_assertions::assert_not_impl_any!(FamilyBuild: Clone, Copy);
static_assertions::assert_not_impl_any!(FamilyStep: Clone, Copy);

/// Whether `key`'s family builds as a continuation
/// ([`ProjectSemanticDispatch::begin_family_build`]).
pub(super) fn builds_as_continuation(key: &SemanticQueryKey) -> bool {
    match key {
        SemanticQueryKey::TemplateLiteralReduce { .. }
        | SemanticQueryKey::ResolveMacroPayload { .. }
        | SemanticQueryKey::ResolveDecl(_)
        | SemanticQueryKey::ReduceIntersection { .. }
        | SemanticQueryKey::ApparentType { .. }
        | SemanticQueryKey::ResolveClassSurface { .. }
        | SemanticQueryKey::ClassifyMaterializationCycleGate(_)
        | SemanticQueryKey::ProjectPath { .. } => true,
        // Explicit type arguments instantiate each candidate through the
        // substitution engine, which still reads in place.
        SemanticQueryKey::ResolveOverloadSet { type_args, .. } => type_args.is_empty(),
        _ => false,
    }
}

impl<'a, C: crate::resolver_core::ResolverCapabilities> ProjectSemanticDispatch<'a, C> {
    /// The stage capability over this dispatcher.
    pub(super) fn continuation_stage(&self) -> ContinuationStage<'_, 'a, C> {
        ContinuationStage { dispatch: self }
    }

    /// The first stage of `key`'s family build, for a family that builds as
    /// a continuation; `None` for any other family.
    #[inline(never)]
    pub(super) fn begin_family_build(&self, key: &SemanticQueryKey) -> Option<FamilyStep> {
        match key {
            SemanticQueryKey::TemplateLiteralReduce { pattern, args, .. } => {
                Some(self.begin_template_literal_reduce(pattern, args))
            }
            SemanticQueryKey::ProjectPath {
                base,
                path,
                context,
            } => Some(self.begin_project_path(*base, path, *context)),
            SemanticQueryKey::ResolveDecl(key) => {
                Some(self.continuation_stage().begin_resolve_decl(key))
            }
            SemanticQueryKey::ClassifyMaterializationCycleGate(key) => {
                Some(self.continuation_stage().begin_cycle_gate(key))
            }
            SemanticQueryKey::ResolveMacroPayload {
                owner,
                macro_index,
                macro_kind,
                type_args,
                context,
            } => Some(self.continuation_stage().begin_resolve_macro_payload(
                owner,
                *macro_index,
                *macro_kind,
                type_args,
                context.mode,
            )),
            SemanticQueryKey::ReduceIntersection {
                input,
                purpose,
                context,
            } => Some(self.continuation_stage().begin_reduce_intersection(
                input.clone(),
                *purpose,
                context.clone(),
            )),
            SemanticQueryKey::ResolveOverloadSet {
                callee,
                kind,
                type_args,
                ..
            } if type_args.is_empty() => Some(
                self.continuation_stage()
                    .begin_resolve_overload_set(*callee, *kind),
            ),
            SemanticQueryKey::ApparentType { base, context } => Some(
                self.continuation_stage()
                    .begin_apparent_type(*base, context),
            ),
            SemanticQueryKey::ResolveClassSurface {
                decl_slot,
                type_args,
                side: crate::semantic_query::ClassSurfaceSide::Instance,
                ..
            } => Some(
                self.continuation_stage()
                    .begin_class_instance_surface(decl_slot, type_args),
            ),
            SemanticQueryKey::ResolveClassSurface {
                decl_slot,
                type_args,
                side: crate::semantic_query::ClassSurfaceSide::Static,
                context,
            } => Some(self.continuation_stage().begin_class_static_surface(
                decl_slot,
                type_args,
                context.mode,
            )),
            _ => None,
        }
    }

    /// The next stage of `build`, given the read of the query it named:
    /// `deliver` makes or delivers that read. A build consumes it as its
    /// stage begins, except a path walk whose hop stands inside a deferred
    /// evaluation, which has it delivered inside the evaluation's stage.
    #[inline(never)]
    pub(super) fn resume_family_build(
        &self,
        build: FamilyBuild,
        deliver: &mut dyn FnMut() -> CacheRead<QueryResult<SemanticQueryValue>>,
    ) -> FamilyStep {
        // A path walk counts its own work, hop by hop, where each hop is
        // computed (`projection_hops_walked`).
        let build = match build {
            FamilyBuild::ProjectPath(build) => {
                return self.resume_project_path(*build, deliver);
            }
            build => build,
        };
        self.graph().record_continuation_resumed();
        let read = deliver();
        match build {
            FamilyBuild::ProjectPath(_) => unreachable!("a path walk resumed above"),
            FamilyBuild::TemplateUnion(mut output, fallback) => {
                let read = super::narrow_value_cache_read(read);
                output.result = QueryResult::Value(match read.value {
                    QueryResult::Value(node) => node,
                    _ => self.graph().intern_node(*fallback),
                });
                FamilyStep::Done(output)
            }
            FamilyBuild::ResolveDecl(build) => self
                .continuation_stage()
                .finish_resolve_decl(*build, super::narrow_value_cache_read(read)),
            FamilyBuild::CycleGate(build) => self
                .continuation_stage()
                .resume_cycle_gate_hop(*build, super::narrow_value_cache_read(read)),
            FamilyBuild::MacroPayload(build) => FamilyStep::Done(Box::new(
                self.continuation_stage()
                    .finish_resolve_macro_payload(build, super::narrow_value_cache_read(read)),
            )),
            FamilyBuild::ReduceIntersection(build) => self
                .continuation_stage()
                .resume_reduce_intersection(build, super::narrow_value_cache_read(read)),
            FamilyBuild::OverloadSet(build) => self
                .continuation_stage()
                .resume_resolve_overload_set(build, read),
            FamilyBuild::ApparentType(build) => {
                self.continuation_stage().resume_apparent_type(build, read)
            }
            FamilyBuild::ClassInstanceSurface(build) => FamilyStep::Done(Box::new(
                self.continuation_stage()
                    .finish_class_instance_surface(build, super::narrow_value_cache_read(read)),
            )),
            FamilyBuild::ClassStaticSurface(build) => self
                .continuation_stage()
                .resume_class_static_base(build, super::narrow_value_cache_read(read)),
        }
    }

    /// The next stage of `build`, given the indexed input of the file it
    /// named (`None` when the live view serves none).
    #[inline(never)]
    pub(super) fn resume_family_source(
        &self,
        build: FamilyBuild,
        serve: Option<IndexedInputServe>,
    ) -> FamilyStep {
        // A path walk's file reads are its wrapper lookup's; its stages are
        // not counted.
        let build = match build {
            FamilyBuild::ProjectPath(build) => {
                return self
                    .resume_project_path_lookup(*build, super::walk::LookupAnswer::Source(serve));
            }
            build => build,
        };
        self.graph().record_continuation_resumed();
        match build {
            FamilyBuild::CycleGate(build) => self
                .continuation_stage()
                .resume_cycle_gate_owner(*build, serve),
            FamilyBuild::MacroPayload(build) => self
                .continuation_stage()
                .resume_macro_payload_owner(build, serve),
            FamilyBuild::TemplateUnion(..) => unreachable!("a template union is a query read"),
            FamilyBuild::ProjectPath(_) => unreachable!("a path walk resumed above"),
            FamilyBuild::ResolveDecl(build) => self
                .continuation_stage()
                .resume_resolve_decl_source(*build, serve),
            FamilyBuild::ReduceIntersection(_) => {
                unreachable!("an intersection reduction reads no file")
            }
            FamilyBuild::OverloadSet(_) => unreachable!("an overload set reads no file"),
            FamilyBuild::ApparentType(build) => self
                .continuation_stage()
                .resume_apparent_global_source(build, serve),
            FamilyBuild::ClassInstanceSurface(build) => self
                .continuation_stage()
                .resume_class_instance_source(build, serve),
            FamilyBuild::ClassStaticSurface(build) => self
                .continuation_stage()
                .resume_class_static_source(build, serve),
        }
    }

    /// The next stage of `build`, given the answer to the resolver port it
    /// stands at.
    #[inline(never)]
    pub(super) fn resume_family_port(&self, build: FamilyBuild, answer: PortAnswer) -> FamilyStep {
        let build = match build {
            FamilyBuild::ProjectPath(build) => {
                return self
                    .resume_project_path_lookup(*build, super::walk::LookupAnswer::Port(answer));
            }
            build => build,
        };
        self.graph().record_continuation_resumed();
        match (build, answer) {
            (FamilyBuild::ClassStaticSurface(build), PortAnswer::ClassStatic(answer)) => self
                .continuation_stage()
                .resume_class_static_declarations(build, answer),
            (FamilyBuild::ApparentType(build), answer) => self
                .continuation_stage()
                .resume_apparent_global_port(build, answer),
            (FamilyBuild::ResolveDecl(build), answer) => self
                .continuation_stage()
                .resume_resolve_decl_port(*build, answer),
            (FamilyBuild::ClassStaticSurface(_), _) => {
                unreachable!("a class's static surface reads only its declarations")
            }
            (
                FamilyBuild::MacroPayload(_)
                | FamilyBuild::ReduceIntersection(_)
                | FamilyBuild::OverloadSet(_)
                | FamilyBuild::ClassInstanceSurface(_)
                | FamilyBuild::CycleGate(_)
                | FamilyBuild::TemplateUnion(..),
                _,
            ) => unreachable!("this family reads no resolver port"),
            (FamilyBuild::ProjectPath(_), _) => unreachable!("a path walk resumed above"),
        }
    }

    /// Answer the resolver port `read` in place, where the driver stands:
    /// over the deliveries the build retained for the files it loaded, so
    /// the answer loads nothing a `Load` transition did not name. A port is
    /// the one place a family's stage reaches the resolver.
    #[inline(never)]
    pub(super) fn answer_port_in_place(&self, read: &PortRead<'_>) -> PortAnswer {
        match read {
            PortRead::GlobalLib { demand, name } => {
                PortAnswer::GlobalLib(self.lib_global_identity(demand, name))
            }
            PortRead::GlobalPending => {
                PortAnswer::GlobalPending(self.ctx.pending_global_contributors())
            }
            PortRead::GlobalContributors { demand, name } => {
                PortAnswer::GlobalContributors(self.global_contributors_in(
                    demand,
                    name,
                    verter_session_query::facts::SymbolSpace::Type,
                ))
            }
            PortRead::ClassStatic(read) => {
                PortAnswer::ClassStatic(self.read_class_static_declarations(read))
            }
            PortRead::Dependency {
                importer,
                specifier,
            } => PortAnswer::Dependency(
                self.ctx
                    .resolve_type_dependency_canonical_witnessed(importer, specifier)
                    .map(Arc::from),
            ),
            PortRead::BareName {
                canonical,
                owner,
                name,
            } => PortAnswer::BareName(self.resolve_decl_route_in_place(canonical, *owner, name)),
        }
    }

    /// Resume `build` with `key`, the query it names next, read in place
    /// where the build consumes it. A read that drives on the continuation
    /// runtime counts its intent at its root's entry; any other read a
    /// counting build makes is counted here.
    pub(super) fn resume_family_need_in_place(
        &self,
        key: SemanticQueryKey,
        build: FamilyBuild,
    ) -> FamilyStep {
        let retired = build.awaits_retired_read(&self.continuation_stage());
        let counts =
            !retired && build.counts_need_intent(&key) && !self.drives_on_the_runtime(&key);
        if !retired {
            build.trace_need(&key);
        }
        let mut key = Some(key);
        let mut read = || {
            let key = key.take().expect("a read is made once");
            if retired {
                return retired_read();
            }
            if counts {
                self.record_dispatch_intent_counters(&key);
            }
            self.execute_via_cold_build_helper(key)
        };
        self.resume_family_build(build, &mut read)
    }

    /// Build `key`'s family through the synchronous producer's driver,
    /// outside the memo, for the reads it makes: the driver a family read
    /// inside an open obligation is built with. `false` for a family that
    /// does not build as a continuation.
    #[cfg(any(test, feature = "test-support"))]
    pub fn build_family_in_place_for_tests(&self, key: &SemanticQueryKey) -> bool {
        let Some(step) = self.begin_family_build(key) else {
            return false;
        };
        let _ = self.drive_family_value_in_place(step);
        true
    }

    /// The result read an overload set over `callee`'s signatures of `kind`
    /// makes for its first composite candidate, its signature list read in
    /// place first: `None` when the list settles without one.
    #[cfg(any(test, feature = "test-support"))]
    pub fn first_composite_result_read_for_tests(
        &self,
        callee: SemanticNodeId,
        kind: crate::semantic_query::SignatureKind,
    ) -> Option<SemanticQueryKey> {
        use super::continuation_stages::{ListProgress, SignatureListRead};
        let (mut list, read) = SignatureListRead::begin(callee, kind, true);
        let read = self.execute_via_cold_build_helper(read);
        match self
            .continuation_stage()
            .advance_signature_list(&mut list, read)
        {
            ListProgress::Read(key) => Some(key),
            ListProgress::Nodes(_) | ListProgress::Unsettled => None,
        }
    }

    /// The instantiation an apparent type widening to the global interface
    /// `name` reads in `demand`'s project: `None` when the project declares
    /// no such interface.
    #[cfg(any(test, feature = "test-support"))]
    pub fn apparent_interface_read_for_tests(
        &self,
        demand: &str,
        name: &'static str,
    ) -> Option<SemanticQueryKey> {
        self.first_global_declaration(demand, name)
            .map(|declaration| {
                self.continuation_stage()
                    .apparent_surface_key(&declaration, name)
            })
    }

    /// Drive a family build to its output, reading each query it names in
    /// place: the synchronous producer's driver.
    #[inline(never)]
    pub(super) fn drive_family_in_place(&self, step: FamilyStep) -> QueryBuildOutput {
        match self.complete_family_in_place(step) {
            FamilyStep::Done(output) => *output,
            _ => unreachable!("a node family completes in the node domain"),
        }
    }

    pub(super) fn drive_family_value_in_place(
        &self,
        step: FamilyStep,
    ) -> QueryBuildOutput<SemanticQueryValue> {
        match self.complete_family_in_place(step) {
            FamilyStep::Done(output) => (*output).into(),
            FamilyStep::Value(output) => *output,
            _ => unreachable!("the family completed"),
        }
    }

    /// The queries `key`'s family build names as its needs, in order, its
    /// build driven in place outside the memo: what a test holds to
    /// suspend the build at each of its reads, under one connected demand
    /// as a cold build drives it. Empty for a family that does not build as
    /// a continuation.
    #[cfg(any(test, feature = "test-support"))]
    pub fn family_needs_for_tests(&self, key: &SemanticQueryKey) -> Vec<SemanticQueryKey> {
        let (_demand, _) = self.enter_connected_demand(false);
        let mut needs = Vec::new();
        if let Some(step) = self.begin_family_build(key) {
            let _ = self.complete_family_in_place_naming(step, &mut |key| {
                needs.push(key.clone());
            });
        }
        needs
    }

    fn complete_family_in_place(&self, step: FamilyStep) -> FamilyStep {
        self.complete_family_in_place_naming(step, &mut |_| {})
    }

    fn complete_family_in_place_naming(
        &self,
        mut step: FamilyStep,
        named: &mut dyn FnMut(&SemanticQueryKey),
    ) -> FamilyStep {
        loop {
            match step {
                done @ (FamilyStep::Done(_) | FamilyStep::Value(_)) => return done,
                FamilyStep::Need(key, build) => {
                    named(&key);
                    step = self.resume_family_need_in_place(key, *build);
                }
                FamilyStep::Load(canonical, build) => {
                    let serve = self.ctx.ensure_indexed_ready_serve(&canonical);
                    step = self.resume_family_source(*build, serve);
                }
                FamilyStep::Index(canonical, build) => {
                    self.ctx.materialize_pending_contributor(&canonical);
                    step = self.resume_family_source(*build, None);
                }
                FamilyStep::Port(build) => {
                    let answer = self.answer_port_in_place(&build.port_read());
                    step = self.resume_family_port(*build, answer);
                }
            }
        }
    }
}
