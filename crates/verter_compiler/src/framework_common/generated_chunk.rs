//! Truthful composition of compiler-generated chunks from distinct source spaces.

use std::ops::Range;

use oxc_sourcemap::{SourceMap, SourceMapBuilder, Token};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeneratedChunkOutput {
    pub code: String,
    pub source_map: String,
}

pub struct GeneratedUnit<'a> {
    pub code: &'a str,
    pub source_map: &'a str,
    pub source_space: &'a str,
    pub source: &'a str,
}

pub struct GeneratedFragment<'a> {
    pub unit: GeneratedUnit<'a>,
    pub range: Range<u32>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Position {
    line: u32,
    column: u32,
}

fn byte_position(code: &str, offset: u32) -> Option<Position> {
    let offset = usize::try_from(offset).ok()?;
    if offset > code.len() || !code.is_char_boundary(offset) {
        return None;
    }
    let mut position = Position { line: 0, column: 0 };
    for character in code[..offset].chars() {
        if character == '\n' {
            position.line += 1;
            position.column = 0;
        } else {
            position.column += character.len_utf16() as u32;
        }
    }
    Some(position)
}

fn relative(position: Position, origin: Position) -> Option<Position> {
    (position >= origin).then(|| {
        if position.line == origin.line {
            Position {
                line: 0,
                column: position.column - origin.column,
            }
        } else {
            Position {
                line: position.line - origin.line,
                column: position.column,
            }
        }
    })
}

fn append(origin: Position, relative: Position) -> Position {
    if relative.line == 0 {
        Position {
            line: origin.line,
            column: origin.column + relative.column,
        }
    } else {
        Position {
            line: origin.line + relative.line,
            column: relative.column,
        }
    }
}

fn token_position(token: Token) -> Position {
    Position {
        line: token.get_dst_line(),
        column: token.get_dst_col(),
    }
}

fn add_token(builder: &mut SourceMapBuilder, token: Token, position: Position, source_id: u32) {
    builder.add_token(
        position.line,
        position.column,
        token.get_src_line(),
        token.get_src_col(),
        token.get_source_id().map(|_| source_id),
        None,
    );
}

/// Replace a typed hole in one generated unit with a typed fragment from a
/// second generated unit and compose both maps into the resulting space.
///
/// No authored input is concatenated or reparsed: both sides have already
/// passed their native compiler lanes and the splice boundaries name generated
/// bytes only.
pub fn compose_generated_chunk(
    preamble: &str,
    shell: GeneratedUnit<'_>,
    hole: Range<u32>,
    fragment: GeneratedFragment<'_>,
) -> Option<GeneratedChunkOutput> {
    let shell_code = shell.code;
    let fragment_code = fragment.unit.code;
    let output_origin = byte_position(preamble, preamble.len() as u32)?;
    let hole_start = byte_position(shell_code, hole.start)?;
    let hole_end = byte_position(shell_code, hole.end)?;
    let fragment_start = byte_position(fragment_code, fragment.range.start)?;
    let fragment_end = byte_position(fragment_code, fragment.range.end)?;
    let fragment_text =
        fragment_code.get(fragment.range.start as usize..fragment.range.end as usize)?;
    let inserted_text = format!("\n{fragment_text}\n");
    let fragment_origin = append(hole_start, Position { line: 1, column: 0 });
    let inserted_end = append(
        hole_start,
        byte_position(&inserted_text, inserted_text.len() as u32)?,
    );

    let mut code = String::with_capacity(
        preamble.len() + shell_code.len() - (hole.end - hole.start) as usize + inserted_text.len(),
    );
    code.push_str(preamble);
    code.push_str(shell_code.get(..hole.start as usize)?);
    code.push_str(&inserted_text);
    code.push_str(shell_code.get(hole.end as usize..)?);

    let shell_map = SourceMap::from_json_string(shell.source_map).ok()?;
    let fragment_map = SourceMap::from_json_string(fragment.unit.source_map).ok()?;
    let mut builder = SourceMapBuilder::default();
    let shell_source_id = builder.add_source_and_content(shell.source_space, shell.source);
    let fragment_source_id =
        builder.add_source_and_content(fragment.unit.source_space, fragment.unit.source);

    for token in shell_map.get_tokens() {
        let position = token_position(token);
        if position < hole_start {
            add_token(
                &mut builder,
                token,
                append(output_origin, position),
                shell_source_id,
            );
        }
    }

    for token in fragment_map.get_tokens() {
        let position = token_position(token);
        if position >= fragment_start && position < fragment_end {
            let rebased = append(
                output_origin,
                append(fragment_origin, relative(position, fragment_start)?),
            );
            add_token(&mut builder, token, rebased, fragment_source_id);
        }
    }

    for token in shell_map.get_tokens() {
        let position = token_position(token);
        if position >= hole_end {
            let rebased = append(
                output_origin,
                append(inserted_end, relative(position, hole_end)?),
            );
            add_token(&mut builder, token, rebased, shell_source_id);
        }
    }

    Some(GeneratedChunkOutput {
        code,
        source_map: builder.into_sourcemap().to_json_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn position_math_counts_utf16_columns() {
        assert_eq!(
            byte_position("a😀b\nc", "a😀".len() as u32),
            Some(Position { line: 0, column: 3 })
        );
    }
}
