//! Rust-side verification of the `verter_wasm` audit binding
//! serialization surface.
//!
//! `serde-wasm-bindgen` drives the FFI conversion via the same
//! `serde::Serialize` / `serde::Deserialize` impls that `serde_json`
//! consumes; round-tripping through `serde_json` therefore exercises
//! the exact shape the WASM boundary produces, one layer down from
//! the `JsValue` conversion.
//!
//! These tests form the Rust half of a layered coverage story:
//!
//! - **This file** — pins the `serde::Serialize`/`Deserialize` impls
//!   on `RequestAuditRecord` against `serde_json`. Catches regressions
//!   in the serde annotation surface (missing `#[serde(with =
//!   "u64_as_decimal_string")]`, accidental `#[serde(skip)]`,
//!   enum-tagging drift) without requiring a live WASM runtime.
//! - [`packages/wasm/src/audit.spec.ts`](../../../packages/wasm/src/audit.spec.ts)
//!   — drives the REAL `verter_wasm_bg.wasm` binary inside Node.js
//!   via `initSync` with disk bytes. The time primitives that used
//!   to panic on wasm32-unknown-unknown now route through
//!   `verter_session::time` → `web_time`, so the Node.js-based
//!   integration test initializes cleanly.
//! - [`packages/native/index.spec.ts`](../../../packages/native/index.spec.ts)
//!   — drives the native NAPI binary end-to-end.
//!
//! Keeping this Rust-side test distinct from the live-WASM TS test
//! is intentional: this file catches serde-level regressions even
//! when the WASM binary is not yet built, and it runs inside the
//! `cargo test --workspace
//! --tests` gate without a WASM toolchain.

use std::sync::Arc;

use verter_session::component_meta_audit::{
    assertions::RequestAuditRecordAssertions, ChainTermination, ComponentMetaPayload,
    DerivationEdgeRecord, DerivationSubgraph, IndexedReadyBuildRecord, InstantiationRecord,
    NamedIdentity, NodeId, NodeRecord, OriginEdgeKind, OriginEdgeMetaDto, RequestAuditRecord,
    RequestFootprintAudit, RequestKind, RequestKindPayload, RequestMemoryAudit, RequestStoreAudit,
    RequestTimingAudit, SemanticNodeKind, SharedLoadReuseRecord, VfsLayer, VfsReadRecord,
};

/// Build a synthetic `RequestAuditRecord` that covers every record
/// vector and both code paths in `why_loaded` (derivation-graph root
/// + shared-load fallback).
fn synthesize_record() -> RequestAuditRecord {
    let nodes = vec![
        NodeRecord {
            kind: SemanticNodeKind::Primitive,
            named_identity: None,
            structural_hash: [1u8; 16],
            display_label: Arc::from("source"),
        },
        NodeRecord {
            kind: SemanticNodeKind::Alias,
            named_identity: Some(NamedIdentity {
                canonical_id: Arc::from("/Widget.vue"),
                symbol_name: Arc::from("Props"),
                args_fingerprint: [0u8; 16],
            }),
            structural_hash: [2u8; 16],
            display_label: Arc::from("Props"),
        },
    ];
    let edges = vec![DerivationEdgeRecord {
        result: NodeId(1),
        kind: OriginEdgeKind::AliasResolve,
        sources: vec![NodeId(0)],
        meta: OriginEdgeMetaDto::AliasResolve {
            alias_name: Arc::from("from-source"),
        },
    }];
    let footprint = RequestFootprintAudit {
        derivation_subgraph: DerivationSubgraph { nodes, edges },
        vfs_reads: vec![VfsReadRecord {
            canonical_id: Arc::from("/a.ts"),
            layer: VfsLayer::Disk,
            cache_hit: false,
            bytes_read: 42,
            request_id: 7,
        }],
        shared_load_reuses: vec![SharedLoadReuseRecord {
            canonical_id: Arc::from("/shared.ts"),
            winner_request_id: 99,
            winner_audited: false,
        }],
        indexed_ready_builds: vec![IndexedReadyBuildRecord {
            canonical_id: Arc::from("/ir.ts"),
            whole_hash: [3u8; 16],
        }],
        instantiations: vec![InstantiationRecord {
            result: NodeId(1),
            decl_canonical_id: Arc::from("/Widget.vue"),
            decl_symbol_name: Arc::from("Props"),
            args_fingerprint: [0u8; 16],
            args: vec![NodeId(0)],
        }],
        ..Default::default()
    };
    RequestAuditRecord {
        request_id: 7,
        canonical_id: "/Widget.vue".to_string(),
        kind: RequestKind::ComponentMeta,
        parent_request_id: None,
        timings: RequestTimingAudit {
            total_ms: 12.5,
            solver_ms: 2.5,
            ..Default::default()
        },
        store: RequestStoreAudit::default(),
        memory: RequestMemoryAudit {
            process_rss_delta_bytes: -128,
            ..Default::default()
        },
        footprint: Some(footprint),
        scheduler: None,
        files: Vec::new(),
        waits: None,
        from_cache: false,
        kind_payload: RequestKindPayload::ComponentMeta(ComponentMetaPayload {
            total_resolve_steps: 4,
            solve_count: 1,
            ..Default::default()
        }),
        capture_state: verter_audit::AuditCaptureState::ActiveStored,
        trace_id: String::new(),
    }
}

