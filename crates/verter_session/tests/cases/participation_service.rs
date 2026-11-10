//! COX0 participation service: atomic mask/demand-epoch publication and the
//! retirement fence (`docs/arch/kernel/cox0d-participation.md`, demand
//! epochs and withdrawal).

use verter_identity::successor::{CarrierProfileId, ProfileSchemaEpoch};
use verter_language::participation::{
    AdmissionRequest, CapabilityId, CapabilityOperation, CapabilityTarget, CatalogSupport,
    DemandPurpose, MaskConstructionError, NonExecutionReason, ParticipationInputs,
    ParticipationPreset, PresetDeclaration, ProfileRef, ProviderAvailability, ProviderRequirement,
    RequestSite, SelectorScope,
};
use verter_session::participation::{ParticipationService, Transition};

fn preset(preset: ParticipationPreset) -> ParticipationInputs {
    ParticipationInputs {
        presets: vec![PresetDeclaration {
            target: CapabilityTarget::AllCapabilities,
            selector: SelectorScope::AllSources,
            preset,
        }],
        ..ParticipationInputs::default()
    }
}

fn hover() -> CapabilityId {
    let epoch = ProfileSchemaEpoch::for_schema_family("cox0-test.descriptor", 1);
    CapabilityId::new(
        ProfileRef::Carrier(CarrierProfileId::from_carrier("vue", &epoch)),
        CapabilityOperation::Hover,
    )
}

fn interactive(capability: &CapabilityId, provider: ProviderRequirement) -> AdmissionRequest<'_> {
    AdmissionRequest {
        capability,
        site: RequestSite::default(),
        purpose: DemandPurpose::Interactive,
        provider,
        catalog: CatalogSupport::Supported,
    }
}

/// PC07/PC08: full → workspace → disabled → full advances the fence at each
/// effective step. The final mask equals the original one, yet no permit
/// admitted under an earlier epoch is current again, and admission under
/// each intermediate epoch follows that epoch's mask.
#[test]
fn returning_to_an_earlier_mask_never_revives_earlier_work() {
    let service = ParticipationService::new();
    let capability = hover();
    let full = service
        .publish_host_inputs(preset(ParticipationPreset::Full))
        .expect("valid");
    assert!(full.advanced());
    let original = service.epoch();
    let held = service
        .admit(&interactive(&capability, ProviderRequirement::NotRequired))
        .expect("full admits hover");
    assert!(service.is_current(&held));

    let workspace = service
        .publish_host_inputs(preset(ParticipationPreset::Workspace))
        .expect("valid");
    assert!(workspace.advanced());
    assert!(
        !service.is_current(&held),
        "superseded work is never served"
    );
    assert_eq!(
        service
            .admit(&interactive(&capability, ProviderRequirement::NotRequired))
            .unwrap_err()
            .primary,
        NonExecutionReason::PurposeForbiddenByWorkspaceOnly
    );

    service
        .publish_host_inputs(preset(ParticipationPreset::Disabled))
        .expect("valid");
    let back = service
        .publish_host_inputs(preset(ParticipationPreset::Full))
        .expect("valid");
    let Transition::Advanced { previous, current } = back else {
        panic!("returning to full is an effective transition");
    };
    assert_eq!(current.mask(), original.mask(), "same semantic identity");
    assert!(current.fence() > previous.fence());
    assert!(previous.fence() > original.fence());
    assert!(
        !service.is_current(&held),
        "an old epoch never becomes current again"
    );
    let fresh = service
        .admit(&interactive(&capability, ProviderRequirement::NotRequired))
        .expect("full admits hover again");
    assert!(service.is_current(&fresh));
    // PC08: the incremental host answers like a fresh host given the same
    // final inputs.
    let fresh_host = ParticipationService::new();
    fresh_host
        .publish_host_inputs(preset(ParticipationPreset::Full))
        .expect("valid");
    assert_eq!(fresh_host.epoch().mask(), service.epoch().mask());
    assert_eq!(
        fresh_host
            .admit(&interactive(&capability, ProviderRequirement::NotRequired))
            .map(|permit| permit.cell()),
        Ok(fresh.cell())
    );
    // A permit is current only in the scope that admitted it: `held` carries
    // fence 1, which is the fresh host's current fence.
    assert_eq!(fresh_host.epoch().fence(), held.fence());
    assert!(!fresh_host.is_current(&held));
}

/// PC10/PC13: an equivalent re-observation mints no epoch and leaves held
/// work current; a provider-availability change mints a demand epoch on its
/// own, and a refused mask publishes nothing.
#[test]
fn only_effective_changes_mint_demand_epochs() {
    let service = ParticipationService::new();
    let capability = hover();
    let start = service.epoch();
    let held = service
        .admit(&interactive(&capability, ProviderRequirement::TypeScript))
        .expect("auto admits");

    assert_eq!(
        service.publish_host_inputs(ParticipationInputs::default()),
        Ok(Transition::Unchanged(start.clone()))
    );
    assert_eq!(
        service.publish_provider(ProviderAvailability::Available),
        Transition::Unchanged(start.clone())
    );
    assert!(service.is_current(&held));

    let unavailable = service.publish_provider(ProviderAvailability::Unavailable);
    assert!(unavailable.advanced());
    assert!(!service.is_current(&held));
    assert_eq!(
        service
            .admit(&interactive(&capability, ProviderRequirement::TypeScript))
            .unwrap_err()
            .primary,
        NonExecutionReason::ProviderUnavailable
    );
    // Host inputs never overwrite the provider owner's fact.
    service
        .publish_host_inputs(ParticipationInputs {
            typescript_provider: ProviderAvailability::Available,
            ..preset(ParticipationPreset::Full)
        })
        .expect("valid");
    assert_eq!(
        service.state().mask().typescript_provider(),
        ProviderAvailability::Unavailable
    );

    let before = service.epoch();
    let contradictory = ParticipationInputs {
        presets: vec![
            PresetDeclaration {
                target: CapabilityTarget::AllCapabilities,
                selector: SelectorScope::AllSources,
                preset: ParticipationPreset::Full,
            },
            PresetDeclaration {
                target: CapabilityTarget::AllCapabilities,
                selector: SelectorScope::AllSources,
                preset: ParticipationPreset::Disabled,
            },
        ],
        ..ParticipationInputs::default()
    };
    assert!(matches!(
        service.publish_host_inputs(contradictory),
        Err(MaskConstructionError::ContradictoryPreset { .. })
    ));
    assert_eq!(service.epoch(), before, "a refused mask publishes nothing");
}

