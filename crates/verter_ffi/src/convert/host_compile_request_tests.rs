//! Conversion fixtures for the framework-discriminated host compile
//! request: both frameworks, every product kind, unknown wire fields,
//! cross-framework fields, malformed values, and capability refusals.

use verter_compiler::compile_request::svelte::{
    SvelteCompileRequest, SvelteCssRequest, SvelteCustomElementPropType, SvelteFragmentsRequest,
    SvelteNamespaceRequest, SvelteRunesRequest,
};
use verter_compiler::compile_request::vue::{
    VueAssetUrlTransform, VueCompileRequest, VueCssModuleLocalsConvention,
    VueCssModuleScopeBehaviour, VueParsePad, VueWhitespaceStrategy,
};
use verter_compiler::compile_request::{
    CompileProduct, CompileRequest, CompileRequestError, FrameworkCompileRequest, FrameworkOption,
    ProductKind, SvelteOption, VueBackendRequest, VueOption,
};

use crate::convert::{ffi_host_compile_request_to_compile_request, HostResolvedCompileProfiles};
use crate::types::*;

// ── fixtures ─────────────────────────────────────────────────────────────

fn no_profiles() -> HostResolvedCompileProfiles {
    HostResolvedCompileProfiles {
        semantic: None,
        output: None,
        presentation: None,
        serialization: None,
    }
}

fn identity() -> FfiHostCompileIdentity {
    FfiHostCompileIdentity {
        filename: Some("Comp.vue".to_string()),
        component_id: Some("c-1".to_string()),
        is_production: false,
        force_js: false,
    }
}

/// Every Vue slot absent except the four required ones — the baseline a
/// single-option fixture perturbs.
fn vue_options() -> FfiVueCompileOptions {
    FfiVueCompileOptions {
        backend: FfiVueBackend::Inferred,
        ssr: false,
        is_custom_element: Vec::new(),
        delimiters: None,
        whitespace: None,
        comments: None,
        hoist_static: None,
        cache_handlers: None,
        hmr: None,
        optimize_imports: None,
        runtime_module_name: None,
        ssr_runtime_module_name: None,
        parse_pad: None,
        ignore_empty: None,
        babel_parser_plugins: Vec::new(),
        gen_default_as: None,
        props_destructure: None,
        script_custom_element: None,
        transform_asset_urls: None,
        style_trim: None,
        css_modules: None,
        compat_config: None,
        compat_config_mode: None,
        compat_config_compiler_is_on_element: None,
        compat_config_compiler_v_bind_sync: None,
        compat_config_compiler_v_if_v_for_precedence: None,
        compat_config_compiler_v_bind_object_order: None,
        compat_config_compiler_v_on_native: None,
        compat_config_compiler_native_template: None,
        compat_config_compiler_inline_template: None,
        compat_config_compiler_filters: None,
        transform_compat_config: None,
        codegen_mode: None,
    }
}

fn svelte_options() -> FfiSvelteCompileOptions {
    FfiSvelteCompileOptions {
        dev: None,
        generate_module: None,
        experimental_async: None,
        custom_element: None,
        custom_element_descriptor: None,
        namespace: None,
        css: None,
        preserve_comments: None,
        preserve_whitespace: None,
        fragments: None,
        runes: None,
        disclose_version: None,
        compatibility: None,
        loose: None,
        accessors: None,
        immutable: None,
        compatibility_component_api: None,
        hmr: None,
        custom_element_extend: None,
    }
}

fn runtime_product() -> FfiRuntimeProductRequest {
    FfiRuntimeProductRequest {
        inline: None,
        runtime_source_map: false,
    }
}

fn analysis_product() -> FfiAnalysisProductRequest {
    FfiAnalysisProductRequest {
        want_script_bindings: true,
        want_template_data: false,
    }
}

fn ide_product() -> FfiIdeProductRequest {
    FfiIdeProductRequest {
        want_source_map: false,
        embed_ambient_types: false,
        conditional_root_narrowing: false,
        strict_slots: false,
        types_module_name: None,
        ide_chunk_boundaries: false,
    }
}

fn vue(products: Vec<FfiRequestedProduct>, options: FfiVueCompileOptions) -> FfiHostCompileRequest {
    FfiHostCompileRequest::Vue(FfiVueHostCompileRequest {
        identity: identity(),
        products,
        options,
    })
}

fn svelte(
    products: Vec<FfiRequestedProduct>,
    options: FfiSvelteCompileOptions,
) -> FfiHostCompileRequest {
    FfiHostCompileRequest::Svelte(FfiSvelteHostCompileRequest {
        identity: identity(),
        products,
        options,
    })
}

// ── both frameworks reach the canonical request ──────────────────────────

#[test]
fn vue_request_converts_to_exactly_one_canonical_vue_request() {
    let request = ffi_host_compile_request_to_compile_request(
        vue(
            vec![FfiRequestedProduct::RuntimeClient(runtime_product())],
            vue_options(),
        ),
        &no_profiles(),
    )
    .expect("a minimal Vue request converts");

    assert!(matches!(
        request.framework(),
        FrameworkCompileRequest::Vue(_)
    ));
    assert!(
        request.svelte().is_none(),
        "a Vue request has no Svelte arm"
    );
    assert_eq!(request.filename(), Some("Comp.vue"));
    assert_eq!(request.component_id(), Some("c-1"));
    assert!(!request.is_production());
    assert!(!request.force_js());
    assert_eq!(request.products().len(), 1);
}

#[test]
fn svelte_request_converts_to_exactly_one_canonical_svelte_request() {
    let request = ffi_host_compile_request_to_compile_request(
        svelte(
            vec![FfiRequestedProduct::RuntimeClient(runtime_product())],
            svelte_options(),
        ),
        &no_profiles(),
    )
    .expect("a minimal Svelte request converts");

    assert!(matches!(
        request.framework(),
        FrameworkCompileRequest::Svelte(_)
    ));
    assert!(request.vue().is_none(), "a Svelte request has no Vue arm");
}

// ── every product kind ───────────────────────────────────────────────────

/// EXHAUSTIVE over the canonical product vocabulary: adding a
/// [`ProductKind`] variant without a wire representation is a COMPILE
/// ERROR here, not a silently unconverted kind.
fn wire_product_for(kind: ProductKind) -> FfiRequestedProduct {
    match kind {
        ProductKind::RuntimeClient => FfiRequestedProduct::RuntimeClient(runtime_product()),
        ProductKind::RuntimeServer => FfiRequestedProduct::RuntimeServer(runtime_product()),
        ProductKind::IdeCompanion => FfiRequestedProduct::IdeCompanion(ide_product()),
        ProductKind::PublicApi => FfiRequestedProduct::PublicApi,
        ProductKind::Declarations => FfiRequestedProduct::Declarations,
        ProductKind::Analysis => FfiRequestedProduct::Analysis(analysis_product()),
    }
}

const ALL_PRODUCT_KINDS: [ProductKind; 6] = [
    ProductKind::RuntimeClient,
    ProductKind::RuntimeServer,
    ProductKind::IdeCompanion,
    ProductKind::PublicApi,
    ProductKind::Declarations,
    ProductKind::Analysis,
];

#[test]
fn every_product_kind_converts_to_its_own_canonical_product() {
    for kind in ALL_PRODUCT_KINDS {
        // `RuntimeServer` alone is the only kind that changes the Vue
        // backend legality, so it is requested with the VDOM backend.
        let mut options = vue_options();
        options.backend = FfiVueBackend::Vdom;
        let request = ffi_host_compile_request_to_compile_request(
            vue(vec![wire_product_for(kind)], options),
            &no_profiles(),
        )
        .unwrap_or_else(|e| panic!("{kind:?} must convert, got {e:?}"));
        assert_eq!(request.products().len(), 1);
        assert_eq!(request.products()[0].kind(), kind);
    }
}

