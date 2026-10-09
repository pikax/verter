//! Exact project and generated-unit admission for one serving provider instance.
//! Workspace resolution remains with the caller. The hub consumes its retained
//! publication basis and the workspace membership query's typed proof.

use std::collections::HashMap;
use std::sync::Arc;

use verter_workspace::published_state::PublishedRoot;
use verter_workspace::{CanonicalPath, GeneratedUnitAdmission};

use super::{ProviderHub, Serving, Shared};
use crate::traits::{CarrierActivation, CarrierScriptKind, TypeProvider};

/// The inputs which can change whether one project owns a generated unit.
/// The publication is retained by identity: snapshot generation numbers can
/// repeat after a project graph rebuild.
#[derive(Clone)]
pub struct ProjectBasis {
    publication: Arc<PublishedRoot>,
    content_generation: u64,
    project_generation: u64,
}

impl ProjectBasis {
    #[must_use]
    pub fn new(
        publication: Arc<PublishedRoot>,
        content_generation: u64,
        project_generation: u64,
    ) -> Self {
        Self {
            publication,
            content_generation,
            project_generation,
        }
    }
}

impl PartialEq for ProjectBasis {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.publication, &other.publication)
            && self.content_generation == other.content_generation
            && self.project_generation == other.project_generation
    }
}

impl Eq for ProjectBasis {}

/// Resolver-produced facts plus a live reader for the same determining inputs.
/// The reader is checked again by the hub actor immediately before a write.
#[derive(Clone)]
pub struct ProjectBindingInput {
    source: String,
    project: String,
    references: Vec<String>,
    basis: ProjectBasis,
    current_basis: Arc<dyn Fn() -> Option<ProjectBasis> + Send + Sync>,
}

/// Workspace facts for a generated unit. This is input to hub admission, never
/// authorization to write to an engine. Cold desired state retains bytes only;
/// the resolver is called again before replay into a new incarnation.
pub struct GeneratedUnitInput {
    pub binding: ProjectBindingInput,
    pub units: Vec<CanonicalPath>,
    pub proof: GeneratedUnitAdmission,
}

/// `None` identifies an ordinary authored file; generated paths return facts or
/// a typed refusal. The resolver owns workspace observation, not admission.
pub type GeneratedUnitResolver =
    dyn Fn(&str) -> Option<Result<GeneratedUnitInput, AdmissionRefusal>> + Send + Sync;

impl ProjectBindingInput {
    #[must_use]
    pub fn new(
        source: String,
        project: String,
        references: Vec<String>,
        basis: ProjectBasis,
        current_basis: Arc<dyn Fn() -> Option<ProjectBasis> + Send + Sync>,
    ) -> Self {
        Self {
            source,
            project,
            references,
            basis,
            current_basis,
        }
    }
}

/// One carrier registration dropped from the desired state at a replacement
/// install, carrying everything its issuing tier needs to re-publish it through
/// a fresh admission: the registration inputs plus the EXPLICIT parsing mode of
/// its last explicit activation (`None` when it was registered without one).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DroppedAdmittedCarrier {
    pub source_path: String,
    pub companion_path: String,
    pub content: String,
    pub project_file_name: String,
    pub script_kind: Option<CarrierScriptKind>,
}

/// The admitted generated state a replacement install dropped: the carrier
/// registrations and file overlays the desired state will not replay. Emitted
/// through [`super::ProviderNotifier::admitted_state_dropped`] after the
/// replacement is installed, so the tier that minted the admissions can re-arm
/// them against the fresh serving epoch.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DroppedAdmittedState {
    /// The admitted carrier registrations, in companion-path order.
    pub carriers: Vec<DroppedAdmittedCarrier>,
    /// The admitted file-overlay paths (content-less: the re-arm re-derives
    /// them from its own publication authority), in path order.
    pub files: Vec<String>,
}

/// A refusal never carries a provider operation or an applied receipt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdmissionRefusal {
    NoServingProvider,
    StaleBasis,
    StaleProvider,
    WrongProject,
    MissingGeneratedProof,
    IncompleteGeneratedProof,
    GeneratedUnitExcluded,
    ProviderWriteFailed,
    /// A higher-authority live overlay suppressed this mutation before forwarding.
    ShadowedMutation,
    DeadlineElapsed,
    Cancelled,
}

struct WitnessInner {
    input: ProjectBindingInput,
    epoch: verter_identity::identity::ProviderEpoch,
    hub_identity: usize,
    provider_identity: usize,
}

/// Hub-issued exact binding to one project and actual provider incarnation.
#[derive(Clone)]
pub struct ProjectWitness(Arc<WitnessInner>);

impl ProjectWitness {
    #[must_use]
    pub fn project(&self) -> &str {
        &self.0.input.project
    }

    #[must_use]
    pub fn source(&self) -> &str {
        &self.0.input.source
    }

