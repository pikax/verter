//! Hover-extraction grammar — extracts the `<RHS>` of `type <probe_name> =
//! <RHS>` from tsgo's hover (`docs/arch/u0-oracle-harness-design.md`
//! §Q2 "hover-extraction grammar", §4 `hover_extraction_grammar_is_versioned`).
//!
//! PURE + offline (tsgo-free): so BOTH the generator AND the offline
//! `raw_capture_matches_oracle_value` audit re-run it without tsgo. The output
//! RHS is handed to `admission::admit_hover_text` (which parses it via the SAME
//! OXC type parser and runs the backstop + positive allowlist + drop-counter).
//! Versioned by `PROBE_SYNTHESIS_VERSION`.
//!
//! The grammar (FIXED) has TWO ordered shapes, both of which require the probe
//! text to be EXACTLY one top-level type-alias declaration `type <probe_name> =
//! <RHS>` (optional trailing `;`), naming the expected probe, with NO `export` /
//! `declare` modifier and NO type parameters — leading/trailing line/block
//! comments and JSDoc are the ONLY surrounding text allowed:
//!
//! 1. **Fenced shape (markdown caps).** If ANY fenced code block exists, ONLY
//!    fenced ```` ```typescript ```` / ```` ```ts ```` blocks are parsed (prose /
//!    inline / other-language blocks ignored); the FIRST such block whose trimmed
//!    body is EXACTLY the probe alias declaration wins. A hover with NO
//!    probe-naming TS fence FAILS (`NoProbeBlock`); an unclosed fence FAILS
//!    (`UnclosedFence`). Any fence DISABLES the plaintext fallback.
//! 2. **Plaintext shape (empty caps).** If ZERO fenced code blocks of any
//!    language exist, the WHOLE trimmed hover is parsed as the plaintext driver
//!    shape — it must be EXACTLY the probe alias declaration, nothing before or
//!    after it (modulo comments).
//!
//! Driver-shape note (`hover_driver_config_pinned`): the adopted LSP driver
//! (`get_hover`, Q3) initializes tsgo with EMPTY client capabilities
//! (`capabilities: {}`, `tsgo/ipc.rs`), which produce a
//! BARE PLAINTEXT `type <probe_name> = <RHS>` hover with NO markdown fence (Q3).
//! The plaintext shape (2) is therefore the live driver shape; the fenced shape
//! (1) handles a markdown-caps driver. Both are the SAME probe header; the driver
//! config is pinned into `probe_synthesis_version`, so a capability / content
//! shape change is a version change, never a silent mismatch. The whole-hover
//! plaintext parse fires ONLY when the hover carries NO fenced code block of any
//! language (a fenced hover's prose / inline / other blocks never trigger it —
//! any fence DISABLES the fallback).
//!
//! Both shapes feed the same STRICT top-level alias parse (`parse_probe_alias`):
//! a loose substring scan is unsound (it would accept a probe header embedded in
//! prose, an `export`/`declare` modifier, a parameterized alias header, or
//! trailing extra declarations). The RHS bytes the strict parse returns are
//! handed to the admission gate's OXC parser unchanged.

use oxc_allocator::Allocator;
use oxc_ast::ast::{Statement, TSType};
use oxc_parser::Parser;
use oxc_span::{GetSpan, SourceType};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum HoverExtractError {
    /// No fenced ```typescript / ```ts block (nor — in the no-fence plaintext
    /// shape — the whole hover) is EXACTLY the probe alias declaration (the
    /// wrong-hover / no-probe-binding / surrounding-noise case —
    /// `probe_header_names_target`).
    NoProbeBlock,
    /// A fenced block opened but never closed (truncated / malformed hover).
    UnclosedFence,
}

/// Extract the RHS type-text of `type <probe_name> = <rhs>` from `hover_markdown`.
pub(crate) fn extract_probe_rhs(
    hover_markdown: &str,
    probe_name: &str,
) -> Result<String, HoverExtractError> {
    let mut saw_unclosed = false;
    let mut any_fence = false;
    let blocks = fenced_typescript_blocks(hover_markdown, &mut saw_unclosed, &mut any_fence);
    for block in blocks {
        if let Some(rhs) = parse_probe_alias(block, probe_name) {
            return Ok(rhs);
        }
    }
    if saw_unclosed {
        return Err(HoverExtractError::UnclosedFence);
    }
    // WHOLE-TEXT plaintext shape for the BARE (unfenced) empty-caps hover: only
    // when there is NO fenced code block of any language, so a markdown hover's
    // prose / inline / other-language blocks never trigger it (any fence DISABLES
    // the fallback). The whole trimmed hover must be EXACTLY the probe alias.
    if !any_fence {
        if let Some(rhs) = parse_probe_alias(hover_markdown, probe_name) {
            return Ok(rhs);
        }
    }
    Err(HoverExtractError::NoProbeBlock)
}

