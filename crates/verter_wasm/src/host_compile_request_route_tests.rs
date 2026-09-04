// ── the callable route on the generated host object ──────────────────────
//
// The sibling decode fixtures prove what the decode boundary accepts and
// refuses. None of them proves a browser caller can REACH that boundary: a
// Rust function with no binding annotation is reachable from Rust and from
// nothing else, and a route nobody can call is not a route.
//
// These reach the method the way a browser does — read it off the generated
// host object by its JavaScript name and invoke it with real JS values — so
// what they exercise is the generated glue, not the Rust function behind it.
// A method that stopped being exported, or that was exported under a
// different name, fails here while every Rust caller of the same code keeps
// compiling.

use super::*;

use js_sys::{Function, Object, Reflect, JSON};
use serde::Serialize;
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_test::wasm_bindgen_test;

use crate::WasmVerterHost;

const VUE_SFC: &str = r#"<script setup lang="ts">
const greeting = 'hello'
</script>
<template><div class="box">{{ greeting }}</div></template>
<style scoped>.box { color: red }</style>
<i18n>{ "en": { "hi": "hello" } }</i18n>
"#;

const SVELTE_SFC: &str = r#"<script lang="ts">
let name = 'world';
</script>
<h1>hello {name}</h1>
<style>h1 { color: blue }</style>
"#;

/// A carrier that reports source offsets, with multi-byte characters ahead
/// of every one of them: `é` is two UTF-8 bytes, `—` is three, and `😀` is
/// four bytes but TWO UTF-16 code units. Every reported offset therefore has
/// a different value depending on which encoding it is stated in, which is
/// what makes the offset assertion below discriminating rather than
/// decorative.
const VUE_SFC_OFFSET_CARRIER: &str = "<script setup lang=\"ts\">\n\
     // h\u{e9}llo \u{2014} \u{1f600}\n\
     const greeting = 'x'\n\
     const { title = greeting } = defineProps<{ title?: string }>()\n\
     </script>\n\
     <template><div>{{ title }}</div></template>\n";

/// A carrier whose template repeats a directive, so the compile publishes a
/// spanned WARNING and still produces every product. An error-severity
/// fixture would refuse the whole request and leave nothing to inspect, and
/// a malformed expression breaks runtime assembly outright.
///
/// The text before the duplicate is the point: `{PREFIX}` is substituted
/// with two spellings of the SAME UTF-16 length but different byte lengths,
/// so a byte-indexed span moves between the two renderings while a
/// UTF-16-indexed span does not.
const VUE_SFC_DIAGNOSTIC_CARRIER: &str = "<script setup lang=\"ts\">\n\
     const ok = true\n\
     </script>\n\
     <template>\n\
     <p>{PREFIX}</p>\n\
     <div v-if=\"ok\" v-if=\"ok\">shown</div>\n\
     </template>\n";

/// Ten UTF-16 code units in fifteen UTF-8 bytes: `é` is two bytes, `—` is
/// three, and `😀` is four bytes but TWO UTF-16 units.
const MULTI_BYTE_PREFIX: &str = "h\u{e9}llo \u{2014} \u{1f600}";
/// Ten UTF-16 code units in ten UTF-8 bytes.
const ASCII_PREFIX: &str = "plain-text";

fn diagnostic_carrier(prefix: &str) -> String {
    VUE_SFC_DIAGNOSTIC_CARRIER.replace("{PREFIX}", prefix)
}

/// The host as the JavaScript object a browser caller holds. The Rust value
/// is consumed on purpose: everything below reaches the host the way
/// JavaScript does.
fn js_host() -> JsValue {
    JsValue::from(WasmVerterHost::new(JsValue::UNDEFINED).expect("the host constructs"))
}

/// Read a method off the generated object by its JavaScript name.
///
/// This is the point of the module: `Reflect::get` walks the generated class
/// prototype, so a name that is not exported — or is exported under a
/// different spelling — is not a function here.
#[track_caller]
fn method(host: &JsValue, name: &str) -> Function {
    let value = Reflect::get(host, &JsValue::from_str(name))
        .unwrap_or_else(|_| panic!("`{name}` is readable on the generated host object"));
    assert!(
        !value.is_undefined(),
        "the generated host object exposes no `{name}` method"
    );
    value
        .dyn_into::<Function>()
        .unwrap_or_else(|_| panic!("`{name}` is not callable on the generated host object"))
}

/// A `serde_json::Value` fixture as the plain JS object graph a browser
/// caller writes.
fn js(wire: Value) -> JsValue {
    wire.serialize(&serde_wasm_bindgen::Serializer::new().serialize_maps_as_objects(true))
        .expect("the fixture is representable as a plain JS object")
}

/// Parse JSON the way a browser caller commonly obtains request data.
/// Unlike object-literal syntax or `Reflect::set` on an ordinary object,
/// `JSON.parse` creates `__proto__` as an own enumerable data property.
fn json_parsed_js(wire: Value) -> JsValue {
    JSON::parse(&wire.to_string()).expect("the fixture is valid JSON")
}

