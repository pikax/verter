//! Decode + conversion fixtures for the native tagged host compile request:
//! both frameworks, every product kind, every product flag in both
//! polarities, unknown keys at every nesting level, cross-framework keys,
//! closed-vocabulary spellings, and malformed values.

use serde_json::{json, Value};

use crate::{
    decode_host_compile_request, napi_host_compile_request_to_ffi, NapiHostCompileRequest,
};
use verter_compiler::compile_request::{
    CompileProduct, CompileRequestError, FrameworkCompileRequest,
};
use verter_ffi::convert::{
    ffi_host_compile_request_to_compile_request, HostResolvedCompileProfiles,
};
use verter_ffi::types::*;

// ── fixtures ─────────────────────────────────────────────────────────────

fn identity_json() -> Value {
    json!({
        "filename": "Comp.vue",
        "componentId": "c-1",
        "isProduction": false,
        "forceJs": false,
    })
}

fn identity() -> FfiHostCompileIdentity {
    FfiHostCompileIdentity {
        filename: Some("Comp.vue".to_string()),
        component_id: Some("c-1".to_string()),
        is_production: false,
        force_js: false,
        ssr_module_id: None,
        hmr_strategy: None,
    }
}

/// The four required Vue slots; every optional slot absent.
fn vue_options_json() -> Value {
    json!({
        "backend": "inferred",
        "ssr": false,
        "isCustomElement": [],
        "babelParserPlugins": [],
    })
}

/// Every Vue option slot, populated, as the wire spells it.
fn every_vue_option_slot_json() -> Value {
    json!({
        "backend": "vapor",
        "ssr": true,
        "isCustomElement": ["my-el"],
        "delimiters": ["<%", "%>"],
        "whitespace": "condense",
        "comments": true,
        "hoistStatic": false,
        "cacheHandlers": true,
        "hmr": false,
        "optimizeImports": true,
        "runtimeModuleName": "vue/runtime",
        "ssrRuntimeModuleName": "vue/server-renderer",
        "parsePad": "line",
        "ignoreEmpty": false,
        "babelParserPlugins": ["decorators"],
        "genDefaultAs": "_sfc_main",
        "propsDestructure": true,
        "scriptCustomElement": false,
        "transformAssetUrls": {
            "enabled": { "base": "/assets", "includeAbsolute": true, "tags": { "img": ["src"] } }
        },
        "styleTrim": true,
        "cssModules": {
            "scopeBehaviour": "global",
            "hashPrefix": "p",
            "localsConvention": "camelCaseOnly",
            "exportGlobals": true,
        },
        "compatConfig": true,
        "compatConfigMode": false,
        "compatConfigCompilerIsOnElement": true,
        "compatConfigCompilerVBindSync": false,
        "compatConfigCompilerVIfVForPrecedence": true,
        "compatConfigCompilerVBindObjectOrder": false,
        "compatConfigCompilerVOnNative": true,
        "compatConfigCompilerNativeTemplate": false,
        "compatConfigCompilerInlineTemplate": true,
        "compatConfigCompilerFilters": false,
        "transformCompatConfig": true,
        "codegenMode": false,
    })
}

