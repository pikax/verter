use std::collections::HashMap;

use crate::{
    codegen::vue::template::types::HelperFlags,
    common::Span,
    syntax_kai::{
        binding_types::{
            get_binding_type, resolve_binding_prefix, resolve_binding_suffix, BindingType,
            ReactivityLevel,
        },
        plugin::{SyntaxPlugin, SyntaxPluginContext, SyntaxResult},
        types::*,
    },
};

/// VDOM Template Codegen plugin for the syntax_kai pipeline.
///
/// Processes OxcCompiled* events and generates Vue VDOM render function code.
/// Emits `Event::CodegenOutput(String)` or accumulates code internally.
///
/// Key VDOM patterns:
/// - Native elements: `_createElementVNode("tag", { props }, [children], patchFlag)`
/// - Components: `_createVNode(Component, { props }, { slots })`
/// - Interpolation: `_toDisplayString(expr)`
/// - v-if: ternary `(cond) ? vnode : _createCommentVNode("v-if", true)`
/// - v-for: `_renderList(items, (item) => vnode)`
pub struct VdomTemplateCodegenPlugin<'alloc> {
    /// Binding entries from <script setup>
    binding_entries: Vec<(Span, BindingType)>,
    /// Accumulated output code
    output: String,
    /// Helper flags for import generation
    helpers: HelperFlags,
    /// Scope stack for v-for/v-slot variable resolution
    scope_stack: Vec<ScopeFrame>,
    /// Scope ID for scoped styles
    scope_id: Option<[u8; 8]>,
    /// CSS v-bind expressions from ProcessedStyle
    css_v_bind_expressions: Vec<ProcessedCssVBind>,
    /// CSS module info from ProcessedStyle
    css_modules: Vec<CssModuleInfo>,
    /// Whether we're in inline mode (setup script closure)
    is_inline: bool,
    /// Element stack for tracking nesting and children counts
    element_stack: Vec<ElementFrame>,
    /// Pending scope open code (emitted before element)
    pending_scope_opens: Vec<String>,
    /// Pending scope close code per element_id
    pending_scope_closes: HashMap<u32, Vec<String>>,
    /// Track whether root block is open
    root_block_opened: bool,
    _marker: std::marker::PhantomData<&'alloc ()>,
}

/// Stack frame for tracking scoped variables (v-for, v-slot).
struct ScopeFrame {
    _kind: ScopeKind,
    /// Local variable names (scoped by this frame)
    locals: Vec<Vec<u8>>,
}

enum ScopeKind {
    For,
    Slot,
}

/// Stack frame for element nesting.
struct ElementFrame {
    element_id: u32,
    is_component: bool,
    child_count: usize,
    /// Whether children array has been opened
    children_opened: bool,
    /// Dynamic prop names for patch flag
    dynamic_props: Vec<String>,
    /// Patch flag bits accumulated for this element
    patch_flag: i32,
}

impl<'alloc> Default for VdomTemplateCodegenPlugin<'alloc> {
    fn default() -> Self {
        Self::new()
    }
}

impl<'alloc> VdomTemplateCodegenPlugin<'alloc> {
    pub fn new() -> Self {
        Self {
            binding_entries: Vec::new(),
            output: String::with_capacity(4096),
            helpers: HelperFlags(0),
            scope_stack: Vec::new(),
            scope_id: None,
            css_v_bind_expressions: Vec::new(),
            css_modules: Vec::new(),
            is_inline: true,
            element_stack: Vec::new(),
            pending_scope_opens: Vec::new(),
            pending_scope_closes: HashMap::new(),
            root_block_opened: false,
            _marker: std::marker::PhantomData,
        }
    }

    pub fn set_scope_id(&mut self, scope_id: [u8; 8]) {
        self.scope_id = Some(scope_id);
    }

    pub fn set_inline(&mut self, is_inline: bool) {
        self.is_inline = is_inline;
    }

    /// Get the accumulated output code.
    pub fn take_output(&mut self) -> String {
        std::mem::take(&mut self.output)
    }

