//! Parity guard: every `RequestKind` variant has a matching
//! `RequestKindPayload` variant (or the `None` carrier, for variants
//! whose typed payload is not yet defined).
//!
//! The substrate models requests as a `(kind, kind_payload)` pair.
//! Producers select the `RequestKind` discriminant first; the
//! audit-runtime is then expected to fill the matching payload arm.
//! Adding a `RequestKind` variant without a corresponding
//! `RequestKindPayload` arm leaves producers unable to attach typed
//! data and quietly downgrades the variant to `RequestKindPayload::None`.
//!
//! This test enumerates every `RequestKind` variant once and asserts:
//! - the matcher `RequestKind::matches_filter("<Variant>")` accepts
//!   the variant's textual filter form;
//! - a matching `RequestKindPayload` arm exists (the runtime can
//!   construct a typed payload for the variant, even if that
//!   payload is the substrate's `None` placeholder).
//!
//! Discriminator: against the pre-substrate tree the `TypeInfoGraph`
//! variant does not exist on either enum, so the constants below
//! fail to compile (`error[E0599]: no variant or associated item
//! named 'TypeInfoGraph' found`). Against the post-substrate tree
//! both arms compile and the assertions pass.

use std::collections::BTreeSet;

use verter_audit::payloads::TypeInfoGraphPayload;
use verter_audit::record::{RequestKind, RequestKindPayload};

/// Enumerates every `RequestKind` variant once. Adding a new variant
/// requires extending this list — the test below asserts the list
/// has the documented cardinality, so a silent drop fails the gate.
fn every_request_kind() -> Vec<RequestKind> {
    vec![
        RequestKind::ComponentMeta,
        RequestKind::TypeResolution,
        RequestKind::SemanticAnalysis,
        RequestKind::Compile {
            products: verter_audit::payloads::tags::CompileProductSetTag {
                ide_companion: true,
                ..Default::default()
            },
            backend: verter_audit::payloads::tags::CompileBackendTag::Inferred,
        },
        RequestKind::Workspace {
            op: verter_audit::payloads::WorkspaceOp::AuditResolve {
                specifier: "vue".to_string(),
                from: String::new(),
            },
        },
        RequestKind::Lsp {
            method: verter_audit::payloads::tags::LspMethodTag::Hover,
        },
        RequestKind::Mcp {
            tool: "test".to_string(),
        },
        RequestKind::BundlerBatch {
            kind: verter_audit::payloads::tags::BundlerKindTag::Vite,
        },
        RequestKind::Custom {
            name: "test".to_string(),
        },
        RequestKind::TypeInfoGraph,
        RequestKind::FlowReturnInference,
    ]
}

#[test]
fn every_request_kind_appears_exactly_once() {
    let kinds = every_request_kind();
    // Discriminating cardinality — drops surface here.
    assert_eq!(
        kinds.len(),
        11,
        "RequestKind covers exactly 11 variants (the flow-return inference variant lands as #11)",
    );

    // Each variant's filter name is distinct.
    let mut seen: BTreeSet<String> = BTreeSet::new();
    let filters = [
        "ComponentMeta",
        "TypeResolution",
        "SemanticAnalysis",
        "Compile",
        "Workspace",
        "Lsp",
        "Mcp",
        "BundlerBatch",
        "Custom",
        "TypeInfoGraph",
        "FlowReturnInference",
    ];
    for filter in filters {
        assert!(
            seen.insert(filter.to_string()),
            "duplicate filter `{filter}`"
        );
    }
    assert_eq!(seen.len(), filters.len());

    // Every variant matches its filter and nothing else.
    for (i, kind) in kinds.iter().enumerate() {
        let expected = filters[i];
        assert!(
            kind.matches_filter(expected),
            "`{kind:?}` should match filter `{expected}`",
        );
        for (j, other) in filters.iter().enumerate() {
            if i != j {
                assert!(
                    !kind.matches_filter(other),
                    "`{kind:?}` must not match foreign filter `{other}`",
                );
            }
        }
    }
}

#[test]
fn type_info_graph_kind_pairs_with_typeinfo_graph_payload() {
    let kind = RequestKind::TypeInfoGraph;
    assert!(kind.matches_filter("TypeInfoGraph"));

    let payload = RequestKindPayload::TypeInfoGraph(TypeInfoGraphPayload::default());
    // The payload variant must exist alongside the kind variant.
    match payload {
        RequestKindPayload::TypeInfoGraph(_) => {}
        other => panic!("RequestKindPayload::TypeInfoGraph expected, got {other:?}"),
    }
}

