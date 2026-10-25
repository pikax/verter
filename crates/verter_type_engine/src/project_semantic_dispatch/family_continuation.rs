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
//! the transitions between stages. Each stage function stays out of line,
//! so the native frame of whichever driver steps it does not grow with the
//! families it may step.

use std::sync::Arc;

use verter_session_query::inputs::indexed::IndexedInputServe;

use crate::project_semantic_dispatch::walk::QueryBuildOutput;
use crate::semantic_query::{CacheRead, QueryResult, SemanticNodeId, SemanticQueryKey};

use super::build::{MacroPayloadBuild, ReduceIntersectionBuild};
use super::ProjectSemanticDispatch;

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
            } => Some(self.begin_resolve_macro_payload(
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
            } => Some(self.begin_reduce_intersection(input.clone(), *purpose, context.clone())),
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
            FamilyBuild::MacroPayload(build) => {
                FamilyStep::Done(Box::new(self.finish_resolve_macro_payload(build, read)))
            }
            FamilyBuild::ReduceIntersection(build) => self.resume_reduce_intersection(build, read),
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
            FamilyBuild::MacroPayload(build) => self.resume_macro_payload_owner(build, serve),
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