/// The decoded value [`every_vue_option_slot_json`] must produce.
///
/// The literal is EXHAUSTIVE — every field named, no `..` and no
/// `..Default::default()` — so a slot added to the option schema stops
/// this crate compiling. That matters because the option types cross this
/// crate by move and are never destructured here: without the literal,
/// nothing in the native binding notices a new wire slot, and the
/// hand-written `packages/native/host-types.ts` mirror silently lacks it.
/// The nested option structs, the identity and each product payload are
/// named exhaustively for the same reason, in the fixtures below.
fn every_vue_option_slot() -> FfiVueCompileOptions {
    FfiVueCompileOptions {
        backend: FfiVueBackend::Vapor,
        ssr: true,
        is_custom_element: vec!["my-el".to_string()],
        delimiters: Some(vec!["<%".to_string(), "%>".to_string()]),
        whitespace: Some(FfiVueWhitespace::Condense),
        comments: Some(true),
        hoist_static: Some(false),
        cache_handlers: Some(true),
        hmr: Some(false),
        optimize_imports: Some(true),
        runtime_module_name: Some("vue/runtime".to_string()),
        ssr_runtime_module_name: Some("vue/server-renderer".to_string()),
        parse_pad: Some(FfiVueParsePad::Line),
        ignore_empty: Some(false),
        babel_parser_plugins: vec!["decorators".to_string()],
        gen_default_as: Some("_sfc_main".to_string()),
        props_destructure: Some(true),
        script_custom_element: Some(false),
        transform_asset_urls: Some(FfiVueAssetUrlTransform::Enabled(FfiVueAssetUrlOptions {
            base: Some("/assets".to_string()),
            include_absolute: Some(true),
            tags: [("img".to_string(), vec!["src".to_string()])]
                .into_iter()
                .collect(),
        })),
        style_trim: Some(true),
        css_modules: Some(FfiVueCssModules {
            scope_behaviour: Some(FfiVueCssModuleScopeBehaviour::Global),
            hash_prefix: Some("p".to_string()),
            locals_convention: Some(FfiVueCssModuleLocalsConvention::CamelCaseOnly),
            export_globals: Some(true),
        }),
        compat_config: Some(true),
        compat_config_mode: Some(false),
        compat_config_compiler_is_on_element: Some(true),
        compat_config_compiler_v_bind_sync: Some(false),
        compat_config_compiler_v_if_v_for_precedence: Some(true),
        compat_config_compiler_v_bind_object_order: Some(false),
        compat_config_compiler_v_on_native: Some(true),
        compat_config_compiler_native_template: Some(false),
        compat_config_compiler_inline_template: Some(true),
        compat_config_compiler_filters: Some(false),
        transform_compat_config: Some(true),
        codegen_mode: Some(false),
    }
}

/// Every Svelte option slot, populated, as the wire spells it.
fn every_svelte_option_slot_json() -> Value {
    json!({
        "dev": true,
        "generateModule": false,
        "experimentalAsync": true,
        "customElement": false,
        "customElementDescriptor": {
            "tag": "my-el",
            "shadow": true,
            "props": { "value": { "attribute": "value", "reflect": true, "propType": "number" } },
        },
        "namespace": "mathMl",
        "css": "external",
        "preserveComments": true,
        "preserveWhitespace": false,
        "fragments": "tree",
        "runes": "infer",
        "discloseVersion": false,
        "compatibility": {},
        "loose": true,
        "accessors": false,
        "immutable": true,
        "compatibilityComponentApi": false,
        "hmr": true,
        "customElementExtend": false,
    })
}

/// The decoded value [`every_svelte_option_slot_json`] must produce, named
/// exhaustively for the reason [`every_vue_option_slot`] records.
fn every_svelte_option_slot() -> FfiSvelteCompileOptions {
    FfiSvelteCompileOptions {
        dev: Some(true),
        generate_module: Some(false),
        experimental_async: Some(true),
        custom_element: Some(false),
        custom_element_descriptor: Some(FfiSvelteCustomElementDescriptor {
            tag: Some("my-el".to_string()),
            shadow: Some(true),
            props: [(
                "value".to_string(),
                FfiSvelteCustomElementProp {
                    attribute: Some("value".to_string()),
                    reflect: Some(true),
                    prop_type: Some("number".to_string()),
                },
            )]
            .into_iter()
            .collect(),
        }),
        namespace: Some(FfiSvelteNamespace::MathMl),
        css: Some(FfiSvelteCss::External),
        preserve_comments: Some(true),
        preserve_whitespace: Some(false),
        fragments: Some(FfiSvelteFragments::Tree),
        runes: Some(FfiSvelteRunes::Infer),
        disclose_version: Some(false),
        compatibility: Some(FfiSvelteCompatibility {}),
        loose: Some(true),
        accessors: Some(false),
        immutable: Some(true),
        compatibility_component_api: Some(false),
        hmr: Some(true),
        custom_element_extend: Some(false),
    }
}

fn analysis_product_json() -> Value {
    json!({ "kind": "analysis", "wantScriptBindings": true, "wantTemplateData": true })
}

fn vue_request_json(products: Value, options: Value) -> Value {
    json!({
        "framework": "vue",
        "identity": identity_json(),
        "products": products,
        "options": options,
    })
}

fn svelte_request_json(products: Value, options: Value) -> Value {
    json!({
        "framework": "svelte",
        "identity": identity_json(),
        "products": products,
        "options": options,
    })
}

