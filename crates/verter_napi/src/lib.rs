use napi::bindgen_prelude::*;
use napi::{Error, Status};
use napi_derive::napi;
use verter_core::builder::codegen::{
    generate as core_generate, generate_for_vite as core_generate_for_vite,
    CodegenOptions as CoreOptions, FeatureFlags as CoreFeatures,
    ViteCodegenOptions as CoreViteOptions,
};

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
pub struct FeatureFlags {
    /// Enable Options API support (default: true)
    pub options_api: Option<bool>,
    /// Enable reactive destructure for defineProps (default: true)
    pub props_destructure: Option<bool>,
}

#[napi(object)]
#[derive(Default)]
pub struct CodegenOptions {
    /// The filename for source map generation
    pub filename: Option<String>,
    /// Whether to include source content in the source map
    pub include_source_content: Option<bool>,
    /// SSR mode
    pub ssr: Option<bool>,
    /// Production mode - affects component ID generation and optimizations
    pub is_production: Option<bool>,
    /// Custom component ID (overrides auto-generation from filename)
    pub component_id: Option<String>,
    /// Feature flags for codegen
    pub features: Option<FeatureFlags>,
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
    // Create allocator internally - this is critical for memory safety
    // The allocator manages memory for the OXC AST and cannot cross the FFI boundary
    let allocator = oxc_allocator::Allocator::new();

    let opts = options.unwrap_or_default();
    let features = opts.features.unwrap_or_default();

    let core_options = CoreOptions {
        filename: opts.filename,
        include_source_content: opts.include_source_content.unwrap_or(false),
        ssr: opts.ssr.unwrap_or(false),
        is_production: opts.is_production.unwrap_or(false),
        component_id: opts.component_id,
        features: CoreFeatures {
            options_api: features.options_api.unwrap_or(true),
            props_destructure: features.props_destructure.unwrap_or(true),
        },
    };

    let result = with_input_str(input, |input| {
        core_generate(input, &core_options, &allocator)
    })?;

    Ok(CodegenResult {
        code: result.code,
        source_map: result.source_map,
        code_with_source_map: result.code_with_source_map,
        duration_ms: result.duration_ms,
    })
}

/// Synchronous version of compile (same as compile, kept for API compatibility)
#[napi]
pub fn compile_sync(
    input: Either<String, Buffer>,
    options: Option<CodegenOptions>,
) -> Result<CodegenResult> {
    compile(input, options)
}

// =============================================================================
// Vite-specific Compilation API
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
/// Returns split blocks (script, template, styles) for virtual module serving.
/// Each block has its own code, source map, and import metadata with UTF-16 offsets.
///
/// @param input - The Vue SFC source code (string or Buffer)
/// @param options - Optional compilation options
/// @returns Compiled result with split blocks for virtual modules
#[napi]
pub fn compile_for_vite(
    input: Either<String, Buffer>,
    options: Option<ViteCodegenOptions>,
) -> Result<ViteCodegenResult> {
    use verter_core::cursor::position::PositionResolver;

    let allocator = oxc_allocator::Allocator::new();

    let opts = options.unwrap_or_default();

    let core_options = CoreViteOptions {
        filename: opts.filename,
        ssr: opts.ssr.unwrap_or(false),
        is_production: opts.is_production.unwrap_or(false),
        component_id: opts.component_id,
        sourcemap: opts.sourcemap.unwrap_or(true),
    };

    let result = with_input_str(input, |input| {
        core_generate_for_vite(input, &core_options, &allocator)
    })?;

    // Convert BlockOutput to JsBlockOutput with UTF-16 offsets
    let convert_block = |block: verter_core::builder::codegen::BlockOutput| -> JsBlockOutput {
        let resolver = PositionResolver::new(&block.code);
        let imports = block
            .imports
            .into_iter()
            .map(|imp| {
                let start_pos = resolver.to_position(imp.start);
                let end_pos = resolver.to_position(imp.end);
                JsBlockImport {
                    source: imp.source,
                    specifiers: imp.specifiers,
                    start_utf16: start_pos.offset_utf16,
                    end_utf16: end_pos.offset_utf16,
                }
            })
            .collect();
        let body_start_pos = resolver.to_position(block.body_start);
        JsBlockOutput {
            code: block.code,
            source_map: block.source_map,
            imports,
            body_start_utf16: body_start_pos.offset_utf16,
        }
    };

    Ok(ViteCodegenResult {
        script: result.script.map(convert_block),
        template: result.template.map(convert_block),
        styles: result
            .styles
            .into_iter()
            .map(|s| JsStyleBlock {
                code: s.code,
                source_map: s.source_map,
                scoped: s.scoped,
                is_module: s.is_module,
                lang: s.lang,
                module_name: s.module_name,
                module_classes: s
                    .module_classes
                    .into_iter()
                    .map(|(k, v)| vec![k, v])
                    .collect(),
            })
            .collect(),
        duration_ms: result.duration_ms,
    })
}
