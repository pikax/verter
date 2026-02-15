use napi::bindgen_prelude::*;
use napi::{Error, Status};
use napi_derive::napi;
use verter_core::builder::codegen::{compile as core_compile, CodegenOptions as CoreOptions};
use verter_core::strip_types::strip_types as core_strip_types;

fn with_input_str<T>(input: Either<String, Buffer>, f: impl FnOnce(&str) -> T) -> Result<T> {
    match input {
        Either::A(input) => Ok(f(&input)),
        Either::B(buf) => {
            let input_str = std::str::from_utf8(buf.as_ref()).map_err(|e| {
                Error::new(
                    Status::InvalidArg,
                    format!("input must be valid UTF-8: {}", e),
                )
            })?;
            Ok(f(input_str))
        }
    }
}

#[napi(object)]
#[derive(Default)]
pub struct CodegenOptions {
    /// The filename for source map generation
    pub filename: Option<String>,
    /// Production mode - affects component ID generation and optimizations
    pub is_production: Option<bool>,
    /// Custom component ID (overrides auto-generation from filename)
    pub component_id: Option<String>,
    /// Skip source map generation for faster compilation
    pub skip_source_map: Option<bool>,
    /// Custom interpolation delimiters [open, close]. Default: ["{{", "}}"]
    pub delimiters: Option<Vec<String>>,
    /// Tag name prefixes treated as custom elements (skip component resolution).
    /// E.g. ["ion-", "my-"] matches <ion-button>, <my-card>
    pub custom_elements: Option<Vec<String>>,
    /// Whether to preserve HTML comments in output. Default: !isProduction
    pub comments: Option<bool>,
    /// Runtime module name to import helpers from. Default: "vue"
    pub runtime_module_name: Option<String>,
    /// Hoist static VNodes/props to constants. Default: true
    pub hoist_static: Option<bool>,
    /// Whitespace handling: "condense" or "preserve". Default: "condense"
    pub whitespace: Option<String>,
    /// Cache event handler expressions. Default: false
    pub cache_handlers: Option<bool>,
    /// Inline render function in setup(). Default: isProduction
    pub inline: Option<bool>,
    /// Indicates SFC uses :slotted() in styles. Default: true
    pub slotted: Option<bool>,
}

#[napi(object)]
pub struct JsDiagnostic {
    /// Severity level: "error", "warning", or "info"
    pub severity: String,
    /// Vue-compatible error code (e.g., "XMissingEndTag", "XInvalidEndTag")
    pub code: String,
    /// Human-readable error message
    pub message: String,
    /// Optional source span start (byte offset)
    pub span_start: Option<u32>,
    /// Optional source span end (byte offset)
    pub span_end: Option<u32>,
}

#[napi(object)]
pub struct JsCompiledStyleBlock {
    /// Compiled CSS code (scoped selectors, v-bind replacements, module hashing applied)
    pub code: String,
    /// Whether this style block is scoped
    pub scoped: bool,
    /// Style language (css, scss, less, stylus)
    pub lang: Option<String>,
    /// Whether this is a CSS module block
    pub is_module: bool,
    /// CSS module class mappings (each entry is [original, hashed])
    pub module_classes: Vec<Vec<String>>,
    /// CSS processing errors
    pub errors: Vec<String>,
}

#[napi(object)]
pub struct CodegenResult {
    /// The transformed code
    pub code: String,
    /// The source map as JSON string
    pub source_map: String,
    /// The transformed code with inline source map appended
    pub code_with_source_map: String,
    /// Compiled CSS blocks from `<style>` tags
    pub styles: Vec<JsCompiledStyleBlock>,
    /// Scope ID for scoped styles (e.g., "data-v-a4f2eed6"). Empty if no scoped styles.
    pub scope_id: String,
    /// Compilation diagnostics (errors, warnings, info)
    pub errors: Vec<JsDiagnostic>,
    /// Time taken for the Rust pipeline in milliseconds
    pub duration_ms: f64,
}