/// The JS value back as a `serde_json::Value`, for structural reads.
#[track_caller]
fn from_js(value: &JsValue) -> Value {
    serde_wasm_bindgen::from_value(value.clone()).expect("the response is a JSON-shaped value")
}

/// Register a carrier through the generated `upsert` — source only, no
/// compile demand and no profile.
#[track_caller]
fn register(host: &JsValue, canonical_id: &str, source: &str, file_kind: &str) {
    let request = js(json!({
        "canonicalId": canonical_id,
        "inputId": canonical_id,
        "source": source,
        "fileKind": file_kind,
    }));
    method(host, "upsert")
        .call1(host, &request)
        .expect("the source-only registration succeeds");
}

/// One typed compile, through the generated method.
#[track_caller]
fn compile_request(host: &JsValue, canonical_id: &str, request: Value) -> Value {
    compile_request_js(host, canonical_id, js(request))
}

/// One typed compile from a JS object graph that cannot be represented as
/// JSON, such as an own property whose value is `undefined`.
#[track_caller]
fn compile_request_js(host: &JsValue, canonical_id: &str, request: JsValue) -> Value {
    let raw = method(host, "compileRequest")
        .call2(host, &JsValue::from_str(canonical_id), &request)
        .unwrap_or_else(|error| {
            panic!(
                "the typed compile was refused: {}",
                error.as_string().unwrap_or_default()
            )
        });
    from_js(&raw)
}

/// The thrown refusal's message. A response — of any shape — is a failure:
/// this route is complete-only.
#[track_caller]
fn compile_request_refusal(host: &JsValue, canonical_id: &str, request: Value) -> String {
    compile_request_refusal_js(host, canonical_id, js(request))
}

#[track_caller]
fn compile_request_refusal_js(host: &JsValue, canonical_id: &str, request: JsValue) -> String {
    match method(host, "compileRequest").call2(host, &JsValue::from_str(canonical_id), &request) {
        Ok(value) => panic!(
            "expected a thrown refusal, got a response: {:?}",
            from_js(&value)
        ),
        Err(error) => error
            .as_string()
            .expect("the refusal reaches JavaScript as a string"),
    }
}

fn runtime_client_product(source_map: bool) -> Value {
    json!({ "runtimeClient": { "runtimeSourceMap": source_map } })
}

/// The IDE product with the axes the host execution route consumes.
/// `ideChunkBoundaries` stays false because the carrier bridge substitutes
/// that axis and a caller-stated value is refused.
fn ide_companion_product(source_map: bool) -> Value {
    json!({ "ideCompanion": {
        "wantSourceMap": source_map,
        "embedAmbientTypes": false,
        "conditionalRootNarrowing": false,
        "strictSlots": false,
        "ideChunkBoundaries": false,
    }})
}

fn template_analysis_product() -> Value {
    json!({ "analysis": { "wantScriptBindings": false, "wantTemplateData": true } })
}

/// The identity every request below shares. `filename` is absent, so the
/// route binds the executing canonical id — the same source identity a
/// profile-derived compile of the same registration uses.
fn route_identity() -> Value {
    json!({ "isProduction": false, "forceJs": false })
}

fn vue_route_request(products: Value) -> Value {
    vue_route_request_with_ssr(products, false)
}

/// The `ssr` option and the runtime product kind are ONE demand: admission
/// refuses a server product compiled under client options and vice versa,
/// so a server request states both.
fn vue_route_request_with_ssr(products: Value, ssr: bool) -> Value {
    json!({ "vue": {
        "identity": route_identity(),
        "products": products,
        "options": {
            "backend": "inferred",
            "ssr": ssr,
            "isCustomElement": [],
            "babelParserPlugins": [],
            "scriptCustomElement": false,
        },
    }})
}

fn svelte_route_request(products: Value) -> Value {
    json!({ "svelte": {
        "identity": route_identity(),
        "products": products,
        "options": { "customElement": false },
    }})
}

/// The legacy demand that matches the typed requests below: every runtime
/// node, the IDE projection, template data, and maps on.
fn legacy_profile() -> Value {
    json!({ "target": "full", "sourceMap": true, "isProduction": false })
}

#[track_caller]
fn product_row<'response>(response: &'response Value, kind: &str) -> &'response Value {
    response["products"]
        .as_array()
        .expect("the response carries a product list")
        .iter()
        .find(|row| row["kind"] == kind)
        .unwrap_or_else(|| panic!("no `{kind}` product row in {response:?}"))
}

fn product_kinds(response: &Value) -> Vec<String> {
    response["products"]
        .as_array()
        .expect("the response carries a product list")
        .iter()
        .map(|row| row["kind"].as_str().expect("a kind tag").to_string())
        .collect()
}

