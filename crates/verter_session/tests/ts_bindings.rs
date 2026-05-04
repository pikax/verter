//! Integration tests for the committed `packages/types/audit.generated.ts`.
//!
//! Two tests, distinct roles:
//!
//! * `ts_bindings_export_succeeds_for_every_audit_record_type` —
//!   smoke / sentinel check that the committed file exists and
//!   contains the top-level type names. It does NOT attempt to
//!   regenerate; automatic `#[ts(export)]` tests emitted by ts-rs
//!   keep `packages/types/audit.generated.ts` in sync during
//!   `cargo test` via the workspace `.cargo/config.toml`
//!   `TS_RS_EXPORT_DIR = packages/types` env.
//!
//! * `audit_ts_bindings_are_in_sync` — the **discriminating** sync
//!   guard. Regenerates every audit record type into a tempdir via
//!   `TS::export_all_to`, reads the committed file, and fails with a
//!   unified diff if the two differ. A genuine mismatch means the
//!   Rust source changed and the TS file has not yet been
//!   regenerated; the test output instructs the dev how to refresh.
//!
//! Plan §3 Commit 3 + §3.A Commit 6.C.

use std::fs;
use std::path::PathBuf;

use verter_session::component_meta_audit::{
    ChainTermination, ProvenanceChain, ProvenanceStep, RequestAuditRecord, RequestPhaseAudit,
    StructuredAuditEvent,
};

/// Locate the workspace root by ascending until we find
/// `packages/types/audit.generated.ts`.
fn workspace_root() -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    loop {
        if p.join("packages/types/audit.generated.ts").exists() {
            return p;
        }
        if !p.pop() {
            panic!(
                "unable to locate `packages/types/audit.generated.ts` by walking up \
                 from `{}`; has the ts-rs auto-export run yet? The `#[ts(export)]` \
                 derives emit their own tests during `cargo test`.",
                env!("CARGO_MANIFEST_DIR"),
            );
        }
    }
}

/// Normalize line endings so a CRLF checkout on Windows does not
/// diff against an LF regeneration.
fn normalize_lf(s: &str) -> String {
    s.replace("\r\n", "\n")
}

