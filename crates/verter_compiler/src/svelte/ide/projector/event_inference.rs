//! Named DOM-handler parameter inference for Svelte 5 runes components.
//!
//! Svelte's template owns the contextual event surface, but a named function
//! declaration in the instance script is not a child of the JSX attribute and
//! therefore receives no native TypeScript contextual type. For TypeScript
//! runes components only, this pass connects a simple handler reference to the
//! installed `svelte/elements` contract by rewriting the untyped parameter list
//! to a rest-destructured `Parameters<...>` tuple. JavaScript is intentionally
//! excluded: authored JSDoc remains its type authority.

use oxc_ast::ast::{BindingPattern, Declaration, Function, Statement};
use oxc_parser::Parser;
use oxc_span::SourceType;
use rustc_hash::FxHashMap;

use crate::code_transform::CodeTransform;
use crate::svelte::parser::{
    ParsedSvelte, SvelteAttributeKind, SvelteAttributeValue, SvelteDirectiveKind,
    SvelteElementKind, SvelteNode,
};

use super::is_valid_binding_identifier;

/// Apply the inference to the instance script when (and only when) the shared
/// Svelte mode authority classifies the component as runes.
pub(super) fn apply_event_handler_param_inference(
    source: &str,
    parsed: &ParsedSvelte,
    out: &mut CodeTransform<'_>,
) {
    let Some(content) = parsed.instance_content() else {
        return;
    };
    let Some(script) = parsed.instance_script.as_ref() else {
        return;
    };
    if script.lang.as_deref() != Some("ts") {
        return;
    }

    let hints = collect_handler_hints(&parsed.template, source);
    if hints.is_empty() {
        return;
    }

    let body = &source[content.start as usize..content.end as usize];
    let allocator = oxc_allocator::Allocator::default();
    let parsed_script = Parser::new(&allocator, body, SourceType::ts()).parse();
    if !parsed_script.errors.is_empty() {
        return;
    }
    for statement in &parsed_script.program.body {
        match statement {
            Statement::FunctionDeclaration(function) => {
                annotate_function(function, &hints, source, content.start, out);
            }
            Statement::ExportNamedDeclaration(export) => {
                if let Some(Declaration::FunctionDeclaration(function)) = &export.declaration {
                    annotate_function(function, &hints, source, content.start, out);
                }
            }
            _ => {}
        }
    }
}

/// Collect every distinct official DOM-handler tuple for each simple named handler.
fn collect_handler_hints(nodes: &[SvelteNode], source: &str) -> FxHashMap<String, Vec<String>> {
    let mut hints = FxHashMap::default();
    collect_handler_hints_from_nodes(nodes, source, &mut hints);
    hints
}

fn collect_handler_hints_from_nodes(
    nodes: &[SvelteNode],
    source: &str,
    hints: &mut FxHashMap<String, Vec<String>>,
) {
    for node in nodes {
        match node {
            SvelteNode::Element(element) => {
                if matches!(element.kind, SvelteElementKind::Intrinsic) {
                    for attribute in &element.attributes {
                        let (event_prop, value) = match &attribute.kind {
                            SvelteAttributeKind::Plain { name, value, .. }
                                if name.len() > 2 && name.starts_with("on") =>
                            {
                                (name.as_str(), value.as_ref())
                            }
                            SvelteAttributeKind::Directive(directive)
                                if matches!(directive.kind, SvelteDirectiveKind::On) =>
                            {
                                // The projector lowers legacy `on:click` to the
                                // official lowercase `onclick` attribute.
                                // Allocate below only after the handler qualifies.
                                let Some(SvelteAttributeValue::Expression(value)) =
                                    directive.value.as_ref()
                                else {
                                    continue;
                                };
                                let handler =
                                    source[value.start as usize..value.end as usize].trim();
                                if !is_valid_binding_identifier(handler) {
                                    continue;
                                }
                                let event_prop = format!("on{}", directive.local);
                                let tuple = official_event_tuple(&element.name, &event_prop);
                                push_distinct_hint(hints, handler, tuple);
                                continue;
                            }
                            _ => continue,
                        };
                        let Some(SvelteAttributeValue::Expression(value)) = value else {
                            continue;
                        };
                        let handler = source[value.start as usize..value.end as usize].trim();
                        if !is_valid_binding_identifier(handler) {
                            continue;
                        }
                        let tuple = official_event_tuple(&element.name, event_prop);
                        push_distinct_hint(hints, handler, tuple);
                    }
                }
                collect_handler_hints_from_nodes(&element.children, source, hints);
            }
            SvelteNode::Block(block) => {
                collect_handler_hints_from_nodes(&block.children, source, hints);
                for clause in &block.clauses {
                    collect_handler_hints_from_nodes(&clause.children, source, hints);
                }
            }
            SvelteNode::Text(_)
            | SvelteNode::Comment(_)
            | SvelteNode::Interpolation(_)
            | SvelteNode::Tag(_) => {}
        }
    }
}

fn push_distinct_hint(hints: &mut FxHashMap<String, Vec<String>>, handler: &str, tuple: String) {
    let handler_hints = hints.entry(handler.to_string()).or_default();
    if !handler_hints.contains(&tuple) {
        handler_hints.push(tuple);
    }
}

fn official_event_tuple(tag: &str, event_prop: &str) -> String {
    format!(
        "Parameters<NonNullable<import(\"svelte/elements\").SvelteHTMLElements[{tag:?}][{event_prop:?}]>>"
    )
}

fn annotate_function(
    function: &Function<'_>,
    hints: &FxHashMap<String, Vec<String>>,
    source: &str,
    content_start: u32,
    out: &mut CodeTransform<'_>,
) {
    let Some(identifier) = &function.id else {
        return;
    };
    let Some(tuples) = hints.get(identifier.name.as_str()) else {
        return;
    };
    let tuple = tuples.join(" | ");
    if function.params.rest.is_some() || function.params.items.is_empty() {
        return;
    }

    let mut identifiers = Vec::with_capacity(function.params.items.len());
    for parameter in &function.params.items {
        if parameter.type_annotation.is_some() {
            return;
        }
        let BindingPattern::BindingIdentifier(identifier) = &parameter.pattern else {
            return;
        };
        identifiers.push((
            content_start + identifier.span.start,
            content_start + identifier.span.end,
        ));
    }

    let params_start = content_start + function.params.span.start;
    let params_end = content_start + function.params.span.end;
    let Some(&(first_start, _)) = identifiers.first() else {
        return;
    };
    let Some(&(_, last_end)) = identifiers.last() else {
        return;
    };
    if params_end <= params_start || last_end > params_end {
        return;
    }
    let parameter_source = &source[params_start as usize..params_end as usize];
    let has_parentheses = parameter_source.starts_with('(') && parameter_source.ends_with(')');
    let prefix = if has_parentheses { "(...[" } else { "...[" };
    let suffix = if has_parentheses {
        format!("]: {tuple})")
    } else {
        format!("]: {tuple}")
    };
    // Delete only authored punctuation and insert framework scaffolding as
    // unmapped chunks. The identifier bytes remain an Original chunk, so
    // hover/definition/rename map exactly while the synthetic tuple never
    // claims the authored closing-parenthesis span.
    out.remove(params_start, first_start);
    out.prepend_left(first_start, prefix);
    out.remove(last_end, params_end);
    out.prepend_left(params_end, &suffix);
}
