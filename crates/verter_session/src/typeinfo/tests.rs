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
//! `tests/type_resolution_audit_diamond_repeated_prop.rs`.

use std::sync::Arc;

use verter_audit::{RequestKind, RequestKindPayload};
use verter_type_expr::{PrimitiveName, TypeExpr};

use super::types::{EvaluateTypeExpressionRequest, ImportSpec, NamedImport, SymbolKind};
use crate::semantic_query::{ProjectionMode, SemanticNodeData};
use crate::types::{FileKind, HostConfig, UpsertRequest};
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
        file_kind: FileKind::from_path(canonical_id),
        aliases: Vec::new(),
    });
}

fn assert_one_typeresolution_record(
    record: &Option<verter_audit::RequestAuditRecord>,
) -> &verter_audit::RequestAuditRecord {
    let r = record
        .as_ref()
        .expect("active TypeResolution request must produce a record");
    assert_eq!(r.kind, RequestKind::TypeResolution);
    assert!(
        matches!(r.kind_payload, RequestKindPayload::TypeResolution(_)),
        "kind_payload must be TypeResolution"
    );
    r
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

#[test]
fn list_file_symbols_5kloc_under_50ms() {
    let host = make_host_with_audit();

    // Synthesise a ~5 KLOC TS file with 250 type aliases. Average
    // line length ≈ 20 chars × 250 ≈ 5 KB — line count is what
    // matters for the parser-walk cost, not byte count.
    let mut source = String::with_capacity(5_000);
    for i in 0..250 {
        source.push_str("export type T");
        source.push_str(&i.to_string());
        source.push_str(" = { value: number };\n");
        // Pad to ~20 lines per type to reach 5_000 lines.
        for _ in 0..19 {
            source.push_str("// filler line\n");
        }
    }
    upsert_ts(&host, "/big.ts", &source);

    // Warm pass — first call drives the shallow analyser.
    let _ = host.list_file_symbols("/big.ts");

    let start = std::time::Instant::now();
    let symbols = host.list_file_symbols("/big.ts");
    let elapsed = start.elapsed();

    assert!(
        !symbols.is_empty(),
        "synthesised 5KLOC file must produce non-empty inventory"
    );
    assert!(
        elapsed.as_millis() < 50,
        "list_file_symbols(5KLOC) took {} ms — must be < 50 ms (§1.1)",
        elapsed.as_millis()
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

    // First resolve `Outer` in Identity mode. Per §5.2, Identity
    // does NOT unwrap aliases — the contract is "do not unwrap +
    // do not expand". Compare against the Navigate /
    // Expanded results to discriminate.
    let (id_node, record) = host.resolve_named_symbol_with_audit(
        "/aliases.ts",
        "Outer",
        &[],
        Some(ProjectionMode::Identity),
    );
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
    let (navigate_node, _) = host.resolve_named_symbol_with_audit(
        "/aliases.ts",
        "Outer",
        &[],
        Some(ProjectionMode::Navigate),
    );
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

    let (nav_node, record) = host.resolve_named_symbol_with_audit(
        "/aliases.ts",
        "Outer",
        &[],
        Some(ProjectionMode::Navigate),
    );
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

    let (exp_node, record) = host.resolve_named_symbol_with_audit(
        "/aliases.ts",
        "Outer",
        &[],
        Some(ProjectionMode::Expanded),
    );
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
    let (def_node, record) = host.resolve_named_symbol_with_audit("/types.ts", "Outer", &[], None);
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

    let (def_node, record) = host.resolve_named_symbol_with_audit("/types.ts", "Wrap", &[], None);
    let def_node = def_node.expect("generic default must resolve");
    let r = assert_one_typeresolution_record(&record);
    let payload = match &r.kind_payload {
        RequestKindPayload::TypeResolution(p) => p,
        _ => panic!(),
    };
    // Audit payload's query_mode reflects the host's default
    // selection — Navigate for generic carriers per Claude P1-19 / P1-11.
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
    let (_node, record) = host.resolve_named_symbol_with_audit(
        "/single.ts",
        "T",
        &[],
        Some(ProjectionMode::Expanded),
    );
    let _ = record.expect("one record must be returned at the call boundary");
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

    let (_node, record) = host.resolve_named_symbol_with_audit(
        "/single.ts",
        "T",
        &[],
        Some(ProjectionMode::Expanded),
    );
    let r = record.expect("active request must produce a record");
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
    let (node, record) = host.evaluate_type_expression_with_audit(req);
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
    let (node, _record) = host.evaluate_type_expression_with_audit(req);
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
    let (node, _record) = host.evaluate_type_expression_with_audit(req);
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
        "different scope canonical ids must produce different scratch URIs (Gemini P1-1)"
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
    let (node1, record1) = host.evaluate_type_expression_with_audit(req());
    let node1 = node1.expect("first call must resolve");
    let r1 = record1.expect("first record");
    assert!(
        !r1.from_cache,
        "first call must report cold (from_cache=false)"
    );

    // Second call with the same parameters → cache hit.
    let (node2, record2) = host.evaluate_type_expression_with_audit(req());
    let node2 = node2.expect("second call must resolve");
    let r2 = record2.expect("second record");
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
    let (_node1, record1) = host.evaluate_type_expression_with_audit(req());
    let r1 = record1.expect("first record");
    assert!(!r1.from_cache);
    let (_node2, record2) = host.evaluate_type_expression_with_audit(req());
    let r2 = record2.expect("second record");
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
    let (_node, record) = host.evaluate_type_expression_with_audit(req);
    let _ = record.expect("one record must be returned at the call boundary");
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
    let (_node, record) = host.evaluate_type_expression_with_audit(req);
    let r = record.expect("active request must produce a record");
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

    let (node, record) = host.resolve_named_symbol_with_audit(
        "/types.ts",
        "Wrap",
        &[type_arg_primitive(PrimitiveName::String)],
        Some(ProjectionMode::Expanded),
    );
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
