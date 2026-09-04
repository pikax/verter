//! The canonical Vue portion of a [`crate::compile_request::CompileRequest`]
//! — [`VueCompileRequest`] — plus the exhaustive, structural classification
//! of every row of `vue-options.tsv` ([`VueOption`]/[`VueOptionClass`]),
//! which is the compile-error-on-drift proof that every semantics-affecting
//! Vue option maps exactly once onto a field, a derived computation, a
//! host-resolved pass-through, or a typed unsupported refusal.

use std::collections::BTreeMap;

/// One row of `vue-options.tsv` (118 data rows). Variant names are
/// `Surface_option`; a `compatConfig` deprecation key is
/// `ParserOptions_CompatConfig<Key>` / `TransformOptions_CompatConfig`. A
/// row that recurs across two surfaces with the *same* canonical treatment
/// (`isCustomElement`, `hoistStatic`) gets one variant per surface — the
/// exactly-once requirement is per TSV row, not per canonical field; two
/// rows are free to fold onto the same canonical field (see
/// `option-inventories.md`'s "listed once for the semantic key" note).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum VueOption {
    // compiler-core:ParserOptions (27)
    ParserOptionsOnWarn,
    ParserOptionsOnError,
    ParserOptionsCompatConfig,
    ParserOptionsCompatConfigMode,
    ParserOptionsCompatConfigCompilerIsOnElement,
    ParserOptionsCompatConfigCompilerVBindSync,
    ParserOptionsCompatConfigCompilerVIfVForPrecedence,
    ParserOptionsCompatConfigCompilerVBindObjectOrder,
    ParserOptionsCompatConfigCompilerVOnNative,
    ParserOptionsCompatConfigCompilerNativeTemplate,
    ParserOptionsCompatConfigCompilerInlineTemplate,
    ParserOptionsCompatConfigCompilerFilters,
    ParserOptionsParseMode,
    ParserOptionsNs,
    ParserOptionsIsNativeTag,
    ParserOptionsIsVoidTag,
    ParserOptionsIsPreTag,
    ParserOptionsIsIgnoreNewlineTag,
    ParserOptionsIsBuiltInComponent,
    ParserOptionsIsCustomElement,
    ParserOptionsGetNamespace,
    ParserOptionsDelimiters,
    ParserOptionsWhitespace,
    ParserOptionsDecodeEntities,
    ParserOptionsComments,
    ParserOptionsPrefixIdentifiers,
    ParserOptionsExpressionPlugins,

    // compiler-core:TransformOptions (14)
    TransformOptionsNodeTransforms,
    TransformOptionsDirectiveTransforms,
    TransformOptionsTransformHoist,
    TransformOptionsOnWarn,
    TransformOptionsOnError,
    TransformOptionsCompatConfig,
    TransformOptionsIsBuiltInComponent,
    TransformOptionsIsCustomElement,
    TransformOptionsHoistStatic,
    TransformOptionsCacheHandlers,
    TransformOptionsScopeId,
    TransformOptionsSlotted,
    TransformOptionsSsrCssVars,
    TransformOptionsHmr,

    // compiler-core:SharedTransformCodegenOptions (8)
    SharedTransformCodegenOptionsPrefixIdentifiers,
    SharedTransformCodegenOptionsExpressionPlugins,
    SharedTransformCodegenOptionsSsr,
    SharedTransformCodegenOptionsInSsr,
    SharedTransformCodegenOptionsBindingMetadata,
    SharedTransformCodegenOptionsInline,
    SharedTransformCodegenOptionsIsTs,
    SharedTransformCodegenOptionsFilename,

    // compiler-core:CodegenOptions (7)
    CodegenOptionsMode,
    CodegenOptionsSourceMap,
    CodegenOptionsScopeId,
    CodegenOptionsOptimizeImports,
    CodegenOptionsRuntimeModuleName,
    CodegenOptionsSsrRuntimeModuleName,
    CodegenOptionsRuntimeGlobalName,

    // compiler-sfc:parse (7)
    ParseFilename,
    ParseSourceMap,
    ParseSourceRoot,
    ParsePad,
    ParseIgnoreEmpty,
    ParseCompiler,
    ParseTemplateParseOptions,

    // compiler-sfc:compileScript (13)
    CompileScriptId,
    CompileScriptIsProd,
    CompileScriptSourceMap,
    CompileScriptBabelParserPlugins,
    CompileScriptGlobalTypeFiles,
    CompileScriptInlineTemplate,
    CompileScriptGenDefaultAs,
    CompileScriptTemplateOptions,
    CompileScriptHoistStatic,
    CompileScriptPropsDestructure,
    CompileScriptFs,
    CompileScriptCustomElement,
    CompileScriptVapor,

    // compiler-sfc:compileTemplate (17)
    CompileTemplateSource,
    CompileTemplateAst,
    CompileTemplateFilename,
    CompileTemplateId,
    CompileTemplateScoped,
    CompileTemplateSlotted,
    CompileTemplateIsProd,
    CompileTemplateVapor,
    CompileTemplateSsr,
    CompileTemplateSsrCssVars,
    CompileTemplateInMap,
    CompileTemplateCompiler,
    CompileTemplateCompilerOptions,
    CompileTemplatePreprocessLang,
    CompileTemplatePreprocessOptions,
    CompileTemplatePreprocessCustomRequire,
    CompileTemplateTransformAssetUrls,

    // compiler-sfc:AssetURLOptions (3)
    AssetUrlOptionsBase,
    AssetUrlOptionsIncludeAbsolute,
    AssetUrlOptionsTags,

    // compiler-sfc:compileStyle (16)
    CompileStyleSource,
    CompileStyleFilename,
    CompileStyleId,
    CompileStyleScoped,
    CompileStyleTrim,
    CompileStyleIsProd,
    CompileStyleInMap,
    CompileStylePreprocessLang,
    CompileStylePreprocessOptions,
    CompileStylePreprocessCustomRequire,
    CompileStylePostcssOptions,
    CompileStylePostcssPlugins,
    CompileStyleMap,
    CompileStyleIsAsync,
    CompileStyleModules,
    CompileStyleModulesOptions,

    // compiler-sfc:CSSModulesOptions (6)
    CssModulesOptionsScopeBehaviour,
    CssModulesOptionsGenerateScopedName,
    CssModulesOptionsHashPrefix,
    CssModulesOptionsLocalsConvention,
    CssModulesOptionsExportGlobals,
    CssModulesOptionsGlobalModulePaths,
}

/// Where a `VueOption` row's semantics land — the closed vocabulary
/// `option-inventories.md:37-41` defines.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VueOptionClass {
    /// Caller-settable; carried on [`VueCompileRequest`] (directly, via
    /// [`VueOptionAttempt`], as-is on the specific canonical field named).
    SupportedCanonical,
    /// The canonical request computes this solely from other canonical
    /// fields; never itself a caller-settable slot.
    Derived,
    /// The host provides immutable normalized data; the canonical request
    /// validates compatibility but is not the option's source of truth.
    HostResolved,
    /// Passed only to a selected external preprocessor; never bundled into
    /// compiler-core semantics.
    External,
    /// Official-oracle-only; a production request must reject it.
    TestOnly,
    /// Fails request construction — no field represents it; presence is
    /// refused via [`VueOptionAttempt`].
    UnsupportedFailClosed,
    /// Meaningful only for an output mode/shape Verter does not publish;
    /// cannot widen the public product set.
    NotApplicable,
}

