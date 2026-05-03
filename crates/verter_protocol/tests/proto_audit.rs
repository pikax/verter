//! Tier 0 Step 0.7 (D100 corrected): the selective component-meta proto
//! schema is the canonical wire schema for the Tier 1B selective public
//! API. This audit verifies the `.proto` file is present, parseable as
//! UTF-8, and contains the message and enum definitions the migration
//! plan §2.1.8 names.
//!
//! D100 r8 fixed an error in r7's plan that named a Rust-derive scheme;
//! the repo's actual pattern is `.proto` IDL files compiled via
//! `prost-build` from `crates/verter_protocol/build.rs`. This audit
//! pins that decision in tree.

use std::path::PathBuf;

fn workspace_root() -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    // CARGO_MANIFEST_DIR is `crates/verter_protocol/`; ascend two levels
    // to reach the workspace root.
    p.pop();
    p.pop();
    p
}

fn selective_proto_body() -> String {
    let path = workspace_root()
        .join("crates/verter_protocol/proto/verter/v1/selective_component_meta.proto");
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {}", path.display(), e))
}

/// Discriminating (Tier 0 §2.2): the selective component-meta proto
/// schema must be present at the path the migration plan §2.1.8 pins,
/// and must declare the message and enum types the schema names.
///
/// FAIL-pre evidence: removing the file or any required type declaration
/// fails this test with the missing literal in the error message.
/// PASS-post evidence: orchestrator commit `0cf2d765` committed the
/// schema with all required types.
#[test]
fn selective_api_proto_definitions_present_with_required_fields() {
    let body = selective_proto_body();
    // Required messages (per §2.1.8 + §3.3.2).
    for required in [
        "message TypeHandle",
        "message TypeQueryPath",
        "message SubExpressionPath",
        "message InstantiationPath",
        "message NamedTypeHandle",
        "message ComponentMetaSurface",
        "message TypeExpansion",
        "message BridgeError",
        "message DepthExceeded",
        "message StaleAtFrontier",
        "message FileNotFound",
        "enum ChildKind",
        "enum BatchExpandError",
    ] {
        assert!(
            body.contains(required),
            "Tier 0 D100: selective_component_meta.proto must declare `{}`. \
             Schema does not contain that literal.",
            required,
        );
    }

    // The schema must declare the proto3 syntax — a binding contract for
    // prost-build codegen.
    assert!(
        body.contains("syntax = \"proto3\""),
        "Tier 0 D100: selective_component_meta.proto must use `proto3` syntax",
    );

    // The schema must live in the `verter.v1` package so codegen can
    // reach into existing types from `component_meta.proto`.
    assert!(
        body.contains("package verter.v1"),
        "Tier 0 D100: selective_component_meta.proto must declare `package verter.v1`",
    );
}

/// Discriminating: the BridgeError oneof must enumerate the three error
/// kinds D114 named (DepthExceeded / StaleAtFrontier / FileNotFound).
/// Without all three the bridge cannot signal the typed error envelope
/// the plan §2.1.8 + D114 mandate.
#[test]
fn selective_api_bridge_error_oneof_has_three_kinds() {
    let body = selective_proto_body();
    // Find the BridgeError message body and verify it carries a oneof
    // with the three required arms.
    let bridge_error_section = body
        .split("message BridgeError")
        .nth(1)
        .expect("BridgeError message must exist");
    // Take just the BridgeError block — up to the next top-level `}`.
    let mut depth = 0;
    let mut end = 0;
    for (i, ch) in bridge_error_section.chars().enumerate() {
        match ch {
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
    let block = &bridge_error_section[..=end];
    for arm in ["DepthExceeded", "StaleAtFrontier", "FileNotFound"] {
        assert!(
            block.contains(arm),
            "Tier 0 D114: BridgeError oneof must carry `{arm}` arm. Block was:\n{block}",
        );
    }
}

/// Discriminating: ComponentMetaSurface must enumerate the lazy
/// type-bearing fields (props/events/slots/models/exposed/accepted_props/
/// accepted_events/type_registry) as `repeated NamedTypeHandle` per D99.
/// The 14 eager scalar fields have already been verified by D99; this
/// test pins the lazy-handle commitment.
#[test]
fn component_meta_surface_lazy_fields_use_named_type_handle() {
    let body = selective_proto_body();
    let surface_section = body
        .split("message ComponentMetaSurface")
        .nth(1)
        .expect("ComponentMetaSurface message must exist");
    // Trim to the message block.
    let mut depth = 0;
    let mut end = 0;
    for (i, ch) in surface_section.chars().enumerate() {
        match ch {
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
    let block = &surface_section[..=end];

    for field in [
        "props",
        "events",
        "slots",
        "models",
        "exposed",
        "accepted_props",
        "accepted_events",
        "type_registry",
    ] {
        // The schema lists each lazy field as `repeated NamedTypeHandle <field>`.
        let needle_repeated = format!("repeated NamedTypeHandle {field}");
        assert!(
            block.contains(&needle_repeated),
            "Tier 0 D99: ComponentMetaSurface must declare `{}` as `repeated \
             NamedTypeHandle`. Block was:\n{}",
            field,
            block,
        );
    }
}
