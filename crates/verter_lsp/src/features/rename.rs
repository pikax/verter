// Phase 2: Rename — rename bindings across script/template blocks in a single file.
// Phase 3: Enhanced with cross-file rename from TypeProvider.

use std::collections::HashMap;

use tower_lsp_server::lsp_types::*;
use verter_host::FileAnalysisSnapshot;

use crate::documents::line_index::LineIndex;
use crate::documents::sfc_scanner::SfcBlock;
use crate::features::references::{
    collect_css_ref_spans, find_css_target_in_style_refs, find_css_target_in_template_refs,
    CssRefTarget,
};

/// Sentinel URI used when a rename edit is in the same file.
/// The server replaces this with the actual document URI before returning to the client.
pub const SAME_FILE_URI: &str = "verter-internal:same-file";

/// Check if the symbol at the given position can be renamed.
///
/// Returns a `Range` of the symbol if renaming is allowed, or `None` if not.
pub fn prepare_rename(
    position: &Position,
    source: &str,
    _blocks: &[SfcBlock],
    analysis: Option<&FileAnalysisSnapshot>,
    line_index: &LineIndex,
) -> Option<Range> {
    let analysis = analysis?;
    let offset = line_index.position_to_offset(position)? as usize;
    let word = word_at_offset(source, offset)?;

    // Only allow renaming known bindings and non-type imports
    let is_binding = analysis.bindings.iter().any(|b| b.name == word);
    let is_import = analysis
        .imports
        .iter()
        .any(|i| !i.is_type_only && i.bindings.iter().any(|b| b.name == word && !b.is_type_only));
    let is_macro = analysis
        .macros
        .iter()
        .any(|m| m.binding_name.as_ref().is_some_and(|n| n == &word));

    if !is_binding && !is_import && !is_macro {
        // Try CSS class/ID rename
        return prepare_rename_css(offset, source, analysis, line_index);
    }

    // Return the range of the word at the cursor
    let word_start = find_word_start(source.as_bytes(), offset);
    let word_end = word_start + word.len();

    let start = line_index.offset_to_position(word_start as u32)?;
    let end = line_index.offset_to_position(word_end as u32)?;
    Some(Range { start, end })
}

/// Check if a CSS class/ID name can be renamed.
fn prepare_rename_css(
    offset: usize,
    source: &str,
    analysis: &FileAnalysisSnapshot,
    line_index: &LineIndex,
) -> Option<Range> {
    let target = if let Some(template) = &analysis.template {
        find_css_target_in_template_refs(offset, source, template)
    } else {
        None
    }
    .or_else(|| find_css_target_in_style_refs(offset, source, analysis))?;

    let name = match &target {
        CssRefTarget::Class(n) | CssRefTarget::Id(n) => n,
    };

    // Find the range of the CSS name at cursor — scan for its boundaries
    let spans = collect_css_ref_spans(&target, source, analysis);
    // Find the span that contains the cursor
    for (start, end) in &spans {
        if offset as u32 >= *start && (offset as u32) < *end {
            let s = line_index.offset_to_position(*start)?;
            let e = line_index.offset_to_position(*end)?;
            return Some(Range { start: s, end: e });
        }
    }

    // Fallback: use the name length
    let _ = name;
    None
}

/// Convert an analysis span offset (relative to the combined script content that the
/// host passes to OXC) to an SFC-absolute byte offset.
///
/// The host concatenates script block contents in order `[<script>, <script setup>]`
/// separated by `\n`. OXC spans are relative to this combined string. This function
/// determines which block the offset falls in and converts to an SFC-absolute offset.
fn analysis_span_to_sfc_offset(span_offset: u32, blocks: &[SfcBlock]) -> u32 {
    let script_blocks: Vec<&SfcBlock> = blocks.iter().filter(|b| b.tag_name == "script").collect();

    match script_blocks.len() {
        0 => span_offset,
        1 => script_blocks[0].content_range().0 + span_offset,
        _ => {
            // Dual block: host concatenates [normal, setup] with \n separator.
            let normal = script_blocks.iter().find(|b| !b.is_setup());
            let setup = script_blocks.iter().find(|b| b.is_setup());

            match (normal, setup) {
                (Some(n), Some(s)) => {
                    let (n_start, n_end) = n.content_range();
                    let normal_len = n_end - n_start;
                    if span_offset <= normal_len {
                        n_start + span_offset
                    } else {
                        // Skip past normal content + \n separator
                        s.content_range().0 + (span_offset - normal_len - 1)
                    }
                }
                (Some(n), None) => n.content_range().0 + span_offset,
                (None, Some(s)) => s.content_range().0 + span_offset,
                (None, None) => span_offset,
            }
        }
    }
}