/// `wasm_get_component_meta_with_audit_serializes_across_boundary`.
/// Verify that the Rust-side
/// `RequestAuditRecord` round-trips through `serde_json` without loss —
/// this exercises the exact serde impls `serde-wasm-bindgen` consumes
/// to produce a `JsValue` bundle.
#[test]
fn wasm_get_component_meta_with_audit_serializes_across_boundary() {
    let original = synthesize_record();

    let json = serde_json::to_string(&original).expect("serialize");
    // u64 fields MUST be decimal strings on the wire.
    assert!(
        json.contains("\"request_id\":\"7\""),
        "request_id must serialize as a decimal string, got JSON: {json}",
    );
    assert!(
        json.contains("\"winner_request_id\":\"99\""),
        "winner_request_id must serialize as a decimal string, got JSON: {json}",
    );
    assert!(
        json.contains("\"bytes_read\":\"42\""),
        "bytes_read must serialize as a decimal string, got JSON: {json}",
    );
    // i64 fields likewise.
    assert!(
        json.contains("\"process_rss_delta_bytes\":\"-128\""),
        "i64 fields must serialize as decimal strings, got JSON: {json}",
    );

    let recovered: RequestAuditRecord = serde_json::from_str(&json).expect("deserialize");

    assert_eq!(recovered.request_id, original.request_id);
    assert_eq!(recovered.canonical_id, original.canonical_id);
    assert_eq!(
        recovered.memory.process_rss_delta_bytes,
        original.memory.process_rss_delta_bytes,
    );
    assert!(recovered.footprint.is_some());
    let fp = recovered.footprint.as_ref().unwrap();
    assert_eq!(fp.vfs_reads.len(), 1);
    assert_eq!(fp.vfs_reads[0].bytes_read, 42);
    assert_eq!(fp.shared_load_reuses.len(), 1);
    assert!(!fp.shared_load_reuses[0].winner_audited);
    assert_eq!(fp.indexed_ready_builds.len(), 1);
    assert_eq!(fp.instantiations.len(), 1);
    assert_eq!(fp.derivation_subgraph.nodes.len(), 2);
    assert_eq!(fp.derivation_subgraph.edges.len(), 1);
}

/// `wasm_why_loaded_binding_invokes_rust_walker`.
/// End-to-end verification: (1) Rust produces a bundle,
/// (2) the bundle is serialized to JSON (equivalent to what the WASM
/// binding hands to `whyLoadedFromAuditJson`), (3) the serialized
/// JSON is deserialized and the walker invoked on it, (4) the
/// produced `ProvenanceChain` carries the expected structure for a
/// graph-rooted lookup.
#[test]
fn wasm_why_loaded_binding_invokes_rust_walker() {
    let original = synthesize_record();

    let audit_json = serde_json::to_string(&original).expect("serialize");
    let recovered: RequestAuditRecord = serde_json::from_str(&audit_json).expect("deserialize");

    // (A) Graph-root lookup — `/Widget.vue` appears as a named
    // identity on NodeId(1), so the walker roots there and produces
    // one step via the AliasResolve edge.
    let chain = recovered.why_loaded("/Widget.vue");
    assert_eq!(chain.root, Some(NodeId(1)));
    assert_eq!(chain.steps.len(), 1);
    assert!(matches!(chain.terminated, ChainTermination::Complete));
    assert!(chain.shared_load_terminals.is_empty());

    // (B) Shared-load fallback — `/shared.ts` has no derivation
    // node but lives in `shared_load_reuses`; the walker returns
    // `Complete` (not NotFound) with the shared-load terminal
    // carried through.
    let chain2 = recovered.why_loaded("/shared.ts");
    assert!(
        matches!(chain2.terminated, ChainTermination::Complete),
        "shared-load-only case must terminate Complete, got {:?}",
        chain2.terminated,
    );
    assert_eq!(chain2.shared_load_terminals.len(), 1);
    assert!(!chain2.shared_load_terminals[0].winner_audited);
}

/// Cross-check: the `serde_json`-produced audit JSON is value-stable
/// through a struct → JSON → struct → JSON round-trip. Struct
/// Serialize emits fields in declaration order (stable); `Value`
/// Serialize emits alphabetized keys (not byte-equal to the struct
/// output). We therefore compare `Value` trees for semantic
/// equality — this catches any round-trip-lossy field (e.g. a future
/// `#[serde(skip_deserializing)]` that silently drops data) without
/// tripping on benign field-order differences.
#[test]
fn wasm_audit_record_json_is_value_stable_through_round_trip() {
    let original = synthesize_record();
    let once = serde_json::to_string(&original).expect("first serialize");
    let recovered: RequestAuditRecord = serde_json::from_str(&once).expect("deserialize to struct");
    let twice = serde_json::to_string(&recovered).expect("re-serialize struct");
    assert_eq!(
        once, twice,
        "struct → JSON → struct → JSON must be byte-stable",
    );
    let once_value: serde_json::Value = serde_json::from_str(&once).expect("Value(once)");
    let twice_value: serde_json::Value = serde_json::from_str(&twice).expect("Value(twice)");
    assert_eq!(
        once_value, twice_value,
        "JSON values must be semantically equal"
    );
}
