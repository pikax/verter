//! Named event-handler parameter inference for Vue TypeScript script setup.
//!
//! For simple native-element handlers like `<button @click="handleClick">`,
//! rewrite `function handleClick(e) {}` into
//! `function handleClick(...[e]: [...]) {}` so the handler receives the same
//! concrete event payload as its template binding.

use oxc_ast::ast::{BindingPattern, Declaration, Function, Statement};
use rustc_hash::{FxHashMap, FxHashSet};

use super::collect_global_component_fallbacks;
use crate::ast::types::{AstNodeKind, TagType, TemplateAst};
use crate::ide::{event_to_jsx_name, get_directive_name, TemplateComponentBindings};
use crate::template::code_gen::binding::is_simple_ident;
use crate::template::code_gen::types::CodeGenOutput;

/// Infer untyped function-declaration parameters from template event bindings.
///
/// For a simple native-element handler such as `<button
/// @click="handleClick">`, rewrites `function handleClick(e) {}` into a tuple
/// parameter derived from the ambient DOM event map. Component handlers use
/// the component's public event prop tuple instead.
pub(super) fn apply_event_handler_param_inference(
    body: &[Statement<'_>],
    template_ast: Option<&TemplateAst>,
    source: &str,
    content_start: u32,
    available_bindings: &FxHashSet<String>,
    out: &mut CodeGenOutput<'_>,
) {
    let Some(template_ast) = template_ast else {
        return;
    };

    let handler_type_hints =
        collect_event_handler_type_hints(template_ast, source, available_bindings);
    if handler_type_hints.is_empty() {
        return;
    }

    for stmt in body {
        match stmt {
            Statement::FunctionDeclaration(func) => {
                maybe_annotate_function_params(
                    func,
                    &handler_type_hints,
                    source,
                    content_start,
                    out,
                );
            }
            Statement::ExportNamedDeclaration(export) => {
                if let Some(Declaration::FunctionDeclaration(func)) = &export.declaration {
                    maybe_annotate_function_params(
                        func,
                        &handler_type_hints,
                        source,
                        content_start,
                        out,
                    );
                }
            }
            _ => {}
        }
    }
}

/// Annotate untyped function parameters with inferred event handler types.
///
/// Uses targeted removals and unmapped insertions around the parameter identifiers instead of one
/// big overwrite of the entire params span. This preserves per-character source
/// map mappings for identifiers, enabling hover-to-definition on parameters.
///
/// Transform: `(event)` → `(...[event]: Type)` where `event` stays as Original source.
fn maybe_annotate_function_params(
    func: &Function<'_>,
    handler_type_hints: &FxHashMap<String, Vec<String>>,
    source: &str,
    content_start: u32,
    out: &mut CodeGenOutput<'_>,
) {
    let Some(id) = &func.id else {
        return;
    };
    let Some(type_exprs) = handler_type_hints.get(id.name.as_str()) else {
        return;
    };
    let type_expr = type_exprs.join(" | ");

    // Keep existing typing intact.
    if func.params.rest.is_some() || func.params.items.is_empty() {
        return;
    }

    // Collect ident spans (SFC-absolute) for targeted overwrites.
    let mut ident_spans: Vec<(u32, u32)> = Vec::with_capacity(func.params.items.len());
    for param in &func.params.items {
        if param.type_annotation.is_some() {
            return;
        }
        match &param.pattern {
            BindingPattern::BindingIdentifier(ident) => {
                ident_spans.push((
                    content_start + ident.span.start,
                    content_start + ident.span.end,
                ));
            }
            _ => return,
        }
    }

    if ident_spans.is_empty() {
        return;
    }

    let params_start = content_start + func.params.span.start;
    let params_end = content_start + func.params.span.end;
    if params_end <= params_start {
        return;
    }

    let params_src = &source[params_start as usize..params_end as usize];
    let has_parens = params_src.starts_with('(') && params_src.ends_with(')');

    let first_ident_start = ident_spans[0].0;
    let last_ident_end = ident_spans[ident_spans.len() - 1].1;

    // Remove authored punctuation and insert synthetic scaffolding through the
    // unmapped prepend channel. The identifier bytes remain source-owned.
    let prefix = if has_parens { "(...[" } else { "...[" };
    out.overwrite(params_start, first_ident_start, "");
    out.prepend_alloc(first_ident_start, prefix);

    let suffix = if has_parens {
        format!("]: {})", type_expr)
    } else {
        format!("]: {}", type_expr)
    };
    out.overwrite(last_ident_end, params_end, "");
    out.prepend_alloc(params_end, &suffix);
}

fn collect_event_handler_type_hints(
    ast: &TemplateAst,
    source: &str,
    available_bindings: &FxHashSet<String>,
) -> FxHashMap<String, Vec<String>> {
    let mut hints = FxHashMap::default();

    let Some(content) = &ast.root.content else {
        return hints;
    };

    // The same GlobalComponents fallback inventory the template/spread paths consume, so a
    // globally-registered component's simple handler types through its emitted
    // `InstanceType<typeof Pascal>["$props"]` const — consistent with the spread path.
    let components = TemplateComponentBindings::new(collect_global_component_fallbacks(
        Some(ast),
        source,
        |n| available_bindings.contains(n),
    ));

    for &child in content.children.iter() {
        collect_event_handler_type_hints_from_node(
            child,
            ast,
            source,
            available_bindings,
            &components,
            &mut hints,
        );
    }

    hints
}

fn collect_event_handler_type_hints_from_node(
    id: crate::types::NodeId,
    ast: &TemplateAst,
    source: &str,
    available_bindings: &FxHashSet<String>,
    components: &TemplateComponentBindings,
    hints: &mut FxHashMap<String, Vec<String>>,
) {
    let node = &ast.nodes[id.0];
    let AstNodeKind::Element(el_box) = &node.kind else {
        return;
    };
    let el = el_box.as_ref();

    let tag_name = &source[(el.tag_open.start + 1) as usize..el.tag_open.name_end as usize];
    for prop in &el.props {
        if !is_event_directive(prop, source) {
            continue;
        }
        if prop.is_dynamic == Some(true) {
            continue;
        }
        let (Some(arg_start), Some(arg_end)) = (prop.arg_start, prop.arg_end) else {
            continue;
        };
        let (Some(value_start), Some(value_end)) = (prop.value_start, prop.value_end) else {
            continue;
        };

        let handler = source[value_start as usize..value_end as usize].trim();
        if !is_simple_ident(handler) {
            continue;
        }

        let event_name = source[arg_start as usize..arg_end as usize].trim();
        if event_name.is_empty() {
            continue;
        }

        let event_prop = event_to_jsx_name(event_name);
        let component_binding = match el.tag_type {
            TagType::Component => {
                // Resolve through the shared inventory: a local script binding OR a
                // GlobalComponents fallback const. An unresolved component (no binding,
                // no fallback) is skipped.
                match components.resolve(tag_name, el.tag_type, |name| {
                    available_bindings.contains(name)
                }) {
                    Some(binding) => Some(binding),
                    None => continue,
                }
            }
            _ => None,
        };
        let Some(type_expr) = crate::ide::event_handler_params_type(
            el.tag_type,
            component_binding.as_deref(),
            &event_prop,
            Some(event_name),
        ) else {
            continue;
        };

        let handler_hints = hints.entry(handler.to_string()).or_default();
        if !handler_hints.contains(&type_expr) {
            handler_hints.push(type_expr);
        }
    }

    if let Some(content) = &el.content {
        for &child in content.children.iter() {
            collect_event_handler_type_hints_from_node(
                child,
                ast,
                source,
                available_bindings,
                components,
                hints,
            );
        }
    }
}

/// Resolve a template tag name to the script binding that declares the component.
///
/// `is_known` reports whether a given identifier is an in-scope script binding —
/// the script path passes `|n| available_bindings.contains(n)`, the template `$event`
/// spread path passes `|n| resolver.get(n).is_some()`. A simple identifier tag
/// resolves directly; a kebab-case tag resolves via its PascalCase binding.
pub(crate) fn resolve_component_binding_name(
    tag_name: &str,
    is_known: impl Fn(&str) -> bool,
) -> Option<String> {
    if is_simple_ident(tag_name) && is_known(tag_name) {
        return Some(tag_name.to_string());
    }

    if tag_name.contains('-') {
        let pascal = kebab_to_pascal_case(tag_name);
        if is_known(&pascal) {
            return Some(pascal);
        }
    }

    None
}

pub(super) fn kebab_to_pascal_case(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut upper_next = true;
    for ch in input.chars() {
        if ch == '-' || ch == '_' {
            upper_next = true;
            continue;
        }
        if upper_next {
            for up in ch.to_uppercase() {
                out.push(up);
            }
            upper_next = false;
        } else {
            out.push(ch);
        }
    }
    out
}

fn is_event_directive(prop: &crate::types::NodeProp, source: &str) -> bool {
    if !prop.is_directive {
        return false;
    }
    get_directive_name(prop, source) == "on"
}
