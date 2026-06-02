//! Static contract guards for the typeinfo graph node / symbol / origin
//! taxonomies.
//!
//! These guards pin the closed shape of the typeinfo graph wire surface:
//! the node taxonomy, the three `OriginEdgeKind` taxonomies, the symbol
//! node's namespace and decl-slot-identity carriers, the literal value /
//! string-table independence, the cycle carrier, and the closure-policy
//! surface. They are STATIC surface pins (proto shape + Rust enum
//! cardinality) — they assert the contract carriers exist and keep their
//! shape; they do NOT exercise runtime graph emission (that is later
//! blocks).
//!
//! Discriminating shape: each guard reads a specific surface (the proto
//! schema, or a Rust enum via an exhaustive match) and asserts a
//! structural invariant. A wrong arm-list edit, a renamed/dropped field,
//! or a raw-`uint32` discriminant fails the matching guard.

use std::collections::BTreeSet;
use std::path::PathBuf;

use verter_audit::OriginEdgeKind as AuditOriginEdgeKind;
use verter_session::semantic_query::OriginEdgeKind as SessionOriginEdgeKind;

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

/// Slice the body of `message <name> {` (or `enum <name> {`) from proto
/// source, returning the brace-balanced inner text.
fn proto_block_body<'a>(source: &'a str, keyword: &str, name: &str) -> &'a str {
    let needle = format!("{keyword} {name} {{");
    let start = source
        .find(&needle)
        .unwrap_or_else(|| panic!("typeinfo.proto must define `{keyword} {name}`"));
    let body_start = start + needle.len();
    let rest = &source[body_start..];
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

/// Strip `//` and `/* */` comments from proto source.
fn strip_proto_comments(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '/' {
            match chars.peek().copied() {
                Some('/') => {
                    for n in chars.by_ref() {
                        if n == '\n' {
                            break;
                        }
                    }
                    out.push('\n');
                    continue;
                }
                Some('*') => {
                    chars.next();
                    while let Some(n) = chars.next() {
                        if n == '*' && chars.peek().copied() == Some('/') {
                            chars.next();
                            break;
                        }
                    }
                    continue;
                }
                _ => {}
            }
        }
        out.push(c);
    }
    out
}

