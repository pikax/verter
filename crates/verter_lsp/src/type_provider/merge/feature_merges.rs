//! Per-feature merges that map TypeProvider results back to carrier-source
//! positions: references, rename, document highlights, signature help, code
//! actions, semantic tokens, and inlay hints.

use tower_lsp_server::ls_types::*;
use verter_span::TsPosition;

use crate::documents::line_index::LineIndex;
use crate::documents::provider_projection::ProviderPositionMapper;
use crate::type_provider::protocol::{
    self, InlayHint, InlayHintKind, RenameLocation, TypeCodeAction, TypeDocumentHighlight,
    TypeDocumentHighlightKind, TypeLocation,
};

use super::definition::{
    is_carrier_api_or_dts_path, is_carrier_ide_path, normalize_carrier_path, path_to_uri,
    resolve_carrier_tsx_range,
};
use super::position::{tsx_range_to_carrier_range, ExternalIdeResolver};

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
    mapper: &ProviderPositionMapper,
    carrier_line_index: &LineIndex,
    external_resolver: Option<ExternalIdeResolver<'_>>,
    carrier_source_exists: &dyn Fn(&str) -> bool,
) -> Option<Vec<Location>> {
    let mut result = verter_refs.unwrap_or_default();

    for loc in &type_refs {
        // For .vue.tsx/.vue.jsx targets, map back to .vue positions
        if is_carrier_ide_path(&loc.path) {
            let range = resolve_carrier_tsx_range(
                &loc.path,
                loc.start,
                loc.end,
                tsx_line_index,
                mapper,
                carrier_line_index,
                external_resolver,
            );
            let carrier_path = normalize_carrier_path(&loc.path, carrier_source_exists);
            if let Some(uri) = path_to_uri(carrier_path) {
                // Deduplicate: skip if we already have a ref at this position
                let dup = result
                    .iter()
                    .any(|r| r.uri == uri && r.range.start == range.start);
                if !dup {
                    result.push(Location { uri, range });
                }
            }
        } else if is_carrier_api_or_dts_path(&loc.path, carrier_source_exists) {
            // DTS declarations (.vue.d.ts or .vue.ts): strip suffix, use default range
            let carrier_path = normalize_carrier_path(&loc.path, carrier_source_exists);
            if let Some(uri) = path_to_uri(carrier_path) {
                result.push(Location {
                    uri,
                    range: Range::default(),
                });
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
#[allow(clippy::mutable_key_type, clippy::too_many_arguments)]
pub fn merge_rename_locations(
    verter_edit: Option<WorkspaceEdit>,
    type_locations: Vec<RenameLocation>,
    new_name: &str,
    tsx_line_index: &LineIndex,
    mapper: &ProviderPositionMapper,
    carrier_line_index: &LineIndex,
    external_resolver: Option<ExternalIdeResolver<'_>>,
    carrier_source_exists: &dyn Fn(&str) -> bool,
) -> Option<WorkspaceEdit> {
    let mut edit = verter_edit.unwrap_or_else(|| WorkspaceEdit {
        changes: Some(std::collections::HashMap::new()),
        ..Default::default()
    });

    let changes = edit
        .changes
        .get_or_insert_with(std::collections::HashMap::new);

    for loc in &type_locations {
        if is_carrier_ide_path(&loc.path) {
            let range = resolve_carrier_tsx_range(
                &loc.path,
                loc.start,
                loc.end,
                tsx_line_index,
                mapper,
                carrier_line_index,
                external_resolver,
            );
            let carrier_path = normalize_carrier_path(&loc.path, carrier_source_exists);
            if let Some(uri) = path_to_uri(carrier_path) {
                let edits = changes.entry(uri).or_default();
                let dup = edits.iter().any(|e| e.range.start == range.start);
                if !dup {
                    edits.push(TextEdit {
                        range,
                        new_text: new_name.to_string(),
                    });
                }
            }
        } else if is_carrier_api_or_dts_path(&loc.path, carrier_source_exists) {
            let carrier_path = normalize_carrier_path(&loc.path, carrier_source_exists);
            if let Some(uri) = path_to_uri(carrier_path) {
                let edits = changes.entry(uri).or_default();
                edits.push(TextEdit {
                    range: Range::default(),
                    new_text: new_name.to_string(),
                });
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
    mapper: &ProviderPositionMapper,
    carrier_line_index: &LineIndex,
) -> Option<Vec<DocumentHighlight>> {
    let mut result = verter_highlights.unwrap_or_default();

    for th in type_highlights {
        if let Some(range) =
            tsx_range_to_carrier_range(th.start, th.end, tsx_line_index, mapper, carrier_line_index)
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
) -> Option<tower_lsp_server::ls_types::SignatureHelp> {
    let sig = type_sig?;
    Some(tower_lsp_server::ls_types::SignatureHelp {
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
    mapper: &ProviderPositionMapper,
    carrier_line_index: &LineIndex,
    carrier_source_exists: &dyn Fn(&str) -> bool,
) -> Vec<CodeActionOrCommand> {
    type_actions
        .into_iter()
        .filter_map(|action| {
            let mut changes: std::collections::HashMap<Uri, Vec<TextEdit>> =
                std::collections::HashMap::new();

            for edit in action.edits {
                if is_carrier_ide_path(&edit.path) {
                    if let Some(range) = tsx_range_to_carrier_range(
                        edit.start,
                        edit.end,
                        tsx_line_index,
                        mapper,
                        carrier_line_index,
                    ) {
                        let carrier_path =
                            normalize_carrier_path(&edit.path, carrier_source_exists);
                        if let Some(uri) = path_to_uri(carrier_path) {
                            changes.entry(uri).or_default().push(TextEdit {
                                range,
                                new_text: edit.new_text,
                            });
                        }
                    }
                } else if is_carrier_api_or_dts_path(&edit.path, carrier_source_exists) {
                    let carrier_path = normalize_carrier_path(&edit.path, carrier_source_exists);
                    if let Some(uri) = path_to_uri(carrier_path) {
                        changes.entry(uri).or_default().push(TextEdit {
                            range: Range::default(),
                            new_text: edit.new_text,
                        });
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
    mapper: &ProviderPositionMapper,
    carrier_line_index: &LineIndex,
) -> Vec<tower_lsp_server::ls_types::SemanticToken> {
    // Map each token's whole half-open range `[start, start+length)` from TSX to Vue
    // through the strict run-compatible range API. A token is emitted ONLY when both
    // endpoints resolve inside compatible mapped runs; otherwise it is dropped. There is
    // NO fallback to the original TSX `token.length` when the end does not map — such a
    // fallback could emit a Vue token whose length straddled synthetic content.
    let mut mapped: Vec<(u32, u32, u32, u32, u32)> = Vec::new(); // (line, char, length, type, mods)

    for token in type_tokens {
        let Some(carrier_range) = tsx_range_to_carrier_range(
            token.start,
            token.start + token.length,
            tsx_line_index,
            mapper,
            carrier_line_index,
        ) else {
            continue;
        };

        // The strict range API only composes compatible runs, but a multi-line token would
        // produce a cross-line range; semantic tokens are single-line, so require it.
        if carrier_range.start.line != carrier_range.end.line
            || carrier_range.end.character < carrier_range.start.character
        {
            continue;
        }
        let carrier_length = carrier_range.end.character - carrier_range.start.character;

        // Skip zero-length tokens (collapsed by mapping)
        if carrier_length == 0 {
            continue;
        }

        mapped.push((
            carrier_range.start.line,
            carrier_range.start.character,
            carrier_length,
            token.token_type,
            token.token_modifiers,
        ));
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

        result.push(tower_lsp_server::ls_types::SemanticToken {
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
    mapper: &ProviderPositionMapper,
    carrier_line_index: &LineIndex,
) -> Vec<tower_lsp_server::ls_types::InlayHint> {
    let mut result = Vec::with_capacity(type_hints.len());

    for hint in type_hints {
        // Convert TSX byte offset → TSX line/col
        let Some(tsx_pos) = tsx_line_index.offset_to_position(hint.position) else {
            continue;
        };

        // Map TSX line/col → Vue line/col via sourcemap
        let Some(carrier_mapped) = mapper
            .tsx_to_carrier(TsPosition::new(tsx_pos.line, tsx_pos.character))
            .map(|m| m.pos)
        else {
            continue;
        };

        let carrier_pos = Position {
            line: carrier_mapped.line,
            character: carrier_mapped.character,
        };

        // Validate the Vue position is within bounds
        if carrier_line_index
            .position_to_offset(&carrier_pos)
            .is_none()
        {
            continue;
        }

        let kind = hint.kind.map(|k| match k {
            InlayHintKind::Type => tower_lsp_server::ls_types::InlayHintKind::TYPE,
            InlayHintKind::Parameter => tower_lsp_server::ls_types::InlayHintKind::PARAMETER,
        });

        result.push(tower_lsp_server::ls_types::InlayHint {
            position: carrier_pos,
            label: tower_lsp_server::ls_types::InlayHintLabel::String(hint.label),
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
