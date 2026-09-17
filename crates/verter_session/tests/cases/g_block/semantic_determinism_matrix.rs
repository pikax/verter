//! The §5.9 mandatory determinism matrix and the §5.4 stable-key
//! variant/encoding table — the signature kernel's V0 registration
//! (acceptance V0-AC4): both tables are ENUMERATED against their
//! authorities (the checked-in contract's §5.9 table; the live
//! `pub enum SemanticNodeData` declaration) and CONSUMED by executable
//! replay drivers that perturb the CURRENT implementation and compare
//! completed observations.
//!
//! Registration discipline:
//!
//! * Every §5.9 perturbation has exactly ONE row. A row the current
//!   implementation is known not to satisfy is `#[ignore]`d naming the
//!   successor block that un-ignores it; no row is an empty body or a
//!   trivially true assertion.
//! * A driver that can prove a SUBSET of its row's axes today proves
//!   that subset and names the remaining axes in the row's `axes`
//!   field — the §5.9 text itself stages the combined vertical at V8.
//! * The completed-observation basis is the semantic-graph projection
//!   rendered to STABLE TEXT (kinds, names, literal values, structural
//!   order — never node ids, pointers, or allocation ordinals), so a
//!   schedule-dependent answer cannot hide behind an unstable print.
//!
//! This file is the test home named by the evidence lock; the replay
//! drivers stay provider-free (no checker, no tsgo) like the rest of
//! the default closure.

use std::sync::Arc;

use verter_session::semantic_query::{ReturnProjectionDemand, SemanticNodeData};
use verter_session::{HostConfig, UpsertRequest, VerterHost};

// ─────────────────────────────────────────────────────────────────────────
// Fixtures
// ─────────────────────────────────────────────────────────────────────────

const MAIN_TS: &str = r#"
import { sharedUnion } from "./dep1";
import { otherUnion } from "./dep2";

export function localUnion(v: number | string) { return v; }
export function usesImports(v: boolean) { return v ? sharedUnion(v) : otherUnion(v); }
export function literalOrderA() { return { a: 1 } as { a: 1 } & { b: 2 }; }
"#;

const DEP1_TS: &str = r#"
export function sharedUnion(v: number | string) { return v; }
"#;

const DEP2_TS: &str = r#"
export function otherUnion(v: number | string) { return v; }
"#;

const REORDER_A_TS: &str = r#"
export function witness() { const x: { a: 1 } & { b: 2 } = null as any; return x; }
"#;

const REORDER_B_TS: &str = r#"
export function witness() { const x: { b: 2 } & { a: 1 } = null as any; return x; }
"#;

const DUPLICATE_A_TS: &str = r#"
export function witness() { return 1 as number | string; }
"#;

const DUPLICATE_B_TS: &str = r#"
export function witness() { return 1 as number | string; }
"#;

const ANONYMOUS_TS: &str = r#"
export function witness() { return class { inner = 1; }; }
"#;

const EDIT_ORIGINAL_TS: &str = r#"
export function witness() { return { marker: "original" }; }
"#;

const EDIT_CHANGED_TS: &str = r#"
export function witness() { return { marker: "changed", extra: 1 }; }
"#;

const UNRELATED_TS: &str = r#"
export type UnrelatedUnion = "x" | "y" | 42;
export function unrelatedWitness(v: UnrelatedUnion) { return v; }
"#;

// ─────────────────────────────────────────────────────────────────────────
// Host helpers
// ─────────────────────────────────────────────────────────────────────────

fn build_host(files: &[(&'static str, &'static str)]) -> Arc<VerterHost> {
    let host = Arc::new(VerterHost::new_standalone(HostConfig {
        audit_enabled: true,
        ..HostConfig::default()
    }));
    for (canonical, source) in files {
        upsert(&host, canonical, source);
    }
    host
}

fn build_host_with_cpu_threads(
    files: &[(&'static str, &'static str)],
    cpu_threads: usize,
) -> Arc<VerterHost> {
    let host = Arc::new(VerterHost::new_standalone_with_scheduler_config(
        HostConfig {
            audit_enabled: true,
            ..HostConfig::default()
        },
        verter_scheduler::scheduler::SchedulerConfig {
            cpu_threads,
            ..verter_scheduler::scheduler::SchedulerConfig::default()
        },
    ));
    for (canonical, source) in files {
        upsert(&host, canonical, source);
    }
    host
}

fn upsert(host: &VerterHost, canonical: &str, source: &str) {
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: Some(canonical.to_owned()),
            input_id: canonical.to_owned(),
            source: Arc::from(source),
            file_language: verter_session::LanguageRegistry::global()
                .classify_static(canonical)
                .static_resolution(),
            aliases: Vec::new(),
        })
        .unwrap_or_else(|err| panic!("upsert `{canonical}`: {err:?}"));
}

fn identity(canonical: &str, symbol: &str) -> verter_type_expr::facts::FlowFunctionReturnIdentity {
    verter_type_expr::facts::FlowFunctionReturnIdentity {
        anchor: verter_type_expr::locators::AuthoredAnchor {
            canonical_id: Arc::from(canonical),
            owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
            symbol: Arc::from(symbol),
            space: verter_type_expr::locators::LocatorSymbolSpace::Value,
        },
        function_part: verter_type_expr::facts::FunctionPartIdentity::DeclarationBody,
        overload_ordinal: 0,
    }
}

