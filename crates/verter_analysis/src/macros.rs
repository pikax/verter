use oxc_ast::ast::*;
use oxc_span::GetSpan;

use crate::types::{AnalyzedEmitField, AnalyzedMacro, AnalyzedMacroKind, AnalyzedPropField};

/// Classify a callee name as a Vue compiler macro.
fn classify_macro(name: &str) -> Option<AnalyzedMacroKind> {
    match name {
        "defineProps" => Some(AnalyzedMacroKind::DefineProps),
        "defineEmits" => Some(AnalyzedMacroKind::DefineEmits),
        "defineModel" => Some(AnalyzedMacroKind::DefineModel),
        "defineExpose" => Some(AnalyzedMacroKind::DefineExpose),
        "defineOptions" => Some(AnalyzedMacroKind::DefineOptions),
        "defineSlots" => Some(AnalyzedMacroKind::DefineSlots),
        "withDefaults" => Some(AnalyzedMacroKind::WithDefaults),
        _ => None,
    }
}

/// Collect all type reference names from a TypeScript type annotation.
///
/// Walks recursively through the type AST and collects every `TSTypeReference`
/// identifier name. This includes both user-defined types and built-in generics
/// like `Partial`, `Required`, etc.
///
/// # Examples
/// - `{foo: MyType}` → `["MyType"]`
/// - `MyType` → `["MyType"]`
/// - `MyType & OtherType` → `["MyType", "OtherType"]`
/// - `{foo: string, bar: number}` → `[]`
/// - `Partial<MyType>` → `["Partial", "MyType"]`
pub fn collect_type_references(ts_type: &TSType<'_>) -> Vec<String> {
    let mut refs = Vec::new();
    collect_type_references_recursive(ts_type, &mut refs);
    refs
}

fn collect_type_references_recursive(ts_type: &TSType<'_>, refs: &mut Vec<String>) {
    match ts_type {
        TSType::TSTypeReference(type_ref) => {
            // Collect the type name
            match &type_ref.type_name {
                TSTypeName::IdentifierReference(id) => {
                    refs.push(id.name.to_string());
                }
                TSTypeName::QualifiedName(qualified) => {
                    collect_qualified_name_root(qualified, refs);
                }
                _ => {}
            }
            // Also recurse into type arguments: e.g., Partial<MyType>
            if let Some(params) = &type_ref.type_arguments {
                for param in &params.params {
                    collect_type_references_recursive(param, refs);
                }
            }
        }
        TSType::TSUnionType(union_type) => {
            for ty in &union_type.types {
                collect_type_references_recursive(ty, refs);
            }
        }
        TSType::TSIntersectionType(intersection) => {
            for ty in &intersection.types {
                collect_type_references_recursive(ty, refs);
            }
        }
        TSType::TSTypeLiteral(literal) => {
            for member in &literal.members {
                match member {
                    TSSignature::TSPropertySignature(prop) => {
                        if let Some(ref ta) = prop.type_annotation {
                            collect_type_references_recursive(&ta.type_annotation, refs);
                        }
                    }
                    TSSignature::TSMethodSignature(method) => {
                        if let Some(ref ret) = method.return_type {
                            collect_type_references_recursive(&ret.type_annotation, refs);
                        }
                        // In OXC 0.112, type annotations are on FormalParameter, not BindingPattern
                        for param in &method.params.items {
                            if let Some(ref ta) = param.type_annotation {
                                collect_type_references_recursive(&ta.type_annotation, refs);
                            }
                        }
                    }
                    TSSignature::TSCallSignatureDeclaration(call) => {
                        if let Some(ref ret) = call.return_type {
                            collect_type_references_recursive(&ret.type_annotation, refs);
                        }
                        for param in &call.params.items {
                            if let Some(ref ta) = param.type_annotation {
                                collect_type_references_recursive(&ta.type_annotation, refs);
                            }
                        }
                    }
                    TSSignature::TSIndexSignature(idx) => {
                        collect_type_references_recursive(
                            &idx.type_annotation.type_annotation,
                            refs,
                        );
                    }
                    TSSignature::TSConstructSignatureDeclaration(_) => {}
                }
            }
        }
        TSType::TSArrayType(arr) => {
            collect_type_references_recursive(&arr.element_type, refs);
        }
        TSType::TSTupleType(tuple) => {
            for elem in &tuple.element_types {
                // TSTupleElement can be converted to TSType via to_ts_type()
                collect_type_references_recursive(elem.to_ts_type(), refs);
            }
        }
        TSType::TSConditionalType(cond) => {
            collect_type_references_recursive(&cond.check_type, refs);
            collect_type_references_recursive(&cond.extends_type, refs);
            collect_type_references_recursive(&cond.true_type, refs);
            collect_type_references_recursive(&cond.false_type, refs);
        }
        TSType::TSMappedType(mapped) => {
            // In OXC 0.112, constraint is directly on TSMappedType (not optional)
            collect_type_references_recursive(&mapped.constraint, refs);
            if let Some(ref type_annotation) = mapped.type_annotation {
                collect_type_references_recursive(type_annotation, refs);
            }
        }
        TSType::TSIndexedAccessType(idx) => {
            collect_type_references_recursive(&idx.object_type, refs);
            collect_type_references_recursive(&idx.index_type, refs);
        }
        TSType::TSTypeOperatorType(op) => {
            collect_type_references_recursive(&op.type_annotation, refs);
        }
        TSType::TSParenthesizedType(paren) => {
            collect_type_references_recursive(&paren.type_annotation, refs);
        }
        TSType::TSTemplateLiteralType(tpl) => {
            for ty in &tpl.types {
                collect_type_references_recursive(ty, refs);
            }
        }
        TSType::TSFunctionType(func) => {
            // return_type is Box<TSTypeAnnotation>, not optional
            collect_type_references_recursive(&func.return_type.type_annotation, refs);
            for param in &func.params.items {
                if let Some(ref ta) = param.type_annotation {
                    collect_type_references_recursive(&ta.type_annotation, refs);
                }
            }
        }
        TSType::TSConstructorType(ctor) => {
            // return_type is Box<TSTypeAnnotation>, not optional
            collect_type_references_recursive(&ctor.return_type.type_annotation, refs);
        }
        TSType::TSInferType(_) => {}
        TSType::TSTypeQuery(query) => {
            if let TSTypeQueryExprName::IdentifierReference(ident) = &query.expr_name {
                refs.push(ident.name.to_string());
            }
        }
        TSType::TSImportType(_) => {}
        // Primitives and literals — no type references
        _ => {}
    }
}

