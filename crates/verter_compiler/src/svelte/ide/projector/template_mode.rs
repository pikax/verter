//! Scope-aware template facts used by Svelte mode classification.
//!
//! `$host` selects Svelte 5 runes mode only when it is a free reference. This
//! analyzer owns that decision for every IDE-projection consumer. It mirrors
//! Svelte template lexical scopes (block patterns, inline await bindings,
//! `let:` aliases, snippets, and declaration tags) and delegates expression
//! free-reference detection to the shared store/rune scanner.

use verter_span::Span;

use crate::svelte::parser::{
    SvelteAttributeKind, SvelteAttributeValue, SvelteBlockKind, SvelteClauseKind,
    SvelteDirectiveKind, SvelteNode, SvelteTagKind,
};

use super::super::store_scan::{
    collect_pattern_dollar_names, scan_pattern_default_store_subs_and_host,
    scan_store_subscriptions_and_host_with,
};

/// Whether the template contains a free `$host` rune reference.
///
/// The byte precheck keeps the common no-`$host` path allocation- and
/// traversal-free. `script_declared` contains the `$`-prefixed bindings visible
/// to template expressions.
pub(super) fn template_uses_host_rune(
    nodes: &[SvelteNode],
    source: &str,
    script_declared: &[String],
) -> bool {
    source.contains("$host") && scan_nodes_for_host(nodes, source, script_declared)
}

fn scan_nodes_for_host(nodes: &[SvelteNode], source: &str, outer: &[String]) -> bool {
    // Declaration tags and snippet declarations bind their complete sibling
    // scope. Collect them before scanning expressions so a reference is never
    // mistaken for the rune merely because its declaration appears later.
    let mut declared = outer.to_vec();
    collect_sibling_declarations(nodes, source, &mut declared);

    nodes
        .iter()
        .any(|node| scan_node_for_host(node, source, &declared))
}

fn collect_sibling_declarations(nodes: &[SvelteNode], source: &str, out: &mut Vec<String>) {
    for node in nodes {
        match node {
            SvelteNode::Tag(tag)
                if matches!(
                    tag.kind,
                    SvelteTagKind::Const | SvelteTagKind::LegacyConst | SvelteTagKind::Let
                ) =>
            {
                out.extend(collect_pattern_dollar_names(slice(source, tag.inner)));
            }
            SvelteNode::Block(block) => {
                if let SvelteBlockKind::Snippet { name_text, .. } = &block.kind {
                    out.extend(collect_pattern_dollar_names(name_text));
                }
            }
            _ => {}
        }
    }
}

