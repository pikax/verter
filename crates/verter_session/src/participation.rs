//! The live participation service of one host participation scope (COX0;
//! contract `docs/arch/kernel/cox0d-participation.md`).
//!
//! It is the sole participation authority of its scope: it publishes one
//! immutable [`CapabilityOwnershipMask`] together with the demand epoch that
//! mask defines, atomically, before any newer demand is admitted. A
//! participation, ownership, client-support, optional-visual or
//! provider-availability change mints a new demand epoch only when the mask
//! identity changes; it never touches activation or parse generations.
//!
//! Every effective change also advances the scope's retirement fence. A
//! [`ParticipationPermit`] carries the fence it was admitted under and is
//! current only while that fence is, so returning to an earlier mask never
//! makes earlier work current again. Hosts check the permit at every
//! publication point (response, diagnostics, warm cache, contribution
//! commit, client-local presentation) and discard work whose permit is no
//! longer current.
//!
//! The demand epoch composes the mask identity with the fence. DEM0 `DR21`
//! also composes the activation epoch; no activation-epoch producer exists
//! yet, so it joins here when one does.
//!
//! Scopes are independent: an editor scope's service never governs an
//! explicit CLI, browser or agent invocation, which owns its own service.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use arc_swap::ArcSwap;
use parking_lot::Mutex;
use verter_identity::identity::SourceUnitId;
use verter_language::participation::{
    AdmissionRequest, AdmittedCell, CapabilityOwnershipMask, MaskConstructionError, MaskIdentity,
    NonExecution, ParticipationInputs, ProfileRef, ProviderAvailability,
};
use verter_language::FileLanguage;
use verter_protocol::participation::{ParticipationResolver, ProfileKind};

/// Source of process-unique participation scope ids.
static NEXT_SCOPE: AtomicU64 = AtomicU64::new(0);

/// One demand epoch of a participation scope: the mask identity and the
/// retirement fence it was published under.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DemandEpoch {
    mask: MaskIdentity,
    fence: u64,
}

impl DemandEpoch {
    pub fn mask(&self) -> &MaskIdentity {
        &self.mask
    }

    /// The retirement fence: strictly increasing across every effective
    /// transition of this scope.
    pub fn fence(&self) -> u64 {
        self.fence
    }
}

/// The published state: the mask and the epoch it defines, swapped as one.
#[derive(Debug)]
pub struct ParticipationState {
    host_inputs: ParticipationInputs,
    mask: CapabilityOwnershipMask,
    epoch: DemandEpoch,
}

impl ParticipationState {
    pub fn mask(&self) -> &CapabilityOwnershipMask {
        &self.mask
    }

    pub fn epoch(&self) -> &DemandEpoch {
        &self.epoch
    }
}

/// What one publication did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Transition {
    /// The mask identity is unchanged: no epoch, no fence, no work.
    Unchanged(DemandEpoch),
    /// A new demand epoch is current; work admitted under `previous` must be
    /// cancelled or completed return-only, never served or published.
    Advanced {
        previous: DemandEpoch,
        current: DemandEpoch,
    },
}

impl Transition {
    pub fn current(&self) -> &DemandEpoch {
        match self {
            Self::Unchanged(epoch) | Self::Advanced { current: epoch, .. } => epoch,
        }
    }

    pub fn advanced(&self) -> bool {
        matches!(self, Self::Advanced { .. })
    }
}

/// Proof that one demand was admitted under one demand epoch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[must_use = "a permit must be checked at publication"]
pub struct ParticipationPermit {
    cell: AdmittedCell,
    scope: u64,
    fence: u64,
}

impl ParticipationPermit {
    pub fn cell(&self) -> AdmittedCell {
        self.cell
    }

    pub fn fence(&self) -> u64 {
        self.fence
    }
}

/// The participation authority of one host participation scope.
#[derive(Debug)]
pub struct ParticipationService {
    /// Process-unique id of this participation scope: a permit is current
    /// only in the scope that admitted it.
    scope: u64,
    current: ArcSwap<ParticipationState>,
    /// Serializes publications so each one reads the state it replaces.
    publish: Mutex<()>,
}

