//! Code generation for `defineEmits` macro.
//!
//! Transforms:
//! - `defineEmits<{ (e: 'change'): void }>()` → extracts emit names from call signatures
//! - `defineEmits<{ change: [id: number] }>()` → extracts emit names from shorthand properties
//! - `defineEmits(['change', 'update'])` → passes through array
//! - `defineEmits({ change: (id: number) => true })` → passes through object

use crate::code_transform::CodeTransform;
use crate::common::Span;
use crate::syntax::plugins::code_gen::script::macros::types::MacroProcessReturn;
use crate::utils::oxc::vue::{MacroArrayArg, MacroDeclarator, MacroObjectArg, MacroTypeParams};

/// Process a `defineEmits` macro call.
///
/// The macro call is replaced with `__emit`, which is the emit function
/// from the setup context. For type-based emits, the emit names are extracted
/// and can be used for runtime validation.
pub fn process_define_emits<'a>(
    span: &Span,
    declarator: &Option<MacroDeclarator<'a>>,
    type_params: &Option<MacroTypeParams>,
    object_arg: &Option<MacroObjectArg<'a>>,
    array_arg: &Option<MacroArrayArg>,
    code_transform: &mut CodeTransform,
    _source: &str,
) -> Option<MacroProcessReturn> {
    let has_declarator = declarator.is_some();

    // Handle type params - extract emit names from the resolved emits
    if let Some(tp) = type_params {
        if !tp.resolved.emits.is_empty() {
            // Build the emit names array: ["change", "update"]
            let emit_names: Vec<String> = tp
                .resolved
                .emits
                .iter()
                .map(|e| format!("\"{}\"", e.name))
                .collect();
            let emit_array = format!("[{}]", emit_names.join(","));

            // Overwrite the type params with the generated array
            code_transform.overwrite(tp.lt_span.start, tp.gt_span.end, &emit_array);

            // Replace the macro name with __emit, keeping the parens and generated array
            if has_declarator {
                code_transform.overwrite(span.start, tp.lt_span.start, "__emit");
            } else {
                // No declarator - remove the macro call but keep the emits for moving
                code_transform.overwrite(span.start, tp.lt_span.start, "");
            }

            // Remove the empty parens at the end: ()
            code_transform.remove(tp.gt_span.end, span.end);

            return Some(MacroProcessReturn {
                move_span: Some(Span {
                    start: tp.lt_span.start,
                    end: tp.gt_span.end,
                }),
                overwrite_span: None,
                remove: None,
            });
        } else {
            // No emits resolved from type params
            if has_declarator {
                code_transform.overwrite(span.start, span.end, "__emit");
            } else {
                code_transform.remove(span.start, span.end);
            }
        }

        return None;
    }

    // Handle array argument - replace macro with __emit, keep array for move
    if let Some(arr) = array_arg {
        if has_declarator {
            code_transform.overwrite(span.start, arr.span.start, "__emit");
        } else {
            // No declarator - remove the macro call but keep the array for moving
            code_transform.overwrite(span.start, arr.span.start, "");
        }
        code_transform.remove(arr.span.end, span.end);

        return Some(MacroProcessReturn {
            move_span: Some(Span {
                start: arr.span.start,
                end: arr.span.end,
            }),
            overwrite_span: None,
            remove: None,
        });
    }

    // Handle object argument - replace macro with __emit, keep object for move
    if let Some(obj) = object_arg {
        if has_declarator {
            code_transform.overwrite(span.start, obj.span.start, "__emit");
        } else {
            // No declarator - remove the macro call but keep the object for moving
            code_transform.overwrite(span.start, obj.span.start, "");
        }
        code_transform.remove(obj.span.end, span.end);

        return Some(MacroProcessReturn {
            move_span: Some(Span {
                start: obj.span.start,
                end: obj.span.end,
            }),
            overwrite_span: None,
            remove: None,
        });
    }

    // No arguments - just handle the macro call itself
    if has_declarator {
        code_transform.overwrite(span.start, span.end, "__emit");
    } else {
        code_transform.remove(span.start, span.end);
    }
    None
}
