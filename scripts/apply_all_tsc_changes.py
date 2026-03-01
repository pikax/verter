"""Apply ALL tsc-related changes in one atomic batch."""
import sys
import os

os.chdir(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))

def patch_file(path, replacements):
    with open(path, 'r') as f:
        content = f.read()
    for old, new in replacements:
        if old in content:
            content = content.replace(old, new)
        else:
            print(f'  SKIP: pattern not found in {path}')
    with open(path, 'w') as f:
        f.write(content)
    print(f'  OK: {path}')

def append_file(path, text):
    with open(path, 'a') as f:
        f.write(text)
    print(f'  OK: appended to {path}')

# ══════════════════════════════════════════════════════════════════════════
# 1. MACROS.RS — Add new fields to MacroProperty
# ══════════════════════════════════════════════════════════════════════════
print('1. Patching macros.rs...')
patch_file('crates/verter_core/src/utils/oxc/vue/script/macros.rs', [(
'''pub struct MacroProperty<'a> {
    /// The property name
    pub name: &'a str,
    /// Span of the property name
    pub name_span: Span,
    /// Span of the value (Some for { foo: String }, None for shorthand)
    pub value_span: Option<Span>,
    /// Whether this property uses method shorthand (e.g., `foo() { ... }`)
    pub is_method: bool,
}''',
'''pub struct MacroProperty<'a> {
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
    /// Span of the TS type from a type cast (PropType<X> or () => X)
    pub ts_type_span: Option<Span>,
    /// Type reference names found in ts_type_span
    pub ts_type_refs: Vec<&'a str>,
}'''
)])