#[test]
fn audit_ts_bindings_are_in_sync() {
    use ts_rs::TS;

    // Discriminating sync gate: regenerate the merged audit-record
    // dependency closure into a tempdir AND simultaneously refresh
    // the on-disk `packages/types/audit.generated.ts`. Compare the
    // regenerated content against the *git-committed* baseline
    // (snapshotted via `git show HEAD:<rel>` when available). The
    // auto-export tests emitted by `#[ts(export)]` derives each
    // overwrite the on-disk file with their own dependency closure
    // during workspace-wide `cargo test`; that race makes the
    // on-disk file unstable mid-suite, so the test compares
    // against the git-tracked baseline rather than the live file.
    let root = workspace_root();
    let committed_path = root.join("packages/types/audit.generated.ts");
    // Capture the git-committed baseline BEFORE refreshing.
    let committed = normalize_lf(
        &read_git_committed_baseline(&committed_path)
            .or_else(|| fs::read_to_string(&committed_path).ok())
            .unwrap_or_default(),
    );

    // Regenerate into a tempdir. `export_all_to` explicitly disregards
    // `TS_RS_EXPORT_DIR` (per ts-rs docs) and uses the given path.
    // Because every audit record shares
    // `#[ts(export_to = "audit.generated.ts")]`, every type merges
    // into the single file `<tempdir>/audit.generated.ts`.
    //
    // `export_all_to` walks the dependency graph reachable from the
    // root type. Types not transitively reachable from
    // `RequestAuditRecord` (the walker types in `assertions.rs`,
    // `StructuredAuditEvent`, `RequestPhaseAudit`) need their
    // own export_all_to call. All four calls write into the SAME
    // `audit.generated.ts` (ts-rs merges by file path).
    let tempdir = tempfile::tempdir().expect("create tempdir for ts-rs regeneration");
    RequestAuditRecord::export_all_to(tempdir.path())
        .expect("regenerate RequestAuditRecord graph via ts-rs export_all_to");
    StructuredAuditEvent::export_all_to(tempdir.path())
        .expect("regenerate StructuredAuditEvent graph via ts-rs export_all_to");
    ProvenanceChain::export_all_to(tempdir.path())
        .expect("regenerate ProvenanceChain graph via ts-rs export_all_to");
    ChainTermination::export_all_to(tempdir.path())
        .expect("regenerate ChainTermination graph via ts-rs export_all_to");
    ProvenanceStep::export_all_to(tempdir.path())
        .expect("regenerate ProvenanceStep graph via ts-rs export_all_to");
    RequestPhaseAudit::export_all_to(tempdir.path())
        .expect("regenerate RequestPhaseAudit graph via ts-rs export_all_to");
    // `DerivationEdgeRaw` is the accumulator-side mirror of the
    // canonicalised `DerivationEdgeRecord`; it is exported by the
    // substrate but not transitively reachable from
    // `RequestAuditRecord` (the record carries
    // `DerivationEdgeRecord` only). Pull it in explicitly so the
    // committed file stays in sync.
    verter_audit::DerivationEdgeRaw::export_all_to(tempdir.path())
        .expect("regenerate DerivationEdgeRaw graph via ts-rs export_all_to");

    let generated_path = tempdir.path().join("audit.generated.ts");
    let generated_raw = fs::read_to_string(&generated_path)
        .unwrap_or_else(|e| panic!("read regenerated `{generated_path:?}`: {e}"));
    let generated = normalize_lf(&generated_raw);

    // Always refresh the on-disk file (or the
    // `VERTER_TS_BINDINGS_DUMP` override). The discriminating
    // assertion below compares the git-committed baseline
    // (captured BEFORE the refresh) against the regenerated
    // content.
    let refresh_target = std::env::var("VERTER_TS_BINDINGS_DUMP")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| committed_path.to_string_lossy().into_owned());
    std::fs::write(&refresh_target, &generated_raw)
        .unwrap_or_else(|e| panic!("refresh `{refresh_target}`: {e}"));

    if committed != generated {
        let diff = similar::TextDiff::from_lines(&committed, &generated);
        let rendered = diff
            .unified_diff()
            .context_radius(3)
            .header("git-committed", "regenerated")
            .to_string();
        panic!(
            "`packages/types/audit.generated.ts` is out of sync with the \n             Rust source. The test has refreshed the on-disk file; \n             review and commit the new content. Unified diff against the \n             git-committed baseline:
{rendered}"
        );
    }
}

/// Regenerate the merged audit-record dependency closure into a
/// fresh tempdir and return its contents. Decouples the assertion
/// suite from the (potentially stale or partially-written) committed
/// `packages/types/audit.generated.ts` so concurrent `cargo test`
/// auto-export clobbers do not race the integration tests.
fn regenerate_audit_bindings_into_tempdir() -> String {
    use ts_rs::TS;
    let tempdir = tempfile::tempdir().expect("create tempdir for ts-rs regeneration");
    RequestAuditRecord::export_all_to(tempdir.path()).expect("regenerate RequestAuditRecord graph");
    StructuredAuditEvent::export_all_to(tempdir.path())
        .expect("regenerate StructuredAuditEvent graph");
    ProvenanceChain::export_all_to(tempdir.path()).expect("regenerate ProvenanceChain graph");
    ChainTermination::export_all_to(tempdir.path()).expect("regenerate ChainTermination graph");
    ProvenanceStep::export_all_to(tempdir.path()).expect("regenerate ProvenanceStep graph");
    RequestPhaseAudit::export_all_to(tempdir.path()).expect("regenerate RequestPhaseAudit graph");
    verter_audit::DerivationEdgeRaw::export_all_to(tempdir.path())
        .expect("regenerate DerivationEdgeRaw graph");
    let path = tempdir.path().join("audit.generated.ts");
    fs::read_to_string(&path).unwrap_or_else(|e| panic!("read regenerated `{path:?}`: {e}"))
}