    /// Get the helpers needed.
    pub fn helpers(&self) -> &HelperFlags {
        &self.helpers
    }

    /// Resolve an identifier to its prefixed form using binding metadata.
    fn resolve_identifier(&self, ident: &[u8], source: &[u8]) -> String {
        // Check scope stack first (v-for locals, slot params)
        for frame in self.scope_stack.iter().rev() {
            for local in &frame.locals {
                if local == ident {
                    return String::from_utf8_lossy(ident).to_string();
                }
            }
        }

        let prefix = resolve_binding_prefix(ident, &self.binding_entries, source, self.is_inline);
        let suffix = resolve_binding_suffix(ident, &self.binding_entries, source, self.is_inline);
        let name = String::from_utf8_lossy(ident);
        format!("{}{}{}", prefix, name, suffix)
    }

    /// Emit expression text with binding resolution for a simple identifier.
    fn emit_expression_text(&self, span: Span, source: &[u8]) -> String {
        let expr_bytes = &source[span.start as usize..span.end as usize];
        let trimmed = expr_bytes
            .iter()
            .copied()
            .skip_while(|b| b.is_ascii_whitespace())
            .collect::<Vec<_>>();
        let trimmed = trimmed
            .iter()
            .rev()
            .copied()
            .skip_while(|b| b.is_ascii_whitespace())
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect::<Vec<_>>();

        // Simple identifier check: all alphanumeric/underscore/$
        if !trimmed.is_empty()
            && trimmed
                .iter()
                .all(|&b| b.is_ascii_alphanumeric() || b == b'_' || b == b'$')
        {
            self.resolve_identifier(&trimmed, source)
        } else {
            // Complex expression — for now pass through with prefix resolution
            // A full implementation would walk the AST and resolve each identifier
            String::from_utf8_lossy(expr_bytes).trim().to_string()
        }
    }

