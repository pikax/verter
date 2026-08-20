//! Assembled-module source map: the authorized `__sfc__` rewrite (applied
//! from a producer-declared fact, never rediscovered by scanning generated
//! text) and the chained per-fragment map it produces.

use verter_compiler::assembly::SfcExportPlacement;
use verter_compiler::code_transform::{CodeTransform, SourceMapChainError};
use verter_compiler::oxc_sourcemap::{SourceMap, Token};

use super::map_input::DecodedFragmentMap;

/// Every `binding_ranges` entry's own bytes must equal this literal — the
/// identifier every runtime-emission site writes before host assembly
/// renames it (see `verter_compiler::script::SFC_BINDING`, which this
/// mirrors as an independent constant rather than a cross-crate `pub`
/// surface for one literal).
const SFC_BINDING: &str = "__sfc__";
/// Every declared binding reference is renamed to this.
const SFC_MAIN_BINDING: &str = "_sfc_main";
/// The exact bytes a declared `export_statement_range` must contain — the
/// terminal statement removed once the assembled module re-exports the
/// composed result under its own name.
const EXPORT_STATEMENT_TEXT: &str = "export default __sfc__;\n";

/// Why a declared [`SfcExportPlacement`] fact was refused. Every variant is
/// a producer defect — a script whose own bytes disagree with what its
/// producer claims about them — never a condition [`rewrite_script`]
/// recovers from by falling back to scanning. A MISSING fact (`None`) is
/// deliberately not one of these: it is indistinguishable, without
/// scanning, from a genuinely empty declared fact (a script with nothing to
/// rewrite — e.g. one authored purely to exercise unrelated map-composition
/// mechanics), so `rewrite_script` treats the two identically rather than
/// refusing one of two equally-untestable-without-a-scan possibilities.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SfcRewriteRefusal {
    /// A declared range's `end` exceeds the script's own byte length, or
    /// its `start`/`end` do not land on a UTF-8 character boundary.
    OutOfBounds { start: u32, end: u32 },
    /// A declared binding range's bytes are not literally `__sfc__`.
    InconsistentBindingRange { start: u32, end: u32 },
    /// The declared export-statement range's bytes are not literally
    /// `export default __sfc__;\n`.
    InconsistentExportStatement { start: u32, end: u32 },
    /// A declared binding range partially overlaps the declared
    /// export-statement range — neither fully inside it (where it would be
    /// removed as part of the whole statement) nor fully outside it.
    BindingRangeOverlapsExportStatement { start: u32, end: u32 },
    /// Chaining the rewrite's own transform onto the caller-supplied input
    /// map failed — the input map names a generated position the rewrite's
    /// transform does not tile (out of bounds, or an unsupported chunk
    /// shape). The rewrite's own transform is always overwrite-only over
    /// positions already validated against `code`; a chain failure means
    /// the INPUT map disagrees with the script it claims to describe.
    ChainFailed(SourceMapChainError),
}

fn checked_slice(code: &str, start: u32, end: u32) -> Result<&str, SfcRewriteRefusal> {
    let (s, e) = (start as usize, end as usize);
    if start > end || e > code.len() || !code.is_char_boundary(s) || !code.is_char_boundary(e) {
        return Err(SfcRewriteRefusal::OutOfBounds { start, end });
    }
    Ok(&code[s..e])
}

/// Apply the ONE authorized rewrite — every `binding_ranges` entry renamed
/// to `_sfc_main`, and the declared `export_statement_range` (if any)
/// removed — driven entirely by `fact`'s declared ranges. Never scans
/// `code` for the landmark strings: an out-of-bounds or inconsistent fact
/// is a typed [`SfcRewriteRefusal`], not a rescan. `fact: None` is treated
/// identically to a declared-but-empty fact (nothing to rewrite) — see
/// [`SfcRewriteRefusal`]'s own doc for why a missing fact is not itself a
/// refusal.
///
/// Runs whether or not a map was requested: the rewrite determines the
/// module's bytes regardless of `map`.
pub(crate) fn rewrite_script(
    code: &str,
    fact: Option<&SfcExportPlacement>,
    map: Option<&DecodedFragmentMap>,
) -> Result<(String, Option<String>), SfcRewriteRefusal> {
    let empty = SfcExportPlacement::default();
    let fact = fact.unwrap_or(&empty);

    // Validate every declared range to completion BEFORE any edit is
    // queued — a refusal must never leave a partially-rewritten transform.
    for range in &fact.binding_ranges {
        let slice = checked_slice(code, range.start, range.end)?;
        if slice != SFC_BINDING {
            return Err(SfcRewriteRefusal::InconsistentBindingRange {
                start: range.start,
                end: range.end,
            });
        }
    }
    if let Some(export) = &fact.export_statement_range {
        let slice = checked_slice(code, export.start, export.end)?;
        if slice != EXPORT_STATEMENT_TEXT {
            return Err(SfcRewriteRefusal::InconsistentExportStatement {
                start: export.start,
                end: export.end,
            });
        }
        for range in &fact.binding_ranges {
            let inside = range.start >= export.start && range.end <= export.end;
            let outside = range.end <= export.start || range.start >= export.end;
            if !inside && !outside {
                return Err(SfcRewriteRefusal::BindingRangeOverlapsExportStatement {
                    start: range.start,
                    end: range.end,
                });
            }
        }
    }

    // A binding fully inside the export statement is removed wholesale
    // with it — renaming it separately would be a redundant, overlapping
    // edit over the same bytes the export-statement overwrite already
    // covers.
    let inside_export = |range: &std::ops::Range<u32>| {
        fact.export_statement_range
            .as_ref()
            .is_some_and(|export| range.start >= export.start && range.end <= export.end)
    };

    let allocator = oxc_allocator::Allocator::default();
    let mut ct = CodeTransform::new(code, &allocator);
    for range in &fact.binding_ranges {
        if inside_export(range) {
            continue;
        }
        ct.overwrite(range.start, range.end, SFC_MAIN_BINDING);
    }
    if let Some(export) = &fact.export_statement_range {
        ct.overwrite(export.start, export.end, "");
    }
    let rewritten = ct.build_string();

    // The rewrite is an overwrite-only transform over positions this
    // function already validated against `code`, so a chain failure here
    // is unexpected — but `chain_source_map` genuinely returns failures for
    // an out-of-bounds or malformed INPUT map (`map` came from the caller,
    // not from this function's own transform), so it is reported typed
    // rather than unwound.
    let chained = map
        .map(|map| {
            ct.chain_source_map(&to_source_map(map))
                .map(|chained_map| chained_map.to_json_string())
        })
        .transpose()
        .map_err(SfcRewriteRefusal::ChainFailed)?;

    Ok((rewritten, chained))
}

