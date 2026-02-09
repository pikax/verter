//! Vue compiler macro code generation.
//!
//! This module provides code generation helpers for Vue's compiler macros:
//! - `defineProps` / `withDefaults` - Props declaration
//! - `defineEmits` - Emits declaration
//! - `defineExpose` - Component expose
//! - `defineOptions` - Component options
//! - `defineModel` - Two-way binding model
//! - `defineSlots` - Typed slots

mod emits;
mod expose;
mod model;
mod options;
mod props;
mod slots;
pub mod types;

pub use emits::process_define_emits;
pub use expose::process_define_expose;
pub use model::process_define_model;
pub use options::process_define_options;
pub use props::{process_define_props, process_with_defaults};
pub use slots::process_define_slots;

use crate::code_transform::CodeTransform;
use crate::codegen::vue::macros::types::MacroProcessReturn;
use crate::syntax::types::OxcScriptContent;
use crate::utils::oxc::vue::ScriptMacro;

/// Process a Vue macro and apply transformations to the code.
pub fn process_macro<'a>(
    _event: &OxcScriptContent<'a>,
    macro_item: &ScriptMacro<'a>,
    code_transform: &mut CodeTransform<'a>,
    source: &'a str,
    is_production: bool,
) -> Option<MacroProcessReturn> {
    match macro_item {
        ScriptMacro::DefineProps {
            span,
            declarator,
            type_params,
            object_arg,
            array_arg,
        } => {
            return process_define_props(
                span,
                declarator,
                type_params,
                object_arg,
                array_arg,
                code_transform,
                source,
                is_production,
            );
        }
        ScriptMacro::DefineEmits {
            span,
            declarator,
            type_params,
            object_arg,
            array_arg,
        } => {
            return process_define_emits(
                span,
                declarator,
                type_params,
                object_arg,
                array_arg,
                code_transform,
                source,
            );
        }
        ScriptMacro::DefineExpose { span, .. } => {
            process_define_expose(*span, code_transform);
        }
        ScriptMacro::DefineOptions {
            span,
            declarator,
            object_arg,
        } => {
            return process_define_options(span, declarator, object_arg, code_transform);
        }
        ScriptMacro::DefineModel {
            declarator,
            span,
            name_span,
            options_span,
            type_params,
        } => {
            return process_define_model(
                declarator,
                *span,
                type_params,
                name_span.as_ref(),
                options_span.as_ref(),
                code_transform,
                source,
            );
        }
        ScriptMacro::DefineSlots { span, .. } => {
            process_define_slots(*span, code_transform);
        }
        ScriptMacro::WithDefaults {
            span,
            declarator,
            define_props_type_params,
            defaults,
            ..
        } => {
            return process_with_defaults(
                *span,
                declarator,
                define_props_type_params.as_ref(),
                defaults.as_ref(),
                code_transform,
                source,
                is_production,
            );
        }
    }

    None
}
