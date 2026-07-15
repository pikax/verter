//! Static contract guards for the typeinfo request surface.
//!
//! These guards pin the request-envelope contract: every graph request
//! carries a schema version and a projection mode; the closed
//! `TypeInfoRequestError` union is consistent across the proto and the
//! validator; the seven graph-payload request types carry
//! `context + closure + displayPolicy` (with `flowNarrowing` /
//! `contextualType` the two NAMED closure exemptions); `listSymbols` is
//! a scalar list; `relate` carries no closure; and every closure variant
//! has a concrete resource bound.
//!
//! Two shapes:
//! - BEHAVIORAL validator tests (the validator is in scope): construct a
//!   request and assert the validator's typed `TypeInfoRequestError`.
//! - STATIC surface pins (proto / source text): assert the DTO carries
//!   (or omits) a field. These pin the static request surface ONLY —
//!   the runtime path-projection cascade and schema-version
//!   echo/negotiation are later blocks.

use std::path::PathBuf;

use verter_protocol::typeinfo::graph as wire;
use verter_protocol::typeinfo::graph::TYPEINFO_GRAPH_SCHEMA_VERSION;
use verter_protocol::verter::v1::{
    graph_closure_policy, type_info_graph_request as wire_request, type_info_request_error,
};
use verter_session::typeinfo::request_validation::{
    validate_resolve_symbol_graph_request, validate_type_info_graph_request,
    MAX_EXPANSION_DEPTH_BUDGET, MAX_EXPANSION_NODE_BUDGET,
    SUPPORTED_TYPEINFO_GRAPH_SCHEMA_VERSIONS,
};

fn workspace_root() -> PathBuf {
    let crate_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    crate_root
        .parent()
        .and_then(|p| p.parent())
        .expect("CARGO_MANIFEST_DIR must be `<workspace>/crates/verter_session`")
        .to_path_buf()
}

fn read_proto() -> String {
    let path = workspace_root().join("crates/verter_protocol/proto/verter/v1/typeinfo.proto");
    std::fs::read_to_string(&path)
        .unwrap_or_else(|err| panic!("file {} should be readable: {err}", path.display()))
}

// ───────────────────────── request construction ──────────────────────────

fn default_context() -> wire::ProjectionReductionContext {
    wire::ProjectionReductionContext {
        mode: wire::ProjectionMode::Expanded as i32,
        demand: wire::ReductionDemand::Published as i32,
    }
}

fn one_level_closure() -> wire::ClosurePolicy {
    wire::ClosurePolicy {
        kind: Some(graph_closure_policy::Kind::OneLevel(
            wire::ClosureOneLevel {},
        )),
    }
}

fn expanded_closure(node_budget: u32, depth_budget: u32) -> wire::ClosurePolicy {
    wire::ClosurePolicy {
        kind: Some(graph_closure_policy::Kind::Expanded(
            wire::ClosureExpanded {
                node_budget,
                depth_budget,
            },
        )),
    }
}

fn default_display_policy() -> wire::DisplayPolicy {
    wire::DisplayPolicy {
        qualification: wire::DisplayQualification::Qualified as i32,
        branding: wire::DisplayBranding::On as i32,
        budgets: Some(wire::DisplayBudgets {
            max_string_length: 4096,
            max_depth: 16,
        }),
    }
}

fn resolve_symbol_request_with(
    context: Option<wire::ProjectionReductionContext>,
    closure: Option<wire::ClosurePolicy>,
) -> wire::ResolveSymbolGraphRequest {
    wire::ResolveSymbolGraphRequest {
        canonical_id: "/a.ts".to_string(),
        name: "Foo".to_string(),
        context,
        closure,
        display_policy: Some(default_display_policy()),
        include_provenance: false,
        include_diagnostics: false,
        include_projection: vec![],
        include_degraded: false,
    }
}

fn valid_resolve_symbol_request() -> wire::ResolveSymbolGraphRequest {
    resolve_symbol_request_with(Some(default_context()), Some(one_level_closure()))
}

fn graph_request(
    schema_version: u32,
    payload: wire::ResolveSymbolGraphRequest,
) -> wire::TypeInfoGraphRequest {
    wire::TypeInfoGraphRequest {
        schema_version,
        operation: wire::Operation::ResolveSymbol as i32,
        payload: Some(wire_request::Payload::ResolveSymbol(payload)),
    }
}

