//! Slot expression binding extraction.
//!
//! This module handles extraction of bindings from Vue slot expressions,
//! which use function parameter syntax.

use oxc_ast::ast::FormalParameters;
use rustc_hash::FxHashSet;

use super::helpers::{
    collect_expression_references, collect_pattern_locals, collect_pattern_references,
    collect_type_references,
};
use super::types::ParameterBindingsResult;
use crate::utils::oxc::vue::VSlotParseResult;

/// Extract bindings from parsed slot parameters (FormalParameters).
///
/// For input like `{ rowData: role }: { rowData: ProjectRole }`:
/// - locals: `["role"]` (the declared parameter binding)
/// - references: `["ProjectRole"]` (external identifiers from type annotations)
///
/// For input like `{ item, index = defaultIdx }`:
/// - locals: `["item", "index"]`
/// - references: `["defaultIdx"]` (from the default value)
///
/// # Arguments
/// * `result` - The parsed VSlotParseResult containing FormalParameters
///
/// # Example
/// ```ignore
/// let allocator = Allocator::default();
/// let parse_result = parse_vslot(&allocator, "{ data }", SourceType::tsx());
/// let bindings = extract_slot_bindings(&parse_result);
/// assert_eq!(bindings.locals, vec!["data"]);
/// assert!(bindings.references.is_empty());
/// ```
pub fn extract_slot_bindings(result: &VSlotParseResult<'_>) -> ParameterBindingsResult {
    // Check for parse errors
    if result.errors.is_some() {
        return ParameterBindingsResult {
            has_errors: true,
            ..Default::default()
        };
    }

    // Handle case where params is None (empty slot)
    let params = match &result.params {
        Some(p) => p,
        None => return ParameterBindingsResult::default(),
    };

    extract_bindings_from_formal_parameters(params)
}