fn scan_node_for_host(node: &SvelteNode, source: &str, declared: &[String]) -> bool {
    match node {
        SvelteNode::Text(_) | SvelteNode::Comment(_) => false,
        SvelteNode::Interpolation(span) => scan_span_for_host(*span, source, declared),
        SvelteNode::Tag(tag) => {
            if matches!(
                tag.kind,
                SvelteTagKind::Const | SvelteTagKind::LegacyConst | SvelteTagKind::Let
            ) {
                scan_pattern_for_host(tag.inner, source, declared)
            } else {
                scan_span_for_host(tag.inner, source, declared)
            }
        }
        SvelteNode::Element(element) => {
            let mut child_declared = declared.to_vec();
            for attribute in &element.attributes {
                match &attribute.kind {
                    SvelteAttributeKind::Directive(directive)
                        if matches!(directive.kind, SvelteDirectiveKind::Let) =>
                    {
                        match &directive.value {
                            Some(SvelteAttributeValue::Expression(span)) => {
                                if scan_pattern_for_host(*span, source, declared) {
                                    return true;
                                }
                                child_declared
                                    .extend(collect_pattern_dollar_names(slice(source, *span)));
                            }
                            _ => child_declared
                                .extend(collect_pattern_dollar_names(&directive.local)),
                        }
                    }
                    SvelteAttributeKind::Plain {
                        value: Some(SvelteAttributeValue::Expression(span)),
                        ..
                    }
                    | SvelteAttributeKind::Directive(crate::svelte::parser::SvelteDirective {
                        value: Some(SvelteAttributeValue::Expression(span)),
                        ..
                    })
                    | SvelteAttributeKind::Spread(span)
                    | SvelteAttributeKind::Attach { expr_span: span }
                        if scan_span_for_host(*span, source, declared) =>
                    {
                        return true;
                    }
                    _ => {}
                }
            }
            scan_nodes_for_host(&element.children, source, &child_declared)
        }
        SvelteNode::Block(block) => {
            if block
                .head_expr
                .is_some_and(|span| scan_span_for_host(span, source, declared))
            {
                return true;
            }

            let mut body_declared = declared.to_vec();
            match &block.kind {
                SvelteBlockKind::Each { item, index, key } => {
                    for span in [item, index].into_iter().flatten() {
                        if scan_pattern_for_host(*span, source, declared) {
                            return true;
                        }
                        body_declared.extend(collect_pattern_dollar_names(slice(source, *span)));
                    }
                    // The keyed expression is evaluated inside the item/index
                    // scope, unlike the iterable head.
                    if key.is_some_and(|span| scan_span_for_host(span, source, &body_declared)) {
                        return true;
                    }
                }
                SvelteBlockKind::Snippet { params, .. } => {
                    if let Some(span) = params {
                        if scan_pattern_for_host(*span, source, declared) {
                            return true;
                        }
                        body_declared.extend(collect_pattern_dollar_names(slice(source, *span)));
                    }
                }
                SvelteBlockKind::Await {
                    then_binding,
                    catch_binding,
                } => {
                    let has_then_clause = block
                        .clauses
                        .iter()
                        .any(|clause| matches!(clause.kind, SvelteClauseKind::Then));
                    let has_catch_clause = block
                        .clauses
                        .iter()
                        .any(|clause| matches!(clause.kind, SvelteClauseKind::Catch));
                    let inline_binding = if !has_then_clause && then_binding.is_some() {
                        *then_binding
                    } else if !has_then_clause && !has_catch_clause && catch_binding.is_some() {
                        *catch_binding
                    } else {
                        None
                    };
                    if let Some(span) = inline_binding {
                        if scan_pattern_for_host(span, source, declared) {
                            return true;
                        }
                        body_declared.extend(collect_pattern_dollar_names(slice(source, span)));
                    }
                }
                SvelteBlockKind::If | SvelteBlockKind::Key => {}
            }

            if scan_nodes_for_host(&block.children, source, &body_declared) {
                return true;
            }

            for clause in &block.clauses {
                let mut clause_declared = declared.to_vec();
                match clause.kind {
                    SvelteClauseKind::ElseIf => {
                        if clause
                            .expr
                            .is_some_and(|span| scan_span_for_host(span, source, declared))
                        {
                            return true;
                        }
                    }
                    SvelteClauseKind::Then | SvelteClauseKind::Catch => {
                        if let Some(span) = clause.expr {
                            if scan_pattern_for_host(span, source, declared) {
                                return true;
                            }
                            clause_declared
                                .extend(collect_pattern_dollar_names(slice(source, span)));
                        }
                    }
                    SvelteClauseKind::Else => {}
                }
                if scan_nodes_for_host(&clause.children, source, &clause_declared) {
                    return true;
                }
            }
            false
        }
    }
}

fn scan_span_for_host(span: Span, source: &str, declared: &[String]) -> bool {
    scan_store_subscriptions_and_host_with(slice(source, span), declared).uses_host_rune
}

fn scan_pattern_for_host(span: Span, source: &str, declared: &[String]) -> bool {
    let pattern = slice(source, span);
    let mut in_scope = declared.to_vec();
    in_scope.extend(collect_pattern_dollar_names(pattern));
    scan_pattern_default_store_subs_and_host(pattern, &in_scope).uses_host_rune
}

fn slice(source: &str, span: Span) -> &str {
    &source[span.start as usize..span.end as usize]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::svelte::parser::parse_svelte;

    fn uses_host(source: &str) -> bool {
        let parsed = parse_svelte(source);
        template_uses_host_rune(&parsed.template, source, &[])
    }

    #[test]
    fn free_template_host_is_a_rune_mode_fact() {
        assert!(uses_host("<button onclick={() => $host()}>host</button>"));
    }

    #[test]
    fn inline_await_host_binding_shadows_the_rune() {
        assert!(!uses_host(
            "{#await pending then $host}<span>{$host.id}</span>{/await}"
        ));
    }

    #[test]
    fn let_alias_host_binding_shadows_the_rune_in_component_children() {
        assert!(!uses_host("<Comp let:item={$host}>{$host.id}</Comp>"));
    }

    #[test]
    fn sibling_declaration_host_binding_shadows_the_rune() {
        assert!(!uses_host(
            "{@const $host = () => ({ id: 1 })}<span>{$host().id}</span>"
        ));
    }
}
