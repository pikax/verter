use crate::{
    code_transform::CodeTransform,
    syntax_kai::{
        plugin::SyntaxPluginContext,
        plugins::code_gen::{
            template::shared::helper::escape_js_string, types::TemplateImportDependencies,
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
    code_transform: &mut CodeTransform<'alloc>,
    ev: &Text,
    ctx: &SyntaxPluginContext<'alloc>,
    state: &mut StateStack,
    _imports: &mut TemplateImportDependencies,
) {
    state.children.push(ChildInfo {
        start: ev.start,
        kind: ChildKind::Text,
        scope_prefix: String::new(),
    });

    // Escape the text content for use inside a JS string literal.
    // Characters like `"`, `\`, newlines, etc. must be escaped.
    let raw_text = &ctx.input[ev.start as usize..ev.end as usize];
    let escaped = escape_js_string(raw_text);

    if escaped != raw_text {
        // Text needs escaping — overwrite with escaped version.
        // The opening quote is added by the close phase via ChildKind::content_prefix().
        code_transform.overwrite(ev.start, ev.end, &format!("{}\"", escaped));
    } else {
        // No escaping needed — just add the closing quote.
        // The opening quote is added by the close phase as part of the separator
        // insertion (see ChildKind::content_prefix). This is necessary because
        // prepend_left is FIFO at the same position — if we prepend `"` here and
        // the close phase later prepends `, `, the quote would appear first:
        // `", hello"` instead of `, "hello"`.
        code_transform.append_left(ev.end, "\"");
    }
}