/// Internal compile implementation shared by sync and async APIs.
fn compile_impl(
    input: Either<String, Buffer>,
    options: Option<CodegenOptions>,
) -> Result<CodegenResult> {
    let allocator = oxc_allocator::Allocator::new();

    let opts = options.unwrap_or_default();

    let delimiters = opts.delimiters.and_then(|d| {
        if d.len() == 2 {
            Some((d[0].clone(), d[1].clone()))
        } else {
            None
        }
    });

    let whitespace = opts.whitespace.and_then(|w| match w.as_str() {
        "preserve" => Some(verter_core::builder::codegen::WhitespaceStrategy::Preserve),
        "condense" => Some(verter_core::builder::codegen::WhitespaceStrategy::Condense),
        _ => None,
    });

    let core_options = CoreOptions {
        filename: opts.filename,
        is_production: opts.is_production.unwrap_or(false),
        component_id: opts.component_id,
        include_tsx: false,
        skip_source_map: opts.skip_source_map.unwrap_or(false),
        delimiters,
        custom_elements: opts.custom_elements,
        comments: opts.comments,
        runtime_module_name: opts.runtime_module_name,
        hoist_static: opts.hoist_static,
        whitespace,
        cache_handlers: opts.cache_handlers,
        inline: opts.inline,
        slotted: opts.slotted,
        prefix_identifiers: None,
    };

    let result = with_input_str(input, |input| {
        core_compile(input, &core_options, &allocator)
    })?;

    let styles = result
        .styles
        .into_iter()
        .map(|s| {
            let module_classes = s
                .module
                .as_ref()
                .map(|m| {
                    m.classes
                        .iter()
                        .map(|c| vec![c.original.clone(), c.hashed.clone()])
                        .collect()
                })
                .unwrap_or_default();
            JsCompiledStyleBlock {
                code: s.code,
                scoped: s.scoped,
                lang: s.lang.map(|l| match l {
                    verter_core::syntax::types::StyleLang::Css => "css".to_string(),
                    verter_core::syntax::types::StyleLang::Scss => "scss".to_string(),
                    verter_core::syntax::types::StyleLang::Sass => "sass".to_string(),
                    verter_core::syntax::types::StyleLang::Less => "less".to_string(),
                    verter_core::syntax::types::StyleLang::Stylus => "stylus".to_string(),
                    verter_core::syntax::types::StyleLang::Unknown => "unknown".to_string(),
                }),
                is_module: s.module.is_some(),
                module_classes,
                errors: s.errors,
            }
        })
        .collect();

    let errors = result
        .errors
        .iter()
        .map(|d| {
            let severity = match d.severity {
                verter_core::builder::codegen::CompileDiagnosticSeverity::Error => "error",
                verter_core::builder::codegen::CompileDiagnosticSeverity::Warning => "warning",
                verter_core::builder::codegen::CompileDiagnosticSeverity::Info => "info",
            };
            JsDiagnostic {
                severity: severity.to_string(),
                code: d.code.clone(),
                message: d.message.clone(),
                span_start: d.span.map(|s| s.start),
                span_end: d.span.map(|s| s.end),
            }
        })
        .collect();

    Ok(CodegenResult {
        code: result.code,
        source_map: result.source_map,
        code_with_source_map: result.code_with_source_map,
        styles,
        scope_id: result.scope_id,
        errors,
        duration_ms: result.duration_ms,
    })
}

/// Compile a Vue SFC to JavaScript (synchronous).
///
/// @param input - The Vue SFC source code (string or Buffer)
/// @param options - Optional compilation options
/// @returns The compiled result with code, source map, and code with inline source map
#[napi(js_name = "compileSync")]
pub fn compile_sync(
    input: Either<String, Buffer>,
    options: Option<CodegenOptions>,
) -> Result<CodegenResult> {
    compile_impl(input, options)
}

/// Compile a Vue SFC to JavaScript (async, runs on libuv thread pool).
///
/// @param input - The Vue SFC source code (string or Buffer)
/// @param options - Optional compilation options
/// @returns Promise resolving to the compiled result
#[napi]
pub fn compile(
    input: Either<String, Buffer>,
    options: Option<CodegenOptions>,
) -> Result<CodegenResult> {
    compile_impl(input, options)
}

// =============================================================================
// Vite-specific Compilation API (thin wrapper)
// =============================================================================

#[napi(object)]
#[derive(Default)]
pub struct ViteCodegenOptions {
    /// The filename for source map generation
    pub filename: Option<String>,
    /// SSR mode
    pub ssr: Option<bool>,
    /// Production mode
    pub is_production: Option<bool>,
    /// Custom component ID
    pub component_id: Option<String>,
    /// Whether to generate source maps
    pub sourcemap: Option<bool>,
}

/// An import statement in a block's output (with UTF-16 offsets for JS).
#[napi(object)]
pub struct JsBlockImport {
    /// Import source (e.g., "vue")
    pub source: String,
    /// Specifier strings (e.g., ["openBlock as _openBlock", ...])
    pub specifiers: Vec<String>,
    /// UTF-16 code unit offset of import start in block's code
    pub start_utf16: u32,
    /// UTF-16 code unit offset of import end in block's code
    pub end_utf16: u32,
}

