//! Participation mask contract: profile-qualified capability identity, per-cell modes, canonical mask
//! identity and the typed non-execution reason order.

use verter_identity::encoding::{CanonicalDigest, CanonicalEncode, CanonicalEncoder};
use verter_identity::identity::{SourceId, SourceUnitId};
use verter_identity::successor::{
    CarrierProfileId, FrameworkProfileId, ProfileSchemaEpoch, ReleaseCoordinates, ReleaseId,
};
use verter_language::participation::{
    AdmissionRequest, CapabilityId, CapabilityOperation as Op, CapabilityOwnershipMask,
    CapabilityTarget, CatalogSupport, ClientSupport, DemandPurpose, EffectiveMode,
    InteractiveOwnership, MaskConstructionError, NonExecutionReason, OptionalPresentation,
    OwnershipDeclaration, OwnershipEvidence, ParticipationInputs, ParticipationPreset,
    PresetDeclaration, ProfileRef, ProviderAvailability, ProviderRequirement, RequestSite,
    SelectorScope,
};

fn epoch() -> ProfileSchemaEpoch {
    ProfileSchemaEpoch::for_schema_family("participation-test.descriptor", 1)
}

fn carrier(family: &str) -> ProfileRef {
    ProfileRef::Carrier(CarrierProfileId::from_carrier(family, &epoch()))
}

fn framework(family: &str, version: &str, major: u32) -> ProfileRef {
    let coordinates = ReleaseCoordinates::decode(family, &[version], &[major]).expect("exact");
    let release = ReleaseId::from_coordinates(&coordinates, &epoch());
    let carriers = [CarrierProfileId::from_carrier(family, &epoch())];
    ProfileRef::Framework(FrameworkProfileId::from_release_and_carriers(
        &release,
        &carriers,
        &epoch(),
    ))
}

struct Path<'a>(&'a str);

impl CanonicalEncode for Path<'_> {
    const DOMAIN_TAG: &'static str = "participation-test.source";

    fn encode_fields(&self, encoder: &mut CanonicalEncoder) {
        encoder.field_str(1, self.0);
    }
}

fn unit(path: &str) -> SourceUnitId {
    SourceUnitId::from_lineage(&SourceId::from_canonical(&Path(path)), "file")
}

fn evidence(row: &str) -> OwnershipEvidence {
    OwnershipEvidence(CanonicalDigest::of_bytes(row.as_bytes()))
}

fn vue() -> ProfileRef {
    framework("vue", "3.5.0", 3)
}

fn css() -> ProfileRef {
    carrier("css")
}

fn site<'a>(unit: &'a SourceUnitId, role: &'a str) -> RequestSite<'a> {
    RequestSite {
        unit: Some(unit),
        region_role: Some(role),
    }
}

fn request<'a>(
    capability: &'a CapabilityId,
    site: RequestSite<'a>,
    purpose: DemandPurpose,
) -> AdmissionRequest<'a> {
    AdmissionRequest {
        capability,
        site,
        purpose,
        provider: ProviderRequirement::NotRequired,
        catalog: CatalogSupport::Supported,
    }
}

fn mode(
    mask: &CapabilityOwnershipMask,
    capability: &CapabilityId,
    at: RequestSite<'_>,
) -> EffectiveMode {
    mask.cell(capability, at).mode
}

fn overlap(capability: CapabilityId, selector: SelectorScope, row: &str) -> OwnershipDeclaration {
    OwnershipDeclaration {
        capability,
        selector,
        evidence: evidence(row),
    }
}

/// Capability half of the release law: a new release re-mints the framework capability
/// and leaves the carrier capability equal; the facet alone is never the
/// identity.
#[test]
fn capability_identity_follows_its_profile_release() {
    let v35 = CapabilityId::new(vue(), Op::Hover);
    let v36 = CapabilityId::new(framework("vue", "3.6.0", 3), Op::Hover);
    assert_ne!(v35, v36);
    assert_ne!(v35.digest(), v36.digest());
    assert_eq!(
        CapabilityId::new(css(), Op::Hover),
        CapabilityId::new(carrier("css"), Op::Hover)
    );
    assert_ne!(CapabilityId::new(css(), Op::Hover), v35);
    assert_ne!(
        CapabilityId::new(vue(), Op::Hover),
        CapabilityId::new(vue(), Op::Completion)
    );
    for operation in Op::ALL {
        assert_eq!(Op::from_key(operation.key()), Some(*operation));
    }
    assert_eq!(Op::from_key("vue"), None, "a display family is not a facet");
}

