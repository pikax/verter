//! Code generation for `defineProps` and `withDefaults` macros.
//!
//! Transforms TypeScript type parameters into Vue runtime prop definitions:
//! - `defineProps<{ foo: string }>()` → `{ foo: { type: String, required: true } }`
//! - `defineProps<{ foo?: string }>()` → `{ foo: { type: String, required: false } }`
//! - `withDefaults(defineProps<{ foo?: string }>(), { foo: 'bar' })`
//!   → `{ foo: { type: String, required: false, default: 'bar' } }`

use crate::code_transform::CodeTransform;
use crate::common::Span;
use crate::syntax::plugins::code_gen::script::macros::types::MacroProcessReturn;
use crate::utils::oxc::vue::{
    format_runtime_types, MacroArrayArg, MacroDeclarator, MacroObjectArg, MacroTypeParams,
};

/// Process a `defineProps` macro call.
///
/// Transforms TypeScript type parameters into Vue runtime prop definitions.
/// Each property `foo: string` becomes `foo: { type: String, required: true }`.
#[allow(clippy::too_many_arguments)]
pub fn process_define_props<'a>(
    span: &Span,
    _declarator: &Option<MacroDeclarator<'a>>,
    type_params: &Option<MacroTypeParams>,
    object_arg: &Option<MacroObjectArg<'a>>,
    array_arg: &Option<MacroArrayArg>,
    code_transform: &mut CodeTransform,
    source: &str,
    is_production: bool,
) -> Option<MacroProcessReturn> {
    if let Some(type_params) = type_params {
        return Some(transform_type_params_to_runtime(
            span,
            type_params,
            None, // no defaults
            code_transform,
            source,
            is_production,
        ));
    } else if let Some(obj) = object_arg {
        return Some(MacroProcessReturn {
            move_span: Some(Span {
                start: obj.span.start,
                end: obj.span.end,
            }),
            overwrite_span: Some((
                Span {
                    start: span.start,
                    end: span.end,
                },
                "__props;".to_string(),
            )),
            remove: None,
        });
    } else if let Some(arr) = array_arg {
        return Some(MacroProcessReturn {
            move_span: Some(Span {
                start: arr.span.start,
                end: arr.span.end,
            }),
            overwrite_span: Some((
                Span {
                    start: span.start,
                    end: span.end,
                },
                "__props;".to_string(),
            )),
            remove: None,
        });
    }
    // Fallback: argument is an expression (e.g., defineProps(createAdProps())).
    // Extract the argument span by finding the first '(' and using span.end - 1 as ')'.
    let call_str = &source[span.start as usize..span.end as usize];
    if let Some(paren_pos) = call_str.find('(') {
        let arg_start = span.start + paren_pos as u32 + 1;
        let arg_end = span.end - 1; // skip the closing )
        if arg_start < arg_end {
            return Some(MacroProcessReturn {
                move_span: Some(Span {
                    start: arg_start,
                    end: arg_end,
                }),
                overwrite_span: Some((
                    Span {
                        start: span.start,
                        end: span.end,
                    },
                    "__props;".to_string(),
                )),
                remove: None,
            });
        }
    }
    Some(MacroProcessReturn {
        move_span: Some(Span {
            start: span.start,
            end: span.end,
        }),
        overwrite_span: None,
        remove: None,
    })
}

/// Process a `withDefaults` macro call.
///
/// Transforms TypeScript type parameters into Vue runtime prop definitions,
/// incorporating default values from the second argument.
pub fn process_with_defaults<'a>(
    span: Span,
    _declarator: &Option<MacroDeclarator<'a>>,
    type_params: Option<&MacroTypeParams>,
    defaults: Option<&MacroObjectArg<'a>>,
    code_transform: &mut CodeTransform,
    source: &str,
    is_production: bool,
) -> Option<MacroProcessReturn> {
    if let Some(tp) = type_params {
        return Some(transform_type_params_to_runtime(
            &span,
            tp,
            defaults,
            code_transform,
            source,
            is_production,
        ));
    }
    // TODO: emit diagnostic — withDefaults requires type parameters (defineProps<T>).
    // Currently returns None which skips the transform silently.
    // Needs ctx access or a diagnostics vec parameter to report the error.
    None
}