/// Extract the snake_case selectors of `oneof <disc> { ... }` inside the
/// given proto message.
fn proto_oneof_arms(source: &str, message: &str, discriminator: &str) -> BTreeSet<String> {
    let body = proto_block_body(source, "message", message);
    let marker = format!("oneof {discriminator} {{");
    let start = body
        .find(&marker)
        .unwrap_or_else(|| panic!("`message {message}` must declare `oneof {discriminator}`"));
    let after = &body[start + marker.len()..];
    let mut depth = 1usize;
    let mut end = after.len();
    for (i, c) in after.char_indices() {
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
    let stripped = strip_proto_comments(&after[..end]);
    let mut arms = BTreeSet::new();
    for raw in stripped.split(';') {
        let stmt = raw.trim();
        if stmt.is_empty() || stmt.starts_with("reserved") || stmt.starts_with("option") {
            continue;
        }
        if let Some((lhs, _)) = stmt.split_once('=') {
            if let Some(selector) = lhs.split_whitespace().nth(1) {
                if selector.chars().next().is_some_and(|c| c.is_ascii_lowercase()) {
                    arms.insert(selector.to_string());
                }
            }
        }
    }
    arms
}

/// Extract the variant identifiers declared inside `enum <name> {`.
fn proto_enum_variants(source: &str, name: &str) -> BTreeSet<String> {
    let body = proto_block_body(source, "enum", name);
    let stripped = strip_proto_comments(body);
    let mut variants = BTreeSet::new();
    for raw in stripped.split(';') {
        let stmt = raw.trim();
        if stmt.is_empty() || stmt.starts_with("reserved") || stmt.starts_with("option") {
            continue;
        }
        if let Some((lhs, _)) = stmt.split_once('=') {
            let ident = lhs.trim();
            if !ident.is_empty() {
                variants.insert(ident.to_string());
            }
        }
    }
    variants
}

// ───────────────────────────── node taxonomy ─────────────────────────────

/// The closed `GraphTypeNode.kind` oneof — exactly these 32 arms.
const NODE_TAXONOMY_ARMS: &[&str] = &[
    "primitive",
    "literal",
    "unique_symbol",
    "union",
    "intersection",
    "object",
    "array",
    "tuple",
    "reference",
    "alias_instantiation",
    "type_parameter",
    "key_of",
    "indexed_access",
    "conditional",
    "mapped",
    "template_literal",
    "typeof_node",
    "satisfies_node",
    "class_node",
    "this_type",
    "merged_declaration",
    "ambient_module",
    "module_augmentation",
    "ambient_namespace",
    "global_augmentation",
    "flow_narrowing",
    "contextual_type",
    "relation_proof",
    "infer_node",
    "enum_node",
    "opaque",
    "cycle",
];

#[test]
fn node_taxonomy_complete() {
    // The `GraphTypeNode.kind` oneof is the closed node taxonomy. This
    // guard pins the EXACT arm set (32 arms) plus the `reserved 33 to 100`
    // additive window. Discriminating: add, drop, or rename any arm and
    // the set comparison fails.
    let proto = read_proto();
    let arms = proto_oneof_arms(&proto, "GraphTypeNode", "kind");
    let expected: BTreeSet<String> = NODE_TAXONOMY_ARMS.iter().map(|s| (*s).to_string()).collect();
    assert_eq!(
        arms.len(),
        32,
        "GraphTypeNode.kind must have exactly 32 oneof arms; found {}",
        arms.len(),
    );
    assert_eq!(
        arms,
        expected,
        "GraphTypeNode.kind oneof drifted.\nproto-only: {:?}\nexpected-only: {:?}",
        arms.difference(&expected).collect::<Vec<_>>(),
        expected.difference(&arms).collect::<Vec<_>>(),
    );
    // The additive window reservation must be present so adding a variant
    // is a deliberate schema_version bump, not a silent tag grab.
    let body = proto_block_body(&proto, "message", "GraphTypeNode");
    assert!(
        body.contains("reserved 33 to 100;"),
        "GraphTypeNode must reserve the additive `oneof kind` tag window \
         (`reserved 33 to 100;`) at the enclosing message level",
    );
}

// ─────────────────────────── origin-edge taxonomy ────────────────────────

/// The 9-arm derivation taxonomy, as Rust variant names. Both the session
/// enum and the audit enum (minus its audit-only extra) carry exactly
/// these, in this order.
const DERIVATION_EDGE_NAMES: &[&str] = &[
    "Instantiate",
    "SubstituteTypeParam",
    "ConditionalSelect",
    "InferBind",
    "ProjectMember",
    "ProjectIndex",
    "ProjectPath",
    "Normalize",
    "AliasResolve",
];

/// Map a session derivation edge to its normative Rust variant name via
/// an EXHAUSTIVE match (no wildcard), so adding or removing a session
/// variant forces this table to change.
fn session_edge_name(kind: SessionOriginEdgeKind) -> &'static str {
    match kind {
        SessionOriginEdgeKind::Instantiate => "Instantiate",
        SessionOriginEdgeKind::SubstituteTypeParam => "SubstituteTypeParam",
        SessionOriginEdgeKind::ConditionalSelect => "ConditionalSelect",
        SessionOriginEdgeKind::InferBind => "InferBind",
        SessionOriginEdgeKind::ProjectMember => "ProjectMember",
        SessionOriginEdgeKind::ProjectIndex => "ProjectIndex",
        SessionOriginEdgeKind::ProjectPath => "ProjectPath",
        SessionOriginEdgeKind::Normalize => "Normalize",
        SessionOriginEdgeKind::AliasResolve => "AliasResolve",
    }
}

