//! Code generation for `defineExpose` macro.
//!
//! Transforms:
//! - `defineExpose({ foo, bar })` → `__expose({ foo, bar })`

use crate::code_transform::CodeTransform;
use crate::common::Span;

/// Process a `defineExpose` macro call.
///
/// The macro name is replaced with `__expose`, preserving the arguments.
/// `__expose` is provided by the setup context to expose component internals.
pub fn process_define_expose(span: Span, code_transform: &mut CodeTransform) {
    // "defineExpose" is 12 characters, replace with "__expose" (8 characters)
    code_transform.overwrite(span.start, span.start + 12, "__expose");
}
