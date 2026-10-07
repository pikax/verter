//! Batch offset conversion at the FFI boundary: a batch of spans visits its
//! source once, and every span converts exactly as a lone conversion does.

use verter_session as host;

use super::offset::visit_counter;
use super::*;

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

/// Reference conversion: decode the clamped prefix and count UTF-16 units.
fn reference_utf16(source: &str, byte_offset: u32) -> u32 {
    let mut clamped = (byte_offset as usize).min(source.len());
    while !source.is_char_boundary(clamped) {
        clamped -= 1;
    }
    source[..clamped].encode_utf16().count() as u32
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
            let expected_end = reference_utf16(&source, end);
            assert!(
                converted
                    .diagnostics
                    .iter()
                    .all(|d| d.span_end == expected_end),
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
            // An absent source passes byte offsets through unclamped.
            assert_eq!(
                (ffi.diagnostics[0].span_start, ffi.diagnostics[0].span_end),
                (3, 999_999)
            );
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
        // Every interior byte (continuation bytes clamp back), EOF, and
        // offsets past EOF (clamp to the end), as starts and ends, including
        // inverted spans the host never produces but the boundary must not
        // reorder.
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
                    (converted.span_start, converted.span_end),
                    (lone.span_start, lone.span_end),
                    "span {:?} of {source:?}",
                    original.span
                );
                let expected = |offset| source.map_or(offset, |s| reference_utf16(s, offset));
                assert_eq!(converted.span_start, expected(original.span.start));
                assert_eq!(converted.span_end, expected(original.span.end));
            }
        }
    }
}

#[test]
fn destructured_bindings_convert_exactly_and_share_one_index() {
    for sfc in parity_sources() {
        let tsx = "const {\u{1F600}: x} = props;";
        let offsets: Vec<u32> = (0..=sfc.len() as u32 + 3).chain([u32::MAX]).collect();
        let bindings: Vec<DestructuredBindingInput<'_>> = offsets
            .iter()
            .map(|&offset| DestructuredBindingInput {
                name: "x",
                source_start: offset,
                source_end: sfc.len() as u32,
            })
            .collect();
        for encoding in [
            OffsetEncoding::Utf8,
            OffsetEncoding::Utf16,
            OffsetEncoding::Utf32,
        ] {
            for block_end in [3, 6, tsx.len() as u32, 999] {
                let meta =
                    convert_destructured_block_meta(&bindings, 1, block_end, sfc, tsx, encoding);
                for (converted, input) in meta.bindings.iter().zip(&bindings) {
                    assert_eq!(
                        converted.source_start,
                        convert_offset(sfc, input.source_start, encoding)
                    );
                    assert_eq!(
                        converted.source_end,
                        convert_offset(sfc, input.source_end, encoding)
                    );
                }
                assert_eq!(meta.block_start, convert_offset(tsx, 1, encoding));
                assert_eq!(meta.block_end, convert_offset(tsx, block_end, encoding));
            }
        }
    }

    // Visits do not grow with the binding count.
    let [_, sfc] = large_sources();
    let end = sfc.len() as u32;
    let per_batch: Vec<u64> = BATCH_SIZES
        .iter()
        .map(|&count| {
            let bindings: Vec<DestructuredBindingInput<'_>> = (0..count)
                .map(|_| DestructuredBindingInput {
                    name: "x",
                    source_start: end - 1,
                    source_end: end,
                })
                .collect();
            visits(|| {
                let _ = convert_destructured_block_meta(
                    &bindings,
                    0,
                    0,
                    &sfc,
                    "",
                    OffsetEncoding::Utf16,
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
