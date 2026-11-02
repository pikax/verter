//! Document-symbol and CSS selector-match projections for the FFI boundary.
//!
//! The single owner both bindings (NAPI and WASM) call: every span is
//! converted from the analysis' UTF-8 byte offsets to UTF-16 against the
//! exact source the analysis ran on, exact or refused — an offset that is
//! off-boundary or past-end refuses the whole projection ([`CoordError`])
//! rather than shipping a clamped range.

use verter_protocol::types::{
    FfiDocumentSymbol, FfiElementMatch, FfiSelectorMatchResult, SpanEncodingTag,
};
use verter_span::CoordError;

use super::offset::{source_range, OffsetEncoding, OffsetIndex};

/// Monaco SymbolKind constants (subset used for document symbols).
pub mod symbol_kind {
    pub const MODULE: u32 = 1;
    pub const VARIABLE: u32 = 12;
    pub const FUNCTION: u32 = 11;
    pub const CLASS: u32 = 4;
    pub const STRUCT: u32 = 22;
    pub const PROPERTY: u32 = 6;
    pub const KEY: u32 = 19;
}

/// A symbol over one converted UTF-16 range (span and selection coincide).
fn symbol(
    name: String,
    detail: Option<String>,
    kind: u32,
    (start, end): (u32, u32),
    children: Vec<FfiDocumentSymbol>,
) -> FfiDocumentSymbol {
    FfiDocumentSymbol {
        name,
        detail,
        kind,
        span_start: start,
        span_end: end,
        selection_start: start,
        selection_end: end,
        span_encoding: SpanEncodingTag::Utf16,
        children,
    }
}

/// The smallest UTF-16 range covering every range of `parts`; `None` when
/// there are none. A container symbol has no source range of its own, so its
/// range is the one its members really occupy, never a placeholder.
fn covering(parts: impl IntoIterator<Item = (u32, u32)>) -> Option<(u32, u32)> {
    parts
        .into_iter()
        .reduce(|(start, end), (s, e)| (start.min(s), end.max(e)))
}

/// Build document symbols from analysis data.
///
/// Every range is converted with [`OffsetIndex::convert_range`], so an
/// inverted, off-boundary or past-end range refuses the whole projection.
/// A container (`script`, `template`, `style`) is emitted only when it has a
/// real covering range.
pub fn build_document_symbols_from_analysis(
    snapshot: &verter_session_query::analysis::file_analysis::FileAnalysisSnapshot,
    source: &str,
) -> Result<Vec<FfiDocumentSymbol>, CoordError> {
    // One index per source, shared by every symbol span.
    let index = OffsetIndex::new(source);
    let utf16 =
        |span: verter_span::Span| index.convert_range(source_range(span)?, OffsetEncoding::Utf16);
    let mut symbols = Vec::new();

    if !snapshot.bindings.is_empty() || !snapshot.imports.is_empty() || !snapshot.macros.is_empty()
    {
        let mut children = Vec::new();
        let mut ranges = Vec::new();

        for imp in &snapshot.imports {
            let range = utf16(imp.span)?;
            ranges.push(range);
            children.push(symbol(
                imp.source.clone(),
                imp.is_type_only.then(|| "type import".to_string()),
                symbol_kind::MODULE,
                range,
                Vec::new(),
            ));
        }

        for binding in &snapshot.bindings {
            let kind = match binding.kind {
                verter_session_query::analysis::types::AnalyzedBindingKind::Function
                | verter_session_query::analysis::types::AnalyzedBindingKind::AsyncFunction => {
                    symbol_kind::FUNCTION
                }
                verter_session_query::analysis::types::AnalyzedBindingKind::Class => {
                    symbol_kind::CLASS
                }
                _ => symbol_kind::VARIABLE,
            };
            let range = utf16(binding.span)?;
            ranges.push(range);
            children.push(symbol(
                binding.name.clone(),
                binding.type_annotation.clone(),
                kind,
                range,
                Vec::new(),
            ));
        }

        for m in snapshot.macros.iter() {
            let range = utf16(m.span)?;
            ranges.push(range);
            children.push(symbol(
                format!("{:?}", m.kind),
                m.is_type_based.then(|| "type-based".to_string()),
                symbol_kind::FUNCTION,
                range,
                Vec::new(),
            ));
        }

        if let Some(range) = covering(ranges) {
            symbols.push(symbol(
                "script".to_string(),
                Some(format!(
                    "{} binding(s), {} import(s)",
                    snapshot.bindings.len(),
                    snapshot.imports.len()
                )),
                symbol_kind::MODULE,
                range,
                children,
            ));
        }
    }

    if let Some(template) = &snapshot.template {
        let mut children = Vec::new();
        let mut ranges = Vec::new();

        for comp in &template.components {
            let range = utf16(comp.span)?;
            ranges.push(range);
            children.push(symbol(
                comp.name.clone(),
                Some(format!("{} prop(s)", comp.props.len())),
                symbol_kind::CLASS,
                range,
                Vec::new(),
            ));
        }
        for element in &template.elements {
            ranges.push(utf16(element.span)?);
        }

        if let Some(range) = covering(ranges) {
            symbols.push(symbol(
                "template".to_string(),
                Some(format!("{} component(s)", template.components.len())),
                symbol_kind::STRUCT,
                range,
                children,
            ));
        }
    }

    for (i, style) in snapshot.styles.iter().enumerate() {
        let mut children = Vec::new();
        let mut ranges = Vec::new();

        if let Some(css) = &style.css {
            for class in &css.classes {
                let range = utf16(class.span)?;
                ranges.push(range);
                children.push(symbol(
                    format!(".{}", class.name),
                    None,
                    symbol_kind::PROPERTY,
                    range,
                    Vec::new(),
                ));
            }
            for selector in &css.selectors {
                ranges.push(utf16(selector.span)?);
            }
        }

        if let Some(range) = covering(ranges) {
            symbols.push(symbol(
                format!(
                    "style{}{}",
                    if i > 0 {
                        format!(" {}", i)
                    } else {
                        String::new()
                    },
                    if style.scoped { " (scoped)" } else { "" }
                ),
                None,
                symbol_kind::KEY,
                range,
                children,
            ));
        }
    }

    Ok(symbols)
}

