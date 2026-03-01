//! Merge logic for combining verter analysis results with TypeProvider results.
//!
//! Each merge function takes verter-only results and TypeProvider results,
//! producing enhanced output. All functions handle the case where either
//! source may be absent (graceful fallback).

use tower_lsp_server::lsp_types::*;

use crate::documents::line_index::LineIndex;
use crate::documents::position_map::PositionMapper;
#[cfg(test)]
use crate::tsgo::protocol::Completion;
use crate::tsgo::protocol::{
    self, CompletionKind, CompletionResult, HoverInfo, InlayHint, InlayHintKind, RenameLocation,
    TypeCodeAction, TypeDiagnostic, TypeDiagnosticSeverity, TypeDocumentHighlight,
    TypeDocumentHighlightKind, TypeLocation,
};

/// Map an LSP `Position` (in the Vue file) to a byte offset in the generated TSX.
///
/// Steps: LSP Position → byte offset via LineIndex → line/col → PositionMapper → TSX line/col → TSX byte offset via TSX LineIndex.
///
/// Returns `None` if any mapping step fails.
pub fn vue_position_to_tsx_offset(
    position: &Position,
    _vue_line_index: &LineIndex,
    mapper: &PositionMapper,
    tsx_line_index: &LineIndex,
) -> Option<u32> {
    let vue_line = position.line;
    let vue_col = position.character;
    let tsx_pos = mapper.vue_to_tsx(vue_line, vue_col)?;
    tsx_line_index.position_to_offset(&Position {
        line: tsx_pos.line,
        character: tsx_pos.column,
    })
}

/// Map a Vue position to a TSX byte offset, with round-trip validation.
///
/// After mapping Vue→TSX, verifies the TSX offset maps back to the same Vue line.
/// Returns `None` if the round-trip fails (indicating the TSX offset is in a synthetic
/// region like generated JSX for HTML elements, where TSGO queries would crash).
pub fn vue_position_to_tsx_offset_validated(
    position: &Position,
    vue_line_index: &LineIndex,
    mapper: &PositionMapper,
    tsx_line_index: &LineIndex,
) -> Option<u32> {
    let tsx_offset = vue_position_to_tsx_offset(position, vue_line_index, mapper, tsx_line_index)?;

    // Round-trip: TSX offset → TSX position → Vue position
    let tsx_pos = tsx_line_index.offset_to_position(tsx_offset)?;
    let vue_roundtrip = mapper.tsx_to_vue(tsx_pos.line, tsx_pos.character)?;

    // The round-trip Vue position should be on the same line as the original.
    // If not, the TSX offset is in a synthetic region with no valid source correlation.
    if vue_roundtrip.line == position.line {
        Some(tsx_offset)
    } else {
        None
    }
}

/// Map a TSX byte offset range back to an LSP `Range` in the Vue source.
///
/// Returns `None` if any mapping step fails.
pub fn tsx_range_to_vue_range(
    tsx_start: u32,
    tsx_end: u32,
    tsx_line_index: &LineIndex,
    mapper: &PositionMapper,
    vue_line_index: &LineIndex,
) -> Option<Range> {
    let start_pos = tsx_line_index.offset_to_position(tsx_start)?;
    let end_pos = tsx_line_index.offset_to_position(tsx_end)?;

    let vue_start = mapper.tsx_to_vue(start_pos.line, start_pos.character)?;
    let vue_end = mapper.tsx_to_vue(end_pos.line, end_pos.character)?;

    // Validate the mapped positions produce valid byte offsets
    let start_lsp = Position {
        line: vue_start.line,
        character: vue_start.column,
    };
    let end_lsp = Position {
        line: vue_end.line,
        character: vue_end.column,
    };
    vue_line_index.position_to_offset(&start_lsp)?;
    vue_line_index.position_to_offset(&end_lsp)?;

    Some(Range {
        start: start_lsp,
        end: end_lsp,
    })
}

// ── Hover merge ────────────────────────────────────────────────────

/// Merge verter hover with TypeProvider hover.
///
/// Strategy:
/// - If TypeProvider provides hover info, prepend the type signature to verter's hover content
/// - If only verter provides hover, use it as-is
/// - If only TypeProvider provides hover, use it (mapped back to Vue positions)
pub fn merge_hover(
    verter_hover: Option<Hover>,
    type_hover: Option<HoverInfo>,
    _mapper: &PositionMapper,
    _tsx_line_index: &LineIndex,
    _vue_line_index: &LineIndex,
) -> Option<Hover> {
    match (verter_hover, type_hover) {
        (Some(verter), Some(type_info)) => {
            // TSGO provides the richer type signature — strip verter's leading code block
            // to avoid duplicate fenced blocks in the merged hover.
            let verter_text = extract_hover_text(&verter);
            let context = strip_leading_code_block(&verter_text);
            let merged = if context.trim().is_empty() {
                format!("```typescript\n{}\n```", type_info.contents)
            } else {
                format!(
                    "```typescript\n{}\n```\n---\n{}",
                    type_info.contents, context
                )
            };
            Some(Hover {
                contents: HoverContents::Markup(MarkupContent {
                    kind: MarkupKind::Markdown,
                    value: merged,
                }),
                range: verter.range,
            })
        }
        (Some(verter), None) => Some(verter),
        (None, Some(type_info)) => Some(Hover {
            contents: HoverContents::Markup(MarkupContent {
                kind: MarkupKind::Markdown,
                value: format!("```typescript\n{}\n```", type_info.contents),
            }),
            range: None,
        }),
        (None, None) => None,
    }
}

fn extract_hover_text(hover: &Hover) -> String {
    match &hover.contents {
        HoverContents::Markup(m) => m.value.clone(),
        HoverContents::Scalar(MarkedString::String(s)) => s.clone(),
        HoverContents::Scalar(MarkedString::LanguageString(ls)) => ls.value.clone(),
        HoverContents::Array(items) => items
            .iter()
            .map(|item| match item {
                MarkedString::String(s) => s.clone(),
                MarkedString::LanguageString(ls) => ls.value.clone(),
            })
            .collect::<Vec<_>>()
            .join("\n"),
    }
}

/// Strip the leading fenced code block from hover text.
///
/// If `text` starts with ` ```...lang\n...\n``` `, removes that block and returns
/// the remainder. This prevents duplicate code fences when merging TSGO + verter hover.
fn strip_leading_code_block(text: &str) -> &str {
    if let Some(rest) = text.strip_prefix("```") {
        if let Some(end) = rest.find("\n```") {
            let after = &rest[end + 4..];
            return after.trim_start_matches('\n');
        }
    }
    text
}

// ── Completion merge ───────────────────────────────────────────────

/// Internal verter helper prefix that should be filtered from completions.
const VERTER_INTERNAL_PREFIX: &str = "___VERTER___";

