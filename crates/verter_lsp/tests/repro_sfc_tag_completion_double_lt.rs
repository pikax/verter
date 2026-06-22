//! HERMETIC REPRO — double-`<` on SFC structure-tag completion.
//!
//! USER-REPORTED BUG: in a `.vue` file, typing `<script` and selecting ANY
//! completion item creates the block but inserts an EXTRA `<` before it:
//!
//!     <script setup lang="ts">      becomes      <<script setup lang="ts">
//!
//! Same class affects `<template>` and `<style>` (all produced by the same
//! `sfc_root_completions` snippet path).
//!
//! ROOT CAUSE (located, not fixed here):
//!   `crates/verter_lsp/src/features/completion.rs`
//!     - `sfc_root_completions()` builds the items via `snippet_item()`.
//!     - `snippet_item()` sets `insert_text = "<script lang=\"ts\">..."` (the
//!       leading `<` is INSIDE insertText) and `insert_text_format = SNIPPET`,
//!       but leaves `text_edit = None`.
//!   When a CompletionItem carries NO `textEdit`, an LSP client computes the
//!   replace range itself from the "current word" at the cursor. `<` is a word
//!   BOUNDARY, so for the typed prefix `<script` the client replaces only the
//!   word `script` (NOT the leading `<`). It then inserts the snippet, whose
//!   text ALSO begins with `<`. Result: the user's original `<` survives and
//!   the snippet's `<` is added → `<<script ...>`.
//!
//! THE FIX (recommendation): every SFC tag snippet whose `insert_text` begins
//! with `<` MUST carry an explicit `text_edit` (a `CompletionTextEdit`) whose
//! REPLACE RANGE starts at the offset of that leading `<` already typed by the
//! user (covering `<script` / `<` partial), so applying the edit yields a single
//! `<`. (Equivalently: drop the `<` from insertText AND set a range that starts
//! after the `<`. Either way the leading-`<` accounting must be explicit, not
//! left to the client's word heuristic.)
//!
//! These tests model the LSP client's default no-`textEdit` behavior and assert
//! the provider supplies a `<`-anchored `text_edit`. They FAIL on the current
//! tree (text_edit == None) and PASS once the fix lands.

use tower_lsp_server::ls_types::{
    CompletionItem, CompletionTextEdit, InsertTextFormat, Position, TextEdit,
};
use verter_lsp::documents::line_index::LineIndex;
use verter_lsp::documents::sfc_scanner::scan_sfc_blocks;
use verter_lsp::features::completion::completions_at_position;

/// Drive the real completion entry-point at a `<script|`-style position and
/// return the items the provider would ship to the client.
fn root_items_at(source: &str, cursor_offset: usize) -> Vec<CompletionItem> {
    let blocks = scan_sfc_blocks(source);
    let line_index = LineIndex::new_utf16(source);
    let pos: Position = line_index
        .offset_to_position(cursor_offset as u32)
        .expect("cursor offset must map to a position");
    let result = completions_at_position(
        &pos,
        source,
        &blocks,
        None,         // analysis
        &line_index,  //
        None,         // resolve_component
        None,         // workspace_components
        None,         // doc_uri
        false,        // ssr_context
    );
    result
        .expect("root-level position must yield completions")
        .items
}

/// Faithful model of what a spec-compliant LSP client does with ONE completion
/// item, given the document text and the cursor offset.
///
/// * If the item carries a `text_edit`, the client applies that edit verbatim
///   (range + newText), with snippet placeholders (`$0`, `${1:..}`) collapsed to
///   empty — we only care about the literal characters around the insertion, not
///   the tabstops.
/// * Otherwise (the buggy case) the client replaces the IDENTIFIER WORD ending
///   at the cursor with `insert_text`. `<`, `>`, `/`, whitespace are word
///   boundaries — exactly VS Code / standard LSP word semantics. This is the
///   behavior that yields the double `<`.
fn apply_item(source: &str, cursor_offset: usize, item: &CompletionItem) -> String {
    let strip_snippet = |s: &str| -> String {
        // Collapse the common snippet tabstops so the assertion reads on literal
        // text; `$0`, `$1`, `${1:foo}` → "" / "foo".
        let mut out = String::with_capacity(s.len());
        let bytes = s.as_bytes();
        let mut i = 0;
        while i < bytes.len() {
            if bytes[i] == b'$' {
                i += 1;
                if i < bytes.len() && bytes[i] == b'{' {
                    // ${N:default} → default text after the colon (if any)
                    let close = s[i..].find('}').map(|p| i + p).unwrap_or(bytes.len());
                    let inner = &s[i + 1..close];
                    if let Some(colon) = inner.find(':') {
                        out.push_str(&inner[colon + 1..]);
                    }
                    i = close + 1;
                } else {
                    // $N → drop the digits
                    while i < bytes.len() && bytes[i].is_ascii_digit() {
                        i += 1;
                    }
                }
            } else {
                out.push(bytes[i] as char);
                i += 1;
            }
        }
        out
    };

    if let Some(edit) = item.text_edit.as_ref() {
        let (range, new_text) = match edit {
            CompletionTextEdit::Edit(TextEdit { range, new_text }) => (range, new_text.clone()),
            CompletionTextEdit::InsertAndReplace(ir) => (&ir.replace, ir.new_text.clone()),
        };
        let li = LineIndex::new_utf16(source);
        let start = li
            .position_to_offset(&range.start)
            .expect("edit range start maps") as usize;
        let end = li
            .position_to_offset(&range.end)
            .expect("edit range end maps") as usize;
        let mut result = String::new();
        result.push_str(&source[..start]);
        result.push_str(&strip_snippet(&new_text));
        result.push_str(&source[end..]);
        return result;
    }

    // No text_edit → client word-replacement fallback (the bug path).
    let insert = item
        .insert_text
        .clone()
        .unwrap_or_else(|| item.label.clone());
    // Find the start of the identifier word ending at the cursor.
    let is_word = |c: char| c.is_alphanumeric() || c == '_' || c == '-';
    let mut word_start = cursor_offset;
    let bytes = source.as_bytes();
    while word_start > 0 && is_word(bytes[word_start - 1] as char) {
        word_start -= 1;
    }
    let mut result = String::new();
    result.push_str(&source[..word_start]);
    result.push_str(&strip_snippet(&insert));
    result.push_str(&source[cursor_offset..]);
    result
}

