use serde::{Deserialize, Serialize};
use verter_core::builder::codegen::{
    generate as core_generate, CodegenOptions as CoreOptions, FeatureFlags as CoreFeatures,
};
use wasm_bindgen::prelude::*;

#[wasm_bindgen(start)]
pub fn init() {
    #[cfg(feature = "console_error_panic_hook")]
    console_error_panic_hook::set_once();
}

fn default_true() -> bool {
    true
}

#[derive(Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct FeatureFlags {
    /// Enable Options API support (default: true)
    #[serde(default = "default_true")]
    pub options_api: bool,
    /// Enable reactive destructure for defineProps (default: true)
    #[serde(default = "default_true")]
    pub props_destructure: bool,
}

#[derive(Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct CodegenOptions {
    /// The filename for source map generation
    pub filename: Option<String>,
    /// Whether to include source content in the source map
    #[serde(default)]
    pub include_source_content: bool,
    /// SSR mode
    #[serde(default)]
    pub ssr: bool,
    /// Production mode - affects component ID generation and optimizations
    #[serde(default)]
    pub is_production: bool,
    /// Custom component ID (overrides auto-generation from filename)
    pub component_id: Option<String>,
    /// Feature flags for codegen
    #[serde(default)]
    pub features: FeatureFlags,
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
    /// Time taken for the Rust pipeline in milliseconds
    pub duration_ms: f64,
}

/// Compile a Vue SFC to JavaScript.
///
/// @param input - The Vue SFC source code
/// @param options - Optional compilation options (as JS object)
/// @returns The compiled result with code, source map, and code with inline source map
#[wasm_bindgen]
pub fn compile(input: &str, options: JsValue) -> Result<JsValue, JsValue> {
    // Create allocator internally - this is critical for memory safety
    // The allocator manages memory for the OXC AST and cannot cross the WASM boundary
    let allocator = oxc_allocator::Allocator::new();

    let opts: CodegenOptions = if options.is_undefined() || options.is_null() {
        CodegenOptions::default()
    } else {
        serde_wasm_bindgen::from_value(options)
            .map_err(|e| JsValue::from_str(&format!("Invalid options: {}", e)))?
    };

    let core_options = CoreOptions {
        filename: opts.filename,
        include_source_content: opts.include_source_content,
        ssr: opts.ssr,
        is_production: opts.is_production,
        component_id: opts.component_id,
        features: CoreFeatures {
            options_api: opts.features.options_api,
            props_destructure: opts.features.props_destructure,
        },
    };

    let result = core_generate(input, &core_options, &allocator);

    let js_result = CodegenResult {
        code: result.code,
        source_map: result.source_map,
        code_with_source_map: result.code_with_source_map,
        duration_ms: result.duration_ms,
    };

    serde_wasm_bindgen::to_value(&js_result)
        .map_err(|e| JsValue::from_str(&format!("Serialization error: {}", e)))
}