/// Label the active `TypeInfoRequestError` variant, exhaustive over the
/// closed union (a new error arm forces this match to change).
fn err_label(err: &wire::TypeInfoRequestError) -> &'static str {
    match err.kind.as_ref().expect("error must carry a kind") {
        type_info_request_error::Kind::MissingProjectionContext(_) => "MissingProjectionContext",
        type_info_request_error::Kind::MissingDisplayPolicy(_) => "MissingDisplayPolicy",
        type_info_request_error::Kind::InvalidMode(_) => "InvalidMode",
        type_info_request_error::Kind::MissingClosurePolicy(_) => "MissingClosurePolicy",
        type_info_request_error::Kind::UnknownSchemaVersion(_) => "UnknownSchemaVersion",
        type_info_request_error::Kind::MalformedPayload(_) => "MalformedPayload",
        type_info_request_error::Kind::OmittedRoots(_) => "OmittedRoots",
        type_info_request_error::Kind::UnstableState(_) => "UnstableState",
        type_info_request_error::Kind::MalformedStructuredExpression(_) => {
            "MalformedStructuredExpression"
        }
        type_info_request_error::Kind::MissingProjectPath(_) => "MissingProjectPath",
        type_info_request_error::Kind::ExpansionBudgetOutOfRange(_) => "ExpansionBudgetOutOfRange",
    }
}

// ───────────────────── behavioral: mode + schema-version ──────────────────

#[test]
fn typeinfo_request_validates_mode_present() {
    // A request whose projection context is absent fails with
    // MissingProjectionContext; a context present but carrying an invalid
    // projection mode discriminant fails with InvalidMode. The validator
    // decodes the mode BEFORE any semantic execution. Discriminating:
    // both error labels are asserted against constructed requests.
    let missing_ctx = validate_resolve_symbol_graph_request(&resolve_symbol_request_with(
        None,
        Some(one_level_closure()),
    ))
    .expect_err("missing projection context must be rejected");
    assert_eq!(
        err_label(&missing_ctx),
        "MissingProjectionContext",
        "a request with no projection context must fail validation with MissingProjectionContext",
    );

    // Present context, but the mode discriminant is an out-of-range
    // integer that decodes to no `ProjectionMode` → InvalidMode.
    let invalid_mode_ctx = wire::ProjectionReductionContext {
        mode: 9999,
        demand: wire::ReductionDemand::Published as i32,
    };
    let invalid_mode = validate_resolve_symbol_graph_request(&resolve_symbol_request_with(
        Some(invalid_mode_ctx),
        Some(one_level_closure()),
    ))
    .expect_err("invalid projection mode must be rejected");
    assert_eq!(
        err_label(&invalid_mode),
        "InvalidMode",
        "a request whose projection mode does not decode must fail with InvalidMode",
    );

    // Positive: a well-formed request with a present, valid mode passes.
    validate_resolve_symbol_graph_request(&valid_resolve_symbol_request())
        .expect("a well-formed request with a present valid mode must validate");
}

#[test]
fn every_typeinfo_request_carries_schema_version() {
    // The top-level graph request carries `schema_version` and validation
    // gates it against the closed supported set BEFORE semantic dispatch.
    // A request below the supported minimum (0) is rejected with
    // UnknownSchemaVersion; a contemporary version validates.
    let stale = validate_type_info_graph_request(&graph_request(0, valid_resolve_symbol_request()))
        .expect_err("schema_version 0 must be rejected before dispatch");
    assert_eq!(
        err_label(&stale),
        "UnknownSchemaVersion",
        "a request below the supported schema-version set must fail with UnknownSchemaVersion",
    );

    validate_type_info_graph_request(&graph_request(
        TYPEINFO_GRAPH_SCHEMA_VERSION,
        valid_resolve_symbol_request(),
    ))
    .expect("a request at the current schema version must validate");

    // A future version (current + 1) is NOT in the closed supported set
    // and must fail rather than reach dispatch.
    let future = validate_type_info_graph_request(&graph_request(
        TYPEINFO_GRAPH_SCHEMA_VERSION + 1,
        valid_resolve_symbol_request(),
    ))
    .expect_err("a future schema_version must be rejected before dispatch");
    assert_eq!(
        err_label(&future),
        "UnknownSchemaVersion",
        "a request above the supported schema-version set must fail with UnknownSchemaVersion",
    );
}