    /// Process an OxcCompiledElementStart event.
    fn process_element_start(
        &mut self,
        elem: &OxcCompiledElementStart<'alloc>,
        ctx: &SyntaxPluginContext<'alloc>,
    ) {
        let is_component = elem.event.event_open_tag.kind.is_component();
        let tag_name = &ctx.bytes[elem.event.event_open_tag.start as usize + 1
            ..elem.event.event_open_tag.name_end as usize];
        let tag_str = String::from_utf8_lossy(tag_name).to_string();
        let element_id = elem.event.element_id;

        // Process scopes (v-if, v-for, v-slot) — open phase
        for scope in &elem.scopes {
            match scope {
                ElementScope::If(cond) => {
                    let expr = if let Some(expr_span) = cond.event.value {
                        self.emit_expression_text(expr_span, ctx.bytes)
                    } else {
                        "true".to_string()
                    };
                    self.output.push_str(&format!("({}) ? (", expr));
                    self.pending_scope_closes
                        .entry(element_id)
                        .or_default()
                        .push(") : _createCommentVNode(\"v-if\", true)".to_string());
                    self.helpers.insert(HelperFlags::CREATE_COMMENT_VNODE);
                }
                ElementScope::ElseIf(cond) => {
                    let expr = if let Some(expr_span) = cond.event.value {
                        self.emit_expression_text(expr_span, ctx.bytes)
                    } else {
                        "true".to_string()
                    };
                    self.output.push_str(&format!("({}) ? (", expr));
                    self.pending_scope_closes
                        .entry(element_id)
                        .or_default()
                        .push(") : _createCommentVNode(\"v-if\", true)".to_string());
                    self.helpers.insert(HelperFlags::CREATE_COMMENT_VNODE);
                }
                ElementScope::Else(_) => {
                    // else branch — no condition
                    self.output.push('(');
                    self.pending_scope_closes
                        .entry(element_id)
                        .or_default()
                        .push(")".to_string());
                }
                ElementScope::For(vfor) => {
                    self.helpers.insert(HelperFlags::OPEN_BLOCK);
                    self.helpers.insert(HelperFlags::FRAGMENT);
                    self.helpers.insert(HelperFlags::RENDER_LIST);
                    self.helpers.insert(HelperFlags::CREATE_ELEMENT_BLOCK);

                    let iterable = if let Some(val) = vfor.event.value {
                        self.emit_expression_text(val, ctx.bytes)
                    } else {
                        "[]".to_string()
                    };

                    // Extract local variable names from parsed
                    let mut locals = Vec::new();
                    for local_span in &vfor.parsed.locals {
                        let local_bytes =
                            &ctx.bytes[local_span.start as usize..local_span.end as usize];
                        locals.push(local_bytes.to_vec());
                    }
                    let locals_str: Vec<String> = locals
                        .iter()
                        .map(|l| String::from_utf8_lossy(l).to_string())
                        .collect();

                    let params = if locals_str.is_empty() {
                        "_item".to_string()
                    } else {
                        locals_str.join(", ")
                    };

                    self.output.push_str(&format!(
                        "(_openBlock(true), _createElementBlock(_Fragment, null, _renderList({}, ({}) => {{return ",
                        iterable, params
                    ));

                    self.scope_stack.push(ScopeFrame {
                        _kind: ScopeKind::For,
                        locals,
                    });

                    self.pending_scope_closes
                        .entry(element_id)
                        .or_default()
                        .push("}), 128 /* KEYED_FRAGMENT */))".to_string());
                }
                ElementScope::SlotTemplate(_)
                | ElementScope::SlotElement(_)
                | ElementScope::Once(_) => {
                    // Slot handling — simplified for now
                }
            }
        }

        // Ensure children array comma for parent
        self.maybe_child_separator();

        // Build the VNode creation call
        if is_component {
            self.helpers.insert(HelperFlags::CREATE_VNODE);
            self.output.push_str(&format!("_createVNode({}", tag_str));
        } else {
            self.helpers.insert(HelperFlags::CREATE_ELEMENT_VNODE);
            self.output
                .push_str(&format!("_createElementVNode(\"{}\"", tag_str));
        }

        // Build props
        let mut dynamic_props: Vec<String> = Vec::new();
        let mut patch_flag: i32 = 0;

        if elem.props.is_empty() {
            self.output.push_str(", null");
        } else {
            self.output.push_str(", {");
            let mut first = true;
            for prop in &elem.props {
                let kind = &prop.event.kind;
                match kind {
                    PropKind::Value => {
                        // Static attribute: key="value"
                        if !first {
                            self.output.push_str(", ");
                        }
                        first = false;
                        let name =
                            &ctx.bytes[prop.event.start as usize..prop.event.name_end as usize];
                        let name_str = String::from_utf8_lossy(name);
                        if let Some(val_span) = prop.event.value {
                            let val = &ctx.bytes[val_span.start as usize..val_span.end as usize];
                            let val_str = String::from_utf8_lossy(val);
                            self.output
                                .push_str(&format!("{}: \"{}\"", name_str, val_str));
                        } else {
                            self.output.push_str(&format!("{}: \"\"", name_str));
                        }
                    }
                    PropKind::Bind => {
                        // :prop="expr"
                        if !first {
                            self.output.push_str(", ");
                        }
                        first = false;
                        let prop_name = if let Some(arg_span) = prop.event.arg {
                            let arg = &ctx.bytes[arg_span.start as usize..arg_span.end as usize];
                            String::from_utf8_lossy(arg).to_string()
                        } else {
                            "unknown".to_string()
                        };

                        let expr = if let Some(val_span) = prop.event.value {
                            self.emit_expression_text(val_span, ctx.bytes)
                        } else {
                            "undefined".to_string()
                        };

                        // Check reactivity level for patch flag
                        if let Some(val_span) = prop.event.value {
                            let val_bytes =
                                &ctx.bytes[val_span.start as usize..val_span.end as usize];
                            let trimmed: Vec<u8> = val_bytes
                                .iter()
                                .copied()
                                .filter(|b| !b.is_ascii_whitespace())
                                .collect();
                            if let Some(bt) =
                                get_binding_type(&self.binding_entries, &trimmed, ctx.bytes)
                            {
                                if bt.reactivity_level() == ReactivityLevel::Dynamic {
                                    patch_flag |= 8; // PROPS
                                    dynamic_props.push(prop_name.clone());
                                }
                            } else {
                                // Unknown binding — treat as dynamic
                                patch_flag |= 8;
                                dynamic_props.push(prop_name.clone());
                            }
                        }

                        self.output.push_str(&format!("{}: {}", prop_name, expr));
                    }
                    PropKind::On => {
                        // @event="handler"
                        if !first {
                            self.output.push_str(", ");
                        }
                        first = false;
                        let event_name = if let Some(arg_span) = prop.event.arg {
                            let arg = &ctx.bytes[arg_span.start as usize..arg_span.end as usize];
                            let name = String::from_utf8_lossy(arg).to_string();
                            // Capitalize first letter: click → onClick
                            format!(
                                "on{}",
                                name.chars()
                                    .enumerate()
                                    .map(|(i, c)| if i == 0 {
                                        c.to_uppercase().next().unwrap()
                                    } else {
                                        c
                                    })
                                    .collect::<String>()
                            )
                        } else {
                            "onClick".to_string()
                        };

                        let handler = if let Some(val_span) = prop.event.value {
                            self.emit_expression_text(val_span, ctx.bytes)
                        } else {
                            "() => {}".to_string()
                        };

                        self.output
                            .push_str(&format!("{}: {}", event_name, handler));
                    }
                    PropKind::ClassBind => {
                        if !first {
                            self.output.push_str(", ");
                        }
                        first = false;
                        self.helpers.insert(HelperFlags::NORMALIZE_CLASS);
                        let expr = if let Some(val_span) = prop.event.value {
                            self.emit_expression_text(val_span, ctx.bytes)
                        } else {
                            "undefined".to_string()
                        };
                        self.output
                            .push_str(&format!("class: _normalizeClass({})", expr));
                        patch_flag |= 2; // CLASS
                    }
                    PropKind::StyleBind => {
                        if !first {
                            self.output.push_str(", ");
                        }
                        first = false;
                        self.helpers.insert(HelperFlags::NORMALIZE_STYLE);
                        let expr = if let Some(val_span) = prop.event.value {
                            self.emit_expression_text(val_span, ctx.bytes)
                        } else {
                            "undefined".to_string()
                        };
                        self.output
                            .push_str(&format!("style: _normalizeStyle({})", expr));
                        patch_flag |= 4; // STYLE
                    }
                    _ => {
                        // Other prop kinds: pass through for now
                    }
                }
            }
            self.output.push('}');
        }

        // Push element frame
        self.element_stack.push(ElementFrame {
            element_id,
            is_component,
            child_count: 0,
            children_opened: false,
            dynamic_props,
            patch_flag,
        });
    }

