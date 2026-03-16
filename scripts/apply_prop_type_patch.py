"""Apply all prop type extraction changes at once."""
import sys
import os

os.chdir(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))

# ── 1. Patch macros.rs ────────────────────────────────────────────────────
MACROS_FILE = 'crates/verter_core/src/utils/oxc/vue/script/macros.rs'
with open(MACROS_FILE, 'r') as f:
    content = f.read()

OLD_STRUCT = '''pub struct MacroProperty<'a> {
    /// The property name
    pub name: &'a str,
    /// Span of the property name
    pub name_span: Span,
    /// Span of the value (Some for { foo: String }, None for shorthand)
    pub value_span: Option<Span>,
    /// Whether this property uses method shorthand (e.g., `foo() { ... }`)
    pub is_method: bool,
}'''

NEW_STRUCT = '''pub struct MacroProperty<'a> {
    /// The property name
    pub name: &'a str,
    /// Span of the property name
    pub name_span: Span,
    /// Span of the value (Some for { foo: String }, None for shorthand)
    pub value_span: Option<Span>,
    /// Whether this property uses method shorthand (e.g., `foo() { ... }`)
    pub is_method: bool,
    /// Whether this prop has `required: true`
    pub required: bool,
    /// Span of the TS type from a type cast (e.g., PropType<X> or () => X)
    pub ts_type_span: Option<Span>,
    /// Type reference names found in ts_type_span
    pub ts_type_refs: Vec<&'a str>,
}'''

if OLD_STRUCT in content:
    content = content.replace(OLD_STRUCT, NEW_STRUCT)
    with open(MACROS_FILE, 'w') as f:
        f.write(content)
    print(f'OK: patched {MACROS_FILE}')
else:
    print(f'SKIP: {MACROS_FILE} already patched or different')

# ── 2. Patch setup.rs ────────────────────────────────────────────────────
SETUP_FILE = 'crates/verter_core/src/utils/oxc/vue/script/setup.rs'
with open(SETUP_FILE, 'r') as f:
    content = f.read()

OLD_EXTRACT = '''/// Extract object argument details
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
}'''

NEW_EXTRACT = '''/// Extract object argument details, including TypeScript type information
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

struct PropTypeInfo<'a> {
    ts_type_span: Option<Span>,
    required: bool,
    type_refs: Vec<&'a str>,
}

fn extract_prop_type_info<'a>(
    value: &'a Expression<'a>,
    ctx: &ScriptParseContext<'_>,
) -> PropTypeInfo<'a> {
    match value {
        Expression::TSAsExpression(ts_as) => {
            let (span, refs) = extract_ts_type_from_annotation(&ts_as.type_annotation);
            PropTypeInfo { ts_type_span: span, required: false, type_refs: refs }
        }
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
                            if let Expression::TSAsExpression(ts_as) = &p.value {
                                let (span, refs) = extract_ts_type_from_annotation(&ts_as.type_annotation);
                                ts_type_span = span;
                                type_refs = refs;
                            }
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
            PropTypeInfo { ts_type_span, required, type_refs }
        }
        _ => PropTypeInfo { ts_type_span: None, required: false, type_refs: Vec::new() },
    }
}

fn extract_ts_type_from_annotation<'a>(annotation: &'a TSType<'a>) -> (Option<Span>, Vec<&'a str>) {
    match annotation {
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
        TSType::TSFunctionType(func) => {
            let return_span = Span::from(func.return_type.type_annotation.span());
            let mut refs = Vec::new();
            collect_type_refs(&func.return_type.type_annotation, &mut refs);
            (Some(return_span), refs)
        }
        _ => (None, Vec::new()),
    }
}

fn collect_type_refs<'a>(node: &'a TSType<'a>, refs: &mut Vec<&'a str>) {
    match node {
        TSType::TSTypeReference(type_ref) => {
            if let TSTypeName::IdentifierReference(id) = &type_ref.type_name {
                let name = id.name.as_str();
                if !is_builtin_type(name) {
                    refs.push(name);
                }
            }
            if let Some(type_args) = &type_ref.type_parameters {
                for param in &type_args.params {
                    collect_type_refs(param, refs);
                }
            }
        }
        TSType::TSArrayType(arr) => collect_type_refs(&arr.element_type, refs),
        TSType::TSTupleType(tuple) => {
            for elem in &tuple.element_types {
                collect_type_refs(&elem.to_ts_type(), refs);
            }
        }
        TSType::TSUnionType(union) => {
            for member in &union.types { collect_type_refs(member, refs); }
        }
        TSType::TSIntersectionType(inter) => {
            for member in &inter.types { collect_type_refs(member, refs); }
        }
        _ => {}
    }
}

fn is_builtin_type(name: &str) -> bool {
    matches!(
        name,
        "string" | "number" | "boolean" | "symbol" | "bigint"
            | "void" | "null" | "undefined" | "never" | "unknown" | "any" | "object"
            | "String" | "Number" | "Boolean" | "Symbol" | "BigInt"
            | "Object" | "Array" | "Function" | "Date" | "RegExp" | "Error"
            | "Map" | "Set" | "WeakMap" | "WeakSet" | "Promise"
            | "Record" | "Partial" | "Required" | "Readonly"
            | "Pick" | "Omit" | "Exclude" | "Extract" | "NonNullable"
            | "ReturnType" | "InstanceType" | "Parameters" | "ConstructorParameters"
            | "PropType"
    )
}'''