/// Build CSS selector match results for visualization.
pub fn build_selector_match_results(
    snapshot: &verter_session_query::analysis::file_analysis::FileAnalysisSnapshot,
    source: &str,
) -> Result<Vec<FfiSelectorMatchResult>, CoordError> {
    let template = match &snapshot.template {
        Some(t) => t,
        None => return Ok(Vec::new()),
    };

    // One index per source, shared by every selector and element span.
    let index = OffsetIndex::new(source);
    let mut results = Vec::new();

    for style in snapshot.styles.iter() {
        let css = match &style.css {
            Some(c) => c,
            None => continue,
        };

        for selector in &css.selectors {
            let parsed = match &selector.structure {
                Some(s) => s.clone(),
                None => continue,
            };

            let mut matches = Vec::new();
            for (idx, element) in template.elements.iter().enumerate() {
                let result = verter_semantic::analysis::selector_match::match_selector(
                    &parsed,
                    idx,
                    &template.elements,
                );
                let element_range =
                    index.convert_range(source_range(element.span)?, OffsetEncoding::Utf16)?;
                matches.push(FfiElementMatch {
                    tag: element.tag.clone(),
                    span_start: element_range.0,
                    span_end: element_range.1,
                    span_encoding: SpanEncodingTag::Utf16,
                    result: match result {
                        verter_semantic::analysis::selector_match::MatchResult::Matches => {
                            "match".to_string()
                        }
                        verter_semantic::analysis::selector_match::MatchResult::MaybeMatches => {
                            "maybe".to_string()
                        }
                        verter_semantic::analysis::selector_match::MatchResult::NoMatch => {
                            "no".to_string()
                        }
                    },
                });
            }

            let selector_range =
                index.convert_range(source_range(selector.span)?, OffsetEncoding::Utf16)?;
            results.push(FfiSelectorMatchResult {
                selector_text: selector.text.clone(),
                selector_start: selector_range.0,
                selector_end: selector_range.1,
                span_encoding: SpanEncodingTag::Utf16,
                matches,
            });
        }
    }

    Ok(results)
}