/// Under `auto`, a proven overlap withdraws exactly the
/// overlapping profile, selector and facet. Unrelated facets, profiles,
/// documents and regions keep Full and still admit interactive demand.
#[test]
fn auto_overlap_withdraws_only_the_overlapping_cell() {
    let app = unit("src/App.vue");
    let other = unit("src/Other.vue");
    let vue_diagnostics = CapabilityId::new(vue(), Op::TypeScriptDiagnostics);
    let css_hover = CapabilityId::new(css(), Op::Hover);
    let mask = CapabilityOwnershipMask::build(ParticipationInputs {
        ownership: vec![
            overlap(
                vue_diagnostics.clone(),
                SelectorScope::Unit(app.clone()),
                "tool-a",
            ),
            overlap(
                css_hover.clone(),
                SelectorScope::RegionRole("style".into()),
                "tool-b",
            ),
        ],
        ..ParticipationInputs::default()
    })
    .expect("valid mask");

    let withdrawn = mask.cell(&vue_diagnostics, site(&app, "template"));
    assert_eq!(withdrawn.preset, ParticipationPreset::Auto);
    assert_eq!(withdrawn.mode, EffectiveMode::WorkspaceOnly);
    assert_eq!(withdrawn.ownership, InteractiveOwnership::OtherTool);
    assert_eq!(withdrawn.evidence, vec![evidence("tool-a")]);
    let refused = mask
        .admit(&request(
            &vue_diagnostics,
            site(&app, "template"),
            DemandPurpose::DiagnosticsPublication,
        ))
        .expect_err("withdrawn publication");
    assert_eq!(
        refused.primary,
        NonExecutionReason::PurposeForbiddenByWorkspaceOnly
    );
    // Separately demanded workspace facts stay eligible.
    assert!(mask
        .admit(&request(
            &vue_diagnostics,
            site(&app, "template"),
            DemandPurpose::IndexContribution,
        ))
        .is_ok());

    for (capability, at) in [
        (CapabilityId::new(vue(), Op::Hover), site(&app, "template")),
        (
            CapabilityId::new(vue(), Op::Completion),
            site(&app, "script"),
        ),
        (
            CapabilityId::new(vue(), Op::Definition),
            site(&app, "script"),
        ),
        (
            CapabilityId::new(vue(), Op::Formatting),
            site(&app, "template"),
        ),
        (vue_diagnostics.clone(), site(&other, "template")),
        (
            CapabilityId::new(framework("svelte", "5.0.0", 5), Op::TypeScriptDiagnostics),
            site(&app, "template"),
        ),
        (css_hover.clone(), site(&app, "template")),
    ] {
        assert_eq!(
            mode(&mask, &capability, at),
            EffectiveMode::Full,
            "{capability:?}"
        );
        let admitted = mask
            .admit(&request(&capability, at, DemandPurpose::Interactive))
            .expect("unrelated cell stays admitted");
        assert_eq!(admitted.ownership, InteractiveOwnership::Verter);
    }
    // An overlap on one facet leaves the scope owing its other facets' work.
    assert!(!mask.admits_no_full_cell(&vue(), site(&app, "template")));
    // A stylesheet-region overlap withdraws CSS hover there only.
    assert_eq!(
        mode(&mask, &css_hover, site(&other, "style")),
        EffectiveMode::WorkspaceOnly
    );
}

/// An explicit effective `full` wins over every competing-tool
/// observation; installation alone (no declaration) withdraws nothing.
#[test]
fn explicit_full_wins_over_observed_overlap() {
    let app = unit("src/App.vue");
    let diagnostics = CapabilityId::new(vue(), Op::TypeScriptDiagnostics);
    let mask = CapabilityOwnershipMask::build(ParticipationInputs {
        presets: vec![PresetDeclaration {
            target: CapabilityTarget::Operation(Op::TypeScriptDiagnostics),
            selector: SelectorScope::AllSources,
            preset: ParticipationPreset::Full,
        }],
        ownership: vec![overlap(
            diagnostics.clone(),
            SelectorScope::AllSources,
            "tool-a",
        )],
        ..ParticipationInputs::default()
    })
    .expect("valid mask");
    let cell = mask.cell(&diagnostics, site(&app, "template"));
    assert_eq!(cell.mode, EffectiveMode::Full);
    assert_eq!(cell.ownership, InteractiveOwnership::Verter);
    assert_eq!(
        cell.evidence,
        vec![evidence("tool-a")],
        "evidence stays explainable"
    );
    assert!(CapabilityOwnershipMask::unrestricted()
        .admit(&request(
            &diagnostics,
            site(&app, "template"),
            DemandPurpose::DiagnosticsPublication
        ))
        .is_ok());
}