impl VueOption {
    /// Exhaustive: a new `vue-options.tsv` row without an arm here is a
    /// compile error, not a silent skip.
    pub const fn class(self) -> VueOptionClass {
        use VueOption::*;
        use VueOptionClass::*;
        match self {
            ParserOptionsOnWarn => Derived,
            ParserOptionsOnError => Derived,
            ParserOptionsCompatConfig => UnsupportedFailClosed,
            ParserOptionsCompatConfigMode => UnsupportedFailClosed,
            ParserOptionsCompatConfigCompilerIsOnElement => UnsupportedFailClosed,
            ParserOptionsCompatConfigCompilerVBindSync => UnsupportedFailClosed,
            ParserOptionsCompatConfigCompilerVIfVForPrecedence => UnsupportedFailClosed,
            ParserOptionsCompatConfigCompilerVBindObjectOrder => UnsupportedFailClosed,
            ParserOptionsCompatConfigCompilerVOnNative => UnsupportedFailClosed,
            ParserOptionsCompatConfigCompilerNativeTemplate => UnsupportedFailClosed,
            ParserOptionsCompatConfigCompilerInlineTemplate => UnsupportedFailClosed,
            ParserOptionsCompatConfigCompilerFilters => UnsupportedFailClosed,
            ParserOptionsParseMode => Derived,
            ParserOptionsNs => Derived,
            ParserOptionsIsNativeTag => Derived,
            ParserOptionsIsVoidTag => Derived,
            ParserOptionsIsPreTag => Derived,
            ParserOptionsIsIgnoreNewlineTag => Derived,
            ParserOptionsIsBuiltInComponent => Derived,
            ParserOptionsIsCustomElement => SupportedCanonical,
            ParserOptionsGetNamespace => Derived,
            ParserOptionsDelimiters => SupportedCanonical,
            ParserOptionsWhitespace => SupportedCanonical,
            ParserOptionsDecodeEntities => HostResolved,
            ParserOptionsComments => SupportedCanonical,
            ParserOptionsPrefixIdentifiers => Derived,
            ParserOptionsExpressionPlugins => HostResolved,

            TransformOptionsNodeTransforms => TestOnly,
            TransformOptionsDirectiveTransforms => TestOnly,
            TransformOptionsTransformHoist => Derived,
            TransformOptionsOnWarn => Derived,
            TransformOptionsOnError => Derived,
            TransformOptionsCompatConfig => UnsupportedFailClosed,
            TransformOptionsIsBuiltInComponent => Derived,
            TransformOptionsIsCustomElement => SupportedCanonical,
            TransformOptionsHoistStatic => SupportedCanonical,
            TransformOptionsCacheHandlers => SupportedCanonical,
            TransformOptionsScopeId => Derived,
            TransformOptionsSlotted => Derived,
            TransformOptionsSsrCssVars => Derived,
            TransformOptionsHmr => SupportedCanonical,

            SharedTransformCodegenOptionsPrefixIdentifiers => Derived,
            SharedTransformCodegenOptionsExpressionPlugins => HostResolved,
            SharedTransformCodegenOptionsSsr => Derived,
            SharedTransformCodegenOptionsInSsr => Derived,
            SharedTransformCodegenOptionsBindingMetadata => Derived,
            SharedTransformCodegenOptionsInline => Derived,
            SharedTransformCodegenOptionsIsTs => Derived,
            SharedTransformCodegenOptionsFilename => HostResolved,

            CodegenOptionsMode => UnsupportedFailClosed,
            CodegenOptionsSourceMap => Derived,
            CodegenOptionsScopeId => Derived,
            CodegenOptionsOptimizeImports => SupportedCanonical,
            CodegenOptionsRuntimeModuleName => SupportedCanonical,
            CodegenOptionsSsrRuntimeModuleName => SupportedCanonical,
            CodegenOptionsRuntimeGlobalName => NotApplicable,

            ParseFilename => HostResolved,
            ParseSourceMap => Derived,
            ParseSourceRoot => HostResolved,
            ParsePad => SupportedCanonical,
            ParseIgnoreEmpty => SupportedCanonical,
            ParseCompiler => TestOnly,
            ParseTemplateParseOptions => Derived,

            CompileScriptId => Derived,
            CompileScriptIsProd => Derived,
            CompileScriptSourceMap => Derived,
            CompileScriptBabelParserPlugins => SupportedCanonical,
            CompileScriptGlobalTypeFiles => HostResolved,
            CompileScriptInlineTemplate => Derived,
            CompileScriptGenDefaultAs => SupportedCanonical,
            CompileScriptTemplateOptions => Derived,
            CompileScriptHoistStatic => SupportedCanonical,
            CompileScriptPropsDestructure => SupportedCanonical,
            CompileScriptFs => HostResolved,
            CompileScriptCustomElement => SupportedCanonical,
            CompileScriptVapor => Derived,

            CompileTemplateSource => Derived,
            CompileTemplateAst => Derived,
            CompileTemplateFilename => HostResolved,
            CompileTemplateId => Derived,
            CompileTemplateScoped => Derived,
            CompileTemplateSlotted => Derived,
            CompileTemplateIsProd => Derived,
            CompileTemplateVapor => Derived,
            CompileTemplateSsr => Derived,
            CompileTemplateSsrCssVars => Derived,
            CompileTemplateInMap => Derived,
            CompileTemplateCompiler => TestOnly,
            CompileTemplateCompilerOptions => Derived,
            CompileTemplatePreprocessLang => External,
            CompileTemplatePreprocessOptions => External,
            CompileTemplatePreprocessCustomRequire => External,
            CompileTemplateTransformAssetUrls => SupportedCanonical,

            AssetUrlOptionsBase => SupportedCanonical,
            AssetUrlOptionsIncludeAbsolute => SupportedCanonical,
            AssetUrlOptionsTags => SupportedCanonical,

            CompileStyleSource => Derived,
            CompileStyleFilename => HostResolved,
            CompileStyleId => Derived,
            CompileStyleScoped => Derived,
            CompileStyleTrim => SupportedCanonical,
            CompileStyleIsProd => Derived,
            CompileStyleInMap => Derived,
            CompileStylePreprocessLang => External,
            CompileStylePreprocessOptions => External,
            CompileStylePreprocessCustomRequire => External,
            CompileStylePostcssOptions => External,
            CompileStylePostcssPlugins => External,
            CompileStyleMap => Derived,
            CompileStyleIsAsync => Derived,
            CompileStyleModules => SupportedCanonical,
            CompileStyleModulesOptions => SupportedCanonical,

            CssModulesOptionsScopeBehaviour => SupportedCanonical,
            CssModulesOptionsGenerateScopedName => HostResolved,
            CssModulesOptionsHashPrefix => SupportedCanonical,
            CssModulesOptionsLocalsConvention => SupportedCanonical,
            CssModulesOptionsExportGlobals => SupportedCanonical,
            CssModulesOptionsGlobalModulePaths => HostResolved,
        }
    }

