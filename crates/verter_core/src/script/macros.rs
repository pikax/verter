//! Vue macro processing for `<script setup>` and companion `<script>` blocks.
//!
//! Handles `defineProps`, `defineEmits`, `defineModel`, `defineOptions`,
//! `defineExpose`, `defineSlots`, and `withDefaults`. Each macro is replaced
//! with its runtime equivalent and its metadata is accumulated in [`MacroState`]
//! for the component definition builder.
//!
//! Also processes companion `<script>` blocks to extract `export default`
//! options and type declarations for cross-block type resolution.

use oxc_parser::Parser;
use oxc_span::SourceType;
use rustc_hash::FxHashMap;

use crate::parser::types::RootNodeScript;
use crate::template::code_gen::binding::BindingType;
use crate::template::code_gen::types::CodeGenOutput;
use crate::utils::oxc::vue::resolve_type::format_runtime_types;
use crate::utils::oxc::vue::{
    extract_companion_types, parse_script, ResolvedElements, ScriptItem, ScriptMacro, ScriptMode,
};

use super::ScriptContext;

// ======================== Helpers ========================

/// Push a method shorthand value as an arrow function.
///
/// Method shorthand `() { return ... }` → arrow function `() => { return ... }`.
/// Finds the matching `)` for the first `(` (handling nesting) and inserts ` =>`.
pub(super) fn push_method_as_arrow(out: &mut String, val: &str) {
    let mut depth = 0;
    for (i, c) in val.char_indices() {
        match c {
            '(' => depth += 1,
            ')' if depth > 0 => {
                depth -= 1;
                if depth == 0 {
                    out.push_str(&val[..i + 1]);
                    out.push_str(" =>");
                    out.push_str(&val[i + 1..]);
                    return;
                }
            }
            _ => {}
        }
    }
    // Fallback: push as-is if no matching parens found
    out.push_str(val);
}

// ======================== Macro state ========================

/// Accumulated macro data collected during item processing.
/// Used to build the component definition sections.
pub(super) struct MacroState {
    /// Props section text (e.g., `{ title: { type: String } }`).
    pub props_section: Option<String>,
    /// Emits section text (e.g., `['click', 'update']`).
    pub emits_section: Option<String>,
    /// Options section text (e.g., `inheritAttrs: false`).
    pub options_section: Option<String>,
    /// Whether `defineExpose` was used.
    pub has_expose: bool,
    /// Whether `defineEmits` was used (needs `__emit` in setup params).
    pub has_emit: bool,
    /// Model names from `defineModel()` calls — each needs a prop and emit declaration.
    pub model_names: Vec<String>,
}

impl MacroState {
    pub fn new() -> Self {
        Self {
            props_section: None,
            emits_section: None,
            options_section: None,
            has_expose: false,
            has_emit: false,
            model_names: Vec::new(),
        }
    }
}

// ======================== process_macro_item ========================