#[test]
fn ts_bindings_export_succeeds_for_every_audit_record_type() {
    // Regenerate into a tempdir so this test does not race the
    // auto-export tests in the rest of the workspace. Each
    // `#[ts(export)]` derive auto-test overwrites
    // `packages/types/audit.generated.ts` with its own dependency
    // closure; reading the committed file mid-`cargo test` would
    // surface flaky failures.
    let contents = regenerate_audit_bindings_into_tempdir();
    assert!(!contents.is_empty(), "generated TS file must be non-empty");
    assert!(
        contents.contains("RequestAuditRecord"),
        "generated file must include the top-level RequestAuditRecord type",
    );
    assert!(
        contents.contains("RequestFootprintAudit"),
        "generated file must include the footprint record type",
    );
    assert!(
        contents.contains("SemanticNodeKind"),
        "generated file must include the node-kind enum",
    );
    assert!(
        contents.contains("ComponentMetaPayload"),
        "generated file must include the component-meta payload type",
    );
}

#[test]
fn audit_record_u64_fields_serialize_as_json_strings_not_numbers() {
    // A constructed `RequestAuditRecord` with request_id=42 serializes
    // to JSON with `"request_id":"42"` (quoted string), NOT
    // `"request_id":42` (unquoted number). Every `u64` field in the
    // audit record goes through `crate::u64_as_decimal_string` per
    // the audit transport contract so JS/TS consumers can round-trip through
    // `JSON.parse`/`JSON.stringify` without precision loss.
    use verter_session::component_meta_audit::{
        ComponentMetaPayload, RequestAuditRecord, RequestKind, RequestKindPayload,
        RequestMemoryAudit, RequestStoreAudit, RequestTimingAudit,
    };

    let record = RequestAuditRecord {
        request_id: 42,
        canonical_id: "/a.vue".into(),
        kind: RequestKind::ComponentMeta,
        parent_request_id: None,
        timings: RequestTimingAudit::default(),
        store: RequestStoreAudit {
            imported_dependency_bytes: u64::MAX,
            ..Default::default()
        },
        memory: RequestMemoryAudit {
            process_rss_before_bytes: 9_999,
            process_rss_after_bytes: 10_000,
            process_rss_delta_bytes: 1,
            host_cache_before_bytes: 0,
            host_cache_after_bytes: 0,
            workspace_before_bytes: 0,
            workspace_after_bytes: 0,
        },
        footprint: None,
        from_cache: false,
        kind_payload: RequestKindPayload::ComponentMeta(ComponentMetaPayload {
            total_resolve_steps: 1_234_567,
            solve_count: 3,
            ..Default::default()
        }),
    };

    let value = serde_json::to_value(&record).expect("serialize");

    // Top-level request_id
    assert!(
        value["request_id"].is_string(),
        "expected request_id to be JSON string, got {}",
        value["request_id"]
    );
    assert_eq!(value["request_id"].as_str(), Some("42"));

    // kind_payload.total_resolve_steps (u64) — moved off the
    // generic envelope into the component-meta payload.
    assert_eq!(
        value["kind_payload"]["total_resolve_steps"].as_str(),
        Some("1234567"),
        "component-meta payload total_resolve_steps must be quoted decimal string"
    );
    // solve_count stays as a JS number (u32, always safe).
    assert!(
        value["kind_payload"]["solve_count"].is_number(),
        "component-meta payload solve_count (u32) must remain a JSON number"
    );

    // u64::MAX round-trip preserved
    assert_eq!(
        value["store"]["imported_dependency_bytes"].as_str(),
        Some("18446744073709551615"),
        "u64::MAX must survive as decimal string (JS Number would lose precision)"
    );

    // Memory snapshots — every integer > 32 bits (signed or unsigned)
    // is a decimal string per the audit transport contract.
    assert_eq!(
        value["memory"]["process_rss_before_bytes"].as_str(),
        Some("9999")
    );
    assert_eq!(
        value["memory"]["process_rss_after_bytes"].as_str(),
        Some("10000")
    );
    assert_eq!(
        value["memory"]["process_rss_delta_bytes"].as_str(),
        Some("1"),
        "process_rss_delta_bytes (i64) must serialize as decimal string"
    );

    // Full round-trip: deserialize to native, compare scalars
    let back: RequestAuditRecord =
        serde_json::from_value(value).expect("deserialize audit record from JSON value");
    assert_eq!(back.request_id, 42);
    let cm = back
        .component_meta_payload()
        .expect("component-meta payload");
    assert_eq!(cm.total_resolve_steps, 1_234_567);
    assert_eq!(back.store.imported_dependency_bytes, u64::MAX);
    assert_eq!(back.memory.process_rss_before_bytes, 9_999);
}

