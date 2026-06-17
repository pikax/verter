//! Taxonomy parity guard for the typeinfo graph wire contracts.
//!
//! The closed `GraphTypeNode`, `StructuredTypeExpression`,
//! `TypeInfoGraphRequest`, and `TypeInfoRequestError` taxonomies are
//! defined twice on the wire surface: once in the proto schema, once
//! in the protoc-gen-es-generated TypeScript types. The two sets MUST
//! stay pairwise equal; this guard parses each surface for its
//! variant set and fails when any drift exists. The third surface
//! (Rust prost output) is regenerated from the same proto every
//! build, so the proto-side count is the Rust-side count.
//!
//! Discriminator: against the pre-substrate tree the proto file does
//! not contain the new `oneof` arms and the TS file does not contain
//! the schema imports — the parity check fails at the first sourcing
//! step. Against the post-substrate tree all sets match the
//! 32-variant `GraphTypeNode` and the 22-variant
//! `StructuredTypeExpression` schemas locked in the schema-sequence
//! verdict.

use std::collections::BTreeSet;
use std::path::PathBuf;

fn workspace_root() -> PathBuf {
    let crate_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    crate_root
        .parent()
        .and_then(|p| p.parent())
        .expect("CARGO_MANIFEST_DIR must be `<workspace>/crates/verter_session`")
        .to_path_buf()
}

fn read_workspace_file(rel: &str) -> String {
    let path = workspace_root().join(rel);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|err| panic!("file {} should be readable: {err}", path.display()))
}

/// Convert a snake_case proto field selector to the camelCase form
/// emitted by protoc-gen-es.
fn snake_to_camel(snake: &str) -> String {
    let mut out = String::with_capacity(snake.len());
    let mut upper_next = false;
    for c in snake.chars() {
        if c == '_' {
            upper_next = true;
            continue;
        }
        if upper_next {
            out.extend(c.to_uppercase());
            upper_next = false;
        } else {
            out.push(c);
        }
    }
    out
}

/// Slice the proto source from the opening `message <name> {` to its
/// matching closing brace. Returns the body without the surrounding
/// braces.
fn proto_message_body<'a>(source: &'a str, message_name: &str) -> &'a str {
    let needle = format!("message {message_name} {{");
    let start_idx = source
        .find(&needle)
        .unwrap_or_else(|| panic!("typeinfo.proto must define `message {message_name} {{`"));
    let body_start = start_idx + needle.len();
    let mut depth = 1usize;
    let rest = &source[body_start..];
    let mut end = 0usize;
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

/// Extract the snake_case selectors of the `oneof <discriminator> { ... }`
/// block declared inside the given proto message.
fn proto_oneof_arms(source: &str, message_name: &str, discriminator: &str) -> BTreeSet<String> {
    let body = proto_message_body(source, message_name);
    let oneof_marker = format!("oneof {discriminator} {{");
    let oneof_start = body.find(&oneof_marker).unwrap_or_else(|| {
        panic!("`message {message_name}` must declare `oneof {discriminator} {{ ... }}`")
    });
    let after = &body[oneof_start + oneof_marker.len()..];

    // Find the closing brace of the oneof block.
    let mut depth = 1usize;
    let mut end = 0usize;
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
    let oneof_body = &after[..end];

    // Strip block / line comments — proto comments are not statements
    // but they can contain the substring `// reserved`.
    let stripped = strip_proto_comments(oneof_body);

    let mut arms = BTreeSet::new();
    for raw_stmt in stripped.split(';') {
        let stmt = raw_stmt.trim();
        if stmt.is_empty() || stmt.starts_with("reserved") || stmt.starts_with("option") {
            continue;
        }
        // The remaining shape is `<Type> <selector> = <tag>`.
        if let Some((lhs, _rhs)) = stmt.split_once('=') {
            let mut tokens = lhs.split_whitespace();
            let _type_token = tokens.next();
            if let Some(selector) = tokens.next() {
                if selector
                    .chars()
                    .next()
                    .is_some_and(|c| c.is_ascii_lowercase())
                {
                    arms.insert(selector.to_string());
                }
            }
        }
    }
    arms
}

fn strip_proto_comments(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '/' {
            if let Some('/') = chars.peek().copied() {
                while let Some(&n) = chars.peek() {
                    if n == '\n' {
                        break;
                    }
                    chars.next();
                }
                continue;
            }
            if let Some('*') = chars.peek().copied() {
                chars.next();
                while let Some(n) = chars.next() {
                    if n == '*' && chars.peek().copied() == Some('/') {
                        chars.next();
                        break;
                    }
                }
                continue;
            }
        }
        out.push(c);
    }
    out
}