fn collect_qualified_name_root(name: &TSQualifiedName<'_>, refs: &mut Vec<String>) {
    match &name.left {
        TSTypeName::IdentifierReference(id) => {
            refs.push(id.name.to_string());
        }
        TSTypeName::QualifiedName(inner) => {
            collect_qualified_name_root(inner, refs);
        }
        _ => {}
    }
}

/// Detect Vue macros from a parsed program's body.
/// Returns analyzed macros with type reference information.
#[cfg(test)]
fn analyze_macros_from_program(program: &Program<'_>) -> Vec<AnalyzedMacro> {
    let mut macros = Vec::new();

    for stmt in &program.body {
        match stmt {
            Statement::ExpressionStatement(expr_stmt) => {
                try_extract_macro_from_expr(&expr_stmt.expression, &mut macros);
            }
            Statement::VariableDeclaration(var_decl) => {
                for decl in &var_decl.declarations {
                    try_extract_macro_from_var_decl(decl, &mut macros);
                }
            }
            _ => {}
        }
    }

    macros
}

/// Try to extract macros from an expression statement.
/// Called per-statement from the single-pass AST walk.
pub(crate) fn try_extract_macro_from_expr(
    expression: &Expression<'_>,
    macros: &mut Vec<AnalyzedMacro>,
) {
    if let Some(m) = try_extract_macro(expression, None) {
        if m.kind == AnalyzedMacroKind::WithDefaults {
            try_extract_inner_macro(expression, macros);
        }
        macros.push(m);
    }
}

/// Try to extract macros from a variable declarator.
/// Called per-declaration from the single-pass AST walk.
pub(crate) fn try_extract_macro_from_var_decl(
    decl: &VariableDeclarator<'_>,
    macros: &mut Vec<AnalyzedMacro>,
) {
    if let Some(ref init) = decl.init {
        let binding_name = if let BindingPattern::BindingIdentifier(id) = &decl.id {
            Some(id.name.to_string())
        } else {
            None
        };
        if let Some(m) = try_extract_macro(init, binding_name) {
            if m.kind == AnalyzedMacroKind::WithDefaults {
                try_extract_inner_macro(init, macros);
            }
            macros.push(m);
        }
    }
}