/// The runtime row's node rows with their addressing, so a comparison
/// against the legacy per-node reads is order-independent.
#[track_caller]
fn runtime_nodes(response: &Value) -> Vec<(String, Option<u64>, &Value)> {
    product_row(response, "runtimeClient")["nodes"]
        .as_array()
        .expect("the runtime row carries its nodes")
        .iter()
        .map(|node| {
            (
                node["node"]["kind"]
                    .as_str()
                    .expect("a node kind string")
                    .to_string(),
                node["node"]["index"].as_u64(),
                node,
            )
        })
        .collect()
}

/// The legacy per-node read for the same demand.
#[track_caller]
fn legacy_virtual_file(
    host: &JsValue,
    canonical_id: &str,
    kind: &str,
    index: Option<u64>,
) -> Value {
    let mut node_kind = json!({ "kind": kind });
    if let Some(index) = index {
        node_kind["index"] = json!(index);
    }
    let query = js(json!({
        "canonicalId": canonical_id,
        "nodeKind": node_kind,
        "compileProfile": legacy_profile(),
    }));
    let raw = method(host, "getVirtualFile")
        .call1(host, &query)
        .unwrap_or_else(|error| {
            panic!(
                "the legacy read of {kind}/{index:?} failed: {}",
                error.as_string().unwrap_or_default()
            )
        });
    assert!(
        !raw.is_null(),
        "the legacy route publishes no {kind}/{index:?} node to compare against"
    );
    from_js(&raw)
}

/// The legacy IDE read, through its ensure-then-read pair.
#[track_caller]
fn legacy_ide(host: &JsValue, canonical_id: &str) -> Value {
    let profile = js(legacy_profile());
    method(host, "ensureIdeCompiled")
        .call2(host, &JsValue::from_str(canonical_id), &profile)
        .expect("the legacy IDE ensure succeeds");
    let raw = method(host, "getIde")
        .call2(host, &JsValue::from_str(canonical_id), &profile)
        .expect("the legacy IDE read succeeds");
    assert!(!raw.is_null(), "the legacy route published no IDE surface");
    from_js(&raw)
}

/// Every published runtime node's bytes, map, language and metadata against
/// the legacy per-node read of the same demand.
#[track_caller]
fn assert_runtime_nodes_match_legacy(
    host: &JsValue,
    canonical_id: &str,
    response: &Value,
    expected: &[(&str, Option<u64>)],
) {
    let nodes = runtime_nodes(response);
    let mut published: Vec<(String, Option<u64>)> = nodes
        .iter()
        .map(|(kind, index, _)| (kind.clone(), *index))
        .collect();
    published.sort();
    let mut wanted: Vec<(String, Option<u64>)> = expected
        .iter()
        .map(|(kind, index)| ((*kind).to_string(), *index))
        .collect();
    wanted.sort();
    // Both directions: iterating only the published list would let the route
    // silently stop publishing a node and still read as green.
    assert_eq!(
        published, wanted,
        "the runtime row must publish exactly this node set"
    );

    for (kind, index, node) in nodes {
        let legacy = legacy_virtual_file(host, canonical_id, &kind, index);
        assert_eq!(
            node["code"], legacy["code"],
            "{kind}/{index:?} bytes must match the legacy route"
        );
        assert_eq!(
            node["sourceMap"], legacy["sourceMap"],
            "{kind}/{index:?} source map must match the legacy route"
        );
        assert_eq!(
            node["lang"], legacy["lang"],
            "{kind}/{index:?} output language must match the legacy route"
        );
        assert_eq!(
            node["meta"], legacy["meta"],
            "{kind}/{index:?} metadata must match the legacy route"
        );
    }
}

// ── the route exists on the generated object ─────────────────────────────

/// The control for everything below, and the acceptance this route exists
/// for: the typed compile is a method on the generated host object,
/// reachable by its JavaScript name and taking its two arguments separately.
#[wasm_bindgen_test]
fn the_generated_host_object_exposes_the_typed_compile_method() {
    let host = js_host();
    let compile_request = method(&host, "compileRequest");
    assert_eq!(
        compile_request.length(),
        2,
        "the method takes the canonical id and the typed request as separate arguments"
    );
}

// ── one registration, one typed call ─────────────────────────────────────

/// The whole transaction from JavaScript: register the source once, then
/// execute one typed request against it. No second compile call, no ensure
/// step, and no copy of the source into the request.
#[wasm_bindgen_test]
fn one_registration_and_one_typed_call_produce_every_requested_vue_product() {
    let host = js_host();
    register(&host, "/src/App.vue", VUE_SFC, "vue");

    let response = compile_request(
        &host,
        "/src/App.vue",
        vue_route_request(json!([
            runtime_client_product(true),
            ide_companion_product(true),
            template_analysis_product(),
        ])),
    );

    assert_eq!(response["canonicalId"], json!("/src/App.vue"));
    assert_eq!(
        product_kinds(&response),
        vec!["runtimeClient", "ideCompanion", "analysis"],
        "one row per requested kind, in request order"
    );
    assert!(
        product_row(&response, "ideCompanion")["code"]
            .as_str()
            .is_some_and(|code| !code.is_empty()),
        "the IDE row carries its projected surface"
    );
    assert!(
        product_row(&response, "analysis")["analysis"]["bindingOccurrences"].is_array(),
        "the analysis row carries the template payload, not an empty marker"
    );
}