/// Process a single Vue macro call (defineProps, defineEmits, etc.).
///
/// Replaces the macro call with its runtime equivalent and accumulates
/// metadata in [`MacroState`] for later use in the component definition.
///
/// Also extracts prop names directly from defineProps arguments and
/// adds them to bindings with `BindingType::Props`. This avoids
/// relying on `parse_result.bindings` for Props, which has inconsistent
/// span coordinate systems (object-syntax keys are SFC-absolute, while
/// array-syntax keys are content-relative).
#[cfg_attr(feature = "hotpath", hotpath::measure)]
pub(super) fn process_macro_item<'a>(
    mac: &ScriptMacro<'_>,
    content_start: u32,
    content_str: &'a str,
    ctx: &mut ScriptContext<'a>,
    state: &mut MacroState,
) {
    match mac {
        ScriptMacro::DefineExpose { span, .. } => {
            state.has_expose = true;
            // Replace "defineExpose" (12 chars) with "__expose", keeping args
            let abs_start = content_start + span.start;
            ctx.out.overwrite(
                abs_start,
                abs_start + "defineExpose".len() as u32,
                "__expose",
            );
        }

        ScriptMacro::DefineSlots { span, .. } => {
            // Replace entire macro call with _useSlots()
            let abs_start = content_start + span.start;
            let abs_end = content_start + span.end;
            ctx.out.overwrite(abs_start, abs_end, "_useSlots()");
            ctx.imports.push("_useSlots");
        }

        ScriptMacro::DefineProps {
            span,
            declarator,
            object_arg,
            array_arg,
            type_params,
            ..
        } => {
            let abs_start = content_start + span.start;
            let abs_end = content_start + span.end;

            // Extract props section from runtime argument and prop names for bindings.
            // We extract prop names here (not from parse_result.bindings) because
            // parse_script returns inconsistent span coordinate systems for Props.
            if let Some(obj) = object_arg {
                let obj_text = &content_str[obj.span.start as usize..obj.span.end as usize];
                state.props_section = Some(obj_text.to_string());
                // Extract property key names from the object
                extract_object_prop_names(obj_text, content_str, obj.span.start, &mut ctx.bindings);
            } else if let Some(arr) = array_arg {
                let arr_text = &content_str[arr.span.start as usize..arr.span.end as usize];
                state.props_section = Some(arr_text.to_string());
                // Extract prop names from array strings
                extract_array_prop_names(arr_text, content_str, arr.span.start, &mut ctx.bindings);
            }

            // Type-based defineProps: extract prop names from resolved type elements
            // and build a runtime props declaration for the component definition.
            // For intra-file types, key spans are SFC-absolute. For external types
            // (cross-file), key_name is pre-resolved since spans reference another file.
            if let Some(tp) = type_params {
                if !tp.resolved.props.is_empty() {
                    let mut props_obj = String::from("{\n");
                    for prop in &tp.resolved.props {
                        // Use pre-resolved key_name for external types, span extraction for intra-file
                        let name: &'a str = if let Some(ref kn) = prop.key_name {
                            ctx.alloc.alloc_str(kn)
                        } else {
                            let key_start = prop.key.start as usize;
                            let key_end = prop.key.end as usize;
                            if key_end > ctx.source.len() {
                                continue;
                            }
                            &ctx.source[key_start..key_end]
                        };
                        ctx.bindings.insert(name, BindingType::Props);

                        // Build runtime prop definition
                        let type_str = format_runtime_types(&prop.types);
                        props_obj.push_str("    ");
                        props_obj.push_str(name);
                        props_obj.push_str(": { type: ");
                        props_obj.push_str(&type_str);
                        if !prop.optional {
                            props_obj.push_str(", required: true");
                        }
                        props_obj.push_str(" },\n");
                    }
                    props_obj.push('}');
                    state.props_section = Some(props_obj);
                }
            }

            // Replace macro call
            if declarator.is_some() {
                // const props = defineProps({...}) → const props = __props
                ctx.out.overwrite(abs_start, abs_end, "__props");
            } else {
                // defineProps({...}) → (removed)
                ctx.out.overwrite(abs_start, abs_end, "");
            }
        }

        ScriptMacro::DefineEmits {
            span,
            declarator,
            object_arg,
            array_arg,
            type_params,
            ..
        } => {
            state.has_emit = true;
            let abs_start = content_start + span.start;
            let abs_end = content_start + span.end;

            // Extract emits section from runtime argument
            if let Some(obj) = object_arg {
                let obj_text = &content_str[obj.span.start as usize..obj.span.end as usize];
                state.emits_section = Some(obj_text.to_string());
            } else if let Some(arr) = array_arg {
                let arr_text = &content_str[arr.span.start as usize..arr.span.end as usize];
                state.emits_section = Some(arr_text.to_string());
            }

            // Type-based defineEmits: extract emit event names from resolved type
            if let Some(tp) = type_params {
                if !tp.resolved.emits.is_empty() {
                    let mut emit_names: Vec<String> = Vec::new();
                    for emit in &tp.resolved.emits {
                        emit_names.push(format!("\"{}\"", emit.name));
                    }
                    state.emits_section = Some(format!("[{}]", emit_names.join(", ")));
                }
            }

            // Replace macro call
            if declarator.is_some() {
                // const emit = defineEmits([...]) → const emit = __emit
                ctx.out.overwrite(abs_start, abs_end, "__emit");
            } else {
                // defineEmits([...]) → (removed)
                ctx.out.overwrite(abs_start, abs_end, "");
            }
        }

        ScriptMacro::DefineOptions {
            span, object_arg, ..
        } => {
            let abs_start = content_start + span.start;
            let abs_end = content_start + span.end;

            // Extract options object content (the object's inner content)
            if let Some(obj) = object_arg {
                // Get the content between { and }, stripping the braces
                let obj_text = &content_str[obj.span.start as usize..obj.span.end as usize];
                // Remove outer braces: "{ inheritAttrs: false }" → " inheritAttrs: false "
                if obj_text.starts_with('{') && obj_text.ends_with('}') {
                    let inner = obj_text[1..obj_text.len() - 1].trim();
                    // Strip trailing comma to avoid double commas in the generated
                    // object literal (we add our own comma after each section).
                    let inner = inner.trim_end_matches(',').trim_end();
                    if !inner.is_empty() {
                        state.options_section = Some(inner.to_string());
                    }
                }
            }

            // Remove the entire macro call
            ctx.out.overwrite(abs_start, abs_end, "");
        }

        ScriptMacro::DefineModel {
            span, name_span, ..
        } => {
            let abs_start = content_start + span.start;
            let abs_end = content_start + span.end;

            // Get model name (default: 'modelValue').
            // OXC StringLiteral span includes surrounding quotes, so strip them.
            let model_name = name_span
                .map(|ns| {
                    let raw = &content_str[ns.start as usize..ns.end as usize];
                    raw.trim_matches(|c| c == '\'' || c == '"')
                })
                .unwrap_or("modelValue");

            // Track this model name for prop/emit declaration generation
            state.model_names.push(model_name.to_string());

            // Replace with _useModel(__props, 'name')
            let replacement = format!("_useModel(__props, '{}')", model_name);
            ctx.out.overwrite(abs_start, abs_end, &replacement);

            ctx.imports.push("_useModel");
        }

        ScriptMacro::WithDefaults {
            span,
            declarator,
            define_props_type_params,
            defaults,
            defaults_arg_span,
            ..
        } => {
            let abs_start = content_start + span.start;
            let abs_end = content_start + span.end;

            // Build props from type-based defineProps with defaults merged in.
            // Type-based: withDefaults(defineProps<{ color?: string }>(), { color: 'primary' })
            // → props: { color: { type: String, default: 'primary' } }
            if let Some(tp) = define_props_type_params {
                if !tp.resolved.props.is_empty() {
                    let mut props_obj = String::from("{\n");
                    for prop in &tp.resolved.props {
                        // key spans are SFC-absolute. For same-block types they
                        // point into the setup content; for cross-block types
                        // (companion <script>) they point elsewhere in `source`.
                        // Use the full SFC `source` so both cases work.
                        let key_start = prop.key.start as usize;
                        let key_end = prop.key.end as usize;
                        if key_end <= ctx.source.len() {
                            let name = &ctx.source[key_start..key_end];
                            ctx.bindings.insert(name, BindingType::Props);

                            let type_str = format_runtime_types(&prop.types);
                            props_obj.push_str("    ");
                            props_obj.push_str(name);
                            props_obj.push_str(": { type: ");
                            props_obj.push_str(&type_str);

                            // Check if this prop has a default in the defaults object.
                            // defaults spans are OXC-local (0-based within content_str).
                            let default_prop = defaults
                                .as_ref()
                                .and_then(|d| d.properties.iter().find(|p| p.name == name));
                            let default_value = default_prop.and_then(|p| {
                                p.value_span
                                    .map(|vs| &content_str[vs.start as usize..vs.end as usize])
                            });

                            if let Some(val) = default_value {
                                props_obj.push_str(", default: ");
                                if default_prop.is_some_and(|p| p.is_method) {
                                    push_method_as_arrow(&mut props_obj, val);
                                } else {
                                    props_obj.push_str(val);
                                }
                            } else if !prop.optional {
                                props_obj.push_str(", required: true");
                            }
                            props_obj.push_str(" },\n");
                        }
                    }
                    props_obj.push('}');
                    state.props_section = Some(props_obj);
                } else if defaults.is_some() || defaults_arg_span.is_some() {
                    // Unresolvable type reference (e.g., `defineProps<ImportedType>()`)
                    // with defaults present. Vue's `mergeDefaults({}, defaults)` does NOT
                    // create new prop declarations from an empty base — it only merges
                    // defaults into existing declarations. We must create the declarations
                    // ourselves.
                    //
                    // Case 1: Object literal defaults — extract keys at compile time
                    //   `{ key: { default: val }, ... }`
                    // Case 2: Variable reference defaults — convert at runtime
                    //   `((d)=>{const p={};for(const k in d)p[k]={default:d[k]};return p})(VAR)`
                    if let Some(d) = defaults {
                        // Object literal: build inline prop declarations from parsed keys
                        let mut props_obj = String::from("{\n");
                        for (i, prop) in d.properties.iter().enumerate() {
                            let val = prop
                                .value_span
                                .map(|vs| &content_str[vs.start as usize..vs.end as usize])
                                .unwrap_or("undefined");
                            props_obj.push_str("    ");
                            props_obj.push_str(prop.name);
                            props_obj.push_str(": { default: ");
                            if prop.is_method {
                                push_method_as_arrow(&mut props_obj, val);
                            } else {
                                props_obj.push_str(val);
                            }
                            props_obj.push_str(" }");
                            if i < d.properties.len() - 1 {
                                props_obj.push(',');
                            }
                            props_obj.push('\n');
                        }
                        props_obj.push('}');
                        state.props_section = Some(props_obj);
                    } else if let Some(arg_span) = defaults_arg_span {
                        // Variable reference: convert at runtime using IIFE
                        let defaults_src =
                            &content_str[arg_span.start as usize..arg_span.end as usize];
                        state.props_section = Some(format!(
                            "((d)=>{{const p={{}};for(const k in d)p[k]={{default:d[k]}};return p}})({})",
                            defaults_src
                        ));
                    }
                }
            }

            // Replace macro call
            if declarator.is_some() {
                ctx.out.overwrite(abs_start, abs_end, "__props");
            } else {
                ctx.out.overwrite(abs_start, abs_end, "");
            }
        }
    }
}

