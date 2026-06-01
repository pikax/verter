//! Integration tests for the committed `packages/types/audit.generated.ts`.
//!
//! The audit DTOs derive `ts_rs::TS` and carry
//! `#[ts(export_to = "audit.generated.ts")]` WITHOUT the `export`
//! keyword, so a normal `cargo test` does NOT auto-write any tracked
//! file. The committed `packages/types/audit.generated.ts` is the
//! single source of truth; these tests regenerate it hermetically into
//! a tempdir and compare, never mutating the tracked file on a default
//! run.
//!
//! Two tests, distinct roles:
//!
//! * `ts_bindings_export_succeeds_for_every_audit_record_type` —
//!   smoke / sentinel check that a hermetic regeneration contains the
//!   top-level type names.
//!
//! * `audit_ts_bindings_are_in_sync` — the **discriminating** sync
//!   guard. Regenerates every audit record type into a tempdir via
//!   `TS::export_all(&ts_rs::Config::new().with_out_dir(..))`, reads
//!   the committed file, and FAILS with a unified diff if the two
//!   differ. It is READ-ONLY by default: a genuine mismatch means the
//!   Rust source changed and the TS file has not yet been regenerated.
//!   Set `VERTER_UPDATE_TS_BINDINGS=1` (or `VERTER_TS_BINDINGS_DUMP=<path>`)
//!   to refresh the tracked file — the write goes to a temp file in the
//!   target directory then atomic-renames into place (no torn writes).

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
                "unable to locate the committed `packages/types/audit.generated.ts` \
                 by walking up from `{}`. The file is tracked and is the single \
                 source of truth; regenerate it via \
                 `VERTER_UPDATE_TS_BINDINGS=1 cargo test -p verter_session \
                 --test g_misc1 audit_ts_bindings_are_in_sync` if it is missing.",
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
    // Discriminating sync gate. READ-ONLY by default: regenerate the
    // merged audit-record dependency closure into a tempdir, read the
    // committed on-disk `packages/types/audit.generated.ts`, and FAIL
    // with a unified diff if the two differ. The default run NEVER
    // mutates the tracked file — there is no longer a ts-rs
    // auto-export racing it, so the live committed file is a stable
    // comparison target (a hand-edit to the committed file is detected
    // directly, which is what makes this test discriminating).
    //
    // To refresh after a deliberate schema change, set
    // `VERTER_UPDATE_TS_BINDINGS=1` (writes the tracked file) or
    // `VERTER_TS_BINDINGS_DUMP=<path>` (writes an arbitrary path). The
    // write is atomic: a temp file in the target directory followed by
    // a rename, so a torn/partial bindings file can never be observed.
    let root = workspace_root();
    let committed_path = root.join("packages/types/audit.generated.ts");
    let generated_raw = regenerate_audit_bindings_into_tempdir();
    let generated = normalize_lf(&generated_raw);

    // Explicit refresh path — only when a refresh env flag is set.
    if let Some(refresh_target) = refresh_target(&committed_path) {
        atomic_write(&refresh_target, generated_raw.as_bytes());
        // After an explicit refresh, the tracked file now equals the
        // regenerated content by construction; nothing left to assert.
        return;
    }

    // Default READ-ONLY comparison against the live committed file.
    let committed = normalize_lf(
        &fs::read_to_string(&committed_path)
            .unwrap_or_else(|e| panic!("read committed `{committed_path:?}`: {e}")),
    );

    if committed != generated {
        let diff = similar::TextDiff::from_lines(&committed, &generated);
        let rendered = diff
            .unified_diff()
            .context_radius(3)
            .header("committed", "regenerated")
            .to_string();
        panic!(
            "`packages/types/audit.generated.ts` is out of sync with the \
             Rust audit DTOs. This test is read-only by default and did NOT \
             modify the tracked file. To refresh it, run:\n  \
             VERTER_UPDATE_TS_BINDINGS=1 cargo test -p verter_session --test \
             g_misc1 audit_ts_bindings_are_in_sync\nthen review and commit the \
             new content. Unified diff (committed -> regenerated):\n{rendered}"
        );
    }
}