/// Output block with code, source map, and import metadata (UTF-16 offsets for JS).
#[napi(object)]
pub struct JsBlockOutput {
    /// Generated code for this block
    pub code: String,
    /// Source map as JSON string
    pub source_map: Option<String>,
    /// Import statements with UTF-16 offsets
    pub imports: Vec<JsBlockImport>,
    /// UTF-16 code unit offset where non-import code begins
    pub body_start_utf16: u32,
}

#[napi(object)]
pub struct JsStyleBlock {
    /// Processed CSS content
    pub code: String,
    /// Source map for CSS transformations
    pub source_map: Option<String>,
    /// Is scoped style
    pub scoped: bool,
    /// Is CSS module
    pub is_module: bool,
    /// Language (css, scss, less)
    pub lang: Option<String>,
    /// Module name (e.g., "$style")
    pub module_name: Option<String>,
    /// CSS module class mappings (original → hashed)
    pub module_classes: Vec<Vec<String>>,
}

#[napi(object)]
pub struct ViteCodegenResult {
    /// Script block (component definition)
    pub script: Option<JsBlockOutput>,
    /// Template block (render function)
    pub template: Option<JsBlockOutput>,
    /// Style blocks
    pub styles: Vec<JsStyleBlock>,
    /// Whether the SFC has a default export (script setup or script with export default)
    pub has_default_export: bool,
    /// Build time in milliseconds
    pub duration_ms: f64,
}

/// Internal compile_for_vite implementation shared by sync and async APIs.
fn compile_for_vite_impl(
    input: Either<String, Buffer>,
    options: Option<ViteCodegenOptions>,
) -> Result<ViteCodegenResult> {
    let allocator = oxc_allocator::Allocator::new();

    let opts = options.unwrap_or_default();

    let core_options = CoreOptions {
        filename: opts.filename,
        is_production: opts.is_production.unwrap_or(false),
        component_id: opts.component_id,
        include_tsx: false,
        skip_source_map: false,
        ..Default::default()
    };

    let result = with_input_str(input, |input| {
        core_compile(input, &core_options, &allocator)
    })?;

    let styles = result
        .styles
        .into_iter()
        .map(|s| {
            let module_classes = s
                .module
                .as_ref()
                .map(|m| {
                    m.classes
                        .iter()
                        .map(|c| vec![c.original.clone(), c.hashed.clone()])
                        .collect()
                })
                .unwrap_or_default();
            let module_name = s.module.as_ref().map(|m| {
                m.custom_name
                    .map(|_span| "custom".to_string())
                    .unwrap_or_else(|| "$style".to_string())
            });
            JsStyleBlock {
                code: s.code,
                source_map: None,
                scoped: s.scoped,
                is_module: s.module.is_some(),
                lang: s.lang.map(|l| match l {
                    verter_core::syntax::types::StyleLang::Css => "css".to_string(),
                    verter_core::syntax::types::StyleLang::Scss => "scss".to_string(),
                    verter_core::syntax::types::StyleLang::Sass => "sass".to_string(),
                    verter_core::syntax::types::StyleLang::Less => "less".to_string(),
                    verter_core::syntax::types::StyleLang::Stylus => "stylus".to_string(),
                    verter_core::syntax::types::StyleLang::Unknown => "unknown".to_string(),
                }),
                module_name,
                module_classes,
            }
        })
        .collect();

    // The Rust compiler now emits `const __sfc__ = ...`, `__sfc__.__scopeId = "...";`,
    // and `export default __sfc__;` at the end.  For the Vite path, strip these lines
    // so that generateMainModule in TypeScript can handle metadata and export.
    let mut code = result.code;
    let mut has_default_export = false;

    if let Some(pos) = code.rfind("\nexport default __sfc__;\n") {
        code.replace_range(pos..pos + "\nexport default __sfc__;\n".len(), "\n");
        has_default_export = true;
    } else if code.ends_with("export default __sfc__;\n") {
        let pos = code.len() - "export default __sfc__;\n".len();
        code.truncate(pos);
        has_default_export = true;
    }

    // Strip __sfc__.__scopeId line — generateMainModule handles scopeId via metadata
    if let Some(start) = code.find("\n__sfc__.__scopeId = ") {
        if let Some(end) = code[start + 1..].find('\n') {
            code.replace_range(start..start + 1 + end, "");
        }
    }

    Ok(ViteCodegenResult {
        script: Some(JsBlockOutput {
            code,
            source_map: Some(result.source_map),
            imports: vec![],
            body_start_utf16: 0,
        }),
        template: None,
        styles,
        has_default_export,
        duration_ms: result.duration_ms,
    })
}