/// Extract bindings directly from FormalParameters.
///
/// This is useful when you already have parsed FormalParameters
/// and don't need to go through VSlotParseResult.
pub fn extract_bindings_from_formal_parameters(
    params: &FormalParameters<'_>,
) -> ParameterBindingsResult {
    let mut locals = Vec::new();
    let mut references_set = FxHashSet::default();

    // Extract locals from parameters
    for param in &params.items {
        collect_pattern_locals(&param.pattern, &mut locals);
    }
    if let Some(rest) = &params.rest {
        collect_pattern_locals(&rest.rest.argument, &mut locals);
    }

    // Build ignored set from locals (to exclude them from references)
    let ignored: FxHashSet<&[u8]> = locals.iter().map(|s| s.as_bytes()).collect();

    // Extract references from type annotations and default values
    for param in &params.items {
        // Default value (initializer)
        if let Some(init) = &param.initializer {
            collect_expression_references(init, &ignored, &mut references_set);
        }
        // Type annotation on the parameter (on FormalParameter, not BindingPattern)
        if let Some(annotation) = &param.type_annotation {
            collect_type_references(&annotation.type_annotation, &mut references_set);
        }
        // References in default values within the pattern
        collect_pattern_references(&param.pattern, &ignored, &mut references_set);
    }
    if let Some(rest) = &params.rest {
        if let Some(annotation) = &rest.type_annotation {
            collect_type_references(&annotation.type_annotation, &mut references_set);
        }
    }

    // Convert &str to String for the result struct
    let locals: Vec<String> = locals.into_iter().map(String::from).collect();
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
    use crate::utils::oxc::vue::parse_vslot;
    use oxc_allocator::Allocator;
    use oxc_span::SourceType;

    fn parse(content: &str) -> ParameterBindingsResult {
        let allocator = Box::leak(Box::new(Allocator::default()));
        let parse_result = parse_vslot(allocator, content, SourceType::tsx());
        extract_slot_bindings(&parse_result)
    }

    #[test]
    fn test_simple_slot() {
        let result = parse("data");
        assert!(!result.has_errors);
        assert_eq!(result.locals, vec!["data"]);
        assert!(result.references.is_empty());
    }

    #[test]
    fn test_destructured_slot() {
        let result = parse("{ item, index }");
        assert!(!result.has_errors);
        assert_eq!(result.locals.len(), 2);
        assert!(result.locals.contains(&"item".to_string()));
        assert!(result.locals.contains(&"index".to_string()));
        assert!(result.references.is_empty());
    }

    #[test]
    fn test_renamed_destructuring() {
        let result = parse("{ rowData: role }");
        assert!(!result.has_errors);
        assert_eq!(result.locals, vec!["role"]);
        assert!(result.references.is_empty());
    }

    #[test]
    fn test_with_default_value() {
        let result = parse("{ item = defaultItem }");
        assert!(!result.has_errors);
        assert_eq!(result.locals, vec!["item"]);
        assert_eq!(result.references, vec!["defaultItem"]);
    }

    #[test]
    fn test_with_type_annotation() {
        let result = parse("{ data }: { data: MyType }");
        assert!(!result.has_errors);
        assert_eq!(result.locals, vec!["data"]);
        assert_eq!(result.references, vec!["MyType"]);
    }

    #[test]
    fn test_complex_slot() {
        let result = parse("{ rowData: role }: { rowData: ProjectRole }");
        assert!(!result.has_errors);
        assert_eq!(result.locals, vec!["role"]);
        assert_eq!(result.references, vec!["ProjectRole"]);
    }

    #[test]
    fn test_multiple_params() {
        let result = parse("item, index, extra");
        assert!(!result.has_errors);
        assert_eq!(result.locals.len(), 3);
        assert!(result.locals.contains(&"item".to_string()));
        assert!(result.locals.contains(&"index".to_string()));
        assert!(result.locals.contains(&"extra".to_string()));
        assert!(result.references.is_empty());
    }

    #[test]
    fn test_rest_parameter() {
        let result = parse("first, ...rest");
        assert!(!result.has_errors);
        assert_eq!(result.locals.len(), 2);
        assert!(result.locals.contains(&"first".to_string()));
        assert!(result.locals.contains(&"rest".to_string()));
        assert!(result.references.is_empty());
    }

    #[test]
    fn test_nested_destructuring() {
        let result = parse("{ user: { name, id } }");
        assert!(!result.has_errors);
        assert_eq!(result.locals.len(), 2);
        assert!(result.locals.contains(&"name".to_string()));
        assert!(result.locals.contains(&"id".to_string()));
        assert!(result.references.is_empty());
    }

    #[test]
    fn test_array_destructuring() {
        let result = parse("[first, second]");
        assert!(!result.has_errors);
        assert_eq!(result.locals.len(), 2);
        assert!(result.locals.contains(&"first".to_string()));
        assert!(result.locals.contains(&"second".to_string()));
        assert!(result.references.is_empty());
    }

    #[test]
    fn test_empty_slot() {
        let result = parse("");
        assert!(!result.has_errors);
        assert!(result.locals.is_empty());
        assert!(result.references.is_empty());
    }

    #[test]
    fn test_whitespace_only() {
        let result = parse("   ");
        assert!(!result.has_errors);
        assert!(result.locals.is_empty());
        assert!(result.references.is_empty());
    }

    #[test]
    fn test_invalid_syntax() {
        let result = parse("{ invalid: }");
        assert!(result.has_errors);
    }

    #[test]
    fn test_complex_type_annotation() {
        let result = parse("data: Array<Item>");
        assert!(!result.has_errors);
        assert_eq!(result.locals, vec!["data"]);
        assert_eq!(result.references.len(), 2);
        assert!(result.references.contains(&"Array".to_string()));
        assert!(result.references.contains(&"Item".to_string()));
    }

    #[test]
    fn test_union_type_annotation() {
        let result = parse("data: string | MyType");
        assert!(!result.has_errors);
        assert_eq!(result.locals, vec!["data"]);
        assert_eq!(result.references, vec!["MyType"]);
    }

    #[test]
    fn test_default_with_function_call() {
        let result = parse("data = getData()");
        assert!(!result.has_errors);
        assert_eq!(result.locals, vec!["data"]);
        assert_eq!(result.references, vec!["getData"]);
    }

    #[test]
    fn test_default_with_member_expression() {
        let result = parse("data = config.default");
        assert!(!result.has_errors);
        assert_eq!(result.locals, vec!["data"]);
        assert_eq!(result.references, vec!["config"]);
    }
}