#[test]
fn unknown_schema_version_shape_uniform_across_plan() {
    // When a schema version is rejected, the UnknownSchemaVersion error
    // is shaped uniformly: it echoes the client's `wire_version`, the
    // `server_version`, and the closed `server_supported_versions` set —
    // the same three fields everywhere this error is raised. A client
    // that decoded the original schema can always intersect its own
    // supported set against `server_supported_versions`.
    let err = validate_type_info_graph_request(&graph_request(0, valid_resolve_symbol_request()))
        .expect_err("schema_version 0 must be rejected");
    let payload = match err.kind.as_ref().expect("error kind") {
        type_info_request_error::Kind::UnknownSchemaVersion(p) => p,
        other => panic!("expected UnknownSchemaVersion, got {other:?}"),
    };
    assert_eq!(
        payload.wire_version, 0,
        "UnknownSchemaVersion must echo the client's wire_version (0)",
    );
    assert_eq!(
        payload.server_version, TYPEINFO_GRAPH_SCHEMA_VERSION,
        "UnknownSchemaVersion must report the server's current schema version",
    );
    assert_eq!(
        payload.server_supported_versions, SUPPORTED_TYPEINFO_GRAPH_SCHEMA_VERSIONS,
        "UnknownSchemaVersion must report the closed supported-version set so the \
         client can intersect against it",
    );

    // The capability handshake response carries the SAME closed set —
    // uniform shape across the request-error and the handshake surfaces.
    let proto = read_proto();
    let handshake = request_message_body(&proto, "TypeInfoCapabilityHandshakeResponse");
    assert!(
        handshake.contains("repeated uint32 server_supported_versions ="),
        "the capability handshake response must expose the same \
         `server_supported_versions` set the UnknownSchemaVersion error reports",
    );
}

// ───────────────────── behavioral: closure resource bounds ────────────────

#[test]
fn every_closure_variant_has_concrete_resource_bound() {
    // The only closure variant carrying caller-supplied numeric budgets
    // is `Expanded` (node_budget / depth_budget). It is bounded by
    // MAX_EXPANSION_NODE_BUDGET / MAX_EXPANSION_DEPTH_BUDGET, and an
    // all-zero budget is rejected (an unbounded expansion). The other
    // four closure variants (RootOnly / Path / OneLevel /
    // ProjectionRequired) are STRUCTURALLY bounded — they carry no
    // open-ended numeric budget. This guard pins the bound on Expanded
    // behaviorally and pins the structural variants by proto shape.

    // (1) An Expanded closure over the node-budget ceiling is rejected.
    let over_node = validate_resolve_symbol_graph_request(&resolve_symbol_request_with(
        Some(default_context()),
        Some(expanded_closure(MAX_EXPANSION_NODE_BUDGET + 1, 8)),
    ))
    .expect_err("node_budget over the ceiling must be rejected");
    assert_eq!(err_label(&over_node), "ExpansionBudgetOutOfRange");

    // (2) An Expanded closure over the depth-budget ceiling is rejected.
    let over_depth = validate_resolve_symbol_graph_request(&resolve_symbol_request_with(
        Some(default_context()),
        Some(expanded_closure(8, MAX_EXPANSION_DEPTH_BUDGET + 1)),
    ))
    .expect_err("depth_budget over the ceiling must be rejected");
    assert_eq!(err_label(&over_depth), "ExpansionBudgetOutOfRange");

    // (3) An all-zero Expanded budget is an unbounded expansion → rejected.
    let zero = validate_resolve_symbol_graph_request(&resolve_symbol_request_with(
        Some(default_context()),
        Some(expanded_closure(0, 0)),
    ))
    .expect_err("an all-zero Expanded budget must be rejected as unbounded");
    assert_eq!(err_label(&zero), "ExpansionBudgetOutOfRange");

    // (4) A within-bounds Expanded budget validates.
    validate_resolve_symbol_graph_request(&resolve_symbol_request_with(
        Some(default_context()),
        Some(expanded_closure(
            MAX_EXPANSION_NODE_BUDGET,
            MAX_EXPANSION_DEPTH_BUDGET,
        )),
    ))
    .expect("a within-bounds Expanded budget must validate");

    // (5) The structurally-bounded variants carry NO numeric budget field
    // in the proto (so there is no unbounded knob to exceed).
    let proto = read_proto();
    for (msg, label) in [
        ("GraphClosureRootOnly", "RootOnly"),
        ("GraphClosureOneLevel", "OneLevel"),
    ] {
        // These are empty messages — structurally bounded (`{}`).
        let needle = format!("message {msg} {{}}");
        assert!(
            proto.contains(&needle),
            "closure variant `{label}` ({msg}) must be a structurally-bounded \
             empty message — it carries no open-ended numeric budget",
        );
    }
    // The Expanded message DOES carry the two numeric budgets the
    // validator bounds.
    let expanded_idx = proto
        .find("message GraphClosureExpanded {")
        .expect("GraphClosureExpanded must exist");
    let expanded_body = &proto[expanded_idx..expanded_idx + 160];
    assert!(
        expanded_body.contains("uint32 node_budget =")
            && expanded_body.contains("uint32 depth_budget ="),
        "GraphClosureExpanded must carry the `node_budget` and `depth_budget` the validator bounds",
    );
}

