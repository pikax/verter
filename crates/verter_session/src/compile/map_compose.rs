//! Assembled-module source map support: lifting a decoded fragment map into
//! the typed wire form [`crate::compile::assemble_vue_main_module`]'s own
//! composition needs. The `__sfc__` → `_sfc_main` rewrite itself
//! ([`verter_compiler::assembly::rewrite_script`], `pub(crate)` there) now
//! lives in `verter_compiler` — the SAME algorithm both this crate's
//! host-decorated composer and the direct one-shot core drive through
//! [`verter_compiler::assembly::compose_main_module`]. This module's own
//! remaining job is decode-regime-specific: only THIS crate's hardened,
//! multi-fragment [`super::map_input::DecodedFragmentMap`] needs lifting
//! into an `oxc_sourcemap::SourceMap` before it can cross that boundary.

use verter_compiler::oxc_sourcemap::{SourceMap, Token};

use super::map_input::DecodedFragmentMap;

/// Lift a decoded map into the typed wire form
/// [`verter_compiler::assembly::compose_main_module`]'s request consumes
/// (also used by the caller to re-encode an already-validated template map
/// through this crate's OWN encoder before sequencing: `oxc_sourcemap`'s
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
/// TEST-ONLY: derive an [`verter_compiler::assembly::SfcExportPlacement`]
/// fact for a hand-authored fixture by literal-scanning it — mirroring what
/// a real producer would have declared for that exact text. Legitimate here
/// (fixture setup, not a production code path): "Carrier Geometry From
/// Registered Facts" scopes its scan prohibition to production; tests may
/// scan fixture text to build decoys and inputs.
///
/// Finds AT MOST ONE `export default __sfc__;\n` occurrence (a real
/// producer never emits more than one terminal default export — a fixture
/// simulating two is exercising the retired text-scan behaviour, not this
/// fact-driven one, and is out of this helper's scope) and every literal
/// `__sfc__` occurrence not already covered by it.
pub(crate) fn literal_scan_placement_for_fixture(
    code: &str,
) -> Option<verter_compiler::assembly::SfcExportPlacement> {
    const SFC_BINDING: &str = "__sfc__";
    const EXPORT_STATEMENT_TEXT: &str = "export default __sfc__;\n";

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
    Some(verter_compiler::assembly::SfcExportPlacement {
        binding_ranges,
        export_statement_range,
    })
}
