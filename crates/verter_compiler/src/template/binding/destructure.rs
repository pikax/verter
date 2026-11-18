//! Shared leaf accessors for parsed loop and slot destructuring patterns.

use oxc_ast::ast::{
    ArrayExpressionElement, AssignmentTarget, BindingPattern, Expression, ObjectPropertyKind,
    PropertyKey,
};

/// A leaf's access relative to the target's loop-item or slot-props base.
pub(crate) enum DestructureAccessor {
    Path(String),
    Rest {
        suffix: String,
        excluded_keys: Vec<String>,
    },
    Default {
        suffix: String,
        resolved_default_expr: String,
    },
}

impl DestructureAccessor {
    pub(crate) fn render(&self, base: &str) -> String {
        match self {
            Self::Path(suffix) => format!("{base}{suffix}"),
            Self::Rest {
                suffix,
                excluded_keys,
            } => {
                let mut keys = String::with_capacity(excluded_keys.len() * 8);
                for (i, key) in excluded_keys.iter().enumerate() {
                    if i > 0 {
                        keys.push_str(", ");
                    }
                    keys.push('"');
                    crate::template::vue_syntax::escape_js_string_into(&mut keys, key);
                    keys.push('"');
                }
                format!("_getRestElement({base}{suffix}, [{keys}])")
            }
            Self::Default {
                suffix,
                resolved_default_expr,
            } => {
                format!("_getDefaultValue({base}{suffix}, () => ({resolved_default_expr}))")
            }
        }
    }
}

enum Pending<'n, 'a> {
    Expression(&'n Expression<'a>, String),
    Binding(&'n BindingPattern<'a>, String),
    Leaf(String, DestructureAccessor),
    Unsupported,
}

/// Collect a parsed loop-value pattern. Unsupported computed keys, array rest,
/// and nested rest/default targets retain the caller's raw-parameter fallback.
pub(crate) fn collect_destructure_paths<'a>(
    expression: &Expression<'a>,
    resolve_default: impl FnMut(&Expression<'a>) -> String,
) -> Option<Vec<(String, DestructureAccessor)>> {
    collect(
        Pending::Expression(expression, String::new()),
        resolve_default,
    )
}

/// Collect a parsed slot binding with the same eligibility and leaf order.
pub(crate) fn collect_binding_destructure_paths<'a>(
    pattern: &BindingPattern<'a>,
    resolve_default: impl FnMut(&Expression<'a>) -> String,
) -> Option<Vec<(String, DestructureAccessor)>> {
    collect(Pending::Binding(pattern, String::new()), resolve_default)
}

fn property_path(path: &str, key: &PropertyKey<'_>, computed: bool) -> Option<(String, String)> {
    if computed {
        return None;
    }
    let mut child = String::with_capacity(path.len() + 8);
    child.push_str(path);
    let name = match key {
        PropertyKey::StaticIdentifier(name) => {
            child.push('.');
            child.push_str(name.name.as_str());
            name.name.as_str()
        }
        PropertyKey::StringLiteral(value) => {
            child.push_str("[\"");
            crate::template::vue_syntax::escape_js_string_into(&mut child, value.value.as_str());
            child.push_str("\"]");
            value.value.as_str()
        }
        _ => return None,
    };
    Some((child, name.to_string()))
}