/// Yield the inner text of each fenced ```typescript / ```ts block, in order.
/// Sets `saw_unclosed` if a ts fence opened with no closing fence; sets
/// `any_fence` if ANY fenced code block (any language) was seen.
fn fenced_typescript_blocks<'a>(
    md: &'a str,
    saw_unclosed: &mut bool,
    any_fence: &mut bool,
) -> Vec<&'a str> {
    let mut blocks = Vec::new();
    let bytes = md.as_bytes();
    let mut i = 0;
    while i < md.len() {
        // A fence opener is ``` at the start of the string or right after a `\n`.
        let at_line_start = i == 0 || bytes[i - 1] == b'\n';
        if at_line_start && md[i..].starts_with("```") {
            *any_fence = true;
            let after_ticks = i + 3;
            // The info string runs to the end of the line.
            let line_end = md[after_ticks..]
                .find('\n')
                .map(|r| after_ticks + r)
                .unwrap_or(md.len());
            let info = md[after_ticks..line_end].trim();
            let is_ts = info.eq_ignore_ascii_case("typescript") || info.eq_ignore_ascii_case("ts");
            // The body starts after the info line's newline.
            let body_start = if line_end < md.len() {
                line_end + 1
            } else {
                md.len()
            };
            // Find the closing fence ``` at a line start.
            let mut j = body_start;
            let mut closing: Option<usize> = None;
            while j < md.len() {
                let line_at_start = j == body_start || bytes[j - 1] == b'\n';
                if line_at_start && md[j..].starts_with("```") {
                    closing = Some(j);
                    break;
                }
                j += 1;
            }
            match closing {
                Some(close) => {
                    if is_ts {
                        blocks.push(&md[body_start..close]);
                    }
                    // Advance past the closing fence line.
                    let close_line_end = md[close..]
                        .find('\n')
                        .map(|r| close + r + 1)
                        .unwrap_or(md.len());
                    i = close_line_end;
                    continue;
                }
                None => {
                    if is_ts {
                        *saw_unclosed = true;
                    }
                    break;
                }
            }
        }
        i += 1;
    }
    blocks
}

/// STRICT parse of a candidate text against the EXACT probe-alias grammar: the
/// WHOLE trimmed candidate (modulo leading/trailing comments) must be exactly
/// `type <probe_name> = <RHS>` with an optional trailing `;` — ONE top-level
/// type-alias declaration, the correct probe name, NO `export` / `declare`
/// modifier, NO type parameters, and NO surrounding prose / trailing
/// declarations. Returns the RHS bytes on a match, `None` otherwise.
///
/// This replaces the prior loose `type <probe> = …` substring scan, which would
/// accept the header embedded in prose, behind an `export`/`declare` modifier, on
/// a parameterized alias, or followed by extra declarations. The parse is over
/// the OXC TS program (comments are skipped by the parser, so leading/trailing
/// comments and JSDoc are tolerated), then the alias type-annotation span slices
/// the original RHS bytes — fed UNCHANGED to the admission gate's OXC parser.
fn parse_probe_alias(candidate: &str, probe_name: &str) -> Option<String> {
    let trimmed = candidate.trim();
    if trimmed.is_empty() {
        return None;
    }
    let allocator = Allocator::default();
    let ret = Parser::new(&allocator, trimmed, SourceType::ts()).parse();
    if ret.panicked || !ret.errors.is_empty() {
        // A parse error (truncated/unbalanced/invalid) is NOT a clean probe alias.
        return None;
    }
    // EXACTLY one top-level statement, and it must be a bare type-alias
    // declaration (no `export` wrapper, no other statements before/after it).
    let mut stmts = ret.program.body.iter();
    let first = stmts.next()?;
    if stmts.next().is_some() {
        // Trailing extra declaration / statement after the alias → REJECT.
        return None;
    }
    let Statement::TSTypeAliasDeclaration(alias) = first else {
        return None;
    };
    // No `declare` modifier (an `export` modifier produces an
    // `ExportNamedDeclaration` statement, already rejected by the variant match).
    if alias.declare {
        return None;
    }
    // The alias must name EXACTLY the expected probe.
    if alias.id.name.as_str() != probe_name {
        return None;
    }
    // No type parameters on the alias header (`type P<T> = …` is out of grammar).
    if alias.type_parameters.is_some() {
        return None;
    }
    // Slice the ORIGINAL RHS bytes from the alias type-annotation span (the OXC
    // parser already balanced the body), and hand them to the admission gate
    // unchanged.
    let ann_span = type_annotation_span(&alias.type_annotation);
    let rhs = &trimmed[ann_span.start as usize..ann_span.end as usize];
    Some(rhs.trim().to_string())
}

/// The source span of a `TSType` (the alias RHS). Pulled out so the slice site is
/// a single, named conversion.
fn type_annotation_span(ts: &TSType<'_>) -> oxc_span::Span {
    ts.span()
}

#[cfg(test)]
mod tests;