/// With TypeScript's default optional-property semantics, an optional
/// `never` property admits an explicitly written `undefined`. The public
/// route therefore reads an undefined sibling tag as absent at both tagged
/// union layers while retaining populated duplicate-tag refusals.
#[wasm_bindgen_test]
fn explicitly_undefined_sibling_tags_are_absent_on_the_public_route() {
    let host = js_host();
    register(&host, "/src/App.vue", VUE_SFC, "vue");

    let outer = js(vue_route_request(json!([template_analysis_product()])));
    Reflect::set(&outer, &JsValue::from_str("svelte"), &JsValue::UNDEFINED)
        .expect("the framework sibling tag is writable");
    let outer_response = compile_request_js(&host, "/src/App.vue", outer);
    assert_eq!(product_kinds(&outer_response), vec!["analysis"]);

    let product = js(vue_route_request(json!([template_analysis_product()])));
    let vue = Reflect::get(&product, &JsValue::from_str("vue")).expect("the Vue arm exists");
    let products =
        Reflect::get(&vue, &JsValue::from_str("products")).expect("the product list exists");
    let first =
        Reflect::get(&products, &JsValue::from_f64(0.0)).expect("the analysis product exists");
    Reflect::set(
        &first,
        &JsValue::from_str("runtimeClient"),
        &JsValue::UNDEFINED,
    )
    .expect("the product sibling tag is writable");
    let product_response = compile_request_js(&host, "/src/App.vue", product);
    assert_eq!(product_kinds(&product_response), vec!["analysis"]);
}

/// Undefined sibling normalization must not weaken the externally tagged
/// union: two populated arms remain a decoder refusal at either layer.
#[wasm_bindgen_test]
fn populated_sibling_tags_remain_refused_on_the_public_route() {
    let host = js_host();
    register(&host, "/src/App.vue", VUE_SFC, "vue");

    let both_frameworks = set(
        vue_route_request(json!([template_analysis_product()])),
        &[],
        "svelte",
        svelte_route_request(json!([template_analysis_product()]))["svelte"].clone(),
    );
    let framework_message = compile_request_refusal(&host, "/src/App.vue", both_frameworks);
    assert!(
        framework_message.contains("expected map with a single key"),
        "expected the populated framework sibling to remain a decoder refusal, got: \
         {framework_message}"
    );

    let both_products = vue_route_request(json!([{
        "analysis": { "wantScriptBindings": false, "wantTemplateData": true },
        "runtimeClient": { "runtimeSourceMap": false },
    }]));
    let product_message = compile_request_refusal(&host, "/src/App.vue", both_products);
    assert!(
        product_message.contains("expected map with a single key"),
        "expected the populated product sibling to remain a decoder refusal, got: \
         {product_message}"
    );
}

/// The analysis payload is nested under its own key, and the `kind`
/// discriminant sits BESIDE it rather than inside it.
///
/// A flattened arm would serialise the snapshot's fields into the same
/// object as the tag, so a snapshot field named `kind` — a type this wire
/// does not own and cannot freeze — would overwrite the discriminant while
/// the row still carried its data, and every consumer's `kind === "analysis"`
/// branch would stop matching.
#[wasm_bindgen_test]
fn the_analysis_payload_is_nested_beside_its_discriminant_not_flattened_into_it() {
    let host = js_host();
    register(&host, "/src/App.vue", VUE_SFC, "vue");

    let response = compile_request(
        &host,
        "/src/App.vue",
        vue_route_request(json!([template_analysis_product()])),
    );

    let row = product_row(&response, "analysis");
    let mut keys: Vec<&str> = row
        .as_object()
        .expect("the analysis row is an object")
        .keys()
        .map(String::as_str)
        .collect();
    keys.sort_unstable();
    assert_eq!(
        keys,
        vec!["analysis", "kind"],
        "the analysis row carries exactly its tag and its nested payload — a flattened arm would \
         put the snapshot's own fields here beside `kind`"
    );
    assert!(
        row["analysis"]["bindingOccurrences"].is_array(),
        "the payload lives under `analysis`, not at the row root"
    );
}

