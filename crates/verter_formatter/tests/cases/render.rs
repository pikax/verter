//! Document algebra laws, deterministic provenance-carrying rendering,
//! bounded adversarial work and dependency isolation of the renderer.

use verter_formatter::coordinates::{AuthoredFormatByteOffset, AuthoredFormatByteRange};
use verter_formatter::doc::{Doc, DocId, Line};
use verter_formatter::provenance::{AuthoredProvenance, FormatProvenanceTable, Provenanced};
use verter_formatter::render::{render, RenderOptions, RenderedDoc, Segment};

fn render_at(doc: &Doc, root: DocId, print_width: usize) -> String {
    render(
        doc,
        root,
        RenderOptions {
            print_width,
            ..Default::default()
        },
    )
    .unwrap()
    .text
}

/// A group measures its complete flat body plus the pending suffix queue from
/// the current column; ordinary following siblings are not part of the fit.
#[test]
fn group_fit_measures_its_body_not_following_siblings() {
    for (xy_inside, expected) in [(false, "ab cdxy"), (true, "ab\ncdxy")] {
        let mut d = Doc::new();
        let ab = d.text("ab");
        let line = d.line(Line::Space);
        let cd = d.text("cd");
        let xy = d.text("xy");
        let root = if xy_inside {
            let body = d.concat([ab, line, cd, xy]);
            d.group(body)
        } else {
            let body = d.concat([ab, line, cd]);
            let group = d.group(body);
            d.concat([group, xy])
        };
        assert_eq!(render_at(&d, root, 5), expected, "xy inside: {xy_inside}");
    }
    // Suffixes queued before the group flush after its flat body on the same
    // line, so they are part of its fit: alone "aab cd" fits width 7, with a
    // queued two-column suffix it breaks and the suffix flushes first.
    for (suffix, expected) in [("", "aab cd"), ("!!", "aab!!\ncd")] {
        let mut d = Doc::new();
        let a = d.text("a");
        let queued = d.line_suffix(suffix);
        let ab = d.text("ab");
        let line = d.line(Line::Space);
        let cd = d.text("cd");
        let body = d.concat([ab, line, cd]);
        let group = d.group(body);
        let root = d.concat([a, queued, group]);
        assert_eq!(render_at(&d, root, 7), expected, "suffix {suffix:?}");
    }
}

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
    for row in include_str!("../width-cases.tsv").lines() {
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

/// Conditional content selects by the nearest enclosing group's mode; outside
/// any group the root is broken.
#[test]
fn conditional_content_follows_the_nearest_group_mode() {
    // (wrap in inner group, wrap in breaking outer group, expected)
    for (inner, outer, expected) in [
        (false, false, "B"),
        (true, false, "F"),
        (true, true, "F\nwide-content"),
        (false, true, "B\nwide-content"),
    ] {
        let mut d = Doc::new();
        let broken = d.text("B");
        let flat = d.text("F");
        let mut node = d.if_break(broken, flat);
        if inner {
            node = d.group(node);
        }
        if outer {
            let line = d.line(Line::Soft);
            let wide = d.text("wide-content");
            let body = d.concat([node, line, wide]);
            node = d.group(body);
        }
        assert_eq!(
            render_at(&d, node, 5),
            expected,
            "inner {inner} outer {outer}"
        );
    }
}

/// The rendered source oracle: every source-backed segment carries its exact
/// foundation binding, in emission order, and its rendered bytes equal the
/// authored bytes it is bound to. Zero-width source units keep their binding.
#[test]
fn source_segments_retain_exact_bindings_and_bytes_in_emission_order() {
    const AUTHORED: &str = "é🙂";
    let range = |start, end| {
        AuthoredFormatByteRange::for_source(
            AUTHORED,
            AuthoredFormatByteOffset::for_source(AUTHORED, start).unwrap(),
            AuthoredFormatByteOffset::for_source(AUTHORED, end).unwrap(),
        )
        .unwrap()
    };
    let mut table = FormatProvenanceTable::new(17_u128);
    let bind = |table: &mut FormatProvenanceTable<u128>, r: AuthoredFormatByteRange| {
        AuthoredProvenance::new(table.bind_authored(r), r)
    };
    let (e, smile, eof) = (
        bind(&mut table, range(0, 2)),
        bind(&mut table, range(2, 6)),
        bind(&mut table, range(6, 6)),
    );
    let authored = |p: &AuthoredProvenance| {
        &AUTHORED[p.range().start().get() as usize..p.range().end().get() as usize]
    };
    let build = || {
        let mut d = Doc::new();
        let suffix = d.source_line_suffix(Provenanced::new(authored(&smile).into(), smile.clone()));
        let text = d.source(Provenanced::new(authored(&e).into(), e.clone()));
        let empty = d.source(Provenanced::new(String::new(), eof.clone()));
        let indent_line = d.line(Line::Hard);
        let body = d.concat([indent_line, empty]);
        let body = d.indent(2, body);
        let hard = d.line(Line::Hard);
        let root = d.concat([suffix, text, body, hard]);
        render(&d, root, RenderOptions::default()).unwrap()
    };
    let output = build();
    // The zero-width unit emits no indentation, so no trailing padding.
    assert_eq!(output.text, "é🙂\n\n");
    let sources: Vec<_> = source_segments(&output);
    assert_eq!(
        sources.iter().map(|(p, _)| p.clone()).collect::<Vec<_>>(),
        [e.clone(), smile.clone(), eof.clone()]
    );
    for (binding, rendered) in &sources {
        assert_eq!(*rendered, authored(binding), "{binding:?}");
        table.validate(&17, binding).unwrap();
    }
    // Every emitted byte is covered exactly once, in order.
    let mut cursor = 0;
    for segment in &output.segments {
        let range = match segment {
            Segment::Source(s) => s.value().range,
            Segment::Generated(g) => g.range,
        };
        assert_eq!(range.start().get(), cursor);
        cursor = range.end().get();
    }
    assert_eq!(cursor as usize, output.text.len());
    // Identical documents and options render byte- and segment-identically.
    for _ in 0..8 {
        assert_eq!(build(), output);
    }
}

fn source_segments(output: &RenderedDoc) -> Vec<(AuthoredProvenance, &str)> {
    output
        .segments
        .iter()
        .filter_map(|s| match s {
            Segment::Source(s) => {
                let r = s.value().range;
                let text = &output.text[r.start().get() as usize..r.end().get() as usize];
                Some((s.provenance().clone(), text))
            }
            Segment::Generated(_) => None,
        })
        .collect()
}

/// Outer groups overflow and break while each nested group still owes the
/// content that follows it. A renderer that re-walked a group's body or its
/// continuation per decision would do quadratic work here.
fn breaking_ladder(size: usize) -> usize {
    let mut d = Doc::new();
    let mut body = d.text("x");
    for _ in 0..size {
        let head = d.text("y");
        let line = d.line(Line::Soft);
        let concat = d.concat([body, line, head]);
        let indented = d.indent(1, concat);
        body = d.group(indented);
    }
    let out = render(
        &d,
        body,
        RenderOptions {
            print_width: 4,
            ..Default::default()
        },
    )
    .unwrap();
    let visible: String = out.text.split_whitespace().collect();
    assert_eq!(visible, format!("x{}", "y".repeat(size)));
    assert!(out.text.lines().count() > size / 2, "outer groups break");
    assert!(out.work.doc_visits <= 3 * d.len() + 64);
    assert!(out.work.peak_stack_entries <= 2 * d.len() + 64);
    out.work.doc_visits
}

#[test]
fn breaking_group_ladder_does_linear_work() {
    let (a, b, c) = (
        breaking_ladder(100),
        breaking_ladder(200),
        breaking_ladder(300),
    );
    // Equal size steps cost equal work: no superlinear term.
    assert_eq!(b - a, c - b);
}

/// The renderer's only production dependency is the width-table regex engine:
/// it cannot reach a parser, semantic analysis or a request router.
#[test]
fn renderer_has_no_parser_semantic_or_routing_dependency() {
    let output = std::process::Command::new(env!("CARGO"))
        .args(["tree", "--locked", "--offline", "--edges", "normal,build"])
        .args(["--depth", "1", "--prefix", "none", "--format", "{p}"])
        .args([
            "--manifest-path",
            concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml"),
        ])
        .args(["--package", "verter_formatter"])
        .output()
        .expect("cargo tree must spawn");
    assert!(
        output.status.success(),
        "cargo tree failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let mut direct: Vec<String> = String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .filter_map(|line| line.split_whitespace().next().map(str::to_owned))
        .collect();
    direct.sort();
    direct.dedup();
    assert_eq!(direct, ["regex", "verter_formatter"]);
}

/// A multi-line authored literal is supplied as one binding per line joined by
/// raw newline text. A queued suffix flushes before the first newline exactly
/// as for generated text, and every source segment keeps its exact authored
/// bytes under a valid binding.
#[test]
fn queued_suffix_orders_identically_for_source_backed_and_generated_multiline_text() {
    const AUTHORED: &str = "a\nb";
    let mut table = FormatProvenanceTable::new(3_u128);
    let mut bind = |start, end| {
        let at = |o| AuthoredFormatByteOffset::for_source(AUTHORED, o).unwrap();
        let range = AuthoredFormatByteRange::for_source(AUTHORED, at(start), at(end)).unwrap();
        AuthoredProvenance::new(table.bind_authored(range), range)
    };
    let (first, second) = (bind(0, 1), bind(2, 3));
    let mut d = Doc::new();
    let suffix = d.line_suffix("!");
    let a = d.source(Provenanced::new("a".into(), first.clone()));
    let newline = d.text("\n");
    let b = d.source(Provenanced::new("b".into(), second.clone()));
    let root = d.concat([suffix, a, newline, b]);
    let source_backed = render(&d, root, RenderOptions::default()).unwrap();

    let mut g = Doc::new();
    let suffix = g.line_suffix("!");
    let text = g.text(AUTHORED);
    let root = g.concat([suffix, text]);
    let generated = render(&g, root, RenderOptions::default()).unwrap();

    assert_eq!(source_backed.text, "a!\nb");
    assert_eq!(source_backed.text, generated.text);
    assert_eq!(
        source_segments(&source_backed),
        [(first.clone(), "a"), (second.clone(), "b")]
    );
    table.validate(&3, &first).unwrap();
    table.validate(&3, &second).unwrap();
}

#[test]
#[should_panic(expected = "source text must be single-line")]
fn source_text_refuses_embedded_newlines() {
    const AUTHORED: &str = "a\nb";
    let range = AuthoredFormatByteRange::for_source(
        AUTHORED,
        AuthoredFormatByteOffset::for_source(AUTHORED, 0).unwrap(),
        AuthoredFormatByteOffset::for_source(AUTHORED, 3).unwrap(),
    )
    .unwrap();
    let mut table = FormatProvenanceTable::new(3_u128);
    let binding = AuthoredProvenance::new(table.bind_authored(range), range);
    Doc::new().source(Provenanced::new(AUTHORED.into(), binding));
}

#[test]
fn output_beyond_the_offset_domain_is_refused_before_allocation() {
    use verter_formatter::render::RenderError;
    let mut d = Doc::new();
    let hard = d.line(Line::Hard);
    let x = d.text("x");
    let body = d.concat([hard, x]);
    let root = d.indent(usize::MAX, body);
    assert_eq!(
        render(&d, root, RenderOptions::default()),
        Err(RenderError::OutputTooLarge)
    );
}