#[test]
fn the_full_product_set_converts_as_one_request() {
    let mut options = vue_options();
    options.backend = FfiVueBackend::Vdom;
    let request = ffi_host_compile_request_to_compile_request(
        vue(
            ALL_PRODUCT_KINDS
                .into_iter()
                .map(wire_product_for)
                .collect(),
            options,
        ),
        &no_profiles(),
    )
    .expect("the whole product vocabulary converts as one request");
    let kinds: Vec<ProductKind> = request.products().iter().map(|p| p.kind()).collect();
    assert_eq!(kinds, ALL_PRODUCT_KINDS.to_vec());

    // Closes the exhaustiveness loop on [`ALL_PRODUCT_KINDS`]: the width of
    // `reached` is fixed by an exhaustive match over the canonical product
    // vocabulary, so a variant added there forces this array to widen and
    // then FAILS until the wire fixture list covers it too.
    let mut reached = [false; 6];
    for product in request.products() {
        reached[match product {
            CompileProduct::RuntimeClient(_) => 0,
            CompileProduct::RuntimeServer(_) => 1,
            CompileProduct::IdeCompanion(_) => 2,
            CompileProduct::PublicApi(_) => 3,
            CompileProduct::Declarations(_) => 4,
            CompileProduct::Analysis(_) => 5,
        }] = true;
    }
    assert!(
        reached.iter().all(|hit| *hit),
        "every canonical product variant must be reachable from the wire schema: {reached:?}"
    );
}

#[test]
fn per_product_options_survive_the_conversion() {
    let request = ffi_host_compile_request_to_compile_request(
        vue(
            vec![
                FfiRequestedProduct::RuntimeClient(FfiRuntimeProductRequest {
                    inline: Some(false),
                    runtime_source_map: true,
                }),
                FfiRequestedProduct::IdeCompanion(FfiIdeProductRequest {
                    want_source_map: false,
                    embed_ambient_types: true,
                    conditional_root_narrowing: true,
                    strict_slots: true,
                    types_module_name: Some("@custom/types".to_string()),
                    ide_chunk_boundaries: true,
                }),
                FfiRequestedProduct::Analysis(FfiAnalysisProductRequest {
                    want_script_bindings: false,
                    want_template_data: true,
                }),
            ],
            vue_options(),
        ),
        &no_profiles(),
    )
    .expect("converts");

    assert!(
        request.wants_runtime_source_map(),
        "the runtime product's own map demand survives"
    );
    assert!(
        !request.wants_ide_source_map(),
        "the IDE product's map demand is independent and stays off"
    );
    assert_eq!(request.resolve_inline(), Some(false));
    assert!(request.wants_template_data());

    match &request.products()[1] {
        CompileProduct::IdeCompanion(ide) => {
            assert!(ide.embed_ambient_types);
            assert!(ide.conditional_root_narrowing);
            assert!(ide.strict_slots);
            assert!(ide.ide_chunk_boundaries);
            assert_eq!(ide.types_module_name.as_deref(), Some("@custom/types"));
        }
        other => panic!("expected the IDE companion product, got {other:?}"),
    }
}

#[test]
fn inline_absent_resolves_through_the_canonical_production_derivation() {
    // `inline: None` is not substituted at the boundary: the canonical
    // request derives it from `isProduction`, so the same wire payload
    // resolves differently for a production identity.
    let mut prod_identity = identity();
    prod_identity.is_production = true;
    let request = ffi_host_compile_request_to_compile_request(
        FfiHostCompileRequest::Vue(FfiVueHostCompileRequest {
            identity: prod_identity,
            products: vec![FfiRequestedProduct::RuntimeClient(runtime_product())],
            options: vue_options(),
        }),
        &no_profiles(),
    )
    .expect("converts");
    assert_eq!(request.resolve_inline(), Some(true));
}

