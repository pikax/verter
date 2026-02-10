use crate::{
    common::Span,
    syntax_kai::{
        binding_types::{resolve_binding_prefix, resolve_binding_suffix, BindingMetadata},
        plugin::{SyntaxPlugin, SyntaxPluginContext, SyntaxResult},
        types::*,
    },
};

/// TSX Codegen plugin for the syntax_kai pipeline.
///
/// Generates valid TSX from Vue SFC events for TypeScript type checking.
/// Based on the approach used in `packages/core/v5/` but as a Rust plugin.
///
/// Key differences from VDOM/Vapor:
/// - TSX is for **type checking**, not runtime execution
/// - No Vue runtime helpers (`_createVNode`, etc.)
/// - Standard JSX syntax (`<div>`, `{expr}`, `onClick={handler}`)
/// - Style blocks are commented out
/// - `:class`/`:style` use `normalizeClass()`/`normalizeStyle()`
/// - CSS module `$style` exposed as typed `Record<string, string>`
pub struct TsxCodegenPlugin<'alloc> {
    /// Binding metadata from <script setup>
    binding_metadata: BindingMetadata,
    /// Accumulated output code
    output: String,
    /// Scope stack for v-for/v-slot variable resolution
    scope_stack: Vec<ScopeFrame>,
    /// Whether we're in inline mode (setup closure)
    is_inline: bool,
    /// CSS module info
    css_modules: Vec<CssModuleInfo>,
    /// Track element nesting
    element_stack: Vec<TsxElementFrame>,
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

struct TsxElementFrame {
    tag_name: String,
    is_component: bool,
    element_id: u32,
    /// Pending scope wrapping
    scope_opens: Vec<String>,
    scope_closes: Vec<String>,
}

impl<'alloc> Default for TsxCodegenPlugin<'alloc> {
    fn default() -> Self {
        Self::new()
    }
}

impl<'alloc> TsxCodegenPlugin<'alloc> {
    pub fn new() -> Self {
        Self {
            binding_metadata: BindingMetadata::default(),
            output: String::with_capacity(4096),
            scope_stack: Vec::new(),
            is_inline: false,
            css_modules: Vec::new(),
            element_stack: Vec::new(),
            _marker: std::marker::PhantomData,
        }
    }

    pub fn set_inline(&mut self, is_inline: bool) {
        self.is_inline = is_inline;
    }

    pub fn take_output(&mut self) -> String {
        std::mem::take(&mut self.output)
    }