    /// The exact (`surface`, `option`) column pair of this row in
    /// `packages/framework-conformance-harness/evidence/vue-options.tsv` —
    /// the schema identity a refusal names, never a spelling derived from
    /// the Rust variant. Exhaustive for the same reason [`Self::class`] is: a
    /// new TSV row without an arm here is a compile error.
    pub const fn tsv_row(self) -> (&'static str, &'static str) {
        use VueOption::*;
        match self {
            ParserOptionsOnWarn => ("compiler-core:ParserOptions", "onWarn"),
            ParserOptionsOnError => ("compiler-core:ParserOptions", "onError"),
            ParserOptionsCompatConfig => ("compiler-core:ParserOptions", "compatConfig"),
            ParserOptionsCompatConfigMode => ("compiler-core:ParserOptions", "compatConfig.MODE"),
            ParserOptionsCompatConfigCompilerIsOnElement => (
                "compiler-core:ParserOptions",
                "compatConfig.COMPILER_IS_ON_ELEMENT",
            ),
            ParserOptionsCompatConfigCompilerVBindSync => (
                "compiler-core:ParserOptions",
                "compatConfig.COMPILER_V_BIND_SYNC",
            ),
            ParserOptionsCompatConfigCompilerVIfVForPrecedence => (
                "compiler-core:ParserOptions",
                "compatConfig.COMPILER_V_IF_V_FOR_PRECEDENCE",
            ),
            ParserOptionsCompatConfigCompilerVBindObjectOrder => (
                "compiler-core:ParserOptions",
                "compatConfig.COMPILER_V_BIND_OBJECT_ORDER",
            ),
            ParserOptionsCompatConfigCompilerVOnNative => (
                "compiler-core:ParserOptions",
                "compatConfig.COMPILER_V_ON_NATIVE",
            ),
            ParserOptionsCompatConfigCompilerNativeTemplate => (
                "compiler-core:ParserOptions",
                "compatConfig.COMPILER_NATIVE_TEMPLATE",
            ),
            ParserOptionsCompatConfigCompilerInlineTemplate => (
                "compiler-core:ParserOptions",
                "compatConfig.COMPILER_INLINE_TEMPLATE",
            ),
            ParserOptionsCompatConfigCompilerFilters => (
                "compiler-core:ParserOptions",
                "compatConfig.COMPILER_FILTERS",
            ),
            ParserOptionsParseMode => ("compiler-core:ParserOptions", "parseMode"),
            ParserOptionsNs => ("compiler-core:ParserOptions", "ns"),
            ParserOptionsIsNativeTag => ("compiler-core:ParserOptions", "isNativeTag"),
            ParserOptionsIsVoidTag => ("compiler-core:ParserOptions", "isVoidTag"),
            ParserOptionsIsPreTag => ("compiler-core:ParserOptions", "isPreTag"),
            ParserOptionsIsIgnoreNewlineTag => {
                ("compiler-core:ParserOptions", "isIgnoreNewlineTag")
            }
            ParserOptionsIsBuiltInComponent => {
                ("compiler-core:ParserOptions", "isBuiltInComponent")
            }
            ParserOptionsIsCustomElement => ("compiler-core:ParserOptions", "isCustomElement"),
            ParserOptionsGetNamespace => ("compiler-core:ParserOptions", "getNamespace"),
            ParserOptionsDelimiters => ("compiler-core:ParserOptions", "delimiters"),
            ParserOptionsWhitespace => ("compiler-core:ParserOptions", "whitespace"),
            ParserOptionsDecodeEntities => ("compiler-core:ParserOptions", "decodeEntities"),
            ParserOptionsComments => ("compiler-core:ParserOptions", "comments"),
            ParserOptionsPrefixIdentifiers => ("compiler-core:ParserOptions", "prefixIdentifiers"),
            ParserOptionsExpressionPlugins => ("compiler-core:ParserOptions", "expressionPlugins"),
            TransformOptionsNodeTransforms => ("compiler-core:TransformOptions", "nodeTransforms"),
            TransformOptionsDirectiveTransforms => {
                ("compiler-core:TransformOptions", "directiveTransforms")
            }
            TransformOptionsTransformHoist => ("compiler-core:TransformOptions", "transformHoist"),
            TransformOptionsOnWarn => ("compiler-core:TransformOptions", "onWarn"),
            TransformOptionsOnError => ("compiler-core:TransformOptions", "onError"),
            TransformOptionsCompatConfig => ("compiler-core:TransformOptions", "compatConfig"),
            TransformOptionsIsBuiltInComponent => {
                ("compiler-core:TransformOptions", "isBuiltInComponent")
            }
            TransformOptionsIsCustomElement => {
                ("compiler-core:TransformOptions", "isCustomElement")
            }
            TransformOptionsHoistStatic => ("compiler-core:TransformOptions", "hoistStatic"),
            TransformOptionsCacheHandlers => ("compiler-core:TransformOptions", "cacheHandlers"),
            TransformOptionsScopeId => ("compiler-core:TransformOptions", "scopeId"),
            TransformOptionsSlotted => ("compiler-core:TransformOptions", "slotted"),
            TransformOptionsSsrCssVars => ("compiler-core:TransformOptions", "ssrCssVars"),
            TransformOptionsHmr => ("compiler-core:TransformOptions", "hmr"),
            SharedTransformCodegenOptionsPrefixIdentifiers => (
                "compiler-core:SharedTransformCodegenOptions",
                "prefixIdentifiers",
            ),
            SharedTransformCodegenOptionsExpressionPlugins => (
                "compiler-core:SharedTransformCodegenOptions",
                "expressionPlugins",
            ),
            SharedTransformCodegenOptionsSsr => {
                ("compiler-core:SharedTransformCodegenOptions", "ssr")
            }
            SharedTransformCodegenOptionsInSsr => {
                ("compiler-core:SharedTransformCodegenOptions", "inSSR")
            }
            SharedTransformCodegenOptionsBindingMetadata => (
                "compiler-core:SharedTransformCodegenOptions",
                "bindingMetadata",
            ),
            SharedTransformCodegenOptionsInline => {
                ("compiler-core:SharedTransformCodegenOptions", "inline")
            }
            SharedTransformCodegenOptionsIsTs => {
                ("compiler-core:SharedTransformCodegenOptions", "isTS")
            }
            SharedTransformCodegenOptionsFilename => {
                ("compiler-core:SharedTransformCodegenOptions", "filename")
            }
            CodegenOptionsMode => ("compiler-core:CodegenOptions", "mode"),
            CodegenOptionsSourceMap => ("compiler-core:CodegenOptions", "sourceMap"),
            CodegenOptionsScopeId => ("compiler-core:CodegenOptions", "scopeId"),
            CodegenOptionsOptimizeImports => ("compiler-core:CodegenOptions", "optimizeImports"),
            CodegenOptionsRuntimeModuleName => {
                ("compiler-core:CodegenOptions", "runtimeModuleName")
            }
            CodegenOptionsSsrRuntimeModuleName => {
                ("compiler-core:CodegenOptions", "ssrRuntimeModuleName")
            }
            CodegenOptionsRuntimeGlobalName => {
                ("compiler-core:CodegenOptions", "runtimeGlobalName")
            }
            ParseFilename => ("compiler-sfc:parse", "filename"),
            ParseSourceMap => ("compiler-sfc:parse", "sourceMap"),
            ParseSourceRoot => ("compiler-sfc:parse", "sourceRoot"),
            ParsePad => ("compiler-sfc:parse", "pad"),
            ParseIgnoreEmpty => ("compiler-sfc:parse", "ignoreEmpty"),
            ParseCompiler => ("compiler-sfc:parse", "compiler"),
            ParseTemplateParseOptions => ("compiler-sfc:parse", "templateParseOptions"),
            CompileScriptId => ("compiler-sfc:compileScript", "id"),
            CompileScriptIsProd => ("compiler-sfc:compileScript", "isProd"),
            CompileScriptSourceMap => ("compiler-sfc:compileScript", "sourceMap"),
            CompileScriptBabelParserPlugins => ("compiler-sfc:compileScript", "babelParserPlugins"),
            CompileScriptGlobalTypeFiles => ("compiler-sfc:compileScript", "globalTypeFiles"),
            CompileScriptInlineTemplate => ("compiler-sfc:compileScript", "inlineTemplate"),
            CompileScriptGenDefaultAs => ("compiler-sfc:compileScript", "genDefaultAs"),
            CompileScriptTemplateOptions => ("compiler-sfc:compileScript", "templateOptions"),
            CompileScriptHoistStatic => ("compiler-sfc:compileScript", "hoistStatic"),
            CompileScriptPropsDestructure => ("compiler-sfc:compileScript", "propsDestructure"),
            CompileScriptFs => ("compiler-sfc:compileScript", "fs"),
            CompileScriptCustomElement => ("compiler-sfc:compileScript", "customElement"),
            CompileScriptVapor => ("compiler-sfc:compileScript", "vapor"),
            CompileTemplateSource => ("compiler-sfc:compileTemplate", "source"),
            CompileTemplateAst => ("compiler-sfc:compileTemplate", "ast"),
            CompileTemplateFilename => ("compiler-sfc:compileTemplate", "filename"),
            CompileTemplateId => ("compiler-sfc:compileTemplate", "id"),
            CompileTemplateScoped => ("compiler-sfc:compileTemplate", "scoped"),
            CompileTemplateSlotted => ("compiler-sfc:compileTemplate", "slotted"),
            CompileTemplateIsProd => ("compiler-sfc:compileTemplate", "isProd"),
            CompileTemplateVapor => ("compiler-sfc:compileTemplate", "vapor"),
            CompileTemplateSsr => ("compiler-sfc:compileTemplate", "ssr"),
            CompileTemplateSsrCssVars => ("compiler-sfc:compileTemplate", "ssrCssVars"),
            CompileTemplateInMap => ("compiler-sfc:compileTemplate", "inMap"),
            CompileTemplateCompiler => ("compiler-sfc:compileTemplate", "compiler"),
            CompileTemplateCompilerOptions => ("compiler-sfc:compileTemplate", "compilerOptions"),
            CompileTemplatePreprocessLang => ("compiler-sfc:compileTemplate", "preprocessLang"),
            CompileTemplatePreprocessOptions => {
                ("compiler-sfc:compileTemplate", "preprocessOptions")
            }
            CompileTemplatePreprocessCustomRequire => {
                ("compiler-sfc:compileTemplate", "preprocessCustomRequire")
            }
            CompileTemplateTransformAssetUrls => {
                ("compiler-sfc:compileTemplate", "transformAssetUrls")
            }
            AssetUrlOptionsBase => ("compiler-sfc:AssetURLOptions", "base"),
            AssetUrlOptionsIncludeAbsolute => ("compiler-sfc:AssetURLOptions", "includeAbsolute"),
            AssetUrlOptionsTags => ("compiler-sfc:AssetURLOptions", "tags"),
            CompileStyleSource => ("compiler-sfc:compileStyle", "source"),
            CompileStyleFilename => ("compiler-sfc:compileStyle", "filename"),
            CompileStyleId => ("compiler-sfc:compileStyle", "id"),
            CompileStyleScoped => ("compiler-sfc:compileStyle", "scoped"),
            CompileStyleTrim => ("compiler-sfc:compileStyle", "trim"),
            CompileStyleIsProd => ("compiler-sfc:compileStyle", "isProd"),
            CompileStyleInMap => ("compiler-sfc:compileStyle", "inMap"),
            CompileStylePreprocessLang => ("compiler-sfc:compileStyle", "preprocessLang"),
            CompileStylePreprocessOptions => ("compiler-sfc:compileStyle", "preprocessOptions"),
            CompileStylePreprocessCustomRequire => {
                ("compiler-sfc:compileStyle", "preprocessCustomRequire")
            }
            CompileStylePostcssOptions => ("compiler-sfc:compileStyle", "postcssOptions"),
            CompileStylePostcssPlugins => ("compiler-sfc:compileStyle", "postcssPlugins"),
            CompileStyleMap => ("compiler-sfc:compileStyle", "map"),
            CompileStyleIsAsync => ("compiler-sfc:compileStyle", "isAsync"),
            CompileStyleModules => ("compiler-sfc:compileStyle", "modules"),
            CompileStyleModulesOptions => ("compiler-sfc:compileStyle", "modulesOptions"),
            CssModulesOptionsScopeBehaviour => ("compiler-sfc:CSSModulesOptions", "scopeBehaviour"),
            CssModulesOptionsGenerateScopedName => {
                ("compiler-sfc:CSSModulesOptions", "generateScopedName")
            }
            CssModulesOptionsHashPrefix => ("compiler-sfc:CSSModulesOptions", "hashPrefix"),
            CssModulesOptionsLocalsConvention => {
                ("compiler-sfc:CSSModulesOptions", "localsConvention")
            }
            CssModulesOptionsExportGlobals => ("compiler-sfc:CSSModulesOptions", "exportGlobals"),
            CssModulesOptionsGlobalModulePaths => {
                ("compiler-sfc:CSSModulesOptions", "globalModulePaths")
            }
        }
    }

