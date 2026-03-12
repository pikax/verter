//! Options API extraction for `verter_analysis`.
//!
//! Extracts structured information from `export default { ... }` and
//! `export default defineComponent({ ... })` for cross-component type resolution.

use oxc_ast::ast::*;
use verter_span::Span;

use crate::types::{
    AnalyzedEmitField, AnalyzedOptionsApi, AnalyzedOptionsComponent, AnalyzedOptionsField,
    AnalyzedOptionsProp,
};

#[cfg(test)]
#[path = "options_tests.rs"]
mod options_tests;

/// Try to extract Options API analysis from an `export default` expression.
///
/// Handles:
/// - `export default { ... }` (bare object)
/// - `export default defineComponent({ ... })` (wrapped)
///
/// `import_sources` maps local binding names to their import source strings.
///
/// Returns `None` if the expression is not an options object.
pub(crate) fn try_extract_options_from_expression(
    expr: &Expression<'_>,
    source: &str,
    import_sources: &rustc_hash::FxHashMap<String, String>,
) -> Option<AnalyzedOptionsApi> {
    match expr {
        Expression::ObjectExpression(obj) => {
            Some(extract_options_api(obj, source, false, import_sources))
        }
        Expression::CallExpression(call) => {
            let is_define_component = match &call.callee {
                Expression::Identifier(id) => is_define_component_name(&id.name),
                _ => false,
            };
            if !is_define_component {
                return None;
            }
            let first_arg = call.arguments.first()?.as_expression()?;
            if let Expression::ObjectExpression(obj) = first_arg {
                Some(extract_options_api(obj, source, true, import_sources))
            } else {
                None
            }
        }
        _ => None,
    }
}

fn is_define_component_name(name: &str) -> bool {
    name == "defineComponent"
}

/// Extract full Options API analysis from an object expression.
fn extract_options_api(
    obj: &ObjectExpression<'_>,
    _source: &str,
    is_define_component: bool,
    import_sources: &rustc_hash::FxHashMap<String, String>,
) -> AnalyzedOptionsApi {
    let mut result = AnalyzedOptionsApi {
        is_define_component,
        object_span: Span::from(obj.span),
        ..Default::default()
    };

    for prop in &obj.properties {
        let ObjectPropertyKind::ObjectProperty(p) = prop else {
            continue;
        };
        let key_name = match &p.key {
            PropertyKey::StaticIdentifier(id) => Some(id.name.as_str()),
            PropertyKey::StringLiteral(s) => Some(s.value.as_str()),
            _ => None,
        };
        let Some(key) = key_name else { continue };

        match key {
            "props" => result.props = extract_options_props(&p.value),
            "emits" => result.emits = extract_options_emits(&p.value),
            "data" => result.data_fields = extract_data_fields(&p.value),
            "computed" => result.computed_fields = extract_object_keys(&p.value),
            "methods" => result.methods = extract_object_keys(&p.value),
            "expose" => result.expose = extract_string_array_as_fields(&p.value),
            "provide" => result.provide_keys = extract_provide_keys(&p.value),
            "inject" => result.inject_keys = extract_inject_keys(&p.value),
            "components" => result.components = extract_components(&p.value, import_sources),
            "inheritAttrs" => {
                result.has_inherit_attrs_false = is_false_literal(&p.value);
            }
            _ => {}
        }
    }

    result
}

// ── Props ──

