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
pub struct CodegenResult {
    /// The transformed code
    pub code: String,
    /// The source map as JSON string
    pub source_map: String,
    /// The transformed code with inline source map appended
    pub code_with_source_map: String,
    /// Time taken for the Rust pipeline in milliseconds
    pub duration_ms: f64,
}

/// Compile a Vue SFC to JavaScript.
///
/// @param input - The Vue SFC source code (string or Buffer)
/// @param options - Optional compilation options
/// @returns The compiled result with code, source map, and code with inline source map
#[napi]
pub fn compile(
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

    Ok(CodegenResult {
        code: result.code,
        source_map: result.source_map,
        code_with_source_map: result.code_with_source_map,
        duration_ms: result.duration_ms,
    })
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
    /// Build time in milliseconds
    pub duration_ms: f64,
}

/// Compile a Vue SFC for Vite plugin usage.
///
/// Thin wrapper around compile() that returns the result as a single script block.
/// The Vite-specific split block compilation will be reimplemented in a future version.
///
/// @param input - The Vue SFC source code (string or Buffer)
/// @param options - Optional compilation options
/// @returns Compiled result with split blocks for virtual modules
#[napi]
pub fn compile_for_vite(
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

    Ok(ViteCodegenResult {
        script: Some(JsBlockOutput {
            code: result.code,
            source_map: Some(result.source_map),
            imports: vec![],
            body_start_utf16: 0,
        }),
        template: None,
        styles: vec![],
        duration_ms: result.duration_ms,
    })
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
