//! Batch offset conversion at the FFI boundary: a batch of spans visits its
//! source once, and every span converts exactly as a lone conversion does.

use verter_session as host;

use super::offset::visit_counter;
use super::*;
use verter_protocol::types::SpanEncodingTag;

/// Diagnostic counts a file can realistically carry in one snapshot.
const BATCH_SIZES: [usize; 4] = [128, 256, 512, 1024];

fn diagnostic(start: u32, end: u32) -> host::HostDiagnostic {
    host::HostDiagnostic {
        severity: host::HostSeverity::Error,
        code: "E_SPAN".to_string(),
        message: "span".to_string(),
        arguments: Vec::new(),
        span: verter_span::Span::new(start, end),
    }
}

fn snapshot(spans: impl IntoIterator<Item = (u32, u32)>) -> host::DiagnosticsSnapshot {
    host::DiagnosticsSnapshot {
        diagnostics: spans
            .into_iter()
            .map(|(start, end)| diagnostic(start, end))
            .collect(),
        has_errors: true,
    }
}

/// Source bytes the closure visits on this thread.
fn visits(run: impl FnOnce()) -> u64 {
    visit_counter::reset();
    run();
    visit_counter::source_units_visited()
}

/// A 4 KiB ASCII body and a body of the same size dense with astral and
/// BMP multi-byte characters: the two shapes the per-span rescan cost
/// differed on.
fn large_sources() -> [String; 2] {
    let ascii = "const value = 1;\n".repeat(241);
    let astral = "a\u{1F600}\u{00E9}\u{65E5} ".repeat(341);
    [ascii, astral]
}

/// Reference conversion: decode the prefix and count UTF-16 units. Exact or
/// refused with the wire code a diagnostic carries — an independent model of
/// the contract, not the production path.
fn reference_utf16(source: &str, byte_offset: u32) -> Result<u32, &'static str> {
    let offset = byte_offset as usize;
    if offset > source.len() {
        return Err("InvalidBoundary");
    }
    if !source.is_char_boundary(offset) {
        return Err("NotCharBoundary");
    }
    Ok(source[..offset].encode_utf16().count() as u32)
}

#[test]
fn end_position_diagnostic_batches_visit_the_source_once() {
    for source in large_sources() {
        let end = source.len() as u32;
        let mut per_batch = Vec::new();
        for count in BATCH_SIZES {
            let input = snapshot((0..count).map(|_| (end - 1, end)));
            let mut converted = None;
            per_batch.push(visits(|| {
                converted = Some(host_diagnostics_to_ffi(&input, Some(&source)));
            }));
            let converted = converted.expect("batch converted");
            let expected_end = reference_utf16(&source, end).expect("end of file is a boundary");
            assert!(
                converted
                    .diagnostics
                    .iter()
                    .all(|d| d.span_end == Some(expected_end)),
                "every end-position span maps to the source's UTF-16 length"
            );
        }

        // Work does not grow with the batch: 1024 diagnostics visit exactly
        // what 128 do, and that is at most two passes over the source (the
        // ASCII check, then the character walk for a non-ASCII source).
        assert!(
            per_batch.iter().all(|&visited| visited == per_batch[0]),
            "batch visits must not grow with diagnostic count: {per_batch:?}"
        );
        assert!(
            per_batch[0] <= 2 * source.len() as u64,
            "a batch visits its source at most twice: {} for {} bytes",
            per_batch[0],
            source.len()
        );

        // Control: lone conversions rescan the prefix per span, so the same
        // counter grows linearly with the count. This proves the counter
        // observes the rescan the batch path avoids.
        let lone = |count: usize| {
            let input = snapshot((0..count).map(|_| (end - 1, end)));
            visits(|| {
                for d in &input.diagnostics {
                    let _ = host_diagnostic_to_ffi(d, Some(&source));
                }
            })
        };
        let (lone_small, lone_large) = (lone(BATCH_SIZES[0]), lone(BATCH_SIZES[3]));
        assert_eq!(
            lone_large,
            lone_small * 8,
            "lone conversions grow with count"
        );
        assert!(lone_small > 64 * per_batch[0]);
    }
}

#[test]
fn empty_and_sourceless_batches_visit_nothing() {
    let [source, _] = large_sources();
    assert_eq!(
        visits(|| {
            let _ = host_diagnostics_to_ffi(&host::DiagnosticsSnapshot::default(), Some(&source));
        }),
        0,
        "no diagnostics, no index"
    );
    let input = snapshot([(3, 999_999)]);
    assert_eq!(
        visits(|| {
            let ffi = host_diagnostics_to_ffi(&input, None);
            // An absent source is a typed refusal, never a byte pass-through.
            let d = &ffi.diagnostics[0];
            assert_eq!((d.span_start, d.span_end), (None, None));
            assert_eq!(d.span_error.as_deref(), Some("SourceUnavailable"));
            assert_eq!(d.span_encoding, SpanEncodingTag::Utf16);
        }),
        0
    );
}

