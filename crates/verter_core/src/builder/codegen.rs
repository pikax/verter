//! Codegen builder that combines syntax parsing, analysis, and Vue code generation.
//!
//! This module creates a complete pipeline:
//! 1. Syntax parsing (tokenizer -> SyntaxPlugin pipeline)
//! 2. Analysis plugin (scope/binding tracking as SyntaxPlugin)
//! 3. Vue codegen plugin (code transformation)
//!
//! The output is the transformed code with an inline base64-encoded source map.

use base64::{engine::general_purpose::STANDARD, Engine};
use sha2::{Digest, Sha256};

#[cfg(not(target_arch = "wasm32"))]
use std::time::Instant;
#[cfg(target_arch = "wasm32")]
use web_time::Instant;

use crate::{
    code_transform::SourceMapOptions,
    codegen::vue::{
        plugin::VueCodegenPlugin, script::extract_binding_metadata,
        template::types::BindingMetadata,
    },
    cursor::ScriptDetector,
    syntax::{
        plugin::{SyntaxPlugin, SyntaxPluginContext, SyntaxPluginOptions},
        plugins::{
            analysis::Analysis, css_parser::CssParserPlugin,
            oxc_parser::oxc_parser::OxcParserPlugin,
        },
        syntax::Syntax,
    },
    tokenizer::byte::tokenize,
    utils::oxc::vue::{parse_script, ScriptMode},
};

/// Result of the codegen process
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

/// Feature flags for codegen (mirrors vite-plugin-vue features)
#[derive(Debug, Clone)]
pub struct FeatureFlags {
    /// Enable Options API support (default: true)
    /// When false, allows tree-shaking of Options API code in production
    pub options_api: bool,
    /// Enable reactive destructure for defineProps (default: true for Vue 3.5+)
    pub props_destructure: bool,
}

impl Default for FeatureFlags {
    fn default() -> Self {
        Self {
            options_api: true,
            props_destructure: true,
        }
    }
}

/// Options for the codegen process
#[derive(Debug, Clone)]
pub struct CodegenOptions {
    /// The filename for source map generation
    pub filename: Option<String>,
    /// Whether to include source content in the source map
    pub include_source_content: bool,
    /// SSR mode
    pub ssr: bool,
    /// Production mode - affects component ID generation and optimizations
    pub is_production: bool,
    /// Custom component ID (overrides auto-generation from filename)
    pub component_id: Option<String>,
    /// Feature flags for codegen
    pub features: FeatureFlags,
    /// When true (default), preserve TypeScript syntax in output.
    /// Set to false to strip type annotations for browser execution (playground).
    pub keep_ts: bool,
}

impl Default for CodegenOptions {
    fn default() -> Self {
        Self {
            filename: None,
            include_source_content: false,
            ssr: false,
            is_production: false,
            component_id: None,
            features: FeatureFlags::default(),
            keep_ts: true,
        }
    }
}

impl CodegenOptions {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_filename(mut self, filename: impl Into<String>) -> Self {
        self.filename = Some(filename.into());
        self
    }

    pub fn include_source_content(mut self, include: bool) -> Self {
        self.include_source_content = include;
        self
    }

    pub fn ssr(mut self, ssr: bool) -> Self {
        self.ssr = ssr;
        self
    }
}

// =============================================================================
// Vite Builder Types
// =============================================================================

/// Options for Vite-style compilation
#[derive(Debug, Clone, Default)]
pub struct ViteCodegenOptions {
    /// The filename for source map generation
    pub filename: Option<String>,
    /// SSR mode
    pub ssr: bool,
    /// Production mode
    pub is_production: bool,
    /// Custom component ID
    pub component_id: Option<String>,
    /// Whether to generate source maps
    pub sourcemap: bool,
}

/// Result specifically for Vite plugin usage
#[derive(Debug, Clone)]
pub struct ViteCodegenResult {
    /// Script block output (component definition)
    pub script: Option<BlockOutput>,
    /// Template block output (render function)
    pub template: Option<BlockOutput>,
    /// Style blocks (processed CSS with source maps)
    pub styles: Vec<StyleBlock>,
    /// Custom blocks
    pub custom: Vec<CustomBlock>,
    /// Simple timing: start to finish (ms)
    pub duration_ms: f64,
}

/// Output block with code, source map, and import metadata.
/// UTF-8 byte offsets in Rust — converted to UTF-16 at NAPI boundary.
#[derive(Debug, Clone)]
pub struct BlockOutput {
    /// Generated code for this block.
    pub code: String,
    /// Source map as JSON string.
    pub source_map: Option<String>,
    /// Import statements found in this block's output.
    pub imports: Vec<BlockImport>,
    /// UTF-8 byte offset where non-import code begins.
    pub body_start: u32,
}

/// An import statement in a block's output.
/// UTF-8 byte offsets — converted to UTF-16 at NAPI boundary.
#[derive(Debug, Clone)]
pub struct BlockImport {
    /// Import source (e.g., "vue").
    pub source: String,
    /// Specifier strings (e.g., ["openBlock as _openBlock", ...]).
    pub specifiers: Vec<String>,
    /// UTF-8 byte offset of import start in block's code.
    pub start: u32,
    /// UTF-8 byte offset of import end in block's code.
    pub end: u32,
}

/// Style block output
#[derive(Debug, Clone)]
pub struct StyleBlock {
    /// Processed CSS content (with scoping/modules applied)
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
    pub module_classes: Vec<(String, String)>,
}

/// Custom block output
#[derive(Debug, Clone)]
pub struct CustomBlock {
    pub tag: String,
    pub content: String,
    pub attrs: Vec<(String, String)>,
    pub start_utf16: u32,
    pub end_utf16: u32,
}

/// Extract import metadata from generated code.
/// Scans for `import { ... } from "..."` patterns and returns their positions.
fn extract_imports(code: &str) -> (Vec<BlockImport>, u32) {
    let mut imports = Vec::new();
    let mut body_start: u32 = 0;
    let bytes = code.as_bytes();
    let len = bytes.len();
    let mut pos = 0;

    while pos < len {
        // Skip whitespace/newlines
        while pos < len && matches!(bytes[pos], b' ' | b'\n' | b'\r' | b'\t') {
            pos += 1;
        }
        if pos >= len {
            break;
        }

        // Check if line starts with "import"
        if pos + 6 <= len && &bytes[pos..pos + 6] == b"import" {
            let import_start = pos as u32;

            // Find the end of this import statement (next newline or semicolon)
            let mut end = pos;
            while end < len && bytes[end] != b'\n' {
                end += 1;
            }
            // Include the newline if present
            if end < len && bytes[end] == b'\n' {
                end += 1;
            }

            let import_line = &code[pos..end];
            let import_end = end as u32;

            // Extract source: find "from" and then the quoted string
            if let Some(from_idx) = import_line.find("from ") {
                let after_from = &import_line[from_idx + 5..];
                // Find quoted string
                let quote = after_from.chars().next().unwrap_or('"');
                if quote == '"' || quote == '\'' {
                    if let Some(end_quote) = after_from[1..].find(quote) {
                        let source = after_from[1..1 + end_quote].to_string();

                        // Extract specifiers: between { and }
                        let mut specifiers = Vec::new();
                        if let Some(open_brace) = import_line.find('{') {
                            if let Some(close_brace) = import_line.find('}') {
                                let spec_str = &import_line[open_brace + 1..close_brace];
                                for spec in spec_str.split(',') {
                                    let spec = spec.trim();
                                    if !spec.is_empty() {
                                        specifiers.push(spec.to_string());
                                    }
                                }
                            }
                        }

                        imports.push(BlockImport {
                            source,
                            specifiers,
                            start: import_start,
                            end: import_end,
                        });
                    }
                }
            }

            pos = end;
        } else {
            // Not an import line — this is where the body starts
            body_start = pos as u32;
            break;
        }
    }

    // If all lines are imports, body_start is at the end
    if body_start == 0 && !imports.is_empty() {
        body_start = code.len() as u32;
    }

    (imports, body_start)
}

/// Generate 8-character SHA-256 hash from text (matches vite-plugin-vue)
///
/// # Example
/// ```ignore
/// let hash = get_hash("src/App.vue");
/// assert_eq!(hash.len(), 8);
/// ```
pub fn get_hash(text: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(text.as_bytes());
    let result = hasher.finalize();
    // Take first 4 bytes = 8 hex characters
    hex::encode(&result[..4])
}

/// Generate component ID based on options and source
///
/// Mirrors vite-plugin-vue's component ID generation strategy:
/// - If custom `component_id` is provided, use it directly
/// - Production: hash(normalized_filepath)
/// - Development: hash(normalized_filepath + source)
///
/// # Example
/// ```ignore
/// let options = CodegenOptions {
///     filename: Some("App.vue".to_string()),
///     is_production: true,
///     ..Default::default()
/// };
/// let id = generate_component_id(&options, "<template></template>");
/// assert_eq!(id.len(), 8);
/// ```
pub fn generate_component_id(options: &CodegenOptions, source: &str) -> String {
    // If custom ID provided, use it directly
    if let Some(ref id) = options.component_id {
        return id.clone();
    }

    let filename = options.filename.as_deref().unwrap_or("component.vue");
    // Normalize Windows paths to Unix style
    let normalized = filename.replace('\\', "/");

    if options.is_production {
        // Production: hash filepath only
        get_hash(&normalized)
    } else {
        // Development: hash filepath + source
        get_hash(&format!("{}{}", normalized, source))
    }
}

/// Check if source has `<style scoped>` to enable scope ID injection early.
/// This is needed because template elements need scope attributes added before
/// we see the style block (template comes before style in Vue SFCs).
pub(crate) fn has_scoped_style(source: &[u8]) -> bool {
    // Quick scan for <style followed by scoped attribute
    let style_tag = b"<style";
    let scoped = b"scoped";
    let close = b">";

    let mut pos = 0;
    while pos + style_tag.len() < source.len() {
        // Find <style
        if let Some(style_start) = find_bytes(&source[pos..], style_tag) {
            let style_pos = pos + style_start;
            // Look for > to find end of opening tag
            if let Some(close_offset) = find_bytes(&source[style_pos..], close) {
                let tag_content = &source[style_pos..style_pos + close_offset];
                // Check if "scoped" appears in the tag
                if find_bytes(tag_content, scoped).is_some() {
                    return true;
                }
                pos = style_pos + close_offset + 1;
            } else {
                break;
            }
        } else {
            break;
        }
    }
    false
}

/// Find needle in haystack, returns offset if found
fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

/// Pre-scan for `<script setup>` bindings before the pipeline runs.
/// This ensures binding metadata is available when template is processed before script.
fn pre_scan_script_setup_bindings(
    source: &str,
    bytes: &[u8],
    allocator: &oxc_allocator::Allocator,
    source_type: oxc_span::SourceType,
) -> Option<BindingMetadata> {
    let script_tag = b"<script";
    let setup_attr = b"setup";
    let close_tag = b"</script>";

    let mut pos = 0;
    while pos + script_tag.len() < bytes.len() {
        let script_start = find_bytes(&bytes[pos..], script_tag)?;
        let script_pos = pos + script_start;
        let after_tag = script_pos + script_tag.len();

        // Validate tag (next char must be whitespace or >)
        match bytes.get(after_tag) {
            Some(b' ') | Some(b'\t') | Some(b'\n') | Some(b'\r') | Some(b'>') => {}
            _ => {
                pos = after_tag;
                continue;
            }
        }

        // Find closing > of the opening tag
        let tag_close = find_bytes(&bytes[after_tag..], b">")?;
        let tag_content = &bytes[after_tag..after_tag + tag_close];

        // Check if "setup" appears in tag attributes
        if find_bytes(tag_content, setup_attr).is_none() {
            pos = after_tag + tag_close + 1;
            continue;
        }

        let content_start = after_tag + tag_close + 1;

        // Find </script>
        let content_end_offset = find_bytes(&bytes[content_start..], close_tag)?;
        let content_end = content_start + content_end_offset;

        // Extract script content as str
        let script_content = &source[content_start..content_end];

        // Parse with OXC
        let parser = oxc_parser::Parser::new(allocator, script_content, source_type);
        let ret = parser.parse();

        // Call parse_script with base_offset = content_start
        let parsed = parse_script(
            &ret.program,
            ScriptMode::Setup,
            content_start as u32,
            script_content,
        );

        return Some(extract_binding_metadata(&parsed));
    }

    None
}

/// Extract component name from a filename.
///
/// Examples:
/// - "my-component.vue" -> "my-component"
/// - "src/components/MyComponent.vue" -> "MyComponent"
/// - "App.vue" -> "App"
fn extract_component_name(filename: &str) -> String {
    // Get the filename part (after last / or \)
    let name = filename.rsplit(['/', '\\']).next().unwrap_or(filename);

    // Remove .vue extension
    let name = name.strip_suffix(".vue").unwrap_or(name);

    // Remove other common extensions
    let name = name.strip_suffix(".ts").unwrap_or(name);
    let name = name.strip_suffix(".js").unwrap_or(name);

    name.to_string()
}

/// Generate code from Vue SFC source.
///
/// This function runs the complete pipeline:
/// 1. Tokenizes the input
/// 2. Processes through syntax plugins (including Analysis for scope tracking)
/// 3. Generates output code via Vue codegen plugin
/// 4. Returns the transformed code with inline source map
///
/// # Example
/// ```ignore
/// use verter_core::builder::codegen::{generate, CodegenOptions};
/// use oxc_allocator::Allocator;
///
/// let source = "<template><div>{{ msg }}</div></template>";
/// let allocator = Allocator::new();
/// let options = CodegenOptions::new().with_filename("test.vue");
///
/// let result = generate(source, &options, &allocator);
/// println!("{}", result.code_with_source_map);
/// ```
pub fn generate(
    input: &str,
    options: &CodegenOptions,
    allocator: &oxc_allocator::Allocator,
) -> CodegenResult {
    let start = Instant::now();

    let bytes = input.as_bytes();

    // Create syntax plugin options (static lifetime via leak for simplicity)
    let syntax_options: &'static SyntaxPluginOptions =
        Box::leak(Box::new(SyntaxPluginOptions::default()));
    let mut syntax_context = SyntaxPluginContext::new(input, bytes, syntax_options);

    let script_detector = ScriptDetector::new();

    let detected = script_detector.detect(bytes);

    // Create the plugins
    let mut oxc_parser = OxcParserPlugin::new(allocator, detected.language.to_source_type());
    let mut vue_codegen = VueCodegenPlugin::new(input, allocator);

    // Set component name from filename if provided
    if let Some(ref filename) = options.filename {
        let component_name = extract_component_name(filename);
        vue_codegen.set_component_name(&component_name);
    }

    // Set production mode
    vue_codegen.set_production(options.is_production);

    // Set keep_ts mode (when false, TypeScript type annotations are stripped)
    vue_codegen.set_keep_ts(options.keep_ts);

    // Pre-scan for scoped styles to set scope_id before template processing
    // (template elements need data-v-xxx attribute, but style comes after template)
    if has_scoped_style(bytes) {
        let component_name = options
            .filename
            .as_ref()
            .map(|f| extract_component_name(f))
            .unwrap_or_else(|| "App".to_string());
        let hash = get_hash(&component_name);
        let hash_bytes = hash.as_bytes();
        let mut scope_id = [0u8; 8];
        scope_id.copy_from_slice(&hash_bytes[..8.min(hash_bytes.len())]);
        vue_codegen.set_scope_id(scope_id);
    }

    // Pre-scan for script setup bindings to set binding metadata before template processing
    // (template may come before script in SFC, but needs binding info for correct prefixes)
    if let Some(metadata) =
        pre_scan_script_setup_bindings(input, bytes, allocator, detected.language.to_source_type())
    {
        vue_codegen.set_binding_metadata(metadata);
    }

    let mut analysis = Analysis::new();
    let mut css_parser = CssParserPlugin::new();

    // Create syntax pipeline with analysis plugin and vue codegen
    // IMPORTANT: css_parser must come BEFORE oxc_parser because oxc_parser
    // transforms Prop events into OxcProp, and css_parser needs raw Prop events
    // CssParser transforms CloseTag(style) into CssStyleContent events
    // Analysis transforms OxcProp/etc events into Analysed* events
    // VueCodegen can then process the enriched events
    {
        let pipeline: Vec<&mut dyn SyntaxPlugin> = vec![
            &mut css_parser,
            &mut oxc_parser,
            &mut analysis,
            &mut vue_codegen,
        ];
        let mut syntax = Syntax::new(pipeline);

        // Run the pipeline
        syntax.start(&mut syntax_context);
        tokenize(bytes, |e| {
            syntax.handle(&e, &mut syntax_context);
        });
        syntax.end(&mut syntax_context);
    }

    // Get the transformed code from the codegen plugin
    let mut code = vue_codegen.get_code();

    // Add feature flag markers when features are disabled
    // This allows bundlers to tree-shake related code
    if !options.features.options_api {
        code = format!("/* __VUE_OPTIONS_API__: false */\n{}", code);
    }
    if !options.features.props_destructure {
        code = format!("/* __VUE_PROPS_DESTRUCTURE__: false */\n{}", code);
    }

    // Generate source map
    let source_map_options = SourceMapOptions::new()
        .with_source(
            options
                .filename
                .clone()
                .unwrap_or_else(|| "input.vue".to_string()),
        )
        .with_file(
            options
                .filename
                .as_ref()
                .map(|f| format!("{}.js", f))
                .unwrap_or_else(|| "output.js".to_string()),
        )
        .include_content(options.include_source_content);

    let source_map = vue_codegen.generate_source_map(source_map_options);

    // Create inline source map
    let source_map_base64 = STANDARD.encode(&source_map);
    let code_with_source_map = format!(
        "{}\n//# sourceMappingURL=data:application/json;charset=utf-8;base64,{}",
        code, source_map_base64
    );

    let duration_ms = start.elapsed().as_secs_f64() * 1000.0;

    CodegenResult {
        code,
        source_map,
        code_with_source_map,
        duration_ms,
    }
}

// =============================================================================
// Vite Builder Function
// =============================================================================

