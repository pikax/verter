//! Integration test for the public typeinfo host API.
//!
//! Smoke-checks that the three public host methods land on the
//! `VerterHost` impl and round-trip the boundary types defined in
//! `verter_session::typeinfo::types`.
//!
//! The discriminating tests for each method's individual contract
//! live in `crates/verter_session/src/typeinfo/tests.rs` (in-crate
//! so they can probe `SemanticGraphStore` / private helpers). This
//! integration test verifies that an external caller — building
//! against the lib's public surface — can drive each method to
//! completion.

use std::sync::Arc;

use verter_audit::{RequestKind, RequestKindPayload};
use verter_session::semantic_query::ProjectionMode;
use verter_session::typeinfo::types::{
    EvaluateTypeExpressionRequest, ImportSpec, NamedImport, SymbolKind,
};
use verter_session::{FileKind, HostConfig, UpsertRequest, VerterHost};

const TS_FIXTURE: &str = r#"
export interface IFoo { a: number }
export type AliasFoo = { v: string };
export class CFoo {}
"#;

#[test]
fn list_file_symbols_returns_inventory() {
    let host = Arc::new(VerterHost::new_standalone(HostConfig::default()));
    let _ = host.upsert(UpsertRequest {
        canonical_id: Some("/types.ts".to_string()),
        input_id: "/types.ts".to_string(),
        source: Arc::from(TS_FIXTURE),
        file_kind: FileKind::from_path("/types.ts"),
        aliases: Vec::new(),
    });

    let symbols = host.list_file_symbols("/types.ts");
    assert!(!symbols.is_empty(), "inventory must not be empty");
    let names: Vec<&str> = symbols.iter().map(|s| s.name.as_str()).collect();
    assert!(names.contains(&"IFoo"));
    assert!(names.contains(&"AliasFoo"));
    assert!(names.contains(&"CFoo"));
    let alias = symbols
        .iter()
        .find(|s| s.name == "AliasFoo" && s.kind == SymbolKind::TypeAlias)
        .unwrap();
    assert!(alias.is_exported);
}

#[test]
fn resolve_named_symbol_with_audit_returns_record() {
    let host = Arc::new(VerterHost::new_standalone(HostConfig {
        audit_enabled: true,
        footprint_capture: true,
        ..HostConfig::default()
    }));
    let _ = host.upsert(UpsertRequest {
        canonical_id: Some("/types.ts".to_string()),
        input_id: "/types.ts".to_string(),
        source: Arc::from("export type T = string;\n"),
        file_kind: FileKind::from_path("/types.ts"),
        aliases: Vec::new(),
    });

    let (node, record) = host
        .resolve_named_symbol_with_audit("/types.ts", "T", &[], Some(ProjectionMode::Expanded))
        .into_parts();
    let _node = node.ok().flatten().expect("non-generic decl resolves");
    // record is always present now (carrier `audit` field is mandatory).
    assert_eq!(record.kind, RequestKind::TypeResolution);
    assert!(matches!(
        record.kind_payload,
        RequestKindPayload::TypeResolution(_)
    ));
    assert!(!record.trace_id.is_empty(), "trace_id propagated");
}

#[test]
fn evaluate_type_expression_with_audit_resolves_primitive() {
    let host = Arc::new(VerterHost::new_standalone(HostConfig {
        audit_enabled: true,
        footprint_capture: true,
        ..HostConfig::default()
    }));
    let _ = host.upsert(UpsertRequest {
        canonical_id: Some("/scope.ts".to_string()),
        input_id: "/scope.ts".to_string(),
        source: Arc::from("export type Anchor = number;\n"),
        file_kind: FileKind::from_path("/scope.ts"),
        aliases: Vec::new(),
    });

    let req = EvaluateTypeExpressionRequest {
        scope: "/scope.ts".to_string(),
        expression: "string".to_string(),
        extra_imports: Vec::new(),
        mode: ProjectionMode::Expanded,
        cacheable: false,
    };
    let (node, record) = host.evaluate_type_expression_with_audit(req).into_parts();
    let _node = node.ok().flatten().expect("primitive expression resolves");
    // record is always present now (carrier `audit` field is mandatory).
    assert!(matches!(
        record.kind_payload,
        RequestKindPayload::TypeResolution(_)
    ));
}

#[test]
fn evaluate_with_extra_imports_round_trip() {
    let host = Arc::new(VerterHost::new_standalone(HostConfig {
        audit_enabled: true,
        footprint_capture: true,
        ..HostConfig::default()
    }));
    let _ = host.upsert(UpsertRequest {
        canonical_id: Some("/types.ts".to_string()),
        input_id: "/types.ts".to_string(),
        source: Arc::from("export type Foo = { v: number };\n"),
        file_kind: FileKind::from_path("/types.ts"),
        aliases: Vec::new(),
    });
    let _ = host.upsert(UpsertRequest {
        canonical_id: Some("/scope.ts".to_string()),
        input_id: "/scope.ts".to_string(),
        source: Arc::from("export type Anchor = number;\n"),
        file_kind: FileKind::from_path("/scope.ts"),
        aliases: Vec::new(),
    });

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
        cacheable: true,
    };
    let record = host
        .evaluate_type_expression_with_audit(req)
        .audit()
        .clone();
    assert_eq!(
        record.capture_state,
        verter_audit::AuditCaptureState::ActiveStored,
        "audit-enabled evaluate must produce a stored record",
    );
}
