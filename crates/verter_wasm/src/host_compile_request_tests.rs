//! The typed host-compile-request adapter at the WASM binding boundary:
//! what the wire schema refuses, what the canonical request refuses, and
//! which of the two a caller is told refused it.
//!
//! Per-option, per-flag and per-product value crossing is not restated
//! here: that is the shared FFI converter's behaviour and is exhaustively
//! proven where it lives. What this adapter owns is the wire, so what is
//! proven here is wire-level — JSON `null` against an absent key, a value
//! outside a closed vocabulary, a key the schema does not declare — plus
//! the arm each refusal arrives on.
//!
//! The fixtures drive [`host_compile_request_from_wire`] through a
//! `serde_json::Value` deserializer, which is the representation the JS
//! boundary converts its payload to before reaching that same entry. The
//! `js_boundary` module at the end proves the conversion carries the
//! refusals, because a deserializer is free to hide keys from the schema.

use serde_json::{json, Value};

use verter_compiler::compile_request::{
    CompileProduct, CompileRequest, CompileRequestError, FrameworkCompileRequest, FrameworkOption,
    ProductKind, SvelteOption, VueOption,
};
use verter_ffi::convert::HostResolvedCompileProfiles;

use crate::{host_compile_request_from_wire, HostCompileRequestError};

// ── fixtures ─────────────────────────────────────────────────────────────

fn no_profiles() -> HostResolvedCompileProfiles {
    HostResolvedCompileProfiles {
        semantic: None,
        output: None,
        presentation: None,
        serialization: None,
    }
}

fn convert(wire: Value) -> Result<CompileRequest, HostCompileRequestError<serde_json::Error>> {
    host_compile_request_from_wire(wire, &no_profiles())
}

/// The schema arm must have refused, and the message must name the offence.
#[track_caller]
fn schema_refusal(wire: Value) -> String {
    match convert(wire) {
        Err(HostCompileRequestError::Decode(error)) => error.to_string(),
        Err(HostCompileRequestError::Request(error)) => {
            panic!("expected a schema refusal, got the canonical refusal {error:?}")
        }
        Ok(request) => panic!("expected a schema refusal, got a request for {:?}", {
            request.framework()
        }),
    }
}

/// The canonical constructor must have refused — not the schema.
#[track_caller]
fn canonical_refusal(wire: Value) -> CompileRequestError {
    match convert(wire) {
        Err(HostCompileRequestError::Request(error)) => error,
        Err(HostCompileRequestError::Decode(error)) => {
            panic!("expected a canonical refusal, got the schema refusal: {error}")
        }
        Ok(_) => panic!("expected a canonical refusal, got a converted request"),
    }
}

#[track_caller]
fn converted(wire: Value) -> CompileRequest {
    match convert(wire) {
        Ok(request) => request,
        Err(HostCompileRequestError::Decode(error)) => {
            panic!("schema refused the payload: {error}")
        }
        Err(HostCompileRequestError::Request(error)) => {
            panic!("canonical request refused the payload: {error:?}")
        }
    }
}

/// Descends `path` (object keys, or a decimal index inside an array) and
/// writes `key` into the object found there. Used both to plant an unknown
/// key at a chosen nesting level and to overwrite a well-known one with a
/// malformed value.
#[track_caller]
fn set(mut root: Value, path: &[&str], key: &str, value: Value) -> Value {
    at(&mut root, path)
        .as_object_mut()
        .expect("an object at the target path")
        .insert(key.to_string(), value);
    root
}

/// Removes `key` from the object at `path` — an absent required field.
#[track_caller]
fn unset(mut root: Value, path: &[&str], key: &str) -> Value {
    let removed = at(&mut root, path)
        .as_object_mut()
        .expect("an object at the target path")
        .remove(key);
    assert!(
        removed.is_some(),
        "fixture drift: `{key}` was not present at {path:?} to begin with"
    );
    root
}

#[track_caller]
fn at<'value>(root: &'value mut Value, path: &[&str]) -> &'value mut Value {
    let mut cursor = root;
    for segment in path {
        cursor = match cursor {
            Value::Object(map) => map
                .get_mut(*segment)
                .unwrap_or_else(|| panic!("fixture drift: no `{segment}` key")),
            Value::Array(items) => {
                let index: usize = segment.parse().expect("an array index segment");
                items
                    .get_mut(index)
                    .unwrap_or_else(|| panic!("fixture drift: no index {index}"))
            }
            other => panic!("cannot descend into {other}"),
        };
    }
    cursor
}

