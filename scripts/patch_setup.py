"""Patch setup.rs to add type extraction functions."""
import sys

FILE = 'crates/verter_core/src/utils/oxc/vue/script/setup.rs'

with open(FILE, 'r') as f:
    content = f.read()

OLD = """/// Extract object argument details
fn extract_object_arg<'a>(
    obj: &ObjectExpression<'a>,
    ctx: &ScriptParseContext<'a>,
) -> MacroObjectArg<'a> {
    let mut properties = Vec::new();

    for prop in &obj.properties {
        if let ObjectPropertyKind::ObjectProperty(p) = prop {
            if let Some((name, name_span)) = extract_property_key(&p.key, ctx) {
                let value_span = if p.shorthand {
                    None
                } else {
                    Some(Span::from(p.value.span()))
                };
                properties.push(MacroProperty {
                    name,
                    name_span,
                    value_span,
                    is_method: p.method,
                });
            }
        }
    }

    MacroObjectArg {
        span: Span::from(obj.span),
        properties,
    }
}"""

NEW = r"""/// Extract object argument details, including TypeScript type information
/// from `PropType<X>` and `() => X` type casts in prop values.
fn extract_object_arg<'a>(
    obj: &'a ObjectExpression<'a>,
    ctx: &ScriptParseContext<'a>,
) -> MacroObjectArg<'a> {
    let mut properties = Vec::new();

    for prop in &obj.properties {
        if let ObjectPropertyKind::ObjectProperty(p) = prop {
            if let Some((name, name_span)) = extract_property_key(&p.key, ctx) {
                let value_span = if p.shorthand {
                    None
                } else {
                    Some(Span::from(p.value.span()))
                };

                // Extract type cast info and required flag from the value
                let prop_info = extract_prop_type_info(&p.value, ctx);

                properties.push(MacroProperty {
                    name,
                    name_span,
                    value_span,
                    is_method: p.method,
                    required: prop_info.required,
                    ts_type_span: prop_info.ts_type_span,
                    ts_type_refs: prop_info.type_refs,
                });
            }
        }
    }

    MacroObjectArg {
        span: Span::from(obj.span),
        properties,
    }
}

/// Information extracted from a prop value's type annotation.
struct PropTypeInfo<'a> {
    ts_type_span: Option<Span>,
    required: bool,
    type_refs: Vec<&'a str>,
}

/// Extract TypeScript type information from a prop value expression.
///
/// Handles these patterns:
/// - `Object as PropType<UserInfo>` -> extracts `UserInfo` span + refs
/// - `Object as () => Config` -> extracts `Config` span + refs
/// - `{ type: Object as PropType<X>, required: true }` -> extracts type + required
/// - `{ type: String }` -> no type extraction (handled by constructor mapping)
/// - `String` -> no type extraction
fn extract_prop_type_info<'a>(
    value: &'a Expression<'a>,
    ctx: &ScriptParseContext<'_>,
) -> PropTypeInfo<'a> {
    match value {
        // Direct type cast: `Object as PropType<X>` or `Array as PropType<X[]>`
        Expression::TSAsExpression(ts_as) => {
            let (span, refs) = extract_ts_type_from_annotation(&ts_as.type_annotation);
            PropTypeInfo {
                ts_type_span: span,
                required: false,
                type_refs: refs,
            }
        }

        // Object form: `{ type: X, required: true/false }`
        Expression::ObjectExpression(obj) => {
            let mut ts_type_span = None;
            let mut type_refs = Vec::new();
            let mut required = false;

            for prop in &obj.properties {
                if let ObjectPropertyKind::ObjectProperty(p) = prop {
                    let key_name = match &p.key {
                        PropertyKey::StaticIdentifier(id) => Some(id.name.as_str()),
                        _ => None,
                    };

                    match key_name {
                        Some("type") => {
                            // Check for type cast: `type: Object as PropType<X>`
                            if let Expression::TSAsExpression(ts_as) = &p.value {
                                let (span, refs) =
                                    extract_ts_type_from_annotation(&ts_as.type_annotation);
                                ts_type_span = span;
                                type_refs = refs;
                            }
                            // Plain constructor like `type: String` -- no extraction needed
                        }
                        Some("required") => {
                            if let Expression::BooleanLiteral(b) = &p.value {
                                required = b.value;
                            }
                        }
                        _ => {}
                    }
                }
            }

            PropTypeInfo {
                ts_type_span,
                required,
                type_refs,
            }
        }

        _ => PropTypeInfo {
            ts_type_span: None,
            required: false,
            type_refs: Vec::new(),
        },
    }
}

/// Extract the TypeScript type span and type references from a type annotation.
///
/// Handles:
/// - `PropType<UserInfo>` -> span of `UserInfo`, refs = ["UserInfo"]
/// - `PropType<string[]>` -> span of `string[]`, refs = []
/// - `() => Config` -> span of `Config`, refs = ["Config"]
fn extract_ts_type_from_annotation<'a>(annotation: &'a TSType<'a>) -> (Option<Span>, Vec<&'a str>) {
    match annotation {
        // PropType<X> -> extract X
        TSType::TSTypeReference(type_ref) => {
            let name = match &type_ref.type_name {
                TSTypeName::IdentifierReference(id) => id.name.as_str(),
                _ => "",
            };
            if name == "PropType" {
                if let Some(type_args) = &type_ref.type_parameters {
                    if let Some(first_param) = type_args.params.first() {
                        let span = Span::from(first_param.span());
                        let mut refs = Vec::new();
                        collect_type_refs(first_param, &mut refs);
                        return (Some(span), refs);
                    }
                }
            }
            (None, Vec::new())
        }
        // () => Config -> extract Config (return type)
        TSType::TSFunctionType(func) => {
            let return_span = Span::from(func.return_type.type_annotation.span());
            let mut refs = Vec::new();
            collect_type_refs(&func.return_type.type_annotation, &mut refs);
            (Some(return_span), refs)
        }
        _ => (None, Vec::new()),
    }
}

/// Recursively collect type reference names from a TSType AST node.
///
/// Skips built-in types (string, number, boolean, etc.) and globals.
fn collect_type_refs<'a>(node: &'a TSType<'a>, refs: &mut Vec<&'a str>) {
    match node {
        TSType::TSTypeReference(type_ref) => {
            if let TSTypeName::IdentifierReference(id) = &type_ref.type_name {
                let name = id.name.as_str();
                if !is_builtin_type(name) {
                    refs.push(name);
                }
            }
            // Recurse into type arguments: e.g., `Map<string, UserInfo>` -> collect `UserInfo`
            if let Some(type_args) = &type_ref.type_parameters {
                for param in &type_args.params {
                    collect_type_refs(param, refs);
                }
            }
        }
        TSType::TSArrayType(arr) => {
            collect_type_refs(&arr.element_type, refs);
        }
        TSType::TSTupleType(tuple) => {
            for elem in &tuple.element_types {
                collect_type_refs(&elem.to_ts_type(), refs);
            }
        }
        TSType::TSUnionType(union) => {
            for member in &union.types {
                collect_type_refs(member, refs);
            }
        }
        TSType::TSIntersectionType(inter) => {
            for member in &inter.types {
                collect_type_refs(member, refs);
            }
        }
        // Primitives, literals, keywords -- no refs to collect
        _ => {}
    }
}

/// Check if a type name is a built-in TypeScript type that doesn't need importing.
fn is_builtin_type(name: &str) -> bool {
    matches!(
        name,
        "string"
            | "number"
            | "boolean"
            | "symbol"
            | "bigint"
            | "void"
            | "null"
            | "undefined"
            | "never"
            | "unknown"
            | "any"
            | "object"
            | "String"
            | "Number"
            | "Boolean"
            | "Symbol"
            | "BigInt"
            | "Object"
            | "Array"
            | "Function"
            | "Date"
            | "RegExp"
            | "Error"
            | "Map"
            | "Set"
            | "WeakMap"
            | "WeakSet"
            | "Promise"
            | "Record"
            | "Partial"
            | "Required"
            | "Readonly"
            | "Pick"
            | "Omit"
            | "Exclude"
            | "Extract"
            | "NonNullable"
            | "ReturnType"
            | "InstanceType"
            | "Parameters"
            | "ConstructorParameters"
            | "PropType"
    )
}"""

if OLD in content:
    content = content.replace(OLD, NEW)
    with open(FILE, 'w') as f:
        f.write(content)
    print('OK: patched setup.rs')
else:
    print('ERROR: old text not found in setup.rs')
    sys.exit(1)
