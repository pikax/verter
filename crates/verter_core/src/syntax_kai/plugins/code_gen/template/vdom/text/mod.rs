use crate::{
    code_transform::CodeTransform,
    syntax_kai::{
        plugin::SyntaxPluginContext,
        plugins::code_gen::{
            template::shared::helper::escape_js_string_in_place, types::TemplateImportDependencies,
        },
        types::Text,
    },
};

use super::{ChildInfo, ChildKind, StateStack};

/// Process a text node within a template element.
///
/// Records a `ChildInfo` in the parent state and wraps the text content in quotes
/// to produce a JS string literal. Does NOT add separators — the close phase
/// retroactively inserts separators based on the full children list.
///
/// For source like `hello` between elements, this produces `"hello"`.
/// Text containing special characters (quotes, backslashes, etc.) is escaped
/// to produce valid JS string literals.
///
/// # Ordering Invariant
///
/// This function must NOT call `prepend_left(ev.start, ...)`. The opening `"`
/// quote is deferred to the parent's close phase via `ChildKind::Text.content_prefix()`.
/// Only `append_left(ev.end, "\"")` is safe here because it operates at `ev.end`,
/// a different position from the separator insertion point (`ev.start`).
pub(crate) fn handle_text<'alloc>(
    code_transform: &CodeTransform<'alloc>,
    ev: &Text,
    ctx: &SyntaxPluginContext<'alloc>,
    state: &mut StateStack<'alloc>,
    _imports: &mut TemplateImportDependencies,
    pending_overwrites: &mut Vec<(u32, u32, &'alloc str)>,
    pending_append_lefts: &mut Vec<(u32, &'alloc str)>,
) {
    state.children.push(ChildInfo {
        start: ev.start,
        kind: ChildKind::Text,
        scope_prefix: "",
    });

    // Escape the text content in-place for use inside a JS string literal.
    // Characters like `"`, `\`, newlines, etc. are individually overwritten.
    // Characters that don't need escaping stay in place (preserving source positions).
    escape_js_string_in_place(
        code_transform,
        ev.start,
        ev.end,
        ctx.input,
        pending_overwrites,
    );
    // The opening quote is added by the close phase via ChildKind::content_prefix().
    pending_append_lefts.push((ev.end, "\""));
}
