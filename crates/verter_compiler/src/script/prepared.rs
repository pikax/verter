//! Single script/macro preparation lane.
//!
//! A compile drives several consumers that each need the parsed setup
//! (`<script setup>`) and companion (`<script>`) blocks: invalid-macro-type
//! diagnostics, the macro surfaces and bindings that script codegen lowers, and
//! the force-js type-stripping inputs. Historically each consumer re-parsed the
//! same content with OXC and re-ran the shared type resolver. [`PreparedScript`]
//! parses each block exactly once into the top compile allocator and hands the
//! parsed program, the [`ScriptParseResult`] macro surfaces, and the companion
//! type inventory out read-only to every consumer.
//!
//! The structural facts prepared here (macro spans, type-argument syntax,
//! object/array shapes, binding kinds) come from the single parse; semantic
//! normalization and type resolution stay with the shared resolver invoked once
//! through [`parse_script_with_companion`]. This lane removes duplicated
//! compiler orchestration, not resolver semantics.

use oxc_allocator::Allocator;
use oxc_ast::ast::Program;
use oxc_parser::Parser;
use rustc_hash::FxHashMap;

use crate::parser::types::RootNodeScript;
use crate::utils::oxc::script::type_surface::{extract_companion_types, ResolvedElements};
use crate::utils::oxc::vue::{
    parse_script, parse_script_with_companion, ScriptMode, ScriptParseResult,
};

use super::process::source_type_from_lang;

/// One compile's parsed setup + companion script blocks.
///
/// Built once near the top of the compile (before target gating) so the
/// invalid-macro-type diagnostics — which surface on every target — read from
/// the same parse the script/force-js lanes later consume.
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
    /// The shared-resolver output: macro surfaces, bindings, async status,
    /// diagnostics — produced by the single `parse_script_with_companion` call.
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
    /// Type declarations the companion exposes for cross-block `defineProps<T>`
    /// resolution.
    companion_types: FxHashMap<String, ResolvedElements>,
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
        external_types: Option<&FxHashMap<String, ResolvedElements>>,
    ) -> Self {
        // Parse the companion first: its exported types feed setup-script
        // `defineProps<T>` resolution, exactly as the historical order did.
        // Companion type extraction only matters when a `<script setup>` consumes
        // it; an Options-API standalone `<script>` is still parsed once here (so
        // its codegen and force-js consumers reuse the parse) but skips the
        // unused type pass.
        let extract_companion_types = script_setup.is_some();
        let companion = script
            .and_then(|s| PreparedCompanion::build(source, s, alloc, extract_companion_types));

        let setup = script_setup.and_then(|ss| {
            PreparedSetup::build(source, ss, alloc, companion.as_ref(), external_types)
        });

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
        companion: Option<&PreparedCompanion<'alloc>>,
        external_types: Option<&FxHashMap<String, ResolvedElements>>,
    ) -> Option<Self> {
        let content_span = script_setup.content?;
        let content_start = content_span.start;
        let content_str = &source[content_span.start as usize..content_span.end as usize];
        let source_type = source_type_from_lang(script_setup.lang.as_ref());
        #[cfg(test)]
        parse_counters::bump_setup();
        let parser_ret = Parser::new(alloc, content_str, source_type).parse();
        let program: &'alloc Program<'alloc> = alloc.alloc(parser_ret.program);

        let companion_types =
            merge_companion_types(companion.map(|c| &c.companion_types), external_types);

        let parse_result = parse_script_with_companion(
            program,
            ScriptMode::Setup,
            content_start,
            content_str,
            companion_types,
        );

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
        extract_types: bool,
    ) -> Option<Self> {
        let content_span = script.content?;
        let content_start = content_span.start;
        let content_str = &source[content_span.start as usize..content_span.end as usize];
        let source_type = source_type_from_lang(script.lang.as_ref());
        #[cfg(test)]
        parse_counters::bump_companion();
        let parser_ret = Parser::new(alloc, content_str, source_type).parse();
        let program: &'alloc Program<'alloc> = alloc.alloc(parser_ret.program);

        let parse_result = parse_script(program, ScriptMode::Options, content_start, content_str);
        let companion_types = if extract_types {
            extract_companion_types(program, content_str.as_bytes(), content_start)
        } else {
            FxHashMap::default()
        };

        Some(Self {
            program,
            content_start,
            content_str,
            parse_result,
            companion_types,
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

/// Merge companion-script types with host-provided external types.
///
/// Companion entries take precedence; external types fill the gaps. This
/// mirrors the precedence the diagnostics and script-codegen paths each applied
/// before they shared one prepared parse.
fn merge_companion_types(
    companion: Option<&FxHashMap<String, ResolvedElements>>,
    external: Option<&FxHashMap<String, ResolvedElements>>,
) -> Option<FxHashMap<String, ResolvedElements>> {
    match (companion, external) {
        (Some(companion), Some(external)) => {
            let mut merged = companion.clone();
            for (key, value) in external {
                merged.entry(key.clone()).or_insert_with(|| value.clone());
            }
            Some(merged)
        }
        (Some(companion), None) => Some(companion.clone()),
        (None, Some(external)) => Some(external.clone()),
        (None, None) => None,
    }
}

/// Test-only invocation counters that pin the single-parse lane: a full compile
/// must OXC-parse the setup block once and the companion block once, no matter
/// how many consumers (diagnostics, script codegen, force-js) read from them.
///
/// Counters are thread-local so concurrently-running tests never perturb each
/// other; a compile runs synchronously on the calling thread.
#[cfg(test)]
pub(crate) mod parse_counters {
    use std::cell::Cell;

    thread_local! {
        static SETUP_BLOCK_PARSES: Cell<usize> = const { Cell::new(0) };
        static COMPANION_BLOCK_PARSES: Cell<usize> = const { Cell::new(0) };
    }

    pub(crate) fn bump_setup() {
        SETUP_BLOCK_PARSES.with(|c| c.set(c.get() + 1));
    }

    pub(crate) fn bump_companion() {
        COMPANION_BLOCK_PARSES.with(|c| c.set(c.get() + 1));
    }

    /// Reset both counters on the current thread before an observed compile.
    pub(crate) fn reset() {
        SETUP_BLOCK_PARSES.with(|c| c.set(0));
        COMPANION_BLOCK_PARSES.with(|c| c.set(0));
    }

    pub(crate) fn setup_block_parses() -> usize {
        SETUP_BLOCK_PARSES.with(|c| c.get())
    }

    pub(crate) fn companion_block_parses() -> usize {
        COMPANION_BLOCK_PARSES.with(|c| c.get())
    }
}
