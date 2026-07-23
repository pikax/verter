use super::*;

/// Extract a TypeScript type annotation from a hover markdown string.
///
/// Handles formats like:
/// - "```typescript\nconst x: number\n```"
/// - "(property) x: string"
/// - "let x: Ref<number>"
pub(in crate::server) fn extract_type_from_hover(
    contents: &str,
    binding_name: &str,
) -> Option<String> {
    // Look for pattern: `name: type` or `name = value`
    let patterns = [format!("{binding_name}: "), format!("{binding_name}:")];

    for line in contents.lines() {
        let trimmed = line.trim().trim_start_matches("```typescript").trim();
        for pattern in &patterns {
            if let Some(idx) = trimmed.find(pattern.as_str()) {
                let after = &trimmed[idx + pattern.len()..];
                let type_str = after.trim().trim_end_matches("```").trim();
                if !type_str.is_empty() {
                    return Some(type_str.to_string());
                }
            }
        }
    }

    None
}

pub(in crate::server) fn identifier_prefix_before_offset(
    content: &str,
    offset: usize,
) -> Option<&str> {
    if offset == 0 || offset > content.len() {
        return None;
    }

    let bytes = content.as_bytes();
    let mut start = offset;
    while start > 0 {
        let byte = bytes[start - 1];
        if byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'$' {
            start -= 1;
        } else {
            break;
        }
    }

    if start == offset {
        return None;
    }

    let prefix = &content[start..offset];
    let first = prefix.as_bytes()[0];
    if first.is_ascii_alphabetic() || first == b'_' || first == b'$' {
        Some(prefix)
    } else {
        None
    }
}

pub(in crate::server) fn is_immediately_after_member_access_dot(
    content: &str,
    offset: usize,
) -> bool {
    if offset == 0 || offset > content.len() {
        return false;
    }

    let bytes = content.as_bytes();
    let mut i = offset;
    while i > 0 && bytes[i - 1].is_ascii_whitespace() {
        i -= 1;
    }

    i > 0 && bytes[i - 1] == b'.' && (i < 2 || bytes[i - 2] != b'.')
}

pub(in crate::server) fn is_identifier_prefix_completion_kind(
    kind: crate::type_provider::protocol::CompletionKind,
) -> bool {
    matches!(
        kind,
        crate::type_provider::protocol::CompletionKind::Variable
            | crate::type_provider::protocol::CompletionKind::Function
            | crate::type_provider::protocol::CompletionKind::Method
            | crate::type_provider::protocol::CompletionKind::Property
            | crate::type_provider::protocol::CompletionKind::Field
            | crate::type_provider::protocol::CompletionKind::Constant
            | crate::type_provider::protocol::CompletionKind::EnumMember
    )
}

pub(in crate::server) fn is_member_access_completion_kind(
    kind: crate::type_provider::protocol::CompletionKind,
) -> bool {
    matches!(
        kind,
        crate::type_provider::protocol::CompletionKind::Property
            | crate::type_provider::protocol::CompletionKind::Field
            | crate::type_provider::protocol::CompletionKind::Method
            | crate::type_provider::protocol::CompletionKind::Constant
            | crate::type_provider::protocol::CompletionKind::EnumMember
    )
}

pub(in crate::server) fn filter_type_provider_completion_result(
    type_result: &mut crate::type_provider::protocol::CompletionResult,
    expr_context: Option<&ExpressionContext>,
    identifier_prefix: Option<&str>,
    verter_items: Option<&Vec<CompletionItem>>,
    enforce_verter_scope_allowlist: bool,
    provider_only_template_scope: Option<&std::collections::HashSet<String>>,
) {
    if !matches!(expr_context, Some(ExpressionContext::MemberAccess)) {
        if let Some(scope) = provider_only_template_scope {
            let before = type_result.items.len();
            type_result
                .items
                .retain(|item| scope.contains(item.label.as_str()));
            tracing::debug!(
                "completion: bounded provider-only template scope: {} -> {} items",
                before,
                type_result.items.len()
            );
        }
    }

    if matches!(expr_context, Some(ExpressionContext::MemberAccess)) {
        let before = type_result.items.len();
        type_result
            .items
            .retain(|item| item.kind.is_some_and(is_member_access_completion_kind));
        tracing::debug!(
            "completion: filtered type provider for MemberAccess context: {} -> {} items",
            before,
            type_result.items.len()
        );
    } else if let Some(prefix) = identifier_prefix {
        let before = type_result.items.len();
        type_result.items.retain(|item| {
            item.label.starts_with(prefix)
                && item.kind.is_some_and(is_identifier_prefix_completion_kind)
        });
        tracing::debug!(
            "completion: filtered type provider for IdentifierExpected prefix {:?}: {} -> {} items",
            prefix,
            before,
            type_result.items.len()
        );
    } else if enforce_verter_scope_allowlist
        && matches!(expr_context, Some(ExpressionContext::Unknown))
    {
        let allowlist: std::collections::HashSet<&str> = verter_items
            .map(|items| items.iter().map(|i| i.label.as_str()).collect())
            .unwrap_or_default();
        let before = type_result.items.len();
        type_result
            .items
            .retain(|item| allowlist.contains(item.label.as_str()));
        tracing::debug!(
            "completion: filtered type provider for Unknown context: {} -> {} items",
            before,
            type_result.items.len()
        );
    }
}