/// An equivalent re-observation whose ownership evidence changed keeps the
/// epoch (held work stays current) and explains the cell with the newest
/// evidence, never the evidence of a declaration that is gone.
#[test]
fn equivalent_reobservation_refreshes_cell_evidence() {
    use verter_identity::encoding::CanonicalEncoder;
    use verter_language::participation::{OwnershipDeclaration, OwnershipEvidence};

    let evidence = |row: &str| {
        let mut encoder = CanonicalEncoder::new("cox0-test.evidence");
        encoder.field_str(1, row);
        OwnershipEvidence(encoder.digest())
    };
    let observed = |row: &str| ParticipationInputs {
        ownership: vec![OwnershipDeclaration {
            capability: hover(),
            selector: SelectorScope::AllSources,
            evidence: evidence(row),
        }],
        ..ParticipationInputs::default()
    };
    let service = ParticipationService::new();
    assert!(service
        .publish_host_inputs(observed("tool-a"))
        .expect("valid")
        .advanced());
    let epoch = service.epoch();
    let held = service
        .admit(&AdmissionRequest {
            purpose: DemandPurpose::IndexContribution,
            ..interactive(&hover(), ProviderRequirement::NotRequired)
        })
        .expect("an overlapped cell keeps workspace semantics");

    assert_eq!(
        service.publish_host_inputs(observed("tool-c")),
        Ok(Transition::Unchanged(epoch))
    );
    assert!(service.is_current(&held));
    assert_eq!(
        service
            .state()
            .mask()
            .cell(&hover(), RequestSite::default())
            .evidence,
        vec![evidence("tool-c")]
    );
}

/// The host qualifies a capability by the semantic profile its registry
/// admits for a framework carrier and by the carrier profile otherwise, and
/// resolves a host's wire references only against those identities.
#[test]
fn host_profiles_qualify_cells_and_resolve_wire_references() {
    use std::sync::Arc;
    use verter_protocol::participation::{ParticipationResolver, ProfileKind};
    use verter_session::participation::{file_profile, file_unit, HostParticipationResolver};

    let host = verter_session::VerterHost::new(
        verter_session::HostConfig::default(),
        Arc::new(verter_workspace::MemoryWorkspace::new(Default::default())),
    );
    let vue = file_profile(&host, "/src/App.vue");
    let svelte = file_profile(&host, "/src/App.svelte");
    let script = file_profile(&host, "/src/util.ts");
    assert!(matches!(vue, ProfileRef::Framework(_)), "{vue:?}");
    assert!(matches!(svelte, ProfileRef::Framework(_)), "{svelte:?}");
    assert!(matches!(script, ProfileRef::Carrier(_)), "{script:?}");
    assert_ne!(vue, svelte);
    assert_eq!(vue, file_profile(&host, "/other/Card.vue"));

    let resolver = HostParticipationResolver::new(&host, |uri: &str| {
        uri.strip_prefix("file://").map(str::to_owned)
    });
    for (kind, profile) in [
        (ProfileKind::Framework, &vue),
        (ProfileKind::Framework, &svelte),
        (ProfileKind::Carrier, &script),
    ] {
        assert_eq!(
            resolver.profile(kind, &profile.digest().to_hex()).as_ref(),
            Some(profile)
        );
    }
    // A digest of the wrong kind, an unknown digest and a project profile
    // the host does not serve resolve to nothing.
    assert_eq!(
        resolver.profile(ProfileKind::Carrier, &vue.digest().to_hex()),
        None
    );
    assert_eq!(
        resolver.profile(ProfileKind::Framework, &"0".repeat(64)),
        None
    );
    assert_eq!(
        resolver.profile(ProfileKind::Project, &vue.digest().to_hex()),
        None
    );
    assert_eq!(
        resolver.unit("file:///src/App.vue"),
        Some(file_unit("/src/App.vue"))
    );
    assert_eq!(resolver.unit("untitled:1"), None);

    // An admitted adapter's module (a Svelte rune module) is its own carrier
    // profile: a host can name it and a workspace demand is qualified by it.
    let rune_module = file_profile(&host, "/src/store.svelte.ts");
    assert!(matches!(rune_module, ProfileRef::Carrier(_)));
    assert_ne!(rune_module, script);
    assert_eq!(
        resolver.profile(ProfileKind::Carrier, &rune_module.digest().to_hex()),
        Some(rune_module.clone())
    );
    let served = verter_session::participation::served_profiles(&host);
    assert!(served.contains(&rune_module));
}
