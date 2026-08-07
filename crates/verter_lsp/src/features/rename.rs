// Rename — classify WHICH AUTHORITY owns the symbol a rename at a position
// targets, and build the same-file edit set Verter's own analysis owns.
//
// [`classify_rename_target`] is the SOLE rename classification. The server
// resolves it ONCE per request (`server::rename_plan`) and BOTH
// `textDocument/prepareRename` and `textDocument/rename` consume that one
// resolution, so the two can never disagree about who owns the cursor.

use std::collections::HashMap;

use tower_lsp_server::ls_types::*;
use verter_session::FileAnalysisSnapshot;

use crate::documents::carrier_structure::CarrierBlockView;
use crate::documents::line_index::LineIndex;
use crate::features::references::{
    collect_css_ref_spans, find_css_target_in_style_refs, find_css_target_in_template_refs,
    CssRefTarget,
};

pub use super::sentinel_uris::SAME_FILE_URI;
pub use super::sentinel_uris::SAME_FILE_URI_STR;

/// Whether `offset` lands inside a template `unresolved_bindings` span — an
/// INSTANCE MEMBER access, not a use of a same-named script declaration.
///
/// One map decides both facts. `TemplateAnalysisSnapshot::unresolved_bindings`
/// receives exactly the template occurrences the compiler's template bindings
/// map did NOT contain, and that same map picks the generated IDE accessor: a
/// name in the map lowers to a bare identifier, a name outside it lowers to
/// `___VERTER___instance.<name>`. For a plain `<script>` SFC that map holds only
/// the Options-API surface (data/props/computed/methods/inject on the default
/// export), so a top-level `const` is never in it and `{{ count }}` is an
/// instance property — a different symbol from `const count`.
///
/// Nothing local can tell a VALID instance property (supplied by a
/// `ComponentCustomProperties` augmentation in another file, which cannot change
/// this file's compiler inputs, generated carrier, or analysis snapshot) from a
/// missing one. So the name-based native surface must not answer for such a
/// position, and must not claim such a span as an occurrence of a script symbol:
/// the TypeScript provider is the sole semantic authority there.
///
/// THE SINGLE DEFINITION of that positional rule, owned here with the rename
/// classifier. Rename semantics read it in exactly ONE place —
/// [`classify_rename_target`] — and [`crate::features::references`] consumes
/// this same definition for the references half of the identical
/// symbol-identity question. There is no second predicate anywhere.
pub(crate) fn offset_is_instance_member_access(
    offset: u32,
    analysis: &FileAnalysisSnapshot,
) -> bool {
    analysis.template.as_ref().is_some_and(|template| {
        template
            .unresolved_bindings
            .iter()
            .any(|binding| offset >= binding.span.start && offset < binding.span.end)
    })
}

/// Which authority owns the symbol under a rename cursor.
///
/// The classification is a property of the POSITION, not of the request: prepare
/// and rename read the same one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RenameTargetClass {
    /// The cursor names a component's public prop, either at its declaration
    /// or at a parent component usage. A complete rename would have to edit
    /// every parent, but the current workspace surfaces cannot prove that
    /// negative, so both rename entry points refuse this class before provider
    /// rename is queried.
    PublicComponentProp,
    /// Verter's own name-based analysis resolves the symbol (a script binding,
    /// a value import, or a macro binding). Its same-file occurrence set is
    /// complete for this file; a provider may add cross-file occurrences.
    Native,
    /// The cursor sits inside an instance-member template access
    /// ([`offset_is_instance_member_access`]). Verter's file-local analysis
    /// cannot resolve it — a same-named script declaration is a DIFFERENT symbol
    /// — so the TypeScript provider is the SOLE semantic authority, and with no
    /// provider answer the correct result is no edit at all.
    ProviderOnlyInstanceMember,
    /// A CSS class/id owned by Verter's native workspace index. This surface is
    /// complete WITHOUT a TypeScript provider (a class name has no TS
    /// correlate), so an empty provider answer never revokes it.
    Css,
    /// Nothing under the cursor is renameable by any authority Verter can reach.
    Unavailable,
}