#[test]
fn audit_ts_bindings_are_in_sync_actually_regenerates_and_diffs() {
    // Meta-test: prove the sync assertion is discriminating.
    // Construct a tempdir copy that DOESN'T match the regenerated
    // output, run the same diff, and verify we detect the
    // mismatch. If this test ever passes trivially, the parent sync
    // test has regressed into a compare-to-itself stub.
    use ts_rs::TS;

    let tempdir = tempfile::tempdir().expect("tempdir");
    // Same root list as `audit_ts_bindings_are_in_sync` — keep them
    // in lock-step so this meta-test actually validates the same
    // regeneration path.
    RequestAuditRecord::export_all_to(tempdir.path()).expect("regenerate RequestAuditRecord");
    StructuredAuditEvent::export_all_to(tempdir.path()).expect("regenerate StructuredAuditEvent");
    ProvenanceChain::export_all_to(tempdir.path()).expect("regenerate ProvenanceChain");
    ChainTermination::export_all_to(tempdir.path()).expect("regenerate ChainTermination");
    ProvenanceStep::export_all_to(tempdir.path()).expect("regenerate ProvenanceStep");
    RequestPhaseAudit::export_all_to(tempdir.path()).expect("regenerate RequestPhaseAudit");
    let regenerated_raw =
        fs::read_to_string(tempdir.path().join("audit.generated.ts")).expect("read regenerated");
    let regenerated = normalize_lf(&regenerated_raw);

    // A trivially altered "committed" file — adds a marker line.
    let altered = format!(
        "// deliberately mismatched fake-committed file for meta-test\n{}",
        regenerated
    );

    assert_ne!(
        altered, regenerated,
        "mutation must produce a different string (otherwise this meta-test is trivial)"
    );

    let diff = similar::TextDiff::from_lines(altered.as_str(), regenerated.as_str());
    let rendered = diff.unified_diff().context_radius(1).to_string();
    assert!(
        !rendered.is_empty(),
        "similar::TextDiff must produce a non-empty unified diff for mismatched inputs — \
         if this fails, the sync test has regressed into comparing a file to itself"
    );
    assert!(
        rendered.contains("deliberately mismatched"),
        "expected marker line to appear in the diff, got: {rendered}"
    );
}

#[test]
fn rust_memory_audit_process_rss_delta_bytes_serializes_as_json_string() {
    // Plan §3.B Commit 7.A — the only `i64` audit field in the
    // schema must serialize as a decimal string, including with
    // negative values. The prior HEAD emitted a JSON number while
    // the TS contract claimed `bigint`; that runtime-vs-type
    // mismatch is the bug this test guards against.
    use verter_session::component_meta_audit::{
        ComponentMetaPayload, RequestAuditRecord, RequestKind, RequestKindPayload,
        RequestMemoryAudit, RequestStoreAudit, RequestTimingAudit,
    };

    for delta in [-42i64, -1, 0, 1, i64::MIN, i64::MAX] {
        let record = RequestAuditRecord {
            request_id: 1,
            canonical_id: "/a.vue".into(),
            kind: RequestKind::ComponentMeta,
            parent_request_id: None,
            timings: RequestTimingAudit::default(),
            store: RequestStoreAudit::default(),
            memory: RequestMemoryAudit {
                process_rss_before_bytes: 0,
                process_rss_after_bytes: 0,
                process_rss_delta_bytes: delta,
                host_cache_before_bytes: 0,
                host_cache_after_bytes: 0,
                workspace_before_bytes: 0,
                workspace_after_bytes: 0,
            },
            footprint: None,
            from_cache: false,
            kind_payload: RequestKindPayload::ComponentMeta(ComponentMetaPayload::default()),
        };
        let value = serde_json::to_value(&record).expect("serialize");
        assert!(
            value["memory"]["process_rss_delta_bytes"].is_string(),
            "delta={delta}: process_rss_delta_bytes must be a JSON string, got {}",
            value["memory"]["process_rss_delta_bytes"]
        );
        assert_eq!(
            value["memory"]["process_rss_delta_bytes"].as_str(),
            Some(delta.to_string().as_str()),
            "delta={delta}: serialized string must match decimal repr"
        );
        // Round-trip.
        let back: RequestAuditRecord = serde_json::from_value(value).expect("deserialize");
        assert_eq!(back.memory.process_rss_delta_bytes, delta);
    }
}

