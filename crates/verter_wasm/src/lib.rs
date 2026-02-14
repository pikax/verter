use serde::{Deserialize, Serialize};
use verter_core::builder::codegen::{
    compile as core_compile, compile_with_tsx, CodegenOptions as CoreOptions,
};
use verter_core::strip_types::strip_types as core_strip_types;
use wasm_bindgen::prelude::*;

#[wasm_bindgen(start)]
pub fn init() {
    #[cfg(feature = "console_error_panic_hook")]
    console_error_panic_hook::set_once();
}

#[derive(Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct CodegenOptions {
    /// The filename for source map generation
    pub filename: Option<String>,
    /// Production mode - affects component ID generation and optimizations
    #[serde(default)]
    pub is_production: bool,
    /// Custom component ID (overrides auto-generation from filename)
    pub component_id: Option<String>,
    /// When true, generate TSX output via the syntax_kai pipeline.
    /// Default: false (skip TSX generation to save time).
    #[serde(default)]
    pub include_tsx: bool,
    /// Custom interpolation delimiters [open, close]. Default: ["{{", "}}"]
    pub delimiters: Option<(String, String)>,
    /// Tag name prefixes treated as custom elements (skip component resolution).
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

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompiledStyleBlock {
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

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodegenResult {
    /// The transformed code
    pub code: String,
    /// The source map as JSON string
    pub source_map: String,
    /// The transformed code with inline source map appended
    pub code_with_source_map: String,
    /// Compiled CSS blocks from `<style>` tags
    pub styles: Vec<CompiledStyleBlock>,
    /// Time taken for the Rust pipeline in milliseconds
    pub duration_ms: f64,
    /// The generated TSX code (all blocks: script + template JSX + commented styles)
    pub tsx: String,
    /// Compiled CSS (scoped selectors applied, v-bind replaced) — deprecated, use `styles`
    pub css: String,
    /// Time taken for TSX generation in milliseconds
    pub tsx_duration_ms: f64,
}

fn compile_inner(input: &str, options: JsValue) -> Result<JsValue, JsValue> {
    let allocator = oxc_allocator::Allocator::new();

    let opts: CodegenOptions = if options.is_undefined() || options.is_null() {
        CodegenOptions::default()
    } else {
        serde_wasm_bindgen::from_value(options)
            .map_err(|e| JsValue::from_str(&format!("Invalid options: {}", e)))?
    };

    let whitespace = opts.whitespace.and_then(|w| match w.as_str() {
        "preserve" => Some(verter_core::builder::codegen::WhitespaceStrategy::Preserve),
        "condense" => Some(verter_core::builder::codegen::WhitespaceStrategy::Condense),
        _ => None,
    });

    let core_options = CoreOptions {
        filename: opts.filename.clone(),
        is_production: opts.is_production,
        component_id: opts.component_id.clone(),
        include_tsx: opts.include_tsx,
        skip_source_map: false,
        delimiters: opts.delimiters,
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

    let result = core_compile(input, &core_options, &allocator);

    // Run TSX pipeline
    let tsx_allocator = oxc_allocator::Allocator::new();
    let tsx_result = compile_with_tsx(input, &core_options, &tsx_allocator);

    let styles: Vec<CompiledStyleBlock> = result
        .styles
        .iter()
        .map(|s| {
            let module_classes = s
                .module
                .as_ref()
                .map(|m| {
                    m.classes
                        .iter()
                        .map(|c| vec![String::new(), c.hashed.clone()])
                        .collect()
                })
                .unwrap_or_default();
            CompiledStyleBlock {
                code: s.code.clone(),
                scoped: s.scoped,
                lang: s.lang.map(|l| match l {
                    verter_core::syntax_kai::types::StyleLang::Css => "css".to_string(),
                    verter_core::syntax_kai::types::StyleLang::Scss => "scss".to_string(),
                    verter_core::syntax_kai::types::StyleLang::Sass => "sass".to_string(),
                    verter_core::syntax_kai::types::StyleLang::Less => "less".to_string(),
                    verter_core::syntax_kai::types::StyleLang::Stylus => "stylus".to_string(),
                    verter_core::syntax_kai::types::StyleLang::Unknown => "unknown".to_string(),
                }),
                is_module: s.module.is_some(),
                module_classes,
                errors: s.errors.clone(),
            }
        })
        .collect();

    let js_result = CodegenResult {
        code: result.code,
        source_map: result.source_map,
        code_with_source_map: result.code_with_source_map,
        styles,
        duration_ms: result.duration_ms,
        tsx: tsx_result.tsx,
        css: tsx_result.css,
        tsx_duration_ms: tsx_result.duration_ms,
    };

    serde_wasm_bindgen::to_value(&js_result)
        .map_err(|e| JsValue::from_str(&format!("Serialization error: {}", e)))
}

/// Compile a Vue SFC to JavaScript.
///
/// @param input - The Vue SFC source code
/// @param options - Optional compilation options (as JS object)
/// @returns The compiled result with code, source map, and code with inline source map
#[wasm_bindgen]
pub fn compile(input: &str, options: JsValue) -> Result<JsValue, JsValue> {
    compile_inner(input, options)
}

/// Compile a Vue SFC to JavaScript from UTF-8 bytes.
///
/// @param input - The Vue SFC source code as UTF-8 bytes (Uint8Array)
/// @param options - Optional compilation options (as JS object)
/// @returns The compiled result with code, source map, and code with inline source map
#[wasm_bindgen(js_name = compileBytes)]
pub fn compile_bytes(input: &[u8], options: JsValue) -> Result<JsValue, JsValue> {
    let input_str = std::str::from_utf8(input)
        .map_err(|e| JsValue::from_str(&format!("input must be valid UTF-8: {}", e)))?;
    compile_inner(input_str, options)
}

// =============================================================================
// Standalone TypeScript Stripping
// =============================================================================

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StripTypesResult {
    /// The JavaScript output with TypeScript syntax removed.
    pub code: String,
    /// Any parse errors encountered.
    pub errors: Vec<String>,
}

/// Strip TypeScript syntax from a standalone `.ts`/`.tsx` file.
///
/// Removes type annotations, interfaces, type aliases, and converts enums to JavaScript.
/// Useful for the playground to execute TypeScript without a separate transform step.
///
/// @param source - The TypeScript source code
/// @returns The stripped JavaScript code and any parse errors
#[wasm_bindgen(js_name = stripTypes)]
pub fn strip_types(source: &str) -> Result<JsValue, JsValue> {
    let allocator = oxc_allocator::Allocator::new();
    let result = core_strip_types(source, &allocator);

    let js_result = StripTypesResult {
        code: result.code,
        errors: result.errors,
    };

    serde_wasm_bindgen::to_value(&js_result)
        .map_err(|e| JsValue::from_str(&format!("Serialization error: {}", e)))
}
