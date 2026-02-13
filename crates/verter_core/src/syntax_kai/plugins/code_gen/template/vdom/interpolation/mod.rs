use rustc_hash::FxHashMap;

use crate::syntax_kai::{
    binding_types::BindingType,
    plugins::code_gen::template::shared::helper::collect_binding_patches, types::OxcInterpolation,
};

/// Handle an interpolation expression `{{ expr }}`.
///
/// Collects binding patches (deferred) and transforms `{{ expr }}` → `(expr)`.
///
/// # Ordering Invariant
///
/// The `_toDisplayString` prefix is NOT emitted here. It is deferred to the
/// parent's close phase via `ChildKind::Interpolation.content_prefix()`.
/// The `overwrite` calls are safe because they replace existing content at
/// fixed positions, not inserting at the child's start position.
pub(crate) fn handle_interpolation<'alloc>(
    ev: &OxcInterpolation<'alloc>,
    map: &FxHashMap<&'alloc str, BindingType>,
    is_inline: bool,
    binding_patches: &mut Vec<(u32, &'alloc str)>,
    pending_overwrites: &mut Vec<(u32, u32, &'alloc str)>,
) {
    collect_binding_patches(ev.bindings.as_ref(), map, is_inline, binding_patches);

    // convert {{ to (
    // Note: the `_toDisplayString` prefix is NOT prepended here — it's added by
    // the close phase as part of the separator insertion (see ChildKind::content_prefix).
    // This avoids FIFO ordering issues with prepend_left at the same position.
    pending_overwrites.push((ev.start, ev.content.start, "("));

    // convert }} to )
    pending_overwrites.push((ev.content.end, ev.end, ")"));
}
