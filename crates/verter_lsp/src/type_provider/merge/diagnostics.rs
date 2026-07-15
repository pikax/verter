//! Diagnostics merge: map TypeProvider diagnostics from TSX positions back to
//! carrier-source positions and combine with verter diagnostics.

use tower_lsp_server::ls_types::*;

use crate::documents::line_index::LineIndex;
use crate::documents::provider_projection::ProviderPositionMapper;
use crate::type_provider::protocol::{
    DiagnosticRelatedInfo, TypeDiagnostic, TypeDiagnosticSeverity, TypeDiagnosticTag,
};

use super::definition::{
    is_carrier_ide_path, normalize_carrier_path, path_to_uri, resolve_carrier_ide_range_strict,
    resolve_external_target_range,
};
use super::position::{tsx_range_to_carrier_range, ExternalIdeResolver, ExternalSourceReader};

/// Merge verter diagnostics with TypeProvider diagnostics.
///
/// Strategy:
/// - Verter diagnostics are already in Vue positions
/// - TypeProvider diagnostics are in TSX positions; map back to Vue
/// - Filter out diagnostics that map to unmapped regions (generated code)
///
/// A diagnostic's `related_information` secondary spans (the "see declaration
/// here" links TS attaches) are mapped back to carrier-source `Location`s through
/// the SAME cross-file/carrier mappers references/rename/code-actions use — hence
/// the external-resolution params (`current_tsx_path`, `external_resolver`,
/// `carrier_source_exists`, `negotiated_encoding`, `source_reader`) mirror
/// [`super::merge_definitions`]. A related span that cannot be mapped is dropped
/// fail-closed (never a line-0 link); a primary diagnostic with all-unmappable
/// related spans still publishes (with no related list).
#[expect(
    clippy::too_many_arguments,
    reason = "related-span map-back needs the same cross-file resolver inputs as merge_definitions"
)]
pub fn merge_diagnostics(
    verter_diags: Vec<Diagnostic>,
    type_diags: Vec<TypeDiagnostic>,
    current_tsx_path: &str,
    tsx_line_index: &LineIndex,
    mapper: &ProviderPositionMapper,
    carrier_line_index: &LineIndex,
    external_resolver: Option<ExternalIdeResolver<'_>>,
    carrier_source_exists: &dyn Fn(&str) -> bool,
    negotiated_encoding: PositionEncodingKind,
    source_reader: ExternalSourceReader<'_>,
) -> Vec<Diagnostic> {
    let mut result = verter_diags;
    let mut dropped = 0u32;

    for diag in &type_diags {
        let range = tsx_range_to_carrier_range(
            diag.start,
            diag.end,
            tsx_line_index,
            mapper,
            carrier_line_index,
        );

        if let Some(range) = range {
            // Map each related span back to a carrier-source Location through the
            // shared cross-file/carrier resolver, fail-closed (drop the unmappable
            // ones). The PRIMARY diagnostic survives regardless of related-span
            // mapping — a missing secondary link never drops the squiggle.
            let related = map_related_information(
                &diag.related_information,
                current_tsx_path,
                tsx_line_index,
                mapper,
                carrier_line_index,
                external_resolver,
                carrier_source_exists,
                negotiated_encoding.clone(),
                source_reader,
            );
            result.push(Diagnostic {
                range,
                severity: Some(convert_severity(diag.severity)),
                code: diag.code.clone().map(NumberOrString::String),
                source: Some("ts".to_string()),
                message: diag.message.clone(),
                tags: convert_tags(&diag.tags),
                related_information: related,
                ..Default::default()
            });
        } else {
            dropped += 1;
            tracing::debug!(
                "merge_diagnostics: dropped type provider diagnostic (unmapped range) — {:?} at offsets {}..{}",
                diag.message,
                diag.start,
                diag.end,
            );
        }
    }

    if dropped > 0 {
        tracing::debug!(
            "merge_diagnostics: {dropped}/{} type provider diagnostics dropped (unmapped ranges)",
            type_diags.len()
        );
    }

    result
}