/// Each preset's admission table, including `workspace` admitting only
/// bounded workspace semantics and a caller-only batch check.
#[test]
fn presets_admit_exactly_their_purposes() {
    use DemandPurpose::*;
    let app = unit("src/App.vue");
    let hover = CapabilityId::new(vue(), Op::Hover);
    let purposes = [
        Interactive,
        DiagnosticsPublication,
        BatchCheck,
        IndexContribution,
        Compile,
        Explain,
    ];
    for (preset, admitted) in [
        (ParticipationPreset::Disabled, &[][..]),
        (
            ParticipationPreset::Workspace,
            &[BatchCheck, IndexContribution][..],
        ),
        (ParticipationPreset::Full, &purposes[..]),
        (ParticipationPreset::Auto, &purposes[..]),
    ] {
        let mask = CapabilityOwnershipMask::build(ParticipationInputs {
            presets: vec![PresetDeclaration {
                target: CapabilityTarget::AllCapabilities,
                selector: SelectorScope::AllSources,
                preset,
            }],
            ..ParticipationInputs::default()
        })
        .expect("valid mask");
        for purpose in purposes {
            let outcome = mask.admit(&request(&hover, site(&app, "template"), purpose));
            assert_eq!(
                outcome.is_ok(),
                admitted.contains(&purpose),
                "{preset:?} {purpose:?}"
            );
            if preset == ParticipationPreset::Disabled {
                assert_eq!(outcome.unwrap_err().primary, NonExecutionReason::Disabled);
            }
        }
        // Only a scope with no Full facet owes no eager work.
        assert_eq!(
            mask.admits_no_full_cell(&vue(), site(&app, "template")),
            matches!(
                preset,
                ParticipationPreset::Disabled | ParticipationPreset::Workspace
            )
        );
    }
    assert_eq!(
        ParticipationPreset::from_key("off"),
        None,
        "invalid is not auto"
    );
}

/// Scope precedence is fixed by specificity, never by declaration order:
/// region over unit over role over all; capability over facet over profile
/// over every capability.
#[test]
fn the_most_specific_declaration_decides_in_any_order() {
    let app = unit("src/App.vue");
    let hover = CapabilityId::new(vue(), Op::Hover);
    let declaration = |target, selector, preset| PresetDeclaration {
        target,
        selector,
        preset,
    };
    let declarations = vec![
        declaration(
            CapabilityTarget::AllCapabilities,
            SelectorScope::AllSources,
            ParticipationPreset::Disabled,
        ),
        declaration(
            CapabilityTarget::Profile(vue()),
            SelectorScope::AllSources,
            ParticipationPreset::Workspace,
        ),
        declaration(
            CapabilityTarget::Operation(Op::Hover),
            SelectorScope::AllSources,
            ParticipationPreset::Full,
        ),
        declaration(
            CapabilityTarget::AllCapabilities,
            SelectorScope::RegionRole("style".into()),
            ParticipationPreset::Workspace,
        ),
        declaration(
            CapabilityTarget::AllCapabilities,
            SelectorScope::UnitRegion {
                unit: app.clone(),
                role: "style".into(),
            },
            ParticipationPreset::Full,
        ),
    ];
    let mut reversed = declarations.clone();
    reversed.reverse();
    for presets in [declarations, reversed] {
        let mask = CapabilityOwnershipMask::build(ParticipationInputs {
            presets,
            ..ParticipationInputs::default()
        })
        .expect("valid mask");
        let other = unit("src/Other.vue");
        assert_eq!(
            mask.preset(&hover, site(&other, "template")),
            ParticipationPreset::Full
        );
        assert_eq!(
            mask.preset(
                &CapabilityId::new(vue(), Op::Completion),
                site(&other, "template")
            ),
            ParticipationPreset::Workspace
        );
        assert_eq!(
            mask.preset(
                &CapabilityId::new(css(), Op::Completion),
                site(&other, "template")
            ),
            ParticipationPreset::Disabled
        );
        assert_eq!(
            mask.preset(&hover, site(&other, "style")),
            ParticipationPreset::Workspace
        );
        assert_eq!(
            mask.preset(&hover, site(&app, "style")),
            ParticipationPreset::Full
        );
    }
}

