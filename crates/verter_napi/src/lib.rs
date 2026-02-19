use std::sync::Arc;

use napi::bindgen_prelude::*;
use napi::{Error, Status};
use napi_derive::napi;
use verter_core::builder::codegen::{compile as core_compile, CodegenOptions as CoreOptions};
use verter_core::strip_types::strip_types as core_strip_types;
use verter_host as host;

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
pub struct JsCustomBlock {
    /// The tag name (e.g., "i18n", "docs")
    pub block_type: String,
    /// Raw content between open and close tags
    pub content: String,
    /// Attributes as key-value pairs [[key, value], ...]
    pub attrs: Vec<Vec<String>>,
}

#[napi(object)]
pub struct ViteCodegenResult {
    /// Script block (component definition)
    pub script: Option<JsBlockOutput>,
    /// Template block (render function)
    pub template: Option<JsBlockOutput>,
    /// Style blocks
    pub styles: Vec<JsStyleBlock>,
    /// Custom blocks (e.g., `<i18n>`, `<docs>`)
    pub custom_blocks: Vec<JsCustomBlock>,
    /// Whether the SFC has a default export (script setup or script with export default)
    pub has_default_export: bool,
    /// Whether the output contains a standalone `function render()` that must be
    /// attached to the component via `_sfc_main.render = render`.
    pub has_render: bool,
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

    // Merge dual script blocks: when both <script> and <script setup> exist,
    // the compiler produces two `const __sfc__ = ...` declarations. Rename the
    // first to `__default__` and inject `...__default__,` into the second one's
    // _defineComponent({) so options (inheritAttrs, name, etc.) are preserved.
    {
        let first = code.find("const __sfc__ = ");
        if let Some(first_pos) = first {
            let after_first = first_pos + "const __sfc__ = ".len();
            if let Some(second_offset) = code[after_first..].find("const __sfc__ = ") {
                let second_pos = after_first + second_offset;
                // Rename first declaration: __sfc__ → __default__
                code.replace_range(
                    first_pos..first_pos + "const __sfc__ = ".len(),
                    "const __default__ = ",
                );
                // The second declaration shifted by 4 chars ("__default__" is 4 longer than "__sfc__")
                let adjusted_second = second_pos + 4;
                // Find the opening `{` of _defineComponent({ after the second declaration
                if let Some(brace_offset) = code[adjusted_second..].find("_defineComponent({") {
                    let brace_pos = adjusted_second + brace_offset + "_defineComponent({".len();
                    code.insert_str(brace_pos, "...__default__,");
                }
            }
        }
    }

    let custom_blocks = result
        .custom_blocks
        .into_iter()
        .map(|b| JsCustomBlock {
            block_type: b.block_type,
            content: b.content,
            attrs: b.attrs.into_iter().map(|(k, v)| vec![k, v]).collect(),
        })
        .collect();