// ───────────────── static: error union consistency across sections ────────

/// The closed `TypeInfoRequestError` union — exactly these 11 active
/// variant selectors (proto field 11 / `missing_relate_endpoint` is
/// retired via `reserved`).
const REQUEST_ERROR_VARIANTS: &[&str] = &[
    "missing_projection_context",
    "missing_display_policy",
    "invalid_mode",
    "missing_closure_policy",
    "unknown_schema_version",
    "malformed_payload",
    "omitted_roots",
    "unstable_state",
    "malformed_structured_expression",
    "missing_project_path",
    "expansion_budget_out_of_range",
];

#[test]
fn typeinfo_request_error_union_is_consistent_across_sections() {
    // The closed error union must be consistent across the two surfaces
    // that enumerate it: the proto `TypeInfoRequestError` oneof and the
    // validator's exhaustive `err_label` match (which mirrors the
    // generated `type_info_request_error::Kind`). Discriminating: drop a
    // proto arm and the proto set no longer matches the expected 11.
    let proto = read_proto();

    // Parse the proto oneof selectors.
    let start = proto
        .find("message TypeInfoRequestError {")
        .expect("TypeInfoRequestError message must exist");
    let body = &proto[start..];
    let oneof_start = body.find("oneof kind {").expect("oneof kind block");
    let after = &body[oneof_start..];
    let oneof_end = after.find("\n  }").expect("oneof block must close");
    let oneof_body = &after[..oneof_end];

    let mut proto_arms: Vec<String> = Vec::new();
    for line in oneof_body.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty()
            || trimmed.starts_with("//")
            || trimmed.starts_with("reserved")
            || trimmed.starts_with("oneof")
        {
            continue;
        }
        if let Some((lhs, _)) = trimmed.split_once('=') {
            if let Some(selector) = lhs.split_whitespace().nth(1) {
                proto_arms.push(selector.to_string());
            }
        }
    }
    proto_arms.sort();
    let mut expected: Vec<String> = REQUEST_ERROR_VARIANTS
        .iter()
        .map(|s| (*s).to_string())
        .collect();
    expected.sort();
    assert_eq!(
        proto_arms, expected,
        "the proto TypeInfoRequestError oneof must carry exactly the 11 closed \
         error selectors",
    );

    // The retired `missing_relate_endpoint` tag 11 must stay reserved for
    // wire compatibility (so off-tree clients keep round-tripping it).
    assert!(
        proto[start..].contains("reserved 11;")
            && proto[start..].contains("reserved \"missing_relate_endpoint\";"),
        "TypeInfoRequestError must reserve the retired tag 11 / name \
         `missing_relate_endpoint` for wire compatibility",
    );

    // The validator's `err_label` is exhaustive over the same union (it
    // would not compile if a `Kind` arm were missing) — exercising it
    // here ties the validator surface to the proto count.
    assert_eq!(
        REQUEST_ERROR_VARIANTS.len(),
        11,
        "the validator-side error union mirror must list exactly 11 variants",
    );
}

// ───────────── static: per-request context / closure carriage ─────────────

