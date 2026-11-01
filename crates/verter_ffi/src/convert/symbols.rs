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

use super::offset::OffsetIndex;

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

/// Build document symbols from analysis data.
pub fn build_document_symbols_from_analysis(
    snapshot: &verter_session_query::analysis::file_analysis::FileAnalysisSnapshot,
    source: &str,
) -> Result<Vec<FfiDocumentSymbol>, CoordError> {
    // One index per source, shared by every symbol span.
    let index = OffsetIndex::new(source);
    let mut symbols = Vec::new();

    if !snapshot.bindings.is_empty() || !snapshot.imports.is_empty() || !snapshot.macros.is_empty()
    {
        let mut children = Vec::new();

        for imp in &snapshot.imports {
            children.push(FfiDocumentSymbol {
                name: imp.source.clone(),
                detail: if imp.is_type_only {
                    Some("type import".to_string())
                } else {
                    None
                },
                kind: symbol_kind::MODULE,
                span_start: 0,
                span_end: 0,
                selection_start: 0,
                selection_end: 0,
                span_encoding: SpanEncodingTag::Utf16,
                children: Vec::new(),
            });
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
            children.push(FfiDocumentSymbol {
                name: binding.name.clone(),
                detail: binding.type_annotation.clone(),
                kind,
                span_start: index.to_utf16(binding.span.start)?,
                span_end: index.to_utf16(binding.span.end)?,
                selection_start: index.to_utf16(binding.span.start)?,
                selection_end: index.to_utf16(binding.span.end)?,
                span_encoding: SpanEncodingTag::Utf16,
                children: Vec::new(),
            });
        }

        for m in snapshot.macros.iter() {
            children.push(FfiDocumentSymbol {
                name: format!("{:?}", m.kind),
                detail: if m.is_type_based {
                    Some("type-based".to_string())
                } else {
                    None
                },
                kind: symbol_kind::FUNCTION,
                span_start: 0,
                span_end: 0,
                selection_start: 0,
                selection_end: 0,
                span_encoding: SpanEncodingTag::Utf16,
                children: Vec::new(),
            });
        }

        symbols.push(FfiDocumentSymbol {
            name: "script".to_string(),
            detail: Some(format!(
                "{} binding(s), {} import(s)",
                snapshot.bindings.len(),
                snapshot.imports.len()
            )),
            kind: symbol_kind::MODULE,
            span_start: 0,
            span_end: 0,
            selection_start: 0,
            selection_end: 0,
            span_encoding: SpanEncodingTag::Utf16,
            children,
        });
    }

    if let Some(template) = &snapshot.template {
        let mut children = Vec::new();

        for comp in &template.components {
            children.push(FfiDocumentSymbol {
                name: comp.name.clone(),
                detail: Some(format!("{} prop(s)", comp.props.len())),
                kind: symbol_kind::CLASS,
                span_start: index.to_utf16(comp.span.start)?,
                span_end: index.to_utf16(comp.span.end)?,
                selection_start: index.to_utf16(comp.span.start)?,
                selection_end: index.to_utf16(comp.span.end)?,
                span_encoding: SpanEncodingTag::Utf16,
                children: Vec::new(),
            });
        }

        symbols.push(FfiDocumentSymbol {
            name: "template".to_string(),
            detail: Some(format!("{} component(s)", template.components.len())),
            kind: symbol_kind::STRUCT,
            span_start: 0,
            span_end: index.to_utf16(source.len() as u32)?,
            selection_start: 0,
            selection_end: 0,
            span_encoding: SpanEncodingTag::Utf16,
            children,
        });
    }

    for (i, style) in snapshot.styles.iter().enumerate() {
        let mut children = Vec::new();

        if let Some(css) = &style.css {
            for class in &css.classes {
                children.push(FfiDocumentSymbol {
                    name: format!(".{}", class.name),
                    detail: None,
                    kind: symbol_kind::PROPERTY,
                    span_start: index.to_utf16(class.span.start)?,
                    span_end: index.to_utf16(class.span.end)?,
                    selection_start: index.to_utf16(class.span.start)?,
                    selection_end: index.to_utf16(class.span.end)?,
                    span_encoding: SpanEncodingTag::Utf16,
                    children: Vec::new(),
                });
            }
        }

        symbols.push(FfiDocumentSymbol {
            name: format!(
                "style{}{}",
                if i > 0 {
                    format!(" {}", i)
                } else {
                    String::new()
                },
                if style.scoped { " (scoped)" } else { "" }
            ),
            detail: None,
            kind: symbol_kind::KEY,
            span_start: 0,
            span_end: 0,
            selection_start: 0,
            selection_end: 0,
            span_encoding: SpanEncodingTag::Utf16,
            children,
        });
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
                matches.push(FfiElementMatch {
                    tag: element.tag.clone(),
                    span_start: index.to_utf16(element.span.start)?,
                    span_end: index.to_utf16(element.span.end)?,
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

            results.push(FfiSelectorMatchResult {
                selector_text: selector.text.clone(),
                selector_start: index.to_utf16(selector.span.start)?,
                selector_end: index.to_utf16(selector.span.end)?,
                span_encoding: SpanEncodingTag::Utf16,
                matches,
            });
        }
    }

    Ok(results)
}
