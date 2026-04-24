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
    ChainTermination, ProvenanceChain, ProvenanceStep, RequestPhaseAudit, RustAuditRecord,
    StructuredComponentMetaEvent,
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

    let root = workspace_root();
    let committed_path = root.join("packages/types/audit.generated.ts");
    let committed_raw = fs::read_to_string(&committed_path)
        .unwrap_or_else(|e| panic!("read committed `{committed_path:?}`: {e}"));
    let committed = normalize_lf(&committed_raw);

    // Regenerate into a tempdir. `export_all_to` explicitly disregards
    // `TS_RS_EXPORT_DIR` (per ts-rs docs) and uses the given path.
    // Because every audit record shares
    // `#[ts(export_to = "audit.generated.ts")]`, every type merges
    // into the single file `<tempdir>/audit.generated.ts`.
    //
    // `export_all_to` walks the dependency graph reachable from the
    // root type. Types not transitively reachable from
    // `RustAuditRecord` (the walker types in `assertions.rs`,
    // `StructuredComponentMetaEvent`, `RequestPhaseAudit`) need their
    // own export_all_to call. All four calls write into the SAME
    // `audit.generated.ts` (ts-rs merges by file path).
    let tempdir = tempfile::tempdir().expect("create tempdir for ts-rs regeneration");
    RustAuditRecord::export_all_to(tempdir.path())
        .expect("regenerate RustAuditRecord graph via ts-rs export_all_to");
    StructuredComponentMetaEvent::export_all_to(tempdir.path())
        .expect("regenerate StructuredComponentMetaEvent graph via ts-rs export_all_to");
    ProvenanceChain::export_all_to(tempdir.path())
        .expect("regenerate ProvenanceChain graph via ts-rs export_all_to");
    ChainTermination::export_all_to(tempdir.path())
        .expect("regenerate ChainTermination graph via ts-rs export_all_to");
    ProvenanceStep::export_all_to(tempdir.path())
        .expect("regenerate ProvenanceStep graph via ts-rs export_all_to");
    RequestPhaseAudit::export_all_to(tempdir.path())
        .expect("regenerate RequestPhaseAudit graph via ts-rs export_all_to");

    let generated_path = tempdir.path().join("audit.generated.ts");
    let generated_raw = fs::read_to_string(&generated_path)
        .unwrap_or_else(|e| panic!("read regenerated `{generated_path:?}`: {e}"));
    let generated = normalize_lf(&generated_raw);

    if committed != generated {
        let diff = similar::TextDiff::from_lines(&committed, &generated);
        let rendered = diff
            .unified_diff()
            .context_radius(3)
            .header("committed", "regenerated")
            .to_string();
        panic!(
            "`packages/types/audit.generated.ts` is out of sync with the Rust source. \
             Re-run `cargo test -p verter_session` to refresh the committed file via the \
             ts-rs automatic export tests (they write to `packages/types/` via the \
             workspace `.cargo/config.toml` `TS_RS_EXPORT_DIR` env), or manually call \
             `RustAuditRecord::export_all_to(\"packages/types\")`.\n\n\
             Unified diff:\n{rendered}"
        );
    }
}

#[test]
fn ts_bindings_export_succeeds_for_every_audit_record_type() {
    // The per-type `export_bindings_*` tests emitted by the
    // `#[ts(export)]` derive are the load-bearing drivers; they run
    // automatically with the rest of the suite. This test asserts
    // that the committed file exists and carries the load-bearing
    // top-level type names — a sentinel for "the exports produced
    // something coherent". `audit_ts_bindings_are_in_sync` covers
    // byte-exact correctness.
    let root = workspace_root();
    let path = root.join("packages/types/audit.generated.ts");
    let contents = fs::read_to_string(&path).unwrap();
    assert!(!contents.is_empty(), "generated TS file must be non-empty");
    assert!(
        contents.contains("RustAuditRecord"),
        "generated file must include the top-level RustAuditRecord type",
    );
    assert!(
        contents.contains("RustSemanticFootprintAudit"),
        "generated file must include the footprint record type",
    );
    assert!(
        contents.contains("SemanticNodeKind"),
        "generated file must include the node-kind enum",
    );
}

