use verter_formatter::doc::{Doc, Line};
use verter_formatter::render::{render, RenderOptions};

#[test]
fn group_width_and_broken_condition() {
    for (width, expected) in [(6, "ab cd"), (4, "ab\ncd!")] {
        let mut doc = Doc::new();
        let a = doc.text("ab");
        let line = doc.line(Line::Space);
        let b = doc.text("cd");
        let flat = doc.text("");
        let broken = doc.text("!");
        let conditional = doc.if_break(broken, flat);
        let body = doc.concat([a, line, b, conditional]);
        let root = doc.group(body);
        let output = render(
            &doc,
            root,
            RenderOptions {
                print_width: width,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(output.text, expected);
    }
}

#[test]
fn nesting_indentation_alignment_and_line_endings() {
    use verter_formatter::render::LineEnding;
    for (ending, newline) in [
        (LineEnding::Lf, "\n"),
        (LineEnding::CrLf, "\r\n"),
        (LineEnding::Cr, "\r"),
    ] {
        for use_tabs in [false, true] {
            let mut d = Doc::new();
            let a = d.text("a");
            let hard = d.line(Line::Hard);
            let b = d.text("b");
            let line = d.line(Line::Soft);
            let c = d.text("c");
            let body = d.concat([hard, b, line, c]);
            let body = d.indent(4, body);
            let body = d.group(body);
            let tail = d.line(Line::Hard);
            let root = d.concat([a, body, tail]);
            let out = render(
                &d,
                root,
                RenderOptions {
                    use_tabs,
                    tab_width: 4,
                    line_ending: ending,
                    ..Default::default()
                },
            )
            .unwrap();
            let pad = if use_tabs { "\t" } else { "    " };
            assert_eq!(
                out.text,
                format!("a{newline}{pad}b{newline}{pad}c{newline}")
            );
        }
    }
    let mut d = Doc::new();
    let a = d.text("abc");
    let line = d.line(Line::Hard);
    let b = d.text("b");
    let body = d.concat([line, b]);
    let body = d.align(1, body);
    let root = d.concat([a, body]);
    assert_eq!(
        render(&d, root, RenderOptions::default()).unwrap().text,
        "abc\n    b"
    );
}

#[test]
fn nested_groups_decide_independently_and_soft_lines_disappear() {
    let mut d = Doc::new();
    let a = d.text("ab");
    let line = d.line(Line::Soft);
    let b = d.text("cd");
    let inner = d.concat([a, line, b]);
    let inner = d.group(inner);
    let hard = d.line(Line::Hard);
    let end = d.text("end");
    let outer = d.concat([inner, hard, end]);
    let root = d.group(outer);
    assert_eq!(
        render(&d, root, RenderOptions::default()).unwrap().text,
        "abcd\nend"
    );
}

#[test]
fn suffixes_flush_fifo_at_lines_raw_newlines_and_eof_and_participate_in_fit() {
    for raw in [false, true] {
        let mut d = Doc::new();
        let a = d.text("a");
        let s1 = d.line_suffix(" //1");
        let s2 = d.line_suffix(" //2");
        let line = if raw {
            d.text("!\r\nraw\n")
        } else {
            d.line(Line::Hard)
        };
        let s3 = d.line_suffix("end");
        let root = d.concat([a, s1, s2, line, s3]);
        let expected = if raw {
            "a! //1 //2\r\nraw\nend"
        } else {
            "a //1 //2\nend"
        };
        assert_eq!(
            render(&d, root, RenderOptions::default()).unwrap().text,
            expected
        );
    }
    let mut d = Doc::new();
    let a = d.text("a");
    let suffix = d.line_suffix("!");
    let line = d.line(Line::Space);
    let b = d.text("b");
    let root = d.concat([a, suffix, line, b]);
    let root = d.group(root);
    assert_eq!(
        render(
            &d,
            root,
            RenderOptions {
                print_width: 3,
                ..Default::default()
            }
        )
        .unwrap()
        .text,
        "a!\nb"
    );
}

#[test]
fn locked_unicode_ansi_and_tab_columns() {
    for (text, columns, tab) in [
        ("é🙂", 3, 2),
        ("e\u{301}", 1, 2),
        ("\tX", 5, 4),
        ("界", 2, 2),
        ("\x1b[31ma\x1b[0m", 1, 2),
        ("👩‍💻", 2, 2),
    ] {
        for (width, broken) in [(columns + 2, false), (columns + 1, true)] {
            let mut d = Doc::new();
            let a = d.text(text);
            let line = d.line(Line::Space);
            let b = d.text("x");
            let body = d.concat([a, line, b]);
            let root = d.group(body);
            let out = render(
                &d,
                root,
                RenderOptions {
                    print_width: width,
                    tab_width: tab,
                    ..Default::default()
                },
            )
            .unwrap();
            assert_eq!(
                out.text,
                format!("{text}{}x", if broken { "\n" } else { " " })
            );
        }
    }
    let mut d = Doc::new();
    let prefix = d.text("ab");
    let tab = d.text("\t");
    let x = d.text("x");
    let line = d.line(Line::Space);
    let y = d.text("y");
    let body = d.concat([tab, x, line, y]);
    let group = d.group(body);
    let root = d.concat([prefix, group]);
    assert_eq!(
        render(
            &d,
            root,
            RenderOptions {
                print_width: 7,
                tab_width: 4,
                ..Default::default()
            }
        )
        .unwrap()
        .text,
        "ab\tx y"
    );
}

fn adversary(size: usize, wide: bool) -> usize {
    let mut d = Doc::new();
    let root = if wide {
        let children: Vec<_> = (0..size).map(|_| d.text("x")).collect();
        let body = d.concat(children);
        d.group(body)
    } else {
        let mut body = d.text("x");
        for _ in 0..size {
            body = d.group(body);
        }
        body
    };
    let out = render(&d, root, RenderOptions::default()).unwrap();
    assert_eq!(out.text, "x".repeat(if wide { size } else { 1 }));
    assert!(out.work.doc_visits <= 3 * d.len() + 64);
    assert!(out.work.peak_stack_entries <= 2 * d.len() + 64);
    assert_eq!(out.work.emitted_bytes, out.text.len());
    out.work.doc_visits
}

#[test]
fn linear_work_and_heap_stack_for_deep_and_wide_trees() {
    std::thread::Builder::new()
        .stack_size(128 * 1024)
        .spawn(|| {
            for wide in [false, true] {
                let a = adversary(1000, wide);
                let b = adversary(2000, wide);
                assert!(b <= 2 * a);
            }
        })
        .unwrap()
        .join()
        .unwrap();
}

mod nightly_scale {
    #[test]
    fn deep_and_wide_trees() {
        for wide in [false, true] {
            super::adversary(100_000, wide);
        }
    }
}

#[test]
fn invalid_options_foreign_handles_and_empty_output() {
    use verter_formatter::render::RenderError;
    let mut d = Doc::new();
    let root = d.concat([]);
    assert_eq!(render(&d, root, RenderOptions::default()).unwrap().text, "");
    assert_eq!(
        render(
            &d,
            root,
            RenderOptions {
                tab_width: 0,
                ..Default::default()
            }
        ),
        Err(RenderError::InvalidTabWidth)
    );
    assert_eq!(
        render(&Doc::new(), root, RenderOptions::default()),
        Err(RenderError::ForeignRoot)
    );
}

#[test]
#[should_panic(expected = "already attached")]
fn subtree_reuse_cannot_expand_work_exponentially() {
    let mut d = Doc::new();
    let leaf = d.text("x");
    d.concat([leaf, leaf]);
}

#[test]
fn pinned_width_oracle_cases_choose_both_sides_of_the_boundary() {
    for row in include_str!("width-cases.tsv").lines() {
        let (points, columns) = row.split_once('\t').unwrap();
        let text: String = points
            .split(',')
            .map(|c| char::from_u32(u32::from_str_radix(c, 16).unwrap()).unwrap())
            .collect();
        if text.contains(['\r', '\n', '\t']) {
            continue;
        }
        let columns: usize = columns.parse().unwrap();
        for (width, separator) in [(columns + 2, " "), (columns + 1, "\n")] {
            let mut d = Doc::new();
            let a = d.text(&text);
            let line = d.line(Line::Space);
            let b = d.text("x");
            let body = d.concat([a, line, b]);
            let root = d.group(body);
            assert_eq!(
                render(
                    &d,
                    root,
                    RenderOptions {
                        print_width: width,
                        ..Default::default()
                    }
                )
                .unwrap()
                .text,
                format!("{text}{separator}x"),
                "points={points}"
            );
        }
    }
}