/// Merge verter completions with TypeProvider completions.
///
/// Strategy:
/// - Combine both lists
/// - Filter out internal `___VERTER___` identifiers from TypeProvider results
/// - Deduplicate by label (verter items take priority for sort ordering)
pub fn merge_completions(
    verter_items: Vec<CompletionItem>,
    type_result: CompletionResult,
    mapper: &PositionMapper,
    tsx_line_index: &LineIndex,
    vue_line_index: &LineIndex,
) -> (Vec<CompletionItem>, bool) {
    let is_incomplete = type_result.is_incomplete;
    let mut result = verter_items;
    let mut seen_labels: std::collections::HashSet<String> =
        result.iter().map(|i| i.label.clone()).collect();

    for item in type_result.items {
        // Filter internal verter identifiers
        if item.label.starts_with(VERTER_INTERNAL_PREFIX) {
            continue;
        }
        // Filter $V_ prefixed types (string-exported type helpers)
        if item.label.starts_with("$V_") {
            continue;
        }
        // Skip if already seen (from verter or a previous TSGO item)
        if !seen_labels.insert(item.label.clone()) {
            continue;
        }

        let edit_range =
            if let (Some(start), Some(end)) = (item.edit_range_start, item.edit_range_end) {
                tsx_range_to_vue_range(start, end, tsx_line_index, mapper, vue_line_index)
            } else {
                None
            };

        let text_edit = edit_range.map(|range| {
            CompletionTextEdit::Edit(TextEdit {
                range,
                new_text: item.insert_text.clone().unwrap_or(item.label.clone()),
            })
        });

        result.push(CompletionItem {
            label: item.label,
            kind: item.kind.map(convert_completion_kind),
            detail: item.detail,
            documentation: item.documentation.map(|d| {
                Documentation::MarkupContent(MarkupContent {
                    kind: MarkupKind::Markdown,
                    value: d,
                })
            }),
            sort_text: item.sort_text,
            text_edit,
            ..Default::default()
        });
    }

    (result, is_incomplete)
}

fn convert_completion_kind(kind: CompletionKind) -> CompletionItemKind {
    match kind {
        CompletionKind::Function => CompletionItemKind::FUNCTION,
        CompletionKind::Variable => CompletionItemKind::VARIABLE,
        CompletionKind::Property => CompletionItemKind::PROPERTY,
        CompletionKind::Class => CompletionItemKind::CLASS,
        CompletionKind::Interface => CompletionItemKind::INTERFACE,
        CompletionKind::Module => CompletionItemKind::MODULE,
        CompletionKind::Keyword => CompletionItemKind::KEYWORD,
        CompletionKind::Snippet => CompletionItemKind::SNIPPET,
        CompletionKind::Text => CompletionItemKind::TEXT,
        CompletionKind::Method => CompletionItemKind::METHOD,
        CompletionKind::Field => CompletionItemKind::FIELD,
        CompletionKind::Enum => CompletionItemKind::ENUM,
        CompletionKind::EnumMember => CompletionItemKind::ENUM_MEMBER,
        CompletionKind::Constant => CompletionItemKind::CONSTANT,
        CompletionKind::TypeParameter => CompletionItemKind::TYPE_PARAMETER,
    }
}

// ── Diagnostics merge ──────────────────────────────────────────────

/// Merge verter diagnostics with TypeProvider diagnostics.
///
/// Strategy:
/// - Verter diagnostics are already in Vue positions
/// - TypeProvider diagnostics are in TSX positions; map back to Vue
/// - Filter out diagnostics that map to unmapped regions (generated code)
pub fn merge_diagnostics(
    verter_diags: Vec<Diagnostic>,
    type_diags: Vec<TypeDiagnostic>,
    tsx_line_index: &LineIndex,
    mapper: &PositionMapper,
    vue_line_index: &LineIndex,
) -> Vec<Diagnostic> {
    let mut result = verter_diags;
    let mut dropped = 0u32;

    for diag in &type_diags {
        let range =
            tsx_range_to_vue_range(diag.start, diag.end, tsx_line_index, mapper, vue_line_index);

        if let Some(range) = range {
            result.push(Diagnostic {
                range,
                severity: Some(convert_severity(diag.severity)),
                code: diag.code.clone().map(NumberOrString::String),
                source: Some("ts".to_string()),
                message: diag.message.clone(),
                ..Default::default()
            });
        } else {
            dropped += 1;
            tracing::debug!(
                "merge_diagnostics: dropped TSGO diagnostic (unmapped range) — {:?} at offsets {}..{}",
                diag.message,
                diag.start,
                diag.end,
            );
        }
    }

    if dropped > 0 {
        tracing::debug!(
            "merge_diagnostics: {dropped}/{} TSGO diagnostics dropped (unmapped ranges)",
            type_diags.len()
        );
    }

    result
}

fn convert_severity(sev: TypeDiagnosticSeverity) -> DiagnosticSeverity {
    match sev {
        TypeDiagnosticSeverity::Error => DiagnosticSeverity::ERROR,
        TypeDiagnosticSeverity::Warning => DiagnosticSeverity::WARNING,
        TypeDiagnosticSeverity::Info => DiagnosticSeverity::INFORMATION,
        TypeDiagnosticSeverity::Hint => DiagnosticSeverity::HINT,
    }
}

// ── Definition merge ───────────────────────────────────────────────

/// Merge verter definition with TypeProvider definitions.
///
/// Strategy:
/// - If verter provides a definition, use it (it's already precise for in-file navigation)
/// - TypeProvider definitions are used for cross-file navigation (import targets, etc.)
/// - Map TypeProvider locations back to Vue positions where applicable
pub fn merge_definitions(
    verter_def: Option<GotoDefinitionResponse>,
    type_defs: Vec<TypeLocation>,
    tsx_line_index: &LineIndex,
    mapper: &PositionMapper,
    vue_line_index: &LineIndex,
) -> Option<GotoDefinitionResponse> {
    // If verter provides a definition, prefer it when:
    // - TSGO returned nothing, or
    // - verter resolved cross-file (TSGO often returns *.vue shim declarations)
    if let Some(ref vd) = verter_def {
        let is_cross_file = matches!(vd, GotoDefinitionResponse::Scalar(loc) if loc.uri.as_str() != crate::features::definition::SAME_FILE_URI);
        if type_defs.is_empty() || is_cross_file {
            return verter_def;
        }
    }

    // If TypeProvider provides definitions, convert them
    if !type_defs.is_empty() {
        let locations: Vec<Location> = type_defs
            .into_iter()
            .filter_map(|loc| {
                // TypeProvider returns paths; convert to URIs
                // For .vue files, the path is the TSX path; strip the .tsx suffix
                let file_path = if loc.path.ends_with(".vue.tsx") {
                    loc.path.trim_end_matches(".tsx").to_string()
                } else {
                    loc.path.clone()
                };
                let uri = path_to_uri(&file_path)?;
                // Map TSX byte offsets back to Vue positions for .vue targets
                let range = if loc.path.ends_with(".vue.tsx") {
                    tsx_range_to_vue_range(
                        loc.start,
                        loc.end,
                        tsx_line_index,
                        mapper,
                        vue_line_index,
                    )
                    .unwrap_or_default()
                } else {
                    Range::default()
                };
                Some(Location { uri, range })
            })
            .collect();

        if locations.is_empty() {
            return verter_def;
        }

        return Some(if locations.len() == 1 {
            GotoDefinitionResponse::Scalar(locations.into_iter().next().unwrap())
        } else {
            GotoDefinitionResponse::Array(locations)
        });
    }

    verter_def
}