fn decode(value: Value) -> NapiHostCompileRequest {
    decode_host_compile_request(value).expect("fixture decodes")
}

fn convert(value: Value) -> FfiHostCompileRequest {
    napi_host_compile_request_to_ffi(decode(value))
}

/// The refusal message for a request the adapter declines to decode. Every
/// refusal is an invalid-argument refusal, checked here so each caller can
/// pin the field and rule its own fixture violates.
fn refusal(value: Value) -> String {
    let error = decode_host_compile_request(value).expect_err("fixture is refused");
    assert_eq!(
        error.status,
        napi::Status::InvalidArg,
        "a malformed request must be refused as an invalid argument"
    );
    error.reason.clone()
}

/// The single FFI product a one-product request converts to.
fn only_product(value: Value) -> FfiRequestedProduct {
    let products = match convert(value) {
        FfiHostCompileRequest::Vue(request) => request.products,
        FfiHostCompileRequest::Svelte(request) => request.products,
    };
    let [product] = <[FfiRequestedProduct; 1]>::try_from(products)
        .expect("fixture requests exactly one product");
    product
}

fn no_profiles() -> HostResolvedCompileProfiles {
    HostResolvedCompileProfiles {
        semantic: None,
        output: None,
        presentation: None,
        serialization: None,
    }
}

// ── framework arms ───────────────────────────────────────────────────────

#[test]
fn a_vue_request_converts_to_the_vue_ffi_arm_field_for_field() {
    let converted = convert(vue_request_json(
        json!([analysis_product_json()]),
        vue_options_json(),
    ));

    let FfiHostCompileRequest::Vue(request) = converted else {
        panic!("a vue-tagged request must convert to the vue arm");
    };
    assert_eq!(request.identity, identity());
    assert_eq!(
        request.products,
        vec![FfiRequestedProduct::Analysis(FfiAnalysisProductRequest {
            want_script_bindings: true,
            want_template_data: true,
        })]
    );
    assert_eq!(request.options.backend, FfiVueBackend::Inferred);
    assert!(!request.options.ssr);
}

#[test]
fn a_svelte_request_converts_to_the_svelte_ffi_arm_field_for_field() {
    let converted = convert(svelte_request_json(
        json!([analysis_product_json()]),
        json!({ "dev": true }),
    ));

    let FfiHostCompileRequest::Svelte(request) = converted else {
        panic!("a svelte-tagged request must convert to the svelte arm");
    };
    assert_eq!(request.identity, identity());
    assert_eq!(
        request.products,
        vec![FfiRequestedProduct::Analysis(FfiAnalysisProductRequest {
            want_script_bindings: true,
            want_template_data: true,
        })]
    );
    assert_eq!(request.options.dev, Some(true));
}

#[test]
fn the_identity_flags_convert_in_both_polarities() {
    for is_production in [false, true] {
        for force_js in [false, true] {
            let request = vue_request_json(json!([analysis_product_json()]), vue_options_json());
            let mut request = request;
            request["identity"] = json!({
                "filename": Value::Null,
                "componentId": Value::Null,
                "isProduction": is_production,
                "forceJs": force_js,
            });

            let FfiHostCompileRequest::Vue(converted) = convert(request) else {
                panic!("vue arm");
            };
            assert_eq!(
                converted.identity,
                FfiHostCompileIdentity {
                    filename: None,
                    component_id: None,
                    is_production,
                    force_js,
                    ssr_module_id: None,
                    hmr_strategy: None,
                },
                "identity flags must cross unchanged for \
                 isProduction={is_production} forceJs={force_js}"
            );
        }
    }
}

// ── products ─────────────────────────────────────────────────────────────

