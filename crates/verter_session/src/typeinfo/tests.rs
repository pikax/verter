//! Inline tests for the host typeinfo substrate. Cover the
//! discriminating invariants for each public method:
//!
//! - `list_file_symbols` — top-level types / values, exporter flag,
//!   absent-imported-symbols, performance.
//! - `resolve_named_symbol_with_audit` — Identity / Navigate /
//!   Expanded / generic-default-Navigate / non-generic-default-Expanded
//!   plus exactly-one-record + trace_id propagation.
//! - `evaluate_type_expression_with_audit` — primitive / object /
//!   extra-imports / scratch-URI-determinism / scratch-URI-scope-distinct
//!   / cache-hit / cache-bypass / LRU-eviction / one-record / trace_id.
//!
//! Tests follow the project pattern from
//! `tests/cases/g_type/type_resolution_audit_diamond_repeated_prop.rs`.

use std::sync::Arc;

use verter_audit::{RequestKind, RequestKindPayload};
use verter_type_expr::{PrimitiveName, TypeExpr};

use super::types::{EvaluateTypeExpressionRequest, ImportSpec, NamedImport, SymbolKind};
use crate::semantic_query::{ProjectionMode, SemanticNodeData};
use crate::types::{HostConfig, UpsertRequest};
use crate::VerterHost;

// ---------------------------------------------------------------------------
// Test helpers
// ---------------------------------------------------------------------------

fn make_host_with_audit() -> Arc<VerterHost> {
    Arc::new(VerterHost::new_standalone(HostConfig {
        audit_enabled: true,
        footprint_capture: true,
        ..HostConfig::default()
    }))
}

fn upsert_ts(host: &VerterHost, canonical_id: &str, source: &str) {
    let _ = host.upsert(UpsertRequest {
        canonical_id: Some(canonical_id.to_string()),
        input_id: canonical_id.to_string(),
        source: Arc::from(source),
        file_language: crate::LanguageRegistry::global()
            .classify_static(canonical_id)
            .static_resolution(),
        aliases: Vec::new(),
    });
}

/// Split a typeinfo `AuditedResult<Option<node>, E>` carrier into the
/// `(node, record)` pair the happy-path tests inspect. Both a non-fault
/// miss (`Ok(None)`) and a dispatch fault (`Err`) collapse to `None`
/// here — these helper callers only exercise the success path, so the
/// flatten is benign. The fault-routing tests below use `into_parts()`
/// directly to discriminate `Err` from `Ok(None)`. The audit record is
/// always present.
fn parts<E>(
    carrier: verter_audit::AuditedResult<Option<crate::semantic_query::SemanticNodeId>, E>,
) -> (
    Option<crate::semantic_query::SemanticNodeId>,
    verter_audit::RequestAuditRecord,
) {
    let (outcome, record) = carrier.into_parts();
    (outcome.ok().flatten(), record)
}

fn assert_one_typeresolution_record(
    record: &verter_audit::RequestAuditRecord,
) -> &verter_audit::RequestAuditRecord {
    assert_eq!(record.kind, RequestKind::TypeResolution);
    assert!(
        matches!(record.kind_payload, RequestKindPayload::TypeResolution(_)),
        "kind_payload must be TypeResolution"
    );
    assert_eq!(
        record.capture_state,
        verter_audit::AuditCaptureState::ActiveStored,
        "active TypeResolution request must produce a stored record"
    );
    record
}

fn type_arg_ref(name: &str) -> Arc<TypeExpr> {
    Arc::new(TypeExpr::Ref {
        name: Arc::from(name),
        type_arguments: Arc::from(Vec::new().into_boxed_slice()),
    })
}

fn type_arg_primitive(name: PrimitiveName) -> Arc<TypeExpr> {
    Arc::new(TypeExpr::Primitive(name))
}

// ---------------------------------------------------------------------------
// list_file_symbols
// ---------------------------------------------------------------------------

#[test]
fn list_file_symbols_returns_top_level_types() {
    let host = make_host_with_audit();
    upsert_ts(
        &host,
        "/types.ts",
        r#"
export type AliasFoo = { x: number };
export interface IFoo { a: string }
export class CFoo {}
type Internal = boolean;
"#,
    );

    let symbols = host.list_file_symbols("/types.ts");
    assert!(!symbols.is_empty(), "expected non-empty inventory");

    // Required entries:
    let by_name_kind =
        |name: &str, kind: SymbolKind| symbols.iter().any(|s| s.name == name && s.kind == kind);
    assert!(by_name_kind("AliasFoo", SymbolKind::TypeAlias));
    assert!(by_name_kind("IFoo", SymbolKind::Interface));
    assert!(by_name_kind("CFoo", SymbolKind::Class));
    // Class also surfaces value-side (dual-space).
    assert!(by_name_kind("CFoo", SymbolKind::ClassValue));
    // Internal type is present even though not exported.
    assert!(by_name_kind("Internal", SymbolKind::TypeAlias));

    // Exporter flag — exported symbols must report is_exported.
    let alias = symbols
        .iter()
        .find(|s| s.name == "AliasFoo" && s.kind == SymbolKind::TypeAlias)
        .unwrap();
    assert!(alias.is_exported, "AliasFoo is exported");
    let internal = symbols
        .iter()
        .find(|s| s.name == "Internal" && s.kind == SymbolKind::TypeAlias)
        .unwrap();
    assert!(!internal.is_exported, "Internal is not exported");
}

#[test]
fn list_file_symbols_excludes_imported_symbols() {
    let host = make_host_with_audit();
    upsert_ts(
        &host,
        "/types.ts",
        r#"
export interface External { v: number }
"#,
    );
    upsert_ts(
        &host,
        "/owner.ts",
        r#"
import type { External } from './types';
export type Local = External;
"#,
    );

    let symbols = host.list_file_symbols("/owner.ts");
    let names: Vec<&str> = symbols.iter().map(|s| s.name.as_str()).collect();
    assert!(
        names.contains(&"Local"),
        "owner declares Local; got {names:?}"
    );
    // `External` is imported, not declared — must NOT appear in
    // the inventory of /owner.ts.
    assert!(
        !names.contains(&"External"),
        "imported External must not surface as a declaration on the importing file; got {names:?}"
    );
}

#[test]
fn list_file_symbols_attaches_span_when_analysis_present() {
    let host = make_host_with_audit();
    upsert_ts(
        &host,
        "/types.ts",
        r#"
export interface IFoo { a: string }
"#,
    );

    let symbols = host.list_file_symbols("/types.ts");
    let ifoo = symbols
        .iter()
        .find(|s| s.name == "IFoo" && s.kind == SymbolKind::Interface)
        .expect("IFoo must surface in inventory");
    let span = ifoo
        .span
        .as_ref()
        .expect("IFoo interface declaration must have a span from the analysis snapshot");
    assert!(
        span.start < span.end,
        "span must be non-empty for declared symbol; got {:?}",
        span
    );
}