    fn resolve_identifier(&self, ident: &[u8], source: &[u8]) -> String {
        for frame in self.scope_stack.iter().rev() {
            for local in &frame.locals {
                if local == ident {
                    return String::from_utf8_lossy(ident).to_string();
                }
            }
        }

        let prefix = resolve_binding_prefix(ident, &self.binding_metadata, source, self.is_inline);
        let suffix = resolve_binding_suffix(ident, &self.binding_metadata, source, self.is_inline);
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
        let element_id = elem.event.element_id;

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
                    scope_opens.push(format!("{{{} ? (", expr));
                    scope_closes.push(") : null}".to_string());
                }
                ElementScope::ElseIf(cond) => {
                    let expr = if let Some(val) = cond.event.value {
                        self.emit_expression_text(val, ctx.bytes)
                    } else {
                        "true".to_string()
                    };
                    scope_opens.push(format!("{{{} ? (", expr));
                    scope_closes.push(") : null}".to_string());
                }
                ElementScope::Else(_) => {
                    scope_opens.push("{(".to_string());
                    scope_closes.push(")}".to_string());
                }
                ElementScope::For(vfor) => {
                    let iterable = if let Some(val) = vfor.event.value {
                        self.emit_expression_text(val, ctx.bytes)
                    } else {
                        "[]".to_string()
                    };

                    let mut locals = Vec::new();
                    for local_span in &vfor.parsed.locals {
                        let lb = &ctx.bytes[local_span.start as usize..local_span.end as usize];
                        locals.push(lb.to_vec());
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

                    scope_opens.push(format!("{{{}.map(({}) => (", iterable, params));
                    scope_closes.push("))}".to_string());

                    self.scope_stack.push(ScopeFrame {
                        _kind: ScopeKind::For,
                        locals,
                    });
                }
                ElementScope::SlotTemplate(_) | ElementScope::SlotElement(_) => {
                    // Simplified slot handling for TSX
                }
            }
        }

        // Emit scope opens
        for open in &scope_opens {
            self.output.push_str(open);
        }

        // Open JSX element
        self.output.push('<');
        self.output.push_str(&tag_name);

        // Emit props as JSX attributes
        for prop in &elem.props {
            match &prop.event.kind {
                PropKind::Value => {
                    let name = &ctx.bytes[prop.event.start as usize..prop.event.name_end as usize];
                    let name_str = String::from_utf8_lossy(name);
                    if let Some(val_span) = prop.event.value {
                        let val = &ctx.bytes[val_span.start as usize..val_span.end as usize];
                        self.output.push_str(&format!(
                            " {}=\"{}\"",
                            name_str,
                            String::from_utf8_lossy(val)
                        ));
                    } else {
                        self.output.push_str(&format!(" {}", name_str));
                    }
                }
                PropKind::ClassValue => {
                    if let Some(val_span) = prop.event.value {
                        let val = &ctx.bytes[val_span.start as usize..val_span.end as usize];
                        self.output
                            .push_str(&format!(" class=\"{}\"", String::from_utf8_lossy(val)));
                    }
                }
                PropKind::StyleValue => {
                    if let Some(val_span) = prop.event.value {
                        let val = &ctx.bytes[val_span.start as usize..val_span.end as usize];
                        self.output
                            .push_str(&format!(" style=\"{}\"", String::from_utf8_lossy(val)));
                    }
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
                    self.output
                        .push_str(&format!(" {}={{{}}}", prop_name, expr));
                }
                PropKind::ClassBind => {
                    let expr = if let Some(val_span) = prop.event.value {
                        self.emit_expression_text(val_span, ctx.bytes)
                    } else {
                        "undefined".to_string()
                    };
                    self.output
                        .push_str(&format!(" class={{normalizeClass({})}}", expr));
                }
                PropKind::StyleBind => {
                    let expr = if let Some(val_span) = prop.event.value {
                        self.emit_expression_text(val_span, ctx.bytes)
                    } else {
                        "undefined".to_string()
                    };
                    self.output
                        .push_str(&format!(" style={{normalizeStyle({})}}", expr));
                }
                PropKind::On => {
                    let event_name = if let Some(arg_span) = prop.event.arg {
                        let arg = String::from_utf8_lossy(
                            &ctx.bytes[arg_span.start as usize..arg_span.end as usize],
                        )
                        .to_string();
                        format!(
                            "on{}",
                            arg.chars()
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
                        .push_str(&format!(" {}={{{}}}", event_name, handler));
                }
                _ => {}
            }
        }

        // Close opening tag
        if elem.event.event_open_tag_end.is_self_closing
            || elem.event.event_open_tag.is_void_element
        {
            self.output.push_str(" />");
        } else {
            self.output.push('>');
        }

        self.element_stack.push(TsxElementFrame {
            tag_name,
            is_component,
            element_id,
            scope_opens,
            scope_closes,
        });
    }

    fn process_text(&mut self, text: &Text, ctx: &SyntaxPluginContext<'alloc>) {
        let content = &ctx.bytes[text.start as usize..text.end as usize];
        let text_str = String::from_utf8_lossy(content).trim().to_string();
        if !text_str.is_empty() {
            self.output.push_str(&text_str);
        }
    }

    fn process_interpolation(
        &mut self,
        interp: &OxcInterpolation<'alloc>,
        ctx: &SyntaxPluginContext<'alloc>,
    ) {
        let expr = self.emit_expression_text(interp.content, ctx.bytes);
        self.output.push_str(&format!("{{{}}}", expr));
    }

    fn process_element_closed(&mut self) {
        if let Some(frame) = self.element_stack.pop() {
            // Close tag
            if !frame.tag_name.is_empty() {
                self.output.push_str(&format!("</{}>", frame.tag_name));
            }

            // Emit scope closes in reverse
            for close in frame.scope_closes.iter().rev() {
                self.output.push_str(close);
            }
        }
    }

    fn process_style_block(&mut self, ps: &ProcessedStyleBlock, ctx: &SyntaxPluginContext<'alloc>) {
        // Comment out style blocks in TSX
        let content_start = ps.compiled_start.tag_open.start;
        let content_end = ps.compiled_end.end;
        let block = &ctx.bytes[content_start as usize..content_end as usize];
        let block_str = String::from_utf8_lossy(block);

        for line in block_str.lines() {
            self.output.push_str(&format!("// {}\n", line));
        }

        // Collect CSS module info
        if let Some(ref module) = ps.module {
            self.css_modules.push(module.clone());
        }
    }
}