    /// The host compile request's own slot for this option, as
    /// `packages/native/host-compile-request.generated.ts` declares it —
    /// the field path a caller actually writes, and the path a refusal
    /// names.
    ///
    /// `None` means the request carries no slot for the row at all: the
    /// option is derived, host-resolved, preprocessor-external,
    /// oracle-only, or not applicable to any published product, so no
    /// caller can have written it. Every option a
    /// [`crate::compile_request::CompileRequestError`] names answers
    /// `Some`.
    ///
    /// This is deliberately NOT [`Self::tsv_row`]. The inventory describes
    /// the OFFICIAL framework's option surfaces
    /// (`compiler-core:ParserOptions` + `compatConfig.MODE`); the host
    /// request is a FLAT camelCase object with one slot per admitted or
    /// explicitly refused row (`compatConfigMode`). Naming the offending
    /// property from the inventory would name a field the caller's request
    /// object does not have, and would collapse the two distinct
    /// `compatConfig` slots — `compatConfig` and `transformCompatConfig` —
    /// onto one path.
    ///
    /// Two rows may legitimately share a slot: `isCustomElement` and
    /// `hoistStatic` are each inventoried on two surfaces and fold onto
    /// one canonical field, which is the exactly-once-per-ROW rule
    /// [`Self::class`] documents.
    ///
    /// Exhaustive for the same reason [`Self::class`] is: a new
    /// `vue-options.tsv` row without an arm here is a compile error, not a
    /// silently pathless refusal.
    pub const fn request_field(self) -> Option<&'static str> {
        use VueOption::*;
        match self {
            ParserOptionsIsCustomElement | TransformOptionsIsCustomElement => {
                Some("isCustomElement")
            }
            ParserOptionsDelimiters => Some("delimiters"),
            ParserOptionsWhitespace => Some("whitespace"),
            ParserOptionsComments => Some("comments"),
            ParserOptionsCompatConfig => Some("compatConfig"),
            ParserOptionsCompatConfigMode => Some("compatConfigMode"),
            ParserOptionsCompatConfigCompilerIsOnElement => Some("compatConfigCompilerIsOnElement"),
            ParserOptionsCompatConfigCompilerVBindSync => Some("compatConfigCompilerVBindSync"),
            ParserOptionsCompatConfigCompilerVIfVForPrecedence => {
                Some("compatConfigCompilerVIfVForPrecedence")
            }
            ParserOptionsCompatConfigCompilerVBindObjectOrder => {
                Some("compatConfigCompilerVBindObjectOrder")
            }
            ParserOptionsCompatConfigCompilerVOnNative => Some("compatConfigCompilerVOnNative"),
            ParserOptionsCompatConfigCompilerNativeTemplate => {
                Some("compatConfigCompilerNativeTemplate")
            }
            ParserOptionsCompatConfigCompilerInlineTemplate => {
                Some("compatConfigCompilerInlineTemplate")
            }
            ParserOptionsCompatConfigCompilerFilters => Some("compatConfigCompilerFilters"),
            TransformOptionsCompatConfig => Some("transformCompatConfig"),
            TransformOptionsHoistStatic | CompileScriptHoistStatic => Some("hoistStatic"),
            TransformOptionsCacheHandlers => Some("cacheHandlers"),
            TransformOptionsHmr => Some("hmr"),
            SharedTransformCodegenOptionsSsr => Some("ssr"),
            CodegenOptionsMode => Some("codegenMode"),
            CodegenOptionsOptimizeImports => Some("optimizeImports"),
            CodegenOptionsRuntimeModuleName => Some("runtimeModuleName"),
            CodegenOptionsSsrRuntimeModuleName => Some("ssrRuntimeModuleName"),
            ParsePad => Some("parsePad"),
            ParseIgnoreEmpty => Some("ignoreEmpty"),
            CompileScriptBabelParserPlugins => Some("babelParserPlugins"),
            CompileScriptGenDefaultAs => Some("genDefaultAs"),
            CompileScriptPropsDestructure => Some("propsDestructure"),
            CompileScriptCustomElement => Some("scriptCustomElement"),
            CompileTemplateTransformAssetUrls => Some("transformAssetUrls"),
            AssetUrlOptionsBase => Some("transformAssetUrls.enabled.base"),
            AssetUrlOptionsIncludeAbsolute => Some("transformAssetUrls.enabled.includeAbsolute"),
            AssetUrlOptionsTags => Some("transformAssetUrls.enabled.tags"),
            CompileStyleTrim => Some("styleTrim"),
            CompileStyleModules | CompileStyleModulesOptions => Some("cssModules"),
            CssModulesOptionsScopeBehaviour => Some("cssModules.scopeBehaviour"),
            CssModulesOptionsHashPrefix => Some("cssModules.hashPrefix"),
            CssModulesOptionsLocalsConvention => Some("cssModules.localsConvention"),
            CssModulesOptionsExportGlobals => Some("cssModules.exportGlobals"),

            // No slot: the request derives these, resolves them from the
            // host, forwards them to an external preprocessor, or does not
            // publish the output shape they apply to.
            ParserOptionsOnWarn
            | ParserOptionsOnError
            | ParserOptionsParseMode
            | ParserOptionsNs
            | ParserOptionsIsNativeTag
            | ParserOptionsIsVoidTag
            | ParserOptionsIsPreTag
            | ParserOptionsIsIgnoreNewlineTag
            | ParserOptionsIsBuiltInComponent
            | ParserOptionsGetNamespace
            | ParserOptionsDecodeEntities
            | ParserOptionsPrefixIdentifiers
            | ParserOptionsExpressionPlugins
            | TransformOptionsNodeTransforms
            | TransformOptionsDirectiveTransforms
            | TransformOptionsTransformHoist
            | TransformOptionsOnWarn
            | TransformOptionsOnError
            | TransformOptionsIsBuiltInComponent
            | TransformOptionsScopeId
            | TransformOptionsSlotted
            | TransformOptionsSsrCssVars
            | SharedTransformCodegenOptionsPrefixIdentifiers
            | SharedTransformCodegenOptionsExpressionPlugins
            | SharedTransformCodegenOptionsInSsr
            | SharedTransformCodegenOptionsBindingMetadata
            | SharedTransformCodegenOptionsInline
            | SharedTransformCodegenOptionsIsTs
            | SharedTransformCodegenOptionsFilename
            | CodegenOptionsSourceMap
            | CodegenOptionsScopeId
            | CodegenOptionsRuntimeGlobalName
            | ParseFilename
            | ParseSourceMap
            | ParseSourceRoot
            | ParseCompiler
            | ParseTemplateParseOptions
            | CompileScriptId
            | CompileScriptIsProd
            | CompileScriptSourceMap
            | CompileScriptGlobalTypeFiles
            | CompileScriptInlineTemplate
            | CompileScriptTemplateOptions
            | CompileScriptFs
            | CompileScriptVapor
            | CompileTemplateSource
            | CompileTemplateAst
            | CompileTemplateFilename
            | CompileTemplateId
            | CompileTemplateScoped
            | CompileTemplateSlotted
            | CompileTemplateIsProd
            | CompileTemplateVapor
            | CompileTemplateSsr
            | CompileTemplateSsrCssVars
            | CompileTemplateInMap
            | CompileTemplateCompiler
            | CompileTemplateCompilerOptions
            | CompileTemplatePreprocessLang
            | CompileTemplatePreprocessOptions
            | CompileTemplatePreprocessCustomRequire
            | CompileStyleSource
            | CompileStyleFilename
            | CompileStyleId
            | CompileStyleScoped
            | CompileStyleIsProd
            | CompileStyleInMap
            | CompileStylePreprocessLang
            | CompileStylePreprocessOptions
            | CompileStylePreprocessCustomRequire
            | CompileStylePostcssOptions
            | CompileStylePostcssPlugins
            | CompileStyleMap
            | CompileStyleIsAsync
            | CssModulesOptionsGenerateScopedName
            | CssModulesOptionsGlobalModulePaths => None,
        }
    }
}