/// The analysis row publishes the TEMPLATE snapshot — the value the legacy
/// `getAnalysis` read carries under its `template` field — not the
/// whole-file snapshot that read returns.
///
/// This is the run-time half of the published declaration's claim: the
/// declaration says which of the two shapes the row is, and nothing but
/// this comparison holds it to that. Both directions are asserted, so a row
/// that quietly became the file snapshot fails here rather than reaching a
/// consumer that reads `row.analysis.template.bindingOccurrences` and finds
/// nothing.
#[wasm_bindgen_test]
fn the_analysis_row_is_the_legacy_reads_template_snapshot_not_its_file_snapshot() {
    let host = js_host();
    register(&host, "/src/App.vue", VUE_SFC, "vue");

    let response = compile_request(
        &host,
        "/src/App.vue",
        vue_route_request(json!([template_analysis_product()])),
    );
    let published = &product_row(&response, "analysis")["analysis"];

    let legacy_raw = method(&host, "getAnalysis")
        .call1(&host, &JsValue::from_str("/src/App.vue"))
        .expect("the legacy whole-file analysis read succeeds");
    assert!(
        !legacy_raw.is_null(),
        "the legacy read publishes no analysis to compare against"
    );
    let legacy = from_js(&legacy_raw);

    assert!(
        legacy["template"].is_object(),
        "fixture drift: the legacy file snapshot must nest a template snapshot"
    );
    assert_eq!(
        *published, legacy["template"],
        "the analysis row must be the legacy read's `template` value"
    );
    assert_ne!(
        *published, legacy,
        "the analysis row must NOT be the whole-file snapshot"
    );
}

/// The server runtime arm is tagged `runtimeServer`, not folded into the
/// client tag.
///
/// The two runtime kinds publish through one host row shape, so the wire
/// tag is the ONLY thing distinguishing an SSR bundle from a client one on
/// the JavaScript side. A caller that received the client tag for a server
/// compile would ship server output to a browser with nothing to notice it
/// by.
#[wasm_bindgen_test]
fn the_server_runtime_product_publishes_under_its_own_wire_tag() {
    let host = js_host();
    register(&host, "/src/App.vue", VUE_SFC, "vue");

    let response = compile_request(
        &host,
        "/src/App.vue",
        vue_route_request_with_ssr(
            json!([json!({ "runtimeServer": { "runtimeSourceMap": false } })]),
            true,
        ),
    );

    assert_eq!(
        product_kinds(&response),
        vec!["runtimeServer"],
        "a server runtime demand publishes exactly one server-tagged row"
    );
    assert!(
        product_row(&response, "runtimeServer")["nodes"]
            .as_array()
            .is_some_and(|nodes| !nodes.is_empty()),
        "the server row carries its compiled nodes"
    );
}

/// The Svelte carrier through the same method, the same way. A route proven
/// on one framework proves nothing about the other: the request's framework
/// arm decides which backend executes.
#[wasm_bindgen_test]
fn one_registration_and_one_typed_call_produce_every_requested_svelte_product() {
    let host = js_host();
    register(&host, "/src/Widget.svelte", SVELTE_SFC, "svelte");

    let response = compile_request(
        &host,
        "/src/Widget.svelte",
        svelte_route_request(json!([
            runtime_client_product(true),
            ide_companion_product(true),
        ])),
    );

    assert_eq!(response["canonicalId"], json!("/src/Widget.svelte"));
    assert_eq!(
        product_kinds(&response),
        vec!["runtimeClient", "ideCompanion"],
    );
    assert!(
        product_row(&response, "ideCompanion")["code"]
            .as_str()
            .is_some_and(|code| !code.is_empty()),
        "the Svelte IDE row carries its projected surface"
    );
}

// ── equivalence with the legacy route ────────────────────────────────────

/// Same demand, same bytes: every runtime node and the IDE projection match
/// what the legacy profile-bearing pair produces for the same registration,
/// through the same JavaScript boundary.
#[wasm_bindgen_test]
fn the_vue_route_is_equivalent_to_the_legacy_pair_for_the_same_demand() {
    let host = js_host();
    register(&host, "/src/App.vue", VUE_SFC, "vue");

    let response = compile_request(
        &host,
        "/src/App.vue",
        vue_route_request(json!([
            runtime_client_product(true),
            ide_companion_product(true),
        ])),
    );

    assert_runtime_nodes_match_legacy(
        &host,
        "/src/App.vue",
        &response,
        &[
            ("main", None),
            ("script", None),
            ("template", None),
            ("style", Some(0)),
            ("custom", Some(0)),
        ],
    );

    let legacy = legacy_ide(&host, "/src/App.vue");
    let ide = product_row(&response, "ideCompanion");
    assert_eq!(ide["code"], legacy["code"]);
    assert_eq!(ide["sourceMap"], legacy["sourceMap"]);
    assert_eq!(ide["isJsx"], legacy["isJsx"]);

    // Diagnostics are named in the equivalence contract beside the bytes and
    // the maps, so the whole set — codes, messages and spans — is compared
    // against the legacy compile's rather than assumed to match.
    let legacy_main = legacy_virtual_file(&host, "/src/App.vue", "main", None);
    assert_eq!(response["diagnostics"], legacy_main["diagnostics"]);
}