// ======================== Companion script processing ========================

/// Process the companion `<script>` block when `<script setup>` is present.
///
/// The companion script's tags are already stripped by `compile.rs`, so its
/// content remains in the output. This function:
/// 1. Finds `export default { ... }` and removes it (to avoid duplicate exports)
/// 2. Extracts the object's inner content as component-level options (like
///    `defineOptions`)
/// 3. Extracts type declarations (interfaces, type aliases) for cross-block
///    type resolution in `defineProps<T>()`
/// 4. Extracts non-type import binding names for template resolution
///
/// Returns `(companion_types, companion_import_names)`.
pub(super) fn process_companion_script(
    script: &RootNodeScript,
    source: &str,
    out: &mut CodeGenOutput<'_>,
    macro_state: &mut MacroState,
) -> (FxHashMap<String, ResolvedElements>, Vec<String>) {
    let content_span = match &script.content {
        Some(span) => span,
        None => return (FxHashMap::default(), Vec::new()),
    };

    let content_start = content_span.start;
    let content_str = &source[content_span.start as usize..content_span.end as usize];

    // Parse with OXC to find the default export
    let oxc_alloc = oxc_allocator::Allocator::default();
    let source_type = SourceType::tsx();
    let parser_ret = Parser::new(&oxc_alloc, content_str, source_type).parse();
    let parse_result = parse_script(
        &parser_ret.program,
        ScriptMode::Options,
        content_start,
        content_str,
    );

    // Extract type declarations for cross-block type resolution
    let companion_types =
        extract_companion_types(&parser_ret.program, content_str.as_bytes(), content_start);

    // Collect non-type import binding names for template resolution
    let mut companion_import_names = Vec::new();

    for item in &parse_result.items {
        match item {
            ScriptItem::Import(imp) => {
                // Skip type-only imports — they don't exist at runtime
                if !imp.is_type_only {
                    for binding in &imp.bindings {
                        if !binding.is_type_only {
                            companion_import_names.push(binding.name.to_string());
                        }
                    }
                }
            }
            ScriptItem::DefaultExport(de) => {
                let abs_start = content_start + de.span.start;
                let abs_end = content_start + de.span.end;

                // Extract options from `export default { inheritAttrs: false }` etc.
                if let Some(obj_span) = &de.object_span {
                    let obj_start = content_start + obj_span.start;
                    let obj_end = content_start + obj_span.end;
                    let obj_text = &source[obj_start as usize..obj_end as usize];
                    // Strip outer braces: "{ inheritAttrs: false }" → "inheritAttrs: false"
                    if obj_text.starts_with('{') && obj_text.ends_with('}') {
                        let inner = obj_text[1..obj_text.len() - 1].trim();
                        let inner = inner.trim_end_matches(',').trim_end();
                        if !inner.is_empty() && macro_state.options_section.is_none() {
                            macro_state.options_section = Some(inner.to_string());
                        }
                    }
                }

                // Remove the entire `export default { ... }` statement
                out.overwrite(abs_start, abs_end, "");
            }
            _ => {}
        }
    }

    (companion_types, companion_import_names)
}