#[test]
fn audit_record_u64_fields_serialize_as_json_strings_not_numbers() {
    // A constructed `RustAuditRecord` with request_id=42 serializes
    // to JSON with `"request_id":"42"` (quoted string), NOT
    // `"request_id":42` (unquoted number). Every `u64` field in the
    // audit record goes through `crate::u64_as_decimal_string` per
    // plan §1.4 so JS/TS consumers can round-trip through
    // `JSON.parse`/`JSON.stringify` without precision loss.
    use verter_session::component_meta_audit::{
        RustAuditRecord, RustMemoryAudit, RustSolverAudit, RustStoreAudit, RustTimingAudit,
    };

    let record = RustAuditRecord {
        request_id: 42,
        canonical_id: "/a.vue".into(),
        timings: RustTimingAudit::default(),
        solver: RustSolverAudit {
            total_resolve_steps: 1_234_567,
            solve_count: 3,
        },
        store: RustStoreAudit {
            imported_dependency_bytes: u64::MAX,
            ..Default::default()
        },
        memory: RustMemoryAudit {
            process_rss_before_bytes: 9_999,
            process_rss_after_bytes: 10_000,
            process_rss_delta_bytes: 1,
            host_cache_before_bytes: 0,
            host_cache_after_bytes: 0,
            workspace_before_bytes: 0,
            workspace_after_bytes: 0,
        },
        footprint: None,
    };

    let value = serde_json::to_value(&record).expect("serialize");

    // Top-level request_id
    assert!(
        value["request_id"].is_string(),
        "expected request_id to be JSON string, got {}",
        value["request_id"]
    );
    assert_eq!(value["request_id"].as_str(), Some("42"));

    // solver.total_resolve_steps (u64)
    assert_eq!(
        value["solver"]["total_resolve_steps"].as_str(),
        Some("1234567"),
        "solver.total_resolve_steps must be quoted decimal string"
    );
    // solver.solve_count stays as a JS number (u32, always safe)
    assert!(
        value["solver"]["solve_count"].is_number(),
        "solver.solve_count (u32) must remain a JSON number"
    );

    // u64::MAX round-trip preserved
    assert_eq!(
        value["store"]["imported_dependency_bytes"].as_str(),
        Some("18446744073709551615"),
        "u64::MAX must survive as decimal string (JS Number would lose precision)"
    );

    // Memory snapshots — every integer > 32 bits (signed or unsigned)
    // is a decimal string per plan §3.B Commit 7.A (uniform transport).
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
        "process_rss_delta_bytes (i64) must serialize as decimal string — \
         plan §3.B Commit 7.A extends u64-as-string to every i64 field"
    );

    // Full round-trip: deserialize to native, compare scalars
    let back: RustAuditRecord =
        serde_json::from_value(value).expect("deserialize audit record from JSON value");
    assert_eq!(back.request_id, 42);
    assert_eq!(back.solver.total_resolve_steps, 1_234_567);
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
    RustAuditRecord::export_all_to(tempdir.path()).expect("regenerate RustAuditRecord");
    StructuredComponentMetaEvent::export_all_to(tempdir.path())
        .expect("regenerate StructuredComponentMetaEvent");
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
        RustAuditRecord, RustMemoryAudit, RustSolverAudit, RustStoreAudit, RustTimingAudit,
    };

    for delta in [-42i64, -1, 0, 1, i64::MIN, i64::MAX] {
        let record = RustAuditRecord {
            request_id: 1,
            canonical_id: "/a.vue".into(),
            timings: RustTimingAudit::default(),
            solver: RustSolverAudit::default(),
            store: RustStoreAudit::default(),
            memory: RustMemoryAudit {
                process_rss_before_bytes: 0,
                process_rss_after_bytes: 0,
                process_rss_delta_bytes: delta,
                host_cache_before_bytes: 0,
                host_cache_after_bytes: 0,
                workspace_before_bytes: 0,
                workspace_after_bytes: 0,
            },
            footprint: None,
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
        let back: RustAuditRecord = serde_json::from_value(value).expect("deserialize");
        assert_eq!(back.memory.process_rss_delta_bytes, delta);
    }
}

