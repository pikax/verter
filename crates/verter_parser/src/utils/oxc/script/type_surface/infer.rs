//! Runtime-type inference from TypeScript type annotations.
//!
//! Infers the set of possible Vue runtime types (`String`, `Number`, ...) for a
//! given `TSType`. The mapping is the same one Vue's official `resolveType.ts`
//! uses for `defineProps<T>()` runtime emission, so the renderer's prop schema
//! reflects what TypeScript reports for `T`.
//!
//! The module also owns helpers used by the rest of the resolver to extract
//! reference names and surface a type alias's annotation:
//!
//! - [`extract_heritage_type_names`] enumerates names appearing on an
//!   `extends` clause for diagnostics and dependency analysis.
//! - [`get_type_reference_name`] flattens a possibly-qualified type name into
//!   the dot-separated string used as the cache key.
//! - [`resolve_value_declaration_type`] and [`infer_props_from_object_literal`]
//!   support the `typeof X` flow where `X` is a value declaration.
//!
//! The five-mode resolution kernel (Identity / Navigate / Shallow / Expanded /
//! Skeleton — see `/type-resolution`) is unchanged here: this is the cold-path
//! inference layer that the kernels fall back to when a structural shape is
//! not available.

use oxc_ast::ast::*;
use oxc_span::GetSpan;

use super::{
    resolve_type_elements_with_ctx_ref, ResolvedElements, ResolvedMemberVisibility, ResolvedProp,
    RuntimeType, TypeResolutionContext,
};

/// Infer runtime type(s) from a TypeScript type annotation.
///
/// Returns a list of possible runtime types. For union types,
/// this returns all possible types. For simple types, returns a single type.
pub fn infer_runtime_type(node: &TSType) -> Vec<RuntimeType> {
    match node {
        // Primitive types
        TSType::TSStringKeyword(_) => vec![RuntimeType::String],
        TSType::TSNumberKeyword(_) => vec![RuntimeType::Number],
        TSType::TSBooleanKeyword(_) => vec![RuntimeType::Boolean],
        TSType::TSObjectKeyword(_) => vec![RuntimeType::Object],
        TSType::TSSymbolKeyword(_) => vec![RuntimeType::Symbol],
        TSType::TSNullKeyword(_) => vec![RuntimeType::Null],
        TSType::TSUndefinedKeyword(_) => vec![RuntimeType::Unknown],
        TSType::TSVoidKeyword(_) => vec![RuntimeType::Unknown],
        TSType::TSAnyKeyword(_) => vec![RuntimeType::Unknown],
        TSType::TSUnknownKeyword(_) => vec![RuntimeType::Unknown],
        TSType::TSNeverKeyword(_) => vec![RuntimeType::Unknown],
        TSType::TSBigIntKeyword(_) => vec![RuntimeType::Number],

        // Literal types
        TSType::TSLiteralType(lit) => infer_literal_type(lit),

        // Object/interface types
        TSType::TSTypeLiteral(_) => vec![RuntimeType::Object],

        // Array types
        TSType::TSArrayType(_) | TSType::TSTupleType(_) => vec![RuntimeType::Array],

        // Function types
        TSType::TSFunctionType(_) | TSType::TSConstructorType(_) => vec![RuntimeType::Function],

        // Parenthesized: (Type)
        TSType::TSParenthesizedType(paren) => infer_runtime_type(&paren.type_annotation),

        // Union: Type1 | Type2
        TSType::TSUnionType(union) => {
            let mut types = Vec::new();
            for ty in &union.types {
                for t in infer_runtime_type(ty) {
                    if !types.contains(&t) {
                        types.push(t);
                    }
                }
            }
            types
        }

        // Intersection: Type1 & Type2 - typically results in Object
        TSType::TSIntersectionType(intersection) => {
            // For intersections, try to infer from all types
            let mut types = Vec::new();
            for ty in &intersection.types {
                for t in infer_runtime_type(ty) {
                    if t != RuntimeType::Unknown && !types.contains(&t) {
                        types.push(t);
                    }
                }
            }
            if types.is_empty() {
                vec![RuntimeType::Object]
            } else {
                types
            }
        }

        // Type reference: SomeType or SomeType<T>
        TSType::TSTypeReference(type_ref) => infer_type_reference(type_ref),

        // Conditional type: T extends U ? X : Y — union both branches
        TSType::TSConditionalType(cond) => {
            let mut types = infer_runtime_type(&cond.true_type);
            for t in infer_runtime_type(&cond.false_type) {
                if !types.contains(&t) {
                    types.push(t);
                }
            }
            if types.is_empty() {
                vec![RuntimeType::Unknown]
            } else {
                types
            }
        }

        // Mapped type: { [K in keyof T]: T[K] }
        TSType::TSMappedType(_) => vec![RuntimeType::Object],

        // Indexed access: T[K]
        TSType::TSIndexedAccessType(_) => vec![RuntimeType::Unknown],

        // Template literal type: `${string}`
        TSType::TSTemplateLiteralType(_) => vec![RuntimeType::String],

        // Type query: typeof x — in defineProps context, always refers to an object shape
        TSType::TSTypeQuery(_) => vec![RuntimeType::Object],

        // Import type: import("...").Type
        TSType::TSImportType(_) => vec![RuntimeType::Unknown],

        // Type operator: keyof T, readonly T, unique symbol
        TSType::TSTypeOperatorType(op) => {
            if matches!(op.operator, TSTypeOperatorOperator::Keyof) {
                // keyof usually results in string | number | symbol
                vec![
                    RuntimeType::String,
                    RuntimeType::Number,
                    RuntimeType::Symbol,
                ]
            } else {
                infer_runtime_type(&op.type_annotation)
            }
        }

        // Infer type: infer T
        TSType::TSInferType(_) => vec![RuntimeType::Unknown],

        // This type
        TSType::TSThisType(_) => vec![RuntimeType::Object],

        // Intrinsic keyword
        TSType::TSIntrinsicKeyword(_) => vec![RuntimeType::Unknown],

        // Catch-all for any new types
        _ => vec![RuntimeType::Unknown],
    }
}