/// `(request message, carries_closure)` for the seven graph-payload
/// request types. `flowNarrowing` / `contextualType` are the two NAMED
/// closure exemptions (span-based narrowing carries no closure); all
/// seven carry `context` + `display_policy`.
const GRAPH_PAYLOAD_REQUESTS: &[(&str, bool)] = &[
    ("ResolveSymbolGraphRequest", true),
    ("EvaluateTypeExpressionGraphRequest", true),
    ("ProjectPathGraphRequest", true),
    ("FlowNarrowingRequest", false),
    ("ContextualTypeRequest", false),
    ("ExpandGraphAroundRequest", true),
    ("FrameworkSurfaceRequest", true),
];

fn request_message_body<'a>(proto: &'a str, message: &str) -> &'a str {
    let needle = format!("message {message} {{");
    let start = proto
        .find(&needle)
        .unwrap_or_else(|| panic!("typeinfo.proto must define `message {message}`"));
    let body_start = start + needle.len();
    let rest = &proto[body_start..];
    let mut depth = 1usize;
    let mut end = rest.len();
    for (i, c) in rest.char_indices() {
        match c {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    end = i;
                    break;
                }
            }
            _ => {}
        }
    }
    &rest[..end]
}

#[test]
fn every_typeinfo_request_carries_context_or_is_exempted_with_rationale() {
    // Each of the seven graph-payload request types carries a projection
    // `context` and a `display_policy`. Five carry a `closure`; the two
    // span-based requests (FlowNarrowing / ContextualType) are the NAMED
    // closure exemptions — they narrow at a span and have no closure to
    // bound. Discriminating: drop `context` from any request, or add
    // `closure` to FlowNarrowing, and this fails.
    let proto = read_proto();
    for (message, carries_closure) in GRAPH_PAYLOAD_REQUESTS {
        let body = request_message_body(&proto, message);
        assert!(
            body.contains("GraphProjectionReductionContext context ="),
            "request `{message}` must carry a projection `context`",
        );
        assert!(
            body.contains("GraphDisplayPolicy display_policy ="),
            "request `{message}` must carry a `display_policy`",
        );
        let has_closure = body.contains("GraphClosurePolicy closure =");
        assert_eq!(
            has_closure, *carries_closure,
            "request `{message}` closure-carriage mismatch: expected carries_closure={carries_closure}, \
             found {has_closure}. FlowNarrowing/ContextualType are the only closure exemptions \
             (span-based narrowing); every other graph-payload request carries a closure.",
        );
    }

    // The two exemptions ARE span-based — they carry a span ref, which is
    // the documented rationale for omitting closure.
    for exempt in ["FlowNarrowingRequest", "ContextualTypeRequest"] {
        let body = request_message_body(&proto, exempt);
        assert!(
            body.contains("GraphSpanRef span ="),
            "the closure-exempt request `{exempt}` must carry a `span` ref — \
             span-based narrowing is the rationale for the closure exemption",
        );
    }
}

// ───────────────── static: listSymbols scalar / relate no closure ─────────

#[test]
fn list_symbols_is_scalar() {
    // The list-symbols surface is a flat scalar list: `SymbolEntryListDto`
    // is `repeated SymbolEntryDto`, and each `SymbolEntryDto` is a flat
    // record (name / kind / span / exported). It is NOT a graph request —
    // it carries no projection context, no closure, no graph payload.
    // Discriminating: add a `GraphClosurePolicy` to SymbolEntryDto and
    // this fails.
    let proto = read_proto();

    let list_body = request_message_body(&proto, "SymbolEntryListDto");
    assert!(
        list_body.contains("repeated SymbolEntryDto entries ="),
        "SymbolEntryListDto must be a flat `repeated SymbolEntryDto` list",
    );

    let entry_body = request_message_body(&proto, "SymbolEntryDto");
    // A scalar symbol entry carries NO graph machinery.
    for forbidden in [
        "GraphClosurePolicy",
        "GraphProjectionReductionContext",
        "SemanticTypeGraph",
        "GraphDisplayPolicy",
    ] {
        assert!(
            !entry_body.contains(forbidden),
            "SymbolEntryDto must be a scalar record — it must NOT carry `{forbidden}`. \
             listSymbols is a scalar list, not a graph request.",
        );
    }
    // Positive: it carries the flat scalar fields.
    assert!(
        entry_body.contains("string name =") && entry_body.contains("string kind ="),
        "SymbolEntryDto must carry the flat scalar `name` / `kind` fields",
    );
}

