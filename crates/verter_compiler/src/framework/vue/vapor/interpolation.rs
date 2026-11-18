//! Dynamic text wraps the one shared expression recording.
use super::{effect::TextPart, ops::Part, VaporCompiler};
use crate::ast::types::InterpolationNode;
use crate::template::code_gen::shared::helpers::VaporHelper;
use crate::template::oxc::types::OxcParsedExpression;

pub(super) fn process_interpolation<'c>(
    compiler: &mut VaporCompiler<'c>,
    interp: &InterpolationNode,
    expression: &OxcParsedExpression<'c>,
) {
    let expr = compiler.record_expression(
        &compiler.inputs.source.text[interp.inner_start as usize..interp.inner_end as usize],
        interp.inner_start,
        Some(expression),
    );
    let mut parts = vec![
        Part::Text("_toDisplayString("),
        Part::Run(expr),
        Part::Text(")"),
    ];
    let run = compiler.state.runs.commit(&mut parts);
    if let Some(parent) = compiler.element_stack.last_mut() {
        parent.text_parts.push(TextPart::Dynamic(run));
        parent.ensure_text_ref(&mut compiler.state.counters);
    }
    compiler.state.use_helper(VaporHelper::ToDisplayString);
}