/// Map a diagnostic's carrier `related_information` spans to LSP
/// [`DiagnosticRelatedInformation`], returning `None` when none survive.
///
/// Each related span's `(path, start, end)` routes through the SAME 3-way
/// classification [`super::merge_definitions`] uses for a navigation target:
/// - a carrier IDE `.tsx`/`.jsx` maps through that file's own CodeTransform
///   sourcemap via [`resolve_carrier_ide_range_strict`], which distinguishes the
///   CURRENT request's TSX (`path` canonically equal to `current_tsx_path` → the
///   in-context mapper) from a FOREIGN carrier (another component's generated file
///   → the external resolver, or DROP when none) — never the current mapper for a
///   foreign file — and emits the carrier-source URI;
/// - any other (real `.ts`/etc.) file reads its own source through the VFS via
///   [`resolve_external_target_range`] and emits a real range + its file URI.
///
/// FAIL-CLOSED: a related entry whose location cannot be mapped is dropped (never
/// a `Range::default()` line-0 link). An empty surviving set yields `None` so an
/// all-unmappable related list never publishes a degenerate link.
#[expect(
    clippy::too_many_arguments,
    reason = "mirrors the merge_definitions per-location resolver inputs"
)]
fn map_related_information(
    related: &[DiagnosticRelatedInfo],
    current_tsx_path: &str,
    tsx_line_index: &LineIndex,
    mapper: &ProviderPositionMapper,
    carrier_line_index: &LineIndex,
    external_resolver: Option<ExternalIdeResolver<'_>>,
    carrier_source_exists: &dyn Fn(&str) -> bool,
    negotiated_encoding: PositionEncodingKind,
    source_reader: ExternalSourceReader<'_>,
) -> Option<Vec<DiagnosticRelatedInformation>> {
    let mapped: Vec<DiagnosticRelatedInformation> = related
        .iter()
        .filter_map(|ri| {
            let location = resolve_related_location(
                ri,
                current_tsx_path,
                tsx_line_index,
                mapper,
                carrier_line_index,
                external_resolver,
                carrier_source_exists,
                negotiated_encoding.clone(),
                source_reader,
            )?;
            Some(DiagnosticRelatedInformation {
                location,
                message: ri.message.clone(),
            })
        })
        .collect();

    (!mapped.is_empty()).then_some(mapped)
}

/// Resolve a single related span `(path, start, end)` to a carrier-source
/// [`Location`], fail-closed (`None` when unmappable).
///
/// This is the diagnostic-side mirror of the per-location routing in
/// [`super::merge_definitions`]: a carrier IDE file maps through its sourcemap and
/// emits the carrier URI; a real file reads its own source and emits a real range.
/// A path whose carrier-suffix normalization rewrites it (a generated decl file
/// with no in-context sourcemap) fails closed.
#[expect(
    clippy::too_many_arguments,
    reason = "mirrors the merge_definitions per-location resolver inputs"
)]
fn resolve_related_location(
    ri: &DiagnosticRelatedInfo,
    current_tsx_path: &str,
    tsx_line_index: &LineIndex,
    mapper: &ProviderPositionMapper,
    carrier_line_index: &LineIndex,
    external_resolver: Option<ExternalIdeResolver<'_>>,
    carrier_source_exists: &dyn Fn(&str) -> bool,
    negotiated_encoding: PositionEncodingKind,
    source_reader: ExternalSourceReader<'_>,
) -> Option<Location> {
    // A carrier IDE virtual file (`{carrier}.tsx`/`.jsx`): map the byte offsets
    // back to the carrier source through that file's own CodeTransform sourcemap —
    // the in-context mapper for the current file, the external resolver for a
    // foreign component. Fail closed when no sourcemap bridges the offsets.
    if is_carrier_ide_path(&ri.path) {
        let uri = path_to_uri(normalize_carrier_path(&ri.path, carrier_source_exists))?;
        let range = resolve_carrier_ide_range_strict(
            &ri.path,
            ri.start,
            ri.end,
            current_tsx_path,
            tsx_line_index,
            mapper,
            carrier_line_index,
            external_resolver,
        )?;
        return Some(Location { uri, range });
    }

    // Every other target: when carrier-suffix normalization is a no-op the path IS
    // the file the byte offsets index (`.ts`/`.d.ts`/`.js`/…). The parser only emits
    // a related entry with a REAL byte offset (a cross-file span with no content is
    // dropped at parse time — never a packed position), so read that source through
    // the VFS and convert the real offsets to a range. Fail closed when the source
    // can't be read (drop, never a line-0 link).
    let normalized = normalize_carrier_path(&ri.path, carrier_source_exists);
    if normalized == ri.path {
        let uri = path_to_uri(normalized)?;
        let range = resolve_external_target_range(
            &ri.path,
            ri.start,
            ri.end,
            negotiated_encoding,
            source_reader,
        )?;
        return Some(Location { uri, range });
    }

    // Normalization rewrote the path (a generated decl file → carrier source) but
    // no in-context sourcemap bridges the offsets: fail closed.
    None
}

/// Translate the provider-neutral carrier tags into LSP `DiagnosticTag`s,
/// mirroring the native lint-bridge mapping in `features::diagnostics_bridge`.
/// `None` when the diagnostic carries no tags (so an untagged diagnostic never
/// publishes an empty `tags` array, keeping parity with the native path).
fn convert_tags(tags: &[TypeDiagnosticTag]) -> Option<Vec<DiagnosticTag>> {
    if tags.is_empty() {
        return None;
    }
    Some(
        tags.iter()
            .map(|t| match t {
                TypeDiagnosticTag::Unnecessary => DiagnosticTag::UNNECESSARY,
                TypeDiagnosticTag::Deprecated => DiagnosticTag::DEPRECATED,
            })
            .collect(),
    )
}

fn convert_severity(sev: TypeDiagnosticSeverity) -> DiagnosticSeverity {
    match sev {
        TypeDiagnosticSeverity::Error => DiagnosticSeverity::ERROR,
        TypeDiagnosticSeverity::Warning => DiagnosticSeverity::WARNING,
        TypeDiagnosticSeverity::Info => DiagnosticSeverity::INFORMATION,
        TypeDiagnosticSeverity::Hint => DiagnosticSeverity::HINT,
    }
}