#[test]
fn list_file_symbols_returns_empty_for_unloaded_file() {
    let host = make_host_with_audit();
    let symbols = host.list_file_symbols("/never_loaded.ts");
    assert!(
        symbols.is_empty(),
        "unloaded file must yield empty inventory; got {} entries",
        symbols.len()
    );
}

/// Number of top-level `export type T{i}` declarations emitted by the
/// 5 KLOC fixture below. Every other line is a `// filler` comment, so
/// this is the EXACT count of listed symbols.
const BIG_TS_SYMBOL_COUNT: usize = 250;

/// Build the ~5 KLOC fixture: `BIG_TS_SYMBOL_COUNT` top-level type
/// aliases, each padded with 19 comment lines (≈ 5_000 lines total).
/// Only the `export type` lines declare symbols; the filler is comments.
fn big_ts_fixture() -> String {
    let mut source = String::with_capacity(5_000);
    for i in 0..BIG_TS_SYMBOL_COUNT {
        source.push_str("export type T");
        source.push_str(&i.to_string());
        source.push_str(" = { value: number };\n");
        // Pad to ~20 lines per type to reach ~5_000 lines.
        for _ in 0..19 {
            source.push_str("// filler line\n");
        }
    }
    source
}

/// The 5 KLOC inventory must be COMPLETE (all 250 declared symbols
/// surface) and WARM (a second `list_file_symbols` call performs zero
/// cold materialisation work — it serves the cached `IndexedReady`
/// snapshot rather than rebuilding it).
///
/// Asserts deterministic completeness + a warm-no-rebuild invariant via
/// provenance counters rather than a wall-clock ceiling, which flakes
/// under nextest process-per-test CPU oversubscription. The wall-clock
/// budget for the 5 KLOC path lives in a `verter_bench` criterion bench.
#[test]
fn list_file_symbols_5kloc_is_complete_and_warm() {
    let host = make_host_with_audit();
    upsert_ts(&host, "/big.ts", &big_ts_fixture());

    // --- Cold pass: first call drives the shared materialisation
    //     pipeline (parse → shallow analysis → IndexedReady). ---
    let before_cold = host.provenance_snapshot();
    let cold = host.list_file_symbols("/big.ts");
    let after_cold = host.provenance_snapshot();

    // Completeness: EXACTLY 250 top-level symbols (the filler lines are
    // comments and contribute nothing). The former `!is_empty()` check
    // passed even if a single symbol surfaced — this pins the full set.
    assert_eq!(
        cold.len(),
        BIG_TS_SYMBOL_COUNT,
        "synthesised 5KLOC file must surface exactly {BIG_TS_SYMBOL_COUNT} top-level symbols",
    );

    // The cold pass must have built the `IndexedReady` snapshot exactly
    // once. `indexed_ready_scheduler_snapshot_reuse` is the cold-work
    // signal `list_file_symbols` actually drives: `ensure_loaded` and the
    // scheduler submission were already paid by `upsert_ts` (the only way
    // a file enters the host), so they stay flat across the read and are
    // NOT discriminating here; the IndexedReady build is. Rooting the
    // warm-delta==0 assertion on this cold delta proves the warm path
    // SKIPS work the cold path provably did — not that the counter is
    // simply never touched.
    let cold_snapshot_builds = after_cold.indexed_ready_scheduler_snapshot_reuse
        - before_cold.indexed_ready_scheduler_snapshot_reuse;
    assert_eq!(
        cold_snapshot_builds, 1,
        "cold list_file_symbols must build the IndexedReady snapshot exactly once \
         (got {cold_snapshot_builds})",
    );

    // --- Warm pass: a second call on the unchanged file must serve the
    //     cached IndexedReady with ZERO cold materialisation work. ---
    let before_warm = host.provenance_snapshot();
    let warm = host.list_file_symbols("/big.ts");
    let after_warm = host.provenance_snapshot();

    // Warm result is still complete.
    assert_eq!(
        warm.len(),
        BIG_TS_SYMBOL_COUNT,
        "warm re-list must still surface exactly {BIG_TS_SYMBOL_COUNT} symbols",
    );
    assert_eq!(
        warm, cold,
        "warm inventory must be identical to the cold inventory",
    );

    // Decisive no-cold-work invariant: the warm call must NOT rebuild the
    // IndexedReady snapshot — it serves the cached artifact through the
    // `ensure_indexed_ready` fast path. A warm path that re-parsed or
    // re-materialised would bump this counter.
    let warm_snapshot_builds = after_warm.indexed_ready_scheduler_snapshot_reuse
        - before_warm.indexed_ready_scheduler_snapshot_reuse;
    assert_eq!(
        warm_snapshot_builds, 0,
        "warm list_file_symbols must NOT rebuild the IndexedReady snapshot \
         (got {warm_snapshot_builds}) — it must reuse the cached artifact",
    );
}

// ---------------------------------------------------------------------------
// resolve_named_symbol_with_audit
// ---------------------------------------------------------------------------

#[test]
fn resolve_named_symbol_identity_returns_alias() {
    let host = make_host_with_audit();
    upsert_ts(
        &host,
        "/aliases.ts",
        r#"
export type Inner = { v: string };
export type Outer = Inner;
"#,
    );

    // First resolve `Outer` in Identity mode. Identity does NOT
    // unwrap aliases — the contract is "do not unwrap + do not
    // expand". Compare against the Navigate / Expanded results to
    // discriminate.
    let (id_node, record) = parts(host.resolve_named_symbol_with_audit(
        "/aliases.ts",
        "Outer",
        Some(ProjectionMode::Identity),
    ));
    let id_node = id_node.expect("Identity must resolve");
    let _ = assert_one_typeresolution_record(&record);

    // Discriminating assertion: Identity returns a NODE that is
    // EITHER (a) `SemanticNodeData::Alias(_)` shell pointing at
    // the unwrapped body, OR (b) the
    // `Opaque(DeclPlaceholder)` carrier the dispatch produces
    // when a name is resolved without materialising its body.
    // Both cases satisfy "do not unwrap". The discriminating
    // negative is that Identity must NOT produce
    // `SemanticNodeData::Object(_)` — that would indicate the
    // body was expanded and the alias was unwrapped, which is
    // exactly what Identity forbids.
    let store = host.project_type_store().semantic_graph();
    let data = store
        .node_data(id_node)
        .expect("node interned during resolution");
    assert!(
        !matches!(data.as_ref(), SemanticNodeData::Object(_)),
        "Identity must NOT expand to a concrete Object body; got {:?}",
        data
    );

    // Discriminate against Navigate / Expanded — those return a
    // *different* node id (the Object body) for the same input,
    // proving Identity's no-unwrap contract holds.
    let (navigate_node, _) = parts(host.resolve_named_symbol_with_audit(
        "/aliases.ts",
        "Outer",
        Some(ProjectionMode::Navigate),
    ));
    let navigate_node = navigate_node.expect("Navigate must resolve");
    assert_ne!(
        id_node, navigate_node,
        "Identity and Navigate must return distinct node ids"
    );
}