// ======================== Prop name extraction ========================

/// Extract property key names from a defineProps object literal text.
///
/// Given text like `{ title: String, count: Number }`, extracts "title" and "count"
/// and inserts them into bindings as `BindingType::Props`.
///
/// Uses the full `content_str` with `obj_offset` to get `&'a str` slices with the
/// correct lifetime (tied to source).
pub(super) fn extract_object_prop_names<'a>(
    _obj_text: &str,
    content_str: &'a str,
    obj_offset: u32,
    bindings: &mut FxHashMap<&'a str, BindingType>,
) {
    // Re-parse the object expression to extract property keys reliably.
    // We parse just the object text as an expression statement.
    let oxc_alloc = oxc_allocator::Allocator::default();
    let expr_src = &content_str[obj_offset as usize..];
    // Find the end of the object expression (matching brace)
    let obj_end = find_matching_brace(expr_src);
    if obj_end == 0 {
        return;
    }
    let obj_src = &expr_src[..obj_end];
    // Wrap in parens to make it a valid expression statement
    let wrapped = format!("({})", obj_src);
    let source_type = SourceType::tsx();
    let parser_ret = Parser::new(&oxc_alloc, &wrapped, source_type).parse();
    // Walk the parsed AST to find property keys
    for stmt in &parser_ret.program.body {
        if let oxc_ast::ast::Statement::ExpressionStatement(es) = stmt {
            if let oxc_ast::ast::Expression::ParenthesizedExpression(paren) = &es.expression {
                if let oxc_ast::ast::Expression::ObjectExpression(obj) = &paren.expression {
                    for prop_kind in &obj.properties {
                        if let oxc_ast::ast::ObjectPropertyKind::ObjectProperty(p) = prop_kind {
                            if let oxc_ast::ast::PropertyKey::StaticIdentifier(ident) = &p.key {
                                // ident.span is relative to `wrapped`, offset by 1 for the opening paren
                                let name_start = obj_offset + ident.span.start - 1; // -1 for wrapping paren
                                let name_end = obj_offset + ident.span.end - 1;
                                let name = &content_str[name_start as usize..name_end as usize];
                                bindings.insert(name, BindingType::Props);
                            }
                        }
                    }
                }
            }
        }
    }
}