# ══════════════════════════════════════════════════════════════════════════
# 2. SETUP.RS — Add extraction functions + update extract_object_arg
# ══════════════════════════════════════════════════════════════════════════
print('2. Patching setup.rs...')
patch_file('crates/verter_core/src/utils/oxc/vue/script/setup.rs', [
# 2a. Replace extract_object_arg with version that populates new fields
(
'''/// Extract object argument details
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
}''',
r'''/// Extract object argument details, including TypeScript type information.
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
    _ctx: &ScriptParseContext<'_>,
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
                if let Some(type_args) = &type_ref.type_arguments {
                    if let Some(first_param) = type_args.params.first() {
                        let oxc_sp: oxc_span::Span = first_param.span();
                        let span = Span::from(oxc_sp);
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
            if let Some(type_args) = &type_ref.type_arguments {
                for param in &type_args.params {
                    collect_type_refs(param, refs);
                }
            }
        }
        TSType::TSArrayType(arr) => collect_type_refs(&arr.element_type, refs),
        TSType::TSTupleType(tuple) => {
            for elem in &tuple.element_types { collect_type_refs(&elem.to_ts_type(), refs); }
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
),
# 2b. Lifetime fixes
("pub fn process_setup_statements<'a>(\n    statements: &[Statement<'a>],",
 "pub fn process_setup_statements<'a>(\n    statements: &'a [Statement<'a>],"),
("pub fn process_setup_statement<'a>(\n    stmt: &Statement<'a>,",
 "pub fn process_setup_statement<'a>(\n    stmt: &'a Statement<'a>,"),
("fn extract_arg_spans<'a>(\n    call: &CallExpression<'a>,\n    ctx: &ScriptParseContext<'a>,\n) -> (Option<MacroObjectArg<'a>>, Option<MacroArrayArg>) {",
 "fn extract_arg_spans<'a>(\n    call: &'a CallExpression<'a>,\n    ctx: &ScriptParseContext<'a>,\n) -> (Option<MacroObjectArg<'a>>, Option<MacroArrayArg>) {"),
("fn extract_object_arg_from_call<'a>(\n    call: &CallExpression<'a>,",
 "fn extract_object_arg_from_call<'a>(\n    call: &'a CallExpression<'a>,"),
("    var_decl: &VariableDeclaration<'a>,",
 "    var_decl: &'a VariableDeclaration<'a>,"),
("    expr_stmt: &ExpressionStatement<'a>,",
 "    expr_stmt: &'a ExpressionStatement<'a>,"),
("fn parse_macro_call<'a>(\n    call: &CallExpression<'a>,",
 "fn parse_macro_call<'a>(\n    call: &'a CallExpression<'a>,"),
("fn try_parse_macro_from_expression<'a>(\n    expr: &Expression<'a>,",
 "fn try_parse_macro_from_expression<'a>(\n    expr: &'a Expression<'a>,"),
])

# ══════════════════════════════════════════════════════════════════════════
# 3. MOD.RS — Lifetime fix for program parameter
# ══════════════════════════════════════════════════════════════════════════
print('3. Patching mod.rs...')
patch_file('crates/verter_core/src/utils/oxc/vue/script/mod.rs', [
("    program: &Program<'a>,\n    mode: ScriptMode,",
 "    program: &'a Program<'a>,\n    mode: ScriptMode,"),
])

# ══════════════════════════════════════════════════════════════════════════
# 4. OPTIONS.RS — Lifetime fixes
# ══════════════════════════════════════════════════════════════════════════
print('4. Patching options.rs...')
patch_file('crates/verter_core/src/utils/oxc/vue/script/options.rs', [
("    statements: &[Statement<'a>],\n    ctx: &ScriptParseContext<'a>,\n    options_ctx: &mut OptionsContext,",
 "    statements: &'a [Statement<'a>],\n    ctx: &ScriptParseContext<'a>,\n    options_ctx: &mut OptionsContext,"),
("    stmt: &Statement<'a>,\n    ctx: &ScriptParseContext<'a>,\n    _options_ctx: &mut OptionsContext,",
 "    stmt: &'a Statement<'a>,\n    ctx: &ScriptParseContext<'a>,\n    _options_ctx: &mut OptionsContext,"),
("    export: &ExportDefaultDeclaration<'a>,\n    ctx: &ScriptParseContext<'a>,",
 "    export: &'a ExportDefaultDeclaration<'a>,\n    ctx: &ScriptParseContext<'a>,"),
("fn analyze_default_export_expression<'a>(\n    expr: &Expression<'a>,",
 "fn analyze_default_export_expression<'a>(\n    expr: &'a Expression<'a>,"),
("fn find_setup_in_object<'a>(\n    obj: &ObjectExpression<'a>,",
 "fn find_setup_in_object<'a>(\n    obj: &'a ObjectExpression<'a>,"),
("fn process_setup_value<'a>(\n    value: &Expression<'a>,",
 "fn process_setup_value<'a>(\n    value: &'a Expression<'a>,"),
])

# ══════════════════════════════════════════════════════════════════════════
# 5. TSC/SCRIPT.RS — Add imports field, update process_props, generate_code
# ══════════════════════════════════════════════════════════════════════════
print('5. Patching tsc/script.rs...')
patch_file('crates/verter_core/src/tsc/script.rs', [
# 5a. Add imports field to TscMacroState
("    // defineModel — each model binding\n    models: Vec<ModelEntry>,\n}",
 "    // defineModel — each model binding\n    models: Vec<ModelEntry>,\n\n    // Targeted type imports needed for PropType<X> references\n    imports: Vec<String>,\n}"),
# 5b. Update generate_code to emit imports
('    // ── Import ────────────────────────────────────────────────────────\n    out.push_str("import { defineComponent } from \\"vue\\"\\n\\n");',
 '''    // ── Import ────────────────────────────────────────────────────────
    out.push_str("import { defineComponent } from \\"vue\\"\\n");

    // Emit targeted type imports (for PropType<X> references)
    for import in &state.imports {
        out.push('\\n');
        out.push_str(import);
    }
    out.push('\\n');'''),
# 5c. Update process_props to use AST-extracted types
('''    } else if let Some(obj) = object_arg {
        // Object-syntax: `defineProps({ title: String, count: { type: Number, required: true } })`
        // Both runtime props AND TypeScript types are generated.
        let mut entries = Vec::new();
        for prop in &obj.properties {
            let value_text = prop
                .value_span
                .map(|vs| &content_str[vs.start as usize..vs.end as usize])
                .unwrap_or("null");

            // Runtime: preserve raw value text
            state
                .props_runtime
                .push((prop.name.to_string(), value_text.to_string()));

            // TypeScript: convert value to TS type
            let (optional, ts_type) = value_to_ts(value_text);
            entries.push(InlinePropEntry {
                name: prop.name.to_string(),
                optional,
                ts_type,
            });
        }
        state.props_ts = Some(PropsTs::Inline(entries));
    }''',
'''    } else if let Some(obj) = object_arg {
        // Object-syntax: `defineProps({ title: String, count: { type: Number, required: true } })`
        let mut entries = Vec::new();
        for prop in &obj.properties {
            let value_text = prop
                .value_span
                .map(|vs| &content_str[vs.start as usize..vs.end as usize])
                .unwrap_or("null");

            // Runtime: strip type casts from value text
            let runtime_value = strip_type_casts(value_text);
            state
                .props_runtime
                .push((prop.name.to_string(), runtime_value));

            // TypeScript: use AST-extracted type if available
            let (optional, ts_type) = if let Some(ts_span) = &prop.ts_type_span {
                let ts_text = content_str[ts_span.start as usize..ts_span.end as usize].trim();
                let ts_type = constructor_to_ts(ts_text)
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| ts_text.to_string());
                for type_ref in &prop.ts_type_refs {
                    if let Some(&source) = type_imports.get(type_ref) {
                        let import_stmt = format!("import type {{ {} }} from \'{}\'", type_ref, source);
                        if !state.imports.contains(&import_stmt) {
                            state.imports.push(import_stmt);
                        }
                    }
                }
                (!prop.required, ts_type)
            } else {
                let (fallback_optional, ts_type) = value_to_ts(value_text);
                let optional = if prop.required { false } else { fallback_optional };
                (optional, ts_type)
            };

            entries.push(InlinePropEntry {
                name: prop.name.to_string(),
                optional,
                ts_type,
            });
        }
        state.props_ts = Some(PropsTs::Inline(entries));
    }'''),
# 5d. Add strip_type_casts function before constructor_to_ts
("/// Map a JavaScript constructor name to its TypeScript primitive equivalent.",
 '''/// Strip TypeScript type casts from a runtime value text.