/// Distinct seeds so a slot wired to the wrong profile class (e.g. the
/// presentation id landing on an output slot) fails rather than passing by
/// coincidence.
struct ProfileSeed(&'static str);

impl verter_identity::encoding::CanonicalEncode for ProfileSeed {
    const DOMAIN_TAG: &'static str = "verter-ffi.compile-request-profile-seed.v1";
    fn encode_fields(&self, e: &mut verter_identity::encoding::CanonicalEncoder) {
        e.field_str(1, self.0);
    }
}

#[test]
fn host_resolved_profiles_reach_every_canonical_slot_that_carries_one() {
    use verter_identity::profile::{
        OutputProfileId, PresentationProfileId, SerializationProfileId, TypeScriptSemanticProfileId,
    };

    let semantic = TypeScriptSemanticProfileId::from_canonical(&ProfileSeed("semantic"));
    let output = OutputProfileId::from_canonical(&ProfileSeed("output"));
    let presentation = PresentationProfileId::from_canonical(&ProfileSeed("presentation"));
    let serialization = SerializationProfileId::from_canonical(&ProfileSeed("serialization"));
    let profiles = HostResolvedCompileProfiles {
        semantic: Some(semantic.clone()),
        output: Some(output.clone()),
        presentation: Some(presentation.clone()),
        serialization: Some(serialization.clone()),
    };

    let mut options = vue_options();
    options.backend = FfiVueBackend::Vdom;
    let request = ffi_host_compile_request_to_compile_request(
        vue(
            ALL_PRODUCT_KINDS
                .into_iter()
                .map(wire_product_for)
                .collect(),
            options,
        ),
        &profiles,
    )
    .expect("converts");

    assert_eq!(request.semantic_profile(), Some(&semantic));
    for product in request.products() {
        match product {
            CompileProduct::RuntimeClient(p) | CompileProduct::RuntimeServer(p) => {
                assert_eq!(p.output_profile.as_ref(), Some(&output));
                assert_eq!(p.serialization.as_ref(), Some(&serialization));
            }
            CompileProduct::IdeCompanion(p) => {
                assert_eq!(p.output_profile.as_ref(), Some(&output));
                assert_eq!(p.diagnostics.as_ref(), Some(&presentation));
                assert_eq!(p.serialization.as_ref(), Some(&serialization));
            }
            CompileProduct::PublicApi(p) => {
                assert_eq!(p.output_profile.as_ref(), Some(&output));
                assert_eq!(p.serialization.as_ref(), Some(&serialization));
            }
            CompileProduct::Declarations(p) => {
                assert_eq!(p.output_profile.as_ref(), Some(&output));
                assert_eq!(p.serialization.as_ref(), Some(&serialization));
            }
            // The analysis product carries no profile slot at all.
            CompileProduct::Analysis(_) => {}
        }
    }
}

// ── every supported option survives ──────────────────────────────────────

#[test]
fn every_supported_vue_option_reaches_the_canonical_request() {
    let mut options = vue_options();
    options.backend = FfiVueBackend::Vapor;
    options.ssr = false;
    options.is_custom_element = vec!["my-".to_string(), "ion-".to_string()];
    options.delimiters = Some(vec!["[[".to_string(), "]]".to_string()]);
    options.whitespace = Some(FfiVueWhitespace::Preserve);
    options.comments = Some(true);
    options.hoist_static = Some(false);
    options.cache_handlers = Some(true);
    options.hmr = Some(true);
    options.optimize_imports = Some(true);
    options.runtime_module_name = Some("custom-vue".to_string());
    options.ssr_runtime_module_name = Some("custom-vue-server".to_string());
    options.parse_pad = Some(FfiVueParsePad::Line);
    options.ignore_empty = Some(true);
    options.babel_parser_plugins = vec!["jsx".to_string()];
    options.gen_default_as = Some("__default__".to_string());
    options.props_destructure = Some(true);
    options.script_custom_element = Some(true);
    options.transform_asset_urls = Some(FfiVueAssetUrlTransform::Enabled(FfiVueAssetUrlOptions {
        base: Some("/assets".to_string()),
        include_absolute: Some(true),
        tags: [("img".to_string(), vec!["src".to_string()])]
            .into_iter()
            .collect(),
    }));
    options.style_trim = Some(true);
    options.css_modules = Some(FfiVueCssModules {
        scope_behaviour: Some(FfiVueCssModuleScopeBehaviour::Global),
        hash_prefix: Some("prefix".to_string()),
        locals_convention: Some(FfiVueCssModuleLocalsConvention::DashesOnly),
        export_globals: Some(true),
    });

    let request = ffi_host_compile_request_to_compile_request(
        vue(
            vec![FfiRequestedProduct::RuntimeClient(runtime_product())],
            options,
        ),
        &no_profiles(),
    )
    .expect("converts");
    let v = request.vue().expect("Vue arm");

    assert_eq!(v.backend, VueBackendRequest::Vapor);
    assert!(!v.ssr);
    assert_eq!(
        v.is_custom_element,
        vec!["my-".to_string(), "ion-".to_string()]
    );
    assert_eq!(v.delimiters, Some(("[[".to_string(), "]]".to_string())));
    assert_eq!(v.whitespace, Some(VueWhitespaceStrategy::Preserve));
    assert_eq!(v.comments, Some(true));
    assert_eq!(v.hoist_static, Some(false));
    assert_eq!(v.cache_handlers, Some(true));
    assert_eq!(v.hmr, Some(true));
    assert_eq!(v.optimize_imports, Some(true));
    assert_eq!(v.runtime_module_name.as_deref(), Some("custom-vue"));
    assert_eq!(
        v.ssr_runtime_module_name.as_deref(),
        Some("custom-vue-server")
    );
    assert_eq!(v.parse_pad, Some(VueParsePad::Line));
    assert_eq!(v.ignore_empty, Some(true));
    assert_eq!(v.babel_parser_plugins, vec!["jsx".to_string()]);
    assert_eq!(v.gen_default_as.as_deref(), Some("__default__"));
    assert_eq!(v.props_destructure, Some(true));
    assert_eq!(v.script_custom_element, Some(true));
    match v.transform_asset_urls.as_ref().expect("asset urls") {
        VueAssetUrlTransform::Enabled(opts) => {
            assert_eq!(opts.base.as_deref(), Some("/assets"));
            assert_eq!(opts.include_absolute, Some(true));
            assert_eq!(opts.tags.get("img"), Some(&vec!["src".to_string()]));
        }
        other => panic!("expected an enabled asset-url transform, got {other:?}"),
    }
    assert_eq!(v.style_trim, Some(true));
    let css = v.css_modules.as_ref().expect("css modules");
    assert_eq!(
        css.scope_behaviour,
        Some(VueCssModuleScopeBehaviour::Global)
    );
    assert_eq!(css.hash_prefix.as_deref(), Some("prefix"));
    assert_eq!(
        css.locals_convention,
        Some(VueCssModuleLocalsConvention::DashesOnly)
    );
    assert_eq!(css.export_globals, Some(true));
}

#[test]
fn every_supported_svelte_option_reaches_the_canonical_request() {
    let mut options = svelte_options();
    options.dev = Some(true);
    options.custom_element = Some(true);
    options.custom_element_descriptor = Some(FfiSvelteCustomElementDescriptor {
        tag: Some("my-el".to_string()),
        shadow: Some(false),
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
    });
    options.namespace = Some(FfiSvelteNamespace::MathMl);
    options.css = Some(FfiSvelteCss::External);
    options.preserve_comments = Some(true);
    options.preserve_whitespace = Some(true);
    options.fragments = Some(FfiSvelteFragments::Tree);
    options.runes = Some(FfiSvelteRunes::Infer);
    options.disclose_version = Some(false);
    options.compatibility = Some(FfiSvelteCompatibility {});

    let request = ffi_host_compile_request_to_compile_request(
        svelte(
            vec![FfiRequestedProduct::RuntimeClient(runtime_product())],
            options,
        ),
        &no_profiles(),
    )
    .expect("converts");
    let s = request.svelte().expect("Svelte arm");

    assert_eq!(s.dev, Some(true));
    assert_eq!(s.custom_element, Some(true));
    let descriptor = s
        .custom_element_descriptor
        .as_ref()
        .expect("custom element descriptor");
    assert_eq!(descriptor.tag.as_deref(), Some("my-el"));
    assert_eq!(descriptor.shadow, Some(false));
    let prop = descriptor.props.get("value").expect("prop descriptor");
    assert_eq!(prop.attribute.as_deref(), Some("value"));
    assert_eq!(prop.reflect, Some(true));
    assert_eq!(prop.prop_type, Some(SvelteCustomElementPropType::Number));
    assert_eq!(s.namespace, Some(SvelteNamespaceRequest::MathMl));
    assert_eq!(s.css, Some(SvelteCssRequest::External));
    assert_eq!(s.preserve_comments, Some(true));
    assert_eq!(s.preserve_whitespace, Some(true));
    assert_eq!(s.fragments, Some(SvelteFragmentsRequest::Tree));
    assert_eq!(s.runes, Some(SvelteRunesRequest::Infer));
    assert_eq!(s.disclose_version, Some(false));
    assert!(s.compatibility.is_some());
}

// ── every pass-through discriminates, in both polarities ─────────────────
//
// A single-polarity fixture cannot tell a real pass-through from a
// hardcoded constant: replacing the field with whatever value the fixture
// happens to use would leave the suite green. So every boolean is asserted
// at BOTH `true` and `false` on its own canonical slot, every closed enum
// at EVERY variant, and every optional slot both supplied and absent.

fn convert_vue(options: FfiVueCompileOptions) -> CompileRequest {
    ffi_host_compile_request_to_compile_request(
        vue(
            vec![FfiRequestedProduct::RuntimeClient(runtime_product())],
            options,
        ),
        &no_profiles(),
    )
    .expect("the perturbed Vue fixture converts")
}

fn convert_svelte(options: FfiSvelteCompileOptions) -> CompileRequest {
    ffi_host_compile_request_to_compile_request(
        svelte(
            vec![FfiRequestedProduct::RuntimeClient(runtime_product())],
            options,
        ),
        &no_profiles(),
    )
    .expect("the perturbed Svelte fixture converts")
}

type SetVueBool = fn(&mut FfiVueCompileOptions, bool);
type ReadVueBool = fn(&VueCompileRequest) -> Option<bool>;

#[test]
fn every_vue_boolean_option_round_trips_in_both_polarities() {
    let cases: [(&str, SetVueBool, ReadVueBool); 12] = [
        ("ssr", |o, v| o.ssr = v, |r| Some(r.ssr)),
        ("comments", |o, v| o.comments = Some(v), |r| r.comments),
        (
            "hoistStatic",
            |o, v| o.hoist_static = Some(v),
            |r| r.hoist_static,
        ),
        (
            "cacheHandlers",
            |o, v| o.cache_handlers = Some(v),
            |r| r.cache_handlers,
        ),
        ("hmr", |o, v| o.hmr = Some(v), |r| r.hmr),
        (
            "optimizeImports",
            |o, v| o.optimize_imports = Some(v),
            |r| r.optimize_imports,
        ),
        (
            "ignoreEmpty",
            |o, v| o.ignore_empty = Some(v),
            |r| r.ignore_empty,
        ),
        (
            "propsDestructure",
            |o, v| o.props_destructure = Some(v),
            |r| r.props_destructure,
        ),
        (
            "scriptCustomElement",
            |o, v| o.script_custom_element = Some(v),
            |r| r.script_custom_element,
        ),
        ("styleTrim", |o, v| o.style_trim = Some(v), |r| r.style_trim),
        (
            "cssModules.exportGlobals",
            |o, v| {
                o.css_modules = Some(FfiVueCssModules {
                    scope_behaviour: None,
                    hash_prefix: None,
                    locals_convention: None,
                    export_globals: Some(v),
                })
            },
            |r| r.css_modules.as_ref().and_then(|m| m.export_globals),
        ),
        (
            "transformAssetUrls.includeAbsolute",
            |o, v| {
                o.transform_asset_urls =
                    Some(FfiVueAssetUrlTransform::Enabled(FfiVueAssetUrlOptions {
                        base: None,
                        include_absolute: Some(v),
                        tags: Default::default(),
                    }))
            },
            |r| match r.transform_asset_urls.as_ref() {
                Some(VueAssetUrlTransform::Enabled(a)) => a.include_absolute,
                _ => None,
            },
        ),
    ];

    for (name, set, read) in cases {
        for value in [true, false] {
            let mut options = vue_options();
            set(&mut options, value);
            let request = convert_vue(options);
            assert_eq!(
                read(request.vue().expect("Vue arm")),
                Some(value),
                "{name} must reach the canonical request as {value}"
            );
        }
    }
}

type SetSvelteBool = fn(&mut FfiSvelteCompileOptions, bool);
type ReadSvelteBool = fn(&SvelteCompileRequest) -> Option<bool>;

#[test]
fn every_svelte_boolean_option_round_trips_in_both_polarities() {
    let cases: [(&str, SetSvelteBool, ReadSvelteBool); 7] = [
        ("dev", |o, v| o.dev = Some(v), |r| r.dev),
        (
            "customElement",
            |o, v| o.custom_element = Some(v),
            |r| r.custom_element,
        ),
        (
            "preserveComments",
            |o, v| o.preserve_comments = Some(v),
            |r| r.preserve_comments,
        ),
        (
            "preserveWhitespace",
            |o, v| o.preserve_whitespace = Some(v),
            |r| r.preserve_whitespace,
        ),
        (
            "discloseVersion",
            |o, v| o.disclose_version = Some(v),
            |r| r.disclose_version,
        ),
        (
            "customElementDescriptor.shadow",
            |o, v| {
                o.custom_element_descriptor = Some(FfiSvelteCustomElementDescriptor {
                    tag: None,
                    shadow: Some(v),
                    props: Default::default(),
                })
            },
            |r| r.custom_element_descriptor.as_ref().and_then(|d| d.shadow),
        ),
        (
            "customElementDescriptor.props.reflect",
            |o, v| {
                o.custom_element_descriptor = Some(FfiSvelteCustomElementDescriptor {
                    tag: None,
                    shadow: None,
                    props: [(
                        "value".to_string(),
                        FfiSvelteCustomElementProp {
                            attribute: None,
                            reflect: Some(v),
                            prop_type: None,
                        },
                    )]
                    .into_iter()
                    .collect(),
                })
            },
            |r| {
                r.custom_element_descriptor
                    .as_ref()
                    .and_then(|d| d.props.get("value"))
                    .and_then(|p| p.reflect)
            },
        ),
    ];

    for (name, set, read) in cases {
        for value in [true, false] {
            let mut options = svelte_options();
            set(&mut options, value);
            let request = convert_svelte(options);
            assert_eq!(
                read(request.svelte().expect("Svelte arm")),
                Some(value),
                "{name} must reach the canonical request as {value}"
            );
        }
    }
}

#[test]
fn every_wire_enum_variant_reaches_its_own_canonical_counterpart() {
    for (wire, expected) in [
        (FfiVueBackend::Inferred, VueBackendRequest::Inferred),
        (FfiVueBackend::Vdom, VueBackendRequest::Vdom),
        (FfiVueBackend::Vapor, VueBackendRequest::Vapor),
    ] {
        let mut options = vue_options();
        options.backend = wire;
        assert_eq!(
            convert_vue(options).vue().expect("Vue arm").backend,
            expected
        );
    }
    for (wire, expected) in [
        (FfiVueWhitespace::Preserve, VueWhitespaceStrategy::Preserve),
        (FfiVueWhitespace::Condense, VueWhitespaceStrategy::Condense),
    ] {
        let mut options = vue_options();
        options.whitespace = Some(wire);
        assert_eq!(
            convert_vue(options).vue().expect("Vue arm").whitespace,
            Some(expected)
        );
    }
    for (wire, expected) in [
        (FfiVueParsePad::Space, VueParsePad::Space),
        (FfiVueParsePad::Line, VueParsePad::Line),
        (FfiVueParsePad::Off, VueParsePad::Off),
    ] {
        let mut options = vue_options();
        options.parse_pad = Some(wire);
        assert_eq!(
            convert_vue(options).vue().expect("Vue arm").parse_pad,
            Some(expected)
        );
    }
    for (wire, expected) in [
        (
            FfiVueCssModuleScopeBehaviour::Local,
            VueCssModuleScopeBehaviour::Local,
        ),
        (
            FfiVueCssModuleScopeBehaviour::Global,
            VueCssModuleScopeBehaviour::Global,
        ),
    ] {
        let mut options = vue_options();
        options.css_modules = Some(FfiVueCssModules {
            scope_behaviour: Some(wire),
            hash_prefix: None,
            locals_convention: None,
            export_globals: None,
        });
        assert_eq!(
            convert_vue(options)
                .vue()
                .expect("Vue arm")
                .css_modules
                .as_ref()
                .and_then(|m| m.scope_behaviour),
            Some(expected)
        );
    }
    for (wire, expected) in [
        (
            FfiVueCssModuleLocalsConvention::CamelCase,
            VueCssModuleLocalsConvention::CamelCase,
        ),
        (
            FfiVueCssModuleLocalsConvention::CamelCaseOnly,
            VueCssModuleLocalsConvention::CamelCaseOnly,
        ),
        (
            FfiVueCssModuleLocalsConvention::Dashes,
            VueCssModuleLocalsConvention::Dashes,
        ),
        (
            FfiVueCssModuleLocalsConvention::DashesOnly,
            VueCssModuleLocalsConvention::DashesOnly,
        ),
        (
            FfiVueCssModuleLocalsConvention::AsIs,
            VueCssModuleLocalsConvention::AsIs,
        ),
    ] {
        let mut options = vue_options();
        options.css_modules = Some(FfiVueCssModules {
            scope_behaviour: None,
            hash_prefix: None,
            locals_convention: Some(wire),
            export_globals: None,
        });
        assert_eq!(
            convert_vue(options)
                .vue()
                .expect("Vue arm")
                .css_modules
                .as_ref()
                .and_then(|m| m.locals_convention.clone()),
            Some(expected)
        );
    }

    // The disabled asset-url arm is a distinct variant, not an absent option.
    let mut options = vue_options();
    options.transform_asset_urls = Some(FfiVueAssetUrlTransform::Disabled);
    assert!(matches!(
        convert_vue(options)
            .vue()
            .expect("Vue arm")
            .transform_asset_urls,
        Some(VueAssetUrlTransform::Disabled)
    ));

    for (wire, expected) in [
        (FfiSvelteNamespace::Html, SvelteNamespaceRequest::Html),
        (FfiSvelteNamespace::Svg, SvelteNamespaceRequest::Svg),
        (FfiSvelteNamespace::MathMl, SvelteNamespaceRequest::MathMl),
        (FfiSvelteNamespace::Foreign, SvelteNamespaceRequest::Foreign),
    ] {
        let mut options = svelte_options();
        options.namespace = Some(wire);
        assert_eq!(
            convert_svelte(options)
                .svelte()
                .expect("Svelte arm")
                .namespace,
            Some(expected)
        );
    }
    for (wire, expected) in [
        (FfiSvelteCss::Injected, SvelteCssRequest::Injected),
        (FfiSvelteCss::External, SvelteCssRequest::External),
    ] {
        let mut options = svelte_options();
        options.css = Some(wire);
        assert_eq!(
            convert_svelte(options).svelte().expect("Svelte arm").css,
            Some(expected)
        );
    }
    for (wire, expected) in [
        (FfiSvelteFragments::Html, SvelteFragmentsRequest::Html),
        (FfiSvelteFragments::Tree, SvelteFragmentsRequest::Tree),
    ] {
        let mut options = svelte_options();
        options.fragments = Some(wire);
        assert_eq!(
            convert_svelte(options)
                .svelte()
                .expect("Svelte arm")
                .fragments,
            Some(expected)
        );
    }
    for (wire, expected) in [
        (FfiSvelteRunes::True, SvelteRunesRequest::True),
        (FfiSvelteRunes::False, SvelteRunesRequest::False),
        (FfiSvelteRunes::Infer, SvelteRunesRequest::Infer),
    ] {
        let mut options = svelte_options();
        options.runes = Some(wire);
        assert_eq!(
            convert_svelte(options).svelte().expect("Svelte arm").runes,
            Some(expected)
        );
    }
}

/// The absent polarity for every optional slot: the supplied polarity is
/// asserted by the two "every supported option" fixtures above, so a slot
/// that pinned itself to either value would fail one of the two.
#[test]
fn absent_optional_option_slots_stay_absent() {
    let request = convert_vue(vue_options());
    let v = request.vue().expect("Vue arm");
    assert_eq!(v.delimiters, None);
    assert_eq!(v.whitespace, None);
    assert_eq!(v.comments, None);
    assert_eq!(v.hoist_static, None);
    assert_eq!(v.cache_handlers, None);
    assert_eq!(v.hmr, None);
    assert_eq!(v.optimize_imports, None);
    assert_eq!(v.runtime_module_name, None);
    assert_eq!(v.ssr_runtime_module_name, None);
    assert_eq!(v.parse_pad, None);
    assert_eq!(v.ignore_empty, None);
    assert_eq!(v.gen_default_as, None);
    assert_eq!(v.props_destructure, None);
    assert_eq!(v.script_custom_element, None);
    assert!(v.transform_asset_urls.is_none());
    assert_eq!(v.style_trim, None);
    assert!(v.css_modules.is_none());
    assert!(v.is_custom_element.is_empty());
    assert!(v.babel_parser_plugins.is_empty());

    let request = convert_svelte(svelte_options());
    let s = request.svelte().expect("Svelte arm");
    assert_eq!(s.dev, None);
    assert_eq!(s.custom_element, None);
    assert!(s.custom_element_descriptor.is_none());
    assert_eq!(s.namespace, None);
    assert_eq!(s.css, None);
    assert_eq!(s.preserve_comments, None);
    assert_eq!(s.preserve_whitespace, None);
    assert_eq!(s.fragments, None);
    assert_eq!(s.runes, None);
    assert_eq!(s.disclose_version, None);
    assert!(s.compatibility.is_none());
}

#[test]
fn identity_flags_round_trip_in_both_polarities() {
    for is_production in [true, false] {
        for force_js in [true, false] {
            let request = ffi_host_compile_request_to_compile_request(
                FfiHostCompileRequest::Vue(FfiVueHostCompileRequest {
                    identity: FfiHostCompileIdentity {
                        filename: Some("Comp.vue".to_string()),
                        component_id: Some("c-1".to_string()),
                        is_production,
                        force_js,
                    },
                    products: vec![FfiRequestedProduct::Analysis(analysis_product())],
                    options: vue_options(),
                }),
                &no_profiles(),
            )
            .expect("converts");
            assert_eq!(request.is_production(), is_production);
            assert_eq!(request.force_js(), force_js);
        }
    }
}

#[test]
fn identity_names_are_carried_both_present_and_absent() {
    let present = ffi_host_compile_request_to_compile_request(
        vue(
            vec![FfiRequestedProduct::Analysis(analysis_product())],
            vue_options(),
        ),
        &no_profiles(),
    )
    .expect("converts");
    assert_eq!(present.filename(), Some("Comp.vue"));
    assert_eq!(present.component_id(), Some("c-1"));

    let absent = ffi_host_compile_request_to_compile_request(
        FfiHostCompileRequest::Vue(FfiVueHostCompileRequest {
            identity: FfiHostCompileIdentity {
                filename: None,
                component_id: None,
                is_production: false,
                force_js: false,
            },
            products: vec![FfiRequestedProduct::Analysis(analysis_product())],
            options: vue_options(),
        }),
        &no_profiles(),
    )
    .expect("converts");
    assert_eq!(absent.filename(), None);
    assert_eq!(absent.component_id(), None);
}

#[test]
fn every_product_flag_round_trips_in_both_polarities() {
    for value in [true, false] {
        let runtime = ffi_host_compile_request_to_compile_request(
            vue(
                vec![FfiRequestedProduct::RuntimeClient(
                    FfiRuntimeProductRequest {
                        inline: Some(value),
                        runtime_source_map: value,
                    },
                )],
                vue_options(),
            ),
            &no_profiles(),
        )
        .expect("converts");
        assert_eq!(runtime.wants_runtime_source_map(), value);
        assert_eq!(runtime.resolve_inline(), Some(value));

        let ide = ffi_host_compile_request_to_compile_request(
            vue(
                vec![FfiRequestedProduct::IdeCompanion(FfiIdeProductRequest {
                    want_source_map: value,
                    embed_ambient_types: value,
                    conditional_root_narrowing: value,
                    strict_slots: value,
                    types_module_name: None,
                    ide_chunk_boundaries: value,
                })],
                vue_options(),
            ),
            &no_profiles(),
        )
        .expect("converts");
        assert_eq!(ide.wants_ide_source_map(), value);
        match &ide.products()[0] {
            CompileProduct::IdeCompanion(p) => {
                assert_eq!(p.embed_ambient_types, value);
                assert_eq!(p.conditional_root_narrowing, value);
                assert_eq!(p.strict_slots, value);
                assert_eq!(p.ide_chunk_boundaries, value);
                assert_eq!(p.types_module_name, None, "an absent name stays absent");
            }
            other => panic!("expected the IDE companion product, got {other:?}"),
        }

        // The two analysis axes are set to OPPOSITE values so a conversion
        // that read one slot for both would fail.
        let analysis = ffi_host_compile_request_to_compile_request(
            vue(
                vec![FfiRequestedProduct::Analysis(FfiAnalysisProductRequest {
                    want_script_bindings: value,
                    want_template_data: !value,
                })],
                vue_options(),
            ),
            &no_profiles(),
        )
        .expect("converts");
        assert_eq!(analysis.wants_template_data(), !value);
        match &analysis.products()[0] {
            CompileProduct::Analysis(p) => {
                assert_eq!(p.want_script_bindings, value);
                assert_eq!(p.want_template_data, !value);
            }
            other => panic!("expected the analysis product, got {other:?}"),
        }
    }
}

/// `inline` is the only field on a product struct shared by both framework
/// arms whose canonical meaning is framework-specific: `CompileRequest`
/// refuses it outside the Vue arm rather than ignoring it. Every other
/// shared product field is framework-neutral at construction, so this one
/// case is the whole class.
#[test]
fn an_inline_runtime_product_on_a_svelte_request_is_refused() {
    let err = ffi_host_compile_request_to_compile_request(
        svelte(
            vec![FfiRequestedProduct::RuntimeClient(
                FfiRuntimeProductRequest {
                    inline: Some(true),
                    runtime_source_map: false,
                },
            )],
            svelte_options(),
        ),
        &no_profiles(),
    )
    .expect_err("inline has no Svelte axis");
    assert_eq!(err, CompileRequestError::InlineSsrUnsupported);

    // Non-vacuity: the same product without the Vue-only axis converts.
    let request = ffi_host_compile_request_to_compile_request(
        svelte(
            vec![FfiRequestedProduct::RuntimeClient(
                FfiRuntimeProductRequest {
                    inline: Some(false),
                    runtime_source_map: false,
                },
            )],
            svelte_options(),
        ),
        &no_profiles(),
    )
    .expect("a Svelte runtime product without inline converts");
    assert_eq!(request.resolve_inline(), Some(false));
}

// ── malformed values ─────────────────────────────────────────────────────

#[test]
fn delimiters_of_the_wrong_arity_are_a_typed_malformed_value_refusal() {
    for wrong in [
        vec![],
        vec!["[[".to_string()],
        vec!["a".into(), "b".into(), "c".into()],
    ] {
        let mut options = vue_options();
        let len = wrong.len();
        options.delimiters = Some(wrong);
        let err = ffi_host_compile_request_to_compile_request(
            vue(
                vec![FfiRequestedProduct::RuntimeClient(runtime_product())],
                options,
            ),
            &no_profiles(),
        )
        .expect_err("a delimiter pair must have exactly two elements");
        match err {
            CompileRequestError::MalformedOptionValue { option, .. } => assert_eq!(
                option,
                FrameworkOption::Vue(VueOption::ParserOptionsDelimiters),
                "the refusal names the delimiters row"
            ),
            other => panic!("expected MalformedOptionValue for {len} delimiters, got {other:?}"),
        }
    }
}

/// The wire owns no membership over the custom-element prop-type
/// vocabulary, so an unrecognised spelling DECODES — and is refused one
/// stage later, at canonical request construction, by the same decision
/// the direct canonical entry point makes. The refusal identity and the
/// offending value survive the move.
#[test]
fn a_custom_element_prop_type_outside_the_admitted_vocabulary_is_refused_at_admission() {
    let json = SVELTE_WIRE.replace(r#""propType": "number""#, r#""propType": "nonsense""#);
    let decoded = decode(&json).expect("the wire carries the caller's spelling verbatim");
    match ffi_host_compile_request_to_compile_request(decoded, &no_profiles())
        .expect_err("an unrecognised prop type is refused at canonical admission")
    {
        CompileRequestError::MalformedOptionValue { option, value } => {
            assert_eq!(
                option,
                FrameworkOption::Svelte(SvelteOption::CustomElementPropsType),
                "the refusal names the customElement.props.type row"
            );
            assert_eq!(value, "nonsense", "the offending value is preserved");
        }
        other => panic!("expected MalformedOptionValue, got {other:?}"),
    }
}

/// Every casing outside the ten admitted spellings is refused through the
/// wire route too — there is no case-normalisation rule the transport
/// could apply on its own.
#[test]
fn wire_casing_outside_the_admitted_vocabulary_is_refused() {
    for spelling in ["STRING", "nUmBeR", "Symbol", "sTring", "OBJECT", "arrays"] {
        let json = SVELTE_WIRE.replace(
            r#""propType": "number""#,
            &format!(r#""propType": "{spelling}""#),
        );
        let decoded = decode(&json).expect("the wire forwards any spelling");
        let err = ffi_host_compile_request_to_compile_request(decoded, &no_profiles())
            .err()
            .unwrap_or_else(|| panic!("`{spelling}` is not an admitted spelling"));
        match err {
            CompileRequestError::MalformedOptionValue { option, value } => {
                assert_eq!(
                    option,
                    FrameworkOption::Svelte(SvelteOption::CustomElementPropsType)
                );
                assert_eq!(value, spelling);
            }
            other => panic!("expected MalformedOptionValue for `{spelling}`, got {other:?}"),
        }
    }
}

/// A capitalised spelling is admitted through a real JSON decode, not only
/// through a hand-built struct.
///
/// The derived test below builds `FfiSvelteCustomElementProp` directly, so it
/// proves the conversion admits both cases but never exercises the decoder on
/// one. Every other JSON fixture here spells the prop type lowercase, which is
/// what the wire accepted before the vocabulary was unified — so without this
/// case a decoder that rejected the capitalised half would still pass.
#[test]
fn a_capitalised_spelling_is_admitted_through_a_json_decode() {
    let json = SVELTE_WIRE.replace(r#""propType": "number""#, r#""propType": "Number""#);
    let decoded = decode(&json).expect("the wire forwards a capitalised spelling");
    ffi_host_compile_request_to_compile_request(decoded, &no_profiles())
        .expect("`Number` is an admitted spelling at the wire entry");
}

/// Every admitted spelling reaches its canonical variant through the wire.
///
/// The cases are DERIVED from the canonical vocabulary itself
/// ([`SvelteCustomElementPropType::ALL`]) rather than restated here, so a
/// sixth prop type added to that one list is covered by this test without
/// editing it — and a wire route that stopped forwarding, or started
/// re-deciding, a spelling fails here.
#[test]
fn every_admitted_spelling_reaches_its_canonical_variant_through_the_wire() {
    for expected in SvelteCustomElementPropType::ALL {
        let capitalised = expected.as_svelte_name();
        for spelling in [capitalised.to_string(), capitalised.to_ascii_lowercase()] {
            let mut options = svelte_options();
            options.custom_element_descriptor = Some(FfiSvelteCustomElementDescriptor {
                tag: None,
                shadow: None,
                props: [(
                    "value".to_string(),
                    FfiSvelteCustomElementProp {
                        attribute: None,
                        reflect: None,
                        prop_type: Some(spelling.clone()),
                    },
                )]
                .into_iter()
                .collect(),
            });
            let request = ffi_host_compile_request_to_compile_request(
                svelte(
                    vec![FfiRequestedProduct::RuntimeClient(runtime_product())],
                    options,
                ),
                &no_profiles(),
            )
            .unwrap_or_else(|e| panic!("`{spelling}` is an admitted spelling, got {e:?}"));
            assert_eq!(
                request
                    .svelte()
                    .and_then(|s| s.custom_element_descriptor.as_ref())
                    .and_then(|d| d.props.get("value"))
                    .and_then(|p| p.prop_type),
                Some(*expected),
                "`{spelling}` must admit as {expected:?}"
            );
        }
    }
}

// ── refusals: options, capabilities, product set ─────────────────────────

type SetVueOption = fn(&mut FfiVueCompileOptions);
type SetSvelteOption = fn(&mut FfiSvelteCompileOptions);

#[test]
fn each_refused_vue_option_names_its_own_row_even_when_explicitly_false() {
    let cases: [(SetVueOption, VueOption); 12] = [
        (
            |o| o.compat_config = Some(false),
            VueOption::ParserOptionsCompatConfig,
        ),
        (
            |o| o.compat_config_mode = Some(false),
            VueOption::ParserOptionsCompatConfigMode,
        ),
        (
            |o| o.compat_config_compiler_is_on_element = Some(false),
            VueOption::ParserOptionsCompatConfigCompilerIsOnElement,
        ),
        (
            |o| o.compat_config_compiler_v_bind_sync = Some(false),
            VueOption::ParserOptionsCompatConfigCompilerVBindSync,
        ),
        (
            |o| o.compat_config_compiler_v_if_v_for_precedence = Some(false),
            VueOption::ParserOptionsCompatConfigCompilerVIfVForPrecedence,
        ),
        (
            |o| o.compat_config_compiler_v_bind_object_order = Some(false),
            VueOption::ParserOptionsCompatConfigCompilerVBindObjectOrder,
        ),
        (
            |o| o.compat_config_compiler_v_on_native = Some(false),
            VueOption::ParserOptionsCompatConfigCompilerVOnNative,
        ),
        (
            |o| o.compat_config_compiler_native_template = Some(false),
            VueOption::ParserOptionsCompatConfigCompilerNativeTemplate,
        ),
        (
            |o| o.compat_config_compiler_inline_template = Some(false),
            VueOption::ParserOptionsCompatConfigCompilerInlineTemplate,
        ),
        (
            |o| o.compat_config_compiler_filters = Some(false),
            VueOption::ParserOptionsCompatConfigCompilerFilters,
        ),
        (
            |o| o.transform_compat_config = Some(false),
            VueOption::TransformOptionsCompatConfig,
        ),
        (
            |o| o.codegen_mode = Some(false),
            VueOption::CodegenOptionsMode,
        ),
    ];
    for (set, expected) in cases {
        let mut options = vue_options();
        set(&mut options);
        let err = ffi_host_compile_request_to_compile_request(
            vue(
                vec![FfiRequestedProduct::RuntimeClient(runtime_product())],
                options,
            ),
            &no_profiles(),
        )
        .expect_err("a refused option is refused on presence");
        match err {
            CompileRequestError::UnsupportedOption { option, .. } => {
                assert_eq!(option, FrameworkOption::Vue(expected));
            }
            other => panic!("expected UnsupportedOption for {expected:?}, got {other:?}"),
        }
    }
}

#[test]
fn each_refused_svelte_option_names_its_own_row_even_when_explicitly_false() {
    let cases: [(SetSvelteOption, SvelteOption); 6] = [
        (|o| o.loose = Some(false), SvelteOption::ParseLoose),
        (
            |o| o.accessors = Some(false),
            SvelteOption::CompileOptionsAccessors,
        ),
        (
            |o| o.immutable = Some(false),
            SvelteOption::CompileOptionsImmutable,
        ),
        (
            |o| o.compatibility_component_api = Some(false),
            SvelteOption::CompileOptionsCompatibilityComponentApi,
        ),
        (|o| o.hmr = Some(false), SvelteOption::CompileOptionsHmr),
        (
            |o| o.custom_element_extend = Some(false),
            SvelteOption::CustomElementExtend,
        ),
    ];
    for (set, expected) in cases {
        let mut options = svelte_options();
        set(&mut options);
        let err = ffi_host_compile_request_to_compile_request(
            svelte(
                vec![FfiRequestedProduct::RuntimeClient(runtime_product())],
                options,
            ),
            &no_profiles(),
        )
        .expect_err("a refused option is refused on presence");
        match err {
            CompileRequestError::UnsupportedOption { option, .. } => {
                assert_eq!(option, FrameworkOption::Svelte(expected));
            }
            other => panic!("expected UnsupportedOption for {expected:?}, got {other:?}"),
        }
    }
}

#[test]
fn an_option_gated_on_an_unsupported_capability_names_that_capability() {
    use verter_compiler::compile_request::CapabilityCell;
    for (set, expected) in [
        (
            (|o: &mut FfiSvelteCompileOptions| o.generate_module = Some(true)) as SetSvelteOption,
            SvelteOption::ModuleGenerate,
        ),
        (
            |o: &mut FfiSvelteCompileOptions| o.experimental_async = Some(true),
            SvelteOption::ModuleExperimentalAsync,
        ),
    ] {
        let mut options = svelte_options();
        set(&mut options);
        let err = ffi_host_compile_request_to_compile_request(
            svelte(
                vec![FfiRequestedProduct::RuntimeClient(runtime_product())],
                options,
            ),
            &no_profiles(),
        )
        .expect_err("the module-compilation capability is not available");
        match err {
            CompileRequestError::UnsupportedOption { option, capability } => {
                assert_eq!(option, FrameworkOption::Svelte(expected));
                assert_eq!(
                    capability,
                    Some(CapabilityCell::SvelteModule),
                    "the refusal names the unavailable capability, not just the option"
                );
            }
            other => panic!("expected UnsupportedOption, got {other:?}"),
        }
    }
}

#[test]
fn an_unsupported_backend_and_product_combination_is_refused_at_construction() {
    let mut options = vue_options();
    options.backend = FfiVueBackend::Vapor;
    let err = ffi_host_compile_request_to_compile_request(
        vue(
            vec![FfiRequestedProduct::RuntimeServer(runtime_product())],
            options,
        ),
        &no_profiles(),
    )
    .expect_err("there is no server compiler backend for this client backend");
    assert_eq!(err, CompileRequestError::SsrVaporBackendUnsupported);
}

#[test]
fn an_inline_server_product_is_refused_rather_than_silently_demoted() {
    let mut options = vue_options();
    options.backend = FfiVueBackend::Vdom;
    let err = ffi_host_compile_request_to_compile_request(
        vue(
            vec![FfiRequestedProduct::RuntimeServer(
                FfiRuntimeProductRequest {
                    inline: Some(true),
                    runtime_source_map: false,
                },
            )],
            options,
        ),
        &no_profiles(),
    )
    .expect_err("inline has no server axis");
    assert_eq!(err, CompileRequestError::InlineSsrUnsupported);
}

#[test]
fn an_empty_product_set_is_refused() {
    let err =
        ffi_host_compile_request_to_compile_request(vue(Vec::new(), vue_options()), &no_profiles())
            .expect_err("a request must name at least one product");
    assert_eq!(err, CompileRequestError::EmptyProductSet);
}

#[test]
fn a_duplicated_product_kind_is_refused() {
    let err = ffi_host_compile_request_to_compile_request(
        vue(
            vec![
                FfiRequestedProduct::Analysis(analysis_product()),
                FfiRequestedProduct::Analysis(analysis_product()),
            ],
            vue_options(),
        ),
        &no_profiles(),
    )
    .expect_err("a product kind may be requested once");
    assert_eq!(
        err,
        CompileRequestError::DuplicateProduct(ProductKind::Analysis)
    );
}

// ── decode-time refusals: unknown and cross-framework fields ─────────────

/// Populates every nested Vue-arm struct, so a key injected at any of them
/// lands inside a struct the decoder actually visits.
const VUE_WIRE: &str = r#"{"vue":{
  "identity": {"filename": "Comp.vue", "componentId": "c-1", "isProduction": false, "forceJs": false},
  "products": [
    {"runtimeClient": {"inline": false, "runtimeSourceMap": false}},
    {"ideCompanion": {"wantSourceMap": false, "embedAmbientTypes": false,
                      "conditionalRootNarrowing": false, "strictSlots": false,
                      "typesModuleName": null, "ideChunkBoundaries": false}},
    {"analysis": {"wantScriptBindings": true, "wantTemplateData": false}}
  ],
  "options": {
    "backend": "vdom",
    "ssr": false,
    "isCustomElement": [],
    "babelParserPlugins": [],
    "transformAssetUrls": {"enabled": {"base": null, "includeAbsolute": null, "tags": {}}},
    "cssModules": {"scopeBehaviour": "local", "hashPrefix": null,
                   "localsConvention": "camelCase", "exportGlobals": true}
  }
}}"#;

/// The Svelte-arm counterpart, populating the descriptor, one prop, and the
/// compatibility marker.
const SVELTE_WIRE: &str = r#"{"svelte":{
  "identity": {"filename": "Comp.svelte", "componentId": "c-2", "isProduction": false, "forceJs": false},
  "products": [{"runtimeClient": {"inline": false, "runtimeSourceMap": true}}],
  "options": {
    "customElementDescriptor": {"tag": "my-el", "shadow": true,
      "props": {"value": {"attribute": "value", "reflect": true, "propType": "number"}}},
    "compatibility": {}
  }
}}"#;

