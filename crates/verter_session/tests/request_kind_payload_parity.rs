//! Every [`verter_audit::RequestKind`] variant must construct a
//! `RequestAuditRecord` whose `kind_payload` matches the
//! discriminant. Discriminating: a missing branch in
//! [`RequestKindPayload`] would make this test fail to compile
//! before it can even run; a wrong-payload pairing surfaces as the
//! `assert_eq!` mismatch.

use verter_audit::{
    BundlerBatchPayload, BundlerKindTag, CompilePayload, CompileTargetTag, ComponentMetaPayload,
    LspMethodTag, LspRequestPayload, McpToolPayload, RequestAuditRecord, RequestKind,
    RequestKindPayload, SemanticAnalysisPayload, TypeResolutionPayload, WorkspaceOp,
    WorkspacePayload,
};

fn empty_envelope(kind: RequestKind, kind_payload: RequestKindPayload) -> RequestAuditRecord {
    RequestAuditRecord {
        request_id: 1,
        canonical_id: String::new(),
        kind,
        parent_request_id: None,
        from_cache: false,
        timings: Default::default(),
        memory: Default::default(),
        store: Default::default(),
        footprint: None,
        scheduler: None,
        files: Vec::new(),
        waits: None,
        kind_payload,
    }
}

#[test]
fn every_request_kind_variant_constructs_a_record_with_matching_payload_variant() {
    let cases: Vec<(RequestKind, RequestKindPayload)> = vec![
        (
            RequestKind::ComponentMeta,
            RequestKindPayload::ComponentMeta(ComponentMetaPayload::default()),
        ),
        (
            RequestKind::TypeResolution,
            RequestKindPayload::TypeResolution(TypeResolutionPayload::default()),
        ),
        (
            RequestKind::SemanticAnalysis,
            RequestKindPayload::SemanticAnalysis(SemanticAnalysisPayload::default()),
        ),
        (
            RequestKind::Compile {
                target: CompileTargetTag::Ide,
            },
            RequestKindPayload::Compile(CompilePayload::default()),
        ),
        (
            RequestKind::Workspace {
                op: WorkspaceOp::AuditResolve {
                    specifier: "vue".to_string(),
                    from: None,
                },
            },
            RequestKindPayload::Workspace(WorkspacePayload::default()),
        ),
        (
            RequestKind::Lsp {
                method: LspMethodTag::Hover,
            },
            RequestKindPayload::Lsp(LspRequestPayload::default()),
        ),
        (
            RequestKind::Mcp {
                tool: "list_components".to_string(),
            },
            RequestKindPayload::Mcp(McpToolPayload::default()),
        ),
        (
            RequestKind::BundlerBatch {
                kind: BundlerKindTag::Vite,
            },
            RequestKindPayload::BundlerBatch(BundlerBatchPayload::default()),
        ),
        (
            RequestKind::Custom {
                name: "tooling".to_string(),
            },
            RequestKindPayload::None,
        ),
    ];

    for (kind, payload) in cases {
        let record = empty_envelope(kind.clone(), payload.clone());
        // The discriminant on the record matches what we constructed.
        assert_eq!(record.kind, kind);
        // The payload variant matches the kind, except for `Custom`
        // which by design carries `None` (the open escape hatch).
        match (&record.kind, &record.kind_payload) {
            (RequestKind::ComponentMeta, RequestKindPayload::ComponentMeta(_)) => {}
            (RequestKind::TypeResolution, RequestKindPayload::TypeResolution(_)) => {}
            (RequestKind::SemanticAnalysis, RequestKindPayload::SemanticAnalysis(_)) => {}
            (RequestKind::Compile { .. }, RequestKindPayload::Compile(_)) => {}
            (RequestKind::Workspace { .. }, RequestKindPayload::Workspace(_)) => {}
            (RequestKind::Lsp { .. }, RequestKindPayload::Lsp(_)) => {}
            (RequestKind::Mcp { .. }, RequestKindPayload::Mcp(_)) => {}
            (RequestKind::BundlerBatch { .. }, RequestKindPayload::BundlerBatch(_)) => {}
            (RequestKind::Custom { .. }, RequestKindPayload::None) => {}
            (k, p) => panic!(
                "request_kind_payload_parity: kind/payload mismatch — kind={:?} \
                 paired with {:?}; expected the matching payload variant",
                k, p
            ),
        }
    }
}

#[test]
fn typed_payload_accessors_return_some_only_for_matching_kinds() {
    let cm = empty_envelope(
        RequestKind::ComponentMeta,
        RequestKindPayload::ComponentMeta(ComponentMetaPayload::default()),
    );
    assert!(cm.component_meta_payload().is_some());
    assert!(cm.type_resolution_payload().is_none());
    assert!(cm.compile_payload().is_none());

    let tr = empty_envelope(
        RequestKind::TypeResolution,
        RequestKindPayload::TypeResolution(TypeResolutionPayload::default()),
    );
    assert!(tr.type_resolution_payload().is_some());
    assert!(tr.component_meta_payload().is_none());

    let lsp = empty_envelope(
        RequestKind::Lsp {
            method: LspMethodTag::Hover,
        },
        RequestKindPayload::Lsp(LspRequestPayload::default()),
    );
    assert!(lsp.lsp_payload().is_some());
    assert!(lsp.component_meta_payload().is_none());
}