/// Extract the kind selector strings from the camelCase oneof block
/// inside the TS interface generated for `message verter.v1.<Name>`.
fn ts_oneof_arms(source: &str, ts_message_name: &str, discriminator: &str) -> BTreeSet<String> {
    let needle =
        format!("export type {ts_message_name} = Message<\"verter.v1.{ts_message_name}\">");
    let start = source
        .find(&needle)
        .unwrap_or_else(|| panic!("typeinfo_pb.ts must declare `{ts_message_name}`"));
    // Slice from the start of the interface until the closing `};`.
    let body_after = &source[start..];
    let close_marker = "\n};";
    let close = body_after
        .find(close_marker)
        .unwrap_or_else(|| panic!("`{ts_message_name}` declaration must terminate"));
    let body = &body_after[..close];
    let discriminator_marker = format!("{discriminator}:");
    let kind_idx = body.find(&discriminator_marker).unwrap_or_else(|| {
        panic!("`{ts_message_name}` must declare a `{discriminator}:` discriminator")
    });
    let kind_block = &body[kind_idx..];

    let mut arms = BTreeSet::new();
    let case_marker = "case: \"";
    let mut cursor = kind_block;
    while let Some(idx) = cursor.find(case_marker) {
        let case_start = idx + case_marker.len();
        let rest = &cursor[case_start..];
        let end = rest
            .find('"')
            .unwrap_or_else(|| panic!("malformed TS `case:` literal in `{ts_message_name}`"));
        let arm = &rest[..end];
        if !arm.is_empty() {
            arms.insert(arm.to_string());
        }
        cursor = &rest[end + 1..];
    }
    arms
}

fn assert_proto_ts_parity(
    message_name: &str,
    ts_message_name: &str,
    discriminator: &str,
    expected_count: usize,
) {
    let proto = read_workspace_file("crates/verter_protocol/proto/verter/v1/typeinfo.proto");
    let ts = read_workspace_file("packages/proto/src/gen/verter/v1/typeinfo_pb.ts");

    let proto_arms = proto_oneof_arms(&proto, message_name, discriminator);
    let ts_arms = ts_oneof_arms(&ts, ts_message_name, discriminator);

    assert_eq!(
        proto_arms.len(),
        expected_count,
        "`{message_name}` must have exactly {expected_count} oneof arms in the proto",
    );

    // Translate proto's snake_case selectors to camelCase to compare
    // against TS arms.
    let proto_camel: BTreeSet<String> = proto_arms.iter().map(|s| snake_to_camel(s)).collect();
    assert_eq!(
        proto_camel,
        ts_arms,
        "{message_name} oneof set must match between proto and TS.\n\
         proto-only (camelCase): {:?}\nts-only: {:?}",
        proto_camel.difference(&ts_arms).collect::<Vec<_>>(),
        ts_arms.difference(&proto_camel).collect::<Vec<_>>(),
    );
}

/// Slice the proto source from the opening `enum <name> {` to its
/// matching closing brace and return the `(NAME, tag)` value pairs,
/// comments stripped.
fn proto_enum_values(source: &str, enum_name: &str) -> Vec<(String, i32)> {
    let needle = format!("enum {enum_name} {{");
    let start_idx = source
        .find(&needle)
        .unwrap_or_else(|| panic!("typeinfo.proto must define `enum {enum_name} {{`"));
    let body_start = start_idx + needle.len();
    let rest = &source[body_start..];
    let end = rest
        .find('}')
        .unwrap_or_else(|| panic!("`enum {enum_name}` must terminate"));
    let stripped = strip_proto_comments(&rest[..end]);

    let mut values = Vec::new();
    for raw_stmt in stripped.split(';') {
        let stmt = raw_stmt.trim();
        if stmt.is_empty() || stmt.starts_with("reserved") || stmt.starts_with("option") {
            continue;
        }
        if let Some((name, tag)) = stmt.split_once('=') {
            let name = name.trim().to_string();
            let tag: i32 = tag
                .trim()
                .parse()
                .unwrap_or_else(|e| panic!("`enum {enum_name}` value `{name}` tag: {e}"));
            values.push((name, tag));
        }
    }
    values
}

#[test]
fn type_node_taxonomy_proto_ts_parity() {
    assert_proto_ts_parity("GraphTypeNode", "GraphTypeNode", "kind", 32);
}