/// Map an audit origin edge to its Rust variant name via an EXHAUSTIVE
/// match (no wildcard), so adding or removing an audit variant forces
/// this table to change. `SharedLoadReuse` is the audit-only extra.
fn audit_edge_name(kind: AuditOriginEdgeKind) -> &'static str {
    match kind {
        AuditOriginEdgeKind::Instantiate => "Instantiate",
        AuditOriginEdgeKind::SubstituteTypeParam => "SubstituteTypeParam",
        AuditOriginEdgeKind::ConditionalSelect => "ConditionalSelect",
        AuditOriginEdgeKind::InferBind => "InferBind",
        AuditOriginEdgeKind::ProjectMember => "ProjectMember",
        AuditOriginEdgeKind::ProjectIndex => "ProjectIndex",
        AuditOriginEdgeKind::ProjectPath => "ProjectPath",
        AuditOriginEdgeKind::Normalize => "Normalize",
        AuditOriginEdgeKind::AliasResolve => "AliasResolve",
        AuditOriginEdgeKind::SharedLoadReuse => "SharedLoadReuse",
    }
}

/// The proto `GraphOriginEdgeKind` wire taxonomy — a DIFFERENT 10-arm
/// graph-relationship taxonomy that is NOT name-isomorphic to the Rust
/// derivation taxonomy. Pinned exactly so a wrong arm-list edit fails.
const PROTO_ORIGIN_EDGE_VARIANTS: &[&str] = &[
    "GRAPH_ORIGIN_EDGE_KIND_DECLARES",
    "GRAPH_ORIGIN_EDGE_KIND_INSTANTIATES",
    "GRAPH_ORIGIN_EDGE_KIND_REFERENCES",
    "GRAPH_ORIGIN_EDGE_KIND_MEMBER_OF",
    "GRAPH_ORIGIN_EDGE_KIND_RESOLVES_TO",
    "GRAPH_ORIGIN_EDGE_KIND_SHARED_LOAD_REUSE",
    "GRAPH_ORIGIN_EDGE_KIND_FALLTHROUGH",
    "GRAPH_ORIGIN_EDGE_KIND_RELATION_PROOF_STEP",
    "GRAPH_ORIGIN_EDGE_KIND_BACK_EDGE_CYCLE",
    "GRAPH_ORIGIN_EDGE_KIND_AUGMENTATION_STITCH",
];

