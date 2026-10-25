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
use crate::semantic_query::{CacheRead, QueryResult, SemanticNodeId, SemanticQueryKey};

use super::continuation_stages::{MacroPayloadBuild, ReduceIntersectionBuild};
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
}

/// A family build between two of its stages: what the stages before the
/// read it waits on computed.
pub(super) enum FamilyBuild {
    MacroPayload(MacroPayloadBuild),
    ReduceIntersection(ReduceIntersectionBuild),
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
}

/// Whether `key`'s family builds as a continuation
/// ([`ProjectSemanticDispatch::begin_family_build`]).
pub(super) fn builds_as_continuation(key: &SemanticQueryKey) -> bool {
    matches!(
        key,
        SemanticQueryKey::ResolveMacroPayload { .. } | SemanticQueryKey::ReduceIntersection { .. }
    )
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
            _ => None,
        }
    }

    /// The next stage of `build`, given the read of the query it named.
    #[inline(never)]
    pub(super) fn resume_family_build(
        &self,
        build: FamilyBuild,
        read: CacheRead<QueryResult<SemanticNodeId>>,
    ) -> FamilyStep {
        self.graph().record_continuation_resumed();
        match build {
            FamilyBuild::MacroPayload(build) => FamilyStep::Done(Box::new(
                self.continuation_stage()
                    .finish_resolve_macro_payload(build, read),
            )),
            FamilyBuild::ReduceIntersection(build) => self
                .continuation_stage()
                .resume_reduce_intersection(build, read),
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
                    let read = self.execute_read(key);
                    step = self.resume_family_build(*build, read);
                }
                FamilyStep::Load(canonical, build) => {
                    let serve = self.ctx.ensure_indexed_ready_serve(&canonical);
                    step = self.resume_family_source(*build, serve);
                }
            }
        }
    }
}