/// Whether Verter's own same-file occurrence inventory for a rename target
/// PROVABLY enumerates every authored occurrence of that symbol in the file.
///
/// This is a POSITIVE property, and it is what licenses the rename transaction to
/// delegate a dropped provider location on the request's OWN generated companion
/// to the same-file completeness gate instead of refusing: if the inventory is
/// the whole file, an authored occurrence hidden behind that drop resurfaces as a
/// missing REQUIRED range. Where the inventory is a strict SUBSET, nothing covers
/// the drop and the remainder is a partial rename.
///
/// The claim is complete when BOTH of its two regions are enumerated, and each
/// conjunct is a positive fact — never the absence of a known counterexample:
///
/// 1. SCRIPT — always enumerated. Every `<script>` block's content is searched
///    exhaustively for the identifier, so no script spelling can be missing.
/// 2. MARKUP — enumerated only when the owner GRANTS it
///    ([`RenameTarget::grant_markup_occurrence_enumeration`]), which asserts that
///    this file's template analysis produces the markup occurrence inventory
///    (`binding_occurrences` / `unresolved_bindings`) this surface reads. The
///    classifier itself cannot know that, so it leaves the conjunct ungranted and
///    the claim is a strict subset until the owner says otherwise — the default is
///    fail-closed, never a silent vouch.
/// 3. `<style>` `v-bind()` — a style expression naming this very identifier is an
///    authored occurrence the claim carries no span for, and
///    `FileAnalysisSnapshot::style_vbind_roots` records exactly those root
///    identifiers, so it is decided per NAME and exactly.
///
/// Note what is deliberately NOT the witness: neither the EXISTENCE of a template
/// snapshot nor a lexical scan of the source. A Svelte carrier's template snapshot
/// exists with its occurrence inventories permanently empty, so an `is_some()`
/// test would vouch for markup that was never modelled; and a lexical scan has
/// FALSE NEGATIVES on exactly the framework spellings that matter — a store read
/// `$count` and a kebab-cased prop usage `:my-prop` are different words from
/// `count` and `myProp`, so a scan would report "every spelling accounted for"
/// while the authored occurrence sits unclaimed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SameFileEnumeration {
    /// Every spelling of the identifier in this file is inside an enumerated
    /// region.
    Complete,
    /// The inventory is a strict SUBSET of the file's authored occurrences.
    Partial(UnenumeratedRegion),
}

/// Why a [`SameFileEnumeration::Partial`] inventory does not account for the file
/// — named so the resulting refusal can say what it could not prove.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnenumeratedRegion {
    /// This file's MARKUP occurrences are not enumerated, so every authored
    /// occurrence outside the script blocks is invisible to the claim.
    MarkupOccurrences,
    /// A `<style>` `v-bind()` expression references this very identifier
    /// (`FileAnalysisSnapshot::style_vbind_roots`). The claim carries no span for
    /// it, so it is an authored occurrence a satisfied claim would still leave
    /// behind.
    StyleVBindExpression,
    /// This surface owns no occurrence inventory for the position at all: the
    /// TypeScript provider is the authority and Verter proves at most the
    /// authored token under the cursor.
    NoOccurrenceInventory,
}

/// Whether the file's own template analysis produces the MARKUP occurrence
/// inventory (`binding_occurrences` / `unresolved_bindings`) that
/// [`classify_rename_target`] reads.
///
/// A capability of the FILE's carrier, resolved by the caller
/// (`server::rename_plan`) — this module never asks which framework it is looking
/// at. Today exactly one carrier produces that inventory; a carrier whose template
/// analysis models no occurrences passes
/// [`MarkupOccurrenceInventory::NotModelled`], which makes every same-file rename
/// claim on it a strict subset and keeps the transaction fail-closed instead of
/// shipping a markup-incomplete rename.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MarkupOccurrenceInventory {
    /// The markup occurrence inventory is produced for this file.
    Enumerated,
    /// It is not: the carrier's markup occurrences are in no inventory.
    NotModelled,
}