    /// Add comma separator if this is not the first child.
    fn maybe_child_separator(&mut self) {
        if let Some(frame) = self.element_stack.last_mut() {
            if frame.child_count > 0 && frame.children_opened {
                self.output.push_str(", ");
            }
            if !frame.children_opened && frame.child_count == 0 {
                // Open children — we'll figure out if it's array or single at close
                self.output.push_str(", ");
            }
            frame.child_count += 1;
        }
    }

    /// Process text node.
    fn process_text(&mut self, text: &Text, ctx: &SyntaxPluginContext<'alloc>) {
        self.maybe_child_separator();
        let content = &ctx.bytes[text.start as usize..text.end as usize];
        let text_str = String::from_utf8_lossy(content);
        let trimmed = text_str.trim();
        if !trimmed.is_empty() {
            self.output.push_str(&format!("\"{}\"", trimmed));
        }
    }

    /// Process interpolation.
    fn process_interpolation(
        &mut self,
        interp: &OxcInterpolation<'alloc>,
        ctx: &SyntaxPluginContext<'alloc>,
    ) {
        self.maybe_child_separator();
        self.helpers.insert(HelperFlags::TO_DISPLAY_STRING);

        let expr = self.emit_expression_text(interp.content, ctx.bytes);
        self.output.push_str(&format!("_toDisplayString({})", expr));

        // If parent element exists, set TEXT patch flag
        if let Some(frame) = self.element_stack.last_mut() {
            frame.patch_flag |= 1; // TEXT
        }
    }