/// Equivalent declarations in any order, duplicated, or with
/// different evidence for one cell mint one mask identity; a changed
/// effective input mints another; contradictions and non-optional
/// presentation withdrawals are refused before publication.
#[test]
fn mask_identity_is_canonical_and_contradictions_are_refused() {
    let app = unit("src/App.vue");
    let diagnostics = CapabilityId::new(vue(), Op::TypeScriptDiagnostics);
    let hover = CapabilityId::new(css(), Op::Hover);
    let a = overlap(
        diagnostics.clone(),
        SelectorScope::Unit(app.clone()),
        "tool-a",
    );
    let b = overlap(
        hover.clone(),
        SelectorScope::RegionRole("style".into()),
        "tool-b",
    );
    let preset = PresetDeclaration {
        target: CapabilityTarget::Profile(css()),
        selector: SelectorScope::AllSources,
        preset: ParticipationPreset::Workspace,
    };
    let build = |ownership: Vec<OwnershipDeclaration>, presets: Vec<PresetDeclaration>| {
        CapabilityOwnershipMask::build(ParticipationInputs {
            presets,
            ownership,
            disabled_presentations: vec![CapabilityTarget::Operation(Op::ApiCallDecoration)],
            unsupported_client_operations: vec![Op::InlayHint, Op::CodeLens],
            typescript_provider: ProviderAvailability::Available,
        })
        .expect("valid mask")
    };
    let forward = build(vec![a.clone(), b.clone()], vec![preset.clone()]);
    let reversed = build(
        vec![b.clone(), a.clone(), b.clone()],
        vec![preset.clone(), preset.clone()],
    );
    assert_eq!(forward.identity(), reversed.identity());
    let other_tool = overlap(
        diagnostics.clone(),
        SelectorScope::Unit(app.clone()),
        "tool-c",
    );
    let more_evidence = build(vec![a.clone(), b.clone(), other_tool], vec![preset.clone()]);
    assert_eq!(
        forward.identity(),
        more_evidence.identity(),
        "evidence explains a cell; it does not change its identity"
    );
    let explicit_default = CapabilityOwnershipMask::build(ParticipationInputs {
        presets: vec![PresetDeclaration {
            target: CapabilityTarget::AllCapabilities,
            selector: SelectorScope::AllSources,
            preset: ParticipationPreset::Auto,
        }],
        ..ParticipationInputs::default()
    })
    .expect("valid mask");
    assert_eq!(
        explicit_default.identity(),
        CapabilityOwnershipMask::unrestricted().identity(),
        "restating the implicit default is no change"
    );
    assert_ne!(
        forward.identity(),
        build(vec![a.clone()], vec![preset.clone()]).identity()
    );
    assert_ne!(
        forward.identity(),
        CapabilityOwnershipMask::build(ParticipationInputs {
            presets: vec![preset.clone()],
            ownership: vec![a.clone(), b.clone()],
            disabled_presentations: vec![CapabilityTarget::Operation(Op::ApiCallDecoration)],
            unsupported_client_operations: vec![Op::InlayHint, Op::CodeLens],
            typescript_provider: ProviderAvailability::Unavailable,
        })
        .expect("valid mask")
        .identity(),
        "provider availability is part of the identity"
    );

    let contradiction = PresetDeclaration {
        preset: ParticipationPreset::Full,
        ..preset.clone()
    };
    assert_eq!(
        CapabilityOwnershipMask::build(ParticipationInputs {
            presets: vec![preset.clone(), contradiction],
            ..ParticipationInputs::default()
        })
        .unwrap_err(),
        MaskConstructionError::ContradictoryPreset {
            target: Box::new(preset.target.clone()),
            selector: Box::new(SelectorScope::AllSources),
        }
    );
    for target in [
        CapabilityTarget::Operation(Op::TypeScriptDiagnostics),
        CapabilityTarget::Profile(vue()),
        CapabilityTarget::AllCapabilities,
    ] {
        assert_eq!(
            CapabilityOwnershipMask::build(ParticipationInputs {
                disabled_presentations: vec![target.clone()],
                ..ParticipationInputs::default()
            })
            .unwrap_err(),
            MaskConstructionError::NotAnOptionalPresentation(Box::new(target))
        );
    }
}