#[test]
fn origin_edge_taxonomy_locked() {
    // Pins the THREE OriginEdgeKind taxonomies as they actually are —
    // it does NOT reconcile them (they describe different domains).
    //
    //   (1) verter_session  — 9-arm derivation taxonomy.
    //   (2) verter_audit    — the same 9 + audit-only `SharedLoadReuse`.
    //   (3) proto GraphOriginEdgeKind — a SEPARATE 10-arm wire
    //       graph-relationship taxonomy (DECLARES/REFERENCES/...),
    //       NOT name-isomorphic to the derivation taxonomy.
    //
    // The guard asserts only session<->audit is name-isomorphic (modulo
    // SharedLoadReuse). It MUST NOT claim proto == session + 1.

    // (1) Session enum carries exactly the 9 derivation names.
    let session_names: Vec<&'static str> = [
        SessionOriginEdgeKind::Instantiate,
        SessionOriginEdgeKind::SubstituteTypeParam,
        SessionOriginEdgeKind::ConditionalSelect,
        SessionOriginEdgeKind::InferBind,
        SessionOriginEdgeKind::ProjectMember,
        SessionOriginEdgeKind::ProjectIndex,
        SessionOriginEdgeKind::ProjectPath,
        SessionOriginEdgeKind::Normalize,
        SessionOriginEdgeKind::AliasResolve,
    ]
    .into_iter()
    .map(session_edge_name)
    .collect();
    assert_eq!(
        session_names, DERIVATION_EDGE_NAMES,
        "verter_session::OriginEdgeKind must be exactly the 9-arm derivation taxonomy",
    );

    // (2) Audit enum = the same 9 derivation names + SharedLoadReuse.
    let audit_names: Vec<&'static str> = [
        AuditOriginEdgeKind::Instantiate,
        AuditOriginEdgeKind::SubstituteTypeParam,
        AuditOriginEdgeKind::ConditionalSelect,
        AuditOriginEdgeKind::InferBind,
        AuditOriginEdgeKind::ProjectMember,
        AuditOriginEdgeKind::ProjectIndex,
        AuditOriginEdgeKind::ProjectPath,
        AuditOriginEdgeKind::Normalize,
        AuditOriginEdgeKind::AliasResolve,
        AuditOriginEdgeKind::SharedLoadReuse,
    ]
    .into_iter()
    .map(audit_edge_name)
    .collect();
    let mut expected_audit: Vec<&'static str> = DERIVATION_EDGE_NAMES.to_vec();
    expected_audit.push("SharedLoadReuse");
    assert_eq!(
        audit_names, expected_audit,
        "verter_audit::OriginEdgeKind must be the 9 derivation arms + audit-only SharedLoadReuse",
    );

    // The session<->audit isomorphism: the audit set MINUS SharedLoadReuse
    // is exactly the session set.
    let session_set: BTreeSet<&str> = session_names.iter().copied().collect();
    let audit_set: BTreeSet<&str> = audit_names.iter().copied().collect();
    let audit_minus_extra: BTreeSet<&str> =
        audit_set.iter().copied().filter(|n| *n != "SharedLoadReuse").collect();
    assert_eq!(
        audit_minus_extra, session_set,
        "audit OriginEdgeKind minus `SharedLoadReuse` must equal the session OriginEdgeKind set",
    );
    assert!(
        audit_set.contains("SharedLoadReuse") && !session_set.contains("SharedLoadReuse"),
        "`SharedLoadReuse` is the audit-only edge — present in audit, absent in session",
    );

    // (3) proto GraphOriginEdgeKind is a DIFFERENT taxonomy. Pin its
    // exact 10 variant names and assert it is NOT name-isomorphic to the
    // derivation taxonomy (so the guard cannot be mistaken for a
    // session+1 claim).
    let proto = read_proto();
    let proto_variants = proto_enum_variants(&proto, "GraphOriginEdgeKind");
    let expected_proto: BTreeSet<String> = PROTO_ORIGIN_EDGE_VARIANTS
        .iter()
        .map(|s| (*s).to_string())
        .collect();
    assert_eq!(
        proto_variants, expected_proto,
        "proto GraphOriginEdgeKind drifted.\nproto-only: {:?}\nexpected-only: {:?}",
        proto_variants.difference(&expected_proto).collect::<Vec<_>>(),
        expected_proto.difference(&proto_variants).collect::<Vec<_>>(),
    );
    assert_eq!(
        proto_variants.len(),
        10,
        "proto GraphOriginEdgeKind must have exactly 10 variants",
    );
    // The proto taxonomy is NOT the derivation taxonomy renamed: at most
    // a small overlap of shared concepts (INSTANTIATES vs Instantiate are
    // different identifiers). Assert the derivation names do NOT appear
    // verbatim as proto variant identifiers.
    for derivation in DERIVATION_EDGE_NAMES {
        assert!(
            !proto_variants.contains(*derivation),
            "proto GraphOriginEdgeKind must be a SEPARATE wire taxonomy — it must \
             not contain the derivation variant name `{derivation}` verbatim. The \
             guard pins reality (different domains), it does not claim proto = session + 1.",
        );
    }
}

// ───────────────────────────── symbol node ───────────────────────────────

