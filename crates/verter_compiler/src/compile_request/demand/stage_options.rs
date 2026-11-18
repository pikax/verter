//! The option-impact classification behind [`super::StageKeys`]: which
//! stage each caller-settable request option belongs to, and the canonical
//! bytes of each stage's options.
//!
//! An option belongs to the earliest stage whose output it can change.
//! Because every stage key composes its predecessor, a change to an option
//! re-keys its own stage and every later one, and never an earlier one.

use verter_identity::encoding::{CanonicalDigest, CanonicalEncoder};

use super::TargetKind;
use crate::compile_request::svelte::{
    SvelteCompileRequest, SvelteCssRequest, SvelteFragmentsRequest, SvelteNamespaceRequest,
    SvelteRunesRequest,
};
use crate::compile_request::vue::{
    VueAssetUrlTransform, VueBackendRequest, VueCompileRequest, VueCssModuleLocalsConvention,
    VueCssModuleScopeBehaviour, VueParsePad, VueWhitespaceStrategy,
};
use crate::compile_request::{
    CompileRequest, FrameworkCompileRequest, ProductKind, RuntimeHmrStrategy,
    RuntimeProductRequest, RuntimeStyleProcessing,
};

/// The stage an option first affects.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum OptionStage {
    Semantic,
    Structure,
    TargetPlan,
    Emit,
    Terminal,
}

impl OptionStage {
    const fn domain(self) -> &'static str {
        match self {
            OptionStage::Semantic => "verter.compiler.options.semantic.v1",
            OptionStage::Structure => "verter.compiler.options.structure.v1",
            OptionStage::TargetPlan => "verter.compiler.options.target-plan.v1",
            OptionStage::Emit => "verter.compiler.options.emit.v1",
            OptionStage::Terminal => "verter.compiler.options.terminal.v1",
        }
    }
}

/// The stage of a Vue request field, spelled as
/// [`crate::compile_request::VueOption::request_field`] spells it (the part
/// before the first `.`), plus the Verter-only `backend` knob.
pub(super) fn vue_field_stage(field: &str) -> Option<OptionStage> {
    use OptionStage::*;
    match field {
        // Which tags are components, what the expressions are, how
        // bindings are classified.
        "isCustomElement" | "delimiters" | "propsDestructure" | "babelParserPlugins" => {
            Some(Semantic)
        }
        // The compile structure's nodes: kept comments, condensed
        // whitespace, padded and empty blocks.
        "whitespace" | "comments" | "parsePad" | "ignoreEmpty" => Some(Structure),
        // How each target plans the structure.
        "hoistStatic" | "cacheHandlers" | "transformAssetUrls" | "ssr" | "backend" => {
            Some(TargetPlan)
        }
        // What the emitted module looks like.
        "hmr"
        | "optimizeImports"
        | "runtimeModuleName"
        | "ssrRuntimeModuleName"
        | "genDefaultAs"
        | "scriptCustomElement"
        | "styleTrim"
        | "cssModules" => Some(Emit),
        _ => None,
    }
}