fn strip_type_casts(value: &str) -> String {
    let v = value.trim();
    // Direct type cast: `Ctor as PropType<...>`
    if let Some(pos) = v.find(" as ") {
        if !v.starts_with('{') {
            return v[..pos].trim().to_string();
        }
    }
    // Object form: strip type casts from the `type:` field value
    if v.starts_with('{') && v.ends_with('}') {
        if v.find(" as ").is_some() {
            let mut result = String::with_capacity(v.len());
            let mut chars = v.chars().peekable();
            let mut in_type_field = false;
            let mut depth = 0;
            while let Some(c) = chars.next() {
                match c {
                    '{' => { depth += 1; result.push(c); }
                    '}' => { depth -= 1; result.push(c); }
                    _ if depth == 1 => {
                        result.push(c);
                        if result.ends_with(" as ") && in_type_field {
                            result.truncate(result.len() - 4);
                            let mut angle_depth = 0;
                            let mut paren_depth = 0;
                            while let Some(&nc) = chars.peek() {
                                if nc == '<' { angle_depth += 1; }
                                else if nc == '>' && angle_depth > 0 { angle_depth -= 1; }
                                else if nc == '(' { paren_depth += 1; }
                                else if nc == ')' && paren_depth > 0 { paren_depth -= 1; }
                                else if angle_depth == 0 && paren_depth == 0 && (nc == ',' || nc == '}') { break; }
                                chars.next();
                            }
                            in_type_field = false;
                        } else if result.ends_with("type:") || result.ends_with("type: ") {
                            in_type_field = true;
                        } else if c == ',' { in_type_field = false; }
                    }
                    _ => result.push(c),
                }
            }
            return result;
        }
    }
    v.to_string()
}

