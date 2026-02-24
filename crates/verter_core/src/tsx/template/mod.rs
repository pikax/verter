//! TSX template generation: converts Vue template AST to valid JSX.
//!
//! Walks the [`TemplateAst`] directly (not using the shared `TemplateCodeGen` trait
//! or walker) and converts Vue template syntax to JSX using `CodeTransform` mutations.
//!
//! ## Conversion rules
//!
//! | Vue syntax | JSX output |
//! |---|---|
//! | `{{ expr }}` | `{expr}` |
//! | `<!-- comment -->` | `{/* comment */}` |
//! | `:prop="expr"` | `prop={expr}` |
//! | `@event="handler"` | `onEvent={handler}` |
//! | `v-if="cond"` | `{cond ? (...) : null}` |
//! | `v-for="item in items"` | `{items.map((item) => (...))}` |
//! | `v-show="expr"` | `style={{display: expr ? undefined : 'none'}}` |
//! | `v-model="val"` | `modelValue={val} onUpdate:modelValue={...}` |
//! | `v-bind="obj"` | `{...obj}` |
//! | `v-on="obj"` | `{...obj}` |

pub mod directives;
pub mod props;

use oxc_allocator::Allocator;
use rustc_hash::FxHashMap;

use crate::ast::types::{
    AstNodeKind, CommentNode, ElementNode, InterpolationNode, TagType, TextNode,
};
use crate::template::code_gen::binding::{BindingResolver, BindingType};
use crate::template::code_gen::types::CodeGenOutput;
use crate::template::oxc::types::{OxcNodeData, OxcParsedAst, OxcParsedElement};
use crate::types::NodeId;

use super::TsxTemplateOptions;

/// Generate TSX template (JSX) from the template AST.
///
/// Walks the AST and produces JSX output by overwriting Vue-specific syntax
/// with JSX equivalents. Uses `CodeGenOutput` for deferred batch operations.
pub fn generate_tsx_template<'alloc>(
    ast: &crate::ast::types::TemplateAst,
    oxc_ast: &OxcParsedAst<'alloc>,
    source: &'alloc str,
    out: &mut CodeGenOutput<'alloc>,
    alloc: &'alloc Allocator,
    bindings: &FxHashMap<&'alloc str, BindingType>,
    options: &TsxTemplateOptions<'_>,
) {
    let resolver = BindingResolver::new(bindings.clone(), true);

    let root = &ast.root;
    let content = match &root.content {
        Some(c) => c,
        None => return, // No template content
    };

    // Overwrite <template> tags
    // Replace <template> open tag with empty (we just want the content)
    out.overwrite(root.tag_open.start, root.tag_open.end, "");

    // Replace </template> close tag with empty
    if let Some(tag_close) = &root.tag_close {
        out.overwrite(tag_close.start, tag_close.end, "");
    }

    // Walk root children
    let children = &content.children;

    // If multiple root children, wrap in fragment
    let needs_fragment = children.len() > 1;
    if needs_fragment {
        out.prepend_alloc(content.start, "<>");
    }

    for &child_id in children.iter() {
        walk_node(
            child_id, ast, oxc_ast, source, out, alloc, &resolver, options,
        );
    }

    if needs_fragment {
        out.prepend_alloc(content.end, "</>");
    }
}

/// Walk a single AST node and generate JSX output.
#[allow(clippy::too_many_arguments)]
fn walk_node<'alloc>(
    id: NodeId,
    ast: &crate::ast::types::TemplateAst,
    oxc_ast: &OxcParsedAst<'alloc>,
    source: &'alloc str,
    out: &mut CodeGenOutput<'alloc>,
    alloc: &'alloc Allocator,
    resolver: &BindingResolver<'alloc>,
    options: &TsxTemplateOptions<'_>,
) {
    let node = &ast.nodes[id.0];
    let oxc_data = &oxc_ast.data[id.0];

    match &node.kind {
        AstNodeKind::Element(el) => {
            let oxc_el = match oxc_data {
                OxcNodeData::Element(el) => Some(el.as_ref()),
                _ => None,
            };
            walk_element(
                id, el, oxc_el, ast, oxc_ast, source, out, alloc, resolver, options,
            );
        }
        AstNodeKind::Text(text) => {
            visit_text(text, source, out);
        }
        AstNodeKind::Interpolation(interp) => {
            let oxc_expr = match oxc_data {
                OxcNodeData::Interpolation(expr) => Some(expr),
                _ => None,
            };
            visit_interpolation(interp, oxc_expr, source, out, resolver);
        }
        AstNodeKind::Comment(comment) => {
            visit_comment(comment, out, options);
        }
    }
}