/// For `withDefaults(defineProps<...>(), {...})`, extract the inner macro
/// (e.g. `defineProps`) from the first argument.
fn try_extract_inner_macro(expr: &Expression<'_>, macros: &mut Vec<AnalyzedMacro>) {
    if let Expression::CallExpression(call) = expr {
        if let Some(first_arg) = call.arguments.first() {
            if let Some(inner_expr) = first_arg.as_expression() {
                if let Some(m) = try_extract_macro(inner_expr, None) {
                    macros.push(m);
                }
            }
        }
    }
}

/// Try to extract a macro call from an expression.
fn try_extract_macro(expr: &Expression<'_>, binding_name: Option<String>) -> Option<AnalyzedMacro> {
    match expr {
        Expression::CallExpression(call) => {
            let callee_name = match &call.callee {
                Expression::Identifier(id) => Some(id.name.as_str()),
                _ => None,
            }?;

            let kind = classify_macro(callee_name)?;

            // In OXC 0.112, type parameters on call expressions are `.type_arguments`
            let (is_type_based, type_references) = if let Some(ref type_args) = call.type_arguments
            {
                if let Some(first) = type_args.params.first() {
                    (true, collect_type_references(first))
                } else {
                    (true, Vec::new())
                }
            } else {
                (false, Vec::new())
            };

            // Extract model name from defineModel('name') first string argument
            let model_name = if kind == AnalyzedMacroKind::DefineModel {
                call.arguments.first().and_then(|arg| {
                    if let Some(Expression::StringLiteral(lit)) = arg.as_expression() {
                        Some(lit.value.to_string())
                    } else {
                        None
                    }
                })
            } else {
                None
            };

            // Detect defineOptions({ inheritAttrs: false })
            let has_inherit_attrs_false =
                kind == AnalyzedMacroKind::DefineOptions && has_inherit_attrs_false_in_args(call);

            let prop_fields = if kind == AnalyzedMacroKind::DefineProps {
                extract_prop_fields(call)
            } else {
                Vec::new()
            };

            let emit_fields = if kind == AnalyzedMacroKind::DefineEmits {
                extract_emit_fields(call)
            } else {
                Vec::new()
            };

            Some(AnalyzedMacro {
                kind,
                is_type_based,
                type_references,
                binding_name,
                model_name,
                has_inherit_attrs_false,
                prop_fields,
                emit_fields,
                span: call.span.into(),
            })
        }
        _ => None,
    }
}

/// Extract individual prop field names and spans from a `defineProps` call.
///
/// Handles:
/// - Type-based: `defineProps<{ count: number, name: string }>()`
/// - Runtime object: `defineProps({ count: { type: Number }, name: String })`
/// - Runtime array: `defineProps(['count', 'name'])`
fn extract_prop_fields(call: &CallExpression<'_>) -> Vec<AnalyzedPropField> {
    // Type-based: extract from type parameters
    if let Some(ref type_args) = call.type_arguments {
        if let Some(first) = type_args.params.first() {
            return extract_prop_fields_from_type(first);
        }
    }

    // Runtime: extract from first argument
    if let Some(first_arg) = call.arguments.first() {
        if let Some(expr) = first_arg.as_expression() {
            return extract_prop_fields_from_runtime(expr);
        }
    }

    Vec::new()
}

/// Extract prop fields from a TypeScript type parameter (e.g., `{ count: number }`).
fn extract_prop_fields_from_type(ts_type: &TSType<'_>) -> Vec<AnalyzedPropField> {
    match ts_type {
        TSType::TSTypeLiteral(literal) => literal
            .members
            .iter()
            .filter_map(|member| {
                if let TSSignature::TSPropertySignature(prop) = member {
                    let key_name = match &prop.key {
                        PropertyKey::StaticIdentifier(id) => Some(id.name.to_string()),
                        PropertyKey::StringLiteral(lit) => Some(lit.value.to_string()),
                        _ => None,
                    };
                    key_name.map(|name| AnalyzedPropField {
                        name,
                        span: prop.key.span().into(),
                    })
                } else {
                    None
                }
            })
            .collect(),
        TSType::TSTypeReference(_) => {
            // Interface reference — can't resolve inline, leave empty
            Vec::new()
        }
        TSType::TSIntersectionType(intersection) => {
            // Merge fields from all branches
            intersection
                .types
                .iter()
                .flat_map(|t| extract_prop_fields_from_type(t))
                .collect()
        }
        _ => Vec::new(),
    }
}