/// The ONE synchronous rename classification of a cursor position: who owns the
/// symbol, the authored token range an editor would rename, and every same-file
/// range Verter's own typed analysis proves is an occurrence of it.
///
/// Both consumers read this one value: prepare offers [`RenameTarget::anchor`],
/// rename builds its native edit from [`RenameTarget::same_file_ranges`] and
/// proves the emitted transaction covers exactly that set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenameTarget {
    /// The owning authority.
    pub class: RenameTargetClass,
    /// The authored identifier/token range under the cursor — what an editor
    /// pre-selects for the rename. `None` when the position converts to no
    /// range (fail closed: never a fabricated line-0 range) or nothing is
    /// renameable.
    pub anchor: Option<Range>,
    /// Every range in THIS file a rename at the position must overwrite.
    ///
    /// EMPTY for [`RenameTargetClass::ProviderOnlyInstanceMember`] (the provider
    /// owns the whole occurrence set — Verter must not claim a same-named script
    /// declaration) and for [`RenameTargetClass::Unavailable`].
    pub same_file_ranges: Vec<Range>,
    /// Whether this target's same-file occurrence inventory provably enumerates
    /// the whole file — see [`SameFileEnumeration`].
    pub same_file_enumeration: SameFileEnumeration,
}

impl RenameTarget {
    /// A public component-prop cursor: positively classified, but deliberately
    /// owned by no rename authority until complete cross-file usage proof exists.
    fn public_component_prop(anchor: Option<Range>) -> Self {
        Self {
            class: RenameTargetClass::PublicComponentProp,
            anchor,
            same_file_ranges: Vec::new(),
            same_file_enumeration: SameFileEnumeration::Partial(
                UnenumeratedRegion::NoOccurrenceInventory,
            ),
        }
    }

    /// The fail-closed target: no authority, no anchor, no claimed occurrence,
    /// and no enumeration of anything.
    /// Also what a caller with no open document resolves to.
    pub fn unavailable() -> Self {
        Self {
            class: RenameTargetClass::Unavailable,
            anchor: None,
            same_file_ranges: Vec::new(),
            same_file_enumeration: SameFileEnumeration::Partial(
                UnenumeratedRegion::NoOccurrenceInventory,
            ),
        }
    }

    /// Grant the MARKUP conjunct of this target's completeness witness: this
    /// file's template analysis DOES produce the markup occurrence inventory
    /// ([`MarkupOccurrenceInventory::Enumerated`]) the classifier read.
    ///
    /// MONOTONE and narrow. It promotes ONLY the grantable state
    /// ([`UnenumeratedRegion::MarkupOccurrences`], where markup was the sole
    /// outstanding region) to [`SameFileEnumeration::Complete`]. A terminal
    /// `Partial` — a `<style>` `v-bind()` occurrence the claim cannot carry, or a
    /// position with no occurrence inventory at all — is left exactly as it is, so
    /// no capability can widen a claim into something it does not cover.
    pub fn grant_markup_occurrence_enumeration(&mut self) {
        if self.same_file_enumeration
            == SameFileEnumeration::Partial(UnenumeratedRegion::MarkupOccurrences)
        {
            self.same_file_enumeration = SameFileEnumeration::Complete;
        }
    }

    /// The same-file `WorkspaceEdit` this target's occurrence set requires,
    /// keyed by `uri`.
    ///
    /// `None` when this surface owns no same-file range — which for
    /// [`RenameTargetClass::ProviderOnlyInstanceMember`] is the fail-closed
    /// answer: the provider is the authority and an absent/empty provider
    /// answer must ship no edit at all.
    pub fn same_file_workspace_edit(&self, uri: &Uri, new_name: &str) -> Option<WorkspaceEdit> {
        if self.same_file_ranges.is_empty() {
            return None;
        }
        let edits: Vec<TextEdit> = self
            .same_file_ranges
            .iter()
            .map(|range| TextEdit {
                range: *range,
                new_text: new_name.to_string(),
            })
            .collect();

        #[allow(clippy::mutable_key_type)] // Uri has interior mutability but we only insert once
        let mut changes = HashMap::new();
        changes.insert(uri.clone(), edits);

        Some(WorkspaceEdit {
            changes: Some(changes),
            ..Default::default()
        })
    }
}

