use super::*;
use crate::reporter::{Diagnostic, PositionRenderer, Severity};

use ColumnEncoding::{Utf16, Utf32, Utf8};

// "a😀b": `a` is 1 unit in every encoding; the emoji is 2 UTF-16 units, 4 UTF-8
// bytes, 1 scalar; `b` follows it. One-based columns:
//   utf16: a=1 emoji=2 (inside=3) b=4 end=5
//   utf8:  a=1 emoji=2 (inside=3..=5) b=6 end=7
//   utf32: a=1 emoji=2 b=3 end=4
const LINE: &str = "a\u{1F600}b";

#[test]
fn columns_recount_exactly_between_encodings() {
    let table = LineTable::new(LINE);
    for (utf16, utf8, utf32) in [(1, 1, 1), (2, 2, 2), (4, 6, 3), (5, 7, 4)] {
        assert_eq!(table.convert_column(1, utf16, Utf16, Utf8), Ok(utf8));
        assert_eq!(table.convert_column(1, utf16, Utf16, Utf32), Ok(utf32));
        assert_eq!(table.convert_column(1, utf8, Utf8, Utf16), Ok(utf16));
        assert_eq!(table.convert_column(1, utf32, Utf32, Utf16), Ok(utf16));
    }
}

#[test]
fn a_column_inside_a_character_or_outside_the_file_is_refused_not_snapped() {
    let table = LineTable::new(LINE);
    // The second UTF-16 unit of the emoji, and bytes 3..=5 of its UTF-8 form.
    assert_eq!(
        table.convert_column(1, 3, Utf16, Utf8),
        Err(PositionError::NotCharBoundary)
    );
    for inside in 3..=5 {
        assert_eq!(
            table.convert_column(1, inside, Utf8, Utf16),
            Err(PositionError::NotCharBoundary),
            "utf8 column {inside}"
        );
    }
    // Past the end of the line, a zero base, and a line the file lacks.
    assert_eq!(
        table.convert_column(1, 6, Utf16, Utf8),
        Err(PositionError::OutOfRange)
    );
    assert_eq!(
        table.convert_column(1, 0, Utf16, Utf8),
        Err(PositionError::OutOfRange)
    );
    assert_eq!(
        table.convert_column(0, 1, Utf16, Utf8),
        Err(PositionError::OutOfRange)
    );
    assert_eq!(
        table.convert_column(2, 1, Utf16, Utf8),
        Err(PositionError::OutOfRange)
    );
}

#[test]
fn lines_end_at_the_typescript_terminator_set() {
    // `\r\n` is one terminator; a lone `\r`, U+2028 and U+2029 each end a line.
    let text = "a\r\nb\rc\u{2028}d\u{2029}e";
    let table = LineTable::new(text);
    // Line starts by byte: a=0, b=3, c=5, d=9, e=13.
    for (byte_offset, line_col) in [
        (0u64, (1, 1)),
        (3, (2, 1)),
        (5, (3, 1)),
        (9, (4, 1)),
        (13, (5, 1)),
    ] {
        assert_eq!(table.line_col_of_offset(byte_offset, Utf8), Ok(line_col));
    }
    // An offset ON a terminator belongs to the line it ends.
    assert_eq!(table.line_col_of_offset(1, Utf8), Ok((1, 2)));
    // `\n` between lines: the second byte of `\r\n` is still line 1.
    assert_eq!(table.line_col_of_offset(2, Utf8), Ok((1, 3)));
    // The file end is the end of the last line; past it is refused.
    assert_eq!(table.line_col_of_offset(14, Utf8), Ok((5, 2)));
    assert_eq!(
        table.line_col_of_offset(15, Utf8),
        Err(PositionError::OutOfRange)
    );
}

#[test]
fn offsets_are_counted_in_the_selected_encoding() {
    // "x\n" + "a😀b": the emoji line starts at offset 2 in every encoding.
    let text = "x\na\u{1F600}b";
    let table = LineTable::new(text);
    assert_eq!(table.line_col_of_offset(2, Utf16), Ok((2, 1)));
    assert_eq!(
        table.line_col_of_offset(5, Utf16),
        Ok((2, 4)),
        "b after the emoji"
    );
    assert_eq!(table.line_col_of_offset(7, Utf8), Ok((2, 6)));
    assert_eq!(table.line_col_of_offset(4, Utf32), Ok((2, 3)));
    assert_eq!(
        table.line_col_of_offset(4, Utf16),
        Err(PositionError::NotCharBoundary),
        "inside the surrogate pair"
    );
    assert_eq!(
        table.line_col_of_offset(5, Utf8),
        Err(PositionError::NotCharBoundary),
        "inside the UTF-8 sequence"
    );
}

