//! Source map lookup: map tsc error positions in `.tsc.tsx` back to `.vue` file positions.
//!
//! The generated TSC code ends with:
//! ```text
//! //# sourceMappingURL=data:application/json;base64,<encoded>
//! ```
//! We decode the base64 payload, parse the VLQ source map, and look up
//! `(line, col)` in generated space to get the original `.vue` position.

use base64::prelude::*;
use oxc_sourcemap::OwnedSourceMap;

/// A position in a file (0-indexed line and column).
#[derive(Debug, Clone, Copy)]
pub struct FilePos {
    /// 0-indexed line.
    pub line: u32,
    /// 0-indexed column (UTF-16 units).
    pub col: u32,
}

/// Given the content of a `.tsc.tsx` file and a (1-indexed) line+col from tsc,
/// returns the original source file name and (0-indexed) position, if mappable.
pub fn map_tsc_position(
    tsc_code: &str,
    tsc_line_1: u32,
    tsc_col_1: u32,
) -> Option<(String, FilePos)> {
    let sm = extract_inline_source_map(tsc_code)?;
    let gen_line = tsc_line_1.saturating_sub(1);
    let gen_col = tsc_col_1.saturating_sub(1);
    let lookup_table = sm.generate_lookup_table();
    let token = sm.lookup_token(&lookup_table, gen_line, gen_col)?;
    let source_id = token.get_source_id()?;
    let source = sm.get_source(source_id)?;
    let line = token.get_src_line();
    let col = verbatim_line_column(tsc_code, &sm, source_id, gen_line, line, gen_col)
        .unwrap_or(token.get_src_col());
    Some((source.to_string(), FilePos { line, col }))
}

/// The exact authored column for a position on a line the projection copied
/// VERBATIM from its source, when derivable without guesswork.
///
/// The script block is copied character-for-character into the carrier, and the
/// projection map emits a line-start token per script line, so a token-only
/// lookup would discard the diagnostic's authored column (every script-block
/// error would degrade to column 1). When the generated line's text equals the
/// authored line's text — compared through the map's own `sourcesContent` —
/// both sides count UTF-16 columns over the same characters, so the authored
/// column IS the generated column.
///
/// Lines whose texts differ (generated scaffolding, rewritten template
/// projections) return `None`: they keep the covering token's position and are
/// never presented as precise authored columns.
fn verbatim_line_column(
    tsc_code: &str,
    sm: &OwnedSourceMap,
    source_id: u32,
    gen_line: u32,
    src_line: u32,
    gen_col: u32,
) -> Option<u32> {
    let generated = tsc_code.lines().nth(gen_line as usize)?;
    let authored = sm
        .get_source_content(source_id)?
        .lines()
        .nth(src_line as usize)?;
    (generated == authored).then_some(gen_col)
}

/// Extract and decode the inline `//# sourceMappingURL=data:...` from tsc output.
fn extract_inline_source_map(code: &str) -> Option<OwnedSourceMap> {
    const PREFIX: &str = "//# sourceMappingURL=data:application/json;base64,";
    let line = code.lines().rev().find(|l| l.starts_with(PREFIX))?;
    let b64 = &line[PREFIX.len()..];
    let bytes = BASE64_STANDARD.decode(b64.trim()).ok()?;
    let json = std::str::from_utf8(&bytes).ok()?;
    OwnedSourceMap::from_json_string(json).ok()
}