#[test]
fn typeinfo_graph_payload_accessor_returns_some() {
    use verter_audit::record::RequestAuditRecord;

    let mut record = RequestAuditRecord {
        request_id: 1,
        canonical_id: "/foo.ts".to_string(),
        target_identity: Some(verter_audit::RequestTargetIdentity::RegisteredCanonical(
            "/foo.ts".to_string(),
        )),
        kind: RequestKind::TypeInfoGraph,
        parent_request_id: None,
        from_cache: false,
        timings: Default::default(),
        memory: Default::default(),
        store: Default::default(),
        footprint: None,
        scheduler: None,
        files: Vec::new(),
        waits: None,
        kind_payload: RequestKindPayload::TypeInfoGraph(TypeInfoGraphPayload::default()),
        trace_id: String::new(),
        capture_state: verter_audit::AuditCaptureState::ActiveStored,
    };
    assert!(record.typeinfo_graph_payload().is_some());

    // Switching the payload to a non-typeinfo variant returns None.
    record.kind_payload = RequestKindPayload::None;
    assert!(record.typeinfo_graph_payload().is_none());
}

#[test]
fn flow_return_inference_kind_pairs_with_flow_return_inference_payload() {
    use verter_audit::payloads::FlowReturnInferencePayload;

    let kind = RequestKind::FlowReturnInference;
    assert!(kind.matches_filter("FlowReturnInference"));

    let payload = RequestKindPayload::FlowReturnInference(FlowReturnInferencePayload::default());
    // The payload variant must exist alongside the kind variant.
    match payload {
        RequestKindPayload::FlowReturnInference(_) => {}
        other => panic!("RequestKindPayload::FlowReturnInference expected, got {other:?}"),
    }
}

#[test]
fn flow_return_inference_payload_accessor_returns_some() {
    use verter_audit::payloads::FlowReturnInferencePayload;
    use verter_audit::record::RequestAuditRecord;

    let mut record = RequestAuditRecord {
        request_id: 1,
        canonical_id: "/foo.ts".to_string(),
        target_identity: Some(verter_audit::RequestTargetIdentity::RegisteredCanonical(
            "/foo.ts".to_string(),
        )),
        kind: RequestKind::FlowReturnInference,
        parent_request_id: None,
        from_cache: false,
        timings: Default::default(),
        memory: Default::default(),
        store: Default::default(),
        footprint: None,
        scheduler: None,
        files: Vec::new(),
        waits: None,
        kind_payload: RequestKindPayload::FlowReturnInference(FlowReturnInferencePayload::default()),
        trace_id: String::new(),
        capture_state: verter_audit::AuditCaptureState::ActiveStored,
    };
    assert!(record.flow_return_inference_payload().is_some());

    // Switching the payload to a non-flow variant returns None.
    record.kind_payload = RequestKindPayload::None;
    assert!(record.flow_return_inference_payload().is_none());
}

#[test]
fn every_payload_variant_has_a_matching_kind_arm() {
    // The substrate sets up the pairing as (RequestKind, RequestKindPayload).
    // A payload arm that lacked a matching kind variant would be
    // un-producible. The compiler's exhaustive-match check is the
    // authoritative discriminator — adding a new
    // `RequestKindPayload` arm without listing it here is a
    // compile error.
    let payloads = [
        RequestKindPayload::ComponentMeta(Default::default()),
        RequestKindPayload::TypeResolution(Default::default()),
        RequestKindPayload::SemanticAnalysis(Default::default()),
        RequestKindPayload::Compile(Default::default()),
        RequestKindPayload::Workspace(Default::default()),
        RequestKindPayload::Lsp(Default::default()),
        RequestKindPayload::Mcp(Default::default()),
        RequestKindPayload::BundlerBatch(Default::default()),
        RequestKindPayload::TypeInfoGraph(Default::default()),
        RequestKindPayload::FlowReturnInference(Default::default()),
        RequestKindPayload::None,
    ];

    // Compiler-enforced exhaustiveness: a new payload variant added
    // to the enum without an arm here is a compile error
    // (`E0004: non-exhaustive patterns`). A removed variant whose
    // arm stays in this match is also a compile error
    // (`E0599: no variant`). Either way, structural drift surfaces
    // mechanically.
    let mut typed_arms = 0usize;
    let mut none_arms = 0usize;
    for payload in &payloads {
        match payload {
            RequestKindPayload::ComponentMeta(_)
            | RequestKindPayload::TypeResolution(_)
            | RequestKindPayload::SemanticAnalysis(_)
            | RequestKindPayload::Compile(_)
            | RequestKindPayload::Workspace(_)
            | RequestKindPayload::Lsp(_)
            | RequestKindPayload::Mcp(_)
            | RequestKindPayload::BundlerBatch(_)
            | RequestKindPayload::TypeInfoGraph(_)
            | RequestKindPayload::FlowReturnInference(_) => typed_arms += 1,
            RequestKindPayload::None => none_arms += 1,
        }
    }

    // 10 typed payload arms + `None` = 11 total in the enum.
    assert_eq!(
        typed_arms, 10,
        "10 typed `RequestKindPayload` arms must each appear in the partition"
    );
    assert_eq!(
        none_arms, 1,
        "exactly one `RequestKindPayload::None` placeholder appears"
    );
    assert_eq!(payloads.len(), 11);
}