/// One classified option: its stage, a stable name, and its value bytes.
type Entry = (OptionStage, &'static str, Vec<u8>);

/// The request-wide options of one compile, classified by stage.
pub(super) struct ClassifiedOptions {
    entries: Vec<Entry>,
}

impl ClassifiedOptions {
    pub(super) fn classify(request: &CompileRequest) -> Self {
        let mut entries = Vec::with_capacity(40);
        entries.push((
            OptionStage::Semantic,
            "isProduction",
            flag(request.is_production()),
        ));
        entries.push((
            OptionStage::Semantic,
            "semanticProfile",
            optional(
                request
                    .semantic_profile()
                    .map(|profile| profile.digest().as_bytes().to_vec()),
            ),
        ));
        // The file name is the component's name: self-references resolve
        // through it.
        entries.push((
            OptionStage::Semantic,
            "filename",
            optional(request.filename().map(|name| name.as_bytes().to_vec())),
        ));
        // The component id is the scope id the targets stamp.
        entries.push((
            OptionStage::TargetPlan,
            "componentId",
            optional(request.component_id().map(|id| id.as_bytes().to_vec())),
        ));
        entries.push((OptionStage::Emit, "forceJs", flag(request.force_js())));
        entries.push((
            OptionStage::Emit,
            "ssrModuleId",
            optional(request.ssr_module_id().map(|id| id.as_bytes().to_vec())),
        ));
        entries.push((
            OptionStage::Emit,
            "hmrStrategy",
            variant(match request.hmr_strategy() {
                RuntimeHmrStrategy::None => 0,
                RuntimeHmrStrategy::Vite => 1,
                RuntimeHmrStrategy::Webpack => 2,
            }),
        ));
        entries.push((
            OptionStage::Emit,
            "emitSsrModuleRegistration",
            flag(request.emit_ssr_module_registration()),
        ));
        match request.framework() {
            FrameworkCompileRequest::Vue(vue) => {
                for (field, value) in vue_values(vue) {
                    let stage =
                        vue_field_stage(field).expect("every keyed Vue request field has a stage");
                    entries.push((stage, field, value));
                }
            }
            FrameworkCompileRequest::Svelte(svelte) => {
                entries.extend(svelte_entries(svelte));
            }
        }
        Self { entries }
    }

    /// Canonical bytes of one stage's request-wide options plus `extra`,
    /// the stage's per-product options.
    pub(super) fn bytes(&self, stage: OptionStage, extra: &[Entry]) -> Vec<u8> {
        let mut encoder = CanonicalEncoder::new(stage.domain());
        encoder
            .field_sorted_map(
                1,
                self.entries
                    .iter()
                    .chain(extra)
                    .filter(|(entry_stage, _, _)| *entry_stage == stage)
                    .map(|(_, name, value)| (name.as_bytes(), value.as_slice())),
            )
            .expect("option names are unique");
        encoder.finish()
    }
}

/// The per-product options of one runtime target.
pub(super) fn product_entries(product: &RuntimeProductRequest, is_production: bool) -> [Entry; 4] {
    [
        (
            OptionStage::TargetPlan,
            "inline",
            flag(product.resolve_inline(is_production)),
        ),
        (
            OptionStage::TargetPlan,
            "styleProcessing",
            variant(match product.style_processing {
                RuntimeStyleProcessing::Complete => 0,
                RuntimeStyleProcessing::AuthoredOnly => 1,
            }),
        ),
        (
            OptionStage::Terminal,
            "outputProfile",
            optional(
                product
                    .output_profile
                    .as_ref()
                    .map(|profile| profile.digest().as_bytes().to_vec()),
            ),
        ),
        (
            OptionStage::Terminal,
            "serialization",
            optional(
                product
                    .serialization
                    .as_ref()
                    .map(|profile| profile.digest().as_bytes().to_vec()),
            ),
        ),
    ]
}

/// Digest of the framework whose semantic profile the compile runs under.
pub(super) fn framework_profile(request: &CompileRequest) -> CanonicalDigest {
    let framework = match request.framework() {
        FrameworkCompileRequest::Vue(_) => "vue",
        FrameworkCompileRequest::Svelte(_) => "svelte",
    };
    CanonicalEncoder::new("verter.compiler.framework-profile.v1")
        .field_str(1, framework)
        .digest()
}

/// Digest of the `(product, target)` a target plan executes for.
pub(super) fn target_digest(product: ProductKind, target: TargetKind) -> CanonicalDigest {
    CanonicalEncoder::new("verter.compiler.target.v1")
        .field_str(1, product.wire_tag())
        .field_str(2, target.name())
        .digest()
}

/// Digest of whether the emit stage produces a runtime map.
pub(super) fn map_mode(runtime_source_map: bool) -> CanonicalDigest {
    CanonicalEncoder::new("verter.compiler.map-mode.v1")
        .field_bool(1, runtime_source_map)
        .digest()
}

/// One option value in the canonical encoding, so presence bytes and length
/// prefixes are the identity encoding's own.
fn value(encode: impl FnOnce(&mut CanonicalEncoder)) -> Vec<u8> {
    let mut encoder = CanonicalEncoder::new("verter.compiler.option-value.v1");
    encode(&mut encoder);
    encoder.finish()
}

fn flag(set: bool) -> Vec<u8> {
    value(|encoder| {
        encoder.field_bool(1, set);
    })
}

/// An enum value by its assigned discriminant.
fn variant(discriminant: u32) -> Vec<u8> {
    value(|encoder| {
        encoder.field_enum_discriminant(1, discriminant);
    })
}

/// Absent is distinct from every present value, including an empty one.
fn optional(present: Option<Vec<u8>>) -> Vec<u8> {
    value(|encoder| {
        encoder.field_option(1, present.as_deref());
    })
}

fn optional_flag(present: Option<bool>) -> Vec<u8> {
    optional(present.map(flag))
}

fn optional_text(present: Option<&str>) -> Vec<u8> {
    value(|encoder| {
        encoder.field_option(1, present.map(str::as_bytes));
    })
}

/// An ordered sequence of values, each its own length-delimited field, so
/// neighbours cannot fuse.
fn parts<'p>(items: impl IntoIterator<Item = &'p [u8]>) -> Vec<u8> {
    value(|encoder| {
        for item in items {
            encoder.field_bytes(1, item);
        }
    })
}