/// The 118 rows, for exhaustiveness/count tests. Kept as a `const` array
/// rather than a `strum`-style derive (no such dependency here) — the
/// exhaustiveness proof itself lives in [`VueOption::class`]'s match, not
/// in this list; this list exists only so a test can iterate.
pub const ALL_VUE_OPTIONS: [VueOption; 118] = {
    use VueOption::*;
    [
        ParserOptionsOnWarn,
        ParserOptionsOnError,
        ParserOptionsCompatConfig,
        ParserOptionsCompatConfigMode,
        ParserOptionsCompatConfigCompilerIsOnElement,
        ParserOptionsCompatConfigCompilerVBindSync,
        ParserOptionsCompatConfigCompilerVIfVForPrecedence,
        ParserOptionsCompatConfigCompilerVBindObjectOrder,
        ParserOptionsCompatConfigCompilerVOnNative,
        ParserOptionsCompatConfigCompilerNativeTemplate,
        ParserOptionsCompatConfigCompilerInlineTemplate,
        ParserOptionsCompatConfigCompilerFilters,
        ParserOptionsParseMode,
        ParserOptionsNs,
        ParserOptionsIsNativeTag,
        ParserOptionsIsVoidTag,
        ParserOptionsIsPreTag,
        ParserOptionsIsIgnoreNewlineTag,
        ParserOptionsIsBuiltInComponent,
        ParserOptionsIsCustomElement,
        ParserOptionsGetNamespace,
        ParserOptionsDelimiters,
        ParserOptionsWhitespace,
        ParserOptionsDecodeEntities,
        ParserOptionsComments,
        ParserOptionsPrefixIdentifiers,
        ParserOptionsExpressionPlugins,
        TransformOptionsNodeTransforms,
        TransformOptionsDirectiveTransforms,
        TransformOptionsTransformHoist,
        TransformOptionsOnWarn,
        TransformOptionsOnError,
        TransformOptionsCompatConfig,
        TransformOptionsIsBuiltInComponent,
        TransformOptionsIsCustomElement,
        TransformOptionsHoistStatic,
        TransformOptionsCacheHandlers,
        TransformOptionsScopeId,
        TransformOptionsSlotted,
        TransformOptionsSsrCssVars,
        TransformOptionsHmr,
        SharedTransformCodegenOptionsPrefixIdentifiers,
        SharedTransformCodegenOptionsExpressionPlugins,
        SharedTransformCodegenOptionsSsr,
        SharedTransformCodegenOptionsInSsr,
        SharedTransformCodegenOptionsBindingMetadata,
        SharedTransformCodegenOptionsInline,
        SharedTransformCodegenOptionsIsTs,
        SharedTransformCodegenOptionsFilename,
        CodegenOptionsMode,
        CodegenOptionsSourceMap,
        CodegenOptionsScopeId,
        CodegenOptionsOptimizeImports,
        CodegenOptionsRuntimeModuleName,
        CodegenOptionsSsrRuntimeModuleName,
        CodegenOptionsRuntimeGlobalName,
        ParseFilename,
        ParseSourceMap,
        ParseSourceRoot,
        ParsePad,
        ParseIgnoreEmpty,
        ParseCompiler,
        ParseTemplateParseOptions,
        CompileScriptId,
        CompileScriptIsProd,
        CompileScriptSourceMap,
        CompileScriptBabelParserPlugins,
        CompileScriptGlobalTypeFiles,
        CompileScriptInlineTemplate,
        CompileScriptGenDefaultAs,
        CompileScriptTemplateOptions,
        CompileScriptHoistStatic,
        CompileScriptPropsDestructure,
        CompileScriptFs,
        CompileScriptCustomElement,
        CompileScriptVapor,
        CompileTemplateSource,
        CompileTemplateAst,
        CompileTemplateFilename,
        CompileTemplateId,
        CompileTemplateScoped,
        CompileTemplateSlotted,
        CompileTemplateIsProd,
        CompileTemplateVapor,
        CompileTemplateSsr,
        CompileTemplateSsrCssVars,
        CompileTemplateInMap,
        CompileTemplateCompiler,
        CompileTemplateCompilerOptions,
        CompileTemplatePreprocessLang,
        CompileTemplatePreprocessOptions,
        CompileTemplatePreprocessCustomRequire,
        CompileTemplateTransformAssetUrls,
        AssetUrlOptionsBase,
        AssetUrlOptionsIncludeAbsolute,
        AssetUrlOptionsTags,
        CompileStyleSource,
        CompileStyleFilename,
        CompileStyleId,
        CompileStyleScoped,
        CompileStyleTrim,
        CompileStyleIsProd,
        CompileStyleInMap,
        CompileStylePreprocessLang,
        CompileStylePreprocessOptions,
        CompileStylePreprocessCustomRequire,
        CompileStylePostcssOptions,
        CompileStylePostcssPlugins,
        CompileStyleMap,
        CompileStyleIsAsync,
        CompileStyleModules,
        CompileStyleModulesOptions,
        CssModulesOptionsScopeBehaviour,
        CssModulesOptionsGenerateScopedName,
        CssModulesOptionsHashPrefix,
        CssModulesOptionsLocalsConvention,
        CssModulesOptionsExportGlobals,
        CssModulesOptionsGlobalModulePaths,
    ]
};