fn decode(json: &str) -> Result<FfiHostCompileRequest, serde_json::Error> {
    serde_json::from_str(json)
}

#[test]
fn the_baseline_wire_payloads_decode_and_convert() {
    // Non-vacuity anchor for every decode refusal below: the same payload
    // minus the offending key is accepted, on both arms.
    let vue = ffi_host_compile_request_to_compile_request(
        decode(VUE_WIRE).expect("the Vue payload decodes"),
        &no_profiles(),
    )
    .expect("and converts");
    let kinds: Vec<ProductKind> = vue.products().iter().map(|p| p.kind()).collect();
    assert_eq!(
        kinds,
        vec![
            ProductKind::RuntimeClient,
            ProductKind::IdeCompanion,
            ProductKind::Analysis
        ]
    );

    let svelte = ffi_host_compile_request_to_compile_request(
        decode(SVELTE_WIRE).expect("the Svelte payload decodes"),
        &no_profiles(),
    )
    .expect("and converts");
    assert_eq!(
        svelte
            .svelte()
            .and_then(|s| s.custom_element_descriptor.as_ref())
            .and_then(|d| d.props.get("value"))
            .and_then(|p| p.prop_type),
        Some(SvelteCustomElementPropType::Number)
    );
}

/// Every `deny_unknown_fields` struct on the decode path, addressed inside
/// a payload that populates it. The compile-error-on-a-new-field property
/// and the unknown-key refusal both have to hold at EVERY depth, not only
/// at the top level: dropping `deny_unknown_fields` from any one struct
/// here makes exactly its row accept the injected key and fail.
#[test]
fn an_unknown_key_in_any_wire_struct_on_the_decode_path_is_refused() {
    const CASES: [(&str, &str, &str); 13] = [
        ("vue request", VUE_WIRE, "/vue"),
        ("identity", VUE_WIRE, "/vue/identity"),
        ("vue options", VUE_WIRE, "/vue/options"),
        (
            "vue asset-url options",
            VUE_WIRE,
            "/vue/options/transformAssetUrls/enabled",
        ),
        ("vue css modules", VUE_WIRE, "/vue/options/cssModules"),
        ("runtime product", VUE_WIRE, "/vue/products/0/runtimeClient"),
        ("ide product", VUE_WIRE, "/vue/products/1/ideCompanion"),
        ("analysis product", VUE_WIRE, "/vue/products/2/analysis"),
        ("svelte request", SVELTE_WIRE, "/svelte"),
        ("svelte options", SVELTE_WIRE, "/svelte/options"),
        (
            "svelte custom-element descriptor",
            SVELTE_WIRE,
            "/svelte/options/customElementDescriptor",
        ),
        (
            "svelte custom-element prop",
            SVELTE_WIRE,
            "/svelte/options/customElementDescriptor/props/value",
        ),
        (
            "svelte compatibility",
            SVELTE_WIRE,
            "/svelte/options/compatibility",
        ),
    ];

    for (label, payload, pointer) in CASES {
        let mut value: serde_json::Value = serde_json::from_str(payload)
            .unwrap_or_else(|e| panic!("{label}: the base payload must parse: {e}"));
        value
            .pointer_mut(pointer)
            .unwrap_or_else(|| panic!("{label}: nothing at {pointer}"))
            .as_object_mut()
            .unwrap_or_else(|| panic!("{label}: {pointer} is not an object"))
            .insert("notAField".to_string(), serde_json::Value::Bool(true));

        let err = match decode(&value.to_string()) {
            Ok(_) => panic!("{label}: an unrecognised key at {pointer} was silently dropped"),
            Err(err) => err,
        };
        assert!(
            err.to_string().contains("notAField"),
            "{label}: the refusal names the offending key: {err}"
        );
    }
}