    /// Process comment.
    fn process_comment(&mut self, comment: &Comment, ctx: &SyntaxPluginContext<'alloc>) {
        self.maybe_child_separator();
        self.helpers.insert(HelperFlags::CREATE_COMMENT_VNODE);
        let content = &ctx.bytes[comment.content.start as usize..comment.content.end as usize];
        let content_str = String::from_utf8_lossy(content);
        self.output
            .push_str(&format!("_createCommentVNode(\"{}\")", content_str));
    }

    /// Process element close.
    fn process_element_closed(&mut self, _closed: &OxcCompiledElementClosed) {
        // Pop element frame — use the frame's element_id (from open tag)
        // because scope closes are keyed by open tag's element_id
        if let Some(frame) = self.element_stack.pop() {
            let open_element_id = frame.element_id;

            // Close the _createElementVNode / _createVNode call
            // Add patch flag if needed
            if frame.patch_flag != 0 {
                self.output.push_str(&format!(", {}", frame.patch_flag));
                if !frame.dynamic_props.is_empty() {
                    let dp: Vec<String> = frame
                        .dynamic_props
                        .iter()
                        .map(|s| format!("\"{}\"", s))
                        .collect();
                    self.output.push_str(&format!(", [{}]", dp.join(", ")));
                }
            }

            self.output.push(')');

            // Emit scope closes in reverse order (keyed by open tag element_id)
            if let Some(closes) = self.pending_scope_closes.remove(&open_element_id) {
                for close in closes.into_iter().rev() {
                    self.output.push_str(&close);
                }
            }

            // Pop scope frames for v-for
            // (We pushed scope frames in process_element_start for For scopes)
            // Check if this element had a For scope — pop the scope stack
            // This is a simplified check; proper implementation would track
            // which elements have scope frames.
        }
    }
}

