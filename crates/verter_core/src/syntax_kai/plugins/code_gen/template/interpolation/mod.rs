use rustc_hash::FxHashMap;

use crate::{
    code_transform::CodeTransform,
    syntax_kai::{
        binding_types::BindingType, plugins::code_gen::template::helper::patch_bindings,
        types::OxcInterpolation,
    },
};

/// Handle an interpolation expression `{{ expr }}`.
///
/// Applies binding prefixes to identifiers within the expression, then
/// transforms `{{ expr }}` → `(expr)` using `overwrite`.
///
/// # Ordering Invariant
///
/// The `_toDisplayString` prefix is NOT emitted here. It is deferred to the
/// parent's close phase via `ChildKind::Interpolation.content_prefix()`.
/// The `overwrite` calls are safe because they replace existing content at
/// fixed positions, not inserting at the child's start position.
pub fn handle_interpolation<'alloc>(
    code_transform: &mut CodeTransform<'alloc>,
    ev: &OxcInterpolation<'alloc>,
    map: &FxHashMap<&'alloc str, BindingType>,
    is_inline: bool,
) {
    // Example: transform {{ msg }} to {{ msg.toUpperCase() }}
    // ctx.code_transform.append_right(ev.expression.end, ".toUpperCase()");
    patch_bindings(code_transform, &ev.bindings, map, is_inline);

    // convert {{ to _toDisplayString(
    // Note: the `_toDisplayString` prefix is NOT prepended here — it's added by
    // the close phase as part of the separator insertion (see ChildKind::content_prefix).
    // This avoids FIFO ordering issues with prepend_left at the same position.
    code_transform.overwrite(ev.start, ev.content.start, "(");

    // convert }} to )
    code_transform.overwrite(ev.content.end, ev.end, ")");
}