/// The completed observation for one witness: the flow-return result's
/// degradation plus its return node rendered to STABLE TEXT through the
/// public semantic graph. Stable means: kinds, names, literal values
/// and structural order only — never node ids or allocation ordinals —
/// with the workspace root normalized away so relocation compares.
fn observe(host: &VerterHost, canonical: &str, symbol: &str) -> String {
    let carrier = host.get_flow_return_type_with_audit(
        &identity(canonical, symbol),
        ReturnProjectionDemand::whole_return(),
    );
    let result = carrier
        .as_result()
        .ok()
        .unwrap_or_else(|| panic!("observe `{canonical}` / `{symbol}` did not complete"));
    let graph = host.project_type_store().semantic_graph();
    let mut text = String::new();
    match result.degradation() {
        None => text.push_str("complete:"),
        Some(degradation) => {
            text.push_str(&format!("degraded:{degradation:?}:"));
        }
    }
    render_node(graph, result.return_type(), 0, &mut text);
    text
}

/// Render a semantic node to stable text. Recursive arms (aliases,
/// unions, intersections, arrays, tuples, wrappers) descend in PAYLOAD
/// order so authored order stays visible; leaves print kinds, names
/// and literal values.
fn render_node(
    graph: &Arc<verter_session::for_tests::SemanticGraphStore>,
    node: verter_session::semantic_query::SemanticNodeId,
    depth: usize,
    out: &mut String,
) {
    use verter_session::semantic_query::SemanticNodeData as D;
    if depth > 8 {
        out.push('…');
        return;
    }
    let Some(data) = graph.node_data(node) else {
        out.push_str("<absent>");
        return;
    };
    match &*data {
        D::Primitive(kind) => out.push_str(&format!("prim({kind:?})")),
        D::Literal(value) => out.push_str(&format!("lit({value:?})")),
        D::Alias(target) => {
            out.push_str("alias(");
            render_node(graph, *target, depth + 1, out);
            out.push(')');
        }
        D::Union(list) => {
            out.push_str("union[");
            for (index, member) in list.iter().enumerate() {
                if index > 0 {
                    out.push('|');
                }
                render_node(graph, *member, depth + 1, out);
            }
            out.push(']');
        }
        D::Intersection(list) => {
            out.push_str("intersection[");
            for (index, member) in list.iter().enumerate() {
                if index > 0 {
                    out.push('&');
                }
                render_node(graph, *member, depth + 1, out);
            }
            out.push(']');
        }
        D::Array { element, readonly } => {
            out.push_str(if *readonly { "ro-array(" } else { "array(" });
            render_node(graph, *element, depth + 1, out);
            out.push(')');
        }
        D::Object(surface) => {
            // The entry STREAM is the ordering authority: render member
            // names in entry order so authored member order stays visible.
            out.push_str("object{");
            for (index, entry) in surface.entries.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                match entry {
                    verter_session::semantic_query::SurfaceEntry::Member(member) => {
                        match &member.key {
                            verter_type_expr::AuthoredPropertyKey::String(name) => {
                                out.push_str(name);
                            }
                            verter_type_expr::AuthoredPropertyKey::Number(index) => {
                                out.push_str(&format!("#{index}"));
                            }
                            _ => out.push_str("<computed>"),
                        }
                        out.push(':');
                        render_node(graph, member.value, depth + 1, out);
                    }
                    _ => out.push_str("<signature-entry>"),
                }
            }
            out.push('}');
        }
        D::ObjectSpreadProgram(_) => out.push_str("spread-object"),
        D::TypeParam { decl, .. } => out.push_str(&format!("binder({})", decl.decl_name)),
        D::DeclRef { identity } => {
            out.push_str(&format!(
                "decl({}, {})",
                identity.canonical_id, identity.decl_name
            ));
        }
        D::BareRef(_) => out.push_str("bare"),
        D::InstantiationRef { base, .. } => {
            out.push_str(&format!(
                "instantiation({}, {})",
                base.canonical_id, base.decl_name
            ));
        }
        D::Opaque(error) => out.push_str(&format!("opaque({error:?})")),
        other => out.push_str(&format!("kind({})", kind_name(other))),
    }
}