/// Lift a decoded map into the typed wire form the chain consumes (also
/// used by the caller to re-encode an already-validated template map
/// through this crate's OWN encoder before sequencing it: `oxc_sourcemap`'s
/// decoder rejects an otherwise-valid map declaring BOTH accepted
/// ignore-list spellings as a "duplicate field" — a stricter reader than
/// `validate_and_decode`'s explicit "both spellings, must agree" rule — so
/// only the single canonical spelling this encoder emits may cross that
/// boundary safely). Tables ride along untouched; only the segment
/// sequence is what chaining acts on.
pub(crate) fn to_source_map(map: &DecodedFragmentMap) -> SourceMap<'static> {
    use std::borrow::Cow;

    let tokens: Vec<Token> = map
        .segments
        .iter()
        .map(|segment| match segment.payload {
            Some(payload) => Token::new(
                segment.generated_line,
                segment.generated_column,
                payload.source_line,
                payload.source_column,
                Some(payload.source_index),
                payload.name_index,
            ),
            None => Token::new(
                segment.generated_line,
                segment.generated_column,
                0,
                0,
                None,
                None,
            ),
        })
        .collect();

    let mut source_map = SourceMap::new(
        None,
        map.names
            .iter()
            .map(|name| Cow::Owned(name.clone()))
            .collect(),
        map.source_root.clone().map(Cow::Owned),
        map.sources
            .iter()
            .map(|source| Cow::Owned(source.clone()))
            .collect(),
        map.sources_content
            .as_ref()
            .map(|rows| rows.iter().map(|row| row.clone().map(Cow::Owned)).collect())
            .unwrap_or_default(),
        tokens.into_boxed_slice(),
        None,
    );
    // `chain_source_map` copies its INPUT's ignore list onto its output
    // (see `code_transform::chain`) — it must be set here, on the map fed
    // INTO the chain, or the rewritten script's re-encoded map would
    // silently lose every ignore-listed source row.
    if !map.ignore_list.is_empty() {
        source_map
            .set_x_google_ignore_list(map.ignore_list.iter().map(|entry| *entry as u32).collect());
    }
    source_map
}

#[cfg(test)]
/// TEST-ONLY: derive an [`SfcExportPlacement`] fact for a hand-authored
/// fixture by literal-scanning it — mirroring what a real producer would
/// have declared for that exact text. Legitimate here (fixture setup, not
/// a production code path): "Carrier Geometry From Registered Facts" scopes
/// its scan prohibition to production; tests may scan fixture text to build
/// decoys and inputs.
///
/// Finds AT MOST ONE `export default __sfc__;\n` occurrence (a real
/// producer never emits more than one terminal default export — a fixture
/// simulating two is exercising the retired text-scan behaviour, not this
/// fact-driven one, and is out of this helper's scope) and every literal
/// `__sfc__` occurrence not already covered by it.
pub(crate) fn literal_scan_placement_for_fixture(code: &str) -> Option<SfcExportPlacement> {
    let export_statement_range = code.find(EXPORT_STATEMENT_TEXT).map(|start| {
        let start = start as u32;
        start..start + EXPORT_STATEMENT_TEXT.len() as u32
    });

    let mut binding_ranges = Vec::new();
    let mut from = 0usize;
    while let Some(relative) = code[from..].find(SFC_BINDING) {
        let at = (from + relative) as u32;
        let end = at + SFC_BINDING.len() as u32;
        binding_ranges.push(at..end);
        from = end as usize;
    }

    if binding_ranges.is_empty() && export_statement_range.is_none() {
        return None;
    }
    Some(SfcExportPlacement {
        binding_ranges,
        export_statement_range,
    })
}