/// The Svelte carrier's own equivalence. Its published node set differs
/// from Vue's, which is exactly why it is asserted rather than assumed.
#[wasm_bindgen_test]
fn the_svelte_route_is_equivalent_to_the_legacy_pair_for_the_same_demand() {
    let host = js_host();
    register(&host, "/src/Widget.svelte", SVELTE_SFC, "svelte");

    let response = compile_request(
        &host,
        "/src/Widget.svelte",
        svelte_route_request(json!([
            runtime_client_product(true),
            ide_companion_product(true),
        ])),
    );

    assert_runtime_nodes_match_legacy(
        &host,
        "/src/Widget.svelte",
        &response,
        &[("main", None), ("style", Some(0))],
    );

    let legacy = legacy_ide(&host, "/src/Widget.svelte");
    let ide = product_row(&response, "ideCompanion");
    assert_eq!(ide["code"], legacy["code"]);
    assert_eq!(ide["sourceMap"], legacy["sourceMap"]);
    assert_eq!(ide["isJsx"], legacy["isJsx"]);

    let legacy_main = legacy_virtual_file(&host, "/src/Widget.svelte", "main", None);
    assert_eq!(response["diagnostics"], legacy_main["diagnostics"]);
}

/// IDE offsets retain their distinct coordinate spaces: binding spans index
/// the registered source, while block bounds index the generated IDE code.
/// Both use UTF-16 code units and remain the legacy route's own.
///
/// Legacy equality alone cannot prove either meaning. The binding start is
/// checked against distinct UTF-8 and UTF-16 source indices; the block start
/// is beyond the source string and inside the generated code.
#[wasm_bindgen_test]
fn published_ide_offsets_use_their_own_utf16_coordinate_spaces() {
    let host = js_host();
    register(&host, "/src/Offsets.vue", VUE_SFC_OFFSET_CARRIER, "vue");

    let response = compile_request(
        &host,
        "/src/Offsets.vue",
        vue_route_request(json!([ide_companion_product(true)])),
    );

    let published = &product_row(&response, "ideCompanion")["destructuredBlock"];
    let binding = published["bindings"]
        .as_array()
        .expect("the destructured block carries its bindings")
        .iter()
        .find(|binding| binding["name"] == "greeting")
        .expect("the destructured block reports the `greeting` binding");

    let byte_start = VUE_SFC_OFFSET_CARRIER
        .find("greeting")
        .expect("the fixture contains the binding");
    let utf16_start = VUE_SFC_OFFSET_CARRIER[..byte_start].encode_utf16().count();
    assert_ne!(
        byte_start, utf16_start,
        "fixture drift: the two encodings must disagree for this to prove anything"
    );
    assert_eq!(
        binding["sourceStart"].as_u64(),
        Some(utf16_start as u64),
        "the published offset must be the source's UTF-16 index, not its byte index ({byte_start})"
    );

    let block_start = published["blockStart"]
        .as_u64()
        .expect("the generated block carries a start offset") as usize;
    let generated_code = product_row(&response, "ideCompanion")["code"]
        .as_str()
        .expect("the IDE row carries generated code");
    assert!(
        block_start > VUE_SFC_OFFSET_CARRIER.encode_utf16().count()
            && block_start <= generated_code.encode_utf16().count(),
        "blockStart must index the generated IDE code, not the registered source"
    );

    let legacy = legacy_ide(&host, "/src/Offsets.vue");
    assert_eq!(
        *published, legacy["destructuredBlock"],
        "the published offsets must be the legacy route's own"
    );

    // Every other response shape this route publishes reuses a shared
    // declaration, so a Rust-side field change breaks the legacy binding's
    // declaration too. The destructured block is the exception — the shared
    // IDE response does not name it, so the package declares it locally and
    // nothing else holds that copy to the struct being serialised. These are
    // the keys the local declaration states; a field added, removed or
    // renamed in Rust fails here rather than reaching a consumer whose type
    // says otherwise.
    assert_eq!(
        object_keys(published),
        vec!["bindings", "blockEnd", "blockStart"],
        "the destructured block's published keys must be the ones its declaration states"
    );
    assert_eq!(
        object_keys(binding),
        vec!["name", "sourceEnd", "sourceStart"],
        "a binding row's published keys must be the ones its declaration states"
    );
}

/// An object's own keys, sorted, so a comparison does not depend on the
/// order a serialiser happened to emit.
#[track_caller]
fn object_keys(value: &Value) -> Vec<&str> {
    let mut keys: Vec<&str> = value
        .as_object()
        .expect("the value is an object")
        .keys()
        .map(String::as_str)
        .collect();
    keys.sort_unstable();
    keys
}

