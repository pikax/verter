//! Code generation for `defineProps` and `withDefaults` macros.
//!
//! Transforms TypeScript type parameters into Vue runtime prop definitions:
//! - `defineProps<{ foo: string }>()` → `{ foo: { type: String, required: true } }`
//! - `defineProps<{ foo?: string }>()` → `{ foo: { type: String, required: false } }`
//! - `withDefaults(defineProps<{ foo?: string }>(), { foo: 'bar' })`
//!   → `{ foo: { type: String, required: false, default: 'bar' } }`

use crate::code_transform::CodeTransform;
use crate::codegen::vue::macros::types::MacroProcessReturn;
use crate::common::Span;
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
    // TODO add error
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
        // Type couldn't be resolved (e.g., interface reference like `defineProps<Props>()`)
        // Generate empty props object `{}` as fallback
        // TODO: In the future, we should resolve the interface definition and generate proper props
        code_transform.overwrite(macro_span.start, macro_span.end, "__props;");
        return MacroProcessReturn {
            move_span: Some(Span {
                // Empty span - nothing to move for props
                start: macro_span.end,
                end: macro_span.end,
            }),
            overwrite_span: None,
            remove: None,
        };
    }

    // We have resolved props - transform each property to runtime format

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
        move_span: Some(Span {
            start: type_params.lt_span.start,
            end: type_params.gt_span.end,
        }),
        overwrite_span: None,
        remove: None,
    }
}
