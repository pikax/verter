//! Code generation for `defineOptions` macro.
//!
//! Transforms:
//! - `defineOptions({ inheritAttrs: false })` → removed
//!
//! The options are extracted during analysis and merged into the component
//! definition. The macro call itself is removed from the output.

use crate::code_transform::CodeTransform;
use crate::common::Span;
use crate::syntax::plugins::code_gen::script::macros::types::MacroProcessReturn;
use crate::utils::oxc::vue::{MacroDeclarator, MacroObjectArg};

/// Process a `defineOptions` macro call.
///
/// The entire macro call is removed. The options should be extracted during
/// the analysis phase and merged into the `__sfc__` component definition.
pub fn process_define_options<'a>(
    span: &Span,
    _declarator: &Option<MacroDeclarator<'a>>,
    object_arg: &Option<MacroObjectArg<'a>>,
    _code_transform: &mut CodeTransform,
) -> Option<MacroProcessReturn> {
    // code_transform.overwrite(span.start, span.end, "");

    if let Some(obj) = object_arg {
        return Some(MacroProcessReturn {
            move_span: Some(obj.span),
            overwrite_span: Some((
                Span {
                    start: span.start,
                    end: span.end,
                },
                "".to_string(),
            )),
            remove: None,
            diagnostic: None,
        });
    }

    None
}