fn extract_options_props(value: &Expression<'_>) -> Vec<AnalyzedOptionsProp> {
    match value {
        // props: ['foo', 'bar']
        Expression::ArrayExpression(arr) => arr
            .elements
            .iter()
            .filter_map(|elem| {
                if let ArrayExpressionElement::StringLiteral(s) = elem {
                    Some(AnalyzedOptionsProp {
                        name: s.value.to_string(),
                        span: Span::from(s.span),
                        type_constructor: None,
                        is_required: false,
                        has_default: false,
                    })
                } else {
                    None
                }
            })
            .collect(),

        // props: { foo: String, bar: { type: Number, required: true } }
        Expression::ObjectExpression(obj) => obj
            .properties
            .iter()
            .filter_map(|prop| {
                let ObjectPropertyKind::ObjectProperty(p) = prop else {
                    return None;
                };
                let name = static_key_name(&p.key)?;
                let span = key_span(&p.key);

                match &p.value {
                    // Shorthand: `foo: String`
                    Expression::Identifier(id) => Some(AnalyzedOptionsProp {
                        name,
                        span,
                        type_constructor: Some(id.name.to_string()),
                        is_required: false,
                        has_default: false,
                    }),
                    // Full object: `foo: { type: String, required: true, default: 'x' }`
                    Expression::ObjectExpression(sub_obj) => {
                        let mut type_constructor = None;
                        let mut is_required = false;
                        let mut has_default = false;

                        for sub_prop in &sub_obj.properties {
                            let ObjectPropertyKind::ObjectProperty(sp) = sub_prop else {
                                continue;
                            };
                            let Some(sub_key) = static_key_name(&sp.key) else {
                                continue;
                            };
                            match sub_key.as_str() {
                                "type" => {
                                    if let Expression::Identifier(id) = &sp.value {
                                        type_constructor = Some(id.name.to_string());
                                    }
                                }
                                "required" => {
                                    is_required = is_true_literal(&sp.value);
                                }
                                "default" => {
                                    has_default = true;
                                }
                                _ => {}
                            }
                        }

                        Some(AnalyzedOptionsProp {
                            name,
                            span,
                            type_constructor,
                            is_required,
                            has_default,
                        })
                    }
                    // Array form for multiple types: `foo: [String, Number]` — treat as no single constructor
                    Expression::ArrayExpression(_) => Some(AnalyzedOptionsProp {
                        name,
                        span,
                        type_constructor: None,
                        is_required: false,
                        has_default: false,
                    }),
                    _ => Some(AnalyzedOptionsProp {
                        name,
                        span,
                        type_constructor: None,
                        is_required: false,
                        has_default: false,
                    }),
                }
            })
            .collect(),

        _ => Vec::new(),
    }
}

// ── Emits ──

fn extract_options_emits(value: &Expression<'_>) -> Vec<AnalyzedEmitField> {
    match value {
        // emits: ['click', 'update']
        Expression::ArrayExpression(arr) => arr
            .elements
            .iter()
            .filter_map(|elem| {
                if let ArrayExpressionElement::StringLiteral(s) = elem {
                    Some(AnalyzedEmitField {
                        name: s.value.to_string(),
                        span: Span::from(s.span),
                    })
                } else {
                    None
                }
            })
            .collect(),

        // emits: { click: null, update: (val) => true }
        Expression::ObjectExpression(obj) => obj
            .properties
            .iter()
            .filter_map(|prop| {
                let ObjectPropertyKind::ObjectProperty(p) = prop else {
                    return None;
                };
                let name = static_key_name(&p.key)?;
                Some(AnalyzedEmitField {
                    name,
                    span: key_span(&p.key),
                })
            })
            .collect(),

        _ => Vec::new(),
    }
}

// ── Data ──

fn extract_data_fields(value: &Expression<'_>) -> Vec<AnalyzedOptionsField> {
    match value {
        // data() { return { ... } }
        Expression::FunctionExpression(func) => {
            if let Some(body) = &func.body {
                extract_return_object_keys(&body.statements)
            } else {
                Vec::new()
            }
        }
        // data: () => ({ ... })
        Expression::ArrowFunctionExpression(arrow) => {
            if arrow.expression {
                // Expression body: data: () => ({ count: 0 })
                for stmt in &arrow.body.statements {
                    if let Statement::ExpressionStatement(es) = stmt {
                        let inner = unwrap_parens(&es.expression);
                        if let Expression::ObjectExpression(obj) = inner {
                            return extract_fields_from_object(obj);
                        }
                    }
                }
                Vec::new()
            } else {
                extract_return_object_keys(&arrow.body.statements)
            }
        }
        _ => Vec::new(),
    }
}

