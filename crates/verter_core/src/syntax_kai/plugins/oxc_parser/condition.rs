use crate::syntax_kai::types::Prop;


/// Parse a v-if condition.
pub fn parse_if_condition(
    prop: &Prop,
    element_id: u32,
    ctx: &SyntaxPluginContext<'alloc>,
) -> OxcIfCondition<'alloc> {
    let (expression, errors, bindings) = if let Some(value_span) = prop.value {
        parse_expression(value_span, ctx)
    } else {
        (None, None, None)
    };

    OxcIfCondition {
        element_id,
        start: prop.start,
        end: prop.end,
        expression,
        errors,
        bindings,
        event: ElementScopeConditionIf {
            element_start: element_id,
            start: prop.start,
            end: prop.end,
            value: prop.value,
        },
    }
}

/// Parse a v-else-if condition.
pub fn parse_else_if_condition(
    &self,
    prop: &Prop,
    element_id: u32,
    ctx: &SyntaxPluginContext<'alloc>,
) -> OxcElseIfCondition<'alloc> {
    let (expression, errors, bindings) = if let Some(value_span) = prop.value {
        self.parse_expression(value_span, ctx)
    } else {
        (None, None, None)
    };

    OxcElseIfCondition {
        element_id,
        start: prop.start,
        end: prop.end,
        expression,
        errors,
        bindings,
        event: ElementScopeConditionIf {
            element_start: element_id,
            start: prop.start,
            end: prop.end,
            value: prop.value,
        },
    }
}