/// Every native product tag paired with the FFI product it must become.
/// Adding a wire variant without a row here leaves that variant unproven,
/// and swapping any two rows' targets fails this test.
fn product_rows() -> Vec<(Value, FfiRequestedProduct)> {
    let runtime = || FfiRuntimeProductRequest {
        inline: Some(true),
        runtime_source_map: true,
        style_processing: None,
    };
    vec![
        (
            json!({ "kind": "runtimeClient", "inline": true, "runtimeSourceMap": true }),
            FfiRequestedProduct::RuntimeClient(runtime()),
        ),
        (
            json!({ "kind": "runtimeServer", "inline": true, "runtimeSourceMap": true }),
            FfiRequestedProduct::RuntimeServer(runtime()),
        ),
        (
            json!({
                "kind": "ideCompanion",
                "wantSourceMap": true,
                "embedAmbientTypes": true,
                "conditionalRootNarrowing": true,
                "strictSlots": true,
                "typesModuleName": "verter/types",
                "ideChunkBoundaries": true,
            }),
            FfiRequestedProduct::IdeCompanion(FfiIdeProductRequest {
                want_source_map: true,
                embed_ambient_types: true,
                conditional_root_narrowing: true,
                strict_slots: true,
                types_module_name: Some("verter/types".to_string()),
                ide_chunk_boundaries: true,
            }),
        ),
        (
            json!({ "kind": "publicApi" }),
            FfiRequestedProduct::PublicApi,
        ),
        (
            json!({ "kind": "declarations" }),
            FfiRequestedProduct::Declarations,
        ),
        (
            analysis_product_json(),
            FfiRequestedProduct::Analysis(FfiAnalysisProductRequest {
                want_script_bindings: true,
                want_template_data: true,
            }),
        ),
    ]
}

#[test]
fn every_product_tag_converts_to_its_own_ffi_product() {
    for (wire, expected) in product_rows() {
        let converted = only_product(vue_request_json(json!([wire.clone()]), vue_options_json()));
        assert_eq!(converted, expected, "product {wire} converted wrongly");
    }
}

#[test]
fn the_full_product_set_converts_in_request_order() {
    let rows = product_rows();
    let wire: Vec<Value> = rows.iter().map(|(wire, _)| wire.clone()).collect();
    let expected: Vec<FfiRequestedProduct> = rows.iter().map(|(_, ffi)| ffi.clone()).collect();

    let FfiHostCompileRequest::Vue(request) =
        convert(vue_request_json(json!(wire), vue_options_json()))
    else {
        panic!("vue arm");
    };
    assert_eq!(request.products, expected);
}

#[test]
fn every_product_flag_converts_in_both_polarities() {
    // Each row: the product tag, the flag key, and the FFI flag reader.
    type Read = fn(&FfiRequestedProduct) -> bool;
    let runtime_base = json!({ "kind": "runtimeClient", "runtimeSourceMap": false });
    let ide_base = json!({
        "kind": "ideCompanion",
        "wantSourceMap": false,
        "embedAmbientTypes": false,
        "conditionalRootNarrowing": false,
        "strictSlots": false,
        "ideChunkBoundaries": false,
    });
    let analysis_base = json!({
        "kind": "analysis",
        "wantScriptBindings": false,
        "wantTemplateData": false,
    });

    let rows: Vec<(&Value, &str, Read)> = vec![
        (&runtime_base, "inline", |p| match p {
            FfiRequestedProduct::RuntimeClient(r) => r.inline == Some(true),
            other => panic!("expected a runtime client product, got {other:?}"),
        }),
        (&runtime_base, "runtimeSourceMap", |p| match p {
            FfiRequestedProduct::RuntimeClient(r) => r.runtime_source_map,
            other => panic!("expected a runtime client product, got {other:?}"),
        }),
        (&ide_base, "wantSourceMap", |p| match p {
            FfiRequestedProduct::IdeCompanion(r) => r.want_source_map,
            other => panic!("expected an ide product, got {other:?}"),
        }),
        (&ide_base, "embedAmbientTypes", |p| match p {
            FfiRequestedProduct::IdeCompanion(r) => r.embed_ambient_types,
            other => panic!("expected an ide product, got {other:?}"),
        }),
        (&ide_base, "conditionalRootNarrowing", |p| match p {
            FfiRequestedProduct::IdeCompanion(r) => r.conditional_root_narrowing,
            other => panic!("expected an ide product, got {other:?}"),
        }),
        (&ide_base, "strictSlots", |p| match p {
            FfiRequestedProduct::IdeCompanion(r) => r.strict_slots,
            other => panic!("expected an ide product, got {other:?}"),
        }),
        (&ide_base, "ideChunkBoundaries", |p| match p {
            FfiRequestedProduct::IdeCompanion(r) => r.ide_chunk_boundaries,
            other => panic!("expected an ide product, got {other:?}"),
        }),
        (&analysis_base, "wantScriptBindings", |p| match p {
            FfiRequestedProduct::Analysis(r) => r.want_script_bindings,
            other => panic!("expected an analysis product, got {other:?}"),
        }),
        (&analysis_base, "wantTemplateData", |p| match p {
            FfiRequestedProduct::Analysis(r) => r.want_template_data,
            other => panic!("expected an analysis product, got {other:?}"),
        }),
    ];

    for (base, key, read) in rows {
        for supplied in [false, true] {
            let mut wire = base.clone();
            wire[key] = json!(supplied);
            let product = only_product(vue_request_json(json!([wire]), vue_options_json()));
            assert_eq!(
                read(&product),
                supplied,
                "product flag `{key}` must cross as supplied, not as a constant"
            );
        }
    }
}

