//! Structural translation of TypeProvider completion-resolve auto-import edits.
//!
//! When the user accepts a completion that needs an import, tsserver/TSGO return the
//! auto-import as a `completionItem/resolve` `additionalTextEdit` whose `start`/`end` are
//! byte offsets into the GENERATED TSX. For a brand-new import TypeScript places that edit
//! at the top-of-file / sorted-import boundary, which in Verter's generated TSX lands inside
//! the synthetic, UNMAPPED helper-import preamble (`import { defineComponent } …`, the
//! `@verter/types` helpers, …). The strict [`PositionMapper`](crate::documents::position_map::PositionMapper)
//! correctly returns `None` for synthetic/unmapped content, so a positional mapping cannot
//! recover the insertion point — and must not be weakened to try.
//!
//! The generated offset is therefore NOT authoritative for an import edit. This module
//! translates such edits structurally: the import text is re-anchored at a Vue-source
//! [`ScriptImportInsertionAnchor`] computed from the SFC's own block/import facts (the end of
//! the source import block, the `<script setup>` content start, or a freshly synthesized
//! `<script setup>` block — Volar parity). Edits that DO round-trip through the strict mapper
//! target real user source and are applied verbatim. The translation is all-or-nothing: if an
//! unmapped edit cannot be re-anchored, the whole resolve is rejected rather than silently
//! dropping it.

use tower_lsp_server::ls_types::{Range, TextEdit};

use crate::documents::line_index::LineIndex;
use crate::documents::position_map::PositionMapper;
use crate::documents::sfc_scanner::scan_sfc_blocks;
use crate::tsgo::merge;

/// One TypeProvider text edit as returned by completion resolve: byte offsets into the
/// generated TSX plus the replacement text.
///
/// Mirrors `verter_type_runtime::protocol::ResolvedTextEdit` without coupling the pure
/// translation logic (and its hermetic tests) to the provider DTO.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderImportEdit {
    /// Byte offset start in the generated TSX.
    pub start: u32,
    /// Byte offset end in the generated TSX.
    pub end: u32,
    /// The replacement / inserted text (for a new import, a full `import … from '…'` line).
    pub new_text: String,
}

/// Where a new import should be inserted into the **Vue source** (never the generated TSX).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScriptImportInsertionAnchor {
    /// Insert into an existing `<script setup>` block at the given Vue-source byte offset
    /// (the end of the source import block when user imports exist, otherwise the script
    /// content start).
    ExistingScriptSetup { offset: u32 },
    /// No `<script setup>` block exists — synthesize one (Volar parity). `offset` is the
    /// Vue-source byte offset to insert the new block at; `open_tag` / `close_tag` wrap the
    /// import text into a real block.
    CreateScriptSetup {
        offset: u32,
        open_tag: String,
        close_tag: String,
    },
}

impl ScriptImportInsertionAnchor {
    /// Build a single zero-width-range `TextEdit` inserting `import_texts` (in order) at this
    /// anchor. Returns `None` if the anchor offset is not a valid position in `vue_li`.
    pub fn build_edit(&self, import_texts: &[String], vue_li: &LineIndex) -> Option<TextEdit> {
        let (offset, new_text) = match self {
            ScriptImportInsertionAnchor::ExistingScriptSetup { offset } => {
                (*offset, import_texts.concat())
            }
            ScriptImportInsertionAnchor::CreateScriptSetup {
                offset,
                open_tag,
                close_tag,
            } => {
                let mut text = String::with_capacity(
                    open_tag.len()
                        + close_tag.len()
                        + import_texts.iter().map(String::len).sum::<usize>(),
                );
                text.push_str(open_tag);
                for t in import_texts {
                    text.push_str(t);
                }
                text.push_str(close_tag);
                (*offset, text)
            }
        };
        let pos = vue_li.offset_to_position(offset)?;
        Some(TextEdit {
            range: Range::new(pos, pos),
            new_text,
        })
    }
}