/// Whether one [`VueOptionAttempt`] field was supplied at all.
type VuePresenceProbe = fn(&VueOptionAttempt) -> bool;

/// Every Vue option [`VueOptionAttempt::into_request`] refuses on
/// PRESENCE, in the deterministic order it checks them — each paired ON
/// ITS OWN LINE with the probe that decides it.
///
/// Identity and probe are one row rather than two lists read by a shared
/// index, so a slot inserted, removed or reordered moves both halves at
/// once. The failure this arrangement makes unrepresentable is a refusal
/// telling a caller to remove a field they never wrote, which is what a
/// desynced pair would produce.
const PRESENCE_REFUSED_VUE_SLOTS: [(VueOption, VuePresenceProbe); 12] = {
    use VueOption::*;
    [
        (ParserOptionsCompatConfig, |attempt| {
            attempt.compat_config.is_some()
        }),
        (ParserOptionsCompatConfigMode, |attempt| {
            attempt.compat_config_mode.is_some()
        }),
        (ParserOptionsCompatConfigCompilerIsOnElement, |attempt| {
            attempt.compat_config_compiler_is_on_element.is_some()
        }),
        (ParserOptionsCompatConfigCompilerVBindSync, |attempt| {
            attempt.compat_config_compiler_v_bind_sync.is_some()
        }),
        (
            ParserOptionsCompatConfigCompilerVIfVForPrecedence,
            |attempt| {
                attempt
                    .compat_config_compiler_v_if_v_for_precedence
                    .is_some()
            },
        ),
        (ParserOptionsCompatConfigCompilerVBindObjectOrder, |attempt| {
            attempt.compat_config_compiler_v_bind_object_order.is_some()
        }),
        (ParserOptionsCompatConfigCompilerVOnNative, |attempt| {
            attempt.compat_config_compiler_v_on_native.is_some()
        }),
        (ParserOptionsCompatConfigCompilerNativeTemplate, |attempt| {
            attempt.compat_config_compiler_native_template.is_some()
        }),
        (ParserOptionsCompatConfigCompilerInlineTemplate, |attempt| {
            attempt.compat_config_compiler_inline_template.is_some()
        }),
        (ParserOptionsCompatConfigCompilerFilters, |attempt| {
            attempt.compat_config_compiler_filters.is_some()
        }),
        (TransformOptionsCompatConfig, |attempt| {
            attempt.transform_compat_config.is_some()
        }),
        (CodegenOptionsMode, |attempt| attempt.codegen_mode.is_some()),
    ]
};

/// The option identities of [`PRESENCE_REFUSED_VUE_SLOTS`], projected for
/// consumers that need the refusable SET without the probes.
///
/// Derived, not restated: a test iterating this const iterates exactly the
/// options a presence refusal can name, and adding a slot cannot leave it
/// behind.
pub const PRESENCE_REFUSED_VUE_OPTIONS: [VueOption; PRESENCE_REFUSED_VUE_SLOTS.len()] = {
    let mut options = [VueOption::ParserOptionsCompatConfig; PRESENCE_REFUSED_VUE_SLOTS.len()];
    let mut index = 0;
    while index < PRESENCE_REFUSED_VUE_SLOTS.len() {
        options[index] = PRESENCE_REFUSED_VUE_SLOTS[index].0;
        index += 1;
    }
    options
};

/// Every Vue option a [`crate::compile_request::CompileRequestError`] can
/// name for a caller's VALUE rather than for the option's presence.
///
/// One row today: a `delimiters` array whose arity is not exactly two is
/// refused at the FFI decode boundary
/// (`verter_ffi::convert::input::vue_delimiter_pair`) rather than falling
/// back to the framework's own delimiters.
pub const VALUE_REFUSED_VUE_OPTIONS: [VueOption; 1] = [VueOption::ParserOptionsDelimiters];

// ───────────────────────────── canonical request ─────────────────────────

/// Preserve whitespace vs condense it — mirrors the compiler's existing
/// `WhitespaceStrategy` semantics (kept as its own type here so this module
/// has no dependency on `crate::compile::types`, which it is replacing).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VueWhitespaceStrategy {
    Preserve,
    Condense,
}

/// Which Vue client codegen backend a `RuntimeClient` product resolves to.
/// `Inferred` defers to the parsed source's own `<template vapor>` marker —
/// resolved by [`crate::compile_request::CompileRequest::resolve_vue_backend`]
/// after parsing, since backend inference needs the parsed AST and request
/// construction runs before parsing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum VueBackendRequest {
    #[default]
    Inferred,
    Vdom,
    Vapor,
}