impl Default for ParticipationService {
    fn default() -> Self {
        Self::new()
    }
}

impl ParticipationService {
    /// A scope with no declarations: every capability is `auto` with no
    /// observed overlap, at fence zero.
    pub fn new() -> Self {
        let host_inputs = ParticipationInputs::default();
        let mask = CapabilityOwnershipMask::unrestricted();
        let epoch = DemandEpoch {
            mask: mask.identity().clone(),
            fence: 0,
        };
        Self {
            scope: NEXT_SCOPE.fetch_add(1, Ordering::Relaxed),
            current: ArcSwap::from_pointee(ParticipationState {
                host_inputs,
                mask,
                epoch,
            }),
            publish: Mutex::new(()),
        }
    }

    /// The current state. A caller that needs one consistent view across
    /// several reads holds this `Arc` rather than re-reading.
    pub fn state(&self) -> Arc<ParticipationState> {
        self.current.load_full()
    }

    pub fn epoch(&self) -> DemandEpoch {
        self.current.load().epoch.clone()
    }

    /// The current retirement fence alone, for publication checks that
    /// need no mask.
    pub fn fence(&self) -> u64 {
        self.current.load().epoch.fence
    }

    /// Replace the host-declared inputs (presets, ownership, client support,
    /// optional presentation). Provider availability is not a host input:
    /// the scope keeps the value its Rust owner last reported. A refused
    /// mask changes nothing.
    pub fn publish_host_inputs(
        &self,
        mut inputs: ParticipationInputs,
    ) -> Result<Transition, MaskConstructionError> {
        let _publishing = self.publish.lock();
        let previous = self.current.load_full();
        inputs.typescript_provider = previous.host_inputs.typescript_provider;
        let mask = CapabilityOwnershipMask::build(inputs.clone())?;
        Ok(self.swap(&previous, inputs, mask))
    }

    /// Record the selected TypeScript authority's availability, as its owner
    /// reports it.
    pub fn publish_provider(&self, availability: ProviderAvailability) -> Transition {
        let _publishing = self.publish.lock();
        let previous = self.current.load_full();
        if previous.host_inputs.typescript_provider == availability {
            return Transition::Unchanged(previous.epoch.clone());
        }
        let mut inputs = previous.host_inputs.clone();
        inputs.typescript_provider = availability;
        let mask = CapabilityOwnershipMask::build(inputs.clone())
            .expect("changing provider availability cannot invalidate an accepted mask");
        self.swap(&previous, inputs, mask)
    }

    fn swap(
        &self,
        previous: &ParticipationState,
        host_inputs: ParticipationInputs,
        mask: CapabilityOwnershipMask,
    ) -> Transition {
        if mask.identity() == previous.mask.identity() {
            return Transition::Unchanged(previous.epoch.clone());
        }
        let epoch = DemandEpoch {
            mask: mask.identity().clone(),
            fence: previous.epoch.fence + 1,
        };
        self.current.store(Arc::new(ParticipationState {
            host_inputs,
            mask,
            epoch: epoch.clone(),
        }));
        Transition::Advanced {
            previous: previous.epoch.clone(),
            current: epoch,
        }
    }

    /// Admit or refuse one demand against the current mask. A refusal
    /// starts no producer.
    pub fn admit(
        &self,
        request: &AdmissionRequest<'_>,
    ) -> Result<ParticipationPermit, NonExecution> {
        let state = self.current.load();
        let cell = state.mask.admit(request)?;
        Ok(ParticipationPermit {
            cell,
            scope: self.scope,
            fence: state.epoch.fence,
        })
    }

    /// Whether work admitted under `permit` may still be served, warmed or
    /// published. False after any effective transition, including one that
    /// later returns to the original mask.
    pub fn is_current(&self, permit: &ParticipationPermit) -> bool {
        permit.scope == self.scope && self.current.load().epoch.fence == permit.fence
    }
}