/// Convert a file path to a `file://` URI.
///
/// Handles both Windows (`C:/Users/...`) and Unix (`/home/user/...`) paths.
/// Also available as `file_path_to_uri` for use outside this module.
pub fn file_path_to_uri(path: &str) -> Option<Uri> {
    path_to_uri(path)
}

/// Convert a file path to a `file://` URI (internal).
fn path_to_uri(path: &str) -> Option<Uri> {
    // Normalize path separators
    let normalized = path.replace('\\', "/");

    let uri_str = if normalized.starts_with('/') {
        format!("file://{normalized}")
    } else {
        // Windows path (e.g., "C:/Users/...")
        format!("file:///{normalized}")
    };

    uri_str.parse().ok()
}

// ── References merge ────────────────────────────────────────────────

/// Merge verter references with TypeProvider references.
///
/// Strategy:
/// - Combine verter in-file refs with TypeProvider cross-file refs
/// - Map TSX locations back to Vue for same-file targets
/// - Deduplicate by (uri, range.start)
pub fn merge_references(
    verter_refs: Option<Vec<Location>>,
    type_refs: Vec<TypeLocation>,
    tsx_line_index: &LineIndex,
    mapper: &PositionMapper,
    vue_line_index: &LineIndex,
) -> Option<Vec<Location>> {
    let mut result = verter_refs.unwrap_or_default();

    for loc in type_refs {
        // For .vue.tsx targets, map back to .vue positions
        if loc.path.ends_with(".vue.tsx") {
            if let Some(range) =
                tsx_range_to_vue_range(loc.start, loc.end, tsx_line_index, mapper, vue_line_index)
            {
                let vue_path = loc.path.trim_end_matches(".tsx");
                if let Some(uri) = path_to_uri(vue_path) {
                    // Deduplicate: skip if we already have a ref at this position
                    let dup = result
                        .iter()
                        .any(|r| r.uri == uri && r.range.start == range.start);
                    if !dup {
                        result.push(Location { uri, range });
                    }
                }
            }
        } else {
            // Cross-file .ts/.js targets: pass through
            if let Some(uri) = path_to_uri(&loc.path) {
                result.push(Location {
                    uri,
                    range: Range::default(), // offset mapping for external files requires their content
                });
            }
        }
    }

    if result.is_empty() {
        None
    } else {
        Some(result)
    }
}

// ── Rename merge ────────────────────────────────────────────────────

/// Merge verter rename edits with TypeProvider rename locations.
///
/// Strategy:
/// - Start with verter's same-file WorkspaceEdit
/// - Add TypeProvider's cross-file rename locations as additional TextEdits
/// - Map TSX ranges back to Vue for .vue targets
#[allow(clippy::mutable_key_type)] // Uri has interior mutability but is used as key by tower-lsp API
pub fn merge_rename_locations(
    verter_edit: Option<WorkspaceEdit>,
    type_locations: Vec<RenameLocation>,
    new_name: &str,
    tsx_line_index: &LineIndex,
    mapper: &PositionMapper,
    vue_line_index: &LineIndex,
) -> Option<WorkspaceEdit> {
    let mut edit = verter_edit.unwrap_or_else(|| WorkspaceEdit {
        changes: Some(std::collections::HashMap::new()),
        ..Default::default()
    });

    let changes = edit
        .changes
        .get_or_insert_with(std::collections::HashMap::new);

    for loc in type_locations {
        if loc.path.ends_with(".vue.tsx") {
            if let Some(range) =
                tsx_range_to_vue_range(loc.start, loc.end, tsx_line_index, mapper, vue_line_index)
            {
                let vue_path = loc.path.trim_end_matches(".tsx");
                if let Some(uri) = path_to_uri(vue_path) {
                    let edits = changes.entry(uri).or_default();
                    let dup = edits.iter().any(|e| e.range.start == range.start);
                    if !dup {
                        edits.push(TextEdit {
                            range,
                            new_text: new_name.to_string(),
                        });
                    }
                }
            }
        } else if let Some(uri) = path_to_uri(&loc.path) {
            let edits = changes.entry(uri).or_default();
            edits.push(TextEdit {
                range: Range::default(),
                new_text: new_name.to_string(),
            });
        }
    }

    // Return None if no edits
    if changes.is_empty() {
        None
    } else {
        Some(edit)
    }
}

// ── Document highlights merge ───────────────────────────────────────

/// Merge verter document highlights with TypeProvider highlights.
///
/// Strategy:
/// - Prefer verter's Read/Write distinction
/// - Supplement with TypeProvider highlights that map back to Vue
/// - Deduplicate by range start
pub fn merge_document_highlights(
    verter_highlights: Option<Vec<DocumentHighlight>>,
    type_highlights: Vec<TypeDocumentHighlight>,
    tsx_line_index: &LineIndex,
    mapper: &PositionMapper,
    vue_line_index: &LineIndex,
) -> Option<Vec<DocumentHighlight>> {
    let mut result = verter_highlights.unwrap_or_default();

    for th in type_highlights {
        if let Some(range) =
            tsx_range_to_vue_range(th.start, th.end, tsx_line_index, mapper, vue_line_index)
        {
            let dup = result.iter().any(|h| h.range.start == range.start);
            if !dup {
                result.push(DocumentHighlight {
                    range,
                    kind: Some(match th.kind {
                        TypeDocumentHighlightKind::Read => DocumentHighlightKind::READ,
                        TypeDocumentHighlightKind::Write => DocumentHighlightKind::WRITE,
                        TypeDocumentHighlightKind::Text => DocumentHighlightKind::TEXT,
                    }),
                });
            }
        }
    }

    if result.is_empty() {
        None
    } else {
        Some(result)
    }
}

// ── Signature help merge ────────────────────────────────────────────