/// Classify the rename target at `position` — the single rename classifier.
///
/// Order is semantic, not incidental:
///
/// 1. No analysis, no position, or no identifier word under the cursor ⇒
///    [`RenameTargetClass::Unavailable`]. (Both rename surfaces have always
///    required an authored word here, the CSS legs included.)
/// 2. A public component-prop declaration or parent usage ⇒
///    [`RenameTargetClass::PublicComponentProp`]. This positional check runs
///    before name-based binding classification, so a Svelte/Vue prop key that
///    is also a local binding cannot fall through as an ordinary script rename.
/// 3. Inside an instance-member template access ⇒ the POSITIONAL CSS owner
///    still wins when a class/id token lives at that offset (its surface needs
///    no provider); otherwise [`RenameTargetClass::ProviderOnlyInstanceMember`]
///    — Verter's name-based branch must not answer, because it would hand back
///    the word range of a same-named script declaration, a different symbol.
/// 4. A known binding / value import / macro binding ⇒
///    [`RenameTargetClass::Native`].
/// 5. Otherwise the positional CSS owner, else
///    [`RenameTargetClass::Unavailable`].
pub fn classify_rename_target(
    position: &Position,
    source: &str,
    blocks: &[CarrierBlockView],
    analysis: Option<&FileAnalysisSnapshot>,
    line_index: &LineIndex,
) -> RenameTarget {
    let Some(analysis) = analysis else {
        return RenameTarget::unavailable();
    };
    let Some(offset) = line_index
        .position_to_offset(position)
        .map(|offset| offset as usize)
    else {
        return RenameTarget::unavailable();
    };
    let Some(word) = word_at_offset(source, offset) else {
        return RenameTarget::unavailable();
    };

    if analysis_public_component_prop_at(offset as u32, analysis) {
        return RenameTarget::public_component_prop(word_range(source, offset, &word, line_index));
    }

    // THE read of the positional `unresolved_bindings` rule for rename
    // semantics. Nothing else in the rename path consults it.
    if offset_is_instance_member_access(offset as u32, analysis) {
        return css_target(offset, source, analysis, line_index).unwrap_or_else(|| RenameTarget {
            class: RenameTargetClass::ProviderOnlyInstanceMember,
            anchor: word_range(source, offset, &word, line_index),
            same_file_ranges: Vec::new(),
            // The provider owns the occurrence SET here. Verter enumerates none
            // of it: the file's OTHER spellings of the same instance member
            // (`:title="count"` alongside `{{ count }}`) are not in any inventory
            // this surface builds, so at most the authored token under the cursor
            // is provable.
            same_file_enumeration: SameFileEnumeration::Partial(
                UnenumeratedRegion::NoOccurrenceInventory,
            ),
        });
    }

    // Only known bindings, non-type imports and macro bindings are natively
    // renameable.
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
        return css_target(offset, source, analysis, line_index)
            .unwrap_or_else(RenameTarget::unavailable);
    }

    RenameTarget {
        class: RenameTargetClass::Native,
        anchor: word_range(source, offset, &word, line_index),
        same_file_ranges: to_ranges(
            native_rename_spans(&word, source, blocks, analysis),
            line_index,
        ),
        same_file_enumeration: claim_enumeration_without_markup(&word, analysis),
    }
}

/// Whether existing shallow analysis positively identifies `offset` as a
/// public component-prop name token.
///
/// This is deliberately positional and rename-local. It consumes only exact
/// spans already owned by the framework analyses:
///
/// * Vue macro/runtime prop fields;
/// * Vue Options-API prop keys;
/// * a component usage's prop-name span in either Vue or Svelte markup.
///
/// The usage leg does not resolve the child first. A prop-shaped cursor on an
/// unresolved component must fail closed too; requiring successful resolution
/// here would recreate the `NotChildProp` passthrough hole.
fn analysis_public_component_prop_at(offset: u32, analysis: &FileAnalysisSnapshot) -> bool {
    let contains = |span: verter_span::Span| offset >= span.start && offset < span.end;

    analysis
        .macros
        .iter()
        .flat_map(|mac| &mac.prop_fields)
        .any(|field| contains(field.span))
        || analysis
            .options_api
            .iter()
            .flat_map(|options| &options.props)
            .any(|prop| contains(prop.span))
        || analysis.template.as_ref().is_some_and(|template| {
            template
                .components
                .iter()
                .flat_map(|component| &component.props)
                .any(|prop| contains(prop.name_span))
        })
}

