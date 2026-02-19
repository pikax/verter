use std::sync::Arc;

use serde::{de::DeserializeOwned, Deserialize, Serialize};
use verter_core::builder::codegen::{
    compile as core_compile, compile_with_tsx, CodegenOptions as CoreOptions,
};
use verter_core::new_impl::compile::{compile as verter_compile, VerterCompileOptions};
use verter_core::strip_types::strip_types as core_strip_types;
use verter_host as host;
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
    /// When true, generate TSX output via the syntax pipeline.
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
pub struct WasmDiagnostic {
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
    /// Scope ID for scoped styles (e.g., "data-v-a4f2eed6"). Empty if no scoped styles.
    pub scope_id: String,
    /// Compilation diagnostics (errors, warnings, info)
    pub errors: Vec<WasmDiagnostic>,
    /// Time taken for the Rust pipeline in milliseconds
    pub duration_ms: f64,
    /// The generated TSX code (all blocks: script + template JSX + commented styles)
    pub tsx: String,
    /// Compiled CSS (scoped selectors applied, v-bind replaced) — deprecated, use `styles`
    pub css: String,
    /// Time taken for TSX generation in milliseconds
    pub tsx_duration_ms: f64,
}

/// Build [`CoreOptions`] from the WASM-facing [`CodegenOptions`].
fn build_core_options(opts: &CodegenOptions) -> CoreOptions {
    let whitespace = opts.whitespace.as_ref().and_then(|w| match w.as_str() {
        "preserve" => Some(verter_core::builder::codegen::WhitespaceStrategy::Preserve),
        "condense" => Some(verter_core::builder::codegen::WhitespaceStrategy::Condense),
        _ => None,
    });

    CoreOptions {
        filename: opts.filename.clone(),
        is_production: opts.is_production,
        component_id: opts.component_id.clone(),
        include_tsx: opts.include_tsx,
        skip_source_map: false,
        delimiters: opts.delimiters.clone(),
        custom_elements: opts.custom_elements.clone(),
        comments: opts.comments,
        runtime_module_name: opts.runtime_module_name.clone(),
        hoist_static: opts.hoist_static,
        whitespace,
        cache_handlers: opts.cache_handlers,
        inline: opts.inline,
        slotted: opts.slotted,
        prefix_identifiers: None,
    }
}