#[test]
fn type_info_graph_response_taxonomy_proto_ts_parity() {
    // The response wrapper's closed oneof: `graph`, `error`, and the
    // schema-3 `framework_surface` payload arm.
    assert_proto_ts_parity("TypeInfoGraphResponse", "TypeInfoGraphResponse", "kind", 3);
}

// ---------------------------------------------------------------------------
// PROVISIONAL framework-surface wire pins — OWNED BY block U8.
//
// The field-number / schema-version pins below (the `framework_surface = 3`
// response arm, the `FrameworkSurface*` field-number-stable assertions, the
// schema-3 acceptance) are PROVISIONAL. Block U8 bumps
// `SemanticTypeGraph.schema_version` and retags `FrameworkSurfacePayload.graph`
// to `TypeInfoGraphPayload`; when it does, these pins are re-tagged/re-versioned
// by U8 (the new tags go through `reserved`, never recycled). Until U8 lands they
// pin the current wire surface; do not treat the specific tag/version numbers
// here as the permanent contract.
// ---------------------------------------------------------------------------

#[test]
fn framework_tag_variant_set_is_unchanged() {
    // NEGATIVE pin: NO new `FrameworkTag` values land in this program —
    // a tag lands only together with its adapter's vertical. The live
    // set stays exactly NONE/VUE/SVELTE/REACT/SOLID/OPEN_CANONICAL on
    // the existing tags 0..=5; a stray tag addition (or a renumber)
    // fails this guard.
    let proto = read_workspace_file("crates/verter_protocol/proto/verter/v1/typeinfo.proto");
    let values = proto_enum_values(&proto, "FrameworkTag");
    let expected: Vec<(String, i32)> = [
        ("FRAMEWORK_TAG_NONE", 0),
        ("FRAMEWORK_TAG_VUE", 1),
        ("FRAMEWORK_TAG_SVELTE", 2),
        ("FRAMEWORK_TAG_REACT", 3),
        ("FRAMEWORK_TAG_SOLID", 4),
        ("FRAMEWORK_TAG_OPEN_CANONICAL", 5),
    ]
    .iter()
    .map(|(n, t)| ((*n).to_string(), *t))
    .collect();
    assert_eq!(
        values, expected,
        "the `FrameworkTag` enum must stay exactly the six existing values \
         on their existing tags — new tags land only with their adapter's vertical",
    );

    // The tag-semantics doc comment is pinned on the wire: a tag's
    // existence is NOT a support guarantee. Comment lines are joined
    // before matching so the sentence may wrap freely.
    let tag_decl_idx = proto
        .find("enum FrameworkTag {")
        .expect("FrameworkTag must exist");
    let preceding: String = proto[tag_decl_idx.saturating_sub(1200)..tag_decl_idx]
        .lines()
        .map(|l| l.trim_start().trim_start_matches("//").trim())
        .collect::<Vec<_>>()
        .join(" ");
    assert!(
        preceding.contains("NOT a support guarantee"),
        "the `FrameworkTag` enum must carry the doc comment pinning that a \
         tag value's existence is NOT a support guarantee",
    );
}

#[test]
fn framework_surface_kind_support_is_a_closed_four_value_enum() {
    let proto = read_workspace_file("crates/verter_protocol/proto/verter/v1/typeinfo.proto");
    let values = proto_enum_values(&proto, "FrameworkSurfaceKindSupport");
    let expected: Vec<(String, i32)> = [
        ("FRAMEWORK_SURFACE_KIND_SUPPORT_UNSPECIFIED", 0),
        ("FRAMEWORK_SURFACE_KIND_SUPPORT_SUPPORTED", 1),
        ("FRAMEWORK_SURFACE_KIND_SUPPORT_UNSUPPORTED", 2),
        ("FRAMEWORK_SURFACE_KIND_SUPPORT_PARTIAL", 3),
    ]
    .iter()
    .map(|(n, t)| ((*n).to_string(), *t))
    .collect();
    assert_eq!(
        values, expected,
        "`FrameworkSurfaceKindSupport` must be the closed four-value enum \
         (UNSPECIFIED / SUPPORTED / UNSUPPORTED / PARTIAL)",
    );

    // TS parity for the new enum + status message: the generated TS
    // bindings carry both declarations (drift fails here before the
    // byte-pin runs).
    let ts = read_workspace_file("packages/proto/src/gen/verter/v1/typeinfo_pb.ts");
    assert!(
        ts.contains("export enum FrameworkSurfaceKindSupport"),
        "typeinfo_pb.ts must declare the `FrameworkSurfaceKindSupport` enum",
    );
    assert!(
        ts.contains("export const FrameworkSurfaceKindStatusSchema"),
        "typeinfo_pb.ts must declare the `FrameworkSurfaceKindStatus` message schema",
    );
}