/// Extract prop fields from a runtime argument (object or array).
fn extract_prop_fields_from_runtime(expr: &Expression<'_>) -> Vec<AnalyzedPropField> {
    match expr {
        Expression::ObjectExpression(obj) => obj
            .properties
            .iter()
            .filter_map(|prop| {
                if let ObjectPropertyKind::ObjectProperty(p) = prop {
                    let key_name = match &p.key {
                        PropertyKey::StaticIdentifier(id) => Some(id.name.to_string()),
                        PropertyKey::StringLiteral(lit) => Some(lit.value.to_string()),
                        _ => None,
                    };
                    key_name.map(|name| AnalyzedPropField {
                        name,
                        span: p.key.span().into(),
                    })
                } else {
                    None
                }
            })
            .collect(),
        Expression::ArrayExpression(arr) => arr
            .elements
            .iter()
            .filter_map(|elem| {
                if let ArrayExpressionElement::StringLiteral(lit) = elem {
                    Some(AnalyzedPropField {
                        name: lit.value.to_string(),
                        span: lit.span.into(),
                    })
                } else {
                    None
                }
            })
            .collect(),
        _ => Vec::new(),
    }
}

/// Extract individual emit field names and spans from a `defineEmits` call.
///
/// Handles:
/// - Type-based property-signature: `defineEmits<{ custom: [payload: string]; click: [] }>()`
/// - Type-based call-signature: `defineEmits<{ (e: 'change', id: number): void }>()`
/// - Runtime array: `defineEmits(['custom', 'click'])`
/// - Runtime object: `defineEmits({ custom: null })`
fn extract_emit_fields(call: &CallExpression<'_>) -> Vec<AnalyzedEmitField> {
    // Type-based: extract from type parameters
    if let Some(ref type_args) = call.type_arguments {
        if let Some(first) = type_args.params.first() {
            return extract_emit_fields_from_type(first);
        }
    }

    // Runtime: extract from first argument
    if let Some(first_arg) = call.arguments.first() {
        if let Some(expr) = first_arg.as_expression() {
            return extract_emit_fields_from_runtime(expr);
        }
    }

    Vec::new()
}

/// Extract emit fields from a TypeScript type parameter.
///
/// Handles two TSTypeLiteral member shapes:
/// 1. `TSPropertySignature` — key name is the event name (e.g., `custom: [payload: string]`)
/// 2. `TSCallSignatureDeclaration` — first param's string literal type is the event name
fn extract_emit_fields_from_type(ts_type: &TSType<'_>) -> Vec<AnalyzedEmitField> {
    match ts_type {
        TSType::TSTypeLiteral(literal) => literal
            .members
            .iter()
            .filter_map(|member| match member {
                // Property signature: `custom: [payload: string]`
                TSSignature::TSPropertySignature(prop) => {
                    let key_name = match &prop.key {
                        PropertyKey::StaticIdentifier(id) => Some(id.name.to_string()),
                        PropertyKey::StringLiteral(lit) => Some(lit.value.to_string()),
                        _ => None,
                    };
                    key_name.map(|name| AnalyzedEmitField {
                        name,
                        span: prop.key.span().into(),
                    })
                }
                // Call signature: `(e: 'change', id: number): void`
                TSSignature::TSCallSignatureDeclaration(call) => {
                    // First param should be string literal type: `e: 'change'`
                    let first_param = call.params.items.first()?;
                    let type_ann = first_param.type_annotation.as_ref()?;
                    if let TSType::TSLiteralType(lit) = &type_ann.type_annotation {
                        if let TSLiteral::StringLiteral(s) = &lit.literal {
                            return Some(AnalyzedEmitField {
                                name: s.value.to_string(),
                                span: s.span.into(),
                            });
                        }
                    }
                    None
                }
                _ => None,
            })
            .collect(),
        TSType::TSTypeReference(_) => Vec::new(),
        TSType::TSIntersectionType(intersection) => intersection
            .types
            .iter()
            .flat_map(|t| extract_emit_fields_from_type(t))
            .collect(),
        _ => Vec::new(),
    }
}