/// Perform a rename of the symbol at the given position to `new_name`.
///
/// Finds all occurrences in script and template blocks and returns a
/// `WorkspaceEdit` with text edits for each occurrence.
///
/// ## Coordinate systems
///
/// Analysis spans (`AnalyzedBinding.span.start`, `AnalyzedImportBinding.span.start`)
/// are byte offsets relative to the combined script content that the host passes to
/// OXC for parsing. These must be converted to SFC-absolute offsets before calling
/// `span_to_edit()`. See `analysis_span_to_sfc_offset()` for the conversion logic.
///
/// Template binding occurrences (`TemplateBindingOccurrence.span.start`) are already
/// SFC-absolute and need no conversion.
pub fn rename_at_position(
    position: &Position,
    new_name: &str,
    source: &str,
    blocks: &[SfcBlock],
    analysis: Option<&FileAnalysisSnapshot>,
    line_index: &LineIndex,
) -> Option<WorkspaceEdit> {
    let analysis = analysis?;
    let offset = line_index.position_to_offset(position)? as usize;
    let word = word_at_offset(source, offset)?;

    // Verify renaming is allowed
    let is_binding = analysis.bindings.iter().any(|b| b.name == word);
    let is_import = analysis
        .imports
        .iter()
        .any(|i| !i.is_type_only && i.bindings.iter().any(|b| b.name == word && !b.is_type_only));
    let is_macro = analysis
        .macros
        .iter()
        .any(|m| m.binding_name.as_ref().is_some_and(|n| n == &word));

    if !is_binding && !is_import && !is_macro {
        // Try CSS class/ID rename
        return rename_css(offset, new_name, source, analysis, line_index);
    }

    let mut edits: Vec<TextEdit> = Vec::new();

    // Collect declaration spans (convert script-relative → SFC-absolute)
    if let Some(binding) = analysis.bindings.iter().find(|b| b.name == word) {
        if binding.span.start > 0 || binding.span.end > 0 {
            let abs_start = analysis_span_to_sfc_offset(binding.span.start, blocks);
            let abs_end = analysis_span_to_sfc_offset(binding.span.end, blocks);
            if let Some(edit) = span_to_edit(abs_start, abs_end, new_name, line_index) {
                edits.push(edit);
            }
        }
    }
    for import in &analysis.imports {
        for binding in &import.bindings {
            if binding.name == word && (binding.span.start > 0 || binding.span.end > 0) {
                let abs_start = analysis_span_to_sfc_offset(binding.span.start, blocks);
                let abs_end = analysis_span_to_sfc_offset(binding.span.end, blocks);
                if let Some(edit) = span_to_edit(abs_start, abs_end, new_name, line_index) {
                    edits.push(edit);
                }
            }
        }
    }

    // Use span-based template binding occurrences (precise, no false positives)
    if let Some(template) = &analysis.template {
        for occ in &template.binding_occurrences {
            if occ.name == word {
                // Skip if this overlaps a declaration span edit
                let already_covered = edits.iter().any(|e| {
                    let e_start = line_index.position_to_offset(&e.range.start);
                    e_start == Some(occ.span.start)
                });
                if !already_covered {
                    if let Some(edit) =
                        span_to_edit(occ.span.start, occ.span.end, new_name, line_index)
                    {
                        edits.push(edit);
                    }
                }
            }
        }
    }

    // For script blocks, use text search for usages (beyond declaration spans)
    for block in blocks {
        if block.tag_name != "script" {
            continue;
        }
        let (content_start, content_end) = block.content_range();
        let content = &source[content_start as usize..content_end as usize];

        for occ_offset in find_all_word_occurrences(content, &word) {
            let abs_offset = content_start as usize + occ_offset;
            let abs_end = abs_offset + word.len();

            // Skip if already covered by a declaration or template span
            let already_covered = edits.iter().any(|e| {
                let e_start = line_index.position_to_offset(&e.range.start);
                e_start == Some(abs_offset as u32)
            });
            if already_covered {
                continue;
            }

            if let (Some(start), Some(end)) = (
                line_index.offset_to_position(abs_offset as u32),
                line_index.offset_to_position(abs_end as u32),
            ) {
                edits.push(TextEdit {
                    range: Range { start, end },
                    new_text: new_name.to_string(),
                });
            }
        }
    }

    if edits.is_empty() {
        return None;
    }

    let uri: Uri = SAME_FILE_URI.parse().unwrap();
    #[allow(clippy::mutable_key_type)] // Uri has interior mutability but we only insert once
    let mut changes = HashMap::new();
    changes.insert(uri, edits);

    Some(WorkspaceEdit {
        changes: Some(changes),
        ..Default::default()
    })
}

/// Perform a CSS class/ID rename across template and style blocks.
fn rename_css(
    offset: usize,
    new_name: &str,
    source: &str,
    analysis: &FileAnalysisSnapshot,
    line_index: &LineIndex,
) -> Option<WorkspaceEdit> {
    let target = if let Some(template) = &analysis.template {
        find_css_target_in_template_refs(offset, source, template)
    } else {
        None
    }
    .or_else(|| find_css_target_in_style_refs(offset, source, analysis))?;

    let spans = collect_css_ref_spans(&target, source, analysis);
    if spans.is_empty() {
        return None;
    }

    let edits: Vec<TextEdit> = spans
        .into_iter()
        .filter_map(|(start, end)| span_to_edit(start, end, new_name, line_index))
        .collect();

    if edits.is_empty() {
        return None;
    }

    let uri: Uri = SAME_FILE_URI.parse().unwrap();
    #[allow(clippy::mutable_key_type)]
    let mut changes = HashMap::new();
    changes.insert(uri, edits);

    Some(WorkspaceEdit {
        changes: Some(changes),
        ..Default::default()
    })
}

fn span_to_edit(
    span_start: u32,
    span_end: u32,
    new_name: &str,
    line_index: &LineIndex,
) -> Option<TextEdit> {
    let start = line_index.offset_to_position(span_start)?;
    let end = line_index.offset_to_position(span_end)?;
    Some(TextEdit {
        range: Range { start, end },
        new_text: new_name.to_string(),
    })
}

use crate::utils::{find_all_word_occurrences, find_word_start, word_at_offset};

#[cfg(test)]
#[path = "rename_tests.rs"]
mod rename_tests;
