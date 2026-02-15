//! Code generation for `defineSlots` macro.
//!
//! Transforms:
//! - `defineSlots<Slots>()` → `null`
//!
//! This macro is type-only and provides slot type inference.
//! At runtime, it returns null (or the slots object in some implementations).

use crate::code_transform::CodeTransform;
use crate::common::Span;

/// Process a `defineSlots` macro call.
///
/// The macro is replaced with `null` since it's primarily for type inference.
/// The actual slots are accessed via `useSlots()` if needed at runtime.
pub fn process_define_slots(span: Span, code_transform: &mut CodeTransform) {
    code_transform.overwrite(span.start, span.end, "_useSlots()");
}
