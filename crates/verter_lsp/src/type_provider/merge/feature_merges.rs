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
    is_carrier_api_path, is_carrier_ide_path, normalize_carrier_path, path_to_uri,
    resolve_carrier_ide_range_strict, resolve_external_target_range,
};
use super::position::{
    api_surface_range_to_carrier_range, tsx_range_to_carrier_range, ApiSurfaceResolution,
    ExternalApiResolver, ExternalIdeResolver, ExternalSourceReader,
};

// ── References merge ────────────────────────────────────────────────

/// Merge verter references with TypeProvider references.
///
/// Strategy:
/// - Combine verter in-file refs with TypeProvider cross-file refs.
/// - A carrier IDE target (`{carrier}.tsx`/`.jsx`) maps its byte offsets back to the carrier
///   source through the single shared strict mapper ([`resolve_carrier_ide_range_strict`]): the
///   in-context mapper for the queried file (same canonical path as `current_tsx_path`), the
///   external resolver for a foreign component. A FOREIGN carrier `.tsx` is DROPPED on a resolver
///   miss — never mapped through the current request's sourcemap (its offsets index a different
///   file, so the current map would point at an unrelated location).
/// - Every other target's `start`/`end` are REAL byte offsets into that file: read the target's
///   own source through the host VFS (`source_reader`) and convert the offsets to a line:col
///   `Range` in the client-negotiated `encoding`, exactly as the definition merge does.
/// - FAIL CLOSED: when the source / offsets cannot be resolved (or path normalization rewrote the
///   emitted URI to a carrier source no in-context sourcemap bridges), DROP the reference. Never
///   substitute `Range::default()`, which silently sends "Find All References" to line 0 of the
///   wrong file.
/// - Deduplicate by (uri, range.start).
#[expect(
    clippy::too_many_arguments,
    reason = "references merging needs the current TSX path, mapper, indexes, resolver, encoding, and VFS reader"
)]
pub fn merge_references(
    verter_refs: Option<Vec<Location>>,
    type_refs: Vec<TypeLocation>,
    current_tsx_path: &str,
    tsx_line_index: &LineIndex,
    mapper: &ProviderPositionMapper,
    carrier_line_index: &LineIndex,
    external_resolver: Option<ExternalIdeResolver<'_>>,
    carrier_source_exists: &dyn Fn(&str) -> bool,
    negotiated_encoding: PositionEncodingKind,
    source_reader: ExternalSourceReader<'_>,
) -> Option<Vec<Location>> {
    let mut result = verter_refs.unwrap_or_default();

    for loc in &type_refs {
        // For carrier IDE targets, map back to carrier-source positions through the single shared
        // strict mapper, split by canonical identity: the CURRENT request's TSX uses the in-context
        // mapper; a FOREIGN carrier `.tsx` requires its own context via the external resolver and is
        // DROPPED on a miss/failure. FAIL CLOSED — never fabricate a `Range::default()` (line 0),
        // and never map a foreign file's offsets through the current request's sourcemap.
        if is_carrier_ide_path(&loc.path) {
            let Some(range) = resolve_carrier_ide_range_strict(
                &loc.path,
                loc.start,
                loc.end,
                current_tsx_path,
                tsx_line_index,
                mapper,
                carrier_line_index,
                external_resolver,
            ) else {
                continue;
            };
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
            continue;
        }

        // Every other target: the emitted URI is the file the provider's byte offsets index only
        // when path normalization is a no-op. Read that source and convert the offsets to a real
        // `Range`; fail closed otherwise (a `{carrier}.d.ts`/`{carrier}.ts` whose URI is rewritten
        // to the carrier source has no in-context sourcemap bridging the offsets → drop it).
        let normalized = normalize_carrier_path(&loc.path, carrier_source_exists);
        if normalized != loc.path {
            continue;
        }
        let Some(uri) = path_to_uri(normalized) else {
            continue;
        };
        let Some(range) = resolve_external_target_range(
            &loc.path,
            loc.start,
            loc.end,
            negotiated_encoding.clone(),
            source_reader,
        ) else {
            continue;
        };
        let dup = result
            .iter()
            .any(|r| r.uri == uri && r.range.start == range.start);
        if !dup {
            result.push(Location { uri, range });
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
/// - Start with verter's same-file WorkspaceEdit.
/// - Add TypeProvider's cross-file rename locations as additional TextEdits.
/// - A carrier IDE target (`{carrier}.tsx`/`.jsx`) maps its TSX byte offsets back to the carrier
///   source through the single shared strict mapper ([`resolve_carrier_ide_range_strict`]): the
///   in-context mapper for the queried file, the external resolver for a foreign component (a
///   foreign target is DROPPED on a resolver miss, never line-0'd through the current sourcemap).
/// - A carrier PUBLIC-API target (`{carrier}.ts`) — the surface where an imported component's
///   `defineProps<{ … }>` props are lifted into the `$props` / `new(props?)` declaration — maps its
///   API-surface byte offsets back to the carrier source through that surface's own CodeTransform
///   sourcemap (the `external_api_resolver`). This is THE common cross-file `.vue` prop-rename case:
///   tsserver reports the renamed prop against the child component's `{carrier}.ts`, and without
///   this branch the edit was dropped by carrier-path normalization → the rename touched only the
///   queried file (an incomplete rename = dangling references).
/// - Every other target's `start`/`end` are REAL byte offsets into that file: read its own source
///   through the host VFS (`source_reader`) and convert to a line:col `Range` in the negotiated
///   `encoding`, exactly as the definition / references merges do.
/// - FAIL CLOSED: when the source / offsets cannot be resolved (or normalization rewrote the URI
///   to a carrier source no sourcemap bridges), DROP the edit. A `Range::default()` rename edit is
///   especially dangerous — it would write the new name at line 0 of the wrong file and CORRUPT it.
#[allow(clippy::mutable_key_type)]
#[expect(
    clippy::too_many_arguments,
    reason = "rename merging needs the current TSX path, mapper, indexes, IDE+API resolvers, encoding, and VFS reader"
)]
pub fn merge_rename_locations(
    verter_edit: Option<WorkspaceEdit>,
    type_locations: Vec<RenameLocation>,
    new_name: &str,
    current_tsx_path: &str,
    tsx_line_index: &LineIndex,
    mapper: &ProviderPositionMapper,
    carrier_line_index: &LineIndex,
    external_resolver: Option<ExternalIdeResolver<'_>>,
    external_api_resolver: Option<ExternalApiResolver<'_>>,
    carrier_source_exists: &dyn Fn(&str) -> bool,
    negotiated_encoding: PositionEncodingKind,
    source_reader: ExternalSourceReader<'_>,
) -> Option<WorkspaceEdit> {
    let mut edit = verter_edit.unwrap_or_else(|| WorkspaceEdit {
        changes: Some(std::collections::HashMap::new()),
        ..Default::default()
    });

    let changes = edit
        .changes
        .get_or_insert_with(std::collections::HashMap::new);

    for loc in &type_locations {
        // FAIL CLOSED: a carrier-IDE mapping failure DROPS the rename edit — a `Range::default()`
        // rename edit would write the new name at line 0 of the wrong file and CORRUPT it. Routes
        // through the single shared strict mapper, split by canonical identity: the CURRENT request's
        // TSX uses the in-context mapper; a FOREIGN carrier `.tsx` requires its own context via the
        // external resolver and is DROPPED on a miss — never mapped through the current sourcemap.
        if is_carrier_ide_path(&loc.path) {
            let Some(range) = resolve_carrier_ide_range_strict(
                &loc.path,
                loc.start,
                loc.end,
                current_tsx_path,
                tsx_line_index,
                mapper,
                carrier_line_index,
                external_resolver,
            ) else {
                continue;
            };
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
            continue;
        }

        // Carrier PUBLIC-API target (`{carrier}.ts`, e.g. `Child.vue.ts`): tsserver reports a
        // cross-file Vue prop rename against the imported component's macro-derived public-API
        // surface, whose offsets must map back onto the `.vue` source through that surface's
        // CodeTransform source map.
        //
        // Classification is the resolver's job, not the suffix's. The `external_api_resolver` is
        // identity-gated against the IN-MEMORY synced-virtual-API set and returns a 3-state
        // [`ApiSurfaceResolution`]; the suffix predicate only decides whether to CONSULT it.
        // A bare `Option` could not distinguish "not a virtual surface" from "a known virtual
        // surface we can no longer map" — and the second case, falling through to the real-file
        // branch below, would edit a same-named real file with VIRTUAL offsets and corrupt it.
        // The three outcomes:
        //
        //   1. `Vouched(ctx)` → map the API-surface offsets onto the `.vue` carrier via the API
        //      source map (UTF-16 lookup re-emitted in the negotiated encoding). A vouched surface
        //      whose offsets fail to map is DROPPED (fail closed) — never line-0'd into the `.vue`.
        //   2. `VirtualDrop` → a known virtual surface whose generation was superseded/retired or
        //      whose snapshot has no source map: its offsets index VIRTUAL content, so DROP (fail
        //      closed). NEVER reach the real-file branch (that is the corruption guard).
        //   3. `NotVirtual` → not a virtual surface; the offsets index this exact path's REAL file
        //      (a hand-written `Child.vue.ts` next to `Child.vue`): edit it IN PLACE (read its own
        //      source). Nothing is mapped into the `.vue`. A path with no real backing file then
        //      reads back `None` and the edit is dropped (fail closed).
        if is_carrier_api_path(&loc.path, carrier_source_exists) {
            match external_api_resolver
                .map(|resolver| resolver(&loc.path))
                .unwrap_or(ApiSurfaceResolution::NotVirtual)
            {
                ApiSurfaceResolution::Vouched(ctx) => {
                    // Outcome 1: vouched virtual surface. The negotiated carrier index is mandatory
                    // — it re-emits the UTF-16 source-map result in the negotiated encoding.
                    if let Some(range) =
                        ctx.carrier_negotiated_line_index.as_ref().and_then(|neg| {
                            api_surface_range_to_carrier_range(
                                loc.start,
                                loc.end,
                                &ctx.tsx_line_index,
                                &ctx.mapper,
                                &ctx.carrier_line_index,
                                neg,
                            )
                        })
                    {
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
                    }
                    // Vouched-but-unmappable falls through here → DROP (fail closed).
                }
                ApiSurfaceResolution::VirtualDrop => {
                    // Outcome 2: known virtual surface, no safe mapping → DROP. Crucially do NOT
                    // fall through to the real-file branch: the offsets are virtual and a
                    // same-named real file at this path would be corrupted.
                }
                ApiSurfaceResolution::NotVirtual => {
                    // Outcome 3: not the virtual surface. If a REAL file backs this exact path, the
                    // offsets index IT: edit it in place (never map into the `.vue`). Otherwise the
                    // readback returns `None` and the edit is dropped (fail closed).
                    if let Some(range) = resolve_external_target_range(
                        &loc.path,
                        loc.start,
                        loc.end,
                        negotiated_encoding.clone(),
                        source_reader,
                    ) {
                        if let Some(uri) = path_to_uri(&loc.path) {
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
                }
            }
            continue;
        }

        // Every other target: read its own source and convert the byte offsets, fail closed.
        let normalized = normalize_carrier_path(&loc.path, carrier_source_exists);
        if normalized != loc.path {
            continue;
        }
        let Some(uri) = path_to_uri(normalized) else {
            continue;
        };
        let Some(range) = resolve_external_target_range(
            &loc.path,
            loc.start,
            loc.end,
            negotiated_encoding.clone(),
            source_reader,
        ) else {
            continue;
        };
        let edits = changes.entry(uri).or_default();
        let dup = edits.iter().any(|e| e.range.start == range.start);
        if !dup {
            edits.push(TextEdit {
                range,
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
                            // Map the carrier's two label forms onto LSP's:
                            // `Simple` → literal string; `Offsets` →
                            // `LabelOffsets` so the client bolds the exact param
                            // span within the rendered signature label.
                            label: match p.label {
                                protocol::ParameterLabelKind::Simple(text) => {
                                    ParameterLabel::Simple(text)
                                }
                                protocol::ParameterLabelKind::Offsets(start, end) => {
                                    ParameterLabel::LabelOffsets([start, end])
                                }
                            },
                            documentation: p.documentation.map(|d| {
                                Documentation::MarkupContent(MarkupContent {
                                    kind: MarkupKind::Markdown,
                                    value: d,
                                })
                            }),
                        })
                        .collect(),
                ),
                // Per-signature active parameter (mirrors LSP
                // `SignatureInformation.activeParameter`); takes precedence over
                // the top-level value in clients when present.
                active_parameter: s.active_parameter,
            })
            .collect(),
        active_signature: sig.active_signature,
        active_parameter: sig.active_parameter,
    })
}