/// Published diagnostic spans are UTF-16 code units, and they are the
/// legacy route's own.
///
/// The clean fixtures above publish an EMPTY diagnostic set, so their
/// legacy-equivalence assertion compares nothing. This one publishes a real
/// spanned warning and closes both halves of the contract:
///
/// - **Encoding.** Two carriers that differ only in a prefix of equal
///   UTF-16 length and unequal byte length must report the SAME span. A
///   byte-indexed span would differ by the five-byte gap; comparing only
///   against the legacy route would pass if both published bytes.
/// - **Equivalence.** The multi-byte carrier's set must equal what the
///   legacy per-node read publishes for the same demand — non-empty, so the
///   comparison discriminates.
#[wasm_bindgen_test]
fn published_diagnostic_spans_are_utf16_and_match_the_legacy_route() {
    let multi_byte = diagnostic_carrier(MULTI_BYTE_PREFIX);
    let ascii = diagnostic_carrier(ASCII_PREFIX);
    assert_eq!(
        multi_byte.encode_utf16().count(),
        ascii.encode_utf16().count(),
        "fixture drift: the two carriers must have equal UTF-16 length"
    );
    assert_ne!(
        multi_byte.len(),
        ascii.len(),
        "fixture drift: the two carriers must have unequal byte length, else this proves nothing"
    );

    let host = js_host();
    register(&host, "/src/Warn.vue", &multi_byte, "vue");
    register(&host, "/src/WarnAscii.vue", &ascii, "vue");

    let demand = || vue_route_request(json!([runtime_client_product(false)]));
    let multi_byte_response = compile_request(&host, "/src/Warn.vue", demand());
    let ascii_response = compile_request(&host, "/src/WarnAscii.vue", demand());

    let spans = |response: &Value| -> Vec<(u64, u64)> {
        response["diagnostics"]["diagnostics"]
            .as_array()
            .expect("the response carries a diagnostic list")
            .iter()
            .map(|diagnostic| {
                (
                    diagnostic["spanStart"].as_u64().expect("a start offset"),
                    diagnostic["spanEnd"].as_u64().expect("an end offset"),
                )
            })
            .collect()
    };

    let multi_byte_spans = spans(&multi_byte_response);
    assert!(
        !multi_byte_spans.is_empty(),
        "fixture drift: the malformed interpolation must publish a spanned diagnostic, got {:?}",
        multi_byte_response["diagnostics"]
    );
    assert!(
        multi_byte_spans.iter().any(|(start, _)| *start > 0),
        "a zero-only span set cannot distinguish the two encodings"
    );
    assert_eq!(
        multi_byte_spans,
        spans(&ascii_response),
        "diagnostic offsets must be UTF-16 code units: a byte-indexed span moves when the \
         multi-byte prefix does"
    );

    let legacy_main = legacy_virtual_file(&host, "/src/Warn.vue", "main", None);
    assert_eq!(
        multi_byte_response["diagnostics"], legacy_main["diagnostics"],
        "the published diagnostics must be the legacy route's own"
    );
}

/// The published diagnostic's argument list survives `serde_wasm_bindgen`.
///
/// The argument list is what a caller re-renders a diagnostic message from,
/// and it reaches this route through serde, not through the NAPI struct
/// mirror the native binding uses — so the native binding's own coverage
/// proves nothing about this wire. A serialiser that dropped the field, or
/// a producer that stopped filling it, would leave every browser consumer
/// silently unable to tell an argument-less diagnostic from a diagnostic
/// whose arguments were lost.
///
/// Mutation recipe: add `#[serde(skip)]` to `FfiDiagnostic::arguments` (or
/// drop the field from `host_diagnostic_to_ffi`'s literal and the struct).
/// The key stops reaching the browser wire and `is_array` goes false, while
/// the native binding's own struct-mirror coverage stays green.
#[wasm_bindgen_test]
fn published_diagnostics_carry_their_argument_list_through_serde() {
    let host = js_host();
    register(&host, "/src/Warn.vue", &diagnostic_carrier(ASCII_PREFIX), "vue");

    let response = compile_request(
        &host,
        "/src/Warn.vue",
        vue_route_request(json!([runtime_client_product(false)])),
    );

    let diagnostics = response["diagnostics"]["diagnostics"]
        .as_array()
        .expect("the response carries a diagnostic list");
    assert!(
        !diagnostics.is_empty(),
        "fixture drift: the duplicate directive must publish a diagnostic, got {:?}",
        response["diagnostics"]
    );
    for diagnostic in diagnostics {
        assert!(
            diagnostic["arguments"].is_array(),
            "every published diagnostic carries its argument list: {diagnostic:?}"
        );
    }

    let legacy_main = legacy_virtual_file(&host, "/src/Warn.vue", "main", None);
    assert_eq!(
        response["diagnostics"], legacy_main["diagnostics"],
        "the typed route publishes the legacy route's own diagnostics, arguments included"
    );
}

// ── refusals throw ───────────────────────────────────────────────────────

/// The decode boundary's refusals reach this route unchanged: an unknown key
/// is refused by name, on the method a browser calls.
#[wasm_bindgen_test]
fn an_unknown_property_is_refused_by_name_on_this_route() {
    let host = js_host();
    register(&host, "/src/App.vue", VUE_SFC, "vue");

    let message = compile_request_refusal(
        &host,
        "/src/App.vue",
        set(
            vue_route_request(json!([template_analysis_product()])),
            &["vue", "options"],
            "target",
            json!("ide"),
        ),
    );
    assert!(
        message.contains("unknown field") && message.contains("target"),
        "expected an unknown-field refusal naming `target`, got: {message}"
    );
}

