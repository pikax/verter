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
    ResolveDecl(crate::semantic_query_memo::ObservedGraphSelfRoot),
    CycleGate(Box<super::cycle_gate::CycleGateBuild>),
    MacroPayload(MacroPayloadBuild),
    ReduceIntersection(ReduceIntersectionBuild),
    OverloadSet(OverloadSetBuild),
    ApparentType(ApparentTypeBuild),
    ClassInstanceSurface(ClassInstanceSurfaceBuild),
    ClassStaticSurface(ClassStaticSurfaceBuild),
}

impl FamilyBuild {
    /// Whether the queries this build reads are counted as dispatch intents
    /// where a driver reads them in place: an apparent type's reads are
    /// consumer entries, each counted once whichever driver makes it, as a
    /// frame's need is counted at its entry.
    fn counts_need_intent(&self) -> bool {
        matches!(self, Self::ApparentType(_))
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
            | Self::TemplateUnion(..) => false,
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

/// A global declaration lookup a stage names: the first declaration of the
/// global type `name` that `demand`'s project reads — its library's, else
/// its program's contributors'. The lookup reads the library and the
/// contributing files: a production poll's frame waits on any of them that
/// is not ready, retaining the build, and looks again.
pub(super) struct GlobalRead {
    pub(super) demand: Arc<str>,
    pub(super) name: &'static str,
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
    /// The build looks a global declaration up next, holding everything it
    /// computed.
    Global(GlobalRead, Box<FamilyBuild>),
    /// The build reads prepared declarations and lowers declaration
    /// positions of the files it loaded next, holding everything it
    /// computed.
    Declarations(Box<FamilyBuild>),
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
        | SemanticQueryKey::ClassifyMaterializationCycleGate(_) => true,
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
            SemanticQueryKey::ResolveDecl(key) => Some(self.begin_resolve_decl(key)),
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

    /// The next stage of `build`, given the read of the query it named.
    #[inline(never)]
    pub(super) fn resume_family_build(
        &self,
        build: FamilyBuild,
        read: CacheRead<QueryResult<SemanticQueryValue>>,
    ) -> FamilyStep {
        self.graph().record_continuation_resumed();
        match build {
            FamilyBuild::TemplateUnion(mut output, fallback) => {
                let read = super::narrow_value_cache_read(read);
                output.result = QueryResult::Value(match read.value {
                    QueryResult::Value(node) => node,
                    _ => self.graph().intern_node(*fallback),
                });
                FamilyStep::Done(output)
            }
            FamilyBuild::ResolveDecl(root) => {
                let read = super::narrow_value_cache_read(read);
                let completeness = read.fact_status.completeness();
                let mut roots = vec![root];
                if let QueryResult::Value(node) = &read.value {
                    roots.extend(self.observed_self_roots_from_nodes([*node]));
                }
                let mut output: QueryBuildOutput = (read.value, read.dep_signature).into();
                output.cache_suppress = read.cache_suppress;
                output.merge_completeness(completeness);
                FamilyStep::Done(Box::new(output.with_observed_self_roots(roots)))
            }
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
        self.graph().record_continuation_resumed();
        match build {
            FamilyBuild::CycleGate(build) => self
                .continuation_stage()
                .resume_cycle_gate_owner(*build, serve),
            FamilyBuild::MacroPayload(build) => self
                .continuation_stage()
                .resume_macro_payload_owner(build, serve),
            FamilyBuild::TemplateUnion(..) => unreachable!("a template union is a query read"),
            FamilyBuild::ResolveDecl(_) => unreachable!("a declaration target is a query read"),
            FamilyBuild::ReduceIntersection(_) => {
                unreachable!("an intersection reduction reads no file")
            }
            FamilyBuild::OverloadSet(_) => unreachable!("an overload set reads no file"),
            FamilyBuild::ApparentType(_) => {
                unreachable!("an apparent type reads its files through its global lookup")
            }
            FamilyBuild::ClassInstanceSurface(build) => self
                .continuation_stage()
                .resume_class_instance_source(build, serve),
            FamilyBuild::ClassStaticSurface(build) => self
                .continuation_stage()
                .resume_class_static_source(build, serve),
        }
    }

    /// The next stage of `build`, given the declaration the global lookup
    /// it named found.
    #[inline(never)]
    pub(super) fn resume_family_global(
        &self,
        build: FamilyBuild,
        found: Option<verter_session_query::type_solver::host::ResolvedRootIdentity>,
    ) -> FamilyStep {
        self.graph().record_continuation_resumed();
        match build {
            FamilyBuild::ApparentType(build) => self
                .continuation_stage()
                .resume_apparent_global(build, found),
            FamilyBuild::MacroPayload(_)
            | FamilyBuild::ReduceIntersection(_)
            | FamilyBuild::OverloadSet(_)
            | FamilyBuild::ClassInstanceSurface(_)
            | FamilyBuild::ClassStaticSurface(_)
            | FamilyBuild::CycleGate(_)
            | FamilyBuild::TemplateUnion(..)
            | FamilyBuild::ResolveDecl(_) => {
                unreachable!("only an apparent type looks a global declaration up")
            }
        }
    }

    /// The answer to the declaration read `build` stands at.
    pub(super) fn family_declaration_answer(
        &self,
        build: &FamilyBuild,
    ) -> super::continuation_stages::ClassStaticAnswer {
        let FamilyBuild::ClassStaticSurface(build) = build else {
            unreachable!("only a class's static surface reads declarations")
        };
        self.read_class_static_declarations(&build.declaration_read())
    }

    /// The next stage of `build`, given the answer to the declaration read
    /// it stands at.
    #[inline(never)]
    pub(super) fn resume_family_declarations(
        &self,
        build: FamilyBuild,
        answer: super::continuation_stages::ClassStaticAnswer,
    ) -> FamilyStep {
        self.graph().record_continuation_resumed();
        match build {
            FamilyBuild::ClassStaticSurface(build) => self
                .continuation_stage()
                .resume_class_static_declarations(build, answer),
            FamilyBuild::MacroPayload(_)
            | FamilyBuild::ReduceIntersection(_)
            | FamilyBuild::OverloadSet(_)
            | FamilyBuild::ApparentType(_)
            | FamilyBuild::ClassInstanceSurface(_)
            | FamilyBuild::CycleGate(_)
            | FamilyBuild::TemplateUnion(..)
            | FamilyBuild::ResolveDecl(_) => {
                unreachable!("only a class's static surface reads declarations in place")
            }
        }
    }

    /// Read `key`, the query `build` names next, in place. A read that
    /// drives on the continuation runtime counts its intent at its root's
    /// entry; any other read a counting build makes is counted here.
    pub(super) fn read_family_need_in_place(
        &self,
        key: SemanticQueryKey,
        build: &FamilyBuild,
    ) -> CacheRead<QueryResult<SemanticQueryValue>> {
        if build.awaits_retired_read(&self.continuation_stage()) {
            return retired_read();
        }
        if build.counts_need_intent() && !self.drives_on_the_runtime(&key) {
            self.record_dispatch_intent_counters(&key);
        }
        self.execute_via_cold_build_helper(key)
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

    fn complete_family_in_place(&self, mut step: FamilyStep) -> FamilyStep {
        loop {
            match step {
                done @ (FamilyStep::Done(_) | FamilyStep::Value(_)) => return done,
                FamilyStep::Need(key, build) => {
                    let read = self.read_family_need_in_place(key, &build);
                    step = self.resume_family_build(*build, read);
                }
                FamilyStep::Load(canonical, build) => {
                    let serve = self.ctx.ensure_indexed_ready_serve(&canonical);
                    step = self.resume_family_source(*build, serve);
                }
                FamilyStep::Global(read, build) => {
                    let found = self.first_global_declaration(read.demand.as_ref(), read.name);
                    step = self.resume_family_global(*build, found);
                }
                FamilyStep::Declarations(build) => {
                    let answer = self.family_declaration_answer(&build);
                    step = self.resume_family_declarations(*build, answer);
                }
            }
        }
    }
}
