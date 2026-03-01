//! Source map lookup: map tsc error positions in `.tsc.tsx` back to `.vue` file positions.
//!
//! The generated TSC code ends with:
//! ```text
//! //# sourceMappingURL=data:application/json;base64,<encoded>
//! ```
//! We decode the base64 payload, parse the VLQ source map, and look up
//! `(line, col)` in generated space to get the original `.vue` position.

use base64::prelude::*;
use oxc_sourcemap::SourceMap;

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
    Some((
        source.to_string(),
        FilePos {
            line: token.get_src_line(),
            col: token.get_src_col(),
        },
    ))
}

/// Extract and decode the inline `//# sourceMappingURL=data:...` from tsc output.
fn extract_inline_source_map(code: &str) -> Option<SourceMap> {
    const PREFIX: &str = "//# sourceMappingURL=data:application/json;base64,";
    let line = code.lines().rev().find(|l| l.starts_with(PREFIX))?;
    let b64 = &line[PREFIX.len()..];
    let bytes = BASE64_STANDARD.decode(b64.trim()).ok()?;
    let json = std::str::from_utf8(&bytes).ok()?;
    SourceMap::from_json_string(json).ok()
}
