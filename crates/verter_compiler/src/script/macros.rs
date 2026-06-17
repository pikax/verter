//! Vue macro processing for `<script setup>` and companion `<script>` blocks.
//!
//! Handles `defineProps`, `defineEmits`, `defineModel`, `defineOptions`,
//! `defineExpose`, `defineSlots`, and `withDefaults`. Each macro is replaced
//! with its runtime equivalent and its metadata is accumulated in [`MacroState`]
//! for the component definition builder.
//!
//! Also processes companion `<script>` blocks to extract `export default`
//! options and type declarations for cross-block type resolution.

use rustc_hash::FxHashMap;

use super::prepared::PreparedCompanion;
use crate::template::code_gen::binding::BindingType;
use crate::template::code_gen::types::CodeGenOutput;
use crate::utils::oxc::script::type_surface::format_runtime_types;
use crate::utils::oxc::vue::{ScriptItem, ScriptMacro};

use super::ScriptContext;

/// Force-js-stripped text for a macro-argument expression, keyed by its
/// content-local `(start, end)` span. Built once per setup parse (see
/// [`super::process`]); `None` when TS types are kept.
pub(super) type StrippedSections = FxHashMap<(u32, u32), String>;

/// Return the section source for the macro-argument expression spanning
/// `[start, end)` (content-local): the force-js-stripped text when the caller
/// supplied a stripped-sections map containing it, otherwise the raw slice.
///
/// The synthesized props/emits sections copy macro arguments verbatim, and the
/// macro call range is overwritten before the whole-program force-js pass runs,
/// so a section is stripped here — at the point it is produced — keyed by the
/// exact span the synthesis slices.
fn section_text<'a>(
    start: u32,
    end: u32,
    content_str: &'a str,
    stripped: Option<&'a StrippedSections>,
) -> &'a str {
    stripped
        .and_then(|m| m.get(&(start, end)))
        .map(String::as_str)
        .unwrap_or(&content_str[start as usize..end as usize])
}

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
    /// Model entries from `defineModel()` calls — each needs a prop and emit declaration.
    /// Tuple of (model_name, optional_options_source).
    pub model_names: Vec<(String, Option<String>)>,
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
/// adds them to bindings with `BindingType::Props`. The names come from the
/// macro AST surfaced by the single parse (object property keys and array
/// string-literal elements), not from `parse_result.bindings`, whose Props have
/// inconsistent span coordinate systems (object-syntax keys are SFC-absolute,
/// while array-syntax keys are content-relative).
#[cfg_attr(feature = "hotpath", hotpath::measure)]
pub(super) fn process_macro_item<'a>(
    mac: &ScriptMacro<'a>,
    content_start: u32,
    content_str: &'a str,
    ctx: &mut ScriptContext<'a>,
    state: &mut MacroState,
    stripped: Option<&StrippedSections>,
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

            // Extract props section from runtime argument and prop names for
            // bindings. Prop names come from the macro AST surfaced by the single
            // parse — the object property keys and the array string-literal
            // elements — not from a second text reparse.
            if let Some(obj) = object_arg {
                let obj_text = section_text(obj.span.start, obj.span.end, content_str, stripped);
                state.props_section = Some(obj_text.to_string());
                for prop in &obj.properties {
                    ctx.bindings.insert(prop.name, BindingType::Props);
                }
            } else if let Some(arr) = array_arg {
                let arr_text = section_text(arr.span.start, arr.span.end, content_str, stripped);
                state.props_section = Some(arr_text.to_string());
                for elem in &arr.elements {
                    if let Some(name) = elem.name {
                        ctx.bindings.insert(name, BindingType::Props);
                    }
                }
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
                let obj_text = section_text(obj.span.start, obj.span.end, content_str, stripped);
                state.emits_section = Some(obj_text.to_string());
            } else if let Some(arr) = array_arg {
                let arr_text = section_text(arr.span.start, arr.span.end, content_str, stripped);
                state.emits_section = Some(arr_text.to_string());
            }

            // Type-based defineEmits: extract emit event names from resolved type
            if let Some(tp) = type_params {
                if !tp.resolved.call_signatures.is_empty() {
                    let mut emit_names: Vec<String> = Vec::new();
                    for emit in &tp.resolved.call_signatures {
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
            span,
            name_span,
            options_span,
            ..
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

            // Extract options object source (e.g., `{ type: Boolean, default: false }`)
            let options_src =
                options_span.map(|os| content_str[os.start as usize..os.end as usize].to_string());

            // Track this model name + options for prop/emit declaration generation
            state
                .model_names
                .push((model_name.to_string(), options_src));

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
                        // Use pre-resolved key_name for external types (where spans
                        // reference a different source), span extraction for intra-file.
                        let name: &str = if let Some(ref kn) = prop.key_name {
                            kn.as_str()
                        } else {
                            let key_start = prop.key.start as usize;
                            let key_end = prop.key.end as usize;
                            if key_end > ctx.source.len() {
                                continue;
                            }
                            &ctx.source[key_start..key_end]
                        };
                        ctx.bindings
                            .insert(ctx.alloc.alloc_str(name), BindingType::Props);

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
                                .map(|vs| section_text(vs.start, vs.end, content_str, stripped))
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
                                .map(|vs| section_text(vs.start, vs.end, content_str, stripped))
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
                            section_text(arg_span.start, arg_span.end, content_str, stripped);
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

/// Apply the companion `<script>` codegen when `<script setup>` is present.
///
/// The companion script's tags are already stripped by `compile.rs`, so its
/// content remains in the output. This function:
/// 1. Finds `export default { ... }` and removes it (to avoid duplicate exports)
/// 2. Extracts the object's inner content as component-level options (like
///    `defineOptions`)
/// 3. Collects non-type import binding names for template resolution
///
/// The companion was parsed once when the prepared script was built, and its
/// type declarations were already folded into the setup parse, so this reads the
/// prepared parse facts rather than re-parsing. Returns the companion import
/// names.
pub(super) fn process_companion_script(
    prepared: &PreparedCompanion<'_>,
    source: &str,
    out: &mut CodeGenOutput<'_>,
    macro_state: &mut MacroState,
) -> Vec<String> {
    let content_start = prepared.content_start();
    let parse_result = prepared.parse_result();

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

    companion_import_names
}
