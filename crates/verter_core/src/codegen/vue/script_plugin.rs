//! Script codegen plugin - processes `<script>` blocks independently.
//!
//! Owns a `CodeTransform` wrapping the full SFC source.
//! Handles only script-related events, observes template/style events
//! to record their positions for removal in `end()`.

use crate::{
    code_transform::{CodeTransform, SourceMapOptions},
    syntax::{
        plugin::{SyntaxPlugin, SyntaxPluginContext, SyntaxResult},
        types::{SyntaxEvent, SyntaxTagType},
    },
};

use super::script::process_script;

pub struct ScriptCodegenPlugin<'a> {
    code_transform: CodeTransform<'a>,
    found_script: bool,
    /// Whether a regular <script> (non-setup) block was found.
    has_normal_script: bool,
    /// Content range of the regular <script> block (content_start, content_end).
    normal_script_content: Option<(u32, u32)>,
    /// Setup script positions for dual-script wrapping.
    setup_tag_open_start: Option<u32>,
    setup_tag_open_end: Option<u32>,
    setup_tag_close_start: Option<u32>,
    setup_tag_close_end: Option<u32>,
    /// The closing overwrite text applied by process_script for the setup block.
    /// Saved so we can re-overwrite with ")" added for dual-script.
    setup_closing_text: Option<String>,
    component_name: String,
    is_production: bool,
    /// When true, preserve TypeScript syntax. When false, strip type annotations.
    keep_ts: bool,
    /// Regular <script> tag range (tag_open_start, tag_close_end).
    normal_script_tag_start: Option<u32>,
    normal_script_tag_end: Option<u32>,
    /// Region positions observed from other root elements.
    /// (start, end) in UTF-8 byte offsets of the opening tag start to closing tag end.
    template_region: Option<(u32, u32)>,
    style_regions: Vec<(u32, u32)>,
    /// Scope ID for scoped styles.
    scope_id: Option<[u8; 8]>,
}

impl<'a> ScriptCodegenPlugin<'a> {
    pub fn new(source: &'a str, alloc: &'a oxc_allocator::Allocator) -> Self {
        Self {
            code_transform: CodeTransform::new(source, alloc),
            found_script: false,
            has_normal_script: false,
            normal_script_content: None,
            setup_tag_open_start: None,
            setup_tag_open_end: None,
            setup_tag_close_start: None,
            setup_tag_close_end: None,
            setup_closing_text: None,
            normal_script_tag_start: None,
            normal_script_tag_end: None,
            component_name: "App".to_string(),
            is_production: false,
            keep_ts: true,
            template_region: None,
            style_regions: Vec::new(),
            scope_id: None,
        }
    }

    pub fn set_component_name(&mut self, name: &str) {
        self.component_name = name.to_string();
    }

    pub fn set_production(&mut self, is_production: bool) {
        self.is_production = is_production;
    }

    pub fn set_keep_ts(&mut self, keep_ts: bool) {
        self.keep_ts = keep_ts;
    }

    pub fn set_scope_id(&mut self, scope_id: [u8; 8]) {
        self.scope_id = Some(scope_id);
    }

    /// Get the transformed code (script block only).
    pub fn get_code(&self) -> String {
        self.code_transform.to_string()
    }

    /// Generate source map JSON string.
    pub fn generate_source_map(&self, options: SourceMapOptions) -> String {
        self.code_transform.generate_map_json(options)
    }

    /// Check if any script was found and processed.
    pub fn has_script(&self) -> bool {
        self.found_script
    }
}

