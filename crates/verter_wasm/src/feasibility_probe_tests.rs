//! Native half of BWH1: the same fixture the browser worker drives, through
//! the same session host. Prints `BWH1_NATIVE_RECEIPT:` JSON for the gate.

use std::sync::Arc;

use serde_json::{json, Value};
use verter_ffi::convert::{host_to_ffi_symbol_entry, HostResolvedCompileProfiles};
use verter_session::semantic_query::{ResolveDeclKey, ScopeId, SemanticQueryKey};
use verter_session::{HostConfig, UpsertRequest, VerterHost};

use crate::compile_request_response::compile_request_response_to_wasm;
use crate::host_compile_request_from_wire;
use crate::{build_selector_match_results, typeinfo};

const FIXTURE_TS: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../packages/browser-host/src/fixtures/probe.ts"
));
const FIXTURE_VUE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../packages/browser-host/src/fixtures/probe.vue"
));
const CANONICAL_TS: &str = "/probe.ts";
const CANONICAL_VUE: &str = "/probe.vue";
const TYPEINFO_SYMBOL: &str = "ProbeAlias";

const VUE_COMPILE_WIRE: &str = r#"{
  "vue": {
    "identity": { "isProduction": false, "forceJs": false },
    "products": [
      { "runtimeClient": { "runtimeSourceMap": true } },
      { "analysis": { "wantScriptBindings": false, "wantTemplateData": true } }
    ],
    "options": {
      "backend": "inferred",
      "ssr": false,
      "isCustomElement": [],
      "babelParserPlugins": [],
      "scriptCustomElement": false
    }
  }
}"#;

fn no_profiles() -> HostResolvedCompileProfiles {
    HostResolvedCompileProfiles {
        semantic: None,
        output: None,
        presentation: None,
        serialization: None,
    }
}

fn upsert(host: &VerterHost, canonical: &str, source: &str) {
    let language = verter_session::LanguageRegistry::global()
        .classify_static(canonical)
        .static_resolution();
    let _ = host.upsert(UpsertRequest {
        canonical_id: Some(canonical.to_string()),
        input_id: canonical.to_string(),
        source: Arc::from(source),
        file_language: language,
        aliases: Vec::new(),
    });
}

fn mapping_from_compile(response: &Value) -> Vec<String> {
    let mut maps = Vec::new();
    let Some(products) = response.get("products").and_then(Value::as_array) else {
        return maps;
    };
    for product in products {
        if let Some(nodes) = product.get("nodes").and_then(Value::as_array) {
            for node in nodes {
                if let Some(map) = node.get("sourceMap").and_then(Value::as_str) {
                    if !map.is_empty() {
                        maps.push(map.to_string());
                    }
                }
            }
        }
        if let Some(map) = product.get("sourceMap").and_then(Value::as_str) {
            if !map.is_empty() {
                maps.push(map.to_string());
            }
        }
    }
    maps
}

fn normalize_symbols(symbols: &[verter_protocol::typeinfo::FfiSymbolEntry]) -> Value {
    let mut rows: Vec<Value> = symbols
        .iter()
        .map(|entry| {
            json!({
                "name": entry.name,
                "kind": entry.kind,
                "isExported": entry.is_exported,
            })
        })
        .collect();
    rows.sort_by(|left, right| {
        let ln = left["name"].as_str().unwrap_or("");
        let rn = right["name"].as_str().unwrap_or("");
        ln.cmp(rn).then_with(|| {
            left["kind"]
                .as_str()
                .unwrap_or("")
                .cmp(right["kind"].as_str().unwrap_or(""))
        })
    });
    Value::Array(rows)
}

