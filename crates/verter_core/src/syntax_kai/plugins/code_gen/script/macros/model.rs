//! Code generation for `defineModel` macro.
//!
//! Transforms:
//! - `defineModel()` → `defineModel(__props, 'modelValue')`
//! - `defineModel('count')` → `defineModel(__props, 'count')`
//! - `defineModel('count', { type: Number })` → `defineModel(__props, 'count', { type: Number })`
//! - `defineModel<string>()` → `defineModel(__props, 'modelValue', { type: String })`
//! - `defineModel<string>('count')` → `defineModel(__props, 'count', { type: String })`

use crate::code_transform::CodeTransform;
use crate::common::Span;
use crate::syntax_kai::plugins::code_gen::script::macros::types::MacroProcessReturn;
use crate::utils::oxc::vue::{format_runtime_types, MacroDeclarator, MacroTypeParams};

/// Process a `defineModel` macro call.
///
/// The macro is transformed to use the runtime helper which
/// creates a two-way binding ref that syncs with the parent via v-model.
pub fn process_define_model<'a>(
    _declarator: &Option<MacroDeclarator<'a>>,
    span: Span,
    type_params: &Option<MacroTypeParams>,
    name_span: Option<&Span>,
    options_span: Option<&Span>,
    code_transform: &mut CodeTransform,
    source: &str,
) -> Option<MacroProcessReturn> {
    let mut move_start = 0;
    let mut move_end = 0;

    // Handle type params: transform into options object and use move_wrapped
    if let Some(tp) = type_params {
        if let Some(macro_type) = type_params {
            let runtime_type_str = format_runtime_types(&macro_type.runtime_types);
            code_transform.overwrite(tp.lt_span.end, tp.gt_span.start, runtime_type_str.as_str());
            code_transform.overwrite(tp.lt_span.start, tp.lt_span.end, "{type:");

            if let Some(options_span) = options_span {
                let start = match name_span {
                    Some(name_span) => name_span.end, // this will copy ","
                    None => options_span.start,
                };
                if name_span.is_none() {
                    code_transform.prepend_left(options_span.start, ",");
                }
                code_transform.prepend_left(options_span.start, "...");
                // Move the generated type object inside the options object
                code_transform.move_slice(start, options_span.end, tp.gt_span.start);
            }
            code_transform.overwrite(tp.gt_span.start, tp.gt_span.end, "}");

            move_start = tp.lt_span.start;
            move_end = tp.gt_span.end;
        }
    }

    // override `defineModel(`` with `_useModel(__props,`
    code_transform.overwrite(span.start, span.start + 11, "_useModel");
    if options_span.is_none() && name_span.is_none() {
        code_transform.prepend_left(span.start + 12, "__props,\"modelValue\"");
    } else if let Some(name_span) = name_span {
        code_transform.append_left(name_span.start, "__props,");
    } else if let Some(options_span) = options_span {
        code_transform.append_left(options_span.start, "__props,\"modelValue\"");
    }

    let name = match name_span {
        Some(name_span) => {
            source[name_span.start as usize + 1..name_span.end as usize - 1].to_string()
        }
        None => "modelValue".to_string(),
    };

    Some(MacroProcessReturn {
        move_span: None,
        overwrite_span: Some((
            Span {
                start: move_start,
                end: move_end,
            },
            name,
        )),
        remove: None,
    })
}