fn texts(items: &[String]) -> Vec<u8> {
    parts(items.iter().map(|item| item.as_bytes()))
}

/// Every keyed field of the canonical Vue request, by request field name.
fn vue_values(vue: &VueCompileRequest) -> Vec<(&'static str, Vec<u8>)> {
    vec![
        (
            "backend",
            variant(match vue.backend {
                VueBackendRequest::Inferred => 0,
                VueBackendRequest::Vdom => 1,
                VueBackendRequest::Vapor => 2,
            }),
        ),
        ("ssr", flag(vue.ssr)),
        ("isCustomElement", texts(&vue.is_custom_element)),
        (
            "delimiters",
            optional(
                vue.delimiters
                    .as_ref()
                    .map(|(open, close)| parts([open.as_bytes(), close.as_bytes()])),
            ),
        ),
        (
            "whitespace",
            optional(vue.whitespace.map(|whitespace| {
                variant(match whitespace {
                    VueWhitespaceStrategy::Preserve => 0,
                    VueWhitespaceStrategy::Condense => 1,
                })
            })),
        ),
        ("comments", optional_flag(vue.comments)),
        ("hoistStatic", optional_flag(vue.hoist_static)),
        ("cacheHandlers", optional_flag(vue.cache_handlers)),
        ("hmr", optional_flag(vue.hmr)),
        ("optimizeImports", optional_flag(vue.optimize_imports)),
        (
            "runtimeModuleName",
            optional_text(vue.runtime_module_name.as_deref()),
        ),
        (
            "ssrRuntimeModuleName",
            optional_text(vue.ssr_runtime_module_name.as_deref()),
        ),
        (
            "parsePad",
            optional(vue.parse_pad.map(|pad| {
                variant(match pad {
                    VueParsePad::Space => 0,
                    VueParsePad::Line => 1,
                    VueParsePad::Off => 2,
                })
            })),
        ),
        ("ignoreEmpty", optional_flag(vue.ignore_empty)),
        ("babelParserPlugins", texts(&vue.babel_parser_plugins)),
        ("genDefaultAs", optional_text(vue.gen_default_as.as_deref())),
        ("propsDestructure", optional_flag(vue.props_destructure)),
        (
            "scriptCustomElement",
            optional_flag(vue.script_custom_element),
        ),
        (
            "transformAssetUrls",
            optional(vue.transform_asset_urls.as_ref().map(asset_urls)),
        ),
        ("styleTrim", optional_flag(vue.style_trim)),
        (
            "cssModules",
            optional(vue.css_modules.as_ref().map(|modules| {
                let scope = optional(modules.scope_behaviour.map(|scope| {
                    variant(match scope {
                        VueCssModuleScopeBehaviour::Local => 0,
                        VueCssModuleScopeBehaviour::Global => 1,
                    })
                }));
                let prefix = optional_text(modules.hash_prefix.as_deref());
                let convention = optional(modules.locals_convention.as_ref().map(|convention| {
                    variant(match convention {
                        VueCssModuleLocalsConvention::CamelCase => 0,
                        VueCssModuleLocalsConvention::CamelCaseOnly => 1,
                        VueCssModuleLocalsConvention::Dashes => 2,
                        VueCssModuleLocalsConvention::DashesOnly => 3,
                        VueCssModuleLocalsConvention::AsIs => 4,
                    })
                }));
                let globals = optional_flag(modules.export_globals);
                parts([
                    scope.as_slice(),
                    prefix.as_slice(),
                    convention.as_slice(),
                    globals.as_slice(),
                ])
            })),
        ),
    ]
}

fn asset_urls(transform: &VueAssetUrlTransform) -> Vec<u8> {
    match transform {
        VueAssetUrlTransform::Disabled => variant(0),
        VueAssetUrlTransform::Enabled(options) => {
            let base = optional_text(options.base.as_deref());
            let absolute = optional_flag(options.include_absolute);
            // `tags` is a sorted map, so its iteration order is canonical.
            let tags: Vec<Vec<u8>> = options
                .tags
                .iter()
                .map(|(tag, attributes)| parts([tag.as_bytes(), texts(attributes).as_slice()]))
                .collect();
            let tags = parts(tags.iter().map(Vec::as_slice));
            parts([
                variant(1).as_slice(),
                base.as_slice(),
                absolute.as_slice(),
                tags.as_slice(),
            ])
        }
    }
}