/// Resolve the Vue-source insertion anchor for auto-imports from the SFC's block/import facts.
///
/// `user_import_spans` are the **SFC-absolute** `(start, end)` byte spans of the SFC's top-level
/// imports, exactly as `AnalyzedImport.span` produces them: `verter_session` parses a
/// position-preserving SFC-offset script source, so every analysis span is SFC-absolute by
/// construction (see `verter_semantic::analysis::types::AnalyzedImport`). They are consumed in
/// that coordinate space DIRECTLY — the selected `<script setup>` content start is NEVER re-added.
///
/// * existing `<script setup>` + imports inside it → end of the last in-block import;
/// * existing `<script setup>` + no in-block imports → script content start (on its own line);
/// * no `<script setup>` → synthesize a real block, mirroring an existing `<script>` `lang`
///   when present, else defaulting to TypeScript.
///
/// Imports are filtered to those whose span lies inside the selected `<script setup>` block, so an
/// import in a separate non-setup `<script>` never anchors the setup insertion.
pub fn resolve_script_import_anchor(
    vue_source: &str,
    user_import_spans: &[(u32, u32)],
) -> ScriptImportInsertionAnchor {
    let blocks = scan_sfc_blocks(vue_source);

    if let Some(setup) = blocks.iter().find(|b| b.is_setup()) {
        let (content_start, content_end) = setup.content_range();
        // SFC-absolute import ends are already in the document coordinate space — consume them
        // directly (NO re-add of `content_start`). Only imports whose whole span lies inside THIS
        // block's content range count; an import in a separate non-setup `<script>` is excluded.
        let last_in_block_end = user_import_spans
            .iter()
            .copied()
            .filter(|&(start, end)| start >= content_start && end <= content_end)
            .map(|(_, end)| end)
            .max();
        let offset = match last_in_block_end {
            // After the last in-block import — skip the run of line breaks so the new import
            // lands on the next line rather than fused to the previous statement.
            Some(abs_end) => skip_line_breaks(vue_source, abs_end),
            // No in-block imports — insert at the content start, past a single leading line
            // break so the import is not glued to the `<script setup …>` tag.
            None => skip_one_line_break(vue_source, content_start),
        };
        return ScriptImportInsertionAnchor::ExistingScriptSetup { offset };
    }

    // No `<script setup>` block — create one at the top of the file.
    let open_tag = match blocks.iter().find(|b| b.tag_name == "script") {
        Some(existing) => match existing.lang() {
            Some(lang) => format!("<script setup lang=\"{lang}\">\n"),
            None => "<script setup>\n".to_string(),
        },
        None => "<script setup lang=\"ts\">\n".to_string(),
    };
    ScriptImportInsertionAnchor::CreateScriptSetup {
        offset: 0,
        open_tag,
        close_tag: "</script>\n\n".to_string(),
    }
}

/// Errors that reject a completion resolve as a whole rather than applying a partial edit set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AutoImportEditMappingError {
    /// A re-anchorable auto-import insertion was identified, but no Vue insertion anchor was
    /// available to receive its import text. The whole resolve is rejected.
    NoInsertionAnchor,
    /// A provider edit did not round-trip through the strict mapper and is NOT structurally a
    /// zero-width auto-import insertion in the synthetic helper-import preamble — e.g. a
    /// replacement of synthetic code, a (zero-width) edit in a non-preamble synthetic region, or
    /// an out-of-range offset. It is rejected rather than spliced into user source.
    UnmappableEdit { start: u32, end: u32 },
}

impl std::fmt::Display for AutoImportEditMappingError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AutoImportEditMappingError::NoInsertionAnchor => f.write_str(
                "auto-import edit landed in synthetic generated TSX with no Vue insertion anchor",
            ),
            AutoImportEditMappingError::UnmappableEdit { start, end } => write!(
                f,
                "provider edit [{start}, {end}) is neither mappable user source nor a \
                 synthetic-preamble auto-import insertion",
            ),
        }
    }
}

impl std::error::Error for AutoImportEditMappingError {}

/// Whether an unmapped provider edit is structurally a re-anchorable auto-import insertion: a
/// ZERO-WIDTH insertion located within the synthetic helper-import preamble. Proven from STRUCTURE
/// only — the edit's geometry and the typed preamble-end boundary the IDE codegen publishes on the
/// source map ([`PositionMapper::helper_preamble_end`]) — never from `new_text` content (the
/// no-text-sniffing rule).
///
/// The boundary is the generated-TSX position immediately after the last emitted helper import. An
/// insertion at or before it lands in the preamble (re-anchorable); anything past it is trailing
/// synthetic component/export code and is NOT a preamble insertion. The boundary is the
/// AUTHORITATIVE gate: it is exact even when the generated file has no mapped runs (an empty
/// `<script setup>`) and when user imports precede the helper preamble (a companion `<script>`),
/// the two cases a "before the first mapped run" heuristic gets wrong. With no boundary metadata
/// the edit cannot be proven to be in the preamble, so it is rejected — never re-anchored on a guess.
fn is_preamble_import_insertion(
    edit: &ProviderImportEdit,
    tsx_li: &LineIndex,
    mapper: &PositionMapper,
) -> bool {
    // A non-empty range is a replacement of synthetic code, not an insertion.
    if edit.start != edit.end {
        return false;
    }
    // Must address a real position inside the generated TSX (rejects out-of-range offsets).
    let Some(pos) = tsx_li.offset_to_position(edit.start) else {
        return false;
    };
    match mapper.helper_preamble_end() {
        // At or before the published preamble-end boundary ⇒ inside the synthetic helper-import
        // preamble, so a re-anchorable insertion. Past it ⇒ trailing synthetic component/export
        // code, which is NOT a preamble insertion.
        Some(end) => (pos.line, pos.character) <= (end.line, end.character),
        // No boundary metadata ⇒ the edit cannot be proven to land in the preamble (a non-Verter
        // map, or an older artifact). Reject rather than re-anchor on a guess.
        None => false,
    }
}