#[test]
fn resolve_named_symbol_navigate_unwraps_one_alias() {
    let host = make_host_with_audit();
    upsert_ts(
        &host,
        "/aliases.ts",
        r#"
export type Inner = { v: string };
export type Outer = Inner;
"#,
    );

    let (nav_node, record) = parts(host.resolve_named_symbol_with_audit(
        "/aliases.ts",
        "Outer",
        Some(ProjectionMode::Navigate),
    ));
    let nav_node = nav_node.expect("Navigate must resolve");
    let _ = assert_one_typeresolution_record(&record);

    let store = host.project_type_store().semantic_graph();
    let data = store
        .node_data(nav_node)
        .expect("node interned during resolution");
    assert!(
        !matches!(data.as_ref(), SemanticNodeData::Alias(_)),
        "Navigate must unwrap one alias hop — got Alias() at the surface"
    );
}

#[test]
fn resolve_named_symbol_expanded_unwraps_and_expands() {
    let host = make_host_with_audit();
    upsert_ts(
        &host,
        "/aliases.ts",
        r#"
export type Inner = { v: string };
export type Outer = Inner;
"#,
    );

    let (exp_node, record) = parts(host.resolve_named_symbol_with_audit(
        "/aliases.ts",
        "Outer",
        Some(ProjectionMode::Expanded),
    ));
    let exp_node = exp_node.expect("Expanded must resolve");
    let _ = assert_one_typeresolution_record(&record);

    let store = host.project_type_store().semantic_graph();
    let data = store
        .node_data(exp_node)
        .expect("node interned during resolution");
    assert!(
        !matches!(data.as_ref(), SemanticNodeData::Alias(_)),
        "Expanded must unwrap alias chain"
    );
    // Expanded mode landed on the inner object body.
    assert!(
        matches!(data.as_ref(), SemanticNodeData::Object(_)),
        "Expanded must reach the inner Object body; got {:?}",
        data
    );
}

#[test]
fn resolve_named_symbol_non_generic_default_expands() {
    let host = make_host_with_audit();
    upsert_ts(
        &host,
        "/types.ts",
        r#"
export type Inner = { v: string };
export type Outer = Inner;
"#,
    );

    // No mode override → host defaults to Expanded for non-generic.
    let (def_node, record) =
        parts(host.resolve_named_symbol_with_audit("/types.ts", "Outer", None));
    let def_node = def_node.expect("default must resolve");
    let r = assert_one_typeresolution_record(&record);
    let payload = match &r.kind_payload {
        RequestKindPayload::TypeResolution(p) => p,
        _ => panic!(),
    };
    // Audit payload's query_mode reflects what we ran with.
    assert_eq!(
        payload.query_mode,
        verter_audit::ProjectionModeTag::Expanded,
        "non-generic default must run in Expanded; got {:?}",
        payload.query_mode
    );

    let store = host.project_type_store().semantic_graph();
    let data = store
        .node_data(def_node)
        .expect("node interned during resolution");
    assert!(
        matches!(data.as_ref(), SemanticNodeData::Object(_)),
        "non-generic default must reach the inner body; got {:?}",
        data
    );
}

#[test]
fn resolve_named_symbol_generic_default_navigates() {
    let host = make_host_with_audit();
    upsert_ts(
        &host,
        "/types.ts",
        r#"
export type Wrap<T> = { wrapped: T };
"#,
    );

    let (def_node, record) = parts(host.resolve_named_symbol_with_audit("/types.ts", "Wrap", None));
    let def_node = def_node.expect("generic default must resolve");
    let r = assert_one_typeresolution_record(&record);
    let payload = match &r.kind_payload {
        RequestKindPayload::TypeResolution(p) => p,
        _ => panic!(),
    };
    // Audit payload's query_mode reflects the host's default
    // selection — Navigate for generic carriers (the default-mode
    // policy in the resolve-named-symbol contract).
    assert_eq!(
        payload.query_mode,
        verter_audit::ProjectionModeTag::Navigate,
        "generic carrier default must run in Navigate; got {:?}",
        payload.query_mode
    );
    let _ = def_node;
}

#[test]
fn resolve_named_symbol_with_audit_emits_one_record() {
    let host = make_host_with_audit();
    upsert_ts(&host, "/single.ts", "export type T = number;\n");

    let baseline = host.audit_records.len();
    let (_node, record) = parts(host.resolve_named_symbol_with_audit(
        "/single.ts",
        "T",
        Some(ProjectionMode::Expanded),
    ));
    // record is always present now (carrier `audit` field is mandatory).
    let _ = &record;
    let after = host.audit_records.len();
    // EXACTLY one new record was inserted — discriminating
    // assertion against any "internal sub-query bumped the count"
    // regression.
    assert_eq!(
        after - baseline,
        1,
        "exactly one RequestAuditRecord per call (got {} before, {} after)",
        baseline,
        after
    );
}

#[test]
fn resolve_named_symbol_with_audit_carries_trace_id() {
    let host = make_host_with_audit();
    upsert_ts(&host, "/single.ts", "export type T = number;\n");

    let (_node, record) = parts(host.resolve_named_symbol_with_audit(
        "/single.ts",
        "T",
        Some(ProjectionMode::Expanded),
    ));
    let r = &record;
    assert!(
        !r.trace_id.is_empty(),
        "RequestAuditRecord.trace_id must propagate from RequestContext.trace_id"
    );
    // UUID v4 is 36 chars (8-4-4-4-12) including hyphens.
    assert_eq!(
        r.trace_id.len(),
        36,
        "trace_id must be a uuid v4; got {:?}",
        r.trace_id
    );
}

// ---------------------------------------------------------------------------
// evaluate_type_expression_with_audit
// ---------------------------------------------------------------------------

#[test]
fn evaluate_simple_primitive_returns_primitive() {
    let host = make_host_with_audit();
    upsert_ts(&host, "/scope.ts", "export type Anchor = number;\n");

    let req = EvaluateTypeExpressionRequest {
        scope: "/scope.ts".to_string(),
        expression: "string".to_string(),
        extra_imports: Vec::new(),
        mode: ProjectionMode::Expanded,
        cacheable: false,
    };
    let (node, record) = parts(host.evaluate_type_expression_with_audit(req));
    let node = node.expect("primitive expression must resolve");
    let _ = assert_one_typeresolution_record(&record);

    let store = host.project_type_store().semantic_graph();
    let data = store.node_data(node).expect("primitive node interned");
    assert!(
        matches!(data.as_ref(), SemanticNodeData::Primitive(_)),
        "expression `string` must reduce to a Primitive node; got {:?}",
        data
    );
}