fn compile_inner(input: &str, options: JsValue) -> Result<JsValue, JsValue> {
    let allocator = oxc_allocator::Allocator::new();

    let opts: CodegenOptions = if options.is_undefined() || options.is_null() {
        CodegenOptions::default()
    } else {
        serde_wasm_bindgen::from_value(options)
            .map_err(|e| JsValue::from_str(&format!("Invalid options: {}", e)))?
    };

    let core_options = build_core_options(&opts);

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
                        .map(|c| vec![c.original.clone(), c.hashed.clone()])
                        .collect()
                })
                .unwrap_or_default();
            CompiledStyleBlock {
                code: s.code.clone(),
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
                errors: s.errors.clone(),
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
            WasmDiagnostic {
                severity: severity.to_string(),
                code: d.code.clone(),
                message: d.message.clone(),
                span_start: d.span.map(|s| s.start),
                span_end: d.span.map(|s| s.end),
            }
        })
        .collect();

    // Build css from compiled styles when tsx pipeline hasn't produced CSS
    let css = if tsx_result.css.is_empty() {
        result
            .styles
            .iter()
            .map(|s| s.code.as_str())
            .collect::<Vec<_>>()
            .join("\n")
    } else {
        tsx_result.css
    };

    let js_result = CodegenResult {
        code: result.code,
        source_map: result.source_map,
        code_with_source_map: result.code_with_source_map,
        styles,
        scope_id: result.scope_id,
        errors,
        duration_ms: result.duration_ms,
        tsx: tsx_result.tsx,
        css,
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

// =============================================================================
// compileVerter — AST-based pipeline (new_impl)
// =============================================================================

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct VerterOptions {
    // ── Shared with CodegenOptions ──
    pub filename: Option<String>,
    #[serde(default)]
    pub is_production: bool,
    pub component_id: Option<String>,
    pub delimiters: Option<(String, String)>,
    pub custom_elements: Option<Vec<String>>,
    pub comments: Option<bool>,
    pub runtime_module_name: Option<String>,
    // ── Verter-specific ──
    #[serde(default)]
    pub force_vapor: bool,
    #[serde(default)]
    pub strip_ts: bool,
    #[serde(default)]
    pub source_map: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VerterResult {
    pub script: Option<VerterScriptResult>,
    pub template: Option<VerterTemplateResult>,
    pub styles: Vec<VerterStyleResult>,
    pub custom_blocks: Vec<VerterCustomBlockResult>,
    pub scope_id: String,
    pub errors: Vec<WasmDiagnostic>,
    pub parse_duration_ms: f64,
    pub total_duration_ms: f64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VerterScriptResult {
    pub code: String,
    pub duration_ms: f64,
    pub source_map: String,
    pub setup: bool,
    pub attrs: Vec<(String, String)>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VerterTemplateResult {
    pub code: String,
    pub source_map: String,
    pub imports: Vec<&'static str>,
    pub duration_ms: f64,
    pub attrs: Vec<(String, String)>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VerterStyleResult {
    pub code: String,
    pub scoped: bool,
    pub lang: Option<String>,
    pub duration_ms: f64,
    pub attrs: Vec<(String, String)>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VerterCustomBlockResult {
    #[serde(rename = "type")]
    pub block_type: String,
    pub content: String,
    pub attrs: Vec<(String, String)>,
}

/// Compile a Vue SFC using the AST-based (new_impl) pipeline.
///
/// Returns individual blocks (script, template, styles, custom blocks)
/// with separate timing and source map information.
///
/// @param input - The Vue SFC source code
/// @param options - Optional compilation options (as JS object)
/// @returns The compiled result with individual blocks
#[wasm_bindgen(js_name = compileVerter)]
pub fn compile_verter(input: &str, options: JsValue) -> Result<JsValue, JsValue> {
    let allocator = oxc_allocator::Allocator::new();

    let opts: VerterOptions = if options.is_undefined() || options.is_null() {
        VerterOptions::default()
    } else {
        serde_wasm_bindgen::from_value(options)
            .map_err(|e| JsValue::from_str(&format!("Invalid options: {}", e)))?
    };

    let core_options = CoreOptions {
        filename: opts.filename.clone(),
        is_production: opts.is_production,
        component_id: opts.component_id.clone(),
        include_tsx: false,
        skip_source_map: true,
        delimiters: opts.delimiters,
        custom_elements: opts.custom_elements,
        comments: opts.comments,
        runtime_module_name: opts.runtime_module_name,
        hoist_static: None,
        whitespace: None,
        cache_handlers: None,
        inline: None,
        slotted: None,
        prefix_identifiers: None,
    };

    let verter_opts = VerterCompileOptions {
        force_vapor: opts.force_vapor,
        strip_ts: opts.strip_ts,
        source_map: opts.source_map,
    };

    let result = verter_compile(input, &core_options, &verter_opts, &allocator);

    let errors: Vec<WasmDiagnostic> = result
        .errors
        .iter()
        .map(|d| {
            let severity = match d.severity {
                verter_core::builder::codegen::CompileDiagnosticSeverity::Error => "error",
                verter_core::builder::codegen::CompileDiagnosticSeverity::Warning => "warning",
                verter_core::builder::codegen::CompileDiagnosticSeverity::Info => "info",
            };
            WasmDiagnostic {
                severity: severity.to_string(),
                code: d.code.clone(),
                message: d.message.clone(),
                span_start: d.span.map(|s| s.start),
                span_end: d.span.map(|s| s.end),
            }
        })
        .collect();

    let js_result = VerterResult {
        script: result.script.map(|s| VerterScriptResult {
            code: s.code,
            duration_ms: s.duration_ms,
            source_map: s.source_map,
            setup: s.setup,
            attrs: s.attrs,
        }),
        template: result.template.map(|t| VerterTemplateResult {
            code: t.code,
            source_map: t.source_map,
            imports: t.imports,
            duration_ms: t.duration_ms,
            attrs: t.attrs,
        }),
        styles: result
            .styles
            .into_iter()
            .map(|s| VerterStyleResult {
                code: s.code,
                scoped: s.scoped,
                lang: s.lang,
                duration_ms: s.duration_ms,
                attrs: s.attrs,
            })
            .collect(),
        custom_blocks: result
            .custom_blocks
            .into_iter()
            .map(|cb| VerterCustomBlockResult {
                block_type: cb.block_type,
                content: cb.content,
                attrs: cb.attrs,
            })
            .collect(),
        scope_id: result.scope_id,
        errors,
        parse_duration_ms: result.parse_duration_ms,
        total_duration_ms: result.total_duration_ms,
    };

    serde_wasm_bindgen::to_value(&js_result)
        .map_err(|e| JsValue::from_str(&format!("Serialization error: {}", e)))
}

// =============================================================================
// VerterHost (in-memory virtual file host)
// =============================================================================

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct HostConfigInput {
    dev_mode: Option<bool>,
    compile_error_policy: Option<String>,
    lsp_scheme: Option<String>,
    max_profiles_per_file: Option<u32>,
}

#[derive(Deserialize, Default, Clone)]
#[serde(rename_all = "camelCase")]
struct HostCompileProfileInput {
    filename: Option<String>,
    is_production: Option<bool>,
    ssr: Option<bool>,
    hmr_strategy: Option<String>,
    component_id: Option<String>,
    delimiters: Option<Vec<String>>,
    custom_elements: Option<Vec<String>>,
    comments: Option<bool>,
    runtime_module_name: Option<String>,
    force_vapor: Option<bool>,
    strip_ts: Option<bool>,
    source_map: Option<bool>,
}

#[derive(Deserialize, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct HostVirtualNodeKindValue {
    kind: String,
    index: Option<u32>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct HostUpsertRequestInput {
    canonical_id: Option<String>,
    input_id: String,
    source: String,
    file_kind: Option<String>,
    aliases: Option<Vec<String>>,
    compile_profile: Option<HostCompileProfileInput>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct HostStyleOverrideEntryInput {
    index: u32,
    code: String,
    source_map: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct HostStyleOverrideRequestInput {
    canonical_id: String,
    compile_profile: Option<HostCompileProfileInput>,
    overrides: Vec<HostStyleOverrideEntryInput>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct HostVirtualQueryInput {
    raw_id: Option<String>,
    canonical_id: Option<String>,
    node_kind: Option<HostVirtualNodeKindValue>,
    compile_profile: Option<HostCompileProfileInput>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct HostSliceChangesOutput {
    script_changed: bool,
    template_changed: bool,
    style_indices_changed: Vec<u32>,
    custom_indices_changed: Vec<u32>,
    structure_changed: bool,
    descriptor_changed: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct HostDiagnosticOutput {
    severity: String,
    code: String,
    message: String,
    span_start: Option<u32>,
    span_end: Option<u32>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct HostDiagnosticsSnapshotOutput {
    diagnostics: Vec<HostDiagnosticOutput>,
    has_errors: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct HostExternalSourceRequestOutput {
    owner_canonical_id: String,
    block_kind: String,
    index: u32,
    specifier: String,
    resolved_canonical_id: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct HostUpdateResultOutput {
    canonical_id: String,
    changed: bool,
    slice_changes: HostSliceChangesOutput,
    changed_virtual_nodes: Vec<HostVirtualNodeKindValue>,
    removed_virtual_nodes: Vec<HostVirtualNodeKindValue>,
    changed_virtual_ids: Vec<String>,
    removed_virtual_ids: Vec<String>,
    changed_lsp_ids: Vec<String>,
    removed_lsp_ids: Vec<String>,
    diagnostics: HostDiagnosticsSnapshotOutput,
    external_source_requests: Vec<HostExternalSourceRequestOutput>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct HostResolvedIdOutput {
    canonical_id: String,
    node_kind: HostVirtualNodeKindValue,
    exists_in_host: bool,
    bundler_id: String,
    lsp_id: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct HostVirtualMetaOutput {
    scope_id: Option<String>,
    block_type: Option<String>,
    style_index: Option<u32>,
    custom_index: Option<u32>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct HostVirtualFileResponseOutput {
    id: String,
    code: String,
    source_map: Option<String>,
    lang: Option<String>,
    stale: bool,
    diagnostics: HostDiagnosticsSnapshotOutput,
    meta: HostVirtualMetaOutput,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct HostRemoveResultOutput {
    canonical_id: String,
}

fn parse_wasm_input<T: DeserializeOwned>(value: JsValue) -> Result<T, JsValue> {
    serde_wasm_bindgen::from_value(value)
        .map_err(|e| JsValue::from_str(&format!("Invalid host input: {}", e)))
}

fn to_wasm_value<T: Serialize>(value: &T) -> Result<JsValue, JsValue> {
    serde_wasm_bindgen::to_value(value)
        .map_err(|e| JsValue::from_str(&format!("Host serialization error: {}", e)))
}

fn to_host_config(input: HostConfigInput) -> host::HostConfig {
    let mut out = host::HostConfig::default();
    if let Some(dev_mode) = input.dev_mode {
        out.dev_mode = dev_mode;
    }
    if let Some(policy) = input.compile_error_policy {
        out.compile_error_policy = if policy.eq_ignore_ascii_case("strict")
            || policy.eq_ignore_ascii_case("strict_error")
        {
            host::CompileErrorPolicy::StrictError
        } else {
            host::CompileErrorPolicy::DevServeLastKnownGood
        };
    }
    if let Some(lsp_scheme) = input.lsp_scheme {
        out.lsp_scheme = lsp_scheme;
    }
    if let Some(max_profiles) = input.max_profiles_per_file {
        out.max_profiles_per_file = max_profiles as usize;
    }
    out
}

fn to_host_profile(input: Option<HostCompileProfileInput>) -> host::CompileProfile {
    let mut out = host::CompileProfile::default();
    if let Some(input) = input {
        out.filename = input.filename;
        if let Some(is_production) = input.is_production {
            out.is_production = is_production;
        }
        if let Some(ssr) = input.ssr {
            out.ssr = ssr;
        }
        if let Some(hmr_strategy) = input.hmr_strategy {
            out.hmr_strategy = if hmr_strategy.eq_ignore_ascii_case("vite") {
                host::HmrStrategy::Vite
            } else if hmr_strategy.eq_ignore_ascii_case("webpack") {
                host::HmrStrategy::Webpack
            } else {
                host::HmrStrategy::None
            };
        }
        out.component_id = input.component_id;
        out.delimiters = input.delimiters.and_then(|d| {
            if d.len() == 2 {
                Some((d[0].clone(), d[1].clone()))
            } else {
                None
            }
        });
        out.custom_elements = input.custom_elements;
        out.comments = input.comments;
        if let Some(runtime_module_name) = input.runtime_module_name {
            out.runtime_module_name = Some(runtime_module_name);
        }
        if let Some(force_vapor) = input.force_vapor {
            out.force_vapor = force_vapor;
        }
        if let Some(strip_ts) = input.strip_ts {
            out.strip_ts = strip_ts;
        }
        if let Some(source_map) = input.source_map {
            out.source_map = source_map;
        }
    }
    out
}

fn to_host_file_kind(input: Option<&str>) -> Result<host::FileKind, JsValue> {
    match input.unwrap_or("vue").to_ascii_lowercase().as_str() {
        "vue" | "sfc" | "vue_sfc" => Ok(host::FileKind::VueSfc),
        "non_sfc" | "text" | "file" => Ok(host::FileKind::NonSfc),
        other => Err(JsValue::from_str(&format!(
            "Invalid host file_kind '{}'",
            other
        ))),
    }
}

fn to_host_node_kind(input: HostVirtualNodeKindValue) -> Result<host::VirtualNodeKind, JsValue> {
    match input.kind.to_ascii_lowercase().as_str() {
        "main" => Ok(host::VirtualNodeKind::Main),
        "script" => Ok(host::VirtualNodeKind::Script),
        "template" => Ok(host::VirtualNodeKind::Template),
        "style" => Ok(host::VirtualNodeKind::Style {
            index: input.index.unwrap_or(0) as usize,
        }),
        "custom" => Ok(host::VirtualNodeKind::Custom {
            index: input.index.unwrap_or(0) as usize,
        }),
        other => Err(JsValue::from_str(&format!(
            "Invalid host virtual node kind '{}'",
            other
        ))),
    }
}

fn from_host_node_kind(input: &host::VirtualNodeKind) -> HostVirtualNodeKindValue {
    match input {
        host::VirtualNodeKind::Main => HostVirtualNodeKindValue {
            kind: "main".to_string(),
            index: None,
        },
        host::VirtualNodeKind::Script => HostVirtualNodeKindValue {
            kind: "script".to_string(),
            index: None,
        },
        host::VirtualNodeKind::Template => HostVirtualNodeKindValue {
            kind: "template".to_string(),
            index: None,
        },
        host::VirtualNodeKind::Style { index } => HostVirtualNodeKindValue {
            kind: "style".to_string(),
            index: Some(*index as u32),
        },
        host::VirtualNodeKind::Custom { index } => HostVirtualNodeKindValue {
            kind: "custom".to_string(),
            index: Some(*index as u32),
        },
    }
}

fn from_host_diagnostics(input: &host::DiagnosticsSnapshot) -> HostDiagnosticsSnapshotOutput {
    HostDiagnosticsSnapshotOutput {
        diagnostics: input
            .diagnostics
            .iter()
            .map(|d| HostDiagnosticOutput {
                severity: match d.severity {
                    host::HostSeverity::Error => "error".to_string(),
                    host::HostSeverity::Warning => "warning".to_string(),
                    host::HostSeverity::Info => "info".to_string(),
                },
                code: d.code.clone(),
                message: d.message.clone(),
                span_start: d.span_start,
                span_end: d.span_end,
            })
            .collect(),
        has_errors: input.has_errors,
    }
}

fn from_host_update(input: host::HostUpdateResult) -> HostUpdateResultOutput {
    HostUpdateResultOutput {
        canonical_id: input.canonical_id,
        changed: input.changed,
        slice_changes: HostSliceChangesOutput {
            script_changed: input.slice_changes.script_changed,
            template_changed: input.slice_changes.template_changed,
            style_indices_changed: input
                .slice_changes
                .style_indices_changed
                .into_iter()
                .map(|i| i as u32)
                .collect(),
            custom_indices_changed: input
                .slice_changes
                .custom_indices_changed
                .into_iter()
                .map(|i| i as u32)
                .collect(),
            structure_changed: input.slice_changes.structure_changed,
            descriptor_changed: input.slice_changes.descriptor_changed,
        },
        changed_virtual_nodes: input
            .changed_virtual_nodes
            .iter()
            .map(from_host_node_kind)
            .collect(),
        removed_virtual_nodes: input
            .removed_virtual_nodes
            .iter()
            .map(from_host_node_kind)
            .collect(),
        changed_virtual_ids: input.changed_virtual_ids,
        removed_virtual_ids: input.removed_virtual_ids,
        changed_lsp_ids: input.changed_lsp_ids,
        removed_lsp_ids: input.removed_lsp_ids,
        diagnostics: from_host_diagnostics(&input.diagnostics),
        external_source_requests: input
            .external_source_requests
            .into_iter()
            .map(|req| HostExternalSourceRequestOutput {
                owner_canonical_id: req.owner_canonical_id,
                block_kind: match req.block_kind {
                    host::ExternalBlockKind::Script => "script".to_string(),
                    host::ExternalBlockKind::Template => "template".to_string(),
                    host::ExternalBlockKind::Style => "style".to_string(),
                    host::ExternalBlockKind::Custom => "custom".to_string(),
                },
                index: req.index as u32,
                specifier: req.specifier,
                resolved_canonical_id: req.resolved_canonical_id,
            })
            .collect(),
    }
}

fn from_host_virtual_file(input: host::VirtualFileResponse) -> HostVirtualFileResponseOutput {
    HostVirtualFileResponseOutput {
        id: input.id,
        code: input.code.to_string(),
        source_map: input.source_map.as_ref().map(|s| s.to_string()),
        lang: input.lang,
        stale: input.stale,
        diagnostics: from_host_diagnostics(&input.diagnostics),
        meta: HostVirtualMetaOutput {
            scope_id: input.meta.scope_id,
            block_type: input.meta.block_type,
            style_index: input.meta.style_index.map(|i| i as u32),
            custom_index: input.meta.custom_index.map(|i| i as u32),
        },
    }
}

fn host_error_to_js(err: host::HostError) -> JsValue {
    match err {
        host::HostError::MissingSource { canonical_id } => {
            JsValue::from_str(&format!("HostError::MissingSource: {}", canonical_id))
        }
        host::HostError::InvalidQuery => JsValue::from_str("HostError::InvalidQuery"),
        host::HostError::MissingVirtualNode { canonical_id } => {
            JsValue::from_str(&format!("HostError::MissingVirtualNode: {}", canonical_id))
        }
        host::HostError::CompileError { diagnostics } => {
            let summary = diagnostics
                .diagnostics
                .iter()
                .map(|d| format!("[{}] {}", d.code, d.message))
                .collect::<Vec<_>>()
                .join("; ");
            JsValue::from_str(&format!("HostError::CompileError: {}", summary))
        }
    }
}

#[wasm_bindgen(js_name = VerterHost)]
pub struct WasmVerterHost {
    inner: host::VerterHost,
}

#[wasm_bindgen(js_class = VerterHost)]
impl WasmVerterHost {
    #[wasm_bindgen(constructor)]
    pub fn new(config: JsValue) -> Result<WasmVerterHost, JsValue> {
        let config = if config.is_undefined() || config.is_null() {
            HostConfigInput::default()
        } else {
            parse_wasm_input::<HostConfigInput>(config)?
        };
        Ok(Self {
            inner: host::VerterHost::new(to_host_config(config)),
        })
    }

    #[wasm_bindgen]
    pub fn resolve(&self, raw_id: &str) -> Result<JsValue, JsValue> {
        let output = self
            .inner
            .resolve(raw_id)
            .map(|resolved| HostResolvedIdOutput {
                canonical_id: resolved.canonical_id,
                node_kind: from_host_node_kind(&resolved.node_kind),
                exists_in_host: resolved.exists_in_host,
                bundler_id: resolved.bundler_id,
                lsp_id: resolved.lsp_id,
            });
        to_wasm_value(&output)
    }

    #[wasm_bindgen]
    pub fn upsert(&self, request: JsValue) -> Result<JsValue, JsValue> {
        let request = parse_wasm_input::<HostUpsertRequestInput>(request)?;
        let req = host::UpsertRequest {
            canonical_id: request.canonical_id,
            input_id: request.input_id,
            source: Arc::from(request.source),
            file_kind: to_host_file_kind(request.file_kind.as_deref())?,
            aliases: request.aliases.unwrap_or_default(),
            compile_profile: to_host_profile(request.compile_profile),
        };
        let output = self
            .inner
            .upsert(req)
            .map(from_host_update)
            .map_err(host_error_to_js)?;
        to_wasm_value(&output)
    }

    #[wasm_bindgen(js_name = applyStyleOverrides)]
    pub fn apply_style_overrides(&self, request: JsValue) -> Result<JsValue, JsValue> {
        let request = parse_wasm_input::<HostStyleOverrideRequestInput>(request)?;
        let req = host::StyleOverrideRequest {
            canonical_id: request.canonical_id,
            compile_profile: to_host_profile(request.compile_profile),
            overrides: request
                .overrides
                .into_iter()
                .map(|entry| host::StyleOverrideEntry {
                    index: entry.index as usize,
                    code: Arc::from(entry.code),
                    source_map: entry.source_map.map(Arc::from),
                })
                .collect(),
        };
        let output = self
            .inner
            .apply_style_overrides(req)
            .map(from_host_update)
            .map_err(host_error_to_js)?;
        to_wasm_value(&output)
    }

    #[wasm_bindgen(js_name = getVirtualFile)]
    pub fn get_virtual_file(&self, query: JsValue) -> Result<JsValue, JsValue> {
        let query = parse_wasm_input::<HostVirtualQueryInput>(query)?;
        let node_kind = query.node_kind.map(to_host_node_kind).transpose()?;
        let req = host::VirtualQuery {
            raw_id: query.raw_id,
            canonical_id: query.canonical_id,
            node_kind,
            compile_profile: to_host_profile(query.compile_profile),
        };
        let output = self
            .inner
            .get_virtual_file(req)
            .map(from_host_virtual_file)
            .map_err(host_error_to_js)?;
        to_wasm_value(&output)
    }

    #[wasm_bindgen(js_name = listVirtualFiles)]
    pub fn list_virtual_files(&self, canonical_id: &str) -> Result<JsValue, JsValue> {
        let output: Vec<HostVirtualNodeKindValue> = self
            .inner
            .list_virtual_files(canonical_id)
            .iter()
            .map(from_host_node_kind)
            .collect();
        to_wasm_value(&output)
    }

    #[wasm_bindgen]
    pub fn remove(&self, canonical_or_alias: &str) -> Result<JsValue, JsValue> {
        let output = self
            .inner
            .remove(canonical_or_alias)
            .map(|r| HostRemoveResultOutput {
                canonical_id: r.canonical_id,
            });
        to_wasm_value(&output)
    }
}