/// Walk an element node: handle directives, props, children.
#[allow(clippy::too_many_arguments)]
fn walk_element<'alloc>(
    _id: NodeId,
    el: &ElementNode,
    oxc_el: Option<&OxcParsedElement<'alloc>>,
    ast: &crate::ast::types::TemplateAst,
    oxc_ast: &OxcParsedAst<'alloc>,
    source: &'alloc str,
    out: &mut CodeGenOutput<'alloc>,
    alloc: &'alloc Allocator,
    resolver: &BindingResolver<'alloc>,
    options: &TsxTemplateOptions<'_>,
) {
    // Handle structural directives first
    let has_v_if = el.v_condition.is_some();
    let has_v_for = el.v_for.is_some();

    // v-for wrapping
    if has_v_for {
        directives::emit_v_for_open(el, oxc_el, source, out, alloc, resolver);
    }

    // v-if/v-else-if/v-else wrapping
    if has_v_if {
        directives::emit_v_if_open(el, oxc_el, source, out, alloc, resolver);
    }

    // Handle the element tag itself
    let tag_name = &source[(el.tag_open.start + 1) as usize..el.tag_open.name_end as usize];

    // Convert tag for components
    match el.tag_type {
        TagType::Component => {
            // Handle `<component is="...">` / `<component :is="...">`.
            if tag_name == "component" {
                rewrite_component_is(el, source, out);
            }
        }
        TagType::Template => {
            // <template> in template context → fragment
            // Replace <template with <>
            out.overwrite(el.tag_open.start, el.tag_open.name_end, "<>");
            // Remove all props from the template tag
            if el.tag_open.name_end < el.tag_open.end - 1 {
                // -1 for the > char
                out.overwrite(el.tag_open.name_end, el.tag_open.end - 1, "");
            }
        }
        TagType::SlotOutlet => {
            // <slot> → convert to slot function call pattern
            // For now, pass through as <slot>
        }
        _ => {
            // Native HTML elements — pass through
        }
    }

    // Process props/attributes → JSX
    props::process_element_props(el, oxc_el, source, out, alloc, resolver);

    // Process v-show
    directives::emit_v_show(el, oxc_el, source, out, alloc, resolver);

    // Walk children
    if let Some(content) = &el.content {
        for &child_id in content.children.iter() {
            walk_node(
                child_id, ast, oxc_ast, source, out, alloc, resolver, options,
            );
        }
    }

    // Handle close tag for <template> → </>
    if el.tag_type == TagType::Template {
        if let Some(tag_close) = &el.tag_close {
            out.overwrite(tag_close.start, tag_close.end, "</>");
        }
    }

    // Close v-if ternary
    if has_v_if {
        directives::emit_v_if_close(el, source, out);
    }

    // Close v-for
    if has_v_for {
        directives::emit_v_for_close(el, source, out);
    }
}

