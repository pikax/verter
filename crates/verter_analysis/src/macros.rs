use oxc_ast::ast::*;

use crate::types::{AnalyzedMacro, AnalyzedMacroKind};

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
        TSType::TSTypeQuery(_) => {}
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

            Some(AnalyzedMacro {
                kind,
                is_type_based,
                type_references,
                binding_name,
            })
        }
        _ => None,
    }
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

    /// @ai-generated - typeof X in type refs returns empty (not tracked)
    #[test]
    fn typeof_in_type_refs_returns_empty() {
        // TSTypeQuery (typeof) is intentionally not tracked — returns empty
        let refs = parse_type_refs("typeof X");
        assert!(
            refs.is_empty(),
            "typeof type references should not be collected, got: {:?}",
            refs
        );
    }
}