    Ok(ViteCodegenResult {
        script: Some(JsBlockOutput {
            code,
            source_map: Some(result.source_map),
            imports: vec![],
            body_start_utf16: 0,
        }),
        template: None,
        styles,
        custom_blocks,
        has_default_export,
        has_render: result.has_render,
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

// =============================================================================
// VerterHost (in-memory virtual file host)
// =============================================================================

#[napi(object)]
#[derive(Default)]
pub struct HostConfig {
    pub dev_mode: Option<bool>,
    pub compile_error_policy: Option<String>,
    pub lsp_scheme: Option<String>,
    pub max_profiles_per_file: Option<u32>,
}

#[napi(object)]
#[derive(Default, Clone)]
pub struct HostCompileProfile {
    pub filename: Option<String>,
    pub is_production: Option<bool>,
    pub ssr: Option<bool>,
    pub hmr_strategy: Option<String>,
    pub component_id: Option<String>,
    pub delimiters: Option<Vec<String>>,
    pub custom_elements: Option<Vec<String>>,
    pub comments: Option<bool>,
    pub runtime_module_name: Option<String>,
    pub force_vapor: Option<bool>,
    pub strip_ts: Option<bool>,
    pub source_map: Option<bool>,
}

#[napi(object)]
#[derive(Clone)]
pub struct HostVirtualNodeKind {
    pub kind: String,
    pub index: Option<u32>,
}

#[napi(object)]
pub struct HostSliceChanges {
    pub script_changed: bool,
    pub template_changed: bool,
    pub style_indices_changed: Vec<u32>,
    pub custom_indices_changed: Vec<u32>,
    pub structure_changed: bool,
    pub descriptor_changed: bool,
}

#[napi(object)]
pub struct HostDiagnostic {
    pub severity: String,
    pub code: String,
    pub message: String,
    pub span_start: Option<u32>,
    pub span_end: Option<u32>,
}

#[napi(object)]
pub struct HostDiagnosticsSnapshot {
    pub diagnostics: Vec<HostDiagnostic>,
    pub has_errors: bool,
}

#[napi(object)]
pub struct HostExternalSourceRequest {
    pub owner_canonical_id: String,
    pub block_kind: String,
    pub index: u32,
    pub specifier: String,
    pub resolved_canonical_id: String,
}

#[napi(object)]
pub struct HostUpdateResult {
    pub canonical_id: String,
    pub changed: bool,
    pub slice_changes: HostSliceChanges,
    pub changed_virtual_nodes: Vec<HostVirtualNodeKind>,
    pub removed_virtual_nodes: Vec<HostVirtualNodeKind>,
    pub changed_virtual_ids: Vec<String>,
    pub removed_virtual_ids: Vec<String>,
    pub changed_lsp_ids: Vec<String>,
    pub removed_lsp_ids: Vec<String>,
    pub diagnostics: HostDiagnosticsSnapshot,
    pub external_source_requests: Vec<HostExternalSourceRequest>,
}

#[napi(object)]
pub struct HostResolvedId {
    pub canonical_id: String,
    pub node_kind: HostVirtualNodeKind,
    pub exists_in_host: bool,
    pub bundler_id: String,
    pub lsp_id: String,
}

#[napi(object)]
pub struct HostVirtualMeta {
    pub scope_id: Option<String>,
    pub block_type: Option<String>,
    pub style_index: Option<u32>,
    pub custom_index: Option<u32>,
}

#[napi(object)]
pub struct HostVirtualFileResponse {
    pub id: String,
    pub code: String,
    pub source_map: Option<String>,
    pub lang: Option<String>,
    pub stale: bool,
    pub diagnostics: HostDiagnosticsSnapshot,
    pub meta: HostVirtualMeta,
}

#[napi(object)]
pub struct HostUpsertRequest {
    pub canonical_id: Option<String>,
    pub input_id: String,
    pub source: String,
    pub file_kind: Option<String>,
    pub aliases: Option<Vec<String>>,
    pub compile_profile: Option<HostCompileProfile>,
}

#[napi(object)]
pub struct HostStyleOverrideEntry {
    pub index: u32,
    pub code: String,
    pub source_map: Option<String>,
}

#[napi(object)]
pub struct HostStyleOverrideRequest {
    pub canonical_id: String,
    pub compile_profile: Option<HostCompileProfile>,
    pub overrides: Vec<HostStyleOverrideEntry>,
}

#[napi(object)]
pub struct HostVirtualQuery {
    pub raw_id: Option<String>,
    pub canonical_id: Option<String>,
    pub node_kind: Option<HostVirtualNodeKind>,
    pub compile_profile: Option<HostCompileProfile>,
}

#[napi(object)]
pub struct HostRemoveResult {
    pub canonical_id: String,
}

fn to_host_config(input: Option<HostConfig>) -> host::HostConfig {
    let mut out = host::HostConfig::default();
    if let Some(input) = input {
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
    }
    out
}

fn to_host_profile(input: Option<HostCompileProfile>) -> host::CompileProfile {
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
        if let Some(runtime) = input.runtime_module_name {
            out.runtime_module_name = Some(runtime);
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

fn to_host_file_kind(input: Option<&str>) -> Result<host::FileKind> {
    match input.unwrap_or("vue").to_ascii_lowercase().as_str() {
        "vue" | "sfc" | "vue_sfc" => Ok(host::FileKind::VueSfc),
        "non_sfc" | "text" | "file" => Ok(host::FileKind::NonSfc),
        other => Err(Error::new(
            Status::InvalidArg,
            format!("invalid file_kind '{}'", other),
        )),
    }
}

fn to_host_node_kind(input: HostVirtualNodeKind) -> Result<host::VirtualNodeKind> {
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
        other => Err(Error::new(
            Status::InvalidArg,
            format!("invalid virtual node kind '{}'", other),
        )),
    }
}

fn from_host_node_kind(input: &host::VirtualNodeKind) -> HostVirtualNodeKind {
    match input {
        host::VirtualNodeKind::Main => HostVirtualNodeKind {
            kind: "main".to_string(),
            index: None,
        },
        host::VirtualNodeKind::Script => HostVirtualNodeKind {
            kind: "script".to_string(),
            index: None,
        },
        host::VirtualNodeKind::Template => HostVirtualNodeKind {
            kind: "template".to_string(),
            index: None,
        },
        host::VirtualNodeKind::Style { index } => HostVirtualNodeKind {
            kind: "style".to_string(),
            index: Some(*index as u32),
        },
        host::VirtualNodeKind::Custom { index } => HostVirtualNodeKind {
            kind: "custom".to_string(),
            index: Some(*index as u32),
        },
    }
}

fn from_host_diagnostics(input: &host::DiagnosticsSnapshot) -> HostDiagnosticsSnapshot {
    HostDiagnosticsSnapshot {
        diagnostics: input
            .diagnostics
            .iter()
            .map(|d| HostDiagnostic {
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

fn from_host_update(input: host::HostUpdateResult) -> HostUpdateResult {
    HostUpdateResult {
        canonical_id: input.canonical_id,
        changed: input.changed,
        slice_changes: HostSliceChanges {
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
            .map(|req| HostExternalSourceRequest {
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

fn from_host_virtual_file(input: host::VirtualFileResponse) -> HostVirtualFileResponse {
    HostVirtualFileResponse {
        id: input.id,
        code: input.code.to_string(),
        source_map: input.source_map.as_ref().map(|s| s.to_string()),
        lang: input.lang,
        stale: input.stale,
        diagnostics: from_host_diagnostics(&input.diagnostics),
        meta: HostVirtualMeta {
            scope_id: input.meta.scope_id,
            block_type: input.meta.block_type,
            style_index: input.meta.style_index.map(|i| i as u32),
            custom_index: input.meta.custom_index.map(|i| i as u32),
        },
    }
}

fn host_error(err: host::HostError) -> Error {
    match err {
        host::HostError::MissingSource { canonical_id } => Error::new(
            Status::GenericFailure,
            format!("HostError::MissingSource: {}", canonical_id),
        ),
        host::HostError::InvalidQuery => {
            Error::new(Status::InvalidArg, "HostError::InvalidQuery".to_string())
        }
        host::HostError::MissingVirtualNode { canonical_id } => Error::new(
            Status::GenericFailure,
            format!("HostError::MissingVirtualNode: {}", canonical_id),
        ),
        host::HostError::CompileError { diagnostics } => {
            let summary = diagnostics
                .diagnostics
                .iter()
                .map(|d| format!("[{}] {}", d.code, d.message))
                .collect::<Vec<_>>()
                .join("; ");
            Error::new(
                Status::GenericFailure,
                format!("HostError::CompileError: {}", summary),
            )
        }
    }
}

#[napi(js_name = "VerterHost")]
pub struct NapiVerterHost {
    inner: host::VerterHost,
}

#[napi]
impl NapiVerterHost {
    #[napi(constructor)]
    pub fn new(config: Option<HostConfig>) -> Self {
        Self {
            inner: host::VerterHost::new(to_host_config(config)),
        }
    }

    #[napi]
    pub fn resolve(&self, raw_id: String) -> Option<HostResolvedId> {
        self.inner.resolve(&raw_id).map(|resolved| HostResolvedId {
            canonical_id: resolved.canonical_id,
            node_kind: from_host_node_kind(&resolved.node_kind),
            exists_in_host: resolved.exists_in_host,
            bundler_id: resolved.bundler_id,
            lsp_id: resolved.lsp_id,
        })
    }

    #[napi]
    pub fn upsert(&self, request: HostUpsertRequest) -> Result<HostUpdateResult> {
        let req = host::UpsertRequest {
            canonical_id: request.canonical_id,
            input_id: request.input_id,
            source: Arc::from(request.source),
            file_kind: to_host_file_kind(request.file_kind.as_deref())?,
            aliases: request.aliases.unwrap_or_default(),
            compile_profile: to_host_profile(request.compile_profile),
        };
        self.inner
            .upsert(req)
            .map(from_host_update)
            .map_err(host_error)
    }

    #[napi(js_name = "applyStyleOverrides")]
    pub fn apply_style_overrides(
        &self,
        request: HostStyleOverrideRequest,
    ) -> Result<HostUpdateResult> {
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

        self.inner
            .apply_style_overrides(req)
            .map(from_host_update)
            .map_err(host_error)
    }

    #[napi(js_name = "getVirtualFile")]
    pub fn get_virtual_file(&self, query: HostVirtualQuery) -> Result<HostVirtualFileResponse> {
        let node_kind = query.node_kind.map(to_host_node_kind).transpose()?;
        let q = host::VirtualQuery {
            raw_id: query.raw_id,
            canonical_id: query.canonical_id,
            node_kind,
            compile_profile: to_host_profile(query.compile_profile),
        };

        self.inner
            .get_virtual_file(q)
            .map(from_host_virtual_file)
            .map_err(host_error)
    }

    #[napi(js_name = "listVirtualFiles")]
    pub fn list_virtual_files(&self, canonical_id: String) -> Vec<HostVirtualNodeKind> {
        self.inner
            .list_virtual_files(&canonical_id)
            .iter()
            .map(from_host_node_kind)
            .collect()
    }

    #[napi]
    pub fn remove(&self, canonical_or_alias: String) -> Option<HostRemoveResult> {
        self.inner
            .remove(&canonical_or_alias)
            .map(|r| HostRemoveResult {
                canonical_id: r.canonical_id,
            })
    }
}
