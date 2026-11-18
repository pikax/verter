//! Per-project binding of projection results, and admission of what is published.

use std::sync::Arc;

use parking_lot::Mutex;
use rustc_hash::FxHashMap;
use verter_identity::encoding::{CanonicalEncode, CanonicalEncoder};
use verter_identity::identity::{InputBasisId, RequestGeneration};
use verter_session_query::analysis::types::Hash16;
use verter_span::CanonicalPath;

use crate::semantic_capability::{CertifiedTypeEngineBinding, ServingLease};
use crate::sfc_projection_cache::ProjectionRevisionSet;
use crate::sfc_projection_mapper::MapperProjectHandle;

struct ProjectionInputBasis<'a> {
    config_file: &'a str,
    virtual_config: Hash16,
    settings_identity: &'a [u8; 32],
}

impl CanonicalEncode for ProjectionInputBasis<'_> {
    const DOMAIN_TAG: &'static str = "verter.session.sfc_projection_observations.input_basis.v1";

    fn encode_fields(&self, encoder: &mut CanonicalEncoder) {
        encoder.field_str(1, self.config_file);
        encoder.field_bytes(2, &self.virtual_config);
        encoder.field_bytes(3, self.settings_identity);
    }
}

/// The basis of one project's engine program: its config chain, injected
/// checking paths and mapper settings. Carrier content is tracked separately
/// by admission, so a carrier text edit does not replace the program's basis.
#[must_use]
pub fn projection_input_basis(
    config_file: &str,
    virtual_config: Hash16,
    settings_identity: &[u8; 32],
) -> InputBasisId {
    InputBasisId::from_canonical(&ProjectionInputBasis {
        config_file,
        virtual_config,
        settings_identity,
    })
}

/// A project's results bound to one engine, one config and one input basis.
#[derive(Debug, Clone)]
pub struct CertifiedProjectionBinding {
    pub project: MapperProjectHandle,
    /// The project's tsconfig path.
    pub config_file: Arc<str>,
    /// Caller-certified engine facts, including the membership lease: one per
    /// graph load, advanced on graph reload.
    pub engine: CertifiedTypeEngineBinding,
    /// Advanced on restart, configuration change and changed certification.
    /// A rebind or reopen supplies a generation newer than the previous one.
    pub generation: u64,
}

/// An observation that names the exact snapshot it was made against.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapshotQualifiedObservation<T> {
    pub project: MapperProjectHandle,
    pub generation: u64,
    pub carrier: Arc<str>,
    pub revisions: ProjectionRevisionSet,
    pub value: T,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PublicationRefusal {
    /// The project was rebound or restarted since the observation.
    StaleGeneration,
    /// The carrier's revisions moved since the observation.
    StaleRevisions,
    /// The work that produced it was cancelled.
    Cancelled,
    UnknownProject,
}

/// Decides whether an observation may be published. Its current generation
/// and revisions per project are private. It retains no observation payload
/// and calls no backend or native type engine.
pub struct ProjectionPublicationAdmission {
    projects: Mutex<FxHashMap<MapperProjectHandle, ProjectAdmission>>,
}

struct ProjectAdmission {
    certification: Option<CertifiedTypeEngineBinding>,
    generation: RequestGeneration,
    serving_lease: ServingLease,
    needs_rebind: bool,
    current: FxHashMap<CanonicalPath, ProjectionRevisionSet>,
}

impl ProjectAdmission {
    fn validate_generation(&self, generation: u64) -> Result<(), PublicationRefusal> {
        if self.certification.is_none() {
            return Err(PublicationRefusal::UnknownProject);
        }
        if self.generation != RequestGeneration(generation) || self.needs_rebind {
            return Err(PublicationRefusal::StaleGeneration);
        }
        Ok(())
    }
}