#[test]
fn an_absent_optional_product_slot_stays_absent() {
    let product = only_product(vue_request_json(
        json!([{ "kind": "runtimeClient", "runtimeSourceMap": true }]),
        vue_options_json(),
    ));
    let FfiRequestedProduct::RuntimeClient(runtime) = product else {
        panic!("runtime client product");
    };
    assert_eq!(
        runtime.inline, None,
        "an omitted `inline` must stay absent so the canonical request \
         performs its own production derivation"
    );

    let product = only_product(vue_request_json(
        json!([{
            "kind": "ideCompanion",
            "wantSourceMap": false,
            "embedAmbientTypes": false,
            "conditionalRootNarrowing": false,
            "strictSlots": false,
            "ideChunkBoundaries": false,
        }]),
        vue_options_json(),
    ));
    let FfiRequestedProduct::IdeCompanion(ide) = product else {
        panic!("ide product");
    };
    assert_eq!(ide.types_module_name, None);
}

// ── framework options cross unchanged ────────────────────────────────────

#[test]
fn a_populated_vue_option_set_converts_field_for_field() {
    let FfiHostCompileRequest::Vue(request) = convert(vue_request_json(
        json!([analysis_product_json()]),
        every_vue_option_slot_json(),
    )) else {
        panic!("vue arm");
    };
    assert_eq!(request.options, every_vue_option_slot());
}

#[test]
fn the_bare_string_asset_url_arm_decodes() {
    // `transformAssetUrls` is the only option on either framework's wire
    // whose value may be a bare string: its off arm carries no payload, so
    // external tagging spells it `"disabled"` rather than as an object.
    // Every other closed vocabulary here is an all-unit enum already
    // covered by the spelling refusals, and every other option object is a
    // struct.
    let mut options = every_vue_option_slot_json();
    options["transformAssetUrls"] = json!("disabled");

    let FfiHostCompileRequest::Vue(request) =
        convert(vue_request_json(json!([analysis_product_json()]), options))
    else {
        panic!("vue arm");
    };
    assert_eq!(
        request.options.transform_asset_urls,
        Some(FfiVueAssetUrlTransform::Disabled),
        "the bare-string off arm must decode, not be read as an unknown \
         variant or a missing payload"
    );
}

#[test]
fn a_populated_svelte_option_set_converts_field_for_field() {
    let FfiHostCompileRequest::Svelte(request) = convert(svelte_request_json(
        json!([analysis_product_json()]),
        every_svelte_option_slot_json(),
    )) else {
        panic!("svelte arm");
    };
    assert_eq!(request.options, every_svelte_option_slot());
}

// ── the adapter feeds the canonical request, and owns no rule of its own ─

#[test]
fn a_converted_request_is_accepted_by_the_canonical_request_constructor() {
    let converted = convert(vue_request_json(
        json!([{ "kind": "runtimeClient", "runtimeSourceMap": false }]),
        vue_options_json(),
    ));
    let request = ffi_host_compile_request_to_compile_request(converted, &no_profiles())
        .expect("a well-formed vue request reaches the canonical constructor");
    assert!(matches!(
        request.framework(),
        FrameworkCompileRequest::Vue(_)
    ));
    assert!(request
        .products()
        .iter()
        .any(|product| matches!(product, CompileProduct::RuntimeClient(_))));
}