fn extract_return_object_keys(stmts: &[Statement<'_>]) -> Vec<AnalyzedOptionsField> {
    for stmt in stmts {
        if let Statement::ReturnStatement(ret) = stmt {
            if let Some(expr) = &ret.argument {
                let inner = unwrap_parens(expr);
                if let Expression::ObjectExpression(obj) = inner {
                    return extract_fields_from_object(obj);
                }
            }
        }
    }
    Vec::new()
}

// ── Object key extractors ──

fn extract_object_keys(value: &Expression<'_>) -> Vec<AnalyzedOptionsField> {
    if let Expression::ObjectExpression(obj) = value {
        extract_fields_from_object(obj)
    } else {
        Vec::new()
    }
}

fn extract_fields_from_object(obj: &ObjectExpression<'_>) -> Vec<AnalyzedOptionsField> {
    obj.properties
        .iter()
        .filter_map(|prop| {
            let ObjectPropertyKind::ObjectProperty(p) = prop else {
                return None;
            };
            let name = static_key_name(&p.key)?;
            Some(AnalyzedOptionsField {
                name,
                span: key_span(&p.key),
            })
        })
        .collect()
}

// ── String array → fields ──

fn extract_string_array_as_fields(value: &Expression<'_>) -> Vec<AnalyzedOptionsField> {
    if let Expression::ArrayExpression(arr) = value {
        arr.elements
            .iter()
            .filter_map(|elem| {
                if let ArrayExpressionElement::StringLiteral(s) = elem {
                    Some(AnalyzedOptionsField {
                        name: s.value.to_string(),
                        span: Span::from(s.span),
                    })
                } else {
                    None
                }
            })
            .collect()
    } else {
        Vec::new()
    }
}

// ── Provide ──

fn extract_provide_keys(value: &Expression<'_>) -> Vec<AnalyzedOptionsField> {
    match value {
        // provide: { key: value }
        Expression::ObjectExpression(obj) => extract_fields_from_object(obj),
        // provide() { return { ... } }
        Expression::FunctionExpression(func) => {
            if let Some(body) = &func.body {
                extract_return_object_keys(&body.statements)
            } else {
                Vec::new()
            }
        }
        // provide: () => ({ ... })
        Expression::ArrowFunctionExpression(arrow) => {
            if arrow.expression {
                for stmt in &arrow.body.statements {
                    if let Statement::ExpressionStatement(es) = stmt {
                        let inner = unwrap_parens(&es.expression);
                        if let Expression::ObjectExpression(obj) = inner {
                            return extract_fields_from_object(obj);
                        }
                    }
                }
                Vec::new()
            } else {
                extract_return_object_keys(&arrow.body.statements)
            }
        }
        _ => Vec::new(),
    }
}

// ── Inject ──

fn extract_inject_keys(value: &Expression<'_>) -> Vec<AnalyzedOptionsField> {
    match value {
        // inject: ['key1', 'key2']
        Expression::ArrayExpression(arr) => arr
            .elements
            .iter()
            .filter_map(|elem| {
                if let ArrayExpressionElement::StringLiteral(s) = elem {
                    Some(AnalyzedOptionsField {
                        name: s.value.to_string(),
                        span: Span::from(s.span),
                    })
                } else {
                    None
                }
            })
            .collect(),
        // inject: { key: { from: '...', default: ... } }
        Expression::ObjectExpression(obj) => extract_fields_from_object(obj),
        _ => Vec::new(),
    }
}

// ── Components ──

fn extract_components(
    value: &Expression<'_>,
    import_sources: &rustc_hash::FxHashMap<String, String>,
) -> Vec<AnalyzedOptionsComponent> {
    let Expression::ObjectExpression(obj) = value else {
        return Vec::new();
    };

    obj.properties
        .iter()
        .filter_map(|prop| {
            let ObjectPropertyKind::ObjectProperty(p) = prop else {
                return None;
            };
            let name = static_key_name(&p.key)?;
            let span = key_span(&p.key);

            // Resolve import source from the value binding
            let import_source = if p.shorthand {
                // { MyComp } — shorthand, value name == key name
                import_sources.get(&name).cloned()
            } else if let Expression::Identifier(id) = &p.value {
                // { Alias: MyComp } — explicit value
                import_sources.get(id.name.as_str()).cloned()
            } else {
                None
            };

            Some(AnalyzedOptionsComponent {
                name,
                span,
                import_source,
            })
        })
        .collect()
}

// ── Helpers ──

fn static_key_name(key: &PropertyKey<'_>) -> Option<String> {
    match key {
        PropertyKey::StaticIdentifier(id) => Some(id.name.to_string()),
        PropertyKey::StringLiteral(s) => Some(s.value.to_string()),
        _ => None,
    }
}

fn key_span(key: &PropertyKey<'_>) -> Span {
    match key {
        PropertyKey::StaticIdentifier(id) => Span::from(id.span),
        PropertyKey::StringLiteral(s) => Span::from(s.span),
        _ => Span::default(),
    }
}

fn is_false_literal(expr: &Expression<'_>) -> bool {
    matches!(expr, Expression::BooleanLiteral(b) if !b.value)
}

fn is_true_literal(expr: &Expression<'_>) -> bool {
    matches!(expr, Expression::BooleanLiteral(b) if b.value)
}

fn unwrap_parens<'a>(expr: &'a Expression<'a>) -> &'a Expression<'a> {
    match expr {
        Expression::ParenthesizedExpression(p) => unwrap_parens(&p.expression),
        _ => expr,
    }
}