    #[must_use]
    pub fn same_binding(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

/// Hub-issued capability covering one complete, nonempty generated write set.
#[derive(Clone)]
pub struct AdmittedRequest {
    pub(super) witness: ProjectWitness,
    units: Vec<CanonicalPath>,
}

/// A provider-visible generated-unit write. The path or member set must be
/// covered by the request's complete configured-project membership proof.
#[derive(Debug, Clone, Copy)]
pub enum OverlayFileKind {
    Open,
    Load,
    Update,
}

#[derive(Debug, Clone, Copy)]
pub enum OverlayPriority {
    Foreground,
    Normal,
    Background,
}

pub enum OverlayMutation {
    File {
        path: String,
        content: String,
        kind: OverlayFileKind,
        priority: OverlayPriority,
    },
    RegisterCarrier {
        source_path: String,
        companion_path: String,
        content: String,
        project_file_name: String,
    },
    RegisterCarrierMetadata {
        source_path: String,
        companion_path: String,
        content: String,
        project_file_name: String,
    },
    ActivateCarrier {
        source_path: String,
        companion_path: String,
        project_file_name: String,
        script_kind: CarrierScriptKind,
    },
    ActivateCarriers {
        members: Vec<CarrierActivation>,
    },
}

impl OverlayMutation {
    fn into_desired(
        self,
        admission: &AdmittedRequest,
    ) -> Result<(super::DesiredMutation, super::Lane), AdmissionRefusal> {
        let covered = |path: &str| admission.covers(path);
        let same_source = |source: &str| {
            CanonicalPath::new(source) == CanonicalPath::new(admission.witness.source())
        };
        let same_project = |project: &str| {
            CanonicalPath::new(project) == CanonicalPath::new(admission.witness.project())
        };
        let mut lane = super::Lane::Foreground;
        let mutation = match self {
            Self::File {
                path,
                content,
                kind,
                priority,
            } if covered(&path) => {
                lane = match priority {
                    OverlayPriority::Foreground => super::Lane::Foreground,
                    OverlayPriority::Normal => super::Lane::Normal,
                    OverlayPriority::Background => super::Lane::Background,
                };
                match kind {
                    OverlayFileKind::Open => super::DesiredMutation::Open { path, content },
                    OverlayFileKind::Load => super::DesiredMutation::Load { path, content },
                    OverlayFileKind::Update => super::DesiredMutation::Update { path, content },
                }
            }
            Self::RegisterCarrier {
                source_path,
                companion_path,
                content,
                project_file_name,
            } if covered(&companion_path)
                && same_project(&project_file_name)
                && same_source(&source_path) =>
            {
                super::DesiredMutation::RegisterCarrier {
                    source_path,
                    companion_path,
                    content,
                    project_file_name,
                }
            }
            Self::RegisterCarrierMetadata {
                source_path,
                companion_path,
                content,
                project_file_name,
            } if covered(&companion_path)
                && same_project(&project_file_name)
                && same_source(&source_path) =>
            {
                super::DesiredMutation::RegisterCarrierMetadata {
                    source_path,
                    companion_path,
                    content,
                    project_file_name,
                }
            }
            Self::ActivateCarrier {
                source_path,
                companion_path,
                project_file_name,
                script_kind,
            } if covered(&companion_path)
                && same_project(&project_file_name)
                && same_source(&source_path) =>
            {
                super::DesiredMutation::ActivateCarrier {
                    source_path,
                    companion_path,
                    project_file_name,
                    script_kind,
                }
            }
            Self::ActivateCarriers { members }
                if !members.is_empty()
                    && members.iter().all(|m| {
                        covered(&m.companion_path)
                            && same_project(&m.project_file_name)
                            && same_source(&m.source_path)
                    }) =>
            {
                super::DesiredMutation::ActivateCarriers { members }
            }
            _ => return Err(AdmissionRefusal::IncompleteGeneratedProof),
        };
        Ok((mutation, lane))
    }
}

impl AdmittedRequest {
    #[must_use]
    pub fn project_witness(&self) -> &ProjectWitness {
        &self.witness
    }

    #[must_use]
    pub fn covers(&self, path: &str) -> bool {
        self.units.binary_search(&CanonicalPath::new(path)).is_ok()
    }
}

#[derive(Default)]
pub(super) struct AdmissionState {
    bindings: HashMap<String, ProjectWitness>,
    requests: HashMap<(String, String, Vec<CanonicalPath>), AdmittedRequest>,
    managed_requests: HashMap<String, AdmittedRequest>,
    /// Generated-unit proofs a read-only query was admitted on, keyed by the
    /// write set and retained with the membership inputs that decided them, so
    /// a later query under the same publication and project generation reuses
    /// the proof instead of re-running the membership query.
    query_proofs: HashMap<(String, String, Vec<CanonicalPath>), MembershipInputs>,
    /// Read-only admissions of generated units, keyed by the queried path and
    /// reused while their membership inputs hold.
    query_requests: HashMap<String, AdmittedRequest>,
}

/// The inputs that decide project membership: the exact publication (retained,
/// so its identity cannot be reused by a later allocation) and the project
/// generation. The workspace content generation is deliberately absent.
#[derive(Clone)]
struct MembershipInputs {
    publication: Arc<PublishedRoot>,
    project_generation: u64,
}

impl MembershipInputs {
    fn of(basis: &ProjectBasis) -> Self {
        Self {
            publication: Arc::clone(&basis.publication),
            project_generation: basis.project_generation,
        }
    }

    fn decides(&self, basis: &ProjectBasis) -> bool {
        Arc::ptr_eq(&self.publication, &basis.publication)
            && self.project_generation == basis.project_generation
    }
}

fn provider_identity<P: ?Sized>(provider: &Arc<P>) -> usize {
    Arc::as_ptr(provider) as *const () as usize
}

pub(super) fn check_current<P: ?Sized>(
    shared: &Shared<P>,
    admission: &AdmittedRequest,
) -> Result<(), AdmissionRefusal> {
    check_witness_current(shared, &admission.witness)
}

/// Whether the inputs that DECIDED `admission`'s generated-unit membership —
/// the exact publication and the project generation — are still the live
/// ones. `true` under a content-only drift: the proof was derived from this
/// very publication, so the admitted units are still members of the bound
/// project and an engine holding them holds nothing the live basis excludes.
pub(super) fn membership_inputs_current(admission: &AdmittedRequest) -> bool {
    witness_membership_current(&admission.witness)
}

fn witness_membership_current(witness: &ProjectWitness) -> bool {
    let input = &witness.0.input;
    (input.current_basis)().is_some_and(|live| MembershipInputs::of(&input.basis).decides(&live))
}

/// The currency of a READ-ONLY query: the witness still names the serving
/// incarnation of this hub, and the inputs that decided its project and
/// membership are still the live ones. A read writes nothing the engine keeps,
/// so — unlike [`check_witness_for_serving`] — neither the workspace content
/// generation nor the hub's warm binding memo takes part: an unrelated
/// document's edit cannot refuse it. The coordinates the answer decodes
/// against are the query's own capability, bound at its wire position.
fn check_query_witness<P: ?Sized>(
    shared: &Shared<P>,
    witness: &ProjectWitness,
) -> Result<(), AdmissionRefusal> {
    let serving = shared
        .serving()
        .ok_or(AdmissionRefusal::NoServingProvider)?;
    let bound = &witness.0;
    if bound.hub_identity != std::ptr::from_ref(shared) as usize
        || serving.epoch != bound.epoch
        || provider_identity(&serving.provider) != bound.provider_identity
    {
        return Err(AdmissionRefusal::StaleProvider);
    }
    if !witness_membership_current(witness) {
        return Err(AdmissionRefusal::StaleBasis);
    }
    Ok(())
}

/// Whether a complete generated-unit `proof` admits exactly `requested` into
/// `witness`'s project under the publication the witness was bound to.
fn validate_proof(
    witness: &ProjectWitness,
    requested: &[CanonicalPath],
    proof: &GeneratedUnitAdmission,
) -> Result<(), AdmissionRefusal> {
    let GeneratedUnitAdmission::Admitted(admitted) = proof else {
        return Err(AdmissionRefusal::GeneratedUnitExcluded);
    };
    if admitted.tsconfig_path() != &CanonicalPath::new(witness.project()) {
        return Err(AdmissionRefusal::WrongProject);
    }
    if admitted.snapshot_identity()
        != Arc::as_ptr(&witness.0.input.basis.publication.snapshot) as usize
    {
        return Err(AdmissionRefusal::StaleBasis);
    }
    if admitted.units() != requested {
        return Err(AdmissionRefusal::IncompleteGeneratedProof);
    }
    Ok(())
}

/// Validate membership of bytes already replayed into this incarnation. A
/// content edit can evict the warm binding without excluding those units.
/// This checkpoint never authorizes a live write, query or warm admission.
pub(super) fn check_replay_membership_for_serving<P: ?Sized>(
    shared: &Shared<P>,
    serving: &Serving<P>,
    request: &AdmittedRequest,
) -> Result<(), AdmissionRefusal> {
    let witness = &request.witness.0;
    if witness.hub_identity != std::ptr::from_ref(shared) as usize
        || serving.epoch != witness.epoch
        || provider_identity(&serving.provider) != witness.provider_identity
    {
        return Err(AdmissionRefusal::StaleProvider);
    }
    if !membership_inputs_current(request) {
        return Err(AdmissionRefusal::StaleBasis);
    }
    Ok(())
}

fn check_witness_current<P: ?Sized>(
    shared: &Shared<P>,
    witness: &ProjectWitness,
) -> Result<(), AdmissionRefusal> {
    let serving = shared
        .serving()
        .ok_or(AdmissionRefusal::NoServingProvider)?;
    check_witness_for_serving(shared, &serving, witness)
}

pub(super) fn check_witness_for_serving<P: ?Sized>(
    shared: &Shared<P>,
    serving: &Serving<P>,
    witness: &ProjectWitness,
) -> Result<(), AdmissionRefusal> {
    let witness = &witness.0;
    if witness.hub_identity != std::ptr::from_ref(shared) as usize {
        return Err(AdmissionRefusal::StaleProvider);
    }
    if (witness.input.current_basis)().as_ref() != Some(&witness.input.basis) {
        return Err(AdmissionRefusal::StaleBasis);
    }
    if serving.epoch != witness.epoch
        || provider_identity(&serving.provider) != witness.provider_identity
    {
        return Err(AdmissionRefusal::StaleProvider);
    }
    let state = shared.admission.lock().unwrap_or_else(|e| e.into_inner());
    if !state
        .bindings
        .get(&witness.input.source)
        .is_some_and(|current| Arc::ptr_eq(&current.0, witness))
    {
        return Err(AdmissionRefusal::StaleBasis);
    }
    Ok(())
}

fn validate_generated_input(input: &GeneratedUnitInput) -> Result<(), AdmissionRefusal> {
    let binding = &input.binding;
    if binding.source.is_empty() || binding.project.is_empty() {
        return Err(AdmissionRefusal::WrongProject);
    }
    if (binding.current_basis)().as_ref() != Some(&binding.basis) {
        return Err(AdmissionRefusal::StaleBasis);
    }
    let GeneratedUnitAdmission::Admitted(proof) = &input.proof else {
        return Err(AdmissionRefusal::GeneratedUnitExcluded);
    };
    if proof.tsconfig_path() != &CanonicalPath::new(&binding.project) {
        return Err(AdmissionRefusal::WrongProject);
    }
    if proof.snapshot_identity() != Arc::as_ptr(&binding.basis.publication.snapshot) as usize {
        return Err(AdmissionRefusal::StaleBasis);
    }
    let mut units = input.units.clone();
    units.sort();
    units.dedup();
    if units.is_empty() || proof.units() != units {
        return Err(AdmissionRefusal::IncompleteGeneratedProof);
    }
    Ok(())
}

/// Validate cold intent, or bind it to this exact serving/replay incarnation.
/// The warm request lives in the same hub admission store as explicit requests.
pub(super) fn generated_request<P: ?Sized>(
    shared: &Shared<P>,
    serving: Option<&Serving<P>>,
    path: &str,
    expected_source: Option<&str>,
    expected_project: Option<&str>,
) -> Result<Option<AdmittedRequest>, AdmissionRefusal> {
    let Some(resolve) = shared.generated_unit_resolver.get() else {
        return Ok(None);
    };
    if let Some(serving) = serving {
        let cached = shared
            .admission
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .managed_requests
            .get(path)
            .cloned();
        if let Some(request) = cached {
            if check_witness_for_serving(shared, serving, &request.witness).is_ok() {
                if expected_source.is_some_and(|s| s != request.witness.source())
                    || expected_project.is_some_and(|p| p != request.witness.project())
                {
                    return Err(AdmissionRefusal::WrongProject);
                }
                return Ok(Some(request));
            }
        }
    }
    let Some(input) = resolve(path) else {
        return Ok(None);
    };
    let input = input?;
    validate_generated_input(&input)?;
    if expected_source.is_some_and(|s| s != input.binding.source)
        || expected_project.is_some_and(|p| p != input.binding.project)
    {
        return Err(AdmissionRefusal::WrongProject);
    }
    if !input.units.contains(&CanonicalPath::new(path)) {
        return Err(AdmissionRefusal::IncompleteGeneratedProof);
    }
    let Some(serving) = serving else {
        // No provider epoch exists. This permits holding desired bytes, never
        // speculative writes, applied receipts, or a reusable admission.
        return Ok(None);
    };
    let witness = bind_project_for_serving(shared, serving, input.binding)?;
    let mut units = input.units;
    units.sort();
    units.dedup();
    let request = AdmittedRequest { witness, units };
    check_witness_for_serving(shared, serving, &request.witness)?;
    let mut state = shared.admission.lock().unwrap_or_else(|e| e.into_inner());
    if state.managed_requests.len() >= 4096 {
        state.managed_requests.clear();
    }
    state
        .managed_requests
        .insert(path.to_string(), request.clone());
    Ok(Some(request))
}

/// Admit a READ-ONLY query on the generated unit at `path` against `serving`.
///
/// A query admission whose membership inputs (the publication and project
/// generation) still hold is reused; otherwise the resolver's proof binds a
/// query-owned witness that neither joins nor evicts the warm write bindings.
/// The workspace content generation decides neither: a read writes nothing the
/// engine keeps.
pub(super) fn generated_query<P: ?Sized>(
    shared: &Shared<P>,
    serving: &Serving<P>,
    path: &str,
    expected_project: Option<&str>,
) -> Result<Option<AdmittedRequest>, AdmissionRefusal> {
    let Some(resolve) = shared.generated_unit_resolver.get() else {
        return Ok(None);
    };
    let cached = shared
        .admission
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .query_requests
        .get(path)
        .cloned();
    if let Some(request) = cached {
        if check_query_witness(shared, &request.witness).is_ok() {
            if expected_project.is_some_and(|p| p != request.witness.project()) {
                return Err(AdmissionRefusal::WrongProject);
            }
            return Ok(Some(request));
        }
    }
    let Some(input) = resolve(path) else {
        return Ok(None);
    };
    let input = input?;
    validate_generated_input(&input)?;
    if expected_project.is_some_and(|p| p != input.binding.project) {
        return Err(AdmissionRefusal::WrongProject);
    }
    if !input.units.contains(&CanonicalPath::new(path)) {
        return Err(AdmissionRefusal::IncompleteGeneratedProof);
    }
    let witness = ProjectWitness(Arc::new(WitnessInner {
        input: input.binding,
        epoch: serving.epoch,
        hub_identity: std::ptr::from_ref(shared) as usize,
        provider_identity: provider_identity(&serving.provider),
    }));
    let mut units = input.units;
    units.sort();
    units.dedup();
    let request = AdmittedRequest { witness, units };
    check_query_witness(shared, &request.witness)?;
    let mut state = shared.admission.lock().unwrap_or_else(|e| e.into_inner());
    if state.query_requests.len() >= 4096 {
        state.query_requests.clear();
    }
    state
        .query_requests
        .insert(path.to_string(), request.clone());
    Ok(Some(request))
}

/// Settle a read admitted by [`generated_query`]: the serving incarnation and
/// the membership inputs it was admitted under, never the content generation.
pub(super) fn check_query_current<P: ?Sized>(
    shared: &Shared<P>,
    admission: &AdmittedRequest,
) -> Result<(), AdmissionRefusal> {
    check_query_witness(shared, &admission.witness)
}

/// Rebind a replayed unit after a content-only edit without repeating its
/// provider write. A changed membership basis still invalidates the install.
pub(super) fn refresh_replay_request<P: ?Sized>(
    shared: &Shared<P>,
    serving: &Serving<P>,
    path: &str,
    request: &mut AdmittedRequest,
) -> Result<(), AdmissionRefusal> {
    match check_witness_for_serving(shared, serving, &request.witness) {
        Ok(()) => return Ok(()),
        Err(AdmissionRefusal::StaleBasis) if membership_inputs_current(request) => {}
        Err(reason) => return Err(reason),
    }
    let refreshed = generated_request(
        shared,
        Some(serving),
        path,
        Some(request.witness.source()),
        Some(request.witness.project()),
    )?
    .ok_or(AdmissionRefusal::MissingGeneratedProof)?;
    // The resolver observes the workspace independently of the actor. Do not
    // let a publication change during re-admission certify earlier writes.
    if !membership_inputs_current(request) {
        return Err(AdmissionRefusal::StaleBasis);
    }
    *request = refreshed;
    check_witness_for_serving(shared, serving, &request.witness)
}

pub(super) fn managed_settlement_required<P: ?Sized>(
    shared: &Shared<P>,
    mutation: &super::DesiredMutation,
) -> bool {
    shared.generated_unit_resolver.get().is_some() && mutation.closed_path().is_none()
}

pub(super) fn generated_mutation_requests<P: ?Sized>(
    shared: &Shared<P>,
    serving: Option<&Serving<P>>,
    mutation: &super::DesiredMutation,
) -> Result<Vec<AdmittedRequest>, AdmissionRefusal> {
    if !managed_settlement_required(shared, mutation) {
        return Ok(Vec::new());
    }
    let mut requests = Vec::new();
    for (ordinal, path) in mutation.touched_paths().into_iter().enumerate() {
        use super::DesiredMutation;
        let expected = match mutation {
            DesiredMutation::RegisterCarrier {
                source_path,
                project_file_name,
                ..
            }
            | DesiredMutation::RegisterCarrierMetadata {
                source_path,
                project_file_name,
                ..
            }
            | DesiredMutation::ActivateCarrier {
                source_path,
                project_file_name,
                ..
            } => Some((source_path.as_str(), project_file_name.as_str())),
            DesiredMutation::ActivateCarriers { members } => members
                .get(ordinal)
                .map(|m| (m.source_path.as_str(), m.project_file_name.as_str())),
            _ => None,
        };
        if let Some(request) = generated_request(
            shared,
            serving,
            &path,
            expected.map(|(source, _)| source),
            expected.map(|(_, project)| project),
        )? {
            requests.push(request);
        }
    }
    Ok(requests)
}

fn bind_project_for_serving<P: ?Sized>(
    shared: &Shared<P>,
    serving: &Serving<P>,
    input: ProjectBindingInput,
) -> Result<ProjectWitness, AdmissionRefusal> {
    if input.source.is_empty() || input.project.is_empty() {
        return Err(AdmissionRefusal::WrongProject);
    }
    if (input.current_basis)().as_ref() != Some(&input.basis) {
        return Err(AdmissionRefusal::StaleBasis);
    }
    let key = input.source.clone();
    let mut state = shared.admission.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(witness) = state.bindings.get(&key) {
        let old = &witness.0;
        if old.epoch == serving.epoch
            && old.hub_identity == std::ptr::from_ref(shared) as usize
            && old.provider_identity == provider_identity(&serving.provider)
            && old.input.basis == input.basis
            && old.input.project == input.project
            && old.input.references == input.references
        {
            return Ok(witness.clone());
        }
    }
    state.bindings.retain(|_, w| w.0.input.basis == input.basis);
    state
        .requests
        .retain(|_, r| r.witness.0.input.basis == input.basis);
    state
        .managed_requests
        .retain(|_, r| r.witness.0.input.basis == input.basis);
    if state.bindings.len() >= 4096 {
        state.bindings.clear();
    }
    let witness = ProjectWitness(Arc::new(WitnessInner {
        input,
        epoch: serving.epoch,
        hub_identity: std::ptr::from_ref(shared) as usize,
        provider_identity: provider_identity(&serving.provider),
    }));
    state.bindings.insert(key, witness.clone());
    Ok(witness)
}

impl<P> ProviderHub<P>
where
    P: TypeProvider + ?Sized + Send + Sync + 'static,
{
    /// Revalidate a bound provider at request execution and settlement.
    pub fn check_project(&self, witness: &ProjectWitness) -> Result<(), AdmissionRefusal> {
        check_witness_current(&self.state.shared, witness)
    }

    /// Revalidate a generated-unit request at execution and settlement.
    pub fn check_admission(&self, admission: &AdmittedRequest) -> Result<(), AdmissionRefusal> {
        check_current(&self.state.shared, admission)
    }

    /// Revalidate a read-only query's binding at execution and settlement:
    /// the serving incarnation and the project membership it was bound under,
    /// never the workspace content generation (see [`check_query_witness`]).
    pub fn check_query(&self, witness: &ProjectWitness) -> Result<(), AdmissionRefusal> {
        check_query_witness(&self.state.shared, witness)
    }

    /// [`Self::check_query`] for a read-only query on a generated unit.
    pub fn check_query_admission(
        &self,
        admission: &AdmittedRequest,
    ) -> Result<(), AdmissionRefusal> {
        check_query_witness(&self.state.shared, &admission.witness)
    }

    /// Bind a READ-ONLY query to the serving incarnation and the project its
    /// resolver facts name. Refused only when the membership inputs (the
    /// publication and project generation) already moved, never on content
    /// drift. The witness is the query's own: it neither joins nor evicts the
    /// warm write bindings.
    pub fn bind_query(
        &self,
        input: ProjectBindingInput,
    ) -> Result<ProjectWitness, AdmissionRefusal> {
        let shared = &self.state.shared;
        let serving = shared
            .serving()
            .ok_or(AdmissionRefusal::NoServingProvider)?;
        if input.source.is_empty() || input.project.is_empty() {
            return Err(AdmissionRefusal::WrongProject);
        }
        let witness = ProjectWitness(Arc::new(WitnessInner {
            input,
            epoch: serving.epoch,
            hub_identity: std::ptr::from_ref(shared.as_ref()) as usize,
            provider_identity: provider_identity(&serving.provider),
        }));
        check_query_witness(shared, &witness)?;
        Ok(witness)
    }

    /// Admit a READ-ONLY query on exactly the generated units a complete
    /// workspace proof places in the witness's project. A proof decided under
    /// the same publication and project generation is reused; content drift
    /// never re-runs or refuses it.
    pub fn admit_query(
        &self,
        witness: &ProjectWitness,
        units: &[CanonicalPath],
        resolve: impl FnOnce() -> GeneratedUnitAdmission,
    ) -> Result<AdmittedRequest, AdmissionRefusal> {
        let mut requested = units.to_vec();
        requested.sort();
        requested.dedup();
        if requested.is_empty() {
            return Err(AdmissionRefusal::IncompleteGeneratedProof);
        }
        let key = (
            witness.source().to_string(),
            witness.project().to_string(),
            requested.clone(),
        );
        let basis = &witness.0.input.basis;
        let proven = self
            .state
            .shared
            .admission
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .query_proofs
            .get(&key)
            .is_some_and(|decided| decided.decides(basis));
        if !proven {
            validate_proof(witness, &requested, &resolve())?;
            let mut state = self
                .state
                .shared
                .admission
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            if state.query_proofs.len() >= 4096 {
                state.query_proofs.clear();
            }
            state.query_proofs.insert(key, MembershipInputs::of(basis));
        }
        let admission = AdmittedRequest {
            witness: witness.clone(),
            units: requested,
        };
        check_query_witness(&self.state.shared, &admission.witness)?;
        Ok(admission)
    }

    /// Bind resolver facts to the actual serving provider and its epoch. A
    /// complete same-basis binding is reused without another provider probe.
    pub fn bind_project(
        &self,
        input: ProjectBindingInput,
    ) -> Result<ProjectWitness, AdmissionRefusal> {
        let serving = self
            .state
            .shared
            .serving()
            .ok_or(AdmissionRefusal::NoServingProvider)?;
        let witness = bind_project_for_serving(&self.state.shared, &serving, input)?;
        check_witness_current(&self.state.shared, &witness)?;
        Ok(witness)
    }
    /// The CURRENT hub-issued witness for `source`, if one still binds the
    /// serving provider and its basis — the warm path of
    /// [`Self::bind_project`] without re-running the caller's resolver. A
    /// witness whose basis drifted, whose provider was replaced, or that
    /// this hub never issued yields `None`: the caller re-resolves and
    /// re-binds. This is the ONLY warm binding memo — no admission decision
    /// outside the hub outlives it.
    #[must_use]
    pub fn bound_project(&self, source: &str) -> Option<ProjectWitness> {
        let witness = {
            let state = self
                .state
                .shared
                .admission
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            state.bindings.get(source).cloned()
        };
        let witness = witness?;
        check_witness_current(&self.state.shared, &witness)
            .ok()
            .map(|()| witness)
    }

    /// Admit exactly the generated units named by a complete workspace proof.
    /// Inferred/default-project answers cannot satisfy this configured-project
    /// proof; the engine has no capability to override exclusions.
    pub fn admit_request(
        &self,
        witness: &ProjectWitness,
        units: &[CanonicalPath],
        proof: Option<&GeneratedUnitAdmission>,
    ) -> Result<AdmittedRequest, AdmissionRefusal> {
        let mut requested = units.to_vec();
        requested.sort();
        requested.dedup();
        if requested.is_empty() {
            return Err(AdmissionRefusal::IncompleteGeneratedProof);
        }
        let proof = proof.ok_or(AdmissionRefusal::MissingGeneratedProof)?;
        validate_proof(witness, &requested, proof)?;
        let admission = AdmittedRequest {
            witness: witness.clone(),
            units: requested,
        };
        check_current(&self.state.shared, &admission)?;
        Ok(admission)
    }

    /// Reuse only a complete admission for the exact live binding and write
    /// set. The membership query runs outside the cache lock on a cold miss.
    pub fn admit_request_with(
        &self,
        witness: &ProjectWitness,
        units: &[CanonicalPath],
        resolve: impl FnOnce() -> GeneratedUnitAdmission,
    ) -> Result<AdmittedRequest, AdmissionRefusal> {
        let mut requested = units.to_vec();
        requested.sort();
        requested.dedup();
        if requested.is_empty() {
            return Err(AdmissionRefusal::IncompleteGeneratedProof);
        }
        let key = (
            witness.source().to_string(),
            witness.project().to_string(),
            requested.clone(),
        );
        let cached = {
            let state = self
                .state
                .shared
                .admission
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            state.requests.get(&key).cloned()
        };
        if let Some(admitted) = cached {
            if admitted.witness.same_binding(witness)
                && check_current(&self.state.shared, &admitted).is_ok()
            {
                return Ok(admitted);
            }
        }
        let proof = resolve();
        let admitted = self.admit_request(witness, &requested, Some(&proof))?;
        let mut state = self
            .state
            .shared
            .admission
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        if state.requests.len() >= 4096 {
            state.requests.clear();
        }
        state.requests.insert(key, admitted.clone());
        Ok(admitted)
    }

    /// Apply one admitted generated unit through the actor. Successful state
    /// is retained but a replacement cannot replay it without fresh admission.
    pub async fn apply_overlay(
        &self,
        admission: &AdmittedRequest,
        mutation: OverlayMutation,
    ) -> Result<(), AdmissionRefusal> {
        let (mutation, lane) = mutation.into_desired(admission)?;
        check_current(&self.state.shared, admission)?;
        let (ack, rx) = tokio::sync::oneshot::channel();
        let deadline = crate::deadline::current();
        self.state
            .commands
            .send(super::Command::ApplyOverlay {
                mutation,
                admissions: vec![admission.clone()],
                lane,
                deadline,
                ack,
            })
            .map_err(|_| AdmissionRefusal::NoServingProvider)?;
        let settled = match deadline {
            Some(at) => tokio::time::timeout_at(at, rx)
                .await
                .map_err(|_| AdmissionRefusal::DeadlineElapsed)?,
            None => rx.await,
        };
        let receipt = settled.map_err(|_| AdmissionRefusal::NoServingProvider)??;
        if receipt.disposition != crate::traits::FileLoadDisposition::Forwarded {
            return Err(AdmissionRefusal::ShadowedMutation);
        }
        if receipt.epoch == Some(admission.witness.0.epoch) {
            Ok(())
        } else {
            Err(AdmissionRefusal::StaleProvider)
        }
    }

    /// Preserve one ordered provider activation batch after each member has
    /// independently passed project and generated-unit admission.
    pub async fn apply_overlay_batch(
        &self,
        members: Vec<(AdmittedRequest, CarrierActivation)>,
    ) -> Result<(), AdmissionRefusal> {
        if members.is_empty() {
            return Ok(());
        }
        for (admission, member) in &members {
            let single = OverlayMutation::ActivateCarrier {
                source_path: member.source_path.clone(),
                companion_path: member.companion_path.clone(),
                project_file_name: member.project_file_name.clone(),
                script_kind: member.script_kind,
            };
            single.into_desired(admission)?;
            check_current(&self.state.shared, admission)?;
        }
        let epoch = members[0].0.witness.0.epoch;
        if members
            .iter()
            .any(|(admission, _)| admission.witness.0.epoch != epoch)
        {
            return Err(AdmissionRefusal::StaleProvider);
        }
        let (ack, rx) = tokio::sync::oneshot::channel();
        let deadline = crate::deadline::current();
        self.state
            .commands
            .send(super::Command::ApplyOverlay {
                mutation: super::DesiredMutation::ActivateCarriers {
                    members: members.iter().map(|(_, member)| member.clone()).collect(),
                },
                admissions: members
                    .into_iter()
                    .map(|(admission, _)| admission)
                    .collect(),
                lane: super::Lane::Foreground,
                deadline,
                ack,
            })
            .map_err(|_| AdmissionRefusal::NoServingProvider)?;
        let settled = match deadline {
            Some(at) => tokio::time::timeout_at(at, rx)
                .await
                .map_err(|_| AdmissionRefusal::DeadlineElapsed)?,
            None => rx.await,
        };
        let receipt = settled.map_err(|_| AdmissionRefusal::NoServingProvider)??;
        if receipt.disposition != crate::traits::FileLoadDisposition::Forwarded {
            return Err(AdmissionRefusal::ShadowedMutation);
        }
        if receipt.epoch == Some(epoch) {
            Ok(())
        } else {
            Err(AdmissionRefusal::StaleProvider)
        }
    }

    /// Forward ONE admitted generated-unit file write directly to the serving
    /// engine, outside the actor queue.
    ///
    /// The caller this serves (the shared overlay) writes a whole open-carrier
    /// set as a CONCURRENT, barrier-coalesced sweep; the single-writer actor
    /// serializes its forwards, which would hand the engine one program
    /// update per carrier — the sequential-sweep latency the overlay's
    /// design exists to avoid. Admission is enforced HERE with the checks the
    /// actor path ([`Self::apply_overlay`]) enforces: complete proof, exact
    /// coverage of `path`, and pre/post currency against the serving epoch
    /// and the live basis. A refused write touches the provider ZERO times;
    /// a provider write failure or a post-write currency failure follows the
    /// actor path's disposition (retire the epoch, or arm its recovery).
    ///
    /// The write is NEVER recorded as replayable desired state: a replacement
    /// engine may receive the unit only through a FRESH admission against its
    /// own serving epoch.
    pub async fn forward_admitted_file(
        &self,
        admission: &AdmittedRequest,
        path: &str,
        content: &str,
        kind: OverlayFileKind,
        priority: OverlayPriority,
    ) -> Result<(), AdmissionRefusal> {
        if !admission.covers(path) {
            return Err(AdmissionRefusal::IncompleteGeneratedProof);
        }
        check_current(&self.state.shared, admission)?;
        let serving = self
            .state
            .shared
            .serving()
            .ok_or(AdmissionRefusal::NoServingProvider)?;
        if serving.epoch != admission.witness.0.epoch {
            return Err(AdmissionRefusal::StaleProvider);
        }
        // From here the write belongs to the engine, not to the issuer: it runs
        // detached to its acknowledgement and settles even if the issuer stops
        // waiting (see [`super::detach_application`]).
        let hub = Self {
            state: Arc::clone(&self.state),
        };
        let admission = admission.clone();
        let path = path.to_string();
        let content = content.to_string();
        super::detach_application(async move {
            hub.settle_admitted_file(serving, &admission, &path, &content, kind, priority)
                .await
        })
        .await
        .unwrap_or(Err(AdmissionRefusal::ProviderWriteFailed))
    }

    /// The physical half of [`Self::forward_admitted_file`]: one write to the
    /// serving engine and its settlement against the live basis.
    async fn settle_admitted_file(
        &self,
        serving: Serving<P>,
        admission: &AdmittedRequest,
        path: &str,
        content: &str,
        kind: OverlayFileKind,
        priority: OverlayPriority,
    ) -> Result<(), AdmissionRefusal> {
        let mut application =
            super::DirectApplicationGuard(Some(Arc::clone(&serving.crash_signal)));
        let forwarded = match kind {
            OverlayFileKind::Open => match priority {
                OverlayPriority::Foreground => serving.provider.open_file(path, content).await,
                OverlayPriority::Normal => serving.provider.open_file_normal(path, content).await,
                OverlayPriority::Background => {
                    serving.provider.open_file_background(path, content).await
                }
            },
            OverlayFileKind::Load => match priority {
                OverlayPriority::Foreground => serving.provider.load_file(path, content).await,
                OverlayPriority::Normal => serving.provider.load_file_normal(path, content).await,
                OverlayPriority::Background => {
                    serving.provider.load_file_background(path, content).await
                }
            },
            OverlayFileKind::Update => match priority {
                OverlayPriority::Foreground => serving.provider.update_file(path, content).await,
                OverlayPriority::Normal => serving.provider.update_file_normal(path, content).await,
                OverlayPriority::Background => {
                    serving.provider.update_file_background(path, content).await
                }
            },
        };
        let result = match forwarded {
            Ok(()) => match check_current(&self.state.shared, admission) {
                Ok(()) => {
                    let disposition = super::note_file_receipt(
                        &self.state.shared,
                        serving.provider.as_ref(),
                        path,
                        content,
                    );
                    if disposition != crate::traits::FileLoadDisposition::Forwarded {
                        Err(AdmissionRefusal::ShadowedMutation)
                    } else {
                        // A confirmed write lifts crash attribution for this path.
                        self.state
                            .shared
                            .query_watch
                            .lock()
                            .unwrap_or_else(|poisoned| poisoned.into_inner())
                            .clear_path(path);
                        Ok(())
                    }
                }
                Err(AdmissionRefusal::StaleBasis) => {
                    // Basis drift alone does not make the engine unhealthy.
                    // Withdraw this write instead of restarting it: a fresh
                    // admission can then reapply on the same incarnation.
                    if serving.provider.close_file(path).await.is_err() {
                        // The write landed but removal is unconfirmed. Retire
                        // this incarnation through the lifecycle owner: a
                        // first overlay has no marker from which a later sweep
                        // could recover withdrawal ownership. The transition
                        // itself must not run on this task — it travels under
                        // the issuer's request deadline (see
                        // [`super::detach_application`]), and recovery that
                        // outlives that bound belongs to the crash monitor's
                        // own task, exactly like a failed forward.
                        self.disposition_after_admitted_write_failure(&serving)
                            .await;
                        // The incarnation no longer serves (or is being
                        // retired fail-closed): reporting a healthy basis
                        // drift here would invite an immediate reapply
                        // against an engine that is going away.
                        Err(AdmissionRefusal::ProviderWriteFailed)
                    } else {
                        // Keep the epoch check and receipt release atomic
                        // with replacement: the serving read guard stays held
                        // across the removal (a named binding drops at the
                        // end of this block, not at the check), so an
                        // incarnation installed later cannot publish its own
                        // receipt for the path before this release lands.
                        // Compensation must not erase a new incarnation's
                        // receipt for the same path.
                        let current = self
                            .state
                            .shared
                            .serving
                            .read()
                            .unwrap_or_else(|poisoned| poisoned.into_inner());
                        if current
                            .as_ref()
                            .is_some_and(|current| current.epoch == serving.epoch)
                        {
                            super::applied_map(&self.state.shared).remove(path);
                        }
                        Err(AdmissionRefusal::StaleBasis)
                    }
                }
                Err(_) => {
                    // The engine was replaced mid-write (or no longer serves):
                    // it must not keep serving this admission's state.
                    self.disposition_after_admitted_write_failure(&serving)
                        .await;
                    Err(AdmissionRefusal::StaleProvider)
                }
            },
            Err(_) => {
                // A partial provider write cannot be allowed to serve.
                self.disposition_after_admitted_write_failure(&serving)
                    .await;
                Err(AdmissionRefusal::ProviderWriteFailed)
            }
        };
        application.0 = None;
        result
    }

    /// The post-failure disposition of a direct admitted write: an on-demand
    /// instance retires fail-closed now; an explicit or lazy-attached
    /// instance arms its crash monitor (bounded recover, or retire-only for
    /// [`super::HubPolicy::lazy_attach`]).
    async fn disposition_after_admitted_write_failure(&self, serving: &Serving<P>) {
        if self.state.policy.on_demand.is_some() {
            super::retire(&self.state.shared, serving.epoch).await;
        } else {
            serving.crash_signal.notify_one();
        }
    }
}
