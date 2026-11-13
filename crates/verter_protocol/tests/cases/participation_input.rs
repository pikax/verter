//! Participation wire input: a host crosses with typed profile
//! digests, selectors and opaque evidence only. Configuration errors refuse
//! the input; invalid observations are reported and establish no overlap.

use verter_identity::encoding::{CanonicalEncode, CanonicalEncoder};
use verter_identity::identity::{SourceId, SourceUnitId};
use verter_identity::successor::{CarrierProfileId, ProfileSchemaEpoch};
use verter_language::participation::{
    CapabilityId, CapabilityOperation, CapabilityOwnershipMask, CapabilityTarget, EffectiveMode,
    ParticipationPreset, ProfileRef, RequestSite,
};
use verter_protocol::participation::{
    InvalidEvidence, ParticipationInput, ParticipationInputError, ParticipationResolver,
    ProfileKind,
};

struct Path<'a>(&'a str);

impl CanonicalEncode for Path<'_> {
    const DOMAIN_TAG: &'static str = "participation-test.source";

    fn encode_fields(&self, encoder: &mut CanonicalEncoder) {
        encoder.field_str(1, self.0);
    }
}

fn unit(uri: &str) -> SourceUnitId {
    SourceUnitId::from_lineage(&SourceId::from_canonical(&Path(uri)), "file")
}

fn vue() -> ProfileRef {
    let epoch = ProfileSchemaEpoch::for_schema_family("participation-test.descriptor", 1);
    ProfileRef::Carrier(CarrierProfileId::from_carrier("vue", &epoch))
}

/// The host's own authorities: one known carrier profile and one open
/// document.
struct Host;

impl ParticipationResolver for Host {
    fn profile(&self, kind: ProfileKind, id: &str) -> Option<ProfileRef> {
        let known = vue();
        (kind == ProfileKind::Carrier && id == known.digest().to_hex()).then_some(known)
    }

    fn unit(&self, uri: &str) -> Option<SourceUnitId> {
        (uri == "file:///src/App.vue").then(|| unit(uri))
    }
}

fn decode(json: serde_json::Value) -> Result<ParticipationInput, serde_json::Error> {
    serde_json::from_value(json)
}

#[test]
fn a_host_input_decodes_into_typed_cells() {
    let id = vue().digest().to_hex();
    let evidence = "a".repeat(64);
    let input = decode(serde_json::json!({
        "version": 1,
        "presets": [
            { "target": { "kind": "operation", "operation": "formatting" },
              "selector": { "kind": "all" }, "preset": "disabled" }
        ],
        "ownership": [
            { "profile": { "kind": "carrier", "id": id }, "operation": "diagnostics.typescript",
              "selector": { "kind": "document", "uri": "file:///src/App.vue" }, "evidence": evidence },
            // Each of these establishes nothing: unknown profile, unknown
            // facet, unknown document, evidence that is not a digest.
            { "profile": { "kind": "framework", "id": id }, "operation": "hover",
              "selector": { "kind": "all" }, "evidence": evidence },
            { "profile": { "kind": "carrier", "id": id }, "operation": "everything",
              "selector": { "kind": "all" }, "evidence": evidence },
            { "profile": { "kind": "carrier", "id": id }, "operation": "hover",
              "selector": { "kind": "document", "uri": "file:///elsewhere.vue" }, "evidence": evidence },
            { "profile": { "kind": "carrier", "id": id }, "operation": "hover",
              "selector": { "kind": "all" }, "evidence": "Vue.volar" }
        ],
        "disabledPresentations": [ { "kind": "operation", "operation": "decoration.apiCalls" } ],
        "unsupportedClientOperations": ["inlayHint", "somethingNew"]
    }))
    .expect("well-formed input");
    let decoded = input.decode(&Host).expect("valid configuration");
    assert_eq!(
        decoded.invalid,
        vec![
            InvalidEvidence::Ownership {
                index: 1,
                cause: "unknown profile"
            },
            InvalidEvidence::Ownership {
                index: 2,
                cause: "unknown operation"
            },
            InvalidEvidence::Ownership {
                index: 3,
                cause: "unknown document"
            },
            InvalidEvidence::Ownership {
                index: 4,
                cause: "malformed evidence"
            },
            InvalidEvidence::UnknownClientOperation("somethingNew".to_owned()),
        ]
    );
    assert_eq!(decoded.inputs.ownership.len(), 1);
    assert_eq!(
        decoded.inputs.presets[0].target,
        CapabilityTarget::Operation(CapabilityOperation::Formatting)
    );
    assert_eq!(
        decoded.inputs.presets[0].preset,
        ParticipationPreset::Disabled
    );

    let mask = CapabilityOwnershipMask::build(decoded.inputs).expect("valid mask");
    let app = unit("file:///src/App.vue");
    let at = RequestSite {
        unit: Some(&app),
        region_role: None,
    };
    let diagnostics = CapabilityId::new(vue(), CapabilityOperation::TypeScriptDiagnostics);
    assert_eq!(
        mask.cell(&diagnostics, at).mode,
        EffectiveMode::WorkspaceOnly
    );
    // The rejected observations withdrew nothing.
    let hover = CapabilityId::new(vue(), CapabilityOperation::Hover);
    assert_eq!(mask.cell(&hover, at).mode, EffectiveMode::Full);
    assert_eq!(
        mask.cell(&hover, RequestSite::default()).mode,
        EffectiveMode::Full
    );
}