#[test]
fn evaluate_object_type() {
    let host = make_host_with_audit();
    upsert_ts(&host, "/scope.ts", "export type Anchor = number;\n");

    let req = EvaluateTypeExpressionRequest {
        scope: "/scope.ts".to_string(),
        expression: "{ x: number; y: string }".to_string(),
        extra_imports: Vec::new(),
        mode: ProjectionMode::Expanded,
        cacheable: false,
    };
    let (node, _record) = parts(host.evaluate_type_expression_with_audit(req));
    let node = node.expect("object expression must resolve");

    let store = host.project_type_store().semantic_graph();
    let data = store.node_data(node).expect("object node interned");
    assert!(
        matches!(data.as_ref(), SemanticNodeData::Object(_)),
        "expression `{{ x; y }}` must reduce to an Object node; got {:?}",
        data
    );
}

#[test]
fn evaluate_with_extra_imports() {
    let host = make_host_with_audit();
    // The synthesised scratch file lives at the
    // `verter://typeinfo/<hash>.ts` URI. Relative imports from a
    // synthetic URI cannot follow the workspace's normal directory
    // anchoring, so callers that need to reference workspace types
    // pass absolute / workspace-rooted specifiers in
    // `extra_imports`. Test fixture mirrors that contract.
    upsert_ts(
        &host,
        "/types.ts",
        r#"
export type Foo = { foo: number };
"#,
    );
    upsert_ts(&host, "/scope.ts", "export type Anchor = number;\n");

    let req = EvaluateTypeExpressionRequest {
        scope: "/scope.ts".to_string(),
        expression: "Foo".to_string(),
        extra_imports: vec![ImportSpec {
            specifier: "/types".to_string(),
            bindings: vec![NamedImport::Named {
                exported_name: "Foo".to_string(),
                local_alias: None,
                type_only: true,
            }],
        }],
        mode: ProjectionMode::Expanded,
        cacheable: false,
    };
    let (node, _record) = parts(host.evaluate_type_expression_with_audit(req));
    let node = node.expect("imported expression must resolve");

    let store = host.project_type_store().semantic_graph();
    let data = store.node_data(node).expect("imported Foo node interned");
    // `Foo` is `{ foo: number }` — Expanded must reach the
    // concrete Object body when the import resolved correctly.
    // The discriminating contract is that `extra_imports` produced
    // a non-Miss resolution (i.e. the scratch synthesis injected
    // the import declaration AND the resolver followed it). A Miss
    // result would indicate the synthesis dropped the import.
    let is_object = matches!(data.as_ref(), SemanticNodeData::Object(_));
    let is_miss = matches!(
        data.as_ref(),
        SemanticNodeData::Opaque(crate::semantic_query::QueryError::Miss)
    );
    assert!(
        is_object || !is_miss,
        "expression `Foo` (imported) must NOT yield Opaque(Miss) — `extra_imports` synthesis is missing the import declaration. Got {:?}",
        data
    );
}

#[test]
fn evaluate_scratch_uri_is_deterministic() {
    use super::evaluate_type_expression::compute_scratch_uri;
    let uri_a = compute_scratch_uri("/scope.ts", "{ x: number }", &[]);
    let uri_b = compute_scratch_uri("/scope.ts", "{ x: number }", &[]);
    assert_eq!(
        uri_a, uri_b,
        "same scope+expression must produce identical scratch URIs"
    );
    assert!(uri_a.starts_with("verter://typeinfo/"));
    assert!(uri_a.ends_with(".ts"));
    // Sha256 truncated to 16 bytes = 32 hex chars; full URI = prefix + 32 + ".ts".
    assert_eq!(
        uri_a.len(),
        "verter://typeinfo/".len() + 32 + ".ts".len(),
        "URI shape: prefix + 32-hex-char digest + .ts; got {uri_a}"
    );
}

#[test]
fn evaluate_scratch_uri_differs_by_scope() {
    use super::evaluate_type_expression::compute_scratch_uri;
    let uri_a = compute_scratch_uri("/scope_a.ts", "{ x: number }", &[]);
    let uri_b = compute_scratch_uri("/scope_b.ts", "{ x: number }", &[]);
    assert_ne!(
        uri_a, uri_b,
        "different scope canonical ids must produce different scratch URIs"
    );
}

#[test]
fn evaluate_caches_when_cacheable_true() {
    let host = make_host_with_audit();
    upsert_ts(&host, "/scope.ts", "export type Anchor = number;\n");

    let req = || EvaluateTypeExpressionRequest {
        scope: "/scope.ts".to_string(),
        expression: "{ a: 1 }".to_string(),
        extra_imports: Vec::new(),
        mode: ProjectionMode::Expanded,
        cacheable: true,
    };
    let (node1, record1) = parts(host.evaluate_type_expression_with_audit(req()));
    let node1 = node1.expect("first call must resolve");
    let r1 = &record1;
    assert!(
        !r1.from_cache,
        "first call must report cold (from_cache=false)"
    );

    // Second call with the same parameters → cache hit.
    let (node2, record2) = parts(host.evaluate_type_expression_with_audit(req()));
    let node2 = node2.expect("second call must resolve");
    let r2 = &record2;
    assert!(
        r2.from_cache,
        "second call with same request must report from_cache=true"
    );
    assert_eq!(node1, node2, "cache hit must return the same node id");
}

#[test]
fn evaluate_skips_cache_when_cacheable_false() {
    let host = make_host_with_audit();
    upsert_ts(&host, "/scope.ts", "export type Anchor = number;\n");

    let req = || EvaluateTypeExpressionRequest {
        scope: "/scope.ts".to_string(),
        expression: "{ a: 2 }".to_string(),
        extra_imports: Vec::new(),
        mode: ProjectionMode::Expanded,
        cacheable: false,
    };
    let (_node1, record1) = parts(host.evaluate_type_expression_with_audit(req()));
    let r1 = &record1;
    assert!(!r1.from_cache);
    let (_node2, record2) = parts(host.evaluate_type_expression_with_audit(req()));
    let r2 = &record2;
    assert!(
        !r2.from_cache,
        "non-cacheable repeated call must always be cold"
    );
}

