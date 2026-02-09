//! v-for expression binding extraction.
//!
//! This module handles extraction of bindings from Vue v-for expressions.

use oxc_ast::ast::{ArrayExpressionElement, Expression, ObjectPropertyKind, PropertyKey};
use rustc_hash::FxHashSet;

use super::helpers::{collect_expression_references, collect_ts_type_references_from_expression};
use super::types::ParameterBindingsResult;
use crate::utils::oxc::vue::VForParseResult;

/// Extract locals (iteration variables) from a v-for left expression.
///
/// Handles various patterns:
/// - `item` → Identifier → ["item"]
/// - `{ id, name }` → ObjectExpression → ["id", "name"]
/// - `[a, b]` → ArrayExpression → ["a", "b"]
/// - `(item, index)` → ParenthesizedExpression(SequenceExpression) → ["item", "index"]
fn collect_vfor_left_locals(expr: &Expression<'_>, locals: &mut Vec<String>) {
    match expr {
        Expression::Identifier(ident) => {
            locals.push(ident.name.to_string());
        }
        Expression::ObjectExpression(obj) => {
            for prop in &obj.properties {
                match prop {
                    ObjectPropertyKind::ObjectProperty(p) => {
                        // For shorthand: { foo } → foo is both key and value
                        // For non-shorthand: { foo: bar } → bar is the binding
                        if p.shorthand {
                            if let PropertyKey::StaticIdentifier(ident) = &p.key {
                                locals.push(ident.name.to_string());
                            }
                        } else {
                            collect_vfor_left_locals(&p.value, locals);
                        }
                    }
                    ObjectPropertyKind::SpreadProperty(spread) => {
                        collect_vfor_left_locals(&spread.argument, locals);
                    }
                }
            }
        }
        Expression::ArrayExpression(arr) => {
            for elem in &arr.elements {
                match elem {
                    ArrayExpressionElement::SpreadElement(spread) => {
                        collect_vfor_left_locals(&spread.argument, locals);
                    }
                    ArrayExpressionElement::Elision(_) => {}
                    _ => {
                        if let Some(e) = elem.as_expression() {
                            collect_vfor_left_locals(e, locals);
                        }
                    }
                }
            }
        }
        Expression::ParenthesizedExpression(paren) => {
            collect_vfor_left_locals(&paren.expression, locals);
        }
        Expression::SequenceExpression(seq) => {
            for e in &seq.expressions {
                collect_vfor_left_locals(e, locals);
            }
        }
        Expression::AssignmentExpression(assign) => {
            // Handle default values like `item = defaultValue`
            use oxc_ast::ast::{AssignmentTarget, SimpleAssignmentTarget};
            if let AssignmentTarget::AssignmentTargetIdentifier(id) = &assign.left {
                locals.push(id.name.to_string());
            } else if let Some(SimpleAssignmentTarget::AssignmentTargetIdentifier(id)) =
                assign.left.as_simple_assignment_target()
            {
                locals.push(id.name.to_string());
            }
        }
        _ => {}
    }
}