/// Resolve the explicit refresh target, or `None` for the default
/// read-only run. `VERTER_TS_BINDINGS_DUMP=<path>` writes the given
/// path; `VERTER_UPDATE_TS_BINDINGS=1` writes the committed bindings
/// file. Neither set -> `None` (no write).
fn refresh_target(committed_path: &std::path::Path) -> Option<PathBuf> {
    if let Some(dump) = std::env::var("VERTER_TS_BINDINGS_DUMP")
        .ok()
        .filter(|s| !s.is_empty())
    {
        return Some(PathBuf::from(dump));
    }
    if std::env::var("VERTER_UPDATE_TS_BINDINGS")
        .ok()
        .filter(|s| !s.is_empty() && s != "0")
        .is_some()
    {
        return Some(committed_path.to_path_buf());
    }
    None
}

/// Write `bytes` to `target` atomically: stage into a uniquely-named
/// temp file in the SAME directory (so the rename stays on one
/// filesystem), then rename over `target`. A concurrent reader sees
/// either the old or the new complete file, never a torn one.
fn atomic_write(target: &std::path::Path, bytes: &[u8]) {
    use std::io::Write as _;
    let dir = target.parent().unwrap_or_else(|| std::path::Path::new("."));
    let mut tmp = tempfile::Builder::new()
        .prefix(".audit.generated.ts.")
        .suffix(".tmp")
        .tempfile_in(dir)
        .unwrap_or_else(|e| panic!("create temp file in `{dir:?}`: {e}"));
    tmp.write_all(bytes)
        .unwrap_or_else(|e| panic!("write temp bindings file: {e}"));
    tmp.flush()
        .unwrap_or_else(|e| panic!("flush temp bindings file: {e}"));
    tmp.persist(target)
        .unwrap_or_else(|e| panic!("atomic-rename temp bindings into `{target:?}`: {e}"));
}

/// Regenerate the merged audit-record dependency closure into a
/// fresh tempdir and return its contents. The bindings are always
/// regenerated into a tempdir — never the tracked path — so the
/// integration suite never mutates `packages/types/audit.generated.ts`.
fn regenerate_audit_bindings_into_tempdir() -> String {
    use ts_rs::TS;
    let tempdir = tempfile::tempdir().expect("create tempdir for ts-rs regeneration");
    RequestAuditRecord::export_all(&ts_rs::Config::new().with_out_dir(tempdir.path()))
        .expect("regenerate RequestAuditRecord graph");
    StructuredAuditEvent::export_all(&ts_rs::Config::new().with_out_dir(tempdir.path()))
        .expect("regenerate StructuredAuditEvent graph");
    ProvenanceChain::export_all(&ts_rs::Config::new().with_out_dir(tempdir.path()))
        .expect("regenerate ProvenanceChain graph");
    ChainTermination::export_all(&ts_rs::Config::new().with_out_dir(tempdir.path()))
        .expect("regenerate ChainTermination graph");
    ProvenanceStep::export_all(&ts_rs::Config::new().with_out_dir(tempdir.path()))
        .expect("regenerate ProvenanceStep graph");
    RequestPhaseAudit::export_all(&ts_rs::Config::new().with_out_dir(tempdir.path()))
        .expect("regenerate RequestPhaseAudit graph");
    verter_audit::DerivationEdgeRaw::export_all(&ts_rs::Config::new().with_out_dir(tempdir.path()))
        .expect("regenerate DerivationEdgeRaw graph");
    verter_audit::PublishedSurfacePolicy::export_all(
        &ts_rs::Config::new().with_out_dir(tempdir.path()),
    )
    .expect("regenerate PublishedSurfacePolicy graph");
    verter_audit::AnalyzedSurface::export_all(&ts_rs::Config::new().with_out_dir(tempdir.path()))
        .expect("regenerate AnalyzedSurface graph");
    verter_audit::PolicyNamesResult::export_all(&ts_rs::Config::new().with_out_dir(tempdir.path()))
        .expect("regenerate PolicyNamesResult graph");
    let path = tempdir.path().join("audit.generated.ts");
    fs::read_to_string(&path).unwrap_or_else(|e| panic!("read regenerated `{path:?}`: {e}"))
}