/// Infer runtime type from a literal type.
pub(super) fn infer_literal_type(lit: &TSLiteralType) -> Vec<RuntimeType> {
    match &lit.literal {
        TSLiteral::StringLiteral(_) => vec![RuntimeType::String],
        TSLiteral::NumericLiteral(_) => vec![RuntimeType::Number],
        TSLiteral::BooleanLiteral(_) => vec![RuntimeType::Boolean],
        TSLiteral::BigIntLiteral(_) => vec![RuntimeType::Number],
        TSLiteral::TemplateLiteral(_) => vec![RuntimeType::String],
        TSLiteral::UnaryExpression(unary) => {
            // -1, +1, etc.
            match &unary.argument {
                Expression::NumericLiteral(_) | Expression::BigIntLiteral(_) => {
                    vec![RuntimeType::Number]
                }
                _ => vec![RuntimeType::Unknown],
            }
        }
    }
}

/// Infer runtime type from a type reference.
pub(super) fn infer_type_reference(type_ref: &TSTypeReference) -> Vec<RuntimeType> {
    let name = get_type_reference_name(&type_ref.type_name);

    match name.as_str() {
        // Built-in JavaScript types
        "Array" | "ReadonlyArray" => vec![RuntimeType::Array],
        "Function" => vec![RuntimeType::Function],
        "Object" => vec![RuntimeType::Object],
        "String" => vec![RuntimeType::String],
        "Number" => vec![RuntimeType::Number],
        "Boolean" => vec![RuntimeType::Boolean],
        "Symbol" => vec![RuntimeType::Symbol],

        // Built-in object types
        "Date" | "RegExp" | "Error" | "Map" | "Set" | "WeakMap" | "WeakSet" | "Promise" => {
            vec![RuntimeType::BuiltIn(name)]
        }

        // TypeScript utility types
        "Partial" | "Required" | "Readonly" | "Record" | "Pick" | "Omit" | "InstanceType" => {
            vec![RuntimeType::Object]
        }
        "Parameters" | "ConstructorParameters" => vec![RuntimeType::Array],
        "ReturnType" => vec![RuntimeType::Unknown],
        "Uppercase" | "Lowercase" | "Capitalize" | "Uncapitalize" => vec![RuntimeType::String],
        "NonNullable" => {
            // Try to infer from the type parameter
            if let Some(args) = &type_ref.type_arguments {
                if let Some(first) = args.params.first() {
                    return infer_runtime_type(first)
                        .into_iter()
                        .filter(|t| *t != RuntimeType::Null)
                        .collect();
                }
            }
            vec![RuntimeType::Unknown]
        }
        "Extract" => {
            // Extract<T, U> - returns U
            if let Some(args) = &type_ref.type_arguments {
                if let Some(second) = args.params.get(1) {
                    return infer_runtime_type(second);
                }
            }
            vec![RuntimeType::Unknown]
        }
        "Exclude" | "OmitThisParameter" => {
            // Exclude<T, U> - returns T without U
            if let Some(args) = &type_ref.type_arguments {
                if let Some(first) = args.params.first() {
                    return infer_runtime_type(first);
                }
            }
            vec![RuntimeType::Unknown]
        }

        // Unknown type reference - can't resolve without scope
        _ => vec![RuntimeType::Unknown],
    }
}

/// Extract type names from interface heritage/extends clauses.
pub(super) fn extract_heritage_type_names(extends: &[TSInterfaceHeritage]) -> Vec<String> {
    extends
        .iter()
        .filter_map(|heritage| match &heritage.expression {
            Expression::Identifier(id) => Some(id.name.to_string()),
            _ => None,
        })
        .collect()
}