/// Sources covering ASCII, BMP multi-byte, astral, combining and empty text.
fn parity_sources() -> [&'static str; 6] {
    [
        "",
        "plain ascii",
        "a\u{1F600}\u{00E9}",
        "\u{1F600}middle\u{1F601}",
        "e\u{0301}x\u{65E5}\u{672C}",
        "\u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F467}",
    ]
}

#[test]
fn batch_diagnostics_convert_exactly_like_lone_conversions() {
    for source in parity_sources() {
        // Every interior byte (continuation bytes are refused), EOF, and
        // offsets past EOF (refused), as starts and ends, including inverted
        // spans the host never produces but the boundary must not reorder.
        let offsets: Vec<u32> = (0..=source.len() as u32 + 3).chain([u32::MAX]).collect();
        let spans: Vec<(u32, u32)> = offsets
            .iter()
            .flat_map(|&start| offsets.iter().map(move |&end| (start, end)))
            .collect();
        let input = snapshot(spans.iter().copied());

        for source in [Some(source), None] {
            let batch = host_diagnostics_to_ffi(&input, source);
            assert_eq!(batch.diagnostics.len(), input.diagnostics.len());
            for (converted, original) in batch.diagnostics.iter().zip(&input.diagnostics) {
                let lone = host_diagnostic_to_ffi(original, source);
                assert_eq!(
                    (
                        converted.span_start,
                        converted.span_end,
                        &converted.span_error
                    ),
                    (lone.span_start, lone.span_end, &lone.span_error),
                    "span {:?} of {source:?}",
                    original.span
                );
                assert_eq!(converted.span_encoding, SpanEncodingTag::Utf16);
                let (start, end) = (original.span.start, original.span.end);
                let expected = match source {
                    None => Err("SourceUnavailable"),
                    Some(_) if start > end => Err("InvalidBoundary"),
                    Some(s) => reference_utf16(s, start)
                        .and_then(|start| reference_utf16(s, end).map(|end| (start, end))),
                };
                match expected {
                    Ok((start, end)) => {
                        assert_eq!(
                            (converted.span_start, converted.span_end),
                            (Some(start), Some(end))
                        );
                        assert_eq!(converted.span_error, None);
                    }
                    Err(code) => {
                        assert_eq!((converted.span_start, converted.span_end), (None, None));
                        assert_eq!(converted.span_error.as_deref(), Some(code));
                    }
                }
            }
        }
    }
}

#[test]
fn destructured_bindings_convert_exactly_and_share_one_index() {
    for sfc in parity_sources() {
        let tsx = "const {\u{1F600}: x} = props;";
        let offsets: Vec<u32> = (0..=sfc.len() as u32 + 3).chain([u32::MAX]).collect();
        for encoding in [SpanEncodingTag::Utf8, SpanEncodingTag::Utf16] {
            let offset_encoding = match encoding {
                SpanEncodingTag::Utf8 => OffsetEncoding::Utf8,
                SpanEncodingTag::Utf16 => OffsetEncoding::Utf16,
            };
            for block_end in [3, 6, tsx.len() as u32, 999] {
                for &offset in &offsets {
                    // A start past the source end makes the range inverted, which
                    // is refused where the typed range is built.
                    let Ok(source) = source_range(verter_span::Span::new(offset, sfc.len() as u32))
                    else {
                        assert!(offset > sfc.len() as u32);
                        continue;
                    };
                    let bindings = [DestructuredBindingInput { name: "x", source }];
                    let meta = convert_destructured_block_meta(
                        &bindings,
                        generated_range(verter_span::Span::new(1, block_end)).expect("ordered"),
                        Some(sfc),
                        tsx,
                        encoding,
                    );
                    let expected_source =
                        convert_offset(sfc, offset, offset_encoding).and_then(|start| {
                            convert_offset(sfc, sfc.len() as u32, offset_encoding)
                                .map(|end| (start, end))
                        });
                    let expected_block =
                        convert_offset(tsx, 1, offset_encoding).and_then(|start| {
                            convert_offset(tsx, block_end, offset_encoding).map(|end| (start, end))
                        });
                    match (expected_source, expected_block) {
                        (Ok((start, end)), Ok((block_start, block_end))) => {
                            let meta = meta.expect("exact spans convert");
                            assert_eq!(
                                (meta.bindings[0].source_start, meta.bindings[0].source_end),
                                (start, end)
                            );
                            assert_eq!(
                                (meta.block_start, meta.block_end),
                                (block_start, block_end)
                            );
                            assert_eq!(meta.bindings[0].source_encoding, encoding);
                            assert_eq!(meta.block_encoding, encoding);
                        }
                        _ => assert!(
                            meta.is_err(),
                            "an off-boundary or past-end span refuses the whole conversion"
                        ),
                    }
                }
            }
        }
    }

    // A missing SFC source is a typed failure when there is a binding to
    // convert, never zero offsets.
    let one = [DestructuredBindingInput {
        name: "x",
        source: source_range(verter_span::Span::new(0, 1)).expect("ordered"),
    }];
    assert!(matches!(
        convert_destructured_block_meta(
            &one,
            generated_range(verter_span::Span::new(0, 0)).expect("ordered"),
            None,
            "",
            SpanEncodingTag::Utf16
        ),
        Err(SpanConversionError::SourceUnavailable)
    ));

    // Visits do not grow with the binding count.
    let [_, sfc] = large_sources();
    let end = sfc.len() as u32;
    let per_batch: Vec<u64> = BATCH_SIZES
        .iter()
        .map(|&count| {
            let bindings: Vec<DestructuredBindingInput<'_>> = (0..count)
                .map(|_| DestructuredBindingInput {
                    name: "x",
                    source: source_range(verter_span::Span::new(end - 1, end)).expect("ordered"),
                })
                .collect();
            visits(|| {
                let _ = convert_destructured_block_meta(
                    &bindings,
                    generated_range(verter_span::Span::new(0, 0)).expect("ordered"),
                    Some(&sfc),
                    "",
                    SpanEncodingTag::Utf16,
                );
            })
        })
        .collect();
    assert!(
        per_batch.iter().all(|&visited| visited == per_batch[0]),
        "binding visits must not grow with binding count: {per_batch:?}"
    );
    assert!(per_batch[0] <= 2 * sfc.len() as u64);
}