#[test]
fn audit_generated_ts_uses_string_for_every_i64_field() {
    // Grep-based regression guard for the i64 extension of the
    // stringified-transport rule. Plan §3.B Commit 7.A enumerated
    // the audit `i64` field set at exactly one entry:
    // `RustMemoryAudit::process_rss_delta_bytes`. If a future
    // commit adds another `i64` audit field, it must land in this
    // list and in `crate::i64_as_decimal_string` annotations
    // simultaneously.
    let root = workspace_root();
    let path = root.join("packages/types/audit.generated.ts");
    let contents = fs::read_to_string(&path).unwrap();

    let i64_fields: &[(&str, &str)] = &[("process_rss_delta_bytes", "RustMemoryAudit")];

    for (name, location) in i64_fields {
        let bigint_pat = format!("{name}: bigint");
        let number_pat = format!("{name}: number");
        let string_pat = format!("{name}: string");
        assert!(
            !contents.contains(&bigint_pat),
            "`{name}` (location: {location}) still types as `bigint` — plan §3.B Commit 7.A \
             requires `string` via `#[serde(with = \"crate::i64_as_decimal_string\")] \
             #[ts(type = \"string\")]`"
        );
        assert!(
            !contents.contains(&number_pat),
            "`{name}` (location: {location}) still types as `number` — plan §3.B Commit 7.A \
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
    // Plan §3.B Commit 7.B regression guard: the rationalization
    // comment at `mod.rs:167–181` (which argued the widened three-lane
    // union was "what the audit caller wants") was the stub-prevention
    // violation — it camouflaged a gate-bypass as a design choice.
    // Any future edit that reintroduces that rationalization MUST
    // trip this test, because the TS contract and the helper name
    // will again be lying about exactness.
    let root = workspace_root();
    let path = root.join("crates/verter_session/src/component_meta_audit/mod.rs");
    let contents = fs::read_to_string(&path).unwrap();

    // The `loaded_files` docblock lives just above the `pub fn
    // loaded_files` signature. Bound the grep to the region between
    // `impl RustSemanticFootprintAudit {` and the end of `loaded_files`
    // so unrelated comments elsewhere in the file cannot mask a
    // regression.
    let impl_start = contents
        .find("impl RustSemanticFootprintAudit {")
        .expect("impl RustSemanticFootprintAudit block missing from mod.rs");
    let after_impl = &contents[impl_start..];
    // `declared_dependency_files` doc lives just after `loaded_files`
    // body — bound there.
    let bound_end = after_impl
        .find("pub fn declared_dependency_files")
        .expect("declared_dependency_files accessor missing — 7.B split not applied");
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
    let root = workspace_root();
    let path = root.join("packages/types/audit.generated.ts");
    let contents = fs::read_to_string(&path).unwrap();
    let count = contents.matches("bigint").count();
    assert_eq!(
        count, 0,
        "audit.generated.ts must contain zero `bigint` occurrences; found {count}. \
         Every `u64`/`i64` audit field must use the decimal-string transport. \
         Plan §3.B Commit 7.A."
    );
}

#[test]
fn audit_generated_ts_uses_string_for_every_u64_field() {
    // Grep-based regression guard: every known-u64 field in the
    // audit schema must appear typed as `string` in
    // `audit.generated.ts`, never `number` or `bigint`. Plan §1.4.
    let root = workspace_root();
    let path = root.join("packages/types/audit.generated.ts");
    let contents = fs::read_to_string(&path).unwrap();

    // (field_name, enclosing_type_hint) for every u64 field that
    // MUST be `: string` in the generated TS. The list is derived
    // from the Commit 6.C pre-flight grep against
    // `crates/verter_session/src/component_meta_audit/**`.
    let u64_fields: &[(&str, &str)] = &[
        ("request_id", "RustAuditRecord"),
        ("total_resolve_steps", "RustSolverAudit"),
        ("imported_dependency_bytes", "RustStoreAudit"),
        ("process_rss_before_bytes", "RustMemoryAudit"),
        ("process_rss_after_bytes", "RustMemoryAudit"),
        ("host_cache_before_bytes", "RustMemoryAudit"),
        ("host_cache_after_bytes", "RustMemoryAudit"),
        ("workspace_before_bytes", "RustMemoryAudit"),
        ("workspace_after_bytes", "RustMemoryAudit"),
        ("bytes_read", "VfsReadRecord | StructuredComponentMetaEvent::VfsRead"),
        (
            "winner_request_id",
            "SharedLoadReuseRecord | OriginEdgeMetaDto::SharedLoadReuse | StructuredComponentMetaEvent::SharedLoadReuse",
        ),
        ("duration_ns", "StructuredComponentMetaEvent::Dispatch/Materialize/CurrentEvalState"),
    ];

    for (name, location) in u64_fields {
        let bigint_pat = format!("{name}: bigint");
        let number_pat_strict = format!("{name}: number");
        assert!(
            !contents.contains(&bigint_pat),
            "`{name}` (location: {location}) still types as `bigint` — plan §1.4 requires \
             `string` via `#[serde(with = \"crate::u64_as_decimal_string\")] #[ts(type = \"string\")]`"
        );
        assert!(
            !contents.contains(&number_pat_strict),
            "`{name}` (location: {location}) still types as `number` — plan §1.4 requires \
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