#[test]
fn product_set_legality_stays_the_canonical_requests_rule() {
    // An empty product set is refused by the canonical constructor, NOT by
    // the adapter: the adapter must decode it without complaint, or it has
    // become a second request authority.
    let converted = convert(vue_request_json(json!([]), vue_options_json()));
    let FfiHostCompileRequest::Vue(ref request) = converted else {
        panic!("vue arm");
    };
    assert!(request.products.is_empty());

    let error = ffi_host_compile_request_to_compile_request(converted, &no_profiles())
        .expect_err("an empty product set is refused downstream");
    assert!(
        matches!(error, CompileRequestError::EmptyProductSet),
        "expected the canonical empty-product-set refusal, got {error:?}"
    );
}

// ── refusals: unknown keys at every nesting level ────────────────────────

#[test]
fn an_unknown_top_level_key_is_refused() {
    let mut request = vue_request_json(json!([analysis_product_json()]), vue_options_json());
    request["bogus"] = json!(1);
    let message = refusal(request);
    assert!(
        message.contains("unknown field `bogus`"),
        "expected an unknown-field refusal naming `bogus`, got: {message}"
    );
}

#[test]
fn an_unknown_identity_key_is_refused() {
    let mut request = vue_request_json(json!([analysis_product_json()]), vue_options_json());
    request["identity"]["sourceMap"] = json!(true);
    let message = refusal(request);
    assert!(
        message.contains("unknown field `sourceMap`"),
        "expected an unknown-field refusal naming `sourceMap`, got: {message}"
    );
}

#[test]
fn an_unknown_option_key_is_refused() {
    let mut options = vue_options_json();
    options["hoistStatick"] = json!(true);
    let message = refusal(vue_request_json(json!([analysis_product_json()]), options));
    assert!(
        message.contains("unknown field `hoistStatick`"),
        "expected an unknown-field refusal naming `hoistStatick`, got: {message}"
    );
}

#[test]
fn an_unknown_key_in_a_nested_option_object_is_refused() {
    let mut options = vue_options_json();
    options["cssModules"] = json!({ "scopeBehaviour": "local", "bogus": true });
    let message = refusal(vue_request_json(json!([analysis_product_json()]), options));
    assert!(
        message.contains("unknown field `bogus`"),
        "expected a nested unknown-field refusal naming `bogus`, got: {message}"
    );

    let mut options = vue_options_json();
    options["transformAssetUrls"] = json!({ "enabled": { "tags": {}, "bogus": true } });
    let message = refusal(vue_request_json(json!([analysis_product_json()]), options));
    assert!(
        message.contains("unknown field `bogus`"),
        "expected a nested unknown-field refusal naming `bogus`, got: {message}"
    );

    // The Svelte nested objects, each of which is reached only through an
    // `Option` slot: closedness there rests on the option type's own
    // `deny_unknown_fields`, not on the request's.
    let svelte_nested: Vec<(Value, &str)> = vec![
        (
            json!({ "customElementDescriptor": { "props": { "v": { "bogus": true } } } }),
            "bogus",
        ),
        (
            json!({ "customElementDescriptor": { "props": {}, "bogus": true } }),
            "bogus",
        ),
        // `compatibility` declares no slot at all: its one inventoried
        // field is refused, so naming it is an unknown key rather than a
        // supplied-but-unsupported option.
        (
            json!({ "compatibility": { "componentApi": true } }),
            "componentApi",
        ),
    ];
    for (options, foreign) in svelte_nested {
        let expected = format!("unknown field `{foreign}`");
        let message = refusal(svelte_request_json(
            json!([analysis_product_json()]),
            options,
        ));
        assert!(
            message.contains(&expected),
            "expected a nested refusal containing `{expected}`, got: {message}"
        );
    }
}

#[test]
fn an_unknown_product_key_is_refused() {
    let message = refusal(vue_request_json(
        json!([{ "kind": "runtimeClient", "runtimeSourceMap": true, "bogus": 1 }]),
        vue_options_json(),
    ));
    assert!(
        message.contains("unknown field `bogus`"),
        "expected an unknown-field refusal naming `bogus`, got: {message}"
    );
}