// ── Code actions merge ──────────────────────────────────────────────

/// Convert TypeProvider code actions to LSP CodeActions.
///
/// A carrier IDE edit maps its TSX byte offsets back to the carrier source through that file's
/// CodeTransform sourcemap, split by whether the edit targets the file currently being queried:
/// - The CURRENT request's TSX (`edit.path == current_tsx_path`) maps through the in-context mapper.
/// - A FOREIGN carrier `.tsx` (a different component) maps through THAT file's own sourcemap via the
///   `external_resolver`. FAIL CLOSED: a foreign edit is NEVER mapped through the current request's
///   mapper — without a resolver (or on a resolver miss / map failure) it is DROPPED, because the
///   offsets index the foreign TSX and the current sourcemap would point at an unrelated location.
///
/// A provider `addMissingImport` quickfix inserts a brand-new `import …` line at the TOP of the
/// generated TSX, which in Verter's projection lands inside the synthetic, unmapped helper-import
/// preamble — so the strict mapper returns `None` and the edit would be DROPPED. Such a CURRENT-file
/// preamble insertion is re-anchored at the carrier's `<script setup>` import insertion site through
/// the SAME proven mechanism the completion auto-import path uses
/// ([`crate::type_provider::auto_import::reanchor_preamble_import_edit`]) — `carrier_source` and
/// `user_import_spans` (the SFC-absolute top-level import spans) drive that anchor. A FOREIGN
/// carrier `.tsx` preamble insertion is NOT re-anchored (its `<script setup>` is not in-context) and
/// stays dropped; a `SelfFile` projection has no preamble boundary, so it fails closed too.
///
/// Every other edit's `start`/`end` are REAL byte offsets into its target file, so read that file's
/// own source through the host VFS (`source_reader`) and convert to a line:col `Range` in the
/// negotiated `encoding`, exactly as the references / rename merges do. FAIL CLOSED: drop an edit
/// whose source / offsets cannot be resolved (or whose URI is a carrier source no sourcemap
/// bridges) rather than emit a `Range::default()` edit that would write at line 0 of the wrong
/// file. An action with no surviving edit is dropped entirely.
#[allow(clippy::mutable_key_type)] // Uri has interior mutability but is used as key by tower-lsp API
#[expect(
    clippy::too_many_arguments,
    reason = "code-action merging needs the current TSX path, mapper, indexes, resolver, encoding, VFS reader, plus the carrier source + import spans for the add-import prelude re-anchor"
)]
pub fn merge_code_actions(
    type_actions: Vec<TypeCodeAction>,
    current_tsx_path: &str,
    tsx_line_index: &LineIndex,
    mapper: &ProviderPositionMapper,
    carrier_line_index: &LineIndex,
    external_resolver: Option<ExternalIdeResolver<'_>>,
    carrier_source_exists: &dyn Fn(&str) -> bool,
    negotiated_encoding: PositionEncodingKind,
    source_reader: ExternalSourceReader<'_>,
    carrier_source: &str,
    user_import_spans: &[(u32, u32)],
) -> Vec<CodeActionOrCommand> {
    type_actions
        .into_iter()
        .filter_map(|action| {
            let mut changes: std::collections::HashMap<Uri, Vec<TextEdit>> =
                std::collections::HashMap::new();

            for edit in action.edits {
                // Canonicalize the edit path once and carry it through every downstream
                // step (identity, carrier-suffix strip, URI emission, source read) so the
                // path spelling stays consistent and host-VFS reads key correctly.
                let edit_path = verter_span::path::canonicalize_path_cow(&edit.path);
                let edit_path = edit_path.as_ref();

                if is_carrier_ide_path(edit_path) {
                    // Map the carrier-IDE offsets through the single shared strict mapper, split by
                    // canonical identity: the CURRENT request's TSX uses the in-context mapper; a
                    // FOREIGN carrier `.tsx` requires its own context via the external resolver and
                    // is DROPPED on a miss/failure — never mapped through the current mapper.
                    let mapped = resolve_carrier_ide_range_strict(
                        edit_path,
                        edit.start,
                        edit.end,
                        current_tsx_path,
                        tsx_line_index,
                        mapper,
                        carrier_line_index,
                        external_resolver,
                    );
                    if let Some(range) = mapped {
                        let carrier_path = normalize_carrier_path(edit_path, carrier_source_exists);
                        if let Some(uri) = path_to_uri(carrier_path) {
                            changes.entry(uri).or_default().push(TextEdit {
                                range,
                                new_text: edit.new_text,
                            });
                        }
                        continue;
                    }

                    // Strict-mapper MISS. A provider `addMissingImport` quickfix inserts a brand-new
                    // import at the synthetic helper-import preamble (offset 0), which the strict
                    // mapper legitimately drops. Before dropping, attempt the SAME prelude re-anchor
                    // the completion auto-import path uses — but ONLY for the CURRENT request's TSX:
                    // `carrier_source` / `user_import_spans` / the in-context mapper describe the
                    // queried file, so a FOREIGN carrier `.tsx` preamble insertion has no in-context
                    // `<script setup>` anchor and MUST stay dropped (re-anchoring it would splice an
                    // import into the wrong `.vue`). `reanchor_preamble_import_edit` itself
                    // fail-closes for a non-preamble miss and for a `SelfFile` projection (no
                    // boundary), so every other miss falls through to the drop below.
                    let is_current_file = verter_span::path::canonicalize_path_cow(edit_path)
                        == verter_span::path::canonicalize_path_cow(current_tsx_path);
                    if is_current_file {
                        let provider_edit = crate::type_provider::auto_import::ProviderImportEdit {
                            start: edit.start,
                            end: edit.end,
                            new_text: edit.new_text.clone(),
                        };
                        if let Some(reanchored) =
                            crate::type_provider::auto_import::reanchor_preamble_import_edit(
                                &provider_edit,
                                tsx_line_index,
                                mapper,
                                carrier_source,
                                carrier_line_index,
                                user_import_spans,
                            )
                        {
                            let carrier_path =
                                normalize_carrier_path(edit_path, carrier_source_exists);
                            if let Some(uri) = path_to_uri(carrier_path) {
                                changes.entry(uri).or_default().push(reanchored);
                            }
                        }
                    }
                    // FAIL CLOSED: any unmapped carrier-IDE edit that is not a re-anchorable
                    // current-file preamble insertion is dropped — never line-0'd.
                    continue;
                }

                // Every other edit: read its own target source and convert the byte offsets, fail
                // closed (drop) — never emit a line-0 edit. A rewritten carrier-source URL has no
                // in-context sourcemap bridging the offsets, so it is dropped too.
                let normalized = normalize_carrier_path(edit_path, carrier_source_exists);
                if normalized != edit_path {
                    continue;
                }
                let Some(uri) = path_to_uri(normalized) else {
                    continue;
                };
                let Some(range) = resolve_external_target_range(
                    edit_path,
                    edit.start,
                    edit.end,
                    negotiated_encoding.clone(),
                    source_reader,
                ) else {
                    continue;
                };
                changes.entry(uri).or_default().push(TextEdit {
                    range,
                    new_text: edit.new_text,
                });
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
