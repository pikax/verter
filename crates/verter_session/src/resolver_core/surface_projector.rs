use verter_type_expr::TypeExpr;

/// Render a `TypeExpr` to a display string for `AnalyzedSlotFieldBinding`
/// and `AnalyzedSlotField.return_type`. Display-only; semantic decisions
/// must read the typed form. Returns `None` for shapes the renderer cannot
/// surface as a single inline display fragment.
///
/// Uses `verter_type_expr`'s heap-worklist renderer: deep finite types have no
/// structural-depth cap and do not consume one Rust call frame per type node.
/// A typed rendering error stays the `None` display signal; the typed carrier
/// remains authoritative and is never converted to a fabricated `unknown`.
pub(crate) fn render_type_expr_display(expr: &TypeExpr) -> Option<String> {
    verter_type_expr::render_type_expr_display(expr)
        .ok()
        .map(|rendered| rendered.text)
}
