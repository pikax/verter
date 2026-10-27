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
    ApparentTypeBuild, MacroPayloadBuild, OverloadSetBuild, ReduceIntersectionBuild,
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
        let key = SemanticQueryKey::Instantiate(crate::semantic_query::InstantiateKey::new(
            slot,
            Arc::from(Vec::new().into_boxed_slice()),
            self.dispatch.instantiate_context_for(
                declaration.canonical_id.as_ref(),
                crate::semantic_query::ProjectionReductionContext::published(
                    crate::semantic_query::ProjectionMode::Expanded,
                ),
            ),
        ));
        self.dispatch.record_dispatch_intent_counters(&key);
        key
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
    MacroPayload(MacroPayloadBuild),
    ReduceIntersection(ReduceIntersectionBuild),
    OverloadSet(OverloadSetBuild),
    ApparentType(ApparentTypeBuild),
}

/// A global declaration lookup a stage names: the first declaration of the
/// global type `name` that `demand`'s project reads — its library's, else
/// its program's contributors'. The lookup reads the library and the
/// contributing files where the driver stands: both drivers make it in
/// place.
pub(super) struct GlobalRead {
    pub(super) demand: Arc<str>,
    pub(super) name: &'static str,
}

/// Where a family build stands after its latest stage.
pub(super) enum FamilyStep {
    /// The build's output.
    Done(Box<QueryBuildOutput>),
    /// The build reads this query next, holding everything it computed.
    Need(SemanticQueryKey, Box<FamilyBuild>),
    /// The build reads this file's indexed input next, holding everything
    /// it computed.
    Load(Arc<str>, Box<FamilyBuild>),
    /// The build looks a global declaration up next, holding everything it
    /// computed.
    Global(GlobalRead, Box<FamilyBuild>),
}

/// Whether `key`'s family builds as a continuation
/// ([`ProjectSemanticDispatch::begin_family_build`]).
pub(super) fn builds_as_continuation(key: &SemanticQueryKey) -> bool {
    match key {
        SemanticQueryKey::ResolveMacroPayload { .. }
        | SemanticQueryKey::ReduceIntersection { .. }
        | SemanticQueryKey::ApparentType { .. } => true,
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
            FamilyBuild::MacroPayload(build) => self
                .continuation_stage()
                .resume_macro_payload_owner(build, serve),
            FamilyBuild::ReduceIntersection(_) => {
                unreachable!("an intersection reduction reads no file")
            }
            FamilyBuild::OverloadSet(_) => unreachable!("an overload set reads no file"),
            FamilyBuild::ApparentType(_) => {
                unreachable!("an apparent type reads its files through its global lookup")
            }
        }
    }

    /// The next stage of `build`, given the declaration the global lookup
    /// it named found.
    #[inline(never)]
    pub(super) fn resume_family_global(&self, build: FamilyBuild, read: GlobalRead) -> FamilyStep {
        self.graph().record_continuation_resumed();
        let found = self.first_global_declaration(read.demand.as_ref(), read.name);
        match build {
            FamilyBuild::ApparentType(build) => self
                .continuation_stage()
                .resume_apparent_global(build, found),
            FamilyBuild::MacroPayload(_)
            | FamilyBuild::ReduceIntersection(_)
            | FamilyBuild::OverloadSet(_) => {
                unreachable!("only an apparent type looks a global declaration up")
            }
        }
    }

    /// Drive a family build to its output, reading each query it names in
    /// place: the synchronous producer's driver.
    #[inline(never)]
    pub(super) fn drive_family_in_place(&self, mut step: FamilyStep) -> QueryBuildOutput {
        loop {
            match step {
                FamilyStep::Done(output) => return *output,
                FamilyStep::Need(key, build) => {
                    let read = self.execute_via_cold_build_helper(key);
                    step = self.resume_family_build(*build, read);
                }
                FamilyStep::Load(canonical, build) => {
                    let serve = self.ctx.ensure_indexed_ready_serve(&canonical);
                    step = self.resume_family_source(*build, serve);
                }
                FamilyStep::Global(read, build) => {
                    step = self.resume_family_global(*build, read);
                }
            }
        }
    }
}