fn rewrite_component_is(el: &ElementNode, source: &str, out: &mut CodeGenOutput<'_>) {
    let static_is_prop = el.props.iter().find(|prop| {
        if prop.is_directive {
            return false;
        }
        &source[prop.start as usize..prop.name_end as usize] == "is"
    });

    // 1) Static `is="div"`
    if let Some(is_prop) = static_is_prop {
        let (Some(value_start), Some(value_end)) = (is_prop.value_start, is_prop.value_end) else {
            return;
        };
        if value_end <= value_start {
            return;
        }

        let target_tag = source[value_start as usize..value_end as usize].trim();
        if target_tag.is_empty() {
            return;
        }

        rewrite_component_tag_name(el, target_tag, out);

        // Remove `is="..."`
        let is_prop_end = props::get_prop_end(is_prop);
        out.overwrite(is_prop.start, is_prop_end, "");
        return;
    }

    // 2) Dynamic `:is="expr"` / `v-bind:is="expr"`
    let bind_is_prop = el.props.iter().find(|prop| {
        if !prop.is_directive || prop.is_dynamic == Some(true) {
            return false;
        }
        if directive_name(prop, source) != "bind" {
            return false;
        }
        let (Some(arg_start), Some(arg_end)) = (prop.arg_start, prop.arg_end) else {
            return false;
        };
        source[arg_start as usize..arg_end as usize].trim() == "is"
    });

    let Some(bind_is_prop) = bind_is_prop else {
        return;
    };

    let (Some(value_start), Some(value_end)) = (bind_is_prop.value_start, bind_is_prop.value_end)
    else {
        return;
    };
    if value_end <= value_start {
        return;
    }

    let value_expr = source[value_start as usize..value_end as usize].trim();
    if value_expr.is_empty() {
        return;
    }

    if let Some(literal_tag) = parse_string_literal(value_expr) {
        rewrite_component_tag_name(el, literal_tag, out);
    } else {
        let temp_name = format!("__verter_component_render_{}", el.tag_open.start);
        out.prepend_alloc(
            el.tag_open.start,
            &format!("const {} = ({});\n", temp_name, value_expr),
        );
        rewrite_component_tag_name(el, &temp_name, out);
    }

    // Remove `:is="..."`
    let prop_end = props::get_prop_end(bind_is_prop);
    out.overwrite(bind_is_prop.start, prop_end, "");
}

fn rewrite_component_tag_name(el: &ElementNode, target_tag: &str, out: &mut CodeGenOutput<'_>) {
    // Rewrite opening `<component` to `<targetTag`.
    out.overwrite(el.tag_open.start + 1, el.tag_open.name_end, target_tag);

    // Rewrite closing `</component>` if present.
    if let Some(tag_close) = &el.tag_close {
        out.overwrite(tag_close.start + 2, tag_close.name_end, target_tag);
    }
}

fn directive_name<'a>(prop: &crate::types::NodeProp, source: &'a str) -> &'a str {
    let name = &source[prop.start as usize..prop.name_end as usize];
    if name.starts_with(':') || name.starts_with('.') {
        return "bind";
    }
    if name.starts_with('@') {
        return "on";
    }
    if name.starts_with('#') {
        return "slot";
    }
    name.strip_prefix("v-").unwrap_or(name)
}

fn parse_string_literal(value_expr: &str) -> Option<&str> {
    let bytes = value_expr.as_bytes();
    if bytes.len() < 2 {
        return None;
    }
    let first = bytes[0];
    let last = bytes[bytes.len() - 1];
    if (first == b'\'' && last == b'\'') || (first == b'"' && last == b'"') {
        return Some(&value_expr[1..value_expr.len() - 1]);
    }
    None
}

/// Visit a text node.
///
/// To keep TSX valid for content like `2 < 1`, non-empty trimmed text is wrapped
/// as a string expression (`{"..."}`), matching v5/process text-plugin semantics.
/// Whitespace-only text remains unchanged.
fn visit_text(text: &TextNode, source: &str, out: &mut CodeGenOutput<'_>) {
    if text.end <= text.start {
        return;
    }

    let raw_text = &source[text.start as usize..text.end as usize];
    let trimmed = raw_text.trim();
    if trimmed.is_empty() || trimmed == "<" {
        return;
    }

    let Some(rel_start) = raw_text.find(trimmed) else {
        return;
    };

    let content_start = text.start + rel_start as u32;
    let content_end = content_start + trimmed.len() as u32;

    // Escape quotes inside the string literal body.
    for (i, b) in trimmed.as_bytes().iter().enumerate() {
        if *b == b'"' {
            let pos = content_start + i as u32;
            out.overwrite(pos, pos + 1, "\\\"");
        }
    }

    out.prepend_alloc(content_start, "{\"");
    out.prepend_alloc(content_end, "\"}");
}

/// Visit an interpolation node: `{{ expr }}` → `{expr}`.
fn visit_interpolation<'alloc>(
    interp: &InterpolationNode,
    oxc_expr: Option<&crate::template::oxc::types::OxcParsedExpression<'alloc>>,
    _source: &'alloc str,
    out: &mut CodeGenOutput<'alloc>,
    resolver: &BindingResolver<'alloc>,
) {
    // Replace `{{` with `{`
    out.overwrite(interp.start, interp.inner_start, "{");

    // Apply binding prefixes to expression identifiers
    if let Some(expr) = oxc_expr {
        if let Some(ref bindings) = expr.bindings {
            resolver.collect_binding_patches(bindings, out);
        }
    }

    // Replace `}}` with `}`
    out.overwrite(interp.inner_end, interp.end, "}");
}

