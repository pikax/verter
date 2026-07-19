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
use verter_macro_dto::{
    MacroRuntimeBundle, MacroRuntimeOutcome, MacroRuntimeShape, ModelRuntimeShape,
    PropsDefaultsAssociation, PropsRuntimeShape, RuntimeConstructor, RuntimeProp,
};

use super::prepared::PreparedCompanion;
use crate::template::code_gen::binding::BindingType;
use crate::template::code_gen::types::CodeGenOutput;
use crate::template::code_gen::vdom::props::needs_quoted_key;
use crate::utils::oxc::vue::{ScriptItem, ScriptMacro};

use super::ScriptContext;

/// Emit a runtime props object key, quoting when the name is not a bare JS
/// identifier (e.g. `onUpdate:visible`, `aria-label`). Unquoted colon keys
/// are a parse error in the generated module (element-plus tooltip, etc.).
fn push_runtime_prop_key(buf: &mut String, name: &str) {
    if needs_quoted_key(name) {
        buf.push('"');
        for c in name.chars() {
            if c == '\\' || c == '"' {
                buf.push('\\');
            }
            buf.push(c);
        }
        buf.push('"');
    } else {
        buf.push_str(name);
    }
}

fn runtime_shape(
    bundle: Option<&MacroRuntimeBundle>,
    syntax_index: u32,
) -> Option<&MacroRuntimeShape> {
    let entry = bundle?
        .entries
        .iter()
        .find(|entry| entry.syntax_index == syntax_index)?;
    match &entry.outcome {
        MacroRuntimeOutcome::Complete(shape) => Some(shape),
        MacroRuntimeOutcome::Partial(_)
        | MacroRuntimeOutcome::Unresolved(_)
        | MacroRuntimeOutcome::Unsupported(_)
        | MacroRuntimeOutcome::Invalid(_) => None,
    }
}

fn render_runtime_prop_options(
    prop: &RuntimeProp,
    is_production: bool,
    retain_function_in_production: bool,
) -> String {
    let constructors = prop
        .type_shape
        .constructors()
        .map(verter_macro_dto::OrderedRuntimeConstructors::as_slice)
        .unwrap_or_default();
    let runtime_expressions: Vec<&str> = constructors
        .iter()
        .filter_map(|constructor| constructor.as_runtime_expression())
        .collect();
    let runtime_type = match runtime_expressions.as_slice() {
        [] => "null".to_string(),
        [constructor] => (*constructor).to_string(),
        constructors => format!("[{}]", constructors.join(", ")),
    };

    if is_production {
        let retains_type = constructors.contains(&RuntimeConstructor::Boolean)
            || (retain_function_in_production
                && constructors.contains(&RuntimeConstructor::Function));
        return if retains_type {
            format!("{{ type: {runtime_type} }}")
        } else {
            "{}".to_string()
        };
    }

    let mut options = format!("{{ type: {runtime_type}");
    if prop.type_shape.skip_check() {
        options.push_str(", skipCheck: true");
    }
    if !prop.optional {
        options.push_str(", required: true");
    }
    options.push_str(" }");
    options
}

fn render_runtime_props(shape: &PropsRuntimeShape, is_production: bool) -> String {
    let mut out = String::from("{\n");
    let retain_function = !matches!(
        shape.defaults,
        PropsDefaultsAssociation::WithDefaults { .. }
    );
    for prop in &shape.props {
        out.push_str("    ");
        push_runtime_prop_key(&mut out, &prop.name);
        out.push_str(": ");
        out.push_str(&render_runtime_prop_options(
            prop,
            is_production,
            retain_function,
        ));
        out.push_str(",\n");
    }
    out.push('}');
    out
}

fn register_runtime_props<'a>(shape: &PropsRuntimeShape, ctx: &mut ScriptContext<'a>) {
    for prop in &shape.props {
        ctx.bindings
            .insert(ctx.alloc.alloc_str(&prop.name), BindingType::Props);
    }
}

fn render_model_options(
    model: &ModelRuntimeShape,
    syntax_options: Option<&str>,
    is_production: bool,
) -> String {
    let base = render_runtime_prop_options(&model.prop, is_production, syntax_options.is_some());
    match syntax_options {
        Some(options) if base == "{}" => options.to_owned(),
        Some(options) => {
            let inner = base
                .strip_prefix('{')
                .and_then(|value| value.strip_suffix('}'))
                .unwrap_or(base.as_str())
                .trim();
            format!("{{ {inner}, ...({options}) }}")
        }
        None => base,
    }
}

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
    /// Authoritative model entries from the runtime semantic bundle.
    pub models: Vec<ModelSection>,
}

pub(super) struct ModelSection {
    pub prop_name: String,
    pub prop_options: String,
    pub modifiers_name: String,
    pub update_event: String,
}