fn identity() -> Value {
    json!({
        "filename": "src/Comp.vue",
        "componentId": "comp-7",
        "isProduction": false,
        "forceJs": false,
    })
}

/// Every optional Vue slot absent; only the four required ones present.
fn minimal_vue_options() -> Value {
    json!({
        "backend": "inferred",
        "ssr": false,
        "isCustomElement": [],
        "babelParserPlugins": [],
    })
}

fn analysis_product() -> Value {
    json!({ "analysis": { "wantScriptBindings": true, "wantTemplateData": false } })
}

fn ide_product() -> Value {
    json!({ "ideCompanion": {
        "wantSourceMap": true,
        "embedAmbientTypes": false,
        "conditionalRootNarrowing": true,
        "strictSlots": false,
        "typesModuleName": "verter/types",
        "ideChunkBoundaries": true,
    }})
}

fn runtime_client_product() -> Value {
    json!({ "runtimeClient": { "inline": false, "runtimeSourceMap": true } })
}

fn vue(products: Value, options: Value) -> Value {
    json!({ "vue": { "identity": identity(), "products": products, "options": options } })
}

fn svelte(products: Value, options: Value) -> Value {
    json!({ "svelte": { "identity": identity(), "products": products, "options": options } })
}

/// The baseline both frameworks perturb: one analysis product, no options.
fn minimal_vue() -> Value {
    vue(json!([analysis_product()]), minimal_vue_options())
}

fn minimal_svelte() -> Value {
    svelte(json!([analysis_product()]), json!({}))
}

// ── the payload reaches exactly one canonical request ────────────────────

#[test]
fn a_vue_payload_becomes_exactly_one_canonical_vue_request() {
    let request = converted(minimal_vue());

    assert!(matches!(
        request.framework(),
        FrameworkCompileRequest::Vue(_)
    ));
    assert!(request.svelte().is_none());
    assert_eq!(request.filename(), Some("src/Comp.vue"));
    assert_eq!(request.component_id(), Some("comp-7"));
    assert!(!request.is_production());
    assert!(!request.force_js());
    assert_eq!(request.products().len(), 1);
    assert_eq!(request.products()[0].kind(), ProductKind::Analysis);
}

#[test]
fn a_svelte_payload_becomes_exactly_one_canonical_svelte_request() {
    let request = converted(minimal_svelte());

    assert!(matches!(
        request.framework(),
        FrameworkCompileRequest::Svelte(_)
    ));
    assert!(request.vue().is_none());
    assert_eq!(request.filename(), Some("src/Comp.vue"));
    assert_eq!(request.products().len(), 1);
    assert_eq!(request.products()[0].kind(), ProductKind::Analysis);
}

fn wire_product_for(kind: ProductKind) -> Value {
    match kind {
        ProductKind::RuntimeClient => runtime_client_product(),
        ProductKind::RuntimeServer => {
            json!({ "runtimeServer": { "inline": null, "runtimeSourceMap": false } })
        }
        ProductKind::IdeCompanion => ide_product(),
        ProductKind::PublicApi => json!("publicApi"),
        ProductKind::Declarations => json!("declarations"),
        ProductKind::Analysis => analysis_product(),
    }
}