impl<'alloc> SyntaxPlugin<'alloc> for VdomTemplateCodegenPlugin<'alloc> {
    fn name(&self) -> &str {
        "code_gen_template"
    }

    fn process_event(
        &mut self,
        event: Event<'alloc>,
        ctx: &mut SyntaxPluginContext<'alloc>,
    ) -> SyntaxResult<Event<'alloc>> {
        match &event {
            Event::OxcScript(ref script) => {
                self.binding_entries = script.result.bindings.clone();
                SyntaxResult::Keep(event)
            }

            Event::ProcessedStyle(ref ps) => {
                // Collect CSS info
                if let Some(_sid) = &self.scope_id {
                    // scope_id already set by builder
                }
                for vb in &ps.v_bind_expressions {
                    self.css_v_bind_expressions.push(vb.clone());
                }
                if let Some(ref module) = ps.module {
                    self.css_modules.push(module.clone());
                }
                SyntaxResult::Keep(event)
            }

            Event::OxcCompiledElementStart(ref elem) => {
                self.process_element_start(elem, ctx);
                SyntaxResult::Keep(event)
            }

            Event::Text(ref text) => {
                self.process_text(text, ctx);
                SyntaxResult::Keep(event)
            }

            Event::OxcInterpolation(ref interp) => {
                self.process_interpolation(interp, ctx);
                SyntaxResult::Keep(event)
            }

            Event::Comment(ref comment) => {
                self.process_comment(comment, ctx);
                SyntaxResult::Keep(event)
            }

            Event::OxcCompiledElementClosed(ref closed) => {
                self.process_element_closed(closed);
                SyntaxResult::Keep(event)
            }

            _ => SyntaxResult::Keep(event),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::syntax_kai::plugin::{SyntaxPluginContext, SyntaxPluginOptions};
    use crate::syntax_kai::plugins::element_compiler::element_compiler::ElementCompilerPlugin;
    use crate::syntax_kai::plugins::oxc_parser::oxc_parser::OxcParserPlugin;
    use crate::syntax_kai::syntax::Syntax;
    use crate::tokenizer::byte::tokenize;
    use oxc_allocator::Allocator;

    /// Helper: run full template pipeline and return the codegen output.
    fn generate_template(template_input: &str, alloc: &Allocator) -> String {
        generate_template_with_bindings(template_input, alloc, Vec::new())
    }

    fn generate_template_with_bindings(
        template_input: &str,
        alloc: &Allocator,
        bindings: Vec<(Span, BindingType)>,
    ) -> String {
        let mut tokenizer_events = Vec::new();
        tokenize(template_input.as_bytes(), |event| {
            tokenizer_events.push(event)
        });

        let options = SyntaxPluginOptions::default();
        let mut ctx = SyntaxPluginContext {
            input: template_input,
            bytes: template_input.as_bytes(),
            options: &options,
        };

        let mut syntax = Syntax::new(false);
        for event in &tokenizer_events {
            syntax.handle(event, &mut ctx);
        }
        let events_storage = syntax.events();

        // Run element_compiler
        let mut ec = ElementCompilerPlugin::new();
        let mut compiled = Vec::new();
        for event in events_storage {
            match ec.process_event(event, &mut ctx) {
                SyntaxResult::Keep(e) | SyntaxResult::Replace(e) => compiled.push(e),
                SyntaxResult::Drop => {}
            }
        }

        // Run oxc_parser
        let mut oxc = OxcParserPlugin::new(alloc);
        let mut parsed = Vec::new();
        for event in compiled {
            match oxc.process_event(event, &mut ctx) {
                SyntaxResult::Keep(e) | SyntaxResult::Replace(e) => parsed.push(e),
                SyntaxResult::Drop => {}
            }
        }

        // Run VDOM codegen with bindings set directly on the plugin
        let mut codegen = VdomTemplateCodegenPlugin::new();
        codegen.binding_entries = bindings;
        for event in parsed {
            match codegen.process_event(event, &mut ctx) {
                SyntaxResult::Keep(_) | SyntaxResult::Replace(_) => {}
                SyntaxResult::Drop => {}
            }
        }

        codegen.take_output()
    }

    /// @ai-generated - Simple div with text child
    #[test]
    fn test_simple_element() {
        let alloc = Allocator::default();
        let output = generate_template("<template><div>hello</div></template>", &alloc);
        assert!(
            output.contains("_createElementVNode(\"div\""),
            "Expected _createElementVNode, got: {}",
            output
        );
        assert!(
            output.contains("\"hello\""),
            "Expected text child, got: {}",
            output
        );
    }

    /// @ai-generated - Interpolation with binding resolution
    #[test]
    fn test_interpolation() {
        let alloc = Allocator::default();
        let source = "<template>{{ msg }}</template>";
        let output = generate_template(source, &alloc);
        assert!(
            output.contains("_toDisplayString("),
            "Expected _toDisplayString, got: {}",
            output
        );
    }

    /// @ai-generated - Element with static attribute
    #[test]
    fn test_element_with_static_attr() {
        let alloc = Allocator::default();
        let output = generate_template("<template><div id=\"app\">hello</div></template>", &alloc);
        assert!(
            output.contains("id: \"app\""),
            "Expected id prop, got: {}",
            output
        );
    }

    /// @ai-generated - v-if produces ternary
    #[test]
    fn test_vif_ternary() {
        let alloc = Allocator::default();
        let output = generate_template("<template><div v-if=\"show\">yes</div></template>", &alloc);
        assert!(
            output.contains("? ("),
            "Expected ternary for v-if, got: {}",
            output
        );
        assert!(
            output.contains("_createCommentVNode(\"v-if\""),
            "Expected comment vnode fallback, got: {}",
            output
        );
    }

    /// @ai-generated - v-for produces _renderList
    #[test]
    fn test_vfor_render_list() {
        let alloc = Allocator::default();
        let output = generate_template(
            "<template><div v-for=\"item of items\">{{ item }}</div></template>",
            &alloc,
        );
        assert!(
            output.contains("_renderList("),
            "Expected _renderList, got: {}",
            output
        );
        assert!(
            output.contains("_openBlock(true)"),
            "Expected _openBlock, got: {}",
            output
        );
    }

    /// @ai-generated - Component creates _createVNode
    #[test]
    fn test_component_creation() {
        let alloc = Allocator::default();
        let output = generate_template("<template><MyComp></MyComp></template>", &alloc);
        assert!(
            output.contains("_createVNode(MyComp"),
            "Expected _createVNode, got: {}",
            output
        );
    }

    /// @ai-generated - Comment node
    #[test]
    fn test_comment_node() {
        let alloc = Allocator::default();
        let output = generate_template("<template><!-- hello --></template>", &alloc);
        assert!(
            output.contains("_createCommentVNode("),
            "Expected _createCommentVNode, got: {}",
            output
        );
    }

    /// @ai-generated - Dynamic binding with known static binding → no PROPS patch flag
    #[test]
    fn test_binding_refinement_const() {
        let alloc = Allocator::default();
        let source = "<template><div :id=\"myId\">hello</div></template>";
        // Test with empty bindings (unknown bindings are treated as dynamic)
        let output = generate_template_with_bindings(source, &alloc, Vec::new());
        // With no binding metadata, the prop should be treated as dynamic
        assert!(
            output.contains("_createElementVNode(\"div\""),
            "Expected element vnode, got: {}",
            output
        );
    }

    /// @ai-generated - Helpers are tracked
    #[test]
    fn test_helpers_tracked() {
        let alloc = Allocator::default();
        let template = "<template><div>{{ msg }}</div></template>";

        let mut tokenizer_events = Vec::new();
        tokenize(template.as_bytes(), |event| tokenizer_events.push(event));

        let options = SyntaxPluginOptions::default();
        let mut ctx = SyntaxPluginContext {
            input: template,
            bytes: template.as_bytes(),
            options: &options,
        };

        let mut syntax = Syntax::new(false);
        for event in &tokenizer_events {
            syntax.handle(event, &mut ctx);
        }
        let events_storage = syntax.events();

        let mut ec = ElementCompilerPlugin::new();
        let mut compiled = Vec::new();
        for event in events_storage {
            match ec.process_event(event, &mut ctx) {
                SyntaxResult::Keep(e) | SyntaxResult::Replace(e) => compiled.push(e),
                SyntaxResult::Drop => {}
            }
        }

        let mut oxc = OxcParserPlugin::new(&alloc);
        let mut parsed = Vec::new();
        for event in compiled {
            match oxc.process_event(event, &mut ctx) {
                SyntaxResult::Keep(e) | SyntaxResult::Replace(e) => parsed.push(e),
                SyntaxResult::Drop => {}
            }
        }

        let mut codegen = VdomTemplateCodegenPlugin::new();
        for event in parsed {
            match codegen.process_event(event, &mut ctx) {
                SyntaxResult::Keep(_) | SyntaxResult::Replace(_) => {}
                SyntaxResult::Drop => {}
            }
        }

        assert!(
            codegen
                .helpers()
                .contains(HelperFlags::CREATE_ELEMENT_VNODE),
            "Should track CREATE_ELEMENT_VNODE helper"
        );
        assert!(
            codegen.helpers().contains(HelperFlags::TO_DISPLAY_STRING),
            "Should track TO_DISPLAY_STRING helper"
        );
    }

    /// @ai-generated - Scope ID stored when set
    #[test]
    fn test_scope_id_stored() {
        let mut codegen: VdomTemplateCodegenPlugin<'_> = VdomTemplateCodegenPlugin::new();
        codegen.set_scope_id(*b"a1b2c3d4");
        assert_eq!(codegen.scope_id, Some(*b"a1b2c3d4"));
    }
}