#[test]
fn selectors_are_one_based_and_parse_strictly() {
    assert_eq!(
        Selector::parse_line_char("src/App.vue:3:5"),
        Ok(Selector::LineChar {
            file: "src/App.vue".into(),
            line: 3,
            column: 5
        })
    );
    // A drive letter keeps its colon: only the two trailing numbers split off.
    assert_eq!(
        Selector::parse_line_char("C:/w/App.vue:1:2"),
        Ok(Selector::LineChar {
            file: "C:/w/App.vue".into(),
            line: 1,
            column: 2
        })
    );
    for bad in [
        "App.vue",
        "App.vue:3",
        "App.vue:0:1",
        "App.vue:1:0",
        ":1:1",
        "App.vue:a:b",
    ] {
        assert!(Selector::parse_line_char(bad).is_err(), "{bad}");
    }
    assert_eq!(
        Selector::parse_offset("App.vue:0"),
        Ok(Selector::Offset {
            file: "App.vue".into(),
            offset: 0
        })
    );
    for bad in ["App.vue", ":4", "App.vue:-1", "App.vue:x"] {
        assert!(Selector::parse_offset(bad).is_err(), "{bad}");
    }

    // Resolution validates against the file: an out-of-range selector is
    // refused instead of matching nothing.
    let table = LineTable::new(LINE);
    let at = |line, column| Selector::LineChar {
        file: "f".into(),
        line,
        column,
    };
    assert_eq!(at(1, 4).resolve(&table, Utf16), Ok((1, 4)));
    assert_eq!(
        at(1, 3).resolve(&table, Utf16),
        Err(PositionError::NotCharBoundary)
    );
    assert_eq!(
        at(2, 1).resolve(&table, Utf16),
        Err(PositionError::OutOfRange)
    );
    let offset = Selector::Offset {
        file: "f".into(),
        offset: 5,
    };
    assert_eq!(offset.resolve(&table, Utf8), Ok((1, 6)));
}

fn diagnostic(file: &str, line: u32, col: u32) -> Diagnostic {
    Diagnostic {
        file: file.to_string(),
        line,
        col,
        ts_code: 2322,
        verter_code: None,
        message: "m".to_string(),
        severity: Severity::Error,
    }
}

#[test]
fn rendered_diagnostics_carry_the_selected_unit_or_no_position() {
    // Dropped (and removed) even when an assertion below fails.
    let scratch = tempfile::TempDir::new().expect("scratch dir");
    let file = scratch.path().join("App.vue");
    std::fs::write(&file, LINE).expect("write fixture");
    let path = file.to_string_lossy().replace('\\', "/");

    // The engine reports UTF-16 column 4 (`b`); the same place is UTF-8 column
    // 6 and UTF-32 column 3.
    let d = diagnostic(&path, 1, 4);
    let render = |encoding| PositionRenderer::new(encoding).render(&d);
    assert_eq!(render(Utf16), format!("{path}(1,4): error TS2322: m"));
    assert_eq!(render(Utf8), format!("{path}(1,6): error TS2322: m"));
    assert_eq!(render(Utf32), format!("{path}(1,3): error TS2322: m"));
    assert_eq!(
        d.to_string(),
        render(Utf16),
        "Display is the tsc convention"
    );

    // Inside the surrogate pair: not convertible, so reported WITHOUT a
    // position — never a guessed column.
    let inside = diagnostic(&path, 1, 3);
    assert_eq!(
        PositionRenderer::new(Utf8).render(&inside),
        format!("{path}: error TS2322: m")
    );
    // An unreadable file cannot be converted either.
    let missing = diagnostic(&format!("{path}.missing"), 1, 4);
    assert_eq!(
        PositionRenderer::new(Utf8).render(&missing),
        format!("{path}.missing: error TS2322: m")
    );
    // Validation runs in every encoding and for column 1: an unreadable file
    // or a line the file does not have is reported without a position, never
    // as a guessed `(line,1)`.
    for encoding in [Utf8, Utf16, Utf32] {
        let renderer = PositionRenderer::new(encoding);
        for (file, line, col) in [
            (format!("{path}.missing"), 7, 1),
            (format!("{path}.missing"), 1, 4),
            (path.clone(), 7, 1),
            (path.clone(), 0, 1),
            (path.clone(), 1, 0),
            (path.clone(), 1, 99),
        ] {
            assert_eq!(
                renderer.render(&diagnostic(&file, line, col)),
                format!("{file}: error TS2322: m"),
                "{encoding:?} {file}({line},{col})"
            );
        }
        // Column 1 of a line the file has stays valid.
        assert_eq!(
            renderer.render(&diagnostic(&path, 1, 1)),
            format!("{path}(1,1): error TS2322: m")
        );
    }
    // Under UTF-16 a column inside a surrogate pair is refused as well.
    assert_eq!(
        PositionRenderer::new(Utf16).render(&inside),
        format!("{path}: error TS2322: m")
    );

    // Selection: a selector in the selected unit matches by file + position.
    let renderer = PositionRenderer::new(Utf8);
    let selected = Selector::parse_line_char(&format!("{path}:1:6")).expect("selector");
    let position = renderer.resolve_selector(&selected).expect("in range");
    assert_eq!(position, (1, 6));
    assert!(renderer.is_at(&d, &path, position));
    // A selector written relative to the reported absolute path matches on a
    // whole path-component suffix, never a partial name.
    assert!(renderer.is_at(&d, "App.vue", position));
    assert!(!renderer.is_at(&d, "pp.vue", position));
    assert!(!renderer.is_at(&diagnostic(&path, 1, 2), &path, position));
    assert!(renderer
        .resolve_selector(&Selector::parse_line_char(&format!("{path}:1:3")).unwrap())
        .is_err());
}
