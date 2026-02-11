use crate::{
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

/// Vapor Template Codegen plugin for the syntax_kai pipeline.
///
/// Generates Vue Vapor mode render function code. Key differences from VDOM:
/// - No `_createVNode`/`_createBlock`/`_openBlock`, no patch flags
/// - Uses `_template()` for static HTML cloning, `_createElement()` for dynamic
/// - Dynamic bindings → `_renderEffect(() => { ... })`
/// - Events → `_on(n0, "click", handler)`
/// - Structural directives → `_createIf()`, `_createFor()`
///
/// Binding refinement via reactivity_level:
/// - Static → one-time `_setProp()`, no `_renderEffect` wrapper
/// - Dynamic → wrapped in `_renderEffect()`
pub struct VaporTemplateCodegenPlugin<'alloc> {
    /// Binding entries from <script setup>
    binding_entries: Vec<(Span, BindingType)>,
    /// Accumulated output code
    output: String,
    /// Node counter for variable naming
    node_counter: u32,
    /// Scope stack for v-for/v-slot variable resolution
    scope_stack: Vec<ScopeFrame>,
    /// Scope ID for scoped styles
    scope_id: Option<[u8; 8]>,
    /// CSS v-bind expressions
    css_v_bind_expressions: Vec<ProcessedCssVBind>,
    /// CSS module info
    css_modules: Vec<CssModuleInfo>,
    /// Whether we're in inline mode
    is_inline: bool,
    /// Element stack for nesting
    element_stack: Vec<VaporElementFrame>,
    /// Deferred effects (collected per element, emitted after template)
    deferred_effects: Vec<String>,
    /// Deferred event handlers
    deferred_events: Vec<String>,
    _marker: std::marker::PhantomData<&'alloc ()>,
}

struct ScopeFrame {
    _kind: ScopeKind,
    locals: Vec<Vec<u8>>,
}

enum ScopeKind {
    For,
    Slot,
}

struct VaporElementFrame {
    element_id: u32,
    node_var: String,
    is_component: bool,
    tag_name: String,
    /// Static props for _template() string
    static_props: Vec<(String, String)>,
    /// Dynamic props that need _renderEffect
    dynamic_props: Vec<(String, String)>,
    /// Static props that need one-time _setProp
    static_set_props: Vec<(String, String)>,
    /// Event handlers
    event_handlers: Vec<(String, String)>,
    /// Children text (for static template)
    child_text: Option<String>,
    /// Scope open code
    scope_opens: Vec<String>,
    /// Scope close code
    scope_closes: Vec<String>,
    /// Has interpolation children (need _renderEffect for text)
    has_dynamic_text: bool,
    dynamic_text_expr: Option<String>,
}

impl<'alloc> Default for VaporTemplateCodegenPlugin<'alloc> {
    fn default() -> Self {
        Self::new()
    }
}

impl<'alloc> VaporTemplateCodegenPlugin<'alloc> {
    pub fn new() -> Self {
        Self {
            binding_entries: Vec::new(),
            output: String::with_capacity(4096),
            node_counter: 0,
            scope_stack: Vec::new(),
            scope_id: None,
            css_v_bind_expressions: Vec::new(),
            css_modules: Vec::new(),
            is_inline: true,
            element_stack: Vec::new(),
            deferred_effects: Vec::new(),
            deferred_events: Vec::new(),
            _marker: std::marker::PhantomData,
        }
    }

    pub fn set_scope_id(&mut self, scope_id: [u8; 8]) {
        self.scope_id = Some(scope_id);
    }

    pub fn set_inline(&mut self, is_inline: bool) {
        self.is_inline = is_inline;
    }

    pub fn take_output(&mut self) -> String {
        std::mem::take(&mut self.output)
    }

    fn next_node_var(&mut self) -> String {
        let var = format!("n{}", self.node_counter);
        self.node_counter += 1;
        var
    }