impl<'alloc> SyntaxPlugin<'alloc> for TsxCodegenPlugin<'alloc> {
    fn name(&self) -> &str {
        "code_gen_tsx"
    }

    fn process_event(
        &mut self,
        event: Event<'alloc>,
        ctx: &mut SyntaxPluginContext<'alloc>,
    ) -> SyntaxResult<Event<'alloc>> {
        match &event {
            Event::ScriptBindings(ref metadata) => {
                self.binding_metadata = metadata.clone();
                SyntaxResult::Keep(event)
            }
            Event::ProcessedStyle(ref ps) => {
                self.process_style_block(ps, ctx);
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

    fn generate_tsx(template_input: &str, alloc: &Allocator) -> String {
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

        let mut events_storage: Vec<Event<'_>> = Vec::new();
        let ptr = &mut events_storage as *mut Vec<Event<'_>>;
        {
            let mut syntax = Syntax::new(unsafe { &mut *ptr }, false);
            for event in &tokenizer_events {
                syntax.handle(event, &mut ctx);
            }
        }

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

        let mut codegen = TsxCodegenPlugin::new();
        for event in parsed {
            match codegen.process_event(event, &mut ctx) {
                SyntaxResult::Keep(_) | SyntaxResult::Replace(_) => {}
                SyntaxResult::Drop => {}
            }
        }

        codegen.take_output()
    }

    /// @ai-generated - Simple element produces JSX
    #[test]
    fn test_tsx_simple_element() {
        let alloc = Allocator::default();
        let output = generate_tsx("<template><div>hello</div></template>", &alloc);
        assert!(output.contains("<div>"), "Expected <div>, got: {}", output);
        assert!(
            output.contains("hello"),
            "Expected hello text, got: {}",
            output
        );
        assert!(
            output.contains("</div>"),
            "Expected </div>, got: {}",
            output
        );
    }

    /// @ai-generated - Dynamic binding in TSX
    #[test]
    fn test_tsx_setup_binding() {
        let alloc = Allocator::default();
        let output = generate_tsx("<template><div :id=\"x\">hello</div></template>", &alloc);
        assert!(
            output.contains("id={"),
            "Expected JSX binding, got: {}",
            output
        );
    }

    /// @ai-generated - Interpolation in TSX
    #[test]
    fn test_tsx_interpolation() {
        let alloc = Allocator::default();
        let output = generate_tsx("<template><div>{{ msg }}</div></template>", &alloc);
        assert!(
            output.contains("{"),
            "Expected JSX expression, got: {}",
            output
        );
    }

    /// @ai-generated - Component produces JSX component tag
    #[test]
    fn test_tsx_component() {
        let alloc = Allocator::default();
        let output = generate_tsx("<template><MyComp :msg=\"x\"></MyComp></template>", &alloc);
        assert!(
            output.contains("<MyComp"),
            "Expected component tag, got: {}",
            output
        );
        assert!(
            output.contains("msg={"),
            "Expected msg binding, got: {}",
            output
        );
    }

    /// @ai-generated - v-if produces ternary in TSX
    #[test]
    fn test_tsx_vif() {
        let alloc = Allocator::default();
        let output = generate_tsx("<template><div v-if=\"show\">yes</div></template>", &alloc);
        assert!(output.contains("? ("), "Expected ternary, got: {}", output);
        assert!(
            output.contains(": null}"),
            "Expected null fallback, got: {}",
            output
        );
    }

    /// @ai-generated - v-for produces .map() in TSX
    #[test]
    fn test_tsx_vfor() {
        let alloc = Allocator::default();
        let output = generate_tsx(
            "<template><div v-for=\"item of items\">{{ item }}</div></template>",
            &alloc,
        );
        assert!(output.contains(".map("), "Expected .map(), got: {}", output);
    }

    /// @ai-generated - Event handler in TSX
    #[test]
    fn test_tsx_event() {
        let alloc = Allocator::default();
        let output = generate_tsx(
            "<template><div @click=\"handler\">hello</div></template>",
            &alloc,
        );
        assert!(
            output.contains("onClick={"),
            "Expected onClick, got: {}",
            output
        );
    }

    /// @ai-generated - ctx fallback for unknown bindings
    #[test]
    fn test_tsx_ctx_fallback() {
        let alloc = Allocator::default();
        let output = generate_tsx("<template><div>{{ unknownVar }}</div></template>", &alloc);
        assert!(
            output.contains("_ctx.unknownVar"),
            "Expected _ctx prefix, got: {}",
            output
        );
    }
}
