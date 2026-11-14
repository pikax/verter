//! Native half of BWH1: the same fixture the browser worker drives, through
//! the same session host. Prints `BWH1_NATIVE_RECEIPT:` JSON for the gate.

use std::sync::Arc;

use serde_json::{json, Value};
use verter_ffi::convert::HostResolvedCompileProfiles;
use verter_session::{HostConfig, UpsertRequest, VerterHost};
use verter_type_engine::semantic_query::{ResolveDeclKey, ScopeId, SemanticQueryKey};

use crate::compile_request_response::compile_request_response_to_wasm;
use crate::host_compile_request_from_wire;
use crate::typeinfo;
use verter_ffi::convert::build_selector_match_results;

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

/// The symbol inventory of `canonical`, answered by the list-symbols
/// operation of the one typeinfo entry, as the browser host reads it.
fn listed_symbols(host: &VerterHost, canonical: &str) -> Vec<Value> {
    use verter_protocol::typeinfo::graph as wire;
    use verter_protocol::verter::v1::{type_info_graph_request, type_info_graph_response};
    let request = wire::TypeInfoGraphRequest {
        schema_version: wire::TYPEINFO_GRAPH_SCHEMA_VERSION,
        operation: wire::Operation::ListSymbols as i32,
        payload: Some(type_info_graph_request::Payload::ListSymbols(
            wire::ListSymbolsRequest {
                canonical_id: canonical.to_string(),
            },
        )),
    };
    let (outcome, _record) = host.type_info_with_audit(request).into_parts();
    let Ok(wire::TypeInfoGraphResponse {
        kind: Some(type_info_graph_response::Kind::SymbolList(list)),
    }) = outcome
    else {
        panic!("the fixture's symbol inventory answers the symbol-list arm");
    };
    list.entries
        .into_iter()
        .map(|entry| {
            json!({
                "name": entry.name,
                "kind": entry.kind,
                "spanStart": entry.span_start,
                "spanEnd": entry.span_end,
                "hasSpan": entry.has_span,
                "isExported": entry.is_exported,
            })
        })
        .collect()
}

fn normalize_symbols(symbols: &[Value]) -> Value {
    let mut rows: Vec<Value> = symbols
        .iter()
        .map(|entry| {
            json!({
                "name": entry["name"],
                "kind": entry["kind"],
                "isExported": entry["isExported"],
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

    let listed = listed_symbols(&host, CANONICAL_TS);

    let (outcome, _record) = host
        .resolve_named_symbol_wire_with_audit(CANONICAL_TS, TYPEINFO_SYMBOL, &[], None)
        .into_parts();
    let (resolved, typeinfo_error) = typeinfo::split_resolve_outcome(outcome);
    let type_expr = match resolved {
        Some(node_id) => host
            .project_node_to_type_expr_json_bytes(&node_id)
            .expect("a settled host")
            .map(|bytes| serde_json::from_slice::<Value>(&bytes).expect("typeexpr json")),
        None => None,
    };

    let style = match host.get_analysis(CANONICAL_VUE) {
        Some(snapshot) => {
            let source = host.get_source(CANONICAL_VUE).unwrap_or_default();
            serde_json::to_value(
                build_selector_match_results(&snapshot, &source).expect("selector spans in extent"),
            )
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
        name: verter_type_expr::OwnedStr::from(TYPEINFO_SYMBOL),
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
            "typeinfo": listed,
            "style": style,
            "mapping": mapping,
            "query": query,
        },
        "identities": {
            "symbols": normalize_symbols(&listed),
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
    // An audit-enabled host stores the query's record exactly when the
    // build captures; a build without `semantic-observe` reports
    // unavailable capture instead.
    assert_eq!(
        operations["query"][0]["hasRecord"],
        json!(cfg!(feature = "semantic-observe")),
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