/// Convert TypeProvider signature help to LSP SignatureHelp.
///
/// No verter equivalent exists; this is a direct conversion from protocol types.
pub fn merge_signature_help(
    type_sig: Option<protocol::SignatureHelp>,
) -> Option<tower_lsp_server::lsp_types::SignatureHelp> {
    let sig = type_sig?;
    Some(tower_lsp_server::lsp_types::SignatureHelp {
        signatures: sig
            .signatures
            .into_iter()
            .map(|s| SignatureInformation {
                label: s.label,
                documentation: s.documentation.map(|d| {
                    Documentation::MarkupContent(MarkupContent {
                        kind: MarkupKind::Markdown,
                        value: d,
                    })
                }),
                parameters: Some(
                    s.parameters
                        .into_iter()
                        .map(|p| ParameterInformation {
                            label: ParameterLabel::Simple(p.label),
                            documentation: p.documentation.map(|d| {
                                Documentation::MarkupContent(MarkupContent {
                                    kind: MarkupKind::Markdown,
                                    value: d,
                                })
                            }),
                        })
                        .collect(),
                ),
                active_parameter: None,
            })
            .collect(),
        active_signature: sig.active_signature,
        active_parameter: sig.active_parameter,
    })
}

// ── Code actions merge ──────────────────────────────────────────────

/// Convert TypeProvider code actions to LSP CodeActions.
///
/// Maps edits from TSX positions back to Vue positions.
/// Filters out actions whose edits don't map back to Vue.
#[allow(clippy::mutable_key_type)] // Uri has interior mutability but is used as key by tower-lsp API
pub fn merge_code_actions(
    type_actions: Vec<TypeCodeAction>,
    tsx_line_index: &LineIndex,
    mapper: &PositionMapper,
    vue_line_index: &LineIndex,
) -> Vec<CodeActionOrCommand> {
    type_actions
        .into_iter()
        .filter_map(|action| {
            let mut changes: std::collections::HashMap<Uri, Vec<TextEdit>> =
                std::collections::HashMap::new();

            for edit in action.edits {
                if edit.path.ends_with(".vue.tsx") {
                    if let Some(range) = tsx_range_to_vue_range(
                        edit.start,
                        edit.end,
                        tsx_line_index,
                        mapper,
                        vue_line_index,
                    ) {
                        let vue_path = edit.path.trim_end_matches(".tsx");
                        if let Some(uri) = path_to_uri(vue_path) {
                            changes.entry(uri).or_default().push(TextEdit {
                                range,
                                new_text: edit.new_text,
                            });
                        }
                    }
                } else if let Some(uri) = path_to_uri(&edit.path) {
                    changes.entry(uri).or_default().push(TextEdit {
                        range: Range::default(),
                        new_text: edit.new_text,
                    });
                }
            }

            if changes.is_empty() {
                return None;
            }

            Some(CodeActionOrCommand::CodeAction(CodeAction {
                title: action.title,
                kind: action.kind.map(CodeActionKind::from),
                edit: Some(WorkspaceEdit {
                    changes: Some(changes),
                    ..Default::default()
                }),
                ..Default::default()
            }))
        })
        .collect()
}

// ── Semantic tokens merge ───────────────────────────────────────────

/// Convert TypeProvider semantic tokens to LSP semantic tokens.
///
/// Maps each token's TSX start offset to Vue position.
/// Re-encodes as delta-encoded sequence. Filters tokens in unmapped regions.
pub fn merge_semantic_tokens(
    type_tokens: Vec<protocol::SemanticToken>,
    tsx_line_index: &LineIndex,
    mapper: &PositionMapper,
    vue_line_index: &LineIndex,
) -> Vec<tower_lsp_server::lsp_types::SemanticToken> {
    // Map each token from TSX to Vue positions, filtering unmapped ones
    let mut mapped: Vec<(u32, u32, u32, u32, u32)> = Vec::new(); // (line, char, length, type, mods)

    for token in type_tokens {
        let start_pos = tsx_line_index.offset_to_position(token.start);

        if let Some(start_lsp) = start_pos {
            if let Some(vs) = mapper.tsx_to_vue(start_lsp.line, start_lsp.character) {
                // Validate start offset is within the Vue source
                let start_lsp_pos = Position {
                    line: vs.line,
                    character: vs.column,
                };
                if vue_line_index.position_to_offset(&start_lsp_pos).is_some() {
                    // Preserve the original token length — source map lookup for the
                    // end position would snap to the nearest token, collapsing length to 0.
                    mapped.push((
                        vs.line,
                        vs.column,
                        token.length,
                        token.token_type,
                        token.token_modifiers,
                    ));
                }
            }
        }
    }

    // Sort by (line, character) for correct delta encoding
    mapped.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));

    // Delta-encode
    let mut result = Vec::with_capacity(mapped.len());
    let mut prev_line = 0u32;
    let mut prev_start = 0u32;

    for (line, character, length, token_type, token_modifiers) in mapped {
        let delta_line = line - prev_line;
        let delta_start = if delta_line > 0 {
            character
        } else {
            character - prev_start
        };

        result.push(tower_lsp_server::lsp_types::SemanticToken {
            delta_line,
            delta_start,
            length,
            token_type,
            token_modifiers_bitset: token_modifiers,
        });

        prev_line = line;
        prev_start = character;
    }

    result
}

// ── Inlay hints merge ─────────────────────────────────────────────