#[test]
fn symbol_node_preserves_type_value_namespace_spaces() {
    // The symbol-node namespace is the closed `GraphSymbolNamespace` enum
    // = { TYPE, VALUE, NAMESPACE }. A type and a value with the same name
    // are DISTINCT symbols, so there is no merged `BOTH_TYPE_VALUE` arm —
    // that would collapse the two spaces. Discriminating: add a
    // `GRAPH_SYMBOL_NAMESPACE_BOTH_TYPE_VALUE` arm and this fails.
    let proto = read_proto();
    let variants = proto_enum_variants(&proto, "GraphSymbolNamespace");
    let expected: BTreeSet<String> = [
        "GRAPH_SYMBOL_NAMESPACE_TYPE",
        "GRAPH_SYMBOL_NAMESPACE_VALUE",
        "GRAPH_SYMBOL_NAMESPACE_NAMESPACE",
    ]
    .iter()
    .map(|s| (*s).to_string())
    .collect();
    assert_eq!(
        variants, expected,
        "GraphSymbolNamespace must be exactly {{ TYPE, VALUE, NAMESPACE }}.\n\
         proto-only: {:?}\nexpected-only: {:?}",
        variants.difference(&expected).collect::<Vec<_>>(),
        expected.difference(&variants).collect::<Vec<_>>(),
    );
    // No merged both-type-value space.
    for forbidden in [
        "BOTH_TYPE_VALUE",
        "BOTHTYPEVALUE",
        "TYPE_AND_VALUE",
        "TYPE_VALUE",
    ] {
        assert!(
            !variants.iter().any(|v| v.contains(forbidden)),
            "GraphSymbolNamespace must NOT carry a merged `{forbidden}` space — \
             type and value namespaces stay distinct",
        );
    }
    // The symbol node carries the namespace via the closed enum, not a
    // raw discriminant.
    let symbol_body = proto_block_body(&proto, "message", "GraphSymbolNode");
    assert!(
        symbol_body.contains("GraphSymbolNamespace namespace ="),
        "GraphSymbolNode must carry `namespace` as the closed `GraphSymbolNamespace` enum",
    );
}

#[test]
fn symbol_node_preserves_resolved_decl_slot_identity() {
    // A symbol's stable identity is a content-free decl-slot identity:
    // `(canonical_name_id, decl_name_id, whole_hash, namespace)`. It is
    // NOT the `(canonical_id, name, symbol_space)` text triple — the slot
    // identity is interned (name-ids) and version-rooted (whole_hash).
    //
    // The wire carrier `GraphResolvedDeclSlotIdentity` already exists
    // (A0a). This guard pins its shape statically; it does NOT require
    // the U2-era Rust `ResolvedDeclSlotIdentity` type.
    let proto = read_proto();
    let body = proto_block_body(&proto, "message", "GraphResolvedDeclSlotIdentity");
    for required in [
        "uint32 canonical_name_id =",
        "uint32 decl_name_id =",
        "bytes whole_hash =",
        "GraphSymbolNamespace namespace =",
    ] {
        assert!(
            body.contains(required),
            "GraphResolvedDeclSlotIdentity must carry `{required} …` — the slot \
             identity is content-free (interned name-ids + version whole_hash + \
             namespace), not a raw text triple",
        );
    }
    // It MUST NOT carry the raw text identity (`string canonical_id` /
    // `string name`) — that would be the non-content-free triple.
    for forbidden in ["string canonical_id", "string name", "string symbol_space"] {
        assert!(
            !body.contains(forbidden),
            "GraphResolvedDeclSlotIdentity must NOT carry `{forbidden}` — the slot \
             identity is content-free (interned ids + whole_hash), not the raw \
             `(canonical_id, name, symbol_space)` text triple",
        );
    }
    // The snapshot maps snapshot-local ids to this stable identity, so the
    // node-id map entry references it.
    assert!(
        proto.contains("GraphResolvedDeclSlotIdentity identity ="),
        "the node-id map must map snapshot-local ids to a stable \
         GraphResolvedDeclSlotIdentity",
    );
}

// ─────────────────────────── literal / string table ──────────────────────