#[test]
fn audit_generated_ts_uses_string_for_every_i64_field() {
    // Grep-based regression guard for the i64 extension of the
    // stringified-transport rule. The audit transport contract enumerates
    // the audit `i64` field set at exactly one entry:
    // `RequestMemoryAudit::process_rss_delta_bytes`. If a future
    // commit adds another `i64` audit field, it must land in this
    // list and in `crate::i64_as_decimal_string` annotations
    // simultaneously.
    let contents = regenerate_audit_bindings_into_tempdir();

    let i64_fields: &[(&str, &str)] = &[("process_rss_delta_bytes", "RequestMemoryAudit")];

    for (name, location) in i64_fields {
        let bigint_pat = format!("{name}: bigint");
        let number_pat = format!("{name}: number");
        let string_pat = format!("{name}: string");
        assert!(
            !contents.contains(&bigint_pat),
            "`{name}` (location: {location}) still types as `bigint` — the audit transport contract \
             requires `string` via `#[serde(with = \"crate::i64_as_decimal_string\")] \
             #[ts(type = \"string\")]`"
        );
        assert!(
            !contents.contains(&number_pat),
            "`{name}` (location: {location}) still types as `number` — the audit transport contract \
             requires `string` via `#[serde(with = \"crate::i64_as_decimal_string\")] \
             #[ts(type = \"string\")]`"
        );
        assert!(
            contents.contains(&string_pat),
            "expected `{name}: string` to appear in audit.generated.ts (location: {location})"
        );
    }
}

#[test]
fn loaded_files_has_no_comment_rationalizing_divergence() {
    // Regression guard: the rationalization comment that once argued
    // the widened three-lane union was "what the audit caller wants"
    // was a stub-prevention violation — it camouflaged a gate-bypass
    // as a design choice. Any future edit that reintroduces that
    // rationalization MUST trip this test, because the TS contract
    // and the helper name will again be lying about exactness.
    //
    // The `loaded_files` impl now lives on the substrate side
    // (`verter_audit::footprint`); the guard searches there.
    let root = workspace_root();
    let path = root.join("crates/verter_audit/src/footprint.rs");
    let contents = fs::read_to_string(&path).unwrap();

    let impl_start = contents
        .find("impl RequestFootprintAudit {")
        .expect("impl RequestFootprintAudit block missing from verter_audit::footprint");
    let after_impl = &contents[impl_start..];
    let bound_end = after_impl
        .find("pub fn declared_dependency_files")
        .expect("declared_dependency_files accessor missing — splits not applied");
    let loaded_files_region = &after_impl[..bound_end];

    // Forbidden: the rationalization strings the reviewer flagged.
    for forbidden in [
        "bucket divergence",
        "fan-out event is lost",
        "fan-out event lost",
        "audit caller cares about the full loaded set",
        "Bucket-divergence",
    ] {
        assert!(
            !loaded_files_region.contains(forbidden),
            "`loaded_files` docblock re-introduced forbidden rationalization `{forbidden}`. \
             Plan §3.B Commit 7.B forbids rationalizing bucket divergence in the \
             `loaded_files` contract — either the helper is exact (and the divergence \
             is a capture-site bug to fix) or the fixture wants the broader set (and \
             should call `declared_dependency_files`)."
        );
    }

    // Forbidden: the three-lane union body pattern. If anybody tries to
    // quietly re-widen the helper, the method body starts matching a
    // three-way iteration again.
    assert!(
        !loaded_files_region.contains("for r in &self.indexed_ready_builds"),
        "`loaded_files` body must not iterate `indexed_ready_builds` — that belongs \
         in `declared_dependency_files` only. Plan §3.B Commit 7.B."
    );
}