/// Provider absence, unsupported client support and a disabled
/// optional visual layer stay distinct from withdrawal and from each other;
/// every restriction is kept and the primary reason follows the fixed order.
#[test]
fn non_execution_reasons_stay_distinct_in_primary_order() {
    let app = unit("src/App.vue");
    let hints = CapabilityId::new(vue(), Op::InlayHint);
    let hover = CapabilityId::new(vue(), Op::Hover);
    let decoration = CapabilityId::new(vue(), Op::ApiCallDecoration);
    let base = ParticipationInputs {
        unsupported_client_operations: vec![Op::InlayHint],
        disabled_presentations: vec![
            CapabilityTarget::Operation(Op::InlayHint),
            CapabilityTarget::Capability(decoration.clone()),
        ],
        typescript_provider: ProviderAvailability::Unavailable,
        ..ParticipationInputs::default()
    };
    let mask = CapabilityOwnershipMask::build(base.clone()).expect("valid mask");
    let typed = |capability: &CapabilityId, provider, catalog| {
        mask.admit(&AdmissionRequest {
            capability,
            site: site(&app, "template"),
            purpose: DemandPurpose::Interactive,
            provider,
            catalog,
        })
    };

    let refused = typed(
        &hints,
        ProviderRequirement::TypeScript,
        CatalogSupport::Supported,
    )
    .unwrap_err();
    assert_eq!(
        refused.primary,
        NonExecutionReason::UnsupportedClientFeature
    );
    assert_eq!(
        refused.restrictions.iter().collect::<Vec<_>>(),
        vec![
            NonExecutionReason::UnsupportedClientFeature,
            NonExecutionReason::DisabledOptionalVisualLayer,
            NonExecutionReason::ProviderUnavailable,
        ]
    );
    let cell = mask.cell(&hints, site(&app, "template"));
    assert_eq!(
        cell.mode,
        EffectiveMode::Full,
        "no restriction is a withdrawal"
    );
    assert_eq!(cell.client, ClientSupport::UnsupportedClientFeature);
    assert_eq!(
        cell.presentation,
        OptionalPresentation::DisabledOptionalVisualLayer
    );

    assert_eq!(
        typed(
            &decoration,
            ProviderRequirement::NotRequired,
            CatalogSupport::Supported
        )
        .unwrap_err()
        .primary,
        NonExecutionReason::DisabledOptionalVisualLayer
    );
    assert_eq!(
        typed(
            &hover,
            ProviderRequirement::TypeScript,
            CatalogSupport::Supported
        )
        .unwrap_err()
        .primary,
        NonExecutionReason::ProviderUnavailable
    );
    // Native work that needs no provider is still admitted.
    assert!(typed(
        &hover,
        ProviderRequirement::NotRequired,
        CatalogSupport::Supported
    )
    .is_ok());
    assert_eq!(
        typed(
            &hover,
            ProviderRequirement::TypeScript,
            CatalogSupport::UnsupportedCapability
        )
        .unwrap_err()
        .primary,
        NonExecutionReason::UnsupportedCapability
    );
    assert_eq!(
        mask.cell(&hover, site(&app, "template")).presentation,
        OptionalPresentation::NotOptional
    );

    let disabled = CapabilityOwnershipMask::build(ParticipationInputs {
        presets: vec![PresetDeclaration {
            target: CapabilityTarget::AllCapabilities,
            selector: SelectorScope::AllSources,
            preset: ParticipationPreset::Disabled,
        }],
        ..base
    })
    .expect("valid mask");
    let refused = disabled
        .admit(&AdmissionRequest {
            capability: &hints,
            site: site(&app, "template"),
            purpose: DemandPurpose::Interactive,
            provider: ProviderRequirement::TypeScript,
            catalog: CatalogSupport::UnsupportedCapability,
        })
        .unwrap_err();
    assert_eq!(refused.primary, NonExecutionReason::Disabled);
    assert_eq!(
        refused.restrictions.iter().collect::<Vec<_>>(),
        vec![
            NonExecutionReason::Disabled,
            NonExecutionReason::UnsupportedCapability,
            NonExecutionReason::UnsupportedClientFeature,
            NonExecutionReason::DisabledOptionalVisualLayer,
            NonExecutionReason::ProviderUnavailable,
        ]
    );
}