/// Map TypeProvider inlay hints from TSX positions back to Vue positions.
///
/// Each hint position (byte offset in TSX) is mapped through the sourcemap
/// back to the Vue source. Hints that fall in generated code (no mapping)
/// are filtered out.
pub fn merge_inlay_hints(
    type_hints: Vec<InlayHint>,
    tsx_line_index: &LineIndex,
    mapper: &PositionMapper,
    vue_line_index: &LineIndex,
) -> Vec<tower_lsp_server::lsp_types::InlayHint> {
    let mut result = Vec::with_capacity(type_hints.len());

    for hint in type_hints {
        // Convert TSX byte offset → TSX line/col
        let Some(tsx_pos) = tsx_line_index.offset_to_position(hint.position) else {
            continue;
        };

        // Map TSX line/col → Vue line/col via sourcemap
        let Some(vue_mapped) = mapper.tsx_to_vue(tsx_pos.line, tsx_pos.character) else {
            continue;
        };

        let vue_pos = Position {
            line: vue_mapped.line,
            character: vue_mapped.column,
        };

        // Validate the Vue position is within bounds
        if vue_line_index.position_to_offset(&vue_pos).is_none() {
            continue;
        }

        let kind = hint.kind.map(|k| match k {
            InlayHintKind::Type => tower_lsp_server::lsp_types::InlayHintKind::TYPE,
            InlayHintKind::Parameter => tower_lsp_server::lsp_types::InlayHintKind::PARAMETER,
        });

        result.push(tower_lsp_server::lsp_types::InlayHint {
            position: vue_pos,
            label: tower_lsp_server::lsp_types::InlayHintLabel::String(hint.label),
            kind,
            text_edits: None,
            tooltip: None,
            padding_left: hint.padding_left,
            padding_right: hint.padding_right,
            data: None,
        });
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Position mapping tests ─────────────────────────────────────

    fn make_mapper_and_indexes() -> (PositionMapper, LineIndex, LineIndex) {
        // Vue source (line 0-1: template, line 3-4: script)
        let vue_source = "<template>\n  <div>{{ msg }}</div>\n</template>\n\n<script setup>\nconst msg = \"hello\";\n</script>";
        // TSX source (script at line 0)
        let tsx_source = "const msg = \"hello\";\n";

        // Source map: TSX line 0 col 0 → Vue line 5 col 0
        let mut builder = oxc_sourcemap::SourceMapBuilder::default();
        let source_id = builder.set_source_and_content("App.vue", vue_source);
        builder.add_token(0, 0, 5, 0, Some(source_id), None);
        builder.add_token(0, 6, 5, 6, Some(source_id), None);
        builder.add_token(0, 10, 5, 10, Some(source_id), None);
        let json = builder.into_sourcemap().to_json_string();

        let mapper = PositionMapper::from_json(&json).unwrap();
        let vue_li = LineIndex::new_utf16(vue_source);
        let tsx_li = LineIndex::new_utf16(tsx_source);

        (mapper, vue_li, tsx_li)
    }

    /// @ai-generated — Vue position maps to correct TSX byte offset
    #[test]
    fn vue_position_maps_to_tsx_offset() {
        let (mapper, vue_li, tsx_li) = make_mapper_and_indexes();

        // Vue line 5, col 6 ("msg") → TSX line 0, col 6 → byte offset 6
        let offset = vue_position_to_tsx_offset(
            &Position {
                line: 5,
                character: 6,
            },
            &vue_li,
            &mapper,
            &tsx_li,
        );
        assert_eq!(offset, Some(6));
    }

    /// @ai-generated — Unmappable Vue position returns None
    #[test]
    fn unmappable_vue_position_returns_none() {
        let (mapper, vue_li, tsx_li) = make_mapper_and_indexes();

        // Line 0 is in the template, not mapped in our source map
        let offset = vue_position_to_tsx_offset(
            &Position {
                line: 0,
                character: 0,
            },
            &vue_li,
            &mapper,
            &tsx_li,
        );
        assert!(offset.is_none());
    }

    // ── Hover merge tests ──────────────────────────────────────────

    fn make_verter_hover(text: &str) -> Hover {
        Hover {
            contents: HoverContents::Markup(MarkupContent {
                kind: MarkupKind::Markdown,
                value: text.to_string(),
            }),
            range: None,
        }
    }

    /// @ai-generated — Both verter and type hover are merged
    #[test]
    fn merge_hover_both_present() {
        let (mapper, vue_li, tsx_li) = make_mapper_and_indexes();
        let verter = make_verter_hover("**msg** (SetupConst)");
        let type_hover = HoverInfo {
            contents: "const msg: string".to_string(),
            range_start: None,
            range_end: None,
        };

        let result = merge_hover(Some(verter), Some(type_hover), &mapper, &tsx_li, &vue_li);
        let text = extract_hover_text(&result.unwrap());
        assert!(text.contains("const msg: string"));
        assert!(text.contains("SetupConst"));
    }

    /// @ai-generated — Only verter hover present
    #[test]
    fn merge_hover_verter_only() {
        let (mapper, vue_li, tsx_li) = make_mapper_and_indexes();
        let verter = make_verter_hover("**msg** (SetupConst)");

        let result = merge_hover(Some(verter), None, &mapper, &tsx_li, &vue_li);
        assert!(result.is_some());
        let text = extract_hover_text(&result.unwrap());
        assert!(text.contains("SetupConst"));
    }

    /// @ai-generated — Only type hover present
    #[test]
    fn merge_hover_type_only() {
        let (mapper, vue_li, tsx_li) = make_mapper_and_indexes();
        let type_hover = HoverInfo {
            contents: "const msg: string".to_string(),
            range_start: None,
            range_end: None,
        };

        let result = merge_hover(None, Some(type_hover), &mapper, &tsx_li, &vue_li);
        assert!(result.is_some());
        let text = extract_hover_text(&result.unwrap());
        assert!(text.contains("const msg: string"));
    }

    /// @ai-generated — Neither hover present returns None
    #[test]
    fn merge_hover_neither() {
        let (mapper, vue_li, tsx_li) = make_mapper_and_indexes();
        let result = merge_hover(None, None, &mapper, &tsx_li, &vue_li);
        assert!(result.is_none());
    }

    // ── Completion merge tests ─────────────────────────────────────

    fn make_verter_completion(label: &str) -> CompletionItem {
        CompletionItem {
            label: label.to_string(),
            kind: Some(CompletionItemKind::VARIABLE),
            ..Default::default()
        }
    }

    fn make_type_completion(label: &str) -> Completion {
        Completion {
            label: label.to_string(),
            kind: Some(CompletionKind::Variable),
            detail: None,
            documentation: None,
            edit_range_start: None,
            edit_range_end: None,
            insert_text: None,
            sort_text: None,
        }
    }

    /// @ai-generated — TypeProvider completions are added alongside verter completions
    #[test]
    fn merge_completions_combines_both() {
        let (mapper, vue_li, tsx_li) = make_mapper_and_indexes();
        let verter = vec![make_verter_completion("msg")];
        let type_result = CompletionResult {
            items: vec![make_type_completion("count"), make_type_completion("name")],
            is_incomplete: false,
        };

        let (result, is_incomplete) =
            merge_completions(verter, type_result, &mapper, &tsx_li, &vue_li);
        assert_eq!(result.len(), 3);
        assert!(!is_incomplete);
        let labels: Vec<&str> = result.iter().map(|i| i.label.as_str()).collect();
        assert!(labels.contains(&"msg"));
        assert!(labels.contains(&"count"));
        assert!(labels.contains(&"name"));
    }

    /// @ai-generated — Duplicate labels are deduplicated (verter wins)
    #[test]
    fn merge_completions_deduplicates() {
        let (mapper, vue_li, tsx_li) = make_mapper_and_indexes();
        let verter = vec![make_verter_completion("msg")];
        let type_result = CompletionResult {
            items: vec![make_type_completion("msg")], // duplicate
            is_incomplete: false,
        };

        let (result, _) = merge_completions(verter, type_result, &mapper, &tsx_li, &vue_li);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].label, "msg");
    }

    /// @ai-generated — ___VERTER___ prefixed completions are filtered
    #[test]
    fn merge_completions_filters_verter_internal() {
        let (mapper, vue_li, tsx_li) = make_mapper_and_indexes();
        let verter = vec![];
        let type_result = CompletionResult {
            items: vec![
                make_type_completion("msg"),
                make_type_completion("___VERTER___hidden"),
            ],
            is_incomplete: false,
        };

        let (result, _) = merge_completions(verter, type_result, &mapper, &tsx_li, &vue_li);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].label, "msg");
    }

    /// @ai-generated — is_incomplete flag is propagated from TypeProvider result
    #[test]
    fn merge_completions_propagates_is_incomplete() {
        let (mapper, vue_li, tsx_li) = make_mapper_and_indexes();
        let verter = vec![make_verter_completion("msg")];
        let type_result = CompletionResult {
            items: vec![make_type_completion("count")],
            is_incomplete: true,
        };

        let (result, is_incomplete) =
            merge_completions(verter, type_result, &mapper, &tsx_li, &vue_li);
        assert_eq!(result.len(), 2);
        assert!(
            is_incomplete,
            "is_incomplete should be propagated from TSGO"
        );
    }

    /// @ai-generated — $V_ prefixed type helpers are filtered
    #[test]
    fn merge_completions_filters_dollar_v_prefix() {
        let (mapper, vue_li, tsx_li) = make_mapper_and_indexes();
        let verter = vec![];
        let type_result = CompletionResult {
            items: vec![
                make_type_completion("msg"),
                make_type_completion("$V_EmitsToProps"),
            ],
            is_incomplete: false,
        };

        let (result, _) = merge_completions(verter, type_result, &mapper, &tsx_li, &vue_li);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].label, "msg");
    }

    /// @ai-generated — TSGO-internal duplicates are deduplicated
    #[test]
    fn merge_completions_deduplicates_tsgo_internal() {
        let (mapper, vue_li, tsx_li) = make_mapper_and_indexes();
        let verter = vec![make_verter_completion("msg")];
        let type_result = CompletionResult {
            items: vec![
                make_type_completion("onMounted"), // local binding
                make_type_completion("onMounted"), // auto-import suggestion (same label)
            ],
            is_incomplete: false,
        };

        let (result, _) = merge_completions(verter, type_result, &mapper, &tsx_li, &vue_li);
        let on_mounted_count = result.iter().filter(|i| i.label == "onMounted").count();
        assert_eq!(
            on_mounted_count, 1,
            "TSGO-internal duplicates should be deduplicated"
        );
        assert_eq!(result.len(), 2); // msg + onMounted
    }

    /// @ai-generated — Labels present in both verter and TSGO are deduplicated (verter wins)
    #[test]
    fn merge_completions_deduplicates_across_all_sources() {
        let (mapper, vue_li, tsx_li) = make_mapper_and_indexes();
        let verter = vec![make_verter_completion("onMounted")];
        let type_result = CompletionResult {
            items: vec![
                make_type_completion("onMounted"), // TSGO local
                make_type_completion("onMounted"), // TSGO auto-import
                make_type_completion("ref"),       // unique
            ],
            is_incomplete: false,
        };

        let (result, _) = merge_completions(verter, type_result, &mapper, &tsx_li, &vue_li);
        let on_mounted_count = result.iter().filter(|i| i.label == "onMounted").count();
        assert_eq!(
            on_mounted_count, 1,
            "onMounted should appear exactly once (from verter)"
        );
        assert_eq!(result.len(), 2); // onMounted + ref
    }

    // ── Diagnostics merge tests ────────────────────────────────────

    fn make_verter_diagnostic(msg: &str) -> Diagnostic {
        Diagnostic {
            range: Range::default(),
            severity: Some(DiagnosticSeverity::ERROR),
            source: Some("verter".to_string()),
            message: msg.to_string(),
            ..Default::default()
        }
    }

    /// @ai-generated — Type diagnostics are mapped and added to verter diagnostics
    #[test]
    fn merge_diagnostics_combines_both() {
        let (mapper, vue_li, tsx_li) = make_mapper_and_indexes();
        let verter = vec![make_verter_diagnostic("parse error")];
        let types = vec![TypeDiagnostic {
            message: "Type 'number' is not assignable to type 'string'".to_string(),
            severity: TypeDiagnosticSeverity::Error,
            start: 6, // TSX offset for "msg"
            end: 9,
            code: Some("2322".to_string()),
        }];

        let result = merge_diagnostics(verter, types, &tsx_li, &mapper, &vue_li);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].source.as_deref(), Some("verter"));
        assert_eq!(result[1].source.as_deref(), Some("ts"));
        assert!(result[1].message.contains("not assignable"));
    }

    /// @ai-generated — Type diagnostics in unmapped regions are filtered out
    #[test]
    fn merge_diagnostics_filters_unmapped() {
        let (mapper, vue_li, tsx_li) = make_mapper_and_indexes();
        let verter = vec![];
        // Offset 100 is beyond the TSX source
        let types = vec![TypeDiagnostic {
            message: "error in generated code".to_string(),
            severity: TypeDiagnosticSeverity::Error,
            start: 100,
            end: 110,
            code: None,
        }];

        let result = merge_diagnostics(verter, types, &tsx_li, &mapper, &vue_li);
        assert!(result.is_empty(), "unmapped diagnostics should be filtered");
    }

    // ── Definition merge tests ─────────────────────────────────────

    /// @ai-generated — Verter definition is preferred when no type definitions
    #[test]
    fn merge_definitions_verter_only() {
        let (mapper, vue_li, tsx_li) = make_mapper_and_indexes();
        let verter = Some(GotoDefinitionResponse::Scalar(Location {
            uri: "file:///test.vue".parse().unwrap(),
            range: Range::default(),
        }));

        let result = merge_definitions(verter, vec![], &tsx_li, &mapper, &vue_li);
        assert!(result.is_some());
    }

    /// @ai-generated — Type definitions used when verter has none
    #[test]
    fn merge_definitions_type_only() {
        let (mapper, vue_li, tsx_li) = make_mapper_and_indexes();
        let types = vec![TypeLocation {
            path: "/project/utils.ts".to_string(),
            start: 0,
            end: 10,
        }];

        let result = merge_definitions(None, types, &tsx_li, &mapper, &vue_li);
        assert!(result.is_some());
    }

    /// @ai-generated — Neither source returns None
    #[test]
    fn merge_definitions_neither() {
        let (mapper, vue_li, tsx_li) = make_mapper_and_indexes();
        let result = merge_definitions(None, vec![], &tsx_li, &mapper, &vue_li);
        assert!(result.is_none());
    }

    // ── path_to_uri tests ──────────────────────────────────────────

    /// @ai-generated — Unix path converted correctly
    #[test]
    fn path_to_uri_unix() {
        let uri = path_to_uri("/home/user/project/App.vue").unwrap();
        assert_eq!(uri.as_str(), "file:///home/user/project/App.vue");
    }

    /// @ai-generated — Windows path converted correctly
    #[test]
    fn path_to_uri_windows() {
        let uri = path_to_uri("C:/Users/dev/project/App.vue").unwrap();
        assert_eq!(uri.as_str(), "file:///C:/Users/dev/project/App.vue");
    }

    // ── References merge tests ────────────────────────────────────────

    /// @ai-generated — TypeProvider references are merged with verter refs
    #[test]
    fn merge_references_both_present() {
        let (mapper, vue_li, tsx_li) = make_mapper_and_indexes();
        let verter = Some(vec![Location {
            uri: "file:///test.vue".parse().unwrap(),
            range: Range::default(),
        }]);
        let type_refs = vec![TypeLocation {
            path: "/project/utils.ts".to_string(),
            start: 0,
            end: 10,
        }];

        let result = merge_references(verter, type_refs, &tsx_li, &mapper, &vue_li);
        assert!(result.is_some());
        assert_eq!(result.unwrap().len(), 2);
    }

    /// @ai-generated — Empty refs from both returns None
    #[test]
    fn merge_references_neither() {
        let (mapper, vue_li, tsx_li) = make_mapper_and_indexes();
        let result = merge_references(None, vec![], &tsx_li, &mapper, &vue_li);
        assert!(result.is_none());
    }

    /// @ai-generated — Verter-only refs returned as-is
    #[test]
    fn merge_references_verter_only() {
        let (mapper, vue_li, tsx_li) = make_mapper_and_indexes();
        let verter = Some(vec![Location {
            uri: "file:///test.vue".parse().unwrap(),
            range: Range::default(),
        }]);

        let result = merge_references(verter, vec![], &tsx_li, &mapper, &vue_li);
        assert!(result.is_some());
        assert_eq!(result.unwrap().len(), 1);
    }

    // ── Document highlights merge tests ───────────────────────────────

    /// @ai-generated — Type highlights mapped and merged with verter highlights
    #[test]
    fn merge_highlights_both_present() {
        let (mapper, vue_li, tsx_li) = make_mapper_and_indexes();
        let verter = Some(vec![DocumentHighlight {
            range: Range {
                start: Position {
                    line: 5,
                    character: 6,
                },
                end: Position {
                    line: 5,
                    character: 9,
                },
            },
            kind: Some(DocumentHighlightKind::READ),
        }]);
        // TSX offset 6-9 maps to Vue line 5, col 6-9
        let type_highlights = vec![TypeDocumentHighlight {
            start: 6,
            end: 9,
            kind: TypeDocumentHighlightKind::Write,
        }];

        let result = merge_document_highlights(verter, type_highlights, &tsx_li, &mapper, &vue_li);
        assert!(result.is_some());
        // Should be 1 (deduplicated since both point to line 5, col 6)
        assert_eq!(result.unwrap().len(), 1);
    }

    /// @ai-generated — Neither highlights returns None
    #[test]
    fn merge_highlights_neither() {
        let (mapper, vue_li, tsx_li) = make_mapper_and_indexes();
        let result = merge_document_highlights(None, vec![], &tsx_li, &mapper, &vue_li);
        assert!(result.is_none());
    }

    // ── Signature help merge tests ────────────────────────────────────

    /// @ai-generated — TypeProvider signature help is converted to LSP type
    #[test]
    fn merge_signature_help_present() {
        let sig = protocol::SignatureHelp {
            signatures: vec![protocol::SignatureInfo {
                label: "fn(x: number): void".to_string(),
                documentation: Some("A test function".to_string()),
                parameters: vec![protocol::ParameterInfo {
                    label: "x".to_string(),
                    documentation: Some("The number param".to_string()),
                }],
            }],
            active_signature: Some(0),
            active_parameter: Some(0),
        };

        let result = merge_signature_help(Some(sig));
        assert!(result.is_some());
        let help = result.unwrap();
        assert_eq!(help.signatures.len(), 1);
        assert_eq!(help.signatures[0].label, "fn(x: number): void");
        assert_eq!(help.active_signature, Some(0));
    }

    /// @ai-generated — None input returns None
    #[test]
    fn merge_signature_help_none() {
        assert!(merge_signature_help(None).is_none());
    }

    // ── Code actions merge tests ──────────────────────────────────────

    /// @ai-generated — Code actions with mappable edits are returned
    #[test]
    fn merge_code_actions_with_edits() {
        let (mapper, vue_li, tsx_li) = make_mapper_and_indexes();
        let actions = vec![TypeCodeAction {
            title: "Add missing import".to_string(),
            kind: Some("quickfix".to_string()),
            edits: vec![protocol::TypeCodeEdit {
                path: "/test.vue.tsx".to_string(),
                start: 0,
                end: 0,
                new_text: "import { ref } from 'vue';\n".to_string(),
            }],
        }];

        let result = merge_code_actions(actions, &tsx_li, &mapper, &vue_li);
        assert_eq!(result.len(), 1);
    }

    /// @ai-generated — Empty actions returns empty vec
    #[test]
    fn merge_code_actions_empty() {
        let (mapper, vue_li, tsx_li) = make_mapper_and_indexes();
        let result = merge_code_actions(vec![], &tsx_li, &mapper, &vue_li);
        assert!(result.is_empty());
    }

    // ── Semantic tokens merge tests ───────────────────────────────────

    /// @ai-generated — Semantic tokens mapped from TSX to Vue
    #[test]
    fn merge_semantic_tokens_basic() {
        let (mapper, vue_li, tsx_li) = make_mapper_and_indexes();
        // Token at TSX offset 6 (= "msg"), length 3
        let tokens = vec![protocol::SemanticToken {
            start: 6,
            length: 3,
            token_type: 8, // VARIABLE
            token_modifiers: 0,
        }];

        let result = merge_semantic_tokens(tokens, &tsx_li, &mapper, &vue_li);
        assert_eq!(result.len(), 1);
        // Should map to Vue line 5, col 6
        assert_eq!(result[0].length, 3);
        assert_eq!(result[0].token_type, 8);
    }

    /// @ai-generated — Empty tokens returns empty vec
    #[test]
    fn merge_semantic_tokens_empty() {
        let (mapper, vue_li, tsx_li) = make_mapper_and_indexes();
        let result = merge_semantic_tokens(vec![], &tsx_li, &mapper, &vue_li);
        assert!(result.is_empty());
    }

    // ── Rename merge tests ────────────────────────────────────────────

    /// @ai-generated — Verter-only rename returns as-is
    #[test]
    fn merge_rename_verter_only() {
        let (mapper, vue_li, tsx_li) = make_mapper_and_indexes();
        let verter = Some(WorkspaceEdit {
            changes: Some({
                let mut m = std::collections::HashMap::new();
                m.insert(
                    "file:///test.vue".parse().unwrap(),
                    vec![TextEdit {
                        range: Range::default(),
                        new_text: "newName".to_string(),
                    }],
                );
                m
            }),
            ..Default::default()
        });

        let result = merge_rename_locations(verter, vec![], "newName", &tsx_li, &mapper, &vue_li);
        assert!(result.is_some());
    }

    /// @ai-generated — Empty rename from both returns None
    #[test]
    fn merge_rename_neither() {
        let (mapper, vue_li, tsx_li) = make_mapper_and_indexes();
        let result = merge_rename_locations(None, vec![], "newName", &tsx_li, &mapper, &vue_li);
        assert!(result.is_none());
    }

    // ── Definition merge tests (Bug 2) ───────────────────────────────

    /// @ai-generated — merge_definitions maps .vue.tsx offsets to correct Vue positions
    ///
    /// This tests Bug 2: merge_definitions was returning Range::default() (0,0)-(0,0)
    /// for all .vue.tsx targets instead of mapping TSX byte offsets back to Vue positions.
    #[test]
    fn merge_definitions_maps_vue_tsx_to_vue_positions() {
        let (mapper, vue_li, tsx_li) = make_mapper_and_indexes();

        // Simulate TSGO returning a definition in App.vue.tsx
        // TSX offset 6..9 = "msg" (in "const msg = ...")
        // This should map back to Vue line 5, col 6..9
        let type_defs = vec![TypeLocation {
            path: "/home/user/App.vue.tsx".to_string(),
            start: 6,
            end: 9,
        }];

        let result = merge_definitions(None, type_defs, &tsx_li, &mapper, &vue_li);
        assert!(result.is_some(), "Expected definition response");

        match result.unwrap() {
            GotoDefinitionResponse::Scalar(loc) => {
                // URI should point to .vue (not .vue.tsx)
                assert!(
                    loc.uri.as_str().ends_with("App.vue"),
                    "URI should be .vue, got: {}",
                    loc.uri.as_str()
                );
                // Range should NOT be (0,0)-(0,0) — that's the bug
                assert_ne!(
                    loc.range,
                    Range::default(),
                    "Definition range should not be (0,0)-(0,0) — \
                     TSX offsets must be mapped to Vue positions"
                );
                // The start should be on Vue line 5 (where "const msg = ..." is)
                assert_eq!(
                    loc.range.start.line, 5,
                    "Expected Vue line 5 for 'msg', got line {}",
                    loc.range.start.line
                );
            }
            GotoDefinitionResponse::Array(locs) => {
                assert_eq!(locs.len(), 1);
                assert_ne!(
                    locs[0].range,
                    Range::default(),
                    "Definition range should not be (0,0)-(0,0)"
                );
            }
            _ => panic!("Unexpected definition response type"),
        }
    }

    /// @ai-generated — merge_definitions passes through non-.vue targets unchanged
    #[test]
    fn merge_definitions_non_vue_targets_unchanged() {
        let (mapper, vue_li, tsx_li) = make_mapper_and_indexes();

        let type_defs = vec![TypeLocation {
            path: "/home/user/utils.ts".to_string(),
            start: 0,
            end: 10,
        }];

        let result = merge_definitions(None, type_defs, &tsx_li, &mapper, &vue_li);
        assert!(result.is_some());
    }

    /// @ai-generated — merge_definitions prefers verter when type_defs is empty
    #[test]
    fn merge_definitions_verter_preferred_when_no_type_defs() {
        let (mapper, vue_li, tsx_li) = make_mapper_and_indexes();

        let verter_def = Some(GotoDefinitionResponse::Scalar(Location {
            uri: "file:///test.vue".parse().unwrap(),
            range: Range {
                start: Position {
                    line: 5,
                    character: 6,
                },
                end: Position {
                    line: 5,
                    character: 9,
                },
            },
        }));

        let result = merge_definitions(verter_def, vec![], &tsx_li, &mapper, &vue_li);
        assert!(result.is_some());
        match result.unwrap() {
            GotoDefinitionResponse::Scalar(loc) => {
                assert_eq!(loc.range.start.line, 5);
                assert_eq!(loc.range.start.character, 6);
            }
            _ => panic!("Expected scalar definition"),
        }
    }

    // ── Hover merge tests ──────────────────────────────────────────

    /// @ai-generated — strip_leading_code_block removes leading fenced block
    #[test]
    fn strip_leading_code_block_removes_fence() {
        let text = "```typescript\nconst count: number\n```\n*(reactive)*";
        assert_eq!(strip_leading_code_block(text), "*(reactive)*");
    }

    /// @ai-generated — strip_leading_code_block returns full text when no fence
    #[test]
    fn strip_leading_code_block_no_fence() {
        let text = "*(reactive)*\nInitialized via `ref()`";
        assert_eq!(strip_leading_code_block(text), text);
    }

    /// @ai-generated — merge_hover deduplicates code fences
    #[test]
    fn merge_hover_no_duplicate_fences() {
        let (mapper, _, tsx_li) = make_mapper_and_indexes();
        let vue_li = LineIndex::new_utf16("");

        let verter = Some(Hover {
            contents: HoverContents::Markup(MarkupContent {
                kind: MarkupKind::Markdown,
                value: "```typescript\nconst count\n```\n\n*(reactive)*".to_string(),
            }),
            range: None,
        });
        let tsgo = Some(HoverInfo {
            range_start: None,
            range_end: None,
            contents: "const count: Ref<number>".to_string(),
        });

        let result = merge_hover(verter, tsgo, &mapper, &tsx_li, &vue_li);
        assert!(result.is_some());

        let text = match result.unwrap().contents {
            HoverContents::Markup(m) => m.value,
            _ => panic!("expected markup"),
        };

        // Should have exactly one code fence from TSGO, plus verter context
        assert!(text.contains("const count: Ref<number>"));
        assert!(text.contains("*(reactive)*"));
        // Count code fence openings — should be exactly 1
        assert_eq!(text.matches("```typescript").count(), 1, "text: {text}");
    }

    /// @ai-generated — merge_hover with verter-only code block and TSGO replaces it cleanly
    #[test]
    fn merge_hover_verter_only_code_block() {
        let (mapper, _, tsx_li) = make_mapper_and_indexes();
        let vue_li = LineIndex::new_utf16("");

        let verter = Some(Hover {
            contents: HoverContents::Markup(MarkupContent {
                kind: MarkupKind::Markdown,
                value: "```typescript\nconst x\n```".to_string(),
            }),
            range: None,
        });
        let tsgo = Some(HoverInfo {
            range_start: None,
            range_end: None,
            contents: "const x: string".to_string(),
        });

        let result = merge_hover(verter, tsgo, &mapper, &tsx_li, &vue_li);
        assert!(result.is_some());

        let text = match result.unwrap().contents {
            HoverContents::Markup(m) => m.value,
            _ => panic!("expected markup"),
        };

        // Only TSGO type block, no "---" separator since verter had nothing extra
        assert_eq!(text, "```typescript\nconst x: string\n```");
    }
}