/// Translate a TypeProvider's completion-resolve `additionalTextEdits` (generated-TSX byte
/// offsets) into Vue-source [`TextEdit`]s, with no silent drops.
///
/// Two routes, plus a rejection:
/// * an edit whose generated range round-trips through the strict [`PositionMapper`] targets
///   real mapped user source (e.g. an `AddToExisting` import extending the user's own import
///   statement) and is applied verbatim at its mapped Vue range;
/// * an edit that does NOT round-trip is re-anchored at the Vue [`ScriptImportInsertionAnchor`]
///   ONLY when it is provably a zero-width auto-import insertion in Verter's synthetic,
///   unmapped helper-import preamble ([`is_preamble_import_insertion`]);
/// * any other mapper miss — a replacement of synthetic code, a zero-width edit in a
///   non-preamble synthetic region, or an out-of-range offset — yields
///   [`AutoImportEditMappingError::UnmappableEdit`] and rejects the whole resolve.
///
/// All re-anchored imports are coalesced into a single edit at the anchor (avoiding overlapping
/// zero-width inserts and synthesizing at most one `<script setup>` block). All-or-nothing: if
/// any edit must be re-anchored but no anchor is available, the whole resolve fails.
pub fn translate_completion_import_edits(
    edits: &[ProviderImportEdit],
    anchor: Option<&ScriptImportInsertionAnchor>,
    tsx_li: &LineIndex,
    mapper: &PositionMapper,
    vue_li: &LineIndex,
) -> Result<Vec<TextEdit>, AutoImportEditMappingError> {
    let mut result: Vec<TextEdit> = Vec::new();
    let mut anchored_imports: Vec<String> = Vec::new();

    for edit in edits {
        match merge::tsx_range_to_vue_range(edit.start, edit.end, tsx_li, mapper, vue_li) {
            // Round-trips through the strict mapper ⇒ targets real mapped user source; apply
            // verbatim at its mapped Vue range (the mapper is never bypassed for these).
            Some(range) => result.push(TextEdit {
                range,
                new_text: edit.new_text.clone(),
            }),
            // A mapper miss is re-anchored ONLY if it is provably a zero-width auto-import
            // insertion in the synthetic helper-import preamble. Every other miss is rejected,
            // never spliced into user source.
            None => {
                if is_preamble_import_insertion(edit, tsx_li, mapper) {
                    anchored_imports.push(edit.new_text.clone());
                } else {
                    return Err(AutoImportEditMappingError::UnmappableEdit {
                        start: edit.start,
                        end: edit.end,
                    });
                }
            }
        }
    }

    if !anchored_imports.is_empty() {
        let anchor = anchor.ok_or(AutoImportEditMappingError::NoInsertionAnchor)?;
        let edit = anchor
            .build_edit(&anchored_imports, vue_li)
            .ok_or(AutoImportEditMappingError::NoInsertionAnchor)?;
        result.push(edit);
    }

    Ok(result)
}

/// Byte offset just past the run of CR/LF bytes starting at `offset`.
fn skip_line_breaks(source: &str, offset: u32) -> u32 {
    let bytes = source.as_bytes();
    let mut i = offset as usize;
    while i < bytes.len() && (bytes[i] == b'\n' || bytes[i] == b'\r') {
        i += 1;
    }
    i as u32
}

/// Byte offset just past a single leading line break (CRLF, LF, or CR) at `offset`.
fn skip_one_line_break(source: &str, offset: u32) -> u32 {
    let bytes = source.as_bytes();
    let mut i = offset as usize;
    if i < bytes.len() && bytes[i] == b'\r' {
        i += 1;
    }
    if i < bytes.len() && bytes[i] == b'\n' {
        i += 1;
    }
    i as u32
}