pub(crate) fn native_feasibility_receipt() -> Value {
    let host = Arc::new(VerterHost::new_standalone(HostConfig {
        audit_enabled: true,
        footprint_capture: true,
        ..HostConfig::default()
    }));
    upsert(&host, CANONICAL_TS, FIXTURE_TS);
    upsert(&host, CANONICAL_VUE, FIXTURE_VUE);

    let wire: Value = serde_json::from_str(VUE_COMPILE_WIRE).expect("compile wire");
    let request = host_compile_request_from_wire(wire, &no_profiles()).expect("compile request");
    let response = host
        .compile_request(CANONICAL_VUE, request)
        .expect("compile executes");
    let source = host.get_source(CANONICAL_VUE);
    let published =
        compile_request_response_to_wasm(response, source.as_deref()).expect("wasm projection");
    let published_json = serde_json::to_value(&published).expect("serialize compile");
    let products = published_json
        .get("products")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    let ffi_symbols: Vec<verter_protocol::typeinfo::FfiSymbolEntry> = host
        .list_file_symbols(CANONICAL_TS)
        .into_iter()
        .map(host_to_ffi_symbol_entry)
        .collect();

    let (outcome, _record) = host
        .resolve_named_symbol_wire_with_audit(CANONICAL_TS, TYPEINFO_SYMBOL, &[], None)
        .into_parts();
    let (resolved, typeinfo_error) = typeinfo::split_resolve_outcome(outcome);
    let type_expr = match resolved {
        Some(node_id) => host
            .project_node_to_type_expr_json_bytes(node_id)
            .map(|bytes| serde_json::from_slice::<Value>(&bytes).expect("typeexpr json")),
        None => None,
    };

    let style = match host.get_analysis(CANONICAL_VUE) {
        Some(snapshot) => {
            let source = host.get_source(CANONICAL_VUE).unwrap_or_default();
            serde_json::to_value(build_selector_match_results(&snapshot, &source))
                .expect("style json")
        }
        None => json!([]),
    };

    let mapping = mapping_from_compile(&published_json);

    let key = SemanticQueryKey::ResolveDecl(ResolveDeclKey {
        scope: ScopeId::file(
            Arc::<str>::from(CANONICAL_TS),
            verter_type_expr::TopLevelOwnerId::ordinary_file(),
        ),
        name: Arc::<str>::from(TYPEINFO_SYMBOL),
    });
    let query_record = host
        .resolve_type_with_audit(key, CANONICAL_TS)
        .audit()
        .clone();
    let query = json!([{
        "decl": TYPEINFO_SYMBOL,
        "kind": format!("{:?}", query_record.kind),
        "hasRecord": query_record.capture_state
            == verter_audit::AuditCaptureState::ActiveStored,
    }]);

    json!({
        "operations": {
            "session": products,
            "typeinfo": ffi_symbols,
            "style": style,
            "mapping": mapping,
            "query": query,
        },
        "identities": {
            "symbols": normalize_symbols(&ffi_symbols),
            "typeinfo": type_expr,
            "typeinfoError": typeinfo_error,
        },
    })
}

#[test]
fn bwh1_native_feasibility_probe() {
    let receipt = native_feasibility_receipt();
    let operations = &receipt["operations"];
    assert!(
        operations["session"]
            .as_array()
            .is_some_and(|rows| !rows.is_empty()),
        "session products must not be empty"
    );
    assert!(
        operations["typeinfo"]
            .as_array()
            .is_some_and(|rows| !rows.is_empty()),
        "typeinfo symbols must not be empty"
    );
    assert!(
        operations["style"]
            .as_array()
            .is_some_and(|rows| !rows.is_empty()),
        "style matches must not be empty"
    );
    assert!(
        operations["mapping"]
            .as_array()
            .is_some_and(|rows| !rows.is_empty()),
        "mapping source maps must not be empty"
    );
    assert!(
        operations["query"]
            .as_array()
            .is_some_and(|rows| !rows.is_empty()),
        "query results must not be empty"
    );
    assert_eq!(
        operations["query"][0]["hasRecord"],
        json!(true),
        "audit-enabled probe must derive hasRecord from ActiveStored"
    );
    assert!(
        !receipt["identities"]["typeinfo"].is_null(),
        "authored TypeInfo observation must be present"
    );
    println!(
        "BWH1_NATIVE_RECEIPT:{}",
        serde_json::to_string(&receipt).expect("receipt json")
    );
}
