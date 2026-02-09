//! Vue codegen plugin for the syntax pipeline.

use oxc_allocator::Allocator;

use crate::{
    code_transform::{CodeTransform, SourceMapOptions},
    common::Span,
    syntax::{
        plugin::{SyntaxPlugin, SyntaxPluginContext, SyntaxResult},
        types::{SyntaxEvent, SyntaxTagType},
    },
};

use super::script::process_script;
use super::template;
use super::template::types::{BindingMetadata, CurrentElement, TemplateCodegenState};

pub struct VueCodegenPlugin<'a> {
    code_transform: CodeTransform<'a>,
    found_script: bool,
    /// State for streaming template codegen.
    state: TemplateCodegenState,
    /// Component name (typically derived from filename).
    component_name: String,
    /// Production mode - affects code generation (inline render, no expose, etc.)
    is_production: bool,
}

impl<'a> VueCodegenPlugin<'a> {
    pub fn new(source: &'a str, alloc: &'a Allocator) -> Self {
        Self {
            code_transform: CodeTransform::new(source, alloc),
            found_script: false,
            state: TemplateCodegenState::new(),
            component_name: "App".to_string(),
            is_production: false,
        }
    }

    /// Create a new plugin with a custom component name.
    pub fn with_component_name(source: &'a str, alloc: &'a Allocator, name: &str) -> Self {
        Self {
            code_transform: CodeTransform::new(source, alloc),
            found_script: false,
            state: TemplateCodegenState::new(),
            component_name: name.to_string(),
            is_production: false,
        }
    }

    /// Create a new plugin with component name and production mode.
    pub fn with_options(
        source: &'a str,
        alloc: &'a Allocator,
        name: &str,
        is_production: bool,
    ) -> Self {
        Self {
            code_transform: CodeTransform::new(source, alloc),
            found_script: false,
            state: TemplateCodegenState::new(),
            component_name: name.to_string(),
            is_production,
        }
    }

    /// Set production mode.
    /// In monolithic generate(), production mode implies inline template mode.
    pub fn set_production(&mut self, is_production: bool) {
        self.is_production = is_production;
        self.state.is_production = is_production;
        // Monolithic generate() inlines the template into setup() in production,
        // so inline_mode = is_production.
        self.state.is_inline_mode = is_production;
    }

    /// Set the component name.
    pub fn set_component_name(&mut self, name: &str) {
        self.component_name = name.to_string();
    }

    /// Set whitespace handling mode (condense or preserve).
    pub fn set_whitespace(&mut self, mode: super::template::types::WhitespaceMode) {
        self.state.whitespace = mode;
    }

    /// Set scope ID for scoped styles (8-byte hash).
    /// This should be called before the pipeline runs if scoped styles are detected.
    pub fn set_scope_id(&mut self, scope_id: [u8; 8]) {
        self.state.scope_id = Some(scope_id);
    }

    /// Set binding metadata from pre-scan (for template-before-script ordering).
    pub fn set_binding_metadata(&mut self, metadata: BindingMetadata) {
        self.state.binding_metadata = metadata;
    }

    /// Get the transformed code as a string
    pub fn get_code(&self) -> String {
        self.code_transform.to_string()
    }

    /// Generate source map JSON string
    pub fn generate_source_map(&self, options: SourceMapOptions) -> String {
        self.code_transform.generate_map_json(options)
    }

    /// Check if the code has been modified
    pub fn is_modified(&self) -> bool {
        self.code_transform.is_modified()
    }

    // ==================== Syntax Plugin Methods ====================

