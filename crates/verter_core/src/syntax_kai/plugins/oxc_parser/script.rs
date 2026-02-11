use crate::{
    syntax_kai::{
        plugins::oxc_parser::helpers::parse_program,
        types::{CompiledRootScriptEnd, CompiledRootScriptStart, OxcScript},
    },
    utils::oxc::vue::ScriptMode,
};
use oxc_allocator::Allocator;

/// Parse a script block.
pub fn parse_script<'alloc>(
    start: CompiledRootScriptStart,
    end: CompiledRootScriptEnd,
    input: &'alloc str,

    alloc: &'alloc Allocator,
    source_type: oxc_span::SourceType,
) -> OxcScript<'alloc> {
    let (program, errors) = if let Some(content) = end.content {
        let result = parse_program(content, input, alloc, source_type);
        (result.program, result.errors)
    } else {
        // Self-closing script or empty — parse empty string
        let result = oxc_parser::Parser::new(alloc, "", source_type).parse();
        (result.program, result.errors)
    };

    let content_start = end.content.map_or(start.tag_open.end, |c| c.start);
    let content_end = end.content.map_or(start.tag_open.end, |c| c.end);

    let result = crate::utils::oxc::vue::parse_script(
        &program,
        if start.setup.is_some() {
            ScriptMode::Setup
        } else {
            ScriptMode::Options
        },
        0, // No offset needed since we already adjusted spans when parsing the program
        input,
    );

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
        result,

        setup: start.setup,
        lang: start.lang,
        generic: start.generic.map(|span| {
            // Parse generic type parameters
            let source_slice = &input[span.start as usize..span.end as usize];
            crate::utils::oxc::vue::parse_generic(alloc, source_slice, span.start)
        }),
        attrs: start.attrs,
        attributes: start.attributes.into_iter().collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::Span;
    use crate::cursor::ScriptLanguage;
    use crate::syntax_kai::types::{
        RootNodeCloseTag, RootNodeKind, RootNodeOpenTagEnd, RootNodeOpenTagStart,
    };
    use oxc_span::{GetSpan, SourceType};

    /// Build a minimal CompiledRootScriptStart for testing.
    fn make_script_start(
        start: u32,
        tag_open: Span,
        setup: Option<Span>,
        lang: Option<ScriptLanguage>,
        generic: Option<Span>,
    ) -> CompiledRootScriptStart {
        CompiledRootScriptStart {
            start,
            name_end: tag_open.start + 7, // after "script"
            tag_open,
            setup,
            lang,
            generic,
            attrs: None,
            attributes: vec![],
            tag_open_event: RootNodeOpenTagStart {
                kind: RootNodeKind::Script,
                start,
                name_end: tag_open.start + 7,
            },
            tag_open_end_event: RootNodeOpenTagEnd {
                kind: RootNodeKind::Script,
                start,
                end: tag_open.end,
                name_end: tag_open.start + 7,
                is_self_closing: false,
            },
        }
    }

    /// Build a CompiledRootScriptEnd for testing.
    fn make_script_end(
        end: u32,
        tag_close: Option<Span>,
        content: Option<Span>,
    ) -> CompiledRootScriptEnd {
        CompiledRootScriptEnd {
            start: tag_close.map_or(end, |t| t.start),
            name_end: tag_close.map_or(end, |t| t.start + 8), // after "</script"
            end,
            tag_close,
            content,
            tag_close_event: tag_close.map(|t| RootNodeCloseTag {
                kind: RootNodeKind::Script,
                start: t.start,
                end: t.end,
                name_end: t.start + 8,
            }),
        }
    }

    /// @ai-generated - Basic script block with simple content.
    #[test]
    fn parse_script_basic() {
        let alloc = Allocator::default();
        //          0         1         2         3         4         5
        //          0123456789012345678901234567890123456789012345678901234567
        let input = "<script>const x = 1;</script>";
        let start = make_script_start(0, Span::new(0, 8), None, None, None);
        let end = make_script_end(29, Some(Span::new(20, 29)), Some(Span::new(8, 20)));

        let result = parse_script(start, end, input, &alloc, SourceType::tsx());

        assert_eq!(result.start, 0);
        assert_eq!(result.end, 29);
        assert_eq!(result.tag_open_start, 0);
        assert_eq!(result.tag_open_end, 8);
        assert_eq!(result.tag_close_start, 20);
        assert_eq!(result.tag_close_end, 29);
        assert_eq!(result.content_start, 8);
        assert_eq!(result.content_end, 20);
        assert_eq!(result.program.body.len(), 1, "Expected 1 statement");
        assert!(result.errors.is_empty());
        assert!(result.setup.is_none());
        assert!(result.lang.is_none());
        assert!(result.generic.is_none());
    }

    /// @ai-generated - Script setup block is detected.
    #[test]
    fn parse_script_setup() {
        let alloc = Allocator::default();
        //          0         1         2         3         4         5         6
        //          01234567890123456789012345678901234567890123456789012345678901234567
        let input = "<script setup>import { ref } from 'vue';\nconst count = ref(0);\n</script>";
        let tag_open_end = "<script setup>".len() as u32; // 14
        let close_start = input.len() as u32 - "</script>".len() as u32;
        let close_end = input.len() as u32;
        let content_start = tag_open_end;
        let content_end = close_start;

        let start = make_script_start(
            0,
            Span::new(0, tag_open_end),
            Some(Span::new(8, 13)), // "setup"
            None,
            None,
        );
        let end = make_script_end(
            close_end,
            Some(Span::new(close_start, close_end)),
            Some(Span::new(content_start, content_end)),
        );

        let result = parse_script(start, end, input, &alloc, SourceType::tsx());

        assert!(result.setup.is_some(), "Expected setup to be set");
        assert_eq!(result.program.body.len(), 2, "Expected import + const");
        assert!(result.errors.is_empty());
    }

    /// @ai-generated - Script with lang="ts" attribute.
    #[test]
    fn parse_script_with_lang() {
        let alloc = Allocator::default();
        let input = r#"<script lang="ts">const x: number = 1;</script>"#;
        let tag_open_end = r#"<script lang="ts">"#.len() as u32; // 18
        let close_start = input.len() as u32 - "</script>".len() as u32;
        let close_end = input.len() as u32;

        let start = make_script_start(
            0,
            Span::new(0, tag_open_end),
            None,
            Some(ScriptLanguage::TypeScript),
            None,
        );
        let end = make_script_end(
            close_end,
            Some(Span::new(close_start, close_end)),
            Some(Span::new(tag_open_end, close_start)),
        );

        let result = parse_script(start, end, input, &alloc, SourceType::tsx());

        assert_eq!(result.lang, Some(ScriptLanguage::TypeScript));
        assert_eq!(result.program.body.len(), 1);
        assert!(result.errors.is_empty());
    }

    /// @ai-generated - Self-closing script tag (no content).
    #[test]
    fn parse_script_self_closing() {
        let alloc = Allocator::default();
        let input = "<script />";
        let start = make_script_start(0, Span::new(0, 10), None, None, None);
        let end = make_script_end(10, None, None);

        let result = parse_script(start, end, input, &alloc, SourceType::tsx());

        assert!(result.program.body.is_empty(), "Expected empty program");
        assert!(result.errors.is_empty());
        // content_start/end should fall back to tag_open.end
        assert_eq!(result.content_start, 10);
        assert_eq!(result.content_end, 10);
        // tag_close falls back to end
        assert_eq!(result.tag_close_start, 10);
        assert_eq!(result.tag_close_end, 10);
    }

    /// @ai-generated - Empty script body.
    #[test]
    fn parse_script_empty_body() {
        let alloc = Allocator::default();
        let input = "<script></script>";
        let start = make_script_start(0, Span::new(0, 8), None, None, None);
        let end = make_script_end(17, Some(Span::new(8, 17)), Some(Span::new(8, 8)));

        let result = parse_script(start, end, input, &alloc, SourceType::tsx());

        assert!(result.program.body.is_empty());
        assert!(result.errors.is_empty());
    }

    /// @ai-generated - Script with generic attribute parses type parameters.
    #[test]
    fn parse_script_with_generic() {
        let alloc = Allocator::default();
        //                     generic="T"
        let input = r#"<script setup generic="T">const x = 1;</script>"#;
        let tag_open_end = r#"<script setup generic="T">"#.len() as u32; // 26
        let close_start = input.len() as u32 - "</script>".len() as u32;
        let close_end = input.len() as u32;
        // The generic value span points to "T" inside the attribute
        let generic_start = r#"<script setup generic=""#.len() as u32; // 22
        let generic_end = generic_start + 1; // 23

        let start = make_script_start(
            0,
            Span::new(0, tag_open_end),
            Some(Span::new(8, 13)),
            None,
            Some(Span::new(generic_start, generic_end)),
        );
        let end = make_script_end(
            close_end,
            Some(Span::new(close_start, close_end)),
            Some(Span::new(tag_open_end, close_start)),
        );

        let result = parse_script(start, end, input, &alloc, SourceType::tsx());

        assert!(result.generic.is_some(), "Expected generic to be parsed");
        let generic = result.generic.unwrap();
        assert!(generic.is_ok(), "Expected generic to parse without errors");
        assert_eq!(generic.param_count(), 1, "Expected 1 type parameter");
    }

    /// @ai-generated - Script content spans are adjusted relative to original source.
    #[test]
    fn parse_script_span_adjustment() {
        let alloc = Allocator::default();
        //          0         1         2         3         4
        //          01234567890123456789012345678901234567890123456
        let input = "<script>const x = 1;</script>";
        let start = make_script_start(0, Span::new(0, 8), None, None, None);
        let end = make_script_end(29, Some(Span::new(20, 29)), Some(Span::new(8, 20)));

        let result = parse_script(start, end, input, &alloc, SourceType::tsx());

        // Statement span should be within content range [8, 20)
        for stmt in &result.program.body {
            let span = stmt.span();
            assert!(
                span.start >= 8,
                "Statement span start {} should be >= 8",
                span.start
            );
            assert!(
                span.end <= 20,
                "Statement span end {} should be <= 20",
                span.end
            );
        }
    }

    /// @ai-generated - Script with multiple imports and declarations.
    #[test]
    fn parse_script_multiple_statements() {
        let alloc = Allocator::default();
        let content = "import { ref, computed } from 'vue';\nconst a = ref(0);\nconst b = computed(() => a.value + 1);\n";
        let input = format!("<script setup>{}</script>", content);
        let tag_open_end = "<script setup>".len() as u32;
        let content_end = tag_open_end + content.len() as u32;
        let close_end = input.len() as u32;

        let start = make_script_start(
            0,
            Span::new(0, tag_open_end),
            Some(Span::new(8, 13)),
            None,
            None,
        );
        let end = make_script_end(
            close_end,
            Some(Span::new(content_end, close_end)),
            Some(Span::new(tag_open_end, content_end)),
        );

        let result = parse_script(start, end, &input, &alloc, SourceType::tsx());

        assert_eq!(result.program.body.len(), 3, "Expected import + 2 consts");
        assert!(result.errors.is_empty());
    }

    /// @ai-generated - Result items are populated from script parse.
    #[test]
    fn parse_script_result_items() {
        let alloc = Allocator::default();
        let content = "import { ref } from 'vue';\nconst x = ref(0);\n";
        let input = format!("<script setup>{}</script>", content);
        let tag_open_end = "<script setup>".len() as u32;
        let content_end = tag_open_end + content.len() as u32;
        let close_end = input.len() as u32;

        let start = make_script_start(
            0,
            Span::new(0, tag_open_end),
            Some(Span::new(8, 13)),
            None,
            None,
        );
        let end = make_script_end(
            close_end,
            Some(Span::new(content_end, close_end)),
            Some(Span::new(tag_open_end, content_end)),
        );

        let result = parse_script(start, end, &input, &alloc, SourceType::tsx());

        // ScriptParseResult should have items (imports, declarations, etc.)
        assert!(
            !result.result.items.is_empty(),
            "Expected result items from script parse"
        );
    }

    /// @ai-generated - Options API script (no setup) uses ScriptMode::Options.
    #[test]
    fn parse_script_options_api() {
        let alloc = Allocator::default();
        let content = "export default { data() { return { x: 1 } } }";
        let input = format!("<script>{}</script>", content);
        let tag_open_end = "<script>".len() as u32;
        let content_end = tag_open_end + content.len() as u32;
        let close_end = input.len() as u32;

        let start = make_script_start(0, Span::new(0, tag_open_end), None, None, None);
        let end = make_script_end(
            close_end,
            Some(Span::new(content_end, close_end)),
            Some(Span::new(tag_open_end, content_end)),
        );

        let result = parse_script(start, end, &input, &alloc, SourceType::tsx());

        assert!(result.setup.is_none(), "Options API should have no setup");
        assert_eq!(result.program.body.len(), 1);
        assert!(result.errors.is_empty());
    }
}
