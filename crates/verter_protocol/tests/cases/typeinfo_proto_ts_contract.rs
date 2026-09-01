//! Structural contracts between the proto schema, generated TypeScript surface,
//! and hand-authored facade. Regeneration freshness is owned by
//! `pnpm proto:check` in the lint/format lane, not by Rust tests.

use std::collections::BTreeSet;
use std::path::PathBuf;

fn workspace_root() -> PathBuf {
    let crate_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    crate_root
        .parent()
        .and_then(|path| path.parent())
        .expect("CARGO_MANIFEST_DIR must be `<workspace>/crates/verter_protocol`")
        .to_path_buf()
}

fn read_workspace_file(relative: &str) -> String {
    let path = workspace_root().join(relative);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("contract should read `{}`: {error}", path.display()))
}

#[test]
fn typeinfo_ts_bindings_reference_every_proto_message() {
    let proto = read_workspace_file("crates/verter_protocol/proto/verter/v1/typeinfo.proto");
    let ts = read_workspace_file("packages/proto/src/gen/verter/v1/typeinfo_pb.ts");
    let mut messages = BTreeSet::new();
    let mut enums = BTreeSet::new();

    for raw_line in proto.lines() {
        let line = raw_line.trim_start();
        if let Some(rest) = line.strip_prefix("message ") {
            if let Some(name) = rest.split_whitespace().next() {
                let name = name.trim_end_matches('{').trim();
                if !name.is_empty() {
                    messages.insert(name.to_string());
                }
            }
        } else if let Some(rest) = line.strip_prefix("enum ") {
            if let Some(name) = rest.split_whitespace().next() {
                let name = name.trim_end_matches('{').trim();
                if !name.is_empty() {
                    enums.insert(name.to_string());
                }
            }
        }
    }

    assert!(
        !messages.is_empty(),
        "typeinfo.proto must declare a message"
    );
    let missing_messages: Vec<_> = messages
        .into_iter()
        .filter(|name| !ts.contains(&format!("export const {name}Schema")))
        .collect();
    let missing_enums: Vec<_> = enums
        .into_iter()
        .filter(|name| {
            !ts.contains(&format!("export enum {name}"))
                && !ts.contains(&format!("export const {name}Schema"))
        })
        .collect();
    assert!(
        missing_messages.is_empty() && missing_enums.is_empty(),
        "typeinfo_pb.ts is stale: missing messages {missing_messages:?}, missing enums {missing_enums:?}; run `pnpm proto:gen`",
    );
}

#[test]
fn typeinfo_ts_facade_schema_version_matches_rust() {
    let rust = verter_protocol::typeinfo::graph::TYPEINFO_GRAPH_SCHEMA_VERSION;
    let ts = read_workspace_file("packages/proto/src/typeinfo.ts");
    let needle = "export const TYPEINFO_GRAPH_SCHEMA_VERSION = ";
    let line = ts
        .lines()
        .find(|line| line.trim_start().starts_with(needle))
        .expect("typeinfo.ts must declare TYPEINFO_GRAPH_SCHEMA_VERSION");
    let value: u32 = line
        .trim()
        .trim_start_matches(needle)
        .trim_end_matches(';')
        .trim()
        .parse()
        .unwrap_or_else(|error| panic!("facade schema version must be numeric: {error}"));
    assert_eq!(
        value, rust,
        "TypeScript and Rust must advertise the same wire schema"
    );
}

#[test]
fn typeinfo_ts_bindings_record_the_proto_file_path() {
    let ts = read_workspace_file("packages/proto/src/gen/verter/v1/typeinfo_pb.ts");
    assert!(
        ts.contains("@generated from file verter/v1/typeinfo.proto"),
        "typeinfo_pb.ts must retain its generated source header",
    );
}