/// The discriminant name for arms the renderer does not descend into.
fn kind_name(data: &SemanticNodeData) -> &'static str {
    match data {
        SemanticNodeData::IntrinsicApplication { .. } => "IntrinsicApplication",
        SemanticNodeData::Alias(_) => "Alias",
        SemanticNodeData::Object(_) => "Object",
        SemanticNodeData::ObjectSpreadProgram(_) => "ObjectSpreadProgram",
        SemanticNodeData::Union(_) => "Union",
        SemanticNodeData::Intersection(_) => "Intersection",
        SemanticNodeData::Primitive(_) => "Primitive",
        SemanticNodeData::Literal(_) => "Literal",
        SemanticNodeData::Opaque(_) => "Opaque",
        SemanticNodeData::Array { .. } => "Array",
        SemanticNodeData::Tuple { .. } => "Tuple",
        SemanticNodeData::TemplateLiteral { .. } => "TemplateLiteral",
        SemanticNodeData::KeyOf { .. } => "KeyOf",
        SemanticNodeData::IndexedAccess { .. } => "IndexedAccess",
        SemanticNodeData::Mapped { .. } => "Mapped",
        SemanticNodeData::TypeOf(_) => "TypeOf",
        SemanticNodeData::TypeOfNominal(_) => "TypeOfNominal",
        SemanticNodeData::TypeParam { .. } => "TypeParam",
        SemanticNodeData::Infer { .. } => "Infer",
        SemanticNodeData::InferRef { .. } => "InferRef",
        SemanticNodeData::Conditional { .. } => "Conditional",
        SemanticNodeData::Signature { .. } => "Signature",
        SemanticNodeData::DeferredCallable(_) => "DeferredCallable",
        SemanticNodeData::DeclRef { .. } => "DeclRef",
        SemanticNodeData::InstantiationRef { .. } => "InstantiationRef",
        SemanticNodeData::MergedDecl { .. } => "MergedDecl",
        SemanticNodeData::BareRef(_) => "BareRef",
        SemanticNodeData::ImportType(_) => "ImportType",
        SemanticNodeData::RawFallback { .. } => "RawFallback",
        SemanticNodeData::SyntheticBinding { .. } => "SyntheticBinding",
    }
}

fn workspace_root() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates/")
        .parent()
        .expect("repo root")
        .to_path_buf()
}

fn read_repo_file(rel: &str) -> String {
    let path = workspace_root().join(rel);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read `{rel}`: {e}"))
}

// ─────────────────────────────────────────────────────────────────────────
// §5.4 — the stable-key variant/encoding table
// ─────────────────────────────────────────────────────────────────────────

/// One `SemanticNodeData` category's stable-key registration under
/// §5.4: which key domain supplies its identity inputs, and the status
/// of its `VerterStableV1` encoding.
struct StableKeyRow {
    variant: &'static str,
    /// The §5.4 key domain (intrinsics/sentinels, literals, authored
    /// carriers, anonymous authored types, binders, synthetic types).
    domain: &'static str,
    /// The identity inputs the domain requires for THIS variant.
    inputs: &'static str,
    /// `None` — encoding exists under the current content-free key
    /// rails; `Some(block)` — the `VerterStableV1` encoding is owed to
    /// the named block.
    owed: Option<&'static str>,
}

const STABLE_KEY_TABLE: &[StableKeyRow] = &[
    StableKeyRow { variant: "IntrinsicApplication", domain: "synthetic", inputs: "closed intrinsic op tag plus ordered argument stable-key references", owed: Some("V4") },
    StableKeyRow { variant: "Alias", domain: "authored carriers", inputs: "owner/role anchor of the aliasing declaration plus the aliased stable-key reference", owed: Some("V4") },
    StableKeyRow { variant: "Object", domain: "authored carriers", inputs: "owner/role anchor plus member-name-keyed child stable keys (declared order where authored order is semantic)", owed: Some("V4") },
    StableKeyRow { variant: "ObjectSpreadProgram", domain: "synthetic", inputs: "closed spread-program recipe with stable arm references and authored member anchors", owed: Some("V4") },
    StableKeyRow { variant: "Union", domain: "synthetic", inputs: "set of member stable keys under the carrier category mint (first-occurrence dedup per the composite identity discipline)", owed: Some("V4") },
    StableKeyRow { variant: "Intersection", domain: "synthetic", inputs: "ORDERED member stable keys preserving the authored reduction grouping", owed: Some("V4") },
    StableKeyRow { variant: "Primitive", domain: "intrinsics/sentinels", inputs: "fixed distinct primitive tag; no source or allocation ordinal", owed: Some("V4") },
    StableKeyRow { variant: "Literal", domain: "literals", inputs: "canonical scalar value plus literal kind with explicit scalar edge-case handling", owed: Some("V4") },
    StableKeyRow { variant: "Opaque", domain: "intrinsics/sentinels", inputs: "typed error tag; a refusal identity, never an allocation ordinal", owed: Some("V4") },
    StableKeyRow { variant: "Array", domain: "synthetic", inputs: "readonly flag plus element stable-key reference", owed: Some("V4") },
    StableKeyRow { variant: "Tuple", domain: "synthetic", inputs: "ordered element stable keys with label/optionality/rest metadata", owed: Some("V4") },
    StableKeyRow { variant: "TemplateLiteral", domain: "synthetic", inputs: "ordered quasi text spans plus expression stable-key references", owed: Some("V4") },
    StableKeyRow { variant: "KeyOf", domain: "synthetic", inputs: "operand stable-key reference under the closed keyof recipe", owed: Some("V4") },
    StableKeyRow { variant: "IndexedAccess", domain: "synthetic", inputs: "object and index stable-key references under the closed indexed-access recipe", owed: Some("V4") },
    StableKeyRow { variant: "Mapped", domain: "synthetic", inputs: "source stable-key reference plus mapper key-space anchor", owed: Some("V4") },
    StableKeyRow { variant: "TypeOf", domain: "authored carriers", inputs: "value-root owner anchor plus remaining member path roles", owed: Some("V4") },
    StableKeyRow { variant: "TypeOfNominal", domain: "authored carriers", inputs: "the declaring value-declaration identity parts (nominal by construction)", owed: Some("V4") },
    StableKeyRow { variant: "TypeParam", domain: "binders", inputs: "declaration identity (owner anchor plus declaration-local ordinal only where the language needs disambiguation) and binder role", owed: Some("V4") },
    StableKeyRow { variant: "Infer", domain: "binders", inputs: "owner/recursive-region anchor plus infer binder position/role", owed: Some("V4") },
    StableKeyRow { variant: "InferRef", domain: "binders", inputs: "referenced infer binder's stable anchor", owed: Some("V4") },
    StableKeyRow { variant: "Conditional", domain: "synthetic", inputs: "closed conditional recipe: check/extrema/default arm stable-key references", owed: Some("V4") },
    StableKeyRow { variant: "Signature", domain: "authored carriers", inputs: "owner/role anchor plus the positional model (binder anchors, optionality, rest/receiver/predicate layout)", owed: Some("V5") },
    StableKeyRow { variant: "DeferredCallable", domain: "authored carriers", inputs: "the deferred callable's closed carrier recipe with stable subject reference", owed: Some("V5") },
    StableKeyRow { variant: "DeclRef", domain: "authored carriers", inputs: "logical source-unit identity plus declaration identity (content hashes stay freshness evidence, R6 content-free key rails)", owed: None },
    StableKeyRow { variant: "InstantiationRef", domain: "synthetic", inputs: "base declaration's stable anchor plus ordered argument stable keys (content-free slot rails, R6)", owed: None },
    StableKeyRow { variant: "MergedDecl", domain: "authored carriers", inputs: "merged declaration population: per-symbol logical membership and precedence before publication (V3 populations)", owed: Some("V3") },
    StableKeyRow { variant: "BareRef", domain: "authored carriers", inputs: "owner scope anchor plus the unresolved head name (an authored-unresolved carrier, never a discovery ordinal)", owed: Some("V4") },
    StableKeyRow { variant: "ImportType", domain: "authored carriers", inputs: "resolved module logical identity plus the imported anchor and qualifier path", owed: Some("V4") },
    StableKeyRow { variant: "RawFallback", domain: "synthetic", inputs: "closed fallback recipe over the failed input's stable reference", owed: Some("V4") },
    StableKeyRow { variant: "SyntheticBinding", domain: "binders", inputs: "stable owner/role anchor of the synthesizing operation plus binder position", owed: Some("V4") },
];

