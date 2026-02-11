
    /// Parse an interpolation expression.
    pub fn parse_interpolation(
        &self,
        interp: Interpolation,
        ctx: &SyntaxPluginContext<'alloc>,
    ) -> OxcInterpolation<'alloc> {
        let (expression, errors, bindings) = self.parse_expression(interp.content, ctx);

        OxcInterpolation {
            parent_id: interp.parent_id,
            start: interp.start,
            end: interp.end,
            content: interp.content,
            expression,
            errors,
            bindings,
            event: interp,
        }
    }