/// Map a JavaScript constructor name to its TypeScript primitive equivalent.'''),
])

# ══════════════════════════════════════════════════════════════════════════
# 6. TESTS.RS — Append new tests
# ══════════════════════════════════════════════════════════════════════════
print('6. Appending tests...')
append_file('crates/verter_core/src/tsc/tests.rs', r'''
// ── PropType<X> cast extraction ──────────────────────────────────────────────

#[test]
fn tsc_codegen_proptype_cast_extracts_type() {
    let r = gen_tsc(
        r#"<script setup lang="ts">
import type { PropType } from 'vue'
import type { UserInfo } from './types'
defineProps({
  user: { type: Object as PropType<UserInfo>, required: true },
  tags: Array as PropType<string[]>
})
</script><template/>"#,
    );

    assert!(r.contains("props: {"), "runtime props present");
    assert!(r.contains("user: UserInfo"), "user typed as UserInfo\n{r}");
    assert!(r.contains("tags?: string[]"), "tags typed as string[]\n{r}");
    assert!(r.contains("import type { UserInfo } from './types'"), "import for UserInfo emitted\n{r}");
    assert!(!r.contains("import type { PropType }"), "PropType import not emitted");
}

#[test]
fn tsc_codegen_factory_function_cast() {
    let r = gen_tsc(
        r#"<script setup lang="ts">
import type { Config } from './config'
defineProps({
  config: { type: Object as () => Config, required: true }
})
</script><template/>"#,
    );

    assert!(r.contains("config: Config"), "factory function type extracted\n{r}");
    assert!(r.contains("import type { Config } from './config'"), "import for Config emitted\n{r}");
}

#[test]
fn tsc_codegen_mixed_proptype_and_plain() {
    let r = gen_tsc(
        r#"<script setup lang="ts">
import type { PropType } from 'vue'
import type { Item } from './types'
defineProps({
  title: String,
  count: { type: Number, required: true },
  items: { type: Array as PropType<Item[]>, required: true }
})
</script><template/>"#,
    );

    assert!(r.contains("title?: string"), "title is optional string\n{r}");
    assert!(r.contains("count: number"), "count is required number\n{r}");
    assert!(r.contains("items: Item[]"), "items typed as Item[]\n{r}");
    assert!(r.contains("import type { Item } from './types'"), "import for Item emitted\n{r}");
}

#[test]
fn tsc_codegen_local_type_no_import_needed() {
    let r = gen_tsc(
        r#"<script setup lang="ts">
import type { PropType } from 'vue'
interface UserInfo { name: string; age: number }
defineProps({
  user: { type: Object as PropType<UserInfo> }
})
</script><template/>"#,
    );

    assert!(r.contains("user?: UserInfo"), "user typed as UserInfo\n{r}");
    assert!(!r.contains("import type { UserInfo }"), "no import for local type\n{r}");
}

#[test]
fn tsc_codegen_runtime_value_strips_type_cast() {
    let r = gen_tsc(
        r#"<script setup lang="ts">
import type { PropType } from 'vue'
defineProps({
  user: { type: Object as PropType<{name: string}>, required: true }
})
</script><template/>"#,
    );

    let runtime_section = r.split("declare const").next().unwrap_or("");
    assert!(!runtime_section.contains("PropType"), "PropType stripped from runtime\n{runtime_section}");
    assert!(!runtime_section.contains(" as "), "type cast stripped from runtime\n{runtime_section}");
}
''')

print('\nAll changes applied!')