impl ProjectionPublicationAdmission {
    #[must_use]
    pub fn new() -> Self {
        Self {
            projects: Mutex::new(FxHashMap::default()),
        }
    }
    /// Bind one configured project. Invalid project/config labels and bindings
    /// older than the recorded generation or membership lease are ignored.
    /// Repeating the current certification and generation is idempotent.
    /// A restart's reserved generation is consumed without another advance.
    /// Otherwise a changed certification or reopen advances the generation,
    /// even when the caller accidentally reuses it, so old observations stay stale.
    pub fn bind(&self, binding: &CertifiedProjectionBinding) {
        if binding.project != MapperProjectHandle::of_config(&binding.config_file)
            || binding.project != MapperProjectHandle::of_config(binding.engine.project())
        {
            return;
        }
        let lease = binding.engine.serving_lease();
        let mut projects = self.projects.lock();
        if let Some(state) = projects.get_mut(&binding.project) {
            if binding.generation < state.generation.0
                || lease.value() < state.serving_lease.value()
            {
                return;
            }
            if state.certification.as_ref() == Some(&binding.engine)
                && state.generation.0 == binding.generation
                && !state.needs_rebind
            {
                return;
            }
            let restarted = state.needs_rebind && binding.generation == state.generation.0;
            if !restarted {
                let next = state
                    .generation
                    .0
                    .checked_add(1)
                    .expect("project generation exhausted");
                state.generation = RequestGeneration(binding.generation.max(next));
                state.current.clear();
            }
            state.certification = Some(binding.engine.clone());
            state.serving_lease = lease;
            state.needs_rebind = false;
        } else {
            projects.insert(
                binding.project.clone(),
                ProjectAdmission {
                    certification: Some(binding.engine.clone()),
                    generation: RequestGeneration(binding.generation),
                    serving_lease: lease,
                    needs_rebind: false,
                    current: FxHashMap::default(),
                },
            );
        }
    }
    /// Record authoritative source state for an open project. Unknown and
    /// closed projects are inert; observations never create source state.
    pub fn record_current(
        &self,
        project: &MapperProjectHandle,
        carrier: &str,
        revisions: ProjectionRevisionSet,
    ) {
        let carrier = CanonicalPath::new(carrier);
        if let Some(state) = self.projects.lock().get_mut(project) {
            if state.certification.is_some() {
                state.current.insert(carrier, revisions);
            }
        }
    }
    /// Reserve a new generation, retaining current source revisions. Earlier
    /// observations are stale; publication resumes after `bind` supplies the
    /// replacement certification at this generation. Unknown or closed
    /// projects return zero.
    pub fn restart(&self, project: &MapperProjectHandle) -> u64 {
        let mut projects = self.projects.lock();
        let Some(state) = projects.get_mut(project) else {
            return 0;
        };
        if state.certification.is_none() {
            return 0;
        }
        state.generation.0 = state
            .generation
            .0
            .checked_add(1)
            .expect("project generation exhausted");
        state.needs_rebind = true;
        state.generation.0
    }
    /// Return only a live, uncancelled observation of the recorded source
    /// revisions. No payload, successful or refused, is cached here.
    pub fn admit<T>(
        &self,
        observation: SnapshotQualifiedObservation<T>,
        cancelled: bool,
    ) -> Result<T, PublicationRefusal> {
        if cancelled {
            return Err(PublicationRefusal::Cancelled);
        }
        let carrier = CanonicalPath::new(&observation.carrier);
        let projects = self.projects.lock();
        let state = projects
            .get(&observation.project)
            .ok_or(PublicationRefusal::UnknownProject)?;
        state.validate_generation(observation.generation)?;
        if state.current.get(&carrier) != Some(&observation.revisions) {
            return Err(PublicationRefusal::StaleRevisions);
        }
        Ok(observation.value)
    }
    /// Admit project-level results, including configured projects with no
    /// carriers. The certified binding and restart/lease fences are the same as
    /// for carrier observations; no carrier revision or payload is retained.
    pub fn admit_project<T>(
        &self,
        project: &MapperProjectHandle,
        generation: u64,
        value: T,
        cancelled: bool,
    ) -> Result<T, PublicationRefusal> {
        if cancelled {
            return Err(PublicationRefusal::Cancelled);
        }
        let projects = self.projects.lock();
        let state = projects
            .get(project)
            .ok_or(PublicationRefusal::UnknownProject)?;
        state.validate_generation(generation)?;
        Ok(value)
    }
    /// Release the certification and source inventory. Keep only the
    /// generation and lease high-water marks until this admission owner drops,
    /// so reopening the same handle cannot revive an earlier observation.
    pub fn close(&self, project: &MapperProjectHandle) {
        if let Some(state) = self.projects.lock().get_mut(project) {
            state.certification = None;
            state.needs_rebind = false;
            state.current = FxHashMap::default();
        }
    }
}

impl Default for ProjectionPublicationAdmission {
    fn default() -> Self {
        Self::new()
    }
}