/// SFC padding strategy for lines before the first script/template block.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VueParsePad {
    Space,
    Line,
    Off,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct VueAssetUrlOptions {
    pub base: Option<String>,
    pub include_absolute: Option<bool>,
    pub tags: BTreeMap<String, Vec<String>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VueAssetUrlTransform {
    Disabled,
    Enabled(VueAssetUrlOptions),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VueCssModuleScopeBehaviour {
    Local,
    Global,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VueCssModuleLocalsConvention {
    CamelCase,
    CamelCaseOnly,
    Dashes,
    DashesOnly,
    AsIs,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct VueCssModulesOptions {
    pub scope_behaviour: Option<VueCssModuleScopeBehaviour>,
    pub hash_prefix: Option<String>,
    pub locals_convention: Option<VueCssModuleLocalsConvention>,
    pub export_globals: Option<bool>,
}

/// The canonical, exhaustively-classified Vue portion of a compile request.
/// Every field here corresponds to exactly one (or, for the two
/// documented same-canonical-field pairs — `isCustomElement`,
/// `hoistStatic` — exactly two) `supported canonical` row of
/// `vue-options.tsv`. There is no field for any `unsupported fail-closed`
/// row: those are structurally unrepresentable here — the type itself is
/// the fail-closed-by-construction proof, and [`VueOptionAttempt`] is the
/// typed refusal surface a transport-boundary decoder uses when a caller
/// still supplies the legacy option shape.
#[derive(Debug, Clone, Default)]
pub struct VueCompileRequest {
    pub backend: VueBackendRequest,
    pub ssr: bool,
    /// `ParserOptions.isCustomElement` / `TransformOptions.isCustomElement`
    /// — the canonical template-tag matcher; distinct from
    /// `script_custom_element` (trap: two different `customElement` axes).
    pub is_custom_element: Vec<String>,
    pub delimiters: Option<(String, String)>,
    pub whitespace: Option<VueWhitespaceStrategy>,
    pub comments: Option<bool>,
    pub hoist_static: Option<bool>,
    pub cache_handlers: Option<bool>,
    pub hmr: Option<bool>,
    pub optimize_imports: Option<bool>,
    pub runtime_module_name: Option<String>,
    pub ssr_runtime_module_name: Option<String>,
    pub parse_pad: Option<VueParsePad>,
    pub ignore_empty: Option<bool>,
    pub babel_parser_plugins: Vec<String>,
    pub gen_default_as: Option<String>,
    pub props_destructure: Option<bool>,
    /// `compileScript.customElement` — the script runtime-prop policy axis;
    /// distinct from `is_custom_element` (template-tag matcher).
    pub script_custom_element: Option<bool>,
    pub transform_asset_urls: Option<VueAssetUrlTransform>,
    pub style_trim: Option<bool>,
    pub css_modules: Option<VueCssModulesOptions>,
}

impl VueBackendRequest {
    pub const fn is_vapor(self) -> bool {
        matches!(self, VueBackendRequest::Vapor)
    }
}

/// Every field a legacy/transport-facing caller might still try to set for
/// Vue — the 29 `supported canonical` rows (folded to 27 slots) plus the 12
/// `unsupported fail-closed` rows. Exists ONLY as the typed refusal/decode
/// surface a transport boundary (NAPI/FFI/session) builds from raw input
/// before calling [`Self::into_request`] — never itself a second option
/// authority read by any downstream compiler stage. `Some(_)` on an
/// unsupported-shaped field means "the caller supplied this option",
/// refused regardless of the inner value (including an explicit `false`).
#[derive(Debug, Clone, Default)]
pub struct VueOptionAttempt {
    pub backend: VueBackendRequest,
    pub ssr: bool,
    pub is_custom_element: Vec<String>,
    pub delimiters: Option<(String, String)>,
    pub whitespace: Option<VueWhitespaceStrategy>,
    pub comments: Option<bool>,
    pub hoist_static: Option<bool>,
    pub cache_handlers: Option<bool>,
    pub hmr: Option<bool>,
    pub optimize_imports: Option<bool>,
    pub runtime_module_name: Option<String>,
    pub ssr_runtime_module_name: Option<String>,
    pub parse_pad: Option<VueParsePad>,
    pub ignore_empty: Option<bool>,
    pub babel_parser_plugins: Vec<String>,
    pub gen_default_as: Option<String>,
    pub props_destructure: Option<bool>,
    pub script_custom_element: Option<bool>,
    pub transform_asset_urls: Option<VueAssetUrlTransform>,
    pub style_trim: Option<bool>,
    pub css_modules: Option<VueCssModulesOptions>,

    // The 12 unsupported-fail-closed slots. `Some(true)` OR `Some(false)`
    // both mean "the caller supplied this option" — presence is what is
    // refused, even an explicit `false`.
    pub compat_config: Option<bool>,
    pub compat_config_mode: Option<bool>,
    pub compat_config_compiler_is_on_element: Option<bool>,
    pub compat_config_compiler_v_bind_sync: Option<bool>,
    pub compat_config_compiler_v_if_v_for_precedence: Option<bool>,
    pub compat_config_compiler_v_bind_object_order: Option<bool>,
    pub compat_config_compiler_v_on_native: Option<bool>,
    pub compat_config_compiler_native_template: Option<bool>,
    pub compat_config_compiler_inline_template: Option<bool>,
    pub compat_config_compiler_filters: Option<bool>,
    /// `TransformOptions.compatConfig` — inherits the SAME complete
    /// refusal as `ParserOptions.compatConfig` (trap #3): a separate flag
    /// so a caller who supplies it only on the transform surface is still
    /// caught.
    pub transform_compat_config: Option<bool>,
    pub codegen_mode: Option<bool>,
}

impl VueOptionAttempt {
    /// Converts this attempt into the canonical [`VueCompileRequest`],
    /// refusing on the first unsupported field present (deterministic
    /// declaration order) via
    /// [`crate::compile_request::CompileRequestError::UnsupportedOption`].
    pub fn into_request(
        self,
    ) -> Result<VueCompileRequest, crate::compile_request::CompileRequestError> {
        for (option, is_present) in PRESENCE_REFUSED_VUE_SLOTS {
            if is_present(&self) {
                return Err(
                    crate::compile_request::CompileRequestError::UnsupportedOption {
                        option: crate::compile_request::FrameworkOption::Vue(option),
                        capability: None,
                    },
                );
            }
        }
        Ok(VueCompileRequest {
            backend: self.backend,
            ssr: self.ssr,
            is_custom_element: self.is_custom_element,
            delimiters: self.delimiters,
            whitespace: self.whitespace,
            comments: self.comments,
            hoist_static: self.hoist_static,
            cache_handlers: self.cache_handlers,
            hmr: self.hmr,
            optimize_imports: self.optimize_imports,
            runtime_module_name: self.runtime_module_name,
            ssr_runtime_module_name: self.ssr_runtime_module_name,
            parse_pad: self.parse_pad,
            ignore_empty: self.ignore_empty,
            babel_parser_plugins: self.babel_parser_plugins,
            gen_default_as: self.gen_default_as,
            props_destructure: self.props_destructure,
            script_custom_element: self.script_custom_element,
            transform_asset_urls: self.transform_asset_urls,
            style_trim: self.style_trim,
            css_modules: self.css_modules,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_vue_options_list_matches_the_118_row_count() {
        assert_eq!(
            ALL_VUE_OPTIONS.len(),
            118,
            "vue-options.tsv has 118 data rows"
        );
    }

    #[test]
    fn vue_option_classification_counts_match_the_committed_tsv() {
        use VueOptionClass::*;
        let count = |c: VueOptionClass| {
            ALL_VUE_OPTIONS
                .iter()
                .filter(|o| std::mem::discriminant(&o.class()) == std::mem::discriminant(&c))
                .count()
        };
        assert_eq!(count(Derived), 52, "vue-options.tsv: 52 `derived` rows");
        assert_eq!(
            count(SupportedCanonical),
            29,
            "vue-options.tsv: 29 `supported canonical` rows"
        );
        assert_eq!(
            count(UnsupportedFailClosed),
            12,
            "vue-options.tsv: 12 `unsupported fail-closed` rows"
        );
        assert_eq!(
            count(HostResolved),
            12,
            "vue-options.tsv: 12 `host-resolved` rows"
        );
        assert_eq!(count(External), 8, "vue-options.tsv: 8 `external` rows");
        assert_eq!(count(TestOnly), 4, "vue-options.tsv: 4 `test-only` rows");
        assert_eq!(
            count(NotApplicable),
            1,
            "vue-options.tsv: 1 `not applicable` row"
        );
    }

    #[test]
    fn every_unsupported_fail_closed_row_is_unrepresentable_on_vue_compile_request() {
        // Structural half: `VueCompileRequest` simply has no field for any
        // of the 12 unsupported rows — enforced at compile time by the
        // type declaration, not something a runtime test can additionally
        // verify (Rust has no field-absence reflection).
        //
        // The discriminating half a runtime test CAN prove: every
        // SUPPORTED field survives `VueOptionAttempt::into_request`
        // end to end. Set every one of the 20 `Option`/`Vec`-shaped
        // supported fields to a distinctive non-default value and assert
        // each one reaches the exact same value on the resulting
        // `VueCompileRequest` — a regression that silently dropped a field
        // in `into_request`'s field-by-field construction (the exact class
        // `attempt_with_no_unsupported_fields_constructs` only spot-checks
        // two fields for) fails this test.
        let attempt = VueOptionAttempt {
            backend: VueBackendRequest::Vapor,
            ssr: true,
            is_custom_element: vec!["my-".to_string(), "ion-".to_string()],
            delimiters: Some(("[[".to_string(), "]]".to_string())),
            whitespace: Some(VueWhitespaceStrategy::Preserve),
            comments: Some(true),
            hoist_static: Some(false),
            cache_handlers: Some(true),
            hmr: Some(true),
            optimize_imports: Some(true),
            runtime_module_name: Some("custom-vue".to_string()),
            ssr_runtime_module_name: Some("custom-vue-server".to_string()),
            parse_pad: Some(VueParsePad::Line),
            ignore_empty: Some(true),
            babel_parser_plugins: vec!["jsx".to_string()],
            gen_default_as: Some("__default__".to_string()),
            props_destructure: Some(true),
            script_custom_element: Some(true),
            transform_asset_urls: Some(VueAssetUrlTransform::Disabled),
            style_trim: Some(true),
            css_modules: Some(VueCssModulesOptions {
                scope_behaviour: Some(VueCssModuleScopeBehaviour::Local),
                hash_prefix: Some("prefix".to_string()),
                locals_convention: None,
                export_globals: Some(true),
            }),
            ..Default::default()
        };
        let request = attempt.into_request().expect("no unsupported field set");

        assert_eq!(request.backend, VueBackendRequest::Vapor);
        assert!(request.ssr);
        assert_eq!(
            request.is_custom_element,
            vec!["my-".to_string(), "ion-".to_string()]
        );
        assert_eq!(
            request.delimiters,
            Some(("[[".to_string(), "]]".to_string()))
        );
        assert_eq!(request.whitespace, Some(VueWhitespaceStrategy::Preserve));
        assert_eq!(request.comments, Some(true));
        assert_eq!(request.hoist_static, Some(false));
        assert_eq!(request.cache_handlers, Some(true));
        assert_eq!(request.hmr, Some(true));
        assert_eq!(request.optimize_imports, Some(true));
        assert_eq!(request.runtime_module_name, Some("custom-vue".to_string()));
        assert_eq!(
            request.ssr_runtime_module_name,
            Some("custom-vue-server".to_string())
        );
        assert_eq!(request.parse_pad, Some(VueParsePad::Line));
        assert_eq!(request.ignore_empty, Some(true));
        assert_eq!(request.babel_parser_plugins, vec!["jsx".to_string()]);
        assert_eq!(request.gen_default_as, Some("__default__".to_string()));
        assert_eq!(request.props_destructure, Some(true));
        assert_eq!(request.script_custom_element, Some(true));
        assert_eq!(
            request.transform_asset_urls,
            Some(VueAssetUrlTransform::Disabled)
        );
        assert_eq!(request.style_trim, Some(true));
        let css_modules = request.css_modules.expect("css_modules survives");
        assert_eq!(
            css_modules.scope_behaviour,
            Some(VueCssModuleScopeBehaviour::Local)
        );
        assert_eq!(css_modules.hash_prefix, Some("prefix".to_string()));
        assert_eq!(css_modules.export_globals, Some(true));
    }

    #[test]
    fn attempt_refuses_unsupported_option_even_when_explicitly_false() {
        let attempt = VueOptionAttempt {
            compat_config: Some(false),
            ..Default::default()
        };
        let err = attempt.into_request().unwrap_err();
        match err {
            crate::compile_request::CompileRequestError::UnsupportedOption { option, .. } => {
                assert_eq!(
                    option,
                    crate::compile_request::FrameworkOption::Vue(
                        VueOption::ParserOptionsCompatConfig
                    )
                );
            }
            other => panic!("expected UnsupportedOption, got {other:?}"),
        }
    }

    #[test]
    fn attempt_with_no_unsupported_fields_constructs() {
        let attempt = VueOptionAttempt {
            comments: Some(true),
            hoist_static: Some(false),
            ..Default::default()
        };
        let request = attempt.into_request().expect("no unsupported field set");
        assert_eq!(request.comments, Some(true));
        assert_eq!(request.hoist_static, Some(false));
    }

    /// Each presence-refused field refuses, AND names its own option.
    ///
    /// Naming matters as much as refusing: `unsupported_slots` reads its
    /// option identities out of `PRESENCE_REFUSED_VUE_OPTIONS` positionally
    /// beside the per-field presence flags, so a slot inserted, removed, or
    /// reordered on one side and not the other would report a neighbour's
    /// option and tell a caller to remove a field they never wrote.
    ///
    /// Mutation recipes:
    /// - Swap two entries in `PRESENCE_REFUSED_VUE_OPTIONS` (or two lines
    ///   of `unsupported_slots`' `present` array): both swapped slots
    ///   report the other's option here.
    /// - Return `Ok` from `into_request` for one slot: that slot's
    ///   `unwrap_err` panics.
    #[test]
    fn each_unsupported_slot_refuses_by_its_own_identity() {
        let setters: [fn(&mut VueOptionAttempt); 12] = [
            |a| a.compat_config = Some(true),
            |a| a.compat_config_mode = Some(true),
            |a| a.compat_config_compiler_is_on_element = Some(true),
            |a| a.compat_config_compiler_v_bind_sync = Some(true),
            |a| a.compat_config_compiler_v_if_v_for_precedence = Some(true),
            |a| a.compat_config_compiler_v_bind_object_order = Some(true),
            |a| a.compat_config_compiler_v_on_native = Some(true),
            |a| a.compat_config_compiler_native_template = Some(true),
            |a| a.compat_config_compiler_inline_template = Some(true),
            |a| a.compat_config_compiler_filters = Some(true),
            |a| a.transform_compat_config = Some(true),
            |a| a.codegen_mode = Some(true),
        ];
        assert_eq!(setters.len(), PRESENCE_REFUSED_VUE_OPTIONS.len());
        for (index, set) in setters.into_iter().enumerate() {
            let mut attempt = VueOptionAttempt::default();
            set(&mut attempt);
            let expected = PRESENCE_REFUSED_VUE_OPTIONS[index];
            match attempt.into_request().unwrap_err() {
                crate::compile_request::CompileRequestError::UnsupportedOption {
                    option, ..
                } => assert_eq!(
                    option,
                    crate::compile_request::FrameworkOption::Vue(expected),
                    "slot {index} refused under another slot's identity"
                ),
                other => panic!("slot {index}: expected UnsupportedOption, got {other:?}"),
            }
        }
    }
}