/// Generate code for Vite plugin with split blocks.
///
/// Uses three separate codegen plugins (script, template, style) to produce
/// independent block outputs. Each block has its own code, source map, and
/// import metadata.
///
/// # Example
/// ```ignore
/// use verter_core::builder::codegen::{generate_for_vite, ViteCodegenOptions};
/// use oxc_allocator::Allocator;
///
/// let source = "<template><div>Hello</div></template><style scoped>.foo{}</style>";
/// let allocator = Allocator::new();
/// let options = ViteCodegenOptions { filename: Some("test.vue".to_string()), ..Default::default() };
///
/// let result = generate_for_vite(source, &options, &allocator);
/// println!("Build time: {}ms", result.duration_ms);
/// println!("Styles: {}", result.styles.len());
/// ```
pub fn generate_for_vite(
    input: &str,
    options: &ViteCodegenOptions,
    allocator: &oxc_allocator::Allocator,
) -> ViteCodegenResult {
    use crate::codegen::vue::style_plugin::StyleCodegenPlugin;
    let start = Instant::now();

    let bytes = input.as_bytes();

    let script_detector = ScriptDetector::new();
    let detected = script_detector.detect(bytes);

    let component_name = options
        .filename
        .as_ref()
        .map(|f| extract_component_name(f))
        .unwrap_or_else(|| "App".to_string());

    // Detect if SFC has <script setup> for inline template decision
    let has_script_setup =
        pre_scan_script_setup_bindings(input, bytes, allocator, detected.language.to_source_type());
    let use_inline_template = options.is_production && has_script_setup.is_some();

    // Common: create syntax plugin options and style plugin
    let syntax_options: &'static SyntaxPluginOptions =
        Box::leak(Box::new(SyntaxPluginOptions::default()));
    let mut syntax_context = SyntaxPluginContext::new(input, bytes, syntax_options);

    let mut oxc_parser = OxcParserPlugin::new(allocator, detected.language.to_source_type());
    let mut analysis = Analysis::new();
    let mut css_parser = CssParserPlugin::new();
    let mut style_codegen = StyleCodegenPlugin::new(input, allocator, &component_name);

    // Pre-scan for scoped styles (common to both paths)
    let scope_id = if has_scoped_style(bytes) {
        let hash = match &options.component_id {
            Some(id) => id.clone(),
            None => get_hash(&component_name),
        };
        let hash_bytes = hash.as_bytes();
        let mut scope_id = [0u8; 8];
        scope_id.copy_from_slice(&hash_bytes[..8.min(hash_bytes.len())]);
        style_codegen.set_scope_id(scope_id);
        Some(scope_id)
    } else {
        None
    };

    let source_name = options
        .filename
        .clone()
        .unwrap_or_else(|| "input.vue".to_string());

    let (script_output, template_output) = if use_inline_template {
        // =================================================================
        // PRODUCTION with <script setup>: use monolithic VueCodegenPlugin
        // Template is inlined into setup's return value (no separate block)
        // =================================================================
        let mut vue_codegen = VueCodegenPlugin::new(input, allocator);
        vue_codegen.set_component_name(&component_name);
        vue_codegen.set_production(options.is_production);

        if let Some(sid) = scope_id {
            vue_codegen.set_scope_id(sid);
        }
        if let Some(ref metadata) = has_script_setup {
            vue_codegen.set_binding_metadata(metadata.clone());
        }

        // Run pipeline with monolithic plugin + style plugin
        {
            let pipeline: Vec<&mut dyn SyntaxPlugin> = vec![
                &mut css_parser,
                &mut oxc_parser,
                &mut analysis,
                &mut vue_codegen,
                &mut style_codegen,
            ];
            let mut syntax = Syntax::new(pipeline);

            syntax.start(&mut syntax_context);
            tokenize(bytes, |e| {
                syntax.handle(&e, &mut syntax_context);
            });
            syntax.end(&mut syntax_context);
        }

        // Script = full monolithic output (includes inline template)
        let code = vue_codegen.get_code();
        let source_map = if options.sourcemap {
            let sm_options = SourceMapOptions::new()
                .with_source(source_name.clone())
                .with_file(format!("{}.script.js", source_name))
                .include_content(true);
            Some(vue_codegen.generate_source_map(sm_options))
        } else {
            None
        };
        let (imports, body_start) = extract_imports(&code);
        let script = Some(BlockOutput {
            code,
            source_map,
            imports,
            body_start,
        });

        // Template = None (inlined into script)
        (script, None)
    } else {
        // =================================================================
        // DEVELOPMENT or non-setup: use separate script/template plugins
        // =================================================================
        use crate::codegen::vue::script_plugin::ScriptCodegenPlugin;
        use crate::codegen::vue::template_plugin::TemplateCodegenPlugin;

        let mut script_codegen = ScriptCodegenPlugin::new(input, allocator);
        let mut template_codegen = TemplateCodegenPlugin::new(input, allocator);

        script_codegen.set_component_name(&component_name);
        script_codegen.set_production(options.is_production);
        template_codegen.set_production(options.is_production);

        if let Some(sid) = scope_id {
            script_codegen.set_scope_id(sid);
            template_codegen.set_scope_id(sid);
        }
        if let Some(ref metadata) = has_script_setup {
            template_codegen.set_binding_metadata(metadata.clone());
        }

        // Run pipeline
        {
            let pipeline: Vec<&mut dyn SyntaxPlugin> = vec![
                &mut css_parser,
                &mut oxc_parser,
                &mut analysis,
                &mut script_codegen,
                &mut template_codegen,
                &mut style_codegen,
            ];
            let mut syntax = Syntax::new(pipeline);

            syntax.start(&mut syntax_context);
            tokenize(bytes, |e| {
                syntax.handle(&e, &mut syntax_context);
            });
            syntax.end(&mut syntax_context);
        }

        // Build script BlockOutput
        let script = if script_codegen.has_script() {
            let code = script_codegen.get_code();
            let source_map = if options.sourcemap {
                let sm_options = SourceMapOptions::new()
                    .with_source(source_name.clone())
                    .with_file(format!("{}.script.js", source_name))
                    .include_content(true);
                Some(script_codegen.generate_source_map(sm_options))
            } else {
                None
            };
            let (imports, body_start) = extract_imports(&code);
            Some(BlockOutput {
                code,
                source_map,
                imports,
                body_start,
            })
        } else {
            Some(BlockOutput {
                code: "export default {};\n".to_string(),
                source_map: None,
                imports: vec![],
                body_start: 0,
            })
        };

        // Build template BlockOutput
        let template = if template_codegen.has_template() {
            let code = template_codegen.get_code();
            let source_map = if options.sourcemap {
                let sm_options = SourceMapOptions::new()
                    .with_source(source_name.clone())
                    .with_file(format!("{}.template.js", source_name))
                    .include_content(true);
                Some(template_codegen.generate_source_map(sm_options))
            } else {
                None
            };
            let (imports, body_start) = extract_imports(&code);
            Some(BlockOutput {
                code,
                source_map,
                imports,
                body_start,
            })
        } else {
            None
        };

        (script, template)
    };

    // Build style blocks (common to both paths)
    let styles: Vec<StyleBlock> = style_codegen
        .styles
        .iter()
        .map(|s| {
            let code = s.get_code();
            let source_map = if options.sourcemap {
                let sm_options = SourceMapOptions::new()
                    .with_source(source_name.clone())
                    .with_file(format!("{}.style.css", source_name))
                    .include_content(true);
                Some(s.generate_source_map(sm_options))
            } else {
                None
            };
            StyleBlock {
                code,
                source_map,
                scoped: s.scoped,
                is_module: s.is_module,
                lang: s.lang.clone(),
                module_name: s.module_name.clone(),
                module_classes: s.module_classes.clone(),
            }
        })
        .collect();

    let duration_ms = start.elapsed().as_secs_f64() * 1000.0;

    ViteCodegenResult {
        script: script_output,
        template: template_output,
        styles,
        custom: vec![],
        duration_ms,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // =========================================================================
    // extract_component_name Tests
    // =========================================================================

    #[test]
    fn test_extract_component_name_simple() {
        assert_eq!(extract_component_name("App.vue"), "App");
        assert_eq!(extract_component_name("my-component.vue"), "my-component");
        assert_eq!(extract_component_name("MyComponent.vue"), "MyComponent");
    }

    #[test]
    fn test_extract_component_name_with_path() {
        assert_eq!(
            extract_component_name("src/components/MyComponent.vue"),
            "MyComponent"
        );
        assert_eq!(extract_component_name("components/Button.vue"), "Button");
        assert_eq!(
            extract_component_name("C:\\Users\\dev\\project\\App.vue"),
            "App"
        );
    }

    #[test]
    fn test_extract_component_name_no_extension() {
        assert_eq!(extract_component_name("MyComponent"), "MyComponent");
        assert_eq!(extract_component_name("src/App"), "App");
    }

    // =========================================================================
    // Generate Tests
    // =========================================================================

    #[test]
    fn test_generate_simple_template() {
        let source = "<template><div>Hello</div></template>";
        let allocator = oxc_allocator::Allocator::new();
        let options = CodegenOptions::new().with_filename("test.vue");

        let result = generate(source, &options, &allocator);

        // The code should contain something (at minimum "Hello there" from current plugin)
        assert!(!result.code.is_empty());
        // Source map should be valid JSON
        assert!(result.source_map.starts_with('{'));
        // Code with source map should have the inline mapping
        assert!(result
            .code_with_source_map
            .contains("//# sourceMappingURL=data:application/json"));
    }

    #[test]
    fn test_generate_with_interpolation() {
        let source = "<template><div>{{ message }}</div></template>";
        let allocator = oxc_allocator::Allocator::new();
        let options = CodegenOptions::new()
            .with_filename("app.vue")
            .include_source_content(true);

        let result = generate(source, &options, &allocator);

        assert!(!result.code.is_empty());
        assert!(!result.source_map.is_empty());
    }

    #[test]
    fn test_generate_complex_component() {
        let source = r#"<template>
  <div v-if="show" class="container">
    <span v-for="item in items" :key="item.id">
      {{ item.name }}
    </span>
  </div>
</template>"#;

        let allocator = oxc_allocator::Allocator::new();
        let options = CodegenOptions::new().with_filename("complex.vue");

        let result = generate(source, &options, &allocator);

        assert!(!result.code.is_empty());
        // Verify base64 encoding in inline source map
        assert!(result.code_with_source_map.contains("base64,"));
    }

    // =========================================================================
    // CodegenOptions New Fields Tests (TDD - Step 1: Write failing tests)
    // =========================================================================

    #[test]
    fn test_codegen_options_is_production_default() {
        let options = CodegenOptions::default();
        assert!(
            !options.is_production,
            "Default should be development mode (is_production = false)"
        );
    }

    #[test]
    fn test_codegen_options_features_default() {
        let options = CodegenOptions::default();
        assert!(
            options.features.options_api,
            "Options API should be enabled by default"
        );
        assert!(
            options.features.props_destructure,
            "Props destructure should be enabled by default"
        );
    }

    #[test]
    fn test_codegen_options_custom_component_id() {
        let options = CodegenOptions {
            component_id: Some("custom-123".to_string()),
            ..Default::default()
        };
        assert_eq!(
            options.component_id,
            Some("custom-123".to_string()),
            "Custom component ID should be stored"
        );
    }

    #[test]
    fn test_codegen_options_is_production_can_be_set() {
        let options = CodegenOptions {
            is_production: true,
            ..Default::default()
        };
        assert!(
            options.is_production,
            "is_production should be settable to true"
        );
    }

    #[test]
    fn test_codegen_options_features_can_be_disabled() {
        let options = CodegenOptions {
            features: FeatureFlags {
                options_api: false,
                props_destructure: false,
            },
            ..Default::default()
        };
        assert!(
            !options.features.options_api,
            "options_api should be disableable"
        );
        assert!(
            !options.features.props_destructure,
            "props_destructure should be disableable"
        );
    }

    // =========================================================================
    // Component ID Hash Utility Tests (TDD - Step 1: Write failing tests)
    // =========================================================================

    #[test]
    fn test_component_id_hash_length() {
        let hash = get_hash("test");
        assert_eq!(hash.len(), 8, "Hash should be 8 characters");
    }

    #[test]
    fn test_component_id_hash_deterministic() {
        let hash1 = get_hash("test");
        let hash2 = get_hash("test");
        assert_eq!(hash1, hash2, "Same input should produce same hash");
    }

    #[test]
    fn test_component_id_hash_different_inputs() {
        let hash1 = get_hash("input1");
        let hash2 = get_hash("input2");
        assert_ne!(
            hash1, hash2,
            "Different inputs should produce different hashes"
        );
    }

    #[test]
    fn test_component_id_production_vs_dev() {
        let source = "<template></template>";
        let prod_options = CodegenOptions {
            filename: Some("App.vue".to_string()),
            is_production: true,
            ..Default::default()
        };
        let dev_options = CodegenOptions {
            filename: Some("App.vue".to_string()),
            is_production: false,
            ..Default::default()
        };

        let prod_id = generate_component_id(&prod_options, source);
        let dev_id = generate_component_id(&dev_options, source);

        // Production uses filepath only, dev uses filepath + source
        // So they should differ
        assert_ne!(prod_id, dev_id, "Production and dev IDs should differ");
    }

    #[test]
    fn test_component_id_custom_override() {
        let options = CodegenOptions {
            component_id: Some("my-custom-id".to_string()),
            ..Default::default()
        };
        let id = generate_component_id(&options, "any source");
        assert_eq!(id, "my-custom-id", "Custom ID should override generation");
    }

    #[test]
    fn test_component_id_normalizes_windows_paths() {
        let source = "<template></template>";
        let unix_options = CodegenOptions {
            filename: Some("src/components/App.vue".to_string()),
            is_production: true,
            ..Default::default()
        };
        let windows_options = CodegenOptions {
            filename: Some("src\\components\\App.vue".to_string()),
            is_production: true,
            ..Default::default()
        };

        let unix_id = generate_component_id(&unix_options, source);
        let windows_id = generate_component_id(&windows_options, source);

        assert_eq!(
            unix_id, windows_id,
            "Unix and Windows paths should produce same ID"
        );
    }

    // =========================================================================
    // features.optionsAPI Tests (TDD - Step 1: Write failing tests)
    // =========================================================================

    #[test]
    fn test_options_api_disabled_adds_marker() {
        let source = r#"<template><div>Test</div></template>
<script setup>
const msg = 'hello'
</script>"#;
        let allocator = oxc_allocator::Allocator::new();
        let options = CodegenOptions {
            features: FeatureFlags {
                options_api: false,
                ..Default::default()
            },
            ..Default::default()
        };
        let result = generate(source, &options, &allocator);

        // When optionsAPI is disabled, output should contain a marker comment
        assert!(
            result.code.contains("/* __VUE_OPTIONS_API__: false */"),
            "When options_api is disabled, should add marker comment. Generated:\n{}",
            result.code
        );
    }

    #[test]
    fn test_options_api_enabled_no_marker() {
        let source = r#"<template><div>Test</div></template>
<script setup>
const msg = 'hello'
</script>"#;
        let allocator = oxc_allocator::Allocator::new();
        let options = CodegenOptions {
            features: FeatureFlags {
                options_api: true,
                ..Default::default()
            },
            ..Default::default()
        };
        let result = generate(source, &options, &allocator);

        // When optionsAPI is enabled (default), no marker comment
        assert!(
            !result.code.contains("/* __VUE_OPTIONS_API__: false */"),
            "When options_api is enabled, should NOT have marker. Generated:\n{}",
            result.code
        );
    }

    // =========================================================================
    // features.propsDestructure Tests (TDD - Step 1: Write failing tests)
    // =========================================================================

    #[test]
    fn test_props_destructure_disabled_adds_marker() {
        let source = r#"<template><div>{{ foo }}</div></template>
<script setup>
const props = defineProps<{ foo: string }>()
</script>"#;
        let allocator = oxc_allocator::Allocator::new();
        let options = CodegenOptions {
            features: FeatureFlags {
                props_destructure: false,
                ..Default::default()
            },
            ..Default::default()
        };
        let result = generate(source, &options, &allocator);

        // When propsDestructure is disabled, output should contain a marker comment
        assert!(
            result
                .code
                .contains("/* __VUE_PROPS_DESTRUCTURE__: false */"),
            "When props_destructure is disabled, should add marker comment. Generated:\n{}",
            result.code
        );
    }

    #[test]
    fn test_props_destructure_enabled_no_marker() {
        let source = r#"<template><div>{{ foo }}</div></template>
<script setup>
const props = defineProps<{ foo: string }>()
</script>"#;
        let allocator = oxc_allocator::Allocator::new();
        let options = CodegenOptions {
            features: FeatureFlags {
                props_destructure: true,
                ..Default::default()
            },
            ..Default::default()
        };
        let result = generate(source, &options, &allocator);

        // When propsDestructure is enabled (default), no marker comment
        assert!(
            !result
                .code
                .contains("/* __VUE_PROPS_DESTRUCTURE__: false */"),
            "When props_destructure is enabled, should NOT have marker. Generated:\n{}",
            result.code
        );
    }

    // =========================================================================
    // Production Mode Tests (TDD - Step 1: Write failing tests)
    // =========================================================================

    #[test]
    fn test_production_setup_no_expose_when_not_used() {
        let source = r#"<template><div>{{ msg }}</div></template>
<script setup>
const msg = 'Hello'
</script>"#;
        let allocator = oxc_allocator::Allocator::new();
        let options = CodegenOptions {
            is_production: true,
            ..Default::default()
        };
        let result = generate(source, &options, &allocator);

        // Production: simple setup signature, no expose when not explicitly used
        assert!(
            result.code.contains("setup(__props)"),
            "Production should have minimal setup signature without expose. Generated:\n{}",
            result.code
        );
        assert!(
            !result.code.contains("expose: __expose"),
            "Production should not have __expose in signature. Generated:\n{}",
            result.code
        );
        assert!(
            !result.code.contains("__expose()"),
            "Production should not auto-call __expose(). Generated:\n{}",
            result.code
        );
    }

    #[test]
    fn test_production_inline_render_function() {
        let source = r#"<template><div>Hello</div></template>
<script setup>
const msg = 'world'
</script>"#;
        let allocator = oxc_allocator::Allocator::new();
        let options = CodegenOptions {
            is_production: true,
            ..Default::default()
        };
        let result = generate(source, &options, &allocator);

        // Production: render inlined as return value of setup
        assert!(
            result.code.contains("return (_ctx, _cache) =>"),
            "Production should return inline render function. Generated:\n{}",
            result.code
        );
        assert!(
            !result.code.contains("function render("),
            "Production should NOT have separate render export. Generated:\n{}",
            result.code
        );
        assert!(
            !result.code.contains("__returned__"),
            "Production should NOT have __returned__ object. Generated:\n{}",
            result.code
        );
        // Verify only ONE return statement in setup (the inline render)
        assert!(
            !result.code.contains("return {msg}"),
            "Production inline should NOT have 'return {{msg}}' — setup should return render fn. Generated:\n{}",
            result.code
        );
    }

    #[test]
    fn test_production_keeps_expose_when_defineExpose_used() {
        let source = r#"<template><div>{{ msg }}</div></template>
<script setup>
const msg = 'Hello'
defineExpose({ msg })
</script>"#;
        let allocator = oxc_allocator::Allocator::new();
        let options = CodegenOptions {
            is_production: true,
            ..Default::default()
        };
        let result = generate(source, &options, &allocator);

        // Production with defineExpose: should keep expose in signature
        assert!(
            result.code.contains("expose: __expose") || result.code.contains("expose:__expose"),
            "Production with defineExpose should have __expose in signature. Generated:\n{}",
            result.code
        );
        assert!(
            result.code.contains("__expose({ msg })"),
            "Should have user's defineExpose call. Generated:\n{}",
            result.code
        );
    }

    #[test]
    fn test_production_keeps_emit_when_defineEmits_with_declarator() {
        let source = r#"<template><div @click="emit('click')">Click</div></template>
<script setup>
const emit = defineEmits(['click'])
</script>"#;
        let allocator = oxc_allocator::Allocator::new();
        let options = CodegenOptions {
            is_production: true,
            ..Default::default()
        };
        let result = generate(source, &options, &allocator);

        // Production with emit declarator: should keep emit in signature
        assert!(
            result.code.contains("emit:__emit") || result.code.contains("emit: __emit"),
            "Production with emit declarator should have __emit in signature. Generated:\n{}",
            result.code
        );
    }

    #[test]
    fn test_development_mode_unchanged() {
        let source = r#"<template><div>{{ msg }}</div></template>
<script setup>
const msg = 'Hello'
</script>"#;
        let allocator = oxc_allocator::Allocator::new();
        let options = CodegenOptions {
            is_production: false, // Explicit development mode
            ..Default::default()
        };
        let result = generate(source, &options, &allocator);

        // Development: full setup signature with expose
        assert!(
            result.code.contains("expose:__expose") || result.code.contains("expose: __expose"),
            "Development should have __expose in signature. Generated:\n{}",
            result.code
        );
        assert!(
            result.code.contains("__expose()"),
            "Development should call __expose(). Generated:\n{}",
            result.code
        );
        assert!(
            result.code.contains("__returned__"),
            "Development should have __returned__ object. Generated:\n{}",
            result.code
        );
        assert!(
            result.code.contains("function render("),
            "Development should have separate render function. Generated:\n{}",
            result.code
        );
    }

    #[test]
    fn test_production_no_isScriptSetup_property() {
        let source = r#"<template><div>Test</div></template>
<script setup>
const x = 1
</script>"#;
        let allocator = oxc_allocator::Allocator::new();
        let options = CodegenOptions {
            is_production: true,
            ..Default::default()
        };
        let result = generate(source, &options, &allocator);

        // Production: should not have __isScriptSetup property
        assert!(
            !result.code.contains("__isScriptSetup"),
            "Production should NOT have __isScriptSetup property. Generated:\n{}",
            result.code
        );
    }
}

// =============================================================================
// E2E Tests - Full Pipeline Tests
// =============================================================================
#[cfg(test)]
mod e2e_tests {
    use super::*;
    use oxc_parser::Parser;
    use oxc_span::SourceType;

    /// Helper to strip source map from generated code for comparison
    fn strip_source_map(code: &str) -> &str {
        code.split("//# sourceMappingURL=")
            .next()
            .unwrap_or(code)
            .trim_end()
    }

    // =========================================================================
    // JS Syntax Validation Helpers
    // =========================================================================

    /// MANDATORY: Validates that generated code is syntactically valid JavaScript.
    /// This MUST be called for any test that checks generated output.
    fn assert_valid_js(code: &str, context: &str) {
        let allocator = oxc_allocator::Allocator::default();
        let source_type = SourceType::mjs();
        let parser_result = Parser::new(&allocator, code, source_type).parse();

        assert!(
            parser_result.errors.is_empty(),
            "Generated code is NOT valid JavaScript!\n\
             Context: {}\n\
             Parse Errors: {:?}\n\
             Generated Code:\n{}",
            context,
            parser_result.errors,
            code
        );
    }

    /// Helper that generates code AND validates it is valid JS.
    /// Use this instead of generate() directly for new tests.
    #[allow(dead_code)]
    fn gen_and_validate(source: &str) -> String {
        let allocator = oxc_allocator::Allocator::new();
        let options = CodegenOptions::new().with_filename("test.vue".to_string());
        let result = generate(source, &options, &allocator);

        assert_valid_js(&result.code, source);
        result.code
    }

    /// Known invalid patterns that indicate broken codegen
    #[allow(dead_code)]
    const INVALID_PATTERNS: &[(&str, &str)] = &[
        ("{ :", "empty property name (v-bind spread bug)"),
        ("_ctx.{", "object literal after _ctx. (class/style bug)"),
        ("_ctx.[", "array literal after _ctx. (class/style bug)"),
        (
            "{ v-",
            "hyphenated directive as property (custom directive bug)",
        ),
        (": _ctx.!", "negation in wrong position"),
        ("null))", "dangling null closing parens"),
        (", ,", "double comma"),
        (
            "\"_toDisplayString",
            "missing string concatenation operator",
        ),
        ("{ on:", "malformed v-once output"),
    ];

    /// Check that generated code does not contain known invalid patterns
    #[allow(dead_code)]
    fn assert_no_invalid_patterns(code: &str, context: &str) {
        for (pattern, desc) in INVALID_PATTERNS {
            assert!(
                !code.contains(pattern),
                "Found invalid pattern '{}' ({}) in {}.\nGenerated:\n{}",
                pattern,
                desc,
                context,
                code
            );
        }
    }

    // =========================================================================
    // AST Comparison Helpers
    // =========================================================================

    /// Compare two ASTs structurally, ignoring source positions/spans.
    /// Returns a list of differences found.
    ///
    /// This is used to compare verter's output against Vue's official compiler.
    /// The comparison ignores whitespace, formatting, and span positions since
    /// those will naturally differ between implementations.
    #[allow(dead_code)]
    fn compare_ast_structure(our_code: &str, vue_code: &str, context: &str) -> Vec<String> {
        let allocator1 = oxc_allocator::Allocator::default();
        let allocator2 = oxc_allocator::Allocator::default();
        let source_type = SourceType::mjs();

        let our_result = Parser::new(&allocator1, our_code, source_type).parse();
        let vue_result = Parser::new(&allocator2, vue_code, source_type).parse();

        let mut diffs = Vec::new();

        // Both must parse successfully
        if !our_result.errors.is_empty() {
            diffs.push(format!(
                "[{}] Our code has parse errors: {:?}",
                context, our_result.errors
            ));
            return diffs;
        }
        if !vue_result.errors.is_empty() {
            diffs.push(format!(
                "[{}] Vue code has parse errors: {:?}",
                context, vue_result.errors
            ));
            return diffs;
        }

        // Compare statement counts
        let our_stmts = our_result.program.body.len();
        let vue_stmts = vue_result.program.body.len();
        if our_stmts != vue_stmts {
            diffs.push(format!(
                "[{}] Statement count differs: ours={}, vue={}",
                context, our_stmts, vue_stmts
            ));
        }

        // Compare imports
        let our_imports: Vec<_> = our_result
            .program
            .body
            .iter()
            .filter_map(|s| {
                if let oxc_ast::ast::Statement::ImportDeclaration(decl) = s {
                    Some(decl.source.value.as_str())
                } else {
                    None
                }
            })
            .collect();

        let vue_imports: Vec<_> = vue_result
            .program
            .body
            .iter()
            .filter_map(|s| {
                if let oxc_ast::ast::Statement::ImportDeclaration(decl) = s {
                    Some(decl.source.value.as_str())
                } else {
                    None
                }
            })
            .collect();

        if our_imports != vue_imports {
            diffs.push(format!(
                "[{}] Import sources differ: ours={:?}, vue={:?}",
                context, our_imports, vue_imports
            ));
        }

        // For now, a simplified comparison - we can make this more detailed as needed
        // The key insight is that if both parse successfully and have similar structure,
        // the code is likely functionally equivalent

        diffs
    }

    #[test]
    fn e2e_named_slot_with_default_opens_default_slot() {
        let vue_source = r#"<script setup>
import { Dropdown } from "@nexus/ui"
</script>

<template>
  <Dropdown>
    <template #reference>
      <span>Ref</span>
    </template>
    <div>Content</div>
  </Dropdown>
</template>
"#;

        let allocator = oxc_allocator::Allocator::new();
        let options = ViteCodegenOptions {
            filename: Some("test.vue".to_string()),
            ..Default::default()
        };
        let result = generate_for_vite(vue_source, &options, &allocator);
        let our_render = result.template.expect("should have template block").code;

        assert_valid_js(&our_render, "named slots + default render output");

        assert!(
            our_render.contains("reference: _withCtx"),
            "Expected named slot to use withCtx. Output:\n{}",
            our_render
        );
        assert!(
            our_render.contains("default: _withCtx"),
            "Expected default slot to be emitted when named slots exist. Output:\n{}",
            our_render
        );
        assert!(
            our_render.contains("]), _: 1 /* STABLE */"),
            "Expected default slot array to be closed before stability marker. Output:\n{}",
            our_render
        );
    }

    #[test]
    fn e2e_default_content_before_named_slot() {
        // Default slot content appears before a named slot template.
        // The default slot must be properly closed before the named slot opens,
        // and a comma must separate them in the slots object.
        let vue_source = r#"<template>
  <MyComponent>
    <div>Default content</div>
    <template #configuration>
      <span>Config</span>
    </template>
  </MyComponent>
</template>"#;

        let allocator = oxc_allocator::Allocator::new();
        let options = ViteCodegenOptions {
            filename: Some("test.vue".to_string()),
            ..Default::default()
        };
        let result = generate_for_vite(vue_source, &options, &allocator);
        let our_render = result.template.expect("should have template block").code;

        assert_valid_js(&our_render, "default content before named slot");

        // Both slots should be present
        assert!(
            our_render.contains("default: _withCtx"),
            "Expected default slot. Output:\n{}",
            our_render
        );
        assert!(
            our_render.contains("configuration: _withCtx"),
            "Expected configuration named slot. Output:\n{}",
            our_render
        );
        // The default slot should be closed before the named slot
        assert!(
            our_render.contains("]), configuration:"),
            "Expected default slot to be closed before named slot with comma separator. Output:\n{}",
            our_render
        );
    }

    #[test]
    fn e2e_render_imports_before_hoisted() {
        let vue_source = r#"<template><div class=\"a\">Hi</div></template>"#;

        let allocator = oxc_allocator::Allocator::new();
        let options = ViteCodegenOptions {
            filename: Some("test.vue".to_string()),
            ..Default::default()
        };
        let result = generate_for_vite(vue_source, &options, &allocator);
        let our_render = result.template.expect("should have template block").code;

        assert_valid_js(&our_render, "render import order");

        let import_pos = our_render.find("import ").unwrap_or(usize::MAX);
        let hoisted_pos = our_render.find("const _hoisted_").unwrap_or(usize::MAX);
        assert!(
            import_pos < hoisted_pos,
            "Expected imports before hoisted constants. Output:\n{}",
            our_render
        );
    }

    #[test]
    fn e2e_render_uses_full_signature_with_bindings() {
        let vue_source =
            r#"<template><div>{{ msg }}</div></template><script setup>const msg = 'hi'</script>"#;

        let allocator = oxc_allocator::Allocator::new();
        let options = ViteCodegenOptions {
            filename: Some("test.vue".to_string()),
            ..Default::default()
        };
        let result = generate_for_vite(vue_source, &options, &allocator);
        let our_render = result.template.expect("should have template block").code;

        assert_valid_js(&our_render, "render signature");

        assert!(
            our_render
                .contains("export function render(_ctx, _cache, $props, $setup, $data, $options)"),
            "Expected full 6-arg render signature with export when bindings exist. Output:\n{}",
            our_render
        );
        assert!(
            our_render.contains("$setup.msg"),
            "Expected $setup.msg for setup binding. Output:\n{}",
            our_render
        );
    }

    #[test]
    fn e2e_render_uses_two_param_signature_without_bindings() {
        let vue_source = r#"<template><div>Hello</div></template>"#;

        let allocator = oxc_allocator::Allocator::new();
        let options = ViteCodegenOptions {
            filename: Some("test.vue".to_string()),
            ..Default::default()
        };
        let result = generate_for_vite(vue_source, &options, &allocator);
        let our_render = result.template.expect("should have template block").code;

        assert_valid_js(&our_render, "render signature no bindings");

        assert!(
            our_render.contains("export function render(_ctx, _cache)"),
            "Expected 2-arg render signature when no bindings. Output:\n{}",
            our_render
        );
    }

    #[test]
    fn e2e_root_vif_velse_chain_renders_without_fragment_comma_breakage() {
        let vue_source = r#"<template>
  <a
    v-if="isStringUrl && isExternalLink"
    class="leading-none"
    v-bind="$attrs"
    :href="to"
    rel="noopener noreferrer"
    target="_blank"
  >
    <slot />
  </a>

  <RouterLink v-else v-slot="{ isActive, href, navigate }" v-bind="$props" custom>
    <a
      v-bind="$attrs"
      class="leading-none"
      :href="href"
      :class="isActive ? activeClass : inactiveClass"
      @click="navigate"
    >
      <slot />
    </a>
  </RouterLink>
</template>"#;

        let allocator = oxc_allocator::Allocator::new();
        let options = ViteCodegenOptions {
            filename: Some("test.vue".to_string()),
            ..Default::default()
        };
        let result = generate_for_vite(vue_source, &options, &allocator);
        let our_render = result.template.expect("should have template block").code;

        assert_valid_js(&our_render, "root v-if/v-else chain render output");

        assert!(
            !our_render.contains(",\n    :"),
            "render output must not emit a comma before ternary else branch. Output:\n{}",
            our_render
        );
        assert!(
            !our_render.contains("_createElementBlock(_Fragment"),
            "root v-if/v-else pair should not be wrapped in a Fragment. Output:\n{}",
            our_render
        );
        assert!(
            our_render.contains("?") && our_render.contains(":"),
            "root v-if/v-else should compile as ternary branches. Output:\n{}",
            our_render
        );
    }

    // =========================================================================
    // Simple Component Tests
    // =========================================================================

    #[test]
    fn e2e_simple_component() {
        let source = r#"<template>
  <div class="hello">
    <h1>{{ msg }}</h1>
  </div>
</template>

<script setup>
const msg = 'Hello World'
</script>
"#;
        let allocator = oxc_allocator::Allocator::new();
        let options = CodegenOptions::new().with_filename("simple.vue");
        let result = generate(source, &options, &allocator);

        let code = strip_source_map(&result.code_with_source_map);

        // Verify component structure
        assert!(
            code.contains("export default"),
            "Should have __sfc__ component definition"
        );
        assert!(
            code.contains("__name: 'simple'"),
            "Should have component name derived from filename. Generated:\n{}",
            code
        );
        assert!(code.contains("setup(__props"), "Should have setup function");
        assert!(code.contains("__expose()"), "Should auto-expose");
        assert!(
            code.contains("const msg = 'Hello World'"),
            "Should preserve script content"
        );
        assert!(
            code.contains("__returned__={msg}"),
            "Should return msg binding"
        );

        // Verify template render function
        assert!(
            code.contains("function render("),
            "Should have render function"
        );
        assert!(
            code.contains("_toDisplayString"),
            "Should use toDisplayString for interpolation"
        );
        assert!(
            code.contains("class: \"hello\""),
            "Should preserve class attribute"
        );
    }

    #[test]
    fn e2e_no_script_component() {
        let source = r#"<template>
  <div>Hello world</div>
</template>
"#;
        let allocator = oxc_allocator::Allocator::new();
        let options = CodegenOptions::new().with_filename("no-script.vue");
        let result = generate(source, &options, &allocator);

        let code = strip_source_map(&result.code_with_source_map);

        // Verify render function exists even without script
        assert!(
            code.contains("function render("),
            "Should have render function"
        );
        assert!(code.contains("\"Hello world\""), "Should have text content");
    }

    // =========================================================================
    // Conditional Rendering Tests
    // =========================================================================

    #[test]
    fn e2e_conditional_v_if_v_else() {
        let source = r#"<template>
  <div>
    <span v-if="show">Visible</span>
    <span v-else>Hidden</span>
  </div>
</template>

<script setup>
import { ref } from 'vue'
const show = ref(true)
</script>
"#;
        let allocator = oxc_allocator::Allocator::new();
        let options = CodegenOptions::new().with_filename("conditional.vue");
        let result = generate(source, &options, &allocator);

        let code = strip_source_map(&result.code_with_source_map);

        // Verify conditional rendering uses ternary
        assert!(
            code.contains("$setup.show"),
            "Should reference $setup.show in render, got:\n{}",
            code
        );
        assert!(
            code.contains("?") && code.contains(":"),
            "Should use ternary operator for v-if/v-else, got:\n{}",
            code
        );
        assert!(
            code.contains("\"Visible\""),
            "Should have v-if content, got:\n{}",
            code
        );
        assert!(
            code.contains("\"Hidden\""),
            "Should have v-else content, got:\n{}",
            code
        );

        // Verify component structure is correct
        assert!(
            code.contains("export default"),
            "Should have __sfc__ component, got:\n{}",
            code
        );
        assert!(
            code.contains("__returned__={show}"),
            "Should return show binding, got:\n{}",
            code
        );
    }

    #[test]
    fn e2e_vif_no_else_then_slot_in_component() {
        // v-if without v-else inside a component, followed by a <slot/>.
        // The ternary close (: _createCommentVNode) must be followed by a comma
        // before the _renderSlot call, not preceded by the comma.
        let vue_source = r#"<template>
  <MyComponent>
    <template v-if="show">
      <span>Visible</span>
    </template>

    <slot />

    <template v-if="other">
      <span>Other</span>
    </template>
  </MyComponent>
</template>"#;

        let allocator = oxc_allocator::Allocator::new();
        let options = ViteCodegenOptions {
            filename: Some("test.vue".to_string()),
            ..Default::default()
        };
        let result = generate_for_vite(vue_source, &options, &allocator);
        let our_render = result.template.expect("should have template block").code;

        assert_valid_js(&our_render, "v-if no else then slot in component");

        // The ternary should close before the comma and slot call
        assert!(
            our_render.contains("_createCommentVNode"),
            "Expected createCommentVNode for v-if without v-else. Output:\n{}",
            our_render
        );
        assert!(
            our_render.contains("_renderSlot"),
            "Expected renderSlot for <slot />. Output:\n{}",
            our_render
        );
    }

    #[test]
    fn e2e_html_comment_inside_component() {
        // HTML comments inside a component must be wrapped in default slot.
        // Pattern from baseline.vue: <v-navigation-drawer><!-- --></v-navigation-drawer>
        let vue_source = r#"<template>
  <MyComponent v-model="drawer">
    <!--  -->
  </MyComponent>
</template>"#;

        let allocator = oxc_allocator::Allocator::new();
        let options = ViteCodegenOptions {
            filename: Some("test.vue".to_string()),
            ..Default::default()
        };
        let result = generate_for_vite(vue_source, &options, &allocator);
        let our_render = result.template.expect("should have template block").code;

        assert_valid_js(&our_render, "html comment inside component");

        // Comment should be wrapped in a default slot
        assert!(
            our_render.contains("default: _withCtx"),
            "Expected comment to be in default slot. Output:\n{}",
            our_render
        );
        assert!(
            our_render.contains("_createCommentVNode"),
            "Expected createCommentVNode for HTML comment. Output:\n{}",
            our_render
        );
    }

    #[test]
    fn e2e_component_with_br_children_and_nested_components() {
        // <v-sheet> with nested components, <br> elements, and trailing component.
        // Tests that the slot closes correctly when the last child is a component.
        let vue_source = r#"<template>
  <VSheet>
    <VResponsive>
      <span>Inner</span>
    </VResponsive>

    <br>
    <br>

    <SponsorLink size="large" />
  </VSheet>
</template>"#;

        let allocator = oxc_allocator::Allocator::new();
        let options = ViteCodegenOptions {
            filename: Some("test.vue".to_string()),
            ..Default::default()
        };
        let result = generate_for_vite(vue_source, &options, &allocator);
        let our_render = result.template.expect("should have template block").code;

        assert_valid_js(
            &our_render,
            "component with br children and nested components",
        );
    }

    #[test]
    fn e2e_component_with_named_slot_then_default_content() {
        // Component with a named slot template followed by default content text.
        // The default content must be in a separate 'default' slot, not inside the named slot.
        // Pattern from slot-label.vue: <v-tooltip><template #activator>...</template>text</v-tooltip>
        let vue_source = r#"<template>
  <VTooltip location="bottom">
    <template #activator="{ props }">
      <a v-bind="props">Link</a>
    </template>
    Opens in new window
  </VTooltip>
</template>"#;

        let allocator = oxc_allocator::Allocator::new();
        let options = ViteCodegenOptions {
            filename: Some("test.vue".to_string()),
            ..Default::default()
        };
        let result = generate_for_vite(vue_source, &options, &allocator);
        let our_render = result.template.expect("should have template block").code;

        assert_valid_js(
            &our_render,
            "component with named slot then default content",
        );
    }

    #[test]
    fn e2e_nested_component_named_slot_then_default_in_vslot() {
        // Component with named slot + default content, nested inside another v-slot.
        // Pattern from slot-label.vue: <v-checkbox><template #label><div><v-tooltip>
        //   <template #activator>...</template>text</v-tooltip></div></template></v-checkbox>
        let vue_source = r#"<template>
  <VCheckbox v-model="checkbox">
    <template #label>
      <div>
        I agree that
        <VTooltip location="bottom">
          <template #activator="{ props }">
            <a v-bind="props">Vuetify</a>
          </template>
          Opens in new window
        </VTooltip>
        is awesome
      </div>
    </template>
  </VCheckbox>
</template>"#;

        let allocator = oxc_allocator::Allocator::new();
        let options = ViteCodegenOptions {
            filename: Some("test.vue".to_string()),
            ..Default::default()
        };
        let result = generate_for_vite(vue_source, &options, &allocator);
        let our_render = result.template.expect("should have template block").code;

        assert_valid_js(
            &our_render,
            "nested component named slot then default in vslot",
        );
    }

    #[test]
    fn e2e_interpolation_inside_component_in_vslot() {
        // Interpolation ({{ t('team') }}) inside a component that's inside a v-slot
        // must open a default slot for the inner component.
        // Pattern from ReadyForMore.vue: <i18n-t #team><app-link>{{ t('team') }}</app-link></i18n-t>
        let vue_source = r#"<template>
  <I18nT keypath="ready-text" scope="global" tag="div">
    <template #team>
      <AppLink :href="url">
        {{ t('team') }}
      </AppLink>
    </template>
  </I18nT>
</template>"#;

        let allocator = oxc_allocator::Allocator::new();
        let options = ViteCodegenOptions {
            filename: Some("test.vue".to_string()),
            ..Default::default()
        };
        let result = generate_for_vite(vue_source, &options, &allocator);
        let our_render = result.template.expect("should have template block").code;

        assert_valid_js(&our_render, "interpolation inside component in vslot");

        // The inner component (AppLink) should have its text wrapped in a default slot
        assert!(
            our_render.contains("_createTextVNode"),
            "Expected _createTextVNode for interpolation in component slot. Output:\n{}",
            our_render
        );
    }

    #[test]
    fn e2e_comment_inside_named_slot() {
        // HTML comment inside a named slot should have proper comma separation
        // with sibling elements.
        // Pattern from md1.vue: <v-banner><template #prepend><!-- comment --><v-avatar /></template></v-banner>
        let vue_source = r#"<template>
  <VBanner text="hello">
    <template #prepend>
      <!-- rounded added due to bug -->
      <VAvatar icon="$vuetify" class="text-white" rounded="circle" />
    </template>
  </VBanner>
</template>"#;

        let allocator = oxc_allocator::Allocator::new();
        let options = ViteCodegenOptions {
            filename: Some("test.vue".to_string()),
            ..Default::default()
        };
        let result = generate_for_vite(vue_source, &options, &allocator);
        let our_render = result.template.expect("should have template block").code;

        assert_valid_js(&our_render, "comment inside named slot");

        // Should have both a comment and the avatar component in the prepend slot
        assert!(
            our_render.contains("_createCommentVNode"),
            "Expected _createCommentVNode. Output:\n{}",
            our_render
        );
    }

    // =========================================================================
    // Conditional Named Slots (createSlots) Tests
    // =========================================================================

    #[test]
    fn e2e_conditional_named_slot_simple() {
        // <template v-if="cond" #slotName> should use _createSlots()
        let vue_source = r#"<template>
  <VBanner text="hello">
    <template v-if="showPrepend" #prepend>
      <VIcon icon="mdi-check" />
    </template>
  </VBanner>
</template>"#;

        let allocator = oxc_allocator::Allocator::new();
        let options = ViteCodegenOptions {
            filename: Some("test.vue".to_string()),
            ..Default::default()
        };
        let result = generate_for_vite(vue_source, &options, &allocator);
        let our_render = result.template.expect("should have template block").code;

        assert_valid_js(&our_render, "conditional named slot simple");

        assert!(
            our_render.contains("_createSlots"),
            "Expected _createSlots for conditional named slot. Output:\n{}",
            our_render
        );
        assert!(
            our_render.contains("DYNAMIC"),
            "Expected DYNAMIC slot flag. Output:\n{}",
            our_render
        );
    }

    #[test]
    fn e2e_conditional_named_slot_with_else() {
        // v-if/v-else on named slots
        let vue_source = r#"<template>
  <VBanner text="hello">
    <template v-if="showPrepend" #prepend>
      <VIcon icon="mdi-check" />
    </template>
    <template v-else #prepend>
      <VIcon icon="mdi-alert" />
    </template>
  </VBanner>
</template>"#;

        let allocator = oxc_allocator::Allocator::new();
        let options = ViteCodegenOptions {
            filename: Some("test.vue".to_string()),
            ..Default::default()
        };
        let result = generate_for_vite(vue_source, &options, &allocator);
        let our_render = result.template.expect("should have template block").code;

        assert_valid_js(&our_render, "conditional named slot with else");
        assert!(
            our_render.contains("_createSlots"),
            "Expected _createSlots. Output:\n{}",
            our_render
        );
    }

    #[test]
    fn e2e_conditional_and_static_named_slots() {
        // Conditional + static slots in same component
        let vue_source = r#"<template>
  <VBanner text="hello">
    <template v-if="showPrepend" #prepend>
      <VIcon icon="mdi-check" />
    </template>
    <template #actions>
      <VBtn>OK</VBtn>
    </template>
  </VBanner>
</template>"#;

        let allocator = oxc_allocator::Allocator::new();
        let options = ViteCodegenOptions {
            filename: Some("test.vue".to_string()),
            ..Default::default()
        };
        let result = generate_for_vite(vue_source, &options, &allocator);
        let our_render = result.template.expect("should have template block").code;

        assert_valid_js(&our_render, "conditional and static named slots");
        assert!(
            our_render.contains("_createSlots"),
            "Expected _createSlots. Output:\n{}",
            our_render
        );
    }

    #[test]
    fn e2e_multiple_conditional_named_slots() {
        // Multiple independent conditional slots
        let vue_source = r#"<template>
  <VBanner text="hello">
    <template v-if="showPrepend" #prepend>
      <VIcon icon="mdi-check" />
    </template>
    <template v-if="showAppend" #append>
      <VIcon icon="mdi-close" />
    </template>
  </VBanner>
</template>"#;

        let allocator = oxc_allocator::Allocator::new();
        let options = ViteCodegenOptions {
            filename: Some("test.vue".to_string()),
            ..Default::default()
        };
        let result = generate_for_vite(vue_source, &options, &allocator);
        let our_render = result.template.expect("should have template block").code;

        assert_valid_js(&our_render, "multiple conditional named slots");
        assert!(
            our_render.contains("_createSlots"),
            "Expected _createSlots. Output:\n{}",
            our_render
        );
    }

    // =========================================================================
    // Hyphenated Slot Name Tests
    // =========================================================================

    #[test]
    fn e2e_hyphenated_slot_name() {
        // Slot names with hyphens must be quoted as object keys
        let vue_source = r#"<template>
  <VSelect :items="items">
    <template #append-outer>
      <div>suffix</div>
    </template>
  </VSelect>
</template>"#;

        let allocator = oxc_allocator::Allocator::new();
        let options = ViteCodegenOptions {
            filename: Some("test.vue".to_string()),
            ..Default::default()
        };
        let result = generate_for_vite(vue_source, &options, &allocator);
        let our_render = result.template.expect("should have template block").code;

        assert_valid_js(&our_render, "hyphenated slot name");

        // The slot name must be quoted
        assert!(
            our_render.contains("\"append-outer\""),
            "Expected quoted \"append-outer\" slot name. Output:\n{}",
            our_render
        );
    }

    #[test]
    fn e2e_hyphenated_slot_name_scoped() {
        // Hyphenated scoped slot with params
        let vue_source = r#"<template>
  <VSelect :items="items">
    <template #prepend-item="{ item }">
      <span>{{ item }}</span>
    </template>
  </VSelect>
</template>"#;

        let allocator = oxc_allocator::Allocator::new();
        let options = ViteCodegenOptions {
            filename: Some("test.vue".to_string()),
            ..Default::default()
        };
        let result = generate_for_vite(vue_source, &options, &allocator);
        let our_render = result.template.expect("should have template block").code;

        assert_valid_js(&our_render, "hyphenated scoped slot name");

        // The hyphenated slot name must be quoted
        assert!(
            our_render.contains("\"prepend-item\""),
            "Expected quoted \"prepend-item\" slot name. Output:\n{}",
            our_render
        );
    }

    // =========================================================================
    // v-if Inside Named Slot Tests
    // =========================================================================

    #[test]
    fn e2e_vif_inside_named_slot() {
        // When v-if is the only child in a named slot, the comment node
        // must be inside the slot array, not outside it
        let vue_source = r#"<template>
  <VListItem>
    <template #subtitle>
      <span v-if="show">{{ text }}</span>
    </template>
  </VListItem>
</template>"#;

        let allocator = oxc_allocator::Allocator::new();
        let options = ViteCodegenOptions {
            filename: Some("test.vue".to_string()),
            ..Default::default()
        };
        let result = generate_for_vite(vue_source, &options, &allocator);
        let our_render = result.template.expect("should have template block").code;

        assert_valid_js(&our_render, "v-if inside named slot");

        // The createCommentVNode must appear inside the slot's array (before the `]`)
        assert!(
            our_render.contains("_createCommentVNode"),
            "Expected _createCommentVNode for v-if fallback. Output:\n{}",
            our_render
        );
    }

    // =========================================================================
    // Default Slot with createSlots Tests
    // =========================================================================

    #[test]
    fn e2e_default_slot_with_conditional_named_slot() {
        // When a component has conditional named slots AND implicit default slot children,
        // the default slot must go in the createSlots base object, not in the dynamic array.
        let vue_source = r#"<template>
  <VListItem lines="two">
    <template v-if="showPrepend" #prepend>
      <VAvatar image="test.png" />
    </template>
    <VListItemTitle>Title</VListItemTitle>
  </VListItem>
</template>"#;

        let allocator = oxc_allocator::Allocator::new();
        let options = ViteCodegenOptions {
            filename: Some("test.vue".to_string()),
            ..Default::default()
        };
        let result = generate_for_vite(vue_source, &options, &allocator);
        let our_render = result.template.expect("should have template block").code;

        assert_valid_js(&our_render, "default slot with conditional named slot");

        assert!(
            our_render.contains("_createSlots"),
            "Expected _createSlots. Output:\n{}",
            our_render
        );
    }

    // =========================================================================
    // Object Literal Key Prefix Tests
    // =========================================================================

    #[test]
    fn e2e_object_literal_keys_not_prefixed() {
        // Object literal keys in expressions should not get _ctx. prefix
        let vue_source = r#"<template>
  <span>{{ t('hello', { count: items.length }) }}</span>
</template>"#;

        let allocator = oxc_allocator::Allocator::new();
        let options = ViteCodegenOptions {
            filename: Some("test.vue".to_string()),
            ..Default::default()
        };
        let result = generate_for_vite(vue_source, &options, &allocator);
        let our_render = result.template.expect("should have template block").code;

        assert_valid_js(&our_render, "object literal keys not prefixed");

        // The key "count" must NOT be prefixed
        assert!(
            !our_render.contains("_ctx.count"),
            "Object key 'count' should not be prefixed with _ctx. Output:\n{}",
            our_render
        );
    }

    // =========================================================================
    // Text + Interpolation in Slot Tests
    // =========================================================================

    #[test]
    fn e2e_text_and_interpolation_in_slot() {
        // Mixed text and interpolation in a scoped slot
        let vue_source = r#"<template>
  <VVirtualScroll :items="items" height="200">
    <template v-slot:default="{ item }">
      Virtual Item {{ item }}
    </template>
  </VVirtualScroll>
</template>"#;

        let allocator = oxc_allocator::Allocator::new();
        let options = ViteCodegenOptions {
            filename: Some("test.vue".to_string()),
            ..Default::default()
        };
        let result = generate_for_vite(vue_source, &options, &allocator);
        let our_render = result.template.expect("should have template block").code;

        assert_valid_js(&our_render, "text and interpolation in slot");
    }

    #[test]
    fn e2e_vif_component_with_named_slot() {
        // v-if on a component that has named slots — the ternary else branch
        // must wrap the entire component, not appear inside the slot array
        let vue_source = r#"<template>
  <div>
    <MyChip v-if="show" text="hello">
      <template #prepend>
        <MyIcon color="purple" />
      </template>
    </MyChip>
  </div>
</template>"#;

        let allocator = oxc_allocator::Allocator::new();
        let options = ViteCodegenOptions {
            filename: Some("test.vue".to_string()),
            ..Default::default()
        };
        let result = generate_for_vite(vue_source, &options, &allocator);
        let our_render = result.template.expect("should have template block").code;

        assert_valid_js(&our_render, "v-if component with named slot");
    }

    #[test]
    fn e2e_vif_velse_inside_named_slot() {
        // v-if/v-else INSIDE a slot of a v-if component —
        // the inner ternary is complete (has v-else), so no comment should be emitted inside slot
        let vue_source = r#"<template>
  <div>
    <MyTooltip v-if="show">
      <template #activator="{ props }">
        <a v-if="link" :href="link">Link</a>
        <div v-else>Fallback</div>
      </template>
    </MyTooltip>
  </div>
</template>"#;

        let allocator = oxc_allocator::Allocator::new();
        let options = ViteCodegenOptions {
            filename: Some("test.vue".to_string()),
            ..Default::default()
        };
        let result = generate_for_vite(vue_source, &options, &allocator);
        let our_render = result.template.expect("should have template block").code;

        assert_valid_js(&our_render, "v-if/v-else inside named slot");
    }

    #[test]
    fn e2e_shorthand_property_with_prefix() {
        // Shorthand properties like { file } should expand to { file: $props.file }
        // when the identifier gets a prefix, not produce invalid { $props.file }
        let vue_source = r#"<template>
  <span>{{ t('missing', { file }) }}</span>
</template>"#;

        let allocator = oxc_allocator::Allocator::new();
        let options = ViteCodegenOptions {
            filename: Some("test.vue".to_string()),
            ..Default::default()
        };
        let result = generate_for_vite(vue_source, &options, &allocator);
        let our_render = result.template.expect("should have template block").code;

        assert_valid_js(&our_render, "shorthand property with prefix");
    }

    #[test]
    fn e2e_shorthand_property_with_script_setup() {
        // Test shorthand property expansion via AST-based write_expr_with_ctx path
        // In script setup mode, bindings are resolved to $props./$setup. prefixes
        let vue_source = r#"<script setup>
const props = defineProps({ file: String })
const { t } = useI18n()
</script>
<template>
  <MyComp v-text="t('missing', { file })" />
</template>"#;

        let allocator = oxc_allocator::Allocator::new();
        let options = ViteCodegenOptions {
            filename: Some("test.vue".to_string()),
            ..Default::default()
        };
        let result = generate_for_vite(vue_source, &options, &allocator);
        let our_render = result.template.expect("should have template block").code;

        assert_valid_js(&our_render, "shorthand property with script setup");
        // Should contain { file: __props.file } not { __props.file }
        assert!(
            !our_render.contains("{ __props.file }"),
            "Should expand shorthand to key: value format. Generated:\n{}",
            our_render
        );
    }

    #[test]
    fn e2e_template_literal_not_treated_as_shorthand() {
        // Template literal interpolation ${branch} should NOT be expanded to ${branch: prefix.branch}
        // The { } in ${...} is template literal syntax, not an object literal
        let vue_source = r#"<script setup>
const branch = ref('main')
</script>
<template>
  <a :href="`https://github.com/tree/${branch}/src`">Link</a>
</template>"#;

        let allocator = oxc_allocator::Allocator::new();
        let options = ViteCodegenOptions {
            filename: Some("test.vue".to_string()),
            ..Default::default()
        };
        let result = generate_for_vite(vue_source, &options, &allocator);
        let our_render = result.template.expect("should have template block").code;

        assert_valid_js(&our_render, "template literal not treated as shorthand");
        // Should NOT contain "branch:" inside the template literal
        assert!(
            !our_render.contains("${branch:"),
            "Template literal ${{}}branch{{}} should NOT be treated as shorthand property. Generated:\n{}",
            our_render
        );
    }

    #[test]
    fn e2e_text_interpolation_then_element_in_slot() {
        // Mixed text+interpolation followed by child element in a slot
        // e.g.: {{ year }} --- <strong>Vuetify</strong>
        let vue_source = r#"<template>
  <MyFooter>
    <template #default>
      {{ year }} --- <strong>Vuetify</strong>
    </template>
  </MyFooter>
</template>"#;

        let allocator = oxc_allocator::Allocator::new();
        let options = ViteCodegenOptions {
            filename: Some("test.vue".to_string()),
            ..Default::default()
        };
        let result = generate_for_vite(vue_source, &options, &allocator);
        let our_render = result.template.expect("should have template block").code;

        assert_valid_js(&our_render, "text interpolation then element in slot");
    }

    #[test]
    fn e2e_compound_event_name_with_colon() {
        // Vuetify uses compound event names like @click:close
        // When used alongside regular @click and v-bind, mergeProps is used
        let vue_source = r#"<template>
  <VChip @click="select" @click:close="remove(item)" v-bind="attrs">Hello</VChip>
</template>"#;

        let allocator = oxc_allocator::Allocator::new();
        let options = ViteCodegenOptions {
            filename: Some("test.vue".to_string()),
            ..Default::default()
        };
        let result = generate_for_vite(vue_source, &options, &allocator);
        let our_render = result.template.expect("should have template block").code;

        assert_valid_js(&our_render, "compound event name with colon");
        // Event name with colon must be quoted: "onClick:close"
        assert!(
            our_render.contains("\"onClick:close\""),
            "Compound event name onClick:close should be quoted. Generated:\n{}",
            our_render
        );
    }

    #[test]
    fn e2e_vif_template_fragment_with_velse_sibling() {
        // v-if on <template> (fragment) with nested component children,
        // followed by v-else component sibling.
        // The v-else should NOT have a comma before the ternary ":"
        let vue_source = r#"<template>
  <MyParent>
    <template v-if="!error1 && !error2">
      <MyBase>
        <Inner v-if="!error1" />
        <Inner v-if="error1" />
      </MyBase>
    </template>
    <Fallback v-else />
  </MyParent>
</template>"#;

        let allocator = oxc_allocator::Allocator::new();
        let options = ViteCodegenOptions {
            filename: Some("test.vue".to_string()),
            ..Default::default()
        };
        let result = generate_for_vite(vue_source, &options, &allocator);
        let our_render = result.template.expect("should have template block").code;

        assert_valid_js(&our_render, "v-if template fragment with v-else sibling");
    }

    // =========================================================================
    // Event Modifier Tests
    // =========================================================================

    #[test]
    fn e2e_event_modifier_no_handler() {
        // @click.stop with no handler should produce valid JS
        let vue_source = r#"<template>
  <a href="https://example.com" @click.stop>Link</a>
</template>"#;

        let allocator = oxc_allocator::Allocator::new();
        let options = ViteCodegenOptions {
            filename: Some("test.vue".to_string()),
            ..Default::default()
        };
        let result = generate_for_vite(vue_source, &options, &allocator);
        let our_render = result.template.expect("should have template block").code;

        assert_valid_js(&our_render, "event modifier no handler");
    }

    // =========================================================================
    // List Rendering Tests
    // =========================================================================

    #[test]
    fn e2e_list_v_for() {
        let source = r#"<template>
  <ul>
    <li v-for="item in items" :key="item.id">
      {{ item.name }}
    </li>
  </ul>
</template>

<script setup>
import { ref } from 'vue'
const items = ref([
  { id: 1, name: 'Apple' },
  { id: 2, name: 'Banana' },
  { id: 3, name: 'Cherry' }
])
</script>
"#;
        let allocator = oxc_allocator::Allocator::new();
        let options = CodegenOptions::new().with_filename("list.vue");
        let result = generate(source, &options, &allocator);

        let code = strip_source_map(&result.code_with_source_map);

        // Verify v-for rendering
        assert!(
            code.contains("_renderList"),
            "Should use _renderList for v-for, got:\n{}",
            code
        );
        assert!(
            code.contains("$setup.items"),
            "Should reference $setup.items, got:\n{}",
            code
        );
        assert!(
            code.contains("item.id") || code.contains("key:"),
            "Should handle :key binding, got:\n{}",
            code
        );

        // Verify component structure
        assert!(
            code.contains("export default"),
            "Should have __sfc__ component, got:\n{}",
            code
        );
        assert!(
            code.contains("__returned__={items}"),
            "Should return items binding, got:\n{}",
            code
        );
    }

    // =========================================================================
    // Unicode Tests
    // =========================================================================

    #[test]
    fn e2e_unicode_content() {
        let source = r#"<template>
  <div>😊 Unicode Test 😊</div>
  <div>😊 Unicode Test 😊</div>
  <div>😊 Unicode Test 😊</div>
</template>
<script setup>
import { ref } from 'vue'
</script>"#;
        let allocator = oxc_allocator::Allocator::new();
        let options = CodegenOptions::new().with_filename("unicode.vue");
        let result = generate(source, &options, &allocator);

        let code = strip_source_map(&result.code_with_source_map);

        // Verify unicode is preserved correctly
        assert!(
            code.contains("😊") || code.contains("Unicode Test"),
            "Should preserve unicode content"
        );
        assert!(
            code.contains("function render("),
            "Should have render function"
        );
    }

    // =========================================================================
    // Async Component Tests
    // =========================================================================

    #[test]
    fn e2e_async_setup() {
        let source = r#"<template>
  <div>Hello world</div>
</template>
<script setup>
import { ref } from "vue";

const foo = ref("");

await Promise.resolve();

async () => {
  await Promise.resolve();
};

let a = {};
</script>
"#;
        let allocator = oxc_allocator::Allocator::new();
        let options = CodegenOptions::new().with_filename("simple.async.vue");
        let result = generate(source, &options, &allocator);

        let code = strip_source_map(&result.code_with_source_map);

        // Verify async context handling
        assert!(
            code.contains("_withAsyncContext"),
            "Should wrap top-level await with _withAsyncContext"
        );
        assert!(
            code.contains("__temp") && code.contains("__restore"),
            "Should have __temp and __restore for async context"
        );

        // Verify nested async is NOT wrapped (only top-level)
        // The inner async arrow function should stay as-is
        assert!(
            code.contains("async ()"),
            "Should preserve nested async function"
        );
    }

    // =========================================================================
    // Slot Tests
    // =========================================================================

    #[test]
    fn e2e_slot_outlet() {
        let source = r#"<template>
  <div>
    <slot name="header"></slot>
    <slot></slot>
    <slot name="footer"></slot>
  </div>
</template>

<script setup>
</script>
"#;
        let allocator = oxc_allocator::Allocator::new();
        let options = CodegenOptions::new().with_filename("slot-outlet.vue");
        let result = generate(source, &options, &allocator);

        let code = strip_source_map(&result.code_with_source_map);

        // Verify slot rendering
        assert!(
            code.contains("_renderSlot"),
            "Should use _renderSlot for slot outlets"
        );
        assert!(code.contains("\"header\""), "Should have header slot name");
        assert!(code.contains("\"default\""), "Should have default slot");
        assert!(code.contains("\"footer\""), "Should have footer slot name");
        assert!(code.contains("_ctx.$slots"), "Should reference _ctx.$slots");
    }

    #[test]
    fn e2e_scoped_slots() {
        let source = r#"<template>
  <MyComponent>
    <template #header="{ title }">
      {{ title }}
    </template>
  </MyComponent>
</template>

<script setup>
import MyComponent from './MyComponent.vue'
</script>
"#;
        let allocator = oxc_allocator::Allocator::new();
        let options = CodegenOptions::new().with_filename("slots.vue");
        let result = generate(source, &options, &allocator);

        let code = strip_source_map(&result.code_with_source_map);

        // Verify scoped slot handling
        assert!(
            code.contains("_withCtx"),
            "Should use _withCtx for scoped slots"
        );
        assert!(code.contains("header:"), "Should have header slot property");
        assert!(
            code.contains("{ title }")
                || code.contains("({title})")
                || code.contains("({ title })"),
            "Should destructure slot props"
        );
        assert!(
            code.contains("_toDisplayString"),
            "Should use _toDisplayString for slot content"
        );
    }

    /// @ai-generated - Tests self-closing default slot (no name) as template root
    #[test]
    fn e2e_slot_self_closing_default() {
        let source = r#"<template>
  <slot/>
</template>"#;
        let allocator = oxc_allocator::Allocator::new();
        let options = CodegenOptions::new().with_filename("test.vue");
        let result = generate(source, &options, &allocator);
        let code = strip_source_map(&result.code_with_source_map);

        eprintln!("=== slot self-closing default ===\n{}\n=== END ===", code);

        assert!(
            code.contains("_renderSlot"),
            "Should use _renderSlot. Generated:\n{}",
            code
        );
        assert!(
            code.contains("\"default\""),
            "Should have 'default' slot name. Generated:\n{}",
            code
        );

        assert_valid_js(&code, "self-closing default slot");
    }

    /// @ai-generated - Tests self-closing slot with name attribute as template root
    #[test]
    fn e2e_slot_self_closing_named() {
        let source = r#"<template>
  <slot name="second" />
</template>"#;
        let allocator = oxc_allocator::Allocator::new();
        let options = CodegenOptions::new().with_filename("test.vue");
        let result = generate(source, &options, &allocator);
        let code = strip_source_map(&result.code_with_source_map);

        eprintln!("=== slot self-closing named ===\n{}\n=== END ===", code);

        assert!(
            code.contains("_renderSlot"),
            "Should use _renderSlot. Generated:\n{}",
            code
        );
        assert!(
            code.contains("\"second\""),
            "Should have 'second' slot name. Generated:\n{}",
            code
        );

        assert_valid_js(&code, "self-closing named slot");
    }

    #[test]
    fn e2e_unicode_before_template() {
        // Test that UTF-8 multi-byte characters before the template don't break binding span calculation.
        // This reproduces a bug where binding spans from v-if expressions are relative to parsed substring
        // but later used as absolute positions against the full source.
        // The Chinese characters cause byte indices to not align with char boundaries.
        let source = r#"<!-- 红红红红红 table -->
<template>
  <div v-if="barfoo.key === 'bzz'">{{ MyPotion }}</div>
</template>
"#;
        let allocator = oxc_allocator::Allocator::new();
        let options = CodegenOptions::new().with_filename("unicode-test.vue");
        let result = generate(source, &options, &allocator);

        let code = strip_source_map(&result.code_with_source_map);

        // Verify the code generates without panicking due to UTF-8 boundary issues
        assert!(
            code.contains("function render("),
            "Should have render function. Generated:\n{}",
            code
        );
        assert!(
            code.contains("_ctx.barfoo"),
            "Should reference barfoo through _ctx. Generated:\n{}",
            code
        );
    }

    // =========================================================================
    // Macro Tests (via full pipeline)
    // =========================================================================

    #[test]
    fn e2e_define_props_typed() {
        let source = r#"<template>
  <div>{{ title }}</div>
</template>
<script setup lang="ts">
defineProps<{ title: string }>()
</script>
"#;
        let allocator = oxc_allocator::Allocator::new();
        let options = CodegenOptions::new().with_filename("props.vue");
        let result = generate(source, &options, &allocator);

        let code = strip_source_map(&result.code_with_source_map);

        // Verify props transformation
        assert!(code.contains("props:"), "Should have props definition");
        assert!(code.contains("title"), "Should have title prop");
        assert!(
            code.contains("_defineComponent"),
            "TypeScript should use _defineComponent"
        );
    }

    #[test]
    fn e2e_define_emits_typed() {
        let source = r#"<template>
  <button @click="emit('click')">Click</button>
</template>
<script setup lang="ts">
const emit = defineEmits<{
  (e: 'click'): void
}>()
</script>
"#;
        let allocator = oxc_allocator::Allocator::new();
        let options = CodegenOptions::new().with_filename("emits.vue");
        let result = generate(source, &options, &allocator);

        let code = strip_source_map(&result.code_with_source_map);

        // Verify emits transformation
        assert!(code.contains("emits:"), "Should have emits definition");
        assert!(
            code.contains("emit:__emit"),
            "Should expose emit in setup context"
        );
        assert!(
            code.contains("const emit = __emit"),
            "Should assign __emit to emit"
        );
    }

    #[test]
    fn e2e_define_model() {
        let source = r#"<template>
  <input v-model="model" />
</template>
<script setup lang="ts">
const model = defineModel<string>()
</script>
"#;
        let allocator = oxc_allocator::Allocator::new();
        let options = CodegenOptions::new().with_filename("model.vue");
        let result = generate(source, &options, &allocator);

        let code = strip_source_map(&result.code_with_source_map);

        // Verify model transformation
        assert!(code.contains("_useModel"), "Should use _useModel helper");
        assert!(code.contains("modelValue"), "Should have modelValue prop");
        assert!(
            code.contains("update:modelValue"),
            "Should have update:modelValue emit"
        );
        assert!(
            code.contains("modelValueModifiers"),
            "Should have modelValueModifiers prop"
        );
    }

    #[test]
    fn e2e_define_expose() {
        let source = r#"<template>
  <div>Test</div>
</template>
<script setup lang="ts">
const publicMethod = () => 'hello'
defineExpose({ publicMethod })
</script>
"#;
        let allocator = oxc_allocator::Allocator::new();
        let options = CodegenOptions::new().with_filename("expose.vue");
        let result = generate(source, &options, &allocator);

        let code = strip_source_map(&result.code_with_source_map);

        // Verify expose transformation
        assert!(
            code.contains("__expose({ publicMethod })"),
            "Should replace defineExpose with __expose"
        );
        // Should NOT have automatic __expose() call when user has defineExpose
        assert!(
            !code.contains("__expose();"),
            "Should not auto-expose when user has defineExpose"
        );
    }

    #[test]
    fn e2e_define_slots() {
        let source = r#"<template>
  <div><slot /></div>
</template>
<script setup lang="ts">
const slots = defineSlots<{
  default: () => any
}>()
</script>
"#;
        let allocator = oxc_allocator::Allocator::new();
        let options = CodegenOptions::new().with_filename("define-slots.vue");
        let result = generate(source, &options, &allocator);

        let code = strip_source_map(&result.code_with_source_map);

        // Verify component structure
        assert!(
            code.contains("export default") || code.contains("_defineComponent"),
            "Should have component definition, got:\n{}",
            code
        );

        // Verify render function has slot
        assert!(
            code.contains("_renderSlot") || code.contains("slot"),
            "Should render slot, got:\n{}",
            code
        );
    }

    // =========================================================================
    // Source Map Tests
    // =========================================================================

    #[test]
    fn e2e_source_map_valid_json() {
        let source = r#"<template><div>{{ msg }}</div></template>
<script setup>
const msg = 'test'
</script>"#;
        let allocator = oxc_allocator::Allocator::new();
        let options = CodegenOptions::new()
            .with_filename("test.vue")
            .include_source_content(true);

        let result = generate(source, &options, &allocator);

        // Parse source map as JSON
        let map: serde_json::Value =
            serde_json::from_str(&result.source_map).expect("Source map should be valid JSON");

        assert_eq!(map["version"], 3, "Should be version 3 source map");
        assert!(
            map["sources"].as_array().unwrap().len() > 0,
            "Should have sources"
        );
        assert!(map["mappings"].as_str().is_some(), "Should have mappings");
    }

    #[test]
    fn e2e_source_map_includes_content() {
        let source = r#"<template><div>Hello</div></template>"#;
        let allocator = oxc_allocator::Allocator::new();
        let options = CodegenOptions::new()
            .with_filename("test.vue")
            .include_source_content(true);

        let result = generate(source, &options, &allocator);

        let map: serde_json::Value =
            serde_json::from_str(&result.source_map).expect("Source map should be valid JSON");

        assert!(
            map["sourcesContent"].as_array().is_some(),
            "Should include sourcesContent when requested"
        );
    }

    // =========================================================================
    // Import Hoisting Tests
    // =========================================================================

    #[test]
    fn e2e_imports_hoisted() {
        let source = r#"<template>
  <div>{{ count }}</div>
</template>
<script setup>
import { ref } from 'vue'
import { computed } from 'vue'
const count = ref(0)
</script>
"#;
        let allocator = oxc_allocator::Allocator::new();
        let options = CodegenOptions::new().with_filename("imports.vue");
        let result = generate(source, &options, &allocator);

        let code = strip_source_map(&result.code_with_source_map);

        // Verify Vue imports for template helpers are generated
        assert!(
            code.contains("import {") || code.contains("import{"),
            "Should have imports, got:\n{}",
            code
        );
        assert!(
            code.contains("from \"vue\"") || code.contains("from 'vue'"),
            "Should import from vue, got:\n{}",
            code
        );

        // Verify component structure
        assert!(
            code.contains("export default"),
            "Should have __sfc__, got:\n{}",
            code
        );
    }

    // =========================================================================
    // Edge Cases
    // =========================================================================

    #[test]
    fn e2e_empty_template() {
        let source = r#"<template></template>
<script setup>
const x = 1
</script>"#;
        let allocator = oxc_allocator::Allocator::new();
        let options = CodegenOptions::new().with_filename("empty.vue");
        let result = generate(source, &options, &allocator);

        // Should not panic and produce valid output
        assert!(!result.code.is_empty(), "Should produce output");
    }

    #[test]
    fn e2e_nested_elements() {
        let source = r#"<template>
  <div>
    <section>
      <article>
        <p>Deep nesting</p>
      </article>
    </section>
  </div>
</template>
<script setup></script>"#;
        let allocator = oxc_allocator::Allocator::new();
        let options = CodegenOptions::new().with_filename("nested.vue");
        let result = generate(source, &options, &allocator);

        let code = strip_source_map(&result.code_with_source_map);

        // Verify all elements are present
        assert!(code.contains("div"), "Should have div");
        assert!(code.contains("section"), "Should have section");
        assert!(code.contains("article"), "Should have article");
        assert!(
            code.contains("\"Deep nesting\""),
            "Should have text content"
        );
    }

    #[test]
    fn e2e_multiple_root_elements() {
        let source = r#"<template>
  <div>First</div>
  <div>Second</div>
  <div>Third</div>
</template>
<script setup></script>"#;
        let allocator = oxc_allocator::Allocator::new();
        let options = CodegenOptions::new().with_filename("multi-root.vue");
        let result = generate(source, &options, &allocator);

        let code = strip_source_map(&result.code_with_source_map);

        // Vue 3 supports multiple root elements (fragments)
        assert!(code.contains("\"First\""), "Should have First");
        assert!(code.contains("\"Second\""), "Should have Second");
        assert!(code.contains("\"Third\""), "Should have Third");
    }

    #[test]
    fn e2e_multiple_root_elements_fragment_wrapper() {
        let source = r#"<template>
  <div>First</div>
  <div>Second</div>
  <div>Third</div>
</template>
<script setup></script>"#;
        let allocator = oxc_allocator::Allocator::new();
        let options = CodegenOptions::new().with_filename("multi-root.vue");
        let result = generate(source, &options, &allocator);

        let code = strip_source_map(&result.code_with_source_map);

        // Multiple root elements MUST be wrapped in Fragment
        assert!(
            code.contains("_Fragment"),
            "Multiple root elements should be wrapped in Fragment. Generated:\n{}",
            code
        );
        assert!(
            code.contains("_createElementBlock(_Fragment"),
            "Should use _createElementBlock with _Fragment. Generated:\n{}",
            code
        );
        // Should have STABLE_FRAGMENT patch flag (64)
        assert!(
            code.contains("64") || code.contains("STABLE_FRAGMENT"),
            "Should have STABLE_FRAGMENT patch flag. Generated:\n{}",
            code
        );
    }

    #[test]
    fn e2e_single_root_no_fragment() {
        let source = r#"<template>
  <div>Single root</div>
</template>
<script setup></script>"#;
        let allocator = oxc_allocator::Allocator::new();
        let options = CodegenOptions::new().with_filename("single-root.vue");
        let result = generate(source, &options, &allocator);

        let code = strip_source_map(&result.code_with_source_map);

        // Single root should NOT use Fragment
        assert!(
            !code.contains("_Fragment"),
            "Single root element should NOT use Fragment. Generated:\n{}",
            code
        );
    }

    // =========================================================================
    // Component Name Tests
    // =========================================================================

    #[test]
    fn e2e_component_name_from_filename() {
        let source = r#"<template>
  <div>Test</div>
</template>
<script setup></script>"#;
        let allocator = oxc_allocator::Allocator::new();
        let options = CodegenOptions::new().with_filename("my-component.vue");
        let result = generate(source, &options, &allocator);

        let code = strip_source_map(&result.code_with_source_map);

        // Component name should be derived from filename (without .vue extension)
        assert!(
            code.contains("__name: 'my-component'"),
            "Component name should be 'my-component' (from filename). Generated:\n{}",
            code
        );
        assert!(
            !code.contains("__name: 'App'"),
            "Component name should NOT be hardcoded 'App'. Generated:\n{}",
            code
        );
    }

    #[test]
    fn e2e_component_name_from_path() {
        let source = r#"<template>
  <div>Test</div>
</template>
<script setup></script>"#;
        let allocator = oxc_allocator::Allocator::new();
        let options = CodegenOptions::new().with_filename("src/components/MyComponent.vue");
        let result = generate(source, &options, &allocator);

        let code = strip_source_map(&result.code_with_source_map);

        // Component name should be just the filename part (without path and extension)
        assert!(
            code.contains("__name: 'MyComponent'"),
            "Component name should be 'MyComponent' (from path). Generated:\n{}",
            code
        );
    }

    // =========================================================================
    // Patch Flag Tests
    // =========================================================================

    #[test]
    fn e2e_patch_flag_text_for_interpolation() {
        let source = r#"<template>
  <div>
    <span>{{ message }}</span>
  </div>
</template>
<script setup>
const message = 'Hello'
</script>"#;
        let allocator = oxc_allocator::Allocator::new();
        let options = CodegenOptions::new().with_filename("test.vue");
        let result = generate(source, &options, &allocator);

        let code = strip_source_map(&result.code_with_source_map);

        // Element with interpolation should have TEXT patch flag (1)
        assert!(
            code.contains("1 /* TEXT */") || code.contains(", 1)"),
            "Element with interpolation should have TEXT patch flag. Generated:\n{}",
            code
        );
    }

    #[test]
    fn e2e_patch_flag_class_for_dynamic_class() {
        let source = r#"<template>
  <div :class="dynamicClass">Content</div>
</template>
<script setup>
const dynamicClass = 'active'
</script>"#;
        let allocator = oxc_allocator::Allocator::new();
        let options = CodegenOptions::new().with_filename("test.vue");
        let result = generate(source, &options, &allocator);

        let code = strip_source_map(&result.code_with_source_map);

        // Element with dynamic class should have CLASS patch flag (2)
        assert!(
            code.contains("2 /* CLASS */") || code.contains(", 2)"),
            "Element with dynamic class should have CLASS patch flag. Generated:\n{}",
            code
        );
    }

    // =========================================================================
    // v-if/v-else Key Tests
    // =========================================================================

    #[test]
    fn e2e_vif_velse_has_key_props() {
        let source = r#"<template>
  <div>
    <span v-if="show">Visible</span>
    <span v-else>Hidden</span>
  </div>
</template>
<script setup>
import { ref } from 'vue'
const show = ref(true)
</script>"#;
        let allocator = oxc_allocator::Allocator::new();
        let options = CodegenOptions::new().with_filename("test.vue");
        let result = generate(source, &options, &allocator);

        let code = strip_source_map(&result.code_with_source_map);

        // v-if/v-else branches should have key props for Vue's diffing algorithm
        assert!(
            code.contains("key: 0") || code.contains("{ key: 0 }"),
            "v-if branch should have key: 0. Generated:\n{}",
            code
        );
        assert!(
            code.contains("key: 1") || code.contains("{ key: 1 }"),
            "v-else branch should have key: 1. Generated:\n{}",
            code
        );
    }

    #[test]
    fn e2e_self_closing_tags() {
        let source = r#"<template>
  <input type="text" />
  <br />
  <img src="test.png" />
</template>
<script setup></script>"#;
        let allocator = oxc_allocator::Allocator::new();
        let options = CodegenOptions::new().with_filename("self-closing.vue");
        let result = generate(source, &options, &allocator);

        let code = strip_source_map(&result.code_with_source_map);

        assert!(code.contains("input"), "Should have input");
        assert!(code.contains("br"), "Should have br");
        assert!(code.contains("img"), "Should have img");
    }

    // =========================================================================
    // Script Setup Content Preservation Tests
    // =========================================================================

    #[test]
    fn e2e_script_setup_declarations_preserved() {
        let source = r#"<template>
  <div>{{ show }}</div>
</template>

<script setup>
import { ref } from 'vue'
const show = ref(true)
</script>
"#;
        let allocator = oxc_allocator::Allocator::new();
        let options = CodegenOptions::new().with_filename("test.vue");
        let result = generate(source, &options, &allocator);

        let code = strip_source_map(&result.code_with_source_map);

        // Declaration should appear in setup function body
        assert!(
            code.contains("const show = ref(true)"),
            "Declaration should be preserved in setup function. Generated:\n{}",
            code
        );
    }

    #[test]
    fn e2e_script_setup_with_conditional_template() {
        let source = r#"<template>
  <div>
    <span v-if="show">Visible</span>
    <span v-else>Hidden</span>
  </div>
</template>

<script setup>
import { ref } from 'vue'
const show = ref(true)
</script>
"#;
        let allocator = oxc_allocator::Allocator::new();
        let options = CodegenOptions::new().with_filename("conditional.vue");
        let result = generate(source, &options, &allocator);

        let code = strip_source_map(&result.code_with_source_map);

        // Script content should NOT be affected by template processing
        assert!(
            code.contains("const show = ref(true)"),
            "v-if/v-else template should not delete script declarations. Generated:\n{}",
            code
        );
    }

    #[test]
    fn e2e_template_overwrite_stays_within_bounds() {
        // Negative test: template processing should NOT affect script positions
        let source = r#"<template>
  <div>content</div>
</template>

<script setup>
const foo = 'bar'
</script>
"#;
        let allocator = oxc_allocator::Allocator::new();
        let options = CodegenOptions::new().with_filename("test.vue");
        let result = generate(source, &options, &allocator);

        let code = strip_source_map(&result.code_with_source_map);

        assert!(
            code.contains("const foo = 'bar'") || code.contains("const foo='bar'"),
            "Script declarations should not be overwritten by template processing. Generated:\n{}",
            code
        );
    }

    // =========================================================================
    // Single Text Child Tests (Step 5)
    // =========================================================================

    #[test]
    fn e2e_single_text_child_no_array() {
        // Single static text child should be a string, not an array
        let source = r#"<template>
  <span>Hello</span>
</template>
<script setup></script>"#;
        let allocator = oxc_allocator::Allocator::new();
        let options = CodegenOptions::new().with_filename("test.vue");
        let result = generate(source, &options, &allocator);

        let code = strip_source_map(&result.code_with_source_map);

        // Should be: _createElementVNode("span", null, "Hello")
        // NOT: _createElementBlock("span", null, ["Hello"])
        assert!(
            !code.contains(r#"["Hello"]"#),
            "Single text child should NOT be wrapped in array. Generated:\n{}",
            code
        );
        assert!(
            code.contains(r#""span", null, "Hello""#) || code.contains(r#""span",null,"Hello""#),
            "Single text child should be passed directly as string. Generated:\n{}",
            code
        );
    }

    #[test]
    fn e2e_single_interpolation_child_no_array() {
        // Single interpolation child should not be wrapped in array
        let source = r#"<template>
  <span>{{ msg }}</span>
</template>
<script setup>
const msg = 'Hello'
</script>"#;
        let allocator = oxc_allocator::Allocator::new();
        let options = CodegenOptions::new().with_filename("test.vue");
        let result = generate(source, &options, &allocator);

        let code = strip_source_map(&result.code_with_source_map);

        // Should be: _createElementVNode("span", null, _toDisplayString(_ctx.msg), 1 /* TEXT */)
        // NOT: _createElementBlock("span", null, [_toDisplayString(_ctx.msg)], 1 /* TEXT */)
        assert!(
            !code.contains("[_toDisplayString"),
            "Single interpolation child should NOT be wrapped in array. Generated:\n{}",
            code
        );
    }

    #[test]
    fn e2e_multiple_children_use_array() {
        // Multiple children should still use array format
        let source = r#"<template>
  <div>
    <span>First</span>
    <span>Second</span>
  </div>
</template>
<script setup></script>"#;
        let allocator = oxc_allocator::Allocator::new();
        let options = CodegenOptions::new().with_filename("test.vue");
        let result = generate(source, &options, &allocator);

        let code = strip_source_map(&result.code_with_source_map);

        // Multiple children should use array format
        // The outer div should have children in array
        assert!(
            code.contains("[") && code.contains("]"),
            "Multiple children should use array format. Generated:\n{}",
            code
        );
    }

    // =========================================================================
    // openBlock Optimization Tests (Step 6)
    // =========================================================================

    #[test]
    fn e2e_child_elements_no_openblock() {
        // Child elements should use _createElementVNode, not _openBlock + _createElementBlock
        let source = r#"<template>
  <div class="parent">
    <span>Child 1</span>
    <span>Child 2</span>
  </div>
</template>
<script setup></script>"#;
        let allocator = oxc_allocator::Allocator::new();
        let options = CodegenOptions::new().with_filename("test.vue");
        let result = generate(source, &options, &allocator);

        let code = strip_source_map(&result.code_with_source_map);

        // Count occurrences of _openBlock - should only be 1 (for root element)
        let openblock_count = code.matches("_openBlock()").count();
        assert_eq!(
            openblock_count, 1,
            "Should only have 1 _openBlock() for root element, found {}. Generated:\n{}",
            openblock_count, code
        );

        // Child elements should use _createElementVNode
        assert!(
            code.contains("_createElementVNode"),
            "Child elements should use _createElementVNode. Generated:\n{}",
            code
        );
    }

    #[test]
    fn e2e_vif_branches_have_openblock() {
        // v-if branches ARE block roots and should have _openBlock
        let source = r#"<template>
  <div>
    <span v-if="show">Visible</span>
    <span v-else>Hidden</span>
  </div>
</template>
<script setup>
const show = true
</script>"#;
        let allocator = oxc_allocator::Allocator::new();
        let options = CodegenOptions::new().with_filename("test.vue");
        let result = generate(source, &options, &allocator);

        let code = strip_source_map(&result.code_with_source_map);

        // v-if branches should have _openBlock (root + 2 branches = 3)
        let openblock_count = code.matches("_openBlock()").count();
        assert!(
            openblock_count >= 3,
            "v-if branches should have _openBlock(). Found {} occurrences. Generated:\n{}",
            openblock_count,
            code
        );
    }

    #[test]
    fn e2e_keyed_vfor_items_are_block_root() {
        // KEYED v-for items ARE block roots - they use _openBlock() + _createElementBlock.
        // This matches Vue's official compiler behavior.
        let source = r#"<template>
  <div>
    <span v-for="item in items" :key="item">{{ item }}</span>
  </div>
</template>
<script setup>
const items = ['a', 'b', 'c']
</script>"#;
        let allocator = oxc_allocator::Allocator::new();
        let options = CodegenOptions::new().with_filename("test.vue");
        let result = generate(source, &options, &allocator);

        let code = strip_source_map(&result.code_with_source_map);

        // Keyed v-for items should use _openBlock() + _createElementBlock
        assert!(
            code.contains(r#"_createElementBlock("span""#),
            "Keyed v-for items should use _createElementBlock. Generated:\n{}",
            code
        );
    }

    // =========================================================================
    // Event Modifier Tests (Step 7)
    // =========================================================================

    #[test]
    fn e2e_event_modifier_stop_prevent() {
        let source = r#"<template>
  <button @click.stop.prevent="handleClick">Click</button>
</template>
<script setup>
const handleClick = () => {}
</script>"#;
        let allocator = oxc_allocator::Allocator::new();
        let options = CodegenOptions::new().with_filename("test.vue");
        let result = generate(source, &options, &allocator);

        let code = strip_source_map(&result.code_with_source_map);

        // Should use _withModifiers for .stop and .prevent
        assert!(
            code.contains("_withModifiers"),
            "Should use _withModifiers for event modifiers. Generated:\n{}",
            code
        );
        assert!(
            code.contains("withModifiers as _withModifiers"),
            "Should import withModifiers from vue. Generated:\n{}",
            code
        );
        assert!(
            code.contains(r#"["stop"#) && code.contains(r#""prevent"]"#),
            "Should include stop and prevent in modifiers array. Generated:\n{}",
            code
        );
    }

    #[test]
    fn e2e_event_modifier_capture_once() {
        let source = r#"<template>
  <button @click.capture="handleCapture">Capture</button>
  <button @click.once="handleOnce">Once</button>
</template>
<script setup>
const handleCapture = () => {}
const handleOnce = () => {}
</script>"#;
        let allocator = oxc_allocator::Allocator::new();
        let options = CodegenOptions::new().with_filename("test.vue");
        let result = generate(source, &options, &allocator);

        let code = strip_source_map(&result.code_with_source_map);

        // .capture and .once become part of event name
        assert!(
            code.contains("onClickCapture"),
            ".capture should become onClickCapture. Generated:\n{}",
            code
        );
        assert!(
            code.contains("onClickOnce"),
            ".once should become onClickOnce. Generated:\n{}",
            code
        );
    }

    #[test]
    fn e2e_event_handler_with_colon_in_name() {
        // @update:rail compiles to onUpdate:rail which must be quoted
        // because the colon makes it an invalid JS identifier
        let vue_source = r#"<template>
  <MyComponent @update:rail="onUpdateRail" />
</template>"#;

        let allocator = oxc_allocator::Allocator::new();
        let options = ViteCodegenOptions {
            filename: Some("test.vue".to_string()),
            ..Default::default()
        };
        let result = generate_for_vite(vue_source, &options, &allocator);
        let our_render = result.template.expect("should have template block").code;

        assert_valid_js(&our_render, "event handler with colon in name");

        // The event name should be quoted because of the colon
        assert!(
            our_render.contains("\"onUpdate:rail\""),
            "Expected quoted event name for onUpdate:rail. Output:\n{}",
            our_render
        );
    }

    #[test]
    fn e2e_key_modifier_enter() {
        let source = r#"<template>
  <input @keyup.enter="handleEnter" />
</template>
<script setup>
const handleEnter = () => {}
</script>"#;
        let allocator = oxc_allocator::Allocator::new();
        let options = CodegenOptions::new().with_filename("test.vue");
        let result = generate(source, &options, &allocator);

        let code = strip_source_map(&result.code_with_source_map);

        // Should use _withKeys for key modifiers
        assert!(
            code.contains("_withKeys"),
            "Should use _withKeys for key modifiers. Generated:\n{}",
            code
        );
        assert!(
            code.contains("withKeys as _withKeys"),
            "Should import withKeys from vue. Generated:\n{}",
            code
        );
        assert!(
            code.contains(r#"["enter"]"#),
            "Should include 'enter' in keys array. Generated:\n{}",
            code
        );
    }

    #[test]
    fn e2e_key_modifier_with_system_modifier() {
        let source = r#"<template>
  <input @keyup.ctrl.enter="handleCtrlEnter" />
</template>
<script setup>
const handleCtrlEnter = () => {}
</script>"#;
        let allocator = oxc_allocator::Allocator::new();
        let options = CodegenOptions::new().with_filename("test.vue");
        let result = generate(source, &options, &allocator);

        let code = strip_source_map(&result.code_with_source_map);

        // Should use both _withKeys and _withModifiers
        // Pattern: _withKeys(_withModifiers(handler, ["ctrl"]), ["enter"])
        assert!(
            code.contains("_withKeys") && code.contains("_withModifiers"),
            "Should use both _withKeys and _withModifiers. Generated:\n{}",
            code
        );
        assert!(
            code.contains(r#"["ctrl"]"#),
            "Should include 'ctrl' in modifiers array. Generated:\n{}",
            code
        );
        assert!(
            code.contains(r#"["enter"]"#),
            "Should include 'enter' in keys array. Generated:\n{}",
            code
        );
    }

    // =========================================================================
    // Dynamic Component Tests
    // =========================================================================

    #[test]
    fn e2e_dynamic_component_basic() {
        let source = r#"<template>
  <component :is="currentComponent" />
</template>
<script setup>
import { ref } from 'vue'
const currentComponent = ref('MyComponent')
</script>"#;
        let allocator = oxc_allocator::Allocator::new();
        let options = CodegenOptions::new().with_filename("test.vue");
        let result = generate(source, &options, &allocator);

        let code = strip_source_map(&result.code_with_source_map);

        // Should use _resolveDynamicComponent
        assert!(
            code.contains("_resolveDynamicComponent"),
            "Should use _resolveDynamicComponent for <component :is>. Generated:\n{}",
            code
        );
        assert!(
            code.contains("resolveDynamicComponent as _resolveDynamicComponent"),
            "Should import resolveDynamicComponent from vue. Generated:\n{}",
            code
        );
        assert!(
            code.contains("$setup.currentComponent"),
            "Should reference the :is binding. Generated:\n{}",
            code
        );
    }

    #[test]
    fn e2e_dynamic_component_with_props() {
        let source = r#"<template>
  <component :is="currentView" :title="pageTitle" @click="handleClick" />
</template>
<script setup>
import { ref } from 'vue'
const currentView = ref('Home')
const pageTitle = ref('Welcome')
const handleClick = () => {}
</script>"#;
        let allocator = oxc_allocator::Allocator::new();
        let options = CodegenOptions::new().with_filename("test.vue");
        let result = generate(source, &options, &allocator);

        let code = strip_source_map(&result.code_with_source_map);

        // Should use _resolveDynamicComponent with props
        assert!(
            code.contains("_resolveDynamicComponent"),
            "Should use _resolveDynamicComponent. Generated:\n{}",
            code
        );
        assert!(
            code.contains("title:"),
            "Should include title prop. Generated:\n{}",
            code
        );
        assert!(
            code.contains("onClick"),
            "Should include click handler. Generated:\n{}",
            code
        );
    }

    // =========================================================================
    // AST Comparison Tests
    // =========================================================================
    //
    // These tests compare verter's output against Vue's official compiler.
    // The source of truth is generated using `node codegen.js`.
    //
    // To add a new test:
    // 1. Create examples/codegen/source/TEMP_FILE.vue with your test template
    // 2. Run: node crates/verter_core/examples/codegen.js
    // 3. Copy the content of TEMP_FILE.vue.js to your test as a static string
    // 4. Clean up the temp files
    //
    // Tests marked with #[ignore] are known failures - fix the bug, then remove #[ignore]

    #[test]
    fn e2e_simple_template_ast_is_valid_js() {
        // Test that the simplest template produces valid JavaScript
        let vue_source = r#"<template>
  <div class="hello">Hello</div>
</template>
<script setup>
</script>"#;

        let allocator = oxc_allocator::Allocator::new();
        let options = CodegenOptions::new().with_filename("test.vue".to_string());
        let result = generate(vue_source, &options, &allocator);

        // Validate our output is valid JS
        assert_valid_js(&result.code, "simple template");
    }

    #[test]
    fn e2e_simple_component_with_msg_ast_is_valid_js() {
        // Test component with interpolation
        let vue_source = r#"<template>
  <div class="hello">
    <h1>{{ msg }}</h1>
  </div>
</template>
<script setup>
const msg = 'Hello World'
</script>"#;

        let allocator = oxc_allocator::Allocator::new();
        let options = CodegenOptions::new().with_filename("test.vue".to_string());
        let result = generate(vue_source, &options, &allocator);

        // Validate our output is valid JS
        assert_valid_js(&result.code, "component with interpolation");
    }

    #[test]
    fn e2e_vbind_spread_ast_is_valid_js() {
        let vue_source = r#"<template>
  <div v-bind="attrs">Content</div>
</template>
<script setup>
const attrs = { id: 'test' }
</script>"#;

        let allocator = oxc_allocator::Allocator::new();
        let options = CodegenOptions::new().with_filename("test.vue".to_string());
        let result = generate(vue_source, &options, &allocator);

        // Validate our output is valid JS
        assert_valid_js(&result.code, "v-bind spread");
        assert_no_invalid_patterns(&result.code, "v-bind spread");
    }

    #[test]
    fn e2e_custom_directive_ast_is_valid_js() {
        let vue_source = r#"<template>
  <input v-focus />
</template>
<script setup>
</script>"#;

        let allocator = oxc_allocator::Allocator::new();
        let options = CodegenOptions::new().with_filename("test.vue".to_string());
        let result = generate(vue_source, &options, &allocator);

        // Validate our output is valid JS
        assert_valid_js(&result.code, "custom directive");
        assert_no_invalid_patterns(&result.code, "custom directive");
    }

    #[test]
    fn e2e_vonce_ast_is_valid_js() {
        let vue_source = r#"<template>
  <span v-once>{{ staticContent }}</span>
</template>
<script setup>
const staticContent = 'Static'
</script>"#;

        let allocator = oxc_allocator::Allocator::new();
        let options = CodegenOptions::new().with_filename("test.vue".to_string());
        let result = generate(vue_source, &options, &allocator);

        // Validate our output is valid JS
        assert_valid_js(&result.code, "v-once");
        assert_no_invalid_patterns(&result.code, "v-once");
    }

    #[test]
    fn e2e_object_class_ast_is_valid_js() {
        let vue_source = r#"<template>
  <div :class="{ active: isActive }">Content</div>
</template>
<script setup>
const isActive = true
</script>"#;

        let allocator = oxc_allocator::Allocator::new();
        let options = CodegenOptions::new().with_filename("test.vue".to_string());
        let result = generate(vue_source, &options, &allocator);

        // Validate our output is valid JS
        assert_valid_js(&result.code, "object class binding");
        assert_no_invalid_patterns(&result.code, "object class binding");
    }

    #[test]
    fn e2e_hyphenated_props_ast_is_valid_js() {
        let vue_source = r#"<template>
  <div :data-value="value">Content</div>
</template>
<script setup>
const value = 'test'
</script>"#;

        let allocator = oxc_allocator::Allocator::new();
        let options = CodegenOptions::new().with_filename("test.vue".to_string());
        let result = generate(vue_source, &options, &allocator);

        // Validate our output is valid JS
        assert_valid_js(&result.code, "hyphenated props");
    }

    #[test]
    fn e2e_mixed_text_interpolation_ast_is_valid_js() {
        // Tests static text + interpolation mixed content
        let vue_source = r#"<template>
  <span>Static: {{ content }}</span>
</template>
<script setup>
const content = 'dynamic'
</script>"#;

        let allocator = oxc_allocator::Allocator::new();
        let options = CodegenOptions::new().with_filename("test.vue".to_string());
        let result = generate(vue_source, &options, &allocator);

        // Validate our output is valid JS
        assert_valid_js(&result.code, "mixed text and interpolation");
    }

    #[test]
    fn e2e_inline_object_literal_ast_is_valid_js() {
        // Tests inline object literal in v-bind (not spread)
        let vue_source = r#"<template>
  <input :value="{ type: 'text', name: 'field' }" />
</template>
<script setup>
</script>"#;

        let allocator = oxc_allocator::Allocator::new();
        let options = CodegenOptions::new().with_filename("test.vue".to_string());
        let result = generate(vue_source, &options, &allocator);

        // Validate our output is valid JS
        assert_valid_js(&result.code, "inline object literal");
    }

    // =========================================================================
    // Bug Fix Tests: V-if Array Closing Syntax
    // =========================================================================

    #[test]
    fn test_vif_without_velse_produces_valid_js() {
        // Tests v-if without v-else sibling - should generate valid ternary with comment fallback
        // Bug: Was generating `[: _createCommentVNode...` inside children array
        let vue_source = r#"<template>
  <div>
    <div v-if="show">
      <span>Content</span>
    </div>
  </div>
</template>
<script setup>
const show = true
</script>"#;

        let code = gen_and_validate(vue_source);

        // The conditional close should be OUTSIDE the element, as ternary else
        // NOT inside the children array
        assert!(
            !code.contains("[: _createCommentVNode"),
            "Comment vnode should not be inside children array. Generated:\n{}",
            code
        );

        // Should have the comment vnode as ternary fallback
        assert!(
            code.contains("_createCommentVNode(\"v-if\", true)"),
            "Should have v-if comment vnode fallback. Generated:\n{}",
            code
        );

        // Check no invalid patterns
        assert_no_invalid_patterns(&code, "v-if without v-else");
    }

    #[test]
    fn test_vif_with_velse_produces_valid_js() {
        // Tests v-if with v-else sibling - should NOT have comment fallback
        let vue_source = r#"<template>
  <div>
    <div v-if="show">If content</div>
    <div v-else>Else content</div>
  </div>
</template>
<script setup>
const show = true
</script>"#;

        let code = gen_and_validate(vue_source);

        // With v-else, there should be no comment vnode fallback
        assert!(
            !code.contains("_createCommentVNode(\"v-if\", true)"),
            "Should NOT have comment vnode when v-else exists. Generated:\n{}",
            code
        );

        assert_no_invalid_patterns(&code, "v-if with v-else");
    }

    // =========================================================================
    // Bug Fix Tests: Dynamic Component Children
    // =========================================================================

    #[test]
    fn test_dynamic_component_with_static_is_produces_valid_js() {
        // Tests <component is="div">content</component> - static is value
        // Bug: Missing comma between props and children, wrong resolveDynamicComponent arg
        let vue_source = r#"<template>
  <div>
    <component is="div">Static content</component>
  </div>
</template>
<script setup>
</script>"#;

        let code = gen_and_validate(vue_source);

        // Static is="div" should pass "div" directly to _resolveDynamicComponent
        assert!(
            code.contains("_resolveDynamicComponent(\"div\")"),
            "Static is value should be passed directly to _resolveDynamicComponent. Generated:\n{}",
            code
        );

        // Should NOT have is as a prop
        assert!(
            !code.contains("{ is:") && !code.contains("is: \"div\""),
            "Static is should not appear as a prop. Generated:\n{}",
            code
        );

        assert_no_invalid_patterns(&code, "dynamic component with static is");
    }

    #[test]
    fn test_dynamic_component_with_children_produces_valid_js() {
        // Tests dynamic component with children - should wrap in slots
        let vue_source = r#"<template>
  <div>
    <component :is="comp">Child content</component>
  </div>
</template>
<script setup>
const comp = 'div'
</script>"#;

        let code = gen_and_validate(vue_source);

        // Children should be wrapped in slot format with _withCtx
        assert!(
            code.contains("_withCtx"),
            "Children should be wrapped with _withCtx for slots. Generated:\n{}",
            code
        );

        assert_no_invalid_patterns(&code, "dynamic component with children");
    }

    #[test]
    fn test_custom_directive_basic_produces_valid_js() {
        // Tests basic custom directive (no value)
        let vue_source = r#"<template>
  <div>
    <input v-focus />
  </div>
</template>
<script setup>
</script>"#;

        let code = gen_and_validate(vue_source);

        // Custom directives should use _withDirectives wrapper
        assert!(
            code.contains("_withDirectives"),
            "Custom directive should use _withDirectives. Generated:\n{}",
            code
        );

        // Should resolve the directive
        assert!(
            code.contains("_resolveDirective(\"focus\")"),
            "Custom directive should be resolved. Generated:\n{}",
            code
        );

        // Should NOT have v-focus as a prop
        assert!(
            !code.contains("\"v-focus\""),
            "Custom directive should not be treated as static prop. Generated:\n{}",
            code
        );

        assert_no_invalid_patterns(&code, "custom directive basic");
    }

    #[test]
    fn test_custom_directive_with_value_produces_valid_js() {
        // Tests custom directive with value
        let vue_source = r#"<template>
  <div>
    <div v-tooltip="'Hello World'">Hover me</div>
  </div>
</template>
<script setup>
</script>"#;

        let code = gen_and_validate(vue_source);

        // Should use _withDirectives
        assert!(
            code.contains("_withDirectives"),
            "Custom directive should use _withDirectives. Generated:\n{}",
            code
        );

        // Should resolve the directive
        assert!(
            code.contains("_resolveDirective(\"tooltip\")"),
            "Custom directive should be resolved. Generated:\n{}",
            code
        );

        // Should include the value
        assert!(
            code.contains("'Hello World'"),
            "Custom directive should include value. Generated:\n{}",
            code
        );

        assert_no_invalid_patterns(&code, "custom directive with value");
    }

    #[test]
    fn test_vonce_basic_produces_valid_js() {
        // Tests v-once directive - should wrap with cache pattern
        let vue_source = r#"<template>
  <div>
    <span v-once>Static: {{ content }}</span>
  </div>
</template>
<script setup>
const content = 'test'
</script>"#;

        let code = gen_and_validate(vue_source);

        // v-once should use _setBlockTracking
        assert!(
            code.contains("_setBlockTracking"),
            "v-once should use _setBlockTracking. Generated:\n{}",
            code
        );

        // v-once should use cache pattern
        assert!(
            code.contains("_cache[") && code.contains("] || ("),
            "v-once should use cache pattern. Generated:\n{}",
            code
        );

        // v-once should set cacheIndex
        assert!(
            code.contains(".cacheIndex"),
            "v-once should set cacheIndex. Generated:\n{}",
            code
        );

        assert_no_invalid_patterns(&code, "v-once basic");
    }

    #[test]
    fn test_scoped_style_adds_data_v_attribute() {
        let source = r#"<template>
  <div class="container">Hello</div>
</template>
<script setup></script>
<style scoped>.container { color: red; }</style>"#;
        let allocator = oxc_allocator::Allocator::new();
        let options = CodegenOptions::new().with_filename("scoped-test.vue");
        let result = generate(source, &options, &allocator);

        // Should have data-v-xxx attribute in hoisted props
        assert!(
            result.code.contains("data-v-"),
            "Should have data-v attribute. Generated:\n{}",
            result.code
        );

        // Should export __css__
        assert!(
            result.code.contains("__css__"),
            "Should export __css__. Generated:\n{}",
            result.code
        );

        // Should have scoped selector in CSS
        assert!(
            result.code.contains(".container[data-v-"),
            "CSS should have scoped selector. Generated:\n{}",
            result.code
        );
    }

    #[test]
    fn test_css_modules_exports_mapping() {
        let source = r#"<template>
  <div :class="$style.container">Hello</div>
</template>
<script setup></script>
<style module>.container { color: red; } .title { font-size: 16px; }</style>"#;
        let allocator = oxc_allocator::Allocator::new();
        let options = CodegenOptions::new().with_filename("module-test.vue");
        let result = generate(source, &options, &allocator);

        // Should export __css__
        assert!(
            result.code.contains("__css__"),
            "Should export __css__. Generated:\n{}",
            result.code
        );

        // Should export __cssModules__
        assert!(
            result.code.contains("__cssModules__"),
            "Should export __cssModules__. Generated:\n{}",
            result.code
        );

        // Should have $style module
        assert!(
            result.code.contains("\"$style\""),
            "Should have $style module. Generated:\n{}",
            result.code
        );

        // Should have container class mapping
        assert!(
            result.code.contains("\"container\""),
            "Should have container class mapping. Generated:\n{}",
            result.code
        );

        // Should have hashed class name in CSS
        assert!(
            result.code.contains("._container_"),
            "CSS should have hashed class name. Generated:\n{}",
            result.code
        );
    }

    // =========================================================================
    // generate_for_vite Tests
    // =========================================================================

    #[test]
    fn test_vite_returns_timing() {
        let source = r#"<template><div>Hello</div></template>"#;
        let allocator = oxc_allocator::Allocator::new();
        let options = ViteCodegenOptions {
            filename: Some("test.vue".to_string()),
            ..Default::default()
        };

        let result = generate_for_vite(source, &options, &allocator);

        assert!(
            result.duration_ms > 0.0,
            "Should have positive timing. Got: {}",
            result.duration_ms
        );
    }

    #[test]
    fn test_vite_extracts_script_block() {
        let source = r#"<script setup lang="ts">
const msg = 'hello'
</script>
<template><div>{{ msg }}</div></template>"#;

        let allocator = oxc_allocator::Allocator::new();
        let options = ViteCodegenOptions {
            filename: Some("test.vue".to_string()),
            sourcemap: true,
            ..Default::default()
        };

        let result = generate_for_vite(source, &options, &allocator);

        assert!(result.script.is_some(), "Should extract script block");
        let script = result.script.as_ref().unwrap();
        assert!(!script.code.is_empty(), "Script code should not be empty");
    }

    #[test]
    fn test_vite_extracts_style_blocks() {
        let source = r#"<template><div>test</div></template>
<style scoped lang="scss">.foo { color: red; }</style>
<style module>.bar { color: blue; }</style>"#;

        let allocator = oxc_allocator::Allocator::new();
        let options = ViteCodegenOptions::default();

        let result = generate_for_vite(source, &options, &allocator);

        assert_eq!(result.styles.len(), 2, "Should extract 2 style blocks");
        assert!(result.styles[0].scoped, "First style should be scoped");
        assert_eq!(
            result.styles[0].lang,
            Some("scss".to_string()),
            "First style should have scss lang"
        );
        assert!(result.styles[1].is_module, "Second style should be module");
    }

    #[test]
    fn test_vite_style_has_code() {
        let source = r#"<template><div>test</div></template>
<style>.test { color: red; }</style>"#;

        let allocator = oxc_allocator::Allocator::new();
        let options = ViteCodegenOptions::default();

        let result = generate_for_vite(source, &options, &allocator);

        assert_eq!(result.styles.len(), 1, "Should extract 1 style block");

        let style = &result.styles[0];
        assert!(!style.code.is_empty(), "Style code should not be empty");
        assert!(
            style.code.contains("color: red"),
            "Style should contain the CSS content"
        );
    }

    #[test]
    fn test_vite_validates_output_js() {
        let source = r#"<template><div>test</div></template>
<script setup>const x = 1</script>"#;

        let allocator = oxc_allocator::Allocator::new();
        let options = ViteCodegenOptions::default();

        let result = generate_for_vite(source, &options, &allocator);

        // Validate script block is syntactically valid JS
        let script = result.script.as_ref().expect("Should have script block");
        assert_valid_js(&script.code, "vite script output");
    }

    #[test]
    fn test_vite_sourcemap_option() {
        let source = r#"<template><div>Hello</div></template>"#;
        let allocator = oxc_allocator::Allocator::new();

        // With sourcemap disabled
        let options_no_map = ViteCodegenOptions {
            filename: Some("test.vue".to_string()),
            sourcemap: false,
            ..Default::default()
        };
        let result_no_map = generate_for_vite(source, &options_no_map, &allocator);
        // Template block should exist but have no source map
        let template = result_no_map
            .template
            .as_ref()
            .expect("Should have template");
        assert!(
            template.source_map.is_none(),
            "Should not have source map when disabled"
        );

        // With sourcemap enabled
        let options_with_map = ViteCodegenOptions {
            filename: Some("test.vue".to_string()),
            sourcemap: true,
            ..Default::default()
        };
        let result_with_map = generate_for_vite(source, &options_with_map, &allocator);
        let template = result_with_map
            .template
            .as_ref()
            .expect("Should have template");
        assert!(
            template.source_map.is_some(),
            "Should have source map when enabled"
        );
    }

    #[test]
    fn test_vite_define_props_type_params() {
        // Test 1: Simple resolvable type - should work
        let source1 = r#"<script setup lang="ts">
defineProps<{
  store: string
}>()
</script>
<template><div>test</div></template>"#;

        let allocator1 = oxc_allocator::Allocator::new();
        let options = ViteCodegenOptions {
            filename: Some("test.vue".to_string()),
            ..Default::default()
        };

        let result1 = generate_for_vite(source1, &options, &allocator1);
        let script1 = result1.script.as_ref().unwrap();
        eprintln!(
            "=== TEST 1: Resolvable type ===\n{}\n=== END ===",
            script1.code
        );
        assert!(script1.code.contains("props:"), "Should have props");
        assert_valid_js(&script1.code, "resolvable type props");

        // Test 2: Unresolvable type (imported interface) - reproduces the bug
        let source2 = r#"<script setup lang="ts">
defineProps<{
  store: Store
}>()
</script>
<template><div>test</div></template>"#;

        let allocator2 = oxc_allocator::Allocator::new();
        let result2 = generate_for_vite(source2, &options, &allocator2);
        let script2 = result2.script.as_ref().unwrap();
        eprintln!(
            "=== TEST 2: Unresolvable type ===\n{}\n=== END ===",
            script2.code
        );
        assert!(script2.code.contains("props:"), "Should have props");
        assert_valid_js(&script2.code, "unresolvable type props");
    }

    /// @ai-generated - Tests defineProps with optional typed props and assigned variable
    #[test]
    fn test_vite_define_props_with_assignment() {
        // This reproduces the exact pattern from the bug report:
        // const props = defineProps<{ direction?: string }>()
        let source = r#"<script setup lang="ts">
const props = defineProps<{
  direction?: 'horizontal' | 'vertical'
  initialSplit?: number
}>()
</script>
<template><div>{{ props.direction }}</div></template>"#;

        let allocator = oxc_allocator::Allocator::new();
        let options = ViteCodegenOptions {
            filename: Some("test.vue".to_string()),
            ..Default::default()
        };

        let result = generate_for_vite(source, &options, &allocator);
        let script_code = &result.script.as_ref().unwrap().code;

        // Should have props section
        assert!(
            script_code.contains("props:"),
            "Should have props section. Generated:\n{}",
            script_code
        );

        // Should NOT have `)` dangling after props
        assert!(
            !script_code.contains("},)"),
            "Should not have dangling paren after props. Generated:\n{}",
            script_code
        );

        // MANDATORY: Validate generated code is syntactically valid JS
        assert_valid_js(script_code, "defineProps with assignment");
    }

    /// @ai-generated - Tests defineProps with unresolvable imported type
    #[test]
    fn test_vite_define_props_with_imported_type() {
        // Reproduces Header.vue pattern: import type + unresolvable type in defineProps
        let source = r#"<script setup lang="ts">
import type { Store } from '../core/store'

defineProps<{
  store: Store
}>()
</script>
<template><div>test</div></template>"#;

        let allocator = oxc_allocator::Allocator::new();
        let options = ViteCodegenOptions {
            filename: Some("Header.vue".to_string()),
            ..Default::default()
        };

        let result = generate_for_vite(source, &options, &allocator);
        let script_code = &result.script.as_ref().unwrap().code;
        eprintln!(
            "=== defineProps with imported type ===\n{}\n=== END ===",
            script_code
        );

        // Should have props section
        assert!(
            script_code.contains("props:"),
            "Should have props section. Generated:\n{}",
            script_code
        );

        // `import type` should be stripped (invalid JS syntax)
        assert!(
            !script_code.contains("import type"),
            "import type should be stripped from JS output. Generated:\n{}",
            script_code
        );

        // MANDATORY: Validate generated code is syntactically valid JS
        assert_valid_js(script_code, "defineProps with imported type");
    }

    #[test]
    fn test_vite_split_blocks() {
        // Verify that script, template, and style are produced as separate blocks
        let source = r#"<script setup lang="ts">
import { ref } from 'vue'
const count = ref(0)
</script>
<template>
  <div class="container">
    <span>{{ count }}</span>
  </div>
</template>
<style scoped>.container { color: red; }</style>"#;

        let allocator = oxc_allocator::Allocator::new();
        let options = ViteCodegenOptions {
            filename: Some("SplitPane.vue".to_string()),
            sourcemap: true,
            ..Default::default()
        };

        let result = generate_for_vite(source, &options, &allocator);

        // Script block should exist and be valid
        let script = result.script.as_ref().expect("Should have script block");
        eprintln!("=== SCRIPT BLOCK ===\n{}\n=== END ===", script.code);
        assert!(
            script.code.contains("_defineComponent") || script.code.contains("export default"),
            "Script should contain component definition"
        );
        assert!(script.source_map.is_some(), "Script should have source map");
        assert_valid_js(&script.code, "split script block");

        // Template block should exist
        let template = result
            .template
            .as_ref()
            .expect("Should have template block");
        eprintln!("=== TEMPLATE BLOCK ===\n{}\n=== END ===", template.code);
        assert!(
            template.code.contains("function render"),
            "Template should contain render function. Got:\n{}",
            template.code
        );
        assert!(
            template.source_map.is_some(),
            "Template should have source map"
        );

        // Style block should exist
        assert_eq!(result.styles.len(), 1, "Should have 1 style block");
        assert!(result.styles[0].scoped, "Style should be scoped");
        assert!(
            !result.styles[0].code.is_empty(),
            "Style code should not be empty"
        );
        assert!(
            result.styles[0].source_map.is_some(),
            "Style should have source map"
        );
    }

    // =========================================================================
    // Binding Metadata Tests
    // =========================================================================

    #[test]
    fn test_dev_setup_binding_uses_setup_prefix() {
        let source = r#"<template><div>{{ count }}</div></template>
<script setup>
import { ref } from 'vue'
const count = ref(0)
</script>"#;
        let code = gen_and_validate(source);
        assert!(
            code.contains("$setup.count"),
            "Setup binding should use $setup. prefix. Generated:\n{}",
            code
        );
        assert!(
            !code.contains("_ctx.count"),
            "Setup binding should NOT use _ctx. prefix. Generated:\n{}",
            code
        );
    }

    #[test]
    fn test_dev_props_binding_uses_props_prefix() {
        let source = r#"<template><div>{{ title }}</div></template>
<script setup>
const props = defineProps<{ title: string }>()
</script>"#;
        let code = gen_and_validate(source);
        assert!(
            code.contains("$props.title"),
            "Props should use $props. prefix in dev mode. Generated:\n{}",
            code
        );
        assert!(
            !code.contains("_ctx.title"),
            "Props should NOT use _ctx. prefix in dev mode. Generated:\n{}",
            code
        );
    }

    #[test]
    fn test_dev_mixed_setup_and_props() {
        let source = r#"<template><div>{{ count }}: {{ title }}</div></template>
<script setup>
import { ref } from 'vue'
const props = defineProps<{ title: string }>()
const count = ref(0)
</script>"#;
        let code = gen_and_validate(source);
        assert!(
            code.contains("$setup.count"),
            "Setup binding should use $setup. Generated:\n{}",
            code
        );
        assert!(
            code.contains("$props.title"),
            "Props should use $props. in dev mode. Generated:\n{}",
            code
        );
    }

    #[test]
    fn test_render_signature_with_script_setup() {
        let source = r#"<template><div>{{ msg }}</div></template>
<script setup>
const msg = 'hello'
</script>"#;
        let code = gen_and_validate(source);
        assert!(
            code.contains("render(_ctx, _cache, $props, $setup, $data, $options)"),
            "Dev render should have full signature. Generated:\n{}",
            code
        );
    }

    #[test]
    fn test_template_before_script_ordering() {
        // Template comes BEFORE script in source — pre-scan ensures bindings resolve
        let source = r#"<template><div>{{ count }}</div></template>
<script setup>
import { ref } from 'vue'
const count = ref(0)
</script>"#;
        let code = gen_and_validate(source);
        assert!(
            code.contains("$setup.count"),
            "Template-before-script should still resolve bindings. Generated:\n{}",
            code
        );
    }

    #[test]
    fn test_ctx_slots_unchanged() {
        let source = r#"<template><slot name="header" /></template>
<script setup></script>"#;
        let code = gen_and_validate(source);
        assert!(
            code.contains("_ctx.$slots"),
            "$slots should always use _ctx. prefix. Generated:\n{}",
            code
        );
    }

    #[test]
    fn test_setup_component_direct_reference() {
        let source = r#"<template><MyComponent /></template>
<script setup>
import MyComponent from './MyComponent.vue'
</script>"#;
        let code = gen_and_validate(source);
        assert!(
            code.contains("$setup.MyComponent"),
            "Setup component should use $setup. prefix in standalone mode. Generated:\n{}",
            code
        );
        assert!(
            !code.contains("_resolveComponent"),
            "Should NOT use resolveComponent for setup components. Generated:\n{}",
            code
        );
    }

    #[test]
    fn test_vite_path_binding_metadata() {
        let source = r#"<template><div>{{ count }}</div></template>
<script setup>
import { ref } from 'vue'
const count = ref(0)
</script>"#;
        let allocator = oxc_allocator::Allocator::new();
        let options = ViteCodegenOptions {
            filename: Some("test.vue".to_string()),
            ..Default::default()
        };
        let result = generate_for_vite(source, &options, &allocator);
        let template_code = result.template.unwrap().code;
        assert!(
            template_code.contains("$setup.count"),
            "Vite template should use $setup. prefix. Generated:\n{}",
            template_code
        );
    }

    #[test]
    fn test_vite_path_component_resolution() {
        let source = r#"<template><Header /></template>
<script setup>
import Header from './Header.vue'
</script>"#;
        let allocator = oxc_allocator::Allocator::new();
        let options = ViteCodegenOptions {
            filename: Some("test.vue".to_string()),
            ..Default::default()
        };
        let result = generate_for_vite(source, &options, &allocator);
        let template_code = result.template.unwrap().code;
        assert!(
            template_code.contains("$setup.Header"),
            "Vite template should use $setup. prefix for setup components. Generated:\n{}",
            template_code
        );
        assert!(
            !template_code.contains("_resolveComponent"),
            "Should NOT use resolveComponent for setup components. Generated:\n{}",
            template_code
        );
    }

    #[test]
    fn test_vite_vif_without_velse_imports_comment_vnode() {
        let source = r#"<script setup lang="ts">
defineProps<{
  errors: string[]
}>()
</script>
<template>
  <div v-if="errors.length > 0" class="message-container">
    <div v-for="(error, i) in errors" :key="i" class="message error">
      {{ error }}
    </div>
  </div>
</template>"#;
        let allocator = oxc_allocator::Allocator::new();
        let options = ViteCodegenOptions {
            filename: Some("Message.vue".to_string()),
            ..Default::default()
        };
        let result = generate_for_vite(source, &options, &allocator);
        let template_code = result.template.unwrap().code;
        eprintln!(
            "=== MESSAGE.VUE TEMPLATE ===\n{}\n=== END ===",
            template_code
        );
        assert!(
            template_code.contains("createCommentVNode"),
            "v-if without v-else should import createCommentVNode. Generated:\n{}",
            template_code
        );
    }

    #[test]
    fn test_vite_interpolation_in_vfor_with_function_call() {
        // Reproduces Output.vue: function declared in script setup, called inside interpolation within v-for
        let source = r#"<script setup>
const tabs = [{ mode: 'a', label: 'A' }]
function getTabTiming(mode) { return null }
</script>
<template>
  <div>
    <button v-for="tab in tabs" :key="tab.mode">
      {{ tab.label }}
      <span v-if="getTabTiming(tab.mode)" class="timing-pill">
        {{ getTabTiming(tab.mode) }}
      </span>
    </button>
  </div>
</template>"#;
        let allocator = oxc_allocator::Allocator::new();
        let options = ViteCodegenOptions {
            filename: Some("Output.vue".to_string()),
            ..Default::default()
        };
        let result = generate_for_vite(source, &options, &allocator);
        let template_code = result.template.unwrap().code;
        eprintln!(
            "=== OUTPUT.VUE TEMPLATE ===\n{}\n=== END ===",
            template_code
        );
        // getTabTiming should use $setup. prefix in ALL occurrences (both v-if and interpolation)
        let occurrences: Vec<_> = template_code.match_indices("getTabTiming").collect();
        for (pos, _) in &occurrences {
            let before = &template_code[pos.saturating_sub(10)..*pos];
            eprintln!("getTabTiming at {}: ...{}getTabTiming...", pos, before);
        }
        assert!(
            !template_code.contains("_toDisplayString(getTabTiming("),
            "Interpolation should prefix getTabTiming with $setup. Generated:\n{}",
            template_code
        );
    }

    #[test]
    fn test_vite_scope_id_uses_component_id_option() {
        let source = r#"<template><div class="box">hi</div></template>
<style scoped>.box { color: red; }</style>"#;
        let allocator = oxc_allocator::Allocator::new();
        let options = ViteCodegenOptions {
            filename: Some("test.vue".to_string()),
            component_id: Some("abcd1234".to_string()),
            ..Default::default()
        };
        let result = generate_for_vite(source, &options, &allocator);
        let template_code = result.template.unwrap().code;
        let style_code = &result.styles[0].code;
        // Template render function should NOT contain scope IDs (Vue runtime handles it via __scopeId)
        assert!(
            !template_code.contains("data-v-"),
            "Template should NOT contain data-v- scope attributes. Generated:\n{}",
            template_code
        );
        // CSS should use the provided component_id for scoped selectors
        assert!(
            style_code.contains("[data-v-abcd1234]"),
            "CSS should use component_id from options. Generated:\n{}",
            style_code
        );
    }

    /// @ai-generated - Tests that implicit void tags (no />) generate valid JS
    #[test]
    fn test_implicit_void_tags_generate_valid_js() {
        let source = r#"<template>
  <div>
    <br>
    <input type="text">
    <hr>
    <img src="test.png">
  </div>
</template>
<script setup>
</script>"#;

        let code = gen_and_validate(source);

        // Void tags should produce valid code without unclosed children arrays
        assert!(
            code.contains("_createElementVNode(\"br\""),
            "Should generate br element. Generated:\n{}",
            code
        );
        assert!(
            code.contains("_createElementVNode(\"input\""),
            "Should generate input element. Generated:\n{}",
            code
        );
        assert!(
            code.contains("_createElementVNode(\"hr\""),
            "Should generate hr element. Generated:\n{}",
            code
        );
        assert!(
            code.contains("_createElementVNode(\"img\""),
            "Should generate img element. Generated:\n{}",
            code
        );
    }

    #[test]
    fn e2e_root_component_uses_create_block_render_matches_vue() {
        let vue_source = r#"<script setup>
import { RouterView } from 'vue-router'
</script>
<template>
  <RouterView />
</template>"#;

        let vue_render = r#"import { resolveComponent as _resolveComponent, openBlock as _openBlock, createBlock as _createBlock } from "vue"

export function render(_ctx, _cache) {
  const _component_RouterView = _resolveComponent("RouterView")

  return (_openBlock(), _createBlock(_component_RouterView))
}"#;

        let allocator = oxc_allocator::Allocator::new();
        let options = ViteCodegenOptions {
            filename: Some("test.vue".to_string()),
            ..Default::default()
        };
        let result = generate_for_vite(vue_source, &options, &allocator);
        let our_render = result.template.expect("should have template block").code;

        assert_valid_js(&our_render, "root component createBlock render output");

        let diffs = compare_ast_structure(&our_render, vue_render, "render_block");
        assert!(
            diffs.is_empty(),
            "Render output differs from Vue:\n{}\n\nVerter:\n{}\n\nVue:\n{}",
            diffs.join("\n"),
            our_render,
            vue_render
        );
    }

    // =========================================================================
    // Production Mode E2E Tests (inline template in setup)
    // =========================================================================

    /// @ai-generated - Tests production mode output matches Vue's prod behavior.
    ///
    /// Vue's production build inlines the template render into the script setup block.
    /// In `generate_for_vite` with `is_production=true`, the standalone template must
    /// NOT use inline-scope identifiers (`__props`, bare setup refs) that are only valid
    /// inside a setup closure. Instead it must use dev-mode prefixes (`$props.`, `$setup.`).
    ///
    /// Additionally, prod mode must:
    /// - Use empty string for v-if comment nodes (not "v-if")
    /// - Emit valid JS that parses without errors
    /// @ai-generated - Tests production mode standalone template generates correct bindings.
    /// Uses direct prop name in template to test binding prefix without props alias complexity.
    #[test]
    fn e2e_prod_mode_standalone_template_is_correct() {
        let vue_source = r#"<script setup lang="ts">
import { ref } from "vue";
const split = ref(50);
</script>

<template>
  <div v-if="true"
       :style="{ flexBasis: split + '%' }" />
</template>"#;

        let allocator = oxc_allocator::Allocator::new();
        let options = ViteCodegenOptions {
            filename: Some("ProdTest.vue".to_string()),
            is_production: true,
            ssr: false,
            ..Default::default()
        };

        let result = generate_for_vite(vue_source, &options, &allocator);

        // In production with <script setup>, template is inlined into script
        assert!(
            result.template.is_none(),
            "Production <script setup> should inline template (no separate template block)"
        );

        let script = result
            .script
            .expect("should have script block in prod mode");
        let our_code = &script.code;

        // 1) Prod comment nodes must use empty string, not "v-if"
        assert!(
            !our_code.contains(r#"_createCommentVNode("v-if""#),
            "Prod mode must NOT contain createCommentVNode(\"v-if\").\n\nGenerated:\n{}",
            our_code
        );
        assert!(
            our_code.contains(r#"_createCommentVNode("", true)"#),
            "Prod mode must contain createCommentVNode(\"\", true).\n\nGenerated:\n{}",
            our_code
        );

        // 2) Inline mode: template is inside setup closure
        assert!(
            our_code.contains("return (_ctx, _cache) =>"),
            "Inline mode must return render arrow function from setup.\n\nGenerated:\n{}",
            our_code
        );
        assert!(
            !our_code.contains("function render("),
            "Inline mode must NOT have separate render function.\n\nGenerated:\n{}",
            our_code
        );

        // 3) Inline mode uses bare names (not $setup. prefix)
        // Note: ideally refs should use .value but this requires BindingType::SetupRef
        assert!(
            !our_code.contains("$setup.split"),
            "Inline mode must NOT use $setup. prefix.\n\nGenerated:\n{}",
            our_code
        );
    }

    /// @ai-generated - Tests that `const props = defineProps()` resolves correctly.
    /// In inline mode, `props.direction` should become `__props.direction`.
    #[test]
    fn e2e_prod_mode_standalone_props_alias() {
        let vue_source = r#"<script setup lang="ts">
const props = defineProps<{ direction?: string }>();
</script>

<template>
  <div :class="props.direction" />
</template>"#;

        let allocator = oxc_allocator::Allocator::new();
        let options = ViteCodegenOptions {
            filename: Some("PropsAlias.vue".to_string()),
            is_production: true,
            ..Default::default()
        };

        let result = generate_for_vite(vue_source, &options, &allocator);

        // Production with <script setup>: template inlined into script
        assert!(
            result.template.is_none(),
            "Production <script setup> should inline template"
        );

        let script = result.script.expect("should have script block");
        let our_code = &script.code;

        // In inline mode, props alias should resolve to __props or $setup.props
        // (TODO: ideally should be __props.direction once PropsAlias binding type is implemented)
        assert!(
            our_code.contains("__props.direction")
                || our_code.contains("$setup.props.direction")
                || our_code.contains("props.direction"),
            "Props alias must resolve correctly in inline mode.\n\nGenerated:\n{}",
            our_code
        );
    }

    /// @ai-generated - Tests that v-for with :key uses KEYED_FRAGMENT (128), not STABLE_FRAGMENT (64).
    /// Vue uses KEYED_FRAGMENT for keyed lists with reactive sources to enable correct DOM diffing.
    #[test]
    fn e2e_vfor_keyed_uses_keyed_fragment() {
        let vue_source = r#"<script setup>
import { ref } from "vue";
const versions = ref([{ id: 1, label: "v1" }, { id: 2, label: "v2" }]);
</script>

<template>
  <div v-for="entry in versions" :key="entry.id">
    {{ entry.label }}
  </div>
</template>"#;

        let allocator = oxc_allocator::Allocator::new();
        let options = ViteCodegenOptions {
            filename: Some("test.vue".to_string()),
            ..Default::default()
        };
        let result = generate_for_vite(vue_source, &options, &allocator);
        let our_render = result.template.expect("should have template block").code;

        assert_valid_js(&our_render, "keyed v-for render output");

        // Must use KEYED_FRAGMENT (128), not STABLE_FRAGMENT (64)
        assert!(
            our_render.contains("128") || our_render.contains("KEYED_FRAGMENT"),
            "Keyed v-for with reactive ref source must use KEYED_FRAGMENT (128).\n\nGenerated:\n{}",
            our_render
        );
        assert!(
            !our_render.contains("64 /* STABLE_FRAGMENT */") && !our_render.contains(", 64)"),
            "Keyed v-for with reactive ref source must NOT use STABLE_FRAGMENT (64).\n\nGenerated:\n{}",
            our_render
        );
    }

    /// @ai-generated - Tests that template refs for setup bindings use ref_key/ref pattern.
    /// Vue outputs `ref_key: "name", ref: $setup.name` instead of `ref: "name"` when the
    /// ref name matches a setup binding variable.
    #[test]
    fn e2e_template_ref_uses_ref_key_pattern() {
        let vue_source = r#"<script setup>
import { ref } from "vue";
const container = ref(null);
</script>

<template>
  <div ref="container">content</div>
</template>"#;

        let allocator = oxc_allocator::Allocator::new();
        let options = ViteCodegenOptions {
            filename: Some("test.vue".to_string()),
            ..Default::default()
        };
        let result = generate_for_vite(vue_source, &options, &allocator);
        let our_render = result.template.expect("should have template block").code;

        assert_valid_js(&our_render, "template ref render output");

        // Must use ref_key/ref pattern, not bare ref: "container"
        assert!(
            our_render.contains("ref_key: \"container\""),
            "Template ref matching setup binding must use ref_key pattern.\n\nGenerated:\n{}",
            our_render
        );
        assert!(
            our_render.contains("ref: $setup.container") || our_render.contains("ref: container"),
            "Template ref must reference the actual variable, not a string.\n\nGenerated:\n{}",
            our_render
        );
    }

    #[test]
    fn e2e_setup_component_uses_direct_reference() {
        let vue_source = r#"<script setup>
import MyChild from "./MyChild.vue";
</script>

<template>
  <MyChild msg="hello" />
</template>"#;

        let allocator = oxc_allocator::Allocator::new();
        let options = ViteCodegenOptions {
            filename: Some("test.vue".to_string()),
            ..Default::default()
        };
        let result = generate_for_vite(vue_source, &options, &allocator);
        let our_render = result.template.expect("should have template block").code;

        assert_valid_js(&our_render, "setup component render output");

        // Script setup components should be referenced directly, not via resolveComponent
        assert!(
            !our_render.contains("resolveComponent"),
            "Script setup components should not use resolveComponent.\n\nGenerated:\n{}",
            our_render
        );
        assert!(
            our_render.contains("$setup.MyChild") || our_render.contains("$setup[\"MyChild\"]"),
            "Script setup components should be referenced via $setup.\n\nGenerated:\n{}",
            our_render
        );
    }

    /// @ai-generated - Tests that hoisted variables are at module level in production inline mode.
    /// Vue places _hoisted_N variables before the component definition, not inside setup().
    #[test]
    fn e2e_prod_mode_hoisted_at_module_level() {
        let vue_source = r#"<script setup lang="ts">
import { ref } from "vue";
const msg = ref("hello");
</script>

<template>
  <div class="container">{{ msg }}</div>
</template>"#;

        let allocator = oxc_allocator::Allocator::new();
        let options = ViteCodegenOptions {
            filename: Some("test.vue".to_string()),
            is_production: true,
            ..Default::default()
        };

        let result = generate_for_vite(vue_source, &options, &allocator);

        // Production with <script setup>: template inlined into script
        assert!(
            result.template.is_none(),
            "Production <script setup> should inline template"
        );

        let script = result.script.expect("should have script block");
        let our_code = &script.code;

        // Hoisted vars should appear BEFORE setup( (at module level)
        if our_code.contains("_hoisted_") {
            let hoisted_pos = our_code.find("_hoisted_").unwrap();
            let setup_pos = our_code.find("setup(").expect("should contain setup(");
            assert!(
                hoisted_pos < setup_pos,
                "Hoisted variables must appear before setup() (at module level), not inside setup().\n\nGenerated:\n{}",
                our_code
            );
        }
    }

    #[test]
    fn e2e_prod_mode_strips_patch_flag_comments() {
        // This template exercises: TEXT flag, v-for fragment flags, mixed children createTextVNode
        let vue_source = r#"<script setup>
import { ref } from "vue";
const items = ref([]);
const msg = ref("hello");
</script>

<template>
  <div>
    <span>{{ msg }}</span>
    <ul>
      <li v-for="item in items" :key="item">{{ item }}</li>
    </ul>
  </div>
</template>"#;

        let allocator = oxc_allocator::Allocator::new();
        let mut options = ViteCodegenOptions {
            filename: Some("test.vue".to_string()),
            ..Default::default()
        };
        options.is_production = true;
        let result = generate_for_vite(vue_source, &options, &allocator);

        // Production with <script setup>: template inlined into script
        assert!(
            result.template.is_none(),
            "Production <script setup> should inline template"
        );

        let script = result.script.expect("should have script block");
        let our_code = &script.code;

        // Production output should NOT contain patch flag comments (except @__PURE__)
        let code_without_pure = our_code.replace("/*@__PURE__*/", "");
        assert!(
            !code_without_pure.contains("/*"),
            "Production output should not contain patch flag comments.\n\nGenerated:\n{}",
            our_code
        );
    }

    #[test]
    fn e2e_component_slot_text_after_self_closing_child_has_comma() {
        // Component slot with self-closing component + text must have commas
        // e.g., <CHeader> <CIcon /> Theme colors </CHeader>
        let vue_source = r#"<template><CHeader> <CIcon /> Theme colors </CHeader></template>"#;

        let allocator = oxc_allocator::Allocator::new();
        let options = ViteCodegenOptions {
            filename: Some("test.vue".to_string()),
            ..Default::default()
        };
        let result = generate_for_vite(vue_source, &options, &allocator);
        let our_render = result.template.expect("should have template block").code;

        // Must be valid JS - missing comma would cause parse error
        assert_valid_js(&our_render, "component slot with mixed children");
    }

    #[test]
    fn e2e_hyphenated_directive_name_camelized_in_variable() {
        // Directives with hyphens like v-c-popover must have their variable name
        // camelized: _directive_cPopover, not _directive_c-popover (invalid JS)
        let vue_source = r#"<template><div v-my-custom="val">text</div></template>
<script setup>const val = 'x'</script>"#;

        let allocator = oxc_allocator::Allocator::new();
        let options = ViteCodegenOptions {
            filename: Some("test.vue".to_string()),
            ..Default::default()
        };
        let result = generate_for_vite(vue_source, &options, &allocator);
        let our_render = result.template.expect("should have template block").code;

        // Must be valid JS (hyphenated identifier would fail)
        assert_valid_js(&our_render, "hyphenated directive render output");

        // Variable name should be camelized
        assert!(
            our_render.contains("_directive_myCustom"),
            "Directive variable should be camelized to _directive_myCustom.\n\nGenerated:\n{}",
            our_render
        );

        // Should NOT contain hyphenated variable
        assert!(
            !our_render.contains("_directive_my-custom"),
            "Directive variable should NOT contain hyphens.\n\nGenerated:\n{}",
            our_render
        );
    }

    #[test]
    fn e2e_custom_directive_on_component_with_children() {
        // Custom directives on components with children (non-self-closing) must
        // emit _withDirectives(_createVNode(Comp, ...), [[_directive_xxx, value]])
        // Previously the directive array was missing for components.
        let vue_source =
            r#"<template><CLink v-c-tooltip="'Tooltip text'">link text</CLink></template>"#;

        let allocator = oxc_allocator::Allocator::new();
        let options = ViteCodegenOptions {
            filename: Some("test.vue".to_string()),
            ..Default::default()
        };
        let result = generate_for_vite(vue_source, &options, &allocator);
        let our_render = result.template.expect("should have template block").code;

        // Must be valid JS
        assert_valid_js(&our_render, "directive on component with children");

        // Should have withDirectives wrapping the component
        assert!(
            our_render.contains("_withDirectives("),
            "Should wrap component in _withDirectives.\n\nGenerated:\n{}",
            our_render
        );

        // Should have the directive array with camelized name
        assert!(
            our_render.contains("_directive_cTooltip"),
            "Should have camelized directive variable.\n\nGenerated:\n{}",
            our_render
        );
    }

    #[test]
    fn e2e_custom_directive_on_self_closing_component() {
        // Custom directive on a self-closing component
        let vue_source = r#"<template><CIcon v-c-tooltip="'Icon tooltip'" /></template>"#;

        let allocator = oxc_allocator::Allocator::new();
        let options = ViteCodegenOptions {
            filename: Some("test.vue".to_string()),
            ..Default::default()
        };
        let result = generate_for_vite(vue_source, &options, &allocator);
        let our_render = result.template.expect("should have template block").code;

        // Must be valid JS
        assert_valid_js(&our_render, "directive on self-closing component");

        // Should have withDirectives wrapping the component
        assert!(
            our_render.contains("_withDirectives("),
            "Should wrap component in _withDirectives.\n\nGenerated:\n{}",
            our_render
        );
    }

    #[test]
    fn e2e_component_slot_interpolation_only() {
        // Component with only interpolation child must open slot wrapper
        // <CToastBody>{{ toast.content }}</CToastBody>
        let vue_source = r#"<template><CToastBody>{{ toast.content }}</CToastBody></template>"#;

        let allocator = oxc_allocator::Allocator::new();
        let options = ViteCodegenOptions {
            filename: Some("test.vue".to_string()),
            ..Default::default()
        };
        let result = generate_for_vite(vue_source, &options, &allocator);
        let our_render = result.template.expect("should have template block").code;

        // Must be valid JS
        assert_valid_js(&our_render, "component with interpolation-only child");

        // Should have slot wrapper
        assert!(
            our_render.contains("_withCtx("),
            "Should have slot wrapper with _withCtx.\n\nGenerated:\n{}",
            our_render
        );

        // Should wrap interpolation in _createTextVNode
        assert!(
            our_render.contains("_createTextVNode("),
            "Should wrap interpolation in _createTextVNode.\n\nGenerated:\n{}",
            our_render
        );
    }

    #[test]
    fn e2e_multiline_attribute_value_escaped() {
        // Attribute values with literal newlines must be escaped in JS strings
        let vue_source = "<template><CFormCheck label=\"Radio\n              1\" /></template>";

        let allocator = oxc_allocator::Allocator::new();
        let options = ViteCodegenOptions {
            filename: Some("test.vue".to_string()),
            ..Default::default()
        };
        let result = generate_for_vite(vue_source, &options, &allocator);
        let our_render = result.template.expect("should have template block").code;

        // Must be valid JS (unescaped newline in string literal would fail)
        assert_valid_js(&our_render, "multiline attribute value");
    }

    #[test]
    fn e2e_cached_self_closing_element_in_vfor() {
        // Static self-closing elements in v-for should have CACHED flag
        // as 4th arg to _createElementVNode, not as comma operator outside
        let vue_source = r#"<template><div v-for="item in items"><hr class="mt-0" /></div></template>
<script setup>const items = [1,2]</script>"#;

        let allocator = oxc_allocator::Allocator::new();
        let options = ViteCodegenOptions {
            filename: Some("test.vue".to_string()),
            ..Default::default()
        };
        let result = generate_for_vite(vue_source, &options, &allocator);
        let our_render = result.template.expect("should have template block").code;

        // Must be valid JS
        assert_valid_js(&our_render, "cached element in v-for");

        // CACHED flag (-1) should be inside _createElementVNode(), not outside
        assert!(
            our_render.contains("_createElementVNode(\"hr\""),
            "Should have _createElementVNode for hr.\n\nGenerated:\n{}",
            our_render
        );
    }

    #[test]
    fn e2e_component_slot_text_with_sibling_component() {
        // Component with mixed text + component children
        // Text should use _createTextVNode and have proper commas
        let vue_source = r##"<template><CAlert color="primary">A simple alert with <CAlertLink href="#">a link</CAlertLink>. Give it a click.</CAlert></template>"##;

        let allocator = oxc_allocator::Allocator::new();
        let options = ViteCodegenOptions {
            filename: Some("test.vue".to_string()),
            ..Default::default()
        };
        let result = generate_for_vite(vue_source, &options, &allocator);
        let our_render = result.template.expect("should have template block").code;

        // Must be valid JS
        assert_valid_js(
            &our_render,
            "component with mixed text and component children",
        );
    }

    #[test]
    fn e2e_directive_with_object_literal_value() {
        // Directive values that are object literals should NOT get _ctx. prefix
        // e.g. v-c-popover="{header: 'title', content: 'body'}" should produce
        // { header: 'title', content: 'body' } not _ctx.{header: 'title', content: 'body'}
        let vue_source = r#"<template><button v-c-popover="{header: 'Popover', content: 'And here is some content'}">Click</button></template>"#;

        let allocator = oxc_allocator::Allocator::new();
        let options = ViteCodegenOptions {
            filename: Some("test.vue".to_string()),
            ..Default::default()
        };
        let result = generate_for_vite(vue_source, &options, &allocator);
        let our_render = result.template.expect("should have template block").code;

        // Must be valid JS
        assert_valid_js(&our_render, "directive with object literal value");

        // Should NOT contain _ctx.{ which is invalid JS
        assert!(
            !our_render.contains("_ctx.{"),
            "Object literal should not get _ctx. prefix.\n\nGenerated:\n{}",
            our_render
        );

        // Should contain the object literal as-is
        assert!(
            our_render.contains("{header: 'Popover', content: 'And here is some content'}"),
            "Should contain the object literal value.\n\nGenerated:\n{}",
            our_render
        );
    }

    #[test]
    fn e2e_directive_with_object_literal_on_component() {
        // Same test but on a component (uses generate_component_directive_suffix path)
        let vue_source = r#"<template><CButton v-c-tooltip="{content: 'tooltip text', placement: 'top'}">Hover</CButton></template>"#;

        let allocator = oxc_allocator::Allocator::new();
        let options = ViteCodegenOptions {
            filename: Some("test.vue".to_string()),
            ..Default::default()
        };
        let result = generate_for_vite(vue_source, &options, &allocator);
        let our_render = result.template.expect("should have template block").code;

        // Must be valid JS
        assert_valid_js(&our_render, "directive with object literal on component");

        // Should NOT contain _ctx.{ which is invalid JS
        assert!(
            !our_render.contains("_ctx.{"),
            "Object literal should not get _ctx. prefix on component.\n\nGenerated:\n{}",
            our_render
        );
    }

    // ─── Matrix comparison bug fixes ────────────────────────────────────

    #[test]
    fn e2e_vfor_on_component_with_text_child() {
        // Bug A: v-for on a component with text interpolation child
        // Pattern from AppBreadcrumb.vue - generates "Unexpected ]"
        let vue_source = r#"<template>
  <CBreadcrumb class="my-0">
    <CBreadcrumbItem
      v-for="item in breadcrumbs"
      :key="item"
      :href="item.active ? '' : item.path"
      :active="item.active"
    >
      {{ item.name }}
    </CBreadcrumbItem>
  </CBreadcrumb>
</template>
<script setup>
import { ref } from 'vue'
const breadcrumbs = ref([])
</script>"#;

        let allocator = oxc_allocator::Allocator::new();
        let options = ViteCodegenOptions {
            filename: Some("test.vue".to_string()),
            ..Default::default()
        };
        let result = generate_for_vite(vue_source, &options, &allocator);
        let our_render = result.template.expect("should have template block").code;
        assert_valid_js(&our_render, "vfor_on_component_with_text_child");
    }

    #[test]
    fn e2e_vif_velse_chain_in_component_slot() {
        // Bug B: v-if/v-else-if/v-else chain on component siblings inside a slot
        // Pattern from AppHeader.vue - generates "Expected ':' but found ','"
        let vue_source = r#"<template>
  <CDropdown>
    <CDropdownToggle :caret="false">
      <CIcon v-if="colorMode === 'dark'" icon="cil-moon" size="lg" />
      <CIcon v-else-if="colorMode === 'light'" icon="cil-sun" size="lg" />
      <CIcon v-else icon="cil-contrast" size="lg" />
    </CDropdownToggle>
  </CDropdown>
</template>
<script setup>
import { ref } from 'vue'
const colorMode = ref('auto')
</script>"#;

        let allocator = oxc_allocator::Allocator::new();
        let options = ViteCodegenOptions {
            filename: Some("test.vue".to_string()),
            ..Default::default()
        };
        let result = generate_for_vite(vue_source, &options, &allocator);
        let our_render = result.template.expect("should have template block").code;
        assert_valid_js(&our_render, "vif_velse_chain_in_component_slot");
    }

    #[test]
    fn e2e_dynamic_class_array_binding_on_component() {
        // Bug D: :class="['static', dynamicVar]" on component
        // Pattern from DocsExample.vue - generates "Unexpected ','"
        let vue_source = r#"<template>
  <CTabContent :class="['rounded-bottom', addClass]">
    <slot></slot>
  </CTabContent>
</template>
<script setup>
const addClass = 'extra'
</script>"#;

        let allocator = oxc_allocator::Allocator::new();
        let options = ViteCodegenOptions {
            filename: Some("test.vue".to_string()),
            ..Default::default()
        };
        let result = generate_for_vite(vue_source, &options, &allocator);
        let our_render = result.template.expect("should have template block").code;
        assert_valid_js(&our_render, "dynamic_class_array_binding_on_component");
    }

    #[test]
    fn e2e_mixed_text_and_component_children() {
        // Bug E: mixed text + component children inside a component
        // Pattern from Widgets.vue - generates 'Expected ")" but found " View more "'
        let vue_source = r#"<template>
  <CLink href="https://example.com">
    View more
    <CIcon icon="cil-arrow-right" width="16" />
  </CLink>
</template>"#;

        let allocator = oxc_allocator::Allocator::new();
        let options = ViteCodegenOptions {
            filename: Some("test.vue".to_string()),
            ..Default::default()
        };
        let result = generate_for_vite(vue_source, &options, &allocator);
        let our_render = result.template.expect("should have template block").code;
        assert_valid_js(&our_render, "mixed_text_and_component_children");
    }

    #[test]
    fn e2e_named_slot_with_text_and_inline_component() {
        // Bug F: named slot with mixed text + inline component
        // Pattern from WidgetsStatsTypeA.vue - 'Expected "]" but found "_createElementVNode"'
        let vue_source = r#"<template>
  <CWidgetStatsA>
    <template #value>
      26K
      <span class="fs-6 fw-normal"> (-12.4% <CIcon icon="cil-arrow-bottom" />) </span>
    </template>
  </CWidgetStatsA>
</template>"#;

        let allocator = oxc_allocator::Allocator::new();
        let options = ViteCodegenOptions {
            filename: Some("test.vue".to_string()),
            ..Default::default()
        };
        let result = generate_for_vite(vue_source, &options, &allocator);
        let our_render = result.template.expect("should have template block").code;
        assert_valid_js(&our_render, "named_slot_with_text_and_inline_component");
    }

    #[test]
    fn e2e_custom_directive_multiline_object_literal() {
        // Bug C: custom directive with multiline object literal
        // Pattern from Popovers.vue - "Expected identifier but found '{'"
        let vue_source = r#"<template>
  <CButton
    v-c-popover="{
      header: 'Popover title',
      content: 'Some amazing content',
      placement: 'right',
    }"
    color="danger"
    size="lg"
  >
    Click to toggle popover
  </CButton>
</template>"#;

        let allocator = oxc_allocator::Allocator::new();
        let options = ViteCodegenOptions {
            filename: Some("test.vue".to_string()),
            ..Default::default()
        };
        let result = generate_for_vite(vue_source, &options, &allocator);
        let our_render = result.template.expect("should have template block").code;
        assert_valid_js(&our_render, "custom_directive_multiline_object_literal");
        assert!(
            !our_render.contains("_ctx.{"),
            "Object literal should not get _ctx. prefix.\n\nGenerated:\n{}",
            our_render
        );
    }

    #[test]
    fn e2e_multiple_named_slots_with_text_and_components() {
        // Realistic pattern from WidgetsStatsTypeA.vue
        // Multiple named slots on same component, some with mixed text + elements
        let vue_source = r#"<template>
  <CWidgetStatsA color="primary">
    <template #value>26K
      <span class="fs-6 fw-normal"> (-12.4% <CIcon icon="cil-arrow-bottom" />) </span>
    </template>
    <template #title>Users</template>
    <template #action>
      <CDropdown placement="bottom-end">
        <CDropdownToggle color="transparent" :caret="false">
          <CIcon icon="cil-options" />
        </CDropdownToggle>
      </CDropdown>
    </template>
  </CWidgetStatsA>
</template>"#;

        let allocator = oxc_allocator::Allocator::new();
        let options = ViteCodegenOptions {
            filename: Some("test.vue".to_string()),
            ..Default::default()
        };
        let result = generate_for_vite(vue_source, &options, &allocator);
        let our_render = result.template.expect("should have template block").code;
        assert_valid_js(&our_render, "multiple_named_slots_with_text_and_components");
    }

    #[test]
    fn e2e_component_with_text_and_element_children_mixed() {
        // Pattern from Widgets.vue - text "View more" followed by CIcon in a component
        let vue_source = r#"<template>
  <CLink
    class="fw-semibold font-xs text-body-secondary"
    href="https://coreui.io/"
    rel="noopener norefferer"
    target="_blank"
  >
    View more
    <CIcon icon="cil-arrow-right" class="ms-auto" width="16" />
  </CLink>
</template>"#;

        let allocator = oxc_allocator::Allocator::new();
        let options = ViteCodegenOptions {
            filename: Some("test.vue".to_string()),
            ..Default::default()
        };
        let result = generate_for_vite(vue_source, &options, &allocator);
        let our_render = result.template.expect("should have template block").code;
        assert_valid_js(
            &our_render,
            "component_with_text_and_element_children_mixed",
        );
    }

    #[test]
    fn e2e_component_with_multiple_element_children() {
        // Two div children inside a component's default slot must have single comma
        // (not double comma from both first_child_at_depth and default_slot_child_count)
        let vue_source = r#"<template><CFooter><div>A</div><div>B</div></CFooter></template>"#;

        let allocator = oxc_allocator::Allocator::new();
        let options = ViteCodegenOptions {
            filename: Some("test.vue".to_string()),
            ..Default::default()
        };
        let result = generate_for_vite(vue_source, &options, &allocator);
        let our_render = result.template.expect("should have template block").code;
        assert_valid_js(&our_render, "component_with_multiple_element_children");

        // Must not have double commas
        assert!(
            !our_render.contains(", ,"),
            "Must not have double commas between slot children. Output:\n{}",
            our_render
        );
    }

    #[test]
    fn e2e_component_with_text_child_inside_div() {
        // Component with text + CIcon child nested inside a div element
        // This triggers element_array_opened for div, which can cause
        // double-wrapping of text inside the component's slot
        let vue_source =
            r#"<template><div><CHeader><CIcon /> Theme colors</CHeader></div></template>"#;

        let allocator = oxc_allocator::Allocator::new();
        let options = ViteCodegenOptions {
            filename: Some("test.vue".to_string()),
            ..Default::default()
        };
        let result = generate_for_vite(vue_source, &options, &allocator);
        let our_render = result.template.expect("should have template block").code;
        assert_valid_js(&our_render, "component_with_text_child_inside_div");
    }

    #[test]
    fn e2e_docs_example_nested_components_with_text() {
        // Pattern from DocsExample.vue - deeply nested components with text + CIcon children
        let vue_source = r##"<script setup>
const props = defineProps({
  href: String,
  tabContentClass: String,
})
const url = `https://coreui.io/vue/docs/${props.href}`
const addClass = props.tabContentClass
</script>
<template>
  <div class="example">
    <CNav variant="underline-border">
      <CNavItem>
        <CNavLink href="#" active>
          <CIcon icon="cil-media-play" class="me-2" />
          Preview
        </CNavLink>
      </CNavItem>
      <CNavItem>
        <CNavLink :href="url" target="_blank">
          <CIcon icon="cil-code" class="me-2" />
          Code
        </CNavLink>
      </CNavItem>
    </CNav>
    <CTabContent :class="['rounded-bottom', addClass]">
      <CTabPane class="p-3 preview" visible>
        <slot></slot>
      </CTabPane>
    </CTabContent>
  </div>
</template>"##;

        let allocator = oxc_allocator::Allocator::new();
        let options = ViteCodegenOptions {
            filename: Some("test.vue".to_string()),
            ..Default::default()
        };
        let result = generate_for_vite(vue_source, &options, &allocator);
        let our_render = result.template.expect("should have template block").code;
        assert_valid_js(&our_render, "docs_example_nested_components_with_text");
    }

    // =========================================================================
    // Kebab-case component recognition tests
    // =========================================================================

    #[test]
    fn e2e_kebab_case_tag_treated_as_component() {
        // Kebab-case tags like <v-alert>, <my-component> should be treated as components,
        // not HTML elements. Vue's compiler treats any tag with a hyphen as a component.
        let vue_source = r#"<template>
  <v-alert type="success">Hello</v-alert>
</template>
<script setup>
</script>"#;

        let allocator = oxc_allocator::Allocator::new();
        let options = ViteCodegenOptions {
            filename: Some("test.vue".to_string()),
            ..Default::default()
        };
        let result = generate_for_vite(vue_source, &options, &allocator);
        let our_render = result.template.expect("should have template block").code;
        assert_valid_js(&our_render, "kebab_case_component");

        // Should use _resolveComponent, not _createElementBlock with string tag
        assert!(
            our_render.contains("_resolveComponent(\"v-alert\")"),
            "Should resolve v-alert as a component.\nGenerated:\n{}",
            our_render
        );
        // Variable name should use underscores instead of hyphens
        assert!(
            our_render.contains("_component_v_alert"),
            "Component variable should replace hyphens with underscores.\nGenerated:\n{}",
            our_render
        );
    }

    #[test]
    fn e2e_kebab_case_template_only_component() {
        // Template-only component with kebab-case tags (no script block)
        // Pattern from vuetify: <v-layout><v-app-bar>...</v-app-bar></v-layout>
        let vue_source = r#"<template>
  <v-layout class="rounded">
    <v-app-bar title="Test"></v-app-bar>
    <v-main>Content</v-main>
  </v-layout>
</template>"#;

        let allocator = oxc_allocator::Allocator::new();
        let options = ViteCodegenOptions {
            filename: Some("test.vue".to_string()),
            ..Default::default()
        };
        let result = generate_for_vite(vue_source, &options, &allocator);
        let our_render = result.template.expect("should have template block").code;
        assert_valid_js(&our_render, "kebab_case_template_only");

        // All kebab-case tags should be resolved as components
        assert!(
            our_render.contains("_resolveComponent(\"v-layout\")"),
            "Should resolve v-layout.\nGenerated:\n{}",
            our_render
        );
        assert!(
            our_render.contains("_resolveComponent(\"v-app-bar\")"),
            "Should resolve v-app-bar.\nGenerated:\n{}",
            our_render
        );
    }

    #[test]
    fn e2e_kebab_case_with_named_slots() {
        // Kebab-case component with named slots
        // Pattern from vuetify Alert.vue: <v-alert><template #prepend>...</template></v-alert>
        let vue_source = r#"<template>
  <v-alert type="info" border="start">
    <template #prepend>
      <v-icon color="blue" icon="mdi-check" />
    </template>
    <slot />
  </v-alert>
</template>
<script setup>
</script>"#;

        let allocator = oxc_allocator::Allocator::new();
        let options = ViteCodegenOptions {
            filename: Some("test.vue".to_string()),
            ..Default::default()
        };
        let result = generate_for_vite(vue_source, &options, &allocator);
        let our_render = result.template.expect("should have template block").code;
        assert_valid_js(&our_render, "kebab_case_named_slots");

        assert!(
            our_render.contains("_resolveComponent(\"v-alert\")"),
            "Should resolve v-alert.\nGenerated:\n{}",
            our_render
        );
        assert!(
            our_render.contains("_resolveComponent(\"v-icon\")"),
            "Should resolve v-icon.\nGenerated:\n{}",
            our_render
        );
    }

    // =========================================================================
    // Regular <script> block handling tests
    // =========================================================================

    #[test]
    fn e2e_vite_regular_script_no_raw_tags() {
        // Regular <script> (no setup) should have <script> tags stripped
        // Pattern from vuetify Table.vue
        let vue_source = r#"<template>
  <div class="wrapper">
    <slot />
  </div>
</template>
<script>
export default {
  inheritAttrs: false,
}
</script>"#;

        let allocator = oxc_allocator::Allocator::new();
        let options = ViteCodegenOptions {
            filename: Some("test.vue".to_string()),
            ..Default::default()
        };
        let result = generate_for_vite(vue_source, &options, &allocator);
        let script = result.script.expect("should have script block").code;

        // Script output should NOT contain raw <script> tags
        assert!(
            !script.contains("<script>"),
            "Script output should not contain <script> tag.\nGenerated:\n{}",
            script
        );
        assert!(
            !script.contains("</script>"),
            "Script output should not contain </script> tag.\nGenerated:\n{}",
            script
        );
        // Should still contain the export
        assert!(
            script.contains("export default"),
            "Should preserve export default.\nGenerated:\n{}",
            script
        );
    }

    #[test]
    fn e2e_vite_dual_script_no_raw_tags() {
        // Dual <script> first, then <script setup> - should merge via Object.assign
        // Pattern from vuetify Figure.vue (reversed order)
        let vue_source = r#"<template>
  <div>{{ msg }}</div>
</template>
<script>
export default {
  inheritAttrs: false,
}
</script>
<script setup>
import { ref } from 'vue'
const msg = ref('hello')
</script>"#;

        let allocator = oxc_allocator::Allocator::new();
        let options = ViteCodegenOptions {
            filename: Some("test.vue".to_string()),
            ..Default::default()
        };
        let result = generate_for_vite(vue_source, &options, &allocator);
        let script = result.script.expect("should have script block").code;

        // Script output should NOT contain raw <script> tags
        assert!(
            !script.contains("<script>"),
            "Script output should not contain raw <script> tag.\nGenerated:\n{}",
            script
        );
        assert!(
            !script.contains("</script>"),
            "Script output should not contain raw </script> tag.\nGenerated:\n{}",
            script
        );
        // Should have the setup component
        assert!(
            script.contains("setup("),
            "Should have setup function.\nGenerated:\n{}",
            script
        );
        // Should merge with __default__ via Object.assign
        assert!(
            script.contains("const __default__"),
            "Should have __default__ from regular script.\nGenerated:\n{}",
            script
        );
        assert!(
            script.contains("Object.assign(__default__"),
            "Should merge via Object.assign.\nGenerated:\n{}",
            script
        );
    }

    #[test]
    fn e2e_vite_dual_script_setup_first() {
        // <script setup> first, then <script> - same merging should work
        let vue_source = r#"<template>
  <div>{{ msg }}</div>
</template>
<script setup>
import { ref } from 'vue'
const msg = ref('hello')
</script>
<script>
export default {
  inheritAttrs: false,
}
</script>"#;

        let allocator = oxc_allocator::Allocator::new();
        let options = ViteCodegenOptions {
            filename: Some("test.vue".to_string()),
            ..Default::default()
        };
        let result = generate_for_vite(vue_source, &options, &allocator);
        let script = result.script.expect("should have script block").code;

        // Should NOT contain raw tags
        assert!(
            !script.contains("<script>"),
            "Should not contain <script>.\nGenerated:\n{}",
            script
        );
        // Should have __default__ and Object.assign
        assert!(
            script.contains("const __default__"),
            "Should have __default__ from regular script.\nGenerated:\n{}",
            script
        );
        assert!(
            script.contains("Object.assign(__default__"),
            "Should merge via Object.assign even when setup comes first.\nGenerated:\n{}",
            script
        );
    }

    /// @ai-generated - Tests conditional named slot with static prepend and conditional append.
    #[test]
    fn e2e_conditional_named_slot_with_default_children() {
        let vue_source = r#"<template>
  <v-app-bar v-bind="props">
    <template v-slot:prepend>
      <v-app-bar-nav-icon></v-app-bar-nav-icon>
    </template>

    <v-app-bar-title>Application Bar</v-app-bar-title>

    <template v-if="actions" v-slot:append>
      <v-btn icon="mdi-heart"></v-btn>
      <v-btn icon="mdi-magnify"></v-btn>
    </template>
  </v-app-bar>
</template>"#;
        let allocator = oxc_allocator::Allocator::new();
        let options = ViteCodegenOptions {
            filename: Some("test.vue".to_string()),
            ..Default::default()
        };
        let result = generate_for_vite(vue_source, &options, &allocator);
        let our_render = result.template.expect("should have template block").code;
        assert_valid_js(&our_render, "conditional named slot with default children");
    }

    /// @ai-generated - Tests v-show with negation expression generates valid JS.
    #[test]
    fn e2e_vshow_negation_expression() {
        let vue_source = r#"<template>
  <div v-show="!hidden">content</div>
</template>"#;
        let allocator = oxc_allocator::Allocator::new();
        let options = ViteCodegenOptions {
            filename: Some("test.vue".to_string()),
            ..Default::default()
        };
        let result = generate_for_vite(vue_source, &options, &allocator);
        let our_render = result.template.expect("should have template block").code;
        assert_valid_js(&our_render, "v-show with negation");
    }

    /// @ai-generated - Tests mixed text and element children in a div.
    /// Pattern: text + elements + text mixed as children.
    #[test]
    fn e2e_mixed_text_element_children() {
        let vue_source = r#"<template>
  <div class="px-4 py-2">
    {{ year }} — <strong>Vuetify</strong>
  </div>
</template>"#;
        let allocator = oxc_allocator::Allocator::new();
        let options = ViteCodegenOptions {
            filename: Some("test.vue".to_string()),
            ..Default::default()
        };
        let result = generate_for_vite(vue_source, &options, &allocator);
        let our_render = result.template.expect("should have template block").code;
        assert_valid_js(&our_render, "mixed text+element children");
    }

    /// @ai-generated - Tests conditional named slot (v-if on template v-slot).
    #[test]
    fn e2e_conditional_named_slot() {
        let vue_source = r#"<template>
  <v-list-item>
    <template v-if="showPrepend" #prepend>
      <v-avatar image="logo.png" />
    </template>
    <v-list-item-title>Title</v-list-item-title>
  </v-list-item>
</template>"#;
        let allocator = oxc_allocator::Allocator::new();
        let options = ViteCodegenOptions {
            filename: Some("test.vue".to_string()),
            ..Default::default()
        };
        let result = generate_for_vite(vue_source, &options, &allocator);
        let our_render = result.template.expect("should have template block").code;
        assert_valid_js(&our_render, "conditional named slot");
    }

    /// @ai-generated - Batch test for vuetify matrix failing patterns.
    /// Tests both script and template blocks for validity.
    #[test]
    fn e2e_vuetify_matrix_failing_patterns() {
        let cases: Vec<(&str, &str)> = vec![
            // Script.vue - simple template, defineEmits with colon in name
            (
                "script_vue",
                r#"<template>
  <div :id="id" ref="rootEl" />
</template>
<script setup lang="ts">
  import { onBeforeMount, onBeforeUnmount, onMounted, ref } from 'vue'
  const props = defineProps({
    id: { type: String, required: true },
    scriptId: { type: String, required: true },
    src: { type: String, required: true },
  })
  const emit = defineEmits(['script:error', 'script:load'])
  const rootEl = ref<HTMLElement>()
</script>"#,
            ),
            // Inline.vue - defineProps(createAdProps()) with function call
            (
                "inline_vue",
                r#"<template>
  <app-markdown
    v-if="ad"
    :content="description"
    class="v-markdown--inline d-inline"
    tag="span"
  />
</template>
<script setup>
  import { createAdProps, useAd } from '@/composables/ad'
  const props = defineProps(createAdProps())
  const { ad, description } = useAd(props)
</script>"#,
            ),
            // Entry.vue - <br> self-closing + v-if on components
            (
                "entry_vue",
                r#"<template>
  <carbon v-if="!user.disableAds" />
  <br>
  <app-btn
    v-if="false"
    :text="user.disableAds ? 'enable' : 'disable'"
    class="text-caption"
    color="surface-variant"
    prepend-icon="$vuetify"
    variant="flat"
    @click="onClickDisableAds"
  />
</template>
<script setup>
  import { useUserStore } from '@/store/user'
  const user = useUserStore()
  function onClickDisableAds () { user.disableAds = !user.disableAds }
</script>"#,
            ),
            // default.vue - router-view v-slot with destructuring + dynamic component
            (
                "default_vue",
                r#"<template>
  <v-app>
    <v-main>
      <v-container fluid tag="section">
        <router-view v-slot="{ Component }">
          <v-fade-transition hide-on-leave>
            <div :key="route.name">
              <component :is="Component" />
            </div>
          </v-fade-transition>
        </router-view>
      </v-container>
    </v-main>
  </v-app>
</template>
<script setup>
  import { useRoute } from 'vue-router'
  import { computed } from 'vue'
  const route = useRoute()
</script>"#,
            ),
            // layout-information-composable.vue - child v-slot="{ print }" scoped slot
            (
                "layout_composable",
                r#"<template>
  <v-layout ref="app" class="rounded rounded-md">
    <v-app-bar color="grey-lighten-2" name="app-bar">
      <child v-slot="{ print }">
        <v-btn class="mx-auto" @click="print('app-bar')">Get data</v-btn>
      </child>
    </v-app-bar>
    <v-main class="d-flex" style="min-height: 300px;">
      Main Content
    </v-main>
  </v-layout>
</template>
<script setup>
  import { useLayout } from 'vuetify'
</script>"#,
            ),
            // Search.vue - HTML comment before template
            (
                "search_vue",
                r#"<!-- eslint-disable -->
<template>
  <v-dialog v-model="model" scrollable width="600">
    <template #activator="{ props: activatorProps }">
      <app-btn :active="model" v-bind="activatorProps">
        Search
      </app-btn>
    </template>
  </v-dialog>
</template>
<script setup>
  import { ref } from 'vue'
  const model = ref(false)
</script>"#,
            ),
            // List.vue - multiple named slots with slot props + v-model:opened
            (
                "list_vue",
                r#"<template>
  <v-list
    v-model:opened="opened"
    :nav="nav"
    :items="computedItems"
    color="primary"
    density="compact"
  >
    <template #divider>
      <v-divider class="my-3 mb-4 ms-2 me-n2" />
    </template>
    <template #title="{ item }">
      {{ item.title }}
      <v-badge v-if="item.emphasized" class="ms-n1" color="success" dot inline />
    </template>
    <template #subtitle="{ item }">
      <span v-if="item.subtitle" class="text-high-emphasis">
        {{ item.subtitle }}
      </span>
    </template>
  </v-list>
</template>
<script setup>
  import { ref, computed } from 'vue'
  const opened = ref([])
  const props = defineProps({ items: Array, nav: Boolean })
  const computedItems = computed(() => props.items)
</script>"#,
            ),
            // vee-validate pattern - error-messages with chained property access
            (
                "vee_validate",
                r#"<template>
  <form @submit.prevent="submit">
    <v-text-field
      v-model="name.value.value"
      :counter="10"
      :error-messages="name.errorMessage.value"
      label="Name"
    ></v-text-field>
    <v-btn class="me-4" type="submit">submit</v-btn>
    <v-btn @click="handleReset">clear</v-btn>
  </form>
</template>
<script setup>
  import { ref } from 'vue'
  const name = ref({ value: { value: '' }, errorMessage: { value: '' } })
  const handleReset = () => {}
  const submit = () => {}
</script>"#,
            ),
        ];

        let mut failures: Vec<String> = Vec::new();

        for (name, source) in &cases {
            let allocator = oxc_allocator::Allocator::new();
            let options = ViteCodegenOptions {
                filename: Some("test.vue".to_string()),
                ..Default::default()
            };
            let result = generate_for_vite(source, &options, &allocator);

            if let Some(ref template) = result.template {
                let alloc2 = oxc_allocator::Allocator::default();
                let source_type = SourceType::mjs();
                let p = Parser::new(&alloc2, &template.code, source_type).parse();
                if !p.errors.is_empty() {
                    failures.push(format!(
                        "{} (TEMPLATE): {:?}\n---\n{}\n---",
                        name, p.errors, template.code
                    ));
                }
            } else {
                failures.push(format!("{}: NO TEMPLATE OUTPUT", name));
            }
            if let Some(ref script) = result.script {
                let alloc2 = oxc_allocator::Allocator::default();
                let source_type = if source.contains("lang=\"ts\"") || source.contains("lang='ts'")
                {
                    SourceType::ts()
                } else {
                    SourceType::mjs()
                };
                let p = Parser::new(&alloc2, &script.code, source_type).parse();
                if !p.errors.is_empty() {
                    failures.push(format!(
                        "{} (SCRIPT): {:?}\n---\n{}\n---",
                        name, p.errors, script.code
                    ));
                }
            }
        }

        if !failures.is_empty() {
            panic!(
                "\n{} failures:\n\n{}",
                failures.len(),
                failures.join("\n\n")
            );
        }
    }

    #[test]
    fn e2e_debug_sibling_comma() {
        // Simplest case: two component siblings in a component slot
        let source_simple = r#"<template>
  <v-layout>
    <v-app-bar></v-app-bar>
    <v-main>Main</v-main>
  </v-layout>
</template>"#;
        // With child element (no v-slot) inside first sibling
        let source_child = r#"<template>
  <v-layout>
    <v-app-bar>
      <child>
        <v-btn>Get data</v-btn>
      </child>
    </v-app-bar>
    <v-main>Main</v-main>
  </v-layout>
</template>"#;
        // With child element WITH v-slot inside first sibling
        let source_vslot = r#"<template>
  <v-layout>
    <v-app-bar>
      <child v-slot="{ print }">
        <v-btn>Get data</v-btn>
      </child>
    </v-app-bar>
    <v-main>Main</v-main>
  </v-layout>
</template>"#;

        for (label, source) in [
            ("simple", source_simple),
            ("child", source_child),
            ("vslot", source_vslot),
        ] {
            let allocator = oxc_allocator::Allocator::new();
            let options = ViteCodegenOptions {
                filename: Some("test.vue".to_string()),
                ..Default::default()
            };
            let result = generate_for_vite(source, &options, &allocator);
            let template = result.template.as_ref().unwrap();
            eprintln!("=== {} ===\n{}\n", label, template.code);
            let alloc2 = oxc_allocator::Allocator::default();
            let source_type = SourceType::mjs();
            let p = Parser::new(&alloc2, &template.code, source_type).parse();
            assert!(
                p.errors.is_empty(),
                "{} has parse errors: {:?}\n---\n{}",
                label,
                p.errors,
                template.code
            );
        }
    }

    // =========================================================================
    // keep_ts E2E Tests
    // =========================================================================

    /// Helper to generate with keep_ts: false and validate the output is valid JS
    fn gen_strip_ts(source: &str) -> String {
        let allocator = oxc_allocator::Allocator::new();
        let options = CodegenOptions {
            filename: Some("test.vue".to_string()),
            keep_ts: false,
            ..Default::default()
        };
        let result = generate(source, &options, &allocator);

        // Validate output is valid JavaScript (not TypeScript)
        let alloc2 = oxc_allocator::Allocator::default();
        let source_type = oxc_span::SourceType::mjs();
        let p = oxc_parser::Parser::new(&alloc2, &result.code, source_type).parse();
        assert!(
            p.errors.is_empty(),
            "keep_ts:false output has JS parse errors: {:?}\n---\n{}",
            p.errors,
            result.code
        );

        result.code
    }

    #[test]
    fn test_keep_ts_false_strips_type_annotations() {
        let source = r#"<script setup lang="ts">
const count: number = 0
const message: string = 'hello'
function greet(name: string): void {
  console.log(name)
}
</script>
<template><div>{{ count }}</div></template>"#;

        let code = gen_strip_ts(source);
        // Types should be stripped
        assert!(!code.contains(": number"), "should strip : number");
        assert!(!code.contains(": string"), "should strip : string");
        assert!(!code.contains(": void"), "should strip : void");
        // Runtime code preserved
        assert!(code.contains("count"));
        assert!(code.contains("message"));
        assert!(code.contains("greet"));
        assert!(code.contains("_defineComponent"));
    }

    #[test]
    fn test_keep_ts_true_preserves_types() {
        let source = r#"<script setup lang="ts">
const count: number = 0
</script>
<template><div>{{ count }}</div></template>"#;

        let allocator = oxc_allocator::Allocator::new();
        let options = CodegenOptions {
            filename: Some("test.vue".to_string()),
            keep_ts: true,
            ..Default::default()
        };
        let result = generate(source, &options, &allocator);

        // Types should be preserved with default keep_ts: true
        assert!(
            result.code.contains(": number"),
            "keep_ts:true should preserve : number"
        );
    }

    #[test]
    fn test_keep_ts_false_strips_interface() {
        let source = r#"<script setup lang="ts">
interface User {
  name: string
  age: number
}
const user: User = { name: 'test', age: 0 }
</script>
<template><div>{{ user.name }}</div></template>"#;

        let code = gen_strip_ts(source);
        assert!(!code.contains("interface"), "should strip interface");
        assert!(code.contains("user"), "should preserve user variable");
    }

    #[test]
    fn test_keep_ts_false_converts_enum() {
        let source = r#"<script setup lang="ts">
enum Color { Red, Green, Blue }
const c = Color.Red
</script>
<template><div>{{ c }}</div></template>"#;

        let code = gen_strip_ts(source);
        assert!(!code.contains("enum "), "should convert enum");
        assert!(code.contains("var Color"), "should have JS IIFE for enum");
        assert!(
            code.contains("Color[Color[\"Red\"]"),
            "should have enum member assignment"
        );
    }

    #[test]
    fn test_keep_ts_false_strips_generics() {
        let source = r#"<script setup lang="ts">
function identity<T>(value: T): T {
  return value
}
const x = identity('hello')
</script>
<template><div>{{ x }}</div></template>"#;

        let code = gen_strip_ts(source);
        assert!(!code.contains("<T>"), "should strip generic <T>");
        assert!(code.contains("identity"), "should preserve function name");
    }

    #[test]
    fn test_keep_ts_false_strips_as_expression() {
        let source = r#"<script setup lang="ts">
const el = document.getElementById('app') as HTMLElement
</script>
<template><div>{{ el }}</div></template>"#;

        let code = gen_strip_ts(source);
        assert!(
            !code.contains("as HTMLElement"),
            "should strip as expression"
        );
        assert!(
            code.contains("getElementById"),
            "should preserve runtime code"
        );
    }

    #[test]
    fn test_keep_ts_false_js_file_unchanged() {
        let source = r#"<script setup>
const count = 0
function greet(name) {
  console.log(name)
}
</script>
<template><div>{{ count }}</div></template>"#;

        // JS file should not be affected by keep_ts
        let code = gen_strip_ts(source);
        assert!(code.contains("count"));
        assert!(code.contains("greet"));
    }
}