#[test]
fn framework_surface_kind_entry_field_numbers_are_stable() {
    // Wire-compat negative: the two pre-existing `FrameworkSurfaceKindEntry`
    // fields keep their numbers; the per-kind status lands as the NEW
    // tag 3 (never a recycled one). The status message reuses the
    // EXISTING `GraphExactness` + `GraphDiagnostic` vocabulary — no
    // parallel diagnostic taxonomy.
    let proto = read_workspace_file("crates/verter_protocol/proto/verter/v1/typeinfo.proto");
    let entry = proto_message_body(&proto, "FrameworkSurfaceKindEntry");
    assert!(
        entry.contains("FrameworkSurfaceKind kind = 1;"),
        "`FrameworkSurfaceKindEntry.kind` must stay field 1",
    );
    assert!(
        entry.contains("repeated FrameworkSurfaceMember members = 2;"),
        "`FrameworkSurfaceKindEntry.members` must stay field 2",
    );
    assert!(
        entry.contains("FrameworkSurfaceKindStatus status = 3;"),
        "`FrameworkSurfaceKindEntry.status` must be the new field 3",
    );

    let status = proto_message_body(&proto, "FrameworkSurfaceKindStatus");
    assert!(
        status.contains("FrameworkSurfaceKindSupport support = 1;"),
        "`FrameworkSurfaceKindStatus.support` must be field 1",
    );
    assert!(
        status.contains("GraphExactness exactness = 2;"),
        "`FrameworkSurfaceKindStatus.exactness` must reuse the existing `GraphExactness` (field 2)",
    );
    assert!(
        status.contains("repeated GraphDiagnostic diagnostics = 3;"),
        "`FrameworkSurfaceKindStatus.diagnostics` must reuse the existing `GraphDiagnostic` (field 3)",
    );

    // The response wrapper's pre-existing arms keep their numbers; the
    // framework-surface arm is the NEW tag 3.
    let response = proto_message_body(&proto, "TypeInfoGraphResponse");
    assert!(
        response.contains("SemanticTypeGraph graph = 1;"),
        "`TypeInfoGraphResponse.graph` must stay field 1",
    );
    assert!(
        response.contains("TypeInfoRequestError error = 2;"),
        "`TypeInfoGraphResponse.error` must stay field 2",
    );
    assert!(
        response.contains("FrameworkSurfacePayload framework_surface = 3;"),
        "`TypeInfoGraphResponse.framework_surface` must be the new field 3",
    );
}

