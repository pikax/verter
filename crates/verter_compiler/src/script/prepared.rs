//! Single script/macro preparation lane.
//!
//! A compile drives several consumers that each need the parsed setup
//! (`<script setup>`) and companion (`<script>`) blocks: syntax-owned macro
//! facts, bindings, and force-js type-stripping inputs. Historically each consumer re-parsed the
//! same content with OXC. [`PreparedScript`]
//! parses each block exactly once into the top compile allocator and hands the
//! parsed program and [`ScriptParseResult`] macro surfaces out read-only to
//! every consumer.
//!
//! The structural facts prepared here (macro spans, type-argument syntax,
//! object/array shapes, binding kinds) come from the single parse. Semantic
//! normalization and type resolution arrive separately through macro DTOs.

use crate::parser::types::RootNodeScript;
use crate::utils::oxc::vue::{parse_script, ScriptMode, ScriptParseResult};
use oxc_allocator::Allocator;
use oxc_ast::ast::Program;
use oxc_parser::Parser;

use super::process::source_type_from_lang;

/// One compile's parsed setup + companion script blocks.
///
/// Built once near the top of the compile so syntax-owned script and force-js
/// lanes share the same parse.
pub struct PreparedScript<'alloc> {
    setup: Option<PreparedSetup<'alloc>>,
    companion: Option<PreparedCompanion<'alloc>>,
}

/// The parsed `<script setup>` block.
pub struct PreparedSetup<'alloc> {
    /// The OXC program, parsed once into the top compile allocator.
    program: &'alloc Program<'alloc>,
    /// Byte offset of the setup content within the SFC source.
    content_start: u32,
    /// The setup content slice (`source[content_span]`).
    content_str: &'alloc str,
    /// Syntax macro surfaces, bindings, async status, and diagnostics.
    parse_result: ScriptParseResult<'alloc>,
}

/// The parsed companion `<script>` block (present when `<script setup>` exists).
pub struct PreparedCompanion<'alloc> {
    /// The OXC program, parsed once into the top compile allocator.
    program: &'alloc Program<'alloc>,
    /// Byte offset of the companion content within the SFC source.
    content_start: u32,
    /// The companion content slice (`source[content_span]`).
    content_str: &'alloc str,
    /// Options-mode parse: imports and default-export facts script codegen lowers.
    parse_result: ScriptParseResult<'alloc>,
}

impl<'alloc> PreparedScript<'alloc> {
    /// Parse the setup + companion blocks once.
    ///
    /// `script` / `script_setup` supply tag and content spans only (borrowed
    /// from the parsed SFC, not retained). Parsed programs and result facts live
    /// in `alloc`, which must outlive every consumer of this `PreparedScript`.
    pub fn build<'p>(
        source: &'alloc str,
        script: Option<&'p RootNodeScript>,
        script_setup: Option<&'p RootNodeScript>,
        alloc: &'alloc Allocator,
    ) -> Self {
        let companion = script.and_then(|s| PreparedCompanion::build(source, s, alloc));

        let setup =
            script_setup.and_then(|ss| PreparedSetup::build(source, ss, alloc, companion.as_ref()));

        Self { setup, companion }
    }

    /// The parsed setup block, if `<script setup>` with content is present.
    pub fn setup(&self) -> Option<&PreparedSetup<'alloc>> {
        self.setup.as_ref()
    }

    /// The parsed companion block, if a `<script>` with content is present.
    pub fn companion(&self) -> Option<&PreparedCompanion<'alloc>> {
        self.companion.as_ref()
    }
}

impl<'alloc> PreparedSetup<'alloc> {
    fn build<'p>(
        source: &'alloc str,
        script_setup: &'p RootNodeScript,
        alloc: &'alloc Allocator,
        _companion: Option<&PreparedCompanion<'alloc>>,
    ) -> Option<Self> {
        let content_span = script_setup.content?;
        let content_start = content_span.start;
        let content_str = &source[content_span.start as usize..content_span.end as usize];
        let source_type = source_type_from_lang(script_setup.lang.as_ref());
        let parser_ret = Parser::new(alloc, content_str, source_type).parse();
        let program: &'alloc Program<'alloc> = alloc.alloc(parser_ret.program);

        let parse_result = parse_script(program, ScriptMode::Setup, content_start, content_str);

        Some(Self {
            program,
            content_start,
            content_str,
            parse_result,
        })
    }

    pub fn program(&self) -> &'alloc Program<'alloc> {
        self.program
    }

    pub fn content_start(&self) -> u32 {
        self.content_start
    }

    pub fn content_str(&self) -> &'alloc str {
        self.content_str
    }

    pub fn parse_result(&self) -> &ScriptParseResult<'alloc> {
        &self.parse_result
    }
}

impl<'alloc> PreparedCompanion<'alloc> {
    fn build<'p>(
        source: &'alloc str,
        script: &'p RootNodeScript,
        alloc: &'alloc Allocator,
    ) -> Option<Self> {
        let content_span = script.content?;
        let content_start = content_span.start;
        let content_str = &source[content_span.start as usize..content_span.end as usize];
        let source_type = source_type_from_lang(script.lang.as_ref());
        let parser_ret = Parser::new(alloc, content_str, source_type).parse();
        let program: &'alloc Program<'alloc> = alloc.alloc(parser_ret.program);

        let parse_result = parse_script(program, ScriptMode::Options, content_start, content_str);
        Some(Self {
            program,
            content_start,
            content_str,
            parse_result,
        })
    }

    pub fn program(&self) -> &'alloc Program<'alloc> {
        self.program
    }

    pub fn content_start(&self) -> u32 {
        self.content_start
    }

    pub fn content_str(&self) -> &'alloc str {
        self.content_str
    }

    pub fn parse_result(&self) -> &ScriptParseResult<'alloc> {
        &self.parse_result
    }
}