/// Compile a Vue SFC for Vite plugin usage (synchronous).
///
/// @param input - The Vue SFC source code (string or Buffer)
/// @param options - Optional compilation options
/// @returns Compiled result with split blocks for virtual modules
#[napi(js_name = "compileForViteSync")]
pub fn compile_for_vite_sync(
    input: Either<String, Buffer>,
    options: Option<ViteCodegenOptions>,
) -> Result<ViteCodegenResult> {
    compile_for_vite_impl(input, options)
}

/// Compile a Vue SFC for Vite plugin usage (async, runs on libuv thread pool).
///
/// @param input - The Vue SFC source code (string or Buffer)
/// @param options - Optional compilation options
/// @returns Promise resolving to the compiled result
#[napi(js_name = "compileForVite")]
pub fn compile_for_vite(
    input: Either<String, Buffer>,
    options: Option<ViteCodegenOptions>,
) -> Result<ViteCodegenResult> {
    compile_for_vite_impl(input, options)
}

// =============================================================================
// Standalone CSS Style Processing (for preprocessed CSS from Vite plugin)
// =============================================================================

#[napi(object)]
#[derive(Default)]
pub struct ProcessStyleOptions {
    /// Scope ID string (e.g., "a4f2eed6")
    pub scope_id: String,
    /// Whether this style block is scoped
    pub scoped: Option<bool>,
    /// Whether this is a CSS module block
    pub is_module: Option<bool>,
    /// Custom module name (None = "$style")
    pub module_name: Option<String>,
    /// Source filename for source map generation
    pub filename: Option<String>,
    /// Whether to generate source maps
    pub sourcemap: Option<bool>,
}

#[napi(object)]
pub struct ProcessStyleVBind {
    /// The original expression text (e.g., "color" or "theme.color")
    pub expression: String,
    /// The generated CSS variable name (e.g., "--a4f2eed6-color")
    pub var_name: String,
}

#[napi(object)]
pub struct ProcessStyleResult {
    /// Transformed CSS code
    pub code: String,
    /// Source map as JSON string (if sourcemap was requested)
    pub source_map: Option<String>,
    /// CSS module class mappings (original → hashed), each entry is [original, hashed]
    pub module_classes: Vec<Vec<String>>,
    /// v-bind() expressions found and replaced
    pub v_bind_vars: Vec<ProcessStyleVBind>,
}

/// Process a CSS style block: apply scoping, CSS modules, and v-bind replacement.
///
/// Called by the Vite plugin after preprocessing SCSS/Less/Stylus to valid CSS.
/// For plain CSS blocks, the Rust compiler handles this inline during compileForVite().
///
/// @param css - Valid CSS string (already preprocessed if originally SCSS/Less/etc.)
/// @param options - Processing options (scope ID, scoped, modules, etc.)
/// @returns Processed CSS with scoping/modules applied, plus v-bind metadata
#[napi]
pub fn process_style(css: String, options: ProcessStyleOptions) -> Result<ProcessStyleResult> {
    let core_options = verter_core::css::ProcessStyleOptions {
        scope_id: &options.scope_id,
        scoped: options.scoped.unwrap_or(false),
        is_module: options.is_module.unwrap_or(false),
        filename: options.filename.as_deref(),
        sourcemap: options.sourcemap.unwrap_or(false),
    };

    let result = verter_core::css::process_style(&css, &core_options)
        .map_err(|e| Error::new(Status::GenericFailure, e))?;

    Ok(ProcessStyleResult {
        code: result.code,
        source_map: result.source_map,
        module_classes: result
            .module_classes
            .into_iter()
            .map(|(k, v)| vec![k, v])
            .collect(),
        v_bind_vars: result
            .v_bind_vars
            .into_iter()
            .map(|v| ProcessStyleVBind {
                expression: v.expression,
                var_name: v.var_name,
            })
            .collect(),
    })
}

// =============================================================================
// Standalone TypeScript Stripping
// =============================================================================

#[napi(object)]
pub struct StripTypesResult {
    /// The JavaScript output with TypeScript syntax removed.
    pub code: String,
    /// Any parse errors encountered.
    pub errors: Vec<String>,
}

/// Strip TypeScript syntax from a standalone `.ts`/`.tsx` file.
///
/// Removes type annotations, interfaces, type aliases, and converts enums to JavaScript.
///
/// @param source - The TypeScript source code (string or Buffer)
/// @returns The stripped JavaScript code and any parse errors
#[napi]
pub fn strip_types(source: Either<String, Buffer>) -> Result<StripTypesResult> {
    let allocator = oxc_allocator::Allocator::new();

    let result = with_input_str(source, |s| core_strip_types(s, &allocator))?;

    Ok(StripTypesResult {
        code: result.code,
        errors: result.errors,
    })
}