#[test]
fn framework_surface_member_default_and_origin_field_numbers_are_stable() {
    // Wire-compat (schema 4): the four pre-existing `FrameworkSurfaceMember`
    // fields keep their numbers; the add-only `default_value_id` is the NEW
    // optional tag 5 and `origin` the NEW tag 6 (never recycled tags). The
    // origin sub-messages + their closed enums land typeinfo-LOCAL (NOT the
    // component-meta `PropOrigin`), and the TS bindings carry them (drift fails
    // here before the byte-pin runs).
    let proto = read_workspace_file("crates/verter_protocol/proto/verter/v1/typeinfo.proto");
    let member = proto_message_body(&proto, "FrameworkSurfaceMember");
    assert!(
        member.contains("uint32 name_id = 1;"),
        "`FrameworkSurfaceMember.name_id` must stay field 1",
    );
    assert!(
        member.contains("uint32 type_node_id = 2;"),
        "`FrameworkSurfaceMember.type_node_id` must stay field 2",
    );
    assert!(
        member.contains("bool required = 3;"),
        "`FrameworkSurfaceMember.required` must stay field 3",
    );
    assert!(
        member.contains("bool readonly = 4;"),
        "`FrameworkSurfaceMember.readonly` must stay field 4",
    );
    assert!(
        member.contains("optional uint32 default_value_id = 5;"),
        "`FrameworkSurfaceMember.default_value_id` must be the new presence-aware field 5",
    );
    assert!(
        member.contains("FrameworkSurfaceMemberOrigin origin = 6;"),
        "`FrameworkSurfaceMember.origin` must be the new field 6",
    );

    // The origin shapes are typeinfo-graph-LOCAL (their own GraphStringTable
    // ids), NOT a cross-import of the component-meta `PropOrigin`.
    for message in [
        "FrameworkSurfaceMemberOrigin",
        "FrameworkSurfaceMemberDeclaration",
        "FrameworkSurfaceOriginHop",
    ] {
        assert!(
            proto.contains(&format!("message {message} {{")),
            "typeinfo.proto must declare the typeinfo-local `{message}` message",
        );
    }
    for enum_name in [
        "FrameworkSurfaceOriginHopKind",
        "FrameworkSurfaceDeclarationKind",
    ] {
        assert!(
            proto.contains(&format!("enum {enum_name} {{")),
            "typeinfo.proto must declare the closed `{enum_name}` enum",
        );
    }

    // TS parity: the generated bindings carry the new member fields + the
    // origin enums.
    let ts = read_workspace_file("packages/proto/src/gen/verter/v1/typeinfo_pb.ts");
    assert!(
        ts.contains("defaultValueId?: number | undefined;"),
        "typeinfo_pb.ts must carry the optional `defaultValueId` member field",
    );
    assert!(
        ts.contains("export enum FrameworkSurfaceOriginHopKind"),
        "typeinfo_pb.ts must declare the `FrameworkSurfaceOriginHopKind` enum",
    );
    assert!(
        ts.contains("export enum FrameworkSurfaceDeclarationKind"),
        "typeinfo_pb.ts must declare the `FrameworkSurfaceDeclarationKind` enum",
    );

    // The component-meta wire must NOT carry the retired dead `PropOrigin`
    // (the wrong-wire B8k addition was removed; Vue public origin is a separate
    // follow-up).
    let component_meta =
        read_workspace_file("crates/verter_protocol/proto/verter/v1/component_meta.proto");
    assert!(
        !component_meta.contains("PropOrigin origin = 11;"),
        "the dead Vue-path `PropMeta.origin` must be removed from component_meta.proto",
    );
    assert!(
        !component_meta.contains("message PropOrigin"),
        "the dead `PropOrigin` message must be removed from component_meta.proto",
    );
}

#[test]
fn structured_type_expression_taxonomy_proto_ts_parity() {
    assert_proto_ts_parity(
        "StructuredTypeExpression",
        "StructuredTypeExpression",
        "kind",
        22,
    );
}

#[test]
fn type_info_request_error_taxonomy_proto_ts_parity() {
    // 11 active arms; field 11 is reserved for wire compatibility —
    // see the proto `reserved 11;` directive on the
    // `TypeInfoRequestError` oneof.
    assert_proto_ts_parity("TypeInfoRequestError", "TypeInfoRequestError", "kind", 11);
}

#[test]
fn type_info_graph_request_taxonomy_proto_ts_parity() {
    assert_proto_ts_parity("TypeInfoGraphRequest", "TypeInfoGraphRequest", "payload", 7);
}

#[test]
fn rust_generated_module_carries_documented_kind_variants() {
    // The Rust side is regenerated from the same proto every build, so
    // the proto-side variant counts are the Rust-side variant counts.
    // This test asserts the carrier modules we depend on exist in the
    // generated `verter.v1` source. The byte-level Rust-side coverage
    // lives in `verter_protocol::tests::typeinfo_proto_roundtrip` (which
    // constructs every variant and roundtrips through prost).
    //
    // The source under inspection is `verter_protocol`'s own
    // `GENERATED_VERTER_V1_RS` — `include_str!` of the exact
    // `OUT_DIR/verter.v1.rs` that the crate's `include!` compiled and
    // linked into THIS build. There is no `target/` scan, so there is no
    // stale-sibling ambiguity: a `verter_protocol-<hash>/out/` dir from
    // another fingerprint, worktree, or branch sharing the same
    // `CARGO_TARGET_DIR` cannot be inspected in place of the artifact
    // this run actually built. A missing/incomplete generated module is
    // a compile error on the `const`, never a silent skip.
    let rust = verter_protocol::GENERATED_VERTER_V1_RS;
    assert!(
        rust.contains("pub mod graph_type_node {"),
        "generated verter.v1 must include `graph_type_node` oneof carrier module",
    );
    assert!(
        rust.contains("pub mod structured_type_expression {"),
        "generated verter.v1 must include `structured_type_expression` oneof carrier module",
    );
    assert!(
        rust.contains("pub mod type_info_request_error {"),
        "generated verter.v1 must include `type_info_request_error` oneof carrier module",
    );
    assert!(
        rust.contains("pub mod type_info_graph_request {"),
        "generated verter.v1 must include `type_info_graph_request` oneof carrier module",
    );
}