    /// Called when syntax pipeline starts
    pub fn syntax_start(&mut self, _ctx: &SyntaxPluginContext<'a>) {}

    /// Called when syntax pipeline ends
    pub fn syntax_end(&mut self, _ctx: &SyntaxPluginContext<'a>) {
        if !self.found_script {
            // No script found, add default empty component export
            self.code_transform.prepend("export default {};\n");
        }

        // Collect all CSS for __css__ export
        let mut all_css: Vec<String> = Vec::new();

        // Add scoped/plain CSS if present
        if let Some(ref css) = self.state.transformed_css {
            if let Ok(css_str) = std::str::from_utf8(css) {
                all_css.push(Self::escape_css_for_js(css_str));
            }
        }

        // Add CSS modules CSS
        for module in &self.state.css_modules {
            if let Ok(css_str) = std::str::from_utf8(&module.css) {
                all_css.push(Self::escape_css_for_js(css_str));
            }
        }

        // Export __css__ if there's any CSS
        if !all_css.is_empty() {
            let css_entries = all_css
                .iter()
                .map(|s| format!("  \"{}\"", s))
                .collect::<Vec<_>>()
                .join(",\n");
            let css_export = format!("\nexport const __css__ = [\n{}\n];\n", css_entries);
            self.code_transform.append(&css_export);
        }

        // Export __cssModules__ if there are CSS modules
        if !self.state.css_modules.is_empty() {
            let mut modules_obj = String::from("\nexport const __cssModules__ = {\n");

            for module in &self.state.css_modules {
                modules_obj.push_str(&format!("  \"{}\": {{\n", module.name));

                for (original, hashed) in &module.classes {
                    modules_obj.push_str(&format!("    \"{}\": \"{}\",\n", original, hashed));
                }

                modules_obj.push_str("  },\n");
            }

            modules_obj.push_str("};\n");
            self.code_transform.append(&modules_obj);
        }
    }

    /// Escape CSS string for JavaScript string literal
    fn escape_css_for_js(css: &str) -> String {
        css.replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('\n', "\\n")
            .replace('\r', "\\r")
    }

    /// Process CSS style content - extract scope ID and v-bind expressions
    fn process_css_style_content(
        &mut self,
        css: &crate::syntax::types::CssStyleContent,
        ctx: &SyntaxPluginContext<'a>,
    ) {
        use super::template::types::CssModuleEntry;
        use crate::builder::codegen::get_hash;

        let css_content = &ctx.bytes[css.content_start as usize..css.content_end as usize];

        // Handle CSS modules
        if let Some(ref module_span) = css.module {
            // Generate component ID for hashing
            let hash = get_hash(&self.component_name);
            let hash_bytes = hash.as_bytes();
            let mut component_id = [0u8; 8];
            component_id.copy_from_slice(&hash_bytes[..8.min(hash_bytes.len())]);

            // Get module name: default is "$style", or use custom name
            let module_name = if module_span.start == 0 && module_span.end == 0 {
                "$style".to_string()
            } else {
                ctx.input[module_span.start as usize..module_span.end as usize].to_string()
            };

            // Transform CSS for modules
            match crate::syntax::plugins::css_parser::transform_css_modules(
                css_content,
                &component_id,
            ) {
                Ok(result) => {
                    self.state.css_modules.push(CssModuleEntry {
                        name: module_name,
                        classes: result.class_mapping,
                        css: result.css,
                    });
                }
                Err(_e) => {
                    // If transformation fails, store original CSS content
                    self.state.css_modules.push(CssModuleEntry {
                        name: module_name,
                        classes: vec![],
                        css: css_content.to_vec(),
                    });
                }
            }
        }
        // Handle scoped styles
        else if css.scoped {
            // Generate 8-character scope ID from component name
            let hash = get_hash(&self.component_name);
            let hash_bytes = hash.as_bytes();
            let mut scope_id = [0u8; 8];
            scope_id.copy_from_slice(&hash_bytes[..8.min(hash_bytes.len())]);
            self.state.scope_id = Some(scope_id);

            // Transform the CSS content with scoping
            match crate::syntax::plugins::css_parser::transform_scoped_css(
                css_content,
                &scope_id,
                css.content_start,
            ) {
                Ok(result) => {
                    // Store v-bind expressions for inline style injection on root element
                    self.state.css_v_bind_expressions = result.v_bind_expressions;

                    // Store transformed CSS for later export
                    self.state.transformed_css = Some(result.css);
                }
                Err(_e) => {
                    // If transformation fails, store original CSS content
                    self.state.transformed_css = Some(css_content.to_vec());
                }
            }
        }
        // Plain style - just store CSS content
        else {
            self.state.transformed_css = Some(css_content.to_vec());
        }

        // Remove the entire style block from JS output
        // The CSS will be exported separately via __css__ or handled by bundler
        self.code_transform
            .replace(css.tag_open_start, css.tag_close_end, "");
    }

    /// Process a syntax event
    pub fn process_syntax_event(
        &mut self,
        event: SyntaxEvent<'a>,
        ctx: &SyntaxPluginContext<'a>,
    ) -> SyntaxResult<SyntaxEvent<'a>> {
        match &event {
            // Script events
            SyntaxEvent::AnalysedScript(ref info) => {
                self.found_script = true;
                // In monolithic generate(), inline_template = is_production
                // (template is inlined into setup's return value)
                let inline_template = self.is_production;
                let (imports, script_end, binding_metadata, closing_paren, _closing_text) =
                    process_script(
                        info,
                        &mut self.code_transform,
                        ctx.input,
                        &self.component_name,
                        self.is_production,
                        inline_template,
                    );
                // Store script end position for template placement
                self.state.script_end_position = Some(script_end);
                // Store closing paren for inline template finalization
                self.state.inline_closing_paren = closing_paren;
                // Store binding metadata for template accessor prefixes
                if self.state.binding_metadata.is_empty() {
                    self.state.binding_metadata = binding_metadata;
                }

                // Emit script-related Vue imports at the top of the file
                if !imports.is_empty() {
                    let mut import_code = String::new();
                    for import in imports {
                        import_code.push_str("import { ");
                        let specifiers: Vec<String> = import
                            .specifiers
                            .iter()
                            .map(|s| {
                                if let Some(ref alias) = s.alias {
                                    format!("{} as {}", s.name, alias)
                                } else {
                                    s.name.clone()
                                }
                            })
                            .collect();
                        import_code.push_str(&specifiers.join(", "));
                        import_code.push_str(" } from '");
                        import_code.push_str(&import.source);
                        import_code.push_str("'\n");
                    }
                    self.code_transform.prepend(&import_code);
                }
            }

            // ==================== Template Events ====================

            // OpenTagStart: Start building current element
            SyntaxEvent::OpenTagStart(ref open_tag) => {
                // Skip RootTemplate - it's handled in OpenTagEnd
                if open_tag.tag_type != SyntaxTagType::RootTemplate && self.state.render_started {
                    // Start accumulating props for this element
                    self.state.current_element = Some(CurrentElement {
                        tag_name: Span::new(open_tag.start + 1, open_tag.name_end),
                        tag_type: open_tag.tag_type.clone(),
                        props: vec![],
                        v_for: None,
                        v_if: None,
                        v_slot: None,
                        custom_directives: vec![],
                        v_model: None,
                        v_once: false,
                        element_id: open_tag.element_id,
                        scope_id: None,
                        has_key: false,
                        start: open_tag.start,
                    });
                }
            }

            // OpenTagEnd: Emit element opening (all props collected)
            SyntaxEvent::OpenTagEnd(ref open_tag) => {
                if open_tag.tag_type == SyntaxTagType::RootTemplate {
                    // Replace <template> tag with render function opening
                    template::element::process_root_template_open(
                        open_tag,
                        &mut self.state,
                        &mut self.code_transform,
                        ctx.input,
                    );
                } else if self.state.render_started {
                    template::element::process_open_tag_end(
                        open_tag,
                        &mut self.state,
                        &mut self.code_transform,
                        ctx.input,
                    );
                }
            }

            // CloseTag: Emit element closing
            SyntaxEvent::CloseTag(ref close_tag) => {
                if close_tag.tag_type == SyntaxTagType::RootTemplate {
                    // Replace </template> with closing brace
                    template::element::finalize_template_close(
                        close_tag,
                        &mut self.state,
                        &mut self.code_transform,
                        ctx.input,
                    );
                    // Add imports at the start of render function
                    template::element::finalize_template(
                        &mut self.state,
                        &mut self.code_transform,
                        ctx.input,
                    );
                } else if self.state.render_started {
                    template::element::process_close_tag(
                        close_tag,
                        &mut self.state,
                        &mut self.code_transform,
                        ctx.input,
                    );
                }
            }

            // AnalysedCloseScopes: Handle scope closings (v-for, v-slot)
            SyntaxEvent::AnalysedCloseScopes(ref e) => {
                if e.event.tag_type == SyntaxTagType::RootTemplate {
                    // Replace </template> with closing brace
                    template::element::finalize_template_close(
                        &e.event,
                        &mut self.state,
                        &mut self.code_transform,
                        ctx.input,
                    );
                    // Add imports at the start of render function
                    template::element::finalize_template(
                        &mut self.state,
                        &mut self.code_transform,
                        ctx.input,
                    );
                } else if self.state.render_started {
                    template::element::process_close_scopes(
                        e,
                        &mut self.state,
                        &mut self.code_transform,
                        ctx.input,
                    );
                }
            }

            // ==================== Interpolation Events ====================
            SyntaxEvent::AnalysedInterpolation(ref analysed) => {
                if self.state.render_started {
                    template::interpolation::process_analysed_interpolation(
                        analysed,
                        &mut self.state,
                        &mut self.code_transform,
                        ctx.input,
                    );
                }
            }

            // ==================== Directive Events ====================

            // Analysed directive events (with scope information) - these come before OpenTagEnd
            SyntaxEvent::AnalysedCondition(ref analysed) => {
                if self.state.render_started {
                    template::directives::process_analysed_v_if(
                        analysed,
                        &mut self.state,
                        &mut self.code_transform,
                        ctx.input,
                    );
                }
            }
            SyntaxEvent::AnalysedVFor(ref analysed) => {
                if self.state.render_started {
                    template::directives::process_analysed_v_for(
                        analysed,
                        &mut self.state,
                        &mut self.code_transform,
                        ctx.input,
                    );
                }
            }
            SyntaxEvent::AnalysedVSlot(ref analysed) => {
                if self.state.render_started {
                    template::directives::process_analysed_v_slot(
                        analysed,
                        &mut self.state,
                        &mut self.code_transform,
                        ctx.input,
                    );
                }
            }
            SyntaxEvent::AnalysedProp(ref analysed) => {
                if self.state.render_started {
                    template::directives::process_analysed_prop(
                        analysed,
                        &mut self.state,
                        &mut self.code_transform,
                        ctx.input,
                    );
                }
            }

            // Text events
            SyntaxEvent::Text(ref text) => {
                if self.state.render_started {
                    template::element::process_text(
                        text,
                        &mut self.state,
                        &mut self.code_transform,
                        ctx.input,
                    );
                }
            }

            // Comment events
            SyntaxEvent::Comment(ref comment) => {
                if self.state.render_started {
                    template::element::process_comment(
                        comment,
                        &mut self.state,
                        &mut self.code_transform,
                        ctx.input,
                    );
                }
            }

            // CSS Style events
            SyntaxEvent::CssStyleContent(ref css) => {
                self.process_css_style_content(css, ctx);
            }

            _ => {}
        }

        SyntaxResult::Keep(event)
    }
}

impl<'a> SyntaxPlugin<'a> for VueCodegenPlugin<'a> {
    fn name(&self) -> &str {
        "VueCodegen"
    }

    fn start(&mut self, ctx: &SyntaxPluginContext<'a>) {
        self.syntax_start(ctx);
    }

    fn end(&mut self, ctx: &SyntaxPluginContext<'a>) {
        self.syntax_end(ctx);
    }

    fn process_event(
        &mut self,
        event: SyntaxEvent<'a>,
        ctx: &mut SyntaxPluginContext<'a>,
    ) -> SyntaxResult<SyntaxEvent<'a>> {
        self.process_syntax_event(event, ctx)
    }
}