/// Find the position of the matching closing brace for text starting with `{`.
pub(super) fn find_matching_brace(s: &str) -> usize {
    if !s.starts_with('{') {
        return 0;
    }
    let mut depth = 0i32;
    for (i, ch) in s.char_indices() {
        match ch {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return i + 1;
                }
            }
            _ => {}
        }
    }
    0
}

/// Extract prop names from a defineProps array literal text.
///
/// Given text like `['title', 'count']`, extracts "title" and "count"
/// from content_str with correct lifetime.
pub(super) fn extract_array_prop_names<'a>(
    arr_text: &str,
    content_str: &'a str,
    arr_offset: u32,
    bindings: &mut FxHashMap<&'a str, BindingType>,
) {
    // Simple parsing: find string literals in the array
    let mut i = 0;
    let bytes = arr_text.as_bytes();
    while i < bytes.len() {
        if bytes[i] == b'\'' || bytes[i] == b'"' {
            let quote = bytes[i];
            let start = i + 1;
            i += 1;
            while i < bytes.len() && bytes[i] != quote {
                if bytes[i] == b'\\' && i + 1 < bytes.len() {
                    i += 1; // skip escaped character
                }
                i += 1;
            }
            if i < bytes.len() {
                // Found a string literal from start..i
                let abs_start = arr_offset as usize + start;
                let abs_end = arr_offset as usize + i;
                if abs_end <= content_str.len() {
                    let name = &content_str[abs_start..abs_end];
                    bindings.insert(name, BindingType::Props);
                }
            }
        }
        i += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_array_prop_names_basic() {
        let arr_text = "['title', \"count\"]";
        let content_str = arr_text;
        let mut bindings = FxHashMap::default();
        extract_array_prop_names(arr_text, content_str, 0, &mut bindings);
        assert_eq!(bindings.len(), 2);
        assert!(bindings.contains_key("title"));
        assert!(bindings.contains_key("count"));
    }

    #[test]
    fn test_extract_array_prop_names_escaped_quote() {
        // Prop name with escaped quote should be extracted correctly
        let arr_text = r#"['foo\'bar']"#;
        let content_str = arr_text;
        let mut bindings = FxHashMap::default();
        extract_array_prop_names(arr_text, content_str, 0, &mut bindings);
        assert_eq!(bindings.len(), 1);
        assert!(
            bindings.contains_key(r"foo\'bar"),
            "Expected foo\\'bar, got: {:?}",
            bindings.keys().collect::<Vec<_>>()
        );
    }
}