/// Visit a comment node: `<!-- text -->` → `{/* text */}`.
fn visit_comment(
    comment: &CommentNode,
    out: &mut CodeGenOutput<'_>,
    options: &TsxTemplateOptions<'_>,
) {
    if !options.comments {
        // Strip comment entirely
        out.overwrite(comment.start, comment.end, "");
        return;
    }

    // Convert HTML comment to JSX comment
    // <!-- → {/*  and  --> → */}
    // Keep original comment-inner spacing untouched.
    out.overwrite(comment.start, comment.content_start, "{/*");
    out.overwrite(comment.content_end, comment.end, "*/}");
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::code_transform::CodeTransform;

    /// Helper: compile a full SFC with TSX template generation.
    /// Returns the template portion of the TSX output.
    fn gen_tsx_template(source: &str) -> String {
        let alloc = Allocator::new();
        let bytes = source.as_bytes();

        // Parse SFC
        let mut syntax = crate::parser::Syntax::new(false);
        crate::tokenizer::byte::tokenize_sfc(bytes, |e| {
            syntax.handle(
                &e,
                &crate::diagnostics::SyntaxPluginContext {
                    input: source,
                    bytes,
                    options: &crate::diagnostics::SyntaxPluginOptions::default(),
                    diagnostics: Vec::new(),
                },
            )
        });

        let template_ast = match syntax.take_template_ast() {
            Some(ast) => ast,
            None => return String::new(),
        };

        // Parse template expressions
        let source_type = oxc_span::SourceType::tsx();
        let oxc_ast = crate::template::oxc::parse_template_expressions(
            &template_ast,
            source,
            &alloc,
            source_type,
        );

        // Generate TSX template
        let tpl_alloc = Allocator::new();
        let mut tpl_ct = CodeTransform::new(source, &tpl_alloc);
        let mut out = CodeGenOutput::new(&tpl_alloc);
        let bindings = FxHashMap::default();
        let options = TsxTemplateOptions {
            self_name: "App",
            comments: true,
        };

        generate_tsx_template(
            &template_ast,
            &oxc_ast,
            source,
            &mut out,
            &tpl_alloc,
            &bindings,
            &options,
        );
        out.apply_to(&mut tpl_ct);

        let full = tpl_ct.build_string();

        // Extract just the template region
        let tpl_start = template_ast.root.tag_open.start as usize;
        let tpl_end = template_ast
            .root
            .tag_close
            .as_ref()
            .map(|tc| tc.end as usize)
            .unwrap_or(full.len());
        let suffix_len = source.len() - tpl_end;
        full[tpl_start..full.len() - suffix_len].to_string()
    }

    fn gen_tsx_template_with_bindings(source: &str, bindings: &[(&str, BindingType)]) -> String {
        let alloc = Allocator::new();
        let bytes = source.as_bytes();

        let mut syntax = crate::parser::Syntax::new(false);
        crate::tokenizer::byte::tokenize_sfc(bytes, |e| {
            syntax.handle(
                &e,
                &crate::diagnostics::SyntaxPluginContext {
                    input: source,
                    bytes,
                    options: &crate::diagnostics::SyntaxPluginOptions::default(),
                    diagnostics: Vec::new(),
                },
            )
        });

        let template_ast = match syntax.take_template_ast() {
            Some(ast) => ast,
            None => return String::new(),
        };

        let source_type = oxc_span::SourceType::tsx();
        let oxc_ast = crate::template::oxc::parse_template_expressions(
            &template_ast,
            source,
            &alloc,
            source_type,
        );

        let tpl_alloc = Allocator::new();
        let mut tpl_ct = CodeTransform::new(source, &tpl_alloc);
        let mut out = CodeGenOutput::new(&tpl_alloc);

        let mut binding_map: FxHashMap<&str, BindingType> = FxHashMap::default();
        for &(name, bt) in bindings {
            binding_map.insert(tpl_alloc.alloc_str(name), bt);
        }

        let options = TsxTemplateOptions {
            self_name: "App",
            comments: true,
        };

        generate_tsx_template(
            &template_ast,
            &oxc_ast,
            source,
            &mut out,
            &tpl_alloc,
            &binding_map,
            &options,
        );
        out.apply_to(&mut tpl_ct);

        let full = tpl_ct.build_string();
        let tpl_start = template_ast.root.tag_open.start as usize;
        let tpl_end = template_ast
            .root
            .tag_close
            .as_ref()
            .map(|tc| tc.end as usize)
            .unwrap_or(full.len());
        let suffix_len = source.len() - tpl_end;
        full[tpl_start..full.len() - suffix_len].to_string()
    }

    // ── Basic nodes ────────────────────────────────────────────

    #[test]
    fn basic_div() {
        let result = gen_tsx_template("<template><div></div></template>");
        assert!(result.contains("<div></div>"), "got: {}", result);
    }

    #[test]
    fn text_content() {
        let result = gen_tsx_template("<template><div>hello</div></template>");
        assert!(result.contains("<div>{\"hello\"}</div>"), "got: {}", result);
    }

    #[test]
    fn text_content_with_lt_wrapped() {
        let result = gen_tsx_template("<template>2 < 1</template>");
        assert!(
            result.contains("{\"2 < 1\"}")
                || (result.contains("{\"2\"}") && result.contains("{\"< 1\"}")),
            "got: {}",
            result
        );
    }

    #[test]
    fn text_content_escapes_quote() {
        let result = gen_tsx_template("<template>\"</template>");
        assert!(result.contains("{\"\\\"\"}"), "got: {}", result);
    }

    #[test]
    fn interpolation_basic() {
        let result = gen_tsx_template("<template><div>{{ msg }}</div></template>");
        assert!(
            result.contains("{ _ctx.msg }"),
            "{{ msg }} should become {{ _ctx.msg }}, got: {}",
            result
        );
    }

    #[test]
    fn interpolation_expression() {
        let result = gen_tsx_template("<template><div>{{ a + b }}</div></template>");
        assert!(result.contains("{ _ctx.a + _ctx.b }"), "got: {}", result);
    }

    #[test]
    fn comment_preserved() {
        let result = gen_tsx_template("<template><!-- hello --></template>");
        assert!(
            result.contains("{/* hello */}"),
            "Comment should be converted to JSX, got: {}",
            result
        );
    }

    #[test]
    fn self_closing_element() {
        let result = gen_tsx_template("<template><br/></template>");
        assert!(result.contains("<br/>"), "got: {}", result);
    }

    #[test]
    fn nested_elements() {
        let result = gen_tsx_template("<template><div><span></span></div></template>");
        assert!(
            result.contains("<div><span></span></div>"),
            "got: {}",
            result
        );
    }

    #[test]
    fn multiple_root_elements() {
        let result = gen_tsx_template("<template><div></div><span></span></template>");
        assert!(
            result.contains("<>") && result.contains("</>"),
            "Multiple root elements should be wrapped in fragment, got: {}",
            result
        );
    }

    // ── Interpolation with bindings ────────────────────────────

    #[test]
    fn interpolation_with_setup_ref() {
        let result = gen_tsx_template_with_bindings(
            "<template><div>{{ count }}</div></template>",
            &[("count", BindingType::SetupRef)],
        );
        // In inline mode, SetupRef gets no prefix but gets .value suffix
        assert!(
            result.contains("count.value"),
            "SetupRef should get .value suffix in inline mode, got: {}",
            result
        );
    }

    #[test]
    fn interpolation_with_setup_const() {
        let result = gen_tsx_template_with_bindings(
            "<template><div>{{ msg }}</div></template>",
            &[("msg", BindingType::SetupConst)],
        );
        // SetupConst in inline mode: no prefix, no suffix
        assert!(
            result.contains("{ msg }"),
            "SetupConst should have no prefix/suffix, got: {}",
            result
        );
    }

    #[test]
    fn interpolation_with_props() {
        let result = gen_tsx_template_with_bindings(
            "<template><div>{{ title }}</div></template>",
            &[("title", BindingType::Props)],
        );
        // Props in inline mode: __props. prefix
        assert!(
            result.contains("__props.title"),
            "Props should get __props. prefix, got: {}",
            result
        );
    }
}