#[test]
fn evaluate_evicts_oldest_at_cache_limit() {
    use super::scratch_cache::{ScratchCache, DEFAULT_CAPACITY};
    use crate::semantic_query::SemanticNodeId;
    // Pure-cache eviction discrimination — fills the cache to the
    // default 64 with synthetic node ids, then proves a 65th
    // insertion drops the OLDEST URI (entry 0). Bypasses the
    // upsert pipeline so the test runs in milliseconds and stays
    // discriminating against the cache's own LRU policy.
    let mut cache = ScratchCache::with_default_capacity();
    assert_eq!(cache.len(), 0);
    for i in 0..DEFAULT_CAPACITY {
        let evicted = cache.insert(
            format!("verter://typeinfo/k{i}.ts"),
            SemanticNodeId(i as u64),
        );
        assert!(
            evicted.is_none(),
            "no eviction should occur until capacity exceeded; got eviction at i={i}"
        );
    }
    assert_eq!(cache.len(), DEFAULT_CAPACITY);

    // 65th insert — must evict the oldest (k0) since none of the
    // previous entries have been touched between insertions.
    let evicted = cache
        .insert(
            "verter://typeinfo/k64.ts".to_string(),
            SemanticNodeId(64u64),
        )
        .expect("eviction must occur on overflow");
    assert_eq!(
        evicted, "verter://typeinfo/k0.ts",
        "LRU eviction must drop the oldest unaccessed entry first; got {evicted}"
    );
    assert_eq!(cache.len(), DEFAULT_CAPACITY);
    // The new entry is present, the evicted one is not.
    assert!(cache.get("verter://typeinfo/k64.ts").is_some());
    assert!(cache.get("verter://typeinfo/k0.ts").is_none());
}

#[test]
fn evaluate_with_audit_emits_one_record() {
    let host = make_host_with_audit();
    upsert_ts(&host, "/scope.ts", "export type Anchor = number;\n");

    let baseline = host.audit_records.len();
    let req = EvaluateTypeExpressionRequest {
        scope: "/scope.ts".to_string(),
        expression: "string".to_string(),
        extra_imports: Vec::new(),
        mode: ProjectionMode::Expanded,
        cacheable: false,
    };
    let (_node, record) = parts(host.evaluate_type_expression_with_audit(req));
    // record is always present now (carrier `audit` field is mandatory).
    let _ = &record;
    let after = host.audit_records.len();
    assert_eq!(
        after - baseline,
        1,
        "exactly one RequestAuditRecord per evaluate call (got {} before, {} after)",
        baseline,
        after
    );
}

#[test]
fn evaluate_with_audit_carries_trace_id() {
    let host = make_host_with_audit();
    upsert_ts(&host, "/scope.ts", "export type Anchor = number;\n");

    let req = EvaluateTypeExpressionRequest {
        scope: "/scope.ts".to_string(),
        expression: "string".to_string(),
        extra_imports: Vec::new(),
        mode: ProjectionMode::Expanded,
        cacheable: false,
    };
    let (_node, record) = parts(host.evaluate_type_expression_with_audit(req));
    let r = &record;
    assert!(
        !r.trace_id.is_empty(),
        "trace_id must propagate from RequestContext.trace_id"
    );
    assert_eq!(r.trace_id.len(), 36, "trace_id must be uuid v4");
}

// ---------------------------------------------------------------------------
// Edge-case: type_args lowering
// ---------------------------------------------------------------------------

#[test]
fn resolve_named_symbol_with_type_args_instantiates() {
    let host = make_host_with_audit();
    upsert_ts(
        &host,
        "/types.ts",
        r#"
export type Wrap<T> = { wrapped: T };
"#,
    );

    // Type-argument instantiation routes through the WIRE entry — the sole
    // public path that accepts (symbolic) type args; it lowers them to node
    // ids INSIDE its one audited request under the request's single store
    // view. The bare `resolve_named_symbol_with_audit` takes no type args.
    let (node, record) = parts(host.resolve_named_symbol_wire_with_audit(
        "/types.ts",
        "Wrap",
        &[type_arg_primitive(PrimitiveName::String)],
        Some(ProjectionMode::Expanded),
    ));
    let node = node.expect("Wrap<string> must resolve");
    let _ = assert_one_typeresolution_record(&record);
    let store = host.project_type_store().semantic_graph();
    let data = store.node_data(node).expect("instantiated node interned");
    // The instantiated body is an Object with a single member
    // `wrapped: string`.
    match data.as_ref() {
        SemanticNodeData::Object(_view) => {
            // Successful instantiation reaches Object.
        }
        other => panic!("Wrap<string> must instantiate to an Object body; got {other:?}"),
    }
    // Suppress unused warning on the helper.
    let _ = type_arg_ref;
}

#[test]
fn wire_lowering_miss_reports_computed_default_effective_mode() {
    // FIX D regression: an early miss with `mode = None` must record the
    // resolver's COMPUTED default effective mode in the audit record — for a
    // NON-GENERIC scope that is `Expanded`, NOT a hardcoded `Navigate`.
    //
    // A canonical with no shallow file state fails the very first
    // `shallow_file_state?` inside `lower_type_expr_in_scope_with_mode`, so
    // ANY non-empty wire arg deterministically hits an early-miss return in
    // `resolve_named_symbol_request` (the wire-lowering miss; and if the store
    // view were momentarily non-current, the non-current-view miss — BOTH were
    // fixed to return the computed `effective_mode`). The scope is non-generic
    // (no shallow decl ⇒ not a generic carrier), so the computed default is
    // `Expanded`.
    let host = make_host_with_audit();
    // Settle the host with a real file so the store view proves current and
    // the request reaches the wire-lowering step.
    upsert_ts(&host, "/real.ts", "export type T = number;\n");

    let (node, record) = host
        .resolve_named_symbol_wire_with_audit(
            "/absent.ts",
            "Whatever",
            &[type_arg_primitive(PrimitiveName::String)],
            None,
        )
        .into_parts();
    // A wire-lowering miss is a NON-FAULT `Ok(None)` — never a fault, never a
    // partial instantiation.
    assert!(
        matches!(node, Ok(None)),
        "a wire-lowering miss is a non-fault Ok(None); got {node:?}"
    );
    let r = assert_one_typeresolution_record(&record);
    let payload = match &r.kind_payload {
        RequestKindPayload::TypeResolution(p) => p,
        _ => panic!("the miss still emits a type-resolution audit payload"),
    };
    // DISCRIMINATING: the pre-fix miss hardcoded `Navigate`
    // (`requested_mode.unwrap_or(Navigate)`); the fix returns the computed
    // default `Expanded` for this non-generic scope. A regression to the
    // hardcoded fallback flips this to `Navigate` and fails here.
    assert_eq!(
        payload.query_mode,
        verter_audit::ProjectionModeTag::Expanded,
        "a wire-lowering miss (mode=None, non-generic scope) must record the \
         COMPUTED default Expanded, not a hardcoded Navigate; got {:?}",
        payload.query_mode
    );
}