/// The [`SameFileEnumeration`] witness a claim over `name` can establish WITHOUT
/// knowing whether the file's markup occurrences are enumerated — the safe state.
///
/// Two conjuncts decide completeness. This resolves the one the classifier can
/// see (`<style>` `v-bind()`) and leaves the other UNGRANTED, so the default is
/// fail-closed: a caller that never learns the carrier's markup capability holds a
/// strict-subset claim and cannot vouch for a dropped provider location.
/// `server::rename_plan` grants the markup conjunct through
/// [`RenameTarget::grant_markup_occurrence_enumeration`].
///
/// * `Partial(StyleVBindExpression)` is TERMINAL — a style expression naming this
///   identifier is an authored occurrence the claim carries no span for, and no
///   markup capability can make that whole.
/// * `Partial(MarkupOccurrences)` is the grantable state: script spellings are
///   already complete (collected by exhaustive lexical search over every
///   `<script>` block), so markup is the only outstanding region.
fn claim_enumeration_without_markup(
    name: &str,
    analysis: &FileAnalysisSnapshot,
) -> SameFileEnumeration {
    if analysis.style_vbind_roots.iter().any(|root| root == name) {
        return SameFileEnumeration::Partial(UnenumeratedRegion::StyleVBindExpression);
    }
    SameFileEnumeration::Partial(UnenumeratedRegion::MarkupOccurrences)
}

/// The authored range of the identifier `word` containing `offset`. `None` when
/// either endpoint does not convert (fail closed — never a line-0 range).
fn word_range(source: &str, offset: usize, word: &str, line_index: &LineIndex) -> Option<Range> {
    let word_start = find_word_start(source.as_bytes(), offset);
    let word_end = word_start + word.len();
    Some(Range {
        start: line_index.offset_to_position(word_start as u32)?,
        end: line_index.offset_to_position(word_end as u32)?,
    })
}

/// The CSS class/id target at `offset`, as a [`RenameTargetClass::Css`] rename
/// target: the whole cross-region span set, anchored on the span the cursor sits
/// in. `None` when no CSS name owns the offset.
fn css_target(
    offset: usize,
    source: &str,
    analysis: &FileAnalysisSnapshot,
    line_index: &LineIndex,
) -> Option<RenameTarget> {
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
    let anchor = spans
        .iter()
        .find(|(start, end)| offset as u32 >= *start && (offset as u32) < *end)
        .and_then(|(start, end)| {
            Some(Range {
                start: line_index.offset_to_position(*start)?,
                end: line_index.offset_to_position(*end)?,
            })
        });
    // A CSS class/id's claim spans the markup class attributes and the `<style>`
    // selectors, so the same markup conjunct decides completeness. A `v-bind()`
    // expression cannot reference a class NAME, so that conjunct never applies —
    // but it is evaluated through the one shared witness rather than a second
    // rule.
    let name = match &target {
        CssRefTarget::Class(name) | CssRefTarget::Id(name) => name.clone(),
    };
    let same_file_enumeration = claim_enumeration_without_markup(&name, analysis);
    Some(RenameTarget {
        class: RenameTargetClass::Css,
        anchor,
        same_file_ranges: to_ranges(spans, line_index),
        same_file_enumeration,
    })
}

/// Every same-file span a NATIVE rename of `word` must overwrite: its
/// declaration, its value-import bindings, its template occurrences, and its
/// lexical script occurrences.
fn native_rename_spans(
    word: &str,
    source: &str,
    blocks: &[CarrierBlockView],
    analysis: &FileAnalysisSnapshot,
) -> Vec<(u32, u32)> {
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

        for occ_offset in find_all_word_occurrences(content, word) {
            let abs_offset = content_start as usize + occ_offset;
            let abs_end = abs_offset + word.len();
            push_span(&mut spans, abs_offset as u32, abs_end as u32);
        }
    }

    spans
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