impl MacroState {
    pub fn new() -> Self {
        Self {
            props_section: None,
            emits_section: None,
            options_section: None,
            has_expose: false,
            has_emit: false,
            models: Vec::new(),
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
    syntax_index: u32,
    content_start: u32,
    content_str: &'a str,
    ctx: &mut ScriptContext<'a>,
    state: &mut MacroState,
    stripped: Option<&StrippedSections>,
    runtime_bundle: Option<&MacroRuntimeBundle>,
    is_production: bool,
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

            if type_params.is_some() {
                state.props_section = match runtime_shape(runtime_bundle, syntax_index) {
                    Some(MacroRuntimeShape::Props(shape)) => {
                        register_runtime_props(shape, ctx);
                        Some(render_runtime_props(shape, is_production))
                    }
                    _ => None,
                };
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

            if type_params.is_some() {
                state.emits_section = match runtime_shape(runtime_bundle, syntax_index) {
                    Some(MacroRuntimeShape::Emits(emits)) => Some(format!(
                        "[{}]",
                        emits
                            .iter()
                            .map(|emit| format!("\"{}\"", emit.name))
                            .collect::<Vec<_>>()
                            .join(", ")
                    )),
                    _ => None,
                };
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
            type_params,
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

            let options_src =
                options_span.map(|span| section_text(span.start, span.end, content_str, stripped));

            if type_params.is_some() {
                if let Some(MacroRuntimeShape::Model(model)) =
                    runtime_shape(runtime_bundle, syntax_index)
                {
                    ctx.bindings
                        .insert(ctx.alloc.alloc_str(&model.prop.name), BindingType::Props);
                    state.models.push(ModelSection {
                        prop_name: model.prop.name.clone(),
                        prop_options: render_model_options(model, options_src, is_production),
                        modifiers_name: model.modifiers_prop.name.clone(),
                        update_event: model.update_event.name.clone(),
                    });
                }
            } else {
                state.models.push(ModelSection {
                    prop_name: model_name.to_string(),
                    prop_options: options_src.unwrap_or("{}").to_string(),
                    modifiers_name: if model_name == "modelValue" {
                        "modelModifiers".to_string()
                    } else {
                        format!("{model_name}Modifiers")
                    },
                    update_event: format!("update:{model_name}"),
                });
            }

            // Replace with _useModel(__props, 'name')
            let replacement = format!("_useModel(__props, '{}')", model_name);
            ctx.out.overwrite(abs_start, abs_end, &replacement);

            ctx.imports.push("_useModel");
        }

        ScriptMacro::WithDefaults {
            span,
            declarator,
            define_props_type_params,
            defaults_arg_span,
            ..
        } => {
            let abs_start = content_start + span.start;
            let abs_end = content_start + span.end;

            if define_props_type_params.is_some() {
                state.props_section = match (
                    runtime_shape(runtime_bundle, syntax_index),
                    defaults_arg_span,
                ) {
                    (Some(MacroRuntimeShape::Props(shape)), Some(defaults_span)) => {
                        register_runtime_props(shape, ctx);
                        let defaults = section_text(
                            defaults_span.start,
                            defaults_span.end,
                            content_str,
                            stripped,
                        );
                        ctx.imports.push("_mergeDefaults");
                        Some(format!(
                            "_mergeDefaults({}, {})",
                            render_runtime_props(shape, is_production),
                            defaults
                        ))
                    }
                    _ => None,
                };
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
/// 4. Collects local runtime declarations (`const`/`let`/`function`/`class`) so
///    template expressions like `isNumber(modelValue)` (reka-ui ProgressRoot)
///    resolve via `$setup` instead of missing `_ctx.isNumber`
///
/// The companion was parsed once when the prepared script was built, and its
/// type declarations were already folded into the setup parse, so this reads the
/// prepared parse facts rather than re-parsing. Returns companion binding names
/// (imports + local declarations) that setup should expose.
pub(super) fn process_companion_script(
    prepared: &PreparedCompanion<'_>,
    source: &str,
    out: &mut CodeGenOutput<'_>,
    macro_state: &mut MacroState,
) -> Vec<String> {
    let content_start = prepared.content_start();
    let parse_result = prepared.parse_result();

    // Collect non-type import binding names + local runtime declarations.
    let mut companion_binding_names = Vec::new();

    for item in &parse_result.items {
        match item {
            ScriptItem::Import(imp) => {
                // Skip type-only imports — they don't exist at runtime
                if !imp.is_type_only {
                    for binding in &imp.bindings {
                        if !binding.is_type_only {
                            companion_binding_names.push(binding.name.to_string());
                        }
                    }
                }
            }
            ScriptItem::Declaration(decl) => {
                // Top-level companion locals (const isNumber = …, function f(){})
                // are in module scope and must be re-exported from setup for
                // template access — same as companion imports.
                if let Some(name) = decl.name {
                    companion_binding_names.push(name.to_string());
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

    companion_binding_names
}