/// Distinct seeds so a slot wired to the wrong profile class fails rather
/// than passing by coincidence.
struct ProfileSeed(&'static str);

impl verter_identity::encoding::CanonicalEncode for ProfileSeed {
    const DOMAIN_TAG: &'static str = "verter-wasm.compile-request-profile-seed.v1";
    fn encode_fields(&self, encoder: &mut verter_identity::encoding::CanonicalEncoder) {
        encoder.field_str(1, self.0);
    }
}

#[test]
fn the_callers_resolved_profiles_reach_the_canonical_request() {
    use verter_identity::profile::{
        OutputProfileId, PresentationProfileId, SerializationProfileId, TypeScriptSemanticProfileId,
    };

    let semantic = TypeScriptSemanticProfileId::from_canonical(&ProfileSeed("semantic"));
    let output = OutputProfileId::from_canonical(&ProfileSeed("output"));
    let presentation = PresentationProfileId::from_canonical(&ProfileSeed("presentation"));
    let serialization = SerializationProfileId::from_canonical(&ProfileSeed("serialization"));

    let request = host_compile_request_from_wire(
        vue(
            json!([runtime_client_product(), ide_product()]),
            minimal_vue_options(),
        ),
        &HostResolvedCompileProfiles {
            semantic: Some(semantic.clone()),
            output: Some(output.clone()),
            presentation: Some(presentation.clone()),
            serialization: Some(serialization.clone()),
        },
    )
    .expect("converts");

    assert_eq!(request.semantic_profile(), Some(&semantic));
    for product in request.products() {
        match product {
            CompileProduct::RuntimeClient(runtime) => {
                assert_eq!(runtime.output_profile.as_ref(), Some(&output));
                assert_eq!(runtime.serialization.as_ref(), Some(&serialization));
            }
            CompileProduct::IdeCompanion(ide) => {
                assert_eq!(ide.output_profile.as_ref(), Some(&output));
                assert_eq!(ide.diagnostics.as_ref(), Some(&presentation));
                assert_eq!(ide.serialization.as_ref(), Some(&serialization));
            }
            other => panic!("unexpected product {other:?}"),
        }
    }
}

// ── the schema refuses ───────────────────────────────────────────────────

#[track_caller]
fn assert_unknown_field(label: &str, wire: Value, field: &str) {
    let message = schema_refusal(wire);
    assert!(
        message.contains("unknown field") && message.contains(field),
        "{label}: expected an unknown-field refusal naming `{field}`, got: {message}"
    );
}

#[test]
fn an_unknown_key_is_refused_at_every_nesting_level() {
    let cases: Vec<(&str, Value)> = vec![
        (
            "vue arm",
            set(minimal_vue(), &["vue"], "target", json!("ide")),
        ),
        (
            "identity",
            set(minimal_vue(), &["vue", "identity"], "target", json!("ide")),
        ),
        (
            "vue options",
            set(minimal_vue(), &["vue", "options"], "target", json!("ide")),
        ),
        (
            "vue css modules",
            set(
                vue(
                    json!([analysis_product()]),
                    set(minimal_vue_options(), &[], "cssModules", json!({})),
                ),
                &["vue", "options", "cssModules"],
                "target",
                json!("ide"),
            ),
        ),
        (
            "vue asset-url options",
            set(
                vue(
                    json!([analysis_product()]),
                    set(
                        minimal_vue_options(),
                        &[],
                        "transformAssetUrls",
                        json!({ "enabled": { "tags": {} } }),
                    ),
                ),
                &["vue", "options", "transformAssetUrls", "enabled"],
                "target",
                json!("ide"),
            ),
        ),
        (
            "runtime product",
            set(
                vue(json!([runtime_client_product()]), minimal_vue_options()),
                &["vue", "products", "0", "runtimeClient"],
                "target",
                json!("ide"),
            ),
        ),
        (
            "ide product",
            set(
                vue(json!([ide_product()]), minimal_vue_options()),
                &["vue", "products", "0", "ideCompanion"],
                "target",
                json!("ide"),
            ),
        ),
        (
            "analysis product",
            set(
                minimal_vue(),
                &["vue", "products", "0", "analysis"],
                "target",
                json!("ide"),
            ),
        ),
        (
            "svelte arm",
            set(minimal_svelte(), &["svelte"], "target", json!("ide")),
        ),
        (
            "svelte options",
            set(
                minimal_svelte(),
                &["svelte", "options"],
                "target",
                json!("ide"),
            ),
        ),
        (
            "svelte compatibility",
            set(
                svelte(json!([analysis_product()]), json!({ "compatibility": {} })),
                &["svelte", "options", "compatibility"],
                "componentApi",
                json!(4),
            ),
        ),
        (
            "svelte custom-element descriptor",
            set(
                svelte(
                    json!([analysis_product()]),
                    json!({ "customElementDescriptor": { "props": {} } }),
                ),
                &["svelte", "options", "customElementDescriptor"],
                "target",
                json!("ide"),
            ),
        ),
        (
            "svelte custom-element prop",
            set(
                svelte(
                    json!([analysis_product()]),
                    json!({ "customElementDescriptor": { "props": { "label": {} } } }),
                ),
                &[
                    "svelte",
                    "options",
                    "customElementDescriptor",
                    "props",
                    "label",
                ],
                "target",
                json!("ide"),
            ),
        ),
    ];

    for (label, wire) in cases {
        let field = if label == "svelte compatibility" {
            "componentApi"
        } else {
            "target"
        };
        assert_unknown_field(label, wire, field);
    }
}

#[test]
fn the_other_frameworks_option_inside_an_arm_is_refused_at_decode() {
    assert_unknown_field(
        "a Svelte option in the Vue arm",
        set(minimal_vue(), &["vue", "options"], "runes", json!("infer")),
        "runes",
    );
    assert_unknown_field(
        "a Vue option in the Svelte arm",
        set(
            minimal_svelte(),
            &["svelte", "options"],
            "propsDestructure",
            json!(true),
        ),
        "propsDestructure",
    );
}

#[test]
fn an_unknown_framework_is_refused_at_decode() {
    let message = schema_refusal(json!({ "react": {
        "identity": identity(),
        "products": [analysis_product()],
        "options": {},
    }}));
    assert!(
        message.contains("unknown variant") && message.contains("react"),
        "expected an unknown-variant refusal naming `react`, got: {message}"
    );
}

#[test]
fn two_framework_arms_in_one_payload_are_refused_at_decode() {
    let mut wire = minimal_vue();
    wire.as_object_mut()
        .expect("the framework map")
        .insert("svelte".to_string(), minimal_svelte()["svelte"].clone());

    // Not an assertion about the wording — only that a payload naming two
    // frameworks never resolves to one of them.
    schema_refusal(wire);
}

#[test]
fn an_unknown_product_is_refused_at_decode() {
    let message = schema_refusal(vue(json!(["styles"]), minimal_vue_options()));
    assert!(
        message.contains("unknown variant") && message.contains("styles"),
        "expected an unknown-variant refusal naming `styles`, got: {message}"
    );
}

#[test]
fn a_value_outside_a_closed_vocabulary_is_refused_at_decode() {
    let vue_cases: Vec<(&str, Value, &str)> = vec![
        ("backend", json!("wasm"), "wasm"),
        ("whitespace", json!("trim"), "trim"),
        ("parsePad", json!("tab"), "tab"),
    ];
    for (option, value, spelling) in vue_cases {
        let message = schema_refusal(set(minimal_vue(), &["vue", "options"], option, value));
        assert!(
            message.contains("unknown variant") && message.contains(spelling),
            "{option}: expected an unknown-variant refusal naming `{spelling}`, got: {message}"
        );
    }

    let css_module_cases: Vec<(&str, Value, &str)> = vec![
        ("scopeBehaviour", json!("scoped"), "scoped"),
        ("localsConvention", json!("snakeCase"), "snakeCase"),
    ];
    for (option, value, spelling) in css_module_cases {
        let message = schema_refusal(vue(
            json!([analysis_product()]),
            set(
                minimal_vue_options(),
                &[],
                "cssModules",
                json!({ option: value }),
            ),
        ));
        assert!(
            message.contains("unknown variant") && message.contains(spelling),
            "cssModules.{option}: expected an unknown-variant refusal naming `{spelling}`, got: {message}"
        );
    }

    let svelte_cases: Vec<(&str, Value, &str)> = vec![
        ("namespace", json!("xml"), "xml"),
        ("css", json!("inline"), "inline"),
        ("fragments", json!("dom"), "dom"),
        ("runes", json!("maybe"), "maybe"),
    ];
    for (option, value, spelling) in svelte_cases {
        let message = schema_refusal(set(minimal_svelte(), &["svelte", "options"], option, value));
        assert!(
            message.contains("unknown variant") && message.contains(spelling),
            "{option}: expected an unknown-variant refusal naming `{spelling}`, got: {message}"
        );
    }

    // The custom-element prop-type vocabulary is decided by the canonical
    // request constructor, not the wire schema: an unrecognised spelling
    // reaches the canonical arm, naming the option row and the value.
    match canonical_refusal(svelte(
        json!([analysis_product()]),
        json!({ "customElementDescriptor": {
            "props": { "label": { "propType": "Symbol" } },
        }}),
    )) {
        CompileRequestError::MalformedOptionValue { option, value } => {
            assert_eq!(
                option,
                FrameworkOption::Svelte(SvelteOption::CustomElementPropsType)
            );
            assert_eq!(value, "Symbol");
        }
        other => panic!("propType: expected MalformedOptionValue, got {other:?}"),
    }
}

#[test]
fn an_absent_required_field_is_refused_rather_than_defaulted() {
    let cases: Vec<(&str, Value)> = vec![
        (
            "identity",
            unset(minimal_vue(), &["vue", "identity"], "isProduction"),
        ),
        (
            "identity",
            unset(minimal_vue(), &["vue", "identity"], "forceJs"),
        ),
        (
            "options",
            unset(minimal_vue(), &["vue", "options"], "backend"),
        ),
        ("options", unset(minimal_vue(), &["vue", "options"], "ssr")),
        (
            "options",
            unset(minimal_vue(), &["vue", "options"], "isCustomElement"),
        ),
        (
            "options",
            unset(minimal_vue(), &["vue", "options"], "babelParserPlugins"),
        ),
        (
            "analysis product",
            unset(
                minimal_vue(),
                &["vue", "products", "0", "analysis"],
                "wantScriptBindings",
            ),
        ),
        (
            "runtime product",
            unset(
                vue(json!([runtime_client_product()]), minimal_vue_options()),
                &["vue", "products", "0", "runtimeClient"],
                "runtimeSourceMap",
            ),
        ),
        (
            "ide product",
            unset(
                vue(json!([ide_product()]), minimal_vue_options()),
                &["vue", "products", "0", "ideCompanion"],
                "strictSlots",
            ),
        ),
        (
            "asset-url options",
            unset(
                vue(
                    json!([analysis_product()]),
                    set(
                        minimal_vue_options(),
                        &[],
                        "transformAssetUrls",
                        json!({ "enabled": { "tags": {} } }),
                    ),
                ),
                &["vue", "options", "transformAssetUrls", "enabled"],
                "tags",
            ),
        ),
        (
            "svelte descriptor",
            unset(
                svelte(
                    json!([analysis_product()]),
                    json!({ "customElementDescriptor": { "props": {} } }),
                ),
                &["svelte", "options", "customElementDescriptor"],
                "props",
            ),
        ),
    ];

    for (label, wire) in cases {
        let message = schema_refusal(wire);
        assert!(
            message.contains("missing field"),
            "{label}: expected a missing-field refusal, got: {message}"
        );
    }
}

#[test]
fn a_value_of_the_wrong_type_is_refused_at_decode() {
    let cases: Vec<(&str, Value)> = vec![
        (
            "a string where a flag belongs",
            set(
                minimal_vue(),
                &["vue", "identity"],
                "isProduction",
                json!("yes"),
            ),
        ),
        (
            "an object where the product list belongs",
            set(minimal_vue(), &["vue"], "products", json!({})),
        ),
        (
            "a string where the tag list belongs",
            set(
                minimal_vue(),
                &["vue", "options"],
                "isCustomElement",
                json!("my-el"),
            ),
        ),
        (
            "a number where a module name belongs",
            set(
                minimal_vue(),
                &["vue", "options"],
                "runtimeModuleName",
                json!(7),
            ),
        ),
    ];

    for (label, wire) in cases {
        // Each is a schema refusal, never a coerced value.
        let message = schema_refusal(wire);
        assert!(
            message.contains("invalid type"),
            "{label}: expected an invalid-type refusal, got: {message}"
        );
    }
}

// ── the canonical request refuses ────────────────────────────────────────

#[test]
fn a_refused_option_reaches_the_caller_as_the_canonical_arm_naming_its_row() {
    assert_eq!(
        canonical_refusal(set(
            minimal_vue(),
            &["vue", "options"],
            "codegenMode",
            json!(false),
        )),
        CompileRequestError::UnsupportedOption {
            option: FrameworkOption::Vue(VueOption::CodegenOptionsMode),
            capability: None,
        },
        "a supplied unsupported option is refused on presence, even at `false`"
    );

    let refusal = canonical_refusal(set(
        minimal_svelte(),
        &["svelte", "options"],
        "accessors",
        json!(false),
    ));
    assert!(
        matches!(
            refusal,
            CompileRequestError::UnsupportedOption {
                option: FrameworkOption::Svelte(SvelteOption::CompileOptionsAccessors),
                ..
            }
        ),
        "expected the Svelte `accessors` row, got {refusal:?}"
    );
}

#[test]
fn a_supplied_option_at_null_is_read_as_absent_not_as_supplied() {
    let request = converted(set(
        minimal_vue(),
        &["vue", "options"],
        "codegenMode",
        Value::Null,
    ));
    assert!(request.vue().is_some());
}

#[test]
fn an_inadmissible_product_set_is_refused_by_the_canonical_request() {
    assert_eq!(
        canonical_refusal(vue(json!([]), minimal_vue_options())),
        CompileRequestError::EmptyProductSet
    );
    assert_eq!(
        canonical_refusal(vue(
            json!([analysis_product(), analysis_product()]),
            minimal_vue_options(),
        )),
        CompileRequestError::DuplicateProduct(ProductKind::Analysis)
    );
    assert_eq!(
        canonical_refusal(vue(
            json!([wire_product_for(ProductKind::RuntimeServer)]),
            set(minimal_vue_options(), &[], "backend", json!("vapor")),
        )),
        CompileRequestError::SsrVaporBackendUnsupported
    );
    assert_eq!(
        canonical_refusal(vue(
            json!([{ "runtimeServer": { "inline": true, "runtimeSourceMap": false } }]),
            minimal_vue_options(),
        )),
        CompileRequestError::InlineSsrUnsupported
    );
}

// ── the two arms stay distinguishable to the caller ──────────────────────

#[test]
fn each_arm_renders_a_message_that_names_which_authority_refused() {
    let schema = HostCompileRequestError::Decode(
        serde_json::from_str::<crate::FfiHostCompileIdentity>("{}").expect_err("a decode error"),
    )
    .to_string();
    assert!(
        schema.starts_with("invalid host compile request:") && schema.contains("missing field"),
        "a schema refusal must read as one: {schema}"
    );

    let canonical =
        HostCompileRequestError::<serde_json::Error>::Request(CompileRequestError::EmptyProductSet)
            .to_string();
    assert!(
        canonical.starts_with("refused host compile request:")
            && canonical.contains("product set is empty"),
        "a canonical refusal must read as one and name its rule: {canonical}"
    );
    // The rule's own words come from the compiler's shared refusal
    // vocabulary, not from a Rust variant spelling this binding printed.
    assert!(
        !canonical.contains("EmptyProductSet"),
        "a browser caller must not be shown the Rust variant name: {canonical}"
    );
    assert_ne!(
        schema, canonical,
        "the two authorities must not render identically"
    );
}

// ── the JS boundary ──────────────────────────────────────────────────────
//
// Everything above drives the shared entry through `serde_json`. These
// drive the JS entry with a real JS object graph, because a deserializer
// is free to visit only the fields a schema declares and thereby never
// reach `deny_unknown_fields` at all. A payload carrying an unknown key,
// or the other framework's option key, must be refused where a browser
// caller meets it and not merely where a Rust fixture does.
//
// The last three go one step further: a `serde_json::Value` fixture cannot
// state an own key valued `undefined` — JSON has no such value, so the key
// vanishes on the round trip — while a browser caller produces one by
// forwarding an unset variable. Those cases build the object graph
// directly, because that shape is exactly where a closed schema can be
// closed on paper and open in practice.

#[cfg(target_arch = "wasm32")]
mod js_boundary {
    use super::*;

    use js_sys::Reflect;
    use serde::Serialize;
    use wasm_bindgen::JsValue;
    use wasm_bindgen_test::wasm_bindgen_test;

    use crate::host_compile_request_from_js;

    /// The fixture as a plain JS object graph — what a browser caller
    /// hands the binding. `serialize_maps_as_objects` matters: the default
    /// serializer renders a map as a JS `Map`, which is not the shape
    /// under test.
    fn js(wire: Value) -> JsValue {
        wire.serialize(&serde_wasm_bindgen::Serializer::new().serialize_maps_as_objects(true))
            .expect("the fixture is representable as a plain JS object")
    }

    #[track_caller]
    fn js_refusal(wire: Value) -> String {
        js_refusal_js(js(wire))
    }

    /// The refusal path for a payload that had to be built as a JS object
    /// graph rather than serialised from a `Value`.
    #[track_caller]
    fn js_refusal_js(payload: JsValue) -> String {
        match host_compile_request_from_js(payload, &no_profiles()) {
            Ok(request) => panic!(
                "expected a refusal, got a request for {:?}",
                request.framework()
            ),
            Err(error) => error
                .as_string()
                .expect("the refusal reaches JS as a string"),
        }
    }

    /// The control. Without it a harness that refuses everything — or one
    /// that never ran the fixture at all — would read as proof.
    #[wasm_bindgen_test]
    fn a_valid_js_payload_reaches_the_canonical_request() {
        let request = host_compile_request_from_js(js(minimal_vue()), &no_profiles())
            .expect("the minimal Vue payload is admissible");

        assert!(matches!(
            request.framework(),
            FrameworkCompileRequest::Vue(_)
        ));
        assert_eq!(request.filename(), Some("src/Comp.vue"));
        assert_eq!(request.component_id(), Some("comp-7"));
        assert_eq!(request.products().len(), 1);
        assert_eq!(request.products()[0].kind(), ProductKind::Analysis);
    }

    #[wasm_bindgen_test]
    fn an_unknown_key_on_a_js_payload_is_refused() {
        let message = js_refusal(set(
            minimal_vue(),
            &["vue", "options"],
            "target",
            json!("ide"),
        ));
        assert!(
            message.contains("unknown field") && message.contains("target"),
            "expected an unknown-field refusal naming `target`, got: {message}"
        );
    }

    #[wasm_bindgen_test]
    fn the_other_frameworks_option_on_a_js_payload_is_refused() {
        let message = js_refusal(set(
            minimal_vue(),
            &["vue", "options"],
            "runes",
            json!("infer"),
        ));
        assert!(
            message.contains("unknown field") && message.contains("runes"),
            "expected an unknown-field refusal naming `runes`, got: {message}"
        );
    }

    /// The one payload shape a `serde_json::Value` fixture cannot state: an
    /// own key whose value is `undefined`. Serialising a `Value` cannot
    /// produce it — JSON has no such value, so the key is simply absent —
    /// yet it is trivially what a browser caller writes when it forwards an
    /// unset variable. This builds the fixture as a real JS object graph and
    /// then writes `undefined` at `path`/`key` through `Reflect::set`, so the
    /// key is genuinely own and enumerable when the schema enumerates it.
    fn js_with_undefined_at(wire: Value, path: &[&str], key: &str) -> JsValue {
        let root = js(wire);
        let mut cursor = root.clone();
        for segment in path {
            cursor = Reflect::get(&cursor, &JsValue::from_str(segment))
                .expect("the fixture exposes every path segment");
            assert!(
                cursor.is_object(),
                "fixture drift: `{segment}` is not an object"
            );
        }
        assert!(
            Reflect::set(&cursor, &JsValue::from_str(key), &JsValue::UNDEFINED)
                .expect("a plain object accepts an own key"),
            "the own key was not written"
        );
        root
    }

    /// The control for the two below. Without it a schema that refused every
    /// payload carrying an `undefined`, or a writer that wrote nothing at
    /// all, would read as proof. `codegenMode` is a declared optional slot,
    /// so stating it as `undefined` must stay admissible and be read as
    /// absent — the same reading a JSON `null` gets.
    #[wasm_bindgen_test]
    fn an_optional_slot_stated_as_undefined_stays_admissible() {
        let request = host_compile_request_from_js(
            js_with_undefined_at(minimal_vue(), &["vue", "options"], "codegenMode"),
            &no_profiles(),
        )
        .expect("an optional slot stated as undefined is read as absent");

        assert!(matches!(
            request.framework(),
            FrameworkCompileRequest::Vue(_)
        ));
    }

    #[wasm_bindgen_test]
    fn an_own_key_stated_as_undefined_is_still_refused_by_name() {
        let message = js_refusal_js(js_with_undefined_at(
            minimal_vue(),
            &["vue", "options"],
            "runes",
        ));
        assert!(
            message.contains("unknown field") && message.contains("runes"),
            "expected an unknown-field refusal naming `runes`, got: {message}"
        );
    }

    #[wasm_bindgen_test]
    fn a_required_slot_stated_as_undefined_is_refused_as_a_null_value() {
        let message = js_refusal_js(js_with_undefined_at(
            minimal_vue(),
            &["vue", "options"],
            "backend",
        ));
        assert!(
            message.contains("invalid type: null"),
            "expected a null-valued refusal for the required `backend` slot, got: {message}"
        );
    }
}

// The callable route on the generated host object, housed beside the decode
// fixtures it depends on: same wire form, same refusals, reached the way a
// browser reaches it rather than the way Rust does.
#[cfg(target_arch = "wasm32")]
#[path = "host_compile_request_route_tests.rs"]
mod js_route;