/// Normalizing explicit `undefined` tags must not erase an unknown property
/// with JavaScript's special prototype-setter spelling. The request, its
/// framework body, and every product row are cloned independently, so each
/// level is a distinct boundary.
#[wasm_bindgen_test]
fn an_own_enumerable_proto_property_is_refused_at_every_normalized_level() {
    let host = js_host();
    register(&host, "/src/App.vue", VUE_SFC, "vue");

    let cases = [
        (
            "request",
            Vec::<&str>::new(),
            false,
            set(
                vue_route_request(json!([template_analysis_product()])),
                &[],
                "__proto__",
                json!({ "polluted": true }),
            ),
        ),
        (
            "framework body",
            vec!["vue"],
            true,
            set(
                vue_route_request(json!([template_analysis_product()])),
                &["vue"],
                "__proto__",
                json!({ "polluted": true }),
            ),
        ),
        (
            "product row",
            vec!["vue", "products", "0"],
            false,
            set(
                vue_route_request(json!([template_analysis_product()])),
                &["vue", "products", "0"],
                "__proto__",
                json!({ "polluted": true }),
            ),
        ),
    ];

    for (level, path, names_unknown_field, wire) in cases {
        let request = json_parsed_js(wire);
        let mut target = request.clone();
        for segment in path {
            target = Reflect::get(&target, &JsValue::from_str(segment))
                .expect("the parsed fixture exposes every path segment");
        }
        assert!(
            Object::keys(target.unchecked_ref::<Object>())
                .iter()
                .any(|key| key.as_string().as_deref() == Some("__proto__")),
            "fixture drift: {level} does not carry an own enumerable `__proto__` property"
        );

        let message = compile_request_refusal_js(&host, "/src/App.vue", request);
        assert!(
            !message.is_empty(),
            "expected {level}'s unknown `__proto__` property to be refused"
        );
        if names_unknown_field {
            assert!(
                message.contains("unknown field") && message.contains("__proto__"),
                "expected {level}'s unknown `__proto__` property to be refused by name, got: {message}"
            );
        }
    }
}

/// The other framework's option key is refused on this route too — the arms
/// are structurally separate, and the separation has to hold where a browser
/// caller meets it.
#[wasm_bindgen_test]
fn the_other_frameworks_option_is_refused_on_this_route() {
    let host = js_host();
    register(&host, "/src/App.vue", VUE_SFC, "vue");

    let message = compile_request_refusal(
        &host,
        "/src/App.vue",
        set(
            vue_route_request(json!([template_analysis_product()])),
            &["vue", "options"],
            "runes",
            json!("infer"),
        ),
    );
    assert!(
        message.contains("unknown field") && message.contains("runes"),
        "expected an unknown-field refusal naming `runes`, got: {message}"
    );
}

/// A request whose framework arm contradicts the registered carrier throws
/// rather than compiling the carrier under the wrong framework.
#[wasm_bindgen_test]
fn a_framework_arm_the_carrier_contradicts_throws() {
    let host = js_host();
    register(&host, "/src/App.vue", VUE_SFC, "vue");

    let message = compile_request_refusal(
        &host,
        "/src/App.vue",
        svelte_route_request(json!([runtime_client_product(false)])),
    );
    let lowered = message.to_ascii_lowercase();
    assert!(
        message.contains("/src/App.vue") && lowered.contains("svelte") && lowered.contains("vue"),
        "the refusal must name the carrier and both frameworks, got: {message}"
    );
}

/// An unregistered canonical id throws. The nearest wrong answers — `null`,
/// `undefined`, or a response carrying an empty product list — would each
/// make a caller believe the carrier compiled to nothing.
#[wasm_bindgen_test]
fn an_unregistered_canonical_id_throws_rather_than_answering_empty() {
    let host = js_host();

    let message = compile_request_refusal(
        &host,
        "/src/Missing.vue",
        vue_route_request(json!([template_analysis_product()])),
    );
    let lowered = message.to_ascii_lowercase();
    assert!(
        message.contains("/src/Missing.vue") && lowered.contains("missingsource"),
        "the refusal must name the missing carrier and say the source is absent, got: {message}"
    );
}

/// A product the host integration has no production route for fails the
/// WHOLE request: no sibling product is published beside it.
///
/// The offending product is named the way the REQUEST named it. The
/// assertion is byte-exact rather than case-folded on purpose: folding
/// would accept the Rust variant spelling `PublicApi`, and a refusal that
/// answers in a vocabulary the caller never wrote is a refusal the caller
/// cannot match against the product it sent.
#[wasm_bindgen_test]
fn an_unproducible_product_is_refused_in_the_requests_own_vocabulary() {
    let host = js_host();
    register(&host, "/src/App.vue", VUE_SFC, "vue");

    let message = compile_request_refusal(
        &host,
        "/src/App.vue",
        vue_route_request(json!([template_analysis_product(), "publicApi"])),
    );
    assert!(
        message.contains("/src/App.vue") && message.contains("publicApi"),
        "the refusal must name the carrier and the unproducible kind as `publicApi`, got: {message}"
    );
}