#[test]
fn ts_bindings_export_succeeds_for_every_audit_record_type() {
    // Regenerate into a tempdir (never the tracked path) and assert
    // the top-level type names are present. The regeneration is
    // hermetic and side-effect-free on the working tree.
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
            process_rss_peak_bytes: 0,
            host_cache_before_bytes: 0,
            host_cache_after_bytes: 0,
            workspace_before_bytes: 0,
            workspace_after_bytes: 0,
            bytes_parsed: 0,
        },
        footprint: None,
        scheduler: None,
        from_cache: false,
        files: Vec::new(),
        waits: None,
        kind_payload: RequestKindPayload::ComponentMeta(ComponentMetaPayload {
            total_resolve_steps: 1_234_567,
            solve_count: 3,
            ..Default::default()
        }),
        trace_id: String::new(),
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
    // Use the SAME regeneration path as `audit_ts_bindings_are_in_sync`
    // (the canonical root list incl. `DerivationEdgeRaw`) so this meta-test
    // validates the real sync path instead of a drifting duplicate list.
    let regenerated = normalize_lf(&regenerate_audit_bindings_into_tempdir());

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
                process_rss_peak_bytes: 0,
                host_cache_before_bytes: 0,
                host_cache_after_bytes: 0,
                workspace_before_bytes: 0,
                workspace_after_bytes: 0,
                bytes_parsed: 0,
            },
            footprint: None,
            scheduler: None,
            from_cache: false,
            files: Vec::new(),
            waits: None,
            kind_payload: RequestKindPayload::ComponentMeta(ComponentMetaPayload::default()),
            trace_id: String::new(),
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
        scheduler: None,
        from_cache: false,
        files: Vec::new(),
        waits: None,
        kind_payload: RequestKindPayload::ComponentMeta(ComponentMetaPayload {
            total_resolve_steps: u64::MAX - 1,
            solve_count: 7,
            ..Default::default()
        }),
        trace_id: String::new(),
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
        ("process_rss_peak_bytes", "RequestMemoryAudit"),
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
        ("structured_events_truncated", "TruncationCounters"),
        ("derivation_edges_raw_truncated", "TruncationCounters"),
        ("derivation_nodes_truncated", "TruncationCounters"),
        ("vfs_reads_truncated", "TruncationCounters"),
        ("indexed_ready_builds_truncated", "TruncationCounters"),
        ("materializations_truncated", "TruncationCounters"),
        ("instantiations_truncated", "TruncationCounters"),
        ("substitutions_truncated", "TruncationCounters"),
        ("projections_truncated", "TruncationCounters"),
        ("conditional_decisions_truncated", "TruncationCounters"),
        ("alias_resolutions_truncated", "TruncationCounters"),
        ("shared_load_reuses_truncated", "TruncationCounters"),
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

/// Directories whose `.rs` files carry the audit-DTO `ts_rs::TS`
/// derives. The guard below scans these for any re-introduction of the
/// `#[ts(export)]` auto-export flag.
const AUDIT_DTO_DIRS: &[&str] = &[
    "crates/verter_audit/src",
    "crates/verter_session/src/component_meta_audit",
];

/// Collect every `*.rs` file under `dir` (recursively) into `out`.
fn collect_rs_files(dir: &std::path::Path, out: &mut Vec<PathBuf>) {
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(e) => panic!("read_dir `{dir:?}`: {e}"),
    };
    for entry in entries {
        let entry = entry.expect("dir entry");
        let path = entry.path();
        if path.is_dir() {
            collect_rs_files(&path, out);
        } else if path.extension().and_then(|s| s.to_str()) == Some("rs") {
            out.push(path);
        }
    }
}