/// The participation profile of one host file, classified by its path. A
/// host that holds the document's authoritative resolved language (an open
/// editor document) uses [`language_profile`] instead.
pub fn file_profile(host: &crate::VerterHost, canonical: &str) -> ProfileRef {
    language_profile(host, &host.language_classifier().classify(canonical))
}

/// The participation profile of a resolved file language: the semantic
/// profile this host's registry admits for a framework carrier, otherwise the
/// language's own carrier profile. Never a display family or an extension
/// spelling.
pub fn language_profile(host: &crate::VerterHost, language: &FileLanguage) -> ProfileRef {
    if let FileLanguage::Framework { adapter_id, .. } = language {
        if let Some(entry) = host.framework_registry().profile_for_adapter(adapter_id) {
            return ProfileRef::Framework(entry.profile.clone());
        }
    }
    ProfileRef::Carrier(crate::workspace_index::carrier_profile(language))
}

/// The source unit a participation selector addresses for one host file:
/// the same unit the workspace index records it under.
pub fn file_unit(canonical: &str) -> SourceUnitId {
    crate::workspace_index::host_unit(canonical)
}

/// Resolves a host's wire references against this host's own authorities:
/// its admitted semantic profiles, its carrier profiles and its document
/// identities. `canonical_of` maps a host document address to the host's
/// canonical id.
pub struct HostParticipationResolver<'h, F> {
    host: &'h crate::VerterHost,
    canonical_of: F,
}

impl<'h, F> HostParticipationResolver<'h, F>
where
    F: Fn(&str) -> Option<String>,
{
    pub fn new(host: &'h crate::VerterHost, canonical_of: F) -> Self {
        Self { host, canonical_of }
    }
}

impl<F> ParticipationResolver for HostParticipationResolver<'_, F>
where
    F: Fn(&str) -> Option<String>,
{
    fn profile(&self, kind: ProfileKind, id: &str) -> Option<ProfileRef> {
        match kind {
            ProfileKind::Framework => self
                .host
                .framework_registry()
                .advertised_profiles()
                .find(|entry| entry.profile.digest().to_hex() == id)
                .map(|entry| ProfileRef::Framework(entry.profile.clone())),
            ProfileKind::Carrier => carrier_languages(self.host)
                .map(|language| crate::workspace_index::carrier_profile(&language))
                .find(|carrier| carrier.digest().to_hex() == id)
                .map(ProfileRef::Carrier),
            // This host serves no project-profile overlay yet.
            ProfileKind::Project => None,
        }
    }

    fn unit(&self, uri: &str) -> Option<SourceUnitId> {
        (self.canonical_of)(uri).map(|canonical| file_unit(&canonical))
    }
}

/// Every file language this host can qualify a carrier capability by: its
/// registered carriers and the script dialects.
fn carrier_languages(host: &crate::VerterHost) -> impl Iterator<Item = FileLanguage> + '_ {
    use verter_language::{JsModuleKind, ScriptSourceType};
    let scripts = [
        ScriptSourceType::Ts,
        ScriptSourceType::Tsx,
        ScriptSourceType::Dts,
    ]
    .into_iter()
    .chain(
        [
            JsModuleKind::Unambiguous,
            JsModuleKind::Module,
            JsModuleKind::CommonJs,
            JsModuleKind::Script,
        ]
        .into_iter()
        .flat_map(|kind| [ScriptSourceType::Js(kind), ScriptSourceType::Jsx(kind)]),
    )
    .map(FileLanguage::script);
    host.language_classifier()
        .carrier_rows()
        .into_iter()
        .map(|(_, language)| language)
        .chain(scripts)
}

/// Every profile this host serves: its admitted semantic profiles in
/// advertisement order, then the carrier profiles of its carriers and
/// script dialects. A workspace-wide demand is qualified by each.
pub fn served_profiles(host: &crate::VerterHost) -> Vec<ProfileRef> {
    host.framework_registry()
        .advertised_profiles()
        .map(|entry| ProfileRef::Framework(entry.profile.clone()))
        .chain(carrier_languages(host).map(|language| {
            ProfileRef::Carrier(crate::workspace_index::carrier_profile(&language))
        }))
        .collect()
}