/// Recursively collect member names across `Object` / `Intersection` /
/// `Union` arms. An `Opaque` arm contributes the sentinel `"<opaque-miss>"`
/// so a collapsed heritage carrier is observable in the assertion.
fn collect_surface_member_names(
    store: &crate::semantic_query_memo::SemanticGraphStore,
    node: crate::semantic_query::SemanticNodeId,
    out: &mut Vec<String>,
) {
    match store.node_data(node).as_deref() {
        Some(SemanticNodeData::Object(view)) => {
            for m in view.members.iter() {
                out.push(m.name.to_string());
            }
        }
        Some(SemanticNodeData::Intersection(arms) | SemanticNodeData::Union(arms)) => {
            for arm in arms.iter() {
                collect_surface_member_names(store, *arm, out);
            }
        }
        Some(SemanticNodeData::Opaque(_)) => out.push("<opaque-miss>".to_string()),
        _ => {}
    }
}

/// Regression — generic-`Omit`-of-`Pick`-of-generic heritage must NOT
/// collapse the heritage carrier to `Opaque(Miss)` under
/// `Instantiate(Published(Expanded))`.
///
/// `interface ColorModeSelectProps extends Omit<SelectMenuProps<Item[]>, 'items'>`
/// where `SelectMenuProps<T> extends Pick<RootProps<T>, 'open'|'defaultOpen'|'disabled'>`
/// instantiates to `Intersection([<heritage>, <own body>])`. Before the
/// `object_filter_source_surface` compound-carrier fix, the outer `Omit`'s
/// source (`SelectMenuProps<Item[]>`, itself an `Intersection` because it has
/// heritage) failed `object_filter_source_surface` (which only handled
/// `Object` / `DeclRef` / `InstantiationRef`), so the heritage arm collapsed to
/// `Opaque(Miss)` and `open`/`defaultOpen`/`disabled` were LOST.
///
/// Discrimination: pre-fix the heritage arm is `Opaque(Miss)` (member set is
/// `["<opaque-miss>"]`, missing the three inherited props); post-fix it carries
/// `open`/`defaultOpen`/`disabled` and omits `items`.
#[test]
fn generic_omit_pick_heritage_instantiation_preserves_inherited_members() {
    let host = make_host_with_audit();
    upsert_ts(
        &host,
        "/types.ts",
        r#"
export interface Item { label: string }
export interface RootProps<T> {
  open?: boolean;
  defaultOpen?: boolean;
  disabled?: boolean;
  modelValue?: T;
}
export interface SelectMenuProps<T> extends Pick<RootProps<T>, 'open' | 'defaultOpen' | 'disabled'> {
  items?: T;
}
export interface ColorModeSelectProps extends Omit<SelectMenuProps<Item[]>, 'items'> {}
"#,
    );

    // (1) Direct dispatch: `Instantiate(Published(Expanded))` of the
    //     non-generic `ColorModeSelectProps`.
    use crate::semantic_query::SemanticQueryApi;
    let store = host.project_type_store().semantic_graph();
    let _shallow = host
        .shallow_file_state("/types.ts")
        .expect("shallow state for /types.ts");
    let key = crate::semantic_query::ResolvedDeclSlotIdentity::type_slot_unscoped(
        Arc::from("/types.ts"),
        Arc::from("ColorModeSelectProps"),
    );
    let dispatch = crate::project_semantic_dispatch::ProjectSemanticDispatch::new(host.as_ref());
    let node =
        match dispatch.execute_type_node(crate::semantic_query::SemanticQueryKey::Instantiate(
            crate::semantic_query::InstantiateKey::new(
                key,
                Arc::from(Vec::new().into_boxed_slice()),
                crate::semantic_query::InstantiateContext::non_file(
                    crate::semantic_query::ProjectionReductionContext::published(
                        ProjectionMode::Expanded,
                    ),
                    Default::default(),
                    crate::project_semantic_dispatch::BodySourceWitness::mint_for_unit_tests(),
                ),
            ),
        )) {
            crate::semantic_query::QueryResult::Value(
                crate::semantic_query::SemanticQueryOutput { value: n, .. },
            ) => n,
            crate::semantic_query::QueryResult::Recursive(n) => n,
            crate::semantic_query::QueryResult::Error(e) => {
                panic!("Instantiate(ColorModeSelectProps, Expanded) errored: {e:?}")
            }
        };
    let mut names = Vec::new();
    collect_surface_member_names(store, node, &mut names);
    for inherited in ["open", "defaultOpen", "disabled"] {
        assert!(
            names.iter().any(|n| n == inherited),
            "direct Instantiate(Published(Expanded)) of \
             `ColorModeSelectProps extends Omit<SelectMenuProps<Item[]>, 'items'>` \
             MUST carry inherited member `{inherited}`; the generic-Omit-of-Pick \
             heritage carrier collapsed. Got members: {names:?}"
        );
    }
    assert!(
        !names.iter().any(|n| n == "items"),
        "the outer `Omit<…, 'items'>` MUST exclude `items`. Got: {names:?}"
    );
    assert!(
        !names.iter().any(|n| n == "<opaque-miss>"),
        "no surface arm may remain `Opaque(Miss)` — the heritage carrier must \
         resolve, not collapse. Got: {names:?}"
    );

    // (2) Public consumer: `resolve_named_symbol_with_audit` (defaults to
    //     Expanded for a non-generic decl) must return a type carrying the
    //     inherited members.
    let (resolved, record) = parts(host.resolve_named_symbol_with_audit(
        "/types.ts",
        "ColorModeSelectProps",
        Some(ProjectionMode::Expanded),
    ));
    let resolved = resolved.expect("ColorModeSelectProps must resolve");
    let _ = assert_one_typeresolution_record(&record);
    let mut public_names = Vec::new();
    collect_surface_member_names(store, resolved, &mut public_names);
    for inherited in ["open", "defaultOpen", "disabled"] {
        assert!(
            public_names.iter().any(|n| n == inherited),
            "resolve_named_symbol_with_audit(\"ColorModeSelectProps\") MUST \
             return a type carrying inherited member `{inherited}` — `raise`/the \
             public path must not drop it through an `Opaque(Miss)`/`Unknown` \
             heritage arm. Got members: {public_names:?}"
        );
    }
    assert!(
        !public_names.iter().any(|n| n == "items"),
        "the public consumer's type MUST exclude omitted `items`. Got: {public_names:?}"
    );
    assert!(
        !public_names.iter().any(|n| n == "<opaque-miss>"),
        "the public consumer's type MUST NOT carry an `Opaque(Miss)` heritage \
         arm. Got: {public_names:?}"
    );
}

// ---------------------------------------------------------------------------
// Dispatch-fault routing (F1)
//
// The typeinfo resolution entry-points decode every `QueryResult::Error`
// arm through `classify_dispatch_error`, which routes a genuine dispatch
// FAULT to the carrier's `Err(TypeResolutionRequestError)` arm and a
// non-fault MISS to `Ok(fallback)`. These tests pin that split.
//
// DISCRIMINATING: the pre-change entry-points collapsed every
// `QueryResult::Error(_)` to `None` (→ `Ok(None)` in carrier terms), so
// `classify_dispatch_error` did not exist and a fault could never reach
// the `Err` arm. The fault assertions below therefore FAIL against the
// old collapse and PASS against the routed split. The end-to-end miss
// tests pin the complementary half — a well-formed request that resolves
// no node rides `Ok`, never `Err`.
// ---------------------------------------------------------------------------