/// Get the name from a type reference's type name.
///
/// For qualified names like `Namespace.Props`, returns the full path
/// (`"Namespace.Props"`) by recursively walking the left side.
pub(super) fn get_type_reference_name(type_name: &TSTypeName) -> String {
    match type_name {
        TSTypeName::IdentifierReference(id) => id.name.to_string(),
        TSTypeName::QualifiedName(qualified) => {
            let left = get_type_reference_name(&qualified.left);
            format!("{}.{}", left, qualified.right.name)
        }
        TSTypeName::ThisExpression(_) => "this".to_string(),
    }
}

/// Resolve a value declaration's type shape (for `typeof X` support).
///
/// Looks for variable declarations matching `type_name` in both exported and
/// non-exported positions. If the variable has a type annotation, resolves that.
/// Otherwise, if it has an object literal initializer, infers prop types from
/// the property values.
#[cfg_attr(feature = "hotpath", hotpath::measure)]
pub(super) fn resolve_value_declaration_type<'ctx, 'a: 'ctx>(
    type_name: &str,
    program: &'ctx Program<'a>,
    source_bytes: &[u8],
    base_offset: u32,
    ctx: &TypeResolutionContext<'ctx, 'a>,
) -> Option<ResolvedElements> {
    let name_bytes = type_name.as_bytes();

    for stmt in &program.body {
        // Check both `export const X` and plain `const X`
        let var_decl = match stmt {
            Statement::ExportNamedDeclaration(export) => match &export.declaration {
                Some(Declaration::VariableDeclaration(decl)) => Some(decl.as_ref()),
                _ => None,
            },
            Statement::VariableDeclaration(decl) => Some(decl.as_ref()),
            _ => None,
        };

        if let Some(decl) = var_decl {
            for declarator in &decl.declarations {
                let BindingPattern::BindingIdentifier(id) = &declarator.id else {
                    continue;
                };
                if id.name.as_bytes() != name_bytes {
                    continue;
                }

                // 1. Type annotation on the declarator: `const X: { foo: string } = ...`
                //
                // Members reached through this `typeof X` path are NOT
                // the macro T's own body — the user wrote `typeof X`,
                // not the prop names. The sub-resolution therefore
                // enters with `from_root_body = false`.
                if let Some(ref annotation) = declarator.type_annotation {
                    return Some(resolve_type_elements_with_ctx_ref(
                        &annotation.type_annotation,
                        base_offset,
                        ctx,
                        false,
                    ));
                }

                // 2. Object literal initializer: `const X = { foo: 'str', bar: 42 }`
                if let Some(Expression::ObjectExpression(obj)) = &declarator.init {
                    return Some(infer_props_from_object_literal(obj, source_bytes));
                }
            }
        }
    }

    None
}

/// Infer prop types from an object literal's property values.
pub(super) fn infer_props_from_object_literal(
    obj: &oxc_ast::ast::ObjectExpression<'_>,
    _source_bytes: &[u8],
) -> ResolvedElements {
    let mut result = ResolvedElements {
        root_runtime_types: vec![RuntimeType::Object],
        ..ResolvedElements::default()
    };

    for prop in &obj.properties {
        let ObjectPropertyKind::ObjectProperty(p) = prop else {
            continue;
        };
        let key_span: oxc_span::Span = p.key.span();
        let runtime_type = match &p.value {
            Expression::StringLiteral(_) | Expression::TemplateLiteral(_) => {
                vec![RuntimeType::String]
            }
            Expression::NumericLiteral(_) => vec![RuntimeType::Number],
            Expression::BooleanLiteral(_) => vec![RuntimeType::Boolean],
            Expression::ArrayExpression(_) => vec![RuntimeType::Array],
            Expression::ObjectExpression(_) => vec![RuntimeType::Object],
            Expression::NullLiteral(_) => vec![RuntimeType::Null],
            _ => vec![RuntimeType::Unknown],
        };

        // SAFETY: `infer_props_from_object_literal` is reached only
        // via `resolve_typeof_object_literal_init` — the `typeof X`
        // indirection case (`defineProps(typeof X)` where
        // `const X = { ... }`). Members reach the surface via the
        // `typeof X` heritage hop, NOT via the author writing them
        // in the macro's T own body. `false` is the structural truth.
        result.props.push(ResolvedProp {
            span: crate::common::Span::new(key_span.start, key_span.end),
            key: crate::common::Span::new(key_span.start, key_span.end),
            key_name: None,
            types: runtime_type,
            optional: false,
            visibility: ResolvedMemberVisibility::Public,
            type_span: None,
            type_text: None,
            map_local: true,
            span_is_absolute: false,
            declared_in_macro_type_arg: false,
        });
    }

    result
}