/// Extract bindings from a parsed v-for expression.
///
/// For input like `item of items`:
/// - locals: `["item"]` (the iteration variable)
/// - references: `["items"]` (the iterable being referenced)
///
/// For input like `{ id, name } of items`:
/// - locals: `["id", "name"]`
/// - references: `["items"]`
///
/// # Arguments
/// * `result` - The parsed VForParseResult containing left and right expressions
///
/// # Example
/// ```ignore
/// let allocator = Allocator::default();
/// let parse_result = parse_vfor(&allocator, "item of items", SourceType::tsx());
/// let bindings = extract_vfor_bindings(&parse_result);
/// assert_eq!(bindings.locals, vec!["item"]);
/// assert_eq!(bindings.references, vec!["items"]);
/// ```
pub fn extract_vfor_bindings(result: &VForParseResult<'_>) -> ParameterBindingsResult {
    // Check for parse errors
    if result.has_left_errors() || result.has_right_errors() {
        return ParameterBindingsResult {
            has_errors: true,
            ..Default::default()
        };
    }

    let mut locals = Vec::new();
    let mut references_set = FxHashSet::default();

    // Extract locals from the left side (iteration variables)
    if let Some(left) = &result.left {
        collect_vfor_left_locals(left, &mut locals);
    }

    // Build ignored set from locals
    let ignored: FxHashSet<&[u8]> = locals.iter().map(|s| s.as_bytes()).collect();

    // Extract references from the right side (the iterable)
    if let Some(right) = &result.right {
        collect_expression_references(right, &ignored, &mut references_set);
        // Also extract TypeScript type references from type assertions
        collect_ts_type_references_from_expression(right, &mut references_set);
    }

    let references: Vec<String> = references_set.into_iter().map(String::from).collect();

    ParameterBindingsResult {
        locals,
        references,
        has_errors: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::utils::oxc::vue::parse_vfor;
    use oxc_allocator::Allocator;
    use oxc_span::SourceType;

    fn parse(content: &str) -> ParameterBindingsResult {
        let allocator = Box::leak(Box::new(Allocator::default()));
        let parse_result = parse_vfor(allocator, content, SourceType::tsx());
        extract_vfor_bindings(&parse_result)
    }

    // ===========================================
    // Basic v-for expressions
    // ===========================================

    #[test]
    fn test_simple_of() {
        let result = parse("item of items");
        assert!(!result.has_errors);
        assert_eq!(result.locals, vec!["item"]);
        assert_eq!(result.references, vec!["items"]);
    }

    #[test]
    fn test_simple_in() {
        let result = parse("item in items");
        assert!(!result.has_errors);
        assert_eq!(result.locals, vec!["item"]);
        assert_eq!(result.references, vec!["items"]);
    }

    // ===========================================
    // Destructuring patterns
    // ===========================================

    #[test]
    fn test_object_destructuring() {
        let result = parse("{ id, name } of items");
        assert!(!result.has_errors);
        assert_eq!(result.locals.len(), 2);
        assert!(result.locals.contains(&"id".to_string()));
        assert!(result.locals.contains(&"name".to_string()));
        assert_eq!(result.references, vec!["items"]);
    }

    #[test]
    fn test_renamed_destructuring() {
        let result = parse("{ id: itemId } of items");
        assert!(!result.has_errors);
        assert_eq!(result.locals, vec!["itemId"]);
        assert_eq!(result.references, vec!["items"]);
    }

    #[test]
    fn test_nested_destructuring() {
        let result = parse("{ user: { name } } of items");
        assert!(!result.has_errors);
        assert_eq!(result.locals, vec!["name"]);
        assert_eq!(result.references, vec!["items"]);
    }

    #[test]
    fn test_array_destructuring() {
        let result = parse("[first, second] of items");
        assert!(!result.has_errors);
        assert_eq!(result.locals.len(), 2);
        assert!(result.locals.contains(&"first".to_string()));
        assert!(result.locals.contains(&"second".to_string()));
        assert_eq!(result.references, vec!["items"]);
    }

    // ===========================================
    // Complex iterables
    // ===========================================

    #[test]
    fn test_member_expression_iterable() {
        let result = parse("item of data.items");
        assert!(!result.has_errors);
        assert_eq!(result.locals, vec!["item"]);
        assert_eq!(result.references, vec!["data"]);
    }

    #[test]
    fn test_function_call_iterable() {
        let result = parse("item of getItems()");
        assert!(!result.has_errors);
        assert_eq!(result.locals, vec!["item"]);
        assert_eq!(result.references, vec!["getItems"]);
    }

    #[test]
    fn test_computed_property_iterable() {
        let result = parse("item of data[key]");
        assert!(!result.has_errors);
        assert_eq!(result.locals, vec!["item"]);
        assert_eq!(result.references.len(), 2);
        assert!(result.references.contains(&"data".to_string()));
        assert!(result.references.contains(&"key".to_string()));
    }

    // ===========================================
    // TypeScript support
    // ===========================================

    #[test]
    fn test_type_assertion() {
        let result = parse("item of (items as Item[])");
        assert!(!result.has_errors);
        assert_eq!(result.locals, vec!["item"]);
        assert!(result.references.contains(&"items".to_string()));
        assert!(result.references.contains(&"Item".to_string()));
    }

    #[test]
    fn test_generic_type_assertion() {
        let result = parse("item of (items as Array<Item>)");
        assert!(!result.has_errors);
        assert_eq!(result.locals, vec!["item"]);
        assert!(result.references.contains(&"items".to_string()));
        assert!(result.references.contains(&"Array".to_string()));
        assert!(result.references.contains(&"Item".to_string()));
    }

    // ===========================================
    // Edge cases
    // ===========================================

    #[test]
    fn test_empty_input() {
        let result = parse("");
        // Empty input now returns has_errors because parse_vfor returns no left/right
        assert!(result.locals.is_empty());
        assert!(result.references.is_empty());
    }

    #[test]
    fn test_whitespace_only() {
        // Whitespace without " in " or " of " is invalid
        let result = parse("   ");
        assert!(result.has_errors);
    }

    #[test]
    fn test_no_separator() {
        let result = parse("item items");
        assert!(result.has_errors);
    }

    #[test]
    fn test_invalid_syntax() {
        let result = parse("{ invalid: } of items");
        assert!(result.has_errors);
    }

    // ===========================================
    // Multi-variable syntax (Vue-specific)
    // ===========================================

    #[test]
    fn test_parenthesized_with_index() {
        let result = parse("(item, index) of items");
        assert!(!result.has_errors);
        assert_eq!(result.locals.len(), 2);
        assert!(result.locals.contains(&"item".to_string()));
        assert!(result.locals.contains(&"index".to_string()));
        assert_eq!(result.references, vec!["items"]);
    }

    #[test]
    fn test_parenthesized_with_key_index() {
        let result = parse("(value, key, index) in obj");
        assert!(!result.has_errors);
        assert_eq!(result.locals.len(), 3);
        assert!(result.locals.contains(&"value".to_string()));
        assert!(result.locals.contains(&"key".to_string()));
        assert!(result.locals.contains(&"index".to_string()));
        assert_eq!(result.references, vec!["obj"]);
    }
}