#[test]
fn a_product_option_belonging_to_another_product_is_refused() {
    // Both directions, because each product payload is its own closed
    // type: the request's `deny_unknown_fields` does not reach inside a
    // payload, so a payload that stopped being closed would still pass a
    // one-directional check.
    let rows: Vec<(Value, &str)> = vec![
        (
            json!({ "kind": "runtimeClient", "runtimeSourceMap": true, "strictSlots": true }),
            "strictSlots",
        ),
        (
            json!({
                "kind": "ideCompanion",
                "wantSourceMap": false,
                "embedAmbientTypes": false,
                "conditionalRootNarrowing": false,
                "strictSlots": false,
                "ideChunkBoundaries": false,
                "inline": true,
            }),
            "inline",
        ),
        (
            json!({
                "kind": "analysis",
                "wantScriptBindings": false,
                "wantTemplateData": false,
                "wantSourceMap": true,
            }),
            "wantSourceMap",
        ),
    ];

    for (product, foreign) in rows {
        let message = refusal(vue_request_json(json!([product]), vue_options_json()));
        assert!(
            message.contains(&format!("unknown field `{foreign}`")),
            "expected an unknown-field refusal naming `{foreign}`, got: {message}"
        );
    }
}

#[test]
fn an_option_slot_on_a_product_that_carries_none_is_refused() {
    let message = refusal(vue_request_json(
        json!([{ "kind": "publicApi", "inline": true }]),
        vue_options_json(),
    ));
    assert!(
        message.contains("unknown field `inline`"),
        "expected an unknown-field refusal naming `inline`, got: {message}"
    );
}

// ── refusals: cross-framework and unknown discriminants ──────────────────

#[test]
fn a_svelte_option_inside_the_vue_arm_is_refused() {
    let mut options = vue_options_json();
    options["runes"] = json!("infer");
    let message = refusal(vue_request_json(json!([analysis_product_json()]), options));
    assert!(
        message.contains("unknown field `runes`"),
        "expected a cross-framework refusal naming `runes`, got: {message}"
    );
}

#[test]
fn a_vue_option_inside_the_svelte_arm_is_refused() {
    let message = refusal(svelte_request_json(
        json!([analysis_product_json()]),
        json!({ "backend": "vdom" }),
    ));
    assert!(
        message.contains("unknown field `backend`"),
        "expected a cross-framework refusal naming `backend`, got: {message}"
    );
}

#[test]
fn an_unknown_framework_is_refused() {
    let mut request = vue_request_json(json!([analysis_product_json()]), vue_options_json());
    request["framework"] = json!("angular");
    let message = refusal(request);
    assert!(
        message.contains("unknown variant `angular`"),
        "expected an unknown-variant refusal naming `angular`, got: {message}"
    );
}

#[test]
fn a_missing_framework_tag_is_refused() {
    let mut request = vue_request_json(json!([analysis_product_json()]), vue_options_json());
    request.as_object_mut().expect("object").remove("framework");
    let message = refusal(request);
    assert!(
        message.contains("missing field `framework`"),
        "expected a missing-tag refusal naming `framework`, got: {message}"
    );
}

#[test]
fn an_unknown_product_tag_is_refused() {
    let message = refusal(vue_request_json(
        json!([{ "kind": "bundle" }]),
        vue_options_json(),
    ));
    assert!(
        message.contains("unknown variant `bundle`"),
        "expected an unknown-variant refusal naming `bundle`, got: {message}"
    );
}

#[test]
fn a_missing_product_tag_is_refused() {
    let message = refusal(vue_request_json(
        json!([{ "runtimeSourceMap": true }]),
        vue_options_json(),
    ));
    assert!(
        message.contains("missing field `kind`"),
        "expected a missing-tag refusal naming `kind`, got: {message}"
    );
}

// ── refusals: missing required slots and malformed values ────────────────

#[test]
fn a_missing_required_option_is_refused() {
    for missing in ["backend", "ssr", "isCustomElement", "babelParserPlugins"] {
        let mut options = vue_options_json();
        options.as_object_mut().expect("object").remove(missing);
        let message = refusal(vue_request_json(json!([analysis_product_json()]), options));
        assert!(
            message.contains(&format!("missing field `{missing}`")),
            "expected a missing-field refusal naming `{missing}`, got: {message}"
        );
    }
}