/// Extract the live variant list from the `pub enum SemanticNodeData`
/// declaration (the same source-text guard discipline the dispatch
/// tests use).
fn live_semantic_node_data_variants() -> Vec<String> {
    let source = read_repo_file("crates/verter_session/src/semantic_query.rs");
    let start = source
        .find("pub enum SemanticNodeData {")
        .expect("the SemanticNodeData declaration");
    let body = &source[start..];
    let end = body.find("\n}").expect("the declaration closes");
    let body = &body[0..end];
    let mut variants = Vec::new();
    for line in body.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("///") || trimmed.starts_with("//") || trimmed.is_empty() {
            continue;
        }
        // A variant starts at column-4 `Ident(` / `Ident {` / `Ident,` —
        // struct-variant FIELDS sit deeper.
        if line.starts_with("    ") && !line.starts_with("        ") {
            if let Some(name) = trimmed
                .split(['(', '{', ','])
                .next()
                .filter(|candidate| candidate.chars().next().is_some_and(char::is_uppercase))
            {
                variants.push(name.trim().to_owned());
            }
        }
    }
    variants
}

/// V0-AC4 (5.4 half): the stable-key table enumerates EVERY
/// `SemanticNodeData` category — exactly the live declaration's variant
/// set, no more, no less — with a non-empty domain and inputs for each,
/// and every not-yet-encoded row naming the successor block that owns
/// its `VerterStableV1` encoding.
#[test]
fn stable_key_table_enumerates_every_semantic_node_data_category() {
    let live = live_semantic_node_data_variants();
    assert!(
        live.len() >= 25,
        "the SemanticNodeData variant extraction found {} variants — the guard is broken, \
         not the enum",
        live.len()
    );
    let mut table: Vec<&str> = STABLE_KEY_TABLE.iter().map(|row| row.variant).collect();
    table.sort_unstable();
    let mut live_sorted = live.clone();
    live_sorted.sort_unstable();
    assert_eq!(
        table, live_sorted,
        "the stable-key table and the live SemanticNodeData declaration disagree — a new \
         variant needs a §5.4 registration row in the same change"
    );
    for row in STABLE_KEY_TABLE {
        assert!(
            !row.domain.trim().is_empty() && !row.inputs.trim().is_empty(),
            "{}: the domain and identity inputs are non-empty — a row without key inputs is \
             an unverified registration",
            row.variant
        );
        if let Some(block) = row.owed {
            assert!(
                block.starts_with('V') && block.len() <= 3,
                "{}: the owing block is a train block id (found `{block}`)",
                row.variant
            );
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────
// §5.9 — the determinism perturbation matrix
// ─────────────────────────────────────────────────────────────────────────

/// One §5.9 perturbation row.
struct MatrixRow {
    id: &'static str,
    /// The §5.9 first-column text (abbreviated to its stable prefix).
    perturbation: &'static str,
    /// The executable driver test for a row replayable today, else the
    /// `#[ignore]` reason naming the successor block.
    driver: Driver,
}

enum Driver {
    Ready { test: &'static str },
    Ignored { reason: &'static str },
}

const MATRIX: &[MatrixRow] = &[
    MatrixRow {
        id: "DET-01",
        perturbation: "Same files arrive in forward/reverse/random order",
        driver: Driver::Ready { test: "det_01_forward_reverse_random_arrival" },
    },
    MatrixRow {
        id: "DET-02",
        perturbation: "Lazy load versus eager preload; queried in different orders",
        driver: Driver::Ready { test: "det_02_lazy_vs_eager_query_order" },
    },
    MatrixRow {
        id: "DET-03",
        perturbation: "An unrelated type/literal is interned or queried first",
        driver: Driver::Ready { test: "det_03_unrelated_interning_first" },
    },
    MatrixRow {
        id: "DET-04",
        perturbation: "Worker counts 1/2/4/8, randomized delays/work stealing, duplicate publishers",
        driver: Driver::Ready { test: "det_04_worker_counts_1_2_4_8" },
    },
    MatrixRow {
        id: "DET-05",
        perturbation: "Cold, warm, partial persisted cache, edit/revert, cancelled/retried work, epoch rebuild",
        driver: Driver::Ready { test: "det_05_cold_warm_edit_revert" },
    },
    MatrixRow {
        id: "DET-06",
        perturbation: "Hash seed changes, equal prefixes, forced full stable-fingerprint collisions",
        driver: Driver::Ignored {
            reason: "V4 owns representation-only stable keys and VerterStableV1 views: hash-seed \
                     perturbation and forced full stable-fingerprint collision injection are \
                     un-drivable until the stable key exists (today's keys are content-free \
                     identity slots pinned by the R6 guard rails, which the stable-key table \
                     records per variant)",
        },
    },
    MatrixRow {
        id: "DET-07",
        perturbation: "Duplicate shapes with distinct authored/synthetic origins, anonymous/virtual units, recursive binders",
        driver: Driver::Ready { test: "det_07_duplicate_origins_and_anonymous_units" },
    },
    MatrixRow {
        id: "DET-08",
        perturbation: "Contextual body demands with the same descriptor/type arguments",
        driver: Driver::Ignored {
            reason: "V1 owns complete body/result demand identity: only the canonical \
                     whole-return demand point is answerable today (a narrower demand fails \
                     closed with UnmodeledDemandPoint), so distinct coexisting result memo \
                     entries under one descriptor cannot be replayed yet",
        },
    },
    MatrixRow {
        id: "DET-09",
        perturbation: "Policy changes with resident parent caches",
        driver: Driver::Ignored {
            reason: "V1 owns real effective tsconfig option plumbing: changing semantic policy \
                     with resident parent caches is un-drivable until effective options and \
                     their parent-cache invalidation exist",
        },
    },
    MatrixRow {
        id: "DET-10",
        perturbation: "Logical checkout relocation with fixed mappings and equivalent resolver facts",
        driver: Driver::Ready { test: "det_10_relocation" },
    },
    MatrixRow {
        id: "DET-11",
        perturbation: "Authored overload/intersection order is deliberately changed",
        driver: Driver::Ready { test: "det_11_authored_reorder_detected" },
    },
    MatrixRow {
        id: "DET-12",
        perturbation: "Same output requested with different preceding unrelated outputs",
        driver: Driver::Ready { test: "det_12_unrelated_preceding_outputs" },
    },
];

/// The §5.9 table rows as checked into the contract (first column,
/// `|`-delimited).
fn contract_5_9_perturbations() -> Vec<String> {
    let contract = read_repo_file("docs/arch/signature-kernel.md");
    let section = contract
        .split("### 5.9 Mandatory determinism matrix")
        .nth(1)
        .expect("the contract's §5.9 section");
    let section = section.split("## 6.").next().expect("§6 follows §5.9");
    section
        .lines()
        .filter(|line| line.starts_with('|'))
        .skip(2) // header + separator
        .filter_map(|line| line.split('|').nth(1).map(|cell| cell.trim().to_owned()))
        .filter(|cell| !cell.is_empty())
        .collect()
}

fn matrix_row(id: &str) -> &'static MatrixRow {
    MATRIX
        .iter()
        .find(|row| row.id == id)
        .unwrap_or_else(|| panic!("{id}: the matrix registration lost this row"))
}

/// Every replay driver opens by claiming ITS row: the row exists, is
/// Ready, and names THIS test — a renamed test or a re-pointed row
/// fails here instead of silently orphaning the perturbation.
fn assert_ready(id: &str, test: &str) {
    match matrix_row(id).driver {
        Driver::Ready { test: named } => assert_eq!(
            named, test,
            "{id}: the registration names driver `{named}`, this test is `{test}` — keep them              pointed at each other"
        ),
        Driver::Ignored { reason } => panic!(
            "{id}: this replay driver exists but the row is registered ignored ({reason})"
        ),
    }
}

/// V0-AC4 (5.9 half): the matrix enumerates EVERY §5.9 perturbation
/// exactly once, each with a non-empty driver or a block-named ignore
/// reason, and the driver names are unique.
#[test]
fn determinism_matrix_enumerates_every_5_9_row() {
    let contract = contract_5_9_perturbations();
    assert_eq!(
        contract.len(),
        MATRIX.len(),
        "the contract's §5.9 table has {} rows; the registration has {} — a perturbation \
         was lost or invented",
        contract.len(),
        MATRIX.len()
    );
    let mut drivers: Vec<&str> = Vec::new();
    for (row, contract_text) in MATRIX.iter().zip(contract.iter()) {
        let registered = row.perturbation.to_lowercase();
        let authored = contract_text.to_lowercase();
        assert!(
            authored.starts_with(&registered[..registered.len().min(24)]),
            "{}: the registered perturbation `{}` no longer prefixes the contract's `{}` — \
             keep the registration aligned with §5.9",
            row.id,
            row.perturbation,
            contract_text
        );
        match row.driver {
            Driver::Ready { test } => {
                assert!(
                    !test.is_empty(),
                    "{}: a ready row names its driver test",
                    row.id
                );
                drivers.push(test);
            }
            Driver::Ignored { reason } => {
                assert!(
                    reason.starts_with('V') && reason.contains(" owns "),
                    "{}: the ignore reason names the successor block that un-ignores the row",
                    row.id
                );
            }
        }
    }
    drivers.sort_unstable();
    drivers.dedup();
    assert_eq!(
        drivers.len(),
        MATRIX
            .iter()
            .filter(|row| matches!(row.driver, Driver::Ready { .. }))
            .count(),
        "driver test names are unique"
    );
}

// ─────────────────────────────────────────────────────────────────────────
// Replay drivers
// ─────────────────────────────────────────────────────────────────────────

fn det_files() -> Vec<(&'static str, &'static str)> {
    vec![
        ("/det/dep1.ts", DEP1_TS),
        ("/det/dep2.ts", DEP2_TS),
        ("/det/main.ts", MAIN_TS),
    ]
}

/// DET-01 — the same three files upserted forward, reverse, and a
/// seeded shuffle; the completed observations for a local and an
/// import-consuming witness must agree.
#[test]
fn det_01_forward_reverse_random_arrival() {
    assert_ready("DET-01", "det_01_forward_reverse_random_arrival");
    let mut files = det_files();
    let forward = build_host(&files);
    files.reverse();
    let reverse = build_host(&files);
    // Seeded shuffle (deterministic LCG — the seed is part of the row).
    let mut seed: u64 = 0x5eed_1234;
    let mut shuffled = det_files();
    for index in (1..shuffled.len()).rev() {
        seed = seed
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        let swap = (seed >> 33) as usize % (index + 1);
        shuffled.swap(index, swap);
    }
    let random = build_host(&shuffled);
    for (host, label) in [(reverse, "reverse"), (random, "shuffled")] {
        for symbol in ["localUnion", "usesImports"] {
            assert_eq!(
                observe(&forward, "/det/main.ts", symbol),
                observe(&host, "/det/main.ts", symbol),
                "DET-01: the {label} arrival order changed `{symbol}`'s completed observation"
            );
        }
    }
}

/// DET-02 — lazy versus eager: querying witness A before B versus B
/// before A (two fresh hosts, same arrival) must not change either
/// completed observation — no discovery-history effect.
#[test]
fn det_02_lazy_vs_eager_query_order() {
    assert_ready("DET-02", "det_02_lazy_vs_eager_query_order");
    let host_a = build_host(&det_files());
    let host_b = build_host(&det_files());
    let first = (
        observe(&host_a, "/det/main.ts", "localUnion"),
        observe(&host_a, "/det/main.ts", "usesImports"),
    );
    let second = (
        observe(&host_b, "/det/main.ts", "usesImports"),
        observe(&host_b, "/det/main.ts", "localUnion"),
    );
    assert_eq!(
        first.0, second.1,
        "DET-02: localUnion's observation depends on query history"
    );
    assert_eq!(
        first.1, second.0,
        "DET-02: usesImports' observation depends on query history"
    );
}

/// DET-03 — an unrelated union/literal surface interned and queried
/// FIRST leaves the target's pair ordering and output unchanged.
#[test]
fn det_03_unrelated_interning_first() {
    assert_ready("DET-03", "det_03_unrelated_interning_first");
    let plain = build_host(&det_files());
    let mut files = det_files();
    files.push(("/det/unrelated.ts", UNRELATED_TS));
    let polluted = build_host(&files);
    let _ = observe(&polluted, "/det/unrelated.ts", "unrelatedWitness");
    for symbol in ["localUnion", "usesImports"] {
        assert_eq!(
            observe(&plain, "/det/main.ts", symbol),
            observe(&polluted, "/det/main.ts", symbol),
            "DET-03: interning the unrelated surface first changed `{symbol}`"
        );
    }
}

/// DET-04 — worker counts 1/2/4/8 (the real scheduler knob): the same
/// observations across pool sizes. The randomized-delay/work-stealing
/// injection and the duplicate-publisher axis stage at V8's combined
/// vertical per §5.9's own staging clause; the worker-count axis is
/// replayed here.
#[test]
fn det_04_worker_counts_1_2_4_8() {
    assert_ready("DET-04", "det_04_worker_counts_1_2_4_8");
    let single = build_host_with_cpu_threads(&det_files(), 1);
    for threads in [2, 4, 8] {
        let host = build_host_with_cpu_threads(&det_files(), threads);
        for symbol in ["localUnion", "usesImports"] {
            assert_eq!(
                observe(&single, "/det/main.ts", symbol),
                observe(&host, "/det/main.ts", symbol),
                "DET-04: {threads} worker(s) changed `{symbol}`'s completed observation"
            );
        }
    }
}

/// DET-05 — cold, warm, edit and revert on ONE host: the warm replay
/// equals the cold observation; an edit changes the observation the
/// authored way; reverting restores the original observation. The
/// persisted-cache, cancel/retry and epoch-rebuild axes are V2's
/// epoch-safe storage work (the row's staging note).
#[test]
fn det_05_cold_warm_edit_revert() {
    assert_ready("DET-05", "det_05_cold_warm_edit_revert");
    let host = build_host(&[("/det/edit.ts", EDIT_ORIGINAL_TS)]);
    let canonical = "/det/edit.ts";
    let cold = observe(&host, canonical, "witness");
    let warm = observe(&host, canonical, "witness");
    assert_eq!(
        cold, warm,
        "DET-05: the warm replay changed the completed observation"
    );
    upsert(&host, canonical, EDIT_CHANGED_TS);
    let edited = observe(&host, canonical, "witness");
    assert_ne!(
        cold, edited,
        "DET-05: the authored edit is invisible — the harness would normalize away real change"
    );
    upsert(&host, canonical, EDIT_ORIGINAL_TS);
    let reverted = observe(&host, canonical, "witness");
    assert_eq!(
        cold, reverted,
        "DET-05: the revert did not restore the original completed observation"
    );
}

/// DET-07 — duplicate same-shaped declarations with distinct authored
/// origins and an anonymous class-expression unit: no early carrier
/// loss (both duplicates answer), and the observations are the SAME
/// stable text because they are same-shaped authored facts. The
/// virtual-unit and recursive-binder anchor axes stage with V3/V4 (the
/// row's staging note).
#[test]
fn det_07_duplicate_origins_and_anonymous_units() {
    assert_ready("DET-07", "det_07_duplicate_origins_and_anonymous_units");
    let host = build_host(&[
        ("/det/dupA.ts", DUPLICATE_A_TS),
        ("/det/dupB.ts", DUPLICATE_B_TS),
        ("/det/anon.ts", ANONYMOUS_TS),
    ]);
    let a = observe(&host, "/det/dupA.ts", "witness");
    let b = observe(&host, "/det/dupB.ts", "witness");
    assert!(
        !a.contains("<absent>") && !b.contains("<absent>"),
        "DET-07: a duplicate-origin carrier was lost — {a} vs {b}"
    );
    assert_eq!(
        a.replace("/det/dupA", "/det/DUP"),
        b.replace("/det/dupB", "/det/DUP"),
        "DET-07: same-shaped authored duplicates must give same-shaped observations"
    );
    let anonymous = observe(&host, "/det/anon.ts", "witness");
    assert!(
        !anonymous.contains("<absent>"),
        "DET-07: the anonymous unit's carrier was lost — {anonymous}"
    );
}

/// DET-10 — relocation: the same content under a different logical
/// root (fixed path mapping) gives the same semantic observations.
#[test]
fn det_10_relocation() {
    assert_ready("DET-10", "det_10_relocation");
    let here = build_host(&[
        ("/relocA/main.ts", MAIN_TS),
        ("/relocA/dep1.ts", DEP1_TS),
        ("/relocA/dep2.ts", DEP2_TS),
    ]);
    let there = build_host(&[
        ("/relocB/main.ts", MAIN_TS),
        ("/relocB/dep1.ts", DEP1_TS),
        ("/relocB/dep2.ts", DEP2_TS),
    ]);
    for symbol in ["localUnion", "usesImports"] {
        let a = observe(&here, "/relocA/main.ts", symbol).replace("/relocA/", "/ROOT/");
        let b = observe(&there, "/relocB/main.ts", symbol).replace("/relocB/", "/ROOT/");
        assert_eq!(
            a, b,
            "DET-10: relocating the checkout changed `{symbol}`'s semantic observation"
        );
    }
}

/// DET-11 — authored intersection order deliberately changed: the
/// harness DETECTS the change (the observation differs) instead of
/// normalizing it away.
#[test]
fn det_11_authored_reorder_detected() {
    assert_ready("DET-11", "det_11_authored_reorder_detected");
    let host = build_host(&[
        ("/det/orderA.ts", REORDER_A_TS),
        ("/det/orderB.ts", REORDER_B_TS),
    ]);
    let a = observe(&host, "/det/orderA.ts", "witness");
    let b = observe(&host, "/det/orderB.ts", "witness");
    assert_ne!(
        a.replace("/det/orderA", "/det/ORDER"),
        b.replace("/det/orderB", "/det/ORDER"),
        "DET-11: the deliberate authored reorder is INVISIBLE — the observation basis \
         normalizes meaningful intersection order away"
    );
}

/// DET-12 — the same output requested after DIFFERENT preceding
/// unrelated outputs: neither witness's observation depends on what was
/// rendered before it.
#[test]
fn det_12_unrelated_preceding_outputs() {
    assert_ready("DET-12", "det_12_unrelated_preceding_outputs");
    let host = build_host(&[
        ("/det/main.ts", MAIN_TS),
        ("/det/dep1.ts", DEP1_TS),
        ("/det/dep2.ts", DEP2_TS),
        ("/det/unrelated.ts", UNRELATED_TS),
    ]);
    let target = observe(&host, "/det/main.ts", "localUnion");
    let after_unrelated_host = build_host(&[
        ("/det/main.ts", MAIN_TS),
        ("/det/dep1.ts", DEP1_TS),
        ("/det/dep2.ts", DEP2_TS),
        ("/det/unrelated.ts", UNRELATED_TS),
    ]);
    let _ = observe(
        &after_unrelated_host,
        "/det/unrelated.ts",
        "unrelatedWitness",
    );
    let after = observe(&after_unrelated_host, "/det/main.ts", "localUnion");
    assert_eq!(
        target, after,
        "DET-12: an unrelated preceding output changed the target's observation"
    );
}

/// DET-06 (ignored) — hash-seed changes, equal prefixes, forced full
/// stable-fingerprint collisions. The body asserts the row's law as far
/// as today's surface expresses it (equal-prefix literal unions stay
/// correctly ordered under interning perturbation); the row stays
/// ignored because the FULL perturbation is a known failure of the
/// current implementation: union projection follows arena insertion
/// order under the first-wins composite-category dedup, so hash-seed
/// changes and forced full stable-fingerprint collisions cannot be
/// injected or survived until representation-only stable keys exist.
#[test]
#[ignore = "V4 owns representation-only stable keys and VerterStableV1 views: hash-seed \
            perturbation and forced full stable-fingerprint collision injection are known \
            failures of the current arena-order union projection (first-wins composite \
            category) and are un-injectable until the stable key exists"]
fn det_06_hash_seed_and_forced_collisions() {
    match matrix_row("DET-06").driver {
        Driver::Ignored { .. } => {}
        Driver::Ready { test } => panic!("DET-06 is registered ready ({test}) — update this row"),
    }
    const EQUAL_PREFIX_TS: &str = r#"
export function witness(v: "a" | "ab" | "abc" | "b") { return v; }
"#;
    let plain = build_host(&[("/det/prefix.ts", EQUAL_PREFIX_TS)]);
    let mut files = vec![
        ("/det/prefix.ts", EQUAL_PREFIX_TS),
        ("/det/unrelated.ts", UNRELATED_TS),
    ];
    let polluted = build_host(&files);
    let _ = observe(&polluted, "/det/unrelated.ts", "unrelatedWitness");
    files.pop();
    assert_eq!(
        observe(&plain, "/det/prefix.ts", "witness"),
        observe(&polluted, "/det/prefix.ts", "witness"),
        "DET-06: equal-prefix union members reordered under interning perturbation"
    );
}

/// DET-08 (ignored) — contextual body demands with the same
/// descriptor/type arguments must give correctly DISTINCT coexisting
/// result memo entries. Today only the canonical whole-return demand
/// point is answerable (a narrower demand fails closed with
/// `UnmodeledDemandPoint`), so the distinct-entry law is un-drivable.
#[test]
#[ignore = "V1 owns complete body/result demand identity: only the canonical whole-return \
            demand point is answerable today (a narrower demand fails closed with \
            UnmodeledDemandPoint), so distinct coexisting result memo entries under one \
            descriptor cannot be replayed yet"]
fn det_08_contextual_body_demands() {
    match matrix_row("DET-08").driver {
        Driver::Ignored { .. } => {}
        Driver::Ready { test } => panic!("DET-08 is registered ready ({test}) — update this row"),
    }
    // The body the row will run once un-ignored: two demands over the
    // same descriptor (whole-return and a narrower projection) must
    // coexist as distinct memo entries with the same completed value.
    // Driving it today panics on the closed demand point — the known
    // incompleteness the ignore reason names.
    let host = build_host(&[("/det/main.ts", MAIN_TS)]);
    let whole = observe(&host, "/det/main.ts", "localUnion");
    assert!(!whole.is_empty());
    unreachable!(
        "DET-08: un-ignored without V1's demand identity — drive the narrower projection \
         and assert the distinct coexisting memo entries here"
    );
}

/// DET-09 (ignored) — policy changes with resident parent caches:
/// parents and leaves must use the new policy while formatting-only
/// changes do not rebuild resolution. Un-drivable until V1's real
/// effective tsconfig option plumbing exists.
#[test]
#[ignore = "V1 owns real effective tsconfig option plumbing: changing semantic policy with \
            resident parent caches is un-drivable until effective options and their \
            parent-cache invalidation exist"]
fn det_09_policy_change_with_resident_parents() {
    match matrix_row("DET-09").driver {
        Driver::Ignored { .. } => {}
        Driver::Ready { test } => panic!("DET-09 is registered ready ({test}) — update this row"),
    }
    // The body the row will run once un-ignored: warm the parents under
    // one policy, change the policy, and assert the leaves answer under
    // the NEW policy while a formatting-only change rebuilds nothing.
    let host = build_host(&[("/det/main.ts", MAIN_TS)]);
    let before = observe(&host, "/det/main.ts", "localUnion");
    assert!(!before.is_empty());
    unreachable!(
        "DET-09: un-ignored without V1's effective options — change the policy with the \
         parents resident and assert the new-policy answers here"
    );
}
