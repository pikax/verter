//! Freshness guard for `packages/types/audit.generated.ts`.
//!
//! `ts-rs` regenerates the file from the Rust audit DTOs every time
//! `cargo test -p verter_audit` runs (the macro emits export hooks
//! that fire at test-discovery time and dump the bound types into
//! `audit.generated.ts`). This guard inspects the committed file and
//! asserts the typeinfo-graph audit shapes are present.
//!
//! Discriminator: against the pre-substrate tree the committed
//! `audit.generated.ts` does not contain the typeinfo graph DTO
//! names (no Rust types ever existed for ts-rs to emit). The test
//! fails on each missing symbol. Against the post-substrate tree
//! every symbol is present and the assertions pass.

use std::path::PathBuf;

fn workspace_root() -> PathBuf {
    let crate_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    crate_root
        .parent()
        .and_then(|p| p.parent())
        .expect("CARGO_MANIFEST_DIR must be `<workspace>/crates/verter_audit`")
        .to_path_buf()
}

fn read_audit_generated_ts() -> String {
    let path = workspace_root().join("packages/types/audit.generated.ts");
    std::fs::read_to_string(&path).unwrap_or_else(|err| {
        panic!(
            "audit.generated.ts at {} must be readable: {err}",
            path.display()
        )
    })
}

#[test]
fn typeinfo_graph_payload_type_is_exported() {
    let ts = read_audit_generated_ts();
    assert!(
        ts.contains("export type TypeInfoGraphPayload"),
        "audit.generated.ts must export `TypeInfoGraphPayload` after running cargo test -p verter_audit",
    );
}

#[test]
fn typeinfo_graph_payload_tags_are_exported() {
    let ts = read_audit_generated_ts();
    let expected = [
        "export type GraphOperationTag",
        "export type ReductionDemandTag",
        "export type GraphClosurePolicyTag",
        "export type ExactnessTag",
        "export type TypeInfoDegradationReasonTag",
        "export type FrameworkSurfaceKindSupportTag",
    ];
    let mut missing: Vec<&str> = Vec::new();
    for symbol in expected {
        if !ts.contains(symbol) {
            missing.push(symbol);
        }
    }
    assert!(
        missing.is_empty(),
        "audit.generated.ts must export every tag — missing: {missing:?}",
    );
}

#[test]
fn request_kind_payload_includes_typeinfo_graph_arm() {
    let ts = read_audit_generated_ts();
    assert!(
        ts.contains("RequestKindPayload"),
        "audit.generated.ts must declare `RequestKindPayload`",
    );
    assert!(
        ts.contains("\"TypeInfoGraph\"") && ts.contains("TypeInfoGraphPayload"),
        "RequestKindPayload union must include the `TypeInfoGraph` arm carrying `TypeInfoGraphPayload`",
    );
}

#[test]
fn request_kind_union_includes_typeinfo_graph_variant() {
    let ts = read_audit_generated_ts();
    assert!(
        ts.contains("export type RequestKind"),
        "audit.generated.ts must declare `RequestKind`",
    );
    assert!(
        ts.contains("\"TypeInfoGraph\""),
        "RequestKind discriminator must include the `TypeInfoGraph` variant",
    );
}

#[test]
fn structured_audit_event_includes_typeinfo_graph_variants() {
    let ts = read_audit_generated_ts();
    let variants = [
        "TypeInfoGraphPublished",
        "TypeInfoGraphDegraded",
        "TypeInfoGraphCacheHit",
    ];
    let mut missing = Vec::new();
    for v in variants {
        if !ts.contains(v) {
            missing.push(v);
        }
    }
    assert!(
        missing.is_empty(),
        "StructuredAuditEvent union must carry the typeinfo graph publication / degradation / cache-hit variants — missing: {missing:?}",
    );
}

#[test]
fn ts_bindings_record_the_typeinfo_graph_payload_fields() {
    let ts = read_audit_generated_ts();
    let expected_fields = [
        "operation",
        "snapshot_node_count",
        "snapshot_edge_count",
        "exactness_counts",
        "publication_retries",
        "degradation_reasons",
    ];
    let mut missing = Vec::new();
    for field in expected_fields {
        if !ts.contains(field) {
            missing.push(field);
        }
    }
    assert!(
        missing.is_empty(),
        "TypeInfoGraphPayload must expose its counter fields in the TS bindings — missing: {missing:?}",
    );
}

#[test]
fn typeinfo_graph_payload_carries_every_documented_degradation_reason() {
    let ts = read_audit_generated_ts();
    let expected = [
        "BudgetExceededNodes",
        "BudgetExceededDepth",
        "UnstablePublicationFence",
        "CycleDetected",
        "UnsupportedConstruct",
        "ColdMiss",
        "UnresolvedGeneric",
        "RequestValidation",
    ];
    let mut missing = Vec::new();
    for reason in expected {
        if !ts.contains(reason) {
            missing.push(reason);
        }
    }
    assert!(
        missing.is_empty(),
        "TypeInfoDegradationReasonTag must enumerate every closed reason — missing: {missing:?}",
    );
}

#[test]
fn typeinfo_graph_payload_carries_every_documented_exactness_status() {
    let ts = read_audit_generated_ts();
    let expected = [
        "ExactResolved",
        "ExactSymbolic",
        "UnresolvedGeneric",
        "Partial",
        "Miss",
        "Unsupported",
        "BudgetExceeded",
        "Unstable",
        "Cycle",
    ];
    let mut missing = Vec::new();
    for status in expected {
        if !ts.contains(status) {
            missing.push(status);
        }
    }
    assert!(
        missing.is_empty(),
        "ExactnessTag must enumerate every closed status — missing: {missing:?}",
    );
}