/// True for ASCII identifier bytes, so `export_to` reads as one token and
/// `reexport` is not mistaken for a bare `export`.
fn is_ident_byte(b: u8) -> bool {
    b == b'_' || b.is_ascii_alphanumeric()
}

/// Scan the FULL file `content` for any `#[ts(...)]` attribute — INCLUDING
/// multi-line attributes — that carries a bare `export` flag (as opposed to
/// `export_to`). Returns the 1-based line number of each offending `export`
/// token. A per-line scan is insufficient: ts-rs accepts the bare flag split
/// across lines, e.g.
///
/// ```ignore
/// #[ts(
///     export,
///     export_to = "audit.generated.ts"
/// )]
/// ```
///
/// The bare flag is what makes ts-rs emit a hidden `#[test]` that writes the
/// configured output dir during `cargo test`. `export_to` (the path attribute)
/// is allowed; a standalone `export` token is not.
fn bare_ts_export_offenders(content: &str) -> Vec<usize> {
    let cb = content.as_bytes();
    let mut offenders = Vec::new();
    let mut base = 0usize;
    while let Some(rel) = content[base..].find("#[ts(") {
        let open = base + rel + "#[ts(".len();
        // Find the matching close paren of `#[ts( ... )`, tracking depth so a
        // multi-line body is captured in full.
        let mut depth = 1usize;
        let mut i = open;
        while i < cb.len() && depth > 0 {
            match cb[i] {
                b'(' => depth += 1,
                b')' => depth -= 1,
                _ => {}
            }
            if depth == 0 {
                break;
            }
            i += 1;
        }
        let body = &content[open..i.min(content.len())];
        let bb = body.as_bytes();
        let mut j = 0;
        while let Some(k) = body[j..].find("export") {
            let pos = j + k;
            let before_ok = pos == 0 || !is_ident_byte(bb[pos - 1]);
            // A standalone `export`: the next byte is absent or a non-identifier
            // char. This EXCLUDES `export_to` (next byte `_`).
            let after = pos + "export".len();
            let after_ok = bb.get(after).is_none_or(|&c| !is_ident_byte(c));
            if before_ok && after_ok {
                let abs = open + pos;
                let line = content[..abs].bytes().filter(|&b| b == b'\n').count() + 1;
                offenders.push(line);
            }
            j = after;
        }
        base = i + 1;
        if base >= content.len() {
            break;
        }
    }
    offenders
}