#[test]
fn a_missing_required_identity_or_product_slot_is_refused() {
    let mut request = vue_request_json(json!([analysis_product_json()]), vue_options_json());
    request["identity"]
        .as_object_mut()
        .expect("object")
        .remove("isProduction");
    let message = refusal(request);
    assert!(
        message.contains("missing field `isProduction`"),
        "expected a missing-field refusal naming `isProduction`, got: {message}"
    );

    let message = refusal(vue_request_json(
        json!([{ "kind": "runtimeClient" }]),
        vue_options_json(),
    ));
    assert!(
        message.contains("missing field `runtimeSourceMap`"),
        "expected a missing-field refusal naming `runtimeSourceMap`, got: {message}"
    );
}

#[test]
fn a_missing_top_level_section_is_refused() {
    for missing in ["identity", "products", "options"] {
        let mut request = vue_request_json(json!([analysis_product_json()]), vue_options_json());
        request.as_object_mut().expect("object").remove(missing);
        let message = refusal(request);
        assert!(
            message.contains(&format!("missing field `{missing}`")),
            "expected a missing-field refusal naming `{missing}`, got: {message}"
        );
    }
}

#[test]
fn a_value_of_the_wrong_type_is_refused_rather_than_coerced() {
    let mut options = vue_options_json();
    options["ssr"] = json!("yes");
    let message = refusal(vue_request_json(json!([analysis_product_json()]), options));
    // serde names the rule and both types; the offending slot is `ssr`, the
    // only boolean the fixture perturbs.
    assert!(
        message.contains("invalid type: string \"yes\"") && message.contains("expected a boolean"),
        "expected an invalid-type refusal, got: {message}"
    );

    let mut request = vue_request_json(json!([analysis_product_json()]), vue_options_json());
    request["products"] = json!("analysis");
    let message = refusal(request);
    assert!(
        message.contains("invalid type: string \"analysis\"")
            && message.contains("expected a sequence"),
        "expected an invalid-type refusal for a non-array product set, got: {message}"
    );

    let message = refusal(json!("vue"));
    assert!(
        message.contains("invalid type: string \"vue\""),
        "expected an invalid-type refusal for a non-object request, got: {message}"
    );
}

#[test]
fn a_spelling_outside_a_closed_vocabulary_is_refused() {
    let cases: Vec<(&str, Value, &str)> = vec![
        ("backend", json!("runtime"), "runtime"),
        ("whitespace", json!("trim"), "trim"),
        ("parsePad", json!("tab"), "tab"),
    ];
    for (key, value, spelling) in cases {
        let mut options = vue_options_json();
        options[key] = value;
        let message = refusal(vue_request_json(json!([analysis_product_json()]), options));
        assert!(
            message.contains(&format!("unknown variant `{spelling}`")),
            "expected an unknown-variant refusal naming `{spelling}`, got: {message}"
        );
    }

    let svelte_cases: Vec<(&str, Value, &str)> = vec![
        ("namespace", json!("xml"), "xml"),
        ("css", json!("inline"), "inline"),
        ("runes", json!("maybe"), "maybe"),
        ("fragments", json!("dom"), "dom"),
    ];
    for (key, value, spelling) in svelte_cases {
        let message = refusal(svelte_request_json(
            json!([analysis_product_json()]),
            json!({ key: value }),
        ));
        assert!(
            message.contains(&format!("unknown variant `{spelling}`")),
            "expected an unknown-variant refusal naming `{spelling}`, got: {message}"
        );
    }

    // The custom-element prop-type vocabulary is NOT a decode-boundary
    // vocabulary: the wire carries the caller's spelling verbatim and the
    // canonical request constructor is the one place that decides
    // membership, so an unrecognised spelling decodes here and is refused
    // one stage later, naming the option row and the offending value.
    let request = decode(svelte_request_json(
        json!([analysis_product_json()]),
        json!({ "customElementDescriptor": { "props": { "v": { "propType": "symbol" } } } }),
    ));
    match ffi_host_compile_request_to_compile_request(
        napi_host_compile_request_to_ffi(request),
        &HostResolvedCompileProfiles {
            semantic: None,
            output: None,
            presentation: None,
            serialization: None,
        },
    )
    .expect_err("an unrecognised prop type is refused at canonical admission")
    {
        CompileRequestError::MalformedOptionValue { value, .. } => {
            assert_eq!(value, "symbol", "the offending spelling is preserved")
        }
        other => panic!("expected MalformedOptionValue naming `symbol`, got {other:?}"),
    }
}