#[test]
fn relate_has_no_closure_field() {
    // `relate` is a relation-check operation, not a graph-projection
    // request. It is NOT one of the `TypeInfoGraphRequest.payload` oneof
    // arms, and routing the RELATE operation through the graph envelope
    // is a typed MalformedPayload error. There is therefore no closure
    // field carried for relate. Discriminating: add a `relate` arm to the
    // payload oneof and this fails; or change the validator to accept
    // RELATE through the graph path and the behavioral assertion fails.
    let proto = read_proto();

    // (1) RELATE is NOT a payload oneof arm of TypeInfoGraphRequest.
    let req_body = request_message_body(&proto, "TypeInfoGraphRequest");
    let oneof_idx = req_body.find("oneof payload {").expect("payload oneof");
    let payload_block = &req_body[oneof_idx..];
    assert!(
        !payload_block.contains("relate ="),
        "TypeInfoGraphRequest.payload must NOT carry a `relate` arm — relate is \
         not a graph-projection request and carries no closure",
    );

    // (2) The validator rejects the RELATE operation routed through the
    // graph envelope (MalformedPayload) rather than carrying a closure
    // for it.
    let relate_through_graph = wire::TypeInfoGraphRequest {
        schema_version: TYPEINFO_GRAPH_SCHEMA_VERSION,
        operation: wire::Operation::Relate as i32,
        payload: Some(wire_request::Payload::ResolveSymbol(
            valid_resolve_symbol_request(),
        )),
    };
    let err = validate_type_info_graph_request(&relate_through_graph)
        .expect_err("RELATE through the graph envelope must be rejected");
    assert_eq!(
        err_label(&err),
        "MalformedPayload",
        "routing RELATE through the graph payload must fail with MalformedPayload — \
         relate has no graph-projection closure",
    );
}

// ───────────── static: path-projection-mode-cascade DTO surface ───────────

#[test]
fn path_projection_mode_cascade() {
    // STATIC DTO/validator surface pin for path projection (the runtime
    // Navigate-intermediate / terminal-caller-mode cascade is a later
    // block). The path-projection request carries an ordered
    // `repeated GraphTypePathSegment path`, and an EMPTY path is a typed
    // MissingProjectPath error — a path projection must name at least one
    // hop for the cascade to traverse. Discriminating: an empty-path
    // request must be rejected; a populated-path request validates.
    let proto = read_proto();
    let body = request_message_body(&proto, "ProjectPathGraphRequest");
    assert!(
        body.contains("repeated GraphTypePathSegment path ="),
        "ProjectPathGraphRequest must carry an ordered `path` segment list — the \
         cascade traverses these hops",
    );
    // The path carries the projection context (so each hop knows the
    // caller's reduction mode) — the static surface the cascade reads.
    assert!(
        body.contains("GraphProjectionReductionContext context ="),
        "ProjectPathGraphRequest must carry the projection `context` the cascade \
         applies at the terminal hop",
    );

    // Behavioral: an empty path is a typed error.
    let empty_path = wire::ProjectPathGraphRequest {
        canonical_id: "/a.ts".to_string(),
        name: "Foo".to_string(),
        path: vec![],
        context: Some(default_context()),
        closure: Some(one_level_closure()),
        display_policy: Some(default_display_policy()),
        include_provenance: false,
        include_diagnostics: false,
        include_projection: vec![],
        include_degraded: false,
    };
    let err = verter_session::typeinfo::request_validation::validate_project_path_graph_request(
        &empty_path,
    )
    .expect_err("an empty path must be rejected");
    assert_eq!(
        err_label(&err),
        "MissingProjectPath",
        "a path projection with an empty path must fail with MissingProjectPath — \
         the cascade needs at least one hop",
    );

    // A populated path validates.
    let mut populated = empty_path.clone();
    populated.path = vec![wire::TypePathSegment {
        kind: Some(
            verter_protocol::verter::v1::graph_type_path_segment::Kind::Property(
                wire::wire_path_segment_property(7),
            ),
        ),
    }];
    verter_session::typeinfo::request_validation::validate_project_path_graph_request(&populated)
        .expect("a populated path must validate");
}