use crate::host_resolve_type_audit::TypeResolutionRequestError;
use crate::resolver_core::shallow_file_state::{BudgetDomain, BudgetExceededFailure};
use crate::semantic_query::QueryError;
use crate::typeinfo::resolve_named_symbol::classify_dispatch_error;

fn sample_budget_failure() -> BudgetExceededFailure {
    BudgetExceededFailure {
        domain: BudgetDomain::ProjectionOperation,
        limit: 1,
        actual: 2,
        context: "typeinfo-fault-test".to_string(),
    }
}

#[test]
fn classify_dispatch_error_routes_budget_exceeded_to_err() {
    let err = QueryError::BudgetExceeded(sample_budget_failure());
    let out = classify_dispatch_error(&err, None);
    assert_eq!(
        out,
        Err(TypeResolutionRequestError::BudgetExceeded(
            sample_budget_failure()
        )),
        "a BudgetExceeded dispatch fault MUST ride the carrier's `Err` arm, \
         NOT collapse to `Ok(None)`"
    );
}

#[test]
fn classify_dispatch_error_routes_alias_cycle_to_err() {
    let chain: Arc<[Arc<str>]> = Arc::from(vec![Arc::from("A"), Arc::from("B")]);
    let err = QueryError::AliasCycle {
        chain: Arc::clone(&chain),
    };
    let out = classify_dispatch_error(&err, None);
    assert!(
        matches!(out, Err(TypeResolutionRequestError::AliasCycle { .. })),
        "an AliasCycle dispatch fault MUST ride `Err`; got {out:?}"
    );
}

#[test]
fn classify_dispatch_error_routes_unstable_state_to_err() {
    let err = QueryError::UnstableState { attempts: 3 };
    let out = classify_dispatch_error(&err, None);
    assert_eq!(
        out,
        Err(TypeResolutionRequestError::UnstableState { attempts: 3 }),
    );
}

#[test]
fn classify_dispatch_error_routes_unsupported_intrinsic_to_err() {
    let err = QueryError::UnsupportedIntrinsic {
        name: Arc::from("NoSuchIntrinsic"),
    };
    let out = classify_dispatch_error(&err, None);
    assert!(
        matches!(
            out,
            Err(TypeResolutionRequestError::UnsupportedIntrinsic { .. })
        ),
        "an UnsupportedIntrinsic dispatch fault MUST ride `Err`; got {out:?}"
    );
}

#[test]
fn classify_dispatch_error_routes_miss_to_ok_fallback() {
    // A non-fault miss rides `Ok`, carrying whatever fallback node the
    // caller already resolved (here `None`).
    assert_eq!(classify_dispatch_error(&QueryError::Miss, None), Ok(None));
    // RecursiveRef and DeclPlaceholder are also non-faults.
    assert_eq!(
        classify_dispatch_error(
            &QueryError::RecursiveRef {
                name: Arc::from("Tree")
            },
            None
        ),
        Ok(None)
    );
    assert_eq!(
        classify_dispatch_error(
            &QueryError::DeclPlaceholder {
                canonical_id: Arc::from("/a.ts"),
                name: Arc::from("Foo"),
                whole_hash: Default::default(),
            },
            None
        ),
        Ok(None)
    );
}

#[test]
fn resolve_named_symbol_unknown_symbol_rides_ok_not_err() {
    let host = make_host_with_audit();
    upsert_ts(&host, "/miss.ts", "export type T = string;\n");
    let (outcome, _record) = host
        .resolve_named_symbol_with_audit(
            "/miss.ts",
            "DefinitelyNotDeclared",
            Some(ProjectionMode::Expanded),
        )
        .into_parts();
    assert!(
        outcome.is_ok(),
        "an unknown symbol is a non-fault miss and MUST ride the `Ok` arm, \
         never `Err`; got {outcome:?}"
    );
}

#[test]
fn evaluate_type_expression_unresolvable_rides_ok_not_err() {
    let host = make_host_with_audit();
    upsert_ts(&host, "/eval_miss.ts", "export type T = string;\n");
    let req = EvaluateTypeExpressionRequest {
        scope: "/eval_miss.ts".to_string(),
        expression: "NoSuchTypeName".to_string(),
        extra_imports: Vec::new(),
        mode: ProjectionMode::Expanded,
        cacheable: false,
    };
    let (outcome, _record) = host.evaluate_type_expression_with_audit(req).into_parts();
    assert!(
        outcome.is_ok(),
        "an unresolvable expression is a non-fault miss and MUST ride the \
         `Ok` arm, never `Err`; got {outcome:?}"
    );
}

#[test]
fn nested_materialization_hard_fault_rides_err_not_degraded_ok() {
    use crate::semantic_query::{QueryResult, SemanticNodeId};
    use crate::typeinfo::resolve_named_symbol::{
        classify_materialization_step, MaterializationStep,
    };

    // Discriminating guard for the nested-materialization fault hop.
    //
    // The placeholder-materialisation loop (in the ONE canonical
    // `materialize_through_aliases`, shared by both typeinfo entry-points)
    // re-dispatches `Instantiate { args: [] }` against a `DeclPlaceholder`'s
    // identity and routes the result through `classify_materialization_step`.
    // When that
    // nested dispatch HARD-FAULTS (a `BudgetExceeded` / `UnstableState` /
    // `AliasCycle` / `UnsupportedIntrinsic` / `Other` `QueryResult::Error`
    // arm), the fault MUST propagate as `Err` — not silently degrade to
    // `Ok(Stop(placeholder))`.
    //
    // Pre-change the loop's `QueryResult::Error(_) => return current` arm
    // returned the un-materialised placeholder node, so a hard fault would
    // observe `Ok(Stop(_))`. This assertion FAILS against that behaviour
    // and PASSES once the arm routes through `classify_dispatch_error`.
    let current = SemanticNodeId(7);

    // HARD FAULT → Err (the bug the change fixed).
    let fault = QueryResult::Error(QueryError::BudgetExceeded(sample_budget_failure()));
    let out = classify_materialization_step(fault, current);
    assert!(
        matches!(out, Err(TypeResolutionRequestError::BudgetExceeded(_))),
        "a hard dispatch fault during NESTED materialization MUST propagate as \
         `Err`, never degrade to `Ok(Stop(placeholder))`; got {out:?}"
    );

    // NON-FAULT MISS → degraded-but-successful `Ok(Stop(current))`
    // (degraded-but-successful rides Ok, per native-flow-return.md).
    let miss = classify_materialization_step(QueryResult::Error(QueryError::Miss), current);
    assert_eq!(
        miss,
        Ok(MaterializationStep::Stop(current)),
        "a non-fault miss keeps the degraded `current` node as a successful Ok"
    );

    // Progressing VALUE → Continue(next).
    let next = SemanticNodeId(9);
    let step = classify_materialization_step(QueryResult::Value(next), current);
    assert_eq!(
        step,
        Ok(MaterializationStep::Continue(next)),
        "a fresh value node advances the loop"
    );

    // VALUE that did not progress (next == current) → Stop(current).
    let no_progress = classify_materialization_step(QueryResult::Value(current), current);
    assert_eq!(
        no_progress,
        Ok(MaterializationStep::Stop(current)),
        "a dispatch that returns the same placeholder stops the loop"
    );
}