/// Collect the template locals visible at `cursor_offset` from the semantic
/// element tree. This complements script/render-proxy names when a TypeScript
/// provider owns completion output: v-for and v-slot bindings are genuine
/// generated lexical locals, but are not script bindings and therefore cannot
/// be recovered from the outer component scope.
pub(in crate::server) fn template_lexical_scope_names(
    template: &verter_semantic::analysis::template::TemplateAnalysisSnapshot,
    cursor_offset: u32,
) -> std::collections::HashSet<String> {
    let mut names = std::collections::HashSet::new();
    let Some(mut element_index) = template
        .elements
        .iter()
        .enumerate()
        .filter(|(_, element)| {
            element.span.start <= cursor_offset && cursor_offset <= element.span.end
        })
        .max_by_key(|(_, element)| element.nesting_depth)
        .map(|(index, _)| index)
    else {
        return names;
    };

    loop {
        let element = &template.elements[element_index];
        if let Some(v_for) = &element.v_for {
            names.insert(v_for.variable.clone());
            if let Some(index) = &v_for.index {
                names.insert(index.clone());
            }
        }
        for directive in &element.directives {
            if directive.name == "slot" {
                if let Some(pattern) = directive.expression.as_deref() {
                    names.extend(parse_template_binding_pattern_names(pattern));
                }
            }
        }

        let Some(parent_index) = element.parent_index else {
            break;
        };
        let Ok(parent_index) = usize::try_from(parent_index) else {
            break;
        };
        if parent_index >= template.elements.len() {
            break;
        }
        element_index = parent_index;
    }

    names
}

fn parse_template_binding_pattern_names(pattern: &str) -> std::collections::HashSet<String> {
    let allocator = oxc_allocator::Allocator::new();
    let wrapped = format!("({pattern}) => {{}}");
    let Ok(expression) = oxc_parser::Parser::new(&allocator, &wrapped, oxc_span::SourceType::tsx())
        .parse_expression()
    else {
        return std::collections::HashSet::new();
    };
    let oxc_ast::ast::Expression::ArrowFunctionExpression(arrow) = expression else {
        return std::collections::HashSet::new();
    };

    let mut names = std::collections::HashSet::new();
    for parameter in &arrow.params.items {
        collect_template_binding_pattern_names(&parameter.pattern, &mut names);
    }
    names
}

fn collect_template_binding_pattern_names(
    pattern: &oxc_ast::ast::BindingPattern<'_>,
    names: &mut std::collections::HashSet<String>,
) {
    use oxc_ast::ast::BindingPattern;
    match pattern {
        BindingPattern::BindingIdentifier(identifier) => {
            names.insert(identifier.name.to_string());
        }
        BindingPattern::ObjectPattern(object) => {
            for property in &object.properties {
                collect_template_binding_pattern_names(&property.value, names);
            }
            if let Some(rest) = &object.rest {
                collect_template_binding_pattern_names(&rest.argument, names);
            }
        }
        BindingPattern::ArrayPattern(array) => {
            for element in array.elements.iter().flatten() {
                collect_template_binding_pattern_names(element, names);
            }
            if let Some(rest) = &array.rest {
                collect_template_binding_pattern_names(&rest.argument, names);
            }
        }
        BindingPattern::AssignmentPattern(assignment) => {
            collect_template_binding_pattern_names(&assignment.left, names);
        }
    }
}