#[test]
fn literal_value_key_is_independent_of_wire_string_table() {
    // A literal node's value is carried by its own `GraphLiteralValue`
    // oneof (string-name-id / number-bits / boolean / bigint-name-id),
    // distinct from the `GraphStringTable` interning carrier. The literal
    // VALUE is part of the node's identity; it is not stored as a bare
    // `GraphStringTable` index that could alias an unrelated interned
    // string. Discriminating: collapse `GraphLiteral` to a bare
    // `GraphStringTable` reference and this fails.
    let proto = read_proto();

    // GraphLiteralValue is a distinct oneof with the four value kinds.
    let lit_arms = proto_oneof_arms(&proto, "GraphLiteralValue", "kind");
    let expected: BTreeSet<String> = ["string_name_id", "number_bits", "boolean_value", "bigint_name_id"]
        .iter()
        .map(|s| (*s).to_string())
        .collect();
    assert_eq!(
        lit_arms, expected,
        "GraphLiteralValue.kind must be exactly the 4 value kinds \
         (string_name_id / number_bits / boolean_value / bigint_name_id).\n\
         proto-only: {:?}\nexpected-only: {:?}",
        lit_arms.difference(&expected).collect::<Vec<_>>(),
        expected.difference(&lit_arms).collect::<Vec<_>>(),
    );

    // The literal NODE carries a `GraphLiteralValue value`, NOT a raw
    // `GraphStringTable` index — the value is keyed independently of the
    // string table.
    let literal_node = proto_block_body(&proto, "message", "GraphLiteral");
    assert!(
        literal_node.contains("GraphLiteralValue value ="),
        "GraphLiteral must carry its value via `GraphLiteralValue`, keyed \
         independently of the wire string table",
    );
    assert!(
        !literal_node.contains("GraphStringTable"),
        "GraphLiteral must NOT carry its value as a bare `GraphStringTable` index — \
         the literal value key is independent of the string-table carrier",
    );

    // The number-bits arm uses the f64 bit pattern (fixed64), not a
    // truncating numeric — so NaN / -0.0 literals stay distinct.
    assert!(
        proto_block_body(&proto, "message", "GraphLiteralValue").contains("fixed64 number_bits ="),
        "GraphLiteralValue.number_bits must be a `fixed64` f64 bit pattern so \
         NaN and -0.0 literals stay distinct keys",
    );
}

// ───────────────────────────────── cycle ─────────────────────────────────

#[test]
fn cycle_id_propagates_canonicalization_conflicts() {
    // STATIC carrier pin: a detected cycle is represented by `GraphCycle`,
    // which carries a `cycle_root_node_id` (the canonical cycle id) plus
    // the participating declaration nodes. The carrier is what later
    // graph-construction uses to propagate a canonicalization conflict
    // through the cycle; U0 pins only that the carrier exists and keeps
    // its shape. Runtime conflict propagation is a later block.
    let proto = read_proto();
    let body = proto_block_body(&proto, "message", "GraphCycle");
    assert!(
        body.contains("uint32 cycle_root_node_id ="),
        "GraphCycle must carry a `cycle_root_node_id` — the canonical cycle id \
         through which a canonicalization conflict propagates",
    );
    assert!(
        body.contains("repeated uint32 participants ="),
        "GraphCycle must carry the `participants` node list so a cycle conflict \
         names every contributing declaration",
    );
    // The node taxonomy exposes the cycle node arm, so a cycle is a
    // first-class node, not a degraded miss.
    let node_arms = proto_oneof_arms(&proto, "GraphTypeNode", "kind");
    assert!(
        node_arms.contains("cycle"),
        "GraphTypeNode.kind must expose a `cycle` arm so cycles are first-class nodes",
    );
}

// ─────────────────────────── closure-policy surface ──────────────────────

#[test]
fn closure_policy_surface_is_the_closed_five_variant_set() {
    // The closure policy is the closed `GraphClosurePolicy.kind` oneof —
    // exactly the five reduction strategies. A request's closure is one
    // of these; there is no open/unbounded sixth strategy. Discriminating:
    // add or drop a closure arm and this fails.
    let proto = read_proto();
    let arms = proto_oneof_arms(&proto, "GraphClosurePolicy", "kind");
    let expected: BTreeSet<String> = [
        "root_only",
        "path",
        "one_level",
        "expanded",
        "projection_required",
    ]
    .iter()
    .map(|s| (*s).to_string())
    .collect();
    assert_eq!(
        arms, expected,
        "GraphClosurePolicy.kind must be exactly the 5 closure strategies \
         (root_only / path / one_level / expanded / projection_required).\n\
         proto-only: {:?}\nexpected-only: {:?}",
        arms.difference(&expected).collect::<Vec<_>>(),
        expected.difference(&arms).collect::<Vec<_>>(),
    );
}
