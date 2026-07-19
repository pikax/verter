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
    PropsRuntimeShape, RuntimeConstructor, RuntimeProp,
};

use super::prepared::PreparedCompanion;
use crate::template::code_gen::binding::BindingType;
use crate::template::code_gen::shared::helpers::escape_js_string_into;
use crate::template::code_gen::types::CodeGenOutput;
use crate::template::code_gen::vdom::props::needs_quoted_key;
use crate::utils::oxc::vue::{MacroObjectArg, MacroProperty};
use crate::utils::oxc::vue::{ScriptItem, ScriptMacro};

use super::ScriptContext;

/// Emit a runtime props object key, quoting when the name is not a bare JS
/// identifier (e.g. `onUpdate:visible`, `aria-label`). Unquoted colon keys
/// are a parse error in the generated module (element-plus tooltip, etc.).
pub(super) fn push_js_string_literal(buf: &mut String, value: &str) {
    buf.push('"');
    escape_js_string_into(buf, value);
    buf.push('"');
}

pub(super) fn js_string_literal(value: &str) -> String {
    let mut literal = String::with_capacity(value.len().saturating_add(2));
    push_js_string_literal(&mut literal, value);
    literal
}

pub(super) fn push_runtime_prop_key(buf: &mut String, name: &str) {
    if needs_quoted_key(name) {
        push_js_string_literal(buf, name);
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

#[derive(Clone, Copy)]
struct RuntimePropProfile {
    is_production: bool,
    custom_element: bool,
}

fn runtime_type_expression(prop: &RuntimeProp) -> (Vec<RuntimeConstructor>, String) {
    let constructors = prop
        .type_shape
        .constructors()
        .map(verter_macro_dto::OrderedRuntimeConstructors::as_slice)
        .unwrap_or_default()
        .to_vec();
    let runtime_expressions: Vec<&str> = constructors
        .iter()
        .filter_map(|constructor| constructor.as_runtime_expression())
        .collect();
    let runtime_type = match runtime_expressions.as_slice() {
        [] => "null".to_string(),
        [constructor] => (*constructor).to_string(),
        constructors => format!("[{}]", constructors.join(", ")),
    };
    (constructors, runtime_type)
}

fn render_runtime_prop_options(
    prop: &RuntimeProp,
    profile: RuntimePropProfile,
    retain_function_in_production: bool,
    static_default: Option<&str>,
) -> String {
    let (constructors, runtime_type) = runtime_type_expression(prop);

    if profile.is_production {
        let retains_type = constructors.contains(&RuntimeConstructor::Boolean)
            || (retain_function_in_production
                && constructors.contains(&RuntimeConstructor::Function));
        if retains_type {
            let mut fields = vec![format!("type: {runtime_type}")];
            fields.extend(static_default.map(str::to_owned));
            return format!("{{ {} }}", fields.join(", "));
        }
        if profile.custom_element {
            let mut fields = Vec::with_capacity(2);
            fields.extend(static_default.map(str::to_owned));
            fields.push(format!("type: {runtime_type}"));
            return format!("{{ {} }}", fields.join(", "));
        }
        return static_default
            .map(|default| format!("{{ {default} }}"))
            .unwrap_or_else(|| "{}".to_owned());
    }

    let mut fields = vec![format!("type: {runtime_type}")];
    fields.push(format!("required: {}", !prop.optional));
    if prop.type_shape.skip_check() {
        fields.push("skipCheck: true".to_owned());
    }
    fields.extend(static_default.map(str::to_owned));
    format!("{{ {} }}", fields.join(", "))
}

#[derive(Default)]
struct StaticPropDefaults {
    by_name: FxHashMap<String, String>,
}

fn render_runtime_props(
    shape: &PropsRuntimeShape,
    profile: RuntimePropProfile,
    static_defaults: Option<&StaticPropDefaults>,
) -> String {
    let mut out = String::from("{\n");
    for prop in &shape.props {
        let static_default = static_defaults.and_then(|defaults| defaults.by_name.get(&prop.name));
        // Official Vue retains Function in production when defaults are
        // dynamic, or when this exact statically-defaulted row has a default.
        let retain_function = static_defaults.is_none() || static_default.is_some();
        out.push_str("    ");
        push_runtime_prop_key(&mut out, &prop.name);
        out.push_str(": ");
        out.push_str(&render_runtime_prop_options(
            prop,
            profile,
            retain_function,
            static_default.map(String::as_str),
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
    let (_, runtime_type) = runtime_type_expression(&model.prop);
    let constructors = model
        .prop
        .type_shape
        .constructors()
        .map(verter_macro_dto::OrderedRuntimeConstructors::as_slice)
        .unwrap_or_default();
    let keep_type = !is_production
        || constructors.contains(&RuntimeConstructor::Boolean)
        || (syntax_options.is_some() && constructors.contains(&RuntimeConstructor::Function));
    let mut type_fields = Vec::new();
    if keep_type {
        type_fields.push(format!("type: {runtime_type}"));
        if !is_production && model.prop.type_shape.skip_check() {
            type_fields.push("skipCheck: true".to_owned());
        }
    }
    let base = if type_fields.is_empty() {
        "{}".to_owned()
    } else {
        format!("{{ {} }}", type_fields.join(", "))
    };
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

fn render_static_defaults(
    defaults: &MacroObjectArg<'_>,
    content_str: &str,
    stripped: Option<&StrippedSections>,
) -> Option<StaticPropDefaults> {
    if !defaults.static_eligibility.is_eligible() {
        return None;
    }
    let mut rendered = StaticPropDefaults::default();
    for property in &defaults.properties {
        let value = render_static_default(property, content_str, stripped)?;
        // Official Vue selects the first matching default property.
        rendered
            .by_name
            .entry(property.name.to_owned())
            .or_insert(value);
    }
    Some(rendered)
}

fn render_static_default(
    property: &MacroProperty<'_>,
    content_str: &str,
    stripped: Option<&StrippedSections>,
) -> Option<String> {
    if !property.is_method {
        let value = match property.value_span {
            Some(span) => section_text(span.start, span.end, content_str, stripped),
            None => section_text(
                property.name_span.start,
                property.name_span.end,
                content_str,
                stripped,
            ),
        };
        return Some(format!("default: {value}"));
    }

    let value_span = property.value_span?;
    if property.property_span.start > property.name_span.start
        || property.name_span.end > value_span.start
        || value_span.end > property.property_span.end
    {
        return None;
    }
    let prefix = content_str
        .get(property.property_span.start as usize..property.name_span.start as usize)?;
    let between = content_str.get(property.name_span.end as usize..value_span.start as usize)?;
    let function = section_text(value_span.start, value_span.end, content_str, stripped);
    // A quoted key is valid for every object-method kind (`async`, getter,
    // setter, generator) and avoids reinterpreting computed-key punctuation.
    Some(format!("{prefix}\"default\"{between}{function}"))
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
    custom_element: bool,
) {
    let runtime_profile = RuntimePropProfile {
        is_production,
        custom_element,
    };
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
                        Some(render_runtime_props(shape, runtime_profile, None))
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
                            .map(|emit| js_string_literal(&emit.name))
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
            name,
            options_span,
            ..
        } => {
            let abs_start = content_start + span.start;
            let abs_end = content_start + span.end;

            // Public semantics use OXC's decoded string value. The authored
            // span is retained independently for diagnostics and source maps.
            let model_name = name.unwrap_or("modelValue");

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

            let replacement = format!("_useModel(__props, {})", js_string_literal(model_name));
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

            if define_props_type_params.is_some() {
                state.props_section = match (
                    runtime_shape(runtime_bundle, syntax_index),
                    defaults_arg_span,
                ) {
                    (Some(MacroRuntimeShape::Props(shape)), Some(defaults_span)) => {
                        register_runtime_props(shape, ctx);
                        if let Some(static_defaults) = defaults.as_ref().and_then(|defaults| {
                            render_static_defaults(defaults, content_str, stripped)
                        }) {
                            Some(render_runtime_props(
                                shape,
                                runtime_profile,
                                Some(&static_defaults),
                            ))
                        } else {
                            let defaults = section_text(
                                defaults_span.start,
                                defaults_span.end,
                                content_str,
                                stripped,
                            );
                            ctx.imports.push("_mergeDefaults");
                            Some(format!(
                                "_mergeDefaults({}, {})",
                                render_runtime_props(shape, runtime_profile, None),
                                defaults
                            ))
                        }
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