#[test]
fn configuration_errors_refuse_the_whole_input() {
    let refused = |json: serde_json::Value| {
        decode(json)
            .expect("well-formed input")
            .decode(&Host)
            .unwrap_err()
    };
    assert_eq!(
        refused(serde_json::json!({ "version": 2 })),
        ParticipationInputError::UnsupportedVersion(2)
    );
    assert_eq!(
        refused(serde_json::json!({ "version": 1, "presets": [
            { "target": { "kind": "all" }, "selector": { "kind": "all" }, "preset": "off" }
        ]})),
        ParticipationInputError::InvalidPreset {
            index: 0,
            value: "off".to_owned()
        }
    );
    assert_eq!(
        refused(serde_json::json!({ "version": 1, "presets": [
            { "target": { "kind": "profile", "profile": { "kind": "project", "id": "00" } },
              "selector": { "kind": "all" }, "preset": "full" }
        ]})),
        ParticipationInputError::UnknownProfile { index: 0 }
    );
}

/// Rust never receives editor identity: an extension id or a file-suffix
/// ownership rule has no field to travel in.
#[test]
fn editor_identity_has_no_wire_field() {
    let id = vue().digest().to_hex();
    for json in [
        serde_json::json!({ "version": 1, "extensionIds": ["Vue.volar"] }),
        serde_json::json!({ "version": 1, "ownership": [
            { "profile": { "kind": "carrier", "id": id }, "operation": "hover",
              "selector": { "kind": "all" }, "evidence": "a".repeat(64),
              "extensionId": "Vue.volar" }
        ]}),
        serde_json::json!({ "version": 1, "presets": [
            { "target": { "kind": "all" }, "selector": { "kind": "extension", "suffix": ".vue" },
              "preset": "disabled" }
        ]}),
        // A broad selector or target cannot smuggle a narrower intent (or an
        // editor id) in an ignored field and so withdraw everything.
        serde_json::json!({ "version": 1, "presets": [
            { "target": { "kind": "all" },
              "selector": { "kind": "all", "uri": "file:///src/App.vue", "extensionId": "Vue.volar" },
              "preset": "disabled" }
        ]}),
        serde_json::json!({ "version": 1, "presets": [
            { "target": { "kind": "all", "operation": "hover" }, "selector": { "kind": "all" },
              "preset": "disabled" }
        ]}),
    ] {
        assert!(decode(json).is_err());
    }
}