/// Cross-surface authored-location equivalence: the NAPI/WASM wire (UTF-16),
/// the MCP wire (UTF-8 bytes) and a requester offset converted back through
/// its tag all name the SAME authored text of a source with astral characters
/// before the target. DISCRIMINATING: a surface that skips or double-applies
/// its conversion names different text.
#[test]
fn every_surface_names_the_same_authored_text_after_declared_conversion() {
    let source = "<template><p>\u{1F600}\u{1F600} {{ bad }}</p></template>";
    let target = "bad";
    let start = source.find(target).expect("target is authored") as u32;
    let end = start + target.len() as u32;
    let units: Vec<u16> = source.encode_utf16().collect();
    let lint = |start, end| verter_diagnostics::LintDiagnostic {
        rule: "r".to_string(),
        category: "c".to_string(),
        severity: verter_diagnostics::Severity::Error,
        message: "m".to_string(),
        span: verter_span::Span::new(start, end),
        tags: vec![],
        span_kind: verter_diagnostics::DiagnosticSpanKind::Interpolation,
        certainty: verter_diagnostics::Certainty::Definite,
        evidence: Vec::new(),
        related_files: Vec::new(),
    };

    // NAPI / WASM lint wire: UTF-16 code units under `utf16.v1`.
    let wire = lint_diagnostics_to_wire_utf16(vec![lint(start, end)], Some(source));
    let span = &wire[0].span;
    assert_eq!(span.encoding, SpanEncodingTag::Utf16);
    let (s16, e16) = (span.start.expect("exact"), span.end.expect("exact"));
    assert_eq!(
        String::from_utf16(&units[s16 as usize..e16 as usize]).expect("utf16"),
        target
    );
    assert_ne!(
        (s16, e16),
        (start, end),
        "the astral prefix makes the units differ"
    );

    // MCP lint wire: UTF-8 bytes under `utf8.v1`.
    let mcp = lint_diagnostics_to_wire_utf8(vec![lint(start, end)], Some(source));
    let span = &mcp[0].span;
    assert_eq!(span.encoding, SpanEncodingTag::Utf8);
    assert_eq!(
        &source[span.start.expect("exact") as usize..span.end.expect("exact") as usize],
        target
    );

    // The host diagnostic wire agrees with the lint wire.
    let host_diagnostic = host_diagnostic_to_ffi(
        &host::HostDiagnostic {
            severity: host::HostSeverity::Error,
            code: "E".to_string(),
            message: "m".to_string(),
            arguments: Vec::new(),
            span: verter_span::Span::new(start, end),
        },
        Some(source),
    );
    assert_eq!(
        (host_diagnostic.span_start, host_diagnostic.span_end),
        (Some(s16), Some(e16))
    );

    // A requester offset converts back to the native byte offset through its tag.
    assert_eq!(
        request_offset_to_byte(source, s16, SpanEncodingTag::Utf16),
        Ok(start)
    );
    assert_eq!(
        request_offset_to_byte(source, start, SpanEncodingTag::Utf8),
        Ok(start)
    );
}