/// Transform TypeScript type parameters into Vue runtime prop definitions.
///
/// For each property in the type literal:
/// - `foo: string` → `foo: { type: String, required: true }`
/// - `foo?: string` → `foo: { type: String, required: false }`
/// - With default: `foo: { type: String, required: false, default: <moved value> }`
///
/// If the type couldn't be resolved (e.g., `defineProps<Props>()` where Props is an interface),
/// generates an empty props object `{}` as a fallback.
fn transform_type_params_to_runtime<'a>(
    macro_span: &Span,
    type_params: &MacroTypeParams,
    defaults: Option<&MacroObjectArg<'a>>,
    code_transform: &mut CodeTransform,
    source: &str,
    is_production: bool,
) -> MacroProcessReturn {
    // Check if we have resolved props from the type literal
    if type_params.resolved.props.is_empty() {
        // Type couldn't be resolved (e.g., imported type like `defineProps<ExternalProps>()`)
        // Replace macro with __props and emit empty props object as fallback
        code_transform.overwrite(macro_span.start, macro_span.end, "__props;");
        return MacroProcessReturn {
            move_span: None,
            overwrite_span: None,
            remove: None,
        };
    }

    // We have resolved props - transform each property to runtime format

    // Check if resolved props are "inline" (their spans are within type_span)
    // or "external" (resolved from an interface/type alias body at a different location).
    let props_are_inline = type_params.resolved.props.iter().all(|p| {
        p.span.start >= type_params.type_span.start && p.span.end <= type_params.type_span.end
    });

    if !props_are_inline {
        // External type reference (e.g., defineProps<Props>() where Props is an interface).
        // The resolved prop spans point to the interface/type alias body, not between < and >.
        // We build the complete props object as a string instead of using individual overwrites.
        return transform_external_type_to_runtime(
            macro_span,
            type_params,
            defaults,
            code_transform,
            source,
            is_production,
        );
    }

    // Inline type literal — props are between < and >, use move_span + individual overwrites.

    // Overwrite the macro call start (e.g., "defineProps<" or "withDefaults(defineProps<")
    // with just "__props;" to capture the props reference
    code_transform.overwrite(macro_span.start, type_params.type_span.start, "__props;");

    // Transform each property in the type literal
    for prop in &type_params.resolved.props {
        let key_name = &source[prop.key.start as usize..prop.key.end as usize];

        if is_production {
            // Production mode: strip type info, emit just `propName: {},`
            // Vue's compiler omits type/required in production for smaller bundle size.
            // However, defaults must still be preserved.
            let default_value_span = defaults.and_then(|d| {
                d.properties
                    .iter()
                    .find(|p| p.name == key_name)
                    .and_then(|p| p.value_span)
            });

            if let Some(default_span) = default_value_span {
                let prefix = format!("{}: {{ default: ", key_name);
                code_transform.overwrite(prop.span.start, prop.span.end, &prefix);
                code_transform.move_wrapped(
                    default_span.start,
                    default_span.end,
                    prop.span.end,
                    "",
                    " },",
                );
            } else {
                let replacement = format!("{}: {{}},", key_name);
                code_transform.overwrite(prop.span.start, prop.span.end, &replacement);
            }
        } else {
            let type_str = format_runtime_types(&prop.types);
            let required = !prop.optional;

            // Find if there's a default for this prop
            let default_value_span = defaults.and_then(|d| {
                d.properties
                    .iter()
                    .find(|p| p.name == key_name)
                    .and_then(|p| p.value_span)
            });

            if let Some(default_span) = default_value_span {
                // Has default: overwrite prop signature, then move default value with closing brace
                // Result: `foo: { type: String, required: false, default: <value> }`
                let prefix = format!(
                    "{}: {{ type: {}, required: {}, default: ",
                    key_name, type_str, required
                );
                code_transform.overwrite(prop.span.start, prop.span.end, &prefix);
                // Move the default value with closing brace suffix
                code_transform.move_wrapped(
                    default_span.start,
                    default_span.end,
                    prop.span.end,
                    "",    // no prefix needed
                    " },", // closing brace as suffix
                );
            } else {
                // No default: just overwrite the prop signature
                // Result: `foo: { type: String, required: true }`
                let replacement = format!(
                    "{}: {{ type: {}, required: {} }},",
                    key_name, type_str, required
                );
                code_transform.overwrite(prop.span.start, prop.span.end, &replacement);
            }
        }
    }

    // Remove the closing part of the macro call (e.g., ">()" or ">(), { defaults })")
    code_transform.overwrite(type_params.type_span.end, macro_span.end, "");
    MacroProcessReturn {
        // Move only the type content (between < and >), not the delimiters themselves.
        // The delimiters are already handled by the overwrites above.
        move_span: Some(type_params.type_span),
        overwrite_span: None,
        remove: None,
    }
}