#[test]
fn an_unknown_option_key_is_refused_at_decode() {
    let json = VUE_WIRE.replace(r#""ssr": false"#, r#""ssr": false, "notAnOption": true"#);
    let err = decode(&json).expect_err("an unrecognised key is never dropped");
    assert!(
        err.to_string().contains("notAnOption"),
        "the refusal names the offending key: {err}"
    );
}

#[test]
fn a_svelte_option_inside_the_vue_arm_is_refused_at_decode() {
    let json = VUE_WIRE.replace(r#""ssr": false"#, r#""ssr": false, "runes": "infer""#);
    let err = decode(&json).expect_err("a cross-framework option is not accepted");
    assert!(
        err.to_string().contains("runes"),
        "the refusal names the cross-framework key: {err}"
    );
}

#[test]
fn a_vue_option_inside_the_svelte_arm_is_refused_at_decode() {
    let json = SVELTE_WIRE.replace(
        r#""compatibility": {}"#,
        r#""compatibility": {}, "cacheHandlers": true"#,
    );
    let err = decode(&json).expect_err("a cross-framework option is not accepted");
    assert!(
        err.to_string().contains("cacheHandlers"),
        "the refusal names the cross-framework key: {err}"
    );
}

#[test]
fn an_unknown_framework_is_refused_at_decode() {
    let json = VUE_WIRE.replace(r#"{"vue":"#, r#"{"solid":"#);
    let err = decode(&json).expect_err("only registered frameworks decode");
    assert!(
        err.to_string().contains("solid"),
        "the refusal names the unknown framework: {err}"
    );
}

#[test]
fn an_unknown_product_is_refused_at_decode() {
    let json = VUE_WIRE.replace(r#""analysis""#, r#""sourceMapOnly""#);
    let err = decode(&json).expect_err("the product vocabulary is closed");
    assert!(
        err.to_string().contains("sourceMapOnly"),
        "the refusal names the unknown product: {err}"
    );
}

#[test]
fn an_absent_required_field_is_refused_rather_than_defaulted() {
    let json = VUE_WIRE.replace(r#""isProduction": false, "#, "");
    let err = decode(&json).expect_err("a required flag has no default");
    assert!(
        err.to_string().contains("isProduction"),
        "the refusal names the missing field: {err}"
    );
}

// ── the legacy profile is untouched ──────────────────────────────────────

#[test]
fn the_legacy_compile_profile_keeps_its_exact_field_set_and_its_query_slot() {
    // Compile-level pin: this destructure names every legacy field with no
    // rest pattern, so adding, removing, or renaming one fails to compile;
    // binding the query's slot as `Option<FfiCompileProfile>` fails to
    // compile if the legacy profile is ever swapped for the new schema.
    let query = FfiVirtualQuery {
        raw_id: None,
        canonical_id: None,
        node_kind: None,
        compile_profile: Some(FfiCompileProfile {
            filename: Some("Legacy.vue".to_string()),
            is_production: Some(true),
            custom_element: None,
            ssr: None,
            ssr_module_id: None,
            hmr_strategy: None,
            component_id: None,
            delimiters: None,
            custom_elements: None,
            comments: None,
            runtime_module_name: None,
            types_module_name: None,
            force_vapor: None,
            force_js: None,
            source_map: None,
            target: Some("ide".to_string()),
            inline: None,
            strict_slots: None,
            requested_mode: None,
        }),
    };
    let slot: Option<FfiCompileProfile> = query.compile_profile;
    let FfiCompileProfile {
        filename,
        is_production,
        custom_element: _,
        ssr: _,
        ssr_module_id: _,
        hmr_strategy: _,
        component_id: _,
        delimiters: _,
        custom_elements: _,
        comments: _,
        runtime_module_name: _,
        types_module_name: _,
        force_vapor: _,
        force_js: _,
        source_map: _,
        target,
        inline: _,
        strict_slots: _,
        requested_mode: _,
    } = slot.expect("the legacy slot still carries the legacy profile");

    // Behavioural pin: the legacy route still produces a host compile
    // profile, unchanged by the new schema living beside it.
    let host_profile = crate::convert::ffi_profile_to_host(Some(FfiCompileProfile {
        filename: filename.clone(),
        is_production,
        custom_element: None,
        ssr: None,
        ssr_module_id: None,
        hmr_strategy: None,
        component_id: None,
        delimiters: None,
        custom_elements: None,
        comments: None,
        runtime_module_name: None,
        types_module_name: None,
        force_vapor: None,
        force_js: None,
        source_map: None,
        target,
        inline: None,
        strict_slots: None,
        requested_mode: None,
    }))
    .expect("the legacy profile route still converts");
    assert_eq!(host_profile.filename.as_deref(), Some("Legacy.vue"));
    assert!(host_profile.is_production);
    assert_eq!(host_profile.target, verter_session::CompileTarget::IDE);
}
