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

#[test]
fn type_node_taxonomy_proto_ts_parity() {
    assert_proto_ts_parity("GraphTypeNode", "GraphTypeNode", "kind", 32);
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
    // This test asserts the generated Rust file lives where the
    // verter_protocol crate expects it AND that the carrier modules
    // we depend on exist. The byte-level Rust-side coverage lives in
    // `verter_protocol::tests::typeinfo_proto_roundtrip` (which
    // constructs every variant and roundtrips through prost).
    //
    // The generated `verter.v1.rs` is produced by `prost-build` from
    // `crates/verter_protocol/build.rs` during a normal cargo build.
    // A missing artifact means the dependency graph never built
    // `verter_protocol` before this test ran — that is a build
    // configuration bug, not a runtime condition to silently skip.
    let path = find_generated_rust().unwrap_or_else(|| {
        panic!(
            "verter.v1 generated Rust not built; \
             run `cargo build -p verter_protocol` before this test \
             (or run with a CARGO_TARGET_DIR that already contains \
             a built verter_protocol artifact)."
        )
    });
    let rust = std::fs::read_to_string(&path)
        .unwrap_or_else(|err| panic!("read generated Rust {}: {err}", path.display()));
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

fn find_generated_rust() -> Option<PathBuf> {
    let target_dir = std::env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .or_else(|| workspace_root().join("target").canonicalize().ok())?;
    let build_dir = target_dir.join("debug").join("build");
    let entries = std::fs::read_dir(&build_dir).ok()?;
    for entry in entries.flatten() {
        let name = entry.file_name();
        if name.to_string_lossy().starts_with("verter_protocol-") {
            let candidate = entry.path().join("out").join("verter.v1.rs");
            if candidate.exists() {
                return Some(candidate);
            }
        }
    }
    None
}