/// Extract emit fields from a runtime argument (object keys or array string elements).
fn extract_emit_fields_from_runtime(expr: &Expression<'_>) -> Vec<AnalyzedEmitField> {
    match expr {
        Expression::ObjectExpression(obj) => obj
            .properties
            .iter()
            .filter_map(|prop| {
                if let ObjectPropertyKind::ObjectProperty(p) = prop {
                    let key_name = match &p.key {
                        PropertyKey::StaticIdentifier(id) => Some(id.name.to_string()),
                        PropertyKey::StringLiteral(lit) => Some(lit.value.to_string()),
                        _ => None,
                    };
                    key_name.map(|name| AnalyzedEmitField {
                        name,
                        span: p.key.span().into(),
                    })
                } else {
                    None
                }
            })
            .collect(),
        Expression::ArrayExpression(arr) => arr
            .elements
            .iter()
            .filter_map(|elem| {
                if let ArrayExpressionElement::StringLiteral(lit) = elem {
                    Some(AnalyzedEmitField {
                        name: lit.value.to_string(),
                        span: lit.span.into(),
                    })
                } else {
                    None
                }
            })
            .collect(),
        _ => Vec::new(),
    }
}

/// Check if a `defineOptions()` call has `inheritAttrs: false` in its first object argument.
fn has_inherit_attrs_false_in_args(call: &CallExpression<'_>) -> bool {
    let Some(first_arg) = call.arguments.first() else {
        return false;
    };
    let Some(Expression::ObjectExpression(obj)) = first_arg.as_expression() else {
        return false;
    };
    for prop in &obj.properties {
        if let ObjectPropertyKind::ObjectProperty(p) = prop {
            let is_inherit_attrs = match &p.key {
                PropertyKey::StaticIdentifier(id) => id.name == "inheritAttrs",
                PropertyKey::StringLiteral(lit) => lit.value == "inheritAttrs",
                _ => false,
            };
            if is_inherit_attrs {
                if let Expression::BooleanLiteral(b) = &p.value {
                    return !b.value; // inheritAttrs: false
                }
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use oxc_allocator::Allocator;
    use oxc_parser::{ParseOptions, Parser};
    use oxc_span::SourceType;

    use super::*;

    fn parse_type_refs(type_annotation: &str) -> Vec<String> {
        // Parse as a type annotation inside a variable declaration
        let code = format!("let _x: {type_annotation};");
        let alloc = Allocator::new();
        let parser =
            Parser::new(&alloc, &code, SourceType::ts()).with_options(ParseOptions::default());
        let result = parser.parse();
        assert!(!result.panicked, "failed to parse: {}", code);

        // In OXC 0.112, type annotations are on VariableDeclarator, not BindingPattern
        for stmt in &result.program.body {
            if let Statement::VariableDeclaration(var_decl) = stmt {
                if let Some(decl) = var_decl.declarations.first() {
                    if let Some(ref ta) = decl.type_annotation {
                        return collect_type_references(&ta.type_annotation);
                    }
                }
            }
        }
        panic!("could not find type annotation in parsed code");
    }

    /// @ai-generated - Object literal with type reference
    #[test]
    fn object_with_type_ref() {
        let refs = parse_type_refs("{foo: MyType}");
        assert_eq!(refs, vec!["MyType"]);
    }

    /// @ai-generated - Simple type reference
    #[test]
    fn simple_type_ref() {
        let refs = parse_type_refs("MyType");
        assert_eq!(refs, vec!["MyType"]);
    }

    /// @ai-generated - Intersection type
    #[test]
    fn intersection_type() {
        let refs = parse_type_refs("MyType & OtherType");
        assert_eq!(refs, vec!["MyType", "OtherType"]);
    }

    /// @ai-generated - Only primitives, no type references
    #[test]
    fn only_primitives() {
        let refs = parse_type_refs("{foo: string, bar: number}");
        assert!(refs.is_empty());
    }

    /// @ai-generated - Generic type with nested type reference
    #[test]
    fn generic_with_nested_ref() {
        let refs = parse_type_refs("Partial<MyType>");
        assert_eq!(refs, vec!["Partial", "MyType"]);
    }

    /// @ai-generated - Union type
    #[test]
    fn union_type() {
        let refs = parse_type_refs("MyType | OtherType");
        assert_eq!(refs, vec!["MyType", "OtherType"]);
    }

    /// @ai-generated - Array of type reference
    #[test]
    fn array_type() {
        let refs = parse_type_refs("MyType[]");
        assert_eq!(refs, vec!["MyType"]);
    }

    /// @ai-generated - Nested object with type refs
    #[test]
    fn nested_object() {
        let refs = parse_type_refs("{foo: {bar: MyType}, baz: OtherType}");
        assert_eq!(refs, vec!["MyType", "OtherType"]);
    }

    /// @ai-generated - Conditional type
    #[test]
    fn conditional_type() {
        let refs = parse_type_refs("A extends B ? C : D");
        assert_eq!(refs, vec!["A", "B", "C", "D"]);
    }

    /// @ai-generated - Mapped type
    #[test]
    fn mapped_type() {
        let refs = parse_type_refs("{[K in keyof T]: V}");
        assert_eq!(refs, vec!["T", "V"]);
    }

    /// @ai-generated - Indexed access type
    #[test]
    fn indexed_access() {
        let refs = parse_type_refs("T[K]");
        assert_eq!(refs, vec!["T", "K"]);
    }

    fn parse_macros(code: &str) -> Vec<AnalyzedMacro> {
        let alloc = Allocator::new();
        let parser =
            Parser::new(&alloc, code, SourceType::ts()).with_options(ParseOptions::default());
        let result = parser.parse();
        assert!(!result.panicked, "failed to parse: {}", code);
        analyze_macros_from_program(&result.program)
    }

    /// @ai-generated - Detect defineProps with type param
    #[test]
    fn detect_define_props_type_based() {
        let macros = parse_macros("const props = defineProps<{foo: MyType}>()");
        assert_eq!(macros.len(), 1);
        assert_eq!(macros[0].kind, AnalyzedMacroKind::DefineProps);
        assert!(macros[0].is_type_based);
        assert_eq!(macros[0].type_references, vec!["MyType"]);
        assert_eq!(macros[0].binding_name.as_deref(), Some("props"));
    }

    /// @ai-generated - Detect defineProps without type param
    #[test]
    fn detect_define_props_runtime() {
        let macros = parse_macros("const props = defineProps({foo: String})");
        assert_eq!(macros.len(), 1);
        assert_eq!(macros[0].kind, AnalyzedMacroKind::DefineProps);
        assert!(!macros[0].is_type_based);
        assert!(macros[0].type_references.is_empty());
    }

    /// @ai-generated - Detect defineEmits
    #[test]
    fn detect_define_emits() {
        let macros = parse_macros("const emit = defineEmits<{(e: 'click'): void}>()");
        assert_eq!(macros.len(), 1);
        assert_eq!(macros[0].kind, AnalyzedMacroKind::DefineEmits);
        assert!(macros[0].is_type_based);
    }

    /// @ai-generated - Detect defineModel
    #[test]
    fn detect_define_model() {
        let macros = parse_macros("const model = defineModel<string>()");
        assert_eq!(macros.len(), 1);
        assert_eq!(macros[0].kind, AnalyzedMacroKind::DefineModel);
        assert!(macros[0].is_type_based);
    }

    /// @ai-generated - Bare macro call (no binding)
    #[test]
    fn bare_macro_call() {
        let macros = parse_macros("defineExpose({})");
        assert_eq!(macros.len(), 1);
        assert_eq!(macros[0].kind, AnalyzedMacroKind::DefineExpose);
        assert!(macros[0].binding_name.is_none());
    }

    /// @ai-generated - Imported type reference in defineProps
    #[test]
    fn imported_type_in_define_props() {
        let macros = parse_macros("defineProps<MyImportedType>()");
        assert_eq!(macros.len(), 1);
        assert_eq!(macros[0].type_references, vec!["MyImportedType"]);
    }

    /// @ai-generated - Multiple macros
    #[test]
    fn multiple_macros() {
        let code = r#"
const props = defineProps<{foo: string}>()
const emit = defineEmits<{(e: 'click'): void}>()
defineExpose({ props })
"#;
        let macros = parse_macros(code);
        assert_eq!(macros.len(), 3);
    }

    /// @ai-generated - withDefaults wrapping defineProps extracts both macros
    #[test]
    fn with_defaults_extracts_inner_define_props() {
        let code = r#"const props = withDefaults(defineProps<{foo: MyType}>(), { foo: 'bar' })"#;
        let macros = parse_macros(code);
        assert!(
            macros.len() >= 2,
            "should extract both withDefaults and defineProps, got {}",
            macros.len()
        );
        assert!(
            macros
                .iter()
                .any(|m| m.kind == AnalyzedMacroKind::WithDefaults),
            "should have withDefaults"
        );
        assert!(
            macros
                .iter()
                .any(|m| m.kind == AnalyzedMacroKind::DefineProps),
            "should have defineProps"
        );
        // The inner defineProps should capture type references
        let define_props = macros
            .iter()
            .find(|m| m.kind == AnalyzedMacroKind::DefineProps)
            .unwrap();
        assert!(
            define_props.is_type_based,
            "inner defineProps should be type-based"
        );
        assert!(
            define_props.type_references.contains(&"MyType".to_string()),
            "inner defineProps should capture type references"
        );
    }

    /// @ai-generated - import("./foo").Bar in type refs returns empty (not tracked)
    #[test]
    fn import_type_in_type_refs_returns_empty() {
        // TSImportType is intentionally not tracked — returns empty
        let refs = parse_type_refs("import('./foo').Bar");
        assert!(
            refs.is_empty(),
            "import() type references should not be collected, got: {:?}",
            refs
        );
    }

    /// @ai-generated - typeof X in type refs extracts the identifier
    #[test]
    fn typeof_in_type_refs_extracts_identifier() {
        // TSTypeQuery (typeof) should collect the referenced identifier
        let refs = parse_type_refs("typeof X");
        assert_eq!(refs, vec!["X".to_string()]);
    }

    // =========================================================================
    // Prop field extraction tests
    // =========================================================================

    #[test]
    fn prop_fields_type_based_literal() {
        let code = "defineProps<{ count: number, name: string }>()";
        let macros = parse_macros(code);
        assert_eq!(macros.len(), 1);
        let fields = &macros[0].prop_fields;
        assert_eq!(
            fields.len(),
            2,
            "should extract 2 prop fields: {:?}",
            fields
        );
        assert_eq!(fields[0].name, "count");
        assert_eq!(fields[1].name, "name");
        // Verify spans point to prop keys
        assert_eq!(
            &code[fields[0].span.start as usize..fields[0].span.end as usize],
            "count"
        );
        assert_eq!(
            &code[fields[1].span.start as usize..fields[1].span.end as usize],
            "name"
        );
    }

    #[test]
    fn prop_fields_type_based_with_assignment() {
        let code = "const props = defineProps<{ msg: string }>()";
        let macros = parse_macros(code);
        let dp = macros
            .iter()
            .find(|m| m.kind == AnalyzedMacroKind::DefineProps)
            .unwrap();
        assert_eq!(dp.prop_fields.len(), 1);
        assert_eq!(dp.prop_fields[0].name, "msg");
        assert_eq!(
            &code[dp.prop_fields[0].span.start as usize..dp.prop_fields[0].span.end as usize],
            "msg"
        );
    }

    #[test]
    fn prop_fields_runtime_object() {
        let code = "defineProps({ count: { type: Number }, name: String })";
        let macros = parse_macros(code);
        assert_eq!(macros.len(), 1);
        let fields = &macros[0].prop_fields;
        assert_eq!(
            fields.len(),
            2,
            "should extract 2 runtime prop fields: {:?}",
            fields
        );
        assert_eq!(fields[0].name, "count");
        assert_eq!(fields[1].name, "name");
        assert_eq!(
            &code[fields[0].span.start as usize..fields[0].span.end as usize],
            "count"
        );
        assert_eq!(
            &code[fields[1].span.start as usize..fields[1].span.end as usize],
            "name"
        );
    }

    #[test]
    fn prop_fields_runtime_array() {
        let code = "defineProps(['count', 'name'])";
        let macros = parse_macros(code);
        assert_eq!(macros.len(), 1);
        let fields = &macros[0].prop_fields;
        assert_eq!(
            fields.len(),
            2,
            "should extract 2 array prop fields: {:?}",
            fields
        );
        assert_eq!(fields[0].name, "count");
        assert_eq!(fields[1].name, "name");
    }

    #[test]
    fn prop_fields_with_defaults() {
        let code = "withDefaults(defineProps<{ msg: string, count: number }>(), { msg: 'hi' })";
        let macros = parse_macros(code);
        let dp = macros
            .iter()
            .find(|m| m.kind == AnalyzedMacroKind::DefineProps)
            .unwrap();
        assert_eq!(
            dp.prop_fields.len(),
            2,
            "withDefaults inner defineProps should have prop fields: {:?}",
            dp.prop_fields
        );
        assert_eq!(dp.prop_fields[0].name, "msg");
        assert_eq!(dp.prop_fields[1].name, "count");
    }

    #[test]
    fn prop_fields_non_define_props_empty() {
        let code = "defineEmits<{(e: 'click'): void}>()";
        let macros = parse_macros(code);
        assert_eq!(macros.len(), 1);
        assert!(
            macros[0].prop_fields.is_empty(),
            "defineEmits should have no prop fields"
        );
    }

    #[test]
    fn prop_fields_type_reference_empty() {
        // Interface reference — can't resolve inline, prop_fields is empty
        let code = "defineProps<MyProps>()";
        let macros = parse_macros(code);
        assert_eq!(macros.len(), 1);
        assert!(
            macros[0].prop_fields.is_empty(),
            "type reference should yield empty prop fields"
        );
    }

    // =========================================================================
    // Emit field extraction tests
    // =========================================================================

    #[test]
    fn emit_fields_type_based_property_signature() {
        let code = "defineEmits<{ custom: [payload: string]; click: [] }>()";
        let macros = parse_macros(code);
        assert_eq!(macros.len(), 1);
        let fields = &macros[0].emit_fields;
        assert_eq!(
            fields.len(),
            2,
            "should extract 2 emit fields: {:?}",
            fields
        );
        assert_eq!(fields[0].name, "custom");
        assert_eq!(fields[1].name, "click");
        // Verify spans point to event name keys
        assert_eq!(
            &code[fields[0].span.start as usize..fields[0].span.end as usize],
            "custom"
        );
        assert_eq!(
            &code[fields[1].span.start as usize..fields[1].span.end as usize],
            "click"
        );
    }

    #[test]
    fn emit_fields_type_based_call_signature() {
        let code = "defineEmits<{ (e: 'change', id: number): void }>()";
        let macros = parse_macros(code);
        assert_eq!(macros.len(), 1);
        let fields = &macros[0].emit_fields;
        assert_eq!(
            fields.len(),
            1,
            "should extract 1 emit field from call signature: {:?}",
            fields
        );
        assert_eq!(fields[0].name, "change");
    }

    #[test]
    fn emit_fields_type_based_mixed_signatures() {
        let code = "defineEmits<{ (e: 'change', id: number): void; custom: [payload: string] }>()";
        let macros = parse_macros(code);
        assert_eq!(macros.len(), 1);
        let fields = &macros[0].emit_fields;
        assert_eq!(
            fields.len(),
            2,
            "should extract 2 emit fields from mixed signatures: {:?}",
            fields
        );
        assert_eq!(fields[0].name, "change");
        assert_eq!(fields[1].name, "custom");
    }

    #[test]
    fn emit_fields_runtime_array() {
        let code = "defineEmits(['custom', 'click'])";
        let macros = parse_macros(code);
        assert_eq!(macros.len(), 1);
        let fields = &macros[0].emit_fields;
        assert_eq!(
            fields.len(),
            2,
            "should extract 2 runtime array emit fields: {:?}",
            fields
        );
        assert_eq!(fields[0].name, "custom");
        assert_eq!(fields[1].name, "click");
    }

    #[test]
    fn emit_fields_runtime_object() {
        let code = "defineEmits({ custom: null })";
        let macros = parse_macros(code);
        assert_eq!(macros.len(), 1);
        let fields = &macros[0].emit_fields;
        assert_eq!(
            fields.len(),
            1,
            "should extract 1 runtime object emit field: {:?}",
            fields
        );
        assert_eq!(fields[0].name, "custom");
    }

    #[test]
    fn emit_fields_non_define_emits_empty() {
        let code = "defineProps<{ count: number }>()";
        let macros = parse_macros(code);
        assert_eq!(macros.len(), 1);
        assert!(
            macros[0].emit_fields.is_empty(),
            "defineProps should have no emit fields"
        );
    }

    #[test]
    fn emit_fields_type_reference_empty() {
        // Interface reference — can't resolve inline, emit_fields is empty
        let code = "defineEmits<MyEvents>()";
        let macros = parse_macros(code);
        assert_eq!(macros.len(), 1);
        assert!(
            macros[0].emit_fields.is_empty(),
            "type reference should yield empty emit fields"
        );
    }

    #[test]
    fn prop_fields_intersection_type() {
        let code = "defineProps<{ a: string } & { b: number }>()";
        let macros = parse_macros(code);
        assert_eq!(macros.len(), 1);
        let fields = &macros[0].prop_fields;
        assert_eq!(
            fields.len(),
            2,
            "intersection should merge fields: {:?}",
            fields
        );
        assert_eq!(fields[0].name, "a");
        assert_eq!(fields[1].name, "b");
    }
}