impl<'a> SyntaxPlugin<'a> for ScriptCodegenPlugin<'a> {
    fn name(&self) -> &str {
        "ScriptCodegen"
    }

    fn end(&mut self, _ctx: &SyntaxPluginContext<'a>) {
        // Remove all content outside script block(s).
        // This covers template/style regions AND any stray content (HTML comments,
        // whitespace, etc.) before/after/between recognized blocks.
        let script_starts = [self.setup_tag_open_start, self.normal_script_tag_start];
        let script_ends = [self.setup_tag_close_end, self.normal_script_tag_end];
        let min_start = script_starts.iter().flatten().copied().min();
        let max_end = script_ends.iter().flatten().copied().max();

        if let Some(start) = min_start {
            if start > 0 {
                self.code_transform.remove(0, start);
            }
        }
        if let Some(end) = max_end {
            let src_len = _ctx.input.len() as u32;
            if end < src_len {
                self.code_transform.remove(end, src_len);
            }
        }

        // For dual-script, also remove content between the two script blocks
        // (template, styles, comments, etc. that fall between them).
        if self.setup_tag_open_start.is_some() && self.normal_script_tag_start.is_some() {
            let first_end = script_ends.iter().flatten().copied().min().unwrap();
            let second_start = script_starts.iter().flatten().copied().max().unwrap();
            if first_end < second_start {
                self.code_transform.remove(first_end, second_start);
            }
        }

        // If no script found, add default empty component export
        if !self.found_script {
            self.code_transform.prepend("export default {};\n");
        }

        // Dual-script handling: when both <script> and <script setup> exist.
        let is_dual_script = self.has_normal_script && self.setup_tag_open_start.is_some();

        if is_dual_script {
            let setup_open_start = self.setup_tag_open_start.unwrap();
            let setup_open_end = self.setup_tag_open_end.unwrap();
            let setup_close_start = self.setup_tag_close_start.unwrap();
            let setup_close_end = self.setup_tag_close_end.unwrap();

            // 1. Replace "export default" → "const __default__ =" in regular script content.
            if let Some((content_start, content_end)) = self.normal_script_content {
                let content = &_ctx.input[content_start as usize..content_end as usize];
                if let Some(pos) = content.find("export default") {
                    let abs_pos = content_start + pos as u32;
                    self.code_transform
                        .overwrite(abs_pos, abs_pos + 14, "const __default__ =");
                }

                // 2. If regular script comes after setup, move it before setup.
                // This ensures `const __default__` is defined before `Object.assign(__default__, ...)`.
                if content_start > setup_open_start {
                    self.code_transform
                        .move_slice(content_start, content_end, setup_open_start);
                }
            }

            // 3. Wrap setup opening with Object.assign(__default__, ...).
            // Re-overwrite the opening tag range to include the Object.assign wrapper.
            self.code_transform.overwrite(
                setup_open_start,
                setup_open_end,
                "export default /*@__PURE__*/Object.assign(__default__, ",
            );

            // 4. Re-overwrite the closing tag to include the closing ")" for Object.assign.
            if let Some(ref closing_text) = self.setup_closing_text {
                // Insert ")" before the final ";" in the closing text.
                let modified = if let Some(semi_pos) = closing_text.rfind(';') {
                    format!(
                        "{}){}",
                        &closing_text[..semi_pos],
                        &closing_text[semi_pos..]
                    )
                } else {
                    // Fallback: just append ")"
                    format!("{})", closing_text)
                };
                self.code_transform
                    .overwrite(setup_close_start, setup_close_end, &modified);
            }
        }
    }

    fn process_event(
        &mut self,
        event: SyntaxEvent<'a>,
        ctx: &mut SyntaxPluginContext<'a>,
    ) -> SyntaxResult<SyntaxEvent<'a>> {
        match &event {
            // Handle script events
            SyntaxEvent::AnalysedScript(ref info) => {
                self.found_script = true;

                // Track regular <script> (no setup) for dual-script merging
                if info.event.setup.is_none() {
                    self.has_normal_script = true;
                    self.normal_script_content =
                        Some((info.event.content_start, info.event.content_end));
                    self.normal_script_tag_start = Some(info.event.tag_open_start);
                    self.normal_script_tag_end = Some(info.event.tag_close_end);
                } else {
                    // Track setup script positions for dual-script wrapping in end()
                    self.setup_tag_open_start = Some(info.event.tag_open_start);
                    self.setup_tag_open_end = Some(info.event.tag_open_end);
                    self.setup_tag_close_start = Some(info.event.tag_close_start);
                    self.setup_tag_close_end = Some(info.event.tag_close_end);
                }

                // Vite split mode: template is always standalone (separate block)
                let (imports, _script_end, _binding_metadata, _closing_paren, closing_text) =
                    process_script(
                        info,
                        &mut self.code_transform,
                        ctx.input,
                        &self.component_name,
                        self.is_production,
                        false, // inline_template = false for Vite split mode
                        self.keep_ts,
                    );

                // Save setup closing text for potential dual-script re-overwrite in end()
                if info.event.setup.is_some() && !closing_text.is_empty() {
                    self.setup_closing_text = Some(closing_text);
                }

                // Emit script-related Vue imports at the top
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

            // Observe template region — record positions for removal
            SyntaxEvent::OpenTagEnd(ref open_tag)
                if open_tag.tag_type == SyntaxTagType::RootTemplate =>
            {
                // Record template start (the opening tag start)
                self.template_region = Some((open_tag.start, open_tag.start));
            }
            SyntaxEvent::CloseTag(ref close_tag)
                if close_tag.tag_type == SyntaxTagType::RootTemplate =>
            {
                // Update template end
                if let Some(ref mut region) = self.template_region {
                    region.1 = close_tag.end;
                }
            }
            SyntaxEvent::AnalysedCloseScopes(ref e)
                if e.event.tag_type == SyntaxTagType::RootTemplate =>
            {
                if let Some(ref mut region) = self.template_region {
                    region.1 = e.event.end;
                }
            }

            // Observe style regions — record positions for removal
            SyntaxEvent::CssStyleContent(ref css) => {
                self.style_regions
                    .push((css.tag_open_start, css.tag_close_end));
            }

            _ => {}
        }

        SyntaxResult::Keep(event)
    }
}