fn collect<'n, 'a>(
    root: Pending<'n, 'a>,
    mut resolve_default: impl FnMut(&Expression<'a>) -> String,
) -> Option<Vec<(String, DestructureAccessor)>> {
    let mut stack = vec![root];
    let mut out = Vec::new();
    while let Some(task) = stack.pop() {
        let children_start = stack.len();
        match task {
            Pending::Leaf(name, accessor) => out.push((name, accessor)),
            Pending::Unsupported => return None,
            Pending::Expression(expression, path) => match expression {
                Expression::Identifier(id) => {
                    out.push((id.name.to_string(), DestructureAccessor::Path(path)))
                }
                Expression::ParenthesizedExpression(inner) => {
                    stack.push(Pending::Expression(&inner.expression, path))
                }
                Expression::AssignmentExpression(assign) => {
                    let AssignmentTarget::AssignmentTargetIdentifier(id) = &assign.left else {
                        return None;
                    };
                    out.push((
                        id.name.to_string(),
                        DestructureAccessor::Default {
                            suffix: path,
                            resolved_default_expr: resolve_default(&assign.right),
                        },
                    ));
                }
                Expression::ObjectExpression(object) => {
                    let mut declared_keys = Vec::new();
                    for property in &object.properties {
                        match property {
                            ObjectPropertyKind::ObjectProperty(property) => {
                                let Some((child, key)) =
                                    property_path(&path, &property.key, property.computed)
                                else {
                                    stack.push(Pending::Unsupported);
                                    break;
                                };
                                declared_keys.push(key);
                                stack.push(Pending::Expression(&property.value, child));
                            }
                            ObjectPropertyKind::SpreadProperty(spread) => {
                                let Expression::Identifier(id) = &spread.argument else {
                                    stack.push(Pending::Unsupported);
                                    break;
                                };
                                stack.push(Pending::Leaf(
                                    id.name.to_string(),
                                    DestructureAccessor::Rest {
                                        suffix: path.clone(),
                                        excluded_keys: declared_keys.clone(),
                                    },
                                ));
                            }
                        }
                    }
                }
                Expression::ArrayExpression(array) => {
                    for (index, element) in array.elements.iter().enumerate() {
                        match element {
                            ArrayExpressionElement::Elision(_) => {}
                            ArrayExpressionElement::SpreadElement(_) => {
                                stack.push(Pending::Unsupported);
                                break;
                            }
                            _ => {
                                let Some(expression) = element.as_expression() else {
                                    stack.push(Pending::Unsupported);
                                    break;
                                };
                                stack.push(Pending::Expression(
                                    expression,
                                    format!("{path}[{index}]"),
                                ));
                            }
                        }
                    }
                }
                _ => return None,
            },
            Pending::Binding(pattern, path) => match pattern {
                BindingPattern::BindingIdentifier(id) => {
                    out.push((id.name.to_string(), DestructureAccessor::Path(path)))
                }
                BindingPattern::AssignmentPattern(assign) => {
                    let BindingPattern::BindingIdentifier(id) = &assign.left else {
                        return None;
                    };
                    out.push((
                        id.name.to_string(),
                        DestructureAccessor::Default {
                            suffix: path,
                            resolved_default_expr: resolve_default(&assign.right),
                        },
                    ));
                }
                BindingPattern::ObjectPattern(object) => {
                    let mut declared_keys = Vec::new();
                    let mut supported = true;
                    for property in &object.properties {
                        let Some((child, key)) =
                            property_path(&path, &property.key, property.computed)
                        else {
                            stack.push(Pending::Unsupported);
                            supported = false;
                            break;
                        };
                        declared_keys.push(key);
                        stack.push(Pending::Binding(&property.value, child));
                    }
                    if supported {
                        if let Some(rest) = &object.rest {
                            if let BindingPattern::BindingIdentifier(id) = &rest.argument {
                                stack.push(Pending::Leaf(
                                    id.name.to_string(),
                                    DestructureAccessor::Rest {
                                        suffix: path,
                                        excluded_keys: declared_keys,
                                    },
                                ));
                            } else {
                                stack.push(Pending::Unsupported);
                            }
                        }
                    }
                }
                BindingPattern::ArrayPattern(array) => {
                    if array.rest.is_some() {
                        return None;
                    }
                    for (index, element) in array.elements.iter().enumerate() {
                        if let Some(pattern) = element {
                            stack.push(Pending::Binding(pattern, format!("{path}[{index}]")));
                        }
                    }
                }
            },
        }
        // Source-order DFS keeps duplicate leaves and default resolution ordered.
        stack[children_start..].reverse();
    }
    Some(out)
}
