//! Template codegen plugin - processes `<template>` blocks independently.
//!
//! Owns a `CodeTransform` wrapping the full SFC source.
//! Handles template events, observes other events to capture region positions.
//! In `end()`, removes non-template regions and produces a standalone render function.

use crate::{
    code_transform::{CodeTransform, SourceMapOptions},
    common::Span,
    syntax::{
        plugin::{SyntaxPlugin, SyntaxPluginContext, SyntaxResult},
        types::{SyntaxEvent, SyntaxTagType},
    },
};

use super::template;
use super::template::types::{BindingMetadata, CurrentElement, TemplateCodegenState};

pub struct TemplateCodegenPlugin<'a> {
    code_transform: CodeTransform<'a>,
    state: TemplateCodegenState,
    /// Start of the `<template>` opening tag (for region removal).
    template_tag_start: Option<u32>,
    /// End of the `</template>` closing tag (for region removal).
    template_tag_end: Option<u32>,
    /// Whether a template was found.
    has_template: bool,
    /// Source length for computing removal regions.
    source_len: u32,
    /// Observed script region (tag_open_start .. tag_close_end).
    script_region: Option<(u32, u32)>,
    /// Observed style regions (tag_open_start .. tag_close_end).
    style_regions: Vec<(u32, u32)>,
}

impl<'a> TemplateCodegenPlugin<'a> {
    pub fn new(source: &'a str, alloc: &'a oxc_allocator::Allocator) -> Self {
        Self {
            code_transform: CodeTransform::new(source, alloc),
            state: TemplateCodegenState::new(),
            template_tag_start: None,
            template_tag_end: None,
            has_template: false,
            source_len: source.len() as u32,
            script_region: None,
            style_regions: Vec::new(),
        }
    }

    pub fn set_production(&mut self, is_production: bool) {
        self.state.is_production = is_production;
    }

    /// Set scope ID for scoped styles.
    pub fn set_scope_id(&mut self, scope_id: [u8; 8]) {
        self.state.scope_id = Some(scope_id);
    }

    /// Set binding metadata from pre-scan (for template-before-script ordering).
    pub fn set_binding_metadata(&mut self, metadata: BindingMetadata) {
        self.state.binding_metadata = metadata;
    }

    /// Get the transformed code (render function only).
    pub fn get_code(&self) -> String {
        self.code_transform.to_string()
    }

    /// Generate source map JSON string.
    pub fn generate_source_map(&self, options: SourceMapOptions) -> String {
        self.code_transform.generate_map_json(options)
    }

    /// Whether a template was found and processed.
    pub fn has_template(&self) -> bool {
        self.has_template
    }
}

impl<'a> SyntaxPlugin<'a> for TemplateCodegenPlugin<'a> {
    fn name(&self) -> &str {
        "TemplateCodegen"
    }

    fn end(&mut self, _ctx: &SyntaxPluginContext<'a>) {
        if !self.has_template {
            return;
        }

        // Remove script region
        if let Some((start, end)) = self.script_region {
            self.code_transform.remove(start, end);
        }

        // Remove style regions
        for (start, end) in &self.style_regions {
            self.code_transform.remove(*start, *end);
        }

        // Remove everything before the template tag
        if let Some(start) = self.template_tag_start {
            if start > 0 {
                self.code_transform.remove(0, start);
            }
        }

        // Remove everything after the template tag
        if let Some(end) = self.template_tag_end {
            if end < self.source_len {
                self.code_transform.remove(end, self.source_len);
            }
        }
    }

    fn process_event(
        &mut self,
        event: SyntaxEvent<'a>,
        ctx: &mut SyntaxPluginContext<'a>,
    ) -> SyntaxResult<SyntaxEvent<'a>> {
        match &event {
            // ==================== Template Events ====================
            SyntaxEvent::OpenTagStart(ref open_tag) => {
                if open_tag.tag_type != SyntaxTagType::RootTemplate && self.state.render_started {
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

            SyntaxEvent::OpenTagEnd(ref open_tag) => {
                if open_tag.tag_type == SyntaxTagType::RootTemplate {
                    self.has_template = true;
                    self.template_tag_start = Some(open_tag.start);
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

            SyntaxEvent::CloseTag(ref close_tag) => {
                if close_tag.tag_type == SyntaxTagType::RootTemplate {
                    template::element::finalize_template_close(
                        close_tag,
                        &mut self.state,
                        &mut self.code_transform,
                        ctx.input,
                    );
                    // Use standalone finalization (wraps in-place, no move_wrapped)
                    template::element::finalize_template_standalone(
                        &mut self.state,
                        &mut self.code_transform,
                        ctx.input,
                    );
                    self.template_tag_end = Some(close_tag.end);
                } else if self.state.render_started {
                    template::element::process_close_tag(
                        close_tag,
                        &mut self.state,
                        &mut self.code_transform,
                        ctx.input,
                    );
                }
            }

            SyntaxEvent::AnalysedCloseScopes(ref e) => {
                if e.event.tag_type == SyntaxTagType::RootTemplate {
                    template::element::finalize_template_close(
                        &e.event,
                        &mut self.state,
                        &mut self.code_transform,
                        ctx.input,
                    );
                    template::element::finalize_template_standalone(
                        &mut self.state,
                        &mut self.code_transform,
                        ctx.input,
                    );
                    self.template_tag_end = Some(e.event.end);
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

            // ==================== Text / Comment Events ====================
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

            // ==================== Observed events (for region tracking) ====================

            // Observe script region
            SyntaxEvent::AnalysedScript(ref info) => {
                self.script_region = Some((info.event.tag_open_start, info.event.tag_close_end));
            }

            // Observe style regions
            SyntaxEvent::CssStyleContent(ref css) => {
                self.style_regions
                    .push((css.tag_open_start, css.tag_close_end));
                // Extract v-bind expressions for root element style injection
                if !css.v_bind_expressions.is_empty() {
                    self.state.css_v_bind_expressions = css.v_bind_expressions.clone();
                }
            }

            _ => {}
        }

        SyntaxResult::Keep(event)
    }
}