/// Svelte requests have no staged route. Their options key the semantic
/// stage, so any change re-keys every stage: never stale, only coarser.
fn svelte_entries(svelte: &SvelteCompileRequest) -> Vec<Entry> {
    let custom_element_descriptor =
        optional(svelte.custom_element_descriptor.as_ref().map(|descriptor| {
            let tag = optional_text(descriptor.tag.as_deref());
            let shadow = optional_flag(descriptor.shadow);
            let props: Vec<Vec<u8>> = descriptor
                .props
                .iter()
                .map(|(name, prop)| {
                    let attribute = optional_text(prop.attribute.as_deref());
                    let reflect = optional_flag(prop.reflect);
                    let prop_type =
                        optional_text(prop.prop_type.map(|prop_type| prop_type.as_svelte_name()));
                    parts([
                        name.as_bytes(),
                        attribute.as_slice(),
                        reflect.as_slice(),
                        prop_type.as_slice(),
                    ])
                })
                .collect();
            let props = parts(props.iter().map(Vec::as_slice));
            parts([tag.as_slice(), shadow.as_slice(), props.as_slice()])
        }));
    [
        ("svelteDev", optional_flag(svelte.dev)),
        ("svelteCustomElement", optional_flag(svelte.custom_element)),
        ("svelteCustomElementDescriptor", custom_element_descriptor),
        (
            "svelteNamespace",
            optional(svelte.namespace.map(|namespace| {
                variant(match namespace {
                    SvelteNamespaceRequest::Html => 0,
                    SvelteNamespaceRequest::Svg => 1,
                    SvelteNamespaceRequest::MathMl => 2,
                    SvelteNamespaceRequest::Foreign => 3,
                })
            })),
        ),
        (
            "svelteCss",
            optional(svelte.css.map(|css| {
                variant(match css {
                    SvelteCssRequest::Injected => 0,
                    SvelteCssRequest::External => 1,
                })
            })),
        ),
        (
            "sveltePreserveComments",
            optional_flag(svelte.preserve_comments),
        ),
        (
            "sveltePreserveWhitespace",
            optional_flag(svelte.preserve_whitespace),
        ),
        (
            "svelteFragments",
            optional(svelte.fragments.map(|fragments| {
                variant(match fragments {
                    SvelteFragmentsRequest::Html => 0,
                    SvelteFragmentsRequest::Tree => 1,
                })
            })),
        ),
        (
            "svelteRunes",
            optional(svelte.runes.map(|runes| {
                variant(match runes {
                    SvelteRunesRequest::True => 0,
                    SvelteRunesRequest::False => 1,
                    SvelteRunesRequest::Infer => 2,
                })
            })),
        ),
        (
            "svelteDiscloseVersion",
            optional_flag(svelte.disclose_version),
        ),
        (
            "svelteCompatibility",
            optional(svelte.compatibility.map(|_| Vec::new())),
        ),
    ]
    .into_iter()
    .map(|(name, value)| (OptionStage::Semantic, name, value))
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compile_request::vue::{VueOptionClass, ALL_VUE_OPTIONS};

    #[test]
    fn every_caller_settable_vue_option_is_keyed_in_one_stage() {
        let keyed: Vec<&str> = vue_values(&VueCompileRequest::default())
            .into_iter()
            .map(|(field, _)| field)
            .collect();
        for option in ALL_VUE_OPTIONS {
            if option.class() != VueOptionClass::SupportedCanonical {
                continue;
            }
            let field = option
                .request_field()
                .expect("a caller-settable option has a request field");
            let field = field.split('.').next().unwrap_or(field);
            assert!(
                vue_field_stage(field).is_some(),
                "{option:?} ({field}) has no stage"
            );
            assert!(keyed.contains(&field), "{option:?} ({field}) is not keyed");
        }
        for field in keyed {
            assert!(vue_field_stage(field).is_some(), "{field} has no stage");
        }
    }

    #[test]
    fn absent_and_present_values_encode_differently() {
        assert_ne!(optional_flag(None), optional_flag(Some(false)));
        assert_ne!(optional_text(None), optional_text(Some("")));
        assert_ne!(
            texts(&["ab".to_owned(), "c".to_owned()]),
            texts(&["a".to_owned(), "bc".to_owned()])
        );
    }
}
