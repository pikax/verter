
    /// Parse a script block.
    pub fn parse_script(
        &self,
        start: CompiledRootScriptStart,
        end: CompiledRootScriptEnd,
        ctx: &SyntaxPluginContext<'alloc>,
    ) -> OxcScript<'alloc> {
        let (program, errors) = if let Some(content) = end.content {
            let result = super::helpers::parse_with_offset(
                content,
                ctx.input,
                self.alloc,
                self.source_type,
            );
            (result.program, result.errors)
        } else {
            // Self-closing script or empty — parse empty string
            let result = oxc_parser::Parser::new(self.alloc, "", self.source_type).parse();
            (result.program, result.errors)
        };

        let content_start = end.content.map_or(start.tag_open.end, |c| c.start);
        let content_end = end.content.map_or(start.tag_open.end, |c| c.end);

        OxcScript {
            start: start.start,
            end: end.end,
            tag_open_start: start.tag_open.start,
            tag_open_end: start.tag_open.end,
            tag_close_start: end.tag_close.map_or(end.end, |t| t.start),
            tag_close_end: end.tag_close.map_or(end.end, |t| t.end),
            content_start,
            content_end,
            program,
            errors,
            setup: start.setup,
            lang: start.lang,
            generic: start.generic.map(|span| {
                // Parse generic type parameters
                let source_slice = &ctx.input[span.start as usize..span.end as usize];
                crate::utils::oxc::vue::parse_generic(self.alloc, source_slice, span.start)
            }),
            attrs: start.attrs,
            attributes: start.attributes.into_iter().collect(),
        }
    }