#[test]
fn operator_reduction_step_propagates_fault_opaque_not_masked_as_carrier() {
    use crate::semantic_query::{LiteralValue, QueryResult};
    use crate::typeinfo::resolve_named_symbol::{
        classify_operator_reduction_step, MaterializationStep,
    };

    // Discriminating guard for the operator-reduction step's FAULT axis.
    //
    // The shared `ProjectPath` / `KeyOf` reducer can hand a fault-bearing
    // opaque back as `QueryResult::Value(Opaque(err))` (not as a
    // `QueryResult::Error`). A naive wildcard over ALL `Opaque(_)` results to
    // `Stop(carrier)` would hide a FAILED reduction behind a `T[K]` / `keyof T`
    // operator carrier, making it look like a valid deferred operator. The step
    // instead partitions the opaque through `classify_dispatch_error`: a fault
    // PROPAGATES as `Err`, a non-fault miss preserves the carrier.
    //
    // This test discriminates the two: it FAILS against an undifferentiated
    // wildcard (fault → `Ok(Stop(carrier))`) and PASSES against the partitioned
    // step (fault → `Err`).
    let host = make_host_with_audit();
    let graph = host.project_type_store().semantic_graph();

    // The operator carrier the step preserves on a non-fault.
    let carrier = graph.intern_node(SemanticNodeData::Opaque(QueryError::RecursiveRef {
        name: Arc::from("Carrier"),
    }));

    // (a) FAULT opaque as a Value → MUST propagate as `Err`, never mask.
    let fault_node = graph.intern_node(SemanticNodeData::Opaque(QueryError::Other(Arc::from(
        "reduction blew up",
    ))));
    let out = classify_operator_reduction_step(graph, QueryResult::Value(fault_node), carrier);
    assert!(
        matches!(out, Err(TypeResolutionRequestError::Other(_))),
        "a fault-bearing `Opaque` reduction result MUST propagate as `Err`, \
         not be masked behind the operator carrier; got {out:?}"
    );

    // (b) NON-FAULT miss opaque as a Value → preserve the carrier (never
    //     degrade to a `semanticMiss` terminal).
    let miss_node = graph.intern_node(SemanticNodeData::Opaque(QueryError::Miss));
    let miss = classify_operator_reduction_step(graph, QueryResult::Value(miss_node), carrier);
    assert_eq!(
        miss,
        Ok(MaterializationStep::Stop(carrier)),
        "a non-fault miss opaque preserves the operator carrier"
    );

    // (c) A genuine reduced value (non-opaque) → advance the loop.
    let reduced = graph.intern_node(SemanticNodeData::Literal(LiteralValue::String(
        "id".to_string(),
    )));
    let step = classify_operator_reduction_step(graph, QueryResult::Value(reduced), carrier);
    assert_eq!(
        step,
        Ok(MaterializationStep::Continue(reduced)),
        "a genuine reduced value advances the loop past the operator carrier"
    );

    // (d) A `QueryResult::Error` HARD FAULT → `Err` (the Error arm, distinct
    //     from the Value(Opaque) arm).
    let hard = classify_operator_reduction_step(
        graph,
        QueryResult::Error(QueryError::BudgetExceeded(sample_budget_failure())),
        carrier,
    );
    assert!(
        matches!(hard, Err(TypeResolutionRequestError::BudgetExceeded(_))),
        "a hard `QueryResult::Error` fault propagates as `Err`; got {hard:?}"
    );

    // (e) A `Recursive` back-edge preserves the carrier (a cycle, not a
    //     reduction).
    let recursive =
        classify_operator_reduction_step(graph, QueryResult::Recursive(reduced), carrier);
    assert_eq!(
        recursive,
        Ok(MaterializationStep::Stop(carrier)),
        "a recursive back-edge preserves the operator carrier"
    );
}

#[test]
fn keyof_base_surfacing_propagates_fault_not_swallows_to_base() {
    use crate::semantic_query::{QueryResult, SemanticNodeId};
    use crate::typeinfo::resolve_named_symbol::classify_base_surfacing;

    // Discriminating guard for the keyof base-surfacing step's FAULT axis.
    //
    // Before the keyof reducer runs, the operand must be surfaced through an
    // empty-path `ProjectPath`. An undifferentiated `QueryResult::Error(_) =>
    // *base` arm would swallow EVERY error — including a genuine
    // `BudgetExceeded` / `AliasCycle` fault — and fall back to the un-surfaced
    // base, after which the keyof reducer publishes a DEFERRED CARRIER instead
    // of surfacing the fault. The step instead routes the `Error` arm through
    // `classify_dispatch_error`: a fault PROPAGATES, only a non-fault miss falls
    // back.
    //
    // This test discriminates the two: it FAILS against an error-swallowing arm
    // (fault → `Ok(base)`) and PASSES against the partitioned decode (fault →
    // `Err`).
    let base = SemanticNodeId(11);

    // HARD FAULT during surfacing → propagate as `Err` (never fall back to base).
    let fault = classify_base_surfacing(
        QueryResult::Error(QueryError::BudgetExceeded(sample_budget_failure())),
        base,
    );
    assert!(
        matches!(fault, Err(TypeResolutionRequestError::BudgetExceeded(_))),
        "a fault while surfacing the keyof operand MUST propagate as `Err`, not \
         silently fall back to the un-surfaced base; got {fault:?}"
    );

    // NON-FAULT miss → fall back to the un-surfaced base (the reducer round-trips
    // it as the deferred carrier).
    let miss = classify_base_surfacing(QueryResult::Error(QueryError::Miss), base);
    assert_eq!(
        miss,
        Ok(base),
        "a non-fault miss falls back to the un-surfaced base"
    );

    // A genuine surfaced value / recursive node is used verbatim.
    let surfaced = SemanticNodeId(12);
    assert_eq!(
        classify_base_surfacing(QueryResult::Value(surfaced), base),
        Ok(surfaced),
        "a surfaced operand node is used verbatim"
    );
    assert_eq!(
        classify_base_surfacing(QueryResult::Recursive(surfaced), base),
        Ok(surfaced),
        "a recursive surfaced node is used verbatim"
    );
}