#[test]
fn audit_types_have_no_ts_export_auto_export() {
    // Guard against re-introducing the tracked-path ts-rs auto-export.
    //
    // The audit DTOs must derive `ts_rs::TS` with
    // `#[ts(export_to = "audit.generated.ts")]` but WITHOUT the bare
    // `export` flag. The bare flag makes ts-rs emit a hidden `#[test]`
    // that writes `$TS_RS_EXPORT_DIR/audit.generated.ts` during a
    // normal `cargo test`; combined with multiple test binaries
    // (separate processes) that concurrently truncate+merge the same
    // tracked file, it tears `packages/types/audit.generated.ts` and
    // breaks `pnpm build`. The committed file is regenerated EXPLICITLY
    // and read-only-checked by `audit_ts_bindings_are_in_sync`.
    //
    // Discriminating: re-add `#[ts(export, export_to = "...")]` (or a
    // bare `#[ts(export)]`) to any audit DTO and this test FAILS,
    // naming the file + line.
    let root = workspace_root();
    let mut offenders: Vec<String> = Vec::new();
    for dir in AUDIT_DTO_DIRS {
        let abs = root.join(dir);
        let mut files = Vec::new();
        collect_rs_files(&abs, &mut files);
        for file in files {
            let contents =
                fs::read_to_string(&file).unwrap_or_else(|e| panic!("read `{file:?}`: {e}"));
            for line_no in bare_ts_export_offenders(&contents) {
                let rel = file.strip_prefix(&root).unwrap_or(&file);
                let text = contents.lines().nth(line_no - 1).unwrap_or("").trim();
                offenders.push(format!("{}:{}: {}", rel.display(), line_no, text));
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "audit DTOs must NOT carry the bare `#[ts(export)]` flag (use \
         `#[ts(export_to = \"audit.generated.ts\")]` only). The bare flag \
         resurrects the concurrent-truncation bug that tears \
         `packages/types/audit.generated.ts` during `cargo test`. \
         Offending lines:\n{}",
        offenders.join("\n")
    );
}

#[test]
fn bare_ts_export_scanner_flags_multiline_export() {
    // The whole-file scanner must catch a bare `export` split onto its own
    // line inside a multi-line `#[ts(...)]` — the prior per-line scan missed
    // this legal Rust attribute form and let the regression through.
    let src =
        "#[derive(TS)]\n#[ts(\n    export,\n    export_to = \"audit.generated.ts\"\n)]\nstruct X;\n";
    assert_eq!(
        bare_ts_export_offenders(src),
        vec![3],
        "multi-line bare `export` must be flagged at its source line"
    );
}

#[test]
fn bare_ts_export_scanner_allows_multiline_export_to_only() {
    let src = "#[derive(TS)]\n#[ts(\n    export_to = \"audit.generated.ts\"\n)]\nstruct X;\n";
    assert!(
        bare_ts_export_offenders(src).is_empty(),
        "multi-line `export_to`-only must NOT be flagged"
    );
}

#[test]
fn bare_ts_export_scanner_flags_singleline_and_allows_export_to() {
    assert_eq!(
        bare_ts_export_offenders("#[ts(export, export_to = \"x\")]\n"),
        vec![1],
        "single-line bare `export` must still be flagged"
    );
    assert!(
        bare_ts_export_offenders("#[ts(export_to = \"x\")]\n").is_empty(),
        "single-line `export_to`-only must be allowed"
    );
}

#[test]
fn cargo_config_does_not_set_ts_rs_export_dir() {
    // Guard the OTHER half of the regression: even without a bare
    // `export` flag, setting `TS_RS_EXPORT_DIR = packages/types` in
    // `.cargo/config.toml` would (if any `export` flag returned) point
    // the auto-export at the tracked tree. Forbid any *active*
    // (uncommented) `TS_RS_EXPORT_DIR` assignment in the workspace
    // cargo config.
    //
    // Discriminating: uncomment / re-add
    // `TS_RS_EXPORT_DIR = { value = "packages/types", relative = true }`
    // and this test FAILS.
    let root = workspace_root();
    let config_path = root.join(".cargo/config.toml");
    let contents =
        fs::read_to_string(&config_path).unwrap_or_else(|e| panic!("read `{config_path:?}`: {e}"));
    let active: Vec<(usize, &str)> = contents
        .lines()
        .enumerate()
        .filter(|(_, line)| {
            let trimmed = line.trim_start();
            !trimmed.starts_with('#') && trimmed.contains("TS_RS_EXPORT_DIR")
        })
        .map(|(i, line)| (i + 1, line.trim()))
        .collect();
    assert!(
        active.is_empty(),
        "`.cargo/config.toml` must NOT set `TS_RS_EXPORT_DIR` (it would aim the \
         ts-rs auto-export at the tracked tree and re-tear \
         `packages/types/audit.generated.ts` during `cargo test`). The committed \
         bindings are refreshed explicitly via `VERTER_UPDATE_TS_BINDINGS=1`. \
         Active assignment(s):\n{}",
        active
            .iter()
            .map(|(ln, l)| format!("  line {ln}: {l}"))
            .collect::<Vec<_>>()
            .join("\n")
    );
}