fn find_item<'a>(items: &'a [CompletionItem], label: &str) -> &'a CompletionItem {
    items
        .iter()
        .find(|i| i.label == label)
        .unwrap_or_else(|| panic!("expected a `{label}` completion item; got {:?}",
            items.iter().map(|i| &i.label).collect::<Vec<_>>()))
}

// ---------------------------------------------------------------------------
// Primary repro: `<script` selecting "script setup" must NOT double the `<`.
// ---------------------------------------------------------------------------

#[test]
fn script_setup_completion_does_not_double_left_angle() {
    // User has typed `<script` at the top of an otherwise-empty `.vue` file.
    let source = "<script";
    let cursor = source.len(); // cursor right after the `t` in `<script`

    let items = root_items_at(source, cursor);
    let item = find_item(&items, "script setup");

    // It IS a snippet whose insertText begins with `<`.
    assert_eq!(
        item.insert_text_format,
        Some(InsertTextFormat::SNIPPET),
        "tag completion should be a snippet"
    );
    assert!(
        item.insert_text
            .as_deref()
            .is_some_and(|t| t.starts_with('<')),
        "insertText is expected to start with `<` (this is half of the bug): {:?}",
        item.insert_text
    );

    // CORE ASSERTION — discriminates buggy vs fixed:
    // With a `<`-typed prefix and a `<`-prefixed snippet, the provider MUST carry
    // a text_edit whose replace range starts at the `<` (offset 0 here), so the
    // edit application yields a single `<`. Today text_edit == None → FAILS here.
    assert!(
        item.text_edit.is_some(),
        "REPRO: `script setup` ships NO text_edit, so the client word-heuristic \
         leaves the typed `<` in place and the snippet adds another → `<<script`. \
         The provider must supply a `<`-anchored text_edit."
    );

    // End-to-end behavioral assertion: applying the selected item must produce a
    // single `<script`, never `<<script`.
    let applied = apply_item(source, cursor, item);
    assert!(
        !applied.contains("<<script"),
        "double `<` reproduced: applied result was {applied:?}"
    );
    assert!(
        applied.starts_with("<script setup"),
        "expected single `<script setup ...>`, got {applied:?}"
    );
}

// ---------------------------------------------------------------------------
// Same class for <template> and <style>.
// ---------------------------------------------------------------------------

#[test]
fn template_completion_does_not_double_left_angle() {
    let source = "<template";
    let cursor = source.len();
    let items = root_items_at(source, cursor);
    let item = find_item(&items, "template");

    assert!(
        item.text_edit.is_some(),
        "REPRO: `template` ships no `<`-anchored text_edit → `<<template`"
    );
    let applied = apply_item(source, cursor, item);
    assert!(
        !applied.contains("<<template"),
        "double `<` reproduced for <template>: {applied:?}"
    );
    assert!(
        applied.starts_with("<template>"),
        "expected single `<template>`, got {applied:?}"
    );
}

#[test]
fn style_completion_does_not_double_left_angle() {
    let source = "<style";
    let cursor = source.len();
    let items = root_items_at(source, cursor);
    let item = find_item(&items, "style");

    assert!(
        item.text_edit.is_some(),
        "REPRO: `style` ships no `<`-anchored text_edit → `<<style`"
    );
    let applied = apply_item(source, cursor, item);
    assert!(
        !applied.contains("<<style"),
        "double `<` reproduced for <style>: {applied:?}"
    );
    assert!(
        applied.starts_with("<style>"),
        "expected single `<style>`, got {applied:?}"
    );
}

// ---------------------------------------------------------------------------
// Control: with NO `<` typed yet (cursor on a bare word `scr`), the same item
// applies cleanly today. This proves the test models the client faithfully and
// that the failure above is specifically the leading-`<` accounting, not a
// blanket "snippets are broken" claim. This test PASSES on the current tree.
// ---------------------------------------------------------------------------

#[test]
fn control_word_only_prefix_has_no_double_left_angle_today() {
    // No `<` typed — just a stray word at root level. (Not a real Vue trigger,
    // but it isolates the leading-`<` variable.)
    let source = "scr";
    let cursor = source.len();
    let items = root_items_at(source, cursor);
    let item = find_item(&items, "script setup");
    let applied = apply_item(source, cursor, item);
    // Whether or not text_edit is set, there is no pre-existing `<` to duplicate.
    assert!(
        !applied.contains("<<script"),
        "control should never double `<`: {applied:?}"
    );
}