if OLD_EXTRACT in content:
    content = content.replace(OLD_EXTRACT, NEW_EXTRACT)
    with open(SETUP_FILE, 'w') as f:
        f.write(content)
    print(f'OK: patched {SETUP_FILE}')
else:
    print(f'SKIP: {SETUP_FILE} already patched or different')

# ── 3. Patch lifetime in extract_object_arg_from_call ──────────────────────
with open(SETUP_FILE, 'r') as f:
    content = f.read()

# The call needs &'a CallExpression<'a> to thread the lifetime
content = content.replace(
    "fn extract_object_arg_from_call<'a>(\n    call: &CallExpression<'a>,",
    "fn extract_object_arg_from_call<'a>(\n    call: &'a CallExpression<'a>,",
)
with open(SETUP_FILE, 'w') as f:
    f.write(content)
print('OK: patched lifetimes in setup.rs')

# ── 4. Patch mod.rs lifetimes ─────────────────────────────────────────────
MOD_FILE = 'crates/verter_core/src/utils/oxc/vue/script/mod.rs'
with open(MOD_FILE, 'r') as f:
    content = f.read()

content = content.replace(
    "    program: &Program<'a>,\n    mode: ScriptMode,",
    "    program: &'a Program<'a>,\n    mode: ScriptMode,",
)
with open(MOD_FILE, 'w') as f:
    f.write(content)
print(f'OK: patched {MOD_FILE}')

# ── 5. Patch options.rs lifetimes ────────────────────────────────────────
OPT_FILE = 'crates/verter_core/src/utils/oxc/vue/script/options.rs'
with open(OPT_FILE, 'r') as f:
    content = f.read()

content = content.replace(
    "    statements: &[Statement<'a>],\n    ctx: &ScriptParseContext<'a>,\n    options_ctx: &mut OptionsContext,",
    "    statements: &'a [Statement<'a>],\n    ctx: &ScriptParseContext<'a>,\n    options_ctx: &mut OptionsContext,",
)
content = content.replace(
    "    stmt: &Statement<'a>,\n    ctx: &ScriptParseContext<'a>,\n    _options_ctx: &mut OptionsContext,",
    "    stmt: &'a Statement<'a>,\n    ctx: &ScriptParseContext<'a>,\n    _options_ctx: &mut OptionsContext,",
)
content = content.replace(
    "    export: &ExportDefaultDeclaration<'a>,\n    ctx: &ScriptParseContext<'a>,",
    "    export: &'a ExportDefaultDeclaration<'a>,\n    ctx: &ScriptParseContext<'a>,",
)
content = content.replace(
    "fn analyze_default_export_expression<'a>(\n    expr: &Expression<'a>,",
    "fn analyze_default_export_expression<'a>(\n    expr: &'a Expression<'a>,",
)
content = content.replace(
    "fn find_setup_in_object<'a>(\n    obj: &ObjectExpression<'a>,",
    "fn find_setup_in_object<'a>(\n    obj: &'a ObjectExpression<'a>,",
)
content = content.replace(
    "fn process_setup_value<'a>(\n    value: &Expression<'a>,",
    "fn process_setup_value<'a>(\n    value: &'a Expression<'a>,",
)

with open(OPT_FILE, 'w') as f:
    f.write(content)
print(f'OK: patched {OPT_FILE}')

print('\nAll patches applied!')