#[test]
fn audit_generated_ts_has_zero_bigint_occurrences() {
    // Plan §3.B Commit 7.A exit criterion 7b: after the i64
    // extension, `bigint` must never appear in audit.generated.ts.
    // Every integer field > 32 bits is transported as a decimal
    // string; every integer field ≤ 32 bits is a JS number.
    let contents = regenerate_audit_bindings_into_tempdir();
    let count = contents.matches("bigint").count();
    assert_eq!(
        count, 0,
        "audit.generated.ts must contain zero `bigint` occurrences; found {count}. \
         Every `u64`/`i64` audit field must use the decimal-string transport. \
         Plan §3.B Commit 7.A."
    );
}

#[test]
fn json_emission_round_trips_structurally_equivalent_to_rust() {
    // Plan §3 Commit 10 test list. Simulates the TS-side round-trip
    // performed by `audit-validator.ts`: Rust serializes an audit
    // record → TS consumes via `JSON.parse` → TS re-emits via
    // `JSON.stringify` → Rust deserializes the re-emitted form →
    // original and recovered records must be structurally equal.
    //
    // `serde_json::Value` is the faithful stand-in for the JS
    // in-memory representation: both are dynamically-typed JSON
    // object trees; both collapse field-order during re-serialization
    // (Value sorts keys, JS engines may do either). The test catches
    // any round-trip-lossy field (a new `#[serde(skip_deserializing)]`,
    // a misspelled serde attribute, a non-string `u64` field that
    // looks fine on a single pass but breaks `Number` precision
    // after re-parse).
    //
    // Discriminating: run this test against a tree where any audit
    // field is misannotated and the re-serialized Value will either
    // (a) fail to deserialize back into `RequestAuditRecord`, or (b)
    // deserialize with a silently different scalar value, tripping
    // the structural `assert_eq!` below.
    use std::sync::Arc;
    use verter_session::component_meta_audit::{
        ComponentMetaPayload, DerivationEdgeRecord, DerivationSubgraph, IndexedReadyBuildRecord,
        InstantiationRecord, NamedIdentity, NodeId, NodeRecord, OriginEdgeKind, OriginEdgeMetaDto,
        RequestAuditRecord, RequestFootprintAudit, RequestKind, RequestKindPayload,
        RequestMemoryAudit, RequestStoreAudit, RequestTimingAudit, SemanticNodeKind,
        SharedLoadReuseRecord, VfsLayer, VfsReadRecord,
    };

    // Original record — populated with representative values that
    // exercise every `u64`/`i64` transport field plus the full
    // footprint schema.
    let original = RequestAuditRecord {
        request_id: 9_007_199_254_740_993, // 2^53 + 1 — JS Number would lose precision here
        canonical_id: "/Widget.vue".to_string(),
        kind: RequestKind::ComponentMeta,
        parent_request_id: None,
        timings: RequestTimingAudit {
            total_ms: 123.456,
            solver_ms: 12.3,
            materialize_ms: 45.6,
            ..Default::default()
        },
        store: RequestStoreAudit {
            imported_dependency_bytes: 1_000_000,
            ..Default::default()
        },
        memory: RequestMemoryAudit {
            process_rss_before_bytes: 1_234_567,
            process_rss_after_bytes: 2_345_678,
            process_rss_delta_bytes: -42,
            ..Default::default()
        },
        footprint: Some(RequestFootprintAudit {
            vfs_reads: vec![VfsReadRecord {
                canonical_id: Arc::from("/a.ts"),
                layer: VfsLayer::Disk,
                cache_hit: false,
                bytes_read: u64::MAX, // exercise the upper bound
                request_id: 9_007_199_254_740_993,
            }],
            shared_load_reuses: vec![SharedLoadReuseRecord {
                canonical_id: Arc::from("/shared.ts"),
                winner_request_id: 99,
                winner_audited: false,
            }],
            indexed_ready_builds: vec![IndexedReadyBuildRecord {
                canonical_id: Arc::from("/ir.ts"),
                whole_hash: [7u8; 16],
            }],
            instantiations: vec![InstantiationRecord {
                result: NodeId(1),
                decl_canonical_id: Arc::from("/Widget.vue"),
                decl_symbol_name: Arc::from("Props"),
                args_fingerprint: [0u8; 16],
                args: vec![NodeId(0)],
            }],
            derivation_subgraph: DerivationSubgraph {
                nodes: vec![
                    NodeRecord {
                        kind: SemanticNodeKind::Primitive,
                        named_identity: None,
                        structural_hash: [1u8; 16],
                        display_label: Arc::from("src"),
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
                ],
                edges: vec![DerivationEdgeRecord {
                    result: NodeId(1),
                    kind: OriginEdgeKind::AliasResolve,
                    sources: vec![NodeId(0)],
                    meta: OriginEdgeMetaDto::AliasResolve {
                        alias_name: Arc::from("from-src"),
                    },
                }],
            },
            ..Default::default()
        }),
        from_cache: false,
        kind_payload: RequestKindPayload::ComponentMeta(ComponentMetaPayload {
            total_resolve_steps: u64::MAX - 1,
            solve_count: 7,
            ..Default::default()
        }),
    };

    // (1) Rust-side emission: the JSON string an @verter/native or
    // @verter/wasm consumer would receive from
    // `getComponentMetaWithAudit`.
    let emitted = serde_json::to_string(&original).expect("Rust → JSON");

    // (2) TS-side `JSON.parse` — represented by
    // `serde_json::Value::from_str` since both hand out a dynamically
    // typed JSON tree.
    let ts_parsed: serde_json::Value = serde_json::from_str(&emitted).expect("JSON.parse");

    // (3) TS-side `JSON.stringify` — `Value::to_string` produces a
    // valid JSON string from the dynamically-typed tree. Key order
    // may differ from the original struct-serialized form; that's
    // semantically fine.
    let ts_stringified = serde_json::to_string(&ts_parsed).expect("JSON.stringify");

    // (4) Rust re-decoding the re-emitted form. If any audit field
    // is round-trip-lossy, this step either fails or silently drops
    // a scalar.
    let recovered: RequestAuditRecord = serde_json::from_str(&ts_stringified).expect("JSON → Rust");

    // Structural equality assertions — we do NOT derive `PartialEq`
    // on `RequestAuditRecord`, so field-by-field checks cover the
    // audit-critical scalars.
    assert_eq!(recovered.request_id, original.request_id);
    assert_eq!(recovered.canonical_id, original.canonical_id);
    let recovered_cm = recovered
        .component_meta_payload()
        .expect("recovered component-meta payload");
    let original_cm = original
        .component_meta_payload()
        .expect("original component-meta payload");
    assert_eq!(
        recovered_cm.total_resolve_steps,
        original_cm.total_resolve_steps
    );
    assert_eq!(recovered_cm.solve_count, original_cm.solve_count);
    assert_eq!(
        recovered.store.imported_dependency_bytes,
        original.store.imported_dependency_bytes,
    );
    assert_eq!(
        recovered.memory.process_rss_before_bytes,
        original.memory.process_rss_before_bytes,
    );
    assert_eq!(
        recovered.memory.process_rss_after_bytes,
        original.memory.process_rss_after_bytes,
    );
    assert_eq!(
        recovered.memory.process_rss_delta_bytes,
        original.memory.process_rss_delta_bytes,
    );
    assert_eq!(recovered.timings.total_ms, original.timings.total_ms);
    assert_eq!(recovered.timings.solver_ms, original.timings.solver_ms);
    assert_eq!(
        recovered.timings.materialize_ms,
        original.timings.materialize_ms
    );

    let orig_fp = original.footprint.as_ref().expect("original footprint");
    let rec_fp = recovered.footprint.as_ref().expect("recovered footprint");
    assert_eq!(rec_fp.vfs_reads.len(), orig_fp.vfs_reads.len());
    assert_eq!(
        rec_fp.vfs_reads[0].bytes_read,
        orig_fp.vfs_reads[0].bytes_read
    );
    assert_eq!(
        rec_fp.vfs_reads[0].request_id,
        orig_fp.vfs_reads[0].request_id
    );
    assert_eq!(
        rec_fp.shared_load_reuses.len(),
        orig_fp.shared_load_reuses.len()
    );
    assert_eq!(
        rec_fp.shared_load_reuses[0].winner_request_id,
        orig_fp.shared_load_reuses[0].winner_request_id,
    );
    assert_eq!(
        rec_fp.shared_load_reuses[0].winner_audited,
        orig_fp.shared_load_reuses[0].winner_audited,
    );
    assert_eq!(
        rec_fp.indexed_ready_builds.len(),
        orig_fp.indexed_ready_builds.len(),
    );
    assert_eq!(
        rec_fp.indexed_ready_builds[0].whole_hash,
        orig_fp.indexed_ready_builds[0].whole_hash,
    );
    assert_eq!(rec_fp.instantiations.len(), orig_fp.instantiations.len());
    assert_eq!(
        rec_fp.instantiations[0].decl_canonical_id,
        orig_fp.instantiations[0].decl_canonical_id,
    );
    assert_eq!(
        rec_fp.instantiations[0].args_fingerprint,
        orig_fp.instantiations[0].args_fingerprint,
    );
    assert_eq!(
        rec_fp.derivation_subgraph.nodes.len(),
        orig_fp.derivation_subgraph.nodes.len(),
    );
    assert_eq!(
        rec_fp.derivation_subgraph.edges.len(),
        orig_fp.derivation_subgraph.edges.len(),
    );
}

#[test]
fn audit_generated_ts_uses_string_for_every_u64_field() {
    // Grep-based regression guard: every known-u64 field in the
    // audit schema must appear typed as `string` in
    // `audit.generated.ts`, never `number` or `bigint`. Plan §1.4.
    let contents = regenerate_audit_bindings_into_tempdir();

    // (field_name, enclosing_type_hint) for every u64 field that
    // MUST be `: string` in the generated TS. The list is derived
    // from the Commit 6.C pre-flight grep against
    // `crates/verter_session/src/component_meta_audit/**`.
    let u64_fields: &[(&str, &str)] = &[
        ("request_id", "RequestAuditRecord"),
        ("total_resolve_steps", "ComponentMetaPayload"),
        ("imported_dependency_bytes", "RequestStoreAudit"),
        ("process_rss_before_bytes", "RequestMemoryAudit"),
        ("process_rss_after_bytes", "RequestMemoryAudit"),
        ("host_cache_before_bytes", "RequestMemoryAudit"),
        ("host_cache_after_bytes", "RequestMemoryAudit"),
        ("workspace_before_bytes", "RequestMemoryAudit"),
        ("workspace_after_bytes", "RequestMemoryAudit"),
        ("bytes_read", "VfsReadRecord | StructuredAuditEvent::VfsRead"),
        (
            "winner_request_id",
            "SharedLoadReuseRecord | OriginEdgeMetaDto::SharedLoadReuse | StructuredAuditEvent::SharedLoadReuse",
        ),
        ("duration_ns", "StructuredAuditEvent::Dispatch/Materialize/CurrentEvalState"),
    ];

    for (name, location) in u64_fields {
        let bigint_pat = format!("{name}: bigint");
        let number_pat_strict = format!("{name}: number");
        assert!(
            !contents.contains(&bigint_pat),
            "`{name}` (location: {location}) still types as `bigint` — the audit transport contract requires \
             `string` via `#[serde(with = \"crate::u64_as_decimal_string\")] #[ts(type = \"string\")]`"
        );
        assert!(
            !contents.contains(&number_pat_strict),
            "`{name}` (location: {location}) still types as `number` — the audit transport contract requires \
             `string` via `#[serde(with = \"crate::u64_as_decimal_string\")] #[ts(type = \"string\")]`"
        );
        // Positive assertion: somewhere in the file, `<name>: string` appears.
        let string_pat = format!("{name}: string");
        assert!(
            contents.contains(&string_pat),
            "expected `{name}: string` to appear in audit.generated.ts (location: {location})"
        );
    }
}

/// Read the git-committed baseline of `path` via `git show HEAD:<rel>`.
/// Returns `None` when git is unavailable, the path is untracked, or
/// the read otherwise fails.
fn read_git_committed_baseline(path: &std::path::Path) -> Option<String> {
    let root = workspace_root();
    let rel = path
        .strip_prefix(&root)
        .ok()?
        .to_string_lossy()
        .into_owned();
    let rel_unix = rel.replace('\\', "/");
    let output = std::process::Command::new("git")
        .arg("show")
        .arg(format!("HEAD:{rel_unix}"))
        .current_dir(&root)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8(output.stdout).ok()
}