/// Transform an external type reference (interface/type alias) to runtime props.
///
/// Unlike inline type literals where we can overwrite prop spans in place,
/// external type references have their prop definitions in a different source location
/// (the interface/type alias body). We build the complete props object as a string
/// and inject it via `overwrite_span`.
fn transform_external_type_to_runtime<'a>(
    macro_span: &Span,
    type_params: &MacroTypeParams,
    defaults: Option<&MacroObjectArg<'a>>,
    code_transform: &mut CodeTransform,
    source: &str,
    is_production: bool,
) -> MacroProcessReturn {
    // Build the complete props object string
    let mut props_str = String::from("{");

    for (i, prop) in type_params.resolved.props.iter().enumerate() {
        let key_name = &source[prop.key.start as usize..prop.key.end as usize];

        if is_production {
            let default_value_span = defaults.and_then(|d| {
                d.properties
                    .iter()
                    .find(|p| p.name == key_name)
                    .and_then(|p| p.value_span)
            });

            if let Some(default_span) = default_value_span {
                let default_value = &source[default_span.start as usize..default_span.end as usize];
                props_str.push_str(&format!("{}: {{ default: {} }}", key_name, default_value));
            } else {
                props_str.push_str(&format!("{}: {{}}", key_name));
            }
        } else {
            let type_str = format_runtime_types(&prop.types);
            let required = !prop.optional;

            let default_value_span = defaults.and_then(|d| {
                d.properties
                    .iter()
                    .find(|p| p.name == key_name)
                    .and_then(|p| p.value_span)
            });

            if let Some(default_span) = default_value_span {
                let default_value = &source[default_span.start as usize..default_span.end as usize];
                props_str.push_str(&format!(
                    "{}: {{ type: {}, required: {}, default: {} }}",
                    key_name, type_str, required, default_value
                ));
            } else {
                props_str.push_str(&format!(
                    "{}: {{ type: {}, required: {} }}",
                    key_name, type_str, required
                ));
            }
        }

        if i < type_params.resolved.props.len() - 1 {
            props_str.push_str(", ");
        }
    }

    props_str.push('}');

    // Overwrite non-overlapping ranges:
    // 1. From macro start to type_span start (the "defineProps<" part) → "__props;"
    code_transform.overwrite(macro_span.start, type_params.type_span.start, "__props;");
    // 2. The type content between < and > → the built props object
    code_transform.overwrite(
        type_params.type_span.start,
        type_params.type_span.end,
        &props_str,
    );
    // 3. From type_span end to macro end (the ">()" or ">(), { defaults })") → empty
    code_transform.overwrite(type_params.type_span.end, macro_span.end, "");

    // emit_props_section will move the type_span content (now our built props string)
    // to the insert position with "props:" prefix.
    MacroProcessReturn {
        move_span: Some(type_params.type_span),
        overwrite_span: None,
        remove: None,
    }
}
