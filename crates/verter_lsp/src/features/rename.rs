// Rename — rename bindings across script/template blocks in a single file.
// Enhanced with cross-file rename from TypeProvider.

use std::collections::HashMap;

use tower_lsp_server::ls_types::*;
use verter_session::FileAnalysisSnapshot;

use crate::documents::line_index::LineIndex;
use crate::documents::sfc_scanner::SfcBlock;
use crate::features::references::{
    collect_css_ref_spans, find_css_target_in_style_refs, find_css_target_in_template_refs,
    offset_is_instance_member_access, CssRefTarget,
};

pub use super::sentinel_uris::SAME_FILE_URI;
pub use super::sentinel_uris::SAME_FILE_URI_STR;

/// Check if the symbol at the given position can be renamed.
///
/// Returns a `Range` of the symbol if renaming is allowed, or `None` if not.
///
/// An instance-member template access ([`offset_is_instance_member_access`]) is
/// NOT natively renameable: the name-based match below would hand the editor the
/// word range of a same-named script declaration, which is a different symbol.
/// Only the TypeScript provider can resolve that position, so this surface
/// declines it.
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

    if offset_is_instance_member_access(offset as u32, analysis) {
        return prepare_rename_css(offset, source, analysis, line_index);
    }

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

/// Whether the target is a CSS class/id owned by Verter's native workspace
/// index. This surface is complete without a TypeScript provider.
pub fn is_css_rename_position(
    position: &Position,
    source: &str,
    analysis: &FileAnalysisSnapshot,
    line_index: &LineIndex,
) -> bool {
    let Some(offset) = line_index
        .position_to_offset(position)
        .map(|value| value as usize)
    else {
        return false;
    };
    analysis
        .template
        .as_ref()
        .and_then(|template| find_css_target_in_template_refs(offset, source, template))
        .or_else(|| find_css_target_in_style_refs(offset, source, analysis))
        .is_some()
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

/// Perform a rename of the symbol at the given position to `new_name`.
///
/// Finds all occurrences in script and template blocks and returns a
/// `WorkspaceEdit` with text edits for each occurrence.
///
pub fn rename_at_position(
    position: &Position,
    new_name: &str,
    source: &str,
    blocks: &[SfcBlock],
    analysis: Option<&FileAnalysisSnapshot>,
    line_index: &LineIndex,
) -> Option<WorkspaceEdit> {
    let ranges = same_file_rename_ranges(position, source, blocks, analysis, line_index)?;
    let edits: Vec<TextEdit> = ranges
        .into_iter()
        .map(|range| TextEdit {
            range,
            new_text: new_name.to_string(),
        })
        .collect();
    if edits.is_empty() {
        return None;
    }

    #[allow(clippy::mutable_key_type)] // Uri has interior mutability but we only insert once
    let mut changes = HashMap::new();
    changes.insert(SAME_FILE_URI.clone(), edits);

    Some(WorkspaceEdit {
        changes: Some(changes),
        ..Default::default()
    })
}

/// Every range in THIS file that a rename at `position` must overwrite.
///
/// The single authority for the same-file rename surface: [`rename_at_position`]
/// builds its `TextEdit`s from exactly this set, and the server proves the
/// emitted (possibly provider-merged) transaction still covers it. `None` means
/// nothing under the cursor is renameable BY THIS SURFACE — either nothing
/// resolves, or the position belongs to a symbol only the TypeScript provider
/// can resolve (an instance-member template access), in which case the provider
/// is the sole authority and an empty provider answer must ship no edit at all.
pub fn same_file_rename_ranges(
    position: &Position,
    source: &str,
    blocks: &[SfcBlock],
    analysis: Option<&FileAnalysisSnapshot>,
    line_index: &LineIndex,
) -> Option<Vec<Range>> {
    let analysis = analysis?;
    let offset = line_index.position_to_offset(position)? as usize;
    let word = word_at_offset(source, offset)?;

    // The cursor sits inside an instance-member template access
    // (`___VERTER___instance.<name>`). A same-named script declaration is a
    // DIFFERENT symbol, so the name-based surface below must not answer: yield
    // to the positional CSS owner (which answers `None` here) and let the
    // provider own the position.
    if offset_is_instance_member_access(offset as u32, analysis) {
        return css_rename_spans(offset, source, analysis)
            .map(|spans| to_ranges(spans, line_index));
    }

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
        return css_rename_spans(offset, source, analysis)
            .map(|spans| to_ranges(spans, line_index));
    }

    let mut spans: Vec<(u32, u32)> = Vec::new();
    let push_span = |spans: &mut Vec<(u32, u32)>, start: u32, end: u32| {
        if !spans.iter().any(|(existing, _)| *existing == start) {
            spans.push((start, end));
        }
    };

    // Host analysis spans are already SFC-absolute.
    // Collect declaration spans from the host snapshot.
    if let Some(binding) = analysis.bindings.iter().find(|b| b.name == word) {
        if binding.span.start > 0 || binding.span.end > 0 {
            push_span(&mut spans, binding.span.start, binding.span.end);
        }
    }
    for import in &analysis.imports {
        for binding in &import.bindings {
            if binding.name == word && (binding.span.start > 0 || binding.span.end > 0) {
                push_span(&mut spans, binding.span.start, binding.span.end);
            }
        }
    }

    // Span-based template occurrences (precise, no false positives).
    // `binding_occurrences` is the ONLY template inventory that names this
    // symbol: it holds the expression spans whose name the compiler's template
    // bindings map DID contain, which is exactly the set that lowers to a bare
    // identifier over the script binding. The complement,
    // `unresolved_bindings`, lowers to `___VERTER___instance.<name>` — an
    // instance property, a different symbol — and is never rewritten from here.
    if let Some(template) = &analysis.template {
        for occ in &template.binding_occurrences {
            if occ.name != word {
                continue;
            }
            // `push_span` skips an occurrence already recorded (a declaration
            // span).
            push_span(&mut spans, occ.span.start, occ.span.end);
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
            push_span(&mut spans, abs_offset as u32, abs_end as u32);
        }
    }

    Some(to_ranges(spans, line_index))
}

/// The CSS class/ID spans a rename at `offset` must overwrite, across template
/// and style blocks.
fn css_rename_spans(
    offset: usize,
    source: &str,
    analysis: &FileAnalysisSnapshot,
) -> Option<Vec<(u32, u32)>> {
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
    Some(spans)
}

/// Convert SFC-absolute spans to `Range`s, dropping any that do not convert
/// (fail closed — never a fabricated line-0 range).
fn to_ranges(spans: Vec<(u32, u32)>, line_index: &LineIndex) -> Vec<Range> {
    spans
        .into_iter()
        .filter_map(|(start, end)| {
            Some(Range {
                start: line_index.offset_to_position(start)?,
                end: line_index.offset_to_position(end)?,
            })
        })
        .collect()
}

use crate::utils::{find_all_word_occurrences, find_word_start, word_at_offset};

#[cfg(test)]
#[path = "rename_tests.rs"]
mod rename_tests;
