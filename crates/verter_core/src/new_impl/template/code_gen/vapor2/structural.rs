//! Vapor2 structural directive code generation (v-if, v-for).

use crate::new_impl::ast::types::ElementNodeConditionKind;
use crate::new_impl::template::code_gen::shared::helpers::{self, VaporHelper};
use crate::new_impl::template::code_gen::types::CodeGenOutput;
use crate::new_impl::types::NodeProp;

/// Build the opening code for a v-if / v-else-if / v-else structural directive.
///
/// - v-if: `_createIf(() => (condition), () => {`
/// - v-else-if: `, () => _createIf(() => (condition), () => {`
/// - v-else: `, () => {`
pub fn build_condition_prefix(
    kind: &ElementNodeConditionKind,
    prop: &NodeProp,
    source: &str,
) -> String {
    let condition = match (prop.value_start, prop.value_end) {
        (Some(s), Some(e)) => &source[s as usize..e as usize],
        _ => "true",
    };

    match kind {
        ElementNodeConditionKind::If => {
            let mut buf = String::with_capacity(64);
            buf.push_str("_createIf(() => (");
            buf.push_str(condition);
            buf.push_str("), () => {");
            buf
        }
        ElementNodeConditionKind::ElseIf => {
            let mut buf = String::with_capacity(64);
            buf.push_str(", () => _createIf(() => (");
            buf.push_str(condition);
            buf.push_str("), () => {");
            buf
        }
        ElementNodeConditionKind::Else => ", () => {".to_string(),
    }
}

/// Build the closing code for a v-if / v-else-if / v-else structural directive.
pub fn build_condition_suffix(kind: &ElementNodeConditionKind) -> &'static str {
    match kind {
        ElementNodeConditionKind::If | ElementNodeConditionKind::ElseIf => "})",
        ElementNodeConditionKind::Else => "}",
    }
}

/// Collect v-if imports.
pub fn collect_condition_imports(out: &mut CodeGenOutput<'_>) {
    out.add_vapor_import(VaporHelper::CreateIf);
}

/// Build the opening code for a v-for structural directive.
///
/// Output: `_createFor(() => (iterable), (params) => {`
pub fn build_for_prefix(v_for: &NodeProp, source: &str) -> String {
    let expr = match (v_for.value_start, v_for.value_end) {
        (Some(s), Some(e)) => &source[s as usize..e as usize],
        _ => "",
    };

    let (params, iterable) = helpers::parse_v_for_expression(expr);

    let mut buf = String::with_capacity(64);
    buf.push_str("_createFor(() => (");
    buf.push_str(iterable);
    buf.push_str("), (");
    buf.push_str(params);
    buf.push_str(") => {");
    buf
}

/// Build the closing code for v-for.
pub fn build_for_suffix() -> &'static str {
    "})"
}

/// Collect v-for imports.
pub fn collect_for_imports(out: &mut CodeGenOutput<'_>) {
    out.add_vapor_import(VaporHelper::CreateFor);
}