    fn resolve_identifier(&self, ident: &[u8], source: &[u8]) -> String {
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

    fn emit_expression_text(&self, span: Span, source: &[u8]) -> String {
        let expr_bytes = &source[span.start as usize..span.end as usize];
        let trimmed: Vec<u8> = expr_bytes
            .iter()
            .copied()
            .skip_while(|b| b.is_ascii_whitespace())
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .skip_while(|b| b.is_ascii_whitespace())
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();

        if !trimmed.is_empty()
            && trimmed
                .iter()
                .all(|&b| b.is_ascii_alphanumeric() || b == b'_' || b == b'$')
        {
            self.resolve_identifier(&trimmed, source)
        } else {
            String::from_utf8_lossy(expr_bytes).trim().to_string()
        }
    }

    fn process_element_start(
        &mut self,
        elem: &OxcCompiledElementStart<'alloc>,
        ctx: &SyntaxPluginContext<'alloc>,
    ) {
        let is_component = elem.event.event_open_tag.kind.is_component();
        let tag_bytes = &ctx.bytes[elem.event.event_open_tag.start as usize + 1
            ..elem.event.event_open_tag.name_end as usize];
        let tag_name = String::from_utf8_lossy(tag_bytes).to_string();
        let node_var = self.next_node_var();
        let element_id = elem.event.element_id;

        let mut static_props = Vec::new();
        let mut dynamic_props = Vec::new();
        let mut static_set_props = Vec::new();
        let mut event_handlers = Vec::new();
        let mut scope_opens = Vec::new();
        let mut scope_closes = Vec::new();

        // Process scopes
        for scope in &elem.scopes {
            match scope {
                ElementScope::If(cond) => {
                    let expr = if let Some(val) = cond.event.value {
                        self.emit_expression_text(val, ctx.bytes)
                    } else {
                        "true".to_string()
                    };
                    scope_opens.push(format!(
                        "const {} = _createIf(() => {}, () => {{",
                        node_var, expr
                    ));
                    scope_closes.push("})".to_string());
                }
                ElementScope::For(vfor) => {
                    let iterable = if let Some(val) = vfor.event.value {
                        self.emit_expression_text(val, ctx.bytes)
                    } else {
                        "[]".to_string()
                    };

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

                    scope_opens.push(format!(
                        "const {} = _createFor(() => {}, ({}) => {{",
                        node_var, iterable, params
                    ));
                    scope_closes.push("})".to_string());

                    self.scope_stack.push(ScopeFrame {
                        _kind: ScopeKind::For,
                        locals,
                    });
                }
                _ => {}
            }
        }

        // Process props
        for prop in &elem.props {
            match &prop.event.kind {
                PropKind::Value | PropKind::ClassValue | PropKind::StyleValue => {
                    let name = match &prop.event.kind {
                        PropKind::ClassValue => "class".to_string(),
                        PropKind::StyleValue => "style".to_string(),
                        _ => String::from_utf8_lossy(
                            &ctx.bytes[prop.event.start as usize..prop.event.name_end as usize],
                        )
                        .to_string(),
                    };
                    let val = if let Some(val_span) = prop.event.value {
                        String::from_utf8_lossy(
                            &ctx.bytes[val_span.start as usize..val_span.end as usize],
                        )
                        .to_string()
                    } else {
                        String::new()
                    };
                    static_props.push((name, val));
                }
                PropKind::Bind => {
                    let prop_name = if let Some(arg_span) = prop.event.arg {
                        String::from_utf8_lossy(
                            &ctx.bytes[arg_span.start as usize..arg_span.end as usize],
                        )
                        .to_string()
                    } else {
                        "unknown".to_string()
                    };
                    let expr = if let Some(val_span) = prop.event.value {
                        self.emit_expression_text(val_span, ctx.bytes)
                    } else {
                        "undefined".to_string()
                    };

                    // Check reactivity level
                    let is_dynamic = if let Some(val_span) = prop.event.value {
                        let val_bytes = &ctx.bytes[val_span.start as usize..val_span.end as usize];
                        let trimmed: Vec<u8> = val_bytes
                            .iter()
                            .copied()
                            .filter(|b| !b.is_ascii_whitespace())
                            .collect();
                        if let Some(bt) =
                            get_binding_type(&self.binding_entries, &trimmed, ctx.bytes)
                        {
                            bt.reactivity_level() == ReactivityLevel::Dynamic
                        } else {
                            true // Unknown → dynamic
                        }
                    } else {
                        false
                    };

                    if is_dynamic {
                        dynamic_props.push((prop_name, expr));
                    } else {
                        static_set_props.push((prop_name, expr));
                    }
                }
                PropKind::On => {
                    let event_name = if let Some(arg_span) = prop.event.arg {
                        String::from_utf8_lossy(
                            &ctx.bytes[arg_span.start as usize..arg_span.end as usize],
                        )
                        .to_string()
                    } else {
                        "click".to_string()
                    };
                    let handler = if let Some(val_span) = prop.event.value {
                        self.emit_expression_text(val_span, ctx.bytes)
                    } else {
                        "() => {}".to_string()
                    };
                    event_handlers.push((event_name, handler));
                }
                _ => {}
            }
        }

        // Add scope_id as data attribute for scoped styles
        if let Some(ref sid) = self.scope_id {
            let attr = format!("data-v-{}", std::str::from_utf8(sid).unwrap_or(""));
            static_props.push((attr, String::new()));
        }

        self.element_stack.push(VaporElementFrame {
            element_id,
            node_var,
            is_component,
            tag_name,
            static_props,
            dynamic_props,
            static_set_props,
            event_handlers,
            child_text: None,
            scope_opens,
            scope_closes,
            has_dynamic_text: false,
            dynamic_text_expr: None,
        });
    }

    fn process_text(&mut self, text: &Text, ctx: &SyntaxPluginContext<'alloc>) {
        let content = &ctx.bytes[text.start as usize..text.end as usize];
        let text_str = String::from_utf8_lossy(content).trim().to_string();
        if !text_str.is_empty() {
            if let Some(frame) = self.element_stack.last_mut() {
                frame.child_text = Some(text_str);
            }
        }
    }

    fn process_interpolation(
        &mut self,
        interp: &OxcInterpolation<'alloc>,
        ctx: &SyntaxPluginContext<'alloc>,
    ) {
        let expr = self.emit_expression_text(interp.content, ctx.bytes);
        if let Some(frame) = self.element_stack.last_mut() {
            frame.has_dynamic_text = true;
            frame.dynamic_text_expr = Some(expr);
        }
    }

    fn process_element_closed(&mut self) {
        if let Some(frame) = self.element_stack.pop() {
            // Emit scope opens
            for open in &frame.scope_opens {
                self.output.push_str(open);
                self.output.push('\n');
            }

            // Build template string
            let mut template = String::new();
            template.push('<');
            template.push_str(&frame.tag_name);
            for (name, val) in &frame.static_props {
                if val.is_empty() {
                    template.push_str(&format!(" {}", name));
                } else {
                    template.push_str(&format!(" {}=\"{}\"", name, val));
                }
            }
            template.push('>');
            if let Some(ref text) = frame.child_text {
                template.push_str(text);
            }
            template.push_str(&format!("</{}>", frame.tag_name));

            if frame.is_component {
                self.output.push_str(&format!(
                    "const {} = _createComponent({})",
                    frame.node_var, frame.tag_name
                ));
            } else {
                self.output.push_str(&format!(
                    "const {} = _template(\"{}\")()",
                    frame.node_var, template
                ));
            }
            self.output.push('\n');

            // Emit one-time static _setProp calls
            for (name, expr) in &frame.static_set_props {
                self.output.push_str(&format!(
                    "_setProp({}, \"{}\", {})\n",
                    frame.node_var, name, expr
                ));
            }

            // Emit _on event handlers
            for (event_name, handler) in &frame.event_handlers {
                self.output.push_str(&format!(
                    "_on({}, \"{}\", {})\n",
                    frame.node_var, event_name, handler
                ));
            }

            // Emit _renderEffect for dynamic props
            if !frame.dynamic_props.is_empty() {
                self.output.push_str("_renderEffect(() => {\n");
                for (name, expr) in &frame.dynamic_props {
                    self.output.push_str(&format!(
                        "  _setProp({}, \"{}\", {})\n",
                        frame.node_var, name, expr
                    ));
                }
                self.output.push_str("})\n");
            }

            // Emit _renderEffect for dynamic text
            if frame.has_dynamic_text {
                if let Some(ref expr) = frame.dynamic_text_expr {
                    self.output.push_str(&format!(
                        "_renderEffect(() => _setText({}, _toDisplayString({})))\n",
                        frame.node_var, expr
                    ));
                }
            }

            // Emit scope closes
            for close in frame.scope_closes.iter().rev() {
                self.output.push_str(close);
                self.output.push('\n');
            }
        }
    }
}

impl<'alloc> SyntaxPlugin<'alloc> for VaporTemplateCodegenPlugin<'alloc> {
    fn name(&self) -> &str {
        "code_gen_template_vapor"
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
            Event::OxcCompiledElementClosed(_) => {
                self.process_element_closed();
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

    fn generate_vapor(template_input: &str, alloc: &Allocator) -> String {
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

        let mut ec = ElementCompilerPlugin::new();
        let mut compiled = Vec::new();
        for event in events_storage {
            match ec.process_event(event, &mut ctx) {
                SyntaxResult::Keep(e) | SyntaxResult::Replace(e) => compiled.push(e),
                SyntaxResult::Drop => {}
            }
        }

        let mut oxc = OxcParserPlugin::new(alloc);
        let mut parsed = Vec::new();
        for event in compiled {
            match oxc.process_event(event, &mut ctx) {
                SyntaxResult::Keep(e) | SyntaxResult::Replace(e) => parsed.push(e),
                SyntaxResult::Drop => {}
            }
        }

        let mut codegen = VaporTemplateCodegenPlugin::new();
        for event in parsed {
            match codegen.process_event(event, &mut ctx) {
                SyntaxResult::Keep(_) | SyntaxResult::Replace(_) => {}
                SyntaxResult::Drop => {}
            }
        }

        codegen.take_output()
    }

    /// @ai-generated - Static template uses _template()
    #[test]
    fn test_static_template() {
        let alloc = Allocator::default();
        let output = generate_vapor("<template><div class=\"x\">text</div></template>", &alloc);
        assert!(
            output.contains("_template("),
            "Expected _template(), got: {}",
            output
        );
        assert!(
            output.contains("class=\"x\""),
            "Expected class attr in template, got: {}",
            output
        );
    }

    /// @ai-generated - Dynamic prop uses _renderEffect + _setProp
    #[test]
    fn test_dynamic_prop() {
        let alloc = Allocator::default();
        let output = generate_vapor("<template><div :id=\"x\">hello</div></template>", &alloc);
        assert!(
            output.contains("_renderEffect("),
            "Expected _renderEffect, got: {}",
            output
        );
        assert!(
            output.contains("_setProp("),
            "Expected _setProp, got: {}",
            output
        );
    }

    /// @ai-generated - Event handler uses _on()
    #[test]
    fn test_event_handler() {
        let alloc = Allocator::default();
        let output = generate_vapor(
            "<template><div @click=\"handler\">hello</div></template>",
            &alloc,
        );
        assert!(output.contains("_on("), "Expected _on(), got: {}", output);
        assert!(
            output.contains("\"click\""),
            "Expected click event name, got: {}",
            output
        );
    }

    /// @ai-generated - Interpolation uses _renderEffect + _setText
    #[test]
    fn test_interpolation() {
        let alloc = Allocator::default();
        let output = generate_vapor("<template><div>{{ msg }}</div></template>", &alloc);
        assert!(
            output.contains("_renderEffect("),
            "Expected _renderEffect for text, got: {}",
            output
        );
        assert!(
            output.contains("_setText("),
            "Expected _setText, got: {}",
            output
        );
        assert!(
            output.contains("_toDisplayString("),
            "Expected _toDisplayString, got: {}",
            output
        );
    }

    /// @ai-generated - v-if uses _createIf()
    #[test]
    fn test_vif() {
        let alloc = Allocator::default();
        let output = generate_vapor("<template><div v-if=\"show\">yes</div></template>", &alloc);
        assert!(
            output.contains("_createIf("),
            "Expected _createIf, got: {}",
            output
        );
    }

    /// @ai-generated - v-for uses _createFor()
    #[test]
    fn test_vfor() {
        let alloc = Allocator::default();
        let output = generate_vapor(
            "<template><div v-for=\"item of items\">{{ item }}</div></template>",
            &alloc,
        );
        assert!(
            output.contains("_createFor("),
            "Expected _createFor, got: {}",
            output
        );
    }

    /// @ai-generated - Scoped style adds data-v attribute in template string
    #[test]
    fn test_scoped_attr_in_template() {
        let alloc = Allocator::default();
        let template = "<template><div>hello</div></template>";

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

        let mut codegen = VaporTemplateCodegenPlugin::new();
        codegen.set_scope_id(*b"a1b2c3d4");
        for event in parsed {
            match codegen.process_event(event, &mut ctx) {
                SyntaxResult::Keep(_) | SyntaxResult::Replace(_) => {}
                SyntaxResult::Drop => {}
            }
        }

        let output = codegen.take_output();
        assert!(
            output.contains("data-v-a1b2c3d4"),
            "Expected data-v scope attr in template, got: {}",
            output
        );
    }
}
